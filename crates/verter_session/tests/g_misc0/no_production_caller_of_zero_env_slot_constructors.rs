//! ARCH GUARD — no production caller of the zero-env / fixture-only
//! [`ResolvedDeclSlotIdentity`] slot constructors.
//!
//! The "one-path slot derivation" contract (Cache Architecture
//! (CRITICAL), R6 / R7) requires that EVERY production slot identity
//! for the `Instantiate` / `ResolveMacroPayload` query keys is derived
//! through the shared, env-bearing helpers
//! `ProjectSemanticDispatch::type_slot_for(...)` /
//! `ProjectSemanticDispatch::builtin_type_slot(...)`, which read the
//! live host env (`project_identity` / `type_env_hash` / `lib_env_hash`).
//!
//! The lib also exposes ZERO-env / fixture-only constructors that mint a
//! slot with all env dims defaulted:
//!
//! - `ResolvedDeclSlotIdentity::type_slot_unscoped(...)`
//! - `ResolvedDeclSlotIdentity::builtin_unscoped(...)`
//! - `ResolvedDeclSlotIdentity::from_decl_identity(...)`
//! - `DeclIdentity::to_type_slot_unscoped(...)`
//!
//! These CANNOT be `#[cfg(test)]`-gated in the lib because external
//! `tests/` integration crates consume them (a `cfg(test)` item in the
//! lib is invisible to an external test crate). They are PUBLIC by
//! necessity — so the one-path contract is instead pinned by this
//! discriminating source scan: any PRODUCTION caller is a bypass of the
//! shared `type_slot_for` / `builtin_type_slot` derivation and FAILS this
//! guard.
//!
//! Scan scope (mirrors the sibling
//! `no_default_env_hashes_in_production` /
//! `semantic_graph_production_reads_validated` production-window
//! conventions):
//! - Walk every `crates/*/src/**/*.rs` file.
//! - Exclude `tests/` directories.
//! - Exclude `*_tests.rs` files AND the per-module `tests.rs` files
//!   (extracted `#[cfg(test)] mod tests;` / `mod …_tests;` unit modules
//!   — they are pulled in only under `#[cfg(test)]` at the parent `mod`
//!   declaration, so the `#[cfg(test)]` gate is NOT inside the file).
//! - Exclude files under a `tests/` or `*_tests/` SUBDIRECTORY (e.g.
//!   `typeinfo/typeinfo_tests/*.rs`, included via `#[cfg(test)] mod
//!   typeinfo_tests;`).
//! - Exclude `#[cfg(test)]` blocks inside an otherwise-production file
//!   (the in-crate `#[cfg(test)] mod tests { … }` callers in
//!   `component_meta_materialize.rs` / `loop5_instrumentation.rs` live
//!   under `#[cfg(test)]` and are excluded by this window).
//! - Exclude `impl Default` bodies.
//! - Exclude `semantic_query.rs`'s OWN constructor definitions (the
//!   `pub fn type_slot_unscoped` / `builtin_unscoped` / `from_decl_identity`
//!   /`to_type_slot_unscoped` DEFINITIONS textually contain the marker;
//!   a definition is not a caller).
//!
//! Discrimination: the guard PASSES on the current tree (every caller of
//! these constructors is test-only — verified cold). The negative-control
//! test
//! `window_filter_flags_synthetic_production_caller_and_skips_test_caller`
//! plants a synthetic production-window call and asserts the scan flags
//! it, while a sibling `#[cfg(test)]` call is skipped — so a regression
//! to the window filter that masks a real production caller is itself
//! caught.

use std::fs;
use std::path::{Path, PathBuf};

/// The fixture-only / zero-env slot constructor NAMES. A production-window
/// CALL to any of these — the name with a non-identifier boundary before it
/// (so `to_type_slot_unscoped` matches on its OWN boundary and does not
/// false-match inside an unrelated identifier), then optional
/// whitespace/comments, then `(` — is the bypass we forbid.
///
/// The trailing `(` is NOT part of the marker (Finding 2 hardening): a
/// production call written `type_slot_unscoped (` (space before paren) or
/// `type_slot_unscoped\n(` (newline) must still be caught, so the paren is
/// matched after comment-stripping + whitespace normalization rather than
/// as a literal suffix of the marker.
const ZERO_ENV_SLOT_CONSTRUCTORS: &[&str] = &[
    "type_slot_unscoped",
    "builtin_unscoped",
    "to_type_slot_unscoped",
    "from_decl_identity",
];

/// Files we always skip outright (basename match). `semantic_query.rs`
/// is where the constructors are DEFINED — the `pub fn …(` definition
/// text contains the marker but is not a caller; the definitions are
/// pinned content-free by the R6 guard, and every external caller is
/// covered by the tree walk.
const SKIP_FILES: &[&str] = &[
    "no_production_caller_of_zero_env_slot_constructors.rs",
    "semantic_query.rs",
];

fn is_excluded_path(path: &Path) -> bool {
    // Any directory component named `tests` or ending in `_tests` is a
    // `#[cfg(test)] mod …` unit-test module tree (e.g. `tests/`,
    // `typeinfo_tests/`). Files inside such a directory are test-only —
    // the `#[cfg(test)]` gate is on the parent `mod` declaration, not in
    // the file body, so an in-file scan cannot see it.
    for comp in path.components() {
        if let std::path::Component::Normal(os) = comp {
            if let Some(name) = os.to_str() {
                if name == "tests" || name.ends_with("_tests") {
                    return true;
                }
            }
        }
    }
    let basename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    // Extracted unit-test module files: `*_tests.rs` and the per-module
    // `tests.rs` (pulled in via `#[cfg(test)] mod tests;`).
    if (basename.contains("_tests") || basename == "tests.rs") && basename.ends_with(".rs") {
        return true;
    }
    SKIP_FILES.iter().any(|f| basename.ends_with(f))
}

/// The merged, sorted byte ranges of the production-window EXCLUSIONS:
/// `impl Default for … { }` bodies and `#[cfg(test)]` annotated items.
/// A byte offset inside any returned `(start, end)` range is test-only /
/// non-production and must be skipped by the caller scan.
fn excluded_ranges(source: &str) -> Vec<(usize, usize)> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();

    for marker_idx in find_all(source, "impl Default for ") {
        if let Some(open_rel) = source[marker_idx..].find('{') {
            let open = marker_idx + open_rel;
            if let Some(close) = find_matching_close_brace(source, open) {
                ranges.push((marker_idx, close + 1));
            }
        }
    }
    for marker_idx in find_all(source, "#[cfg(test)]") {
        if let Some(open_rel) = source[marker_idx..].find('{') {
            let open = marker_idx + open_rel;
            if let Some(close) = find_matching_close_brace(source, open) {
                ranges.push((marker_idx, close + 1));
            }
        }
    }

    ranges.sort_by_key(|r| r.0);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

fn find_all(haystack: &str, needle: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(rel) = haystack[cursor..].find(needle) {
        let abs = cursor + rel;
        out.push(abs);
        cursor = abs + needle.len();
    }
    out
}

fn find_matching_close_brace(source: &str, open_idx: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth: i32 = 0;
    let mut idx = open_idx;
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
        idx += 1;
    }
    None
}

fn walk_crate_src_files(crates_dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(crates_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let src = path.join("src");
            if src.is_dir() {
                walk_rs_files(&src, &mut out);
            }
        }
    }
    out
}

fn walk_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk_rs_files(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}

/// Skip a run of ASCII whitespace AND `//` line / `/* */` block comments
/// starting at `pos` in `bytes`, returning the index of the first
/// significant (non-whitespace, non-comment) byte. This lets the call
/// detector tolerate ANY formatting between the constructor name and its
/// `(` — `name (`, `name\n(`, `name /*x*/(` — without false-matching.
fn skip_ws_and_comments(bytes: &[u8], mut pos: usize) -> usize {
    loop {
        // Whitespace.
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        // Line comment.
        if pos + 1 < bytes.len() && bytes[pos] == b'/' && bytes[pos + 1] == b'/' {
            pos += 2;
            while pos < bytes.len() && bytes[pos] != b'\n' {
                pos += 1;
            }
            continue;
        }
        // Block comment.
        if pos + 1 < bytes.len() && bytes[pos] == b'/' && bytes[pos + 1] == b'*' {
            pos += 2;
            while pos + 1 < bytes.len() && !(bytes[pos] == b'*' && bytes[pos + 1] == b'/') {
                pos += 1;
            }
            pos = (pos + 2).min(bytes.len());
            continue;
        }
        return pos;
    }
}

/// True iff `name` occurs at `idx` in `bytes` with a LEADING non-identifier
/// boundary (so `to_type_slot_unscoped` is NOT matched inside another
/// identifier when scanning for `type_slot_unscoped`, and vice-versa), a
/// TRAILING non-identifier boundary (so `type_slot_unscoped_v2` does not
/// match `type_slot_unscoped`), and — after skipping whitespace/comments —
/// is immediately followed by `(` (so it is a CALL, not a bare name).
fn is_boundaried_call_at(bytes: &[u8], idx: usize, name: &[u8]) -> bool {
    // Leading boundary.
    if idx > 0 {
        let prev = bytes[idx - 1];
        if prev.is_ascii_alphanumeric() || prev == b'_' {
            return false;
        }
    }
    let after = idx + name.len();
    // Trailing identifier boundary: the char right after the name must not
    // continue an identifier (rejects `type_slot_unscoped_v2`).
    if let Some(&next) = bytes.get(after) {
        if next.is_ascii_alphanumeric() || next == b'_' {
            return false;
        }
    }
    // After optional whitespace/comments, the next significant char is `(`.
    let call_at = skip_ws_and_comments(bytes, after);
    bytes.get(call_at) == Some(&b'(')
}

/// Collect production-window calls to any zero-env slot constructor in
/// `source`, returning `(line_no, line, marker)` for each.
///
/// Matching is boundary-aware and tolerates arbitrary whitespace/comments
/// before the `(` (Finding 2 hardening): the line-based `line.contains(...)`
/// scan was evadable by `name (` / `name\n(`. The window-exclusion ranges
/// (`impl Default` / `#[cfg(test)]`) are computed once over the raw source;
/// each match's byte offset is tested against them so a call whose `(` lands
/// on a different line than the name is still attributed and filtered.
fn zero_env_callers(source: &str) -> Vec<(usize, String, &'static str)> {
    let bytes = source.as_bytes();
    let excluded = excluded_ranges(source);
    let is_excluded = |pos: usize| excluded.iter().any(|(s, e)| pos >= *s && pos < *e);

    // Precompute line-start offsets for byte-offset → line-number mapping.
    let mut line_starts: Vec<usize> = vec![0];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }
    let line_no_of = |pos: usize| -> usize {
        match line_starts.binary_search(&pos) {
            Ok(i) => i + 1,
            Err(i) => i, // i = number of line-starts <= pos
        }
    };
    let line_text_of = |line_no: usize| -> String {
        source
            .lines()
            .nth(line_no.saturating_sub(1))
            .unwrap_or_default()
            .trim()
            .to_string()
    };

    let mut hits = Vec::new();
    for marker in ZERO_ENV_SLOT_CONSTRUCTORS {
        let needle = marker.as_bytes();
        let mut cursor = 0usize;
        while let Some(rel) = source[cursor..].find(marker) {
            let idx = cursor + rel;
            cursor = idx + needle.len();
            if is_excluded(idx) {
                continue;
            }
            if is_boundaried_call_at(bytes, idx, needle) {
                let line_no = line_no_of(idx);
                hits.push((line_no, line_text_of(line_no), *marker));
            }
        }
    }
    hits
}

#[test]
fn no_production_caller_of_zero_env_slot_constructors() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = crate_root
        .parent()
        .and_then(Path::parent)
        .expect("verter_session lives at crates/verter_session under workspace root");
    let crates_dir = workspace_root.join("crates");
    assert!(
        crates_dir.is_dir(),
        "arch guard fixture invariant: `{}` MUST exist",
        crates_dir.display(),
    );

    let mut violations: Vec<String> = Vec::new();
    for file in walk_crate_src_files(&crates_dir) {
        if is_excluded_path(&file) {
            continue;
        }
        let source = match fs::read_to_string(&file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        for (line_no, line, marker) in zero_env_callers(&source) {
            violations.push(format!(
                "{}:{} — production call to zero-env slot constructor `{}`\n    {}",
                file.display(),
                line_no,
                marker,
                line,
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "ONE-PATH SLOT DERIVATION VIOLATION — production code MUST derive \
         the `Instantiate` / `ResolveMacroPayload` slot identity via the \
         shared, env-bearing `ProjectSemanticDispatch::type_slot_for` / \
         `ProjectSemanticDispatch::builtin_type_slot` (which read the live \
         host `project_identity` / `type_env_hash` / `lib_env_hash`). The \
         zero-env `type_slot_unscoped` / `builtin_unscoped` / \
         `to_type_slot_unscoped` / `from_decl_identity` constructors are \
         TEST-FIXTURE ONLY and mint an all-zero-env slot — calling them \
         from production bypasses env separation. {} violation(s):\n{}",
        violations.len(),
        violations.join("\n"),
    );
}

#[test]
fn window_filter_flags_synthetic_production_caller_and_skips_test_caller() {
    // Negative control: a synthetic fixture must FLAG a production-window
    // call to a zero-env constructor and SKIP a sibling `#[cfg(test)]`
    // call. If the window filter regressed (e.g. stopped excluding
    // `#[cfg(test)]`, or stopped scanning production lines at all), this
    // discriminating control fails.
    let fixture = r#"
fn production_caller() {
    let base = some_id.to_type_slot_unscoped();
    let _ = base;
}

#[cfg(test)]
mod tests {
    fn test_caller() {
        let owner = other_id.to_type_slot_unscoped();
        let _ = owner;
    }
}
"#;
    let hits = zero_env_callers(fixture);
    // The production caller is flagged. Boundary-aware matching attributes
    // `to_type_slot_unscoped(` to the `to_type_slot_unscoped` marker only —
    // the embedded `type_slot_unscoped` substring is rejected by the leading
    // identifier boundary (`_` of `to_`) — so assert on the distinct flagged
    // LINE, not raw hit count.
    assert!(
        hits.iter().any(|(_, t, _)| t.contains("some_id")),
        "filter MUST flag the PRODUCTION caller (`some_id`); got: {hits:?}",
    );
    assert!(
        !hits.iter().any(|(_, t, _)| t.contains("other_id")),
        "the `#[cfg(test)]` caller (`other_id`) must NOT be flagged; got: {hits:?}",
    );
}

/// Finding 2 hardening discrimination: the call detector must catch a
/// production caller written with a SPACE before the paren
/// (`type_slot_unscoped (planted)`) — the exact evasion that defeated the
/// old `line.contains("type_slot_unscoped(")` literal-suffix scan — and a
/// NEWLINE before the paren, while still rejecting a bare mention with no
/// `(` and still skipping a `#[cfg(test)]` caller.
#[test]
fn call_detector_catches_whitespace_before_paren_and_rejects_bare_mention() {
    // Space before paren — MUST flag.
    let space_fixture = r#"
fn production_caller() {
    let base = type_slot_unscoped (planted_arg);
    let _ = base;
}
"#;
    let hits = zero_env_callers(space_fixture);
    assert!(
        hits.iter().any(|(_, t, _)| t.contains("planted_arg")),
        "detector MUST flag `type_slot_unscoped (` with a space before the \
         paren; got: {hits:?}",
    );

    // Newline before paren — MUST flag.
    let newline_fixture = "fn p() {\n    let _ = builtin_unscoped\n        (planted_nl);\n}\n";
    let nl_hits = zero_env_callers(newline_fixture);
    assert!(
        nl_hits.iter().any(|(_, _, m)| *m == "builtin_unscoped"),
        "detector MUST flag `builtin_unscoped` with a newline before the \
         paren; got: {nl_hits:?}",
    );

    // Bare mention with NO call paren — MUST NOT flag (it is not a caller).
    let bare_fixture = "fn p() {\n    // type_slot_unscoped is fixture-only\n    let type_slot_unscoped_ref = 1;\n}\n";
    let bare_hits = zero_env_callers(bare_fixture);
    assert!(
        bare_hits.is_empty(),
        "a non-call mention / longer identifier (`type_slot_unscoped_ref`) \
         must NOT be flagged; got: {bare_hits:?}",
    );

    // `#[cfg(test)]` caller with a space before the paren — MUST be skipped
    // by the window filter even though the new whitespace-tolerant matcher
    // would otherwise catch it.
    let test_window_fixture = r#"
#[cfg(test)]
mod tests {
    fn t() {
        let _ = type_slot_unscoped (planted_test);
    }
}
"#;
    let tw_hits = zero_env_callers(test_window_fixture);
    assert!(
        tw_hits.is_empty(),
        "a `#[cfg(test)]`-window caller (even with whitespace before the \
         paren) must be skipped; got: {tw_hits:?}",
    );
}
