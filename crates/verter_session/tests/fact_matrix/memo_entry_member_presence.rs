//! Matrix slice: `memo_entry` × `member_presence`.
//!
//! Discrimination: the dispatch's `execute_cooperative` cold build
//! drives semantic-query memoization. The cold build is wrapped
//! with `install_fact_tracer`; the counter advances on every cold
//! execute.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn memo_entry_producer_counter_advances_on_cold_dispatch() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/MemoProbe.vue".into()),
        input_id: "/MemoProbe.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             interface Props { label: string }\
             defineProps<Props>();\
             </script>",
        ),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });

    let before = super::harness::read_memo_entry_installs(&host);
    let _ = host.get_component_meta("/MemoProbe.vue");
    let after = super::harness::read_memo_entry_installs(&host);
    assert!(
        after > before,
        "memo_entry cold-compute counter must advance on a fresh getComponentMeta call. before={before}, after={after}",
    );
}
