//! `impl VerterHost` — import-route + dependency canonical resolution.
//!
//! Owns the helpers that drive route resolution / canonical-ID lookup
//! before any external type traversal happens:
//! - Wrappers around the project-store-owned `RouteOwnedShallowDb`
//!   (`invalidate_route_owned_shallow_cache`,
//!   `snapshot_route_owned_shallow_cache_entries`).
//! - `expand_relative_candidates` — pre-snapshot blocker hydration probe.
//! - `authoritative_import_route` + `import_route_target` /
//!   `import_route_is_known_miss` predicates.
//! - The runtime/declaration target classifiers
//!   (`runtime_like_dependency_target`,
//!   `declaration_like_dependency_target`,
//!   `runtime_dependency_target`).
//! - `shallow_route_dependency_target` for shallow re-export consultation.
//! - `prefer_type_dependency_target_from_resolution` /
//!   `normalize_live_type_dependency_target` /
//!   `fallback_relative_type_companion`.
//! - The cache-write helpers `cache_positive_import_route_result` (the
//!   canonical positive-only point-admission producer for
//!   `DerivedRawState.import_routes`) and
//!   `resolve_workspace_dependency_and_cache`.
//! - The public `resolve_loaded_dependency_canonical`,
//!   `resolve_type_dependency_canonical`, and
//!   `resolve_type_dependency_canonical_shallow` entry points.

use super::frontier_helpers::RouteOwnedShallowStateSnapshot;
use crate::host_manage::component_meta_trace_custom;
use crate::VerterHost;

impl VerterHost {
    /// invalidate the route-only shallow entry for a
    /// canonical via the project-store-owned
    /// [`RouteOwnedShallowDb`](crate::project_type_store::RouteOwnedShallowDb).
    /// The pre-migration host-mutex (`route_owned_shallow_cache`) is gone;
    /// this thin wrapper keeps the existing call site
    /// (`host_manage::set_import_dependencies`) working through one indirection
    /// while the body delegates to the store DB. Future cleanup may inline
    /// the call.
    pub(crate) fn invalidate_route_owned_shallow_cache(&self, canonical_id: &str) {
        self.project_type_store
            .route_owned_shallow()
            .remove(canonical_id);
    }

    /// snapshot the route-only shallow entries from the
    /// project-store DB for fact-capture (`resolver_store::derived_hashes`).
    /// Iteration is across `DashMap<Arc<str>, Arc<RouteOwnedShallowEntry>>`;
    /// the projection ([`RouteOwnedShallowStateSnapshot`]) carries the
    /// minimal `(canonical_id, whole_hash, optional route_hash)` shape
    /// consumed by `resolver_store.rs:137`.
    ///
    /// Only EDGE-CURRENT entries are snapshotted: `HostStoreView::build`
    /// snapshots route hashes from this set, so a stale wildcard-bearing
    /// route-owned entry (its resolved edges baked at an earlier workspace
    /// generation, before a dependency appeared or retargeted) would
    /// otherwise publish a `Route` hash that validates a warm `RouteDb`
    /// entry after the change. The SAME `route_owned_entry_is_edge_current`
    /// gate that governs `current_route_surface_hash` filters the snapshot —
    /// one gate, two route-fact producers.
    pub(crate) fn snapshot_route_owned_shallow_cache_entries(
        &self,
    ) -> Vec<RouteOwnedShallowStateSnapshot> {
        self.project_type_store
            .route_owned_shallow()
            .for_each_entry(|canonical_id, entry| {
                self.route_owned_entry_is_edge_current(canonical_id, entry)
                    .then(|| RouteOwnedShallowStateSnapshot::from_entry(canonical_id, entry))
            })
            .into_iter()
            .flatten()
            .collect()
    }

    /// Expand a relative import specifier into all candidate canonical IDs.
    ///
    /// Given an owner file and a relative specifier (e.g. `./types`), returns
    /// a list of candidates: the direct path, then with each resolve extension,
    /// then `/index` variants. Used by pre-snapshot blocker hydration to probe
    /// the filesystem without a full resolver.
    pub fn expand_relative_candidates(
        &self,
        owner_canonical: &str,
        specifier: &str,
    ) -> Vec<String> {
        let direct = crate::id::resolve_external(owner_canonical, specifier);
        let mut candidates = vec![direct.clone()];
        for ext in &self.config.resolve_extensions {
            candidates.push(format!("{direct}{ext}"));
        }
        for ext in &self.config.resolve_extensions {
            candidates.push(format!("{direct}/index{ext}"));
        }
        candidates
    }

    pub(crate) fn authoritative_import_route(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<crate::types::DependencyResolution> {
        let (resolution, source_kind) = if let Some(resolution) =
            self.cached_import_route_resolution(owner_canonical, import_source)
        {
            // `DerivedRawState.import_routes` path — `cached_import_route_resolution`
            // already revalidates known-miss entries against the
            // `import_routes_known_miss_recorded_at_generation` sidecar,
            // so anything it returns is generation-current.
            //
            // Per-request audit attribution: classify the warm hit as
            // positive vs negative (known-miss). Negative warm hits are
            // the discriminating signal that the known-miss sidecar
            // short-circuited a re-resolution for this audited request.
            if let Some(obs) = verter_audit::current_observer() {
                let known_miss = Self::import_route_is_known_miss(&resolution);
                let event = if known_miss {
                    verter_audit::AuditEvent::ResolveImportWarmNegative
                } else {
                    verter_audit::AuditEvent::ResolveImportWarmPositive
                };
                obs.record_event(event);
                if known_miss {
                    obs.record_event(verter_audit::AuditEvent::KnownMissRouteServed);
                }
            }
            (Some(resolution), "host-cache")
        } else {
            // `IndexedReady.import_routes` fallback. This snapshot is
            // built once at materialisation time and carries NO
            // generation tag. A NEGATIVE (known-miss) entry in it is a
            // stale-by-construction snapshot: once a new file appears,
            // the workspace `content_generation` advances but the
            // owner's content (hence its `IndexedReady`) does not, so
            // the negative snapshot would otherwise be served forever
            // and every caller — which maps a known-miss resolution to
            // an unconditional `return None` — would treat the import
            // as permanently unresolvable.
            //
            // Negative entries are therefore NOT served from this
            // fallback: the miss is recomputed cheaply by the caller's
            // `resolve_workspace_dependency_and_cache` path, which
            // re-resolves against the current workspace and reopens the
            // route the moment the target file exists. Positive
            // resolutions stay valid (a new file never invalidates an
            // already-resolved positive) and are served unchanged.
            let from_indexed = self
                .ensure_indexed_ready(owner_canonical)?
                .import_routes
                .get(import_source)
                .cloned()
                .filter(|resolution| !Self::import_route_is_known_miss(resolution));
            // Per-request audit attribution: an `IndexedReady` lookup
            // is a "cold" resolution from the audit's perspective
            // (the host-cache miss above forced us into the
            // snapshot). Classify positive vs negative.
            if let Some(obs) = verter_audit::current_observer() {
                let event = if from_indexed.is_some() {
                    verter_audit::AuditEvent::ResolveImportColdPositive
                } else {
                    verter_audit::AuditEvent::ResolveImportColdNegative
                };
                obs.record_event(event);
            }
            (from_indexed, "indexed_ready")
        };

        component_meta_trace_custom!(
            "authoritative_import_route_result",
            format!(
                "owner={} import={} source={} target={}",
                owner_canonical,
                import_source,
                source_kind,
                resolution
                    .as_ref()
                    .and_then(Self::import_route_target)
                    .as_deref()
                    .unwrap_or("<none>"),
            ),
        );
        resolution
    }

    pub(crate) fn import_route_target(
        resolution: &crate::types::DependencyResolution,
    ) -> Option<String> {
        resolution
            .resolved_canonical_id
            .clone()
            .or_else(|| resolution.effective_target().map(str::to_string))
    }

    pub(crate) fn import_route_is_known_miss(
        resolution: &crate::types::DependencyResolution,
    ) -> bool {
        resolution.resolved_canonical_id.is_none()
            && resolution.effective_target().is_none()
            && resolution.possible_canonical_ids.is_empty()
    }

    fn runtime_like_dependency_target(path: &str) -> bool {
        path.ends_with(".js")
            || path.ends_with(".jsx")
            || path.ends_with(".mjs")
            || path.ends_with(".cjs")
    }

    fn declaration_like_dependency_target(path: &str) -> bool {
        path.ends_with(".d.ts") || path.ends_with(".d.mts") || path.ends_with(".d.cts")
    }

    fn runtime_dependency_target(
        resolution: &crate::types::DependencyResolution,
    ) -> Option<String> {
        let resolved = Self::import_route_target(resolution)?;
        (!Self::declaration_like_dependency_target(&resolved)).then_some(resolved)
    }

    fn shallow_route_dependency_target(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        let state = &self.ensure_indexed_ready(owner_canonical)?.shallow_state;
        state
            .exports
            .values()
            .find_map(|target| match target {
                crate::resolver_core::ExportTarget::Reexport {
                    source_specifier,
                    canonical_id,
                    ..
                } if source_specifier == import_source && !canonical_id.is_empty() => {
                    Some(canonical_id.clone())
                }
                _ => None,
            })
            .or_else(|| {
                state
                    .wildcard_reexports
                    .iter()
                    .find(|target| {
                        target.source_specifier == import_source && !target.canonical_id.is_empty()
                    })
                    .map(|target| target.canonical_id.clone())
            })
    }

    pub(crate) fn prefer_type_dependency_target_from_resolution(
        &self,
        owner_canonical: &str,
        import_source: &str,
        resolution: &crate::types::DependencyResolution,
    ) -> Option<String> {
        if let Some(candidate) = resolution
            .possible_canonical_ids
            .iter()
            .min_by_key(|candidate| crate::types::extension_priority(candidate))
        {
            return Some(candidate.clone());
        }

        let resolved = Self::import_route_target(resolution)?;
        if !import_source.starts_with('.') && Self::runtime_like_dependency_target(&resolved) {
            if let Some(resolved_type) = self
                .ws()
                .resolve_import(
                    owner_canonical,
                    import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::TypeImport,
                    },
                )
                .map(|resolution| resolution.source_id)
            {
                return Some(resolved_type);
            }
        }

        Some(resolved.to_string())
    }

    pub(crate) fn normalize_live_type_dependency_target(
        &self,
        owner_canonical: &str,
        import_source: &str,
        resolved: &str,
    ) -> String {
        if let Some(fallback) = self.resolve_eval_dependency_canonical(resolved) {
            if fallback != resolved {
                return fallback;
            }
        }

        if !import_source.starts_with('.') && Self::runtime_like_dependency_target(resolved) {
            if let Some(resolved_type) = self
                .ws()
                .resolve_import(
                    owner_canonical,
                    import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::TypeImport,
                    },
                )
                .map(|resolution| resolution.source_id)
            {
                return resolved_type;
            }
        }

        resolved.to_string()
    }

    pub(crate) fn fallback_relative_type_companion(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        if !import_source.starts_with('.') || !Self::runtime_like_dependency_target(import_source) {
            return None;
        }
        let direct = crate::id::resolve_external(owner_canonical, import_source);
        self.resolve_eval_dependency_canonical(direct.as_str())
    }

    /// Positive-only point-admission producer for
    /// [`DerivedRawState::import_routes`](crate::types::DerivedRawState).
    ///
    /// Constructs a positive
    /// [`DependencyResolution`](crate::types::DependencyResolution)
    /// with `resolved_canonical_id: Some(...)` and a single-candidate
    /// `possible_canonical_ids` vector, then inserts it into the
    /// owner's `import_routes` map under the supplied
    /// `import_source` specifier and registers the resolved canonical
    /// in the owner's flat `dependencies` set on `DependencyState`.
    ///
    /// Architectural contract:
    ///
    /// * This helper is the **single** positive-route point producer
    ///   for `DerivedRawState.import_routes`. The complete caller-
    ///   supplied route-snapshot writer is
    ///   [`Self::set_import_dependencies`]; lifecycle reset goes
    ///   through [`Self::configure_projects`] and
    ///   [`Self::finish_upsert_post_commit`]. A new positive
    ///   route admission must route through this helper rather than
    ///   inlining a direct
    ///   `derived_raw_cache().entry(...).import_routes.insert(...)`.
    ///
    /// * The helper **must not** touch
    ///   `DerivedRawState::import_routes_known_miss_recorded_at_generation`.
    ///   That sidecar tracks the workspace `content_generation` at
    ///   which known-miss specifiers (no resolved canonical, no
    ///   candidates, no effective target) were admitted, and its
    ///   admission is single-producer at `set_import_dependencies`.
    ///   Stamping a known-miss generation here would extend a stale
    ///   negative answer that should re-resolve when content changes.
    ///   Positive resolutions do not need a generation tag: they stay
    ///   valid until the owner's own source content changes (which
    ///   evicts the `DerivedRawState` entry in
    ///   `finish_upsert_post_commit`).
    ///
    /// * Reader correctness:
    ///   [`Self::import_route_is_known_miss`] requires resolved
    ///   canonical = `None` AND no effective target AND empty
    ///   candidate list — a value constructed here can never satisfy
    ///   that predicate, which is what keeps the architectural
    ///   invariant local to this body.
    ///
    /// The [`import_route_writer_guard`](crate::tests) integration
    /// test enforces both directions of the contract: the strict
    /// known-miss sidecar guard rejects any sidecar mutation outside
    /// `set_import_dependencies` and the two lifecycle reset
    /// methods; the positive-route allow-list rejects any direct
    /// `.import_routes` mutation outside this helper, the snapshot
    /// writer, and the lifecycle reset methods.
    pub(super) fn cache_positive_import_route_result(
        &self,
        owner_canonical: &str,
        import_source: &str,
        resolved_canonical_id: &str,
    ) {
        let resolution = crate::types::DependencyResolution {
            specifier: import_source.to_string(),
            resolved_canonical_id: Some(resolved_canonical_id.to_string()),
            possible_canonical_ids: vec![resolved_canonical_id.to_string()],
        };

        // import_routes is on DerivedRawState; dependencies is on
        // DependencyState (D48 split). Sidecar field
        // `import_routes_known_miss_recorded_at_generation` is
        // intentionally untouched — see docstring for the
        // architectural invariant.
        {
            let mut derived_ref = self
                .derived_raw_cache()
                .entry(owner_canonical.to_string())
                .or_default();
            derived_ref
                .value_mut()
                .import_routes
                .insert(import_source.to_string(), resolution.clone());
        }
        {
            let mut dep_ref = self
                .dependency_cache()
                .entry(owner_canonical.to_string())
                .or_default();
            dep_ref
                .value_mut()
                .dependencies
                .insert(resolved_canonical_id.to_string());
        }
    }

    fn resolve_workspace_dependency_and_cache(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_workspace::ResolveRequestKind,
    ) -> Option<String> {
        let resolved = self
            .ws()
            .resolve_import(
                owner_canonical,
                import_source,
                verter_workspace::ResolutionContext {
                    phase: verter_workspace::ResolvePhase::CodegenBlocker,
                    kind,
                },
            )?
            .source_id;
        self.cache_positive_import_route_result(owner_canonical, import_source, &resolved);
        Some(resolved)
    }

    pub(crate) fn resolve_loaded_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_workspace::ResolveRequestKind,
    ) -> Option<String> {
        if let Some(existing) = self.authoritative_import_route(owner_canonical, import_source) {
            let cached = if kind == verter_workspace::ResolveRequestKind::TypeImport {
                Self::import_route_target(&existing)
            } else {
                Self::runtime_dependency_target(&existing)
            };
            if let Some(resolved) = cached {
                // For type imports, prefer declaration companion (.d.ts) over
                // runtime files (.js) when both exist.
                if kind == verter_workspace::ResolveRequestKind::TypeImport
                    && Self::runtime_like_dependency_target(&resolved)
                {
                    return Some(self.normalize_live_type_dependency_target(
                        owner_canonical,
                        import_source,
                        &resolved,
                    ));
                }
                return Some(resolved);
            }
            if Self::import_route_is_known_miss(&existing) {
                return None;
            }
        }

        let resolved =
            self.resolve_workspace_dependency_and_cache(owner_canonical, import_source, kind)?;
        // For type imports, normalize through declaration companion preference
        // (.d.ts over .js) when both exist.
        if kind == verter_workspace::ResolveRequestKind::TypeImport
            && Self::runtime_like_dependency_target(&resolved)
        {
            return Some(self.normalize_live_type_dependency_target(
                owner_canonical,
                import_source,
                &resolved,
            ));
        }
        Some(resolved)
    }

    pub(crate) fn resolve_type_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        if let Some(resolved) = self
            .authoritative_import_route(owner_canonical, import_source)
            .and_then(|resolution| {
                self.prefer_type_dependency_target_from_resolution(
                    owner_canonical,
                    import_source,
                    &resolution,
                )
            })
        {
            return Some(resolved);
        }
        if self
            .authoritative_import_route(owner_canonical, import_source)
            .is_some_and(|resolution| Self::import_route_is_known_miss(&resolution))
        {
            return None;
        }

        let type_resolved = self
            .resolve_loaded_dependency_canonical(
                owner_canonical,
                import_source,
                verter_workspace::ResolveRequestKind::TypeImport,
            )
            .map(|resolved| {
                self.normalize_live_type_dependency_target(
                    owner_canonical,
                    import_source,
                    resolved.as_str(),
                )
            })
            .or_else(|| self.fallback_relative_type_companion(owner_canonical, import_source));
        let esm_resolved = type_resolved.as_ref().is_none().then(|| {
            self.resolve_loaded_dependency_canonical(
                owner_canonical,
                import_source,
                verter_workspace::ResolveRequestKind::EsmImport,
            )
        });
        if crate::host_manage::component_meta_debug_enabled() {
            crate::host_manage::component_meta_debug(format!(
                "resolve_type_dependency owner={} import={} type={:?} esm={:?}",
                owner_canonical, import_source, type_resolved, esm_resolved,
            ));
        }
        type_resolved.or(esm_resolved.flatten())
    }

    #[allow(dead_code)]
    pub(crate) fn resolve_type_dependency_canonical_shallow(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        if let Some(existing) = self.authoritative_import_route(owner_canonical, import_source) {
            if let Some(resolved) = self.prefer_type_dependency_target_from_resolution(
                owner_canonical,
                import_source,
                &existing,
            ) {
                return Some(resolved);
            }
            if Self::import_route_is_known_miss(&existing) {
                return None;
            }
        }

        if let Some(resolved) = self.shallow_route_dependency_target(owner_canonical, import_source)
        {
            return Some(resolved);
        }

        let resolved = self
            .ws()
            .resolve_import(
                owner_canonical,
                import_source,
                verter_workspace::ResolutionContext {
                    phase: verter_workspace::ResolvePhase::CodegenBlocker,
                    kind: verter_workspace::ResolveRequestKind::TypeImport,
                },
            )?
            .source_id;
        let resolution = crate::types::DependencyResolution {
            specifier: import_source.to_string(),
            resolved_canonical_id: Some(resolved.clone()),
            possible_canonical_ids: vec![resolved.clone()],
        };
        let preferred = self
            .prefer_type_dependency_target_from_resolution(
                owner_canonical,
                import_source,
                &resolution,
            )
            .unwrap_or(resolved);

        self.cache_positive_import_route_result(owner_canonical, import_source, &preferred);
        Some(preferred)
    }

    /// The single shared route-edge resolution policy: given an owner and an
    /// import/reexport specifier, return the canonical id the type-route layer
    /// resolves it to.
    ///
    /// This is the SOLE specifier→canonical policy for type-route edges. It is
    /// shared by route traversal ([`Self::resolve_route_type_edge`], which
    /// layers on a `.vue` store-view gate + `ensure_loaded` side effects),
    /// shallow-state wildcard/reexport canonicalization (so a route-owned
    /// surface resolves the SAME edges as the indexed surface), and
    /// known-miss revalidation
    /// ([`Self::generation_current_known_miss_resolution`]). Keeping the policy
    /// in one place is what guarantees the recorder and the validator agree on
    /// every route-edge canonical, including the ESM-fallback normalization:
    /// a `TypeImport` resolution normalized
    /// through declaration-companion preference, then the relative runtime
    /// companion, then the `EsmImport` fallback — itself normalized identically
    /// (NOT the raw `source_id`, which is what diverged).
    ///
    /// Side-effect-free beyond the workspace engine's own resolution memo: it
    /// does not load files, materialize artifacts, or write route caches.
    pub(crate) fn resolve_route_edge_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        let normalize_workspace_resolution = |kind: verter_workspace::ResolveRequestKind| {
            self.ws()
                .resolve_import(
                    owner_canonical,
                    import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind,
                    },
                )
                .map(|resolution| {
                    self.normalize_live_type_dependency_target(
                        owner_canonical,
                        import_source,
                        resolution.source_id.as_str(),
                    )
                })
        };

        normalize_workspace_resolution(verter_workspace::ResolveRequestKind::TypeImport)
            .or_else(|| self.fallback_relative_type_companion(owner_canonical, import_source))
            .or_else(|| {
                normalize_workspace_resolution(verter_workspace::ResolveRequestKind::EsmImport)
            })
    }

    /// Side-effect-free type-dependency re-resolve for a known-miss
    /// specifier.
    ///
    /// Answers the same question as
    /// [`Self::resolve_type_dependency_canonical`] — "what canonical id
    /// does this type import resolve to under the current workspace?" —
    /// but is safe to call from a cache-validation / read path. It does
    /// NOT consult [`Self::authoritative_import_route`] (whose
    /// `IndexedReady` fallback can call `ensure_indexed_ready` and
    /// materialize a shallow-only importer) and does NOT call
    /// `cache_positive_import_route_result` (which would rewrite the
    /// `DerivedRawState.import_routes` known-miss entry to a positive and
    /// register a new dependency). Resolution flows straight through the
    /// shared [`Self::resolve_route_edge_canonical`] policy, so the only
    /// state it touches is the workspace engine's own resolution memo.
    ///
    /// Routing through the shared policy keeps the absence-sensitive
    /// `ImportRoute` hash on the SAME canonical the route traversal would
    /// record — including the ESM-fallback normalization — so a re-resolved
    /// known-miss never diverges from the live
    /// route resolution.
    pub(crate) fn generation_current_known_miss_resolution(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        self.resolve_route_edge_canonical(owner_canonical, import_source)
    }
}
