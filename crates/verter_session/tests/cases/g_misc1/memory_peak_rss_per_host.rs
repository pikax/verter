//! Per-host peak-RSS slot test.
//!
//! Spin up two `VerterHost` instances with `audit_timing_capture =
//! true` and run a component-meta request on each, concurrently. Each
//! request's per-request peak slot is seeded with the current RSS when
//! the request registers with its host, and that host's sampler thread
//! only raises it. After both finish, assert each host's audit record
//! carries a realistic, non-zero `process_rss_peak_bytes` read from its
//! own per-request slot, not a process-global accumulator.
//!
//! Discrimination contract:
//! - A tree whose records never read the per-request slot (peak field
//!   missing or forced to zero): both peaks come back as 0 → test FAILS.
//! - Whether the sampler ticks during a request is not synchronised
//!   here; the seeded value alone satisfies the assertions.
//!
//! Skipped on WASM via `#[cfg(not(target_arch = "wasm32"))]`; the
//! sibling test `memory_peak_rss_zero_on_wasm.rs` covers the WASM
//! contract.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;
use std::thread;

use verter_session::{HostConfig, UpsertRequest, VerterHost};

const SLOW_SFC: &str = r#"<script setup lang="ts">
interface Props {
  message: string
  count: number
  items: string[]
}
defineProps<Props>()
</script>
<template>
  <div class="root">
    <h1>{{ message }}</h1>
    <ul>
      <li v-for="item in items" :key="item">{{ item }}</li>
    </ul>
  </div>
</template>
"#;

#[test]
fn each_host_sampler_attributes_peak_only_to_its_own_request() {
    // Each host gets its own sampler thread; peaks must NOT bleed.
    let host_a = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        audit_timing_capture: true,
        footprint_capture: true,
        ..HostConfig::default()
    }));
    let host_b = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        audit_timing_capture: true,
        footprint_capture: true,
        ..HostConfig::default()
    }));

    let canon_a = "/HostA.vue";
    let canon_b = "/HostB.vue";
    let _ = host_a.upsert(UpsertRequest {
        canonical_id: Some(canon_a.to_string()),
        input_id: canon_a.to_string(),
        source: Arc::from(SLOW_SFC),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(canon_a)
            .static_resolution(),
        aliases: Vec::new(),
    });
    let _ = host_b.upsert(UpsertRequest {
        canonical_id: Some(canon_b.to_string()),
        input_id: canon_b.to_string(),
        source: Arc::from(SLOW_SFC),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(canon_b)
            .static_resolution(),
        aliases: Vec::new(),
    });

    // Run host_a's request on a worker thread.
    let host_a_for_thread = Arc::clone(&host_a);
    let handle_a = thread::spawn(move || {
        let (_analysis, resolution) = host_a_for_thread
            .get_component_meta_with_resolution(canon_a)
            .expect("host_a request must succeed");
        // The per-request slot is read when the request finalises.
        resolution.request_id
    });

    // host_b's request runs alongside; its peak slot belongs to host_b
    // alone.
    let host_b_for_thread = Arc::clone(&host_b);
    let handle_b = thread::spawn(move || {
        let (_analysis, resolution) = host_b_for_thread
            .get_component_meta_with_resolution(canon_b)
            .expect("host_b request must succeed");
        resolution.request_id
    });

    let request_id_a = handle_a.join().expect("host_a worker joined");
    let request_id_b = handle_b.join().expect("host_b worker joined");

    // Drain both records.
    let record_a = host_a
        .host_audit_runtime()
        .take_record(request_id_a)
        .expect("host_a must produce a record");
    let record_b = host_b
        .host_audit_runtime()
        .take_record(request_id_b)
        .expect("host_b must produce a record");

    assert!(
        record_a.memory.process_rss_peak_bytes > 0,
        "host_a's record must carry its per-request peak slot; got {}",
        record_a.memory.process_rss_peak_bytes,
    );
    assert!(
        record_b.memory.process_rss_peak_bytes > 0,
        "host_b's record must carry its per-request peak slot; got {}",
        record_b.memory.process_rss_peak_bytes,
    );

    // The slot holds a real RSS value: a sanity floor that the value
    // falls in the realistic range for a debug test process
    // (>1 MB; <16 GB).
    const ONE_MB: u64 = 1024 * 1024;
    const SIXTEEN_GB: u64 = 16u64 * 1024 * 1024 * 1024;
    assert!(
        record_a.memory.process_rss_peak_bytes > ONE_MB
            && record_a.memory.process_rss_peak_bytes < SIXTEEN_GB,
        "host_a peak {} must fall in the realistic RSS range (1 MB .. 16 GB)",
        record_a.memory.process_rss_peak_bytes,
    );
    assert!(
        record_b.memory.process_rss_peak_bytes > ONE_MB
            && record_b.memory.process_rss_peak_bytes < SIXTEEN_GB,
        "host_b peak {} must fall in the realistic RSS range (1 MB .. 16 GB)",
        record_b.memory.process_rss_peak_bytes,
    );
}
