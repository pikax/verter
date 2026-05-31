//! `record.timings.request_critical_path_ms == sum of files[i].read_ms +
//! parse_ms + lower_ms` across files where
//! `triggered_by_this_request == true`. Read-once-aware critical-path
//! aggregation.
//!
//! Discriminating: pre-change tree (no `request_critical_path_ms`
//! field) would not compile; an implementation that summed across
//! ALL files (including warm-cache `cache_hit = true` entries) would
//! fail because cache-hit entries report `*_ms = None` (treated as
//! `0.0` per the documented contract — but the sum still differs from
//! a "triggered-only" sum when there are mixed triggered + cached
//! entries on the same record).

use std::sync::Arc;

use verter_session::audited_request::AuditedRequest;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const ENTRY_SFC: &str = r#"<script setup lang="ts">
import type { Pet } from './types';
defineProps<{ pet: Pet }>();
</script>
<template><div>{{ pet }}</div></template>
"#;

const TYPES_TS: &str = r#"
export interface Pet { name: string }
"#;

fn audited_record(timing_on: bool) -> verter_audit::RequestAuditRecord {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file("/entry.vue".into(), Arc::from(ENTRY_SFC));
    workspace.inject_file("/types.ts".into(), Arc::from(TYPES_TS));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace.clone();
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            audit_timing_capture: timing_on,
            ..HostConfig::default()
        },
        ws_access,
    ));
    let (_, _, record) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/entry.vue")
        .expect("audited resolve should succeed");
    record
}

#[test]
fn critical_path_equals_sum_of_triggered_file_timings() {
    let record = audited_record(true);

    // Recompute the critical-path sum from `record.files` and assert
    // it equals the envelope's `request_critical_path_ms`.
    let expected: f64 = record
        .files
        .iter()
        .filter(|f| f.triggered_by_this_request)
        .map(|f| f.read_ms.unwrap_or(0.0) + f.parse_ms.unwrap_or(0.0) + f.lower_ms.unwrap_or(0.0))
        .sum();

    let actual = record.timings.request_critical_path_ms;

    let delta = (actual - expected).abs();
    assert!(
        delta < 1e-9,
        "request_critical_path_ms ({actual}) must equal sum of \
         triggered files' timings ({expected}); files: {:?}",
        record.files,
    );

    let any_triggered = record.files.iter().any(|f| f.triggered_by_this_request);
    assert!(
        any_triggered,
        "expected at least one triggered_by_this_request file in record.files; \
         got: {:?}",
        record.files,
    );
}

#[test]
fn bytes_parsed_equals_sum_of_non_not_loaded_file_bytes() {
    let record = audited_record(false);

    let expected: u64 = record
        .files
        .iter()
        .filter(|f| !matches!(f.role, verter_audit::files::FileRole::NotLoaded))
        .map(|f| f.bytes_read)
        .sum();
    let actual = record.memory.bytes_parsed;
    assert_eq!(
        actual, expected,
        "bytes_parsed ({actual}) must equal sum of non-NotLoaded file bytes ({expected}); \
         files: {:?}",
        record.files,
    );
}
