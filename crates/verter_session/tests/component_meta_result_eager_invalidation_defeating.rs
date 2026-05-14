//! Block 1.B — Block 4 characterisation companion. Asserts that
//! fact-validation ALONE — WITHOUT the eager-invalidation cascade in
//! `VerterHost::upsert` — invalidates the
//! `ComponentMetaResultDb` warm hit when a referenced dep's facts
//! change.
//!
//! Pre-1.B: warm-hit invalidation depended on the eager cascade
//! (`resolver.runtime.evict_canonical`,
//! `project_type_store.evict_canonical`, derived-raw cache drains,
//! `bump_store_view_epoch`). Without that cascade, the legacy
//! `dep_signature` whole-hash oracle COULD still detect a whole-hash
//! shift on the owner — but not on a dep file whose facts were
//! refreshed without an eviction. This test would fail pre-1.B
//! because the entry lacks any fact-precise signature; the
//! `register_facts_for_new_content_without_eviction` hook and
//! `upsert_without_dependent_eviction` helper (both introduced by
//! Block 1.B) are the discriminating substrate.
//!
//! Post-1.B: the entry carries `fact_dep_signature` with the dep's
//! observed fact hashes. After
//! `upsert_without_dependent_eviction` parses the dep's new content
//! through the scheduler and runs the OWN-canonical drain (so the
//! resolver re-emits fresh facts) but SKIPS the reverse-dep cascade
//! (so the owner's warm `ComponentMetaResultDb` entry survives),
//! `StoreView::validates_fact_signature` detects the mismatch through
//! the per-domain validators and the warm hit reports a miss —
//! discriminated by `component_meta_result_cache_misses` advancing.
//!
//! Discrimination property: removing
//! `view.validates_fact_signature(&entry.fact_dep_signature)` from
//! `ComponentMetaResultDb::get_with_view` would make this test FAIL.
//! The warm entry survives the dep's parse + own-cascade because the
//! dep's reverse-dep cascade — the path that would
//! `component_meta_results.invalidate_owner(/src/Comp.vue)` — is
//! deliberately skipped. Without fact-validation, the next call would
//! return the stale warm entry as a hit; the asserted
//! `_cache_misses` delta would not materialise.
//!
//! This is the explicit AMENDMENT-F slice landing inside Block 1.B
//! (the orchestrator's CC F5 / Block 4 hook test).

#![cfg(test)]

use std::sync::atomic::Ordering::Relaxed;
use std::sync::Mutex;

use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::{CompileErrorPolicy, FileKind, HostConfig, UpsertRequest};

/// Block 1.B's eager-invalidation-defeating test mutates the
/// host-level fact registry via
/// `upsert_without_dependent_eviction` +
/// `register_facts_for_new_content_without_eviction`, then asserts a
/// specific delta on the per-host
/// `component_meta_result_cache_misses` provenance counter. Because
/// the host is process-local (constructed inside the test) but the
/// `MemoryWorkspace` overlay storage may be shared across concurrent
/// tests through internal counters / file-id allocators, serialise
/// at this test's granularity so the counter delta assertion is
/// stable.
static EAGER_INVALIDATION_TEST_LOCK: Mutex<()> = Mutex::new(());

fn metahost() -> ComponentMetaHost {
    ComponentMetaHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

#[test]
fn fact_validation_alone_invalidates_warm_hit_without_eviction() {
    let _guard = EAGER_INVALIDATION_TEST_LOCK.lock().unwrap();

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
        "Block 1.B eager-defeating sanity: the warm path must hit \
         before the dep-fact bump (hits {hits_warm_before} → \
         {hits_warm_after})"
    );

    let misses_before = prov.component_meta_result_cache_misses.load(Relaxed);

    // Refresh the dep's source through the test-only helper that
    // parses + drains the dep's OWN caches (so the resolver re-emits
    // fresh facts) but SKIPS the reverse-dep cascade. The reverse-dep
    // cascade is the production path that would
    // `component_meta_results.invalidate_owner(/src/Comp.vue)` and
    // nuke the warm entry directly; omitting it is what makes the
    // discrimination property hold — the warm entry survives the
    // staging step, and only fact-validation against the freshly
    // emitted dep facts can invalidate it.
    let req = UpsertRequest {
        canonical_id: Some("/src/types.ts".to_string()),
        input_id: "/src/types.ts".to_string(),
        source: std::sync::Arc::from("export interface Foo { a: string; }\n"),
        file_kind: FileKind::from_path("/src/types.ts"),
        aliases: Vec::new(),
    };
    let _ = mh
        .host()
        .upsert_without_dependent_eviction(req)
        .expect("ts re-upsert without dependent eviction");

    // Invoke the test-only hook: parse-domain refresh hint without
    // the upsert-driven cascade. With
    // `upsert_without_dependent_eviction` above already invalidating
    // the dep's `semantic_db` entry via `register_facts_for_new_content`,
    // this call is redundant but kept to document the parse-domain
    // refresh contract end-to-end (Block 4 will retire the dep-cascade
    // entirely, at which point this hook becomes the sole driver).
    mh.host()
        .register_facts_for_new_content_without_eviction("/src/types.ts");

    // Next query on the owner: the warm entry survived the staging
    // step (no reverse-dep cascade ran), so cache lookup hits the
    // entry. Fact-validation then runs against the live view, which
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
        "Block 1.B eager-defeating: after a dep-fact bump the warm \
         hit MUST miss via `StoreView::validates_fact_signature`. \
         misses_before={misses_before} misses_after={misses_after}"
    );
}
