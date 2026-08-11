//! `impl VerterHost` — import-route + dependency canonical resolution.
//!
//! Owns the helpers that drive route resolution / canonical-ID lookup
//! before any external type traversal happens:
//! - `expand_relative_candidates` — pre-snapshot blocker hydration probe.
//! - `authoritative_import_route` + `import_route_target` /
//!   `import_route_is_known_miss` predicates.
//! - `prefer_type_dependency_target_from_resolution`.
//! - `record_resolved_dependency_edge` — the resolved-dependency-EDGE
//!   registrar (reverse-dependency bookkeeping; NOT a route memo).
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
            // `DerivedRawState.import_routes` path — the shared
            // per-entry freshness oracle inside
            // `cached_import_route_resolution` already refused every
            // known-miss and every stale-stamped positive, so anything
            // it returns is current.
            //
            // Per-request audit attribution: classify the warm hit as
            // positive vs negative (known-miss).
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
            // No artifact fallback exists any more: `IndexedReady` is a
            // parse/index artifact and retains no resolved route table.
            // The snapshot fallback that used to live here was
            // stale-by-construction — a known-miss baked at
            // materialisation time survived every dependency appearance,
            // because the owner's own content never moved — and the
            // positives it served depended on the deleted edge-currency
            // stamp to be trustworthy. A caller that misses the
            // caller-supplied table re-resolves through the one
            // owner-edge authority, whose warm candidate is reused when
            // its observation set is unchanged.
            (None, "none")
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

    pub(crate) fn prefer_type_dependency_target_from_resolution(
        &self,
        owner_canonical: &str,
        import_source: &str,
        _resolution: &crate::types::DependencyResolution,
    ) -> verter_workspace::ResolutionPublication<String> {
        self.resolve_for_persistent_state(
            owner_canonical,
            import_source,
            verter_workspace::ResolutionContext {
                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                kind: verter_workspace::ResolveRequestKind::TypeImport,
            },
        )
        .map_result(|resolution| resolution.source_id)
    }

    /// Register a resolved dependency EDGE for `owner_canonical`.
    ///
    /// The host no longer memoises resolved routes: the ONE resolution
    /// memo is the workspace's bounded owner-edge candidate slot, whose
    /// candidates are validated per-reader against a captured immutable
    /// resolution world. A second host-side copy on
    /// `DerivedRawState.import_routes` duplicated that slot, and — being
    /// a plain map with no witness — needed a global
    /// `content_generation` equality stamp to decide whether it was
    /// still true. That stamp was the last global-generation warm-
    /// resolution validity test in the session, so both went together.
    ///
    /// `DerivedRawState.import_routes` is now exclusively the
    /// CALLER-SUPPLIED authoritative table
    /// ([`Self::set_import_dependencies`] — the bundler telling the host
    /// how ITS resolver resolves, re-pushed on the bundler's own watch
    /// events). Its currency rides the workspace exact-resolution table
    /// the same push installs, so a caller-pushed specifier is witnessed
    /// through the `ExactResolution` fact like any other resolution.
    ///
    /// What remains host-owned is the flat dependency EDGE set on
    /// `DependencyState`, which is reverse-dependency bookkeeping rather
    /// than a resolution answer.
    pub(super) fn record_resolved_dependency_edge(
        &self,
        owner_canonical: &str,
        resolved_canonical_id: &str,
    ) {
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

        // Additive derived state that does NOT publish into
        // `FileArtifactStore` and is NOT a content/project/env mutation,
        // so it advances the dedicated `load_generation` dimension.
        // `load_generation` is EXCLUDED from `externally_superseded_by`,
        // so a cold compute that resolves its own import as part of its
        // work does not self-fence its own result promotion.
        //
        // bump-IFF-transition: a no-op re-registration advances NOTHING,
        // so a manager-cached base view stays valid and concurrent
        // callers keep coalescing on one lane.
        if dependency_changed {
            self.bump_load_generation();
        }
    }

    /// Test-only shim exposing the `pub(super)`
    /// [`Self::record_resolved_dependency_edge`] producer to the
    /// crate-root inline test modules (which cannot name a `pub(super)`
    /// item of this submodule).
    #[cfg(test)]
    pub(crate) fn record_resolved_dependency_edge_for_tests(
        &self,
        owner_canonical: &str,
        resolved_canonical_id: &str,
    ) {
        self.record_resolved_dependency_edge(owner_canonical, resolved_canonical_id);
    }

    fn resolve_workspace_dependency(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_workspace::ResolveRequestKind,
    ) -> verter_workspace::ResolutionPublication<String> {
        self.resolve_for_persistent_state(
            owner_canonical,
            import_source,
            verter_workspace::ResolutionContext {
                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                kind,
            },
        )
        .map_result(|resolution| resolution.source_id)
    }

    pub(crate) fn resolve_loaded_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
        kind: verter_workspace::ResolveRequestKind,
    ) -> verter_workspace::ResolutionPublication<String> {
        self.resolve_workspace_dependency(owner_canonical, import_source, kind)
    }

    pub(crate) fn resolve_type_dependency_canonical(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> verter_workspace::ResolutionPublication<String> {
        match self.resolve_workspace_dependency(
            owner_canonical,
            import_source,
            verter_workspace::ResolveRequestKind::TypeImport,
        ) {
            verter_workspace::ResolutionPublication::Admitted(admitted)
                if admitted.result().is_some() =>
            {
                return self.normalize_admitted_type_target(owner_canonical, admitted);
            }
            verter_workspace::ResolutionPublication::Admitted(_) => {}
            verter_workspace::ResolutionPublication::Refused(refusal) => {
                return verter_workspace::ResolutionPublication::Refused(refusal);
            }
        }
        let runtime = match self.resolve_workspace_dependency(
            owner_canonical,
            import_source,
            verter_workspace::ResolveRequestKind::EsmImport,
        ) {
            verter_workspace::ResolutionPublication::Admitted(admitted) => admitted,
            verter_workspace::ResolutionPublication::Refused(refusal) => {
                return verter_workspace::ResolutionPublication::Refused(refusal);
            }
        };
        self.normalize_admitted_type_target(owner_canonical, runtime)
    }

    fn normalize_admitted_type_target(
        &self,
        owner_canonical: &str,
        admitted: verter_workspace::AdmittedResolution<String>,
    ) -> verter_workspace::ResolutionPublication<String> {
        let Some(runtime_target) = admitted.result().cloned() else {
            return verter_workspace::ResolutionPublication::Admitted(admitted);
        };
        match self.resolve_workspace_dependency(
            owner_canonical,
            &runtime_target,
            verter_workspace::ResolveRequestKind::TypeImport,
        ) {
            verter_workspace::ResolutionPublication::Admitted(normalized)
                if normalized.result().is_some() =>
            {
                verter_workspace::ResolutionPublication::Admitted(normalized)
            }
            verter_workspace::ResolutionPublication::Admitted(_) => {
                admitted.replace_result(Some(runtime_target))
            }
            verter_workspace::ResolutionPublication::Refused(refusal) => {
                verter_workspace::ResolutionPublication::Refused(refusal)
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn resolve_type_dependency_canonical_shallow(
        &self,
        owner_canonical: &str,
        import_source: &str,
    ) -> verter_workspace::ResolutionPublication<String> {
        self.resolve_type_dependency_canonical(owner_canonical, import_source)
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
    ) -> verter_workspace::ResolutionPublication<String> {
        verter_audit::attribute_scope!(ImportRouteResolve);
        self.resolve_type_dependency_canonical(owner_canonical, import_source)
    }

    /// Side-effect-free re-resolution for a generation-stale
    /// `import_routes` entry, replaying the entry's RECORDED resolution
    /// lane. Answers "what canonical id does this specifier resolve to
    /// under the current workspace?" on cache-validation / read paths:
    /// it does NOT consult [`Self::authoritative_import_route`] and
    /// registers no dependency edge — the only state it touches is the
    /// workspace engine's own resolution memo.
    ///
    /// `recorded_kind` is the workspace resolution LANE to replay, or
    /// `None` to take the shared type-route policy. Two lanes:
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
    ) -> verter_workspace::ResolutionPublication<String> {
        match recorded_kind {
            Some(verter_workspace::ResolveRequestKind::SfcSrcAttr) => self
                .resolve_for_persistent_state(
                    owner_canonical,
                    import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::SfcSrcAttr,
                    },
                )
                .map_result(|resolution| resolution.source_id),
            _ => self.resolve_route_edge_canonical(owner_canonical, import_source),
        }
    }
}
