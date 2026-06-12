//! The narrowed-scope serve branch's public `FileAnalysisSnapshot` is
//! single-generation.
//!
//! `get_analysis_snapshot_internal`'s narrowed-scope branch builds its
//! script analysis from the held source snapshot. Every other product
//! on the served snapshot (style analyses, export signatures) must
//! derive from that SAME held generation: an independent
//! `try_get_analysis` read otherwise pairs gen-N script analysis with
//! gen-N+1 styles / export signatures when an upsert lands in the
//! window — a generation-mixed public snapshot. The branch is
//! serve-only (nothing persists from its products), but the public
//! snapshot contract is one generation per snapshot.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::types::{FileKind, HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_semantic::analysis::AnalysisScope;

const CANONICAL: &str = "/proj/Owner.vue";

/// Generation A: imports `Foo`, exports `alphaExport`, styles `.alpha`.
const SOURCE_A: &str = "<script lang=\"ts\">\nimport Foo from './Foo.vue'\nexport const alphaExport = Foo\n</script>\n<template><div /></template>\n<style>.alpha { color: red }</style>";

/// Generation B — the mid-window move: imports `Bar`, exports
/// `betaExport`, styles `.beta`. A single-generation A snapshot can
/// carry NO `.beta` class and NO `betaExport` signature; their
/// presence next to A-derived script analysis is the discriminating
/// generation-mix marker.
const SOURCE_B: &str = "<script lang=\"ts\">\nimport Bar from './Bar.vue'\nexport const betaExport = Bar\n</script>\n<template><div /></template>\n<style>.beta { color: blue }</style>";

/// A scope that enters the narrowed-scope serve branch
/// (`needs_script_analysis() == false`) while still consuming BOTH
/// analysis-stage products: style analyses (`STYLE_CSS`) and export
/// signatures (`EXPORT_SIGNATURES` — not a script-analysis flag).
fn narrowed_scope() -> AnalysisScope {
    let scope = AnalysisScope::STYLE_CSS | AnalysisScope::EXPORT_SIGNATURES;
    assert!(!scope.needs_script_analysis(), "scope sanity: narrowed");
    assert!(
        scope.needs_style_analysis(),
        "scope sanity: styles consumed"
    );
    scope
}

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
    let config = HostConfig {
        analysis_scope: Some(narrowed_scope()),
        ..HostConfig::default()
    };
    Arc::new(VerterHost::new(config, workspace))
}

fn upsert(host: &VerterHost, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::from_path(CANONICAL),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
}

fn style_classes(snapshot: &crate::types::FileAnalysisSnapshot) -> Vec<String> {
    snapshot
        .styles
        .iter()
        .filter_map(|s| s.css.as_ref())
        .flat_map(|css| css.classes.iter().map(|c| c.name.clone()))
        .collect()
}

fn export_names(snapshot: &crate::types::FileAnalysisSnapshot) -> Vec<String> {
    snapshot
        .export_signatures
        .iter()
        .map(|sig| sig.name.clone())
        .collect()
}

/// THE PIN: a source move landing between the branch's source capture
/// and its products assembly must not hand the caller a snapshot whose
/// script analysis describes one generation and whose styles / export
/// signatures describe another.
#[test]
fn source_move_inside_the_narrowed_scope_window_never_serves_a_generation_mix() {
    let host = make_host();
    upsert(&host, SOURCE_A);

    // Land the move deterministically inside the capture→assembly window.
    let moved = Arc::new(AtomicBool::new(false));
    {
        let hook_host = Arc::clone(&host);
        let moved = Arc::clone(&moved);
        *host.narrowed_scope_serve_seam_hook.lock() = Some(Arc::new(move || {
            if !moved.swap(true, Ordering::SeqCst) {
                upsert(&hook_host, SOURCE_B);
            }
        }));
    }
    let raced = host
        .get_analysis(CANONICAL)
        .expect("the narrowed-scope serve branch must serve its captured snapshot");
    *host.narrowed_scope_serve_seam_hook.lock() = None;
    assert!(
        moved.load(Ordering::SeqCst),
        "choreography sanity: the seam must have landed the move",
    );
    // The branch captured the A source — pin the capture so the pin
    // below is provably capture-vs-products, not capture-vs-capture.
    assert!(
        raced.imports.iter().any(|i| i.source == "./Foo.vue"),
        "choreography sanity: the raced snapshot's script analysis derives from generation A",
    );

    // THE PIN — every product on the served snapshot describes the SAME
    // generation as its script analysis: the A styles and the A export
    // signatures, never the moved B products from an independent later
    // read (and never an empty fallback while the A parse carries both).
    let classes = style_classes(&raced);
    assert!(
        classes.iter().any(|c| c == "alpha") && !classes.iter().any(|c| c == "beta"),
        "the served snapshot mixes generations: A script analysis paired with \
         styles {classes:?} (a single-generation A snapshot carries the .alpha \
         class and cannot carry .beta)",
    );
    let exports = export_names(&raced);
    assert!(
        exports.iter().any(|e| e == "alphaExport") && !exports.iter().any(|e| e == "betaExport"),
        "the served snapshot mixes generations: A script analysis paired with \
         export signatures {exports:?} (a single-generation A snapshot carries \
         alphaExport and cannot carry betaExport)",
    );

    // Recovery: the quiescent next read serves the coherent B snapshot
    // on every product — the raced flight does not wedge anything.
    let recovered = host
        .get_analysis(CANONICAL)
        .expect("the quiescent follow-up read must serve");
    assert!(
        recovered.imports.iter().any(|i| i.source == "./Bar.vue"),
        "the quiescent read's script analysis derives from generation B",
    );
    let classes = style_classes(&recovered);
    assert!(
        classes.iter().any(|c| c == "beta") && !classes.iter().any(|c| c == "alpha"),
        "the quiescent read serves the B styles (got {classes:?})",
    );
    let exports = export_names(&recovered);
    assert!(
        exports.iter().any(|e| e == "betaExport") && !exports.iter().any(|e| e == "alphaExport"),
        "the quiescent read serves the B export signatures (got {exports:?})",
    );
}

/// No-over-decline negative control: the quiescent narrowed-scope read
/// still serves the full product set — style analyses and export
/// signatures — alongside its script analysis, all from one generation.
#[test]
fn quiescent_narrowed_scope_snapshot_still_serves_styles_and_export_signatures() {
    let host = make_host();
    upsert(&host, SOURCE_A);

    let snapshot = host
        .get_analysis(CANONICAL)
        .expect("the narrowed-scope serve branch must serve");
    assert!(
        snapshot.imports.iter().any(|i| i.source == "./Foo.vue"),
        "script analysis is recomputed from the held source",
    );
    let classes = style_classes(&snapshot);
    assert!(
        classes.iter().any(|c| c == "alpha"),
        "the quiescent read serves the A style analyses (got {classes:?})",
    );
    let exports = export_names(&snapshot);
    assert!(
        exports.iter().any(|e| e == "alphaExport"),
        "the quiescent read serves the A export signatures (got {exports:?})",
    );
}
