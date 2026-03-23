use crate::types::Hash16;
use crate::VerterHost;
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};

#[cfg(not(feature = "scheduler"))]
use crate::shared::read_lock;

const STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS: usize = 3;

#[derive(Debug, Clone)]
pub struct HostStoreView {
    compat_token: verter_resolver::StoreViewCompatToken,
    mutation_epoch: u64,
    whole_hashes: FxHashMap<String, Hash16>,
    derived_hashes: FxHashMap<(String, verter_resolver::DerivedFactKind), Hash16>,
    barrel_generations: FxHashMap<String, u64>,
}

impl Default for HostStoreView {
    fn default() -> Self {
        Self {
            compat_token: verter_resolver::StoreViewCompatToken(0),
            mutation_epoch: 0,
            whole_hashes: FxHashMap::default(),
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
                }
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            let files = read_lock(&host.files);
            for (canonical_id, entry) in files.iter() {
                view.whole_hashes
                    .insert(canonical_id.clone(), entry.whole_hash);
            }
        }

        view.compat_token = view.compute_compat_token();
        view
    }

    pub(crate) fn mutation_epoch(&self) -> u64 {
        self.mutation_epoch
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
