//! `slot_binding_graph` dual-emit POSITIVE discriminator.
//!
//! The slot-binding-graph traversal in
//! `crates/verter_session/src/meta_resolve/slot_binding_graph.rs`
//! must emit dispatch facts through BOTH the legacy TLS accumulator
//! (`accumulate_dispatch_dep_signature`, drained at
//! `host_manage/component_meta_methods.rs:869` and folded into
//! `state.fact_versions`) AND the fact-tracer fan-out channel
//! (`observe_fact_signature` →
//! `resolver_core::resolver_context::observe_fan_out_borrowed`) at
//! the five `accumulate_dispatch_dep_signature` call sites —
//! otherwise retiring the legacy channel would lose coverage of
//! slot-binding-graph dispatch reads.
//!
//! Every legacy emission at the five sites
//! (`accumulate_lowered_node_carrier_deps`,
//! `slot_param_root_is_symbolic_only`,
//! `resolve_slot_bindings_graph_native`, and two sites in
//! `compute_bindings_via_graph`) is paired with a fact-tracer
//! fan-out via the file-local helper
//! `emit_slot_binding_graph_dispatch_facts`. Two provenance counters
//! discriminate the pairing:
//!
//! - `slot_binding_graph_fact_tracer_emissions` — bumped on every
//!   `observe_fact_signature` call from this file.
//! - `slot_binding_graph_legacy_accumulator_emissions` — bumped on
//!   every `accumulate_dispatch_dep_signature` call from this file.
//!
//! Both counters must advance in lockstep on a fixture that drives
//! the slot-binding-graph traversal end-to-end via
//! `VerterHost::get_component_meta` against a typed `defineSlots`
//! macro. The discrimination property: removing the
//! `observe_fact_signature` call from the helper would leave the
//! fact-tracer counter at zero while the legacy counter still
//! advances; this test would FAIL.

#![cfg(test)]

use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

fn build_test_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert_vue(host: &VerterHost, id: &str, src: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(src),
            file_language: FileLanguage::vue(),
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
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert ts");
}

#[test]
fn slot_binding_graph_traversal_emits_paired_fact_tracer_and_legacy_signatures() {
    let host = build_test_host();
    // Cross-file slot type with imports — exercises sites 1
    // (`accumulate_lowered_node_carrier_deps`), 3
    // (`ResolveMacroPayload`), and 4-5 (`ProjectPath` Shallow on
    // both slot-surface and per-binding param surface).
    upsert_ts(
        &host,
        "/src/slots.ts",
        "export interface Slots { default(props: { row: string; index: number }): any }",
    );
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Slots } from './slots'\n\
         defineSlots<Slots>()\n\
         </script>\n\
         <template><div /></template>\n",
    );

    // Baseline counters before the meta query.
    let prov = host.provenance();
    let tracer_before = prov.slot_binding_graph_fact_tracer_emissions.load(Relaxed);
    let legacy_before = prov
        .slot_binding_graph_legacy_accumulator_emissions
        .load(Relaxed);

    let meta = host
        .get_component_meta("/src/Comp.vue")
        .expect("component meta resolves for the slots fixture");

    // Sanity: the fixture produces at least one slot row so we know
    // the slot-binding-graph traversal ran (a fixture that never
    // entered `compute_bindings_via_graph` would not exercise sites
    // 4 / 5 and the counter delta would not be discriminating).
    assert!(
        meta.slots.iter().any(|s| s.name == "default"),
        "slot_binding_graph fixture must publish a `default` slot row \
         so the traversal exercised the dispatch reads — slots={:?}",
        meta.slots
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
    );

    let tracer_after = prov.slot_binding_graph_fact_tracer_emissions.load(Relaxed);
    let legacy_after = prov
        .slot_binding_graph_legacy_accumulator_emissions
        .load(Relaxed);

    // Dual-emit discrimination: BOTH counters must advance. If only
    // the legacy counter advances, the fact-tracer fan-out is
    // missing → the legacy channel cannot be retired without
    // losing coverage. If only the tracer counter advances, the
    // legacy drain at `compute_component_meta_state_inner` loses
    // slot-binding-graph facts from `state.fact_versions` → the
    // owner's `fact_dep_signature` shrinks.
    assert!(
        tracer_after > tracer_before,
        "the slot-binding-graph traversal MUST advance \
         `slot_binding_graph_fact_tracer_emissions` — the dual-emit \
         helper bumps the counter at every `observe_fact_signature` \
         call. tracer_before={tracer_before} tracer_after={tracer_after}"
    );
    assert!(
        legacy_after > legacy_before,
        "the slot-binding-graph traversal MUST advance \
         `slot_binding_graph_legacy_accumulator_emissions` — the \
         dual-emit helper bumps the counter at every \
         `accumulate_dispatch_dep_signature` call. \
         legacy_before={legacy_before} legacy_after={legacy_after}"
    );
    // Lockstep: each site bumps both counters once per call, so the
    // deltas must be equal. A discrepancy would mean a site bumped
    // one counter without bumping the other — a substrate bug
    // (e.g. an early-return between the two `fetch_add` calls in
    // the helper).
    let tracer_delta = tracer_after - tracer_before;
    let legacy_delta = legacy_after - legacy_before;
    assert_eq!(
        tracer_delta, legacy_delta,
        "per-call lockstep invariant — both counters must \
         advance by the same delta. tracer_delta={tracer_delta} \
         legacy_delta={legacy_delta}"
    );
    // The fixture exercises five dispatch reads (sites 1, 3, 4,
    // 5). Site 2 (`slot_param_root_is_symbolic_only`'s
    // `Instantiate` read) fires only on InstantiationRef bases and
    // is not reached by this fixture's interface payload. The
    // minimum discriminating threshold is `>= 1` (the assertions
    // above suffice); record the observed delta as a stability
    // sanity check.
    assert!(
        tracer_delta >= 1,
        "at least one fact-tracer emission must fire for \
         this fixture; tracer_delta={tracer_delta}"
    );
}
