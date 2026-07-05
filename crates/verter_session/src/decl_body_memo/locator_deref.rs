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
    TypeBodyPathStep, TypeParamBoundPosition,
};
use verter_type_expr::{ObjectMember, TypeExpr, TypeParam};

use super::{DeclBodyMemo, DemandOutcome};

/// Why a locator deref could not produce the authored typed IR. Every
/// variant is a typed, fail-closed non-result — a deref NEVER fabricates a
/// body and NEVER falls back to a transient re-parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocatorBodyDerefError {
    /// The locator anchor names a DIFFERENT producing canonical than the memo
    /// serving the deref — a locator must deref through the memo of its OWN
    /// producing canonical (`anchor.canonical_id == memo.key.canonical`). A
    /// unit variant (no payload) keeps this hot error enum cheap; the invariant
    /// is structural, so the identities are not needed on the failure path.
    /// Checked up front for every arm, before any body demand — the typed,
    /// release-present successor to the former branch-local `debug_assert_eq!`.
    CanonicalMismatch,
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
    /// A `TypeParamBound` step names a parameter ordinal past the owning
    /// declaration's type-parameter list. Fail-closed, never a fabricated body.
    TypeParamOrdinalOutOfRange { ordinal: u32 },
    /// The referenced type parameter exists but carries no authored body at the
    /// requested bound slot (no constraint for [`TypeParamBoundPosition::Constraint`],
    /// no default for [`TypeParamBoundPosition::Default`]) — analogous to
    /// [`Self::ValueAnnotationAbsent`].
    TypeParamBoundAbsent {
        ordinal: u32,
        position: TypeParamBoundPosition,
    },
    /// A `TypeParamBound` step appears anywhere other than the first path step,
    /// or on a non-type-space anchor. Type parameters live on the declaration
    /// header, not inside the body expression and not on a value / namespace
    /// annotation position, so the step is misplaced by definition. Merged
    /// group-level type parameters are unioned, not per-contributor, so a
    /// contributor-header bound axis does not exist either.
    TypeParamBoundStepMisplaced,
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
        // One top-level anchor-canonical coherence gate for ALL arms: a
        // locator MUST deref through the memo of its OWN producing canonical.
        // Checked BEFORE any body demand — the typed, release-present successor
        // to the former branch-local `debug_assert_eq!` (release-stripped).
        let anchor = match locator {
            AuthoredBodyLocator::MacroPayload(payload) => &payload.anchor,
            AuthoredBodyLocator::DeclBody(slot) => &slot.anchor,
            AuthoredBodyLocator::AugmentationBody(aug) => &aug.anchor,
        };
        if anchor.canonical_id.as_ref() != self.key.canonical.as_ref() {
            return Err(LocatorBodyDerefError::CanonicalMismatch);
        }

        // `TypeParamBound` placement is a SYNTACTIC invariant — validate the
        // WHOLE path UP FRONT, before any body demand/lowering, so a
        // structurally-misplaced bound fails closed with the distinct error
        // regardless of whether an earlier path step would resolve (and never
        // lowers a body for a structurally-invalid path).
        match locator {
            AuthoredBodyLocator::DeclBody(slot) => {
                validate_type_param_bound_placement(slot.anchor.space, &slot.path)?;
            }
            AuthoredBodyLocator::AugmentationBody(aug) => {
                validate_type_param_bound_placement(aug.anchor.space, &aug.path)?;
            }
            AuthoredBodyLocator::MacroPayload(_) => {}
        }

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
                        // A type-decl-header type parameter's bound (leading
                        // `TypeParamBound` step) plus any post-bound descent
                        // route through the ONE shared type-space navigator,
                        // exactly as the augmentation type-space branch does.
                        navigate_type_space_body(
                            lowered.body.clone(),
                            &lowered.type_parameters,
                            &slot.path,
                        )
                    }
                    LocatorSymbolSpace::Value => {
                        // A value-decl / function type parameter lives on the
                        // signature, not on this annotation position, so a
                        // leading `TypeParamBound` step is misplaced by
                        // definition — fail closed without demanding a body.
                        if matches!(
                            slot.path.first(),
                            Some(TypeBodyPathStep::TypeParamBound { .. })
                        ) {
                            return Err(LocatorBodyDerefError::TypeParamBoundStepMisplaced);
                        }
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
                        // A namespace decl has no memo-backed body and no
                        // header type-parameter axis; a leading `TypeParamBound`
                        // step is misplaced (not merely unrouted).
                        if matches!(
                            slot.path.first(),
                            Some(TypeBodyPathStep::TypeParamBound { .. })
                        ) {
                            return Err(LocatorBodyDerefError::TypeParamBoundStepMisplaced);
                        }
                        Err(LocatorBodyDerefError::NamespaceBodyUnrouted)
                    }
                }
            }
            AuthoredBodyLocator::AugmentationBody(aug) => {
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
                        // An augmentation-scoped `interface` / `type` decl is an
                        // authored type-decl-header decl, so its type-param
                        // bounds and body sub-positions navigate through the
                        // SAME shared type-space navigator as a top-level decl.
                        // An empty `path` preserves the whole-body Single/Merged
                        // behavior unchanged.
                        navigate_type_space_body(
                            lowered.body.clone(),
                            &lowered.type_parameters,
                            &aug.path,
                        )
                    }
                    LocatorSymbolSpace::Value | LocatorSymbolSpace::Namespace => {
                        Err(LocatorBodyDerefError::AugmentationBodySpaceUnrouted)
                    }
                }
            }
        }
    }
}

/// Up-front structural validation of `TypeParamBound` placement over a
/// producer-emitted path, run BEFORE any body demand.
///
/// A `TypeParamBound` step addresses a type parameter on the type-declaration
/// HEADER, so it is allowed ONLY as `path[0]` AND only on a TYPE-space anchor:
///
/// - any `TypeParamBound` at `path[1..]` (a mid-path bound) is misplaced —
///   type parameters do not live inside the body expression;
/// - any `TypeParamBound` anywhere in the path (including `path[0]`) on a
///   VALUE / NAMESPACE anchor is misplaced — value / function type parameters
///   live on the signature, and a namespace has no header type-parameter axis.
///
/// Enforcing this up front makes [`LocatorBodyDerefError::TypeParamBoundStepMisplaced`]
/// hold regardless of whether an earlier path step would resolve (an in-body
/// navigation would otherwise swallow it as a generic `PathUnresolved`).
fn validate_type_param_bound_placement(
    space: LocatorSymbolSpace,
    path: &[TypeBodyPathStep],
) -> Result<(), LocatorBodyDerefError> {
    let is_bound =
        |step: &TypeBodyPathStep| matches!(step, TypeBodyPathStep::TypeParamBound { .. });
    if path.iter().skip(1).any(is_bound) {
        return Err(LocatorBodyDerefError::TypeParamBoundStepMisplaced);
    }
    if !matches!(space, LocatorSymbolSpace::Type) && path.iter().any(is_bound) {
        return Err(LocatorBodyDerefError::TypeParamBoundStepMisplaced);
    }
    Ok(())
}

/// The ONE shared type-space navigator over a lowered TYPE declaration body +
/// its header type parameters. Used by BOTH the top-level decl-body and the
/// ambient-augmentation type-space deref branches so the two never diverge
/// into a second navigation engine.
///
/// A leading `TypeParamBound` step is served from the declaration's type
/// parameters (which live on the header, not in the body expression): it
/// selects the constraint / default bound of the parameter at `ordinal`, the
/// remaining steps navigate over the selected bound, and the returned
/// `type_parameters` are the LEXICAL-PREFIX env of that bound (the parameters
/// declared BEFORE `ordinal`). Any other path navigates the body directly
/// with the full header type-parameter env; an empty path yields the whole
/// body (preserving the merged-contributor carrier).
///
/// Placement of a leading bound is presumed already validated by
/// [`validate_type_param_bound_placement`]; a mid-path bound reaching
/// [`navigate_expr`] still fails closed there as defense-in-depth.
fn navigate_type_space_body(
    body: TypeDeclBody,
    type_parameters: &[TypeParam],
    path: &[TypeBodyPathStep],
) -> Result<DerefedAuthoredBody, LocatorBodyDerefError> {
    if let Some(TypeBodyPathStep::TypeParamBound { ordinal, position }) = path.first() {
        let ordinal = *ordinal;
        let position = *position;
        let tp = type_parameters
            .get(ordinal as usize)
            .ok_or(LocatorBodyDerefError::TypeParamOrdinalOutOfRange { ordinal })?;
        let bound = match position {
            TypeParamBoundPosition::Constraint => tp.constraint.as_ref(),
            TypeParamBoundPosition::Default => tp.default.as_ref(),
        }
        .ok_or(LocatorBodyDerefError::TypeParamBoundAbsent { ordinal, position })?;
        let expr = navigate_expr(bound.as_ref().clone(), &path[1..])?;
        return Ok(DerefedAuthoredBody {
            shape: DerefedBodyShape::Single(expr),
            // Engine-current lexical type-parameter env of this bound: the
            // prior-sibling prefix frame (`type_parameters[..ordinal]`) used by
            // the binder-frame family for BOTH constraints and defaults. `T` at
            // ordinal 0 sees an empty prefix; `U extends keyof T` (ordinal 1)
            // binds `T` but not `U` itself or later params. This intentionally
            // mirrors the existing engine convention; TS-exact constraint
            // forward references (TypeScript lets a constraint reference later
            // siblings, unlike a default) are a separate binder-frame policy
            // concern, not handled here. `get` succeeded, so `ordinal < len`
            // and the prefix slice is valid.
            type_parameters: type_parameters[..ordinal as usize].to_vec(),
        });
    }
    let shape = navigate_type_body(body, path)?;
    Ok(DerefedAuthoredBody {
        shape,
        type_parameters: type_parameters.to_vec(),
    })
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
            // A `TypeParamBound` step is valid ONLY as the first path step
            // (served from the decl header before navigation begins); reaching
            // one here means it appeared mid-path — fail closed with the
            // distinct misplaced error rather than a generic path miss.
            (_, TypeBodyPathStep::TypeParamBound { .. }) => {
                return Err(LocatorBodyDerefError::TypeParamBoundStepMisplaced)
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
