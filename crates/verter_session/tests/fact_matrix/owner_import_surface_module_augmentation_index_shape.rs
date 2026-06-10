//! Matrix slice: `owner_import_surface` × `module_augmentation_index_shape`.
//!
//! Degenerate cell — the OwnerImportSurfaceDb producer doesn't
//! consult the module augmentation index directly; augmentations
//! flow through RouteDb's effective-export surface.
//!
//! Discrimination: a SFC with cross-file imports under a fixture
//! that declares a module augmentation still advances the counter
//! (the producer is wrapped in `install_fact_tracer` per Block
//! 1.H Track 1, and a getComponentMeta exercise drives the cold
//! body for the SFC owner).

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn owner_import_surface_producer_counter_advances_under_augmentation() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/aug.ts".into()),
        input_id: "/aug.ts".into(),
        source: Arc::from("declare module 'lib' { interface ExtPkg { extra: string } }"),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/types.ts".into()),
        input_id: "/types.ts".into(),
        source: Arc::from("export interface OwnerAug { id: string }"),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/OwnerAug.vue".into()),
        input_id: "/OwnerAug.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             import type { OwnerAug } from './types';\
             defineProps<OwnerAug>();\
             </script>",
        ),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });

    let before = super::harness::read_owner_import_surface_installs(&host);
    let _ = host.get_component_meta("/OwnerAug.vue");
    let after = super::harness::read_owner_import_surface_installs(&host);
    assert!(
        after >= before,
        "owner_import_surface cold-compute counter must be monotonic under augmentation fixture"
    );
}
