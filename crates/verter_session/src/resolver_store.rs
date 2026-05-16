use crate::types::Hash16;
use crate::VerterHost;

/// Per-request component-meta store counters captured by
/// [`VerterHost::component_meta_audit_store_snapshot`]. The fields
/// live on [`crate::component_meta_audit::ComponentMetaPayload`]
/// rather than the generic
/// [`crate::component_meta_audit::RequestStoreAudit`] envelope; this
/// struct is the cross-call carrier between the snapshot site and
/// the audit-builder finalisation.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ComponentMetaStoreCounters {
    pub materialize_structure_calls: u64,
    pub materialize_structure_cache_hits: u64,
    pub node_arena_lock_acquisitions: u64,
    pub family_map_lock_acquisitions: u64,
    pub dep_signature_merges: u64,
    pub dep_signature_intern_hits: u64,
}
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};

// WASM-only: scheduler is unavailable on web; see CLAUDE.md "Scheduler as Sole Compile Authority".

const STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct HostStoreView {
    compat_token: crate::resolver_core::StoreViewCompatToken,
    mutation_epoch: u64,
    session_id: Option<u64>,
    whole_hashes: FxHashMap<String, Hash16>,
    derived_hashes: FxHashMap<(String, crate::resolver_core::DerivedFactKind), Hash16>,
    import_routes: FxHashMap<(String, String), crate::types::DependencyResolution>,
    /// Route-surface-domain snapshot — augmentation-index fingerprints
    /// keyed by a structural representation of the
    /// `(target_kind_tag, target_payload)` shape. Validation against
    /// `RouteSurfaceFactRef::ModuleAugmentationIndexShape` consults
    /// this map (R29 + G1 + R26).
    ///
    /// The key shape mirrors the one the
    /// `FactKey::ModuleAugmentationIndexShape` variant carries; see
    /// [`route_surface_index_key`] for the canonical mapping. An
    /// absent key means the augmentation-index entry has not yet
    /// been populated — the validator returns `false` so the
    /// downstream cache misses.
    route_surface_index_fingerprints: FxHashMap<RouteSurfaceIndexShapeKey, Hash16>,
    /// Parse-domain snapshot (R26): per-canonical `Arc<FileFacts>`
    /// captured at view-build time. The validator for
    /// `ParseFactRef` reads through this map; one `Arc::clone` per
    /// tracked file at build time, wait-free hash compares
    /// thereafter. Files not present in the snapshot are treated as
    /// untracked (validator returns `false` — a path-precise
    /// consumer expected its fact to be in the registry).
    file_facts: FxHashMap<String, std::sync::Arc<crate::file_artifact_store::FileFacts>>,
    /// Resolve-imports-domain handle (R26): `Arc` clone of the
    /// project store's `ResolvedImportFactsDb`. The validator for
    /// `ResolveImportsFactRef` composes
    /// `ResolvedImportFactsKey { canonical, content_hash,
    /// parse_env_hash, resolve_env_hash, resolver_version,
    /// known_miss_generation }` from the fact's `canonical_id`, this
    /// view's tracked `whole_hashes[canonical]`,
    /// `resolved_import_facts_known_miss_tags[canonical]`, and
    /// `env_hashes`, then looks up the matching
    /// `Arc<ResolvedImportFacts>` and compares the per-fact
    /// `semantic_hash` / `display_hash` of the stored
    /// `ResolvedImportClauseEntry.fact` /
    /// `ResolvedReexportBindingEntry.fact` (per `fact.lane`) against
    /// `expected_hash`.
    ///
    /// One `Arc` clone at view-build time; reads thereafter are
    /// wait-free against concurrent writers because `DashMap` shards
    /// per key.
    resolved_import_facts:
        Option<std::sync::Arc<crate::resolved_import_facts::ResolvedImportFactsDb>>,
    /// Per-canonical known-miss generation tag captured at view-build
    /// time. Folds the owner's
    /// `DerivedRawState::import_routes_known_miss_recorded_at_generation`
    /// map through
    /// [`crate::resolved_import_facts::compute_known_miss_generation_tag`]
    /// so the validator composes the same `known_miss_generation`
    /// key dimension the producer
    /// (`admit_resolved_import_facts_for_owner`) admitted under.
    /// Absent entries fall back to `[0u8; 16]` (owners with no
    /// recorded known-misses or canonicals whose route resolution
    /// never ran). Codex P2.2 / Block 1.f-fix.
    resolved_import_facts_known_miss_tags: FxHashMap<String, Hash16>,
    /// Route-surface-domain handle (R26): `Arc` clone of the
    /// project store's `RouteDb`. The validator for
    /// `RouteSurfaceFactRef` with `FactKey::EffectiveExportSet`
    /// composes
    /// `EffectiveExportSetKey { provider_canonical, project_identity,
    /// resolve_env_hash, lib_env_hash }` from the fact's
    /// `canonical_id` plus the view's `project_identity` and
    /// `env_hashes`, then compares the cached entry's
    /// `augmenter_set_fingerprint` to `expected_hash`.
    ///
    /// One `Arc` clone at view-build time; reads thereafter are
    /// wait-free against concurrent writers.
    route_db: Option<std::sync::Arc<crate::resolver_core::route_db::RouteDb>>,
    /// Env-hash bundle (R21) captured at view-build time.
    /// `env_hashes.parse_env_hash` + `env_hashes.resolve_env_hash`
    /// participate in `ResolvedImportFactsKey` composition;
    /// `env_hashes.resolve_env_hash` + `env_hashes.lib_env_hash`
    /// participate in `EffectiveExportSetKey` composition.
    env_hashes: crate::session_view::EnvHashes,
    /// Project identity captured at view-build time. Participates in
    /// `EffectiveExportSetKey` composition (R21).
    project_identity: crate::file_artifact_store::ProjectIdentity,
}

/// Structural key for snapshotting `ModuleAugmentationIndexShape`
/// fingerprints into [`HostStoreView`]. Mirrors the parallel
/// optional fields of `FactKey::ModuleAugmentationIndexShape`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RouteSurfaceIndexShapeKey {
    pub target_kind_tag: verter_semantic::facts::registry::AugmentationTargetKindTag,
    pub external_specifier: Option<String>,
    pub resolved_relative_canonical: Option<String>,
    pub wildcard_pattern: Option<String>,
}

impl Default for HostStoreView {
    fn default() -> Self {
        Self {
            compat_token: crate::resolver_core::StoreViewCompatToken {
                epoch: 0,
                session: None,
            },
            mutation_epoch: 0,
            session_id: None,
            whole_hashes: FxHashMap::default(),
            derived_hashes: FxHashMap::default(),
            import_routes: FxHashMap::default(),
            route_surface_index_fingerprints: FxHashMap::default(),
            file_facts: FxHashMap::default(),
            resolved_import_facts: None,
            resolved_import_facts_known_miss_tags: FxHashMap::default(),
            route_db: None,
            env_hashes: crate::session_view::EnvHashes::default(),
            project_identity: crate::file_artifact_store::ProjectIdentity([0u8; 16]),
        }
    }
}

impl HostStoreView {
    pub(crate) fn from_host(host: &VerterHost) -> Self {
        for _ in 0..STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS {
            let snapshot_epoch = host.current_store_view_epoch();
            let view = Self::build(host, snapshot_epoch, None);
            if host.current_store_view_epoch() == snapshot_epoch {
                return view;
            }
        }

        let snapshot_epoch = host.current_store_view_epoch();
        Self::build(host, snapshot_epoch, None)
    }

    /// Build a session-scoped store view from a raw session id.
    ///
    /// The compat token includes the session identity so that two sessions
    /// with different overlays but the same epoch never coalesce into the
    /// same singleflight lane.
    ///
    /// This entry point replaces an earlier `from_session(view: &SessionView,
    /// host)` overload. The old overload took a session-scoped
    /// `SessionView` epoch carrier; under R17 the per-session
    /// overlay-mutation machinery is gone, so the singleflight
    /// lane identity is the raw `session_id` plumbed through the
    /// caller; the runtime-side epoch carrier no longer exists.
    pub(crate) fn from_session_id(session_id: u64, host: &VerterHost) -> Self {
        for _ in 0..STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS {
            let snapshot_epoch = host.current_store_view_epoch();
            let sv = Self::build(host, snapshot_epoch, Some(session_id));
            if host.current_store_view_epoch() == snapshot_epoch {
                return sv;
            }
        }

        let snapshot_epoch = host.current_store_view_epoch();
        Self::build(host, snapshot_epoch, Some(session_id))
    }

    fn build(host: &VerterHost, snapshot_epoch: u64, session_id: Option<u64>) -> Self {
        let mut view = Self {
            mutation_epoch: snapshot_epoch,
            session_id,
            ..Self::default()
        };

        {
            let mut canonical_ids = host.scheduler.node_ids();
            canonical_ids.extend(host.compile_cache().iter().map(|entry| entry.key().clone()));
            canonical_ids.sort();
            canonical_ids.dedup();

            for canonical_id in canonical_ids {
                if let Some(source) = host.scheduler.try_get_source(&canonical_id) {
                    view.whole_hashes
                        .insert(canonical_id.clone(), source.whole_hash);
                }

                if !view.whole_hashes.contains_key(&canonical_id) {
                    if let Some(state) = host.effective_file_state(&canonical_id, None) {
                        view.whole_hashes
                            .insert(canonical_id.clone(), state.whole_hash);
                    }
                }

                // import_routes lives on DerivedRawState (D48 split).
                // The known-miss generation sidecar (Codex P2.2 /
                // Block 1.f-fix) lives alongside it; capture both
                // under the same `derived_raw_cache().get(...)` so
                // the validator can compose
                // `ResolvedImportFactsKey.known_miss_generation`
                // identically to the producer.
                if let Some(entry) = host.derived_raw_cache().get(&canonical_id) {
                    for (specifier, resolution) in entry.import_routes.iter() {
                        view.import_routes.insert(
                            (canonical_id.clone(), specifier.clone()),
                            resolution.clone(),
                        );
                    }
                    let tag = crate::resolved_import_facts::compute_known_miss_generation_tag(
                        &entry.import_routes_known_miss_recorded_at_generation,
                    );
                    view.resolved_import_facts_known_miss_tags
                        .insert(canonical_id.clone(), tag);
                }
            }
        }

        // WASM-only: scheduler is unavailable on web; see CLAUDE.md "Scheduler as Sole Compile Authority".

        // Snapshot FileArtifactStore entries into the store view.
        for (canonical_id, indexed) in host.project_type_store.indexed().snapshot_all() {
            let canonical_str = canonical_id.as_ref().to_owned();
            view.whole_hashes
                .entry(canonical_str.clone())
                .or_insert(indexed.whole_hash);
            // Insert Route fact from shallow state.
            if indexed.shallow_state.has_resolvable_surface() {
                view.derived_hashes.insert(
                    (
                        canonical_str.clone(),
                        crate::resolver_core::DerivedFactKind::Route,
                    ),
                    hash_route_surface(&indexed.shallow_state),
                );
            }
            if let Some(hash) = indexed.import_route_hash {
                view.derived_hashes.insert(
                    (
                        canonical_str,
                        crate::resolver_core::DerivedFactKind::ImportRoute,
                    ),
                    hash,
                );
            }
        }

        // TODO(follow-up): substrate-level fix in
        // `HostStoreView::build` — skip the route_owned_shallow
        // snapshot for canonicals that already have an `IndexedReady`
        // entry. The materialiser at `route_owned_shallow.rs:188-193`
        // aborts NEW publishes when `IndexedReady` exists, but
        // pre-existing route_owned_shallow entries persist and
        // overwrite the indexed Route hash that the loop above
        // inserted. A cold-compute that observes the indexed Route
        // hash via `accumulate_route_fact_for`
        // (`frontier_engine.rs:489-527` — `route_shallow_cache` /
        // `route_shallow_state` lookup) records a hash the
        // validator's later view-build cannot reproduce
        // (route-owned-derived hash differs).
        //
        // The component-meta final-result cache no longer trips this:
        // its signature is sourced from the finalised fact tracer
        // read set, which observes route surfaces as `RouteSurface`
        // facts (validated against `RouteDb`, not the dual-source
        // `derived_hashes`). Any cache that places a
        // `DerivedFactHash{Route}` produced by the curated
        // `current_dependency_fact_versions` path into its signature
        // still needs care here until the substrate site is fixed.
        // Defer to Block 6.B (which retires legacy `dep_signature`).
        for snapshot in host.snapshot_route_owned_shallow_cache_entries() {
            let tracked_whole_hash = *view
                .whole_hashes
                .entry(snapshot.canonical_id.clone())
                .or_insert(snapshot.whole_hash);
            if tracked_whole_hash == snapshot.whole_hash {
                if let Some(route_hash) = snapshot.route_hash {
                    view.derived_hashes.insert(
                        (
                            snapshot.canonical_id.clone(),
                            crate::resolver_core::DerivedFactKind::Route,
                        ),
                        route_hash,
                    );
                }
            }
        }

        view.snapshot_tracked_import_route_hashes(host);
        view.snapshot_augmentation_index(host.project_type_store.indexed());
        view.snapshot_file_facts(host.project_type_store.indexed());
        // R26 per-domain producer handles captured at view-build
        // time. Cheap `Arc::clone` per snapshot; reads through the
        // handles are wait-free against concurrent writers because
        // both `ResolvedImportFactsDb` and `RouteDb` shard by key
        // (DashMap-backed).
        view.resolved_import_facts = Some(std::sync::Arc::clone(
            host.project_type_store.resolved_import_facts_handle(),
        ));
        view.route_db = Some(host.project_type_store.routes_handle());
        // R21 env-hash + project-identity capture. Required for
        // `ResolvedImportFactsKey` + `EffectiveExportSetKey`
        // composition inside the per-domain validators.
        view.env_hashes = host.host_view_env_hashes();
        view.project_identity = host.host_view_project_identity();
        view.compat_token = view.compute_compat_token();
        view
    }

    /// Snapshot `Arc<FileFacts>` per canonical from the indexed
    /// store. One refcount bump per tracked file at view-build time;
    /// parse-domain validation reads through these handles
    /// wait-free against concurrent writers because each entry is
    /// immutable.
    ///
    /// If multiple `(content_hash, parse_env_hash)` variants coexist
    /// for one canonical (the multi-candidate cache shape under R20),
    /// the first one encountered wins — subsequent variants do not
    /// overwrite. The view's `whole_hashes` map records the canonical
    /// content hash; a path-precise consumer that observed against
    /// a parse-env-hash variant outside this snapshot will miss
    /// validation and recompute against the current variant.
    fn snapshot_file_facts(&mut self, store: &crate::file_artifact_store::FileArtifactStore) {
        // Snapshot ONLY the `FileFacts` variant whose `content_hash`
        // matches the view's tracked `whole_hashes[canonical]` —
        // that is the source-of-truth content hash for the
        // canonical under this view. Other variants (stale
        // candidates from prior content generations) coexist in
        // the multi-candidate store per R20 but must NOT back the
        // parse-domain validator: a path-precise consumer observed
        // against the live content, so its validation MUST consult
        // the live content's facts.
        //
        // When the artifact store has not yet been refreshed for
        // the new content (lazy `ensure_indexed_ready` has not run
        // yet), the `file_facts` entry for that canonical stays
        // ABSENT. The parse-domain validator interprets absence as
        // a miss (`validates_parse_domain` returns `false` for any
        // observed real-hash fact under an absent entry) — the
        // consumer falls through to cold recompute, which is the
        // correct R3 outcome under stale producer state.
        for (key, artifacts) in store.snapshot_artifacts() {
            let canonical_str = key.canonical.as_ref().to_owned();
            let matches_live = match self.whole_hashes.get(&canonical_str) {
                Some(h) => key.content_hash == *h,
                None => false,
            };
            if matches_live {
                self.file_facts
                    .insert(canonical_str, std::sync::Arc::clone(&artifacts.facts));
            }
        }
    }

    fn snapshot_tracked_import_route_hashes(&mut self, host: &VerterHost) {
        let canonical_ids: Vec<String> = self.whole_hashes.keys().cloned().collect();
        let empty_import_routes = FxHashMap::default();
        let empty_import_route_hash = hash_import_route_targets(&empty_import_routes);

        for canonical_id in canonical_ids {
            if self.derived_hashes.contains_key(&(
                canonical_id.clone(),
                crate::resolver_core::DerivedFactKind::ImportRoute,
            )) {
                continue;
            }

            let import_route_hash = {
                {
                    // import_routes lives on DerivedRawState (D48 split).
                    host.derived_raw_cache()
                        .get(&canonical_id)
                        .and_then(|entry| {
                            (!entry.import_routes.is_empty())
                                .then(|| hash_import_route_targets(&entry.import_routes))
                        })
                }

                // WASM-only: scheduler is unavailable on web; see CLAUDE.md "Scheduler as Sole Compile Authority".
            };

            self.derived_hashes.insert(
                (
                    canonical_id.clone(),
                    crate::resolver_core::DerivedFactKind::ImportRoute,
                ),
                import_route_hash.unwrap_or(empty_import_route_hash),
            );
        }
    }

    /// Snapshot the augmentation-index fingerprints from a
    /// [`FileArtifactStore`] into this view (R29 + G1). Called by
    /// `build` when the host's project-type-store is reachable, and
    /// directly from tests that construct a view over a standalone
    /// `FileArtifactStore`.
    pub(crate) fn snapshot_augmentation_index(
        &mut self,
        artifact_store: &crate::file_artifact_store::FileArtifactStore,
    ) {
        for (key, fingerprint) in artifact_store.snapshot_augmentation_index_fingerprints() {
            let snap_key = RouteSurfaceIndexShapeKey {
                target_kind_tag: augmentation_target_kind_tag_for(&key.target),
                external_specifier: augmentation_target_external_specifier(&key.target),
                resolved_relative_canonical: augmentation_target_resolved_relative_canonical(
                    &key.target,
                ),
                wildcard_pattern: augmentation_target_wildcard_pattern(&key.target),
            };
            self.route_surface_index_fingerprints
                .insert(snap_key, fingerprint);
        }
    }

    pub(crate) fn mutation_epoch(&self) -> u64 {
        self.mutation_epoch
    }

    #[allow(dead_code)]
    pub(crate) fn whole_hash(&self, canonical_id: &str) -> Option<Hash16> {
        self.whole_hashes.get(canonical_id).copied()
    }

    #[allow(dead_code)]
    pub(crate) fn derived_hash(
        &self,
        canonical_id: &str,
        kind: crate::resolver_core::DerivedFactKind,
    ) -> Option<Hash16> {
        self.derived_hashes
            .get(&(canonical_id.to_string(), kind))
            .copied()
    }

    pub(crate) fn invalid_fact_details(
        &self,
        facts: &[crate::resolver_core::FactVersionRef],
        limit: usize,
    ) -> Vec<String> {
        facts
            .iter()
            .filter_map(|fact| self.describe_invalid_fact(fact))
            .take(limit)
            .collect()
    }

    fn describe_invalid_fact(&self, fact: &crate::resolver_core::FactVersionRef) -> Option<String> {
        match fact {
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, hash } => {
                match self.whole_hashes.get(canonical_id) {
                    Some(current) if current == hash => None,
                    Some(current) => Some(format!(
                        "FileWholeHash mismatch canonical={} expected={hash:?} actual={current:?}",
                        canonical_id
                    )),
                    None => Some(format!(
                        "FileWholeHash missing canonical={} expected={hash:?}",
                        canonical_id
                    )),
                }
            }
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind,
                hash,
            } => {
                let current = match kind {
                    crate::resolver_core::DerivedFactKind::DirectSource => {
                        self.whole_hashes.get(canonical_id)
                    }
                    _ => self.derived_hashes.get(&(canonical_id.clone(), *kind)),
                };
                match current {
                    Some(current) if current == hash => None,
                    Some(current) => Some(format!(
                        "DerivedFactHash mismatch canonical={} kind={kind:?} expected={hash:?} actual={current:?}",
                        canonical_id
                    )),
                    None => Some(format!(
                        "DerivedFactHash missing canonical={} kind={kind:?} expected={hash:?}",
                        canonical_id
                    )),
                }
            }
            // R26 per-domain variants — per-domain producers populate
            // the matching stores and produce structured diagnostics
            // there. `HostStoreView` does not observe them directly,
            // so the diagnostic shape is a generic "domain fact not
            // validated yet" string.
            crate::resolver_core::FactVersionRef::Parse(p) => Some(format!(
                "ParseFactRef canonical={} key={:?} lane={:?} expected={:?}",
                p.canonical_id, p.key, p.lane, p.expected_hash
            )),
            crate::resolver_core::FactVersionRef::ResolveImports(r) => Some(format!(
                "ResolveImportsFactRef canonical={} key={:?} lane={:?} expected={:?}",
                r.canonical_id, r.key, r.lane, r.expected_hash
            )),
            crate::resolver_core::FactVersionRef::RouteSurface(r) => Some(format!(
                "RouteSurfaceFactRef canonical={} key={:?} lane={:?} expected={:?}",
                r.canonical_id, r.key, r.lane, r.expected_hash
            )),
        }
    }

    fn compute_compat_token(&self) -> crate::resolver_core::StoreViewCompatToken {
        crate::resolver_core::StoreViewCompatToken {
            epoch: self.mutation_epoch,
            session: self.session_id,
        }
    }
}

pub(crate) fn hash_import_route_targets(
    resolutions: &FxHashMap<String, crate::types::DependencyResolution>,
) -> Hash16 {
    let mut entries: Vec<_> = resolutions.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    hash16_from_sorted(|hasher| {
        for (specifier, resolution) in &entries {
            0u8.hash(hasher);
            specifier.hash(hasher);
            resolution
                .resolved_canonical_id
                .clone()
                .or_else(|| resolution.effective_target().map(str::to_string))
                .hash(hasher);
        }
    })
}

pub(crate) fn hash_route_surface(state: &crate::resolver_core::ShallowFileState) -> Hash16 {
    hash16_from_sorted(|hasher| {
        // Hash sorted export names.
        let mut export_names: Vec<&str> = state.exports.keys().map(|s| s.as_str()).collect();
        export_names.sort_unstable();
        for name in &export_names {
            name.hash(hasher);
        }

        // Hash wildcard reexport source specifiers in declaration order.
        for wildcard in &state.wildcard_reexports {
            wildcard.source_specifier.hash(hasher);
            wildcard.canonical_id.hash(hasher);
        }

        // Hash the file content hash.
        state.whole_hash.hash(hasher);
    })
}

fn hash16_from_sorted(f: impl Fn(&mut rustc_hash::FxHasher)) -> Hash16 {
    let mut left = rustc_hash::FxHasher::default();
    0u8.hash(&mut left);
    f(&mut left);

    let mut right = rustc_hash::FxHasher::default();
    1u8.hash(&mut right);
    f(&mut right);

    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&left.finish().to_le_bytes());
    out[8..].copy_from_slice(&right.finish().to_le_bytes());
    out
}

/// Map an [`AugmentationTargetKind`] into the parallel-fields shape
/// the parse-domain [`FactKey::ModuleAugmentationIndexShape`] +
/// audit-event variants use.
pub(crate) fn augmentation_target_kind_tag_for(
    target: &crate::file_artifact_store::AugmentationTargetKind,
) -> verter_semantic::facts::registry::AugmentationTargetKindTag {
    use crate::file_artifact_store::AugmentationTargetKind;
    use verter_semantic::facts::registry::AugmentationTargetKindTag;
    match target {
        AugmentationTargetKind::ExternalSpecifier(_) => {
            AugmentationTargetKindTag::ExternalSpecifier
        }
        AugmentationTargetKind::ResolvedRelativeCanonical(_) => {
            AugmentationTargetKindTag::ResolvedRelativeCanonical
        }
        AugmentationTargetKind::WildcardAmbient(_) => AugmentationTargetKindTag::WildcardAmbient,
        AugmentationTargetKind::GlobalAugmentation => AugmentationTargetKindTag::GlobalAugmentation,
    }
}

pub(crate) fn augmentation_target_external_specifier(
    target: &crate::file_artifact_store::AugmentationTargetKind,
) -> Option<String> {
    use crate::file_artifact_store::AugmentationTargetKind;
    match target {
        AugmentationTargetKind::ExternalSpecifier(spec) => Some(spec.as_ref().to_owned()),
        _ => None,
    }
}

pub(crate) fn augmentation_target_resolved_relative_canonical(
    target: &crate::file_artifact_store::AugmentationTargetKind,
) -> Option<String> {
    use crate::file_artifact_store::AugmentationTargetKind;
    match target {
        AugmentationTargetKind::ResolvedRelativeCanonical(canon) => Some(canon.as_ref().to_owned()),
        _ => None,
    }
}

pub(crate) fn augmentation_target_wildcard_pattern(
    target: &crate::file_artifact_store::AugmentationTargetKind,
) -> Option<String> {
    use crate::file_artifact_store::AugmentationTargetKind;
    match target {
        AugmentationTargetKind::WildcardAmbient(pat) => Some(pat.as_ref().to_owned()),
        _ => None,
    }
}

impl crate::resolver_core::StoreView for HostStoreView {
    fn compat_token(&self) -> crate::resolver_core::StoreViewCompatToken {
        self.compat_token
    }

    fn validates(&self, fact: &crate::resolver_core::FactVersionRef) -> bool {
        match fact {
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, hash } => {
                match self.whole_hashes.get(canonical_id) {
                    Some(current) => current == hash,
                    // File not tracked by this store view — it was loaded as a
                    // dependency AFTER the view snapshot was taken. Accept it:
                    // the facts were just materialized from current disk/workspace
                    // state and are valid. This avoids forcing every dependency
                    // access through the expensive permissive fallback path in
                    // `ensure_indexed_ready`.
                    None => true,
                }
            }
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind,
                hash,
            } => match kind {
                crate::resolver_core::DerivedFactKind::DirectSource => {
                    match self.whole_hashes.get(canonical_id) {
                        Some(current) => current == hash,
                        // Untracked dependency file — accept (same reasoning
                        // as FileWholeHash above).
                        None => true,
                    }
                }
                _ => self
                    .derived_hashes
                    .get(&(canonical_id.clone(), *kind))
                    .is_some_and(|current| current == hash),
            },
            // R26 per-domain variants — route to the per-domain
            // validators. `HostStoreView` participates in the
            // legacy whole-hash regime today; the per-domain
            // validators are populated by their respective
            // producers. Default impls (returning `false`) are
            // inherited from the trait until per-domain producers
            // wire actual validation through this view.
            // R26 per-domain variants — route to the per-domain
            // validators (which return `false` by trait default;
            // per-domain producers override).
            crate::resolver_core::FactVersionRef::Parse(p) => {
                crate::resolver_core::StoreView::validates_parse_domain(self, p)
            }
            crate::resolver_core::FactVersionRef::ResolveImports(r) => {
                crate::resolver_core::StoreView::validates_resolve_imports_domain(self, r)
            }
            crate::resolver_core::FactVersionRef::RouteSurface(r) => {
                crate::resolver_core::StoreView::validates_route_surface_domain(self, r)
            }
        }
    }

    fn tracks_file(&self, canonical_id: &str) -> bool {
        self.whole_hashes.contains_key(canonical_id)
    }

    /// Parse-domain validator (R26).
    ///
    /// Look up `fact.key` against the file's `FileFacts` registry and
    /// compare the stored fact's `semantic_hash` / `display_hash`
    /// (per `fact.lane`) to the observed `expected_hash`. The lookup
    /// resolves the current `FileArtifacts` for `canonical_id` from
    /// the project type store; the view snapshot's `whole_hashes`
    /// already pins the parse-env-hash slice the artifacts derive
    /// from, so this read is wait-free against concurrent writers.
    ///
    /// `None` outcomes — file untracked, artifacts absent, key not
    /// in registry — all signal "no longer there", which under R3
    /// must invalidate the consumer's warm hit. The validator
    /// therefore returns `false` rather than the optimistic-accept
    /// shape used for `FileWholeHash` untracked files: a path-precise
    /// `Member`/`MemberPresence` consumer expects the fact to BE in
    /// the registry it recorded, so absence is a discriminating miss.
    fn validates_parse_domain(&self, fact: &crate::resolver_core::ParseFactRef) -> bool {
        const ZERO_HASH: Hash16 = [0u8; 16];
        let facts = match self.file_facts.get(fact.canonical_id.as_str()) {
            Some(f) => f,
            // Untracked file — accept if the observed hash was the
            // zero sentinel (producer saw the file as unavailable
            // and recorded the sentinel; absence is consistent).
            // Otherwise reject — the consumer observed a real fact
            // hash but the file has dropped out of the index.
            None => return fact.expected_hash == ZERO_HASH,
        };
        match facts.lookup(&fact.key) {
            Some(stored) => {
                let stored_hash = match fact.lane {
                    verter_semantic::facts::registry::FactLane::Semantic => stored.semantic_hash,
                    verter_semantic::facts::registry::FactLane::Display => stored.display_hash,
                };
                stored_hash == fact.expected_hash
            }
            // Fact absent in registry — accept iff observed was the
            // zero sentinel (consistent absence — see
            // `fact_signature_helpers::parse_fact_ref`).
            None => fact.expected_hash == ZERO_HASH,
        }
    }

    /// Resolve-imports-domain validator (R26).
    ///
    /// Compose `ResolvedImportFactsKey { canonical, content_hash,
    /// parse_env_hash, resolve_env_hash, resolver_version,
    /// known_miss_generation }` from the fact's `canonical_id`, the
    /// view's tracked `whole_hashes[canonical]`,
    /// `resolved_import_facts_known_miss_tags[canonical]`, and the
    /// view's `env_hashes`. Look up the matching
    /// `Arc<ResolvedImportFacts>` from the captured
    /// `ResolvedImportFactsDb` handle and compare the per-binding
    /// `semantic_hash` / `display_hash` (per `fact.lane`) of the
    /// matching `ResolvedImportClauseEntry` or
    /// `ResolvedReexportBindingEntry` against `expected_hash`.
    ///
    /// Outcomes:
    /// - Handle missing (view built without a resolved-import-facts
    ///   snapshot) → reject. A consumer that observed a real fact
    ///   under no producer is a bug; the caller falls back to cold
    ///   compute, which will re-emit through the producer.
    /// - File untracked under the view (no `whole_hashes[canonical]`
    ///   entry) → accept the optimistic content-hash sentinel
    ///   (`expected_hash == ZERO_HASH`); reject any real fact hash
    ///   for an untracked file (same shape as
    ///   `validates_parse_domain`).
    /// - Cache slot absent for the composed key → reject. The cache
    ///   was the recording site; absence means the consumer
    ///   observed a stale slice.
    /// - Binding present and hash matches → accept; hash differs →
    ///   reject (cosmetic-only edit invalidates display-lane
    ///   consumers but not semantic-lane consumers, per the lane
    ///   discriminator).
    fn validates_resolve_imports_domain(
        &self,
        fact: &crate::resolver_core::ResolveImportsFactRef,
    ) -> bool {
        use verter_semantic::facts::registry::FactLane;
        use verter_semantic::facts::FactKey;
        const ZERO_HASH: Hash16 = [0u8; 16];

        let facts_db = match self.resolved_import_facts.as_ref() {
            Some(db) => db,
            None => return false,
        };

        // R26 producer: untracked-file optimistic-accept window. A
        // path-precise resolve-imports consumer that observed against
        // a sentinel hash (`ZERO_HASH`) means "this file produced no
        // value at observation time"; accept that observation against
        // an untracked file (still produces no value).
        let content_hash = match self.whole_hashes.get(fact.canonical_id.as_str()) {
            Some(h) => *h,
            None => return fact.expected_hash == ZERO_HASH,
        };

        // `known_miss_generation` (Codex P2.2 / Block 1.f-fix):
        // captured at view-build time from
        // `DerivedRawState::import_routes_known_miss_recorded_at_generation`.
        // Absent entries → `[0u8; 16]` so an owner that never had
        // `set_import_dependencies` called still composes the same
        // key value the producer admitted under (the producer also
        // reads `[0u8; 16]` when there is no `DerivedRawState`
        // entry yet).
        let known_miss_generation = self
            .resolved_import_facts_known_miss_tags
            .get(fact.canonical_id.as_str())
            .copied()
            .unwrap_or(ZERO_HASH);

        let key = crate::resolved_import_facts::ResolvedImportFactsKey {
            canonical: std::sync::Arc::from(fact.canonical_id.as_str()),
            content_hash,
            parse_env_hash: self.env_hashes.parse_env_hash,
            resolve_env_hash: self.env_hashes.resolve_env_hash,
            resolver_version: crate::resolved_import_facts::RESOLVED_IMPORT_FACTS_RESOLVER_VERSION,
            known_miss_generation,
        };

        let facts = match facts_db.get(&key) {
            Some(f) => f,
            // Cache slot absent — the consumer observed a real fact
            // hash but the resolve-imports producer has not yet
            // populated the entry under this view. Reject so the
            // caller recomputes through the producer (which will
            // populate the cache + re-emit).
            None => return fact.expected_hash == ZERO_HASH,
        };

        // Pick the lane that the consumer observed under.
        let pick_lane = |f: &std::sync::Arc<verter_semantic::facts::registry::Fact>| match fact.lane
        {
            FactLane::Semantic => f.semantic_hash,
            FactLane::Display => f.display_hash,
        };

        match &fact.key {
            FactKey::ResolvedImportClause {
                specifier,
                binding,
                space,
                resolved_canonical,
                resolved_source_name,
            } => facts.import_clauses.iter().any(|entry| {
                entry.specifier == *specifier
                    && entry.binding == *binding
                    && entry.space == *space
                    && entry.resolved_canonical.as_ref().map(|c| c.as_ref())
                        == Some(resolved_canonical.as_ref())
                    && entry.resolved_source_name == *resolved_source_name
                    && pick_lane(&entry.fact) == fact.expected_hash
            }),
            FactKey::ResolvedReexportBinding {
                specifier,
                source_name,
                target_name,
                space,
                resolved_canonical,
                resolved_source_name,
            } => facts.reexport_bindings.iter().any(|entry| {
                entry.specifier == *specifier
                    && entry.source_name == *source_name
                    && entry.target_name == *target_name
                    && entry.space == *space
                    && entry.resolved_canonical.as_ref().map(|c| c.as_ref())
                        == Some(resolved_canonical.as_ref())
                    && entry.resolved_source_name == *resolved_source_name
                    && pick_lane(&entry.fact) == fact.expected_hash
            }),
            // Non-resolve-imports FactKey shapes do not belong to
            // the resolve-imports domain. The dispatch layer routes
            // by `FactDomain` so this arm is defensive.
            _ => false,
        }
    }

    /// Route-surface-domain validator (R26 + R29 + G1).
    ///
    /// `ModuleAugmentationIndexShape` → consult the snapshot of
    /// augmentation-index fingerprints captured at view-build time
    /// (R29 / G1 producer state).
    ///
    /// `EffectiveExportSet` → compose
    /// `EffectiveExportSetKey { provider_canonical,
    /// project_identity, resolve_env_hash, lib_env_hash }` from the
    /// fact's `canonical_id` plus the view's `project_identity` +
    /// `env_hashes`, look up the cached entry in the captured
    /// `RouteDb` handle, and compare the entry's
    /// `augmenter_set_fingerprint` to `fact.expected_hash`.
    fn validates_route_surface_domain(
        &self,
        fact: &crate::resolver_core::RouteSurfaceFactRef,
    ) -> bool {
        use verter_semantic::facts::FactKey;
        match &fact.key {
            FactKey::ModuleAugmentationIndexShape {
                target_kind_tag,
                external_specifier,
                resolved_relative_canonical,
                wildcard_pattern,
            } => {
                let key = RouteSurfaceIndexShapeKey {
                    target_kind_tag: *target_kind_tag,
                    external_specifier: external_specifier.as_ref().map(|s| s.as_ref().to_owned()),
                    resolved_relative_canonical: resolved_relative_canonical
                        .as_ref()
                        .map(|s| s.as_ref().to_owned()),
                    wildcard_pattern: wildcard_pattern.as_ref().map(|s| s.as_ref().to_owned()),
                };
                match self.route_surface_index_fingerprints.get(&key) {
                    Some(current) => current == &fact.expected_hash,
                    // Absent from the snapshot — the augmentation
                    // index has not been populated under this view.
                    // Refuse the candidate so the consumer recomputes
                    // through the cold path (which will populate the
                    // index).
                    None => false,
                }
            }
            FactKey::EffectiveExportSet => {
                let route_db = match self.route_db.as_ref() {
                    Some(db) => db,
                    None => return false,
                };
                // Compose the `EffectiveExportSetKey` from the fact's
                // `canonical_id` (provider) + view env. Then walk the
                // cache slot for `provider_canonical`; we cannot call
                // `get_effective_export_set(_, view)` here because we
                // ARE the view — that would recurse on validation.
                // Permissive cache-state snapshot via `snapshot_all`
                // is acceptable: the validator only needs to find a
                // candidate whose `augmenter_set_fingerprint` matches
                // the consumer's `expected_hash` under the matching
                // `(provider, project, resolve_env, lib_env)`
                // quadruple.
                let target_key = crate::resolver_core::route_db::EffectiveExportSetKey {
                    provider_canonical: fact.canonical_id.clone(),
                    project_identity: self.project_identity,
                    resolve_env_hash: self.env_hashes.resolve_env_hash,
                    lib_env_hash: self.env_hashes.lib_env_hash,
                };
                route_db.lookup_effective_export_set_fingerprint(&target_key)
                    == Some(fact.expected_hash)
            }
            // Other parse-domain / resolve-domain keys do not belong
            // to the route-surface domain; the dispatch layer guards
            // against this so the match is exhaustive defensively.
            _ => false,
        }
    }
}

impl crate::resolver_core::ResolverStore for VerterHost {
    type View = HostStoreView;

    fn snapshot_view(&self) -> Self::View {
        self.resolver_store_view()
    }
}

impl VerterHost {
    pub(crate) fn resolver_store_view(&self) -> HostStoreView {
        HostStoreView::from_host(self)
    }

    pub(crate) fn component_meta_audit_store_snapshot(
        &self,
        store_view: Option<&HostStoreView>,
    ) -> (
        crate::component_meta_audit::RequestStoreAudit,
        ComponentMetaStoreCounters,
    ) {
        let indexed_entries = self.project_type_store.indexed().len() as u32;
        let indexed_bytes = self
            .project_type_store
            .indexed()
            .snapshot_all()
            .iter()
            .map(|(id, indexed)| {
                id.len() as u64 + indexed.raw_source.len() as u64 + indexed.eval_source.len() as u64
            })
            .sum::<u64>();

        let prepared_bundles = self
            .resolver_runtime()
            .prepared_decl_bundles
            .cached_values();
        let prepared_type_decls = prepared_bundles.iter().fold(0u32, |count, bundle| {
            count.saturating_add(bundle.prepared_type_decls.len() as u32)
        });
        let prepared_value_decls = prepared_bundles.iter().fold(0u32, |count, bundle| {
            count.saturating_add(bundle.prepared_value_decls.len() as u32)
        });

        // Pull per-request materialiser/storage counters off the
        // active `RequestContext` (zero ops when no context is
        // installed; the audit pipeline always installs one before
        // taking this snapshot). These counters move into the
        // component-meta payload — they are kind-specific and do
        // not belong on the generic `RequestStoreAudit`.
        let component_meta_counters = match crate::request_context::current_request_context() {
            Some(ctx) => ComponentMetaStoreCounters {
                materialize_structure_calls: ctx
                    .materialize_structure_calls
                    .load(std::sync::atomic::Ordering::Relaxed),
                materialize_structure_cache_hits: ctx
                    .materialize_structure_cache_hits
                    .load(std::sync::atomic::Ordering::Relaxed),
                node_arena_lock_acquisitions: ctx
                    .node_arena_lock_acquisitions
                    .load(std::sync::atomic::Ordering::Relaxed),
                family_map_lock_acquisitions: ctx
                    .family_map_lock_acquisitions
                    .load(std::sync::atomic::Ordering::Relaxed),
                dep_signature_merges: ctx
                    .dep_signature_merges
                    .load(std::sync::atomic::Ordering::Relaxed),
                dep_signature_intern_hits: ctx
                    .dep_signature_intern_hits
                    .load(std::sync::atomic::Ordering::Relaxed),
            },
            None => ComponentMetaStoreCounters::default(),
        };

        let store_audit = crate::component_meta_audit::RequestStoreAudit {
            store_view_hits: u32::from(store_view.is_some()),
            store_view_misses: u32::from(store_view.is_none()),
            structural_merges: 0,
            imported_dependency_entries: indexed_entries,
            imported_dependency_bytes: indexed_bytes,
            prepared_type_decls,
            prepared_value_decls,
            cache_layers: Default::default(),
        };
        (store_audit, component_meta_counters)
    }

    pub(crate) fn component_meta_audit_memory_bytes(&self) -> (u64, u64) {
        let host_cache_bytes: u64 = self
            .project_type_store
            .indexed()
            .snapshot_all()
            .iter()
            .map(|(id, indexed)| {
                id.len() as u64 + indexed.raw_source.len() as u64 + indexed.eval_source.len() as u64
            })
            .sum();

        let workspace = self.workspace();
        let workspace_snapshot = workspace.resource_snapshot();
        let workspace_bytes = workspace_snapshot.overlay_bytes + workspace_snapshot.snapshot_bytes;

        (host_cache_bytes, workspace_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::hash_import_route_targets;
    use crate::types::DependencyResolution;
    use rustc_hash::FxHashMap;

    use crate::resolver_core::StoreView;

    /// Files loaded as dependencies DURING resolution (after the store view
    /// snapshot was taken) are not tracked in `whole_hashes`. The validated
    /// cache must accept facts for these untracked files — otherwise every
    /// access to a dependency falls through to the expensive permissive path.
    #[test]
    fn validates_accepts_untracked_file_whole_hash() {
        let view = super::HostStoreView {
            mutation_epoch: 1,
            whole_hashes: FxHashMap::from_iter([("/src/Accordion.vue".to_string(), [1u8; 16])]),
            ..Default::default()
        };

        // Tracked file with matching hash — should validate.
        assert!(
            view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: "/src/Accordion.vue".to_string(),
                hash: [1u8; 16],
            })
        );

        // Tracked file with mismatching hash — should reject.
        assert!(
            !view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: "/src/Accordion.vue".to_string(),
                hash: [2u8; 16],
            })
        );

        // Untracked dependency file — should accept (loaded after view snapshot).
        assert!(
            view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: "/node_modules/vue/dist/vue.d.mts".to_string(),
                hash: [42u8; 16],
            }),
            "untracked dependency files should be accepted by the store view"
        );
    }

    /// DerivedFactHash::DirectSource for untracked files should be accepted
    /// (same as FileWholeHash — it's a content-hash alias). Non-DirectSource
    /// derived facts for untracked files should NOT be accepted — they are
    /// invalidation signals (import routes, etc.) that must be explicitly
    /// tracked to participate in validation.
    #[test]
    fn validates_derived_fact_hash_semantics() {
        let view = super::HostStoreView {
            mutation_epoch: 1,
            whole_hashes: FxHashMap::default(),
            ..Default::default()
        };

        // DirectSource for untracked file — should accept (content-hash alias).
        assert!(
            view.validates(&crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: "/node_modules/reka-ui/dist/index.d.ts".to_string(),
                kind: crate::resolver_core::DerivedFactKind::DirectSource,
                hash: [99u8; 16],
            }),
            "DirectSource for untracked file should be accepted"
        );

        // Route for untracked file — should NOT accept (invalidation signal).
        assert!(
            !view.validates(&crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: "/node_modules/reka-ui/dist/index.d.ts".to_string(),
                kind: crate::resolver_core::DerivedFactKind::Route,
                hash: [99u8; 16],
            }),
            "Route derived fact for untracked file should NOT be accepted"
        );
    }

    /// Concurrent generations of the same key are distinguished by
    /// per-candidate fact validation against the candidate's own
    /// `fact_dep_signature` (see
    /// `crates/verter_session/src/resolver_core/mod.rs`
    /// `ValidatedFactCache` substrate). For untracked files, the
    /// primary `validates` path accepts the cached hash because the
    /// candidate was admitted from current workspace content.
    #[test]
    fn primary_validates_accepts_untracked_file_whole_hash() {
        let view = super::HostStoreView {
            mutation_epoch: 1,
            whole_hashes: FxHashMap::from_iter([("/src/tracked.ts".to_string(), [1u8; 16])]),
            ..Default::default()
        };

        // Tracked file — matches.
        assert!(
            view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: "/src/tracked.ts".to_string(),
                hash: [1u8; 16],
            })
        );

        // Tracked file — mismatched hash rejected.
        assert!(
            !view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: "/src/tracked.ts".to_string(),
                hash: [2u8; 16],
            })
        );

        // Untracked file — accepted (multi-candidate
        // substrate relies on the candidate's own `fact_dep_signature`
        // to discriminate concurrent generations).
        assert!(
            view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: "/node_modules/vue/dist/vue.d.mts".to_string(),
                hash: [42u8; 16],
            }),
            "untracked files are accepted by primary validation in the multi-candidate substrate"
        );
    }

    #[test]
    fn import_route_hash_ignores_lazy_promotion_to_same_effective_target() {
        let lazy = FxHashMap::from_iter([(
            "./types".to_string(),
            DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: None,
                possible_canonical_ids: vec![
                    "/src/types.d.ts".to_string(),
                    "/src/types.ts".to_string(),
                ],
            },
        )]);
        let promoted = FxHashMap::from_iter([(
            "./types".to_string(),
            DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.d.ts".to_string()),
                possible_canonical_ids: vec![
                    "/src/types.d.ts".to_string(),
                    "/src/types.ts".to_string(),
                ],
            },
        )]);

        assert_eq!(
            hash_import_route_targets(&lazy),
            hash_import_route_targets(&promoted),
            "lazy promotion to the same effective canonical target should not invalidate ImportRoute facts",
        );
    }

    #[test]
    fn import_route_hash_changes_when_effective_target_changes() {
        let before = FxHashMap::from_iter([(
            "./types".to_string(),
            DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: None,
                possible_canonical_ids: vec![
                    "/src/types.d.ts".to_string(),
                    "/src/types.ts".to_string(),
                ],
            },
        )]);
        let after = FxHashMap::from_iter([(
            "./types".to_string(),
            DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: vec![
                    "/src/types.d.ts".to_string(),
                    "/src/types.ts".to_string(),
                ],
            },
        )]);

        assert_ne!(
            hash_import_route_targets(&before),
            hash_import_route_targets(&after),
            "changing the effective canonical target must still invalidate ImportRoute facts",
        );
    }
}
