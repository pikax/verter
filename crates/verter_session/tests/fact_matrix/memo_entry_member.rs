//! Matrix slice: `memo_entry` × `member`.
//!
//! Discrimination: per-member walks dispatch through the memo. Block
//! 1.H Track 1 wraps `execute_cooperative`'s cold build with
//! `install_fact_tracer`; the counter advances.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn memo_entry_producer_counter_advances_on_member_dispatch() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/MemoMember.vue".into()),
        input_id: "/MemoMember.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             interface Props { foo: { bar: { baz: string } } }\
             defineProps<Props>();\
             </script>",
        ),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });

    let before = super::harness::read_memo_entry_installs(&host);
    let _ = host.get_component_meta("/MemoMember.vue");
    let after = super::harness::read_memo_entry_installs(&host);
    assert!(
        after > before,
        "memo_entry cold-compute counter must advance on nested-member dispatch"
    );
}
