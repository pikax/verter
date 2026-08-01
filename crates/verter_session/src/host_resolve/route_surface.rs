//! `impl VerterHost` — route-surface facts, prepared-decl walking, and
//! dependency-source readers.
//!
//! Owns:
//! - `indexed_surface_is_current` — the shared parse-env reuse gate.
//! - `direct_import_canonicals` — the owner's resolved first-level deps.
//! - `resolve_prepared_decl_target` /
//!   `resolve_decl_in_scope_with_reexport_chain` (test-only) — host-state
//!   helpers that consult the prepared-decl bundles.
//! - `resolve_named_type_export_target` — test-only route-DB fixture that
//!   materialises the resolved target through `ensure_indexed_ready_serve`.
//! - `read_dep_source_for_type_resolution` — test-only effective-source reader.

#[cfg(test)]
use std::sync::Arc;

#[cfg(test)]
use crate::host_manage::component_meta_trace_custom;
use crate::VerterHost;

impl VerterHost {
    /// The COMPLETE reuse gate for a published `IndexedReady` surface:
    /// the owner's `parse_env_hash` (the R21 parse dimension) still
    /// equals the environment the artifact was parsed under.
    ///
    /// That is the whole gate. `IndexedReady` is a content-addressed
    /// PARSE artifact — authored import/export syntax, specifiers,
    /// shallow declarations, locators, parse-domain facts — so nothing
    /// on it is dependency-set derived and no route-resolution mutation
    /// (`configure_projects` / `set_exact_resolutions` /
    /// `configure_resolver`) can stale it. The edge-currency oracle this
    /// gate used to compose (a global `content_generation` equality over
    /// baked cross-file targets, plus a `project_generation` stamp) is
    /// deleted with the baked targets it guarded: resolution currency is
    /// a resolve-domain answer carried by the owner's import-route
    /// resolution witness and validated against a store view's captured
    /// immutable resolution world.
    ///
    /// A moved parse environment still routes the artifact through the
    /// FULL re-materialise (re-parse), because its `framework_parse` /
    /// `shallow_state` / `decl_bodies` were produced under the old one.
    pub(crate) fn indexed_surface_is_current(
        &self,
        canonical_id: &str,
        indexed: &crate::project_type_store::IndexedReady,
    ) -> bool {
        self.host_view_env_hashes_for(canonical_id).parse_env_hash == indexed.parse_env_hash
    }

    /// The owner's DIRECT import targets — every authored import
    /// specifier on the owner's shallow surface, resolved through the
    /// shared route-edge policy.
    ///
    /// The shallow inventory is PARSE domain: it names specifiers, never
    /// targets. Consumers that need the resolved first-level dependency
    /// set (the component-meta footprint file-role classifier) demand it
    /// here, through the one resolution authority, instead of reading a
    /// target baked into a content-addressed artifact — a baked target
    /// goes stale whenever the dependency file set moves while the
    /// owner's own bytes stay put.
    ///
    /// A specifier that does not resolve, or whose resolution is
    /// refused, contributes nothing.
    pub(crate) fn direct_import_canonicals(
        &self,
        canonical_id: &str,
    ) -> rustc_hash::FxHashSet<String> {
        let Some(state) = self.shallow_file_state(canonical_id) else {
            return rustc_hash::FxHashSet::default();
        };
        state
            .import_targets
            .values()
            .filter_map(|target| {
                match self.resolve_route_edge_canonical(canonical_id, &target.source_specifier) {
                    verter_workspace::ResolutionPublication::Admitted(admitted) => {
                        admitted.into_result()
                    }
                    verter_workspace::ResolutionPublication::Refused(_) => None,
                }
            })
            .collect()
    }

    /// host-level prepared-decl barrel routing
    /// helper.
    ///
    /// Returns the declaring `(canonical_id, symbol_name)` for the
    /// passed `(canonical_source, resolved_name)` pair after walking
    /// the re-export chain. Mirrors the query engine's
    /// (`resolver_core::component_meta_query_engine`)
    /// `resolve_final_prepared_type_target` semantics:
    /// - When `(canonical_source, resolved_name)` already has a
    ///   `prepared_type_decl`, returns it unchanged.
    /// - Otherwise consults `resolve_named_type_export_target_shallow`
    ///   for a re-export target and verifies the target itself has a
    ///   `prepared_type_decl`.
    /// - Falls back to the original pair when no prepared decl is
    ///   reachable.
    ///
    /// This is a host-state-only operation (no engine instance
    /// required). Dispatch-side helpers consume this directly to
    /// subsume the engine route fast-path's barrel routing.
    ///
    /// Test-only — exercised by the in-tree
    /// `host_resolve_tests::resolve_prepared_decl_target_*` cases
    /// included via the `#[cfg(test)] #[path] mod host_resolve_tests`
    /// declaration at the bottom of this file. The dispatch path no
    /// longer calls this helper directly (subsumed by the dispatch
    /// route projection over the barrel-chain shallow route), so the
    /// helper is gated `#[cfg(test)]` to keep the non-test dead-code
    /// surface minimal while preserving the regression-test contract.
    #[cfg(test)]
    pub(crate) fn resolve_prepared_decl_target(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> (String, String) {
        if self
            .prepared_type_decl(canonical_source, resolved_name)
            .is_some()
        {
            return (canonical_source.to_string(), resolved_name.to_string());
        }
        self.resolve_named_type_export_target_shallow(canonical_source, resolved_name)
            .filter(|(target_canonical, target_name)| {
                self.prepared_type_decl(target_canonical.as_str(), target_name.as_str())
                    .is_some()
            })
            .unwrap_or_else(|| (canonical_source.to_string(), resolved_name.to_string()))
    }

    /// host-level re-export chain walking helper.
    ///
    /// Resolves a bare-name reference in an exact owner scope, walking the
    /// re-export chain to the declaring file. Returns the canonical
    /// `DeclIdentity` describing the declaring file, the resolved
    /// symbol name, and the file's whole-hash.
    ///
    /// This subsumes the query engine's `dispatch_root_instantiated`
    /// two-layer resolution:
    /// 1. `resolve_bare_name_in_scope` → `(canonical_id, symbol_name)`.
    /// 2. `resolve_prepared_decl_target` → final declaring location.
    ///
    /// Returns `None` when the bare name is not visible from that owner. It
    /// never falls back to another top-level owner in the same file.
    ///
    /// Test-only — exercised by the in-tree
    /// `host_resolve_tests::resolve_decl_in_scope_with_reexport_chain_*`
    /// cases. The dispatch pipeline subsumed the helper into the
    /// cooperative bare-name resolution path; the helper is retained
    /// only for regression coverage.
    #[cfg(test)]
    pub(crate) fn resolve_decl_in_scope_with_reexport_chain(
        &self,
        scope_canonical_id: &str,
        scope_owner: verter_type_expr::TopLevelOwnerId,
        symbol_name: &str,
    ) -> Option<crate::semantic_query::DeclIdentity> {
        let scope_payload_arc = self.prepared_decl_bundle(scope_canonical_id).map(|bundle| {
            std::sync::Arc::new(
                crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                    &bundle,
                    scope_owner,
                ),
            )
        });
        let resolved_root = crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
            self,
            scope_canonical_id,
            scope_owner,
            scope_payload_arc.as_deref(),
            symbol_name,
        )?;
        // Walk the re-export chain to land on the declaring file.
        let (declaring_canonical, declaring_symbol) = self.resolve_prepared_decl_target(
            resolved_root.canonical_id.as_ref(),
            resolved_root.symbol_name.as_ref(),
        );
        let whole_hash = self
            .shallow_file_state(declaring_canonical.as_str())
            .map(|s| s.whole_hash)
            .unwrap_or_default();
        Some(crate::semantic_query::DeclIdentity {
            canonical_id: std::sync::Arc::from(declaring_canonical.as_str()),
            owner: resolved_root.owner,
            whole_hash,
            decl_name: std::sync::Arc::from(declaring_symbol.as_str()),
        })
    }

    /// Test-only bare wrapper around the view-bound route-resolution fixture
    /// surface.
    #[cfg(test)]
    pub(crate) fn resolve_named_type_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        // Test-only convenience: seed the resolve-and-cache method with a
        // cold-seed view (either `StoreViewRead` arm). Production
        // warm-validation of this route cache runs at the ctx-bound
        // request boundary, fenced by the outer publish token recheck;
        // these bare wrappers serve only direct-`host` test fixtures and
        // never churn the token mid-resolution.
        let live_view = self
            .resolver_store_view_read()
            .into_cold_seed_view()
            .into_inner();
        self.resolve_named_type_export_target_with_store_view(
            &live_view,
            dep_canonical,
            requested_name,
        )
    }

    /// View-bound test fixture variant.
    #[cfg(test)]
    pub(crate) fn resolve_named_type_export_target_with_store_view(
        &self,
        view: &dyn crate::resolver_core::StoreView,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        let result = self.resolve_named_type_export_target_uncached_with_store_view(
            view,
            dep_canonical,
            requested_name,
        )?;
        let _ = self.ensure_indexed_ready_serve(result.0.as_str());
        component_meta_trace_custom!(
            "resolve_named_type_export_target_result",
            format!(
                "owner={} requested={} source=route_db target={} exported={} materialized=true",
                dep_canonical, requested_name, result.0, result.1
            ),
        );
        Some(result)
    }

    /// Read the effective source for a dependency file for type resolution.
    ///
    /// On the scheduler path, tries the scheduler's source snapshot first.
    /// On the WASM path, tries `self.files` first.
    /// Both fall back to reading from the VFS workspace.
    /// For Vue SFCs, extracts only `<script>` / `<script setup>` content.
    #[cfg(test)]
    pub(crate) fn read_dep_source_for_type_resolution(
        &self,
        dep_canonical: &str,
        profile_hash: Option<u64>,
    ) -> Option<String> {
        component_meta_trace_custom!(
            "read_dep_source_for_type_resolution",
            format!("owner={} store_view={}", dep_canonical, false),
        );
        if let Some(_profile_hash) = profile_hash {
            if let Some(state) = self.effective_file_state(dep_canonical, None) {
                if self.store_view_allows_current_whole_hash(dep_canonical, state.whole_hash) {
                    let eval_source = Arc::<str>::from(Self::build_eval_script_source(
                        dep_canonical,
                        state.source.as_ref(),
                        state.framework_parse.as_deref(),
                    ));
                    component_meta_trace_custom!(
                        "read_dep_source_for_type_resolution_result",
                        format!(
                            "owner={} source=effective-file-state bytes={} has_framework_parse={} whole_hash={:?}",
                            dep_canonical,
                            eval_source.len(),
                            state.framework_parse.is_some(),
                            state.whole_hash,
                        ),
                    );
                    return Some(eval_source.to_string());
                }
            }
        }
        let facts = self.ensure_indexed_ready_serve(dep_canonical)?.indexed;
        let eval_source = Arc::clone(&facts.eval_source);
        component_meta_trace_custom!(
            "read_dep_source_for_type_resolution_result",
            format!(
                "owner={} source=module-facts bytes={} has_framework_parse={} whole_hash={:?}",
                dep_canonical,
                eval_source.len(),
                facts.framework_parse.is_some(),
                facts.whole_hash,
            )
        );
        Some(eval_source.to_string())
    }
}
