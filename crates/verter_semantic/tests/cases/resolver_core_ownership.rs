//! Per-definition crate ownership for the converged resolver core.
//!
//! **These are compiler assertions, not scans.** Each `use` below names a
//! definition at the exact module path its owning crate is required to expose.
//! If a definition is absent, renamed, or made private, this file does not
//! compile, and the canonical gate reports it as
//! a build failure of `verter_semantic`'s test target. Nothing here reads the
//! source tree, so there is no pattern to keep in step with a rename.
//!
//! The obligation this discharges is per-definition ownership: a named
//! definition is exposed by the crate assigned to own it. A dependency-closure test
//! decides only whether an edge crosses a crate boundary; it cannot see whether
//! a particular definition is on the correct side, which is why that test does
//! not discharge this and this file exists.
//!
//! **Scope limit, and it is narrower than "ownership".** A resolving import
//! establishes that a definition IS reachable at its assigned path in this
//! crate. It does NOT by itself establish that no competing public surface
//! exists elsewhere. That absence is witnessed separately by the trybuild fixtures in
//! `crates/verter_workspace/tests/compile-fail/` (`legacy_resolver_surface_is_absent.rs`,
//! `resolver_core_helpers_are_private.rs`): the workspace crate can no longer
//! name the prohibited resolver surface or semantic-owned helpers. Read together, the two
//! checks are the ownership proof; read alone, a green run here means
//! "reachable here", never "uniquely owned here".

// The core DTOs, named individually. A glob import would compile even if most
// of them had never arrived, so each is named and each is load-bearing.
use verter_semantic::resolver_core::dto::ProjectOwnership;
use verter_semantic::resolver_core::dto::ProviderTarget;
use verter_semantic::resolver_core::dto::ResolutionContext;
use verter_semantic::resolver_core::dto::ResolutionKind;
use verter_semantic::resolver_core::dto::ResolvePhase;
use verter_semantic::resolver_core::dto::ResolveRequest;
use verter_semantic::resolver_core::dto::ResolveRequestKind;
use verter_semantic::resolver_core::dto::ResolveResult;
// Project configuration has its own module rather than the shared DTO one.
// Each import names the path the crate actually exposes, so the compiler
// checks the concrete ownership boundary rather than a parallel inventory.
use verter_semantic::resolver_core::project_config::IdeProjectCompilerOptions;
use verter_semantic::resolver_core::project_config::IdeProjectConfig;
use verter_semantic::resolver_core::project_config::WorkspaceAlias;

/// Binds each imported name to a value slot, so an unused-import lint can never
/// be "fixed" by deleting the import and silently dropping the assertion with
/// it. `size_of` forces the type to be complete rather than merely nameable.
fn assert_owned<T>() -> usize {
    core::mem::size_of::<T>()
}

#[test]
fn resolver_core_dtos_are_owned_by_the_semantic_crate() {
    // Each call fails to COMPILE if the type is not at the path named above.
    // The sum is incidental; reaching it at all is the assertion.
    // The assertion is that the imports above RESOLVE — that is what fails to
    // compile when a definition is not at its named path in this crate. The
    // calls below exist to bind each import to a use site so an unused-import
    // lint cannot "fix" the file by deleting an import and silently dropping
    // its assertion.
    //
    // There is deliberately no count check: the load-bearing property is that
    // every named import resolves at its exact semantic-owned module path.
    // A literal-length assertion would be tautological and would not widen
    // what the compiler actually proves.
    let _bound = [
        assert_owned::<ProjectOwnership>(),
        assert_owned::<ResolveRequestKind>(),
        assert_owned::<ResolvePhase>(),
        assert_owned::<ResolutionContext>(),
        assert_owned::<ProviderTarget>(),
        assert_owned::<ResolutionKind>(),
        assert_owned::<ResolveRequest>(),
        assert_owned::<ResolveResult>(),
        assert_owned::<IdeProjectConfig>(),
        assert_owned::<WorkspaceAlias>(),
        assert_owned::<IdeProjectCompilerOptions>(),
    ];
}
