//! Behavioural slice: `memo_entry` × barrel-route dispatch.
//!
//! Retained OUTSIDE the cross-consumer completeness grid (see
//! `app_config_proof_observes_no_route_facts.rs`). Asserts RouteDb route
//! WALKING, unaffected by the deleted `EffectiveExportSet` arm.
//!
//! Discrimination: route-surface dispatches go through the memo.
//! `execute_cooperative`'s cold build is wrapped with
//! `install_fact_tracer`; the counter advances on a barrel-route
//! fixture.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn memo_entry_producer_counter_advances_on_barrel_route_dispatch() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/leaf.ts".into()),
        input_id: "/leaf.ts".into(),
        source: Arc::from("export interface BProps { id: string }"),
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
        canonical_id: Some("/MemoBarrel.vue".into()),
        input_id: "/MemoBarrel.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             import type { BProps } from './barrel';\
             defineProps<BProps>();\
             </script>",
        ),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });

    let before = super::harness::read_memo_entry_installs(&host);
    let _ = host.get_component_meta("/MemoBarrel.vue");
    let after = super::harness::read_memo_entry_installs(&host);
    assert!(
        after > before,
        "memo_entry cold-compute counter must advance on barrel-route dispatch"
    );
}
