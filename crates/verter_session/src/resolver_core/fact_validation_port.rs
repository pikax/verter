//! Request-bound validation port. Answers are owned validity records; neither
//! the view nor the canonical-completion overlay can escape to a caller.

use std::collections::BTreeSet;
use verter_session_query::facts::fact_cache::{
    DerivedFactKind, FactVersionRef, ParseFactRef, ProgramAnalysisFactRef, ResolveImportsFactRef,
    RouteSurfaceFactRef,
};
use verter_session_query::facts::store_view::{ResolverHash16, StoreView, StoreViewCompatToken};

/// Samples the live aggregate clocks a fact basis composes. The clock source
/// type is concrete per host, so a live sample dispatches statically.
pub trait LiveFactValidation: FactValidation {
    /// The workspace clock source a live aggregate sample reads.
    type Clocks: verter_session_query::facts::clocks::WorkspaceClocks + Clone;
    fn aggregate_clock_reader(
        &self,
    ) -> verter_session_query::facts::clocks::AggregateClockReader<Self::Clocks>;
}

pub trait FactValidation {
    fn current_external_supersession_fingerprint(&self) -> u64;
    fn current_project_generation(&self) -> u64;
    fn source_environment(
        &self,
        key: &verter_session_query::source::artifact_key::FileArtifactKey,
    ) -> verter_session_query::source::env_identity::SourceEnvIdentity;
    fn complete_graph_signature(
        &self,
        roots: &[(
            std::sync::Arc<str>,
            verter_session_query::analysis::types::Hash16,
        )],
        facts: &[FactVersionRef],
    ) -> Result<
        crate::fact_signature_helpers::StructuralCarrierReadSet,
        crate::cache_runtime::NonAdmissionReason,
    >;
    fn record_signature_overflow(&self);
    #[cfg(test)]
    fn tracer_forcing(&self) -> (bool, usize);

    // -------- Component-meta-tier bridges --------------------------
    //
    // clippy cleanup — these two trait methods are part of
    // the resolver-context surface contract for component-meta-tier
    // adapters but have no caller in the landed tree. The trait is
    // sealed and the methods are
    // retained for symmetry with the dependency-fact and analysis-snap
    // bridges defined in the impl block below. `#[allow(dead_code)]` is
    // applied at the trait definition so the corresponding
    // implementations do not need
    // their own `#[allow]` annotations.

    #[allow(dead_code)]
    fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &BTreeSet<String>,
    ) -> Vec<FactVersionRef>;

    /// Record one observed fact onto the active tracer, or no-op if
    /// none is active.
    ///
    /// Cold-compute callers MUST call this for each fact they read
    /// from a content-addressed source. Warm-hit fast-path callers
    /// SHOULD NOT call it — the call is cheap, but the design
    /// intent is that warm validation reads the existing
    /// `fact_dep_signature` directly.
    #[inline]
    #[allow(dead_code)]
    fn observe(&self, fact: verter_session_query::facts::fact_cache::FactVersionRef) {
        crate::fact_tracing::tracing::observe_fan_out(fact);
    }

    /// Bulk-record a routed-hit's existing dep-signature onto the
    /// active tracer.
    ///
    /// Used when a higher-tier cold compute consumes a lower-tier
    /// cached result; the caller inherits the callee's observations
    /// without re-walking them.
    #[inline]
    #[allow(dead_code)]
    fn observe_borrowed_signature(
        &self,
        sig: &[verter_session_query::facts::fact_cache::FactVersionRef],
    ) {
        crate::fact_tracing::tracing::observe_fan_out_borrowed(sig);
    }
    fn compat_token(&self) -> StoreViewCompatToken;
    fn validates(&self, fact: &FactVersionRef) -> bool;
    fn validates_parse_domain(&self, fact: &ParseFactRef) -> bool;
    fn validates_resolve_imports_domain(&self, fact: &ResolveImportsFactRef) -> bool;
    fn validates_route_surface_domain(&self, fact: &RouteSurfaceFactRef) -> bool;
    fn validates_program_analysis_domain(&self, fact: &ProgramAnalysisFactRef) -> bool;
    fn validates_file_source_env(
        &self,
        canonical_id: &str,
        parse_env_hash: verter_session_query::facts::fact_cache::ParseEnvHash,
        parse_key: &verter_language::ParseKey,
        file_language_id: &verter_language::FileLanguage,
    ) -> bool;
    fn validates_self_root_whole_hash(&self, canonical_id: &str, hash: &ResolverHash16) -> bool;
    fn strict_self_root_world_identity(
        &self,
    ) -> Option<verter_session_query::facts::fact_cache::StrictSelfRootWorld>;
    fn strict_self_root_is_witnessable(&self, canonical_id: &str) -> bool;
    fn mint_strict_self_root_world(
        &self,
        roots: &[(&str, ResolverHash16)],
    ) -> Option<verter_session_query::facts::fact_cache::StrictSelfRootWorld>;
    fn tracks_file(&self, canonical_id: &str) -> bool;
    fn derived_hash_for(&self, canonical_id: &str, kind: DerivedFactKind)
        -> Option<ResolverHash16>;
    fn aggregate_basis_seed(&self) -> verter_session_query::facts::fact_cache::AggregateBasisSeed;
    fn validates_fact_signature(&self, sig: &[FactVersionRef]) -> bool;
    fn validate_fact_signature(
        &self,
        sig: &[FactVersionRef],
        self_root_canonicals: &[&str],
    ) -> Result<(), usize>;
    fn validates_fact_signature_with_self_roots(
        &self,
        sig: &[FactVersionRef],
        self_root_canonicals: &[&str],
    ) -> bool;
    fn promote_route_completion(
        &self,
        canonical: &str,
        whole_hash: verter_session_query::analysis::types::Hash16,
        route_hash: Option<verter_session_query::analysis::types::Hash16>,
    );
}

/// Internal static adapter for validators which already accept `StoreView`.
/// It contains only the validation port and cannot reach the other five ports.
pub(crate) struct FactValidationView<'a> {
    port: &'a dyn FactValidation,
}

impl<'a> FactValidationView<'a> {
    pub(crate) fn compat_token(&self) -> StoreViewCompatToken {
        self.port.compat_token()
    }

    pub(crate) fn new(port: &'a dyn FactValidation) -> Self {
        Self { port }
    }
}

impl StoreView for FactValidationView<'_> {
    fn compat_token(&self) -> StoreViewCompatToken {
        self.port.compat_token()
    }
    fn validates(&self, fact: &FactVersionRef) -> bool {
        self.port.validates(fact)
    }
    fn validates_parse_domain(&self, fact: &ParseFactRef) -> bool {
        self.port.validates_parse_domain(fact)
    }
    fn validates_resolve_imports_domain(&self, fact: &ResolveImportsFactRef) -> bool {
        self.port.validates_resolve_imports_domain(fact)
    }
    fn validates_route_surface_domain(&self, fact: &RouteSurfaceFactRef) -> bool {
        self.port.validates_route_surface_domain(fact)
    }
    fn validates_program_analysis_domain(&self, fact: &ProgramAnalysisFactRef) -> bool {
        self.port.validates_program_analysis_domain(fact)
    }
    fn validates_file_source_env(
        &self,
        canonical_id: &str,
        parse_env_hash: verter_session_query::facts::fact_cache::ParseEnvHash,
        parse_key: &verter_language::ParseKey,
        file_language_id: &verter_language::FileLanguage,
    ) -> bool {
        self.port.validates_file_source_env(
            canonical_id,
            parse_env_hash,
            parse_key,
            file_language_id,
        )
    }
    fn validates_self_root_whole_hash(&self, canonical_id: &str, hash: &ResolverHash16) -> bool {
        self.port.validates_self_root_whole_hash(canonical_id, hash)
    }
    fn strict_self_root_world_identity(
        &self,
    ) -> Option<verter_session_query::facts::fact_cache::StrictSelfRootWorld> {
        self.port.strict_self_root_world_identity()
    }
    fn strict_self_root_is_witnessable(&self, canonical_id: &str) -> bool {
        self.port.strict_self_root_is_witnessable(canonical_id)
    }
    fn mint_strict_self_root_world(
        &self,
        roots: &[(&str, ResolverHash16)],
    ) -> Option<verter_session_query::facts::fact_cache::StrictSelfRootWorld> {
        self.port.mint_strict_self_root_world(roots)
    }
    fn tracks_file(&self, canonical_id: &str) -> bool {
        self.port.tracks_file(canonical_id)
    }
    fn derived_hash_for(
        &self,
        canonical_id: &str,
        kind: DerivedFactKind,
    ) -> Option<ResolverHash16> {
        self.port.derived_hash_for(canonical_id, kind)
    }
    fn aggregate_basis_seed(&self) -> verter_session_query::facts::fact_cache::AggregateBasisSeed {
        self.port.aggregate_basis_seed()
    }
    fn validates_fact_signature(&self, sig: &[FactVersionRef]) -> bool {
        self.port.validates_fact_signature(sig)
    }
    fn validate_fact_signature(
        &self,
        sig: &[FactVersionRef],
        self_root_canonicals: &[&str],
    ) -> Result<(), usize> {
        self.port.validate_fact_signature(sig, self_root_canonicals)
    }
    fn validates_fact_signature_with_self_roots(
        &self,
        sig: &[FactVersionRef],
        self_root_canonicals: &[&str],
    ) -> bool {
        self.port
            .validates_fact_signature_with_self_roots(sig, self_root_canonicals)
    }
    fn promote_route_completion(
        &self,
        canonical: &str,
        whole_hash: verter_session_query::analysis::types::Hash16,
        route_hash: Option<verter_session_query::analysis::types::Hash16>,
    ) {
        self.port
            .promote_route_completion(canonical, whole_hash, route_hash)
    }
}
