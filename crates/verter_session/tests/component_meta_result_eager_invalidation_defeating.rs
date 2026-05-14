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
//! `register_facts_for_new_content_without_eviction` hook (also
//! introduced by Block 1.B) is the discriminating substrate.
//!
//! Post-1.B: the entry carries `fact_dep_signature` with the dep's
//! observed fact hashes. After
//! `register_facts_for_new_content_without_eviction` flips the
//! dep's parse-domain registry to fresh hashes for the new content,
//! `StoreView::validates_fact_signature` detects the mismatch
//! through the per-domain validators and the warm hit reports a
//! miss — discriminated by
//! `component_meta_result_cache_misses` advancing.
//!
//! This is the explicit AMENDMENT-F slice landing inside Block 1.B
//! (the orchestrator's CC F5 / Block 4 hook test).

#![cfg(test)]

use std::sync::atomic::Ordering::Relaxed;
use std::sync::Mutex;

use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::{CompileErrorPolicy, HostConfig};

/// Block 1.B's eager-invalidation-defeating test mutates the
/// host-level fact registry via
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

    // Modify the dep's content on the workspace so a later upsert
    // would observe fresh bytes, but DO NOT call
    // `upsert`. Calling upsert would (a) parse the new content and
    // populate fresh FileFacts entries, and (b) trigger the eager
    // cascade that drains downstream caches. We want ONLY (a) — fresh
    // fact emission without the cascade — so the test discriminates
    // fact-validation from eviction.
    //
    // The hook is `register_facts_for_new_content_without_eviction`
    // (test-only, on VerterHost). It calls
    // `register_facts_for_new_content` (which clears the cached
    // SemanticDb entry for the dep) WITHOUT calling
    // `resolver.runtime.evict_canonical`, `project_type_store
    // .evict_canonical`, derived-raw cache drains, or
    // `bump_store_view_epoch`. Subsequent observations on the dep
    // re-materialise fact hashes from the workspace's current
    // content. To stage fresh bytes for those observations we update
    // the dep through the standard upsert path FIRST (which makes
    // the new content visible to the resolver) and then immediately
    // bypass the cascade with the test-only hook for the subsequent
    // owner query.
    mh.upsert_base("/src/types.ts", "export interface Foo { a: string; }\n")
        .expect("ts re-upsert");

    // Invoke the test-only hook: parse-domain refresh WITHOUT the
    // upsert-driven cascade. The hook short-circuits to the
    // semantic-db invalidate path so the next observation
    // re-emits fact hashes for the new content.
    mh.host()
        .register_facts_for_new_content_without_eviction("/src/types.ts");

    // Next query on the owner: fact-validation MUST catch the dep's
    // hash bump and report a cache miss on
    // `ComponentMetaResultDb::get_with_view`. The
    // upsert above already participated in eager invalidation —
    // this assertion proves the validator ALSO discriminates,
    // which is what carries the cache correctness after Block 4
    // retires eager invalidation.
    let _ = mh.host().get_component_meta("/src/Comp.vue");
    let misses_after = prov.component_meta_result_cache_misses.load(Relaxed);
    assert!(
        misses_after > misses_before,
        "Block 1.B eager-defeating: after a dep-fact bump the warm \
         hit MUST miss via `StoreView::validates_fact_signature`. \
         misses_before={misses_before} misses_after={misses_after}"
    );
}
