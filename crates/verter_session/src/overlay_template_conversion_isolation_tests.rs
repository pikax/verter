//! Overlay conversion context never populates the base template slot.
//!
//! The lazy template-analysis computation converts compiled template
//! data using the caller snapshot's imports/bindings and persists the
//! result into the BASE canonical-keyed
//! `derived_raw_cache().raw_template_analysis` slot. An overlay-built
//! snapshot must therefore never reach the computation without its own
//! overlay source: pairing base scheduler bytes with overlay
//! imports/bindings converts base content in a session's conversion
//! context — and persisting that mix into the base slot violates
//! overlay isolation (overlay/session results never populate base
//! caches). The overlay caller is still SERVED a template — derived
//! from the overlay's own coherent source — return-only.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::session_view::OverlaidView;
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;

const CANONICAL: &str = "/proj/Owner.vue";

/// Base content: imports `Foo`, renders `<Foo />`.
const BASE_SOURCE: &str = "<script setup lang=\"ts\">\nimport Foo from './Foo.vue'\n</script>\n<template><Foo /></template>";

/// Overlay content: imports `Bar`, renders `<Bar />`. A coherent
/// overlay conversion classifies `Bar` as script-imported
/// (`import_source` = `./Bar.vue`); a base-bytes template converted
/// against the overlay snapshot's imports renders `<Foo />` with no
/// import classification — the discriminating poison marker.
const OVERLAY_SOURCE: &str = "<script setup lang=\"ts\">\nimport Bar from './Bar.vue'\n</script>\n<template><Bar /></template>";

fn make_host() -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.inject_file(
        "/proj/Foo.vue".to_string(),
        Arc::from("<template><div /></template>"),
    );
    workspace.inject_file(
        "/proj/Bar.vue".to_string(),
        Arc::from("<template><div /></template>"),
    );
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(BASE_SOURCE),
            file_language: verter_language::LanguageRegistry::global()
                .classify_static(CANONICAL)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("base upsert must succeed");
    host
}

fn overlay_view(host: &Arc<VerterHost>) -> OverlaidView {
    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(CANONICAL.to_string(), Arc::from(OVERLAY_SOURCE));
    OverlaidView::new(Arc::clone(host), overlays)
}

fn persisted_template(
    host: &VerterHost,
) -> Option<Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>> {
    host.derived_raw_cache().get(CANONICAL).and_then(|d| {
        d.raw_template_analysis()
            .map(|entry| Arc::clone(&entry.template))
    })
}

/// Pin a served template as the coherent OVERLAY conversion: overlay
/// bytes (`<Bar />`) converted with the overlay snapshot's imports
/// (`Bar` → `./Bar.vue`).
fn assert_overlay_coherent(
    tpl: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    lane: &str,
) {
    let bar = tpl
        .components
        .iter()
        .find(|c| c.name == "Bar")
        .unwrap_or_else(|| {
            panic!(
                "{lane}: the overlay caller's template must derive from the overlay \
             bytes (<Bar />); got components={:?} — a template compiled from \
             BASE bytes served to the overlay caller",
                tpl.components
                    .iter()
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>(),
            )
        });
    assert_eq!(
        bar.import_source.as_deref(),
        Some("./Bar.vue"),
        "{lane}: the overlay conversion classifies Bar from the overlay snapshot's imports",
    );
}

/// THE PIN (session `get_analysis` lane): the overlay caller is served
/// a template derived from its own overlay source, and the BASE
/// `derived_raw_cache` slot stays untouched.
#[test]
fn overlay_get_analysis_serves_the_overlay_template_and_never_populates_the_base_slot() {
    let host = make_host();
    assert!(
        persisted_template(&host).is_none(),
        "window sanity: template analysis is lazy — the base upsert alone must \
         not populate the slot",
    );
    let view = overlay_view(&host);

    let snapshot = host
        .get_analysis_via_view(CANONICAL, &view)
        .expect("the overlay arm must serve the overlay snapshot");

    // Conversion-context sanity: the snapshot is overlay-built.
    assert!(
        snapshot.imports.iter().any(|i| i.source == "./Bar.vue"),
        "choreography sanity: the served snapshot derives from the overlay source",
    );

    // THE PIN — overlay isolation: the base slot must stay untouched.
    if let Some(persisted) = persisted_template(&host) {
        panic!(
            "overlay conversion context populated the BASE \
             derived_raw_cache().raw_template_analysis slot: components={:?} \
             (import_sources={:?}) — overlay/session results never populate \
             base caches",
            persisted
                .components
                .iter()
                .map(|c| c.name.clone())
                .collect::<Vec<_>>(),
            persisted
                .components
                .iter()
                .map(|c| c.import_source.clone())
                .collect::<Vec<_>>(),
        );
    }

    // And the overlay caller is served a coherent overlay template.
    let tpl = snapshot
        .template
        .as_ref()
        .expect("the overlay caller must be served a template");
    assert_overlay_coherent(tpl, "session get_analysis");
}

/// THE PIN (component-meta overlay capture lane): same invariant
/// through `capture_component_meta_inputs_with_view` — the captured
/// snapshot carries an overlay-coherent template and the base slot
/// stays untouched.
#[test]
fn overlay_meta_capture_serves_the_overlay_template_and_never_populates_the_base_slot() {
    let host = make_host();
    assert!(
        persisted_template(&host).is_none(),
        "window sanity: template analysis is lazy — the base upsert alone must \
         not populate the slot",
    );
    let view = overlay_view(&host);

    let captured = host
        .capture_component_meta_inputs_with_view(CANONICAL, &view)
        .expect("the overlay capture lane must capture inputs");

    assert!(
        captured
            .snapshot
            .imports
            .iter()
            .any(|i| i.source == "./Bar.vue"),
        "choreography sanity: the captured snapshot derives from the overlay source",
    );

    // THE PIN — overlay isolation: the base slot must stay untouched.
    if let Some(persisted) = persisted_template(&host) {
        panic!(
            "overlay conversion context populated the BASE \
             derived_raw_cache().raw_template_analysis slot: components={:?} \
             (import_sources={:?}) — overlay/session results never populate \
             base caches",
            persisted
                .components
                .iter()
                .map(|c| c.name.clone())
                .collect::<Vec<_>>(),
            persisted
                .components
                .iter()
                .map(|c| c.import_source.clone())
                .collect::<Vec<_>>(),
        );
    }

    // And the captured snapshot carries a coherent overlay template.
    let tpl = captured
        .snapshot
        .template
        .as_ref()
        .expect("the overlay capture must carry a template");
    assert_overlay_coherent(tpl, "component-meta overlay capture");
}

/// No-over-decline negative control: the BASE lane still computes from
/// its own live scheduler reads, persists into the slot, and serves the
/// base-coherent conversion — the isolation keys on the conversion
/// context, not on declining persistence.
#[test]
fn base_get_analysis_still_persists_the_base_template() {
    let host = make_host();

    let snapshot = host
        .get_analysis(CANONICAL)
        .expect("the base lane must serve");
    let tpl = snapshot
        .template
        .as_ref()
        .expect("the base lane computes the template");
    let foo = tpl
        .components
        .iter()
        .find(|c| c.name == "Foo")
        .expect("the base template renders <Foo />");
    assert_eq!(
        foo.import_source.as_deref(),
        Some("./Foo.vue"),
        "the coherent base conversion classifies Foo from the base snapshot's imports",
    );
    assert!(
        persisted_template(&host).is_some(),
        "the base lane's store-published live reads persist the template",
    );
}
