//! The shared `SemanticTypeSource` / locator → [`HotTypeRef`] raising bridge.
//!
//! Consumers holding a lower-crate typed SOURCE (a [`SemanticTypeSource`], an
//! [`AuthoredBodyLocator`], or a session-side [`SessionDemandIdentity`]) and
//! needing a semantic-graph handle for a node-domain decision call ONE of the
//! three raise entries below. Consumers NEVER arm-match a source themselves —
//! the arm routing lives here, and every arm routes through an EXISTING
//! single-engine entry:
//!
//! - authored bodies → [`ProjectSemanticDispatch::lower_locator`] (the
//!   first-class `SemanticQueryKey::LowerLocator` memoized query);
//! - the macro generic type argument → `macro_type_arg_hot_ref` (its sole
//!   sanctioned producer — the memo deliberately rejects a locator deref for
//!   that position so a second producer path cannot exist);
//! - closed leaf facts → the closed-grammar `leaf_type_fact_expr` projection
//!   lowered through the shared in-scope lowerer;
//! - closed / projected / synthesized COMPOSITE facts → a fact-shell
//!   composition that interns the composite carrier node directly, with every
//!   interior body position lowered through `lower_locator` (composition is a
//!   data assembly over already-defined closed facts — reference RESOLUTION
//!   still happens only at the consuming dispatch demands).
//!
//! The returned [`HotTypeRef`] is transient: never persisted, never a cache
//! key (it deliberately implements neither `Hash` nor an ordering). Warm /
//! persisted identity stays the content-free source the caller already holds.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_type_expr::facts::{ClosedTypeFact, ProjectedTypeFact, SemanticTypeSource};
use verter_type_expr::locators::{AuthoredAnchor, AuthoredBodyLocator, MacroPayloadPosition};

use super::ProjectSemanticDispatch;
#[cfg(test)]
use crate::locator_identity::{SessionDemandIdentity, SessionDemandRoute};
use crate::semantic_query::{
    HotTypeRef, NodeScopeId, PathSegment, ProjectionReductionContext, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};

/// Raise context for [`ProjectSemanticDispatch::raise_semantic_type_source_to_hot`]:
/// the consuming scope's canonical id (the file whose name resolution an
/// in-scope leaf lowering runs under, and the anchor a producer-local — empty
/// anchored — locator absolutizes against) plus the caller's exact
/// projection-reduction context.
pub(crate) struct SourceRaiseContext<'a> {
    pub scope_canonical_id: &'a str,
    pub scope_owner: verter_type_expr::TopLevelOwnerId,
    pub context: ProjectionReductionContext,
    /// STRICT-mode interior-failure sink: when armed, every failed
    /// dereference of a PRESENT interior position of a composed fact shell
    /// records its nested path here (first failure wins). Genuinely ABSENT
    /// schema positions (an unannotated parameter, an inferred return, an
    /// absent constraint/default) are NOT failures — they keep interning
    /// the typed miss the demand side re-derives. `None` = the lenient
    /// raise every non-output consumer uses (composition still interns the
    /// typed-miss carrier for interior misses).
    pub interior_failures: Option<&'a InteriorFailureSink>,
}

/// First-failure interior sink for the STRICT source raise
/// ([`ProjectSemanticDispatch::raise_semantic_type_source_to_hot_strict`]).
/// Tracks the live composition path as a stack; the first recorded failure
/// snapshots the stack into the typed
/// [`InteriorSourceStep`](crate::meta_resolve::InteriorSourceStep) path the
/// output error transports, together with its FAILURE KIND.
#[derive(Default)]
pub(crate) struct InteriorFailureSink {
    stack: std::cell::RefCell<Vec<crate::meta_resolve::InteriorSourceStep>>,
    first_failure: std::cell::RefCell<Option<StrictSourceRaiseFailure>>,
}

/// The typed failure of a STRICT source raise — the two fail-closed classes
/// the terminal output sink maps onto its output errors.
#[derive(Debug, Clone)]
pub(crate) enum StrictSourceRaiseFailure {
    /// A PRESENT interior locator of a composed fact shell FAILED its
    /// dereference (the composition interned the typed miss carrier).
    /// Carries the nested position path from the source root.
    InteriorMiss(Arc<[crate::meta_resolve::InteriorSourceStep]>),
    /// A successfully-raised schema-PRESENT position (a direct source-root
    /// deref or a composed shell's present slot) materializes an
    /// unknown-materializing failure carrier at its ROOT or INTERIOR — the
    /// shell fold would render a completed `unknown` for it. Conservative:
    /// the graph carries no per-position absent-vs-failed `Opaque`
    /// provenance, so a deref'd body's interior failure fails the raise.
    /// Proven schema absence never trips this: an ABSENT schema slot of a
    /// composed shell interns the typed miss directly WITHOUT a deref, so
    /// it is never checked — the schema `Option` is the absence proof.
    UnknownMaterializing(Arc<[crate::meta_resolve::InteriorSourceStep]>),
}

impl InteriorFailureSink {
    fn record(&self) {
        let mut slot = self.first_failure.borrow_mut();
        if slot.is_none() {
            *slot = Some(StrictSourceRaiseFailure::InteriorMiss(Arc::from(
                self.stack.borrow().clone().into_boxed_slice(),
            )));
        }
    }

    fn record_unknown_materializing(&self) {
        let mut slot = self.first_failure.borrow_mut();
        if slot.is_none() {
            *slot = Some(StrictSourceRaiseFailure::UnknownMaterializing(Arc::from(
                self.stack.borrow().clone().into_boxed_slice(),
            )));
        }
    }

    fn take_failure(&self) -> Option<StrictSourceRaiseFailure> {
        self.first_failure.borrow_mut().take()
    }
}

impl SourceRaiseContext<'_> {
    /// Run `f` with `step` pushed on the strict sink's composition path
    /// (no-op wrapper when the raise is lenient).
    pub(in crate::project_semantic_dispatch) fn with_interior_step<R>(
        &self,
        step: crate::meta_resolve::InteriorSourceStep,
        f: impl FnOnce() -> R,
    ) -> R {
        if let Some(sink) = self.interior_failures {
            sink.stack.borrow_mut().push(step);
            let out = f();
            sink.stack.borrow_mut().pop();
            out
        } else {
            f()
        }
    }

    /// Record a failed dereference of a PRESENT interior position at the
    /// current composition path (first failure wins; lenient = no-op).
    pub(in crate::project_semantic_dispatch) fn record_interior_failure(&self) {
        if let Some(sink) = self.interior_failures {
            sink.record();
        }
    }

    /// STRICT-path conservative integrity check on a SUCCESSFULLY-raised
    /// schema-PRESENT position (a direct source-root deref or a composed
    /// shell's present slot): when the raised node's materialized shape
    /// carries an unknown-materializing failure carrier ANYWHERE — root or
    /// interior (the shared node-domain whole-tree miss fact; the
    /// legitimately publishable carriers, a recursive reference and a
    /// declaration placeholder, are not misses) — record the typed failure
    /// at the current composition path. Lenient raises skip the check (and
    /// its fold cost) entirely. The SCHEMA split keeps this conservative
    /// check honest: an ABSENT schema slot never raises (the composition
    /// interns the typed miss directly), so a composed shell's
    /// proven-absent interiors keep rendering the typed `Unknown` while a
    /// deref'd body carrying an interior failure fails closed.
    pub(in crate::project_semantic_dispatch) fn check_raised_unknown_materializing(
        &self,
        dispatch: &ProjectSemanticDispatch<'_>,
        hot: Option<&HotTypeRef>,
    ) {
        let Some(sink) = self.interior_failures else {
            return;
        };
        let Some(hot) = hot else {
            return;
        };
        if super::raise::node_contains_semantic_miss_with_dispatch(dispatch, hot.node())
            == Some(true)
        {
            sink.record_unknown_materializing();
        }
    }
}

impl ProjectSemanticDispatch<'_> {
    /// Raise a lower-crate [`SemanticTypeSource`] to a transient semantic-graph
    /// handle through the one shared engine. `None` = the source has no
    /// live graph representation under the current view (unknown file, memo
    /// deref miss, unrouted payload position) — the caller keeps the source
    /// shallow and never fabricates a stand-in node.
    ///
    /// The match is EXHAUSTIVE over every `SemanticTypeSource` arm and every
    /// nested fact arm — a new source arm fails compilation here until it is
    /// explicitly routed.
    pub(crate) fn raise_semantic_type_source_to_hot(
        &self,
        source: &SemanticTypeSource,
        ctx: SourceRaiseContext<'_>,
    ) -> Option<HotTypeRef> {
        match source {
            SemanticTypeSource::Authored(locator) => {
                let locator = absolutize_locator(locator, ctx.scope_canonical_id);
                let hot = self.raise_authored_locator_to_hot(&locator, ctx.context);
                ctx.check_raised_unknown_materializing(self, hot.as_ref());
                hot
            }
            SemanticTypeSource::Projected(fact) => match fact {
                ProjectedTypeFact::Member(member) => {
                    let hot = self.raise_body_slot(&member.ty, ctx.scope_canonical_id);
                    ctx.check_raised_unknown_materializing(self, hot.as_ref());
                    hot
                }
                ProjectedTypeFact::IndexSignature(signature) => {
                    let hot = self.raise_body_slot(&signature.value_type, ctx.scope_canonical_id);
                    ctx.check_raised_unknown_materializing(self, hot.as_ref());
                    hot
                }
                ProjectedTypeFact::CallSignature(signature) => {
                    Some(self.compose_function_fact_node(signature, &ctx, false))
                }
                ProjectedTypeFact::ConstructSignature(signature) => {
                    Some(self.compose_function_fact_node(signature, &ctx, true))
                }
                ProjectedTypeFact::Surface(surface) => {
                    Some(self.compose_projected_surface_node(surface, &ctx))
                }
                ProjectedTypeFact::MemberPath { base, path } => {
                    self.raise_projected_member_path(base, path, &ctx)
                }
                ProjectedTypeFact::CallableParams {
                    base,
                    signature_ordinal,
                    first_param,
                } => self.raise_projected_callable_params(
                    base,
                    *signature_ordinal,
                    *first_param,
                    &ctx,
                ),
                ProjectedTypeFact::IndexPosition {
                    base,
                    signature_ordinal,
                    position,
                } => self.raise_projected_index_position(base, *signature_ordinal, *position, &ctx),
            },
            SemanticTypeSource::Synthesized(shape) => {
                Some(self.raise_synthesized_shape(shape, &ctx))
            }
            SemanticTypeSource::Closed(fact) => match fact {
                ClosedTypeFact::Leaf(leaf) => self.raise_leaf_fact(leaf, &ctx),
                // A closed leaf-union composes directly: each leaf lowers
                // through the shared in-scope lowerer and the ordered union
                // node is interned as data (a decided result — no
                // re-resolution, no normalization pass).
                ClosedTypeFact::LeafUnion(leaves) => {
                    let scope = self.raise_scope(&ctx);
                    let members: Vec<SemanticNodeId> = leaves
                        .iter()
                        .enumerate()
                        .map(|(ordinal, leaf)| {
                            self.raise_required_interior(
                                &ctx,
                                &scope,
                                crate::meta_resolve::InteriorSourceStep::UnionArm {
                                    ordinal: ordinal as u32,
                                },
                                || self.raise_leaf_fact(leaf, &ctx),
                            )
                        })
                        .collect();
                    Some(HotTypeRef::new(self.graph().intern_node_with_scope(
                        SemanticNodeData::Union(Arc::from(members.into_boxed_slice())),
                        scope,
                    )))
                }
                ClosedTypeFact::Object(object) => Some(self.compose_object_fact_node(object, &ctx)),
                ClosedTypeFact::Function(signature) => {
                    Some(self.compose_function_fact_node(signature, &ctx, false))
                }
                ClosedTypeFact::Tuple(tuple) => Some(self.compose_tuple_fact_node(tuple, &ctx)),
                ClosedTypeFact::IndexedAccess(access) => {
                    Some(self.compose_indexed_access_fact_node(access, &ctx))
                }
            },
            SemanticTypeSource::SyntheticSlotBinding(key) => {
                Some(self.raise_synthetic_binding_source_to_hot(key, &ctx))
            }
        }
    }

    /// STRICT raise for the terminal output sink: like
    /// [`Self::raise_semantic_type_source_to_hot`], but two fail-closed
    /// classes PROPAGATE as typed errors instead of silently rendering as
    /// `Unknown` through the shell fold:
    ///
    /// - a failed dereference of a PRESENT interior position of a composed
    ///   fact shell ([`StrictSourceRaiseFailure::InteriorMiss`], carrying
    ///   the nested position path);
    /// - a SUCCESSFULLY-raised schema-PRESENT position whose materialized
    ///   shape carries an unknown-materializing failure carrier at its root
    ///   or interior ([`StrictSourceRaiseFailure::UnknownMaterializing`] —
    ///   the conservative interior fail-close; see
    ///   [`SourceRaiseContext::check_raised_unknown_materializing`]).
    ///
    /// Genuinely ABSENT schema positions (an unannotated parameter, a
    /// deliberately slot-less signature return, an absent constraint/default) stay typed misses
    /// exactly as in the lenient raise and keep rendering the typed
    /// `Unknown` — the two are distinguished by the SCHEMA (present locator
    /// vs absent option), never a heuristic.
    ///
    /// `Ok(None)` remains the ROOT-level "no live graph representation"
    /// non-result (the caller's fail-closed unraisable-source arm).
    pub(crate) fn raise_semantic_type_source_to_hot_strict(
        &self,
        source: &SemanticTypeSource,
        scope_canonical_id: &str,
        scope_owner: verter_type_expr::TopLevelOwnerId,
        context: ProjectionReductionContext,
    ) -> Result<Option<HotTypeRef>, StrictSourceRaiseFailure> {
        let sink = InteriorFailureSink::default();
        let hot = self.raise_semantic_type_source_to_hot(
            source,
            SourceRaiseContext {
                scope_canonical_id,
                scope_owner,
                context,
                interior_failures: Some(&sink),
            },
        );
        match sink.take_failure() {
            Some(failure) => Err(failure),
            None => Ok(hot),
        }
    }

    /// Raise an authored body locator to a transient graph handle.
    ///
    /// The macro generic TYPE-ARGUMENT position first routes through the
    /// analyzer-macro hot mirror (`macro_type_arg_hot_ref`). Framework
    /// script-fact macros occupy a disjoint inventory and fall through to
    /// their retained-AST locator provider (Svelte today). The hot-mirror arm
    /// is mode-split like the per-FIELD arm:
    /// a terminal-demand caller (`Expanded` / `Identity`) completes
    /// carrier-head resolution through the one dispatch (the empty-path
    /// `ProjectPath` re-entry — [`ProjectSemanticDispatch::resolve_hot_handle_with_context`]),
    /// so a demanded published payload source resolves exactly as the
    /// expansion sink resolved the node it published against; a
    /// carrier/shell caller (`Navigate` / `Shallow` / `Skeleton`) keeps the
    /// mode-neutral mirror handle (the published shallow form — bare alias
    /// names survive). A per-FIELD macro payload is
    /// mode-split: a terminal-demand caller (`Expanded` / `Identity`)
    /// replays the producing route the publication surface enumerated the
    /// member through — the same hot-mirror base plus a member-hop
    /// `ProjectPath` projection through the one dispatch (the
    /// [`Self::replay_session_demand_to_hot`] seam), so generic type
    /// arguments substitute exactly as they did for the published surface
    /// (`defineProps<Props<T>>()` members resolve against the INSTANTIATED
    /// payload, never the raw un-substituted decl body) — while a
    /// carrier/shell caller (`Navigate` / `Shallow` / `Skeleton`) reads the
    /// AUTHORED annotation shape through the memoized locator deref (the
    /// published shallow form — bare alias names survive). Every other
    /// authored position derefs through the first-class
    /// `SemanticQueryKey::LowerLocator` memoized query via
    /// [`ProjectSemanticDispatch::lower_locator`]. A deref-unrouted position
    /// (the object-argument payload, which no producer mints today) is an
    /// honest `None` — never a fabricated body.
    pub(crate) fn raise_authored_locator_to_hot(
        &self,
        locator: &AuthoredBodyLocator,
        context: ProjectionReductionContext,
    ) -> Option<HotTypeRef> {
        if let AuthoredBodyLocator::MacroPayload(payload) = locator {
            match payload.payload {
                MacroPayloadPosition::TypeArgument => {
                    if let Some(handle) = crate::structural_carrier_producer::macro_type_arg_hot_ref(
                        self.ctx,
                        payload.anchor.canonical_id.as_ref(),
                        payload.macro_index as usize,
                    ) {
                        // Terminal-demand mode split (mirroring the per-FIELD
                        // arm below): `Expanded` / `Identity` complete the
                        // carrier-head resolution through the one dispatch.
                        if matches!(
                            context.mode,
                            crate::semantic_query::ProjectionMode::Expanded
                                | crate::semantic_query::ProjectionMode::Identity
                        ) {
                            return Some(HotTypeRef::new(
                                self.resolve_hot_handle_with_context(handle, context),
                            ));
                        }
                        return Some(handle);
                    }
                    // Framework script-fact macros are not analyzer-macro
                    // mirror rows. Fall through to the retained-AST locator
                    // provider below (Svelte today); a genuine miss remains
                    // an honest `None`.
                }
                // Mode split for a per-field payload: a terminal-demand
                // caller (`Expanded` / `Identity`) resolves the INSTANTIATED
                // member through the hot-mirror member-hop replay (generic
                // type arguments substitute exactly as they did for the
                // published surface); a carrier/shell caller (`Navigate` /
                // `Shallow` / `Skeleton`) reads the AUTHORED annotation
                // shape through the memoized locator deref below (the
                // published shallow form — bare alias names survive). A
                // replay miss (no type argument / un-enumerable member)
                // falls through to the authored deref, the honest degraded
                // answer.
                MacroPayloadPosition::Field { field_index } => {
                    if matches!(
                        context.mode,
                        crate::semantic_query::ProjectionMode::Expanded
                            | crate::semantic_query::ProjectionMode::Identity
                    ) {
                        if let Some(hot) =
                            self.raise_macro_field_payload_to_hot(payload, field_index, context)
                        {
                            return Some(hot);
                        }
                    }
                }
                MacroPayloadPosition::ObjectArgument | MacroPayloadPosition::TypeAnnotation => {}
            }
        }
        match self.lower_locator(locator.clone()) {
            QueryResult::Value(node) | QueryResult::Recursive(node) => Some(HotTypeRef::new(node)),
            QueryResult::Error(_) => None,
        }
    }

    /// Raise one per-FIELD macro payload (`MacroPayloadPosition::Field`) by
    /// replaying the publication surface's own producing route: the macro
    /// hot-mirror base (the sole sanctioned type-argument producer) plus a
    /// member-hop `ProjectPath` through the one dispatch. The single hop IS
    /// the terminal hop, so it runs in the caller's mode (shallow-by-default
    /// and path-precision preserved).
    ///
    /// The field ordinal maps to its member NAME through the owner's shallow
    /// script-analysis inventory (parse-domain facts on `IndexedReady` — a
    /// fact lookup, not a resolution). Kind-driven family routing covers the
    /// positions whose Field payload addresses the member VALUE: prop fields
    /// and property-signature emit fields. A slot Field payload addresses
    /// the slot function's RETURN type — a member hop cannot express it, so
    /// it stays an honest `None`; a macro without an authored type argument
    /// (the runtime object form) has no hot-mirror base and also stays
    /// `None`.
    fn raise_macro_field_payload_to_hot(
        &self,
        payload: &verter_type_expr::locators::MacroPayloadLocator,
        field_index: u32,
        context: ProjectionReductionContext,
    ) -> Option<HotTypeRef> {
        use verter_semantic::analysis::AnalyzedMacroKind;
        let canonical = payload.anchor.canonical_id.as_ref();
        let member_name: Arc<str> = {
            let serve = self.ctx.ensure_indexed_ready_serve(canonical)?;
            let snapshot = serve.indexed.script_analysis.as_ref()?;
            let mac = snapshot.macros.get(payload.macro_index as usize)?;
            match mac.kind {
                AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
                    Arc::from(mac.prop_fields.get(field_index as usize)?.name.as_str())
                }
                AnalyzedMacroKind::DefineEmits => {
                    Arc::from(mac.emit_fields.get(field_index as usize)?.name.as_str())
                }
                // Slot Field payloads address the slot function's RETURN
                // type (not the member value); expose fields never stamp a
                // payload; model/options macros have no field vocabulary.
                _ => return None,
            }
        };
        let base = crate::structural_carrier_producer::macro_type_arg_hot_ref(
            self.ctx,
            canonical,
            payload.macro_index as usize,
        )?;
        let path: Arc<[PathSegment]> = std::iter::once(PathSegment::Member(member_name)).collect();
        let read = self.execute_read(SemanticQueryKey::ProjectPath {
            base: base.node(),
            path,
            context,
        });
        crate::meta_resolve::emit_dispatch_dep_signature_facts(self.ctx, &read.dep_signature);
        match read.value {
            QueryResult::Value(node) | QueryResult::Recursive(node) => Some(HotTypeRef::new(node)),
            QueryResult::Error(_) => None,
        }
    }

    /// Re-project the one-level Vue macro surface identified by a stamped
    /// type-argument payload. Member-path, callable-parameter, and index-
    /// position sources all replay through this single producer so their
    /// substitution, heritage filtering, and provenance match publication.
    fn replay_vue_macro_type_argument_surface(
        &self,
        payload: &verter_type_expr::locators::MacroPayloadLocator,
    ) -> Option<crate::typeinfo::framework_surface::vue_exec::VueMacroSurface> {
        if payload.payload != MacroPayloadPosition::TypeArgument {
            return None;
        }
        let canonical = payload.anchor.canonical_id.as_ref();
        let macro_kind = {
            let serve = self.ctx.ensure_indexed_ready_serve(canonical)?;
            serve
                .indexed
                .snapshot
                .macros
                .get(payload.macro_index as usize)?
                .kind
        };
        self.ctx
            .host_for_fact_tracer_install()
            .resolve_vue_macro_surface_with_ctx(
                self.ctx,
                &crate::typeinfo::types::VueMacroSurfaceRequest {
                    owner_canonical: Arc::from(canonical),
                    macro_index: payload.macro_index as usize,
                    macro_kind,
                    root_identity: [0u8; 16],
                    level: crate::typeinfo::types::TypeInfoQueryLevel::FullMetadata,
                },
            )
    }

    /// Raise a projected MEMBER-PATH fact ([`ProjectedTypeFact::MemberPath`])
    /// by replaying the publication surface's own producing route. A stamped
    /// macro type-argument base reuses the SAME one-level Vue macro surface
    /// producer as publication, then selects the first path member from that
    /// surface. This is essential for generic substitution and heritage
    /// filtering: re-walking the raw type argument can retain a non-contributing
    /// open mapped arm or miss a union-alias member that the published surface
    /// already resolved. Non-macro bases lower mode-neutrally through the
    /// authored-locator route. Any remaining path segments project through the
    /// one dispatch's existing `ProjectPath` query under the caller's context.
    /// An unroutable base, missing surface member, or projection miss is an
    /// honest `None` — never a fabricated body.
    fn raise_projected_member_path(
        &self,
        base: &AuthoredBodyLocator,
        path: &Arc<[String]>,
        ctx: &SourceRaiseContext<'_>,
    ) -> Option<HotTypeRef> {
        let locator = absolutize_locator(base, ctx.scope_canonical_id);
        if let AuthoredBodyLocator::MacroPayload(payload) = &locator {
            if payload.payload == MacroPayloadPosition::TypeArgument && !path.is_empty() {
                let surface = self.replay_vue_macro_type_argument_surface(payload)?;
                let member = surface
                    .surface
                    .members
                    .iter()
                    .find(|member| member.name.as_ref() == path[0].as_str())?;
                return self.raise_projected_path_from_node(member.value, &path[1..], ctx);
            }
        }
        // The base stays MODE-NEUTRAL (carrier/shell): the caller's terminal
        // demand applies to the PATH projection below, exactly as in the
        // per-FIELD replay.
        let base_hot = self.raise_authored_locator_to_hot(
            &locator,
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                crate::semantic_query::ProjectionMode::Navigate,
            ),
        )?;
        self.raise_projected_path_from_node(base_hot.node(), path, ctx)
    }

    fn raise_projected_path_from_node(
        &self,
        base: SemanticNodeId,
        path: &[String],
        ctx: &SourceRaiseContext<'_>,
    ) -> Option<HotTypeRef> {
        if path.is_empty() {
            if super::raise::node_is_unknown_materializing_failure(self, base) {
                return None;
            }
            let hot = HotTypeRef::new(base);
            ctx.check_raised_unknown_materializing(self, Some(&hot));
            return Some(hot);
        }
        let segments: Arc<[PathSegment]> = path
            .iter()
            .map(|segment| PathSegment::Member(Arc::from(segment.as_str())))
            .collect();
        let read = self.execute_read(SemanticQueryKey::ProjectPath {
            base,
            path: segments,
            context: ctx.context,
        });
        crate::meta_resolve::emit_dispatch_dep_signature_facts(self.ctx, &read.dep_signature);
        match read.value {
            // FAIL-CLOSED: a projection can "succeed" onto an interned
            // failure carrier (the walker's `Opaque(Miss)` for an undeclared
            // member). That is a projection miss, not a resolved member —
            // never a node whose publication silently reads `Unknown`.
            QueryResult::Value(node) | QueryResult::Recursive(node)
                if super::raise::node_is_unknown_materializing_failure(self, node) =>
            {
                None
            }
            QueryResult::Value(node) | QueryResult::Recursive(node) => {
                let hot = HotTypeRef::new(node);
                // Strict-path conservative interior fail-close on the
                // projected node (the root check above is root-only).
                ctx.check_raised_unknown_materializing(self, Some(&hot));
                Some(hot)
            }
            QueryResult::Error(_) => None,
        }
    }

    /// Raise a projected CALLABLE-PARAMS fact
    /// ([`ProjectedTypeFact::CallableParams`]) by replaying the publication
    /// surface's own producing route: the BASE macro type argument re-projects
    /// to the SAME one-level macro surface the normalization read
    /// (`resolve_vue_macro_surface_with_ctx` — the one existing surface entry,
    /// so projection context, provenance, and heritage substitution are
    /// IDENTICAL by construction, and its `ProjectPath` queries hit the shared
    /// memo), the call signature at `signature_ordinal` is selected in the
    /// NODE domain (the surface's declaration-order sequence, the exact
    /// pre-expansion order the producer stamped), the callable realizes
    /// through the SAME shared [`CallableNodeView`] policy the emit
    /// normalization used (`published(Navigate)`), and a TRANSIENT tuple node
    /// is synthesized from the realized signature's RAW parameters from
    /// `first_param` on — label / optionality / rest / ORDER preserved, each
    /// element carrying the parameter's own (possibly substituted) value
    /// node, so nesting, composites, imported references, and generic
    /// substitutions ride through shallow-by-default. Every step routes
    /// through the one shared dispatch (`macro_type_arg_hot_ref`,
    /// `ProjectPath`, `ResolveDecl`/`Instantiate` via the structural-fact
    /// demand primitive) — never a second resolver, never an output-local
    /// walker.
    ///
    /// FAIL-CLOSED, never a fabricated tuple: a non-macro / non-type-argument
    /// base, an unresolvable surface, an out-of-bounds `signature_ordinal`, a
    /// non-callable ordinal, or a `first_param` past the parameter list is an
    /// honest `None` (bounds drift never synthesizes an empty tuple); a
    /// payload parameter whose root stays an UNRESOLVED residual reference
    /// carrier (`BareRef` / `ImportType` the shared demand primitive could
    /// not resolve) or resolves to an unknown-materializing failure carrier
    /// fails the raise — on the strict path with its `.tuple[N]` position
    /// recorded. A RESOLVABLE reference stays its SHALLOW carrier in the
    /// element (the consumer re-resolves it on demand — validation never
    /// replaces the published shallow form with the resolved body).
    ///
    /// [`CallableNodeView`]: crate::meta_resolve::callable_view::CallableNodeView
    fn raise_projected_callable_params(
        &self,
        base: &AuthoredBodyLocator,
        signature_ordinal: u32,
        first_param: u32,
        ctx: &SourceRaiseContext<'_>,
    ) -> Option<HotTypeRef> {
        let locator = absolutize_locator(base, ctx.scope_canonical_id);
        // The only producer-minted base position is the macro TYPE-ARGUMENT
        // (the emit normalization replays off the macro's stamped type
        // argument); any other base has no surface-projection route here.
        let AuthoredBodyLocator::MacroPayload(payload) = &locator else {
            return None;
        };
        let surface = self.replay_vue_macro_type_argument_surface(payload)?;
        // Deterministic NODE-domain signature selection: the ordinal indexes
        // the surface's declaration-order call-signature sequence (the exact
        // pre-expansion sequence the producer stamped). Bounds drift is an
        // honest miss.
        let sig = surface
            .surface
            .call_signatures
            .get(signature_ordinal as usize)?;
        // SAME callable-realization policy as the emit normalization
        // (`emits_from_typeinfo_surface`): `published(Navigate)` realizes an
        // aliased / generic callable to its `Function` node.
        let realize_context =
            ProjectionReductionContext::published(crate::semantic_query::ProjectionMode::Navigate);
        let view = crate::meta_resolve::callable_view::CallableNodeView::new(self, sig.node);
        let signature = view.signature(realize_context)?;
        let params = signature.raw_params();
        // `first_param` past the parameter list is bounds drift — fail
        // honest, NEVER clamp into an empty-tuple synthesis. (`==` is the
        // legitimate zero-payload boundary; the producer publishes those as
        // closed tuples, so it does not arise from the emit rows.)
        if (first_param as usize) > params.len() {
            return None;
        }
        let scope = self.raise_scope(ctx);
        let mut elements = Vec::with_capacity(params.len() - first_param as usize);
        for (ordinal, param) in params[first_param as usize..].iter().enumerate() {
            // Root-resolvability validation through the ONE shared
            // structural-fact demand primitive (`ResolveDecl` /
            // `Instantiate` under the same rails as every node-domain
            // reader). The normalized node is used for VALIDATION ONLY —
            // the published element keeps the RAW shallow param node.
            let unresolvable = self
                .demand_validated_structural_node(param.ty, realize_context)
                .is_none();
            if unresolvable {
                // Strict path: record the failed payload element at its
                // `.tuple[N]` position (first failure wins); lenient callers
                // observe the honest `None`.
                ctx.with_interior_step(
                    crate::meta_resolve::InteriorSourceStep::TupleElement {
                        ordinal: ordinal as u32,
                    },
                    || ctx.record_interior_failure(),
                );
                return None;
            }
            elements.push(crate::semantic_query::TupleElement {
                label: param.name.clone(),
                value: param.ty,
                optional: param.optional,
                rest: param.rest,
            });
        }
        let node = self.graph().intern_node_with_scope(
            SemanticNodeData::Tuple {
                elements: Arc::from(elements.into_boxed_slice()),
                readonly: false,
            },
            scope,
        );
        let hot = HotTypeRef::new(node);
        // Strict-path conservative interior fail-close on the synthesized
        // tuple (nested structure can carry interned miss carriers the
        // per-element root validation above does not reach).
        ctx.check_raised_unknown_materializing(self, Some(&hot));
        Some(hot)
    }

    /// Demand-validate a node through the ONE shared structural-fact demand
    /// primitive (`ResolveDecl` / `Instantiate` under the same rails as every
    /// node-domain reader): `Some(normalized)` when the node resolves to
    /// KNOWN structure (a function / object / tuple / array / composite /
    /// resolved reference — any reached shape); `None` on a GENUINE miss —
    /// no live node data, an unknown-materializing resolver failure carrier,
    /// or a composite (`Intersection` / `Union`) with ANY failed contributing
    /// arm. A stable residual `BareRef` / `ImportType` is semantic content,
    /// not a projection miss: it remains an explicit shallow carrier and the
    /// demand remains `Complete`. VALIDATION ONLY: the normalized node
    /// classifies; it never replaces a published shallow form. Shared by the
    /// callable-params payload-parameter validation and the structural
    /// member-source projection.
    ///
    /// The composite arm rule is the merged-contributor fail-close: an arm
    /// containing an operational projection miss must NOT validate on the
    /// strength of its resolvable siblings alone. Stable unresolved references
    /// are not misses and remain valid arms. The normalized graph is inspected
    /// with an explicit worklist and a visited set — structural nesting never
    /// consumes the host stack, and shared DAG arms are checked once.
    pub(crate) fn demand_validated_structural_node(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<SemanticNodeId> {
        // A PARTIAL demand fails the validation BEFORE any shape match or
        // composite recursion: a truncated resolution's reached node is an
        // intermediate carrier, and validating it would publish a confident
        // classification off an operationally-incomplete read (fail-open).
        let normalized = self
            .normalize_node_for_structural_fact_demand(node, context)
            .into_complete_node()?;
        let mut pending = vec![normalized];
        let mut visited = FxHashSet::default();
        while let Some(current) = pending.pop() {
            if !visited.insert(current) {
                continue;
            }
            let data = super::node_data_for(self.ctx, current)?;
            match data.as_ref() {
                // Stable residual reference carriers are COMPLETE semantic
                // results. Root normalization already distinguished an
                // operational partial before this validation walk starts.
                SemanticNodeData::BareRef(_) | SemanticNodeData::ImportType(_) => {}
                // Root normalization projected every composite child through
                // the shared heap worklist. Validation therefore inspects the
                // resulting graph directly; it never recursively re-demands
                // each suffix (which would be both stackful and quadratic).
                SemanticNodeData::Intersection(arms) | SemanticNodeData::Union(arms) => {
                    pending.extend(arms.iter().copied());
                }
                _ if super::raise::node_is_unknown_materializing_failure(self, current) => {
                    return None;
                }
                _ => {}
            }
        }
        Some(normalized)
    }

    /// Raise a projected INDEX-POSITION fact
    /// ([`ProjectedTypeFact::IndexPosition`]) by replaying the publication
    /// surface's own producing route: the BASE macro type argument re-projects
    /// to the SAME one-level macro surface the normalization read
    /// (`resolve_vue_macro_surface_with_ctx` — the one existing surface entry,
    /// so projection context, provenance, and heritage substitution are
    /// IDENTICAL by construction), the index signature at `signature_ordinal`
    /// is selected in the NODE domain (the surface's declaration-order
    /// index-signature sequence — the exact sequence the normalizer
    /// enumerated), and the selected KEY or VALUE node is handed back as the
    /// transient handle — the position's own (possibly substituted) node, so
    /// nesting, composites, and references ride through shallow-by-default.
    /// Every step routes through the one shared dispatch — never a second
    /// resolver, never an output-local walker.
    ///
    /// FAIL-CLOSED, never a fabricated body: a non-macro / non-type-argument
    /// base, an unresolvable surface, or an out-of-bounds `signature_ordinal`
    /// is an honest `None` (bounds drift never fabricates a position); a
    /// position node that resolves to an unknown-materializing failure
    /// carrier fails the raise — on the strict path with the conservative
    /// interior fail-close recorded.
    fn raise_projected_index_position(
        &self,
        base: &AuthoredBodyLocator,
        signature_ordinal: u32,
        position: verter_type_expr::facts::IndexSignaturePosition,
        ctx: &SourceRaiseContext<'_>,
    ) -> Option<HotTypeRef> {
        let locator = absolutize_locator(base, ctx.scope_canonical_id);
        // The only producer-minted base position is the macro TYPE-ARGUMENT
        // (the index normalization replays off the macro's stamped type
        // argument); any other base has no surface-projection route here.
        let AuthoredBodyLocator::MacroPayload(payload) = &locator else {
            return None;
        };
        let surface = self.replay_vue_macro_type_argument_surface(payload)?;
        // Deterministic NODE-domain signature selection: the ordinal indexes
        // the surface's declaration-order index-signature sequence. Bounds
        // drift is an honest miss.
        let sig = surface
            .surface
            .index_signatures
            .get(signature_ordinal as usize)?;
        let node = match position {
            verter_type_expr::facts::IndexSignaturePosition::Key => sig.key_type,
            verter_type_expr::facts::IndexSignaturePosition::Value => sig.value_type,
        };
        // FAIL-CLOSED: a position node interned as an unknown-materializing
        // failure carrier is a miss, never a body whose publication silently
        // reads `Unknown`.
        if super::raise::node_is_unknown_materializing_failure(self, node) {
            return None;
        }
        let hot = HotTypeRef::new(node);
        // Strict-path conservative interior fail-close on the selected
        // position node (nested structure can carry interned miss carriers
        // the root check above does not reach).
        ctx.check_raised_unknown_materializing(self, Some(&hot));
        Some(hot)
    }

    /// Raise a first-class synthetic slot-binding source
    /// ([`SemanticTypeSource::SyntheticSlotBinding`]) to a transient graph
    /// handle. Mode-split, mirroring the per-FIELD macro payload split above:
    ///
    /// - a carrier/shell caller (`Navigate` / `Shallow` / `Skeleton`) interns
    ///   the terminal [`SemanticNodeData::SyntheticBinding`] carrier — the
    ///   published shallow form (shallow-by-default; the reducer keeps the
    ///   carrier terminal, and the walker refuses to path-navigate it);
    /// - a terminal-demand caller (`Expanded` / `Identity`) deepens through
    ///   the ONE sanctioned synthetic explicit-deepen route
    ///   ([`Self::deepen_synthetic_binding_to_hot`]). A deepen that cannot
    ///   complete (stale seed, evicted node, unresolvable value) falls back
    ///   to the shallow carrier — the honest degraded answer, never a
    ///   fabricated shape.
    fn raise_synthetic_binding_source_to_hot(
        &self,
        key: &Arc<verter_type_expr::SyntheticCarrierKey>,
        ctx: &SourceRaiseContext<'_>,
    ) -> HotTypeRef {
        use crate::semantic_query::ProjectionMode;
        if matches!(
            ctx.context.mode,
            ProjectionMode::Expanded | ProjectionMode::Identity
        ) {
            if let Some(hot) = self.deepen_synthetic_binding_to_hot(key, ctx.context) {
                // Strict-path conservative interior fail-close on the
                // deepened value (a deref'd graph body, not a composed
                // shell).
                ctx.check_raised_unknown_materializing(self, Some(&hot));
                return hot;
            }
        }
        let scope = self.raise_scope(ctx);
        let node = self.graph().intern_node_with_scope(
            SemanticNodeData::SyntheticBinding {
                id: crate::semantic_query::SyntheticBindingId::from_carrier_key(key),
                value_node: key.value_node,
            },
            scope,
        );
        HotTypeRef::new(node)
    }

    /// Terminal-demand explicit deepen for a synthetic slot-binding carrier —
    /// the production consumer of the content-free synthetic-binding cache
    /// route: the host-owned `ShapeCacheDb` slot keyed by
    /// [`crate::component_meta_caches::ShapeCacheKey::synthetic_binding_whole_with_context`]
    /// (identity = the content-free [`crate::semantic_query::SyntheticBindingId`]
    /// projection; the carrier's `value_node` ordinal NEVER enters the key).
    ///
    /// Peek first; a cold miss reduces from the SAME-GENERATION `value_node`
    /// seed — the precise per-binding node the slot-binding graph walk
    /// produced — under the caller's terminal context, through the one
    /// dispatch reducer. The same-generation gate is content-precise: the
    /// seed node must still exist AND its minting file scope's `whole_hash`
    /// must equal that file's LIVE whole hash; a stale seed returns `None`
    /// (the caller falls back to the shallow carrier — fail closed, no cache
    /// write, never a fabricated shape). A genuine-partial reduction is
    /// returned to the caller but REFUSED shared-cache admission (no-poison).
    fn deepen_synthetic_binding_to_hot(
        &self,
        key: &verter_type_expr::SyntheticCarrierKey,
        context: ProjectionReductionContext,
    ) -> Option<HotTypeRef> {
        // ONE cacheability tracer scope around the WHOLE deepen — the peek, the
        // same-generation seed gate, and the cold reduce. The reduce resolves the
        // seed's carrier head through the shared resolver's
        // `ensure_indexed_ready_serve`, so a FENCED (ReturnOnly, `store_published ==
        // false`) serve derives the deepened shape from a served-without-publication
        // basis while the entry's fact signature validates against the LIVE view. A
        // fenced serve is non-cacheable but NOT partial, so the
        // `result_is_partial()`-only gate below cannot reject it; the scope's
        // CACHEABILITY verdict (which also folds a fact-signature overflow) is the
        // rail the `ShapeCacheDb` admission funnel consults.
        let cache = self.ctx.project_type_store().shape_cache_db();
        let value = cache.with_owner_scope(self.ctx, |scope| {
                let id = crate::semantic_query::SyntheticBindingId::from_carrier_key(key);
                let cache_key =
            crate::component_meta_caches::ShapeCacheKey::synthetic_binding_whole_with_context(
                id, context,
            );
                if let Some(cached) = scope.peek(&cache_key) {
                    crate::meta_resolve::emit_dispatch_dep_signature_facts(
                        self.ctx,
                        cached.dep_signature(),
                    );
                    // A warm entry serves its reduced node. An entry without a node
                    // cannot serve a raise; fall through to the cold seed reduce.
                    if let Some(node) = cached.node_id() {
                        return Some(HotTypeRef::new(node));
                    }
                }
                // SAME-GENERATION seed gate: the `value_node` arena ordinal is only
                // meaningful while the graph still holds the node AND the node's
                // minting file scope content is that file's LIVE content. Anything
                // else fails closed to the shallow carrier.
                //
                // The deepen's CACHE identity is the content-free
                // `SyntheticBindingId` sealed into the `ShapeCacheKey` above — the
                // ordinal here is ONLY the cold-compute reduction SUBJECT, never any
                // key: it re-attaches the carrier's value-side provenance to the
                // live graph (the same provenance channel the raise boundary uses
                // via `to_carrier_key`), and the content gate below refuses a stale
                // re-attachment.
                let seed_ordinal: u64 = key.value_node;
                let seed = SemanticNodeId(seed_ordinal);
                self.graph().node_data(seed)?;
                let NodeScopeId::File {
                    canonical_id,
                    whole_hash,
                    ..
                } = self.graph().node_scope(seed)?
                else {
                    return None;
                };
                if self.ctx.get_whole_hash(canonical_id.as_ref())? != whole_hash {
                    return None;
                }
                // Cold reduce from the seed under the caller's terminal context —
                // the one dispatch reducer, no second walker.
                let reduced = self.raise_and_reduce_with_context(seed, context);
                crate::meta_resolve::emit_dispatch_dep_signature_facts(
                    self.ctx,
                    reduced.dep_signature(),
                );
                let node = reduced.node_id()?;
                // Genuine-partial results never warm the shared slot (no-poison);
                // the reduced value still answers this caller.
                if crate::cache_runtime::refuse_result_cache_admission_if_partial(
                    reduced.result_is_partial(),
                ) {
                    return Some(HotTypeRef::new(node));
                }
                let observed_scope = self
                    .ctx
                    .observe_materialize_scope(key.scope_canonical_id.as_ref());
                let reduced_for_closure = reduced.clone();
                let _ = scope.get_or_compute(&cache_key, move || {
            let scope_obs = observed_scope?;
            let parse_fact = scope_obs.syntactic_export_set.clone()?;
            match crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo(
                &scope_obs,
                parse_fact,
                reduced_for_closure.dep_signature(),
            ) {
                crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
                    Some((reduced_for_closure, sig.facts))
                }
                crate::cache_runtime::SignatureAdmission::NonCacheable(_) => None,
            }
        });
                Some(HotTypeRef::new(node))
            });
        value
    }

    /// Replay a session-raised demand to a transient graph handle: re-run the
    /// SAME session graph route the producer used (the macro hot mirror plus a
    /// member-hop `ProjectPath` projection through the one dispatch), memoized
    /// by the existing session graph memo under env + read-set validation.
    ///
    /// This is the ONE sanctioned replay seam for a consumer holding a
    /// [`SessionDemandIdentity`] that must re-raise the identified value
    /// (exercised by the in-crate `semantic_source_tests`). It is a REPLAY
    /// of the producing route, never a second resolver. Slot-binding
    /// DEEPENING does not come through here: a consumer deepening a
    /// session-raised binding routes through the content-free
    /// synthetic-binding identity (`ShapeCacheKey::synthetic_binding_whole`
    /// through `ShapeCacheDb`) — one deepening route, one replay seam.
    ///
    /// The demand's `surface_anchor` carries the producing macro ordinal as a
    /// decimal string — minted by the same session code that builds the
    /// demand, so both sides share one addressing convention. An anchor that
    /// does not parse, an out-of-range ordinal, or a projection miss is an
    /// honest `None`.
    #[cfg(test)]
    pub(crate) fn replay_session_demand_to_hot(
        &self,
        demand: &SessionDemandIdentity,
        context: ProjectionReductionContext,
    ) -> Option<HotTypeRef> {
        let macro_index: usize = demand.owner.surface_anchor.parse().ok()?;
        let base = crate::structural_carrier_producer::macro_type_arg_hot_ref(
            self.ctx,
            demand.owner.canonical.as_ref(),
            macro_index,
        )?;
        match demand.route {
            // The hot-mirror route IS the base handle: the demand names the
            // macro payload itself.
            SessionDemandRoute::MacroHotMirror if demand.member_role_path.is_empty() => Some(base),
            // A member-role path projects off the base through the one
            // dispatch — both routes converge on the same `ProjectPath`
            // projection when a path is present.
            SessionDemandRoute::MacroHotMirror | SessionDemandRoute::ProjectPath => {
                let path: Arc<[PathSegment]> = demand
                    .member_role_path
                    .iter()
                    .map(|segment| PathSegment::Member(Arc::from(segment.as_str())))
                    .collect();
                if path.is_empty() {
                    return Some(base);
                }
                let read = self.execute_read(SemanticQueryKey::ProjectPath {
                    base: base.node(),
                    path,
                    context,
                });
                crate::meta_resolve::emit_dispatch_dep_signature_facts(
                    self.ctx,
                    &read.dep_signature,
                );
                match read.value {
                    QueryResult::Value(node) | QueryResult::Recursive(node) => {
                        Some(HotTypeRef::new(node))
                    }
                    QueryResult::Error(_) => None,
                }
            }
        }
    }
}

/// Absolutize a producer-local (empty-anchored) locator against the consuming
/// scope canonical: the analyzer's local-file convention stamps
/// `canonical_id: ""`; every deref requires the producing canonical.
pub(in crate::project_semantic_dispatch) fn absolutize_locator(
    locator: &AuthoredBodyLocator,
    scope_canonical_id: &str,
) -> AuthoredBodyLocator {
    fn absolutize_anchor(anchor: &AuthoredAnchor, scope_canonical_id: &str) -> AuthoredAnchor {
        if anchor.canonical_id.is_empty() {
            AuthoredAnchor {
                canonical_id: Arc::from(scope_canonical_id),
                owner: anchor.owner,
                symbol: Arc::clone(&anchor.symbol),
                space: anchor.space,
            }
        } else {
            anchor.clone()
        }
    }
    match locator {
        AuthoredBodyLocator::DeclBody(slot) => {
            if slot.anchor.canonical_id.is_empty() {
                AuthoredBodyLocator::DeclBody(verter_type_expr::locators::TypeBodySlot {
                    anchor: absolutize_anchor(&slot.anchor, scope_canonical_id),
                    path: Arc::clone(&slot.path),
                })
            } else {
                locator.clone()
            }
        }
        AuthoredBodyLocator::AugmentationBody(aug) => {
            if aug.anchor.canonical_id.is_empty() {
                AuthoredBodyLocator::AugmentationBody(
                    verter_type_expr::locators::AugmentationBodyLocator {
                        anchor: absolutize_anchor(&aug.anchor, scope_canonical_id),
                        scope: aug.scope.clone(),
                        path: Arc::clone(&aug.path),
                    },
                )
            } else {
                locator.clone()
            }
        }
        AuthoredBodyLocator::JsdocTypedefBody(typedef) => {
            if typedef.anchor.canonical_id.is_empty() {
                AuthoredBodyLocator::JsdocTypedefBody(
                    verter_type_expr::locators::JsdocTypedefBodyLocator {
                        anchor: absolutize_anchor(&typedef.anchor, scope_canonical_id),
                        path: Arc::clone(&typedef.path),
                    },
                )
            } else {
                locator.clone()
            }
        }
        AuthoredBodyLocator::MacroPayload(payload) => {
            if payload.anchor.canonical_id.is_empty() {
                AuthoredBodyLocator::MacroPayload(verter_type_expr::locators::MacroPayloadLocator {
                    anchor: absolutize_anchor(&payload.anchor, scope_canonical_id),
                    macro_index: payload.macro_index,
                    payload: payload.payload,
                })
            } else {
                locator.clone()
            }
        }
    }
}

/// Test-only demand probe: materialize a published [`SemanticTypeSource`]
/// through the ONE shared dispatch — raise the source to a hot handle via
/// [`ProjectSemanticDispatch::raise_semantic_type_source_to_hot`], reduce the
/// raised node under the consumer OBSERVATION demand (the
/// `Published(Expanded)` walk with the observation carrier fence armed), and
/// read the sealed output carrier through its test accessor.
///
/// Publication itself stays shallow-by-default; this probe is the explicit
/// consumer OBSERVATION of a published source that test assertions perform to
/// observe the type a consumer demanding the source resolves. The observation
/// executes the OUTER carrier — an authored generic-alias body
/// (`InstantiationRef`) runs the existing `Instantiate` query, so outer type
/// arguments substitute — and expands the resulting surface, EXCEPT that
/// interior OWNER-local helper references and PACKAGE-backed references stay
/// SHALLOW `Ref` carriers the consumer re-resolves on demand (the owner's own
/// registry / the package registry). Workspace-imported interior branches
/// still expand — they have no registry entry of their own to re-resolve
/// against. It introduces no second engine: raise, reduction, and
/// materialisation all route through the existing dispatch entries.
///
/// `None` = the source has no live graph representation under the current
/// view (unknown file, memo deref miss, unrouted payload position).
#[cfg(any(test, feature = "test-support"))]
pub fn demand_semantic_source_type_expr(
    host: &crate::VerterHost,
    owner_canonical: &str,
    source: &SemanticTypeSource,
) -> Option<verter_type_expr::TypeExpr> {
    demand_semantic_source_type_expr_with_ctx(host, owner_canonical, source)
}

/// Context-generic body of [`demand_semantic_source_type_expr`]: the same
/// demand walk bound to an arbitrary [`ResolverContext`] — a bare host for
/// base-view assertions, a session-bound wrapper for overlay-view
/// assertions. One probe body, two view bindings; never a second engine.
///
/// [`ResolverContext`]: crate::resolver_core::resolver_context::ResolverContext
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn demand_semantic_source_type_expr_with_ctx(
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    owner_canonical: &str,
    source: &SemanticTypeSource,
) -> Option<verter_type_expr::TypeExpr> {
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let context =
        ProjectionReductionContext::published(crate::semantic_query::ProjectionMode::Expanded);
    // A CLOSED fact is DATA — its raise composes carriers verbatim, and the
    // observation reduce below drives resolution (carrier-preserving on a
    // resolution miss, per shallow-by-default). Raising a closed leaf under
    // the demand context would EAGERLY resolve the published name at
    // lowering time and mint a miss for an intentionally-shallow
    // unresolvable name. Authored / projected / synthesized sources keep the
    // demand-context raise (the macro payload arms mode-split on it — the
    // instantiated member-hop replay is a terminal-demand behaviour).
    let raise_context = match source {
        SemanticTypeSource::Closed(_) => ProjectionReductionContext::structural_transit_with_mode(
            crate::semantic_query::ProjectionMode::Navigate,
        ),
        _ => context,
    };
    let hot = dispatch.raise_semantic_type_source_to_hot(
        source,
        SourceRaiseContext {
            scope_canonical_id: owner_canonical,
            scope_owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            context: raise_context,
            interior_failures: None,
        },
    )?;
    let carrier = dispatch.raise_and_reduce_observation(hot.node(), context, owner_canonical);
    Some(carrier.type_expr_for_test().clone())
}

/// Shallow sibling of [`demand_semantic_source_type_expr`]: raise the source
/// and shell-materialize WITHOUT any reduction demand, so a shallow published
/// carrier keeps its published shallow shape (`Ref` / utility carriers
/// survive). Used by tests that assert the published surface\'s
/// shallow-by-default form.
#[cfg(any(test, feature = "test-support"))]
pub fn shallow_semantic_source_type_expr(
    host: &crate::VerterHost,
    owner_canonical: &str,
    source: &SemanticTypeSource,
) -> Option<verter_type_expr::TypeExpr> {
    // A CLOSED leaf / leaf-union fact IS the published shallow shape — render
    // it verbatim. Routing it through raise + shell would re-RESOLVE a bare
    // published name in the owner scope (a package re-export alias would
    // render its terminal internal declaration name instead of the published
    // one), which is a demand, not the published shallow form.
    match source {
        SemanticTypeSource::Closed(verter_type_expr::facts::ClosedTypeFact::Leaf(leaf)) => {
            return Some(super::lower::leaf_type_fact_expr(leaf));
        }
        SemanticTypeSource::Closed(verter_type_expr::facts::ClosedTypeFact::LeafUnion(leaves)) => {
            return Some(verter_type_expr::TypeExpr::Union(
                leaves
                    .iter()
                    .map(super::lower::leaf_type_fact_expr)
                    .collect(),
            ));
        }
        _ => {}
    }
    let dispatch = ProjectSemanticDispatch::new(host);
    let context = ProjectionReductionContext::structural_transit_with_mode(
        crate::semantic_query::ProjectionMode::Navigate,
    );
    let hot = dispatch.raise_semantic_type_source_to_hot(
        source,
        SourceRaiseContext {
            scope_canonical_id: owner_canonical,
            scope_owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            context,
            interior_failures: None,
        },
    )?;
    let sealed = dispatch.output_shell_raise_sealed(hot.node())?;
    let carrier = super::output_materialization::MaterializedOutputTypeExpr::from_parts(
        Some(hot.node()),
        sealed,
        Arc::from(Vec::new().into_boxed_slice()),
        false,
    );
    Some(carrier.type_expr_for_test().clone())
}

#[cfg(test)]
#[path = "semantic_source_tests.rs"]
mod semantic_source_tests;
