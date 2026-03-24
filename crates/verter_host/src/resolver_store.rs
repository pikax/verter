use crate::types::Hash16;
use crate::VerterHost;
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};
use verter_resolver::StoreView;

#[cfg(not(feature = "scheduler"))]
use crate::shared::read_lock;

const STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct HostStoreView {
    compat_token: verter_resolver::StoreViewCompatToken,
    mutation_epoch: u64,
    whole_hashes: FxHashMap<String, Hash16>,
    dependency_resolutions:
        FxHashMap<String, FxHashMap<String, crate::types::DependencyResolution>>,
    derived_hashes: FxHashMap<(String, verter_resolver::DerivedFactKind), Hash16>,
    barrel_generations: FxHashMap<String, u64>,
}

impl Default for HostStoreView {
    fn default() -> Self {
        Self {
            compat_token: verter_resolver::StoreViewCompatToken(0),
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
                                verter_resolver::DerivedFactKind::ExactResolution,
                            ),
                            hash_dependency_resolutions(&entry.dependency_resolutions),
                        );
                    }

                    if let Some(registry) = entry.export_registry.as_ref() {
                        view.derived_hashes.insert(
                            (
                                canonical_id.clone(),
                                verter_resolver::DerivedFactKind::ExportRegistry,
                            ),
                            registry.source_hash,
                        );
                    }

                    if let Some(surface) = entry.barrel_export_surface.as_ref() {
                        view.derived_hashes.insert(
                            (
                                canonical_id.clone(),
                                verter_resolver::DerivedFactKind::BarrelSurface,
                            ),
                            surface.source_hash,
                        );
                        view.barrel_generations
                            .insert(canonical_id.clone(), surface.generation);
                    }

                    if !entry.import_route_cache.is_empty() {
                        view.derived_hashes.insert(
                            (canonical_id.clone(), verter_resolver::DerivedFactKind::Route),
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
                            verter_resolver::DerivedFactKind::ExactResolution,
                        ),
                        hash_dependency_resolutions(&entry.dependency_resolutions),
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

        view.snapshot_transitive_dependency_targets(host);
        view.compat_token = view.compute_compat_token();
        view
    }

    fn snapshot_whole_hash_if_known(&mut self, host: &VerterHost, canonical_id: &str) {
        if self.whole_hashes.contains_key(canonical_id) {
            return;
        }

        #[cfg(feature = "scheduler")]
        {
            if let Some(source) = host.scheduler.try_get_source(canonical_id) {
                self.whole_hashes
                    .insert(canonical_id.to_string(), source.whole_hash);
                return;
            }
        }

        if let Some(state) = host.effective_file_state(canonical_id, None) {
            self.whole_hashes
                .insert(canonical_id.to_string(), state.whole_hash);
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

            self.snapshot_whole_hash_if_known(host, &canonical_id);
            self.snapshot_dependency_resolutions_if_missing(host, &canonical_id);

            if let Some(resolutions) = self.dependency_resolutions.get(&canonical_id) {
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
        self.validates(&verter_resolver::FactVersionRef::FileWholeHash {
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
        kind: verter_resolver::DerivedFactKind,
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
                let resolved_canonical_id = if import.is_type_only
                    || is_declaration_companion_source(canonical_id)
                {
                    host.resolve_type_dependency_canonical(canonical_id, &import.source)
                } else {
                    host.resolve_loaded_dependency_canonical(
                        canonical_id,
                        &import.source,
                        verter_vfs::ResolveRequestKind::EsmImport,
                    )
                    .or_else(|| host.resolve_type_dependency_canonical(canonical_id, &import.source))
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
                        verter_vfs::ResolveRequestKind::EsmImport,
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
                    verter_resolver::DerivedFactKind::ExactResolution,
                ),
                exact_hash,
            );
        }
    }

    fn compute_compat_token(&self) -> verter_resolver::StoreViewCompatToken {
        let mut hasher = rustc_hash::FxHasher::default();
        self.mutation_epoch.hash(&mut hasher);

        let mut whole_hashes: Vec<_> = self.whole_hashes.iter().collect();
        whole_hashes.sort_by(|a, b| a.0.cmp(b.0));
        for (canonical_id, hash) in whole_hashes {
            0u8.hash(&mut hasher);
            canonical_id.hash(&mut hasher);
            hash.hash(&mut hasher);
        }

        let mut dependency_resolutions: Vec<_> = self.dependency_resolutions.iter().collect();
        dependency_resolutions.sort_by(|a, b| a.0.cmp(b.0));
        for (canonical_id, resolutions) in dependency_resolutions {
            let mut entries: Vec<_> = resolutions.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            for (specifier, resolution) in entries {
                3u8.hash(&mut hasher);
                canonical_id.hash(&mut hasher);
                specifier.hash(&mut hasher);
                resolution.specifier.hash(&mut hasher);
                resolution.resolved_canonical_id.hash(&mut hasher);
                let mut candidates = resolution.possible_canonical_ids.clone();
                candidates.sort();
                candidates.hash(&mut hasher);
            }
        }

        let mut derived_hashes: Vec<_> = self.derived_hashes.iter().collect();
        derived_hashes.sort_by(|a, b| {
            a.0 .0
                .cmp(&b.0 .0)
                .then_with(|| derived_fact_kind_rank(a.0 .1).cmp(&derived_fact_kind_rank(b.0 .1)))
        });
        for ((canonical_id, kind), hash) in derived_hashes {
            1u8.hash(&mut hasher);
            canonical_id.hash(&mut hasher);
            kind.hash(&mut hasher);
            hash.hash(&mut hasher);
        }

        let mut barrel_generations: Vec<_> = self.barrel_generations.iter().collect();
        barrel_generations.sort_by(|a, b| a.0.cmp(b.0));
        for (canonical_id, generation) in barrel_generations {
            2u8.hash(&mut hasher);
            canonical_id.hash(&mut hasher);
            generation.hash(&mut hasher);
        }

        verter_resolver::StoreViewCompatToken(hasher.finish())
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
            resolution.resolved_canonical_id.hash(hasher);
            let mut candidates = resolution.possible_canonical_ids.clone();
            candidates.sort();
            candidates.hash(hasher);
        }
    })
}

#[cfg(feature = "scheduler")]
pub(crate) fn hash_import_route_cache(
    route_cache: &FxHashMap<
        (String, String, verter_vfs::ResolveRequestKind),
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
            entry.target.as_ref().map(|target| &target.final_canonical_id).hash(hasher);
            entry.target.as_ref().map(|target| &target.exported_name).hash(hasher);
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

#[cfg(feature = "scheduler")]
fn resolve_request_kind_rank(kind: verter_vfs::ResolveRequestKind) -> u8 {
    match kind {
        verter_vfs::ResolveRequestKind::EsmImport => 0,
        verter_vfs::ResolveRequestKind::TypeImport => 1,
        verter_vfs::ResolveRequestKind::RequireCall => 2,
        verter_vfs::ResolveRequestKind::SfcSrcAttr => 3,
    }
}

fn derived_fact_kind_rank(kind: verter_resolver::DerivedFactKind) -> u8 {
    match kind {
        verter_resolver::DerivedFactKind::ExportRegistry => 0,
        verter_resolver::DerivedFactKind::Route => 1,
        verter_resolver::DerivedFactKind::BarrelSurface => 2,
        verter_resolver::DerivedFactKind::ExactResolution => 3,
        verter_resolver::DerivedFactKind::DirectSource => 4,
    }
}

impl verter_resolver::StoreView for HostStoreView {
    fn compat_token(&self) -> verter_resolver::StoreViewCompatToken {
        self.compat_token
    }

    fn validates(&self, fact: &verter_resolver::FactVersionRef) -> bool {
        match fact {
            verter_resolver::FactVersionRef::FileWholeHash { canonical_id, hash } => self
                .whole_hashes
                .get(canonical_id)
                .is_some_and(|current| current == hash),
            verter_resolver::FactVersionRef::BarrelGeneration {
                canonical_id,
                generation,
            } => self
                .barrel_generations
                .get(canonical_id)
                .is_some_and(|current| current == generation),
            verter_resolver::FactVersionRef::DerivedFactHash {
                canonical_id,
                kind,
                hash,
            } => match kind {
                verter_resolver::DerivedFactKind::DirectSource => self
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

impl verter_resolver::ResolverStore for VerterHost {
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
