//! Dual-emit discriminator for the six dispatch-read sites routed
//! through
//! `meta_resolve::dep_signature::emit_dispatch_dep_signature_facts`.
//!
//! Pre-dual-emit (before this fix): each of the six dispatch reads
//! that have no result cache of their own — three projector sites in
//! `meta_resolve/projectors/mod.rs`
//! (`resolve_macro_payload`, `resolve_payload_surface`,
//! `resolve_member_value_for_classification`), the materialiser in
//! `meta_resolve/materialize/field_types.rs::materialize_component_meta_type_expr_until_stable_full`,
//! the BFS-cycle site in
//! `meta_resolve/resolved_state.rs::lowered_root_reaches_transitive_cycle`,
//! and the registry-materialise site in
//! `resolver_core/component_meta_query_engine/registry_decl.rs::materialize_member_surface_expr`
//! — emitted dispatch facts ONLY through `observe_fact_signature` (the
//! `ACTIVE_TRACERS` fan-out channel) and stopped pushing into the
//! legacy `DISPATCH_DEP_SIGNATURE_ACCUMULATOR`. That severs the
//! `state.fact_versions` → `ComponentMetaResultEntry.fact_dep_signature`
//! producer path for materializer/projector reads that touch a
//! transitive type file NOT in `parts.tracked_dependencies` — edits
//! to such a file no longer invalidate the warm component-meta cache,
//! and stale metadata is served.
//!
//! Post-dual-emit (this fix): every site routes through
//! `emit_dispatch_dep_signature_facts(ctx, sig)`, which fans facts
//! into BOTH channels in lockstep and bumps two provenance counters:
//!
//! - `dispatch_dep_signature_fact_tracer_emissions` — bumped on every
//!   `observe_fact_signature` call from the helper.
//! - `dispatch_dep_signature_legacy_accumulator_emissions` — bumped
//!   on every `accumulate_dispatch_dep_signature` call from the
//!   helper.
//!
//! The discrimination property: removing the
//! `accumulate_dispatch_dep_signature` half of the helper would leave
//! the legacy counter at zero while the tracer counter still
//! advanced; the lockstep-equal assertion below would FAIL. Removing
//! the `observe_fact_signature` half would invert the failure. A
//! tree without the dual-emit fix fails this test because the
//! legacy counter never advances for the six in-scope sites — only
//! the `slot_binding_graph` dual-emit helper still feeds the
//! accumulator there.

#![cfg(test)]

use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};

fn build_test_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert_vue(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert vue");
}

fn upsert_ts(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("upsert ts");
}

#[test]
fn dispatch_dep_signature_dual_emit_in_lockstep() {
    let host = build_test_host();
    // Cross-file `defineProps<Foo>()` — exercises the projector
    // sites in `meta_resolve/projectors/mod.rs`
    // (`resolve_macro_payload` + `resolve_payload_surface`) AND
    // the materialiser
    // (`materialize_component_meta_type_expr_until_stable_full`)
    // when the materialiser raises the imported root through
    // `dispatch.raise_and_reduce`.
    upsert_ts(
        &host,
        "/src/types.ts",
        "export type Foo = { x: number; y: string }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Foo } from './types'\n\
         defineProps<{ value: Foo }>()\n\
         </script>\n\
         <template><div /></template>\n",
    );

    let prov = host.provenance();
    let tracer_before = prov
        .dispatch_dep_signature_fact_tracer_emissions
        .load(Relaxed);
    let legacy_before = prov
        .dispatch_dep_signature_legacy_accumulator_emissions
        .load(Relaxed);

    let meta = host
        .get_component_meta("/src/Comp.vue")
        .expect("component meta resolves for the cross-file Foo fixture");

    // Sanity: the fixture must produce at least one prop row so we
    // know the projector + materialiser path ran (a fixture that
    // never reached `resolve_macro_payload` would not exercise the
    // dispatch reads and the counter delta would not be
    // discriminating).
    assert!(
        meta.props.iter().any(|p| p.name == "value"),
        "dispatch fixture must publish a `value` prop row so the \
         projector + materialiser dispatch reads ran — props={:?}",
        meta.props
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
    );

    let tracer_after = prov
        .dispatch_dep_signature_fact_tracer_emissions
        .load(Relaxed);
    let legacy_after = prov
        .dispatch_dep_signature_legacy_accumulator_emissions
        .load(Relaxed);

    // Dual-emit discrimination: BOTH counters must advance. If only
    // the tracer counter advances, the legacy drain at
    // `compute_component_meta_state_inner` loses dispatch facts from
    // `state.fact_versions` → the curated `fact_dep_signature` on
    // `ComponentMetaResultEntry` shrinks and warm-hit validation
    // misses transitive-type edits. If only the legacy counter
    // advances, the fact-tracer fan-out is missing → the
    // `read_set.finalise()` producer cannot replace
    // `state.fact_versions` without losing coverage.
    assert!(
        tracer_after > tracer_before,
        "dispatch dual-emit: the component-meta query MUST advance \
         `dispatch_dep_signature_fact_tracer_emissions` — the \
         dual-emit helper bumps the counter on every \
         `observe_fact_signature` call from the six in-scope \
         dispatch-read sites. tracer_before={tracer_before} \
         tracer_after={tracer_after}"
    );
    assert!(
        legacy_after > legacy_before,
        "dispatch dual-emit: the component-meta query MUST advance \
         `dispatch_dep_signature_legacy_accumulator_emissions` — \
         the dual-emit helper bumps the counter on every \
         `accumulate_dispatch_dep_signature` call from the six \
         in-scope dispatch-read sites. Without this advance, the \
         legacy drain at `compute_component_meta_state_inner` loses \
         dispatch facts from `state.fact_versions` and edits to a \
         transitive type file outside `parts.tracked_dependencies` \
         no longer invalidate the warm component-meta cache. \
         legacy_before={legacy_before} legacy_after={legacy_after}"
    );

    // Lockstep: each helper call bumps both counters once, so the
    // deltas must be equal. A discrepancy would mean a site bumped
    // one counter without bumping the other — a substrate bug
    // (e.g. an early-return between the two `fetch_add` calls in
    // the helper).
    let tracer_delta = tracer_after - tracer_before;
    let legacy_delta = legacy_after - legacy_before;
    assert_eq!(
        tracer_delta, legacy_delta,
        "dispatch dual-emit: per-call lockstep invariant — both \
         counters must advance by the same delta. \
         tracer_delta={tracer_delta} legacy_delta={legacy_delta}"
    );

    // The cross-file `defineProps<Foo>()` fixture drives at least
    // two projector reads (`resolve_macro_payload` and
    // `resolve_payload_surface`) plus the materialiser, so the
    // discriminating threshold is `>= 2`. Record the observed delta
    // as a stability sanity check.
    assert!(
        tracer_delta >= 2,
        "dispatch dual-emit: at least two dual-emit calls must fire \
         for this fixture (`resolve_macro_payload` + \
         `resolve_payload_surface`); tracer_delta={tracer_delta}"
    );
}
