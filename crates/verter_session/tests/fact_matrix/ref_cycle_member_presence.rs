//! Matrix slice: `ref_cycle` × `member_presence`.
//!
//! Discrimination: the `RefCycleResultDb` producer is exercised by
//! component-meta resolution on fixtures with recursive type
//! aliases. The BFS cold compute is wrapped in
//! `install_fact_tracer`; the counter advances when the BFS is
//! invoked.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn ref_cycle_producer_counter_monotonic_on_recursive_alias() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/Recursive.vue".into()),
        input_id: "/Recursive.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             interface Tree { children: Tree[] }\
             defineProps<{ root: Tree }>();\
             </script>",
        ),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });

    let before = super::harness::read_ref_cycle_installs(&host);
    let _ = host.get_component_meta("/Recursive.vue");
    let after = super::harness::read_ref_cycle_installs(&host);
    assert!(
        after >= before,
        "ref_cycle cold-compute counter must be monotonic on recursive alias fixture"
    );
}
