//! Style v-bind compile inputs derive from the request's SINGLE source
//! snapshot.
//!
//! `style_v_bind_vars` is codegen-affecting compile input: the IDE
//! script wrapper emits a `void(name);` keep-alive for every setup
//! binding referenced only from style `v-bind()` expressions, so TS
//! does not flag it unused. The vars must come from the same coherent
//! source snapshot the compiled bytes and the cache key derive from.
//! Reading them from an independent analysis-snapshot lookup races the
//! scheduler's Source→Analysis commit window (source committed at the
//! node generation, analysis not yet): the racing compile observed
//! `None`, defaulted to EMPTY v-bind vars, and PUBLISHED the wrong
//! bytes warm under a fully-valid session key. The publish fence cannot
//! catch it — the source never moved — and nothing invalidates the slot
//! when the analysis commit lands.

use std::sync::Arc;

use crate::types::{
    CompileProfile, FileKind, HostConfig, UpsertRequest, VirtualNodeKind, VirtualQuery,
};
use crate::VerterHost;

const CANONICAL: &str = "/proj/Themed.vue";

/// `themeColor` is referenced ONLY from the style block's `v-bind()` —
/// never from the script body, never from the template — so its
/// `void(themeColor);` keep-alive exists in the IDE TSX output iff the
/// compile input actually carried the style v-bind vars. (The VDOM
/// `_useCssVars` injection extracts its vars independently inside style
/// codegen, so only the IDE output discriminates the compile input.)
const SOURCE: &str = "<script setup lang=\"ts\">\nconst themeColor = 'red'\n</script>\n<template><div>x</div></template>\n<style>div { color: v-bind(themeColor); }</style>";

const VBIND_KEEPALIVE: &str = "void(themeColor);";

fn make_host() -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    Arc::new(VerterHost::new(HostConfig::default(), workspace))
}

fn upsert(host: &VerterHost) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: Arc::from(SOURCE),
            file_kind: FileKind::from_path(CANONICAL),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
}

/// The LSP's IDE-target profile: TSX codegen, the consumer of
/// `CompileInput.style_v_bind_vars`.
fn ide_profile() -> CompileProfile {
    CompileProfile {
        target: verter_compiler::compile::CompileTarget::IDE,
        ..CompileProfile::default()
    }
}

fn compile(host: &VerterHost) -> crate::types::VirtualFileResponse {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(CANONICAL.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: ide_profile(),
    })
    .expect("compile must serve")
}

/// The published session slot's TSX — the LSP-facing surface the
/// keep-alive lands on. `None` when no admitted slot exists.
fn published_tsx(host: &VerterHost) -> Option<Arc<str>> {
    host.get_ide(CANONICAL, &ide_profile()).map(|r| r.code)
}

/// A compile running inside the Source→Analysis commit window must
/// carry the source snapshot's style v-bind vars, not an empty default
/// read off the absent analysis snapshot.
///
/// Discrimination: pre-fix the compile input read `style_v_bind_vars`
/// from an independent `try_get_analysis` lookup; with the analysis
/// slot empty it defaulted to NO vars, the published TSX lost the
/// `void(themeColor);` keep-alive, and — the source being unmoved — the
/// session publish admitted the wrong bytes warm. Post-fix the vars
/// derive from the request's single source snapshot
/// (`HostSourceData.parse.style_analyses`), so the window is
/// unobservable in the compiled output.
#[test]
fn compile_inside_the_analysis_commit_window_carries_the_snapshot_style_vbind_vars() {
    let host = make_host();
    upsert(&host);

    // Model the commit window deterministically: source committed at
    // the node generation, analysis slot empty.
    host.scheduler.test_clear_analysis(CANONICAL);
    assert!(
        host.scheduler.try_get_source(CANONICAL).is_some(),
        "window sanity: the source snapshot must still be served",
    );
    assert!(
        host.scheduler.try_get_analysis(CANONICAL).is_none(),
        "window sanity: the analysis snapshot must be absent",
    );

    let raced = compile(&host);
    assert!(!raced.cache_hit, "first compile must be cold");
    let raced_tsx = published_tsx(&host)
        .expect("the snapshot-coherent raced compile must publish its session slot");
    // THE PIN: the compile input's style v-bind vars come from the same
    // source snapshot as the compiled bytes, so the keep-alive survives
    // the analysis-absent window.
    assert!(
        raced_tsx.contains(VBIND_KEEPALIVE),
        "a compile racing the analysis commit must carry the snapshot's \
         style v-bind vars — empty vars compiled here would publish warm \
         under an unmoved key with no invalidation when the analysis lands",
    );

    // Snapshot-coherence equivalence: the same content compiled
    // quiescent (analysis present) is byte-identical — the commit
    // window must be unobservable in the compiled output.
    let quiescent_host = make_host();
    upsert(&quiescent_host);
    assert!(
        quiescent_host
            .scheduler
            .try_get_analysis(CANONICAL)
            .is_some(),
        "equivalence sanity: the quiescent host's analysis must be present",
    );
    let _ = compile(&quiescent_host);
    let quiescent_tsx = published_tsx(&quiescent_host).expect("the quiescent compile must publish");
    assert_eq!(
        raced_tsx, quiescent_tsx,
        "the analysis commit window must be unobservable in the compiled bytes",
    );

    // No over-decline: the raced compile's inputs are coherent with its
    // unmoved source, so the session publish must land and serve the
    // next request warm with the SAME bytes.
    let warm = compile(&host);
    assert!(
        warm.cache_hit,
        "the snapshot-coherent raced compile must still publish its \
         session slot — coherence is restored by reading the one \
         snapshot, not by declining publication",
    );
    let warm_tsx = published_tsx(&host).expect("warm slot must keep serving the TSX");
    assert_eq!(warm_tsx, raced_tsx, "warm TSX must be byte-identical");
}

/// No-over-decline negative control: a quiescent compile (analysis
/// present) publishes warm with the real v-bind vars. Also pins the
/// `void(themeColor);` marker itself — if the keep-alive emission ever
/// moves, this control fails alongside the window test, flagging a
/// stale marker rather than a regression.
#[test]
fn quiescent_compile_with_analysis_present_publishes_the_vbind_vars_warm() {
    let host = make_host();
    upsert(&host);
    assert!(
        host.scheduler.try_get_analysis(CANONICAL).is_some(),
        "control sanity: the analysis snapshot must be present",
    );

    let cold = compile(&host);
    assert!(!cold.cache_hit, "first compile must be cold");
    let cold_tsx = published_tsx(&host).expect("the quiescent compile must publish");
    assert!(
        cold_tsx.contains(VBIND_KEEPALIVE),
        "the quiescent compile must emit the v-bind keep-alive",
    );

    let warm = compile(&host);
    assert!(warm.cache_hit, "the quiescent entry must serve warm");
    let warm_tsx = published_tsx(&host).expect("warm slot must keep serving the TSX");
    assert_eq!(warm_tsx, cold_tsx, "warm TSX must be byte-identical");
}
