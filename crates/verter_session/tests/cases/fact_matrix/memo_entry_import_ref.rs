//! Matrix slice: `memo_entry` × `import_ref`.
//!
//! Discrimination: cross-file imports drive Instantiate /
//! ProjectPath dispatches through the memo. The cold build is
//! wrapped with `install_fact_tracer`; the counter advances.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn memo_entry_producer_counter_advances_on_cross_file_dispatch() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/types.ts".into()),
        input_id: "/types.ts".into(),
        source: Arc::from("export interface Imported { id: string }"),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/MemoImport.vue".into()),
        input_id: "/MemoImport.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             import type { Imported } from './types';\
             defineProps<Imported>();\
             </script>",
        ),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });

    let before = super::harness::read_memo_entry_installs(&host);
    let _ = host.get_component_meta("/MemoImport.vue");
    let after = super::harness::read_memo_entry_installs(&host);
    assert!(
        after > before,
        "memo_entry cold-compute counter must advance on cross-file dispatch"
    );
}
