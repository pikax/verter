use super::{NativePropProjectionCache, ResolvedNativePropsOutcome};

#[test]
fn native_props_outcome_keeps_resolved_empty_distinct_from_miss() {
    let resolved = ResolvedNativePropsOutcome::Resolved(Vec::new());

    assert_ne!(resolved, ResolvedNativePropsOutcome::Miss);
    assert!(matches!(
        resolved,
        ResolvedNativePropsOutcome::Resolved(rows) if rows.is_empty()
    ));
}

#[test]
fn native_prop_projection_cache_is_a_dedicated_request_local_cache() {
    let cache = NativePropProjectionCache::default();

    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);
}
