//! Block 1.J.1 item 7 — semantic-graph production warm reads validated.
//!
//! Codex Block 1.J diagnosis: "production warm reads must validate
//! `ReadSetSignature`. Raw `get` should be test/debug only or renamed
//! to make that explicit."
//!
//! Block 1.I added `SemanticGraphStore::get_validated(key, ctx)` — the
//! carrier-aware warm read that validates the entry's
//! `ReadSetSignature` against the live store view BEFORE bubbling its
//! fact observations. The unvalidated raw read was renamed
//! `get` → `get_unvalidated` so the unvalidated nature is explicit at
//! every call site (codex Block 1.J item 7).
//!
//! This guard enforces that the renamed `get_unvalidated` is reachable
//! from EXACTLY ONE production file — `semantic_query_memo/mod.rs`,
//! which owns the cooperative-admission machinery whose
//! validate / remove / cold-recompute coordination happens one level
//! up. Every OTHER production warm read MUST route through
//! `get_validated`. A new seal-scope caller of `get_unvalidated` —
//! the unvalidated bubble-without-validate hole — fails this guard.
//!
//! Discrimination: this guard is writable such that planting a
//! `graph.get_unvalidated(&key)` call into any seal-scope production
//! file other than `semantic_query_memo/mod.rs` makes it FAIL. The
//! `scanner_flags_a_planted_violation` self-test exercises the scanner
//! against a synthetic violation string to prove the scan
//! discriminates rather than passing vacuously.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// The single production file sanctioned to call `get_unvalidated`:
/// `semantic_query_memo/mod.rs` defines `get_unvalidated` AND owns the
/// cooperative-admission slow path / non-admission batch probe whose
/// own coordination flow performs the validate-before-publish dance.
const SANCTIONED_FILE: &str = "crates/verter_session/src/semantic_query_memo/mod.rs";

/// Collect every production (non-test) `.rs` file under the
/// `verter_session` crate `src/` tree. `*_tests.rs` files, `tests.rs`
/// module files, and the `#[cfg(test)]`-gated `host_test_audit.rs`
/// are excluded — they are not production warm-read paths.
fn collect_production_session_src_files() -> Vec<PathBuf> {
    use walkdir::WalkDir;
    let src_root = workspace_root().join("crates/verter_session/src");
    let mut scanned = Vec::new();
    for entry in WalkDir::new(&src_root) {
        let entry = entry.expect("walkdir entry");
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        // Exclude unit-test modules: `*_tests.rs`, the per-module
        // `tests.rs`, and the `#[cfg(test)]`-gated test-audit shim.
        if file_name.ends_with("_tests.rs")
            || file_name == "tests.rs"
            || file_name == "host_test_audit.rs"
        {
            continue;
        }
        scanned.push(path.to_path_buf());
    }
    scanned
}

/// Strip `//`-prefixed line comments and `///`/`//!` doc comments from
/// a source line so a mention of `get_unvalidated` inside prose does
/// not register as a call site. Block comments are not used for this
/// identifier in the scanned files; line-comment stripping is
/// sufficient and keeps the scanner simple.
fn code_before_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(idx) => &line[..idx],
        None => line,
    }
}

/// Every production caller of `SemanticGraphStore::get_unvalidated`
/// must live in the sanctioned cooperative-admission file. A new
/// seal-scope caller is the unvalidated-warm-read hole codex item 7
/// closes.
#[test]
fn get_unvalidated_only_called_from_cooperative_admission_file() {
    let sanctioned_abs = workspace_root().join(SANCTIONED_FILE);
    let mut offenders: Vec<String> = Vec::new();

    for path in collect_production_session_src_files() {
        if path == sanctioned_abs {
            continue;
        }
        let src = fs::read_to_string(&path).expect("read source file");
        for (lineno, line) in src.lines().enumerate() {
            let code = code_before_line_comment(line);
            if code.contains(".get_unvalidated(") {
                let rel = path
                    .strip_prefix(workspace_root())
                    .unwrap_or(path.as_path())
                    .display();
                offenders.push(format!("{rel}:{}", lineno + 1));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "Production warm reads MUST route through \
         `SemanticGraphStore::get_validated` (validates `ReadSetSignature` \
         before bubbling). The unvalidated `get_unvalidated` may only be \
         called from `{SANCTIONED_FILE}` (the cooperative-admission \
         machinery). Offending call sites: {offenders:?}. \
         Either route the read through `get_validated`, or — if a \
         presence-only probe genuinely needs the unvalidated read — move \
         it behind the cooperative-admission API.",
    );
}

/// The sanctioned file MUST actually define `get_unvalidated` (the
/// rename landed) and MUST NOT still expose the old `pub fn get(`
/// signature — the rename codex item 7 mandates is "renamed to make
/// the unvalidated nature explicit".
#[test]
fn semantic_graph_store_exposes_renamed_unvalidated_read() {
    let src = fs::read_to_string(workspace_root().join(SANCTIONED_FILE))
        .expect("read semantic_query_memo/mod.rs");
    assert!(
        src.contains("pub fn get_unvalidated("),
        "SemanticGraphStore must expose the renamed `get_unvalidated` — the \
         explicit-name form codex item 7 mandates for the unvalidated read.",
    );
    assert!(
        !src.contains("pub fn get(&self, key: &SemanticQueryKey)"),
        "the old `pub fn get(&self, key: &SemanticQueryKey)` signature must \
         be gone — renamed to `get_unvalidated` so the unvalidated nature \
         is explicit at every call site.",
    );
    // The validated read must still exist — it is the production
    // warm-read entry point every seal-scope caller routes through.
    assert!(
        src.contains("pub(crate) fn get_validated("),
        "SemanticGraphStore must expose `get_validated` — the production \
         warm-read entry point that validates `ReadSetSignature` before \
         bubbling.",
    );
}

/// Self-test: the scanner discriminates. A synthetic source string
/// containing a `get_unvalidated` call (outside a comment) MUST be
/// flagged; the same call commented out MUST NOT. Without this, a
/// scanner that silently matched nothing — or matched comment prose —
/// would pass vacuously.
#[test]
fn scanner_flags_a_planted_violation() {
    // A planted violation: a bare `get_unvalidated` call in code.
    let violating = "fn probe() { let _ = graph.get_unvalidated(&key); }";
    assert!(
        code_before_line_comment(violating).contains(".get_unvalidated("),
        "scanner self-test: a real `.get_unvalidated(` call site MUST be \
         detected — if not, the production scan above passes vacuously",
    );

    // The same call fully inside a line comment MUST NOT register.
    let commented = "// historical: graph.get_unvalidated(&key) was the hole";
    assert!(
        !code_before_line_comment(commented).contains(".get_unvalidated("),
        "scanner self-test: a `.get_unvalidated(` mention inside a line \
         comment MUST NOT register as a call site — otherwise doc prose \
         would false-trigger the guard",
    );

    // Trailing-comment case: code before the comment is still scanned.
    let trailing = "let _ = graph.get_unvalidated(&key); // sanctioned probe";
    assert!(
        code_before_line_comment(trailing).contains(".get_unvalidated("),
        "scanner self-test: a `.get_unvalidated(` call with a trailing \
         comment MUST still be detected",
    );

    // Sanity: the production scan actually examined a non-empty file
    // set — a scan over zero files would also report "no offenders".
    let scanned = collect_production_session_src_files();
    assert!(
        scanned.len() > 50,
        "scanner self-test: the production file scan must cover the \
         verter_session src tree (got {} files) — a near-empty scan \
         would make the guard vacuous",
        scanned.len(),
    );
    // The sanctioned file itself must be present on disk.
    assert!(
        Path::new(&workspace_root().join(SANCTIONED_FILE)).is_file(),
        "the sanctioned cooperative-admission file must exist",
    );
}
