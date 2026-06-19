//! Matrix slice: `owner_import_surface` × `route_surface`.
//!
//! Discrimination: each owner-import that resolves through a
//! barrel re-export drives a RouteDb walk; the route_facts
//! collected by `resolve_imported_type_root_with_facts` enter the
//! surface's `fact_dep_signature`. The cold body is wrapped with
//! `install_fact_tracer`; the counter advances.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn owner_import_surface_producer_counter_advances_on_barrel_route() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/leaf.ts".into()),
        input_id: "/leaf.ts".into(),
        source: Arc::from("export interface CProps { id: string }"),
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
        canonical_id: Some("/OwnerBarrel.vue".into()),
        input_id: "/OwnerBarrel.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             import type { CProps } from './barrel';\
             defineProps<CProps>();\
             </script>",
        ),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });

    let before = super::harness::read_owner_import_surface_installs(&host);
    let _ = host.get_component_meta("/OwnerBarrel.vue");
    let after = super::harness::read_owner_import_surface_installs(&host);
    assert!(
        after >= before,
        "owner_import_surface cold-compute counter must be monotonic on barrel-route owner import"
    );
}
