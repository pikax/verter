use crate::resolver_core::StoreView;
use crate::types::Hash16;
use crate::VerterHost;
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};

#[cfg(not(feature = "scheduler"))]
use crate::shared::read_lock;

const STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct HostStoreView {
    compat_token: crate::resolver_core::StoreViewCompatToken,
    mutation_epoch: u64,
    whole_hashes: FxHashMap<String, Hash16>,
    dependency_resolutions:
        FxHashMap<String, FxHashMap<String, crate::types::DependencyResolution>>,
    derived_hashes: FxHashMap<(String, crate::resolver_core::DerivedFactKind), Hash16>,
    barrel_generations: FxHashMap<String, u64>,
}

impl Default for HostStoreView {
    fn default() -> Self {
        Self {
            compat_token: crate::resolver_core::StoreViewCompatToken(0),
            mutation_epoch: 0,
            whole_hashes: FxHashMap::default(),
            dependency_resolutions: FxHashMap::default(),
            derived_hashes: FxHashMap::default(),
            barrel_generations: FxHashMap::default(),
        }
    }
}

impl HostStoreView {
    pub(crate) fn from_host(host: &VerterHost) -> Self {
        for _ in 0..STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS {
            let snapshot_epoch = host.current_store_view_epoch();
            let view = Self::build(host, snapshot_epoch);
            if host.current_store_view_epoch() == snapshot_epoch {
                return view;
            }
        }

        let snapshot_epoch = host.current_store_view_epoch();
        Self::build(host, snapshot_epoch)
    }

    fn build(host: &VerterHost, snapshot_epoch: u64) -> Self {
        let mut view = Self {
            mutation_epoch: snapshot_epoch,
            ..Self::default()
        };

        #[cfg(feature = "scheduler")]
        {
            let mut canonical_ids = host.scheduler.node_ids();
            canonical_ids.extend(host.compile_cache.iter().map(|entry| entry.key().clone()));
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

                if let Some(entry) = host.compile_cache.get(&canonical_id) {
                    view.dependency_resolutions
                        .insert(canonical_id.clone(), entry.dependency_resolutions.clone());
                    if !entry.dependency_resolutions.is_empty() {
                        view.derived_hashes.insert(
                            (
                                canonical_id.clone(),
                                crate::resolver_core::DerivedFactKind::ExactResolution,
                            ),
                            hash_dependency_resolutions(&entry.dependency_resolutions),
                        );
                    }

                    if let Some(registry) = entry.export_registry.as_ref() {
                        view.derived_hashes.insert(
                            (
                                canonical_id.clone(),
                                crate::resolver_core::DerivedFactKind::ExportRegistry,
                            ),
                            registry.source_hash,
                        );
                    }

                    if let Some(surface) = entry.barrel_export_surface.as_ref() {
                        view.derived_hashes.insert(
                            (
                                canonical_id.clone(),
                                crate::resolver_core::DerivedFactKind::BarrelSurface,
                            ),
                            surface.source_hash,
                        );
                        view.barrel_generations
                            .insert(canonical_id.clone(), surface.generation);
                    }

                    if !entry.import_route_cache.is_empty() {
                        view.derived_hashes.insert(
                            (
                                canonical_id.clone(),
                                crate::resolver_core::DerivedFactKind::Route,
                            ),
                            hash_import_route_cache(&entry.import_route_cache),
                        );
                    }
                }

                view.snapshot_dependency_resolutions_if_missing(host, &canonical_id);
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&host.files);
            for (canonical_id, entry) in files.iter() {
                view.whole_hashes
                    .insert(canonical_id.clone(), entry.whole_hash);
                view.dependency_resolutions
                    .insert(canonical_id.clone(), entry.dependency_resolutions.clone());
                if !entry.dependency_resolutions.is_empty() {
                    view.derived_hashes.insert(
                        (
                            canonical_id.clone(),
                            crate::resolver_core::DerivedFactKind::ExactResolution,
                        ),
                        hash_dependency_resolutions(&entry.dependency_resolutions),
                    );
                }

                if let Some(registry) = entry.export_registry.as_ref() {
                    view.derived_hashes.insert(
                        (
                            canonical_id.clone(),
                            crate::resolver_core::DerivedFactKind::ExportRegistry,
                        ),
                        registry.source_hash,
                    );
                }

                if let Some(surface) = entry.barrel_export_surface.as_ref() {
                    view.derived_hashes.insert(
                        (
                            canonical_id.clone(),
                            crate::resolver_core::DerivedFactKind::BarrelSurface,
                        ),
                        surface.source_hash,
                    );
                    view.barrel_generations
                        .insert(canonical_id.clone(), surface.generation);
                }

                if !entry.import_route_cache.is_empty() {
                    view.derived_hashes.insert(
                        (
                            canonical_id.clone(),
                            crate::resolver_core::DerivedFactKind::Route,
                        ),
                        hash_import_route_cache(&entry.import_route_cache),
                    );
                }
            }
            drop(files);

            let mut canonical_ids: Vec<_> = view.whole_hashes.keys().cloned().collect();
            canonical_ids.sort();
            for canonical_id in canonical_ids {
                view.snapshot_dependency_resolutions_if_missing(host, &canonical_id);
            }
        }

        let workspace_generation = host.ws().content_generation();
        let imported_entries: Vec<_> = host
            .imported_dependency_cache
            .lock()
            .iter()
            .filter_map(|(canonical_id, entry)| {
                (entry.workspace_generation == workspace_generation)
                    .then(|| (canonical_id.clone(), entry.clone()))
            })
            .collect();
        for (canonical_id, entry) in imported_entries {
            view.whole_hashes
                .entry(canonical_id.clone())
                .or_insert(entry.whole_hash);
            if !entry.dependency_resolutions.is_empty()
                && !view.dependency_resolutions.contains_key(&canonical_id)
            {
                view.dependency_resolutions
                    .insert(canonical_id.clone(), entry.dependency_resolutions.clone());
                view.derived_hashes.insert(
                    (
                        canonical_id.clone(),
                        crate::resolver_core::DerivedFactKind::ExactResolution,
                    ),
                    hash_dependency_resolutions(&entry.dependency_resolutions),
                );
            }
        }

        view.snapshot_transitive_dependency_targets(host);
        view.compat_token = view.compute_compat_token();
        view
    }

    fn snapshot_whole_hash_if_known(&mut self, host: &VerterHost, canonical_id: &str) {
        if self.whole_hashes.contains_key(canonical_id) {
            return;
        }

        if let Some(whole_hash) = host.get_whole_hash(canonical_id) {
            self.whole_hashes
                .insert(canonical_id.to_string(), whole_hash);
        }
    }

    fn snapshot_transitive_dependency_targets(&mut self, host: &VerterHost) {
        let mut pending: Vec<String> = self
            .dependency_resolutions
            .values()
            .flat_map(|resolutions| {
                resolutions.values().filter_map(|resolution| {
                    resolution
                        .resolved_canonical_id
                        .clone()
                        .or_else(|| resolution.effective_target().map(str::to_string))
                })
            })
            .collect();
        let mut visited = rustc_hash::FxHashSet::default();

        while let Some(canonical_id) = pending.pop() {
            if !visited.insert(canonical_id.clone()) {
                continue;
            }

            let existing_resolutions = self.dependency_resolutions.get(&canonical_id).cloned();
            self.snapshot_whole_hash_if_known(host, &canonical_id);
            if let Some(resolutions) = existing_resolutions.as_ref() {
                pending.extend(resolutions.values().filter_map(|resolution| {
                    resolution
                        .resolved_canonical_id
                        .clone()
                        .or_else(|| resolution.effective_target().map(str::to_string))
                }));
            }
        }
    }

    pub(crate) fn mutation_epoch(&self) -> u64 {
        self.mutation_epoch
    }

    pub(crate) fn accepts_whole_hash(&self, canonical_id: &str, hash: Hash16) -> bool {
        self.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: canonical_id.to_string(),
            hash,
        })
    }

    pub(crate) fn tracks_whole_hash(&self, canonical_id: &str) -> bool {
        self.whole_hashes.contains_key(canonical_id)
    }

    pub(crate) fn whole_hash(&self, canonical_id: &str) -> Option<Hash16> {
        self.whole_hashes.get(canonical_id).copied()
    }

    pub(crate) fn derived_hash(
        &self,
        canonical_id: &str,
        kind: crate::resolver_core::DerivedFactKind,
    ) -> Option<Hash16> {
        self.derived_hashes
            .get(&(canonical_id.to_string(), kind))
            .copied()
    }

    pub(crate) fn barrel_generation(&self, canonical_id: &str) -> Option<u64> {
        self.barrel_generations.get(canonical_id).copied()
    }

    pub(crate) fn dependency_resolution(
        &self,
        canonical_id: &str,
        import_source: &str,
    ) -> Option<&crate::types::DependencyResolution> {
        self.dependency_resolutions
            .get(canonical_id)
            .and_then(|resolutions| resolutions.get(import_source))
    }

    pub(crate) fn dependency_resolutions(
        &self,
        canonical_id: &str,
    ) -> Option<&FxHashMap<String, crate::types::DependencyResolution>> {
        self.dependency_resolutions.get(canonical_id)
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
            crate::resolver_core::FactVersionRef::BarrelGeneration {
                canonical_id,
                generation,
            } => match self.barrel_generations.get(canonical_id) {
                Some(current) if current == generation => None,
                Some(current) => Some(format!(
                    "BarrelGeneration mismatch canonical={} expected={} actual={}",
                    canonical_id, generation, current
                )),
                None => Some(format!(
                    "BarrelGeneration missing canonical={} expected={}",
                    canonical_id, generation
                )),
            },
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
        }
    }

    fn snapshot_dependency_resolutions_if_missing(
        &mut self,
        host: &VerterHost,
        canonical_id: &str,
    ) {
        if self
            .dependency_resolutions
            .get(canonical_id)
            .is_some_and(|resolutions| !resolutions.is_empty())
        {
            return;
        }

        let Some(snapshot) = host.get_raw_analysis_snapshot_in_view(canonical_id, None) else {
            return;
        };

        let mut resolutions = self
            .dependency_resolutions
            .remove(canonical_id)
            .unwrap_or_default();

        for import in &snapshot.imports {
            resolutions.entry(import.source.clone()).or_insert_with(|| {
                let resolved_canonical_id =
                    if import.is_type_only || is_declaration_companion_source(canonical_id) {
                        host.resolve_type_dependency_canonical(canonical_id, &import.source)
                    } else {
                        host.resolve_loaded_dependency_canonical(
                            canonical_id,
                            &import.source,
                            verter_workspace::ResolveRequestKind::EsmImport,
                        )
                        .or_else(|| {
                            host.resolve_type_dependency_canonical(canonical_id, &import.source)
                        })
                    };

                crate::types::DependencyResolution {
                    specifier: import.source.clone(),
                    resolved_canonical_id,
                    possible_canonical_ids: Vec::new(),
                }
            });
        }

        for sig in snapshot.export_signatures.iter() {
            let Some(source) = sig.reexport_source.as_ref() else {
                continue;
            };

            resolutions.entry(source.clone()).or_insert_with(|| {
                let resolved_canonical_id = if sig.is_type {
                    host.resolve_type_dependency_canonical(canonical_id, source)
                } else {
                    host.resolve_loaded_dependency_canonical(
                        canonical_id,
                        source,
                        verter_workspace::ResolveRequestKind::EsmImport,
                    )
                    .or_else(|| host.resolve_type_dependency_canonical(canonical_id, source))
                };

                crate::types::DependencyResolution {
                    specifier: source.clone(),
                    resolved_canonical_id,
                    possible_canonical_ids: Vec::new(),
                }
            });
        }

        if !resolutions.is_empty() {
            let exact_hash = hash_dependency_resolutions(&resolutions);
            self.dependency_resolutions
                .insert(canonical_id.to_string(), resolutions);
            self.derived_hashes.insert(
                (
                    canonical_id.to_string(),
                    crate::resolver_core::DerivedFactKind::ExactResolution,
                ),
                exact_hash,
            );
        }
    }

    fn compute_compat_token(&self) -> crate::resolver_core::StoreViewCompatToken {
        crate::resolver_core::StoreViewCompatToken(self.mutation_epoch)
    }
}

fn is_declaration_companion_source(canonical_id: &str) -> bool {
    canonical_id.ends_with(".d.ts")
        || canonical_id.ends_with(".d.mts")
        || canonical_id.ends_with(".d.cts")
}

pub(crate) fn hash_dependency_resolutions(
    resolutions: &FxHashMap<String, crate::types::DependencyResolution>,
) -> Hash16 {
    let mut entries: Vec<_> = resolutions.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    hash16_from_sorted(|hasher| {
        for (specifier, resolution) in &entries {
            0u8.hash(hasher);
            specifier.hash(hasher);
            resolution.specifier.hash(hasher);
            let effective_target = resolution.effective_target().map(str::to_string);
            effective_target.hash(hasher);
            let mut candidates = resolution.possible_canonical_ids.clone();
            candidates.sort();
            candidates.dedup();
            if let Some(ref effective_target) = effective_target {
                candidates.retain(|candidate| candidate != effective_target);
            }
            candidates.hash(hasher);
        }
    })
}

pub(crate) fn hash_import_route_cache(
    route_cache: &FxHashMap<
        (String, String, verter_workspace::ResolveRequestKind),
        crate::types::ImportTypeRouteEntry,
    >,
) -> Hash16 {
    let mut entries: Vec<_> = route_cache.iter().collect();
    entries.sort_by(|a, b| {
        a.0 .0
            .cmp(&b.0 .0)
            .then_with(|| a.0 .1.cmp(&b.0 .1))
            .then_with(|| resolve_request_kind_rank(a.0 .2).cmp(&resolve_request_kind_rank(b.0 .2)))
    });

    hash16_from_sorted(|hasher| {
        for ((import_source, type_name, kind), entry) in &entries {
            1u8.hash(hasher);
            import_source.hash(hasher);
            type_name.hash(hasher);
            resolve_request_kind_rank(*kind).hash(hasher);
            entry.owner_hash.hash(hasher);
            entry
                .target
                .as_ref()
                .map(|target| &target.final_canonical_id)
                .hash(hasher);
            entry
                .target
                .as_ref()
                .map(|target| &target.exported_name)
                .hash(hasher);
            entry.tracked_deps.hash(hasher);
            let mut route_hashes = entry.route_hashes.clone();
            route_hashes.sort_by(|a, b| a.0.cmp(&b.0));
            route_hashes.hash(hasher);
            entry.negative_barrel_gen.hash(hasher);
        }
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

fn resolve_request_kind_rank(kind: verter_workspace::ResolveRequestKind) -> u8 {
    match kind {
        verter_workspace::ResolveRequestKind::EsmImport => 0,
        verter_workspace::ResolveRequestKind::TypeImport => 1,
        verter_workspace::ResolveRequestKind::RequireCall => 2,
        verter_workspace::ResolveRequestKind::SfcSrcAttr => 3,
    }
}

impl crate::resolver_core::StoreView for HostStoreView {
    fn compat_token(&self) -> crate::resolver_core::StoreViewCompatToken {
        self.compat_token
    }

    fn validates(&self, fact: &crate::resolver_core::FactVersionRef) -> bool {
        match fact {
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, hash } => self
                .whole_hashes
                .get(canonical_id)
                .is_some_and(|current| current == hash),
            crate::resolver_core::FactVersionRef::BarrelGeneration {
                canonical_id,
                generation,
            } => self
                .barrel_generations
                .get(canonical_id)
                .is_some_and(|current| current == generation),
            crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind,
                hash,
            } => match kind {
                crate::resolver_core::DerivedFactKind::DirectSource => self
                    .whole_hashes
                    .get(canonical_id)
                    .is_some_and(|current| current == hash),
                _ => self
                    .derived_hashes
                    .get(&(canonical_id.clone(), *kind))
                    .is_some_and(|current| current == hash),
            },
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
}

#[cfg(test)]
mod tests {
    use super::hash_dependency_resolutions;
    use crate::types::DependencyResolution;
    use rustc_hash::FxHashMap;

    #[test]
    fn exact_resolution_hash_ignores_lazy_promotion_to_same_effective_target() {
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
            hash_dependency_resolutions(&lazy),
            hash_dependency_resolutions(&promoted),
            "lazy promotion to the same effective canonical target should not invalidate ExactResolution facts",
        );
    }

    #[test]
    fn exact_resolution_hash_changes_when_effective_target_changes() {
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
            hash_dependency_resolutions(&before),
            hash_dependency_resolutions(&after),
            "changing the effective canonical target must still invalidate ExactResolution facts",
        );
    }
}
