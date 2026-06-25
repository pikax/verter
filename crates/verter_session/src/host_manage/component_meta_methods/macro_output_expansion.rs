//! Sink-owned macro-output expansion demand API for the `compute_evaluated_types`
//! branches in `host_manage::eval_env` (`defineModel`, generic `ProjectPath`,
//! slot-binding).
//!
//! This module is the AUTHORIZED owner-confined materialisation sink for the macro
//! field-expansion branches. It owns the MODULE-PRIVATE node-domain artifact
//! ([`AdmittedExpansionNode`]) + the MODULE-PRIVATE materialiser
//! ([`materialize_admitted_expansion_node`], which mints the parent's
//! `HostManageComponentMetaOutputCap` INTERNALLY — reachable here because this is a
//! descendant of the cap's `pub(in crate::host_manage::component_meta_methods)`
//! mint scope), and exposes ONLY the three high-level closed-demand methods
//! ([`expand_define_model_output`] / [`expand_generic_project_path_output`] /
//! [`expand_slot_binding_output`]). The eval_env callers pass only closed demands
//! (resolver ctx + owner canonical + macro index + the per-branch terminal demand)
//! — never a raw `SemanticNodeId` and never a forgeable node wrapper — so there is
//! no crate-visible `SemanticNodeId → TypeExpr` adapter. Its only other submodule
//! is a `#[cfg(test)]` parity suite, so its whole reachable PRODUCTION scope is
//! output-only.

use std::sync::Arc;

use super::HostManageComponentMetaOutputCap;
use crate::types::ProjectionMode;

/// A session-local node-bearing expansion result: the produced
/// [`SemanticNodeId`](crate::semantic_query::SemanticNodeId) plus the cache
/// metadata the expansion carries, held in NODE-DOMAIN until the sink materialises
/// it. Distinct from
/// [`ExpandedNormalizedExpr`](verter_semantic::analysis::type_expand::ExpandedNormalizedExpr),
/// which OWNS a `TypeExpr` — that materialised form is produced ONLY at the sink
/// by [`materialize_admitted_expansion_node`].
///
/// MODULE-PRIVATE by design: no module outside `macro_output_expansion` can name,
/// construct, or materialise this artifact. It is produced exclusively INSIDE the
/// sink-owned demand methods below (the `VerterHost`-free free functions
/// `expand_define_model_output` / `expand_generic_project_path_output` /
/// `expand_slot_binding_output`), each of which resolves a CLOSED semantic demand
/// internally into the node and then materialises here. The expansion callers in
/// `host_manage::eval_env` pass only closed demands (resolver ctx + owner
/// canonical + macro index + the per-branch terminal demand) — never a raw
/// `SemanticNodeId` and never a forgeable node wrapper — so there is no
/// crate-visible `SemanticNodeId → TypeExpr` adapter.
#[derive(Debug, Clone)]
struct AdmittedExpansionNode {
    /// The produced expansion node (the `ProjectPath` / lower+resolve result),
    /// produced INSIDE the sink from the caller's closed demand.
    node: crate::semantic_query::SemanticNodeId,
    /// The accumulated dependency signature observed while producing `node`.
    /// Facts-rail metadata carried ON the artifact, not consumed by the
    /// materialiser. The current expansion branches mirror the former
    /// mid-flight-raise path, which left this CacheRead metadata unfolded; it is
    /// retained so a future caller can fold it without re-plumbing the artifact
    /// (the parity suite pins its round-trip).
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "facts-rail metadata carried for caller-side folding; the current \
                      expansion path mirrors the former raise path which left it unfolded; \
                      round-trip pinned by the parity suite"
        )
    )]
    dep_signature: crate::semantic_query::DepSignature,
    /// `true` when producing `node` returned a PARTIAL value (budget /
    /// cancellation / same-path recursion). Retained alongside `dep_signature`
    /// for a future caller's admission gate; same carried-not-folded contract.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "facts-rail metadata carried for caller-side folding; the current \
                      expansion path mirrors the former raise path which left it unfolded; \
                      round-trip pinned by the parity suite"
        )
    )]
    result_is_partial: bool,
}

impl AdmittedExpansionNode {
    /// Construct the artifact from its parts (the expansion node + cache
    /// metadata). MODULE-PRIVATE: invoked only by the sink-owned demand methods
    /// after they themselves resolved `node` from a closed demand — never around
    /// a caller-supplied node.
    #[must_use]
    fn new(
        node: crate::semantic_query::SemanticNodeId,
        dep_signature: crate::semantic_query::DepSignature,
        result_is_partial: bool,
    ) -> Self {
        Self {
            node,
            dep_signature,
            result_is_partial,
        }
    }
}

/// Materialisation sink: the SINGLE place an [`AdmittedExpansionNode`] becomes an
/// [`ExpandedNormalizedExpr`](verter_semantic::analysis::type_expand::ExpandedNormalizedExpr).
///
/// MODULE-PRIVATE. It mints the parent's `HostManageComponentMetaOutputCap`
/// INTERNALLY (reachable from this descendant of the cap's mint scope) to
/// materialise the artifact's node into a sealed output carrier and unwrap it.
/// The artifact reaching it was produced inside a sink-owned demand method from a
/// closed demand — never passed in from a sibling module — so the cap's sealed
/// unwrap and the input authority are BOTH owner-confined.
///
/// Returns `ExpandedNormalizedExpr { expr }` (the sealed `OutputProjector`
/// shell-raise of the node), and `None` on a whole-raise miss.
#[must_use]
fn materialize_admitted_expansion_node(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
    artifact: &AdmittedExpansionNode,
) -> Option<verter_semantic::analysis::type_expand::ExpandedNormalizedExpr> {
    use crate::project_semantic_dispatch::output_materialization::OutputProjector;
    // Mint the component-meta host-method output capability (constructor visible
    // only within this authorized owner module tree) and materialise the node into
    // a sealed carrier, then unwrap it — the sole node→`TypeExpr` materialisation
    // for the expansion branch, performed at the sink.
    let cap = HostManageComponentMetaOutputCap::new(dispatch);
    let expr = cap
        .materialize_output_type_expr(artifact.node)?
        .into_type_expr(&cap);
    Some(verter_semantic::analysis::type_expand::ExpandedNormalizedExpr { expr })
}

/// Outcome of [`expand_define_model_output`] — the `defineModel<T>()` prop/model
/// expansion branch. The model value type IS the field's type, so the branch
/// resolves the macro-argument carrier head DIRECTLY (no terminal path
/// projection) and materialises it.
///
/// Each variant surfaces enough for the eval_env caller to reproduce the former
/// node-domain branch EXACTLY: `produced_node_id` (audit parity — set as soon as
/// the carrier head resolves, whether or not the materialisation then succeeds)
/// and the materialised expr.
pub(crate) enum DefineModelOutputExpansion {
    /// The carrier head resolved and materialised. `produced_node_id` is the
    /// resolved head; `normalized` the sealed materialisation.
    Materialized {
        produced_node_id: crate::semantic_query::SemanticNodeId,
        normalized: verter_semantic::analysis::type_expand::ExpandedNormalizedExpr,
    },
    /// The carrier head resolved but the sink materialisation missed. The caller
    /// keeps its `parsed` fallback (already the model's type); `produced_node_id`
    /// is still set for audit parity.
    RaiseMiss {
        produced_node_id: crate::semantic_query::SemanticNodeId,
    },
    /// No macro-argument carrier head (hot-ref producer miss). The caller keeps
    /// its `parsed` fallback; no node is produced.
    CarrierMiss,
}

/// Outcome of [`expand_generic_project_path_output`] and
/// [`expand_slot_binding_output`] — the two branches that lower the macro-arg
/// carrier in a caller-chosen mode and then project a terminal hop off it
/// (a generic `ProjectPath`, or the slot-binding `Function → params[0] → member`
/// descent). The variant→trace mapping lives in the eval_env caller so the two
/// branches keep their distinct `macro_projection_failover` reason strings.
pub(crate) enum MacroPathOutputExpansion {
    /// The terminal hop resolved to a node and materialised.
    Materialized {
        produced_node_id: crate::semantic_query::SemanticNodeId,
        normalized: verter_semantic::analysis::type_expand::ExpandedNormalizedExpr,
    },
    /// The terminal hop resolved to a node but the sink materialisation missed.
    /// `produced_node_id` is set for audit parity; the caller emits its
    /// branch-specific raise-miss trace and preserves symbolically.
    RaiseMiss {
        produced_node_id: crate::semantic_query::SemanticNodeId,
    },
    /// The terminal hop did NOT yield a node (`Error`/`Recursive`). The caller
    /// emits its branch-specific projection-miss trace and preserves symbolically.
    ProjectionMiss,
    /// The macro-arg carrier head did not lower (opaque scope / uninterpretable).
    /// The caller emits the shared `opaque_scope_or_uninterpretable` trace and
    /// preserves symbolically.
    CarrierMiss,
}

/// Lower the macro-argument carrier head for `(owner_canonical, macro_index)` at
/// `mode` through the shared dispatch, returning the resolved carrier node — the
/// node-producing step shared by the path-projection expansion branches. Returns
/// `None` when the carrier hot-ref producer misses (the
/// `opaque_scope_or_uninterpretable` / lowering-miss case).
///
/// CLOSED-INPUT: the resolver ctx + owner canonical + macro index + mode. No node
/// crosses in. The hot-ref producer is the ONE mode-neutral carrier producer; a
/// different DEMAND on its handle, never a second lowering of the macro argument.
fn lower_macro_arg_carrier_head(
    dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_>,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
    mode: ProjectionMode,
) -> Option<crate::semantic_query::SemanticNodeId> {
    crate::structural_carrier_producer::macro_type_arg_hot_ref(ctx, owner_canonical, macro_index)
        .map(|handle| dispatch.resolve_hot_handle_at_mode(handle, mode))
}

/// Sink-owned demand: expand the `defineModel<T>()` prop/model output.
///
/// CLOSED-INPUT demand API — the eval_env `defineModel` branch passes the
/// resolver ctx + owner canonical + macro index ONLY (never the resolved node,
/// never a forgeable wrapper). This fn resolves the macro-argument carrier head
/// at `Expanded` INTERNALLY, mints the cap, and materialises at the sealed sink,
/// returning a [`DefineModelOutputExpansion`] that lets the caller reproduce the
/// former branch (produced-node-id audit parity + the `parsed` fallback) EXACTLY.
#[must_use]
pub(crate) fn expand_define_model_output(
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
) -> DefineModelOutputExpansion {
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    // Read the macro arg's mode-neutral mirror handle (the ONE producer) and
    // resolve it through the shared dispatch at `Expanded` — the model value type
    // IS the field's type. A different DEMAND on the same handle, not a second
    // lowering of the macro arg.
    let Some(base_id) = lower_macro_arg_carrier_head(
        &dispatch,
        ctx,
        owner_canonical,
        macro_index,
        ProjectionMode::Expanded,
    ) else {
        return DefineModelOutputExpansion::CarrierMiss;
    };
    let artifact =
        AdmittedExpansionNode::new(base_id, Arc::from(Vec::new().into_boxed_slice()), false);
    match materialize_admitted_expansion_node(&dispatch, &artifact) {
        Some(normalized) => DefineModelOutputExpansion::Materialized {
            produced_node_id: base_id,
            normalized,
        },
        None => DefineModelOutputExpansion::RaiseMiss {
            produced_node_id: base_id,
        },
    }
}

/// Sink-owned demand: expand a generic macro field via a terminal `ProjectPath`
/// off the lowered carrier head.
///
/// CLOSED-INPUT demand API — the eval_env generic-`ProjectPath` branch passes the
/// resolver ctx + owner canonical + macro index + the carrier-lower mode + the
/// terminal member path (already converted to semantic segments) ONLY. This fn
/// lowers the carrier head at `carrier_lower_mode`, projects the terminal path at
/// `Expanded`, mints the cap, and materialises at the sealed sink — never
/// accepting a raw node or a forgeable wrapper.
#[must_use]
pub(crate) fn expand_generic_project_path_output(
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
    carrier_lower_mode: ProjectionMode,
    terminal_path: Arc<[crate::semantic_query::PathSegment]>,
) -> MacroPathOutputExpansion {
    use crate::semantic_query::{
        QueryResult, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    };
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    let Some(base_id) = lower_macro_arg_carrier_head(
        &dispatch,
        ctx,
        owner_canonical,
        macro_index,
        carrier_lower_mode,
    ) else {
        return MacroPathOutputExpansion::CarrierMiss;
    };
    let projected = dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base: base_id,
        path: terminal_path,
        context: crate::semantic_query::ProjectionReductionContext::published(
            ProjectionMode::Expanded,
        ),
    });
    match projected {
        QueryResult::Value(SemanticQueryOutput { value: node_id, .. }) => {
            let artifact = AdmittedExpansionNode::new(
                node_id,
                Arc::from(Vec::new().into_boxed_slice()),
                false,
            );
            match materialize_admitted_expansion_node(&dispatch, &artifact) {
                Some(normalized) => MacroPathOutputExpansion::Materialized {
                    produced_node_id: node_id,
                    normalized,
                },
                None => MacroPathOutputExpansion::RaiseMiss {
                    produced_node_id: node_id,
                },
            }
        }
        _ => MacroPathOutputExpansion::ProjectionMiss,
    }
}

/// Sink-owned demand: expand a slot-binding member off the lowered carrier head.
///
/// CLOSED-INPUT demand API — the eval_env slot-binding branch passes the resolver
/// ctx + owner canonical + macro index + the carrier-lower mode + the
/// already-destructured slot/binding names ONLY. This fn lowers the carrier head
/// at `carrier_lower_mode`, descends the slot-binding terminal
/// (`Function → params[0] → Member(binding)`) at `Expanded`, mints the cap, and
/// materialises at the sealed sink — never accepting a raw node or a forgeable
/// wrapper.
#[must_use]
pub(crate) fn expand_slot_binding_output(
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
    carrier_lower_mode: ProjectionMode,
    slot_name: &str,
    binding_name: &str,
) -> MacroPathOutputExpansion {
    use crate::semantic_query::QueryResult;
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(ctx);
    let Some(base_id) = lower_macro_arg_carrier_head(
        &dispatch,
        ctx,
        owner_canonical,
        macro_index,
        carrier_lower_mode,
    ) else {
        return MacroPathOutputExpansion::CarrierMiss;
    };
    let slot_binding = dispatch.project_slot_binding_member_with_terminal_id(
        base_id,
        slot_name,
        binding_name,
        ProjectionMode::Expanded,
    );
    match slot_binding.value {
        QueryResult::Value(terminal_id) => {
            let artifact = AdmittedExpansionNode::new(
                terminal_id,
                slot_binding.dep_signature,
                slot_binding.result_is_partial,
            );
            match materialize_admitted_expansion_node(&dispatch, &artifact) {
                Some(normalized) => MacroPathOutputExpansion::Materialized {
                    produced_node_id: terminal_id,
                    normalized,
                },
                None => MacroPathOutputExpansion::RaiseMiss {
                    produced_node_id: terminal_id,
                },
            }
        }
        _ => MacroPathOutputExpansion::ProjectionMiss,
    }
}

#[cfg(test)]
#[path = "macro_output_expansion_tests.rs"]
mod tests;
