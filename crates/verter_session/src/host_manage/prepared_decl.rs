//! `host_manage::prepared_decl` — fact-validated `PreparedDeclBundle`
//! materialisation, shallow-file-state lookup, and import-route resolution
//! used by the resolver / engine layers.
//!
//! Domain F. Owns the largest single block of
//! cache-discipline code in `host_manage`: the bundle materialiser, the
//! prepared-decl freshness gate, the imported-symbol dependency walker,
//! the indexed-ready upsert path, and the owner-direct-import surface.
//! Public surface remains rooted at `crate::host_manage::*`; this file
//! contributes a continuation `impl VerterHost { … }` block.

use std::sync::Arc;

use crate::instant::Instant;

use crate::types::*;
use crate::VerterHost;

use super::{
    collect_type_expr_symbol_refs, component_meta_debug, component_meta_debug_enabled,
    component_meta_trace_custom, dep_edges_from_resolutions, is_builtin_type_symbol,
    is_raw_import_specifier_id, is_runtime_script_target, HostShallowImportResolver,
    ImportedSymbolDependency,
};

impl VerterHost {
    // -----------------------------------------------------------------------
    // Fact-validated PreparedDeclBundle cache
    // -----------------------------------------------------------------------

    /// Look up (or materialize) the fact-validated prepared-decl bundle for a
    /// canonical file.  On a warm read the cost is O(facts.len()) — no
    /// dependency-resolution or route-refresh work is performed.
    pub(crate) fn prepared_decl_bundle(
        &self,
        canonical_id: &str,
    ) -> Option<std::sync::Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>> {
        let normalized_canonical_id = self.normalized_analysis_canonical(canonical_id);
        let canonical_id = normalized_canonical_id.as_ref();

        // Live-host probe: use resolver_store_view for validation.
        let view_for_get = self.resolver_store_view();

        // Fast path: fact-validated cache hit.
        let bundles = &self.resolver.runtime.prepared_decl_bundles;
        let key = canonical_id.to_string();
        if let Some(bundle) = bundles.get_if_valid(&key, &view_for_get) {
            self.provenance
                .bundle_cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Some(bundle);
        }

        // Cold path with singleflight: coalesce concurrent materializations
        // for the same canonical_id + store-view compat token.
        use crate::resolver_core::StoreView;
        let token = view_for_get.compat_token();
        let singleflight = bundles.singleflight();
        let flight = singleflight.run(key.clone(), token, || {
            // Re-check cache inside the singleflight leader closure (another
            // thread may have populated it between our first check and winning
            // the flight).
            if let Some(bundle) = bundles.get_if_valid(&key, &view_for_get) {
                return Ok(crate::resolver_core::StableExecutionValue {
                    value: Some((*bundle).clone()),
                    stable: true,
                });
            }
            let result = self
                .materialize_prepared_decl_bundle_from_route_owned_shallow(canonical_id)
                .or_else(|| self.materialize_prepared_decl_bundle(canonical_id));
            let stable = result.is_some();
            Ok(crate::resolver_core::StableExecutionValue {
                value: result.map(|arc| (*arc).clone()),
                stable,
            })
        });
        match flight {
            Ok(f) => f.value.value.clone().map(std::sync::Arc::new),
            Err(()) => None,
        }
    }

    fn prepared_decl_bundle_route_dep_edges(
        &self,
        canonical_id: &str,
        state: &crate::resolver_core::ShallowFileState,
    ) -> (
        rustc_hash::FxHashMap<String, String>,
        Option<crate::resolver_core::ResolverHash16>,
    ) {
        let declaration_file = canonical_id.ends_with(".d.ts")
            || canonical_id.ends_with(".d.mts")
            || canonical_id.ends_with(".d.cts");
        let mut dep_edges = rustc_hash::FxHashMap::default();
        let mut import_routes = rustc_hash::FxHashMap::default();
        let mut seen_sources = rustc_hash::FxHashSet::default();

        for target in state.import_targets.values() {
            if !seen_sources.insert(target.source_specifier.clone()) {
                continue;
            }

            let cached_resolution =
                self.cached_import_route_resolution(canonical_id, target.source_specifier.as_str());
            let resolved: Option<String> = if let Some(resolution) = cached_resolution.as_ref() {
                self.prefer_type_dependency_target_from_resolution(
                    canonical_id,
                    target.source_specifier.as_str(),
                    resolution,
                )
                .or_else(|| {
                    if Self::import_route_is_known_miss(resolution) {
                        None
                    } else if !(target.canonical_id.is_empty()
                        || declaration_file && is_runtime_script_target(&target.canonical_id))
                    {
                        Some(target.canonical_id.clone())
                    } else {
                        self.resolve_route_type_edge(canonical_id, target.source_specifier.as_str())
                    }
                })
            } else if !(target.canonical_id.is_empty()
                || declaration_file && is_runtime_script_target(&target.canonical_id))
            {
                Some(target.canonical_id.clone())
            } else {
                self.resolve_route_type_edge(canonical_id, target.source_specifier.as_str())
            };
            let Some(resolved) = resolved else {
                continue;
            };

            dep_edges.insert(target.source_specifier.clone(), resolved.clone());
            import_routes.insert(
                target.source_specifier.clone(),
                cached_resolution.unwrap_or(crate::types::DependencyResolution {
                    specifier: target.source_specifier.clone(),
                    resolved_canonical_id: Some(resolved.clone()),
                    possible_canonical_ids: vec![resolved],
                }),
            );
        }

        let import_route_hash = (!import_routes.is_empty())
            .then(|| crate::resolver_store::hash_import_route_targets(&import_routes));
        (dep_edges, import_route_hash)
    }

    fn materialize_prepared_decl_bundle_from_route_owned_shallow(
        &self,
        canonical_id: &str,
    ) -> Option<std::sync::Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>> {
        let declaration_file = canonical_id.ends_with(".d.ts")
            || canonical_id.ends_with(".d.mts")
            || canonical_id.ends_with(".d.cts");
        if !declaration_file {
            return None;
        }

        let state = self.route_owned_shallow_state(canonical_id)?;
        if state.symbols.is_empty()
            && state.value_symbols.is_empty()
            && state.exports.is_empty()
            && state.import_targets.is_empty()
        {
            return None;
        }

        let (dep_edges, import_route_hash) =
            self.prepared_decl_bundle_route_dep_edges(canonical_id, state.as_ref());
        let bundle = std::sync::Arc::new(
            crate::resolver_core::prepared_decl::build_prepared_decl_bundle(
                canonical_id,
                std::sync::Arc::clone(&state),
                dep_edges,
                rustc_hash::FxHashMap::default(),
            ),
        );

        let mut facts = vec![crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.to_string(),
            hash: state.whole_hash,
        }];
        if let Some(import_route_hash) = import_route_hash {
            facts.push(crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: canonical_id.to_string(),
                kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                hash: import_route_hash,
            });
        }

        self.resolver.runtime.prepared_decl_bundles.insert_arc(
            canonical_id.to_string(),
            std::sync::Arc::clone(&bundle),
            facts,
        );

        self.provenance
            .bundle_materializations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        component_meta_trace_custom!(
            "materialize_prepared_decl_bundle",
            format!(
                "owner={} type_decls={} value_decls={} dep_edges={} source=route_shallow",
                canonical_id,
                bundle.prepared_type_decls.len(),
                bundle.prepared_value_decls.len(),
                bundle.dep_edges.len(),
            ),
        );

        Some(bundle)
    }

    /// Materialize a fresh `PreparedDeclBundle` for a canonical file, insert it
    /// into the stable cache with the appropriate fact versions, and return it.
    fn materialize_prepared_decl_bundle(
        &self,
        canonical_id: &str,
    ) -> Option<std::sync::Arc<crate::resolver_core::prepared_decl::PreparedDeclBundle>> {
        // 1. Ensure source/shallow data exists.
        let facts = self.ensure_indexed_ready(canonical_id)?;
        let state = &facts.shallow_state;
        if state.symbols.is_empty()
            && state.value_symbols.is_empty()
            && state.exports.is_empty()
            && state.import_targets.is_empty()
        {
            return None;
        }
        let (dep_edges, import_route_hash) =
            self.prepared_decl_bundle_route_dep_edges(canonical_id, state.as_ref());

        // 4. Build script-setup type bindings for Vue SFCs (once per bundle).
        // Non-Vue files get an empty map — zero cost.
        let script_setup_type_bindings = if canonical_id.ends_with(".vue") {
            self.build_script_setup_type_bindings(canonical_id, state.as_ref(), &dep_edges)
        } else {
            rustc_hash::FxHashMap::default()
        };

        // 5. Build the bundle atomically.
        let bundle = std::sync::Arc::new(
            crate::resolver_core::prepared_decl::build_prepared_decl_bundle(
                canonical_id,
                std::sync::Arc::clone(state),
                dep_edges,
                script_setup_type_bindings,
            ),
        );

        // 6. Compute fact versions.
        // Always include ImportRoute when present — all prepared bundles
        // embed resolved cross-file canonical IDs (dep_edges, import_bindings,
        // name_resolution, external_deps) and must be invalidated when the
        // import graph changes, regardless of whether the file is tracked.
        let whole_hash = facts.whole_hash;
        let mut facts = vec![crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.to_string(),
            hash: whole_hash,
        }];
        if let Some(import_route_hash) = import_route_hash {
            facts.push(crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: canonical_id.to_string(),
                kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                hash: import_route_hash,
            });
        }

        // 7. Insert into the stable cache.
        self.resolver.runtime.prepared_decl_bundles.insert_arc(
            canonical_id.to_string(),
            std::sync::Arc::clone(&bundle),
            facts,
        );

        self.provenance
            .bundle_materializations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        component_meta_trace_custom!(
            "materialize_prepared_decl_bundle",
            format!(
                "owner={} type_decls={} value_decls={} dep_edges={}",
                canonical_id,
                bundle.prepared_type_decls.len(),
                bundle.prepared_value_decls.len(),
                bundle.dep_edges.len(),
            ),
        );

        Some(bundle)
    }

    pub(crate) fn prepared_type_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>> {
        let bundle = self.prepared_decl_bundle(canonical_id)?;
        let result = bundle.prepared_type_decls.get(symbol_name);
        component_meta_trace_custom!(
            "prepared_type_decl_result",
            format!(
                "owner={} symbol={} source=bundle_hit hit={}",
                canonical_id,
                symbol_name,
                result.is_some(),
            ),
        );
        result
    }

    pub(crate) fn prepared_value_decl(
        &self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        let bundle = self.prepared_decl_bundle(canonical_id)?;
        bundle.prepared_value_decls.get(symbol_name)
    }

    /// Route-aware required-import closure.
    /// Uses the shallow file state's `route_closure` to narrow the import set
    /// to only dependencies reachable from the requested route.
    ///
    /// Falls back to the whole-export closure when route-aware data is unavailable.
    pub(crate) fn required_import_routes_for_exported_route(
        &self,
        canonical_id: &str,
        exported_name: &str,
        route: &crate::resolver_core::RouteDemand,
    ) -> rustc_hash::FxHashMap<String, crate::resolver_core::RouteDemand> {
        use crate::resolver_core::shallow_file_state::ExportTarget;
        use crate::resolver_core::RouteDemand;

        if let Some(state) = self.route_owned_shallow_state(canonical_id) {
            let budget = crate::resolver_core::shallow_file_state::ResolutionBudgets::default()
                .local_closure_steps;
            if let Some((symbol_name, _is_alias_export)) = state
                .export_target(exported_name)
                .and_then(|target| match target {
                    ExportTarget::Local { symbol_name } => {
                        Some((symbol_name.as_str(), symbol_name != exported_name))
                    }
                    ExportTarget::Reexport { .. } => None,
                })
            {
                let closure = state.route_closure(symbol_name, route, budget);
                let mut result = rustc_hash::FxHashMap::default();
                for ext in &closure.unresolved_external {
                    result
                        .entry(ext.local_name.clone())
                        .and_modify(|existing| {
                            *existing =
                                crate::resolver_core::merge_route_demands(existing, &ext.route);
                        })
                        .or_insert_with(|| ext.route.clone());
                }
                if state.symbol(symbol_name).is_some_and(|symbol| {
                    symbol.kind == verter_semantic::analysis::type_eval::TypeDeclKind::Class
                }) {
                    if let Some(analysis) = self.external_type_analysis(canonical_id) {
                        for required_name in analysis.required_import_names(exported_name) {
                            result
                                .entry(required_name)
                                .and_modify(|existing| {
                                    *existing = crate::resolver_core::merge_route_demands(
                                        existing,
                                        &RouteDemand::Whole,
                                    );
                                })
                                .or_insert(RouteDemand::Whole);
                        }
                    }
                }
                return result;
            }

            if !matches!(route, RouteDemand::Whole) {
                return self.required_import_routes_for_exported_route(
                    canonical_id,
                    exported_name,
                    &RouteDemand::Whole,
                );
            }
        }

        if matches!(route, RouteDemand::Whole) {
            return self
                .external_type_analysis(canonical_id)
                .map(|analysis| {
                    analysis
                        .required_import_names(exported_name)
                        .into_iter()
                        .map(|name| (name, RouteDemand::Whole))
                        .collect()
                })
                .unwrap_or_default();
        }

        self.required_import_routes_for_exported_route(
            canonical_id,
            exported_name,
            &RouteDemand::Whole,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn required_import_names_for_exported_route(
        &self,
        canonical_id: &str,
        exported_name: &str,
        route: &crate::resolver_core::RouteDemand,
    ) -> rustc_hash::FxHashSet<String> {
        let required_routes =
            self.required_import_routes_for_exported_route(canonical_id, exported_name, route);
        let required = required_routes
            .keys()
            .cloned()
            .collect::<rustc_hash::FxHashSet<_>>();

        if component_meta_debug_enabled() {
            let mut required_list = required.iter().cloned().collect::<Vec<_>>();
            required_list.sort();
            component_meta_debug(format!(
                "required_import_names_for_route source={} exported={} route={:?} source_kind=fresh count={} imports=[{}]",
                canonical_id,
                exported_name,
                route,
                required.len(),
                required_list.join(", "),
            ));
        }

        required
    }

    fn imported_symbol_dependencies(
        &self,
        canonical_id: &str,
        exported_name: &str,
        decl_body: &verter_type_expr::TypeExpr,
    ) -> Vec<ImportedSymbolDependency> {
        let analysis = match self.external_type_analysis(canonical_id) {
            Some(analysis) => analysis,
            None => return Vec::new(),
        };
        let mut dependencies = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        let mut referenced_names = std::collections::BTreeSet::new();
        collect_type_expr_symbol_refs(decl_body, &mut referenced_names);
        for referenced_name in referenced_names {
            let root_name = referenced_name
                .split('.')
                .next()
                .unwrap_or(referenced_name.as_str());
            if root_name == exported_name || is_builtin_type_symbol(root_name) {
                continue;
            }

            if let Some((import_source, imported_name)) =
                analysis.local_import_symbol_target(root_name)
            {
                let (resolved_canonical, resolved_name) = if root_name == referenced_name {
                    // Direct owner import — resolve via the project-global
                    // owner surface so every stage reads the same cached
                    // answer for this `(owner, local_name)` pair.
                    match self.resolve_owner_direct_import(canonical_id, root_name) {
                        Some(resolved) => resolved,
                        None => continue,
                    }
                } else {
                    // Dotted reference like `Foo.Bar` — preserve the legacy
                    // suffixed name lookup path; the direct-import surface
                    // only caches top-level `local_name` entries.
                    let suffix = referenced_name.strip_prefix(root_name).unwrap_or("");
                    let imported_member = format!("{}{}", imported_name, suffix);
                    let Some(dep_canonical) =
                        self.resolve_type_dependency_canonical(canonical_id, import_source)
                    else {
                        continue;
                    };
                    self.resolve_imported_type_root(
                        dep_canonical.as_str(),
                        imported_member.as_str(),
                    )
                };
                if seen.insert((
                    referenced_name.clone(),
                    resolved_canonical.clone(),
                    resolved_name.clone(),
                )) {
                    dependencies.push(ImportedSymbolDependency {
                        local_name: referenced_name,
                        canonical_id: resolved_canonical,
                        exported_name: resolved_name,
                    });
                }
                continue;
            }

            if analysis.local_symbol_span(root_name).is_some()
                && seen.insert((
                    root_name.to_string(),
                    canonical_id.to_string(),
                    root_name.to_string(),
                ))
            {
                dependencies.push(ImportedSymbolDependency {
                    local_name: root_name.to_string(),
                    canonical_id: canonical_id.to_string(),
                    exported_name: root_name.to_string(),
                });
            }
        }
        dependencies.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.canonical_id.cmp(&right.canonical_id))
                .then_with(|| left.exported_name.cmp(&right.exported_name))
        });
        dependencies
    }

    pub(crate) fn imported_symbol_dependencies_for_expr(
        &self,
        canonical_id: &str,
        expr: &verter_type_expr::TypeExpr,
    ) -> Vec<ImportedSymbolDependency> {
        self.cache_only_lookup_symbol_dependencies_for_expr(canonical_id, expr)
    }

    fn cache_only_lookup_symbol_dependencies_for_expr(
        &self,
        canonical_id: &str,
        expr: &verter_type_expr::TypeExpr,
    ) -> Vec<ImportedSymbolDependency> {
        let mut dependencies = self.imported_symbol_dependencies(canonical_id, "", expr);
        dependencies.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.canonical_id.cmp(&right.canonical_id))
                .then_with(|| left.exported_name.cmp(&right.exported_name))
        });
        dependencies
    }

    pub(crate) fn external_type_analysis(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource>>
    {
        component_meta_trace_custom!(
            "external_type_analysis",
            format!("owner={} store_view={}", canonical_id, false),
        );
        let inputs = self.external_type_resolution_inputs(canonical_id)?;
        let analysis = Arc::clone(&inputs.analysis);
        let stats = analysis.stats();
        if inputs.analysis_cache_hit {
            component_meta_trace_custom!(
                "external_type_analysis_cache_hit",
                format!(
                    "owner={} statements={} bindings={} reexports={} wildcards={} import_locals={} local_type_symbols={} local_export_symbols={}",
                    canonical_id,
                    stats.top_level_statement_count,
                    stats.binding_count,
                    stats.direct_reexport_count,
                    stats.wildcard_reexport_count,
                    stats.import_local_count,
                    stats.local_type_symbol_count,
                    stats.local_export_symbol_count,
                ),
            );
        } else {
            component_meta_trace_custom!(
                "external_type_analysis_built",
                format!(
                    "owner={} statements={} bindings={} reexports={} wildcards={} import_locals={} local_type_symbols={} local_export_symbols={}",
                    canonical_id,
                    stats.top_level_statement_count,
                    stats.binding_count,
                    stats.direct_reexport_count,
                    stats.wildcard_reexport_count,
                    stats.import_local_count,
                    stats.local_type_symbol_count,
                    stats.local_export_symbol_count,
                ),
            );
        }
        Some(analysis)
    }

    /// Get or build the canonical shallow type file state for an imported
    /// dependency.  The state is populated through the shared host ensure-path
    /// and cached in `FileArtifactStore`.
    ///
    /// Consumed by the frontier engine (production cache-warming pass in
    /// `resolve_external_type_from_loaded_files`) and integration tests.
    pub(crate) fn shallow_file_state(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        let resolved_canonical_id = self
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());

        // FileArtifactStore fast path (cache read only — no materialization to avoid recursion).
        let cached_facts = self
            .project_type_store
            .indexed()
            .get_any(resolved_canonical_id.as_str());
        if let Some(facts) = cached_facts {
            if facts.shallow_state.has_resolvable_surface() {
                return Some(facts.shallow_state.clone());
            }
        }

        self.route_owned_shallow_state(resolved_canonical_id.as_str())
    }

    /// Ensure the canonical post-parse artifact is materialized for a file.
    ///
    /// This is the single materialization bridge for the semantic DB layer.
    ///
    /// On cache hit, returns the cached `IndexedReady` without any I/O.
    /// On miss, reads the file, parses, builds analysis/snapshot/eval, constructs
    /// `ShallowFileState`, and publishes to `FileArtifactStore`.
    pub(crate) fn ensure_indexed_ready(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::project_type_store::IndexedReady>> {
        let normalized_canonical_id = self.normalized_analysis_canonical(canonical_id);
        let canonical_id = normalized_canonical_id.as_ref();

        // Fast path: check FileArtifactStore through the project-global cache.
        let cached = self.project_type_store.indexed().get_any(canonical_id);
        if let Some(indexed) = cached {
            // Staleness gate: the ambient-or-explicit store view governs hash
            // identity. Inside a request, an outdated entry is rejected and
            // we fall through to re-materialize. Outside a request this gate
            // is permissive.
            if self.store_view_allows_current_whole_hash(canonical_id, indexed.whole_hash) {
                component_meta_trace_custom!(
                    "ensure_indexed_ready_fast_hit",
                    format!("owner={} whole_hash={:?}", canonical_id, indexed.whole_hash),
                );
                return Some(indexed);
            }
            self.project_type_store.indexed().remove(canonical_id);
        }

        if canonical_id.is_empty() || is_raw_import_specifier_id(canonical_id) {
            return None;
        }

        let materialize = || -> Option<Arc<crate::project_type_store::IndexedReady>> {
            // Materialize: read source, build analysis, construct facts.
            //
            // Native: scheduler is the sole source authority. On a scheduler
            // miss, call `ensure_loaded` once to submit the canonical through
            // the scheduler — the canonical way to materialize a file. If
            // the scheduler still misses after `ensure_loaded`, return None
            // (file doesn't exist in the workspace).
            let (raw_source, cached_parse, whole_hash, snapshot) = {
                let state = match self.effective_file_state(canonical_id, None) {
                    Some(state) => state,
                    None => {
                        // On scheduler miss, call ensure_loaded once — the
                        // canonical way to materialize a file into the
                        // scheduler + current request view's extension store.
                        // Raw import specifiers and empty canonicals are
                        // never loadable.
                        if canonical_id.is_empty()
                            || is_raw_import_specifier_id(canonical_id)
                            || !self.ensure_loaded(canonical_id)
                        {
                            return None;
                        }
                        self.effective_file_state(canonical_id, None)?
                    }
                };
                if !self.store_view_allows_current_whole_hash(canonical_id, state.whole_hash) {
                    return None;
                }
                let snapshot =
                    if let Some(snapshot) = self.build_snapshot_from_scheduler(canonical_id) {
                        self.provenance
                            .indexed_ready_scheduler_snapshot_reuse
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        snapshot
                    } else {
                        self.build_snapshot_from_source_state(
                            canonical_id,
                            &state.source,
                            state.cached_parse.as_deref(),
                        )
                    };
                (
                    state.source,
                    state.cached_parse,
                    state.whole_hash,
                    Arc::new(snapshot),
                )
            };

            let eval_source = Arc::<str>::from(Self::build_eval_script_source(
                raw_source.as_ref(),
                cached_parse.as_deref(),
            ));
            let declaration_file = canonical_id.ends_with(".d.ts")
                || canonical_id.ends_with(".d.mts")
                || canonical_id.ends_with(".d.cts");

            // Canonicalize shallow import/reexport edges once during module-facts
            // materialization. Later resolver stages read these facts instead of
            // treating compile-cache/store-view import-route maps as truth.
            //
            // Seed import routes from DerivedRawState if present (set by
            // `set_import_dependencies` — D48 split: import_routes live on
            // DerivedRawState as a sub-mirror of IndexedReady.import_routes).
            // These are authoritative when the host caller has explicitly
            // provided resolution targets.
            let mut import_routes = rustc_hash::FxHashMap::default();
            {
                if let Some(cc) = self.derived_raw_cache().get(canonical_id) {
                    for (specifier, resolution) in cc.import_routes.iter() {
                        import_routes.insert(specifier.clone(), resolution.clone());
                    }
                }
            }
            let mut required_import_sources = snapshot
                .imports
                .iter()
                .map(|import| {
                    (
                        import.source.clone(),
                        // In declaration files (.d.ts), all imports are
                        // effectively type-only even without the `type`
                        // keyword. This ensures the TypeImport resolution
                        // path is used, which prefers .d.ts companions
                        // over .js runtime files.
                        if import.is_type_only || declaration_file {
                            verter_workspace::ResolveRequestKind::TypeImport
                        } else {
                            verter_workspace::ResolveRequestKind::EsmImport
                        },
                    )
                })
                .collect::<Vec<_>>();
            required_import_sources.extend(snapshot.export_signatures.iter().filter_map(
                |export| {
                    let source = export.reexport_source.clone()?;
                    let kind = if declaration_file || export.is_type {
                        verter_workspace::ResolveRequestKind::TypeImport
                    } else {
                        verter_workspace::ResolveRequestKind::EsmImport
                    };
                    Some((source, kind))
                },
            ));
            required_import_sources.sort_by(
                |(left_source, left_kind), (right_source, right_kind)| {
                    left_source.cmp(right_source).then_with(|| {
                        let kind_rank = |kind: verter_workspace::ResolveRequestKind| match kind {
                            verter_workspace::ResolveRequestKind::TypeImport => 0u8,
                            verter_workspace::ResolveRequestKind::EsmImport => 1u8,
                            verter_workspace::ResolveRequestKind::RequireCall => 2u8,
                            verter_workspace::ResolveRequestKind::SfcSrcAttr => 3u8,
                        };
                        kind_rank(*left_kind).cmp(&kind_rank(*right_kind))
                    })
                },
            );
            required_import_sources.dedup();

            let mut resolve_memo: rustc_hash::FxHashMap<
                (String, verter_workspace::ResolveRequestKind),
                Option<String>,
            > = rustc_hash::FxHashMap::default();
            let mut resolve_missing =
                |specifier: &str,
                 kind: verter_workspace::ResolveRequestKind,
                 prefer_live_fallback: bool| {
                    if import_routes.contains_key(specifier) {
                        return;
                    }
                    let primary = resolve_memo
                        .entry((specifier.to_string(), kind))
                        .or_insert_with(|| {
                            self.ws()
                                .resolve_import(
                                    canonical_id,
                                    specifier,
                                    verter_workspace::ResolutionContext {
                                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                                        kind,
                                    },
                                )
                                .map(|resolution| {
                                    if kind == verter_workspace::ResolveRequestKind::TypeImport {
                                        self.normalize_live_type_dependency_target(
                                            canonical_id,
                                            specifier,
                                            resolution.source_id.as_str(),
                                        )
                                    } else {
                                        resolution.source_id
                                    }
                                })
                        })
                        .clone();
                    let resolved: Option<String> = if kind
                        == verter_workspace::ResolveRequestKind::TypeImport
                    {
                        primary
                            .or_else(|| {
                                self.fallback_relative_type_companion(canonical_id, specifier)
                            })
                            .or_else(|| {
                                if !prefer_live_fallback {
                                    return None;
                                }
                                self.ws()
                                    .resolve_import(
                                        canonical_id,
                                        specifier,
                                        verter_workspace::ResolutionContext {
                                            phase: verter_workspace::ResolvePhase::CodegenBlocker,
                                            kind: verter_workspace::ResolveRequestKind::EsmImport,
                                        },
                                    )
                                    .map(|resolution| resolution.source_id)
                            })
                    } else {
                        primary
                    };
                    let mut resolution = DependencyResolution {
                        specifier: specifier.to_string(),
                        resolved_canonical_id: None,
                        possible_canonical_ids: Vec::new(),
                    };
                    if let Some(resolved) = resolved {
                        resolution.resolved_canonical_id = Some(resolved.clone());
                        resolution.possible_canonical_ids.push(resolved);
                    }
                    import_routes.insert(specifier.to_string(), resolution);
                };

            for (source, kind) in &required_import_sources {
                resolve_missing(source, *kind, true);
            }

            let external_type_analysis = self.build_external_type_analysis(
                canonical_id,
                whole_hash,
                raw_source.as_ref(),
                cached_parse.as_deref(),
                &eval_source,
            );

            let import_route_hash = (!import_routes.is_empty())
                .then(|| crate::resolver_store::hash_import_route_targets(&import_routes));
            let dep_edges = dep_edges_from_resolutions(&import_routes);
            let resolver = HostShallowImportResolver {
                dep_edges: &dep_edges,
            };
            // Synthesise the implicit Vue SFC `default` value symbol
            // from type-based macros — see `vue_default_synth` for
            // the policy and rationale.
            let mut shallow_state_inner =
                crate::resolver_core::ShallowFileState::from_analysis_with_resolver(
                    whole_hash,
                    Arc::clone(&external_type_analysis),
                    Some(eval_source.as_ref()),
                    None,
                    &resolver,
                );
            crate::resolver_core::vue_default_synth::inject_vue_default_into_shallow_state(
                canonical_id,
                &mut shallow_state_inner,
                &snapshot.macros,
            );
            let shallow_state = Arc::new(shallow_state_inner);

            // Prefer the scheduler's file state for script_analysis (it may have
            // richer compilation context), but fall back to the snapshot's data
            // for workspace-only files that are not in the scheduler.
            let script_analysis = self
                .effective_file_state(canonical_id, None)
                .filter(|state| state.whole_hash == whole_hash)
                .map(|state| Arc::new(state.script_analysis))
                .or_else(|| {
                    Some(Arc::new(
                        verter_semantic::analysis::ScriptAnalysisSnapshot {
                            imports: snapshot.imports.clone(),
                            module_references: snapshot.module_references.as_ref().clone(),
                            bindings: snapshot.bindings.clone(),
                            macros: snapshot.macros.as_ref().clone(),
                            macro_type_deps: snapshot.macro_type_deps.as_ref().clone(),
                            flags: verter_semantic::analysis::AnalysisFlags::from_bits_truncate(
                                snapshot.script_flags,
                            ),
                            ..Default::default()
                        },
                    ))
                });
            let export_signatures = Some(Arc::clone(&snapshot.export_signatures));

            let import_routes = Arc::new(import_routes);

            // Step 8 / F5: cache the route-surface hash on IndexedReady
            // symmetric to import_route_hash. Populated only when the
            // shallow state has a resolvable surface (matching
            // host_resolve.rs:575's existing pattern). Invalidation
            // lifecycle is identical to IndexedReady's content-hash
            // lifecycle — when canonical's whole_hash changes, a fresh
            // IndexedReady is built and route_hash is recomputed.
            // `current_derived_fact_hash` (meta_resolve.rs) reads this
            // cached hash instead of rehashing per call.
            let route_hash = shallow_state
                .has_resolvable_surface()
                .then(|| crate::resolver_store::hash_route_surface(shallow_state.as_ref()));

            // Publish the canonical post-parse artifact into FileArtifactStore.
            // This is the single authoritative cache consumers read from.
            let indexed = Arc::new(crate::project_type_store::IndexedReady {
                whole_hash,
                shallow_state: Arc::clone(&shallow_state),
                import_routes: Arc::clone(&import_routes),
                import_route_hash,
                route_hash,
                raw_source: Arc::clone(&raw_source),
                eval_source: Arc::clone(&eval_source),
                cached_parse,
                script_analysis,
                export_signatures,
                snapshot,
                external_type_analysis: Arc::clone(&external_type_analysis),
            });
            self.project_type_store
                .indexed()
                .insert(Arc::from(canonical_id), Arc::clone(&indexed));

            Some(indexed)
        };

        // Collapse concurrent cold loads for the same canonical file through
        // the dedicated singleflight group on the resolver runtime.
        let singleflight = &self.resolver.runtime.indexed_singleflight;
        let token = crate::resolver_core::StoreViewCompatToken {
            epoch: 0,
            session: None,
        };
        match singleflight.run(canonical_id.to_owned(), token, || {
            // Re-check cache inside the flight — another thread may have
            // populated it after we dropped the first probe.
            if let Some(indexed) = self.project_type_store.indexed().get_any(canonical_id) {
                return Ok(indexed);
            }
            materialize().ok_or(())
        }) {
            Ok(run_result) => Some((*run_result.value).clone()),
            Err(()) => None,
        }
    }

    /// O(1) lookup of an `AnalyzedMacro` from the
    /// sidecar by its stable index in
    /// `ScriptAnalysisSnapshot.macros`. Returns `None` when the file
    /// is not indexed, has no script analysis, or `macro_index` is
    /// out of range.
    ///
    /// Reads cache-only via `ensure_indexed_ready` — no AST re-walk.
    /// Used by `build_resolve_macro_payload` to consult the analysed
    /// macro for emit field walks (`DefineEmits`) and model name
    /// extraction (`DefineModel`).
    ///
    /// Returns an `Arc<ScriptAnalysisSnapshot>` so the caller holds
    /// the snapshot alive while it reads the macro entry. The macro
    /// is accessed via `snapshot.macros.get(macro_index)`.
    pub(crate) fn analyzed_macro_snapshot(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_semantic::analysis::ScriptAnalysisSnapshot>> {
        let indexed = self.ensure_indexed_ready(canonical_id)?;
        indexed.script_analysis.clone()
    }

    pub(crate) fn resolve_external_type_from_indexed_ready(
        &self,
        dep_canonical: &str,
        type_name: &str,
        imported_companions: &rustc_hash::FxHashMap<
            String,
            verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements,
        >,
    ) -> Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements> {
        component_meta_trace_custom!(
            "resolve_external_type_from_indexed_ready",
            format!(
                "owner={} type={} store_view={}",
                dep_canonical, type_name, false
            ),
        );
        let inputs = self.external_type_resolution_inputs(dep_canonical)?;
        let normalized_canonical_id = self.normalized_analysis_canonical(dep_canonical);
        let canonical_id_for_source_type = normalized_canonical_id.as_ref();
        let source_type = self.imported_eval_source_type_for(
            canonical_id_for_source_type,
            inputs.raw_source.as_ref(),
            inputs.cached_parse.as_deref(),
        );
        let Some(type_context) = self.cached_type_resolution_context_entry(
            canonical_id_for_source_type,
            inputs.whole_hash,
            &inputs.eval_source,
            source_type,
        ) else {
            component_meta_trace_custom!(
                "resolve_external_type_from_indexed_ready_result",
                format!(
                    "owner={} type={} hit=false local_symbol_target={} parse_failed_or_missing_type_context=true",
                    dep_canonical,
                    type_name,
                    inputs.analysis.has_local_symbol_target(type_name),
                ),
            );
            return None;
        };
        let program = type_context.borrow_owner().borrow_dependent();
        let base_ctx = type_context.borrow_dependent();
        let resolved = verter_compiler::utils::oxc::vue::resolve_type::resolve_external_type_in_context_with_analyzed_symbol_companion_and_canonical(
            type_name,
            program,
            type_context.borrow_owner().source_bytes(),
            base_ctx,
            inputs.analysis.as_ref(),
            imported_companions,
            dep_canonical,
        );
        component_meta_trace_custom!(
            "resolve_external_type_from_indexed_ready_result",
            format!(
                "owner={} type={} hit={} local_symbol_target={} parse_failed=false",
                dep_canonical,
                type_name,
                resolved.is_some(),
                inputs.analysis.has_local_symbol_target(type_name),
            ),
        );
        resolved
    }

    pub(crate) fn resolve_direct_type_reexport_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        component_meta_trace_custom!(
            "resolve_direct_type_reexport_target",
            format!("owner={} requested={}", dep_canonical, requested_name),
        );
        let shallow = self.shallow_file_state(dep_canonical)?;
        let crate::resolver_core::ExportTarget::Reexport {
            source_specifier,
            original_name,
            canonical_id,
            ..
        } = shallow.export_target(requested_name)?
        else {
            return None;
        };
        let next_canonical = if canonical_id.is_empty() {
            self.resolve_route_type_edge(dep_canonical, source_specifier)?
        } else {
            canonical_id.clone()
        };
        component_meta_trace_custom!(
            "resolve_direct_type_reexport_target_result",
            format!(
                "owner={} requested={} import_source={} target={} exported={}",
                dep_canonical, requested_name, source_specifier, next_canonical, original_name
            ),
        );
        Some((next_canonical, original_name.clone()))
    }

    pub(crate) fn current_or_read_whole_hash(&self, canonical_id: &str) -> Option<Hash16> {
        // Post-cut: live-host probe. Resolvers that need to load a canonical
        // mid-resolution must call `ensure_loaded` explicitly; only the
        // top-level / test-scaffold path auto-loads on miss.
        if let Some(hash) = self.get_whole_hash(canonical_id) {
            return Some(hash);
        }
        if canonical_id.is_empty() || is_raw_import_specifier_id(canonical_id) {
            return None;
        }
        if self.ensure_loaded(canonical_id) {
            return self.get_whole_hash(canonical_id);
        }
        None
    }

    pub(crate) fn cached_import_route_resolution(
        &self,
        canonical_id: &str,
        import_source: &str,
    ) -> Option<DependencyResolution> {
        // The project-global cache already validates entries through
        // `HostFenceValidator` at publish time, so readers consume the
        // cache permissively here.
        // import_routes lives on DerivedRawState (D48 split).
        if self.is_canonical_evicted(canonical_id) {
            return None;
        }
        let derived = self.derived_raw_cache().get(canonical_id)?;
        derived.import_routes.get(import_source).cloned()
    }

    fn append_file_whole_and_route_fact_versions(
        &self,
        canonical_id: &str,
        known_shallow: Option<&crate::resolver_core::ShallowFileState>,
        facts: &mut Vec<crate::resolver_core::FactVersionRef>,
        seen: &mut rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef>,
    ) {
        // Ambient-view-first hash chain. `current_or_read_whole_hash`
        // already does `ensure_loaded` on view-miss inside a request, so the
        // only remaining fallback is the caller-provided `known_shallow`
        // hash (avoids a redundant ensure_loaded round-trip when the caller
        // already has shallow state in hand).
        let whole_hash = self
            .current_or_read_whole_hash(canonical_id)
            .or_else(|| known_shallow.map(|state| state.whole_hash));
        if let Some(hash) = whole_hash {
            let fact = crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: canonical_id.to_string(),
                hash,
            };
            if seen.insert(fact.clone()) {
                facts.push(fact);
            }
        }

        // Post-cut: live-host probe. Prefer the caller-supplied shallow state,
        // then fall back to the route-owned shallow cache. The ambient
        // request view no longer exists.
        let route_hash = known_shallow
            .filter(|state| state.has_resolvable_surface())
            .map(crate::resolver_store::hash_route_surface)
            .or_else(|| {
                self.route_owned_shallow_state(canonical_id)
                    .filter(|state| state.has_resolvable_surface())
                    .map(|state| crate::resolver_store::hash_route_surface(&state))
            });
        if let Some(hash) = route_hash {
            let fact = crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: canonical_id.to_string(),
                kind: crate::resolver_core::DerivedFactKind::Route,
                hash,
            };
            if seen.insert(fact.clone()) {
                facts.push(fact);
            }
        }
    }

    pub(in crate::host_manage) fn resolve_direct_imported_type_root_fast_path(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> Option<((String, String), Vec<crate::resolver_core::FactVersionRef>)> {
        let shallow = self.route_owned_shallow_state(dep_canonical)?;
        let (target_canonical, target_symbol) = match shallow.export_target(imported_name)? {
            crate::resolver_core::ExportTarget::Reexport {
                source_specifier,
                original_name,
                canonical_id,
                ..
            } => {
                let next_canonical = if canonical_id.is_empty() {
                    self.resolve_route_type_edge(dep_canonical, source_specifier)?
                } else {
                    canonical_id.clone()
                };
                (next_canonical, original_name.clone())
            }
            crate::resolver_core::ExportTarget::Local { symbol_name } => {
                let import_target = shallow.import_target(symbol_name.as_str())?;
                let next_canonical = if import_target.canonical_id.is_empty() {
                    self.resolve_route_type_edge(
                        dep_canonical,
                        import_target.source_specifier.as_str(),
                    )?
                } else {
                    import_target.canonical_id.clone()
                };
                (next_canonical, import_target.imported_name.clone())
            }
        };
        let normalized_target = self
            .resolve_eval_dependency_canonical(target_canonical.as_str())
            .unwrap_or(target_canonical);
        let (leaf_symbol, target_hash) = {
            let target_state = self.route_owned_shallow_state(normalized_target.as_str())?;
            match target_state.export_target(target_symbol.as_str())? {
                crate::resolver_core::ExportTarget::Local { symbol_name }
                    if target_state.import_target(symbol_name.as_str()).is_none() =>
                {
                    (symbol_name.clone(), target_state.whole_hash)
                }
                _ => return None,
            }
        };

        let mut facts = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        self.append_file_whole_and_route_fact_versions(
            dep_canonical,
            Some(shallow.as_ref()),
            &mut facts,
            &mut seen,
        );
        let target_fact = crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: normalized_target.clone(),
            hash: target_hash,
        };
        if seen.insert(target_fact.clone()) {
            facts.push(target_fact);
        }

        Some(((normalized_target, leaf_symbol), facts))
    }

    pub(crate) fn resolve_local_import_symbol_target(
        &self,
        dep_canonical: &str,
        resolved_name: &str,
    ) -> Option<(String, String)> {
        component_meta_trace_custom!(
            "resolve_local_import_symbol_target",
            format!("owner={} requested={}", dep_canonical, resolved_name),
        );
        let shallow = self.shallow_file_state(dep_canonical)?;
        let import_target = shallow.import_target(resolved_name)?;
        let next_canonical = if import_target.canonical_id.is_empty() {
            self.resolve_route_type_edge(dep_canonical, &import_target.source_specifier)?
        } else {
            import_target.canonical_id.clone()
        };
        component_meta_trace_custom!(
            "resolve_local_import_symbol_target_result",
            format!(
                "owner={} requested={} import_source={} target={} exported={}",
                dep_canonical,
                resolved_name,
                import_target.source_specifier,
                next_canonical,
                import_target.imported_name
            ),
        );
        Some((next_canonical, import_target.imported_name.clone()))
    }

    pub(crate) fn resolve_local_export_symbol_target(
        &self,
        canonical_source: &str,
        exported_name: &str,
    ) -> Option<String> {
        component_meta_trace_custom!(
            "resolve_local_export_symbol_target",
            format!("owner={} requested={}", canonical_source, exported_name),
        );
        let analysis = self.external_type_analysis(canonical_source)?;
        let target = analysis
            .local_export_symbol_target(exported_name)
            .map(str::to_string);
        if let Some(target) = target.as_deref() {
            component_meta_trace_custom!(
                "resolve_local_export_symbol_target_result",
                format!(
                    "owner={} requested={} target={}",
                    canonical_source, exported_name, target
                ),
            );
        }
        target
    }

    pub(crate) fn resolve_imported_type_root(
        &self,
        dep_canonical: &str,
        imported_name: &str,
    ) -> (String, String) {
        let audit_started = self.config.audit_enabled.then(Instant::now);

        let normalized_canonical = self
            .resolve_eval_dependency_canonical(dep_canonical)
            .unwrap_or_else(|| dep_canonical.to_string());
        let live_view = self.resolver_store_view();

        let cached_root = self
            .resolver
            .runtime
            .imported_roots
            .get_or_resolve_with_facts(
                normalized_canonical.as_str(),
                imported_name,
                &live_view,
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
        let (resolved, source_kind) = match cached_root {
            Some(cached) => match cached.as_tuple() {
                Some(tuple) => (tuple, "named_export_target"),
                None => (
                    (normalized_canonical.clone(), imported_name.to_string()),
                    "miss",
                ),
            },
            None => (
                (normalized_canonical.clone(), imported_name.to_string()),
                "miss",
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

        resolved
    }

    /// Get-or-build the [`OwnerImportSurface`](crate::owner_import_surface::OwnerImportSurface)
    /// for `owner_canonical`. of the project-global cache overhaul:
    /// direct owner imports resolve exactly once per owner version and every
    /// downstream stage reads the same surface entry.
    ///
    /// Cache identity is `(owner_canonical, owner_whole_hash)`. Stale owner
    /// versions miss at the key level; building populates
    /// `project_type_store().owner_import_surfaces()` with the fully-resolved
    /// root for each direct import binding in the owner file.
    pub(crate) fn owner_import_surface(
        &self,
        owner_canonical: &str,
    ) -> Option<Arc<crate::owner_import_surface::OwnerImportSurface>> {
        let shallow = self.shallow_file_state(owner_canonical)?;
        let whole_hash = shallow.whole_hash;
        let surfaces = self.project_type_store.owner_import_surfaces();
        if let Some(cached) = surfaces.get(owner_canonical, whole_hash) {
            return Some(cached);
        }

        component_meta_trace_custom!(
            "owner_import_surface_build",
            format!("owner={}", owner_canonical),
        );

        // (local_name, final_canonical, final_exported_name, target_whole_hash)
        type SurfaceBuildEntry = (Arc<str>, Arc<str>, Arc<str>, Option<Hash16>);
        let mut entries: Vec<SurfaceBuildEntry> = Vec::with_capacity(shallow.import_targets.len());
        for (local_name, target) in shallow.import_targets.iter() {
            let resolved_canonical_id = if target.canonical_id.is_empty() {
                match self
                    .resolve_type_dependency_canonical(owner_canonical, &target.source_specifier)
                {
                    Some(canonical) => canonical,
                    None => continue,
                }
            } else {
                target.canonical_id.clone()
            };

            let (final_canonical, final_name) = self.resolve_imported_type_root(
                resolved_canonical_id.as_str(),
                target.imported_name.as_str(),
            );

            let target_hash = self
                .shallow_file_state(final_canonical.as_str())
                .map(|s| s.whole_hash);

            entries.push((
                Arc::from(local_name.as_str()),
                Arc::from(final_canonical),
                Arc::from(final_name),
                target_hash,
            ));
        }

        let surface = crate::owner_import_surface::build_owner_import_surface(
            Arc::from(owner_canonical),
            whole_hash,
            entries,
        );
        surfaces.insert(Arc::from(owner_canonical), Arc::clone(&surface));
        Some(surface)
    }

    /// Resolve a direct owner import binding to its final root identity via
    /// the owner import surface. Returns `(final_canonical,
    /// final_exported_name)` matching the legacy
    /// [`Self::resolve_imported_type_root`] contract for direct
    /// owner imports, but sourced from one cached surface per owner version.
    ///
    /// Callers that already have the owner canonical plus a local binding
    /// name must prefer this method over `resolve_imported_type_root`
    /// so direct owner imports resolve exactly once per owner version. The
    /// `resolve_imported_type_root` helper remains the authority for
    /// transitive chain walks inside route/barrel code.
    pub(crate) fn resolve_owner_direct_import(
        &self,
        owner_canonical: &str,
        local_name: &str,
    ) -> Option<(String, String)> {
        let surface = self.owner_import_surface(owner_canonical)?;
        // `Arc<str>` borrows as `&str`, so the surface lookup uses the
        // caller-supplied slice directly without allocating a fresh Arc.
        let binding = surface.bindings.get(local_name)?;
        Some((
            binding.canonical_id.as_ref().to_string(),
            binding.exported_name.as_ref().to_string(),
        ))
    }
}
