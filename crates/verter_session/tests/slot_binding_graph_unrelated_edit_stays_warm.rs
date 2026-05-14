//! Block 1.C — `slot_binding_graph` dual-emit NEGATIVE discriminator.
//!
//! Asserts that an unrelated dep edit (a sibling TS file that does
//! NOT participate in the slot-binding-graph traversal of the Vue
//! owner) does NOT advance the dual-emit counters on the second
//! call beyond what the Block 1.B warm-cache fast-path requires.
//!
//! Concretely: after the first `get_component_meta` call primes
//! `ComponentMetaResultDb` for the owner, the second call must hit
//! the warm cache (Block 1.B `component_meta_result_cache_hits`
//! advances) and MUST NOT re-run the slot-binding-graph traversal
//! (so neither dual-emit counter advances between calls 1 and 2).
//! Editing an UNRELATED file then forces a third call; the third
//! call must STILL warm-hit (the unrelated edit's facts are not in
//! the owner's `fact_dep_signature` so the per-domain validator
//! passes), and the dual-emit counters must stay flat.
//!
//! Discrimination property: a regression that eagerly invalidates
//! the warm cache on EVERY upsert (regardless of whether the
//! upserted file appears in the owner's `fact_dep_signature`) would
//! force the third call to cold-recompute the slot-binding-graph
//! and advance both counters; this test would FAIL.

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
fn unrelated_edit_does_not_advance_slot_binding_graph_emission_counters() {
    let host = build_test_host();

    // Owner imports `Slots` from `/src/slots.ts`. `/src/unrelated.ts`
    // is NOT referenced by the owner.
    upsert_ts(
        &host,
        "/src/slots.ts",
        "export interface Slots { default(props: { row: string }): any }",
    );
    upsert_ts(&host, "/src/unrelated.ts", "export const UNRELATED = 42;\n");
    upsert_vue(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Slots } from './slots'\n\
         defineSlots<Slots>()\n\
         </script>\n\
         <template><div /></template>\n",
    );

    let prov = host.provenance();

    // Prime call — cold compute populates the warm cache and
    // exercises the dual-emit helper.
    let _ = host
        .get_component_meta("/src/Comp.vue")
        .expect("first call resolves");
    let tracer_after_prime = prov.slot_binding_graph_fact_tracer_emissions.load(Relaxed);
    let legacy_after_prime = prov
        .slot_binding_graph_legacy_accumulator_emissions
        .load(Relaxed);
    assert!(
        tracer_after_prime >= 1 && legacy_after_prime >= 1,
        "Block 1.C negative: prime call must have advanced both \
         dual-emit counters (sanity floor — the positive test \
         covers the lockstep delta in detail). \
         tracer={tracer_after_prime} legacy={legacy_after_prime}"
    );

    let hits_before_warm = prov.component_meta_result_cache_hits.load(Relaxed);

    // Edit the unrelated file. This is an `upsert` (full host
    // path) — the eager invalidation cascade currently DOES drop
    // dependent warm entries, but `/src/unrelated.ts` is not a dep
    // of `/src/Comp.vue` so the cascade must leave the owner's
    // warm entry intact.
    upsert_ts(&host, "/src/unrelated.ts", "export const UNRELATED = 43;\n");

    // Second call after the unrelated edit. Block 1.B's
    // `ComponentMetaResultDb` warm-hit fast-path returns the cached
    // entry BEFORE installing a `with_fact_tracer` scope (see
    // `component_meta_entry.rs:109-118`). The slot-binding-graph
    // traversal does NOT run on a warm hit, so neither dual-emit
    // counter advances.
    let _ = host
        .get_component_meta("/src/Comp.vue")
        .expect("second call resolves via warm hit");

    let hits_after_unrelated = prov.component_meta_result_cache_hits.load(Relaxed);
    let tracer_after_unrelated = prov.slot_binding_graph_fact_tracer_emissions.load(Relaxed);
    let legacy_after_unrelated = prov
        .slot_binding_graph_legacy_accumulator_emissions
        .load(Relaxed);

    assert!(
        hits_after_unrelated > hits_before_warm,
        "Block 1.C negative sanity: the second call after an \
         unrelated edit MUST hit the warm `ComponentMetaResultDb` \
         cache. hits_before={hits_before_warm} \
         hits_after={hits_after_unrelated}"
    );
    assert_eq!(
        tracer_after_unrelated, tracer_after_prime,
        "Block 1.C: an unrelated edit MUST NOT advance \
         `slot_binding_graph_fact_tracer_emissions` — the warm-hit \
         fast-path returns before the slot-binding-graph traversal \
         runs. tracer_after_prime={tracer_after_prime} \
         tracer_after_unrelated={tracer_after_unrelated}"
    );
    assert_eq!(
        legacy_after_unrelated, legacy_after_prime,
        "Block 1.C: an unrelated edit MUST NOT advance \
         `slot_binding_graph_legacy_accumulator_emissions` — the \
         warm-hit fast-path returns before the slot-binding-graph \
         traversal runs. legacy_after_prime={legacy_after_prime} \
         legacy_after_unrelated={legacy_after_unrelated}"
    );
}
