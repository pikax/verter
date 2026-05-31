//! Stage 4a smoke test for [`SessionView`] trait + [`HostView`] /
//! [`OverlaidView`] impls.
//!
//! This integration test boots a real `VerterHost`, ingests a tiny
//! source corpus, and asserts the trait's read shape. It is the
//! companion to the unit tests under
//! `crates/verter_session/src/session_view.rs::tests`.
//!
//! Plan provenance: fact-based cache refactor Stage 4a. Binds R17
//! (sessions are views), R18 (no thread-local view globals), R19
//! (fact validation orthogonal to concurrency oracle) — the latter
//! two are tested at Stages 4b/4c. Stage 4a is the read-trait-only
//! commit.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use verter_session::session_view::{HostView, OverlaidView, SessionView};
use verter_session::{CompileErrorPolicy, FileKind, HostConfig, UpsertRequest, VerterHost};

fn host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    }))
}

fn upsert(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .expect("upsert succeeds");
}

fn upsert_and_index(host: &VerterHost, canonical: &str, source: &str) {
    upsert(host, canonical, source);
    // Materialise parse artifacts so `FileArtifactStore` has an
    // entry for this canonical. `evaluate_types` is the public
    // entry point that triggers full indexing (including the
    // `FileArtifactStore` population) without needing access to
    // `ensure_indexed_ready` (which is `pub(crate)`). The
    // result value is irrelevant — the materialisation side
    // effect is what these integration tests need.
    let _ = host.evaluate_types(canonical);
}

#[test]
fn host_view_returns_source_for_ingested_canonical() {
    let host = host();
    upsert(&host, "/m.ts", "export const x = 1;");

    let view = HostView::new(Arc::clone(&host));
    let observed = view.source("/m.ts");
    assert_eq!(observed.as_deref(), Some("export const x = 1;"));
    assert!(view.source("/missing.ts").is_none());
}

#[test]
fn overlaid_view_overlay_wins_then_falls_through_to_base() {
    let host = host();
    upsert(&host, "/base-only.ts", "export const a = 1;");
    upsert(&host, "/will-overlay.ts", "export const b = 2;");

    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(
        "/will-overlay.ts".to_string(),
        Arc::from("export const b = 999;"),
    );
    let view = OverlaidView::new(Arc::clone(&host), overlays);

    // Overlay wins for the masked canonical.
    assert_eq!(
        view.source("/will-overlay.ts").as_deref(),
        Some("export const b = 999;"),
    );
    // Base host serves the canonical that has no overlay.
    assert_eq!(
        view.source("/base-only.ts").as_deref(),
        Some("export const a = 1;"),
    );
}

#[test]
fn overlaid_view_byte_identical_overlay_matches_base_hash() {
    // R17 — byte-identical overlay collapses to base hash. This is
    // the core architectural invariant for Stage 4a: overlay
    // identity is content-addressed, not session-identity-keyed.
    let host = host();
    let body = "export const a = 1;";
    upsert_and_index(&host, "/x.ts", body);

    let base_hash = HostView::new(Arc::clone(&host))
        .content_hash_for("/x.ts")
        .expect("base hash for ingested canonical");

    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert("/x.ts".to_string(), Arc::from(body));
    let overlay_hash = OverlaidView::new(Arc::clone(&host), overlays)
        .content_hash_for("/x.ts")
        .expect("overlay hash present");

    assert_eq!(
        base_hash, overlay_hash,
        "byte-identical overlay must collapse to the base content hash"
    );
}

#[test]
fn overlaid_view_diverging_overlay_diverges_in_hash() {
    let host = host();
    upsert_and_index(&host, "/x.ts", "export const a = 1;");

    let base_hash = HostView::new(Arc::clone(&host))
        .content_hash_for("/x.ts")
        .expect("base hash for ingested canonical");

    let mut overlays: FxHashMap<String, Arc<str>> = FxHashMap::default();
    overlays.insert(
        "/x.ts".to_string(),
        Arc::from("export const a = 'overlay';"),
    );
    let overlay_hash = OverlaidView::new(Arc::clone(&host), overlays)
        .content_hash_for("/x.ts")
        .expect("overlay hash present");

    assert_ne!(
        base_hash, overlay_hash,
        "a divergent overlay source must produce a different content hash"
    );
}

#[test]
fn host_view_and_overlaid_view_share_session_view_trait_object() {
    // Sanity check that both impls coerce to `&dyn SessionView` —
    // the resolver-tier wiring in Stage 4b consumes the trait
    // object form.
    let host = host();
    upsert(&host, "/m.ts", "export const x = 1;");

    let host_view: Box<dyn SessionView> = Box::new(HostView::new(Arc::clone(&host)));
    let overlay_view: Box<dyn SessionView> =
        Box::new(OverlaidView::new(Arc::clone(&host), FxHashMap::default()));

    assert_eq!(
        host_view.source("/m.ts").as_deref(),
        Some("export const x = 1;"),
    );
    assert_eq!(
        overlay_view.source("/m.ts").as_deref(),
        Some("export const x = 1;"),
    );
}
