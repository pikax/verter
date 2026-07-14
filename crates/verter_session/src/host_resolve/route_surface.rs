//! `impl VerterHost` — route-surface facts, prepared-decl walking, and
//! dependency-source readers.
//!
//! Owns:
//! - `current_route_surface_hash` — the single route-fact production
//!   helper (IndexedReady is the SOLE route-surface authority).
//! - `route_surface_is_edge_current` — the shared edge-currency oracle.
//! - `resolve_prepared_decl_target` /
//!   `resolve_decl_in_scope_with_reexport_chain` (test-only) — host-state
//!   helpers that consult the prepared-decl bundles.
//! - `resolve_named_type_export_target` — the production wrapper around
//!   the route-DB cooperative resolve that also runs `ensure_indexed_ready_serve`
//!   on the resolved target.
//! - `read_dep_source_for_type_resolution` — effective-source reader for
//!   external type resolution.
//! - `collect_external_types_from_loaded_files` — adapter that drives the
//!   `HostExternalMacroTypeCollector` over a file's macro-type deps.

#[cfg(test)]
use std::sync::Arc;

use super::external_macro_collector::HostExternalMacroTypeCollector;
use super::frontier_helpers::ResolvedExternalTypes;
use crate::host_manage::component_meta_trace_custom;
use crate::types::*;
use crate::VerterHost;

impl VerterHost {
    /// The current route-surface hash for `canonical` — the single
    /// route-fact production helper. ONE source, identical to the source
    /// [`crate::resolver_store::HostStoreView`] snapshots route facts
    /// from: the current-content `IndexedReady` artifact. There is no
    /// secondary route-surface artifact; a route-only file the indexed
    /// store has not materialised simply has no `Route` fact yet (its
    /// first traversal materialises it through `ensure_indexed_ready_serve`).
    ///
    /// The lookup is content-pinned to the scheduler's authoritative
    /// current hash when one exists; a scheduler-invisible canonical
    /// reads through the NON-RECURSING artifact-only authority
    /// ([`Self::artifact_current_indexed_raw`] — declines for any
    /// scheduler-tracked canonical, so a permissive multi-candidate
    /// read can never bake a stale content hash into the route-fact
    /// oracle), matching the source order the store-view validator
    /// uses.
    pub(crate) fn current_route_surface_hash(&self, canonical_id: &str) -> Option<Hash16> {
        let normalized_canonical = self
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());
        let canonical = normalized_canonical.as_str();
        let current_hash = self
            .effective_file_state(canonical, None)
            .map(|state| state.whole_hash);
        let indexed = match current_hash {
            Some(current_hash) => self
                .project_type_store
                .indexed()
                .get(canonical, current_hash),
            None => self.artifact_current_indexed_raw(canonical),
        }?;
        // The indexed artifact is the route-surface authority ONLY while
        // edge-current: an artifact with cross-file edges whose baked
        // edges are stale (a dependency appeared / retargeted while the
        // owner content stayed put) produces NO `Route` fact, forcing a
        // cold re-resolve against the live file set.
        if !self.indexed_surface_is_current(canonical, &indexed) {
            return None;
        }
        indexed
            .shallow_state
            .has_resolvable_surface()
            .then(|| crate::resolver_store::hash_route_surface(&indexed.shallow_state))
    }

    /// The COMPLETE reuse gate for an `IndexedReady` surface: the
    /// edge-currency oracle ([`Self::route_surface_is_edge_current`])
    /// PLUS the `project_generation` stamp for any surface with
    /// cross-file edges PLUS the owner's `parse_env_hash` (the R21 parse
    /// dimension) once the project graph has moved. Route-resolution
    /// mutations (`configure_projects` / `set_exact_resolutions` /
    /// `configure_resolver`) bump `project_generation` without bumping
    /// `content_generation`; a content-current artifact whose edges were
    /// resolved under the old project graph fails here and routes
    /// through the edge-refresh materialise (payload reused, route
    /// surface rebuilt — parse env permitting; see
    /// `ensure_indexed_ready_serve`'s refresh decision). A surface with NO
    /// cross-file edges is insensitive to route mutations and reuses on
    /// edge currency — but its parse/env payloads are reusable only
    /// while the owner's parse environment is unchanged, so a stale
    /// project stamp demands parse-env equality before the
    /// route-insensitive reuse applies. Every parse-env-moving mutation
    /// bumps `project_generation` (config/workspace mutations), so a
    /// CURRENT project stamp proves the parse env has not moved and the
    /// per-canonical env lookup is skipped on the hot warm path.
    pub(crate) fn indexed_surface_is_current(
        &self,
        canonical_id: &str,
        indexed: &crate::project_type_store::IndexedReady,
    ) -> bool {
        if !self.route_surface_is_edge_current(indexed) {
            return false;
        }
        if indexed.project_generation == self.project_type_store.current_project_generation() {
            return true;
        }
        !indexed.has_cross_file_edges()
            && self.host_view_env_hashes_for(canonical_id).parse_env_hash == indexed.parse_env_hash
    }

    /// The single edge-currency oracle for ANY route surface — base and
    /// session-overlay `IndexedReady` artifacts alike.
    ///
    /// EVERY cross-file edge — a wildcard `export *`, a named reexport, a
    /// plain import target, AND a resolved `import_routes` entry (the
    /// external `src=` class, caller-pushed route snapshots) — bakes its
    /// target `canonical_id` at the workspace generation when the edge was
    /// resolved (`edge_generation`). Those baked canonicals depend on the
    /// DEPENDENCY file set, not the owner's own content, so a
    /// content-pinned surface whose owner content is unchanged can still
    /// hold a STALE edge after a dependency appears or retargets (e.g. a
    /// `.js` edge whose `.d.ts` companion later appears, or a
    /// directory-index edge a more-specific file shadows): the file set
    /// changed and `content_generation` advanced.
    ///
    /// The edge inventory consulted here is the COMPLETE
    /// [`crate::project_type_store::IndexedReady::has_cross_file_edges`]
    /// authority — the shallow-inventory component alone
    /// (`has_shallow_cross_file_edges`) is blind to import-route-only
    /// artifacts, whose only baked targets live in `import_routes`; judging
    /// on the component would keep such a surface "current" forever across
    /// `content_generation` moves and stale-serve route facts and compile
    /// slots after retargets. The oracle therefore takes the artifact, not
    /// a bare shallow state.
    ///
    /// A surface is edge-current iff it carries no cross-file edges (nothing
    /// dependency-set-derived to go stale) OR its `edge_generation` still
    /// matches the live workspace `content_generation`. Every `Route`-fact
    /// producer/validator and the materializer-reuse gates route a surface
    /// through THIS predicate so a non-edge-current surface is never
    /// produced, served, or reused. The check is a cheap per-read stamp
    /// compare; the real work happens once, in the edge-refresh materialise
    /// (route surface rebuilt from the retained content payload — no
    /// re-read, no re-parse) that an edge-stale surface routes through.
    pub(crate) fn route_surface_is_edge_current(
        &self,
        indexed: &crate::project_type_store::IndexedReady,
    ) -> bool {
        !indexed.has_cross_file_edges() || indexed.edge_generation == self.ws().content_generation()
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
    /// Resolves a bare-name reference in a scope, walking the
    /// re-export chain to the declaring file. Returns the canonical
    /// `DeclIdentity` describing the declaring file, the resolved
    /// symbol name, and the file's whole-hash.
    ///
    /// This subsumes the query engine's `dispatch_root_instantiated`
    /// two-layer resolution:
    /// 1. `resolve_bare_name_in_scope` → `(canonical_id, symbol_name)`.
    /// 2. `resolve_prepared_decl_target` → final declaring location.
    ///
    /// Returns `None` only when the bare name cannot be resolved at
    /// all and the requested scope is itself missing a shallow
    /// state.
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
        symbol_name: &str,
    ) -> Option<crate::semantic_query::DeclIdentity> {
        let scope_payload_arc = self.prepared_decl_bundle(scope_canonical_id).map(|bundle| {
            std::sync::Arc::new(
                crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                    &bundle,
                ),
            )
        });
        let resolved_root = crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
            self,
            scope_canonical_id,
            scope_payload_arc.as_deref(),
            symbol_name,
        )
        .map(|root| (root.canonical_id, root.symbol_name))
        .unwrap_or_else(|| (scope_canonical_id.to_string(), symbol_name.to_string()));
        // Walk the re-export chain to land on the declaring file.
        let (declaring_canonical, declaring_symbol) =
            self.resolve_prepared_decl_target(resolved_root.0.as_str(), resolved_root.1.as_str());
        let whole_hash = self
            .shallow_file_state(declaring_canonical.as_str())
            .map(|s| s.whole_hash)
            .unwrap_or_default();
        Some(crate::semantic_query::DeclIdentity {
            canonical_id: std::sync::Arc::from(declaring_canonical.as_str()),
            whole_hash,
            decl_name: std::sync::Arc::from(declaring_symbol.as_str()),
        })
    }

    /// Test-only bare wrapper around the view-bound variant. Production
    /// callers go through `ctx.resolve_named_type_export_target` (which
    /// routes through the request-bound `_with_store_view`); the
    /// test-only arm on `impl ResolverContext for VerterHost` reaches
    /// this wrapper on test fixtures that call `host.<method>` directly.
    #[cfg(any(test, debug_assertions))]
    #[allow(dead_code)]
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

    /// View-bound variant — production-reachable through ctx-bound
    /// `HostResolverContext` / `SessionResolverContext` callers.
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

    pub(super) fn collect_external_types_from_loaded_files(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        owner_canonical: &str,
        macro_type_deps: &[verter_semantic::analysis::MacroTypeDep],
        script_imports: &[verter_semantic::analysis::AnalyzedImport],
        profile_hash: Option<u64>,
    ) -> (
        Option<ResolvedExternalTypes>,
        Vec<HostDiagnostic>,
        std::collections::BTreeSet<String>,
    ) {
        self.collect_external_types_from_loaded_files_with_view(
            ctx,
            owner_canonical,
            macro_type_deps,
            script_imports,
            profile_hash,
            None,
        )
    }

    /// Test-only driver that exercises the PRODUCTION external-macro collector
    /// ([`HostExternalMacroTypeCollector`] — the sole legacy `ResolvedElements`
    /// caller) through a bare-host ctx, without reaching for the full IDE
    /// virtual-file pipeline.
    #[cfg(test)]
    pub(crate) fn collect_external_types_from_loaded_files_for_test(
        &self,
        owner_canonical: &str,
        macro_type_deps: &[verter_semantic::analysis::MacroTypeDep],
        script_imports: &[verter_semantic::analysis::AnalyzedImport],
        profile_hash: Option<u64>,
    ) -> (
        Option<ResolvedExternalTypes>,
        Vec<HostDiagnostic>,
        std::collections::BTreeSet<String>,
    ) {
        crate::resolver_core::with_bare_host_ctx_for_test(self, |ctx| {
            self.collect_external_types_from_loaded_files(
                ctx,
                owner_canonical,
                macro_type_deps,
                script_imports,
                profile_hash,
            )
        })
    }

    /// View-aware variant of [`Self::collect_external_types_from_loaded_files`].
    ///
    /// Plumbs `view` into the [`HostExternalMacroTypeCollector`] so the
    /// per-macro-type-dep loop routes through the session-aware type-resolution
    /// path. Base callers (`view = None`) get the historical behaviour.
    pub(super) fn collect_external_types_from_loaded_files_with_view(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        owner_canonical: &str,
        macro_type_deps: &[verter_semantic::analysis::MacroTypeDep],
        script_imports: &[verter_semantic::analysis::AnalyzedImport],
        profile_hash: Option<u64>,
        view: Option<&dyn crate::session_view::SessionView>,
    ) -> (
        Option<ResolvedExternalTypes>,
        Vec<HostDiagnostic>,
        std::collections::BTreeSet<String>,
    ) {
        let collected = crate::resolver_core::collect_external_macro_types(
            &HostExternalMacroTypeCollector {
                host: self,
                view,
                ctx,
            },
            owner_canonical,
            macro_type_deps,
            script_imports,
            profile_hash,
        );

        (
            collected.resolved,
            collected
                .diagnostics
                .into_iter()
                .map(|diag| HostDiagnostic {
                    severity: HostSeverity::Error,
                    code: diag.code,
                    message: diag.message,
                    span: diag.span,
                })
                .collect(),
            collected.tracked_dependencies,
        )
    }
}
