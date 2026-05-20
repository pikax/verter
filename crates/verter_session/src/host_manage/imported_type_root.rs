//! Cross-file resolved-import-root materialization on [`VerterHost`].
//!
//! Producers ask either [`VerterHost::resolve_imported_type_root`]
//! (legacy tuple return) or
//! [`VerterHost::resolve_imported_type_root_with_facts`] (R3/R26/R28:
//! returns the route-chain `FactVersionRef` list alongside the tuple).
//! Both route through the host's `imported_roots` `ValidatedFactCache`
//! via [`crate::resolver_core::imported_root_db::ImportedRootDb::get_or_resolve_returning_facts`].
//! Direct-binding fast-path + full route-walk fallback live together
//! here so the only stage that materializes imported-root resolutions
//! is one shared function.

use std::sync::Arc;

use super::component_meta_trace_custom;
use crate::instant::Instant;
use crate::VerterHost;

impl VerterHost {
    pub(crate) fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (String, String) {
        self.resolve_imported_type_root_with_facts(dep_canonical, imported_name)
            .0
    }

    /// Like [`Self::resolve_imported_type_root`] but ALSO returns
    /// the full route-chain fact list the resolution observed.
    /// Producers that thread the recorded facts into a downstream
    /// cache entry (e.g. `OwnerImportSurfaceDb` — Gap 1, R3/R26/R28)
    /// consume this variant so the dependent cache observes every
    /// barrel/reexport participant — not only the final target's
    /// `FileWholeHash`.
    pub(crate) fn resolve_imported_type_root_with_facts(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (
        (String, String),
        Arc<[crate::resolver_core::FactVersionRef]>,
    ) {
        let view = self.resolver_store_view();
        self.resolve_imported_type_root_with_facts_with_store_view(
            &view,
            dep_canonical,
            imported_name,
        )
    }

    /// View-bound variant of [`Self::resolve_imported_type_root_with_facts`].
    ///
    /// Validates the cached imported-root entry against the supplied
    /// request-bound view; eliminates the per-call full-workspace
    /// snapshot the pre-6.c rail performed at this site (the diagnostic's
    /// named hot-path site at `imported_type_root.rs:49`).
    pub(crate) fn resolve_imported_type_root_with_facts_with_store_view(
        &self,
        view: &crate::resolver_store::HostStoreView,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (
        (String, String),
        Arc<[crate::resolver_core::FactVersionRef]>,
    ) {
        let audit_started = self.config.audit_enabled.then(Instant::now);

        let normalized_canonical = self
            .resolve_eval_dependency_canonical(dep_canonical)
            .unwrap_or_else(|| dep_canonical.to_string());

        let cached = self
            .resolver
            .runtime
            .imported_roots
            .get_or_resolve_returning_facts(
                normalized_canonical.as_str(),
                imported_name,
                view,
                || {
                    // Trace inside the closure: the closure runs only on
                    // cache miss, so the trace event records actual
                    // resolution work — not redundant lookups.
                    component_meta_trace_custom!(
                        "resolve_imported_type_root",
                        format!("canonical={} imported={}", dep_canonical, imported_name),
                    );

                    if let Some((resolved, facts)) = self
                        .resolve_direct_imported_type_root_fast_path(
                            normalized_canonical.as_str(),
                            imported_name,
                        )
                    {
                        return Some((
                            crate::resolver_core::ImportedRootResult::Resolved {
                                canonical_source: resolved.0,
                                resolved_symbol: resolved.1,
                            },
                            facts,
                        ));
                    }
                    // Use resolve_named_type_export_target which checks
                    // the RouteDb before doing the barrel walk. This avoids
                    // redundant barrel walks when the route has already been
                    // resolved by a prior query. Then collect full route
                    // participant facts via build_named_type_export_route_entry
                    // for proper cache invalidation on intermediate barrel changes.
                    let (route_result, facts) = self.build_named_type_export_route_entry(
                        normalized_canonical.as_str(),
                        imported_name,
                    )?;
                    let root_result = match route_result {
                        crate::resolver_core::RouteResult::Resolved {
                            defining_canonical,
                            defining_symbol,
                        } => crate::resolver_core::ImportedRootResult::Resolved {
                            canonical_source: self
                                .resolve_eval_dependency_canonical(defining_canonical.as_str())
                                .unwrap_or(defining_canonical),
                            resolved_symbol: defining_symbol,
                        },
                        crate::resolver_core::RouteResult::Miss => {
                            crate::resolver_core::ImportedRootResult::Miss
                        }
                    };
                    Some((root_result, facts))
                },
            );
        let (resolved, source_kind, facts) = match cached {
            Some((cached, facts)) => match cached.as_tuple() {
                Some(tuple) => (tuple, "named_export_target", facts),
                None => (
                    (normalized_canonical.clone(), imported_name.to_string()),
                    "miss",
                    facts,
                ),
            },
            None => (
                (normalized_canonical.clone(), imported_name.to_string()),
                "miss",
                Arc::from(Vec::<crate::resolver_core::FactVersionRef>::new()),
            ),
        };

        component_meta_trace_custom!(
            "resolve_imported_type_root_result",
            format!(
                "canonical={} imported={} normalized={} source={} target_canonical={} target_symbol={} store_view={}",
                dep_canonical,
                imported_name,
                normalized_canonical,
                source_kind,
                resolved.0,
                resolved.1,
                false
            ),
        );

        if let Some(started) = audit_started {
            crate::component_meta_audit::record_imported_root_proof_ms(
                started.elapsed().as_secs_f64() * 1000.0,
            );
        }

        (resolved, facts)
    }
}
