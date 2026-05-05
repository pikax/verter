//! Sampler thread MUST NOT run when `audit_timing_capture = false`.
//!
//! When the flag is off (or default), the host-owned sampler thread
//! is never spawned. Active requests therefore observe
//! `process_rss_peak_bytes == 0` on their finalised record, even when
//! `audit_enabled = true`.
//!
//! Discrimination contract:
//! - Pre-change tree (no flag, sampler always runs): peak comes back
//!   `> 0` → test FAILS.
//! - Post-change tree (sampler gated by `audit_timing_capture`): peak
//!   stays at 0 because no thread populated the per-request slot.
//!
//! Skipped on WASM via `#[cfg(not(target_arch = "wasm32"))]`; the
//! WASM contract is identical (always zero) but the WASM-specific
//! test pins the unconditional contract.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use verter_session::{HostConfig, UpsertRequest, VerterHost};

const SFC: &str = r#"<script setup lang="ts">
defineProps<{ message: string }>()
</script>
<template><div>{{ message }}</div></template>
"#;

#[test]
fn audit_timing_capture_disabled_keeps_peak_rss_zero() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        // audit_timing_capture: false (default).
        footprint_capture: true,
        ..HostConfig::default()
    }));
    let canonical = "/no_timing.vue";
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
        "audit_timing_capture is OFF — the host-owned sampler thread \
         must NOT spawn, so no fetch_max ever lands on the per-request \
         peak slot. Pre-change tree (sampler always runs) would show a \
         non-zero value here and fail this test."
    );
}
