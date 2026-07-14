//! Locator deref — the WORKER phase of locator-shape lowering plus the
//! owned-typed-IR navigation helpers it drives.
//!
//! A deref re-borrows the retained snapshot sub-position named by a
//! producer-emitted [`AuthoredBodyLocator`] through the owning memo's own
//! lazy demand cells and returns transient OWNED typed IR. Every failure is
//! a typed [`LocatorBodyDerefError`] — a deref NEVER fabricates a body and
//! NEVER falls back to a transient re-parse.

use std::sync::Arc;

use verter_semantic::analysis::framework_facts::svelte::PropsAnnotationLowering;
use verter_semantic::analysis::type_eval::AugmentationScopeKind;
use verter_semantic::analysis::type_eval_build::LoweredSignatureParts;
use verter_semantic::analysis::MacroFieldPayloadLowering;
use verter_type_expr::locators::{
    AuthoredAugmentationScope, AuthoredBodyLocator, LocatorSymbolSpace, MacroPayloadPosition,
    TypeBodyPathStep, TypeParamBoundPosition, TypeParamVisibility,
};
use verter_type_expr::{FunctionExpr, ObjectMember, TypeExpr, TypeParam};

use super::{DeclBodyMemo, DemandOutcome, TransientValueParts};

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
    /// No deref route exists for the whole-object-argument payload position
    /// (no producer mints it): the memo has no demand cell for it, so a
    /// deref for that position fails closed with this typed error rather
    /// than fabricating a body. (The binding-annotation position
    /// (`MacroPayloadPosition::TypeAnnotation`) and the per-field position
    /// (`MacroPayloadPosition::Field`) are HYDRATED — served by the
    /// dedicated `transient_props_annotation_body` /
    /// `transient_macro_field_payload` demand cells, not this unrouted
    /// miss.)
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
/// owning declaration's generic parameters and their TS lexical visibility
/// from the derefed position (so the session phase can bind them as
/// `TypeParam` shells in the authored position's own lexical scope under
/// the correct per-position frame). NEVER a `SemanticNodeId` — graph
/// lowering is the session phase's job.
#[derive(Debug, Clone)]
pub(crate) struct DerefedAuthoredBody {
    pub(crate) shape: DerefedBodyShape,
    /// The owning declaration's FULL header type-parameter list, in source
    /// order — never pre-truncated. Which of them the shape may reference
    /// is `visibility`'s to say.
    pub(crate) type_parameters: Vec<TypeParam>,
    /// TS lexical visibility of `type_parameters` from the derefed
    /// position: a body position sees every parameter; a constraint bound
    /// sees every sibling (forward refs included); a default bound sees
    /// prior siblings only, with self / later siblings present-as-shadow
    /// but forbidden as references.
    pub(crate) visibility: TypeParamVisibility,
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
            AuthoredBodyLocator::JsdocTypedefBody(typedef) => &typedef.anchor,
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
            // A JSDoc typedef declares no header type parameters, so ANY
            // `TypeParamBound` step over its payload is misplaced.
            AuthoredBodyLocator::JsdocTypedefBody(typedef) => {
                if typedef
                    .path
                    .iter()
                    .any(|step| matches!(step, TypeBodyPathStep::TypeParamBound { .. }))
                {
                    return Err(LocatorBodyDerefError::TypeParamBoundStepMisplaced);
                }
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
                // No deref route exists for the whole-object-argument
                // position (no producer mints it); fail closed with the
                // typed non-result, never a fabricated body.
                MacroPayloadPosition::ObjectArgument => {
                    Err(LocatorBodyDerefError::MacroPayloadPositionUnrouted)
                }
                // A per-FIELD payload: hydrated by the dedicated lease-only
                // re-derivation over THIS memo's retained snapshot, which
                // replays the analyzer's OWN macro assembly (mint side and
                // deref side share one macro-ordinal / field-ordinal
                // addressing engine). The authored SHAPE is served — a prop's
                // annotation, an emit's payload type / call-signature tuple,
                // a slot's return type, a `defineModel` type argument. A
                // macro-payload position carries no header type parameters.
                // The two authored absences stay typed: an addressed field
                // with NO authored payload is the value-annotation absence;
                // an ordinal addressing no field at all is the path miss.
                MacroPayloadPosition::Field { field_index } => {
                    let lowering = transient_outcome(
                        self.transient_macro_field_payload(payload.macro_index, field_index),
                    )?;
                    match lowering.as_ref() {
                        MacroFieldPayloadLowering::Payload(expr) => Ok(DerefedAuthoredBody {
                            shape: DerefedBodyShape::Single(expr.clone()),
                            type_parameters: Vec::new(),
                            visibility: TypeParamVisibility::Body,
                        }),
                        MacroFieldPayloadLowering::Unauthored => {
                            Err(LocatorBodyDerefError::ValueAnnotationAbsent)
                        }
                        MacroFieldPayloadLowering::NoField => {
                            Err(LocatorBodyDerefError::PathUnresolved)
                        }
                    }
                }
                // The `$props()` binding-annotation payload
                // (`let {..}: T = $props()`): hydrated by the dedicated
                // lease-only re-derivation over THIS memo's retained
                // snapshot, which replays the capture's shared macro-ordinal
                // walk (mint side and deref side share one addressing
                // engine). An annotation position carries no header type
                // parameters. The two authored absences stay typed: an
                // addressed `$props()` with NO authored annotation is the
                // value-annotation absence; an ordinal addressing no
                // `$props()` call at all is the path miss.
                MacroPayloadPosition::TypeAnnotation => {
                    let lowering = transient_outcome(
                        self.transient_props_annotation_body(payload.macro_index),
                    )?;
                    match lowering.as_ref() {
                        PropsAnnotationLowering::Annotation(expr) => Ok(DerefedAuthoredBody {
                            shape: DerefedBodyShape::Single(expr.clone()),
                            // A binding annotation declares no header type
                            // parameters (mirrors the JsdocTypedefBody arm).
                            type_parameters: Vec::new(),
                            visibility: TypeParamVisibility::Body,
                        }),
                        PropsAnnotationLowering::Unannotated => {
                            Err(LocatorBodyDerefError::ValueAnnotationAbsent)
                        }
                        PropsAnnotationLowering::NoPropsCall => {
                            Err(LocatorBodyDerefError::PathUnresolved)
                        }
                    }
                }
            },
            AuthoredBodyLocator::DeclBody(slot) => {
                match slot.anchor.space {
                    LocatorSymbolSpace::Type => {
                        // The demand CELL serves the fact/locator record (the
                        // merged-carrier shape + header type parameters) —
                        // body lowering is a fact-level event that runs once
                        // per (canonical, content, symbol). A file-scope miss
                        // falls through to the GLOBAL ambient inventory — the
                        // same file-scope-then-global resolution order the
                        // prepared-decl route applies. A BROKEN-lease demand
                        // surfaces the DISTINCT `LeaseMiss` (a transient
                        // no-warm ReturnOnly), never collapsed into the
                        // cacheable `UnknownSymbol`.
                        let symbol = slot.anchor.symbol.as_ref();
                        let (lowered, aug_scope) = match self.type_decl_outcome(symbol) {
                            DemandOutcome::LeaseMiss => {
                                return Err(LocatorBodyDerefError::LeaseMiss)
                            }
                            DemandOutcome::Ready(Some(lowered)) => (lowered, None),
                            DemandOutcome::Ready(None) => {
                                match self.augmentation_type_decl_outcome(
                                    &AugmentationScopeKind::Global,
                                    symbol,
                                ) {
                                    DemandOutcome::LeaseMiss => {
                                        return Err(LocatorBodyDerefError::LeaseMiss)
                                    }
                                    DemandOutcome::Ready(Some(lowered)) => {
                                        (lowered, Some(AugmentationScopeKind::Global))
                                    }
                                    DemandOutcome::Ready(None) => {
                                        return Err(LocatorBodyDerefError::UnknownSymbol)
                                    }
                                }
                            }
                        };
                        // A dual-space `enum`'s TYPE surface is the union of
                        // its members' projected scalars, DERIVED from the
                        // MERGED value members (`ValueDeclGroup::enum_type_union`
                        // is the single source of truth) — the type-space
                        // contributor registers only a transient placeholder
                        // body. Serve the derived union here so `Status`
                        // used as a type (`` `${Status}` ``, unions,
                        // assignability) resolves to the member-value union,
                        // never the placeholder.
                        if aug_scope.is_none() {
                            if let DemandOutcome::Ready(Some(value_decl)) =
                                self.value_decl_outcome(symbol)
                            {
                                if let Some(members) = value_decl.enum_members.as_ref() {
                                    let arms: Vec<TypeExpr> = members
                                        .members
                                        .iter()
                                        .map(|entry| {
                                            crate::project_semantic_dispatch::lower::enum_scalar_type_expr(
                                                &entry.value,
                                            )
                                        })
                                        .collect();
                                    let union = TypeExpr::union(arms);
                                    return navigate_type_space_body(
                                        DerefedBodyShape::Single(union),
                                        &lowered.type_parameters,
                                        &slot.path,
                                    );
                                }
                            }
                        }
                        // The record stores content-free body LOCATORS; the
                        // authored typed IR is re-borrowed from the retained
                        // snapshot by the lease-only transient service (the
                        // graph-tier `LowerLocator` memo owns caching the
                        // lowered product).
                        let bodies = match aug_scope.as_ref() {
                            None => self.transient_type_bodies(symbol),
                            Some(scope) => self.transient_augmentation_type_bodies(scope, symbol),
                        };
                        let bodies = transient_outcome(bodies)?;
                        let shape = transient_body_shape(&lowered, bodies)?;
                        // A type-decl-header type parameter's bound (leading
                        // `TypeParamBound` step) plus any post-bound descent
                        // route through the ONE shared type-space navigator,
                        // exactly as the augmentation type-space branch does.
                        navigate_type_space_body(shape, &lowered.type_parameters, &slot.path)
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
                        // The transient value-part service carries the SAME
                        // lease-miss / genuine-miss discrimination the demand
                        // cells carry (header presence is checked inside).
                        let parts = transient_outcome(
                            self.transient_value_parts(slot.anchor.symbol.as_ref()),
                        )?;
                        let expr = navigate_value_parts(&parts, &slot.path)?;
                        Ok(DerefedAuthoredBody {
                            shape: DerefedBodyShape::Single(expr),
                            // A plain value annotation position binds no
                            // declared type parameters of its own; a
                            // dual-space declaration (a `class K<T>` whose
                            // constructor shape references `T`) binds its
                            // HEADER parameters, re-borrowed from the same
                            // statements' type-side parts.
                            type_parameters: parts.type_parameters.clone(),
                            visibility: TypeParamVisibility::Body,
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
                        // Serve the fact record through the memo's scoped lazy
                        // demand cell, then re-borrow the transient bodies. A
                        // broken-lease demand surfaces the DISTINCT `LeaseMiss`
                        // no-warm signal, never a cacheable `UnknownSymbol`.
                        let symbol = aug.anchor.symbol.as_ref();
                        let lowered = match self.augmentation_type_decl_outcome(&scope_kind, symbol)
                        {
                            DemandOutcome::LeaseMiss => {
                                return Err(LocatorBodyDerefError::LeaseMiss)
                            }
                            DemandOutcome::Ready(Some(lowered)) => lowered,
                            DemandOutcome::Ready(None) => {
                                return Err(LocatorBodyDerefError::UnknownSymbol)
                            }
                        };
                        let bodies = transient_outcome(
                            self.transient_augmentation_type_bodies(&scope_kind, symbol),
                        )?;
                        let shape = transient_body_shape(&lowered, bodies)?;
                        // An augmentation-scoped `interface` / `type` decl is an
                        // authored type-decl-header decl, so its type-param
                        // bounds and body sub-positions navigate through the
                        // SAME shared type-space navigator as a top-level decl.
                        // An empty `path` preserves the whole-body Single/Merged
                        // behavior unchanged.
                        navigate_type_space_body(shape, &lowered.type_parameters, &aug.path)
                    }
                    LocatorSymbolSpace::Value | LocatorSymbolSpace::Namespace => {
                        Err(LocatorBodyDerefError::AugmentationBodySpaceUnrouted)
                    }
                }
            }
            AuthoredBodyLocator::JsdocTypedefBody(typedef) => {
                // The typedef locator addresses the COMMENT-derived payload
                // specifically (never a same-name TS declaration's statement
                // body) — served by the dedicated lease-only re-derivation.
                let body = transient_outcome(
                    self.transient_jsdoc_typedef_body(typedef.anchor.symbol.as_ref()),
                )?;
                let expr = navigate_expr(body.as_ref().clone(), &typedef.path)?;
                Ok(DerefedAuthoredBody {
                    shape: DerefedBodyShape::Single(expr),
                    // A JSDoc typedef declares no header type parameters.
                    type_parameters: Vec::new(),
                    visibility: TypeParamVisibility::Body,
                })
            }
        }
    }

    /// Deref ONE authored type-argument position ([`TypeArgLocator`]) — the
    /// heritage-argument demand rail. Navigates the producer-emitted `path`
    /// to the arg-BEARING `Ref` position through the shared decl-body deref
    /// ([`Self::deref_locator_body`] — same lease-only purity, same
    /// canonical-coherence gate), then selects the `arg_index`-th authored
    /// type argument. Fail-closed: a non-`Ref` position or an out-of-range
    /// ordinal is the typed [`LocatorBodyDerefError::PathUnresolved`] — never
    /// a fabricated argument.
    pub(crate) fn deref_type_arg(
        &self,
        locator: &verter_type_expr::locators::TypeArgLocator,
    ) -> Result<TypeExpr, LocatorBodyDerefError> {
        let body = self.deref_locator_body(&AuthoredBodyLocator::DeclBody(
            verter_type_expr::locators::TypeBodySlot {
                anchor: locator.anchor.clone(),
                path: Arc::clone(&locator.path),
            },
        ))?;
        let DerefedBodyShape::Single(expr) = body.shape else {
            // An arg-bearing position is always a single navigated
            // sub-position — a whole merged carrier cannot bear type args.
            return Err(LocatorBodyDerefError::PathUnresolved);
        };
        match unwrap_parenthesized(expr) {
            TypeExpr::Ref {
                ref type_arguments, ..
            } => type_arguments
                .get(locator.arg_index as usize)
                .cloned()
                .ok_or(LocatorBodyDerefError::PathUnresolved),
            _ => Err(LocatorBodyDerefError::PathUnresolved),
        }
    }
}

/// Collapse a transient-service [`DemandOutcome`] into the deref's typed
/// error vocabulary: a genuine miss is the cacheable [`UnknownSymbol`]
/// (the symbol is not inventoried, the memo is seeded/service-less, or the
/// parse was fatal), a broken lease pin is the transient
/// [`LeaseMiss`] ReturnOnly.
///
/// [`UnknownSymbol`]: LocatorBodyDerefError::UnknownSymbol
/// [`LeaseMiss`]: LocatorBodyDerefError::LeaseMiss
fn transient_outcome<T>(outcome: DemandOutcome<T>) -> Result<Arc<T>, LocatorBodyDerefError> {
    match outcome {
        DemandOutcome::LeaseMiss => Err(LocatorBodyDerefError::LeaseMiss),
        DemandOutcome::Ready(None) => Err(LocatorBodyDerefError::UnknownSymbol),
        DemandOutcome::Ready(Some(value)) => Ok(value),
    }
}

/// Assemble the derefed whole-body SHAPE from the fact record's body CARRIER
/// (which knows whether the declaration is a same-name merge) and the
/// re-borrowed transient contributor bodies: a merged carrier keeps the
/// DISTINCT per-contributor structure (source order); a single carrier takes
/// the last-wins (primary) body. An empty re-borrow for an inventoried
/// symbol is a statement-drift shape mismatch — fail closed.
fn transient_body_shape(
    lowered: &super::LoweredTypeDecl,
    bodies: Arc<Vec<TypeExpr>>,
) -> Result<DerefedBodyShape, LocatorBodyDerefError> {
    let mut bodies = bodies.as_ref().clone();
    if lowered.body.is_merged() {
        if bodies.is_empty() {
            return Err(LocatorBodyDerefError::PathUnresolved);
        }
        Ok(DerefedBodyShape::Merged(bodies))
    } else {
        match bodies.pop() {
            Some(primary) => Ok(DerefedBodyShape::Single(primary)),
            None => Err(LocatorBodyDerefError::PathUnresolved),
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
/// selects the constraint / default bound of the parameter at `ordinal`, and
/// the remaining steps navigate over the selected bound. The returned
/// `type_parameters` are ALWAYS the FULL sibling list; the returned
/// [`TypeParamVisibility`] says which of them the derefed position may
/// reference — a constraint sees every sibling (TS constraints may reference
/// later siblings and self), a default sees prior siblings only with self /
/// later siblings present-as-shadow but forbidden. Any other path navigates
/// the body directly with body visibility; an empty path yields the whole
/// body (preserving the merged-contributor carrier).
///
/// Placement of a leading bound is presumed already validated by
/// [`validate_type_param_bound_placement`]; a mid-path bound reaching
/// [`navigate_expr`] still fails closed there as defense-in-depth.
fn navigate_type_space_body(
    body: DerefedBodyShape,
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
            // The FULL sibling list plus the bound's position-exact
            // visibility: the binder-frame constructor reconstructs the TS
            // lexical view from `(all params, ordinal, position)` — a
            // pre-truncated prefix could not express a constraint's
            // later-sibling / self references nor a default's
            // present-but-forbidden shadow entries.
            type_parameters: type_parameters.to_vec(),
            visibility: match position {
                TypeParamBoundPosition::Constraint => TypeParamVisibility::Constraint { ordinal },
                TypeParamBoundPosition::Default => TypeParamVisibility::Default { ordinal },
            },
        });
    }
    let shape = navigate_type_body(body, path)?;
    Ok(DerefedAuthoredBody {
        shape,
        type_parameters: type_parameters.to_vec(),
        visibility: TypeParamVisibility::Body,
    })
}

/// Navigate a producer-emitted [`TypeBodyPathStep`] path over the OWNED
/// re-borrowed body shape. Empty path = the whole body (preserving the
/// merged-contributor carrier); a non-empty path selects exactly the named
/// sub-position. Fail-closed: any shape/ordinal mismatch is
/// [`LocatorBodyDerefError::PathUnresolved`].
fn navigate_type_body(
    body: DerefedBodyShape,
    path: &[TypeBodyPathStep],
) -> Result<DerefedBodyShape, LocatorBodyDerefError> {
    let Some((first, rest)) = path.split_first() else {
        return Ok(body);
    };
    let (start, remaining) = match (body, first) {
        (
            DerefedBodyShape::Merged(contributors),
            TypeBodyPathStep::MergedContributor { ordinal },
        ) => {
            let expr = contributors
                .into_iter()
                .nth(*ordinal as usize)
                .ok_or(LocatorBodyDerefError::PathUnresolved)?;
            (expr, rest)
        }
        // A merged body's sub-positions are addressed through a contributor
        // step first; any other first step is unresolvable by shape.
        (DerefedBodyShape::Merged(_), _) => return Err(LocatorBodyDerefError::PathUnresolved),
        // A single body has no contributor axis; the whole path navigates
        // the body expression directly.
        (DerefedBodyShape::Single(expr), _) => (expr, path),
    };
    navigate_expr(start, remaining).map(DerefedBodyShape::Single)
}

/// Navigate a producer-emitted VALUE-space path over the re-borrowed
/// transient value parts. The producer vocabulary is closed:
///
/// - `[ValueSignature { k }, …]` — the GROUP-level `k`-th authored function
///   signature (an overload-group member); an empty remainder derefs to the
///   WHOLE signature (the whole-signature recovery the unannotated-position
///   facts rely on), `FunctionParam`/`FunctionReturn` descend into it.
/// - `[Member { k }, …]` — the `k`-th member of the const-object shape
///   (`MemberValue` / signature / index-signature sub-steps descend).
/// - anything else — a sub-position of the authored ANNOTATION body
///   (empty path = the whole annotation; an absent annotation is the typed
///   [`LocatorBodyDerefError::ValueAnnotationAbsent`] miss).
fn navigate_value_parts(
    parts: &TransientValueParts,
    path: &[TypeBodyPathStep],
) -> Result<TypeExpr, LocatorBodyDerefError> {
    match path.first() {
        Some(TypeBodyPathStep::ValueSignature { ordinal }) => {
            let signature = parts
                .signatures
                .get(*ordinal as usize)
                .ok_or(LocatorBodyDerefError::PathUnresolved)?;
            navigate_signature_parts(signature, &path[1..])
        }
        Some(TypeBodyPathStep::Member { .. }) if parts.object_shape.is_some() => {
            let shape = parts
                .object_shape
                .clone()
                .expect("guarded by the arm condition");
            navigate_expr(TypeExpr::Object(Arc::new(shape)), path)
        }
        _ => {
            // The value declaration's TYPE SURFACE, in the same precedence
            // the `typeof` projection applies: the authored/inferred
            // annotation first, else the const-object / class-constructor
            // shape. A shapeless, annotation-less position is the typed
            // absence.
            if let Some(annotation) = parts.type_annotation.clone() {
                return navigate_expr(annotation, path);
            }
            if let Some(shape) = parts.object_shape.clone() {
                return navigate_expr(TypeExpr::Object(Arc::new(shape)), path);
            }
            Err(LocatorBodyDerefError::ValueAnnotationAbsent)
        }
    }
}

/// Navigate the remainder of a `[ValueSignature { k }, …]` path over one
/// transient signature's typed IR. An empty remainder recovers the WHOLE
/// signature as its function type.
fn navigate_signature_parts(
    signature: &LoweredSignatureParts,
    rest: &[TypeBodyPathStep],
) -> Result<TypeExpr, LocatorBodyDerefError> {
    match rest.first() {
        None => Ok(TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
            signature.parameters.clone(),
            signature.return_type.clone().map(Arc::new),
            signature.type_parameters.clone(),
        )))),
        Some(TypeBodyPathStep::FunctionParam { ordinal }) => {
            let param = signature
                .parameters
                .get(*ordinal as usize)
                .ok_or(LocatorBodyDerefError::PathUnresolved)?;
            navigate_expr(param.ty.clone(), &rest[1..])
        }
        Some(TypeBodyPathStep::FunctionReturn) => {
            let return_type = signature
                .return_type
                .clone()
                .ok_or(LocatorBodyDerefError::PathUnresolved)?;
            navigate_expr(return_type, &rest[1..])
        }
        Some(_) => Err(LocatorBodyDerefError::PathUnresolved),
    }
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
            (NavigatePosition::Expr(expr), TypeBodyPathStep::UnionArm { ordinal }) => {
                match unwrap_parenthesized(expr) {
                    TypeExpr::Union(ref arms) => NavigatePosition::Expr(
                        arms.get(*ordinal as usize)
                            .cloned()
                            .ok_or(LocatorBodyDerefError::PathUnresolved)?,
                    ),
                    _ => return Err(LocatorBodyDerefError::PathUnresolved),
                }
            }
            (NavigatePosition::Expr(expr), TypeBodyPathStep::IndexedAccessObject) => {
                match unwrap_parenthesized(expr) {
                    TypeExpr::IndexedAccess { ref object, .. } => {
                        NavigatePosition::Expr(object.as_ref().clone())
                    }
                    _ => return Err(LocatorBodyDerefError::PathUnresolved),
                }
            }
            (NavigatePosition::Expr(expr), TypeBodyPathStep::IndexedAccessIndex) => {
                match unwrap_parenthesized(expr) {
                    TypeExpr::IndexedAccess { ref index, .. } => {
                        NavigatePosition::Expr(index.as_ref().clone())
                    }
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
            // A selected FUNCTION-LIKE member's authored parameter / return
            // positions (`[Member { k }, FunctionParam { i }]` /
            // `[Member { k }, FunctionReturn]` — the member-signature fact
            // vocabulary). The member's function IR is recovered first, then
            // the sub-position selected; an absent authored position is the
            // typed miss.
            (NavigatePosition::Member(member), TypeBodyPathStep::FunctionParam { ordinal }) => {
                let function = member_function_expr(member)?;
                NavigatePosition::Expr(
                    function
                        .parameters
                        .get(*ordinal as usize)
                        .map(|param| param.ty.clone())
                        .ok_or(LocatorBodyDerefError::PathUnresolved)?,
                )
            }
            (NavigatePosition::Member(member), TypeBodyPathStep::FunctionReturn) => {
                let function = member_function_expr(member)?;
                NavigatePosition::Expr(
                    function
                        .return_type
                        .as_deref()
                        .cloned()
                        .ok_or(LocatorBodyDerefError::PathUnresolved)?,
                )
            }
            // A selected INDEX-SIGNATURE member's authored key / value
            // positions.
            (NavigatePosition::Member(member), TypeBodyPathStep::IndexSignatureKey) => match member
            {
                ObjectMember::IndexSignature(index) => {
                    NavigatePosition::Expr(index.key_type.clone())
                }
                _ => return Err(LocatorBodyDerefError::PathUnresolved),
            },
            (NavigatePosition::Member(member), TypeBodyPathStep::IndexSignatureValue) => {
                match member {
                    ObjectMember::IndexSignature(index) => {
                        NavigatePosition::Expr(index.value_type.clone())
                    }
                    _ => return Err(LocatorBodyDerefError::PathUnresolved),
                }
            }
            // A bare function-typed EXPRESSION position's parameter / return
            // sub-steps (defensive totality over the closed vocabulary — the
            // producers address function positions through their member step,
            // but a function-valued expression position remains navigable).
            (NavigatePosition::Expr(expr), TypeBodyPathStep::FunctionParam { ordinal }) => {
                match unwrap_parenthesized(expr) {
                    TypeExpr::Function(ref function) => NavigatePosition::Expr(
                        function
                            .parameters
                            .get(*ordinal as usize)
                            .map(|param| param.ty.clone())
                            .ok_or(LocatorBodyDerefError::PathUnresolved)?,
                    ),
                    _ => return Err(LocatorBodyDerefError::PathUnresolved),
                }
            }
            (NavigatePosition::Expr(expr), TypeBodyPathStep::FunctionReturn) => {
                match unwrap_parenthesized(expr) {
                    TypeExpr::Function(ref function) => NavigatePosition::Expr(
                        function
                            .return_type
                            .as_deref()
                            .cloned()
                            .ok_or(LocatorBodyDerefError::PathUnresolved)?,
                    ),
                    _ => return Err(LocatorBodyDerefError::PathUnresolved),
                }
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

/// The function IR of a selected FUNCTION-LIKE object member (a method /
/// call-signature / construct-signature — the positions the member-signature
/// facts descend with `FunctionParam` / `FunctionReturn`). A property or
/// index signature is not function-like — fail closed.
fn member_function_expr(member: ObjectMember) -> Result<FunctionExpr, LocatorBodyDerefError> {
    match member {
        ObjectMember::Method(method) => Ok(method.function),
        ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => Ok(func),
        ObjectMember::Property(_) | ObjectMember::IndexSignature(_) => {
            Err(LocatorBodyDerefError::PathUnresolved)
        }
    }
}
