//! Characterisation test — fact-validation ALONE invalidates the
//! `ComponentMetaResultDb` warm hit when a referenced dep's facts
//! change. The owner-upsert path has no eager reverse-dependent
//! invalidation cascade, so a dependency edit never physically nukes
//! a downstream owner's warm result.
//!
//! The owner's warm entry carries `fact_dep_signature` with the dep's
//! observed fact hashes. After a plain `upsert` parses the dep's new
//! content through the scheduler and runs the dep's own-canonical
//! drain (so the resolver re-emits fresh facts) — without any
//! reverse-dependent cascade, so the owner's warm
//! `ComponentMetaResultDb` entry survives —
//! `StoreView::validates_fact_signature` detects the mismatch through
//! the per-domain validators and the warm hit reports a miss —
//! discriminated by `component_meta_result_cache_misses` advancing.
//!
//! Discrimination property: removing
//! `view.validates_fact_signature(&entry.fact_dep_signature)` from
//! `ComponentMetaResultDb::get_with_view` would make this test FAIL.
//! The warm entry survives the dep's parse + own-canonical drain
//! because there is no reverse-dependent cascade that would
//! `component_meta_results.invalidate_owner(/src/Comp.vue)`. Without
//! fact-validation, the next call would return the stale warm entry
//! as a hit; the asserted `_cache_misses` delta would not
//! materialise.

#![cfg(test)]

use std::sync::atomic::Ordering::Relaxed;

use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::{CompileErrorPolicy, HostConfig, UpsertRequest};

fn metahost() -> ComponentMetaHost {
    ComponentMetaHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

// The asserted `component_meta_result_cache_misses` /
// `component_meta_result_cache_hits` counters are read off THIS test's
// own `VerterHost`: `ComponentMetaHost::new_standalone` builds a fresh
// host with a fresh `Arc<MetaProvenance>` and an instance-local
// `MemoryWorkspace` (its own `next_sink_id` allocator). The deltas this
// test asserts are entirely host-local, so the test runs in parallel
// with no shared-process serialization.
#[test]
fn fact_validation_alone_invalidates_warm_hit_without_eviction() {
    let mh = metahost();

    // Setup: owner imports a type from a dep file.
    mh.upsert_base("/src/types.ts", "export interface Foo { a: number; }\n")
        .expect("ts upsert");
    mh.upsert_base(
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Foo } from './types';\n\
         defineProps<Foo>();\n\
         </script>\n",
    )
    .expect("vue upsert");

    // Prime — first call populates the cache entry with the
    // captured fact_dep_signature.
    let prime = mh.host().get_component_meta("/src/Comp.vue");
    assert!(prime.is_some(), "prime call must resolve");

    let prov = mh.host().provenance();
    let hits_warm_before = prov.component_meta_result_cache_hits.load(Relaxed);

    // Sanity: second identical call should be a warm hit (no edits).
    let _ = mh.host().get_component_meta("/src/Comp.vue");
    let hits_warm_after = prov.component_meta_result_cache_hits.load(Relaxed);
    assert!(
        hits_warm_after > hits_warm_before,
        "eager-defeating sanity: the warm path must hit before the \
         dep-fact bump (hits {hits_warm_before} → {hits_warm_after})"
    );

    let misses_before = prov.component_meta_result_cache_misses.load(Relaxed);

    // Refresh the dep's source through the plain `upsert`. The
    // owner-upsert path parses + drains the dep's OWN caches (so the
    // resolver re-emits fresh facts) but has no eager reverse-dependent
    // cascade — there is no path that would
    // `component_meta_results.invalidate_owner(/src/Comp.vue)` and
    // nuke the warm entry directly. That absence is what makes the
    // discrimination property hold — the warm entry survives the
    // dependency edit, and only fact-validation against the freshly
    // emitted dep facts can invalidate it.
    let req = UpsertRequest {
        canonical_id: Some("/src/types.ts".to_string()),
        input_id: "/src/types.ts".to_string(),
        source: std::sync::Arc::from("export interface Foo { a: string; }\n"),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static("/src/types.ts")
            .static_resolution(),
        aliases: Vec::new(),
    };
    let _ = mh.host().upsert(req).expect("ts re-upsert");

    // Next query on the owner: the warm entry survived the dependency
    // edit (no reverse-dependent cascade exists), so cache lookup hits
    // the entry. Fact-validation then runs against the live view, which
    // snapshots the dep's FRESH FileFacts (the OWN-canonical drain
    // forced a re-parse and refreshed FileArtifactStore entries).
    // The stored `fact_dep_signature` carries the dep's OLD parse
    // facts, so `StoreView::validates_fact_signature` returns false
    // and `get_with_view` reports a miss — the asserted
    // `_cache_misses` delta. Without fact-validation, the warm entry
    // would be served and this assertion would fail — the
    // discrimination property holds.
    let _ = mh.host().get_component_meta("/src/Comp.vue");
    let misses_after = prov.component_meta_result_cache_misses.load(Relaxed);
    assert!(
        misses_after > misses_before,
        "eager-defeating: after a dep-fact bump the warm hit MUST \
         miss via `StoreView::validates_fact_signature`. \
         misses_before={misses_before} misses_after={misses_after}"
    );
}
