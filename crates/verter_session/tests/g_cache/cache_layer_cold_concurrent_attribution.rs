//! Cold-concurrent joiner-accounting contract test (CRITICAL).
//!
//! 16 concurrent identical requests on the SAME canonical with NO
//! cache priming. The joiner-accounting contract requires:
//!
//! - Sum of `from_cache=true` flags == 15 (or 16 - see edge case
//!   below): one of the 16 is the cold winner, the rest dedup-join.
//! - Sum of `from_cache=false` flags == 1 (or 0): the winner
//!   recorded a cache miss before publishing.
//! - Sum of `record.store.cache_layers.component_meta.hits` across
//!   16 records == 15 (or 16): each joiner observed a hit on the
//!   final-result cache via the joiner-side bump performed by the
//!   cold path.
//! - Sum of `record.store.cache_layers.component_meta.misses` == 1
//!   (or 0): the winner observed a miss before populating the
//!   final-result cache.
//!
//! Edge case: under tight scheduling, the barrier-released winner
//! may publish its result to the cache before any joiner reaches
//! the warm-cache short-circuit. In that case, ALL 16 requests
//! could see the cache as warm via `try_with_resolution_cache_hit`
//! and be classified as joiners (`from_cache=true`, hits=1 each).
//! The acceptable split is therefore `misses in 0..=1` and
//! `hits in 15..=16`.
//!
//! Discriminating contract: pre-attribution trees and pre-joiner-bump
//! trees would BOTH fail the contract under cold concurrency:
//!
//! - Pre-attribution: no `cache_layers` field - would not compile.
//! - Pre-joiner-bump: 16 cold-concurrent requests all observe an
//!   empty cache via `try_with_resolution_cache_hit`, all bump
//!   `misses += 1` speculatively, then the cold-path singleflight
//!   identifies 15 as Followers but the speculative misses stay.
//!   Result: 16 misses, 0 hits, 0 from_cache=true. The discriminator
//!   used here (`misses in 0..=1` AND `hits in 15..=16`) rejects
//!   that pattern with full force.

use std::sync::{Arc, Barrier};
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
fn sixteen_cold_concurrent_identical_requests_attribute_per_joiner_contract() {
    let host = build_host();
    const N_THREADS: usize = 16;

    // Barrier so all 16 threads start the dispatch at the same
    // instant. Maximises the dedup-join window: under typical OS
    // scheduling, several joiners arrive while the winner is still
    // mid-compute, exercising the singleflight-Follower branch.
    // No primer: the cache starts empty, so the cold path runs
    // for the winner and the 15 joiners dedup-join its result.
    let barrier = Arc::new(Barrier::new(N_THREADS));

    let handles: Vec<_> = (0..N_THREADS)
        .map(|_| {
            let host_clone = Arc::clone(&host);
            let barrier_clone = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier_clone.wait();
                let (_a, _r, record) = AuditedRequest::builder()
                    .attach_to(host_clone)
                    .resolve_component_meta("/c.vue")
                    .expect("cold-concurrent resolve must succeed");
                record
            })
        })
        .collect();

    let mut records: Vec<verter_session::component_meta_audit::RequestAuditRecord> =
        handles.into_iter().map(|h| h.join().unwrap()).collect();
    records.sort_by_key(|r| r.request_id);

    let from_cache_true: usize = records.iter().filter(|r| r.from_cache).count();
    let from_cache_false: usize = records.iter().filter(|r| !r.from_cache).count();

    // Joiner-accounting under cold concurrency. Acceptable splits:
    //   - 15 joiners + 1 winner (typical case)
    //   - 16 joiners + 0 winners (winner published before any
    //     other thread reached `try_with_resolution_cache_hit`)
    assert!(
        from_cache_false <= 1,
        "joiner-accounting contract: at most ONE winner expected (got {} from_cache=false records out of {}). Records: {:?}",
        from_cache_false,
        N_THREADS,
        records.iter().map(|r| (r.request_id, r.from_cache)).collect::<Vec<_>>(),
    );
    assert!(
        from_cache_true >= N_THREADS - 1,
        "joiner-accounting contract: at least {} joiners expected (got {} from_cache=true records). Records: {:?}",
        N_THREADS - 1,
        from_cache_true,
        records.iter().map(|r| (r.request_id, r.from_cache)).collect::<Vec<_>>(),
    );
    assert_eq!(
        from_cache_true + from_cache_false,
        N_THREADS,
        "all 16 records must classify into joiner-or-winner",
    );

    let total_component_meta_hits: u64 = records
        .iter()
        .map(|r| r.store.cache_layers.component_meta.hits)
        .sum();
    let total_component_meta_misses: u64 = records
        .iter()
        .map(|r| r.store.cache_layers.component_meta.misses)
        .sum();

    // Discriminator: misses must be 0 or 1.
    assert!(
        total_component_meta_misses <= 1,
        "joiner-accounting cache attribution: sum of cache_layers.component_meta.misses must be at most 1 (got {}). Records: {:?}",
        total_component_meta_misses,
        records.iter().map(|r| (r.request_id, r.from_cache, r.store.cache_layers.component_meta.hits, r.store.cache_layers.component_meta.misses)).collect::<Vec<_>>(),
    );
    // Discriminator: hits must be 15 or 16.
    assert!(
        total_component_meta_hits >= (N_THREADS - 1) as u64,
        "joiner-accounting cache attribution: sum of cache_layers.component_meta.hits must be at least {} (got {}). Records: {:?}",
        N_THREADS - 1,
        total_component_meta_hits,
        records.iter().map(|r| (r.request_id, r.from_cache, r.store.cache_layers.component_meta.hits, r.store.cache_layers.component_meta.misses)).collect::<Vec<_>>(),
    );
    // Joint sanity: hits + misses across 16 == 16
    assert_eq!(
        total_component_meta_hits + total_component_meta_misses,
        N_THREADS as u64,
        "joiner-accounting: hits + misses across 16 records must equal exactly 16 (got {} + {} = {}). Records: {:?}",
        total_component_meta_hits,
        total_component_meta_misses,
        total_component_meta_hits + total_component_meta_misses,
        records.iter().map(|r| (r.request_id, r.from_cache, r.store.cache_layers.component_meta.hits, r.store.cache_layers.component_meta.misses)).collect::<Vec<_>>(),
    );

    // Per-record discriminator: each joiner observes exactly one
    // hit and zero misses. The winner (if present) observes one
    // miss and zero hits.
    for record in &records {
        let hits = record.store.cache_layers.component_meta.hits;
        let misses = record.store.cache_layers.component_meta.misses;
        if record.from_cache {
            // joiner
            assert_eq!(
                hits, 1,
                "joiner request {}: per-request attribution requires exactly 1 hit, got hits={} misses={}",
                record.request_id, hits, misses,
            );
            assert_eq!(
                misses, 0,
                "joiner request {}: per-request attribution requires 0 misses (joiner-side bump cancels speculative miss), got hits={} misses={}",
                record.request_id, hits, misses,
            );
        } else {
            // winner
            assert_eq!(
                hits, 0,
                "winner request {}: cold path observes no hit on ComponentMetaResultDb, got hits={} misses={}",
                record.request_id, hits, misses,
            );
            assert_eq!(
                misses, 1,
                "winner request {}: cold path observes exactly one miss on ComponentMetaResultDb, got hits={} misses={}",
                record.request_id, hits, misses,
            );
        }
    }
}
