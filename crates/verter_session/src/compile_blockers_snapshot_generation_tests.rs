//! The public `CompileBlockersSnapshot` is single-generation.
//!
//! `get_compile_blockers` builds its external source requests from the
//! held source snapshot. The other product on the served snapshot
//! (`macro_type_deps`) must derive from that SAME held generation: an
//! independent `try_get_analysis` read otherwise pairs gen-N external
//! `src` requests with gen-N+1 macro type deps when an upsert lands in
//! the window — a generation-mixed public snapshot. The analysis stage
//! repackages exactly the parse's `script_analysis` at the source's
//! generation (`AnalysisArcs::from_analysis` in `host_executor.rs`), so
//! deriving from the held parse is the same product at one generation.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::types::{FileKind, HostConfig, UpsertRequest};
use crate::VerterHost;

const CANONICAL: &str = "/proj/Owner.vue";

/// Generation A: macro type dep `AlphaProps` from `./alpha-types`,
/// external style src `./alpha.css`.
const SOURCE_A: &str = "<script setup lang=\"ts\">\nimport type { AlphaProps } from './alpha-types'\ndefineProps<AlphaProps>()\n</script>\n<template><div /></template>\n<style src=\"./alpha.css\"></style>";

/// Generation B — the mid-window move: macro type dep `BetaProps` from
/// `./beta-types`, external style src `./beta.css`. A single-generation
/// A snapshot can carry NO `BetaProps` dep and NO `./beta.css` request;
/// their presence next to A-derived products is the discriminating
/// generation-mix marker.
const SOURCE_B: &str = "<script setup lang=\"ts\">\nimport type { BetaProps } from './beta-types'\ndefineProps<BetaProps>()\n</script>\n<template><div /></template>\n<style src=\"./beta.css\"></style>";

fn make_host() -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.inject_file(
        "/proj/alpha-types.ts".to_string(),
        Arc::from("export interface AlphaProps { alpha: string }"),
    );
    workspace.inject_file(
        "/proj/beta-types.ts".to_string(),
        Arc::from("export interface BetaProps { beta: string }"),
    );
    workspace.inject_file("/proj/alpha.css".to_string(), Arc::from(".alpha {}"));
    workspace.inject_file("/proj/beta.css".to_string(), Arc::from(".beta {}"));
    Arc::new(VerterHost::new(HostConfig::default(), workspace))
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

fn external_specifiers(snapshot: &crate::types::CompileBlockersSnapshot) -> Vec<String> {
    snapshot
        .external_source_requests
        .iter()
        .map(|req| req.specifier.clone())
        .collect()
}

fn macro_dep_sources(snapshot: &crate::types::CompileBlockersSnapshot) -> Vec<String> {
    snapshot
        .macro_type_deps
        .iter()
        .map(|dep| dep.import_source.clone())
        .collect()
}

/// THE PIN: a source move landing between the source capture and the
/// products assembly must not hand the caller a snapshot whose external
/// source requests describe one generation and whose macro type deps
/// describe another.
#[test]
fn source_move_inside_the_compile_blockers_window_never_serves_a_generation_mix() {
    let host = make_host();
    upsert(&host, SOURCE_A);

    // Land the move deterministically inside the capture→assembly window.
    let moved = Arc::new(AtomicBool::new(false));
    {
        let hook_host = Arc::clone(&host);
        let moved = Arc::clone(&moved);
        *host.compile_blockers_serve_seam_hook.lock() = Some(Arc::new(move || {
            if !moved.swap(true, Ordering::SeqCst) {
                upsert(&hook_host, SOURCE_B);
            }
        }));
    }
    let raced = host
        .get_compile_blockers(CANONICAL)
        .expect("get_compile_blockers must serve its captured snapshot");
    *host.compile_blockers_serve_seam_hook.lock() = None;
    assert!(
        moved.load(Ordering::SeqCst),
        "choreography sanity: the seam must have landed the move",
    );
    // The flight captured the A source — pin the capture so the pin
    // below is provably capture-vs-products, not capture-vs-capture.
    let specifiers = external_specifiers(&raced);
    assert!(
        specifiers.iter().any(|s| s == "./alpha.css")
            && !specifiers.iter().any(|s| s == "./beta.css"),
        "choreography sanity: the raced snapshot's external source requests derive \
         from generation A (got {specifiers:?})",
    );

    // THE PIN — every product on the served snapshot describes the SAME
    // generation as its external source requests: the A macro type
    // deps, never the moved B deps from an independent later read.
    let dep_sources = macro_dep_sources(&raced);
    assert!(
        dep_sources.iter().any(|s| s == "./alpha-types")
            && !dep_sources.iter().any(|s| s == "./beta-types"),
        "the served snapshot mixes generations: A external source requests paired \
         with macro type deps {dep_sources:?} (a single-generation A snapshot \
         carries the ./alpha-types dep and cannot carry ./beta-types)",
    );

    // Recovery: the quiescent next read serves the coherent B snapshot
    // on every product — the raced flight does not wedge anything.
    let recovered = host
        .get_compile_blockers(CANONICAL)
        .expect("the quiescent follow-up read must serve");
    let specifiers = external_specifiers(&recovered);
    assert!(
        specifiers.iter().any(|s| s == "./beta.css")
            && !specifiers.iter().any(|s| s == "./alpha.css"),
        "the quiescent read serves the B external source requests (got {specifiers:?})",
    );
    let dep_sources = macro_dep_sources(&recovered);
    assert!(
        dep_sources.iter().any(|s| s == "./beta-types")
            && !dep_sources.iter().any(|s| s == "./alpha-types"),
        "the quiescent read serves the B macro type deps (got {dep_sources:?})",
    );
}

/// No-over-decline negative control: the quiescent read still serves
/// the full product set — external source requests and macro type deps
/// — all from one generation.
#[test]
fn quiescent_compile_blockers_snapshot_still_serves_both_products() {
    let host = make_host();
    upsert(&host, SOURCE_A);

    let snapshot = host
        .get_compile_blockers(CANONICAL)
        .expect("get_compile_blockers must serve");
    let specifiers = external_specifiers(&snapshot);
    assert!(
        specifiers.iter().any(|s| s == "./alpha.css"),
        "the quiescent read serves the A external source requests (got {specifiers:?})",
    );
    let dep_sources = macro_dep_sources(&snapshot);
    assert!(
        dep_sources.iter().any(|s| s == "./alpha-types"),
        "the quiescent read serves the A macro type deps (got {dep_sources:?})",
    );
}
