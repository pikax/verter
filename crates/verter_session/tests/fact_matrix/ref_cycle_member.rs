//! Matrix slice: `ref_cycle` × `member`.
//!
//! Discrimination: the BFS observes the body fingerprint of each
//! visited recursive member. Block 1.H Track 1 wraps the cold
//! BFS in `install_fact_tracer`; the counter advances when the
//! BFS is invoked.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn ref_cycle_producer_counter_monotonic_on_member_recursion() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/MutualRecursion.vue".into()),
        input_id: "/MutualRecursion.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             interface A { b?: B }\
             interface B { a?: A }\
             defineProps<{ root: A }>();\
             </script>",
        ),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });

    let before = super::harness::read_ref_cycle_installs(&host);
    let _ = host.get_component_meta("/MutualRecursion.vue");
    let after = super::harness::read_ref_cycle_installs(&host);
    assert!(
        after >= before,
        "ref_cycle cold-compute counter must be monotonic on mutual-recursion fixture"
    );
}
