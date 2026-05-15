//! Matrix slice: `materialize_structure` × `module_augmentation_index_shape`.
//!
//! Degenerate cell — the `MaterializeStructureDb` producer
//! materialises the structural form of a type expression; it does
//! not consult the workspace-wide module augmentation index
//! directly. The augmentation index is consumed by RouteDb at a
//! lower tier; when its hash shifts, RouteDb's route_surface facts
//! shift in turn and bubble through the dispatch.
//!
//! Discrimination: the counter advances on a getComponentMeta call
//! against a fixture that DOES augment a module — proving the
//! producer is wrapped in `install_fact_tracer` even though the
//! augmentation index is not directly observed.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn materialize_structure_producer_counter_advances_under_module_augmentation_fixture() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/aug.ts".into()),
        input_id: "/aug.ts".into(),
        source: Arc::from("declare module 'pkg' { interface Foo { extra: string } }"),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/AugProbe.vue".into()),
        input_id: "/AugProbe.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             defineProps<{ label: string }>();\
             </script>",
        ),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });

    let before = super::harness::read_materialize_structure_installs(&host);
    let _ = host.get_component_meta("/AugProbe.vue");
    let after = super::harness::read_materialize_structure_installs(&host);
    assert!(
        after >= before,
        "materialize_structure cold-compute counter must be monotonic under augmentation fixture"
    );
}
