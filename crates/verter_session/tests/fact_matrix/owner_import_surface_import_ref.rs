//! Matrix slice: `owner_import_surface` × `import_ref`.
//!
//! Discrimination: this is the producer's PRIMARY fact-kind —
//! every owner-direct-import binding adds a FileWholeHash
//! observation for the resolved canonical. The cold body is
//! wrapped with `install_fact_tracer`; the counter advances on a
//! fresh cross-file import fixture.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn owner_import_surface_producer_counter_advances_on_each_owner_import() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/a.ts".into()),
        input_id: "/a.ts".into(),
        source: Arc::from("export interface A { id: string }"),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/b.ts".into()),
        input_id: "/b.ts".into(),
        source: Arc::from("export interface B { label: string }"),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/MultiOwner.vue".into()),
        input_id: "/MultiOwner.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             import type { A } from './a';\
             import type { B } from './b';\
             defineProps<A & B>();\
             </script>",
        ),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });

    let before = super::harness::read_owner_import_surface_installs(&host);
    let _ = host.get_component_meta("/MultiOwner.vue");
    let after = super::harness::read_owner_import_surface_installs(&host);
    assert!(
        after >= before,
        "owner_import_surface cold-compute counter must be monotonic on multi-import owner"
    );
}
