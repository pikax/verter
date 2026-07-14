//! Per-macro projectors for component-meta extraction.
//!
//! Each projector resolves a single macro's surface members through
//! the shared dispatch primitives (`SemanticQueryKey::ResolveMacroPayload`
//! followed by an empty-path `ProjectPath` in `Shallow` mode), raises
//! members to `TypeExpr`, and produces `Vec<ExpandedField>` (or the
//! macro-specific equivalent).
//!
//! Authority chain:
//!
//! 1. `crate::structural_carrier_producer::macro_type_arg_hot_ref(ctx, file, macro_index)`
//!    reads the macro arg's mode-neutral hot-mirror handle (the ONE producer)
//!    so the dispatch can resolve the macro payload from the structural
//!    carrier.
//! 2. `dispatch.execute_read(SemanticQueryKey::ResolveMacroPayload { .. })`
//!    yields the macro payload's semantic node (the resolved type that
//!    backs the macro instance).
//! 3. `dispatch.execute_read(SemanticQueryKey::ProjectPath { base, path: [], context: crate::semantic_query::ProjectionReductionContext::published(Shallow)})`
//!    enumerates the payload's surface members.
//! 4. For each surface member, the projector raises the member's value
//!    node back to `TypeExpr` and classifies its exactness via
//!    `meta_resolve::exactness::classify_node`.
//!
//! All `dispatch.execute_read` calls must dual-emit their
//! dep-signature into BOTH downstream channels via
//! `emit_dispatch_dep_signature_facts(dispatch.ctx, &read.dep_signature)`:
//! (1) the legacy `DISPATCH_DEP_SIGNATURE_ACCUMULATOR` drained at
//! `compute_component_meta_state_inner` into `state.fact_versions`,
//! and (2) the `ACTIVE_TRACERS` stack captured by the outer
//! `with_fact_tracer` scope. Dual-emit is the migration substrate
//! that lets the `fact_dep_signature` producer source later flip
//! from `state.fact_versions` to `read_set.finalise()` without
//! losing coverage. The final-result cache validates warm hits
//! against `fact_dep_signature` on both branches. Cycle and error
//! branches publish a `MacroExpansionDiagnostics` envelope into
//! `diag_sink` (per §7.5 silent-miss prevention).
//!
//! The macro_index inside each projector identifies which macro this
//! projection corresponds to, for diagnostic correlation and for the
//! shape-merge logic in the parser-side analysis.

use std::sync::Arc;

use verter_semantic::analysis::component_meta::{MacroExpansionDiagnostics, MacroExpansionKind};
use verter_semantic::analysis::type_expand::{
    ExpansionDiagnostic, ExpansionExactness, ExpansionExecutionStatus, ExpansionStopReason,
};
use verter_semantic::analysis::{AnalyzedMacro, AnalyzedMacroKind};
use verter_type_expr::TypeExpr;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{
    DeclIdentity, PathSegment, ProjectionMode, QueryResult, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey, SurfaceMember,
};
use crate::types::FileAnalysisSnapshot;

use super::dep_signature::emit_dispatch_dep_signature_facts;
use super::diagnostic_convert::shallow_diagnostics_to_macro_expansion;

pub(crate) mod define_shapes;
pub(crate) mod emits;
pub(crate) mod exposed;
pub(crate) mod macro_payload_substrate;
pub(crate) mod options;
pub(crate) mod output_sink;
pub(crate) mod props;
// The publication-authority admitted-token chain. A SIBLING module (NOT
// nested under another projector module) so its private token fields + private
// `Seal` are unreachable from `props.rs` / `emits.rs` / …: a child module can
// read ancestor-private items, but a sibling's privates stay sealed. The ONLY
// way to mint a token is its admission functions, and the terminal sink
// (`output_sink::surface_member_to_expanded_field`) consumes the admitted
// token instead of a forgeable `(&SurfaceMember, ProjectionCursor)` pair.
pub(crate) mod publication_authority;
pub(crate) mod published_reducer;
// Published-SOURCE upgrades for reduced publication nodes (leaf / leaf-union
// / member ref-identity) — pure node-domain projections consumed by the
// terminal sink.
mod published_source;
// The demand-validated structural member-source projection — shared with the
// vue_exec DTO normalizers (the normalized prop/expose rows publish the same
// closed/ref/member-path ladder the member sink publishes).
pub(crate) use published_source::structural_member_value_source;
pub(crate) mod slots;

// The projectors' reverse-materialization capability is DEFINED in the terminal
// `output_sink` sink module (the only module that can MINT it). It is
// re-exported here so the owner `project_semantic_dispatch::projector`
// `OutputProjector` impl and the per-kind projector children can NAME the cap
// type at the stable `crate::meta_resolve::projectors::MetaResolveProjectorsOutputCap`
// path; the inherent `new` constructor stays scoped to `output_sink`, so naming
// the type here does NOT grant the ability to mint it (a non-sink helper that
// names the cap and calls `new` is `E0624`).
pub(crate) use output_sink::MetaResolveProjectorsOutputCap;

// Substrate re-exports for the transit-shallow macro publication
// pipeline (see [`macro_payload_substrate`]). The `unused_imports`
// allowance parallels the `dead_code` allowance on each primitive at
// the definition site: not every macro kind currently consumes every
// substrate primitive, but the re-exports keep the boundary stable as
// new consumers wire in.
#[cfg(test)]
pub(crate) use macro_payload_substrate::EMIT_CARRIER_WALK_FUSE;
#[allow(unused_imports)]
pub(crate) use macro_payload_substrate::{
    resolve_emit_payload_to_conditional_root, resolve_macro_payload_diagnostic_probe,
    resolve_payload_surface_with_scope, MemberValueRole, PayloadSurfaceScope,
};

pub(crate) use define_shapes::project_define_macro_shapes;
pub(crate) use emits::project_emits;
pub(crate) use exposed::project_exposed;
pub(crate) use options::project_options;
pub(crate) use props::project_props;
pub(crate) use published_reducer::classify_node_reduction_gates;
pub(crate) use slots::project_slots;

// The boundary-consuming publication functions live in the terminal
// `output_sink` sink module (the only module that touches the reverse
// boundary — `MetaResolveProjectorsOutputCap` mint + carrier unwrap).
// `output_sink` exposes ONLY policy-complete publication operations (returning
// `ExpandedField` / mutating the published surface), never a bare `TypeExpr`
// or a raw boundary helper; the per-field `reduce_field_type_expr_with_mode`
// reducer stays sink-private (callers use `reduce_published_field_types`).
//
// `project_model` is re-exported for the local `project_evaluated_types`
// driver; `reduce_published_field_types` is re-exported at the stable
// `crate::meta_resolve::projectors::*` path for the external host-manage
// caller. The per-member publication API
// (`output_sink::surface_member_to_expanded_field`) is NOT re-exported here —
// it now consumes a policy-admitted `publication_authority` token, and the
// per-kind projector children name it through `super::output_sink::` directly
// so the admitted-token discipline reads at the call site.
pub(crate) use output_sink::{project_model, reduce_published_field_types};

// The output-ENVELOPE builder: the terminal sink materializes the
// component-meta output type lanes and seals them (with the analysis and the
// optional narrowed resolution sidecar) into the request-local
// `crate::meta_resolve::ComponentMetaOutput` the wire converter consumes.
// Re-exported at the stable `crate::meta_resolve::projectors::*` path for the
// view-fenced host entry; envelope construction itself stays inside the sink
// (the envelope constructors require the sink-mintable capability).
pub(crate) use output_sink::build_component_meta_output;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use output_sink::OUTPUT_MATERIALIZE_FORCE_FAIL_FOR;
#[cfg(test)]
pub(crate) use output_sink::{
    LAST_OUTPUT_MATERIALIZE_CALLS, LAST_OUTPUT_MEMO_HASH_OPS, OUTPUT_MATERIALIZE_FORCE_FAIL,
};

/// Merge a projector's `Vec<ExpandedField>` output into the target
/// `Vec<ExpandedField>` on `evaluated_types`.
///
/// Per the project's component-meta shallow-by-default rule (see
/// `CLAUDE.md`), the projector pipeline is the **sole post-projection
/// authority** for finalising published field shapes. Parser-side
/// pre-population of `evaluated_types.props` (from
/// `verter_semantic::analysis::type_eval_build::expand_field_expr`
/// and friends) eagerly inlines alias bodies, which violates the
/// shallow-by-default contract for bare `Ref` references —
/// `defineProps<{ user: Foo }>` where `type Foo = string` lives in
/// the same file pre-populates `user`'s type as `Primitive(String)`,
/// not the bare `Ref { name: "Foo" }` that the rule mandates.
///
/// To honour the shallow-by-default invariant, the projector's
/// output always wins for fields the projector produced: when both
/// `target` and `projected` carry an entry with the same name, the
/// projected entry replaces the existing entry wholesale. Parser-
/// side fields the projector did NOT produce (entries that have no
/// name match in `projected`) are preserved as-is so prop annotations
/// the dispatch path didn't surface still appear in the published
/// analysis.
fn merge_projected_fields_by_name(
    target: &mut Vec<verter_semantic::analysis::type_expand::ExpandedField>,
    projected: Vec<verter_semantic::analysis::type_expand::ExpandedField>,
) {
    for field in projected {
        if let Some(existing) = target.iter_mut().find(|t| t.name == field.name) {
            if std::env::var("VERTER_PROJECTOR_MERGE_TRACE").is_ok() {
                eprintln!(
                    "[MERGE] name={} existing={:?} projected={:?}",
                    field.name, existing.r#type, field.r#type
                );
            }
            // Projector pipeline is the sole post-projection authority
            // — its output always replaces parser-side pre-population.
            *existing = field;
        } else {
            target.push(field);
        }
    }
}

/// Top-level driver that dispatches every type-based macro in the
/// snapshot through its per-kind projector and writes the resulting
/// fields into `evaluated_types`.
///
/// The flat fields this driver populates are a METADATA + display surface
/// (exactness / execution status / diagnostics / raw display types for the
/// `evaluate_types` payload) — NEVER a published-source authority: every
/// published `SourcePosition` is owned by the NORMALIZED macro rows
/// (`ResolvedPropField` / `ResolvedEmitField` / `ResolvedExposeField` and
/// the `define_*` shape lanes built from them). The driver:
///
/// 1. For each `defineProps<T>`, calls [`project_props`] and extends
///    `evaluated_types.props` with the resulting fields.
/// 2. For each `defineEmits<T>`, calls [`project_emits`] and extends
///    `evaluated_types.emits`.
/// 3. For each `defineSlots<T>`, calls [`project_slots`]; the slot
///    fields are not directly published into `evaluated_types`
///    because the slot-shape level is consumed by
///    [`crate::meta_resolve::slot_binding_graph::resolve_slot_bindings_graph_native`]
///    via the same dispatch primitives. The diagnostic sink is the
///    only side-channel the projector contributes for slots.
/// 4. `defineModel`, `defineExpose`, `defineOptions` macros run their
///    projectors, but their downstream merge into the analysis lives
///    on the parser side; their projector results are only inspected
///    for diagnostic sink contributions.
///
/// Silent-miss prevention: every `Recursive` / `Error` branch the
/// projectors hit is appended to `diag_sink`, which the caller merges
/// into `analysis.macro_expansion_diagnostics`. A projector must never
/// silently return an empty surface on `QueryResult::Error` — that
/// would be indistinguishable from a successful empty result.
pub(crate) fn project_evaluated_types(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    file: &str,
    snapshot: &FileAnalysisSnapshot,
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) {
    let owner = build_owner_decl_identity(query_engine.ctx, file);

    // Construct a `SurfaceProjection` per macro kind so each
    // projector entry receives a path-precise cursor.
    // `whole_surface(kind)` admits every published member name;
    // narrower cursors are threaded in when consumer demand is known.
    use crate::meta_resolve::projection_demand::{PublishedSurfaceKind, SurfaceProjection};

    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        match mac.kind {
            AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
                let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Props);
                let fields = project_props(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                    projection.cursor(),
                );
                merge_projected_fields_by_name(&mut evaluated_types.props, fields);
            }
            AnalyzedMacroKind::DefineEmits => {
                let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Emits);
                let fields = project_emits(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                    projection.cursor(),
                );
                merge_projected_fields_by_name(&mut evaluated_types.emits, fields);
            }
            AnalyzedMacroKind::DefineSlots => {
                // Slot-shape projection is consumed by the
                // slot-binding-graph synthesis layer which
                // shares the same dispatch primitives; running the
                // projector here populates the diagnostic stream and
                // primes the dispatch family memo.
                let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Slots);
                let _ = project_slots(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                    projection.cursor(),
                );
            }
            AnalyzedMacroKind::DefineModel => {
                // The parser-side analysis owns the
                // `synthesize_model_prop_and_event` that publishes
                // the prop+event pair. Running the projector here
                // populates the diagnostic stream and primes the
                // dispatch family memo so the parser-side merge can
                // observe the resolved type without a second
                // resolution pass.
                let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Model);
                let _ = project_model(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                    projection.cursor(),
                );
            }
            AnalyzedMacroKind::DefineExpose => {
                let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Exposed);
                // The projected surface members carry the exposed members'
                // typed sources (`ExpandedField.r#type`); publish them on
                // the canonical aggregate's per-macro `exposed` lane so the
                // exposed-analysis join pairs them by the stable
                // `(macro_index, member name)` identity. Idempotent per
                // macro: a re-projection replaces the macro's lane entry.
                let fields = project_exposed(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                    projection.cursor(),
                );
                evaluated_types
                    .exposed
                    .retain(|entry| entry.macro_index != macro_index);
                if !fields.is_empty() {
                    evaluated_types.exposed.push(
                        verter_semantic::analysis::type_expand::ExpandedMacroExposed {
                            macro_index,
                            fields,
                        },
                    );
                }
            }
            AnalyzedMacroKind::DefineOptions => {
                let projection = SurfaceProjection::whole_surface(PublishedSurfaceKind::Options);
                let _ = project_options(
                    query_engine,
                    &owner,
                    file,
                    macro_index,
                    mac,
                    snapshot,
                    diag_sink,
                    projection.cursor(),
                );
            }
        }
    }
}

/// Identifier name used as the synthetic decl-name for a `<script setup>`
/// scope when the dispatch's `DeclIdentity` only consults the canonical
/// id + whole hash for cache keying. Mirrors the constant from
/// [`super::slot_binding_graph`] so projector code paths share the
/// same decl identity for `ResolveMacroPayload` cache keys.
pub(crate) const SFC_SCRIPT_SETUP_DECL_NAME: &str = "<sfc-script-setup>";

/// Build the owner [`DeclIdentity`] for an SFC's macro queries.
///
/// Mirrors `slot_binding_graph::build_owner_decl_identity` so the
/// dispatch keys produced by projectors collide with the existing
/// graph-native synthesis layer (path-independent caching per
/// `CLAUDE.md` Build Philosophy).
pub(crate) fn build_owner_decl_identity(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
) -> DeclIdentity {
    let whole_hash = ctx
        .shallow_file_state(owner_canonical)
        .map(|s| s.whole_hash)
        .unwrap_or_default();
    DeclIdentity {
        canonical_id: Arc::from(owner_canonical),
        whole_hash,
        decl_name: Arc::from(SFC_SCRIPT_SETUP_DECL_NAME),
    }
}

/// Convert an `Arc<[PathSegment]>` empty path constant. Cached locally
/// so each projector call reuses a shared-ref-counted empty path.
#[inline]
pub(crate) fn empty_path() -> Arc<[PathSegment]> {
    Arc::from(Vec::<PathSegment>::new().into_boxed_slice())
}

// =============================================================================
// `peek_member_shape_known` — graph-native type-peek primitive.
//
// The peek is the projector pipeline's shallow-by-default enforcement
// substrate: the caller asks "do you already know the shape of this type
// expression at this scope / mode?" and the implementation answers
// WITHOUT triggering any reducer / resolver / route rebuild.
//
//  - `PeekedShape::Leaf` — the expression is a leaf primitive or literal;
//    publishing it is a clone.
//  - `PeekedShape::BareCarrier` — the expression is an unparameterised
//    `Ref` (a plain alias name). Per the shallow-by-default rule, the
//    projector publishes the Ref shallow; consumers re-resolve through
//    the registry on demand. No reduction needed.
//  - `PeekedShape::Cached` — the expression already has an entry in
//    `ShapeCacheDb` keyed on `(scope, expr, mode)`. The peek re-emits
//    the cached entry's `fact_dep_signature` into the active fact tracer
//    (the same `peek` protocol that the per-member slot of `ShapeCacheDb`
//    indexed by `ShapeSubject::MemberValueNode` follows) so the cm-result
//    cache validation invariants are preserved.
//  - `None` — the cache is cold for this triple; the caller must decide
//    whether to reduce (operator-shape / generic instantiation cases) or
//    publish shallow (bare alias case already covered above).
//
// Strictly request-bound: the `debug_assert!` enforces that the caller's
// `ResolverContext` is request-bound. Bare-host invocation would force a
// workspace snapshot rebuild.
// =============================================================================

/// Result of peeking whether a type's shape is known cheaply.
///
/// `Some(_)` ⇒ the caller may publish or short-circuit WITHOUT
/// triggering reduction. `None` ⇒ the cache is cold; the caller must
/// reduce (or publish the Ref shallow per the shallow-by-default rule).
pub(crate) enum PeekedShape {
    /// The expression is a bare alias carrier; publish the Ref
    /// shallow. No reduction needed. `name` is exposed for
    /// `projectors_peek_tests` behavioural-discrimination assertions;
    /// production match arms use `_`.
    BareCarrier {
        #[allow(dead_code)]
        name: Arc<str>,
    },
    /// The expression is a leaf primitive / literal — publish as-is.
    Leaf(verter_type_expr::TypeExpr),
    /// The expression has already been reduced; return the cached
    /// `MaterializedOutputTypeExpr` verbatim. The peek implementation
    /// re-emits the cached entry's `fact_dep_signature` into the
    /// active fact tracer + dispatch dep-signature accumulator via the
    /// `MaterializeMemoDb::peek` protocol (`bubble_fact_signature` in
    /// `component_meta_caches.rs:1346`).
    Cached(crate::project_semantic_dispatch::raise::MaterializedOutputTypeExpr),
}

/// Peek without expansion.
///
/// Does NOT consult `RouteDb`, `OwnerImportSurfaceDb`, or rebuild any
/// workspace view — observes ONLY the request-bound store_view via
/// `ctx.store_view()` (through the cooperative `MaterializeMemoDb::peek`).
///
/// MUST be invoked from a request-bound context; the
/// `debug_assert!(query_engine.ctx.is_request_bound())` enforces this.
/// Reaching this from a bare-host context would force a workspace
/// snapshot rebuild.
pub(crate) fn peek_member_shape_known(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    expr: &TypeExpr,
    mode: ProjectionMode,
) -> Option<PeekedShape> {
    debug_assert!(
        query_engine.ctx.is_request_bound(),
        "peek_member_shape_known invoked from bare-host context — \
         would force a workspace snapshot rebuild"
    );

    match expr {
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) => Some(PeekedShape::Leaf(expr.clone())),
        TypeExpr::Ref {
            type_arguments,
            name,
        } if type_arguments.is_empty() => Some(PeekedShape::BareCarrier { name: name.clone() }),
        _ => {
            // Operator-shape (Pick/Omit/IndexedAccess/Conditional/Mapped)
            // or generic instantiation: consult `ShapeCacheDb` only.
            // Does NOT consult RouteDb / OwnerImportSurfaceDb (those
            // would rebuild HostStoreView and re-introduce the
            // per-query workspace-snapshot rebuild cost the peek path
            // exists to avoid).
            let ctx: &dyn ResolverContext = query_engine.ctx;
            // Key the operator-shape slot by the EXACT reduction
            // context the whole-expression materialiser
            // (`materialize_component_meta_type_expr_until_stable_full`)
            // writes under, so this peek and that publish share one
            // cache identity. A bare `published(mode)` key would miss a
            // `StructuralTransit(Navigate)`-published entry (or, worse,
            // hit a published entry storing a transit-lowered value).
            // The subject is the LOWERED settled node — lowered through
            // the SAME shared pre-peek helper the materialiser keys and
            // publishes with (`lower_type_expr_for_shape_subject`), so
            // the peeked node identity and the published node identity
            // cannot diverge. A composite expression that NESTS a
            // synthetic carrier has no sound content-free cache key —
            // bypass the cache (no warm hit available) and report a miss
            // so the caller cold-computes; a scope with no view-correct
            // identity likewise yields no key (miss).
            let reduction_context = super::materialize::type_expr_materialize_reduction_context(
                ctx,
                scope_canonical_id,
                expr,
                mode,
            );
            crate::component_meta_caches::ShapeCacheKey::type_expr_whole_with_context(
                Arc::<str>::from(scope_canonical_id),
                expr,
                reduction_context,
                || {
                    super::materialize::lower_type_expr_for_shape_subject(
                        query_engine,
                        scope_canonical_id,
                        expr,
                        reduction_context,
                    )
                    .map(|lowering| lowering.lowered)
                },
            )
            .and_then(|key| {
                ctx.project_type_store()
                    .shape_cache_db()
                    .peek(&key, ctx)
                    .map(PeekedShape::Cached)
            })
        }
    }
}

/// Read the [`SurfaceView`] members backing `node`, if `node` resolves
/// to a `SemanticNodeData::Object` shell. Empty for any other variant
/// — callers treat the empty surface as "no enumerable members".
///
/// Mirrors `slot_binding_graph::read_surface_members`.
pub(crate) fn read_surface_members(
    ctx: &dyn ResolverContext,
    surface_node: SemanticNodeId,
) -> Vec<SurfaceMember> {
    match crate::project_semantic_dispatch::node_data_for(ctx, surface_node).as_deref() {
        Some(SemanticNodeData::Object(view)) => view.members.iter().cloned().collect(),
        _ => Vec::new(),
    }
}

/// Build a [`MacroExpansionDiagnostics`] for a `QueryError` encountered
/// during projection. Mirrors `slot_binding_graph::macro_expansion_for_query_error`.
pub(crate) fn macro_expansion_for_query_error(
    macro_index: usize,
    macro_kind: MacroExpansionKind,
    context: String,
) -> MacroExpansionDiagnostics {
    MacroExpansionDiagnostics {
        macro_kind,
        macro_index,
        diagnostics: vec![ExpansionDiagnostic {
            reason: ExpansionStopReason::InstantiationError,
            context,
            property_name: None,
        }],
        exactness: ExpansionExactness::Incomplete,
        execution_status: ExpansionExecutionStatus::Interrupted,
    }
}

/// Build a [`MacroExpansionDiagnostics`] for a `Recursive` back-edge
/// encountered during projection. Mirrors
/// `slot_binding_graph::macro_expansion_for_cycle`. Cycles are not
/// fatal — they bound the published surface to the non-recursive arms.
pub(crate) fn macro_expansion_for_cycle(
    macro_index: usize,
    macro_kind: MacroExpansionKind,
    context: String,
) -> MacroExpansionDiagnostics {
    MacroExpansionDiagnostics {
        macro_kind,
        macro_index,
        diagnostics: vec![ExpansionDiagnostic {
            reason: ExpansionStopReason::CyclicReference,
            context,
            property_name: None,
        }],
        exactness: ExpansionExactness::Incomplete,
        execution_status: ExpansionExecutionStatus::Completed,
    }
}

/// Surface-provenance for a macro payload's own-body members (by
/// design).
///
/// `defineProps<T>()` and `withDefaults(defineProps<T>(), …)` resolve a
/// PROP surface whose members the author literally wrote in the macro
/// type argument `T`; those members must carry
/// [`verter_session::semantic_query::SurfaceMember::declared_in_macro_type_arg`]
/// `= true`. The bit is consumed by
/// `verter_audit::PublishedSurfacePolicy::Refined` to distinguish
/// author-declared props from heritage-reached props on the PROPS axis
/// only. Every other macro kind (emits / slots / options / model /
/// expose) resolves a structural surface — the bit is always `false`
/// downstream for those kinds — so they lower with
/// [`SurfaceProvenanceContext::Structural`].
#[inline]
#[must_use]
pub(crate) fn macro_payload_surface_provenance(
    macro_kind: AnalyzedMacroKind,
) -> crate::semantic_query::SurfaceProvenanceContext {
    match macro_kind {
        AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
            crate::semantic_query::SurfaceProvenanceContext::MacroTypeArgOwnBody
        }
        AnalyzedMacroKind::DefineEmits
        | AnalyzedMacroKind::DefineSlots
        | AnalyzedMacroKind::DefineModel
        | AnalyzedMacroKind::DefineExpose
        | AnalyzedMacroKind::DefineOptions => {
            crate::semantic_query::SurfaceProvenanceContext::Structural
        }
    }
}

/// Resolve a type-based macro's payload through `ResolveMacroPayload`.
///
/// Reads the macro arg's ONE mode-neutral mirror handle
/// (`crate::structural_carrier_producer::macro_type_arg_hot_ref`) — the producer lowered
/// the `parsed_type_argument` once — then RE-ENTERS the shared dispatch with
/// `ResolveMacroPayload` (Navigate) for the terminal demand over that carrier
/// handle and returns the macro payload node on success. This is a different
/// DEMAND on the same producer, NOT a second lowering of the macro arg.
///
/// On `Recursive` or `Error`, appends a diagnostic to `diag_sink` and
/// returns `None`. Dep-signature is accumulated unconditionally.
///
/// Silent-miss prevention (§7.5): when lowering itself fails (the
/// type expression cannot be lowered to a `SemanticNodeId` — e.g. an
/// unresolved import to a non-existent module), a diagnostic is
/// pushed before returning `None`.
pub(crate) fn resolve_macro_payload(
    dispatch: &ProjectSemanticDispatch<'_>,
    owner: &DeclIdentity,
    file: &str,
    macro_index: usize,
    mac: &AnalyzedMacro,
    macro_kind: AnalyzedMacroKind,
    expansion_kind: MacroExpansionKind,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> Option<SemanticNodeId> {
    let _ = mac.parsed_type_argument.as_ref()?;
    // The macro type-argument graph node is the ONE mode-neutral mirror
    // handle (its macro-T own-body provenance is baked at production time —
    // `defineProps` / `withDefaults` own-body members carry
    // `declared_in_macro_type_arg = true`). The terminal demand below
    // (`ResolveMacroPayload`, Navigate) re-enters the ONE shared dispatch
    // from the carrier handle — a different DEMAND on the same producer, NOT
    // a second lowering of the macro arg.
    let type_args: Arc<[SemanticNodeId]> =
        match crate::structural_carrier_producer::macro_type_arg_hot_ref(
            dispatch.ctx,
            file,
            macro_index,
        ) {
            Some(handle) => Arc::from(vec![handle.node()].into_boxed_slice()),
            None => {
                diag_sink.push(macro_expansion_for_query_error(
                    macro_index,
                    expansion_kind,
                    format!("macro-payload-lowering-failed@{:?}", macro_kind),
                ));
                return None;
            }
        };

    let payload_read = dispatch.execute_read(SemanticQueryKey::ResolveMacroPayload {
        owner: dispatch.type_slot_for(
            Arc::clone(&owner.canonical_id),
            Arc::clone(&owner.decl_name),
        ),
        macro_index,
        macro_kind,
        type_args,
        context: dispatch.macro_payload_context_for(&owner.canonical_id, ProjectionMode::Navigate),
    });
    crate::request_context::observe_component_meta_read_suppress(&payload_read);
    emit_dispatch_dep_signature_facts(dispatch.ctx, &payload_read.dep_signature);
    if !payload_read.walker_diagnostics.is_empty() {
        diag_sink.push(shallow_diagnostics_to_macro_expansion(
            &payload_read.walker_diagnostics,
            macro_index,
            expansion_kind.clone(),
            payload_read.cache_suppress,
        ));
    }

    let payload_node = match payload_read.value {
        QueryResult::Value(id) => id,
        QueryResult::Recursive(_) => {
            diag_sink.push(macro_expansion_for_cycle(
                macro_index,
                expansion_kind,
                format!("cyclic-macro-payload@{:?}", macro_kind),
            ));
            return None;
        }
        QueryResult::Error(e) => {
            diag_sink.push(macro_expansion_for_query_error(
                macro_index,
                expansion_kind,
                format!("macro-payload-error::{:?}", e),
            ));
            return None;
        }
    };

    // Silent-miss prevention (§7.5): when the dispatch returns an
    // opaque-as-value sentinel (i.e. the resolution stuck at an
    // unresolved declaration / cycle inside Navigate mode), publish
    // a diagnostic before bailing. Without this, callers see an
    // empty surface that's indistinguishable from a successful
    // empty payload.
    if let Some(SemanticNodeData::Opaque(err)) =
        crate::project_semantic_dispatch::node_data_for(dispatch.ctx, payload_node).as_deref()
    {
        diag_sink.push(macro_expansion_for_query_error(
            macro_index,
            expansion_kind,
            format!("macro-payload-opaque::{:?}", err),
        ));
        return None;
    }

    // Silent-miss compensation for the carrier-preserving publication
    // contract. Under `Navigate` lowering an unresolved import either
    // collapses to an empty `Object` surface downstream or rides a
    // `DeclRef` carrier whose declaration does not exist (the import
    // mapping is a shallow fact; declaration existence is a resolution
    // fact). Both shapes are indistinguishable from a legitimately
    // empty / carrier payload without a structural probe. Probe the
    // macro's `parsed_type_argument` at `Navigate` and discriminate
    // STRUCTURALLY — typed-IR inspection only, no Expanded lowering of
    // the full type argument:
    //   - a probe node that is an `Opaque` Miss-class sentinel is
    //     unresolved directly;
    //   - a probe node that is a root `DeclRef` carrier resolves ONE
    //     hop through `ResolveDecl`; a Miss-class outcome means the
    //     name routed to a file that does not declare it.
    // Legitimate empty macros (`defineProps<{}>()`) probe to a
    // non-`Opaque` Object; carrier-stopped payloads (an open mapped
    // surface) are neither empty-Object nor root-`DeclRef` shapes and
    // never reach the probe.
    let payload_data = crate::project_semantic_dispatch::node_data_for(dispatch.ctx, payload_node);
    let payload_is_empty_surface = matches!(
        payload_data.as_deref(),
        Some(SemanticNodeData::Object(view))
            if view.members.is_empty()
                && view.call_signatures.is_empty()
                && view.construct_signatures.is_empty()
                && view.index_signatures.is_empty()
    );
    let payload_is_decl_ref_carrier = matches!(
        payload_data.as_deref(),
        Some(SemanticNodeData::DeclRef { .. })
    );
    // An unresolved-reference carrier (`BareRef` / `ImportType` / `TypeOf`)
    // surviving as the macro payload is the carrier-preserving counterpart of
    // the `DeclRef` case: under the query-free macro hot mirror a single-arg
    // props payload (`defineProps<MissingImport>()`) returns the macro arg's
    // structural carrier verbatim, so a payload whose declaration does not
    // exist arrives here as a `BareRef`/`ImportType`/`TypeOf` carrier rather
    // than a pre-resolved `DeclRef`. The structural probe below resolves it ONE
    // Navigate hop through the shared dispatch (which records the value-root
    // `ImportRoute` / `FileWholeHash` facts) and discriminates a genuine miss.
    let payload_is_unresolved_ref_carrier = matches!(
        payload_data.as_deref(),
        Some(data) if data.bare_ref_head().is_some()
            || data.import_type_head().is_some()
            || data.typeof_head().is_some()
    );
    drop(payload_data);
    if (payload_is_empty_surface
        || payload_is_decl_ref_carrier
        || payload_is_unresolved_ref_carrier)
        && mac.parsed_type_argument.is_some()
    {
        {
            // Probe the SAME mirror handle (a different DEMAND on the one
            // producer — never a second lowering). Resolve its carrier head
            // ONE Navigate hop through the shared dispatch so the structural
            // root carrier becomes the resolved `DeclRef` / `Opaque(Miss)`
            // the discrimination below inspects.
            let probe_node = crate::structural_carrier_producer::macro_type_arg_hot_ref(
                dispatch.ctx,
                file,
                macro_index,
            )
            .map(|handle| {
                let probe_read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
                    base: handle.node(),
                    path: empty_path(),
                    context:
                        crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                            ProjectionMode::Navigate,
                        ),
                });
                emit_dispatch_dep_signature_facts(dispatch.ctx, &probe_read.dep_signature);
                match probe_read.value {
                    QueryResult::Value(id) => id,
                    QueryResult::Recursive(id) => id,
                    QueryResult::Error(_) => handle.node(),
                }
            });
            if let Some(probe_node) = probe_node {
                let unresolved: Option<String> =
                    match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, probe_node)
                        .as_deref()
                    {
                        Some(SemanticNodeData::Opaque(err)) => Some(format!("{err:?}")),
                        Some(SemanticNodeData::DeclRef { identity }) => {
                            let read = dispatch.execute_read(SemanticQueryKey::ResolveDecl(
                                crate::semantic_query::ResolveDeclKey {
                                    scope: crate::semantic_query::ScopeId {
                                        canonical_id: std::sync::Arc::clone(&identity.canonical_id),
                                        local_scope: None,
                                    },
                                    name: std::sync::Arc::clone(&identity.decl_name),
                                },
                            ));
                            emit_dispatch_dep_signature_facts(dispatch.ctx, &read.dep_signature);
                            match read.value {
                                QueryResult::Value(anchor) => {
                                    match crate::project_semantic_dispatch::node_data_for(
                                        dispatch.ctx,
                                        anchor,
                                    )
                                    .as_deref()
                                    {
                                        Some(SemanticNodeData::Opaque(
                                            err @ crate::semantic_query::QueryError::Miss,
                                        )) => Some(format!("{err:?}")),
                                        _ => None,
                                    }
                                }
                                QueryResult::Error(err) => Some(format!("{err:?}")),
                                QueryResult::Recursive(_) => None,
                            }
                        }
                        // An unresolved-reference carrier SURVIVING the
                        // Navigate identity retry is a genuine unresolved
                        // route/declaration: a resolvable head would have
                        // become its `DeclRef` / `InstantiationRef` identity
                        // carrier under the Navigate probe. The preserved
                        // `BareRef` / `ImportType` / `TypeOf` shape is the
                        // carrier-preserving counterpart of the pre-carrier
                        // `Opaque(Miss)` terminal — same silent-miss
                        // contract, one diagnostic.
                        Some(data)
                            if data.bare_ref_head().is_some()
                                || data.import_type_head().is_some()
                                || data.typeof_head().is_some() =>
                        {
                            Some("unresolved reference carrier".to_string())
                        }
                        _ => None,
                    };
                if let Some(err) = unresolved {
                    diag_sink.push(macro_expansion_for_query_error(
                        macro_index,
                        expansion_kind,
                        format!("macro-payload-decl-unresolved::{err}"),
                    ));
                    return None;
                }
            }
        }
    }

    Some(payload_node)
}

pub(crate) fn resolve_payload_surface(
    dispatch: &ProjectSemanticDispatch<'_>,
    payload_node: SemanticNodeId,
    macro_index: usize,
    expansion_kind: MacroExpansionKind,
    provenance: crate::semantic_query::SurfaceProvenanceContext,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> Option<SemanticNodeId> {
    // The empty-path `ProjectPath` carries the macro's surface
    // provenance (by design): for a props payload that
    // resolved to a `DeclRef` carrier (`defineProps<FooProps>()`), the
    // walker's `DeclPlaceholder` expansion preserves the
    // `MacroTypeArgOwnBody` provenance onto its `Instantiate`, so
    // `FooProps`'s OWN-body members surface with
    // `declared_in_macro_type_arg = true`. Structural-provenance kinds
    // (emits / slots / …) pass `Structural` and observe `false`.
    // Vue macro object-surface publication. The macro
    // surface enumerates the UNION of object-arm members
    // (`defineProps<FixedProps | BubbleProps>()` declares every arm's
    // props), NOT the TS property-access common-member intersection that
    // an ordinary `Published(Shallow)` ProjectPath would synthesise. The
    // `MacroObjectSurface` demand selects the union-arm rule at the
    // empty-path Shallow terminal surface and is cache-keyed in a distinct
    // `ModeSlot` so the macro surface never collides with an ordinary
    // `Published(Shallow)` read of the same payload node. `provenance`
    // (macro-T own-body for props; structural for slots / emits) rides on
    // the context unchanged.
    let surface_read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: payload_node,
        path: empty_path(),
        context: crate::semantic_query::ProjectionReductionContext::macro_object_surface(
            ProjectionMode::Shallow,
            provenance,
        ),
    });
    crate::request_context::observe_component_meta_read_suppress(&surface_read);
    emit_dispatch_dep_signature_facts(dispatch.ctx, &surface_read.dep_signature);
    if !surface_read.walker_diagnostics.is_empty() {
        diag_sink.push(shallow_diagnostics_to_macro_expansion(
            &surface_read.walker_diagnostics,
            macro_index,
            expansion_kind.clone(),
            surface_read.cache_suppress,
        ));
    }
    match surface_read.value {
        QueryResult::Value(id) => Some(id),
        QueryResult::Recursive(_) => {
            diag_sink.push(macro_expansion_for_cycle(
                macro_index,
                expansion_kind,
                "cyclic-macro-payload-surface".to_string(),
            ));
            None
        }
        QueryResult::Error(e) => {
            diag_sink.push(macro_expansion_for_query_error(
                macro_index,
                expansion_kind,
                format!("macro-payload-surface-error::{:?}", e),
            ));
            None
        }
    }
}

// Macro-payload boundary substrate primitives — diagnostic probe,
// scope tag, branch-merged scope-gated resolver, MemberValueRole —
// live in the sibling [`macro_payload_substrate`] module to keep this file
// under the `no_oversize_files` architecture-guard cap. The
// primitives are re-exported at the module top so call sites
// continue to import them as `crate::meta_resolve::projectors::*`.
