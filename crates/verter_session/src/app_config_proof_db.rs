//! Host-owned `AppConfigNoOverrideProofDb` for the ComponentConfig
//! theme variant fast path (Issue #6).
//!
//! ## Contract (per sidecar §5 + §6.2)
//!
//! Each entry proves: "for the given `(app_config_decl_id,
//! component_key_literal)` tuple, the effective resolved `AppConfig`
//! has NO `ui[component_key_literal]` member regardless of which file
//! declared it (interface merging, module augmentation, generic
//! defaults, all considered)".
//!
//! The proof cache is consulted by the fast path BEFORE deciding
//! whether to project the prepared theme value directly. On cache
//! miss the fast path declines and the slow path runs.
//!
//! ## Population
//!
//! The proof is populated by the slow path: when canonical
//! materialization confirms no `ui[key]` override exists for a given
//! `app_config_decl_id`, it backfills the proof entry with its own
//! dep signature (every contributing file's `content_hash` plus the
//! workspace-level "interface-merging-of-AppConfig generation"
//! counter that bumps when any new `interface AppConfig` declaration
//! is added or removed anywhere in the project).
//!
//! ## Invariants
//!
//! - On invalidation of any contributing file's `content_hash` OR a
//!   bump of the interface-merging generation, the proof entry is
//!   evicted.
//! - There is NO eager workspace-wide effective-interface resolver.
//!   The cache is populated demand-driven by the slow path; until
//!   populated, the fast path declines.
//! - Cache backend follows the same `(Arc<Entry>, dep_signature)`
//!   shape as [`crate::component_meta_caches::ImportedRegistryDb`]
//!   so cooperative-admission is consistent across all
//!   ProjectTypeStore caches.
//!
//! ## §17.7 Deviation status
//!
//! The proof's dep signature requires the workspace-level
//! `interface_merging_of_app_config_generation` counter. Its
//! incrementality contract requires `IndexedReady` to record a
//! per-file `declares_interface_app_config: bool` shallow flag.
//! Until that flag lands, slow-path-side population is a no-op (see
//! the deferred-test marker in
//! `component_meta_component_config_fast_path_tests::component_config_theme_variant_uses_app_config_no_override_proof_when_present`).
//! The DB is published on `ProjectTypeStore` so the fast path's
//! cache-consultation API is in place; it currently always misses
//! and the fast path falls through to the Path-A Record-AppConfig
//! check.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;

use crate::semantic_query::DepSignature;

/// Cache key: `(app_config_decl_canonical_id, component_key_literal)`.
///
/// `app_config_decl_canonical_id` is the canonical id of the file
/// that declares (or first declares, in the merge case) the
/// `AppConfig` interface. `component_key_literal` is the literal key
/// supplied as the third type argument of `ComponentConfig<typeof
/// theme, AppConfig, key>` — e.g. `"button"` or `"variants"`.
pub type AppConfigNoOverrideProofKey = (Arc<str>, Arc<str>);

/// Cache entry: just the dep signature. The presence of an entry IS
/// the proof — we do not need a separate value.
#[derive(Clone)]
pub struct AppConfigNoOverrideProofEntry {
    /// Recorded at slow-path publish time. Includes every contributing
    /// file's content_hash plus the workspace-level
    /// `interface_merging_of_app_config_generation` counter.
    pub dep_signature: DepSignature,
    /// R3/R26/R28 path-precise dep signature sibling. Bubbles into
    /// outer fact tracers via
    /// [`crate::fact_signature_helpers::bubble_fact_signature`].
    /// AND-gate with the legacy `dep_signature` per codex's Stage
    /// 7C.A1b guidance.
    pub fact_dep_signature: Arc<[crate::resolver_core::FactVersionRef]>,
}

/// Host-owned cache. Sole authority for the proof state on
/// [`crate::project_type_store::ProjectTypeStore`].
pub struct AppConfigNoOverrideProofDb {
    entries: DashMap<AppConfigNoOverrideProofKey, Arc<AppConfigNoOverrideProofEntry>>,
    live_counter: Arc<AtomicU64>,
}

impl AppConfigNoOverrideProofDb {
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            live_counter,
        }
    }

    /// Look up a proof entry. Returns `None` on miss; the fast-path
    /// caller declines and the slow path runs.
    ///
    /// The `validate` closure consults
    /// [`crate::host_manage::HostFenceValidator`] to revalidate the
    /// entry's `dep_signature` against current host state. A stale
    /// entry is treated as a miss.
    pub fn peek<F>(
        &self,
        key: &AppConfigNoOverrideProofKey,
        validate: F,
    ) -> Option<Arc<AppConfigNoOverrideProofEntry>>
    where
        F: FnOnce(&DepSignature) -> bool,
    {
        let entry = self.entries.get(key)?;
        if validate(&entry.dep_signature) {
            Some(Arc::clone(entry.value()))
        } else {
            // Stale; drop reference to allow eviction by other paths.
            drop(entry);
            None
        }
    }

    /// Publish a freshly-computed proof entry. Called by the slow
    /// path's canonical materialization side-effect when it confirms
    /// no `ui[key]` override exists for the given decl + key.
    ///
    /// `dep_signature` MUST cover every file content hash that
    /// contributed to the determination plus the workspace-level
    /// `interface_merging_of_app_config_generation` counter.
    pub fn publish(&self, key: AppConfigNoOverrideProofKey, dep_signature: DepSignature) {
        let fact_dep_signature =
            crate::component_meta_materialize::fact_signature_from_fence(dep_signature.as_ref());
        let entry = Arc::new(AppConfigNoOverrideProofEntry {
            dep_signature,
            fact_dep_signature,
        });
        if self.entries.insert(key, entry).is_none() {
            self.live_counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Per-canonical eviction: drop every entry whose dep signature
    /// references `canonical_id` (either as the
    /// `app_config_decl_canonical_id` key component or anywhere in
    /// the dep signature). Called from
    /// [`crate::project_type_store::ProjectTypeStore::evict_canonical`].
    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let to_remove: Vec<AppConfigNoOverrideProofKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (decl_canonical, _) = entry.key();
                let dep_hits = entry
                    .value()
                    .dep_signature
                    .iter()
                    .any(|(canonical, _)| canonical.as_ref() == canonical_id);
                if decl_canonical.as_ref() == canonical_id || dep_hits {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in to_remove {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

    /// Drop every entry. Called on project-generation bump.
    pub fn invalidate_all(&self) {
        let n = self.entries.len() as u64;
        self.entries.clear();
        self.live_counter.fetch_sub(
            n.min(self.live_counter.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
    }

    pub fn live_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for AppConfigNoOverrideProofDb {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::invalidation_domain::ParticipatesInInvalidation for AppConfigNoOverrideProofDb {
    fn domains(&self) -> &'static [crate::invalidation_domain::InvalidationDomain] {
        use crate::invalidation_domain::InvalidationDomain::*;
        // `AppConfigNoOverrideProofDb` participates in
        // [FileContent, AppConfigInterfaceMerge]. The proof's dep
        // signature includes the merge-generation; either a content
        // edit on a flagged file or a workspace-level
        // `interface AppConfig` shape change must invalidate it.
        &[FileContent, AppConfigInterfaceMerge]
    }
    fn invalidate(&self, domain: crate::invalidation_domain::InvalidationDomain) {
        use crate::invalidation_domain::InvalidationDomain::*;
        if matches!(domain, AppConfigInterfaceMerge | ProjectGeneration) {
            self.invalidate_all();
        }
    }
}

impl crate::invalidation_domain::InvalidationByCanonical for AppConfigNoOverrideProofDb {
    fn invalidate_canonical_for(&self, canonical_id: &str) -> usize {
        let before = self.live_count();
        self.invalidate_canonical(canonical_id);
        let after = self.live_count();
        before.saturating_sub(after)
    }
}
