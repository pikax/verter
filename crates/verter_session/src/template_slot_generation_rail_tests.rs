//! The derived template slot is valid only for the source generation
//! its template derives from.
//!
//! The lazy template-analysis computation persists into the
//! canonical-keyed `derived_raw_cache().raw_template_analysis` slot.
//! Capture authority (`store_published`) proves the inputs were
//! coherent live reads when CAPTURED — it cannot prove the slot is
//! still current at PERSIST time: an upsert landing between the
//! coherent capture and the persist clears the slot, and the late
//! persist then repopulates it with a template describing the
//! superseded bytes. The slot therefore carries the scheduler node
//! generation of the source the template derives from, and every
//! reader accepts the entry only at its own snapshot's generation —
//! read-side authoritative, covering every persist path.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;

const CANONICAL: &str = "/proj/Owner.vue";

/// Generation A: imports `Foo`, renders `<Foo />`.
const SOURCE_A: &str = "<script setup lang=\"ts\">\nimport Foo from './Foo.vue'\n</script>\n<template><Foo /></template>";

/// Generation B — the mid-flight move: imports `Bar`, renders
/// `<Bar />`.
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

/// THE PIN: a coherently-captured template persisted AFTER the racing
/// upsert cleared the slot must never serve as current — the next read
/// at the new generation recomputes the coherent conversion instead of
/// warm-serving the superseded one.
#[test]
fn source_move_between_compute_and_persist_never_serves_the_stale_template() {
    let host = make_host();
    upsert(&host, SOURCE_A);

    // Land the move deterministically inside the compute→persist
    // window: the flight's inputs and snapshot are BOTH generation-A
    // coherent (the capture race is closed by the generation join);
    // only the persist is late.
    let moved = Arc::new(AtomicBool::new(false));
    {
        let hook_host = Arc::clone(&host);
        let moved = Arc::clone(&moved);
        *host.template_persist_seam_hook.lock() = Some(Arc::new(move || {
            if !moved.swap(true, Ordering::SeqCst) {
                upsert(&hook_host, SOURCE_B);
            }
        }));
    }
    let raced = host
        .get_raw_analysis_snapshot(CANONICAL)
        .expect("the raced flight must serve its captured snapshot");
    *host.template_persist_seam_hook.lock() = None;
    assert!(
        moved.load(Ordering::SeqCst),
        "choreography sanity: the seam must have landed the move",
    );

    // The raced flight's OWN serve is coherent (A bytes, A snapshot)
    // — return-only truth for its captured generation.
    let raced_tpl = raced
        .template
        .as_ref()
        .expect("the raced flight computed a template from its coherent inputs");
    assert!(
        raced_tpl.components.iter().any(|c| c.name == "Foo"),
        "choreography sanity: the raced flight's template derives from its \
         captured A bytes",
    );

    // THE PIN — the quiescent next read at the new generation must
    // serve the COHERENT B conversion, never the superseded A template
    // the late persist parked in the cleared slot.
    let recovered = host
        .get_raw_analysis_snapshot(CANONICAL)
        .expect("the quiescent follow-up read must serve");
    let tpl = recovered
        .template
        .as_ref()
        .unwrap_or_else(|| panic!("the quiescent follow-up read must carry a template"));
    let bar = tpl
        .components
        .iter()
        .find(|c| c.name == "Bar")
        .unwrap_or_else(|| {
            panic!(
                "stale-publish served as current: the post-move read carries \
             components={:?} — the late persist repopulated the cleared slot \
             with the superseded generation's template and the reader had no \
             rail to reject it",
                tpl.components
                    .iter()
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>(),
            )
        });
    assert_eq!(
        bar.import_source.as_deref(),
        Some("./Bar.vue"),
        "the coherent B conversion classifies Bar from the B snapshot's imports",
    );
}

/// No-over-decline negative control: with no racing move, the persist
/// is current at its own generation — the warm read accepts the rail
/// and serves the persisted value without recomputing.
#[test]
fn quiescent_persist_still_warm_serves_at_its_own_generation() {
    let host = make_host();
    upsert(&host, SOURCE_A);

    let cold = host
        .get_raw_analysis_snapshot(CANONICAL)
        .expect("the cold read must serve");
    let cold_tpl = cold
        .template
        .as_ref()
        .expect("the cold read computes the template");

    let warm = host
        .get_raw_analysis_snapshot(CANONICAL)
        .expect("the warm read must serve");
    let warm_tpl = warm
        .template
        .as_ref()
        .expect("the warm read carries the template");
    assert!(
        Arc::ptr_eq(warm_tpl, cold_tpl),
        "the warm read serves the persisted slot value, not a recompute — \
         the rail accepts at the persisting flight's own generation",
    );
}
