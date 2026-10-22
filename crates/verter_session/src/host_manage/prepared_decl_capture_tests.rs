//! Capture binding for prepared-declaration bundle materialization: a cold
//! build and a content-pinned read attribute to the capture bound at entry,
//! never to an ambient request installed later.

use super::*;
use verter_audit::observe::Captured;
use verter_type_engine::request_context::{RequestContext, RequestContextGuard};
use verter_type_engine::request_footprint::RequestFootprintAccumulator;
use verter_type_engine::request_observers::CaptureTargets;

#[test]
fn cold_bundle_and_content_pinned_reads_keep_the_bound_capture() {
    for (canonical, declaration_file) in [("/bound.ts", false), ("/bound.d.ts", true)] {
        let host = VerterHost::new_standalone(crate::HostConfig::default());
        let _update = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: canonical.to_string(),
                source: Arc::from("export interface Bound { value: string }"),
                file_language: verter_language::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .expect("fixture source");
        let bound_accumulator = Arc::new(RequestFootprintAccumulator::new());
        let bound = RequestContext::new(
            1,
            Arc::from(canonical),
            true,
            Some(Arc::clone(&bound_accumulator)),
        );
        let _bound_guard = RequestContextGuard::install(Arc::clone(&bound));
        let observation = CaptureTargets::installed::<Captured>();
        let ambient_accumulator = Arc::new(RequestFootprintAccumulator::new());
        let ambient = RequestContext::new(
            2,
            Arc::from("/ambient.ts"),
            true,
            Some(Arc::clone(&ambient_accumulator)),
        );
        let _ambient_guard = RequestContextGuard::install(Arc::clone(&ambient));

        let built = if declaration_file {
            let view = host.resolver_store_view().into_owned_view();
            host.materialize_prepared_decl_bundle_from_routed_shallow::<Captured>(
                &observation,
                &view,
                canonical,
            )
        } else {
            host.materialize_prepared_decl_bundle::<Captured>(&observation, canonical)
        };
        assert!(
            matches!(built, Some(BundleMaterialization::Built { .. })),
            "fixture must exercise a cold bundle build: {canonical}"
        );
        assert_eq!(
            bound_accumulator.drain().materializations.len(),
            1,
            "cold materialization belongs to the bound capture: {canonical}"
        );
        assert!(
            ambient_accumulator.drain().materializations.is_empty(),
            "cold materialization must not adopt the ambient capture: {canonical}"
        );

        let bound_before = bound.cache_counters.indexed.snapshot();
        let ambient_before = ambient.cache_counters.indexed.snapshot();
        assert!(host
            .current_content_pinned_indexed_in::<Captured>(&observation, canonical)
            .is_some());
        let bound_after = bound.cache_counters.indexed.snapshot();
        assert_eq!(
            bound_after.0 - bound_before.0,
            1,
            "the typed content-pinned read records its bound hit"
        );
        assert_eq!(
            ambient.cache_counters.indexed.snapshot(),
            ambient_before,
            "the typed content-pinned read must not adopt the ambient capture"
        );
    }
}
