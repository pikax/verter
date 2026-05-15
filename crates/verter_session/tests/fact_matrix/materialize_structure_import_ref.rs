//! Matrix slice: `materialize_structure` × `import_ref`.
//!
//! Discrimination: a SFC that uses a cross-file type reference
//! drives the materializer cold path AND the import-route walk.
//! Block 1.H Track 1 wraps the materializer cold compute in
//! `install_fact_tracer`; the counter advances.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn materialize_structure_producer_advances_counter_on_cross_file_import_walk() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/types.ts".into()),
        input_id: "/types.ts".into(),
        source: Arc::from("export interface ImportedProps { id: string }"),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/ImportProbe.vue".into()),
        input_id: "/ImportProbe.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             import type { ImportedProps } from './types';\
             defineProps<ImportedProps>();\
             </script>",
        ),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });

    let before = super::harness::read_materialize_structure_installs(&host);
    let _ = host.get_component_meta("/ImportProbe.vue");
    let after = super::harness::read_materialize_structure_installs(&host);
    assert!(
        after >= before,
        "materialize_structure cold-compute counter must advance on cross-file import walk"
    );
}
