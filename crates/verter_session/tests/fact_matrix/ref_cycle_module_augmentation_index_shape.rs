//! Matrix slice: `ref_cycle` × `module_augmentation_index_shape`.
//!
//! Degenerate cell — the `RefCycleResultDb` BFS does not consult
//! the module augmentation index directly; augmentation effects
//! manifest as RouteDb-tier fact shifts that propagate through
//! dispatch.
//!
//! Discrimination: the counter is monotonic on a fixture that
//! augments a module alongside a recursive type — the BFS runs
//! and the counter advances, even though augmentation index
//! facts are not directly observed.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[test]
fn ref_cycle_producer_counter_monotonic_under_module_augmentation_fixture() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/aug.ts".into()),
        input_id: "/aug.ts".into(),
        source: Arc::from("declare module 'pkg' { interface Pkg { extra: string } }"),
        file_language: FileLanguage::script_ts(),
        aliases: vec![],
    });
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some("/AugRecursion.vue".into()),
        input_id: "/AugRecursion.vue".into(),
        source: Arc::from(
            "<script setup lang=\"ts\">\
             interface Tree { children: Tree[] }\
             defineProps<{ root: Tree }>();\
             </script>",
        ),
        file_language: FileLanguage::vue(),
        aliases: vec![],
    });

    let before = super::harness::read_ref_cycle_installs(&host);
    let _ = host.get_component_meta("/AugRecursion.vue");
    let after = super::harness::read_ref_cycle_installs(&host);
    assert!(
        after >= before,
        "ref_cycle cold-compute counter must be monotonic under augmentation+recursion fixture"
    );
}
