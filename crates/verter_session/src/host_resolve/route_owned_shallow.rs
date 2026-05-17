//! `impl VerterHost` — route-only shallow cache, prepared-decl walking,
//! and dependency-source readers.
//!
//! Owns the materialiser body for the route-only shallow artifact:
//! - `cached_route_owned_shallow_whole_hash` /
//!   `cached_route_owned_eval_state` /
//!   `cached_route_owned_snapshot` — readers that delegate to the
//!   project-store-owned `RouteOwnedShallowDb` via
//!   [`Self::ensure_route_owned_shallow_entry`].
//! - `ensure_route_owned_shallow_entry` — the singleflight-backed
//!   three-layer materialiser (pre-flight → singleflight → in-flight
//!   re-check / pre-publish fence).
//! - `route_owned_entry_is_fresh` (and the test-only
//!   `route_owned_entry_is_fresh_for_test` accessor).
//! - `resolve_prepared_decl_target` /
//!   `resolve_decl_in_scope_with_reexport_chain` (test-only) — host-state
//!   helpers that consult the prepared-decl bundles.
//! - `resolve_named_type_export_target` — the production wrapper around
//!   the route-DB cooperative resolve that also runs `ensure_indexed_ready`
//!   on the resolved target.
//! - `read_dep_source_for_type_resolution` — effective-source reader for
//!   external type resolution.
//! - `collect_external_types_from_loaded_files` — adapter that drives the
//!   `HostExternalMacroTypeCollector` over a file's macro-type deps.

use std::sync::Arc;

use super::external_macro_collector::HostExternalMacroTypeCollector;
use super::frontier_helpers::ResolvedExternalTypes;
use crate::host_manage::component_meta_trace_custom;
use crate::types::*;
use crate::VerterHost;

impl VerterHost {
    /// query the cached `whole_hash` for a canonical
    /// without forcing a cold materialisation. Used by warm-path callers
    /// that need a content-hash for `store_view_allows_current_whole_hash`
    /// without consuming the full route-only artifact. Reads the
    /// project-store DB directly via `get_any` (no tiered staleness gate
    /// here — callers reapply their own staleness check via
    /// `store_view_allows_current_whole_hash`).
    pub(crate) fn cached_route_owned_shallow_whole_hash(
        &self,
        canonical_id: &str,
    ) -> Option<Hash16> {
        let normalized_canonical = self
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());
        self.project_type_store
            .route_owned_shallow()
            .get_any(normalized_canonical.as_str())
            .map(|entry| entry.whole_hash)
    }

    /// The current route-surface hash for `canonical` — the single
    /// route-fact production helper. ONE source order, identical to the
    /// order [`crate::resolver_store::HostStoreView`] snapshots route
    /// facts in: the current-content `IndexedReady` artifact FIRST, the
    /// route-only shallow cache ONLY when no current indexed artifact
    /// exists.
    ///
    /// The `IndexedReady` artifact is the canonical route-surface
    /// authority. The route-owned-shallow entry is the fallback shape
    /// for a route-only file the indexed store has not materialised.
    /// A producer that built a `DerivedFactHash { Route }` signature
    /// fact from the route-owned-shallow surface while an `IndexedReady`
    /// existed would record a hash the store-view validator (which
    /// prefers the indexed surface) could not reproduce — a false stale
    /// miss. Routing every route-fact producer through this helper
    /// keeps the producer and the validator on one source order.
    ///
    /// The indexed lookup is content-pinned (no permissive `get_any`):
    /// the authoritative current content hash gates the artifact-store
    /// read, so a stale older-content candidate never answers. When the
    /// scheduler tracks a current content hash, the route-owned-shallow
    /// fallback is content-pinned to that same hash; only a genuinely
    /// scheduler-invisible route-only file (no authoritative current
    /// hash) reads the route-owned cache's single current entry
    /// unpinned — that entry is the most-recent publish for the
    /// canonical (the `RouteOwnedShallowDb` keeps one entry per
    /// canonical, replaced on every publish).
    pub(crate) fn current_route_surface_hash(&self, canonical_id: &str) -> Option<Hash16> {
        let normalized_canonical = self
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());
        let canonical = normalized_canonical.as_str();
        let current_hash = self
            .effective_file_state(canonical, None)
            .map(|state| state.whole_hash);
        // Source 1 — the current-content `IndexedReady` artifact. The
        // authoritative current content hash pins the lookup so only a
        // content-current artifact answers.
        if let Some(current_hash) = current_hash {
            if let Some(indexed) = self
                .project_type_store
                .indexed()
                .get(canonical, current_hash)
            {
                if indexed.shallow_state.has_resolvable_surface() {
                    return Some(crate::resolver_store::hash_route_surface(
                        &indexed.shallow_state,
                    ));
                }
                // A current indexed artifact exists but its surface is
                // not route-resolvable — there is no route fact, and
                // the route-owned-shallow fallback must NOT answer (it
                // would publish a hash the indexed authority overrode).
                return None;
            }
        }
        // Source 2 — the route-only shallow cache, ONLY when no current
        // indexed artifact answered. Content-pin to the authoritative
        // current hash when the scheduler tracks one; fall back to the
        // route-owned cache's single current entry when the canonical
        // is scheduler-invisible (a pure route-only file).
        let route_owned = self.project_type_store.route_owned_shallow();
        let entry = match current_hash {
            Some(current_hash) => route_owned.get(canonical, current_hash),
            None => route_owned.get_any(canonical),
        };
        entry
            .filter(|entry| entry.shallow_state.has_resolvable_surface())
            .map(|entry| crate::resolver_store::hash_route_surface(entry.shallow_state.as_ref()))
    }

    /// return the cached eval-state tuple for a
    /// canonical via the materialiser. On cache miss, materialises through
    /// [`Self::ensure_route_owned_shallow_entry`].
    pub(crate) fn cached_route_owned_eval_state(
        &self,
        canonical_id: &str,
    ) -> Option<(
        Arc<str>,
        Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
        Hash16,
    )> {
        let entry = self.ensure_route_owned_shallow_entry(canonical_id)?;
        Some((
            Arc::clone(&entry.raw_source),
            entry.cached_parse.clone(),
            entry.whole_hash,
        ))
    }

    /// return the cached `FileAnalysisSnapshot` for a
    /// canonical via the materialiser. On cache miss, materialises through
    /// [`Self::ensure_route_owned_shallow_entry`].
    pub(crate) fn cached_route_owned_snapshot(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::types::FileAnalysisSnapshot>> {
        let entry = self.ensure_route_owned_shallow_entry(canonical_id)?;
        Some(Arc::clone(&entry.snapshot))
    }

    /// shared materialiser for the route-only
    /// shallow artifact. Three-layer pattern matching the verified
    /// `ensure_indexed_ready` template at `host_manage.rs:3417`:
    ///
    /// 1. **Pre-flight fast path** — `get_any()` + tiered staleness gate
    ///    (warm callers exit zero-I/O).
    /// 2. **Singleflight on miss** — collapses concurrent cold callers to
    ///    one leader via
    ///    [`UnifiedResolverRuntime::route_owned_shallow_singleflight`](crate::resolver_core::resolver_runtime::UnifiedResolverRuntime::route_owned_shallow_singleflight).
    /// 3. **Inside flight**: re-check `get_any()` + tiered gate, capture
    ///    BOTH generations BEFORE the read, hash-validated re-check after
    ///    hashing, parse + analysis, then a **pre-publish fence** that
    ///    re-reads both generations to detect mid-flight mutations.
    ///
    /// The materialiser publishes once per content generation; subsequent
    /// callers within that generation see the published `Arc`.
    /// `Arc::ptr_eq` over the returned entry holds for two concurrent cold
    /// callers (singleflight collapse).
    pub(crate) fn ensure_route_owned_shallow_entry(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::project_type_store::RouteOwnedShallowEntry>> {
        let normalized_canonical = self
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());
        let canonical_id = normalized_canonical.as_str();

        // STEP 1 — pre-flight fast path (warm callers, ZERO I/O).
        if let Some(entry) = self
            .project_type_store
            .route_owned_shallow()
            .get_any(canonical_id)
        {
            if self.route_owned_entry_is_fresh(canonical_id, entry.as_ref()) {
                return Some(entry);
            }
            self.project_type_store
                .route_owned_shallow()
                .remove(canonical_id);
        }

        if canonical_id.is_empty() || crate::host_manage::is_raw_import_specifier_id(canonical_id) {
            return None;
        }

        // STEP 2 — singleflight on miss. Mirrors `indexed_singleflight`
        // pattern at host_manage.rs:3725–3743 — uses `()` error type, returns
        // `Option` at the outer fn.
        let canonical_arc: Arc<str> = Arc::from(canonical_id);
        let token = crate::resolver_core::StoreViewCompatToken {
            epoch: 0,
            session: None,
        };
        let materialize =
            || -> Result<Arc<crate::project_type_store::RouteOwnedShallowEntry>, ()> {
                // STEP 3 — re-check inside flight (apply the tiered gate again).
                if let Some(entry) = self
                    .project_type_store
                    .route_owned_shallow()
                    .get_any(canonical_id)
                {
                    if self.route_owned_entry_is_fresh(canonical_id, entry.as_ref()) {
                        return Ok(entry);
                    }
                    self.project_type_store
                        .route_owned_shallow()
                        .remove(canonical_id);
                }
                // STEP 4 — capture BOTH generations BEFORE read+parse, so any
                // mutation that lands during materialisation produces a generation
                // mismatch on the pre-publish fence (STEP 7).
                let workspace_generation = self.ws().content_generation();
                let project_generation = self.project_type_store.current_project_generation();

                let raw_source = self.read_analysis_source(canonical_id).ok_or(())?;
                let whole_hash = crate::hash::hash_16(raw_source.as_bytes());

                // STEP 5 — hash-validated re-check WITH the tiered gate.
                // A by-hash hit must still pass the full freshness gate (per
                // hard-stop constraint #13): tier-3 may reject the entry even
                // though tier-1 (whole_hash) matches.
                if let Some(entry) = self
                    .project_type_store
                    .route_owned_shallow()
                    .get(canonical_id, whole_hash)
                {
                    if self.route_owned_entry_is_fresh(canonical_id, entry.as_ref()) {
                        return Ok(entry);
                    }
                    self.project_type_store
                        .route_owned_shallow()
                        .remove(canonical_id);
                }

                // Honour the request-scoped store-view gate exactly like the
                // pre-migration body did (`store_view_allows_current_whole_hash`).
                if !self.store_view_allows_current_whole_hash(canonical_id, whole_hash) {
                    return Err(());
                }

                // If a parallel materialisation populated
                // `FileArtifactStore` for THIS content version while we
                // were reading, prefer that authoritative shape — the
                // `IndexedReady` fast path is the canonical reader.
                // Content-pinned lookup: `get_for_current_content`
                // checks for an artifact at exactly `whole_hash`, so a
                // stale older-content candidate (which `get_any` could
                // surface) does NOT spuriously abort the publish, and a
                // current-content candidate is correctly preferred.
                if self
                    .project_type_store
                    .indexed()
                    .get_for_current_content(canonical_id, whole_hash)
                    .is_some()
                {
                    // The IndexedReady authority is preferred; do NOT
                    // publish a route-only shadow.
                    return Err(());
                }

                // STEP 6 — cold parse + analysis.
                let cached_parse = canonical_id.ends_with(".vue").then(|| {
                    Arc::new(verter_compiler::compile::parse_sfc(&raw_source, None, None))
                });
                let eval_source = Arc::<str>::from(Self::build_eval_script_source(
                    raw_source.as_ref(),
                    cached_parse.as_deref(),
                ));
                let snapshot = Arc::new(self.build_route_owned_snapshot_from_source_state(
                    canonical_id,
                    &raw_source,
                    cached_parse.as_deref(),
                    whole_hash,
                ));
                let (eval_env, external_type_analysis) = self
                    .build_eval_env_and_external_type_analysis(
                        canonical_id,
                        whole_hash,
                        raw_source.as_ref(),
                        cached_parse.as_deref(),
                        &eval_source,
                    );
                let shallow_state =
                    Arc::new(crate::resolver_core::ShallowFileState::from_analysis(
                        whole_hash,
                        Arc::clone(&external_type_analysis),
                        Some(eval_env.as_ref()),
                    ));

                // STEP 7 — PRE-PUBLISH FENCE.
                // Re-read both generations. If either has bumped since STEP 4,
                // a route-resolution mutation or content mutation landed during
                // read+parse; the entry we just built is already stale. Abort
                // the publish so the next caller re-cold-materialises against
                // the new state.
                let workspace_generation_post = self.ws().content_generation();
                let project_generation_post = self.project_type_store.current_project_generation();
                if workspace_generation_post != workspace_generation
                    || project_generation_post != project_generation
                {
                    return Err(());
                }

                let entry = Arc::new(crate::project_type_store::RouteOwnedShallowEntry {
                    whole_hash,
                    workspace_generation,
                    project_generation,
                    raw_source: Arc::clone(&raw_source),
                    eval_source,
                    cached_parse,
                    snapshot,
                    external_type_analysis,
                    shallow_state,
                });
                self.project_type_store
                    .route_owned_shallow()
                    .publish(canonical_arc.clone(), Arc::clone(&entry));
                Ok(entry)
            };
        let singleflight = &self.resolver.runtime.route_owned_shallow_singleflight;
        match singleflight.run(canonical_arc.clone(), token, materialize) {
            Ok(run_result) => Some((*run_result.value).clone()),
            Err(()) => None,
        }
    }

    /// tiered staleness gate for route-only entries.
    /// Mirrors pre-migration `cached_route_owned_shallow_state_entry` body
    /// (host_resolve.rs:2128–2147) extended with tier-3 `project_generation`
    /// per tenth-pass Codex P0:
    ///
    /// - **Tier 3** — `entry.project_generation` must match
    ///   [`ProjectTypeStore::current_project_generation`]. Covers
    ///   `configure_projects` / `set_exact_resolutions` /
    ///   `configure_resolver` route-resolution mutations that DO NOT bump
    ///   `content_generation`.
    /// - **Tier 1** — when `get_whole_hash` returns `Some`, the scheduler-
    ///   backed authoritative content hash is the truth.
    /// - **Tier 2** — fallback for route-only files the scheduler hasn't
    ///   seen: `entry.workspace_generation == ws().content_generation()`
    ///   AND `ws().file_exists(canonical_id)`.
    fn route_owned_entry_is_fresh(
        &self,
        canonical_id: &str,
        entry: &crate::project_type_store::RouteOwnedShallowEntry,
    ) -> bool {
        // Tier 3 — project graph / route resolution.
        if entry.project_generation != self.project_type_store.current_project_generation() {
            return false;
        }
        // Tier 1 — scheduler-backed authoritative content hash.
        if let Some(auth_hash) = self.get_whole_hash(canonical_id) {
            return auth_hash == entry.whole_hash;
        }
        // Tier 2 — workspace_generation + file_exists.
        entry.workspace_generation == self.ws().content_generation()
            && self.ws().file_exists(canonical_id)
    }

    /// test-only accessor for the route-only freshness gate.
    /// Used by `cache_identity_invariants_tests` to discriminate tier-2
    /// behaviour without depending on the public materialiser path (which
    /// always populates `compile_cache` and would otherwise put tier-1 in
    /// charge).
    #[cfg(test)]
    pub(crate) fn route_owned_entry_is_fresh_for_test(
        &self,
        canonical_id: &str,
        entry: &crate::project_type_store::RouteOwnedShallowEntry,
    ) -> bool {
        self.route_owned_entry_is_fresh(canonical_id, entry)
    }
    /// host-level prepared-decl barrel routing
    /// helper.
    ///
    /// Returns the declaring `(canonical_id, symbol_name)` for the
    /// passed `(canonical_source, resolved_name)` pair after walking
    /// the re-export chain. Mirrors the legacy engine's
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
    /// longer calls this helper directly (subsumed by the cooperative
    /// `PreparedTargetDb` / barrel-chain pipeline), so the helper is
    /// gated `#[cfg(test)]` to keep the non-test dead-code surface
    /// minimal while preserving the regression-test contract.
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
    /// This subsumes the legacy engine's `dispatch_root_instantiated`
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

    pub(crate) fn resolve_named_type_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        let result =
            self.resolve_named_type_export_target_uncached(dep_canonical, requested_name)?;
        let _ = self.ensure_indexed_ready(result.0.as_str());
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
                        state.cached_parse.as_deref(),
                    ));
                    component_meta_trace_custom!(
                        "read_dep_source_for_type_resolution_result",
                        format!(
                            "owner={} source=effective-file-state bytes={} has_cached_parse={} whole_hash={:?}",
                            dep_canonical,
                            eval_source.len(),
                            state.cached_parse.is_some(),
                            state.whole_hash,
                        ),
                    );
                    return Some(eval_source.to_string());
                }
            }
        }
        let facts = self.ensure_indexed_ready(dep_canonical)?;
        let eval_source = Arc::clone(&facts.eval_source);
        component_meta_trace_custom!(
            "read_dep_source_for_type_resolution_result",
            format!(
                "owner={} source=module-facts bytes={} has_cached_parse={} whole_hash={:?}",
                dep_canonical,
                eval_source.len(),
                facts.cached_parse.is_some(),
                facts.whole_hash,
            )
        );
        Some(eval_source.to_string())
    }

    pub(super) fn collect_external_types_from_loaded_files(
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
        self.collect_external_types_from_loaded_files_with_view(
            owner_canonical,
            macro_type_deps,
            script_imports,
            profile_hash,
            None,
        )
    }

    /// View-aware variant of [`Self::collect_external_types_from_loaded_files`].
    ///
    /// Plumbs `view` into the [`HostExternalMacroTypeCollector`] so the
    /// per-macro-type-dep loop routes through the session-aware type-resolution
    /// path. Base callers (`view = None`) get the historical behaviour.
    pub(super) fn collect_external_types_from_loaded_files_with_view(
        &self,
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
            &HostExternalMacroTypeCollector { host: self, view },
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
