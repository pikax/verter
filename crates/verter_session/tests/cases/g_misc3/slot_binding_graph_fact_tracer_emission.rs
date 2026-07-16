//! Behavioral coverage for slot-binding-graph dependency tracing.
//!
//! A typed `defineSlots` query must publish the graph's dispatch reads
//! to the request fact tracer, which is the sole cache dependency authority.

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
fn slot_binding_graph_traversal_emits_fact_tracer_signatures() {
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
    assert!(
        tracer_after > tracer_before,
        "the slot-binding-graph traversal MUST advance \
         `slot_binding_graph_fact_tracer_emissions`; the graph's \
         dispatch reads must reach the request fact tracer through \
         `observe_fact_signature`. tracer_before={tracer_before} \
         tracer_after={tracer_after}"
    );
    let tracer_delta = tracer_after - tracer_before;
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
