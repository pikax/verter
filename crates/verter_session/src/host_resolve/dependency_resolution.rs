//! `impl VerterHost` — import-route + dependency canonical resolution.
//!
//! Owns the helpers that drive route resolution / canonical-ID lookup
//! before any external type traversal happens:
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

use crate::host_manage::component_meta_trace_custom;
use crate::VerterHost;

impl VerterHost {
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
            // route the moment the target file exists. Positive entries
            // ARE served from here: `ensure_indexed_ready_serve`'s reuse is
            // edge-currency-gated, so an artifact whose baked positive
            // edges predate a dependency-set change has already been
            // routed through the edge-refresh (re-resolving its edges
            // against the live file set) before this read.
            let from_indexed = self
                .ensure_indexed_ready_serve(owner_canonical)?
                .indexed
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
        resolution.is_known_miss()
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
        let state = &self
            .ensure_indexed_ready_serve(owner_canonical)?
            .indexed
            .shallow_state;
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
    /// `resolved_at_generation` is the workspace `content_generation` the
    /// CALLER captured BEFORE performing the resolution this call
    /// memoizes. The stamp must never be a live read taken at record
    /// time: a mutation landing between resolve and record would then
    /// forge a "current" stamp onto a possibly-retargeted resolution.
    /// A pre-captured stamp is at worst conservatively stale — the
    /// entry is refused as generation-current and re-resolves.
    ///
    /// `resolved_kind` is the workspace resolution lane the caller
    /// resolved through; it is recorded on the stamp so a
    /// generation-stale entry re-resolves through the SAME lane (exact
    /// resolutions are kind-keyed — replaying a different lane would
    /// diverge recorder from validator).
    pub(super) fn cache_positive_import_route_result(
        &self,
        owner_canonical: &str,
        import_source: &str,
        resolved_canonical_id: &str,
        resolved_at_generation: u64,
        resolved_kind: verter_workspace::ResolveRequestKind,
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
        // Compare-before-insert: a positive route re-admission is a
        // SEMANTIC NO-OP when repeated blocker hydration / concurrent
        // resolution re-resolves the same `(owner, specifier)` to the same
        // canonical (and the dependency edge already exists). Bumping a
        // token dimension on a no-op needlessly invalidates the
        // `StoreViewManager` base snapshot and forks singleflight lanes.
        // The bump fires IFF either the route map or the dependency set
        // actually changes a value the base store view snapshots BY VALUE.
        //
        // The route map is a NO-OP iff a prior entry exists AND resolves
        // to the same canonical. The single writer here always produces
        // `resolved_canonical_id = Some(resolved)` + a singleton
        // `possible_canonical_ids`, so comparing `resolved_canonical_id`
        // fully captures whether the snapshotted `ImportRoute` derived
        // hash would change.
        let route_changed = {
            let mut derived_ref = self
                .derived_raw_cache()
                .entry(owner_canonical.to_string())
                .or_default();
            let derived = derived_ref.value_mut();
            let previous = derived
                .import_routes
                .insert(import_source.to_string(), resolution.clone());
            // Host-memoized positives are dependency-set-derived: stamp
            // the caller-captured pre-resolve generation so readers / the
            // route-surface rebuild re-resolve once the file set moves (a
            // re-admission of the same canonical refreshes the stamp — the
            // route was just revalidated against the file set the caller
            // resolved under). The stamp is NOT part of any snapshotted
            // hash, so refreshing it is not a token-relevant mutation.
            derived
                .import_routes_positive_recorded_at_generation
                .insert(
                    import_source.to_string(),
                    crate::types::PositiveRouteStamp {
                        generation: resolved_at_generation,
                        kind: resolved_kind,
                    },
                );
            previous
                .map(|prev| prev.resolved_canonical_id.as_deref() != Some(resolved_canonical_id))
                .unwrap_or(true)
        };
        let dependency_changed = {
            let mut dep_ref = self
                .dependency_cache()
                .entry(owner_canonical.to_string())
                .or_default();
            // `BTreeSet::insert` returns `true` iff the canonical was
            // newly inserted (i.e. the dependency edge actually changed).
            dep_ref
                .value_mut()
                .dependencies
                .insert(resolved_canonical_id.to_string())
        };

        // A genuine positive route admission mutates
        // `DerivedRawState.import_routes`, which the base store view
        // snapshots BY VALUE (the `ImportRoute` derived-hash domain, via
        // `generation_current_import_route_hash`'s `DerivedRawState`
        // fallback). Like a first-time additive `ensure_loaded`, this is
        // additive derived-state that does NOT publish into
        // `FileArtifactStore` and is NOT a content/project/env mutation,
        // so it advances the dedicated `load_generation` dimension:
        //
        // * It moves the FULL reuse token → a `StoreViewManager`-cached
        //   base view built before the admission is invalidated on the
        //   next request, so warm validation never compares against a
        //   stale `ImportRoute` hash.
        // * `load_generation` is EXCLUDED from `externally_superseded_by`,
        //   so a cold compute that resolves its own import as part of its
        //   work does not self-fence its own result promotion.
        //
        // bump-IFF-transition: a no-op re-admission (same route + existing
        // dependency edge) advances NOTHING, so a manager-cached base view
        // stays valid and concurrent callers keep coalescing on one lane.
        if route_changed || dependency_changed {
            self.bump_load_generation();
        }
    }

    /// Test-only shim exposing the `pub(super)`
    /// [`Self::cache_positive_import_route_result`] producer to the
    /// crate-root inline test modules (which cannot name a `pub(super)`
    /// item of this submodule). Drives the exact positive-route admission
    /// path so the store-view token-advance regression can assert the
    /// admission moves a token dimension.
    #[cfg(test)]
    pub(crate) fn cache_positive_import_route_result_for_tests(
        &self,
        owner_canonical: &str,
        import_source: &str,
        resolved_canonical_id: &str,
        resolved_at_generation: u64,
    ) {
        self.cache_positive_import_route_result(
            owner_canonical,
            import_source,
            resolved_canonical_id,
            resolved_at_generation,
            verter_workspace::ResolveRequestKind::EsmImport,
        );
    }

    fn resolve_workspace_dependency_and_cache(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_workspace::ResolveRequestKind,
    ) -> Option<String> {
        // Capture-before-resolve: the stamp reflects the file set the
        // resolution ran under, never a later one.
        let resolved_at_generation = self.ws().content_generation();
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
        self.cache_positive_import_route_result(
            owner_canonical,
            import_source,
            &resolved,
            resolved_at_generation,
            kind,
        );
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

        // Capture-before-resolve: the stamp reflects the file set the
        // resolution ran under, never a later one.
        let resolved_at_generation = self.ws().content_generation();
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

        self.cache_positive_import_route_result(
            owner_canonical,
            import_source,
            &preferred,
            resolved_at_generation,
            verter_workspace::ResolveRequestKind::TypeImport,
        );
        Some(preferred)
    }

    /// The single shared route-edge resolution policy: given an owner and an
    /// import/reexport specifier, return the canonical id the type-route layer
    /// resolves it to.
    ///
    /// This is the SOLE specifier→canonical policy for type-route edges. It is
    /// shared by route traversal ([`Self::resolve_route_type_edge`], which
    /// layers on a `.vue` store-view gate + `ensure_loaded` side effects),
    /// shallow-state wildcard/reexport canonicalization (so an
    /// overlay-materialised surface resolves the SAME edges as the base
    /// indexed surface), and
    /// stale-entry revalidation
    /// ([`Self::generation_current_route_resolution`], whose type-route
    /// lane delegates here). Keeping the policy
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

    /// Side-effect-free re-resolution for a generation-stale
    /// `import_routes` entry, replaying the entry's RECORDED resolution
    /// lane. Answers "what canonical id does this specifier resolve to
    /// under the current workspace?" on cache-validation / read paths:
    /// it does NOT consult [`Self::authoritative_import_route`] (whose
    /// `IndexedReady` fallback can call `ensure_indexed_ready_serve` and
    /// materialize a shallow-only importer) and does NOT call
    /// `cache_positive_import_route_result` (which would rewrite the
    /// `DerivedRawState.import_routes` entry and register a new
    /// dependency) — the only state it touches is the workspace engine's
    /// own resolution memo.
    ///
    /// `recorded_kind` is the [`crate::types::PositiveRouteStamp::kind`]
    /// of a host-memoized positive, or `None` for a known-miss (whose
    /// sidecar records no lane). Two lanes:
    ///
    /// * **`SfcSrcAttr`** — an external `src=` include: re-resolve through
    ///   the same `SfcSrcAttr` workspace lane the memo was produced
    ///   through. Exact resolutions are keyed `(specifier, phase, kind)`,
    ///   so replaying the type-route chain here would miss the caller's
    ///   `SfcSrcAttr` exact row and diverge validator from recorder; and a
    ///   `src=` target is whole-content-included, so declaration-companion
    ///   normalization does not apply.
    /// * **everything else** (type/ESM-recorded positives and
    ///   known-misses) — the shared
    ///   [`Self::resolve_route_edge_canonical`] type-route policy, so the
    ///   re-resolved canonical agrees with route traversal including the
    ///   ESM-fallback normalization and the absence-sensitive
    ///   `ImportRoute` hash lands on the SAME canonical the route
    ///   traversal would record.
    pub(crate) fn generation_current_route_resolution(
        &self,
        owner_canonical: &str,
        import_source: &str,
        recorded_kind: Option<verter_workspace::ResolveRequestKind>,
    ) -> Option<String> {
        match recorded_kind {
            Some(verter_workspace::ResolveRequestKind::SfcSrcAttr) => self
                .ws()
                .resolve_import(
                    owner_canonical,
                    import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::SfcSrcAttr,
                    },
                )
                .map(|resolution| resolution.source_id),
            _ => self.resolve_route_edge_canonical(owner_canonical, import_source),
        }
    }
}
