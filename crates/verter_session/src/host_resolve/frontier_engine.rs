//! `impl VerterHost` — named-type export route resolution.
//!
//! Owns the routing/index-fact layer shared by TypeInfo and component-meta:
//! - `append_route_participant_fact_versions` fans the touched canonical
//!   set into `FactVersionRef` entries for cache-fence accounting.
//! - `resolve_route_type_edge` drives the live-host shallow + workspace
//!   resolver chain for a single route hop.
//! - The route-only named-type export resolver
//!   (`resolve_named_type_export_route_from_target` /
//!   `resolve_named_type_export_route_uncached`) — the single non-trivial
//!   intra-file SCC identified by the Tier 0 audit.
//! - `route_shallow_state` / `routed_shallow_state` — request-scoped
//!   readers that join the canonical `IndexedReady` build
//!   (`ensure_indexed_ready_serve`).
//! - `build_named_type_export_route_entry` /
//!   `resolve_named_type_export_target_uncached` /
//!   `resolve_named_type_export_target_shallow` — host-level binding into
//!   the route-DB cooperative resolve.

use std::sync::Arc;

use super::frontier_helpers::{
    ordered_wildcard_indices_for_exported_name, RouteShallowStateCache, RoutedShallowServe,
};
use crate::host_manage::component_meta_trace_custom;
use crate::VerterHost;

impl VerterHost {
    fn append_route_participant_fact_versions(
        &self,
        canonical: &str,
        facts: &mut Vec<crate::resolver_core::FactVersionRef>,
        seen: &mut rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef>,
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

        // Route fact production routes through the single
        // `current_route_surface_hash` helper — the SAME source order
        // (content-pinned `IndexedReady` for a scheduler-tracked
        // canonical, the artifact-only authority otherwise) the
        // `HostStoreView` validator snapshots route facts in. A
        // divergent source order here would record a hash the
        // validator could not reproduce.
        if let Some(hash) = self.current_route_surface_hash(canonical) {
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
        // Resolve the edge through the single shared route-edge policy
        // (`resolve_route_edge_canonical`), then layer the route-traversal-only
        // side effects (carrier store-view gate, `ensure_loaded`) on top. The
        // pure resolution — including the normalized ESM fallback — lives in
        // the shared helper so this path, shallow-state canonicalization, and
        // known-miss revalidation agree on every edge.
        let resolved = self.resolve_route_edge_canonical(owner_canonical, source_specifier)?;

        // Carrier-GENERIC route-edge revalidation: a framework CARRIER target
        // (`.vue`, `.svelte`, …) takes the store-view whole-hash gate; a plain
        // script edge takes the `ensure_loaded` arm. The carrier branch loads
        // the target via `current_or_read_whole_hash` (which ensure-loads on
        // miss), so a `.svelte` carrier is loaded here exactly like a `.vue`
        // one. Classified through the single static classifier, never a
        // hardcoded `.vue` suffix that would strand other carriers.
        let resolved_is_carrier = verter_language::LanguageRegistry::global()
            .classify_static(resolved.as_str())
            .static_resolution()
            .is_framework_carrier();

        if resolved_is_carrier {
            let known_hash = self.current_or_read_whole_hash(resolved.as_str());
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
        unresolved_edge_owners: &mut rustc_hash::FxHashSet<(String, String)>,
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
                        unresolved_edge_owners,
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
                    unresolved_edge_owners,
                    route_shallow_cache,
                )
            }
        }
    }
    /// `route_shallow_state_serve` is the route-only frontier reader
    /// with the publication status flowed BY VALUE (see
    /// [`RoutedShallowServe`]). The cold fall-through JOINS the
    /// canonical `IndexedReady` build
    /// ([`Self::ensure_indexed_ready_serve`] — same singleflight lane
    /// as every other cold consumer); there is no separate route-only
    /// artifact build. The request-scoped `route_shallow_cache`
    /// (frontier-engine memo, kept per-request to avoid repeated `Arc`
    /// clones) is still populated for in-flight frontier traversal, and
    /// every serve exit records its publication status on the cache
    /// (`RouteShallowStateCache::observe_serve`) so a walk threading
    /// one cache per route entry can decline shared-cache admission
    /// for results computed from a fenced (ReturnOnly) surface.
    pub(super) fn route_shallow_state_serve(
        &self,
        canonical_id: &str,
        route_shallow_cache: &mut RouteShallowStateCache,
    ) -> Option<RoutedShallowServe> {
        let normalized_canonical = self
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());

        // Authoritative `IndexedReady` fast path — scheduler-materialised
        // entries take precedence over the request-scoped memo.
        //
        // Current-content-pinned (no bare `get_any`): a stale pre-edit
        // `IndexedReady` can linger past a same-canonical edit, and a
        // `get_any` read here would let that stale artifact shadow the
        // freshly-published current-content entry.
        // `observe_content_pinned_indexed` serves the content-current
        // artifact for a scheduler-tracked canonical and falls back to
        // the artifact-current authority for a genuinely artifact-only
        // canonical (a workspace dependency materialised into
        // `FileArtifactStore` with no live scheduler `DerivedRawState`).
        // The OBSERVE-only read is deliberate: the re-indexing accessors
        // (`current_content_pinned_indexed` / `artifact_current_indexed`)
        // rebuild an edge-stale surface internally through the
        // status-DROPPING `ensure_indexed_ready`, which would launder a
        // fenced rebuild into a published-looking serve — so the
        // edge-stale rebuild runs here, through the serve-carrying
        // entry-point. A stale older-content artifact for a live
        // scheduler scope misses the observe read entirely, so the
        // canonical `ensure_indexed_ready_serve` build (the fall-through
        // below) rebuilds it.
        if let Some(indexed) = self.observe_content_pinned_indexed(normalized_canonical.as_str()) {
            // Reuse a baked indexed surface for route traversal ONLY while it
            // is edge-current. A wildcard-bearing artifact whose `export *`
            // edges were baked at an earlier generation (a dependency since
            // appeared / retargeted) would otherwise feed traversal a stale
            // `canonical_id`. Rebuild it through `ensure_indexed_ready_serve`
            // (which re-resolves the edges against the live file set and
            // replaces the stale candidate) and traverse the fresh surface.
            if self.indexed_surface_is_current(normalized_canonical.as_str(), &indexed) {
                // A store hit IS the published current surface.
                let serve = RoutedShallowServe {
                    state: Arc::clone(&indexed.shallow_state),
                    store_published: true,
                };
                route_shallow_cache.observe_serve(&serve);
                return Some(serve);
            }
            if let Some(fresh) = self.ensure_indexed_ready_serve(normalized_canonical.as_str()) {
                let serve = RoutedShallowServe {
                    state: Arc::clone(&fresh.indexed.shallow_state),
                    store_published: fresh.store_published,
                };
                route_shallow_cache.observe_serve(&serve);
                return Some(serve);
            }
            // The edge-stale rebuild missed. A second
            // `ensure_indexed_ready_serve` attempt would re-run the
            // identical flight under identical conditions (a serve miss
            // means the canonical is unloadable or the store view
            // disallows it — nothing a back-to-back retry changes; every
            // fenced outcome returns `Some`), so serve from the
            // request-scoped memo if an earlier traversal populated it,
            // otherwise decline.
            return route_shallow_cache
                .get(normalized_canonical.as_str())
                .cloned();
        }

        // Request-scoped route memo, never a host-side mirror.
        // Memo entries carry their publication status by value; the fenced
        // observation was already recorded at insert time.
        if let Some(cached) = route_shallow_cache.get(normalized_canonical.as_str()) {
            return Some(cached.clone());
        }

        let indexed_serve = self.ensure_indexed_ready_serve(normalized_canonical.as_str())?;
        let serve = RoutedShallowServe {
            state: Arc::clone(&indexed_serve.indexed.shallow_state),
            store_published: indexed_serve.store_published,
        };
        route_shallow_cache.observe_serve(&serve);
        route_shallow_cache.insert(normalized_canonical.clone(), serve.clone());
        Some(serve)
    }

    /// Thin wrapper over [`Self::route_shallow_state_serve`] that drops
    /// the publication status from the RETURN value. The fenced
    /// observation still lands on the threaded `route_shallow_cache`,
    /// so walk-level consumers stay covered; callers that derive
    /// SHARED-cache entries from the returned state directly must use
    /// the serve variant and gate admission on `store_published` (see
    /// [`RoutedShallowServe`]).
    pub(super) fn route_shallow_state(
        &self,
        canonical_id: &str,
        route_shallow_cache: &mut RouteShallowStateCache,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        self.route_shallow_state_serve(canonical_id, route_shallow_cache)
            .map(|serve| serve.state)
    }

    /// One-shot [`Self::route_shallow_state_serve`] with a fresh memo —
    /// the publication status reflects exactly the requested
    /// canonical's serve.
    pub(crate) fn routed_shallow_state_serve(
        &self,
        canonical_id: &str,
    ) -> Option<RoutedShallowServe> {
        let mut route_shallow_cache = RouteShallowStateCache::default();
        self.route_shallow_state_serve(canonical_id, &mut route_shallow_cache)
    }

    /// Thin wrapper over [`Self::routed_shallow_state_serve`] that drops
    /// the publication status. Callers that derive SHARED-cache entries
    /// from the returned state must use the serve variant and gate
    /// admission on `store_published` (see [`RoutedShallowServe`]).
    pub(crate) fn routed_shallow_state(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        self.routed_shallow_state_serve(canonical_id)
            .map(|serve| serve.state)
    }

    /// Context-threaded variant of [`Self::routed_shallow_state`].
    ///
    /// When `ctx` carries an active [`crate::session_view::SessionView`]
    /// with overlay parse artifacts for `canonical_id`, the overlay-rooted
    /// shallow surface is returned directly — so a session-bearing cold
    /// compute observes overlay re-export / tombstone edits. Otherwise the
    /// base (content-pinned) [`Self::routed_shallow_state`] body runs.
    ///
    /// This is the route-surface fallback
    /// [`Self::shallow_file_state_with_context`] uses; its indexed fast
    /// path is content-pinned via [`Self::route_shallow_state`].
    pub(crate) fn routed_shallow_state_with_context(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        self.routed_shallow_state_with_view(canonical_id, ctx.active_session_view())
    }

    /// View-aware variant of [`Self::routed_shallow_state`].
    ///
    /// When `view: Some(...)` carries parse artifacts for `canonical_id`,
    /// returns the overlay-rooted shallow state directly so route-aware
    /// callers driven from a session-bearing path observe overlay surfaces
    /// (re-export edits, tombstoned dependencies). Base callers
    /// (`view = None`) fall through to the historical
    /// `routed_shallow_state` body — identical behaviour.
    pub(crate) fn routed_shallow_state_with_view(
        &self,
        canonical_id: &str,
        view: Option<&dyn crate::session_view::SessionView>,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        if let Some(view) = view {
            // `canonical_id` is the RAW requested canonical.
            if view.overlay_content_hash_for(canonical_id).is_some() {
                // GENUINELY OVERLAID canonical: route through the gated overlay
                // materialiser accessor so an edge-stale wildcard `export *`
                // surface re-resolves against the live file set (re-materialised
                // from the overlay source, never the base surface — no
                // overlay-blindness) before it is served.
                if let Some(serve) =
                    self.materialize_overlay_indexed_ready_serve_with_view(canonical_id, view)
                {
                    return Some(Arc::clone(&serve.indexed.shallow_state));
                }
            } else {
                // Base-passthrough view: the base-key read returns the
                // published base artifact for a non-overlaid canonical. Serve
                // it only while edge-current; an edge-stale wildcard `export *`
                // surface falls through to the gated base path below
                // (`route_shallow_state`, whose indexed fast path re-indexes on
                // edge-stale) so the edges re-resolve against the live file set.
                let identity = self.overlay_artifact_identity(canonical_id);
                if let Some(facts) = identity.lookup_overlay_artifacts(self, view) {
                    if self.indexed_surface_is_current(canonical_id, &facts.indexed) {
                        return Some(Arc::clone(&facts.indexed.shallow_state));
                    }
                }
            }
        }
        self.routed_shallow_state(canonical_id)
    }

    fn resolve_named_type_export_route_uncached(
        &self,
        provider_canonical: &str,
        exported_name: &str,
        active: &mut rustc_hash::FxHashSet<(String, String)>,
        participants: &mut rustc_hash::FxHashSet<String>,
        unresolved_edge_owners: &mut rustc_hash::FxHashSet<(String, String)>,
        route_shallow_cache: &mut RouteShallowStateCache,
    ) -> Option<crate::resolver_core::RouteResult> {
        let key = (provider_canonical.to_string(), exported_name.to_string());
        if !active.insert(key.clone()) {
            return Some(crate::resolver_core::RouteResult::Miss);
        }
        participants.insert(provider_canonical.to_string());

        let result = (|| {
            // LAYER-ORDERED wildcard walk (Build Philosophy: cross-file
            // deepening happens one import level at a time). Every node of the
            // CURRENT layer is checked for a DIRECT export of the name before
            // ANY node's wildcard children are descended — a same-layer match
            // is chosen without loading a deeper branch reachable through an
            // earlier-declared sibling. Within a layer, nodes keep the
            // score-then-declared wildcard order of their parent.
            let mut visited: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
            visited.insert(provider_canonical.to_string());
            let mut layer: Vec<String> = vec![provider_canonical.to_string()];
            while !layer.is_empty() {
                // Phase 1: same-layer DIRECT export surfaces, in order.
                let mut layer_states = Vec::with_capacity(layer.len());
                for canonical in &layer {
                    participants.insert(canonical.clone());
                    let state = self.route_shallow_state(canonical, route_shallow_cache)?;
                    if let Some(target) = state.export_target(exported_name) {
                        return self.resolve_named_type_export_route_from_target(
                            canonical,
                            target,
                            active,
                            participants,
                            unresolved_edge_owners,
                            route_shallow_cache,
                        );
                    }
                    layer_states.push((canonical.clone(), state));
                }

                // Phase 2: build the NEXT layer from each node's wildcard
                // edges, keeping per-node score-then-declared order.
                let mut next_layer: Vec<String> = Vec::new();
                for (canonical, state) in layer_states {
                    let wildcard_indices = ordered_wildcard_indices_for_exported_name(
                        &state.wildcard_reexports,
                        exported_name,
                    );
                    for wildcard_index in wildcard_indices {
                        let wildcard = &state.wildcard_reexports[wildcard_index];
                        let target_canonical = if wildcard.canonical_id.is_empty() {
                            self.resolve_route_type_edge(
                                canonical.as_str(),
                                wildcard.source_specifier.as_str(),
                            )
                        } else {
                            Some(wildcard.canonical_id.clone())
                        };
                        let Some(target_canonical) = target_canonical else {
                            // The wildcard's source specifier does not resolve
                            // under the current workspace. The Miss this may
                            // produce depends on that unresolved edge
                            // re-resolving when the file set changes — record
                            // the owner AND the unresolved source specifier so
                            // the route entry roots it in the `ImportRoute`
                            // fact rail.
                            // Neither the owner's `FileWholeHash` nor its
                            // `Route` hash re-resolves a known-miss specifier,
                            // so without this the cached Miss is served stale
                            // after the target appears. The SOURCE identity is
                            // threaded (not just the owner) so the rooting
                            // loop can verify the produced `ImportRoute` hash
                            // actually covers this exact wildcard source; an
                            // owner with a route surface that does not track
                            // this source must NOT admit a hash that silently
                            // drops it.
                            unresolved_edge_owners
                                .insert((canonical.clone(), wildcard.source_specifier.clone()));
                            continue;
                        };
                        // A wildcard hop that lands on an ACTIVE (provider,
                        // name) pair is a route cycle: skip the edge (the
                        // in-flight walk already covers it), mirroring the
                        // recursive walk's active-set Miss.
                        if active.contains(&(target_canonical.clone(), exported_name.to_string())) {
                            continue;
                        }
                        if visited.insert(target_canonical.clone()) {
                            next_layer.push(target_canonical);
                        }
                    }
                }
                layer = next_layer;
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
        let mut unresolved_edge_owners = rustc_hash::FxHashSet::default();
        let mut route_shallow_cache = RouteShallowStateCache::default();
        let route_result = self.resolve_named_type_export_route_uncached(
            dep_canonical,
            requested_name,
            &mut active,
            &mut touched_canonical_ids,
            &mut unresolved_edge_owners,
            &mut route_shallow_cache,
        )?;

        // ReturnOnly never publishes — fenced-participant arm. A walk that
        // consumed ANY fenced (ReturnOnly) shallow serve computed its route
        // from a superseded surface (a baked edge resolved under the
        // pre-mutation route table), while the participant facts below are
        // read from the LIVE post-mutation state — an entry the read-side
        // fact rail cannot reject. Serve the result to this caller (its
        // request pre-dates the mutation) with EMPTY facts: the same
        // strict-admission negative-cache pattern as the unproduce-able
        // wildcard-hash case below — the value is returned, `RouteDb` /
        // `ImportedRootDb` never persist it, and the next query re-resolves
        // cold against the live workspace.
        if route_shallow_cache.fenced_serve_observed() {
            return Some((route_result, Vec::new()));
        }

        let mut facts = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        let mut participants: Vec<String> = touched_canonical_ids.into_iter().collect();
        participants.sort();
        participants.dedup();
        for canonical in participants {
            self.append_route_participant_fact_versions(canonical.as_str(), &mut facts, &mut seen);
        }

        // Root any unresolved `export *` wildcard edge the traversal hit in the
        // `ImportRoute` fact rail. The owner's
        // `FileWholeHash` + `Route` facts do NOT re-resolve a known-miss
        // specifier, so a Miss caused by an unresolvable wildcard would be
        // served stale after the target appears. `generation_current_import_route_hash`
        // re-resolves the owner's known-miss specifiers against the live
        // workspace, so the recorded fact changes the moment the edge resolves.
        //
        // When an owner has no import-route surface to root the unresolved edge
        // on (e.g. a barrel whose wildcards resolve into a
        // local `dep_edges` map and never publish `import_routes`), the hash is
        // unproduce-able. We must NOT admit a fact-validated entry — a cached
        // value could stale-serve once the target appears. But we must equally
        // NOT DROP a valid result: returning `None` here makes `RouteDb` serve
        // no value at all, which silently discards a route that resolved through
        // a LATER wildcard (never conflate "refuse to
        // cache" with "no result"). Instead, return the resolved route surface
        // with EMPTY facts: `RouteDb`'s strict admission treats an empty fact
        // signature as the negative-cache pattern — the value is returned to the
        // caller but never persisted — so the next query re-resolves cold
        // against the live workspace.
        //
        // The hash must also COVER every unresolved wildcard source the
        // traversal hit on that owner. An owner can
        // have a fully-resolved route surface (so a bare
        // `generation_current_import_route_hash` returns `Some`) whose table
        // does NOT track the wildcard source — e.g. a PARTIAL import-route
        // snapshot resolving a sibling but omitting the wildcard. That hash is
        // reproduced verbatim after the target appears, so it cannot root the
        // known-miss. `generation_current_import_route_hash_covering_sources`
        // returns `None` for that incomplete case, routing it through the SAME
        // empty-facts negative-cache path as the no-surface case.
        let mut owner_sources: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for (owner, source) in unresolved_edge_owners {
            owner_sources.entry(owner).or_default().push(source);
        }
        for (owner, sources) in owner_sources {
            let Some(import_route_hash) = self
                .generation_current_import_route_hash_covering_sources(owner.as_str(), &sources)
            else {
                // The empty-facts signal alone only protects the caches
                // that inspect route facts directly (`RouteDb` /
                // `ImportedRootDb` strict admission, the owner-import-
                // surface producer's per-binding check). An ENCLOSING
                // traced cold compute (a semantic-memo build, a
                // component-meta proof producer) observes NOTHING from
                // an empty fact list — its own stamps validate against
                // the live view while the folded route silently
                // retargets when the wildcard target appears. Mark cache
                // non-admission by hand, exactly as the route-singleflight
                // follower fallback does for an adopted unrootable route —
                // the leader-produced unrootable route must refuse the same
                // admissions. This is a VALID (Complete) unrootable route,
                // NOT a partial result — cache non-admission only, never
                // request partiality.
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::UnrootableRoute,
                );
                return Some((route_result, Vec::new()));
            };
            let fact = crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: owner,
                kind: crate::resolver_core::DerivedFactKind::ImportRoute,
                hash: import_route_hash,
            };
            if seen.insert(fact.clone()) {
                facts.push(fact);
            }
        }

        Some((route_result, facts))
    }

    /// View-bound resolver for the cached route entry. Validates the
    /// cached `RouteDb` entry against the supplied request-bound view
    /// rather than rebuilding a per-call owned workspace snapshot.
    /// Request-bound callers (`HostResolverContext`,
    /// `SessionResolverContext`) route through this variant; off-path
    /// callers either compose a one-shot owned snapshot at the request
    /// entry boundary or go through the `#[cfg(test)]`-only one-shot
    /// rebuild on `impl ResolverContext for VerterHost`.
    pub(super) fn resolve_named_type_export_target_uncached_with_store_view(
        &self,
        view: &dyn crate::resolver_core::StoreView,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        let normalized_canonical = self
            .resolve_eval_dependency_canonical(dep_canonical)
            .unwrap_or_else(|| dep_canonical.to_string());

        // Build the R6/R21-compliant route key from the PROVIDER's project
        // env (the route resolution is resolve-domain: it depends on the
        // provider project's module-resolution + ambient-augmentation env).
        // The same key serves the warm lookup and the cold publish inside
        // `get_or_resolve_route_observing_facts`, so lookup and publish
        // agree by construction. The named-type-export path is statically
        // type-space.
        let provider = normalized_canonical.as_str();
        let env = self.host_view_env_hashes_for(provider);
        let route_key = crate::resolver_core::route_db::RouteNameKey::new(
            provider,
            requested_name,
            verter_semantic::facts::registry::SymbolSpace::Type,
            self.host_view_project_identity_for(provider),
            env.resolve_env_hash,
            env.lib_env_hash,
        );

        // Consume the route through the fact-observing entry-point so
        // the route's `fact_dep_signature` bubbles into any active
        // outer `with_fact_tracer` scope on the current thread (warm
        // hits + cold leader resolves + coalesced follower joins).
        //
        // `RouteDb` owns the OUTERMOST cacheability scope: the
        // walk inside `build_named_type_export_route_entry` rides
        // `ensure_indexed_ready_serve` and demands decl bodies, so it can consume
        // a fenced serve, a broken decl-body lease, or an unrootable route. The
        // producer's own empty-facts convention covers only the fenced case; the
        // funnel's post-compute verdict is the structural floor that covers all
        // four — including the content-neutral ones, where the hash does not move
        // and a warm-rooted entry would validate forever.
        let cached_route = self
            .resolver
            .runtime
            .routes
            .get_or_resolve_route_observing_facts_with_context(route_key, view, self, || {
                self.build_named_type_export_route_entry(provider, requested_name)
            });
        let cached_route = cached_route?;
        cached_route
            .resolved()
            .map(|(defining_canonical, defining_symbol)| {
                (defining_canonical.to_owned(), defining_symbol.to_owned())
            })
    }

    /// Test-only bare wrapper. Production callers go through
    /// `ctx.resolve_named_type_export_target_shallow` (which routes
    /// through the request-bound `_with_store_view`); the test-only
    /// arm on `impl ResolverContext for VerterHost` reaches this
    /// wrapper on test fixtures that call `host.<method>` directly.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)]
    pub(crate) fn resolve_named_type_export_target_shallow(
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
        self.resolve_named_type_export_target_shallow_with_store_view(
            &live_view,
            dep_canonical,
            requested_name,
        )
    }

    /// View-bound variant — production-reachable through ctx-bound
    /// `HostResolverContext` / `SessionResolverContext` callers.
    pub(crate) fn resolve_named_type_export_target_shallow_with_store_view(
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
