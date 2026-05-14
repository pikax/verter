//! `impl VerterHost` — frontier closure, materialisation, and named-type
//! export route resolution.
//!
//! Owns the BFS / route-resolution layer that sits below
//! `resolve_external_type_from_loaded_files`:
//! - `run_external_type_frontier_closure` drives layered frontier
//!   discovery via [`HostFrontierAdapter`].
//! - `collect_frontier_companion_seeds` /
//!   `materialize_frontier_resolved_type` /
//!   `materialize_frontier_resolved_type_with_memo` walk the resolved
//!   frontier and project the companion shape.
//! - `planned_frontier_companions` caches per-`(canonical, exported, route)`
//!   companion plans on the request-scoped
//!   [`FrontierCompanionPlans`].
//! - `append_route_participant_fact_versions` fans the touched canonical
//!   set into `FactVersionRef` entries for cache-fence accounting.
//! - `resolve_route_type_edge` drives the live-host shallow + workspace
//!   resolver chain for a single route hop.
//! - The route-only named-type export resolver
//!   (`resolve_named_type_export_route_from_target` /
//!   `resolve_named_type_export_route_uncached`) — the single non-trivial
//!   intra-file SCC identified by the Tier 0 audit.
//! - `route_shallow_state` / `route_owned_shallow_state` — request-scoped
//!   readers that delegate to
//!   [`Self::ensure_route_owned_shallow_entry`] (defined in the
//!   `route_owned_shallow` sub-module).
//! - `build_named_type_export_route_entry` /
//!   `resolve_named_type_export_target_uncached` /
//!   `resolve_named_type_export_target_shallow` — host-level binding into
//!   the route-DB cooperative resolve.

use std::cell::RefCell;
use std::sync::Arc;

use super::frontier_adapter::HostFrontierAdapter;
use super::frontier_helpers::{
    external_type_debug, external_type_debug_enabled, external_type_frontier_layer_result_detail,
    external_type_frontier_layer_start_detail, ordered_wildcard_indices_for_exported_name,
    FrontierCompanionPlans, FrontierRequestedRoutes, PlannedFrontierCompanion,
    ResolvedExternalTypes, RouteShallowStateCache,
};
use super::test_guards::assert_route_frontier_allowed;
use crate::host_manage::component_meta_trace_custom;
use crate::VerterHost;
use verter_compiler::utils::oxc::vue::resolve_type::{
    imported_member_name_for_required_alias, required_import_alias_names_for_binding,
};

impl VerterHost {
    /// Base wrapper that fixes `view = None`. Test-only — production paths
    /// flow through the view-aware variant.
    #[cfg(test)]
    #[allow(clippy::type_complexity)]
    pub(super) fn run_external_type_frontier_closure(
        &self,
        dep_canonical: &str,
        type_name: &str,
        requested_routes: &mut FrontierRequestedRoutes,
        companion_plans: &mut FrontierCompanionPlans,
    ) -> Result<
        (
            crate::resolver_core::ExternalTypeFrontier,
            Option<(String, String)>,
            bool,
        ),
        crate::types::ExternalTypeResolveError,
    > {
        self.run_external_type_frontier_closure_with_view(
            dep_canonical,
            type_name,
            requested_routes,
            companion_plans,
            None,
        )
    }

    /// View-aware variant of run_external_type_frontier_closure.
    ///
    /// Constructs the [`HostFrontierAdapter`] with `view` plumbed in so the
    /// frontier reads shallow state through the session overlay when an
    /// overlay candidate is published.
    #[allow(clippy::type_complexity)]
    pub(super) fn run_external_type_frontier_closure_with_view(
        &self,
        dep_canonical: &str,
        type_name: &str,
        requested_routes: &mut FrontierRequestedRoutes,
        companion_plans: &mut FrontierCompanionPlans,
        view: Option<&dyn crate::session_view::SessionView>,
    ) -> Result<
        (
            crate::resolver_core::ExternalTypeFrontier,
            Option<(String, String)>,
            bool,
        ),
        crate::types::ExternalTypeResolveError,
    > {
        assert_route_frontier_allowed();
        let adapter = HostFrontierAdapter {
            host: self,
            materialize_symbols: false,
            // Frontier discovery stays route-only. Materialization resolves only
            // the demanded companion targets after the route is known.
            route_exports_only: true,
            view,
            route_shallow_cache: RefCell::new(RouteShallowStateCache::default()),
        };
        let mut frontier = crate::resolver_core::ExternalTypeFrontier::new();
        let mut inspected_symbols = rustc_hash::FxHashSet::default();
        frontier.seed(std::iter::once(
            crate::resolver_core::PendingExternalSymbol {
                canonical_id: dep_canonical.to_string(),
                exported_name: type_name.to_string(),
                route: Some(
                    requested_routes
                        .get(&(dep_canonical.to_string(), type_name.to_string()))
                        .cloned()
                        .unwrap_or_default(),
                ),
            },
        ));

        let mut frontier_layer = 0usize;
        loop {
            let (target, had_route_cycle) = loop {
                frontier_layer += 1;
                component_meta_trace_custom!(
                    "external_type_frontier_layer_start",
                    external_type_frontier_layer_start_detail(
                        dep_canonical,
                        type_name,
                        frontier_layer,
                        frontier.pending_count(),
                        frontier.resolved_count(),
                    ),
                );
                let has_more = frontier.run_one_level(&adapter).map_err(|failure| {
                    crate::types::ExternalTypeResolveError::StepLimitExceeded {
                        limit: failure.limit,
                        type_name: type_name.to_string(),
                        last_dep: failure.context,
                    }
                })?;
                let (target, had_route_cycle) =
                    frontier.final_target_for_with_cycle(&adapter, dep_canonical, type_name);
                component_meta_trace_custom!(
                    "external_type_frontier_layer_result",
                    external_type_frontier_layer_result_detail(
                        dep_canonical,
                        type_name,
                        frontier_layer,
                        frontier.pending_count(),
                        frontier.resolved_count(),
                        has_more,
                        target.is_some(),
                        had_route_cycle,
                    ),
                );
                if target.is_some() || !has_more {
                    break (target, had_route_cycle);
                }
            };
            if target.is_none() {
                return Ok((frontier, None, had_route_cycle));
            }

            frontier.clear_pending();

            let companion_seeds = self.collect_frontier_companion_seeds(
                &frontier,
                &adapter,
                &mut inspected_symbols,
                requested_routes,
                companion_plans,
            );
            if crate::host_manage::component_meta_debug_enabled() {
                crate::host_manage::component_meta_debug(format!(
                    "frontier_closure source={} exported={} resolved={} new_companions={}",
                    dep_canonical,
                    type_name,
                    frontier.resolved_count(),
                    companion_seeds.len(),
                ));
            }
            if companion_seeds.is_empty() {
                return Ok((frontier, target, had_route_cycle));
            }

            for seed in &companion_seeds {
                let seed_route = seed.route.clone().unwrap_or_default();
                requested_routes
                    .entry((seed.canonical_id.clone(), seed.exported_name.clone()))
                    .and_modify(|existing| {
                        *existing =
                            crate::resolver_core::merge_route_demands(existing, &seed_route);
                    })
                    .or_insert(seed_route);
            }
            frontier.seed(companion_seeds);
        }
    }

    pub(crate) fn collect_frontier_companion_seeds(
        &self,
        frontier: &crate::resolver_core::ExternalTypeFrontier,
        adapter: &HostFrontierAdapter<'_>,
        inspected_symbols: &mut rustc_hash::FxHashSet<(String, String)>,
        requested_routes: &mut FrontierRequestedRoutes,
        companion_plans: &mut FrontierCompanionPlans,
    ) -> Vec<crate::resolver_core::PendingExternalSymbol> {
        let mut seeds = Vec::new();
        let requested_symbols: Vec<_> = requested_routes
            .iter()
            .map(|((canonical_id, exported_name), route)| {
                (canonical_id.clone(), exported_name.clone(), route.clone())
            })
            .collect();

        for (requested_canonical_id, requested_exported_name, requested_route) in requested_symbols
        {
            let Some((canonical_id, exported_name)) = frontier.final_target_for(
                adapter,
                &requested_canonical_id,
                &requested_exported_name,
            ) else {
                continue;
            };
            requested_routes
                .entry((canonical_id.clone(), exported_name.clone()))
                .and_modify(|existing| {
                    *existing =
                        crate::resolver_core::merge_route_demands(existing, &requested_route);
                })
                .or_insert_with(|| requested_route.clone());
            if !inspected_symbols.insert((canonical_id.clone(), exported_name.clone())) {
                continue;
            }

            let planned_companions = self.planned_frontier_companions(
                &canonical_id,
                &exported_name,
                &requested_route,
                companion_plans,
            );
            for companion in planned_companions.iter() {
                let (target_canonical, target_name) = frontier
                    .final_target_for(
                        adapter,
                        &companion.resolved_canonical,
                        &companion.resolved_exported_name,
                    )
                    .unwrap_or((
                        companion.resolved_canonical.clone(),
                        companion.resolved_exported_name.clone(),
                    ));
                seeds.push(crate::resolver_core::PendingExternalSymbol {
                    canonical_id: target_canonical,
                    exported_name: target_name,
                    route: Some(companion.route.clone()),
                });
            }
        }

        seeds
    }

    /// View-aware materializer for resolved frontier elements.
    ///
    /// Constructs the [`HostFrontierAdapter`] with `view`, and passes `view`
    /// down into [`Self::materialize_frontier_resolved_type_with_memo`] so
    /// the indexed-ready fall-through reads the overlay candidate.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn materialize_frontier_resolved_type_with_view(
        &self,
        frontier: &crate::resolver_core::ExternalTypeFrontier,
        requested_routes: &FrontierRequestedRoutes,
        companion_plans: &mut FrontierCompanionPlans,
        canonical_id: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        view: Option<&dyn crate::session_view::SessionView>,
    ) -> Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements> {
        let adapter = HostFrontierAdapter {
            host: self,
            // Frontier routing is already complete before materialization starts.
            // Keep final-target checks on the same shallow/export-owned path so
            // package declaration files do not reopen full imported-state
            // materialization while companion targets are selected.
            materialize_symbols: false,
            route_exports_only: true,
            view,
            route_shallow_cache: RefCell::new(RouteShallowStateCache::default()),
        };
        let mut memo = rustc_hash::FxHashMap::default();
        let mut active = rustc_hash::FxHashSet::default();
        self.materialize_frontier_resolved_type_with_memo(
            frontier,
            requested_routes,
            companion_plans,
            &adapter,
            canonical_id,
            exported_name,
            tracked_deps,
            resolution_deps,
            &mut memo,
            &mut active,
            view,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn materialize_frontier_resolved_type_with_memo(
        &self,
        frontier: &crate::resolver_core::ExternalTypeFrontier,
        requested_routes: &FrontierRequestedRoutes,
        companion_plans: &mut FrontierCompanionPlans,
        adapter: &HostFrontierAdapter<'_>,
        canonical_id: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        memo: &mut rustc_hash::FxHashMap<
            (String, String),
            Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>,
        >,
        active: &mut rustc_hash::FxHashSet<(String, String)>,
        view: Option<&dyn crate::session_view::SessionView>,
    ) -> Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements> {
        let cache_key = (canonical_id.to_string(), exported_name.to_string());
        if let Some(cached) = memo.get(&cache_key) {
            return cached.clone();
        }
        if !active.insert(cache_key.clone()) {
            self.provenance
                .resolver_cycle_detections
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return None;
        }

        tracked_deps.insert(canonical_id.to_string());
        resolution_deps.insert(canonical_id.to_string());

        let resolved = {
            let route = requested_routes
                .get(&(canonical_id.to_string(), exported_name.to_string()))
                .cloned()
                .unwrap_or_default();
            let planned_companions = self.planned_frontier_companions(
                canonical_id,
                exported_name,
                &route,
                companion_plans,
            );
            let mut companion_types = ResolvedExternalTypes::default();
            for companion in planned_companions.iter() {
                let (target_canonical, target_name) = frontier
                    .final_target_for(
                        adapter,
                        &companion.resolved_canonical,
                        &companion.resolved_exported_name,
                    )
                    .unwrap_or((
                        companion.resolved_canonical.clone(),
                        companion.resolved_exported_name.clone(),
                    ));
                if frontier
                    .get_resolved(&target_canonical, &target_name)
                    .is_none()
                {
                    continue;
                }
                if let Some(resolved_companion) = self.materialize_frontier_resolved_type_with_memo(
                    frontier,
                    requested_routes,
                    companion_plans,
                    adapter,
                    &target_canonical,
                    &target_name,
                    tracked_deps,
                    resolution_deps,
                    memo,
                    active,
                    view,
                ) {
                    tracked_deps.insert(target_canonical.clone());
                    resolution_deps.insert(target_canonical.clone());
                    if external_type_debug_enabled() {
                        external_type_debug(format!(
                            "frontier_materialize companion owner={} exported={} alias={} target={}:{} cached_member_count={}",
                            canonical_id,
                            exported_name,
                            companion.alias,
                            target_canonical,
                            target_name,
                            resolved_companion.props.len(),
                        ));
                    }
                    companion_types
                        .entry(companion.alias.clone())
                        .or_insert(resolved_companion);
                }
            }

            self.resolve_external_type_from_indexed_ready_with_view(
                canonical_id,
                exported_name,
                &companion_types,
                view,
            )
        };

        active.remove(&cache_key);
        memo.insert(cache_key, resolved.clone());
        resolved
    }

    fn planned_frontier_companions(
        &self,
        canonical_id: &str,
        exported_name: &str,
        route: &crate::resolver_core::RouteDemand,
        companion_plans: &mut FrontierCompanionPlans,
    ) -> Arc<[PlannedFrontierCompanion]> {
        companion_plans.get_or_compute(canonical_id, exported_name, route, || {
            let Some(analysis) = self.external_type_analysis(canonical_id) else {
                return Vec::new();
            };
            let required_import_routes =
                self.required_import_routes_for_exported_route(canonical_id, exported_name, route);
            let required_import_names = required_import_routes
                .keys()
                .cloned()
                .collect::<rustc_hash::FxHashSet<_>>();
            let mut attempted_requests = rustc_hash::FxHashSet::default();
            let mut planned = Vec::new();

            for binding in &analysis.extracted.bindings {
                let required_aliases =
                    required_import_alias_names_for_binding(binding, &required_import_names);
                for required_alias in required_aliases {
                    let Some(imported_name) =
                        imported_member_name_for_required_alias(binding, &required_alias)
                    else {
                        continue;
                    };
                    let request_key = (
                        required_alias.clone(),
                        binding.source.clone(),
                        imported_name.clone(),
                    );
                    if !attempted_requests.insert(request_key) {
                        continue;
                    }

                    let Some(dep_canonical) =
                        self.resolve_type_dependency_canonical(canonical_id, &binding.source)
                    else {
                        continue;
                    };
                    let (resolved_canonical, resolved_name) = self
                        .resolve_imported_type_root(dep_canonical.as_str(), imported_name.as_str());
                    planned.push(PlannedFrontierCompanion {
                        alias: required_alias.clone(),
                        resolved_canonical,
                        resolved_exported_name: resolved_name,
                        route: required_import_routes
                            .get(&required_alias)
                            .cloned()
                            .unwrap_or_default(),
                    });
                }
            }

            planned
        })
    }

    fn append_route_participant_fact_versions(
        &self,
        canonical: &str,
        facts: &mut Vec<crate::resolver_core::FactVersionRef>,
        seen: &mut rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef>,
        route_shallow_cache: Option<&RouteShallowStateCache>,
    ) {
        if let Some(hash) = self.current_or_read_whole_hash(canonical) {
            let fact = crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: canonical.to_string(),
                hash,
            };
            if seen.insert(fact.clone()) {
                facts.push(fact);
            }
        }

        let route_hash = {
            let normalized_canonical = self
                .resolve_eval_dependency_canonical(canonical)
                .unwrap_or_else(|| canonical.to_string());
            route_shallow_cache
                .and_then(|cache| cache.get(normalized_canonical.as_str()))
                .filter(|state| state.as_ref().has_resolvable_surface())
                .map(|state| crate::resolver_store::hash_route_surface(state.as_ref()))
                .or_else(|| {
                    self.shallow_file_state(canonical)
                        .filter(|state| state.has_resolvable_surface())
                        .map(|state| crate::resolver_store::hash_route_surface(&state))
                })
        };
        if let Some(hash) = route_hash {
            let fact = crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: canonical.to_string(),
                kind: crate::resolver_core::DerivedFactKind::Route,
                hash,
            };
            if seen.insert(fact.clone()) {
                facts.push(fact);
            }
        }
    }

    pub(crate) fn resolve_route_type_edge(
        &self,
        owner_canonical: &str,
        source_specifier: &str,
    ) -> Option<String> {
        let resolved = self
            .ws()
            .resolve_import(
                owner_canonical,
                source_specifier,
                verter_workspace::ResolutionContext {
                    phase: verter_workspace::ResolvePhase::CodegenBlocker,
                    kind: verter_workspace::ResolveRequestKind::TypeImport,
                },
            )
            .map(|resolution| {
                self.normalize_live_type_dependency_target(
                    owner_canonical,
                    source_specifier,
                    resolution.source_id.as_str(),
                )
            })
            .or_else(|| self.fallback_relative_type_companion(owner_canonical, source_specifier))
            .or_else(|| {
                self.ws()
                    .resolve_import(
                        owner_canonical,
                        source_specifier,
                        verter_workspace::ResolutionContext {
                            phase: verter_workspace::ResolvePhase::CodegenBlocker,
                            kind: verter_workspace::ResolveRequestKind::EsmImport,
                        },
                    )
                    .map(|resolution| {
                        self.normalize_live_type_dependency_target(
                            owner_canonical,
                            source_specifier,
                            resolution.source_id.as_str(),
                        )
                    })
            })?;

        if resolved.ends_with(".vue") {
            let known_hash = self
                .current_or_read_whole_hash(resolved.as_str())
                .or_else(|| self.cached_route_owned_shallow_whole_hash(resolved.as_str()));
            if let Some(hash) = known_hash {
                if !self.store_view_allows_current_whole_hash(resolved.as_str(), hash) {
                    return None;
                }
            }
        } else if self.current_or_read_whole_hash(resolved.as_str()).is_none() {
            // Canonical resolver-edge ensure_loaded: when a cross-file type
            // import resolves to a workspace `.ts`/`.d.ts` file the host
            // hasn't seen yet, load it once so subsequent probes hit the
            // cache.
            if !self.is_evalable(resolved.as_str()) && !self.ensure_loaded(resolved.as_str()) {
                return None;
            }
        }

        Some(resolved)
    }

    fn resolve_named_type_export_route_from_target(
        &self,
        provider_canonical: &str,
        target: &crate::resolver_core::ExportTarget,
        active: &mut rustc_hash::FxHashSet<(String, String)>,
        participants: &mut rustc_hash::FxHashSet<String>,
        route_shallow_cache: &mut RouteShallowStateCache,
    ) -> Option<crate::resolver_core::RouteResult> {
        match target {
            crate::resolver_core::ExportTarget::Local { symbol_name } => {
                let state = self.route_shallow_state(provider_canonical, route_shallow_cache)?;
                if state.is_import_local(symbol_name) {
                    let import_target = state.import_target(symbol_name)?;
                    let target_canonical = if import_target.canonical_id.is_empty() {
                        self.resolve_route_type_edge(
                            provider_canonical,
                            import_target.source_specifier.as_str(),
                        )?
                    } else {
                        import_target.canonical_id.clone()
                    };
                    return self.resolve_named_type_export_route_uncached(
                        target_canonical.as_str(),
                        import_target.imported_name.as_str(),
                        active,
                        participants,
                        route_shallow_cache,
                    );
                }

                Some(crate::resolver_core::RouteResult::Resolved {
                    defining_canonical: provider_canonical.to_string(),
                    defining_symbol: symbol_name.clone(),
                })
            }
            crate::resolver_core::ExportTarget::Reexport {
                source_specifier,
                original_name,
                canonical_id,
                ..
            } => {
                let target_canonical = if canonical_id.is_empty() {
                    self.resolve_route_type_edge(provider_canonical, source_specifier.as_str())?
                } else {
                    canonical_id.clone()
                };
                self.resolve_named_type_export_route_uncached(
                    target_canonical.as_str(),
                    original_name.as_str(),
                    active,
                    participants,
                    route_shallow_cache,
                )
            }
        }
    }
    /// `route_shallow_state` is the
    /// route-only frontier reader. Body now delegates to the shared
    /// materialiser ([`Self::ensure_route_owned_shallow_entry`]) and
    /// returns the entry's `shallow_state`. The request-scoped
    /// `route_shallow_cache` (frontier-engine memo, kept per-request to
    /// avoid repeated `Arc` clones) is still populated for in-flight
    /// frontier traversal.
    pub(super) fn route_shallow_state(
        &self,
        canonical_id: &str,
        route_shallow_cache: &mut RouteShallowStateCache,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        let normalized_canonical = self
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());

        // Authoritative `IndexedReady` fast path — preserved from the
        // pre-migration body so scheduler-materialised entries take
        // precedence over route-only shadow entries.
        if let Some(facts) = self
            .project_type_store
            .indexed()
            .get_any(normalized_canonical.as_str())
        {
            return Some(Arc::clone(&facts.shallow_state));
        }

        // Request-scoped memo (frontier engine de-dupe). NOT a host-side
        // mirror — see `HostFrontierAdapter::route_shallow_cache` doc-comment
        //.
        if let Some(cached) = route_shallow_cache.get(normalized_canonical.as_str()) {
            return Some(Arc::clone(cached));
        }

        let entry = self.ensure_route_owned_shallow_entry(normalized_canonical.as_str())?;
        let shallow_state = Arc::clone(&entry.shallow_state);
        route_shallow_cache.insert(normalized_canonical.clone(), Arc::clone(&shallow_state));
        Some(shallow_state)
    }

    pub(crate) fn route_owned_shallow_state(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        let mut route_shallow_cache = RouteShallowStateCache::default();
        self.route_shallow_state(canonical_id, &mut route_shallow_cache)
    }

    fn resolve_named_type_export_route_uncached(
        &self,
        provider_canonical: &str,
        exported_name: &str,
        active: &mut rustc_hash::FxHashSet<(String, String)>,
        participants: &mut rustc_hash::FxHashSet<String>,
        route_shallow_cache: &mut RouteShallowStateCache,
    ) -> Option<crate::resolver_core::RouteResult> {
        let key = (provider_canonical.to_string(), exported_name.to_string());
        if !active.insert(key.clone()) {
            return Some(crate::resolver_core::RouteResult::Miss);
        }
        participants.insert(provider_canonical.to_string());

        let result = (|| {
            let state = self.route_shallow_state(provider_canonical, route_shallow_cache)?;

            if let Some(target) = state.export_target(exported_name) {
                return self.resolve_named_type_export_route_from_target(
                    provider_canonical,
                    target,
                    active,
                    participants,
                    route_shallow_cache,
                );
            }

            let wildcard_indices = ordered_wildcard_indices_for_exported_name(
                &state.wildcard_reexports,
                exported_name,
            );
            for wildcard_index in wildcard_indices {
                let wildcard = &state.wildcard_reexports[wildcard_index];
                let target_canonical = if wildcard.canonical_id.is_empty() {
                    self.resolve_route_type_edge(
                        provider_canonical,
                        wildcard.source_specifier.as_str(),
                    )
                } else {
                    Some(wildcard.canonical_id.clone())
                };
                let Some(target_canonical) = target_canonical else {
                    continue;
                };
                let child = self.resolve_named_type_export_route_uncached(
                    target_canonical.as_str(),
                    exported_name,
                    active,
                    participants,
                    route_shallow_cache,
                )?;
                if !child.is_miss() {
                    return Some(child);
                }
            }

            Some(crate::resolver_core::RouteResult::Miss)
        })();

        active.remove(&key);
        result
    }

    pub(crate) fn build_named_type_export_route_entry(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(
        crate::resolver_core::RouteResult,
        Vec<crate::resolver_core::FactVersionRef>,
    )> {
        let mut active = rustc_hash::FxHashSet::default();
        let mut touched_canonical_ids = rustc_hash::FxHashSet::default();
        let mut route_shallow_cache = RouteShallowStateCache::default();
        let route_result = self.resolve_named_type_export_route_uncached(
            dep_canonical,
            requested_name,
            &mut active,
            &mut touched_canonical_ids,
            &mut route_shallow_cache,
        )?;

        let mut facts = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        let mut participants: Vec<String> = touched_canonical_ids.into_iter().collect();
        participants.sort();
        participants.dedup();
        for canonical in participants {
            self.append_route_participant_fact_versions(
                canonical.as_str(),
                &mut facts,
                &mut seen,
                None,
            );
        }

        Some((route_result, facts))
    }

    pub(super) fn resolve_named_type_export_target_uncached(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        let normalized_canonical = self
            .resolve_eval_dependency_canonical(dep_canonical)
            .unwrap_or_else(|| dep_canonical.to_string());
        let live_view = self.resolver_store_view();

        let cached_route = self
            .resolver
            .runtime
            .routes
            .get_or_resolve_route_with_facts(
                normalized_canonical.as_str(),
                requested_name,
                &live_view,
                || {
                    self.build_named_type_export_route_entry(
                        normalized_canonical.as_str(),
                        requested_name,
                    )
                },
            )?;
        cached_route
            .resolved()
            .map(|(defining_canonical, defining_symbol)| {
                (defining_canonical.to_owned(), defining_symbol.to_owned())
            })
    }

    pub(crate) fn resolve_named_type_export_target_shallow(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        let result =
            self.resolve_named_type_export_target_uncached(dep_canonical, requested_name)?;
        component_meta_trace_custom!(
            "resolve_named_type_export_target_result",
            format!(
                "owner={} requested={} source=route_db target={} exported={} materialized=false",
                dep_canonical, requested_name, result.0, result.1
            ),
        );
        Some(result)
    }
}
