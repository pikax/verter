//! Matrix slice: `memo_entry` × `module_augmentation_index_shape`.
//!
//! Discrimination: augmentation-index facts surface through
//! RouteDb route-surface shifts that the dispatch consults. Block
//! 1.H Track 1 wraps the cold dispatch with
//! `install_fact_tracer`; the counter advances under any cold
//! dispatch, including under augmentation fixtures.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn memo_entry_producer_counter_advances_under_augmentation_fixture() {
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
        canonical_id: Some("/MemoAug.vue".into()),
        input_id: "/MemoAug.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             defineProps<{ label: string }>();\
             </script>",
        ),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });

    let before = super::harness::read_memo_entry_installs(&host);
    let _ = host.get_component_meta("/MemoAug.vue");
    let after = super::harness::read_memo_entry_installs(&host);
    assert!(
        after > before,
        "memo_entry cold-compute counter must advance under augmentation fixture"
    );
}
