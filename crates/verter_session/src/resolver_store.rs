use crate::types::Hash16;
use crate::VerterHost;
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

    /// Build a session-scoped store view.
    ///
    /// The compat token includes the session identity so that two sessions
    /// with different overlays but the same epoch never coalesce into the
    /// same singleflight lane (Path C C14).
    pub(crate) fn from_session(
        view: &crate::session_runtime::SessionView,
        host: &VerterHost,
    ) -> Self {
        for _ in 0..STORE_VIEW_SNAPSHOT_RETRY_ATTEMPTS {
            let snapshot_epoch = host.current_store_view_epoch();
            let sv = Self::build(host, snapshot_epoch, Some(view.session_id));
            if host.current_store_view_epoch() == snapshot_epoch {
                return sv;
            }
        }

        let snapshot_epoch = host.current_store_view_epoch();
        Self::build(host, snapshot_epoch, Some(view.session_id))
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

                if let Some(entry) = host.compile_cache().get(&canonical_id) {
                    for (specifier, resolution) in entry.import_routes.iter() {
                        view.import_routes.insert(
                            (canonical_id.clone(), specifier.clone()),
                            resolution.clone(),
                        );
                    }
                }
            }
        }

        // WASM-only: scheduler is unavailable on web; see CLAUDE.md "Scheduler as Sole Compile Authority".

        // Snapshot IndexedReadyDb entries into the store view.
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
        view.compat_token = view.compute_compat_token();
        view
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
                    host.compile_cache().get(&canonical_id).and_then(|entry| {
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

impl crate::resolver_core::StoreView for HostStoreView {
    fn compat_token(&self) -> crate::resolver_core::StoreViewCompatToken {
        self.compat_token
    }

    fn checks_archive(&self) -> bool {
        true
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
        }
    }

    /// Strict validation for ARCHIVED entries. Archived entries are from a
    /// prior generation — they were soft-invalidated and may be stale.
    /// For untracked files (not in whole_hashes), the content may have
    /// changed on disk since the archive was created. Reject these to
    /// force re-materialization from current workspace content.
    fn validates_archived(&self, fact: &crate::resolver_core::FactVersionRef) -> bool {
        match fact {
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, hash } => self
                .whole_hashes
                .get(canonical_id)
                .is_some_and(|current| current == hash),
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

    fn tracks_file(&self, canonical_id: &str) -> bool {
        self.whole_hashes.contains_key(canonical_id)
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
    ) -> crate::component_meta_audit::RustStoreAudit {
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

        // §3.2 — pull per-request materialiser/storage counters
        // off the active `RequestContext` (zero ops when no context is
        // installed; the audit pipeline always installs one before
        // taking this snapshot).
        let (
            materialize_structure_calls,
            materialize_structure_cache_hits,
            node_arena_lock_acquisitions,
            family_map_lock_acquisitions,
            dep_signature_merges,
            dep_signature_intern_hits,
        ) = match crate::request_context::current_request_context() {
            Some(ctx) => (
                ctx.materialize_structure_calls
                    .load(std::sync::atomic::Ordering::Relaxed),
                ctx.materialize_structure_cache_hits
                    .load(std::sync::atomic::Ordering::Relaxed),
                ctx.node_arena_lock_acquisitions
                    .load(std::sync::atomic::Ordering::Relaxed),
                ctx.family_map_lock_acquisitions
                    .load(std::sync::atomic::Ordering::Relaxed),
                ctx.dep_signature_merges
                    .load(std::sync::atomic::Ordering::Relaxed),
                ctx.dep_signature_intern_hits
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            None => (0, 0, 0, 0, 0, 0),
        };

        crate::component_meta_audit::RustStoreAudit {
            store_view_hits: u32::from(store_view.is_some()),
            store_view_misses: u32::from(store_view.is_none()),
            structural_merges: 0,
            imported_dependency_entries: indexed_entries,
            imported_dependency_bytes: indexed_bytes,
            prepared_type_decls,
            prepared_value_decls,
            materialize_structure_calls,
            materialize_structure_cache_hits,
            node_arena_lock_acquisitions,
            family_map_lock_acquisitions,
            dep_signature_merges,
            dep_signature_intern_hits,
        }
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

    /// validates_archived must be STRICT for untracked files: archived entries
    /// may be from a prior generation where the workspace content was different.
    /// Primary-entry acceptance (validates) for untracked files is safe because
    /// those entries were just materialized from current content.
    #[test]
    fn validates_archived_rejects_untracked_file_whole_hash() {
        let view = super::HostStoreView {
            mutation_epoch: 1,
            whole_hashes: FxHashMap::from_iter([("/src/tracked.ts".to_string(), [1u8; 16])]),
            ..Default::default()
        };

        // Tracked file in archive — validates if hash matches.
        assert!(
            view.validates_archived(&crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: "/src/tracked.ts".to_string(),
                hash: [1u8; 16],
            })
        );

        // Tracked file in archive — rejects if hash mismatches.
        assert!(
            !view.validates_archived(&crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: "/src/tracked.ts".to_string(),
                hash: [2u8; 16],
            })
        );

        // Untracked file in archive — MUST reject (content may have changed).
        assert!(
            !view.validates_archived(&crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: "/node_modules/vue/dist/vue.d.mts".to_string(),
                hash: [42u8; 16],
            }),
            "validates_archived must reject untracked files to prevent stale data"
        );

        // Compare: validates() accepts the same untracked file (primary entries safe).
        assert!(
            view.validates(&crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: "/node_modules/vue/dist/vue.d.mts".to_string(),
                hash: [42u8; 16],
            }),
            "validates() should still accept untracked files in primary cache"
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
