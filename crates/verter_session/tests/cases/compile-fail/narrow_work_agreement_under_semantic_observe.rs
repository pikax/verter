//! The capture-available narrow-work proof. This fixture is compiled by the
//! observation contract crate, whose normal dependency carries
//! `verter_session` with `soak-support` and `semantic-observe` together;
//! trybuild copies that dependency line verbatim, so the full readers below
//! really exist here and the narrow readers read the same counters.

use std::sync::Arc;
use verter_session::{capture, FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_type_engine::semantic_query::FactStatus;

const OWNER: &str = "/src/component.vue";

fn main() {
    // Configuration signals observable through the dependency: capture is
    // compiled in and the broad observation counters are real storage.
    assert_eq!(
        capture::availability(),
        verter_audit::CaptureAvailability::Available
    );
    assert_eq!(
        std::mem::size_of::<verter_audit::observe::ObservedU64>(),
        8,
        "the broad observation counters are real storage in this profile"
    );

    let host = VerterHost::new_standalone(HostConfig {
        audit_enabled: true,
        footprint_capture: true,
        ..HostConfig::default()
    });
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(OWNER.into()),
            input_id: OWNER.into(),
            source: Arc::from("<script setup lang=\"ts\">defineProps<{ x: string }>()</script>"),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert");
    let (_, resolution) = host
        .get_component_meta_with_resolution(OWNER)
        .expect("captured answer");
    assert_eq!(resolution.fact_status(), FactStatus::Complete);
    let narrow = host.work_snapshot();
    let full = host.provenance_snapshot();
    assert!(
        narrow.resolver_node_cache_misses >= 1,
        "a cold captured request is really observed"
    );
    assert_eq!(
        narrow.resolver_node_cache_misses, full.resolver_node_cache_misses,
        "narrow and provenance readers share one counter authority"
    );
    assert_eq!(
        narrow.component_meta_resolved_state_recomputes,
        full.component_meta_resolved_state_recomputes
    );
    assert_eq!(
        narrow.component_meta_resolved_state_recomputes, 1,
        "support and capture never double count"
    );
    let graph = host.project_type_store().semantic_graph();
    assert_eq!(graph.work_snapshot().misses, graph.stats_snapshot().misses);
    assert_eq!(
        graph.work_snapshot().joined_waits,
        graph.stats_snapshot().joined_waits
    );
}
