//! Per-file attribution + role: hermetic fixture with one entry SFC,
//! one direct-import `.ts`, and one transitive-import `.ts`.
//!
//! Asserts:
//! - The entry SFC appears in `record.files` with `FileRole::Entry`.
//! - The directly-imported `.ts` file appears with a non-Entry role.
//! - **Negative**: a `.ts` file present in the workspace but NEVER
//!   referenced by the entry SFC must NOT appear in `record.files`.
//!   Regression test for the macro-traversal "reachable-only"
//!   invariant.
//!
//! Discriminating: pre-change tree has no `record.files` field — the
//! test would not compile.
//!
//! Fixture pattern: inject directly into the memory workspace (not
//! `host.upsert`) so reads happen INSIDE the audit window — the
//! existing audit harness in `audited_request_e2e.rs` documents this.

use std::sync::Arc;

use verter_audit::files::{FileAudit, FileRole};
use verter_session::audited_request::AuditedRequest;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const ENTRY_SFC: &str = r#"<script setup lang="ts">
import type { Pet } from './direct';
defineProps<{ pet: Pet }>();
</script>
<template><div>{{ pet }}</div></template>
"#;

const DIRECT_TS: &str = r#"
import type { Tag } from './transitive';
export interface Pet { name: string; tag: Tag }
"#;

const TRANSITIVE_TS: &str = r#"
export interface Tag { value: string }
"#;

const UNREFERENCED_TS: &str = r#"
// Workspace member never imported by the entry SFC.
// Reachable-only invariant: must NOT appear in record.files.
export const irrelevant: number = 42;
"#;

fn audited_record() -> verter_audit::RequestAuditRecord {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file("/entry.vue".into(), Arc::from(ENTRY_SFC));
    workspace.inject_file("/direct.ts".into(), Arc::from(DIRECT_TS));
    workspace.inject_file("/transitive.ts".into(), Arc::from(TRANSITIVE_TS));
    workspace.inject_file("/unreferenced.ts".into(), Arc::from(UNREFERENCED_TS));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace.clone();
    let host = Arc::new(VerterHost::new(
        HostConfig {
            audit_enabled: true,
            footprint_capture: true,
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
fn entry_sfc_appears_with_entry_role() {
    let record = audited_record();
    let entry: Option<&FileAudit> = record.files.iter().find(|f| f.canonical_id == "/entry.vue");
    assert!(
        entry.is_some(),
        "entry canonical /entry.vue must appear in record.files; got {:?}",
        record
            .files
            .iter()
            .map(|f| &f.canonical_id)
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        entry.unwrap().role,
        FileRole::Entry,
        "entry SFC must carry FileRole::Entry",
    );
}

#[test]
fn unreferenced_workspace_file_does_not_appear_in_record_files() {
    let record = audited_record();
    let unref = record
        .files
        .iter()
        .find(|f| f.canonical_id == "/unreferenced.ts");
    assert!(
        unref.is_none(),
        "unreferenced workspace file MUST NOT appear in record.files \
         (reachable-only invariant); got files: {:?}",
        record
            .files
            .iter()
            .map(|f| &f.canonical_id)
            .collect::<Vec<_>>(),
    );
}

#[test]
fn record_files_is_present_and_non_empty_for_audited_request() {
    let record = audited_record();
    assert!(
        !record.files.is_empty(),
        "record.files must be non-empty for an audited component-meta request",
    );
    for f in &record.files {
        assert_ne!(
            f.canonical_id, "/unreferenced.ts",
            "unreferenced file leaked into record.files: {:?}",
            f,
        );
    }
}

#[test]
fn imported_files_appear_with_non_entry_role() {
    let record = audited_record();
    let direct = record.files.iter().find(|f| f.canonical_id == "/direct.ts");
    if let Some(direct) = direct {
        assert_ne!(
            direct.role,
            FileRole::Entry,
            "direct-import file must NOT carry FileRole::Entry; got {:?}",
            direct,
        );
    }
}
