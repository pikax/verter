//! Block 1A — `DerivedRawState.cached_fallthrough` NEGATIVE
//! discriminator: an unrelated edit must leave the warm cached
//! fallthrough entry alive.
//!
//! Pre-1A: substrate uses `Vec<FactVersionRef>` and the consumer
//! relies on a manual `.iter().all(view.validates(...))` pass over
//! the entire Vec. Conservative invalidation in `host_upsert`'s
//! eager cascade would drop the entry on unrelated edits; this
//! NEGATIVE assertion would fail. Post-1A: per-domain fast-path
//! validator preserves the warm entry under unrelated edits.

use std::sync::atomic::Ordering::Relaxed;

use verter_session::component_meta_host::ComponentMetaHost;
use verter_session::{CompileErrorPolicy, HostConfig};

fn metahost() -> ComponentMetaHost {
    ComponentMetaHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

#[test]
fn unrelated_edit_keeps_cached_fallthrough_warm() {
    let mh = metahost();
    mh.upsert_base(
        "/src/types.ts",
        "export interface RootProps { foo: number }\n",
    )
    .expect("types upsert");
    mh.upsert_base("/src/other.ts", "export interface Other { bar: number }\n")
        .expect("other upsert");
    mh.upsert_base(
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { RootProps } from './types';\n\
         defineProps<RootProps>();\n\
         </script>\n\
         <template><div /></template>\n",
    )
    .expect("vue upsert");

    // Prime — first resolve warms cached_fallthrough.
    let r1 = mh.host().resolve_fallthrough_surface("/src/Comp.vue");
    assert!(r1.is_some(), "prime resolve should succeed");

    // Capture the resolver-node provenance counters AFTER the prime
    // call so we can observe the cache disposition of the post-edit
    // resolve in isolation. The fallthrough request executor maps
    // `RequestSource::Cache` to `resolver_node_cache_hits` and any
    // other outcome (Flight/Fallback i.e. cold compute) to
    // `resolver_node_cache_misses` in `host_manage/fallthrough.rs`.
    let prov = mh.host().provenance();
    let hits_before = prov.resolver_node_cache_hits.load(Relaxed);
    let misses_before = prov.resolver_node_cache_misses.load(Relaxed);

    // Edit unrelated file; Comp.vue does NOT import it. The warm
    // cached_fallthrough entry's fact_versions does not reference
    // /src/other.ts, so the per-domain fast-path validator must NOT
    // invalidate it.
    mh.upsert_base("/src/other.ts", "export interface Other { bar: string }\n")
        .expect("other re-upsert");

    let r2 = mh.host().resolve_fallthrough_surface("/src/Comp.vue");
    assert!(
        r2.is_some(),
        "Block 1A: unrelated edit must NOT prevent fallthrough resolution from \
         succeeding for /src/Comp.vue"
    );

    let hits_after = prov.resolver_node_cache_hits.load(Relaxed);
    let misses_after = prov.resolver_node_cache_misses.load(Relaxed);

    // Discriminator: under warm-hit preservation the second resolve
    // must serve from the resolver-owned node cache (which itself
    // falls back to the `cached_fallthrough` wrapper on the warm
    // path). A regression that eager-invalidates the wrapper on
    // unrelated upserts would cold-recompute, advancing the misses
    // counter past `misses_before` — that is precisely the failure
    // mode this NEGATIVE test must catch.
    assert!(
        hits_after > hits_before,
        "Block 1A: unrelated edit must allow the second fallthrough resolve to \
         HIT the resolver-node cache (which is fed by the warm \
         `cached_fallthrough` wrapper). hits_before={hits_before} \
         hits_after={hits_after}"
    );
    assert_eq!(
        misses_after, misses_before,
        "Block 1A: misses must NOT advance after an unrelated edit. A miss \
         would signal the wrapper was invalidated and the second resolve \
         cold-recomputed despite the unrelated edit. \
         misses_before={misses_before} misses_after={misses_after}"
    );
}
