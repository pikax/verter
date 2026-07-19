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
    /// Test-only bare wrapper. Production callers go through
    /// `ctx.resolve_imported_type_root` (which routes through the
    /// request-bound `_with_store_view`); the test-only arm on
    /// `impl ResolverContext for VerterHost` reaches this wrapper on
    /// test fixtures that call `host.<method>` directly.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)]
    pub(crate) fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> Option<verter_semantic::analysis::type_solver::ResolvedRootIdentity> {
        self.resolve_imported_type_root_with_facts(dep_canonical, imported_name)
            .0
    }

    /// View-bound variant of [`Self::resolve_imported_type_root`].
    ///
    /// Request-bound callers (`HostResolverContext`,
    /// `SessionResolverContext`) route through this variant so the
    /// cached imported-root entry validates against the supplied view
    /// rather than rebuilding a fresh owned workspace snapshot.
    /// Delegates to
    /// [`Self::resolve_imported_type_root_with_facts_with_store_view`]
    /// and discards the route-chain fact tuple.
    ///
    /// Fact-DISCARDING: MUST NOT back a memoized-build path — the
    /// discarded route facts are the only proof a barrel retarget
    /// invalidates the enclosing cache entry. Memoized builds route
    /// through the `_with_facts` sibling and record the facts onto the
    /// active tracer.
    pub(crate) fn resolve_imported_type_root_with_store_view(
        &self,
        view: &dyn crate::resolver_core::StoreView,
        dep_canonical: &str,
        imported_name: &str,
    ) -> Option<verter_semantic::analysis::type_solver::ResolvedRootIdentity> {
        self.resolve_imported_type_root_with_facts_with_store_view(
            view,
            dep_canonical,
            imported_name,
        )
        .0
    }

    /// Like [`Self::resolve_imported_type_root`] but ALSO returns
    /// the full route-chain fact list the resolution observed.
    /// Producers that thread the recorded facts into a downstream
    /// cache entry (e.g. `OwnerImportSurfaceDb`, R3/R26/R28)
    /// consume this variant so the dependent cache observes every
    /// barrel/reexport participant — not only the final target's
    /// `FileWholeHash`.
    ///
    /// Test-only bare wrapper. Production callers compose the request
    /// boundary explicitly (build a view + call
    /// `_with_facts_with_store_view`). The test-only wrapper above
    /// (`resolve_imported_type_root`) reaches this helper transitively.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)]
    pub(crate) fn resolve_imported_type_root_with_facts(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (
        Option<verter_semantic::analysis::type_solver::ResolvedRootIdentity>,
        Arc<[crate::resolver_core::FactVersionRef]>,
    ) {
        // Test-only convenience: seed the resolve-and-cache method with a
        // cold-seed view (either `StoreViewRead` arm). The production
        // warm-validation of this route cache runs at the ctx-bound
        // request boundary and is fenced by the outer
        // `publish_component_meta_cache_entry` token recheck; these bare
        // wrappers exist only for test fixtures that call `host.<method>`
        // directly and never churn the token mid-resolution.
        let view = self
            .resolver_store_view_read()
            .into_cold_seed_view()
            .into_inner();
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
        view: &dyn crate::resolver_core::StoreView,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (
        Option<verter_semantic::analysis::type_solver::ResolvedRootIdentity>,
        Arc<[crate::resolver_core::FactVersionRef]>,
    ) {
        let audit_started = self.config.audit_enabled.then(Instant::now);

        let normalized_canonical = self
            .resolve_eval_dependency_canonical(dep_canonical)
            .unwrap_or_else(|| dep_canonical.to_string());

        // `ImportedRootDb` owns the OUTERMOST cacheability scope: the
        // route walk inside the resolve closure rides `ensure_indexed_ready_serve`
        // and demands decl bodies, so it can consume a FENCED serve, a BROKEN
        // decl-body lease, or an UNROOTABLE route. Three of those four reasons are
        // CONTENT-NEUTRAL — the artifact stays published and content-current — so
        // an entry admitted under one roots on the LIVE hash and validates on every
        // warm read forever. The funnel consults the scope's verdict AFTER the
        // resolve runs and refuses the persist; the root is still returned.
        let cached = self
            .resolver
            .runtime
            .imported_roots
            .get_or_resolve_returning_facts_with_context(
                normalized_canonical.as_str(),
                imported_name,
                view,
                self,
                || {
                    // Trace inside the closure: the closure runs only on
                    // cache miss, so the trace event records actual
                    // resolution work — not redundant lookups.
                    component_meta_trace_custom!("resolve_imported_type_root", {
                        use std::fmt::Write as _;
                        let mut detail =
                            String::with_capacity(24 + dep_canonical.len() + imported_name.len());
                        let _ = write!(
                            detail,
                            "canonical={} imported={}",
                            dep_canonical, imported_name
                        );
                        detail
                    });

                    if let Some((resolved, facts)) = self
                        .resolve_direct_imported_type_root_fast_path(
                            normalized_canonical.as_str(),
                            imported_name,
                        )
                    {
                        return Some((
                            crate::resolver_core::ImportedRootResult::Resolved {
                                canonical_source: resolved.0,
                                owner: resolved.1,
                                resolved_symbol: resolved.2,
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
                            defining_owner,
                            defining_symbol,
                        } => crate::resolver_core::ImportedRootResult::Resolved {
                            canonical_source: self
                                .resolve_eval_dependency_canonical(defining_canonical.as_str())
                                .unwrap_or(defining_canonical),
                            owner: defining_owner,
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
            Some((cached, facts)) => (cached.as_identity(), "named_export_target", facts),
            None => (
                None,
                "miss",
                crate::fact_signature_helpers::empty_fact_signature(),
            ),
        };

        component_meta_trace_custom!("resolve_imported_type_root_result", {
            // Audit-gated (the macro skips this block without an active
            // accumulator). Pre-size the detail so the per-call build is
            // ONE exact allocation instead of the `format!` grow chain —
            // this trace fires on every call, warm hits included.
            use std::fmt::Write as _;
            let mut detail = String::with_capacity(
                96 + dep_canonical.len()
                    + imported_name.len()
                    + normalized_canonical.len()
                    + source_kind.len()
                    + resolved.as_ref().map_or(0, |identity| {
                        identity.canonical_id.len() + identity.symbol_name.len()
                    }),
            );
            let target_canonical = resolved
                .as_ref()
                .map_or("<miss>", |identity| identity.canonical_id.as_ref());
            let target_symbol = resolved
                .as_ref()
                .map_or("<miss>", |identity| identity.symbol_name.as_ref());
            let _ = write!(
                detail,
                "canonical={} imported={} normalized={} source={} target_canonical={} target_symbol={} store_view=false",
                dep_canonical, imported_name, normalized_canonical, source_kind, target_canonical, target_symbol,
            );
            detail
        });

        if let Some(started) = audit_started {
            crate::component_meta_audit::record_imported_root_proof_ms(
                started.elapsed().as_secs_f64() * 1000.0,
            );
        }

        (resolved, facts)
    }
}
