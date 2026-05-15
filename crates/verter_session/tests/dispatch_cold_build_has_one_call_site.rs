//! Architecture guard: there must be EXACTLY ONE production
//! `graph.execute_cooperative(` call site dispatched from
//! `ProjectSemanticDispatch`. The single allowed call site lives
//! inside `ProjectSemanticDispatch::execute_via_cold_build_helper`
//! (tagged with the `arch-guard:single-execute-cooperative-call`
//! marker comment).
//!
//! Both `SemanticQueryApi::execute` (which discards the `CacheRead`
//! rails) and `ProjectSemanticDispatch::execute_read` (which keeps
//! them) route through the helper. A second production call site
//! would mean a cold-build path slipped through bypassing
//! `install_fact_tracer` — exactly the codex round-2 P1.C finding
//! the carrier substrate closes.
//!
//! The scan walks `crates/verter_session/src/**/*.rs`, strips any
//! `#[cfg(test)]` regions (so the existing test-only memo driver
//! invocations at `semantic_query_memo/tests.rs` do not false-trigger),
//! and counts `graph.execute_cooperative(` matches. Production
//! callsites in `*tests.rs` files or under `tests/` paths are not
//! scanned.

use std::fs;
use std::path::PathBuf;

fn read_session_src(rel: &str) -> String {
    let cargo_manifest_dir = env!("CARGO_MANIFEST_DIR");
    let mut path = PathBuf::from(cargo_manifest_dir);
    path.push("src");
    path.push(rel);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

/// Strip `#[cfg(test)]` regions. The pattern matches the attribute
/// on its own line followed by either a `mod` block or a single item;
/// the function below uses a simple state machine that drops lines
/// inside a `#[cfg(test)] mod foo { ... }` region (counting `{`/`}`)
/// and drops individual `#[cfg(test)] fn foo` items up to their
/// closing `}`. This is intentionally conservative — false positives
/// (e.g. stripping more than necessary) bias the scan toward the
/// safer answer; false negatives (missing a real `#[cfg(test)]`
/// region) would cause a spurious count.
fn strip_cfg_test_regions(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut depth: i32 = 0;
    let mut skipping = false;
    let mut in_cfg_test_block = false;
    for line in src.lines() {
        let trimmed = line.trim_start();
        if !skipping {
            if trimmed.starts_with("#[cfg(test)]") {
                skipping = true;
                in_cfg_test_block = false;
                depth = 0;
                continue;
            }
            out.push_str(line);
            out.push('\n');
            continue;
        }
        // We're inside a skipping region. Count braces to find the end.
        let opens = line.matches('{').count() as i32;
        let closes = line.matches('}').count() as i32;
        if !in_cfg_test_block && opens > 0 {
            in_cfg_test_block = true;
        }
        depth += opens - closes;
        if in_cfg_test_block && depth <= 0 {
            // End of the region.
            skipping = false;
            in_cfg_test_block = false;
            depth = 0;
        }
    }
    out
}

fn walk_dir(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read crate dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Exclude test files (file-name ends with `tests.rs` or
            // is exactly `tests.rs`).
            if file_name.ends_with("tests.rs") || file_name == "tests.rs" {
                continue;
            }
            out.push(path);
        }
    }
}

/// Strip line / block comments so a doc comment that mentions
/// `graph.execute_cooperative(` doesn't false-trigger the call-site
/// count. Conservative: removes `// ...` to end-of-line, and `/* ... */`
/// across lines.
fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    let mut in_block = false;
    while i < bytes.len() {
        if in_block {
            if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                in_block = false;
                i += 2;
            } else {
                if bytes[i] == b'\n' {
                    out.push('\n');
                }
                i += 1;
            }
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            // Skip to end-of-line; keep the newline so line tracking
            // still works.
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            in_block = true;
            i += 2;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[test]
fn dispatch_has_exactly_one_production_execute_cooperative_call_site() {
    // Locate every production .rs file under crates/verter_session/src/.
    let cargo_manifest_dir = env!("CARGO_MANIFEST_DIR");
    let src_root = PathBuf::from(cargo_manifest_dir).join("src");
    let mut files = Vec::new();
    walk_dir(&src_root, &mut files);

    // For each production file, strip comments + `#[cfg(test)]`
    // regions then count `graph.execute_cooperative(` occurrences.
    // Doc comments that REFERENCE the call-site pattern (e.g. the
    // architecture guard comment on the helper itself) must NOT
    // false-trigger.
    let mut total: usize = 0;
    let mut hits: Vec<(PathBuf, usize)> = Vec::new();
    for path in &files {
        let raw = fs::read_to_string(path).expect("read file");
        let stripped_cfg = strip_cfg_test_regions(&raw);
        let stripped = strip_comments(&stripped_cfg);
        let count = stripped.matches("graph.execute_cooperative(").count();
        if count > 0 {
            hits.push((path.clone(), count));
            total += count;
        }
    }

    assert_eq!(
        total, 1,
        "exactly ONE production `graph.execute_cooperative(` call site is allowed \
         (inside `ProjectSemanticDispatch::execute_via_cold_build_helper` — the shared \
         cold-build helper). Hits: {hits:?}"
    );

    // Sanity check: the single hit must be inside `mod.rs` (where the
    // shared cold-build helper lives), not in `raise.rs` (where the
    // pre-carrier `execute_read` used to call `execute_cooperative`
    // directly without an `install_fact_tracer` wrapper).
    let (path, _) = &hits[0];
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    assert!(
        file_name.ends_with("mod.rs"),
        "the single production `graph.execute_cooperative(` call site must live in the shared \
         cold-build helper (project_semantic_dispatch/mod.rs), not {file_name}"
    );

    // The host file path must end with `project_semantic_dispatch/mod.rs`.
    let parent = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");
    assert_eq!(
        parent, "project_semantic_dispatch",
        "the single call site must live in `project_semantic_dispatch/mod.rs`, \
         got parent dir `{parent}`"
    );
}

#[test]
fn shared_cold_build_helper_exists_in_dispatch() {
    let src = read_session_src("project_semantic_dispatch/mod.rs");
    assert!(
        src.contains("fn execute_via_cold_build_helper("),
        "ProjectSemanticDispatch must expose the shared cold-build helper \
         `execute_via_cold_build_helper`. Both `SemanticQueryApi::execute` and \
         `ProjectSemanticDispatch::execute_read` route through it so the fact \
         tracer is always installed around the cold-build closure."
    );
    // Negative assertion: the marker comment must accompany the call site.
    assert!(
        src.contains("arch-guard:single-execute-cooperative-call"),
        "the shared helper must carry the `arch-guard:single-execute-cooperative-call` \
         marker comment so future refactors don't accidentally remove the dispatch's \
         single call-site invariant."
    );
}

#[test]
fn execute_read_does_not_call_execute_cooperative_directly() {
    // `execute_read` lives in `project_semantic_dispatch/raise.rs`.
    // After the shared cold-build helper refactor, `execute_read`
    // delegates to `execute_via_cold_build_helper`; it does NOT call
    // `graph.execute_cooperative(` directly.
    let src = read_session_src("project_semantic_dispatch/raise.rs");
    assert!(
        !src.contains("graph.execute_cooperative("),
        "execute_read must NOT call `graph.execute_cooperative(` directly. It must \
         delegate to `execute_via_cold_build_helper` so the fact tracer is always \
         installed around the cold-build closure (closes codex round-2 P1.C)."
    );
    assert!(
        src.contains("execute_via_cold_build_helper"),
        "execute_read must delegate to `execute_via_cold_build_helper`."
    );
}
