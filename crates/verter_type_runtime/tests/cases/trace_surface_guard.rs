//! Architecture guard: the raw type-runtime trace span lifecycle is internal.
//!
//! Production await-crossing spans MUST go through `type_runtime_trace_scope_async`
//! (plus the event/context helpers). The raw guard lifecycle — the
//! `TypeRuntimeTraceGuard` type, the `open_type_runtime_trace_span` opener, and
//! the test-only `type_runtime_trace_scope!` macro — is `pub(crate)` /
//! `cfg(test)` and same-state scoped: it is created AND dropped within one
//! active trace state. Holding a raw guard across an `.await` boundary is out of
//! contract; a guard identity-miss drop is fault containment (a safe no-op), not
//! a supported tracing-semantics path.
//!
//! This guard fences that invariant structurally: it fails if any production
//! `.rs` file (outside the trace implementation itself and outside test code)
//! references `TypeRuntimeTraceGuard`, calls `type_runtime_trace_scope(`, or
//! invokes the `type_runtime_trace_scope!` macro. It is a cheap static source
//! scan in the established `no_*` architecture-guard style (a test, not runtime
//! semantic logic), so scanning source TEXT for the forbidden symbols is the
//! intended mechanism.

use std::fs;
use std::path::{Path, PathBuf};

/// Repo root: `crates/verter_type_runtime/` → `crates/` → repo root.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// The single trace implementation file. It legitimately defines and uses the
/// raw guard lifecycle (including its own inline `#[cfg(test)]` tests), so it is
/// the one production file exempt from the scan. Stored repo-relative with
/// forward slashes for portable comparison.
const TRACE_IMPL_REL: &str = "crates/verter_type_runtime/src/trace.rs";

/// Forbidden raw-guard surface tokens. Each is matched as a literal substring:
///
/// - `type_runtime_trace_scope(` — the raw opener CALL. The trailing `(`
///   excludes both `type_runtime_trace_scope_async(` (which has `_async(` after
///   the stem) and the internal `open_type_runtime_trace_span(`.
/// - `type_runtime_trace_scope!` — the raw sync macro INVOCATION. The trailing
///   `!` excludes `type_runtime_trace_scope_async!`.
/// - `TypeRuntimeTraceGuard` — the raw guard TYPE, in any position (a binding, a
///   return type, a `::noop()` call, or a re-export).
const FORBIDDEN_TOKENS: &[&str] = &[
    "type_runtime_trace_scope(",
    "type_runtime_trace_scope!",
    "TypeRuntimeTraceGuard",
];

/// Per-line predicate: does this source line use a forbidden raw-guard symbol?
///
/// Pure substring matching against [`FORBIDDEN_TOKENS`] — discriminating because
/// the longer `_async` variants do not contain any forbidden token as written
/// (the trailing `(` / `!` is what disambiguates the call/macro forms).
fn line_uses_raw_trace_surface(line: &str) -> bool {
    FORBIDDEN_TOKENS.iter().any(|tok| line.contains(tok))
}

/// Whole-file pre-reject: a file containing none of the forbidden tokens cannot
/// contain a matching line. Coverage-safe (it is a strict OR over the same
/// tokens the per-line predicate uses).
fn file_may_use_raw_trace_surface(src: &str) -> bool {
    FORBIDDEN_TOKENS.iter().any(|tok| src.contains(tok))
}

/// Walk a crate `src/` tree and collect production `.rs` files. Mirrors the
/// established `walk_production_rs` style: skip `tests`/`benches`/`examples`/
/// `target` directories and `*_tests.rs` / `tests.rs` sibling files (inline
/// `#[cfg(test)]` tests living in production files are handled by exempting the
/// trace impl file, the only production file that holds the raw surface).
fn walk_production_rs(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(it) => it,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "tests" || name == "benches" || name == "examples" || name == "target" {
                    continue;
                }
                stack.push(path);
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.ends_with(".rs") {
                continue;
            }
            if name.ends_with("_tests.rs") || name == "tests.rs" {
                continue;
            }
            out.push(path);
        }
    }
    out.sort();
    out
}

/// Repo-relative path with forward slashes (portable across Windows/Unix).
fn relative_to_root(abs: &Path) -> String {
    abs.strip_prefix(workspace_root())
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Walk every `crates/*/src/**` production file and return `(rel, line_no, line)`
/// triples for each forbidden raw-trace-surface use, excluding the trace
/// implementation file itself.
fn raw_trace_surface_violations() -> Vec<(String, usize, String)> {
    let crates_root = workspace_root().join("crates");
    let mut violations = Vec::new();
    let entries = match fs::read_dir(&crates_root) {
        Ok(it) => it,
        Err(_) => return violations,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let src_dir = path.join("src");
        if !src_dir.exists() {
            continue;
        }
        for file in walk_production_rs(&src_dir) {
            let rel = relative_to_root(&file);
            // The trace implementation legitimately owns the raw surface.
            if rel == TRACE_IMPL_REL {
                continue;
            }
            let src = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if !file_may_use_raw_trace_surface(&src) {
                continue;
            }
            for (idx, line) in src.lines().enumerate() {
                if line_uses_raw_trace_surface(line) {
                    violations.push((rel.clone(), idx + 1, line.trim().to_string()));
                }
            }
        }
    }
    violations.sort();
    violations
}

#[test]
fn no_raw_type_runtime_trace_surface_outside_trace_impl() {
    let violations = raw_trace_surface_violations();
    assert!(
        violations.is_empty(),
        "`trace_surface_guard` violations: production source outside the trace\n\
         implementation ({TRACE_IMPL_REL}) references the INTERNAL raw type-runtime\n\
         trace span lifecycle. Await-crossing spans must use\n\
         `type_runtime_trace_scope_async`; the raw `TypeRuntimeTraceGuard` /\n\
         `open_type_runtime_trace_span` / `type_runtime_trace_scope!` surface is\n\
         `pub(crate)` / `cfg(test)` and same-state scoped, and an identity-miss\n\
         drop is fault containment, not a supported tracing path.\n\n\
         Forbidden tokens: {FORBIDDEN_TOKENS:?}\n\n\
         Violations:\n  {}",
        violations
            .iter()
            .map(|(rel, lineno, line)| format!("{rel}:{lineno}: {line}"))
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}

/// Discrimination proof (negative half): a planted forbidden use is FLAGGED by
/// the per-line predicate and is NOT skipped by the whole-file pre-reject. These
/// fixtures model exactly the misuse the guard fences — a production file
/// reintroducing the raw surface.
#[test]
fn predicate_flags_planted_raw_trace_surface_uses() {
    const PLANTED_VIOLATIONS: &[&str] = &[
        // Raw opener call (post-rename name; the literal `type_runtime_trace_scope(`
        // could only reappear as a reintroduction of the old surface).
        r#"    let _g = type_runtime_trace_scope("hover", "backend=tsgo");"#,
        // Raw sync macro invocation.
        r#"    let _g = crate::type_runtime_trace_scope!("hover", "x");"#,
        // Guard type referenced directly (binding).
        r#"    let g: TypeRuntimeTraceGuard = make_guard();"#,
        // Guard type re-exported.
        r#"    pub use trace::TypeRuntimeTraceGuard;"#,
        // Guard `::noop()` construction.
        r#"    let g = TypeRuntimeTraceGuard::noop();"#,
    ];
    for fixture in PLANTED_VIOLATIONS {
        assert!(
            line_uses_raw_trace_surface(fixture),
            "per-line predicate must FLAG planted violation: {fixture:?}"
        );
        // A single planted line is also a one-line "file"; the whole-file
        // pre-reject must not skip it (else the walker would never reach the
        // per-line scan).
        assert!(
            file_may_use_raw_trace_surface(fixture),
            "whole-file pre-reject must NOT skip planted violation: {fixture:?}"
        );
    }
}

/// Discrimination proof (positive half): clean lines — including the PUBLIC
/// async surface, the internal opener's post-rename name, and the event/context
/// helpers — are NOT flagged. This is what makes the guard discriminating rather
/// than a blanket ban on the `type_runtime_trace` stem.
#[test]
fn predicate_passes_clean_and_public_trace_surface() {
    const CLEAN_LINES: &[&str] = &[
        // The PUBLIC await-crossing wrapper — its function form.
        r#"        crate::trace::type_runtime_trace_scope_async("hover", detail, fut).await"#,
        // The PUBLIC await-crossing wrapper — its macro form.
        r#"        crate::type_runtime_trace_scope_async!("hover", "x", async { 1 })"#,
        // The internal opener under its post-rename name (allowed inside the
        // trace impl; never matched by the forbidden tokens anywhere).
        r#"        let _t = open_type_runtime_trace_span(name, detail);"#,
        // Event + context helpers stay public and unaffected.
        r#"    crate::type_runtime_trace_event!("result", "cache_hit=false");"#,
        r#"    let ctx = current_type_runtime_trace_context();"#,
        r#"    with_type_runtime_trace_context_async(ctx, fut).await"#,
        // Unrelated source prose.
        r#"    // open the span on whichever state is active"#,
    ];
    for line in CLEAN_LINES {
        assert!(
            !line_uses_raw_trace_surface(line),
            "per-line predicate must NOT flag clean/public line: {line:?}"
        );
    }
}

/// Sanity: the exempt trace implementation file actually exists at the recorded
/// path (so the exemption is real, not a typo that silently disables the scan
/// of a different file).
#[test]
fn exempt_trace_impl_path_exists() {
    let path = workspace_root().join(TRACE_IMPL_REL);
    assert!(
        path.is_file(),
        "exempt trace implementation file must exist at {TRACE_IMPL_REL} \
         (resolved {})",
        path.display()
    );
}
