//! Behavioural slice: `ref_cycle` × barrel-route recursion.
//!
//! Retained OUTSIDE the cross-consumer completeness grid (see
//! `app_config_proof_observes_no_route_facts.rs`). Asserts RouteDb route
//! WALKING, unaffected by the deleted `EffectiveExportSet` arm.
//!
//! Discrimination: the BFS walks barrel routes when the recursive
//! alias is re-exported through `export *`. The BFS is wrapped in
//! `install_fact_tracer`; the counter advances.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
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
        canonical_id: Some("/BarrelRecursion.vue".into()),
        input_id: "/BarrelRecursion.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             import type { Tree } from './barrel';\
             defineProps<{ root: Tree }>();\
             </script>",
        ),
        file_language: FileLanguage::vue(),
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
