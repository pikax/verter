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

#[test]
fn native_prop_projection_cache_partitions_same_symbol_by_owner() {
    let mut cache = NativePropProjectionCache::default();
    let module_key = (
        "/same.vue".to_string(),
        verter_type_expr::TopLevelOwnerId::module(0),
        "Props".to_string(),
    );
    let instance_key = (
        "/same.vue".to_string(),
        verter_type_expr::TopLevelOwnerId::instance(0),
        "Props".to_string(),
    );

    cache.insert(module_key.clone(), Some(Vec::new()));
    cache.insert(instance_key.clone(), None);

    assert_eq!(cache.len(), 2);
    assert!(matches!(
        cache.get(&module_key),
        Some(Some(rows)) if rows.is_empty()
    ));
    assert_eq!(cache.get(&instance_key), Some(&None));
}
