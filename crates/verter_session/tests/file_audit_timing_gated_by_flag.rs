//! `audit_timing_capture` flag gates per-file `*_ms`. Two runs flipped
//! against each other:
//!
//! - flag OFF: every triggered FileAudit entry's `read_ms` /
//!   `parse_ms` / `lower_ms` is `None`.
//! - flag ON: at least one triggered FileAudit has `parse_ms = Some(>= 0.0)`
//!   (timing populated through the executor source-stage path).
//!
//! Discriminator: an implementation that ignores the flag would
//! populate `parse_ms = Some(...)` on the OFF run too — failing the
//! "all None when off" assertion.
//!
//! Fixture pattern: inject directly into the workspace, NOT via
//! `host.upsert`, so the parse happens INSIDE the audited request
//! window.

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
export interface Pet { name: string; age: number }
"#;

fn run(timing_on: bool) -> verter_audit::RequestAuditRecord {
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
fn timings_are_none_when_flag_off() {
    let record = run(false);
    let mut found_triggered = false;
    for f in &record.files {
        if f.triggered_by_this_request {
            found_triggered = true;
            assert!(
                f.read_ms.is_none(),
                "flag off: read_ms must be None on triggered entry; got {:?}",
                f,
            );
            assert!(
                f.parse_ms.is_none(),
                "flag off: parse_ms must be None on triggered entry; got {:?}",
                f,
            );
            assert!(
                f.lower_ms.is_none(),
                "flag off: lower_ms must be None on triggered entry; got {:?}",
                f,
            );
        }
    }
    assert!(
        found_triggered,
        "expected at least one triggered_by_this_request entry to verify None gating; \
         got files: {:?}",
        record.files,
    );
}

#[test]
fn timings_are_some_when_flag_on() {
    let record = run(true);
    let triggered_entries: Vec<_> = record
        .files
        .iter()
        .filter(|f| f.triggered_by_this_request)
        .collect();
    assert!(
        !triggered_entries.is_empty(),
        "flag on: expected at least one triggered_by_this_request entry; got files: {:?}",
        record.files,
    );
    let any_parse_some = triggered_entries.iter().any(|f| f.parse_ms.is_some());
    assert!(
        any_parse_some,
        "flag on: at least one triggered entry must report parse_ms = Some(...); \
         got: {:?}",
        triggered_entries,
    );
}

#[test]
fn timing_capture_without_audit_fails_validation() {
    let cfg = HostConfig {
        audit_enabled: false,
        audit_timing_capture: true,
        ..HostConfig::default()
    };
    let err = cfg.validate().expect_err("validation must fail");
    let msg = format!("{err}");
    assert!(
        msg.contains("audit_timing_capture"),
        "validation error must mention audit_timing_capture; got: {msg}",
    );
}
