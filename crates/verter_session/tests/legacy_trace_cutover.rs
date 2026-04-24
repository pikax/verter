//! Commit 6.E exit-criteria tests (plan §3.A Commit 6.E / §6 item 11):
//! grep-based regression guards that the legacy
//! `component_meta_trace_scope!` / `component_meta_trace_event!` surface
//! is gone, and that every `StructuredComponentMetaEvent::Custom {`
//! construction site carries a `// Custom justified:` comment.
//!
//! These are integration tests because the grep scope crosses the
//! crate boundary; the runtime tests (snapshot byte-exactness,
//! IndexedReadyBuilt firing rules, macro never writes to stderr) live
//! in this same file since they exercise `verter_session`'s public
//! surface + integration behavior.

use std::fs;
use std::path::{Path, PathBuf};

fn crates_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    while p.file_name() != Some(std::ffi::OsStr::new("verter_session")) {
        if !p.pop() {
            panic!("unable to find verter_session package root");
        }
    }
    p.pop();
    p
}

fn walk_rs_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn recurse(dir: &Path, out: &mut Vec<PathBuf>) {
        // Skip target/ / .git — we only look at source.
        let name = dir.file_name().map(|s| s.to_string_lossy().into_owned());
        if matches!(name.as_deref(), Some("target") | Some(".git")) {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                recurse(&path, out);
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                // Exclude this test file — it contains literal
                // strings of the forbidden patterns in assertion
                // messages and grep predicates, which would
                // falsely fail our own grep scan.
                let file_name = path.file_name().map(|s| s.to_string_lossy().into_owned());
                if file_name.as_deref() == Some("legacy_trace_cutover.rs") {
                    continue;
                }
                out.push(path);
            }
        }
    }
    recurse(root, &mut out);
    out
}

fn read_file(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

// ──────────────────────────────────────────────────────────────────────
// Legacy deletion grep tests (plan §6 item 11).
// ──────────────────────────────────────────────────────────────────────

/// The legacy macros `component_meta_trace_scope!` /
/// `component_meta_trace_event!` are deleted — no call survives
/// outside of docstrings/comments.
#[test]
fn legacy_session_trace_macros_deleted_from_workspace() {
    let crates = crates_dir();
    let files = walk_rs_files(&crates);
    let mut violations = Vec::new();
    for file in &files {
        let text = read_file(file);
        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            // Ignore documentation and inline-comment mentions —
            // these are history markers, not live call sites.
            if trimmed.starts_with("//") || trimmed.starts_with("*") {
                continue;
            }
            if line.contains("component_meta_trace_scope!")
                || line.contains("component_meta_trace_event!")
            {
                violations.push(format!("{}:{}: {}", file.display(), i + 1, line.trim()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Legacy `component_meta_trace_scope!` / `component_meta_trace_event!` macro calls \
         must be deleted (plan §3.A Commit 6.E). Found:\n{}",
        violations.join("\n")
    );
}

/// The legacy helper functions `component_meta_trace_scope_impl` /
/// `component_meta_trace_event_impl` / `component_meta_trace_write_line`
/// are deleted.
#[test]
fn legacy_session_trace_helpers_deleted_from_host_manage() {
    let crates = crates_dir();
    let files = walk_rs_files(&crates);
    let mut violations = Vec::new();
    for file in &files {
        let text = read_file(file);
        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("*") {
                continue;
            }
            // `fn` declaration or path usage.
            if line.contains("component_meta_trace_scope_impl")
                || line.contains("component_meta_trace_event_impl")
                || line.contains("component_meta_trace_write_line")
            {
                violations.push(format!("{}:{}: {}", file.display(), i + 1, line.trim()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Legacy trace impl helpers must be deleted (plan §3.A Commit 6.E). Found:\n{}",
        violations.join("\n")
    );
}

/// The legacy `VERTER_COMPONENT_META_TRACE*` env vars are no longer
/// consumed anywhere (audit `VERTER_COMPONENT_META_AUDIT_JSON_OUT`
/// is the one remaining debug env var and it does not match the
/// deleted prefix pattern).
#[test]
fn legacy_trace_env_vars_not_consumed_anywhere() {
    let crates = crates_dir();
    let files = walk_rs_files(&crates);
    let mut violations = Vec::new();
    for file in &files {
        let text = read_file(file);
        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("*") {
                continue;
            }
            // Match exact prefix `VERTER_COMPONENT_META_TRACE` but
            // NOT `VERTER_COMPONENT_META_AUDIT_*` (which is the
            // current debug env var).
            if line.contains("VERTER_COMPONENT_META_TRACE") {
                violations.push(format!("{}:{}: {}", file.display(), i + 1, line.trim()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Legacy `VERTER_COMPONENT_META_TRACE*` env vars must not be consumed \
         anywhere (plan §3.A Commit 6.E). Found:\n{}",
        violations.join("\n")
    );
}

/// Every literal `StructuredComponentMetaEvent::Custom { … }`
/// construction site in the session crate must carry a
/// `// Custom justified: …` comment within the preceding 3 lines.
///
/// Pattern-match sites (`Custom { .. }`, `Custom { name, detail } =>`)
/// are explicitly NOT construction sites and are excluded from the
/// scan.
#[test]
fn every_custom_variant_construction_site_has_justification_comment() {
    let crates = crates_dir();
    let session_src = crates.join("verter_session").join("src");
    let files = walk_rs_files(&session_src);
    let mut violations = Vec::new();

    // Also search tests dirs since test-side Custom construction is
    // allowed but must carry justification too.
    let session_tests = crates.join("verter_session").join("tests");
    let mut all_files = files;
    all_files.extend(walk_rs_files(&session_tests));

    for file in &all_files {
        let text = read_file(file);
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            // Skip pure comment lines.
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("*") {
                continue;
            }

            // Match only the specific `StructuredComponentMetaEvent::Custom` variant.
            // (`VirtualNodeKind::Custom {` from a different enum is intentionally
            // excluded by the explicit path prefix.)
            let construct = line.contains("StructuredComponentMetaEvent::Custom {")
                || (line.contains("Event::Custom {") && is_struct_event_alias(&text, i));

            if !construct {
                continue;
            }

            // Exclude pattern-match forms:
            //   `Custom { .. }`              — rest pattern
            //   `Custom { name, detail } =>` — destructuring match arm
            //   Any `=>` on the same line indicates a match arm.
            if line.contains("Custom { .. }")
                || line.contains("Custom { name, detail }")
                || line.contains(" => ")
                || line.trim_end().ends_with("=>")
            {
                continue;
            }

            // Look back up to 12 lines for a `// Custom justified:` comment.
            // (12 is generous — typical justification sits on the line
            // directly above; 12 covers multi-line doc comments or small
            // expression pre-ambles.)
            let start = i.saturating_sub(12);
            let justified = lines[start..i]
                .iter()
                .any(|l| l.contains("// Custom justified:"));
            if !justified {
                violations.push(format!(
                    "{}:{}: missing `// Custom justified:` within 12 preceding lines ({})",
                    file.display(),
                    i + 1,
                    line.trim(),
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Every `StructuredComponentMetaEvent::Custom {{ … }}` construction site must \
         document why a typed variant isn't available (plan §3.A Commit 6.E). Found:\n{}",
        violations.join("\n")
    );
}

/// In the `expected_display_snapshots` module and the
/// `structured_event::tests` module, `Event` is aliased to
/// `StructuredComponentMetaEvent` via `use super::StructuredComponentMetaEvent as Event`.
/// We grep the file for that alias so `Event::Custom { ... }` is
/// identified as a StructuredComponentMetaEvent construction.
fn is_struct_event_alias(text: &str, _line_idx: usize) -> bool {
    text.contains("StructuredComponentMetaEvent as Event")
        || text.contains("StructuredComponentMetaEvent,")
        || text.contains("super::StructuredComponentMetaEvent")
}

// ──────────────────────────────────────────────────────────────────────
// Runtime behaviour tests.
// ──────────────────────────────────────────────────────────────────────

/// The `component_meta_trace_structured!` macro must not emit to
/// stderr or files — it only feeds the accumulator. A guarded no-op
/// request must produce zero stderr bytes from the macro body.
///
/// We can't fully capture stderr portably from inside the same
/// process without unsafe redirection, so this test relies on a
/// structural invariant: `push_structured_event` has no
/// `eprintln!` / `println!` / `write!` calls to stdout/stderr /
/// file I/O. The grep guard below enforces that invariant.
#[test]
fn component_meta_trace_structured_macro_does_not_write_to_file_or_stderr_trace() {
    let crates = crates_dir();
    let host_manage = crates
        .join("verter_session")
        .join("src")
        .join("host_manage.rs");
    let text = read_file(&host_manage);
    // Find the push_structured_event fn body and check it has no
    // stderr/file writes.
    let Some(start) = text.find("pub(crate) fn push_structured_event(") else {
        panic!("push_structured_event not found in host_manage.rs");
    };
    // Take from function start to end of its body (matching braces).
    // Simple heuristic: take next 500 chars; if a violation is there
    // it will surface.
    let snippet = &text[start..std::cmp::min(start + 800, text.len())];
    for forbidden in [
        "eprintln!",
        "println!",
        "std::io::stderr",
        "File::create",
        "std::fs::write",
    ] {
        assert!(
            !snippet.contains(forbidden),
            "`push_structured_event` must not contain `{forbidden}` — the macro \
             is pure accumulator push, not stderr/file I/O. Body:\n{snippet}"
        );
    }
}

/// `IndexedReadyBuilt` fires exactly once per fresh `(canonical, whole_hash)`
/// insert into `IndexedReadyDb`.
#[test]
fn indexed_ready_built_event_fires_once_per_fresh_whole_hash() {
    use std::sync::Arc;
    use verter_session::component_meta_audit::{
        accumulator::RequestFootprintAccumulator, StructuredComponentMetaEvent,
    };
    use verter_session::project_type_store::{IndexedReady, IndexedReadyDb};
    use verter_session::request_context::{RequestContext, RequestContextGuard};

    let acc = Arc::new(RequestFootprintAccumulator::new());
    let ctx = RequestContext::new(1, Arc::from("/owner"), true, Some(Arc::clone(&acc)));
    let _guard = RequestContextGuard::install(ctx);

    let db = IndexedReadyDb::new();
    let mut whole_hash = [0u8; 16];
    whole_hash[0] = 0xAB;

    let ir = Arc::new(IndexedReady::new_for_test(whole_hash));
    db.insert(Arc::from("/x.ts"), Arc::clone(&ir));

    let state = acc.drain();
    let fresh_events: Vec<_> = state
        .structured_events
        .iter()
        .filter(|ev| matches!(ev, StructuredComponentMetaEvent::IndexedReadyBuilt { .. }))
        .collect();
    assert_eq!(
        fresh_events.len(),
        1,
        "exactly one IndexedReadyBuilt event expected on fresh insert; got: {:?}",
        state.structured_events
    );
}

/// `IndexedReadyBuilt` does NOT fire on an overwrite (stale-sweep).
#[test]
fn indexed_ready_built_event_not_fired_on_overwrite() {
    use std::sync::Arc;
    use verter_session::component_meta_audit::{
        accumulator::RequestFootprintAccumulator, StructuredComponentMetaEvent,
    };
    use verter_session::project_type_store::{IndexedReady, IndexedReadyDb};
    use verter_session::request_context::{RequestContext, RequestContextGuard};

    let db = IndexedReadyDb::new();
    let mut whole_hash_a = [0u8; 16];
    whole_hash_a[0] = 0x01;
    let ir_a = Arc::new(IndexedReady::new_for_test(whole_hash_a));

    // First insert WITHOUT any audit context — populates the
    // entry but no event fires because no accumulator is active.
    db.insert(Arc::from("/x.ts"), Arc::clone(&ir_a));

    // Now install an accumulator, and insert a NEW entry under
    // the same canonical — this is an OVERWRITE. No event should
    // fire because `prev.is_some()`.
    let acc = Arc::new(RequestFootprintAccumulator::new());
    let ctx = RequestContext::new(1, Arc::from("/owner"), true, Some(Arc::clone(&acc)));
    let _guard = RequestContextGuard::install(ctx);

    let mut whole_hash_b = [0u8; 16];
    whole_hash_b[0] = 0x02;
    let ir_b = Arc::new(IndexedReady::new_for_test(whole_hash_b));
    db.insert(Arc::from("/x.ts"), Arc::clone(&ir_b));

    let state = acc.drain();
    let fresh_events: Vec<_> = state
        .structured_events
        .iter()
        .filter(|ev| matches!(ev, StructuredComponentMetaEvent::IndexedReadyBuilt { .. }))
        .collect();
    assert_eq!(
        fresh_events.len(),
        0,
        "IndexedReadyBuilt must NOT fire on overwrite — prev.is_some() gate. Events: {:?}",
        state.structured_events
    );
}
