//! Terminal component output materialization and published-field finalization.

use verter_type_expr::{TypeExpr, UnknownValue};

use crate::output_sinks::OutputProjector;
use verter_type_engine::project_semantic_dispatch::output_materialization::{
    wrap_degraded_output, wrap_output_type_expr, MaterializedOutputTypeExpr,
};
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::semantic_query::{ProjectionMode, SemanticNodeId};

crate::output_sinks::define_output_capability! {
    /// Component publication's terminal output-sink capability. Callers in
    /// `meta_resolve::projectors` reach this capability through the APIs in this
    /// `output_sink` module; the capability itself can be MINTED only here. Its
    /// constructor is visible ONLY within
    /// `crate::meta_resolve::projectors::output_sink` (this terminal sink
    /// module) — NOT the parent `projectors` module and NOT its NON-sink helper
    /// children — so a planted `MetaResolveProjectorsOutputCap::new` in a
    /// non-sink helper (e.g. `define_shapes`) is `E0624`. This
    /// terminal-sink scoping is what makes the output-materialization fence
    /// compiler-enforced rather than convention-based: only a true output sink
    /// can mint.
    pub(crate) struct MetaResolveProjectorsOutputCap;
    mint: pub(in crate::meta_resolve::projectors::output_sink)
}

/// Raw raise of a graph `node` into a SEALED [`MaterializedOutputTypeExpr`]
/// carrier — the node-domain seal the publication gates use after their
/// node-fact decisions have been made. Mints the cap, materialises the node into
/// the sealed `OutputTypeExpr` payload (NO `into_type_expr` — never produces a
/// bare [`TypeExpr`]), and assembles the carrier with the supplied
/// `dep_signature`. A raise miss seals an `Unknown` shell so the per-member slot
/// still publishes a carrier.
///
/// TERMINAL one-shot sink: materialises ONCE and builds the carrier, making NO
/// decision on the materialised value. The node-fact gate decisions happen on the
/// `node` BEFORE this is called, so the gates never touch a materialised
/// `TypeExpr`. The carrier `node_id` is the producing `node`, so a downstream
/// consumer reads node facts off it (e.g. the root-sentinel gate) instead of
/// re-materialising.
fn raise_node_to_sealed_carrier<M: verter_audit::observe::ObserveExec>(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities<M>>,
    node: SemanticNodeId,
    dep_signature: verter_type_engine::semantic_query::DepSignature,
) -> MaterializedOutputTypeExpr {
    let cap = MetaResolveProjectorsOutputCap::new(dispatch);
    let sealed = match cap.materialize_output_type_expr(node) {
        Some(sealed) => sealed,
        None => {
            if dispatch.graph().read_in_scope(node).is_none() {
                // GENUINE absence (no typed payload minted for this id): the
                // exact `missing_output` — NON-partial.
                wrap_output_type_expr(
                    cap.authority(),
                    TypeExpr::Unknown(UnknownValue::missing_output()),
                )
            } else {
                // PRESENT-BUT-UNRAISABLE (the node minted but a required child
                // failed): degrade as the typed unmaterialized `Miss` carrier —
                // PARTIAL at the choke point, never admitted complete.
                wrap_degraded_output(
                    cap.authority(),
                    verter_type_engine::semantic_query::QueryError::Miss,
                )
            }
        }
    };
    MaterializedOutputTypeExpr::from_parts(Some(node), sealed, dep_signature, false)
}

/// The publication SOURCE for a reduced field value: a node that resolved to
/// a complete LEAF (primitive / string / number / boolean literal) projects to
/// its closed leaf fact — the one node class whose fact is complete by itself;
/// any richer resolved shape publishes the field's existing content-free
/// source unchanged (shallow-by-default: the consumer re-raises it through the
/// one engine on demand — the reduction above already warmed the shared
/// dispatch memos).
/// The owner's authored import-binding alias for a PACKAGE-backed
/// declaration-reference carrier: the local binding name whose forward
/// export-target resolution lands on the carrier's declaration identity.
/// `None` when the carrier is not package-backed or no owner binding
/// resolves to it (the caller keeps the authored annotation slot).
///
/// Typed-domain only: the owner's analyzed import bindings + the shared
/// shallow export-target resolver — no raw-text recovery.
fn authored_package_alias_for_carrier<M: verter_audit::observe::ObserveExec>(
    ctx: &dyn verter_type_engine::resolver_core::ResolverContext<
        crate::resolver_core::HostCapabilities<M>,
    >,
    dispatch: &verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch<
        '_,
        crate::resolver_core::HostCapabilities<M>,
    >,
    owner_canonical: &str,
    node: SemanticNodeId,
) -> Option<String> {
    let data = dispatch.graph().read_in_scope(node)?;
    let verter_type_engine::semantic_query::SemanticNodeData::DeclRef { identity } = data.as_ref()
    else {
        return None;
    };
    if !ctx.workspace_is_package_backed(identity.canonical_id.as_ref()) {
        return None;
    }
    let snapshot = ctx.get_raw_analysis_snapshot(owner_canonical)?;
    snapshot.imports.iter().find_map(|import| {
        let target = import.resolved_canonical_id.as_deref()?;
        if !ctx.workspace_is_package_backed(target) {
            return None;
        }
        import.bindings.iter().find_map(|binding| {
            let imported = binding.imported_name.as_deref().unwrap_or(&binding.name);
            let (resolved_id, resolved_name) =
                ctx.resolve_named_type_export_target_shallow(target, imported)?;
            (resolved_id == identity.canonical_id.as_ref()
                && resolved_name == identity.decl_name.as_ref())
            .then(|| binding.name.clone())
        })
    })
}

/// NODE-start per-field reducer for the publication finaliser: run the shallow
/// gates in NODE DOMAIN off the raised input node, reduce through the
/// graph-native member-value reducer when the gates clear, and apply the
/// input-side no-poison gate off the SAME observed input node.
///
/// Gate order (all node-domain, no `TypeExpr` materialised for a decision):
///
/// 1. package-backed object-like root → publish the input carrier verbatim
///    (shallow-by-default);
/// 2. generic-instantiation transitive-cycle root → publish the input carrier;
/// 3. no reducible operator AND not a generic instantiation → publish the
///    input carrier (nothing to reduce);
/// 4. reduce via [`reduce_member_value_graph_native_with_context`] under the
///    node-derived reduction context;
/// 5. no-poison: when the reduction's ROOT came back as the unmaterialised
///    sentinel and the INPUT node confidently carried no semantic miss (the
///    whole-tree miss fact read off the SAME input node the caller raised —
///    one observed lowering, no second live lower), the input carrier IS the
///    published shape.
fn reduce_field_value_node<M: verter_audit::observe::ObserveExec>(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_, M>,
    scope_canonical_id: &str,
    input_node: SemanticNodeId,
    publish_mode: ProjectionMode,
) -> MaterializedOutputTypeExpr {
    let dispatch = query_engine.dispatch;

    let (package_backed, _fence) =
        crate::meta_resolve::node_package_backed_object_like_root_with_fence(
            query_engine,
            scope_canonical_id,
            input_node,
        );
    if package_backed {
        return raise_node_to_sealed_carrier(
            dispatch,
            input_node,
            verter_type_engine::semantic_query::DepSignature::default(),
        );
    }
    let gates = super::classify_node_reduction_gates(dispatch, input_node);
    if gates.generic_instantiation_ref {
        let (reaches_cycle, _fence) =
            crate::meta_resolve::node_root_reaches_transitive_cycle_with_fence(
                dispatch,
                scope_canonical_id,
                input_node,
            );
        if reaches_cycle {
            return raise_node_to_sealed_carrier(
                dispatch,
                input_node,
                verter_type_engine::semantic_query::DepSignature::default(),
            );
        }
    }
    if !(gates.contains_reducible_operator || gates.generic_instantiation_ref) {
        return raise_node_to_sealed_carrier(
            dispatch,
            input_node,
            verter_type_engine::semantic_query::DepSignature::default(),
        );
    }

    let context = crate::meta_resolve::materialize::node_materialize_reduction_context(
        dispatch,
        input_node,
        publish_mode,
    );
    let reduced = crate::meta_resolve::materialize::reduce_member_value_graph_native_with_context(
        dispatch,
        scope_canonical_id,
        input_node,
        context,
    );

    let root_is_sentinel = reduced.node_id().is_some_and(|node| {
        verter_type_engine::project_semantic_dispatch::raise::node_root_is_unmaterialized_sentinel_with_dispatch(
            dispatch, node,
        )
    });
    if root_is_sentinel
        && verter_type_engine::project_semantic_dispatch::raise::node_contains_semantic_miss_with_dispatch(
            dispatch, input_node,
        ) == Some(false)
    {
        return raise_node_to_sealed_carrier(dispatch, input_node, verter_type_engine::semantic_query::DepSignature::default());
    }
    reduced
}

/// The ONE centralized missing-source output policy: a lane position whose
/// analysis source is `None` (no typed payload available) materializes to the
/// canonical typed `TypeExpr::Unknown`. Every output lane routes an absent
/// source through THIS function — absence semantics are session-owned and
/// decided in exactly one place, never re-derived per lane or at the wire.
fn missing_source_output_type_expr() -> TypeExpr {
    TypeExpr::Unknown(UnknownValue::missing_output())
}

/// Materialize ONE present output source to its wire `TypeExpr` at this
/// terminal sink.
///
/// - `Closed(Leaf)` / `Closed(LeafUnion)` render their published shallow
///   value DIRECTLY — they are NOT raised. Raising a closed source re-runs
///   name resolution in the owner scope, which can resolve a package
///   re-export alias to its internal declaration name: a semantic demand
///   that breaks shallow-by-default (see the shallow-probe special case in
///   `project_semantic_dispatch::semantic_source`).
/// - Every other source raises through the ONE shared engine — the STRICT
///   raise entry (`raise_semantic_type_source_to_hot_strict`) under
///   `Navigate` structural transit — then shell-materializes through the
///   sealed output capability (PLAIN SHELL — refs stay refs; never the
///   reduced/`Expanded` materializer) and strictly unwraps the sealed
///   carrier here, inside the sink.
/// - FAIL-CLOSED: a present source that cannot be raised or
///   shell-materialized returns a typed failure, AND a failed REQUIRED
///   interior dereference inside a successfully-composed root (an interned
///   `Opaque(Miss)` the shell algebra would render as `Unknown`) is
///   REJECTED BEFORE shell materialization with its nested position path
///   ([`crate::meta_resolve::ComponentMetaOutputFailure::InteriorSourceMiss`]).
///   Genuinely ABSENT schema positions at the LANE level render the
///   centralized typed `Unknown` through the `Absent` arm and never reach
///   this raise. There is NO `Unknown`-synthesizing fallback on this path —
///   a materialization failure must never silently become `Unknown`.
/// - CONSERVATIVE interior fail-close: a successfully-raised
///   schema-PRESENT position (a direct source-root deref or a composed
///   shell's present slot) whose materialized shape carries an
///   unknown-materializing resolver-failure carrier ANYWHERE (root or
///   interior) — a graph-interned `Opaque` control failure the shell fold
///   would render as a completed `unknown` — is REJECTED by the strict
///   raise
///   ([`crate::meta_resolve::ComponentMetaOutputFailure::UnknownMaterializingSourceInterior`]),
///   via the shared node-domain whole-tree miss fact (the legitimately
///   publishable carriers — a recursive reference, a declaration
///   placeholder — are not misses and pass). Proven schema absence keeps
///   its typed `Unknown`: an ABSENT slot of a composed fact shell interns
///   the typed miss WITHOUT a deref (the schema `Option` is the proof), so
///   it is never checked. The graph does not carry per-position
///   absent-vs-failed `Opaque` provenance yet, so a DEREF'D body's
///   interior failure conservatively fails the whole position. A nested
///   position can render `unknown` only when the schema itself proves that
///   position absent; an opaque failure inside a dereferenced body carries
///   no equivalent proof.
fn materialize_output_source<M: verter_audit::observe::ObserveExec>(
    dispatch: &ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities<M>>,
    cap: &MetaResolveProjectorsOutputCap<'_, '_, crate::resolver_core::HostCapabilities<M>>,
    scope_canonical_id: &str,
    scope_owner: verter_type_expr::TopLevelOwnerId,
    source: &verter_type_expr::facts::SemanticTypeSource,
) -> Result<TypeExpr, crate::meta_resolve::ComponentMetaOutputFailure> {
    use verter_type_expr::facts::{ClosedTypeFact, SemanticTypeSource};
    match source {
        SemanticTypeSource::Closed(ClosedTypeFact::Leaf(leaf)) => {
            Ok(verter_type_engine::project_semantic_dispatch::lower::leaf_type_fact_expr(leaf))
        }
        SemanticTypeSource::Closed(ClosedTypeFact::LeafUnion(leaves)) => Ok(TypeExpr::Union(
            leaves
                .iter()
                .map(verter_type_engine::project_semantic_dispatch::lower::leaf_type_fact_expr)
                .collect(),
        )),
        other => {
            let transit_ctx =
                verter_type_engine::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                    ProjectionMode::Navigate,
                );
            // The raised carrier owns the node while the cap materialises it.
            let raised = dispatch.owning().raise_semantic_type_source_to_hot_strict(
                other,
                scope_canonical_id,
                scope_owner,
                transit_ctx,
            );
            let hot = raised
                .value()
                .clone()
                .map_err(|failure| match failure {
                    verter_type_engine::project_semantic_dispatch::semantic_source::StrictSourceRaiseFailure::InteriorMiss(path) => {
                        crate::meta_resolve::ComponentMetaOutputFailure::InteriorSourceMiss { path }
                    }
                    verter_type_engine::project_semantic_dispatch::semantic_source::StrictSourceRaiseFailure::UnknownMaterializing(path) => {
                        crate::meta_resolve::ComponentMetaOutputFailure::UnknownMaterializingSourceInterior { path }
                    }
                    // A ROOT-level typed query failure. Previously EVERY
                    // one of these collapsed into the raise's `Ok(None)` and
                    // surfaced as `UnraisableSource` — indistinguishable from
                    // a genuine absence. The typed error now decides, through
                    // the single disposition authority.
                    verter_type_engine::project_semantic_dispatch::semantic_source::StrictSourceRaiseFailure::QueryFailure(err) => {
                        use verter_type_engine::project_semantic_dispatch::query_error_disposition::{query_error_disposition, QueryErrorDisposition};
                        match query_error_disposition(&err) {
                            // A publishable control carrier reaches the failure
                            // arm ONLY when its carrier could not be interned —
                            // i.e. the source genuinely has no live graph
                            // representation. That is `UnraisableSource`.
                            QueryErrorDisposition::RecursionCarrier
                            | QueryErrorDisposition::ExpandableDecl
                            | QueryErrorDisposition::CheckerRecovery
                            | QueryErrorDisposition::BudgetRecovery
                            // Optional absence never reaches here (it is
                            // `Ok(None)`); if the raise contract ever changed,
                            // absence is still the correct answer for it.
                            | QueryErrorDisposition::OptionalAbsence => {
                                crate::meta_resolve::ComponentMetaOutputFailure::UnraisableSource
                            }
                            // Every other disposition is a carrier whose shell
                            // fold would render a completed `unknown`. Fail at
                            // the ROOT position (empty path — that arm's
                            // documented convention for a direct source-root
                            // deref).
                            QueryErrorDisposition::ControlCarrier
                            | QueryErrorDisposition::UnsupportedSurface
                            | QueryErrorDisposition::UnsupportedPosition
                            | QueryErrorDisposition::Partial
                            | QueryErrorDisposition::PermissiveWildcard
                            | QueryErrorDisposition::Failure => {
                                crate::meta_resolve::ComponentMetaOutputFailure::UnknownMaterializingSourceInterior {
                                    path: std::sync::Arc::from(Vec::new().into_boxed_slice()),
                                }
                            }
                        }
                    }
                })?
                .ok_or(crate::meta_resolve::ComponentMetaOutputFailure::UnraisableSource)?;
            let sealed = cap
                .materialize_output_type_expr(hot.node())
                .ok_or(crate::meta_resolve::ComponentMetaOutputFailure::ShellMaterializationMiss)?;
            Ok(sealed.into_type_expr(cap.authority()))
        }
    }
}

// The output-ENVELOPE assembly (the request-local source memo, the 11-lane
// materializer, the session-owned registry overlay finalize, and the envelope
// builder) lives in the `envelope` CHILD module of this sink — inside the
// capability mint scope (`pub(in ...::output_sink)` covers descendants), so
// the terminal-sink fence is unchanged.
// The published-field FINALIZE pass (the whole-surface field-type reducer
// over `ExpandedComponentTypes`) lives in the `published_finalize` CHILD
// module of this sink — inside the capability mint scope
// (`pub(in ...::output_sink)` covers descendants), so the terminal-sink
// fence is unchanged.
mod published_finalize;
pub(crate) use published_finalize::reduce_published_field_types;

mod envelope;
pub(crate) use envelope::build_component_meta_output;
#[cfg(any(test, feature = "test-support"))]
pub(crate) use envelope::OUTPUT_MATERIALIZE_FORCE_FAIL_FOR;
#[cfg(test)]
pub(crate) use envelope::{
    LAST_OUTPUT_MATERIALIZE_CALLS, LAST_OUTPUT_MEMO_HASH_OPS, OUTPUT_MATERIALIZE_FORCE_FAIL,
    OUTPUT_PUBLICATION_FAILURE,
};

#[cfg(test)]
#[path = "output_sink_tests.rs"]
mod semantic_outcome_tests;
