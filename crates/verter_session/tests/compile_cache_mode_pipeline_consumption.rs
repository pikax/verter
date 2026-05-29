//! Pipeline-consumption regression tests for `CompileCacheMode` routing.
//!
//! These guard how `get_virtual_file` / `compile_many` CONSUME the mode
//! classifier — not the classifier matrix itself (that is covered by
//! `compile_cache_mode_classifier`). Each test asserts a real observable
//! and is discriminating: it fails against a tree that consults a cache
//! node before classifying, that serves a stale content entry under an
//! override, that leaves the content-addressed node populated after a
//! cache clear, or that fans a single dedupe-winner mode out to every
//! batch position sharing a canonical.

use std::sync::Arc;

use verter_session::host_compile::{CompileBatchInput, CompileBatchOptions};
use verter_session::{
    BlockOverrideEntry, BlockOverrideRequest, CompileCacheMode, CompileErrorPolicy, CompileProfile,
    DowngradeReason, FileKind, HostConfig, PreprocessorBlockType, UpsertRequest, VerterHost,
    VirtualNodeKind, VirtualQuery,
};

/// A production (non-dev) host config. The default `HostConfig` enables
/// `dev_mode` + `DevServeLastKnownGood`, which fires the `HasDevLastGood`
/// reason on EVERY compile and would downgrade every `Content` request to
/// `Stateless`. A `Content` request is only reachable as `Content` when no
/// reason fires, so these tests use a production config.
fn prod_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn upsert_vue(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: source.into(),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .expect("upsert vue");
}

fn content_profile() -> CompileProfile {
    CompileProfile {
        requested_mode: CompileCacheMode::Content,
        ..CompileProfile::default()
    }
}

fn compile(
    host: &VerterHost,
    canonical: &str,
    node: VirtualNodeKind,
    profile: &CompileProfile,
) -> verter_session::VirtualFileResponse {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(node),
        compile_profile: profile.clone(),
    })
    .expect("compile")
}

// A fact-free SFC carrying a style block: no cross-file deps (so a Content
// request runs as Content) but a style block exists so a style override is
// applicable.
const SFC_WITH_STYLE: &str = "<script setup lang=\"ts\">const n = 1</script>\
     <template><div>{{ n }}</div></template>\
     <style>.a{color:red}</style>";

/// Fix 2 — a `Content` warm hit must NOT be served when a request-time
/// override forces a downgrade. After a `Content` compile publishes a
/// content-addressed entry, applying a style override (which removes the
/// session slot but does NOT bump `whole_hash` nor evict the
/// content-addressed entry) must make the next `Content` request classify
/// to `Stateless` BEFORE the warm-hit consult, so the stale entry is never
/// served.
///
/// Discriminates: on a tree that consults the content node before
/// classifying, the second request serves `actual_mode == Content,
/// downgrade_reason == None` (the stale entry); after the fix it reports
/// `actual_mode == Stateless, downgrade_reason == Some(HasStyleOverride)`.
#[test]
fn content_warm_hit_not_served_when_override_forces_downgrade() {
    let host = prod_host();
    upsert_vue(&host, "/S.vue", SFC_WITH_STYLE);
    let profile = content_profile();

    // First Content compile publishes a content-addressed entry.
    let first = compile(&host, "/S.vue", VirtualNodeKind::Main, &profile);
    assert_eq!(
        first.actual_mode,
        CompileCacheMode::Content,
        "fact-free Content compile must run as Content under a production config"
    );
    assert!(first.downgrade_reason.is_none());
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "first Content compile must publish one content-addressed entry"
    );

    // Apply a style override under the same profile. This removes the
    // session slot but does NOT change the .vue's own source.
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "/S.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry {
                block_type: PreprocessorBlockType::Style,
                index: 0,
                code: Arc::from(".a{color:blue}"),
                source_map: None,
            }],
        })
        .expect("style override");

    // The next Content request must classify to Stateless BEFORE consulting
    // the (still-present) content-addressed entry, so the stale entry is
    // never served.
    let second = compile(&host, "/S.vue", VirtualNodeKind::Main, &profile);
    assert_eq!(
        second.actual_mode,
        CompileCacheMode::Stateless,
        "a Content request with an active style override MUST classify to \
         Stateless, not serve the stale content-addressed entry"
    );
    assert_eq!(
        second.downgrade_reason,
        Some(DowngradeReason::HasStyleOverride),
        "the downgrade reason must be HasStyleOverride"
    );
    assert_eq!(
        second.requested_mode,
        CompileCacheMode::Content,
        "requested mode is still Content"
    );
}

/// Fix 2 (block override variant) — the same gap closes for a block
/// (template/script) override, which fires `HasBlockOverride`.
#[test]
fn content_warm_hit_not_served_when_block_override_forces_downgrade() {
    let host = prod_host();
    let src = "<script setup lang=\"ts\">const n = 1</script>\
         <template><div>{{ n }}</div></template>";
    upsert_vue(&host, "/B.vue", src);
    let profile = content_profile();

    let first = compile(&host, "/B.vue", VirtualNodeKind::Main, &profile);
    assert_eq!(first.actual_mode, CompileCacheMode::Content);
    assert_eq!(host.compile_output_pure_content_entry_count(), 1);

    // Override the script block.
    let _ = host
        .apply_block_overrides(BlockOverrideRequest {
            canonical_id: "/B.vue".to_string(),
            compile_profile: profile.clone(),
            overrides: vec![BlockOverrideEntry {
                block_type: PreprocessorBlockType::Script,
                index: 0,
                code: Arc::from("const n = 2"),
                source_map: None,
            }],
        })
        .expect("block override");

    let second = compile(&host, "/B.vue", VirtualNodeKind::Main, &profile);
    assert_eq!(
        second.actual_mode,
        CompileCacheMode::Stateless,
        "a Content request with an active block override MUST classify to Stateless"
    );
    assert_eq!(
        second.downgrade_reason,
        Some(DowngradeReason::HasBlockOverride),
        "the downgrade reason must be HasBlockOverride"
    );
}

/// Fix 4 — `clear_compile_cache` must flush the content-addressed node.
///
/// Discriminates: pre-fix the content-addressed entry survives the clear
/// (`entry_count` stays 1); after the fix it drops to 0.
#[test]
fn clear_compile_cache_empties_pure_content_node() {
    let host = prod_host();
    upsert_vue(&host, "/C.vue", SFC_WITH_STYLE);
    let profile = content_profile();

    let r = compile(&host, "/C.vue", VirtualNodeKind::Main, &profile);
    assert_eq!(r.actual_mode, CompileCacheMode::Content);
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "Content compile must publish exactly one content-addressed entry"
    );

    host.clear_compile_cache();

    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "clear_compile_cache must flush the content-addressed node"
    );
}

/// `close()` (the release-all teardown) must flush the content-addressed
/// node, symmetric with `clear_compile_cache`. `close()` documents
/// "release all cached data" + frees the backing memory for NAPI-backed
/// hosts, so a Content-mode compile output published into the
/// content-addressed store must not survive it.
///
/// Discriminates: pre-fix `close()` clears the per-file compile cache and
/// session slots but never touches the sibling content-addressed store, so
/// the entry survives (`entry_count` stays 1); after the fix it drops to 0.
/// The count is read directly off the store (not via the scheduler), so it
/// stays valid even after `close()` resets the scheduler.
#[test]
fn close_empties_pure_content_node() {
    let host = prod_host();
    upsert_vue(&host, "/D.vue", SFC_WITH_STYLE);
    let profile = content_profile();

    let r = compile(&host, "/D.vue", VirtualNodeKind::Main, &profile);
    assert_eq!(r.actual_mode, CompileCacheMode::Content);
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "a Content compile must publish exactly one content-addressed entry"
    );

    host.close();

    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "close() must flush the content-addressed PureContent store"
    );
}

/// Fix 5 — `compile_many` must honor per-input `requested_mode`, so two
/// inputs sharing a canonical at different modes each carry their own
/// requested / actual mode.
///
/// Discriminates: pre-fix the canonical-only dedupe compiles one mode and
/// fans the dedupe-winner's entry out to both positions, so both report
/// the SAME requested_mode/actual_mode. After the fix each position
/// reports its own mode.
#[test]
fn compile_many_honors_per_input_requested_mode() {
    let host = prod_host();
    // A fact-free SFC: a Session request stays Session, a Stateless request
    // stays Stateless — the two positions land on different modes.
    let src = "<script setup lang=\"ts\">const n = 1</script>\
         <template><div>{{ n }}</div></template>";
    let source: Arc<str> = Arc::from(src);

    let inputs = vec![
        CompileBatchInput {
            canonical_id: "/M.vue".to_string(),
            source: source.clone(),
            requested_mode: Some(CompileCacheMode::Session),
        },
        CompileBatchInput {
            canonical_id: "/M.vue".to_string(),
            source: source.clone(),
            requested_mode: Some(CompileCacheMode::Stateless),
        },
    ];

    let results = host.compile_many(inputs, CompileBatchOptions::default());
    assert_eq!(results.len(), 2, "one entry per original input position");

    // Position 0 requested Session.
    assert_eq!(
        results[0].requested_mode,
        CompileCacheMode::Session,
        "position 0 must report its OWN requested mode (Session)"
    );
    assert_eq!(
        results[0].actual_mode,
        CompileCacheMode::Session,
        "a fact-free Session request stays Session"
    );

    // Position 1 requested Stateless.
    assert_eq!(
        results[1].requested_mode,
        CompileCacheMode::Stateless,
        "position 1 must report its OWN requested mode (Stateless)"
    );
    assert_eq!(
        results[1].actual_mode,
        CompileCacheMode::Stateless,
        "a Stateless request stays Stateless"
    );

    // The two positions carry DISTINCT modes — the discriminating signal.
    assert_ne!(
        results[0].requested_mode, results[1].requested_mode,
        "mixed-mode batch positions must not collapse to one dedupe-winner mode"
    );

    // Both compiled to real output.
    assert!(!results[0].code.is_empty());
    assert!(!results[1].code.is_empty());
}
