//! The raw-analysis-snapshot scheduler lane joins template-analysis
//! inputs at the SAME generation its analysis snapshot was captured.
//!
//! `get_raw_analysis_snapshot`'s scheduler lane builds its snapshot
//! from the analysis slot, then lazily computes template analysis. The
//! template source must come from the generation the analysis snapshot
//! was read at: a source move landing between the two reads otherwise
//! compiles the NEW bytes' template, converts it with the OLD
//! snapshot's imports/bindings, and persists the mix into the
//! canonical-keyed `derived_raw_cache().raw_template_analysis` slot —
//! a slot with no content rail, cleared BY the very upsert that raced
//! the flight, and served as current by every subsequent
//! scheduler-lane read until the next source change.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;

const CANONICAL: &str = "/proj/Owner.vue";

/// Generation A: imports `Foo`, renders `<Foo />`.
const SOURCE_A: &str = "<script setup lang=\"ts\">\nimport Foo from './Foo.vue'\n</script>\n<template><Foo /></template>";

/// Generation B — the mid-flight move: imports `Bar`, renders
/// `<Bar />`. A coherent B conversion classifies `Bar` as
/// script-imported (`import_source` = `./Bar.vue`); a conversion
/// against the A snapshot's imports cannot — that classification gap
/// is the discriminating poison marker.
const SOURCE_B: &str = "<script setup lang=\"ts\">\nimport Bar from './Bar.vue'\n</script>\n<template><Bar /></template>";

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
    Arc::new(VerterHost::new(HostConfig::default(), workspace))
}

fn upsert(host: &VerterHost, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(source),
            file_language: verter_language::LanguageRegistry::global()
                .classify_static(CANONICAL)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
}

fn persisted_template(
    host: &VerterHost,
) -> Option<Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>> {
    host.derived_raw_cache().get(CANONICAL).and_then(|d| {
        d.raw_template_analysis()
            .map(|entry| Arc::clone(&entry.template))
    })
}

/// THE PIN: a template the flight can only derive from bytes its
/// snapshot was not built from must never persist into the rail-less
/// `derived_raw_cache` slot — and the returned snapshot must not mix
/// generations either.
#[test]
fn source_move_between_analysis_capture_and_template_join_never_persists_the_template() {
    let host = make_host();
    upsert(&host, SOURCE_A);
    assert!(
        persisted_template(&host).is_none(),
        "window sanity: template analysis is lazy — the upsert alone must not populate the slot",
    );

    // Land the move deterministically inside the capture→join window.
    let moved = Arc::new(AtomicBool::new(false));
    {
        let hook_host = Arc::clone(&host);
        let moved = Arc::clone(&moved);
        *host.raw_snapshot_template_join_seam_hook.lock() = Some(Arc::new(move || {
            if !moved.swap(true, Ordering::SeqCst) {
                upsert(&hook_host, SOURCE_B);
            }
        }));
    }
    let raced = host
        .get_raw_analysis_snapshot(CANONICAL)
        .expect("the scheduler lane must serve its captured snapshot");
    *host.raw_snapshot_template_join_seam_hook.lock() = None;
    assert!(
        moved.load(Ordering::SeqCst),
        "choreography sanity: the seam must have landed the move",
    );
    // The lane captured the A analysis — pin the capture so the race
    // below is provably capture-vs-join, not capture-vs-capture.
    assert!(
        raced.imports.iter().any(|i| i.source == "./Foo.vue"),
        "choreography sanity: the raced snapshot derives from generation A",
    );

    // THE PIN — nothing may persist: the only template this flight
    // could compute derives from the moved B bytes the A snapshot was
    // not built from, and the slot it would land in carries no content
    // rail and was already cleared by the very upsert that raced this
    // flight — a persist here is permanent poison, not a transient.
    if let Some(tpl) = persisted_template(&host) {
        let bar = tpl.components.iter().find(|c| c.name == "Bar");
        panic!(
            "poison persisted into derived_raw_cache().raw_template_analysis: \
             components={:?} (a coherent B conversion classifies Bar as \
             script-imported; got Bar import_source={:?}) — a template derived \
             from the moved bytes was published into the rail-less \
             canonical-keyed slot",
            tpl.components
                .iter()
                .map(|c| c.name.clone())
                .collect::<Vec<_>>(),
            bar.map(|c| c.import_source.clone()),
        );
    }

    // The raced caller's own snapshot must not mix generations either:
    // an A-analysis snapshot carrying a template compiled from B bytes.
    if let Some(tpl) = &raced.template {
        assert!(
            !tpl.components.iter().any(|c| c.name == "Bar"),
            "the returned snapshot mixes generations: A imports/bindings with \
             a template compiled from the moved B bytes",
        );
    }

    // Recovery: the quiescent next read computes the COHERENT B
    // template and persists it — the raced flight fails closed, it
    // does not wedge the slot.
    let recovered = host
        .get_raw_analysis_snapshot(CANONICAL)
        .expect("the quiescent follow-up read must serve");
    let tpl = recovered
        .template
        .as_ref()
        .expect("the quiescent follow-up read must compute the template");
    let bar = tpl
        .components
        .iter()
        .find(|c| c.name == "Bar")
        .expect("the B template renders <Bar />");
    assert_eq!(
        bar.import_source.as_deref(),
        Some("./Bar.vue"),
        "the coherent B conversion classifies Bar from the B snapshot's imports",
    );
    let persisted = persisted_template(&host).expect("the quiescent compute persists");
    assert!(
        persisted
            .components
            .iter()
            .any(|c| c.name == "Bar" && c.import_source.is_some()),
        "the persisted slot converges to the coherent B conversion",
    );
}

/// THE PIN (the `raw_template_analysis_for_file` lane — css-var-flow /
/// cross-file template reads): the lane's template inputs and its
/// analysis snapshot must join at one generation. A source move landing
/// between the lane's two scheduler reads otherwise compiles the OLD
/// bytes' template, converts it with the NEW snapshot's
/// imports/bindings (the inverse mix of the raw-snapshot lane: old
/// bytes, new conversion context), and persists the mix into the
/// rail-less canonical-keyed slot.
#[test]
fn source_move_inside_the_template_lane_window_never_persists_the_mix() {
    let host = make_host();
    upsert(&host, SOURCE_A);
    assert!(
        persisted_template(&host).is_none(),
        "window sanity: template analysis is lazy — the upsert alone must not populate the slot",
    );

    // Land the move deterministically inside the lane's window.
    let moved = Arc::new(AtomicBool::new(false));
    {
        let hook_host = Arc::clone(&host);
        let moved = Arc::clone(&moved);
        *host.raw_snapshot_template_join_seam_hook.lock() = Some(Arc::new(move || {
            if !moved.swap(true, Ordering::SeqCst) {
                upsert(&hook_host, SOURCE_B);
            }
        }));
    }
    let raced = host.raw_template_analysis_for_file(CANONICAL);
    *host.raw_snapshot_template_join_seam_hook.lock() = None;
    assert!(
        moved.load(Ordering::SeqCst),
        "choreography sanity: the seam must have landed the move",
    );

    // THE PIN — nothing may persist from the raced flight: the only
    // template it could compute mixes generations (bytes from one
    // read, conversion imports from the other), and the slot it would
    // land in was already cleared by the very upsert that raced it.
    if let Some(tpl) = persisted_template(&host) {
        panic!(
            "poison persisted into derived_raw_cache().raw_template_analysis: \
             components={:?} (import_sources={:?}) — a template whose bytes and \
             conversion context come from different generations was published \
             into the rail-less canonical-keyed slot",
            tpl.components
                .iter()
                .map(|c| c.name.clone())
                .collect::<Vec<_>>(),
            tpl.components
                .iter()
                .map(|c| c.import_source.clone())
                .collect::<Vec<_>>(),
        );
    }

    // The raced caller's own template must not mix generations either:
    // a coherent conversion of either generation classifies its single
    // component as script-imported; the mix cannot.
    if let Some(tpl) = &raced {
        assert!(
            !tpl.components.iter().any(|c| c.import_source.is_none()),
            "the returned template mixes generations: bytes from one read \
             converted against the other read's imports (components={:?})",
            tpl.components
                .iter()
                .map(|c| (c.name.clone(), c.import_source.clone()))
                .collect::<Vec<_>>(),
        );
    }

    // Recovery: the quiescent next read computes the COHERENT B
    // template and persists it — the raced flight fails closed.
    let recovered = host
        .raw_template_analysis_for_file(CANONICAL)
        .expect("the quiescent follow-up read must compute the template");
    let bar = recovered
        .components
        .iter()
        .find(|c| c.name == "Bar")
        .expect("the B template renders <Bar />");
    assert_eq!(
        bar.import_source.as_deref(),
        Some("./Bar.vue"),
        "the coherent B conversion classifies Bar from the B snapshot's imports",
    );
    assert!(
        persisted_template(&host).is_some(),
        "the quiescent compute persists",
    );
}

/// THE PIN (the `get_analysis` scheduler-hit lane): the lane's template
/// inputs and its analysis snapshot must join at one generation. The
/// lane captures template inputs at its source read; a source move
/// landing before its analysis read otherwise pairs the OLD bytes with
/// the NEW snapshot's imports/bindings and persists the mix through
/// `store_published: true`.
#[test]
fn source_move_inside_the_get_analysis_window_never_persists_the_mix() {
    let host = make_host();
    upsert(&host, SOURCE_A);
    assert!(
        persisted_template(&host).is_none(),
        "window sanity: template analysis is lazy — the upsert alone must not populate the slot",
    );

    let moved = Arc::new(AtomicBool::new(false));
    {
        let hook_host = Arc::clone(&host);
        let moved = Arc::clone(&moved);
        *host.raw_snapshot_template_join_seam_hook.lock() = Some(Arc::new(move || {
            if !moved.swap(true, Ordering::SeqCst) {
                upsert(&hook_host, SOURCE_B);
            }
        }));
    }
    let raced = host
        .get_analysis(CANONICAL)
        .expect("the scheduler-hit lane must serve");
    *host.raw_snapshot_template_join_seam_hook.lock() = None;
    assert!(
        moved.load(Ordering::SeqCst),
        "choreography sanity: the seam must have landed the move",
    );

    // THE PIN — nothing may persist from the raced flight.
    if let Some(tpl) = persisted_template(&host) {
        panic!(
            "poison persisted into derived_raw_cache().raw_template_analysis: \
             components={:?} (import_sources={:?}) — a template whose bytes and \
             conversion context come from different generations was published \
             into the rail-less canonical-keyed slot",
            tpl.components
                .iter()
                .map(|c| c.name.clone())
                .collect::<Vec<_>>(),
            tpl.components
                .iter()
                .map(|c| c.import_source.clone())
                .collect::<Vec<_>>(),
        );
    }

    // The raced snapshot must not mix generations either.
    if let Some(tpl) = &raced.template {
        assert!(
            !tpl.components.iter().any(|c| c.import_source.is_none()),
            "the returned snapshot mixes generations: template bytes from one \
             read converted against the other read's imports (components={:?})",
            tpl.components
                .iter()
                .map(|c| (c.name.clone(), c.import_source.clone()))
                .collect::<Vec<_>>(),
        );
    }

    // Recovery: the quiescent next read computes the COHERENT B
    // template and persists it.
    let recovered = host
        .get_analysis(CANONICAL)
        .expect("the quiescent follow-up read must serve");
    let tpl = recovered
        .template
        .as_ref()
        .expect("the quiescent follow-up read must compute the template");
    let bar = tpl
        .components
        .iter()
        .find(|c| c.name == "Bar")
        .expect("the B template renders <Bar />");
    assert_eq!(
        bar.import_source.as_deref(),
        Some("./Bar.vue"),
        "the coherent B conversion classifies Bar from the B snapshot's imports",
    );
    assert!(
        persisted_template(&host).is_some(),
        "the quiescent compute persists",
    );
}

/// No-over-decline negative control: the quiescent scheduler lane
/// still computes the template from its own coherent reads, persists
/// it (store-published live reads), and warm-serves the persisted
/// value on the next read.
#[test]
fn quiescent_scheduler_lane_still_persists_and_warm_serves_the_template() {
    let host = make_host();
    upsert(&host, SOURCE_A);

    let cold = host
        .get_raw_analysis_snapshot(CANONICAL)
        .expect("the scheduler lane must serve");
    let cold_tpl = cold
        .template
        .as_ref()
        .expect("the quiescent lane computes the template");
    let foo = cold_tpl
        .components
        .iter()
        .find(|c| c.name == "Foo")
        .expect("the A template renders <Foo />");
    assert_eq!(
        foo.import_source.as_deref(),
        Some("./Foo.vue"),
        "the coherent A conversion classifies Foo from the A snapshot's imports",
    );

    let persisted = persisted_template(&host)
        .expect("the quiescent compute must persist — live scheduler reads are store-published");
    assert!(
        Arc::ptr_eq(&persisted, cold_tpl),
        "the served template IS the persisted value",
    );

    let warm = host
        .get_raw_analysis_snapshot(CANONICAL)
        .expect("the warm read must serve");
    let warm_tpl = warm
        .template
        .as_ref()
        .expect("the warm read carries the template");
    assert!(
        Arc::ptr_eq(warm_tpl, &persisted),
        "the warm read serves the persisted slot value, not a recompute",
    );
}
