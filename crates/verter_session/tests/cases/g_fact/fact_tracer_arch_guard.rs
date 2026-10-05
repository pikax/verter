//! Architecture-guard for the fact-read tracer substrate.
//!
//! Asserts that the TLS-backed installer's source carries a documented
//! R18 carve-out — a specific docstring substring that justifies why a
//! per-cold-compute thread-local does NOT constitute a hidden view
//! global. Removing it would expose the TLS implementation as if it were
//! a hidden global view rather than per-compute instrumentation.
//!
//! The tracer API itself (`VerterHost::with_fact_tracer` and
//! `VerterHost::current_fact_tracer`) is held by the compiler through
//! its callers and by the behavioural coverage in
//! `tests/cases/g_fact/fact_tracer_observe.rs` and
//! `tests/cases/g_misc3/tracer_stack_nesting_supported.rs`.

use std::fs;
use std::path::PathBuf;

/// Workspace root for this test, derived from the crate's `CARGO_MANIFEST_DIR`.
fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    // `CARGO_MANIFEST_DIR` is `<workspace>/crates/verter_session`.
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("workspace root must exist two levels above CARGO_MANIFEST_DIR")
}

fn read_workspace_file(rel: &str) -> String {
    let path = workspace_root().join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {} failed: {e}", path.display()))
}

#[test]
fn r18_carve_out_documented_for_tls_installer() {
    let src = read_workspace_file("crates/verter_session/src/resolver_core/resolver_context.rs");

    // The documented carve-out is the rationale for why a
    // per-cold-compute thread-local does NOT violate R18. The
    // substring below is from the comment block above the
    // `with_fact_tracer` installer — search for the specific phrase
    // so accidental deletion of the rationale fires the guard.
    assert!(
        src.contains("Why this is NOT an R18 violation"),
        "The TLS installer for the fact tracer must carry a documented R18 carve-out — see the \
         block comment above the `with_fact_tracer` installer. R18 forbids hidden view globals; the \
         tracer is per-compute instrumentation reachable only through a documented trait method, \
         and the carve-out states why that distinction matters."
    );

    // Also verify the carve-out enumerates the three invariants
    // that make the thread-local safe: per-compute scope, the
    // nesting-supported fan-out-to-all-levels mechanism, and
    // trait-method-only readership. The nesting invariant is the
    // ratified Bug B mechanism (nesting IS supported — an
    // evaluator-scoped nested observer requires it), which INVERTED
    // the earlier "nested installers panic" wording; the phrases below
    // pin the current mechanism (per-thread `ACTIVE_TRACERS` stack,
    // observations fan out to ALL active levels), so reverting to the
    // old panic-on-nest rationale — or dropping the nesting story —
    // reddens the guard.
    let block_starts = src
        .find("Why this is NOT an R18 violation")
        .expect("carve-out present");
    let block = &src[block_starts..block_starts + 2048.min(src.len() - block_starts)];
    assert!(
        block.contains("Nesting IS supported")
            && block.contains("ACTIVE_TRACERS")
            && block.contains("fans out to ALL active levels"),
        "R18 carve-out must state the nesting-supported invariant — the ratified Bug B \
         mechanism: nested `with_fact_tracer` scopes push onto the per-thread \
         `ACTIVE_TRACERS` stack and every observation fans out to ALL active levels, so an \
         inner (evaluator-scoped) observer's reads reach every enclosing tracer instead of \
         silently routing to only one."
    );
    assert!(
        block.contains("trait method"),
        "R18 carve-out must state the trait-method-only-readership invariant; \
         the TLS slot is internal substrate, reached only through ResolverContext."
    );
}
