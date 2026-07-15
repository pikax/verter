//! Test-only re-exports for integration tests in `tests/`.
//!
//! Items under this module are NOT a public API — they are
//! hidden from documentation and exist purely so integration
//! tests can probe internal invariants (e.g. the content-addressed
//! `MapperFingerprint` substrate). Production code MUST NOT
//! import from here. The architecture guard
//! `test_only_module_is_only_consumed_by_test_files` (see
//! `tests/cases/architecture_guards.rs`) pins this contract.

/// Test-only probe for the content-addressed `MapperFingerprint`
/// primitive. The wrapper exposes the minimal surface needed by
/// `tests/cases/g_misc3/mapper_fingerprint_content_addressed.rs` without
/// promoting the internal `MapperFingerprint` / `MapperBinderRegistry`
/// types to the crate's public API.
///
/// The `private_interfaces` lint fires here because the wrapper
/// methods are `pub` while the wrapped `MapperFingerprint`
/// stays `pub(crate)`. The whole purpose of the wrapper is to
/// keep the inner type out of the public API while still
/// letting integration tests drive it — so the lint is
/// deliberately suppressed.
pub mod mapper_fingerprint {
    use std::sync::Arc;

    use verter_type_expr::{MappedModifier, TypeExpr};

    use crate::mapper_binder_registry::{MapperBinderRegistry, MapperFingerprint};

    /// Public newtype around the internal `MapperFingerprint`.
    /// This is what `tests/cases/g_misc3/mapper_fingerprint_content_addressed.rs`
    /// asserts equality / inequality on. The newtype keeps the
    /// inner type out of the public API surface while still
    /// letting integration tests drive its observable
    /// behaviour.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Fingerprint(MapperFingerprint);

    /// Owning wrapper around the internal `MapperBinderRegistry`
    /// so the test can drive `ordinal_for` end-to-end without
    /// reaching into the host. Constructing a fresh registry
    /// keeps the test hermetic — no shared state across tests.
    pub struct Registry(MapperBinderRegistry);

    /// Probe substrate. Free functions on this zero-sized type
    /// keep the test's call sites readable: every probe entry
    /// is `MapperFingerprintProbe::*`.
    pub struct MapperFingerprintProbe;

    impl MapperFingerprintProbe {
        /// Build a content-addressed fingerprint from the
        /// same components the production lowering passes.
        #[inline]
        pub fn from_components(
            source: &Arc<TypeExpr>,
            value: &Arc<TypeExpr>,
            optional: MappedModifier,
            readonly: MappedModifier,
            name_type: Option<&Arc<TypeExpr>>,
        ) -> Fingerprint {
            Fingerprint(MapperFingerprint::from_components(
                source, value, optional, readonly, name_type,
            ))
        }

        /// Construct a fresh, empty `MapperBinderRegistry` so
        /// each test has independent ordinal state.
        #[inline]
        pub fn fresh_registry() -> Registry {
            Registry(MapperBinderRegistry::new())
        }

        /// Get / assign the stable ordinal for
        /// `(canonical, display_name, fingerprint)` against the
        /// test's owned registry.
        #[inline]
        pub fn ordinal_for(
            registry: &Registry,
            canonical: &Arc<str>,
            display_name: &Arc<str>,
            fp: Fingerprint,
        ) -> u16 {
            registry.0.ordinal_for(canonical, display_name, fp.0)
        }

        /// Test-only accessor for the raw `u64` content hash.
        /// Used by the stack-safety test to assert the
        /// returned fingerprint is non-default.
        #[inline]
        pub fn raw(fp: Fingerprint) -> u64 {
            fp.0.raw()
        }
    }
}

/// Test-only probe for the budget-exceeded published-surface
/// recognizer. Integration tests (`tests/cases/defect_b_corpus_prevention_gate.rs`)
/// scan a published surface for a leaked budget sentinel; this
/// re-exports the SAME `pub(crate)` recognizer production routing
/// uses (`type_expr_is_budget_exceeded_sentinel`, which keys on the
/// `BUDGET_EXCEEDED_SENTINEL_PREFIX` constant `semantic_query_error_raw`
/// emits) so the test's spelling can NEVER drift from the producer's.
pub mod budget_sentinel {
    use verter_type_expr::TypeExpr;

    /// Returns `true` iff `expr` is the budget-exceeded sentinel
    /// (`TypeExpr::Unknown { raw }` starting with the production
    /// `budgetExceeded(` prefix). Delegates verbatim to the shared
    /// `pub(crate)` production recognizer.
    #[inline]
    pub fn is_budget_exceeded_sentinel(expr: &TypeExpr) -> bool {
        crate::resolver_core::component_meta_query_engine::type_expr_is_budget_exceeded_sentinel(
            expr,
        )
    }
}

/// Test-only demand probes for published `SemanticTypeSource` carriers.
///
/// Publication is shallow-by-default; a test asserting the type a consumer
/// resolves from a published source performs the demand step explicitly
/// through these probes — both route raise → reduction → sealed output
/// materialisation through the ONE shared dispatch (no second engine).
#[cfg(any(test, feature = "test-support"))]
pub mod semantic_source_probe {
    /// Demand-materialize a published source under `Published(Expanded)` —
    /// the explicit full consumer walk.
    pub use crate::project_semantic_dispatch::semantic_source::demand_semantic_source_type_expr as demand_type_expr;
    /// Shell-materialize a published source WITHOUT a reduction demand —
    /// the shallow published shape (`Ref` / utility carriers survive).
    pub use crate::project_semantic_dispatch::semantic_source::shallow_semantic_source_type_expr as shallow_type_expr;
}

/// Test-only arm for the component-meta output force-fail knob: the next
/// `build_component_meta_output` for EXACTLY `canonical` fails with a typed
/// `ComponentMetaOutputError` (consuming that canonical's arm). A
/// canonical-keyed SET — process-global, so it reaches batch pool-worker
/// threads and the LSP integration harness, and multi-arm, so concurrently
/// running tests arming DIFFERENT canonicals never steal (or overwrite)
/// each other's arm.
#[cfg(any(test, feature = "test-support"))]
pub mod component_meta_output {
    /// Arm the canonical-keyed force-fail knob for `canonical`.
    pub fn force_output_failure_for(canonical: &str) {
        crate::meta_resolve::projectors::OUTPUT_MATERIALIZE_FORCE_FAIL_FOR
            .lock()
            .unwrap()
            .insert(canonical.to_string());
    }
}
