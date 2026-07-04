//! Locator deref — the WORKER phase of locator-shape lowering plus the
//! owned-typed-IR navigation helpers it drives.
//!
//! A deref re-borrows the retained snapshot sub-position named by a
//! producer-emitted [`AuthoredBodyLocator`] through the owning memo's own
//! lazy demand cells and returns transient OWNED typed IR. Every failure is
//! a typed [`LocatorBodyDerefError`] — a deref NEVER fabricates a body and
//! NEVER falls back to a transient re-parse.

use verter_semantic::analysis::type_eval::{AugmentationScopeKind, TypeDeclBody};
use verter_type_expr::locators::{
    AuthoredAugmentationScope, AuthoredBodyLocator, LocatorSymbolSpace, MacroPayloadPosition,
    TypeBodyPathStep,
};
use verter_type_expr::{ObjectMember, TypeExpr, TypeParam};

use super::{DeclBodyMemo, DemandOutcome};

/// Why a locator deref could not produce the authored typed IR. Every
/// variant is a typed, fail-closed non-result — a deref NEVER fabricates a
/// body and NEVER falls back to a transient re-parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocatorBodyDerefError {
    /// The locator anchor names no inventoried declaration. This is a
    /// GENUINE, cacheable resolution result (the symbol truly does not
    /// exist) — DISTINCT from [`Self::LeaseMiss`].
    UnknownSymbol,
    /// The demanded body lowering hit a BROKEN lease pin (`ReturnOnly`): the
    /// lowering ran NOTHING and produced NOTHING. This is a transient no-warm
    /// signal, NOT a cacheable resolution result — the enclosing
    /// `LowerLocator` / `Instantiate` build must refuse warm admission
    /// (`cache_suppress`) so a later demand under a live lease recovers.
    /// Never collapsed into [`Self::UnknownSymbol`].
    LeaseMiss,
    /// The producer-emitted path does not resolve against the authored
    /// body (a stale / out-of-range ordinal, or a shape mismatch).
    PathUnresolved,
    /// A VALUE anchor whose declaration carries no authored type
    /// annotation — there is no authored TYPE body at that position.
    ValueAnnotationAbsent,
    /// Namespace bodies are not inventoried by the decl-body memo; a
    /// namespace anchor has no memo-backed authored body to deref.
    NamespaceBodyUnrouted,
    /// No consumer demands an augmentation-scoped VALUE / namespace body
    /// through a locator; the deref fails closed rather than fabricating one.
    AugmentationBodySpaceUnrouted,
    /// The macro generic type argument has exactly ONE sanctioned producer
    /// (`macro_type_arg_hot_ref`, the sole query-free structural
    /// macro-argument producer); a locator deref for it is rejected so a
    /// second producer path for the same payload can never exist.
    MacroTypeArgumentHasSoleHotMirrorProducer,
    /// No producer emits object-argument / analyzed-field payload
    /// locators; a deref for such a position fails closed with this typed
    /// error rather than fabricating a body.
    MacroPayloadPositionUnrouted,
}

/// The derefed authored SHAPE of a locator position: the whole decl body
/// (preserving the distinct merged-contributor carrier) or one
/// path-addressed sub-position.
#[derive(Debug, Clone)]
pub(crate) enum DerefedBodyShape {
    /// A single authored body / sub-position expression.
    Single(TypeExpr),
    /// The ordered same-name merged contributors of a whole merged decl
    /// body. Preserved as a DISTINCT carrier — never collapsed to an
    /// intersection (the merged-decl peer-merge reducer needs the
    /// contributor structure).
    Merged(Vec<TypeExpr>),
}

/// Owned typed-IR product of one locator deref: the derefed shape plus the
/// owning declaration's generic parameters (so the session phase can bind
/// them as `TypeParam` shells in the authored position's own lexical
/// scope). NEVER a `SemanticNodeId` — graph lowering is the session
/// phase's job.
#[derive(Debug, Clone)]
pub(crate) struct DerefedAuthoredBody {
    pub(crate) shape: DerefedBodyShape,
    pub(crate) type_parameters: Vec<TypeParam>,
}

impl DeclBodyMemo {
    /// Locator deref — the WORKER phase of locator-shape lowering: re-borrow
    /// the retained snapshot sub-position named by the locator's
    /// producer-emitted origin path and return transient OWNED typed IR.
    ///
    /// Lease-only purity: the deref serves through the memo's own lazy
    /// demand cells (`type_decl` / `value_decl` / `augmentation_type_decl`),
    /// whose demanded lowering (`lower_demanded`) runs through
    /// [`crate::decl_lowering::DeclLoweringService::run_leased`] against the
    /// memo's retained snapshot (the lease is `ensure_lease`-pinned for the
    /// memo's lifetime) — NO transient parse (a broken lease pin is a
    /// lowering MISS), no host / dispatch / service re-entry inside the job.
    /// Authored macro payloads reuse THIS memo (the producing canonical's
    /// snapshot) — never a separate payload memo. Every failure is a typed
    /// [`LocatorBodyDerefError`], never a fabricated body.
    pub(crate) fn deref_locator_body(
        &self,
        locator: &AuthoredBodyLocator,
    ) -> Result<DerefedAuthoredBody, LocatorBodyDerefError> {
        match locator {
            AuthoredBodyLocator::MacroPayload(payload) => match payload.payload {
                // The macro generic type argument keeps its sole sanctioned
                // producer (`macro_type_arg_hot_ref`); rejecting the deref
                // here means a second producer path cannot come into
                // existence.
                MacroPayloadPosition::TypeArgument => {
                    Err(LocatorBodyDerefError::MacroTypeArgumentHasSoleHotMirrorProducer)
                }
                // No producer emits these payload locators; fail closed
                // with the typed non-result.
                MacroPayloadPosition::ObjectArgument | MacroPayloadPosition::Field { .. } => {
                    Err(LocatorBodyDerefError::MacroPayloadPositionUnrouted)
                }
            },
            AuthoredBodyLocator::DeclBody(slot) => {
                debug_assert_eq!(
                    slot.anchor.canonical_id.as_ref(),
                    self.key.canonical.as_ref(),
                    "a locator must deref through the memo of its OWN producing canonical"
                );
                match slot.anchor.space {
                    LocatorSymbolSpace::Type => {
                        // Serve through the memo's OWN lazy demand cell so a
                        // body lowers exactly once per (canonical, content,
                        // symbol) regardless of which route demands it first.
                        // A file-scope miss falls through to the GLOBAL
                        // ambient inventory — the same file-scope-then-global
                        // resolution order the prepared-decl route applies. A
                        // BROKEN-lease demand surfaces the DISTINCT `LeaseMiss`
                        // (a transient no-warm ReturnOnly), never collapsed into
                        // the cacheable `UnknownSymbol`.
                        let lowered = match self.type_decl_outcome(slot.anchor.symbol.as_ref()) {
                            DemandOutcome::LeaseMiss => {
                                return Err(LocatorBodyDerefError::LeaseMiss)
                            }
                            DemandOutcome::Ready(Some(lowered)) => lowered,
                            DemandOutcome::Ready(None) => {
                                match self.augmentation_type_decl_outcome(
                                    &AugmentationScopeKind::Global,
                                    slot.anchor.symbol.as_ref(),
                                ) {
                                    DemandOutcome::LeaseMiss => {
                                        return Err(LocatorBodyDerefError::LeaseMiss)
                                    }
                                    DemandOutcome::Ready(Some(lowered)) => lowered,
                                    DemandOutcome::Ready(None) => {
                                        return Err(LocatorBodyDerefError::UnknownSymbol)
                                    }
                                }
                            }
                        };
                        let shape = navigate_type_body(lowered.body.clone(), &slot.path)?;
                        Ok(DerefedAuthoredBody {
                            shape,
                            type_parameters: lowered.type_parameters.clone(),
                        })
                    }
                    LocatorSymbolSpace::Value => {
                        let lowered = match self.value_decl_outcome(slot.anchor.symbol.as_ref()) {
                            DemandOutcome::LeaseMiss => {
                                return Err(LocatorBodyDerefError::LeaseMiss)
                            }
                            DemandOutcome::Ready(Some(lowered)) => lowered,
                            DemandOutcome::Ready(None) => {
                                return Err(LocatorBodyDerefError::UnknownSymbol)
                            }
                        };
                        let annotation = lowered
                            .type_annotation
                            .clone()
                            .ok_or(LocatorBodyDerefError::ValueAnnotationAbsent)?;
                        let shape =
                            navigate_type_body(TypeDeclBody::Single(annotation), &slot.path)?;
                        Ok(DerefedAuthoredBody {
                            shape,
                            // A value annotation position binds no declared
                            // type parameters of its own.
                            type_parameters: Vec::new(),
                        })
                    }
                    LocatorSymbolSpace::Namespace => {
                        Err(LocatorBodyDerefError::NamespaceBodyUnrouted)
                    }
                }
            }
            AuthoredBodyLocator::AugmentationBody(aug) => {
                debug_assert_eq!(
                    aug.anchor.canonical_id.as_ref(),
                    self.key.canonical.as_ref(),
                    "a locator must deref through the memo of its OWN producing canonical"
                );
                let scope_kind = match &aug.scope {
                    AuthoredAugmentationScope::Global => AugmentationScopeKind::Global,
                    AuthoredAugmentationScope::Module { specifier } => {
                        AugmentationScopeKind::Module(specifier.as_ref().to_string())
                    }
                };
                match aug.anchor.space {
                    LocatorSymbolSpace::Type => {
                        // Serve through the memo's scoped lazy demand cell
                        // (one lowering per (scope, symbol) per content). A
                        // broken-lease demand surfaces the DISTINCT `LeaseMiss`
                        // no-warm signal, never a cacheable `UnknownSymbol`.
                        let lowered = match self
                            .augmentation_type_decl_outcome(&scope_kind, aug.anchor.symbol.as_ref())
                        {
                            DemandOutcome::LeaseMiss => {
                                return Err(LocatorBodyDerefError::LeaseMiss)
                            }
                            DemandOutcome::Ready(Some(lowered)) => lowered,
                            DemandOutcome::Ready(None) => {
                                return Err(LocatorBodyDerefError::UnknownSymbol)
                            }
                        };
                        let shape = match lowered.body.clone() {
                            TypeDeclBody::Single(expr) => DerefedBodyShape::Single(expr),
                            TypeDeclBody::Merged(merged) => {
                                DerefedBodyShape::Merged(merged.contributors)
                            }
                        };
                        Ok(DerefedAuthoredBody {
                            shape,
                            type_parameters: lowered.type_parameters.clone(),
                        })
                    }
                    LocatorSymbolSpace::Value | LocatorSymbolSpace::Namespace => {
                        Err(LocatorBodyDerefError::AugmentationBodySpaceUnrouted)
                    }
                }
            }
        }
    }
}

/// Navigate a producer-emitted [`TypeBodyPathStep`] path over the OWNED
/// typed body. Empty path = the whole body (preserving the merged-contributor
/// carrier); a non-empty path selects exactly the named sub-position.
/// Fail-closed: any shape/ordinal mismatch is
/// [`LocatorBodyDerefError::PathUnresolved`].
fn navigate_type_body(
    body: TypeDeclBody,
    path: &[TypeBodyPathStep],
) -> Result<DerefedBodyShape, LocatorBodyDerefError> {
    let Some((first, rest)) = path.split_first() else {
        return Ok(match body {
            TypeDeclBody::Single(expr) => DerefedBodyShape::Single(expr),
            TypeDeclBody::Merged(merged) => DerefedBodyShape::Merged(merged.contributors),
        });
    };
    let (start, remaining) = match (body, first) {
        (TypeDeclBody::Merged(merged), TypeBodyPathStep::MergedContributor { ordinal }) => {
            let expr = merged
                .contributors
                .into_iter()
                .nth(*ordinal as usize)
                .ok_or(LocatorBodyDerefError::PathUnresolved)?;
            (expr, rest)
        }
        // A merged body's sub-positions are addressed through a contributor
        // step first; any other first step is unresolvable by shape.
        (TypeDeclBody::Merged(_), _) => return Err(LocatorBodyDerefError::PathUnresolved),
        // A single body has no contributor axis; the whole path navigates
        // the body expression directly.
        (TypeDeclBody::Single(expr), _) => (expr, path),
    };
    navigate_expr(start, remaining).map(DerefedBodyShape::Single)
}

/// The current navigation position: an expression, or a selected object /
/// interface member (from which `MemberValue` — or path termination —
/// descends to the member's value type).
enum NavigatePosition {
    Expr(TypeExpr),
    Member(ObjectMember),
}

/// Navigate `path` over an owned expression. Parenthesized wrappers are
/// structurally transparent at every expression step.
fn navigate_expr(
    expr: TypeExpr,
    path: &[TypeBodyPathStep],
) -> Result<TypeExpr, LocatorBodyDerefError> {
    let mut position = NavigatePosition::Expr(expr);
    for step in path {
        position = match (position, step) {
            (NavigatePosition::Expr(expr), TypeBodyPathStep::IntersectionArm { ordinal }) => {
                match unwrap_parenthesized(expr) {
                    TypeExpr::Intersection(ref arms) => NavigatePosition::Expr(
                        arms.get(*ordinal as usize)
                            .cloned()
                            .ok_or(LocatorBodyDerefError::PathUnresolved)?,
                    ),
                    _ => return Err(LocatorBodyDerefError::PathUnresolved),
                }
            }
            (NavigatePosition::Expr(expr), TypeBodyPathStep::Member { ordinal }) => {
                match unwrap_parenthesized(expr) {
                    TypeExpr::Object(ref obj) => NavigatePosition::Member(
                        obj.properties
                            .get(*ordinal as usize)
                            .cloned()
                            .ok_or(LocatorBodyDerefError::PathUnresolved)?,
                    ),
                    _ => return Err(LocatorBodyDerefError::PathUnresolved),
                }
            }
            (NavigatePosition::Member(member), TypeBodyPathStep::MemberValue) => {
                NavigatePosition::Expr(member_value_expr(member)?)
            }
            _ => return Err(LocatorBodyDerefError::PathUnresolved),
        };
    }
    match position {
        NavigatePosition::Expr(expr) => Ok(expr),
        // A path terminating on a selected member derefs to that member's
        // value type (the one typed-IR expression at a member position).
        NavigatePosition::Member(member) => member_value_expr(member),
    }
}

/// Unwrap structurally-transparent `Parenthesized` layers.
fn unwrap_parenthesized(mut expr: TypeExpr) -> TypeExpr {
    while let TypeExpr::Parenthesized(ref inner) = expr {
        let unwrapped = inner.as_ref().clone();
        expr = unwrapped;
    }
    expr
}

/// The value-type expression of a selected object member. An index
/// signature has no single member-value expression — fail closed.
fn member_value_expr(member: ObjectMember) -> Result<TypeExpr, LocatorBodyDerefError> {
    match member {
        ObjectMember::Property(prop) => Ok(prop.ty),
        ObjectMember::Method(method) => {
            Ok(TypeExpr::Function(std::sync::Arc::new(method.function)))
        }
        ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
            Ok(TypeExpr::Function(std::sync::Arc::new(func)))
        }
        ObjectMember::IndexSignature(_) => Err(LocatorBodyDerefError::PathUnresolved),
    }
}
