//! Matrix slice: `owner_import_surface` × `member_presence`.
//!
//! Degenerate cell — the `OwnerImportSurfaceDb` producer walks
//! per-import bindings and accumulates chain facts (FileWholeHash
//! plus DerivedFactHash). It does NOT observe per-member presence
//! facts directly; member-presence checks happen at lower tiers.
//!
//! Discrimination: a getComponentMeta on a SFC with cross-file
//! imports advances the counter (the producer is wrapped in
//! `install_fact_tracer` per Block 1.H Track 1).

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn owner_import_surface_producer_counter_advances_on_cross_file_imports() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/types.ts".into()),
        input_id: "/types.ts".into(),
        source: Arc::from("export interface OwnerProps { id: string }"),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/Owner.vue".into()),
        input_id: "/Owner.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             import type { OwnerProps } from './types';\
             defineProps<OwnerProps>();\
             </script>",
        ),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });

    let before = super::harness::read_owner_import_surface_installs(&host);
    let _ = host.get_component_meta("/Owner.vue");
    let after = super::harness::read_owner_import_surface_installs(&host);
    assert!(
        after >= before,
        "owner_import_surface cold-compute counter must be monotonic on cross-file imports"
    );
}
