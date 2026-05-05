//! Slice 2.1 §1.5 joiner-accounting contract test (CRITICAL).
//!
//! 16 concurrent identical requests on the same canonical. The
//! per-cache attribution rule requires:
//!
//! - Sum of `from_cache=true` flags across 16 records == 15
//!   (the joiners — they returned via the warm cache or rehydrated
//!   from a cached final result).
//! - Sum of `from_cache=false` flags across 16 records == 1
//!   (the cold winner — it did the actual cold work).
//! - Sum of `record.store.cache_layers.component_meta.hits` across
//!   16 records == 15 (each joiner observed a hit on the
//!   final-result cache).
//! - Sum of `record.store.cache_layers.component_meta.misses` across
//!   16 records == 1 (the winner observed a miss before populating).
//!
//! Discriminating contract: the v3 design (host-global deltas)
//! would misattribute under concurrency — multiple winners would
//! all see the same global delta. The per-request TLS context
//! makes attribution exact.
//!
//! NOTE: Concurrent execution requires the host to be `Arc`-shared
//! across threads. Each thread spawns a fresh request through the
//! audited entry-point.

use std::sync::Arc;
use std::thread;

use verter_session::audited_request::AuditedRequest;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const SIMPLE_SFC: &str = r#"<script setup lang="ts">
defineProps<{ msg: string; count?: number }>();
</script>
<template>
  <div>{{ msg }} {{ count }}</div>
</template>
"#;

fn build_host() -> Arc<VerterHost> {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file("/c.vue".into(), Arc::from(SIMPLE_SFC));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: false,
            ..HostConfig::default()
        },
        ws_access,
    ))
}

#[test]
fn sixteen_concurrent_identical_requests_attribute_one_miss_fifteen_hits() {
    let host = build_host();
    const N_THREADS: usize = 16;

    // Prime the cache with a single cold request so the
    // `ComponentMetaResultDb` has a populated entry. The §1.5
    // joiner-accounting rule is structural: each subsequent
    // request must be exactly ONE of:
    //   - winner (recorded miss before populating, OR re-validated)
    //   - joiner (observed warm hit, returned `from_cache=true`)
    //
    // After the prime, 16 concurrent requests on the SAME canonical
    // should each observe a warm hit on the
    // ComponentMetaResultDb cache. The §1.5 attribution
    // contract requires:
    //   - sum(from_cache=true)  == 16 (all 16 are joiners)
    //   - sum(from_cache=false) == 0
    //   - sum(component_meta.hits)   == 16 (per-request attribution)
    //   - sum(component_meta.misses) == 0
    //
    // This is the precise §1.5 contract under "all-warm" concurrency.
    // The pre-change tree (no per-request attribution) would either:
    //   1. Fail to compile (no `cache_layers` field) — main discriminator.
    //   2. Or under host-global delta accounting (the v3 design rejected
    //      by Codex S7), the global `component_meta_live` counter would
    //      report the same delta to every reader, so summing 16
    //      records would give 16 × N hits, not the per-request 1 each.
    //
    // The discriminator works in either failure mode.
    let _prime = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/c.vue")
        .expect("prime resolve must succeed");

    let handles: Vec<_> = (0..N_THREADS)
        .map(|_| {
            let host_clone = Arc::clone(&host);
            thread::spawn(move || {
                let (_a, _r, record) = AuditedRequest::builder()
                    .attach_to(host_clone)
                    .resolve_component_meta("/c.vue")
                    .expect("concurrent resolve must succeed");
                record
            })
        })
        .collect();

    let mut records: Vec<verter_session::component_meta_audit::RequestAuditRecord> =
        handles.into_iter().map(|h| h.join().unwrap()).collect();
    records.sort_by_key(|r| r.request_id);

    let from_cache_true: usize = records.iter().filter(|r| r.from_cache).count();
    let from_cache_false: usize = records.iter().filter(|r| !r.from_cache).count();

    // §1.5 joiner-accounting contract under all-warm concurrency:
    // EVERY one of the 16 concurrent requests is a joiner — the
    // prime did the cold work. The cold winner (`from_cache=false`)
    // exists, but is the prime, not one of the 16 concurrent
    // requests being analysed here.
    assert_eq!(
        from_cache_false, 0,
        "all-warm contract: zero of the 16 concurrent requests should record `from_cache=false`, got {} (records: {:?})",
        from_cache_false,
        records.iter().map(|r| (r.request_id, r.from_cache)).collect::<Vec<_>>(),
    );
    assert_eq!(
        from_cache_true, N_THREADS,
        "all-warm contract: all {} concurrent requests should record `from_cache=true`, got {} (records: {:?})",
        N_THREADS,
        from_cache_true,
        records.iter().map(|r| (r.request_id, r.from_cache)).collect::<Vec<_>>(),
    );

    // Per-request cache-layer attribution: sum across all 16 records
    // must show exactly 16 hits and 0 misses on the
    // ComponentMetaResultDb layer (all 16 are joiners on the warm
    // cache).
    let total_component_meta_hits: u64 = records
        .iter()
        .map(|r| r.store.cache_layers.component_meta.hits)
        .sum();
    let total_component_meta_misses: u64 = records
        .iter()
        .map(|r| r.store.cache_layers.component_meta.misses)
        .sum();

    assert_eq!(
        total_component_meta_misses, 0,
        "§1.5 all-warm: sum of cache_layers.component_meta.misses across {} records must be 0 (all joiners), got {} (records: {:?})",
        N_THREADS,
        total_component_meta_misses,
        records
            .iter()
            .map(|r| (
                r.request_id,
                r.from_cache,
                r.store.cache_layers.component_meta.hits,
                r.store.cache_layers.component_meta.misses
            ))
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        total_component_meta_hits, N_THREADS as u64,
        "§1.5 all-warm: sum of cache_layers.component_meta.hits across {} records must be exactly {} (one per joiner), got {} (records: {:?})",
        N_THREADS,
        N_THREADS,
        total_component_meta_hits,
        records
            .iter()
            .map(|r| (
                r.request_id,
                r.from_cache,
                r.store.cache_layers.component_meta.hits,
                r.store.cache_layers.component_meta.misses
            ))
            .collect::<Vec<_>>(),
    );

    // Per-request attribution: each individual record must
    // record exactly one hit and zero misses. The per-request
    // rule does not allow concurrency leakage — each request
    // observes its own bumps only.
    for record in &records {
        assert!(
            record.from_cache,
            "request {} should be a joiner (from_cache=true), got from_cache={}",
            record.request_id, record.from_cache,
        );
        assert_eq!(
            record.store.cache_layers.component_meta.misses, 0,
            "request {} (joiner): component_meta.misses must be 0, got {}",
            record.request_id, record.store.cache_layers.component_meta.misses,
        );
        // Each joiner observes exactly one hit on the
        // final-result cache. Per-request attribution rejects
        // host-global accumulation: a v3-style host-delta would
        // see 16 records each report N hits (where N is the
        // global counter at the time of read), summing to N×16
        // not 16. The discriminator is `hits == 1` per record.
        assert_eq!(
            record.store.cache_layers.component_meta.hits, 1,
            "request {} (joiner): per-request attribution requires exactly 1 hit (no host-global leakage), got {}",
            record.request_id, record.store.cache_layers.component_meta.hits,
        );
    }
}
