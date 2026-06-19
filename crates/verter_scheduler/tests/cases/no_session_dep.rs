//! H20 — `verter_scheduler` must NOT depend on `verter_session`.
//!
//! The dependency direction is one-way: `verter_session → verter_scheduler`.
//! Re-introducing `verter_session` as a dependency of the scheduler crate
//! is a cycle and a violation of the leaf-substrate rule. The leaf
//! modules (`cancellation`, `cpu_concurrency`, `cache_id`, `dedupe_hook`)
//! must stay pure scheduler substrate with no back-edge to the session.
//!
//! Two complementary discriminators:
//!
//! 1. `Cargo.toml` must not list `verter_session` under any dependency
//!    table (`[dependencies]`, `[dev-dependencies]`, target-specific, or
//!    build deps).
//! 2. The crate source must not contain a `verter_session` path
//!    reference (no `use verter_session`, no `verter_session::` path).

use std::fs;
use std::path::{Path, PathBuf};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Reads every `.rs` file under `src/`, stripping line comments and
/// doc comments so the scan discriminates a *code-path* dependency
/// (`use verter_session`, a `verter_session::` path) from prose mentions
/// of the H20 boundary in `///` / `//!` / `//` comments. The crate's many
/// doc comments legitimately *describe* the session-side counterparts
/// (`verter_session::request_context`, etc.) without depending on them.
fn read_src_tree_code_only() -> String {
    let mut buf = String::new();
    walk(&manifest_dir().join("src"), &mut buf);
    buf
}

fn walk(dir: &Path, buf: &mut String) {
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            walk(&path, buf);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let raw = fs::read_to_string(&path).expect("read file");
            for line in raw.lines() {
                buf.push_str(strip_comment(line));
                buf.push('\n');
            }
        }
    }
}

/// Drops the `//`-introduced comment tail (covering `//`, `///`, `//!`).
/// Block comments are not used for `verter_session` prose in this crate;
/// a naive `//` split is sufficient and conservative (it only ever
/// removes text, never adds a false code reference).
fn strip_comment(line: &str) -> &str {
    match line.find("//") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

/// Drops the `#`-introduced TOML comment tail. A commented-out dependency
/// line (`# verter_session = ...`) must NOT false-fire the manifest scan,
/// mirroring the `//`-stripping applied to the src scan. Conservative: it
/// only ever removes text, never adds a false dependency reference.
fn strip_toml_comment(line: &str) -> &str {
    match line.find('#') {
        Some(idx) => &line[..idx],
        None => line,
    }
}

/// H20 (BLOCKING): the scheduler `Cargo.toml` must not name
/// `verter_session` in any dependency table. The scan strips `#` TOML
/// comments first, so a commented-out `# verter_session` line cannot
/// false-fire — only a live dependency entry trips the guard.
#[test]
fn scheduler_does_not_depend_on_verter_session() {
    let raw = fs::read_to_string(manifest_dir().join("Cargo.toml")).expect("read Cargo.toml");
    let manifest: String = raw
        .lines()
        .map(strip_toml_comment)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !manifest.contains("verter_session"),
        "H20: `verter_scheduler/Cargo.toml` must NOT name `verter_session` — \
         the dependency direction is `verter_session → verter_scheduler`, never the reverse",
    );
}

/// H20 (BLOCKING): the scheduler crate source — including the leaf
/// modules — must not reference a `verter_session` path. The leaf
/// substrate stays domain-agnostic; the session-side concretes wrap
/// scheduler-owned opaque carriers, not the other way around.
#[test]
fn scheduler_src_has_no_verter_session_path() {
    let code = read_src_tree_code_only();
    // A code-path dependency would show up as an import or a `::`-rooted
    // path in non-comment source. Doc/line comments that merely describe
    // the H20 boundary are stripped above, so any remaining hit is real.
    assert!(
        !code.contains("use verter_session"),
        "H20: scheduler src has a `use verter_session` import — \
         the leaf substrate must stay pure scheduler-owned",
    );
    assert!(
        !code.contains("verter_session::"),
        "H20: scheduler src references a `verter_session::` code path — \
         the dependency direction is `verter_session → verter_scheduler`, \
         never the reverse",
    );
}
