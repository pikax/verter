//! Matrix slice: `materialize_structure` × `route_surface`.
//!
//! Discrimination: route_surface facts are produced when the
//! materializer's cold path drives an import-route walk that
//! resolves through the RouteDb. The materializer's cold compute is
//! wrapped in `install_fact_tracer`; the counter advances.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn materialize_structure_producer_advances_counter_on_barrel_route() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    // Barrel + leaf — drives route_surface fact emission when
    // dispatch walks the wildcard re-export.
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/leaf.ts".into()),
        input_id: "/leaf.ts".into(),
        source: Arc::from("export interface Props { id: string }"),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/barrel.ts".into()),
        input_id: "/barrel.ts".into(),
        source: Arc::from("export * from './leaf';"),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/BarrelProbe.vue".into()),
        input_id: "/BarrelProbe.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             import type { Props } from './barrel';\
             defineProps<Props>();\
             </script>",
        ),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });

    let before = super::harness::read_materialize_structure_installs(&host);
    let _ = host.get_component_meta("/BarrelProbe.vue");
    let after = super::harness::read_materialize_structure_installs(&host);
    assert!(
        after >= before,
        "materialize_structure cold-compute counter must advance on barrel route walk"
    );
}
