//! WASM: `process_rss_peak_bytes` MUST stay 0 regardless of flag
//! state.
//!
//! WASM is single-threaded; the host-owned sampler thread is gated
//! behind `#[cfg(not(target_arch = "wasm32"))]`. On WASM there is no
//! thread to spawn AND `current_process_rss()` returns 0 by design,
//! so `process_rss_peak_bytes` must come back as 0 even when both
//! `audit_enabled` and `audit_timing_capture` are true.
//!
//! Discrimination contract:
//! - Pre-change tree (sampler not WASM-gated): the sampler tries to
//!   spawn, panics or returns junk. Compilation under wasm32 fails
//!   OR the runtime panics at first request.
//! - Post-change tree (sampler WASM-gated): no thread is spawned,
//!   `current_process_rss()` returns 0, and the per-request peak
//!   slot stays at its initial 0 → record's `process_rss_peak_bytes`
//!   == 0.

#![cfg(target_arch = "wasm32")]

use std::sync::Arc;

use verter_session::{HostConfig, UpsertRequest, VerterHost};

const SFC: &str = r#"<script setup lang="ts">
defineProps<{ a: string }>()
</script>
<template><div>{{ a }}</div></template>
"#;

#[test]
fn wasm_peak_rss_is_zero_regardless_of_audit_timing_capture() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        audit_timing_capture: true, // even when ON
        footprint_capture: true,
        ..HostConfig::default()
    }));
    let canonical = "/wasm_peak.vue";
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(SFC),
        file_kind: verter_session::FileKind::from_path(canonical),
        aliases: Vec::new(),
    });
    let (_analysis, resolution) = host
        .get_component_meta_with_resolution(canonical)
        .expect("component-meta resolution must succeed for the fixture");
    let record = host
        .host_audit_runtime()
        .take_record(resolution.request_id)
        .expect("record must exist (audit_enabled=true)");
    assert_eq!(
        record.memory.process_rss_peak_bytes, 0,
        "WASM has no host-owned sampler thread (gated off via \
         `#[cfg(not(target_arch = \"wasm32\"))]`) and \
         `current_process_rss()` returns 0 on this target. The peak \
         slot must therefore stay at 0 even with \
         `audit_timing_capture` enabled."
    );
}
