//! Dispatch fact fan-out.
//!
//! Dispatch reads that have no cache boundary of their own publish their
//! dependency evidence into the active request tracer through
//! [`emit_dispatch_dep_signature_facts`]. The request-level tracer is the sole
//! component-meta signature authority; there is no parallel curated
//! accumulator.

/// Publish facts for dispatch reads that have no result cache of their own.
///
/// The six in-scope dispatch reads — three projector sites in
/// `meta_resolve/projectors/mod.rs` (`resolve_macro_payload`,
/// `resolve_payload_surface`, `resolve_member_value_for_classification`),
/// the materialiser site in
/// `meta_resolve/materialize/field_types.rs::materialize_component_meta_type_expr_until_stable_full`,
/// the cycle-gate site in
/// `meta_resolve/graph_predicates.rs::node_root_reaches_transitive_cycle_with_fence`,
/// and the registry-materialise site in
/// `resolver_core/component_meta_query_engine/registry_decl.rs::materialize_member_surface_expr`
/// — fan their `DepSignature` through this helper. The bridge preserves
/// `WholeHash` and `ProjectGeneration`; `RouteGeneration` has no validating
/// fact representation and is deliberately omitted. The enclosing
/// request-level tracer finalises and owns the reusable cache signature.
pub(crate) fn emit_dispatch_dep_signature_facts(
    ctx: &dyn crate::resolver_core::ResolverContext,
    sig: &crate::semantic_query::DepSignature,
) {
    use std::sync::atomic::Ordering::Relaxed;
    if !sig.is_empty() {
        crate::host_manage::record_dep_signature_merge();
    }

    let bridged = crate::fact_signature_helpers::dep_signature_to_fact_signature(sig);
    crate::fact_signature_helpers::observe_fact_signature(&bridged);
    if let Some(prov) = ctx.project_type_store().semantic_graph().provenance() {
        prov.dispatch_dep_signature_fact_tracer_emissions
            .fetch_add(1, Relaxed);
    }
}
