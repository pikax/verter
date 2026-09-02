//! `lightningcss` and the legacy `crates/verter_compiler/src/css/` pipeline
//! built on it are REMOVED, not shimmed. `verter_css_syntax` (via
//! `style_planner`) is the single CSS-family syntax authority.
//!
//! Two independent negative proofs live here:
//! - A1: the resolved workspace dependency graph carries no `lightningcss`
//!   package node (`cargo metadata --format-version=1`).
//! - A2: `crates/verter_compiler/src/css/` does not exist on disk.
//!
//! A4's executable path-absence half for the remaining Vue-side grammar
//! (row 3) is the same `css/` check. The Svelte-side grammar file
//! (`src/svelte/runtime/css/parse.rs`) is re-asserted here so this file
//! covers both halves of A4's path-absence split; the Svelte-only guard
//! in `svelte_css_grammar_path_absence.rs` stays the dedicated Svelte
//! regression.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Repo root = parent of `crates/` (this crate's manifest dir is
/// `crates/verter_compiler`).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// Locate the cargo binary: prefer the `CARGO` env var cargo sets during a
/// test run; otherwise fall back to `cargo` on PATH. Cross-platform — no
/// hardcoded per-OS binary name.
fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// The resolved workspace dependency graph (full, WITH deps — a
/// `lightningcss` node could only appear transitively through the graph, not
/// the direct-member list `--no-deps` would show) carries no `lightningcss`
/// package.
#[test]
fn a1_resolved_dependency_graph_carries_no_lightningcss_package() {
    let output = Command::new(cargo_bin())
        .args(["metadata", "--format-version", "1"])
        .current_dir(workspace_root())
        .output()
        .expect("run `cargo metadata`");
    assert!(
        output.status.success(),
        "`cargo metadata` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse `cargo metadata` output");
    let packages = json
        .get("packages")
        .and_then(|v| v.as_array())
        .expect("`cargo metadata` packages array");
    assert!(
        !packages.is_empty(),
        "`cargo metadata` returned an empty packages array — an empty graph would make the          lightningcss-absence check vacuously true"
    );
    let names: Vec<&str> = packages
        .iter()
        .filter_map(|pkg| pkg.get("name").and_then(|n| n.as_str()))
        .collect();
    assert!(
        names.contains(&"verter_compiler"),
        "the metadata graph must include this workspace crate as a positive control, got {names:?}"
    );
    let lightningcss_nodes: Vec<&str> = packages
        .iter()
        .filter_map(|p| p.get("name").and_then(|n| n.as_str()))
        .filter(|name| *name == "lightningcss")
        .collect();
    assert!(
        lightningcss_nodes.is_empty(),
        "the resolved dependency graph still carries a `lightningcss` package node — the \
         dependency and the legacy css/ pipeline built on it must be removed together, not left \
         half-wired: {lightningcss_nodes:?}"
    );
}

/// `crates/verter_compiler/src/css/` — the legacy lightningcss-backed
/// pipeline — does not exist on disk. Raw `std::fs` path-absence, not a
/// registered-facts read: this guard's whole job IS to prove the directory
/// is gone, so it must check the filesystem directly.
#[test]
fn a2_legacy_css_module_directory_does_not_exist() {
    let doomed = workspace_root().join("crates/verter_compiler/src/css");
    assert!(
        !Path::new(&doomed).exists(),
        "{} must not exist — the legacy lightningcss-backed CSS pipeline was deleted in the same \
         change that removed the `lightningcss` dependency",
        doomed.display()
    );
}

/// A4 / A11a executable half: Svelte's own grammar file stays deleted.
#[test]
fn a4_svelte_css_grammar_parse_rs_is_absent() {
    let doomed = workspace_root().join("crates/verter_compiler/src/svelte/runtime/css/parse.rs");
    assert!(
        !Path::new(&doomed).exists(),
        "{} must stay deleted — Svelte CSS parses exclusively through StyleSyntaxIr",
        doomed.display()
    );
}

/// Discrimination: prove the assertion actually distinguishes "absent" from
/// "present" — a path this test knows is currently present (this very test
/// file) must NOT be reported as absent by the same existence check the
/// production assertions above use.
#[test]
fn discrimination_a_present_file_is_not_reported_absent() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/cases/no_lightningcss_dependency.rs");
    assert!(
        path.exists(),
        "the discrimination fixture itself must exist: {}",
        path.display()
    );
}
