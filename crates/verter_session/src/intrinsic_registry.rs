//! TypeScript `intrinsic` declaration registry.
//!
//! The plan requires that named utility types like `Pick`, `Omit`, `Partial`,
//! and userland aliases resolve through the normal declaration path —
//! there is **no reserved-name handling**. The resolver only takes the
//! internal intrinsic execution path when a declaration's resolved body is
//! literally `= intrinsic;`.
//!
//! [`IntrinsicRegistry`] holds the mapping from resolved-declaration
//! identity to the in-tree implementation handler. Discovery scanners
//! (active TypeScript SDK, `typescript@latest` audit) walk the
//! `type X<...> = intrinsic;` declarations in `lib*.d.ts` and compare
//! them against this registry. Missing implementations fail the audit.
//!
//! ## Contract
//!
//! - Keys are the **resolved-declaration name** (e.g. `"Uppercase"`,
//!   `"NoInfer"`). Userland aliases that happen to share the same name
//!   never reach the intrinsic path — lookup happens only *after* normal
//!   declaration resolution yields `= intrinsic`.
//! - Entries are registered at runtime (plan) rather than via a macro so
//!   implementations can be added in lockstep with resolver changes.
//! - Missing intrinsics at user runtime return
//!   [`IntrinsicLookup::Unsupported`] so the caller can emit a structured
//!   diagnostic / opaque node rather than crashing. The repo audit uses
//!   [`IntrinsicRegistry::audit_has_entry`] to enforce hard-fail semantics
//!   in maintenance CI.

use std::sync::Arc;

use parking_lot::RwLock;
use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;

/// Well-known intrinsic implementation identity. The resolver chooses the
/// matching arm after the declaration resolves to `= intrinsic`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntrinsicImpl {
    /// String-case intrinsics (`Uppercase`, `Lowercase`, `Capitalize`,
    /// `Uncapitalize`).
    UppercaseString,
    LowercaseString,
    CapitalizeString,
    UncapitalizeString,
    /// `NoInfer<T>` — suppresses inference through the containing call.
    NoInfer,
    /// `BuiltinIteratorReturn` — placeholder; newer SDKs add new
    /// intrinsics without the resolver noticing, so we document its
    /// existence here even when the implementation is a thin passthrough.
    BuiltinIteratorReturn,
    /// `Promise<T>` — the global lib-declared promise type. NOT a
    /// `= intrinsic` alias: this is the registry's one global-TYPE
    /// carve-out so promise identity is decided by registry lookup on the
    /// resolved declaration name (the same lib-decl-identity rail the
    /// intrinsics use) instead of a name-string match inside the
    /// resolver. The lowering fast path interns an `InstantiationRef`
    /// carrier for an unshadowed `Promise<...>` reference, and the
    /// `Awaited` reducer arm consults this entry to recognise the
    /// carrier's declaration identity before unwrapping.
    PromiseGlobal,
}

impl IntrinsicImpl {
    /// Map this intrinsic identity onto the solver's
    /// [`BuiltinUtility`](verter_semantic::analysis::type_solver::builtin::BuiltinUtility)
    /// when one exists. `BuiltinIteratorReturn` has no `BuiltinUtility`
    /// equivalent today — callers are expected to return a symbolic node
    /// or emit a structured diagnostic in that case.
    ///
    /// This conversion is the bridge between the host-owned intrinsic
    /// registry and the existing solver dispatch: the solver looks up a
    /// name through [`IntrinsicRegistry::lookup`], and the session layer
    /// uses [`Self::as_builtin_utility`] to route the hit onto the
    /// established solver arm. Future phases route everything through
    /// `SemanticQueryApi::execute(Expand { .. })` directly.
    #[must_use]
    pub fn as_builtin_utility(self) -> Option<BuiltinUtility> {
        match self {
            Self::UppercaseString => Some(BuiltinUtility::Uppercase),
            Self::LowercaseString => Some(BuiltinUtility::Lowercase),
            Self::CapitalizeString => Some(BuiltinUtility::Capitalize),
            Self::UncapitalizeString => Some(BuiltinUtility::Uncapitalize),
            Self::NoInfer => Some(BuiltinUtility::NoInfer),
            Self::BuiltinIteratorReturn => None,
            // `Promise` is a global lib TYPE, not a utility — it never
            // routes onto a solver utility arm.
            Self::PromiseGlobal => None,
        }
    }
}

/// Lookup outcome. The resolver maps `Found` to the implementation path
/// and `Unsupported` to an opaque diagnostic node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntrinsicLookup {
    /// Registered intrinsic with an implementation handle.
    Found(IntrinsicImpl),
    /// The SDK declared a `= intrinsic` alias the registry has not learned
    /// about yet. At end-user runtime this must downgrade to a structured
    /// diagnostic per the plan; in repo / CI mode it is a hard failure.
    Unsupported { name: Arc<str> },
}

/// Host-owned registry. Internally a small `RwLock<FxHashMap>` — the
/// registry is write-once at construction and read-mostly afterwards.
#[derive(Debug, Default)]
pub struct IntrinsicRegistry {
    entries: RwLock<FxHashMap<Arc<str>, IntrinsicImpl>>,
}

impl IntrinsicRegistry {
    /// Build an empty registry. Use [`Self::with_defaults`] for the
    /// canonical in-tree set of intrinsics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Canonical in-tree registry. Populated with the intrinsics Verter
    /// currently supports — the audit scanner compares this against the
    /// active TypeScript SDK and fails on mismatches.
    pub fn with_defaults() -> Self {
        let reg = Self::new();
        reg.register("Uppercase", IntrinsicImpl::UppercaseString);
        reg.register("Lowercase", IntrinsicImpl::LowercaseString);
        reg.register("Capitalize", IntrinsicImpl::CapitalizeString);
        reg.register("Uncapitalize", IntrinsicImpl::UncapitalizeString);
        reg.register("NoInfer", IntrinsicImpl::NoInfer);
        reg.register(
            "BuiltinIteratorReturn",
            IntrinsicImpl::BuiltinIteratorReturn,
        );
        reg.register("Promise", IntrinsicImpl::PromiseGlobal);
        reg
    }

    pub fn register(&self, name: impl Into<Arc<str>>, impl_id: IntrinsicImpl) {
        self.entries.write().insert(name.into(), impl_id);
    }

    /// Lookup by resolved-declaration name. Only called after a
    /// declaration's body has resolved to `= intrinsic`; userland
    /// shadowing is handled upstream by normal declaration resolution and
    /// never reaches this table.
    #[must_use]
    pub fn lookup(&self, name: &str) -> IntrinsicLookup {
        let entries = self.entries.read();
        let result = entries.get(name).copied();
        if let Some(rctx) = crate::request_context::current_request_context() {
            if result.is_some() {
                rctx.cache_counters
                    .intrinsic_registry
                    .hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            } else {
                rctx.cache_counters
                    .intrinsic_registry
                    .misses
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        match result {
            Some(impl_id) => IntrinsicLookup::Found(impl_id),
            None => IntrinsicLookup::Unsupported {
                name: Arc::from(name),
            },
        }
    }

    /// Audit helper: returns `true` iff the name has a registered
    /// implementation. Used by the repo / CI intrinsic audit scanner.
    pub fn audit_has_entry(&self, name: &str) -> bool {
        self.entries.read().contains_key(name)
    }

    /// Iterate registered names. Primarily used by the audit scanner to
    /// report which intrinsics are supported.
    pub fn registered_names(&self) -> Vec<Arc<str>> {
        self.entries.read().keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.read().is_empty()
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for IntrinsicRegistry {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        &[ProjectGeneration]
    }
    fn invalidate(&self, _domain: crate::invalidation_domain::InvalidationDomain) {
        // The intrinsic registry holds the resolver's static lookup
        // table for `lib*.d.ts` `= intrinsic` declarations. It is
        // re-seeded on TS-SDK swap by the host's project-generation
        // bump path; the registry itself does not own a wholesale
        // invalidation method.
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for IntrinsicRegistry {
    fn invalidate_canonical_for(&self, _canonical_id: &str) -> usize {
        // The intrinsic registry is not file-content keyed — entries
        // are intrinsic names registered at boot from the active TS
        // SDK's `lib*.d.ts`. Per-canonical eviction is a no-op.
        0
    }
}

/// Extract every `type X<...> = intrinsic;` declaration name from a
/// `lib*.d.ts` source. Used by the repo audit (active-SDK + `typescript@latest`)
/// to enumerate intrinsics the resolver must implement.
///
/// This is an intentionally simple textual scan — TypeScript's `lib*.d.ts`
/// files declare intrinsics on one physical line with no conditional
/// gating, so lexical matching is sufficient and keeps the audit
/// dependency-free.
#[allow(dead_code)]
pub fn extract_intrinsics_from_lib_source(source: &str) -> Vec<Arc<str>> {
    let mut names = Vec::new();
    for raw in source.lines() {
        let line = raw.trim_start();
        // Match leading declaration variants:
        //   `type X<...> = intrinsic;`
        //   `declare type X<...> = intrinsic;`
        let rest = if let Some(after_declare) = line.strip_prefix("declare type") {
            after_declare
        } else if let Some(after_type) = line.strip_prefix("type") {
            after_type
        } else {
            continue;
        };
        // Must separate the `type` keyword from the name with whitespace.
        let Some(first) = rest.chars().next() else {
            continue;
        };
        if !first.is_whitespace() {
            continue;
        }
        let trimmed = rest.trim_start();
        // Must end with `= intrinsic;`.
        if !trimmed.contains("= intrinsic") {
            continue;
        }
        // Name is the prefix up to `<` or whitespace or `=`.
        let name_end = trimmed
            .find(|c: char| c == '<' || c.is_whitespace() || c == '=')
            .unwrap_or(trimmed.len());
        let name = &trimmed[..name_end];
        if !name.is_empty() {
            names.push(Arc::from(name));
        }
    }
    names
}

/// Compare a scanned intrinsic set against the registry; return the names
/// that the SDK declares but the registry does not implement. Empty result
/// ⇒ audit passes.
#[allow(dead_code)]
pub fn audit_unsupported(registry: &IntrinsicRegistry, scanned: &[Arc<str>]) -> Vec<Arc<str>> {
    scanned
        .iter()
        .filter(|name| !registry.audit_has_entry(name))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_contains_known_intrinsics() {
        let reg = IntrinsicRegistry::with_defaults();
        assert!(reg.audit_has_entry("Uppercase"));
        assert!(reg.audit_has_entry("Lowercase"));
        assert!(reg.audit_has_entry("Capitalize"));
        assert!(reg.audit_has_entry("Uncapitalize"));
        assert!(reg.audit_has_entry("NoInfer"));
        assert!(reg.audit_has_entry("BuiltinIteratorReturn"));
    }

    #[test]
    fn lookup_returns_unsupported_for_unknown() {
        let reg = IntrinsicRegistry::with_defaults();
        match reg.lookup("TotallyNewIntrinsicInNextSdk") {
            IntrinsicLookup::Unsupported { name } => {
                assert_eq!(name.as_ref(), "TotallyNewIntrinsicInNextSdk");
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn lookup_returns_found_for_registered() {
        let reg = IntrinsicRegistry::with_defaults();
        match reg.lookup("Uppercase") {
            IntrinsicLookup::Found(id) => {
                assert_eq!(id, IntrinsicImpl::UppercaseString);
            }
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn scanner_extracts_type_x_equals_intrinsic() {
        let src = r#"
// Skip declarations without intrinsic body.
type Foo = { x: number };
type Uppercase<S extends string> = intrinsic;
  type Lowercase<S extends string> = intrinsic;
declare type Capitalize<S extends string> = intrinsic;
type NotIntrinsic<T> = T extends string ? T : never;
// Whitespace + weird spacing.
type   NoInfer   <   T  >   = intrinsic   ;
"#;
        let names = extract_intrinsics_from_lib_source(src);
        let names: Vec<_> = names.iter().map(|s| s.as_ref().to_string()).collect();
        assert!(names.contains(&"Uppercase".to_string()));
        assert!(names.contains(&"Lowercase".to_string()));
        assert!(names.contains(&"Capitalize".to_string()));
        assert!(names.contains(&"NoInfer".to_string()));
        assert!(!names.contains(&"Foo".to_string()));
        assert!(!names.contains(&"NotIntrinsic".to_string()));
    }

    #[test]
    fn audit_unsupported_reports_missing_registry_entries() {
        let reg = IntrinsicRegistry::with_defaults();
        let scanned = vec![Arc::from("Uppercase"), Arc::from("NewIntrinsicInLatestSdk")];
        let missing = audit_unsupported(&reg, &scanned);
        let missing: Vec<_> = missing.iter().map(|s| s.as_ref().to_string()).collect();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0], "NewIntrinsicInLatestSdk");
    }

    #[test]
    fn registry_round_trip_register_and_lookup() {
        let reg = IntrinsicRegistry::new();
        reg.register("Custom", IntrinsicImpl::NoInfer);
        match reg.lookup("Custom") {
            IntrinsicLookup::Found(id) => assert_eq!(id, IntrinsicImpl::NoInfer),
            other => panic!("expected Found, got {other:?}"),
        }
    }

    #[test]
    fn intrinsic_impl_maps_to_builtin_utility() {
        assert_eq!(
            IntrinsicImpl::UppercaseString.as_builtin_utility(),
            Some(BuiltinUtility::Uppercase)
        );
        assert_eq!(
            IntrinsicImpl::LowercaseString.as_builtin_utility(),
            Some(BuiltinUtility::Lowercase)
        );
        assert_eq!(
            IntrinsicImpl::CapitalizeString.as_builtin_utility(),
            Some(BuiltinUtility::Capitalize)
        );
        assert_eq!(
            IntrinsicImpl::UncapitalizeString.as_builtin_utility(),
            Some(BuiltinUtility::Uncapitalize)
        );
        assert_eq!(
            IntrinsicImpl::NoInfer.as_builtin_utility(),
            Some(BuiltinUtility::NoInfer)
        );
        // BuiltinIteratorReturn has no BuiltinUtility equivalent today.
        assert_eq!(
            IntrinsicImpl::BuiltinIteratorReturn.as_builtin_utility(),
            None
        );
    }

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
}
