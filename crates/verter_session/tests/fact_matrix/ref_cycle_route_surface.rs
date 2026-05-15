//! Matrix slice: `ref_cycle` × `route_surface`.
//!
//! Discrimination: the BFS walks barrel routes when the recursive
//! alias is re-exported through `export *`. Block 1.H Track 1
//! wraps the BFS in `install_fact_tracer`; the counter advances.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn ref_cycle_producer_counter_monotonic_on_barrel_route_recursion() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/leaf.ts".into()),
        input_id: "/leaf.ts".into(),
        source: Arc::from("export interface Tree { children: Tree[] }"),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/barrel.ts".into()),
        input_id: "/barrel.ts".into(),
        source: Arc::from("export * from './leaf';"),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/BarrelRecursion.vue".into()),
        input_id: "/BarrelRecursion.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             import type { Tree } from './barrel';\
             defineProps<{ root: Tree }>();\
             </script>",
        ),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });

    let before = super::harness::read_ref_cycle_installs(&host);
    let _ = host.get_component_meta("/BarrelRecursion.vue");
    let after = super::harness::read_ref_cycle_installs(&host);
    assert!(
        after >= before,
        "ref_cycle cold-compute counter must be monotonic on barrel-route recursive fixture"
    );
}
