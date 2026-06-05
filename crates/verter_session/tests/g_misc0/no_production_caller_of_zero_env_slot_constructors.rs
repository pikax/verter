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

/// The fixture-only / zero-env slot constructors. A production-window
/// CALL to any of these (`<marker>(`) is the bypass we forbid.
const ZERO_ENV_SLOT_CONSTRUCTORS: &[&str] = &[
    "type_slot_unscoped(",
    "builtin_unscoped(",
    "to_type_slot_unscoped(",
    "from_decl_identity(",
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

/// Yields `(line_no, line)` for every line surviving the production
/// window: lines inside `impl Default for … { }` bodies and inside
/// `#[cfg(test)]` annotated items are dropped.
fn production_lines(source: &str) -> Vec<(usize, &str)> {
    let mut excluded_ranges: Vec<(usize, usize)> = Vec::new();

    for marker_idx in find_all(source, "impl Default for ") {
        if let Some(open_rel) = source[marker_idx..].find('{') {
            let open = marker_idx + open_rel;
            if let Some(close) = find_matching_close_brace(source, open) {
                excluded_ranges.push((marker_idx, close + 1));
            }
        }
    }
    for marker_idx in find_all(source, "#[cfg(test)]") {
        if let Some(open_rel) = source[marker_idx..].find('{') {
            let open = marker_idx + open_rel;
            if let Some(close) = find_matching_close_brace(source, open) {
                excluded_ranges.push((marker_idx, close + 1));
            }
        }
    }

    excluded_ranges.sort_by_key(|r| r.0);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in excluded_ranges {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    let is_excluded = |pos: usize| merged.iter().any(|(s, e)| pos >= *s && pos < *e);

    let mut line_starts: Vec<usize> = vec![0];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }

    let mut out: Vec<(usize, &str)> = Vec::new();
    for (line_no, line) in source.lines().enumerate() {
        let line_start = line_starts.get(line_no).copied().unwrap_or(0);
        if !is_excluded(line_start) {
            out.push((line_no + 1, line));
        }
    }
    out
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

/// Collect production-window calls to any zero-env slot constructor in
/// `source`, returning `(line_no, line, marker)` for each.
fn zero_env_callers(source: &str) -> Vec<(usize, String, &'static str)> {
    let mut hits = Vec::new();
    for (line_no, line) in production_lines(source) {
        for marker in ZERO_ENV_SLOT_CONSTRUCTORS {
            if line.contains(marker) {
                hits.push((line_no, line.trim().to_string(), *marker));
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
                marker.trim_end_matches('('),
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
    // The production caller is flagged. (`to_type_slot_unscoped(` also
    // contains the substring `type_slot_unscoped(`, so the single
    // production line legitimately matches two markers — assert on the
    // distinct flagged LINES, not raw hit count.)
    assert!(
        hits.iter().any(|(_, t, _)| t.contains("some_id")),
        "filter MUST flag the PRODUCTION caller (`some_id`); got: {hits:?}",
    );
    assert!(
        !hits.iter().any(|(_, t, _)| t.contains("other_id")),
        "the `#[cfg(test)]` caller (`other_id`) must NOT be flagged; got: {hits:?}",
    );
}
