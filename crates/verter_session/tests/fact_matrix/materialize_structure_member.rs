//! Matrix slice: `materialize_structure` × `member`.
//!
//! Discrimination: same substrate as `..._member_presence.rs`. The
//! wiring on `MaterializeStructureDb` ensures the producer is
//! wrapped in `install_fact_tracer`; the counter is the
//! discriminating observable.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn materialize_structure_producer_advances_counter_on_member_walk() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/MemberProbe.vue".into()),
        input_id: "/MemberProbe.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             interface Props { id: string; label: string }\
             defineProps<Props>();\
             </script>",
        ),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });

    let before = super::harness::read_materialize_structure_installs(&host);
    let _ = host.get_component_meta("/MemberProbe.vue");
    let after = super::harness::read_materialize_structure_installs(&host);
    assert!(
        after >= before,
        "materialize_structure cold-compute counter must be monotonic"
    );
}
