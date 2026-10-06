//! Return-only reason propagation observed through a live session host's
//! fact tracer.

use crate::cache_runtime::singleflight::*;
use crate::cache_runtime::NonAdmissionReason;
use dashmap::DashMap;
use std::sync::Arc;

/// Typed `ReturnOnly` reasons control whether the returned value taints an
/// enclosing cold compute. Family-local non-retention must leave the outer
/// tracer cacheable; an unresolved derivation basis must refuse the outer
/// admission even though the winner still receives its valid value.
#[test]
fn return_only_reason_propagates_only_transitive_hazards() {
    use crate::types::HostConfig;
    use crate::VerterHost;
    use verter_session_query::facts::fact_read_set::FactReadSetFinalise;

    fn run(reason: NonAdmissionReason) -> FactReadSetFinalise {
        let host = VerterHost::new_standalone(HostConfig::default());
        let map: DashMap<u32, Arc<String>> = DashMap::new();
        let inflight: InflightTable<(u32, u8)> = InflightTable::default();
        let (value, finalise) = host.with_fact_tracer(
            verter_session_query::facts::fact_cache::AggregateBasisSeed::Unvouched,
            || {
                cooperative_admit_with_post_publish_by_flight_key(
                    &map,
                    &inflight,
                    9u32,
                    (9u32, 0u8),
                    |entry: &String| Some(entry.clone()),
                    || ComputeAdmission::<String, String>::ReturnOnly {
                        value: "winner-only".to_string(),
                        reason,
                    },
                    |entry: &String| entry.clone(),
                    |_entry: &String| true,
                    |_k: &u32, _e: &Arc<String>| {},
                    |_e: &Arc<String>, _k: &u32| {},
                    None,
                )
            },
        );
        assert_eq!(value.as_deref(), Some("winner-only"));
        finalise.finalise()
    }

    assert!(matches!(
        run(NonAdmissionReason::IntrinsicNonCacheable),
        FactReadSetFinalise::Ok(_)
    ));
    assert!(matches!(
        run(NonAdmissionReason::UnresolvedProvenance),
        FactReadSetFinalise::NonCacheable(_)
    ));
}
