//! Block 1.6 — ARCH GUARD — no `EnvHashes::default()` or
//! `ProjectIdentity([0u8; 16])` in production code paths.
//!
//! Plan citation: `D:/tmp/verter-stage7-final-cutover-plan.md` plan
//! invariant #2: "No `EnvHashes::default()` / `ProjectIdentity([0u8; 16])`
//! in production code." Block 1.6 plumbs real env-hash + project-identity
//! values through `host_view_env_hashes_for` / `host_view_project_identity_for`
//! so the all-zero placeholders never appear at request-time in
//! production. This guard locks the invariant.
//!
//! Scan scope:
//! - Walk every `crates/*/src/**/*.rs` file.
//! - Exclude `tests/` directories — integration tests legitimately
//!   construct zero-hash fixtures.
//! - Exclude file paths matching `*_tests*.rs` (inline-extracted unit
//!   test modules).
//! - Exclude `for_tests` modules — Rust convention for test-only
//!   helpers visible across the crate.
//! - Exclude `impl Default` bodies and `#[cfg(test)]` blocks — both
//!   represent test-aware code paths the brief calls out explicitly.
//!
//! Forbidden patterns inside the remaining production windows:
//! - `EnvHashes::default()`
//! - `ProjectIdentity([0u8; 16])` (and its variants with optional
//!   whitespace around the array literal).
//!
//! Discrimination chain: the guard FAILS the moment any future commit
//! introduces a new occurrence of either pattern inside a production
//! window. It PASSES against the post-Block-1.6-GREEN tree because the
//! Block 1.6 cleanup replaced the only production callers
//! (`HostView::new`, `OverlaidView::new`, `HostViewRef::new`,
//! `OverlaidViewRef::new`, `*::project_identity()` accessors) with
//! workspace-default-derived values.
//!
//! Negative-control sanity: the guard's filter logic is exercised in
//! `arch_guard_window_filter_recognises_default_impl_and_cfg_test` so
//! a regression to the filter (e.g., accidentally treating a Default
//! body as production code) is caught discriminatingly.

use std::fs;
use std::path::{Path, PathBuf};

/// Files we always skip outright (paths relative to the repo root).
const SKIP_FILES: &[&str] = &[
    // This test file itself contains the forbidden tokens inside string
    // literals — exclude by name so the scanner does not flag itself.
    "no_default_env_hashes_in_production.rs",
];

/// Substring patterns used to identify forbidden source occurrences.
const FORBIDDEN_ENV_HASHES: &str = "EnvHashes::default()";
const FORBIDDEN_PROJECT_IDENTITY_BARE: &str = "ProjectIdentity([0u8; 16])";
const FORBIDDEN_PROJECT_IDENTITY_NOSPACE: &str = "ProjectIdentity([0u8;16])";

/// Yields `(line_index, line)` for every line in `source` that survives
/// the production-window filter:
///
/// 1. Skip any block from `impl Default for ... {` to its matching `}`.
/// 2. Skip any `#[cfg(test)]` annotated item (module / impl / fn).
fn production_lines(source: &str) -> Vec<(usize, &str)> {
    let mut out: Vec<(usize, &str)> = Vec::new();
    let bytes = source.as_bytes();
    let mut idx = 0usize;
    let mut line_no = 0usize;

    // Pre-compute the line starts so we can map an `idx` back to a line.
    let mut line_starts: Vec<usize> = vec![0];
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'\n' {
            line_starts.push(i + 1);
        }
    }
    let line_for_idx = |idx: usize| -> usize {
        match line_starts.binary_search(&idx) {
            Ok(line) => line,
            Err(insert) => insert.saturating_sub(1),
        }
    };

    // Build a set of byte ranges to EXCLUDE (Default impls and #[cfg(test)] blocks).
    let mut excluded_ranges: Vec<(usize, usize)> = Vec::new();

    // 1) Find every `impl Default for ... {` and skip until its matching `}`.
    for marker_idx in find_all(source, "impl Default for ") {
        if let Some(open_rel) = source[marker_idx..].find('{') {
            let open = marker_idx + open_rel;
            if let Some(close) = find_matching_close_brace(source, open) {
                excluded_ranges.push((marker_idx, close + 1));
            }
        }
    }

    // 2) Find every `#[cfg(test)]` annotation and skip the annotated item.
    for marker_idx in find_all(source, "#[cfg(test)]") {
        if let Some(open_rel) = source[marker_idx..].find('{') {
            let open = marker_idx + open_rel;
            if let Some(close) = find_matching_close_brace(source, open) {
                excluded_ranges.push((marker_idx, close + 1));
            }
        }
    }

    // Sort and merge excluded ranges.
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

    let is_excluded = |pos: usize| -> bool {
        for (start, end) in &merged {
            if pos >= *start && pos < *end {
                return true;
            }
        }
        false
    };

    for line in source.lines() {
        let line_start = if line_no < line_starts.len() {
            line_starts[line_no]
        } else {
            idx
        };
        if !is_excluded(line_start) {
            out.push((line_no + 1, line));
        }
        idx = line_start + line.len() + 1;
        line_no += 1;
    }

    // line_for_idx unused now; keep helper for future expansion.
    let _ = line_for_idx;

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

fn is_in_for_tests_module(_path: &Path) -> bool {
    // Future-proof hook for `for_tests` module-name detection. The
    // brief mentions excluding `for_tests` modules; currently the
    // codebase has none under `crates/*/src/`. Treat it as a structural
    // probe for the test-mode marker rather than a path filter — if a
    // module is named `for_tests` inside a file, the `#[cfg(test)]`
    // path catches it. Returning `false` here is a no-op.
    false
}

fn is_excluded_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    if s.contains("/tests/") || s.contains("\\tests\\") {
        return true;
    }
    let basename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if basename.contains("_tests") && basename.ends_with(".rs") {
        return true;
    }
    if SKIP_FILES.iter().any(|f| basename.ends_with(f)) {
        return true;
    }
    is_in_for_tests_module(path)
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

#[test]
fn no_default_env_hashes_in_production_code() {
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
        for (line_no, line) in production_lines(&source) {
            if line.contains(FORBIDDEN_ENV_HASHES) {
                violations.push(format!(
                    "{}:{} — forbidden token `EnvHashes::default()` in production code\n    {}",
                    file.display(),
                    line_no,
                    line.trim(),
                ));
            }
            if line.contains(FORBIDDEN_PROJECT_IDENTITY_BARE)
                || line.contains(FORBIDDEN_PROJECT_IDENTITY_NOSPACE)
            {
                violations.push(format!(
                    "{}:{} — forbidden token `ProjectIdentity([0u8; 16])` in production code\n    {}",
                    file.display(),
                    line_no,
                    line.trim(),
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Plan invariant #2: production code paths MUST NOT use `EnvHashes::default()` or \
         `ProjectIdentity([0u8; 16])`. Use `VerterHost::host_view_env_hashes` / \
         `host_view_env_hashes_for(canonical)` / `host_view_project_identity` / \
         `host_view_project_identity_for(canonical)` instead. {} violation(s):\n{}",
        violations.len(),
        violations.join("\n"),
    );
}

#[test]
fn arch_guard_window_filter_recognises_default_impl_and_cfg_test() {
    // Negative control: a synthetic fixture must classify a Default impl
    // body and a #[cfg(test)] block as EXCLUDED, and a sibling production
    // statement as INCLUDED. Catches a regression to the filter (e.g.,
    // accidentally treating a Default body as production code) that
    // would mask real violations.
    let fixture = r#"
impl Default for Foo {
    fn default() -> Self {
        let x = EnvHashes::default();
        Self { x }
    }
}

#[cfg(test)]
mod tests {
    fn helper() {
        let y = ProjectIdentity([0u8; 16]);
    }
}

fn production_fn() {
    let _z = EnvHashes::default();
}
"#;
    let surviving: Vec<&str> = production_lines(fixture).iter().map(|(_, l)| *l).collect();
    let joined = surviving.join("\n");
    assert!(
        joined.contains("let _z = EnvHashes::default();"),
        "filter MUST keep the production `fn production_fn` body in scope; survivors:\n{}",
        joined,
    );
    assert!(
        !joined.contains("let x = EnvHashes::default();"),
        "filter MUST exclude the `impl Default` body — found the excluded line in survivors:\n{}",
        joined,
    );
    assert!(
        !joined.contains("let y = ProjectIdentity([0u8; 16]);"),
        "filter MUST exclude the `#[cfg(test)]` block — found the excluded line in survivors:\n{}",
        joined,
    );
}
