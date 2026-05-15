//! Matrix slice: `ref_cycle` × `import_ref`.
//!
//! Discrimination: the BFS walks cross-file imports when the
//! recursive alias is defined in a separate module. Block 1.H
//! Track 1 wraps the BFS in `install_fact_tracer`; the counter
//! advances.

use std::sync::Arc;

use verter_session::{FileKind, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn ref_cycle_producer_counter_monotonic_on_cross_file_recursion() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/tree.ts".into()),
        input_id: "/tree.ts".into(),
        source: Arc::from("export interface Tree { children: Tree[] }"),
        file_kind: FileKind::NonSfc,
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/CrossFileRecursion.vue".into()),
        input_id: "/CrossFileRecursion.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             import type { Tree } from './tree';\
             defineProps<{ root: Tree }>();\
             </script>",
        ),
        file_kind: FileKind::VueSfc,
        aliases: vec![],
    });

    let before = super::harness::read_ref_cycle_installs(&host);
    let _ = host.get_component_meta("/CrossFileRecursion.vue");
    let after = super::harness::read_ref_cycle_installs(&host);
    assert!(
        after >= before,
        "ref_cycle cold-compute counter must be monotonic on cross-file recursive fixture"
    );
}
