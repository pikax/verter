//! The capture-available narrow-work proof. This fixture is compiled by the
//! observation contract crate, whose normal dependency carries
//! `verter_session` with `soak-support` and `semantic-observe` together;
//! trybuild copies that dependency line verbatim, so the full readers below
//! really exist here and the narrow readers read the same counters.

use std::sync::Arc;
use verter_session::{capture, FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_type_engine::semantic_query::FactStatus;

const OWNER: &str = "/src/component.vue";
const NAMED: &str = "/src/named.ts";

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
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(NAMED.into()),
            input_id: NAMED.into(),
            source: Arc::from("export interface Box { label: string }"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("named upsert");
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
    // Graph half: the same cold named-symbol query and warm replay the
    // narrow profile proves (narrow_work_observations.rs), observed through
    // BOTH the narrow reader and the full stats snapshot. The two readers
    // load the same counters, so their equality alone proves nothing — the
    // exact deltas are the discriminator: a doubled increment under
    // semantic-observe fails the cold delta, a warm claim fails the
    // zero-delta replay. The cold count is the exact three memo claims this
    // query makes (the decl claim plus its recursive closure; misses count
    // claims, not producer executions).
    let graph = host.project_type_store().semantic_graph();
    let narrow_graph_before = graph.work_snapshot();
    let full_graph_before = graph.stats_snapshot();
    assert_eq!(
        narrow_graph_before.joined_waits,
        full_graph_before.joined_waits
    );
    assert_eq!(
        narrow_graph_before.joined_waits, 0,
        "sequential requests never join"
    );
    let cold = host
        .resolve_named_symbol(NAMED, "Box", None)
        .expect("cold named answer");
    let narrow_graph_cold = graph.work_snapshot();
    let full_graph_cold = graph.stats_snapshot();
    assert_eq!(
        narrow_graph_cold.misses - narrow_graph_before.misses,
        3,
        "the narrow reader counts the cold claims exactly once each"
    );
    assert_eq!(
        full_graph_cold.misses - full_graph_before.misses,
        3,
        "the full snapshot counts the same boundaries exactly once"
    );
    assert_eq!(narrow_graph_cold.joined_waits, 0);
    assert_eq!(full_graph_cold.joined_waits, 0);
    let warm = host
        .resolve_named_symbol(NAMED, "Box", None)
        .expect("warm named answer");
    assert_eq!(warm.id(), cold.id(), "a valid warm replay");
    assert_eq!(
        graph.work_snapshot(),
        narrow_graph_cold,
        "the warm replay adds no misses or joins through the narrow reader"
    );
    // The warm replay legitimately increments `hits`; the work boundary the
    // two readers share is misses and joins, compared field by field.
    let full_graph_warm = graph.stats_snapshot();
    assert_eq!(
        full_graph_warm.misses, full_graph_cold.misses,
        "the full snapshot agrees no warm claim missed"
    );
    assert_eq!(full_graph_warm.joined_waits, full_graph_cold.joined_waits);
}
