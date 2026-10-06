//! Active-SDK intrinsic audit: the workspace's installed TypeScript SDK must
//! declare no intrinsic the default registry does not implement. Reads the SDK
//! through the workspace's intrinsic-library access, so it lives with the host.

use std::sync::Arc;

use crate::intrinsic_registry::{
    audit_unsupported, extract_intrinsics_from_lib_source, IntrinsicRegistry,
};

// ----------------------------------------------------------------------
// Active-SDK intrinsic audit
//
// Walks the workspace's installed TypeScript SDK, scans `lib*.d.ts` for
// `type X<...> = intrinsic;` declarations, and asserts the default
// registry implements every one. The audit is a hard correctness gate
// — a new intrinsic in the active SDK must land with matching
// implementation work in the resolver.
//
// The test is a no-op on machines without an installed TypeScript
// package (e.g. shallow CI images) so `cargo test` stays green without
// pnpm install. CI configurations that want the hard-failure behaviour
// should ensure the workspace `pnpm install` runs before tests.
// ----------------------------------------------------------------------

/// Scan an [`IntrinsicLibraryAccess`] and collect every intrinsic
/// declaration name across its `lib*.d.ts` files.
fn scan_intrinsics_via_library(
    library: &dyn verter_workspace::IntrinsicLibraryAccess,
) -> Vec<Arc<str>> {
    let mut found: Vec<Arc<str>> = Vec::new();
    for name in library.list_intrinsic_libs() {
        if !name.starts_with("lib.") || !name.ends_with(".d.ts") {
            continue;
        }
        let Ok(source) = library.read_intrinsic_lib(&name) else {
            continue;
        };
        found.extend(extract_intrinsics_from_lib_source(&source));
    }
    found.sort();
    found.dedup();
    found
}

/// Build a [`NativeIntrinsicLibrary`] rooted at the workspace
/// (`crates/verter_session/../..`).
fn build_active_intrinsic_library() -> verter_workspace::NativeIntrinsicLibrary {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest_dir);
    verter_workspace::NativeIntrinsicLibrary::discover(&workspace_root)
}

/// Active-SDK audit — hard correctness gate. Fails if any intrinsic
/// declared in the workspace TypeScript is missing from the registry.
#[test]
fn active_ts_sdk_intrinsic_audit_matches_default_registry() {
    let library = build_active_intrinsic_library();
    if library.lib_dir().is_none() {
        // No TypeScript installed (e.g. shallow CI image before
        // `pnpm install`). Skip rather than fail the workspace build.
        eprintln!(
            "skipping active-SDK intrinsic audit: no `typescript` package found under the workspace"
        );
        return;
    }
    // Version-DISCRIMINATING gate: when the workspace pins the TS>=7 rc
    // engine (an `@typescript/typescript-*` platform package is discoverable),
    // discovery MUST select THAT package's libs — not a coexisting legacy
    // `typescript@5/6` lib dir. The pre-fix lexicographic-last selection
    // chose `typescript@6.0.3` (its pnpm store dir sorts AFTER the `@`-named
    // rc store dir), scanning TS6 libs against the rc engine; this assertion
    // FAILS on that selection and PASSES on active-version selection. Both
    // facts are computed in `verter_workspace` (the allowlisted disk-reading
    // layer) so this audit performs no `std::fs` of its own.
    if library.rc_platform_package_available() {
        assert!(
            library.selected_lib_is_rc_platform(),
            "the workspace pins the rc TS>=7 engine, so the intrinsic scan must come from the \
             rc per-platform package (`@typescript/typescript-<platform>`), not a legacy \
             `typescript@<ver>` lib dir — got {:?}",
            library.lib_dir()
        );
    }

    let scanned = scan_intrinsics_via_library(&library);
    assert!(
        !scanned.is_empty(),
        "scanner must find at least one intrinsic declaration in {:?} — \
         an empty scan suggests the walker is broken",
        library.lib_dir()
    );
    let registry = IntrinsicRegistry::with_defaults();
    let missing = audit_unsupported(&registry, &scanned);
    assert!(
        missing.is_empty(),
        "active TypeScript SDK declares intrinsics the registry does not implement: {:?} \
         (lib dir: {:?})",
        missing.iter().map(|s| s.as_ref()).collect::<Vec<_>>(),
        library.lib_dir()
    );
}

/// Maintenance-CI audit — same scanner, opt-in via
/// `cargo test -- --ignored typescript_latest_intrinsic_audit`.
/// Intended to run against `typescript@latest` in a dedicated job so
/// upstream lib changes land in the registry before the pinned SDK
/// catches up.
///
/// The audit reuses the same discovery path as the active-SDK gate so
/// the two only differ in which version is installed; keeping them
/// textually identical avoids drift between the two code paths.
#[test]
#[ignore = "maintenance: run with `cargo test -- --ignored` after installing typescript@latest"]
fn typescript_latest_intrinsic_audit() {
    let library = build_active_intrinsic_library();
    if library.lib_dir().is_none() {
        panic!(
            "typescript@latest audit requires a `typescript` package in the workspace; install with `pnpm install` first"
        );
    }
    let scanned = scan_intrinsics_via_library(&library);
    let registry = IntrinsicRegistry::with_defaults();
    let missing = audit_unsupported(&registry, &scanned);
    assert!(
        missing.is_empty(),
        "typescript@latest declares intrinsics the registry does not implement: {:?}",
        missing.iter().map(|s| s.as_ref()).collect::<Vec<_>>(),
    );
}
