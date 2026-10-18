use verter_session_query::retention::heap_size::OwnedValue;
use verter_session_query::retention::ChargeClass;

use crate::meta::MetaProject;
use crate::types::HostConfig;
use crate::VerterHost;

/// A cold delivery hands out the very cell the final-result cache publishes
/// and a warm delivery shares it: the cache retains the analysis while it
/// keeps the entry, and once the cache lets go every delivery still holding
/// the analysis keeps it charged, pinned, rather than holding an alias the
/// cache's charge no longer covers.
#[test]
fn a_delivered_analysis_shares_the_cached_cell_and_keeps_its_charge_past_the_cache() {
    let project = MetaProject::new(VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    }));
    project
        .upsert_base(
            "/src/Comp.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ title: string }>()\n</script>\n<template><div>{{ title }}</div></template>\n",
        )
        .unwrap();
    let session = project.open_session_batch().unwrap();
    let cold = session
        .get_component_meta("/src/Comp.vue")
        .unwrap()
        .expect("a cold delivery");
    let warm = session
        .get_component_meta("/src/Comp.vue")
        .unwrap()
        .expect("a warm delivery");
    assert!(
        OwnedValue::ptr_eq(cold.shared_analysis(), warm.shared_analysis()),
        "the warm delivery shares the cell the cold one published"
    );
    assert_eq!(
        warm.shared_analysis().charge_class(),
        Some(ChargeClass::Retained),
        "the cache retains the analysis while it keeps the entry"
    );
    assert_eq!(warm.analysis().props.len(), 1);

    project
        .host()
        .project_type_store()
        .component_meta_results()
        .invalidate_all();
    assert_eq!(
        warm.shared_analysis().charge_class(),
        Some(ChargeClass::Pinned),
        "the cache let go: the deliveries keep the analysis charged"
    );
    assert_eq!(cold.analysis().props[0].name, "title");
}
