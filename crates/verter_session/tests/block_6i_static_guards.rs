//! Block 6.i — static architecture guards.
//!
//! Companion to `block_6i_runtime_arch_guards.rs`. These are cheap
//! source-text scans that catch regressions on the architectural
//! invariants Commits A → F establish at the projector / registry /
//! cache / NAPI boundaries.
//!
//! Each guard is named after the commit that introduces it; the
//! commit that lands the rule MUST also un-ignore the guard.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest_dir)
}

fn read_workspace_file(rel: &str) -> String {
    fs::read_to_string(workspace_root().join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

// ---------------------------------------------------------------------------
// Guard A.1 — `collect_component_meta_registry_refs` accepts a
//             `ProjectionCursor` parameter.
//
// Block 6.i Commit A introduces the path-precise projection demand
// substrate (`crates/verter_session/src/meta_resolve/projection_demand.rs`).
// The registry walker MUST thread this cursor through its recursive
// descent — landing the signature is the load-bearing structural
// change. Without it, post-A path-precision is impossible.
// ---------------------------------------------------------------------------
#[test]
fn collect_component_meta_registry_refs_requires_cursor() {
    let src =
        read_workspace_file("crates/verter_session/src/resolver_core/component_meta_registry.rs");

    // Locate the function signature.
    let signature_idx = src
        .find("pub(crate) fn collect_component_meta_registry_refs(")
        .expect(
            "guard A.1: function `collect_component_meta_registry_refs` must exist in \
             `crates/verter_session/src/resolver_core/component_meta_registry.rs`",
        );

    // Bound the search to the signature header (between `(` and `)`).
    let header_start = signature_idx;
    let header_end = src[header_start..]
        .find(") {")
        .map(|i| header_start + i)
        .expect("guard A.1: function signature must close with `) {`");
    let header = &src[header_start..header_end];

    assert!(
        header.contains("ProjectionCursor"),
        "guard A.1: `collect_component_meta_registry_refs` MUST accept a \
         `ProjectionCursor<'_>` parameter in its signature so Block 6.i \
         Commit A's path-precise registry walk threads through. Header:\n{header}",
    );

    // Soft-check: the parameter name `cursor` appears so callers can
    // rely on a consistent forwarding name.
    assert!(
        header.contains("cursor: ") || header.contains("_cursor: "),
        "guard A.1: registry walker's projection-cursor parameter should be named \
         `cursor` (or `_cursor` if temporarily unused). Header:\n{header}",
    );
}

// ---------------------------------------------------------------------------
// Guard A.2 — `projection_demand` module exists with the substrate types.
//
// `SurfaceProjection`, `ProjectionNode`, `KeyFilter`, `PathSegment`
// (re-used from `semantic_query`), `ProjectionCursor`,
// `PublishedSurfaceKind` are the Block 6.i Commit A architectural
// vocabulary. The module being present + naming all of these is the
// minimal contract that subsequent commits depend on.
// ---------------------------------------------------------------------------
#[test]
fn projection_demand_substrate_present() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/projection_demand.rs");

    for symbol in [
        "pub(crate) struct SurfaceProjection",
        "pub(crate) struct ProjectionNode",
        "pub(crate) enum KeyFilter",
        "pub(crate) enum PublishedSurfaceKind",
        "pub(crate) struct ProjectionCursor",
        "pub(crate) fn descend",
        "pub(crate) fn is_terminal",
        "pub(crate) fn admits_key",
    ] {
        assert!(
            src.contains(symbol),
            "guard A.2: `projection_demand` module MUST declare `{symbol}`",
        );
    }
}
