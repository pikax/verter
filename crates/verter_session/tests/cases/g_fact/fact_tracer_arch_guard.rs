//! Architecture-guard for the fact-read tracer substrate.
//!
//! Asserts three structural properties about the landed tree:
//!
//! 1. `ResolverContext` source contains the
//!    `fn current_fact_tracer(` method declaration.
//! 2. `VerterHost` exposes a `pub fn with_fact_tracer<F, R>(`
//!    RAII entry point.
//! 3. The TLS-backed installer's source contains a documented
//!    R18 carve-out — a specific docstring substring that
//!    justifies why a per-cold-compute thread-local does NOT
//!    constitute a hidden view global.
//!
//! Failure modes the guard catches:
//!
//! - Accidentally dropping the `current_fact_tracer` trait method
//!   (would let `observe` / `observe_borrowed_signature` silently
//!   stop routing onto the active tracer).
//! - Replacing `with_fact_tracer` with an unbounded `install_tracer`
//!   API that does not match the RAII scope contract.
//! - Removing the R18 carve-out comment, which would expose the
//!   TLS implementation as if it were a hidden global view rather
//!   than per-compute instrumentation.
//!
//! These are structural-guard assertions, not behavioural tests.
//! Behavioural coverage lives in `tests/cases/g_fact/fact_tracer_observe.rs`.

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
fn resolver_context_declares_current_fact_tracer() {
    let src = read_workspace_file("crates/verter_session/src/resolver_core/resolver_context.rs");

    // The trait method declaration is the contract; check for the
    // exact signature substring so a future refactor that renames
    // or removes the method fires the guard rather than silently
    // breaking the tracer surface.
    assert!(
        src.contains("fn current_fact_tracer(&self) -> Option<&crate::resolver_core::FactReadSetCell>"),
        "ResolverContext must declare `fn current_fact_tracer(&self) -> Option<&FactReadSetCell>` — \
         this is the surface push-style `observe` routes through. If the method is renamed, the \
         default-impl `observe` / `observe_borrowed_signature` methods become no-ops."
    );
}

#[test]
fn verter_host_exposes_with_fact_tracer() {
    let src = read_workspace_file("crates/verter_session/src/resolver_core/resolver_context.rs");

    assert!(
        src.contains("pub fn with_fact_tracer<F, R>"),
        "VerterHost must expose `pub fn with_fact_tracer<F, R>` as the RAII entry point. The \
         tracer must be installed via a scope guard that clears the TLS slot on drop — an \
         unbounded `install_tracer` API would risk leaving stale state in the TLS slot."
    );

    assert!(
        src.contains(
            "pub fn current_fact_tracer(&self) -> Option<&crate::resolver_core::FactReadSetCell>"
        ),
        "VerterHost must expose `pub fn current_fact_tracer` mirroring the resolver-tier \
         trait method, so the test surface and public API can verify warm-hit vs cold-compute \
         behaviour without depending on the sealed trait."
    );
}

#[test]
fn r18_carve_out_documented_for_tls_installer() {
    let src = read_workspace_file("crates/verter_session/src/resolver_core/resolver_context.rs");

    // The documented carve-out is the rationale for why a
    // per-cold-compute thread-local does NOT violate R18. The
    // substring below is from the comment block above the
    // `fact_tracer_tls` module — search for the specific phrase
    // so accidental deletion of the rationale fires the guard.
    assert!(
        src.contains("Why this is NOT an R18 violation"),
        "The TLS installer for the fact tracer must carry a documented R18 carve-out — see the \
         block comment above the `fact_tracer_tls` module. R18 forbids hidden view globals; the \
         tracer is per-compute instrumentation reachable only through a documented trait method, \
         and the carve-out states why that distinction matters."
    );

    // Also verify the carve-out enumerates the three invariants
    // that make the thread-local safe: per-compute scope, nested
    // installer panic, trait-method-only readership.
    let block_starts = src
        .find("Why this is NOT an R18 violation")
        .expect("carve-out present");
    let block = &src[block_starts..block_starts + 2048.min(src.len() - block_starts)];
    assert!(
        block.contains("Nested installers panic")
            || block.contains("Nested scopes panic")
            || block.contains("Nested installer panics")
            || block.contains("nested installers panic")
            || block.contains("Nested installers panic"),
        "R18 carve-out must state the nested-installer-panics invariant; \
         observations must never silently route to a sibling tracer."
    );
    assert!(
        block.contains("trait method"),
        "R18 carve-out must state the trait-method-only-readership invariant; \
         the TLS slot is internal substrate, reached only through ResolverContext."
    );
}
