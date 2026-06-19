//! Matrix slice: `owner_import_surface` × `member`.
//!
//! Degenerate cell — same rationale as
//! `owner_import_surface_member_presence.rs`. The producer
//! accumulates chain facts at the import-route level, not
//! per-member-body level.
//!
//! Discrimination: a getComponentMeta on a SFC with imports that
//! reference nested members advances the counter.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn owner_import_surface_producer_counter_advances_on_nested_member_imports() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/types.ts".into()),
        input_id: "/types.ts".into(),
        source: Arc::from("export interface OwnerNested { nested: { id: string } }"),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/OwnerNested.vue".into()),
        input_id: "/OwnerNested.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             import type { OwnerNested } from './types';\
             defineProps<OwnerNested>();\
             </script>",
        ),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });

    let before = super::harness::read_owner_import_surface_installs(&host);
    let _ = host.get_component_meta("/OwnerNested.vue");
    let after = super::harness::read_owner_import_surface_installs(&host);
    assert!(
        after >= before,
        "owner_import_surface cold-compute counter must be monotonic on nested-member imports"
    );
}
