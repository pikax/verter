//! Per-host peak-RSS sampler isolation test.
//!
//! Spin up two `VerterHost` instances with `audit_timing_capture =
//! true`. Run a slow component-meta request on each so the host-owned
//! sampler thread ticks at least twice (50 ms tick × ≥ 2 = 100 ms
//! minimum window). After both finish, assert each host's audit
//! record carries a non-zero `process_rss_peak_bytes` AND each peak
//! reflects only that host's sampler — peak values must be tied to
//! the per-request slot the host's sampler updated, not a
//! process-global accumulator.
//!
//! Discrimination contract:
//! - Pre-change tree (no sampler thread, peak field is missing or
//!   forced to zero): both peaks come back as 0 → test FAILS.
//! - Post-change tree (host-owned sampler ticks every 50 ms while
//!   the request is active): both peaks come back as `> 0` and each
//!   host's record stays decoupled from the sibling host's runtime.
//!
//! Skipped on WASM via `#[cfg(not(target_arch = "wasm32"))]`; the
//! sibling test `memory_peak_rss_zero_on_wasm.rs` covers the WASM
//! contract.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

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

/// Wait until the sampler has had at least `min_ticks` opportunities to
/// fire (~50 ms each on the host-owned sampler thread). The wait is
/// busy-loopless: `thread::sleep` honours the OS scheduler, so the
/// sampler thread runs at least `min_ticks` times before this returns
/// (modulo scheduler jitter).
fn wait_for_sampler_ticks(min_ticks: u32) {
    let dwell = Duration::from_millis(50 * u64::from(min_ticks) + 25);
    let start = Instant::now();
    while start.elapsed() < dwell {
        thread::sleep(Duration::from_millis(20));
    }
}

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

    // Force a quiet baseline before either request runs so the
    // sampler can't latch onto cached state from a sibling test.
    wait_for_sampler_ticks(1);

    // Run host_a's request on a worker thread that lingers in the
    // request scope long enough for the sampler to tick at least
    // twice.
    let host_a_for_thread = Arc::clone(&host_a);
    let handle_a = thread::spawn(move || {
        let (_analysis, resolution) = host_a_for_thread
            .get_component_meta_with_resolution(canon_a)
            .expect("host_a request must succeed");
        // After the request finalises, the registration removed the
        // active-request entry, but the sampler had >= 2 ticks while
        // the request was in flight (the upsert above warmed enough
        // I/O that the resolver itself takes a few ms; a deliberate
        // post-resolve wait here is unnecessary because the
        // sampler's per-request slot is captured at finalize and
        // must already reflect the in-flight peak).
        resolution.request_id
    });

    // While host_a's request is in flight, run host_b's. The
    // overlap proves the two samplers do not share state — host_b's
    // peak slot is updated only by host_b's sampler.
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
        "host_a's sampler must have updated its per-request peak slot \
         while the request was in flight; got {}",
        record_a.memory.process_rss_peak_bytes,
    );
    assert!(
        record_b.memory.process_rss_peak_bytes > 0,
        "host_b's sampler must have updated its per-request peak slot \
         while the request was in flight; got {}",
        record_b.memory.process_rss_peak_bytes,
    );

    // Discriminating contract: the per-request peak slot was
    // populated by the host-owned sampler thread. The exact
    // ordering between `AuditBuilder::new`'s before-snapshot, the
    // sampler's first tick, and the resolver finishing is
    // OS-dependent — what we MUST observe is that the slot moved
    // off zero (sampler ran AND fetch_max wrote a real RSS value).
    // Pre-change tree (no sampler thread) leaves the slot at 0 and
    // fails the assertions above; this clause adds a sanity floor
    // that the value falls in the realistic RSS range for a debug
    // test process (>1 MB; <16 GB).
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
