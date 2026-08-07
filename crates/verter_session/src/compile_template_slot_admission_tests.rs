//! The compile-publish raw-template persist honours the slot's
//! admission semantics.
//!
//! The profileless canonical-keyed `derived_raw_cache().raw_template_analysis`
//! slot stores the DEFAULT-extraction template for the canonical's own
//! inline bytes — that is what every reader (`get_analysis` /
//! `get_raw_analysis_snapshot`) serves it as. Two compile shapes
//! therefore must never populate it:
//!
//! - a compile under parse-affecting profile options (`delimiters`,
//!   `custom_elements`): the extraction describes a DIFFERENT parse of
//!   the same bytes, yet the entry would carry a valid current
//!   generation stamp — cache poisoning the rail cannot reject;
//! - an external-src SFC: editing the external dep clears compile
//!   slots, not this slot, and the owner's node generation does not
//!   move, so the rail cannot reject the stale entry.
//!
//! Both rules are the SAME admission the lazy template-analysis writer
//! already enforces — one shared write authority, not a per-site gate.

use std::sync::Arc;

use crate::hash::compile_profile_hash;
use crate::types::{
    CompileCacheMode, CompileProfile, HostConfig, UpsertRequest, VirtualNodeKind, VirtualQuery,
};
use crate::VerterHost;

const OWNER: &str = "/proj/Owner.vue";

/// Inline SFC: one script binding interpolated with the DEFAULT
/// delimiters. Under custom delimiters the `{{ msg }}` text is plain
/// text, so the delimiter extraction carries NO `msg` binding
/// occurrence — the discriminating observable.
const INLINE_SOURCE: &str = "<script setup lang=\"ts\">\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>";

/// External-src SFC: the template body lives in `partial.html`.
const EXTERNAL_SRC_SOURCE: &str = "<template src=\"./partial.html\"></template>\n<script setup lang=\"ts\">\nconst ok = true\n</script>";

fn make_host() -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.inject_file(
        "/proj/partial.html".to_string(),
        Arc::from("<div>hello</div>"),
    );
    Arc::new(VerterHost::new(HostConfig::default(), workspace))
}

fn upsert(host: &VerterHost, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(OWNER.to_string()),
            input_id: OWNER.to_string(),
            source: Arc::from(source),
            file_language: verter_language::LanguageRegistry::global()
                .classify_static(OWNER)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
}

fn compile_with(
    host: &VerterHost,
    profile: CompileProfile,
) -> Result<crate::types::VirtualFileResponse, crate::types::HostError> {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(OWNER.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile,
    })
}

/// Whether the Session compile slot for `(OWNER, profile)` holds a
/// published entry — proves the compile's OWN admission was
/// `Cacheable`, so a missing template-slot persist is the slot
/// admission acting, not a refused compile.
fn session_slot_present(host: &VerterHost, profile: &CompileProfile) -> bool {
    let profile_hash = compile_profile_hash(profile);
    host.compile_cache()
        .get(OWNER)
        .map(|cc| {
            crate::cache_runtime::CompileOutputNodeFactValidatedSession::new()
                .peek_signature(&cc, profile_hash)
                .is_some()
        })
        .unwrap_or(false)
}

fn template_slot(
    host: &VerterHost,
) -> Option<Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>> {
    host.derived_raw_cache().get(OWNER).and_then(|cc| {
        cc.raw_template_analysis()
            .map(|entry| Arc::clone(&entry.template))
    })
}

/// THE PIN (parse-affecting profile): a Session compile under custom
/// delimiters extracts a DIFFERENT template from the same bytes — it
/// must not populate the profileless default-extraction slot, and the
/// next default-lane read must serve the default extraction.
#[test]
fn non_default_parse_profile_compile_never_populates_the_default_template_slot() {
    let host = make_host();
    upsert(&host, INLINE_SOURCE);

    let profile = CompileProfile {
        delimiters: Some(("[[".to_string(), "]]".to_string())),
        ..CompileProfile::default()
    };
    let error = compile_with(&host, profile.clone())
        .expect_err("grammar mismatch must fail before compilation");
    assert!(matches!(error, crate::types::HostError::GrammarMismatch(_)));
    assert!(!session_slot_present(&host, &profile));
    assert!(template_slot(&host).is_none());
}

/// THE PIN (external src): an external-src SFC compile must not
/// populate the slot — an external dep edit clears compile slots, not
/// this slot, and the owner's node generation does not move, so the
/// rail could never reject the stale entry.
#[test]
fn external_src_compile_uses_the_session_slot_not_the_template_slot() {
    let host = make_host();
    upsert(&host, EXTERNAL_SRC_SOURCE);
    host.set_exact_resolutions(
        OWNER,
        vec![verter_workspace::ExactResolution {
            specifier: "./partial.html".to_string(),
            phase: verter_workspace::ResolvePhase::CodegenBlocker,
            kind: verter_workspace::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/proj/partial.html".to_string()),
            possible_canonical_ids: vec!["/proj/partial.html".to_string()],
        }],
    );

    let profile = CompileProfile::default();
    let output = compile_with(&host, profile.clone()).expect("external content lowering");
    assert!(output.code.contains("hello"));
    assert!(session_slot_present(&host, &profile));
    assert!(template_slot(&host).is_none());
}

/// No-over-decline negative control: a default-profile inline compile
/// still persists the slot, and the default lane warm-serves the SAME
/// Arc — the admission keys on parse-affecting options and src blocks,
/// not on declining every compile-lane persist.
#[test]
fn default_profile_inline_compile_still_persists_and_warm_serves() {
    let host = make_host();
    upsert(&host, INLINE_SOURCE);

    let profile = CompileProfile::default();
    let compiled = compile_with(&host, profile.clone()).expect("compile must serve");
    assert_eq!(compiled.actual_mode, CompileCacheMode::Session);
    assert!(
        session_slot_present(&host, &profile),
        "the default compile must publish its session slot",
    );

    let persisted =
        template_slot(&host).expect("a default-profile inline compile persists the template slot");
    let snapshot = host
        .get_raw_analysis_snapshot(OWNER)
        .expect("the warm read must serve");
    let served = snapshot
        .template
        .as_ref()
        .expect("the warm read carries a template");
    assert!(
        Arc::ptr_eq(served, &persisted),
        "the default lane warm-serves the compile-persisted value — the \
         admission must not decline the coherent default-extraction persist",
    );
    assert!(
        persisted
            .binding_occurrences
            .iter()
            .any(|occ| occ.name == "msg"),
        "the persisted default extraction carries the msg interpolation occurrence",
    );
}
