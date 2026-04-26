//! Host-owned typed DB wrappers for the 10 component-meta caches that
//! were previously authoritative inside `ComponentMetaQueryEngine`.
//!
//! Plan §3 D3.2 sub-task 3.2.1 (architectural-debt-closure revision 10).
//!
//! ## Architecture
//!
//! Each cache is a typed `*Db` wrapper around `DashMap<Key, Arc<Entry>>`
//! plus a per-cache `InflightTable<Key>` (admission control isolation per
//! plan §3 D3.2). All 10 wrappers share the same shape:
//!
//! - `Entry` carries `(value, dep_signature)`.
//! - `get_or_compute<F>(key, host, compute) -> Option<value>` delegates to
//!   [`cooperative_get_or_insert`] for one-winner-cold-build,
//!   panic-safety, and post-compute revalidation.
//! - `validate(&Entry)` and `revalidate_after_compute(&Entry)` consult
//!   [`HostFenceValidator`](crate::host_manage::HostFenceValidator) to
//!   reject entries whose `dep_signature` no longer matches host state.
//! - Per-canonical and project-generation invalidation hooks wired into
//!   [`ProjectTypeStore::evict_canonical`] and
//!   [`ProjectTypeStore::bump_project_generation_and_evict`].
//!
//! ## D3.5 — `Arc<str>` / `Arc<TypeExpr>` keys
//!
//! Keys live in [`crate::resolver_core::cache_keys`] with the wide-string
//! fields migrated to `Arc<str>` per D3.5. Cloning a key is a cheap
//! refcount bump rather than a heap allocation + copy.
//!
//! ## Engine read-through views
//!
//! `ComponentMetaQueryEngine` keeps a per-request `RefCell<FxHashMap>`
//! mirror of each DB so repeated lookups in one request hit the local
//! mirror first. Per the D3.2 contract, the mirror is **non-authoritative
//! scratch only** — it never inserts entries the host DB doesn't have,
//! never holds an independent dep-signature, and never invalidates
//! independently. The mirror clears on engine drop (per-request scope).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::query_engine::ProjectedMember;

use crate::completion_fence::FenceValidator;
use crate::cooperative_admission::{cooperative_get_or_insert, InflightTable};
use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;
use crate::resolver_core::cache_keys::{
    MaterializedMemberSurfaceKey, PreparedMemberCacheKey, PreparedSurfaceCacheKey,
    PreparedTargetCacheKey, RoutedExprSurfaceCacheKey,
};
use crate::resolver_core::component_meta_query_engine::ResolvedImportedRegistrySymbol;
use crate::resolver_core::ResolvedTypeDeclaration;
use crate::semantic_query::{DepSignature, ProjectionMode};
use crate::VerterHost;

/// Validate every fact in a `DepSignature` against current host state.
///
/// Returns `true` only when every `(canonical_id, version)` pair in the
/// signature still matches what the host reports — a single mismatch
/// invalidates the entry.
pub(crate) fn dep_signature_valid_for_host(signature: &DepSignature, host: &VerterHost) -> bool {
    let validator = crate::host_manage::HostFenceValidator { host };
    signature
        .iter()
        .all(|(canonical, version)| validator.validate(canonical.as_ref(), version))
}

// ===========================================================================
// 1. ImportedRegistryDb — `(canonical, name) → Option<ResolvedImportedRegistrySymbol>`
// ===========================================================================

#[derive(Clone)]
pub struct ImportedRegistryEntry {
    pub value: Option<Arc<ResolvedImportedRegistrySymbol>>,
    pub dep_signature: DepSignature,
}

pub type ImportedRegistryKey = (Arc<str>, Arc<str>);

pub struct ImportedRegistryDb {
    entries: DashMap<ImportedRegistryKey, Arc<ImportedRegistryEntry>>,
    inflight: InflightTable<ImportedRegistryKey>,
    live_counter: Arc<AtomicU64>,
}

impl ImportedRegistryDb {
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
        }
    }

    pub fn get_or_compute<F>(
        &self,
        key: &ImportedRegistryKey,
        host: &VerterHost,
        compute: F,
    ) -> Option<Option<Arc<ResolvedImportedRegistrySymbol>>>
    where
        F: FnOnce() -> Option<(Option<ResolvedImportedRegistrySymbol>, DepSignature)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &ImportedRegistryEntry| {
                if dep_signature_valid_for_host(&entry.dep_signature, host) {
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, dep_signature)| {
                    let inserted_value = value.map(Arc::new);
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    ImportedRegistryEntry {
                        value: inserted_value,
                        dep_signature,
                    }
                })
            },
            |entry: &ImportedRegistryEntry| entry.value.clone(),
            |entry: &ImportedRegistryEntry| {
                dep_signature_valid_for_host(&entry.dep_signature, host)
            },
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<ImportedRegistryKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (canonical, _) = entry.key();
                if canonical.as_ref() == canonical_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

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

impl Default for ImportedRegistryDb {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// 2. DeclarationLookupDb — `(canonical, name) → ResolvedTypeDeclaration`
// ===========================================================================

#[derive(Clone)]
pub struct DeclarationLookupEntry {
    pub value: Arc<ResolvedTypeDeclaration>,
    pub dep_signature: DepSignature,
}

pub type DeclarationLookupKey = (Arc<str>, Arc<str>);

pub struct DeclarationLookupDb {
    entries: DashMap<DeclarationLookupKey, Arc<DeclarationLookupEntry>>,
    inflight: InflightTable<DeclarationLookupKey>,
    live_counter: Arc<AtomicU64>,
}

impl DeclarationLookupDb {
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
        }
    }

    pub fn get_or_compute<F>(
        &self,
        key: &DeclarationLookupKey,
        host: &VerterHost,
        compute: F,
    ) -> Option<Arc<ResolvedTypeDeclaration>>
    where
        F: FnOnce() -> Option<(ResolvedTypeDeclaration, DepSignature)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &DeclarationLookupEntry| {
                if dep_signature_valid_for_host(&entry.dep_signature, host) {
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    DeclarationLookupEntry {
                        value: Arc::new(value),
                        dep_signature,
                    }
                })
            },
            |entry: &DeclarationLookupEntry| entry.value.clone(),
            |entry: &DeclarationLookupEntry| {
                dep_signature_valid_for_host(&entry.dep_signature, host)
            },
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<DeclarationLookupKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (canonical, _) = entry.key();
                if canonical.as_ref() == canonical_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

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

impl Default for DeclarationLookupDb {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// 3. ResolvabilityDb — `(canonical, name) → bool`
// ===========================================================================

#[derive(Clone)]
pub struct ResolvabilityEntry {
    pub value: bool,
    pub dep_signature: DepSignature,
}

pub type ResolvabilityKey = (Arc<str>, Arc<str>);

pub struct ResolvabilityDb {
    entries: DashMap<ResolvabilityKey, Arc<ResolvabilityEntry>>,
    inflight: InflightTable<ResolvabilityKey>,
    live_counter: Arc<AtomicU64>,
}

impl ResolvabilityDb {
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
        }
    }

    pub fn get_or_compute<F>(
        &self,
        key: &ResolvabilityKey,
        host: &VerterHost,
        compute: F,
    ) -> Option<bool>
    where
        F: FnOnce() -> Option<(bool, DepSignature)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &ResolvabilityEntry| {
                if dep_signature_valid_for_host(&entry.dep_signature, host) {
                    Some(entry.value)
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    ResolvabilityEntry {
                        value,
                        dep_signature,
                    }
                })
            },
            |entry: &ResolvabilityEntry| entry.value,
            |entry: &ResolvabilityEntry| dep_signature_valid_for_host(&entry.dep_signature, host),
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<ResolvabilityKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (canonical, _) = entry.key();
                if canonical.as_ref() == canonical_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

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

impl Default for ResolvabilityDb {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// 4. OwnerCollectionDb — `Arc<str> (name) → Option<TypeExpr>`
//
// Note: keyed solely by name within an owner scope. Since multiple owners
// may collide on the same name with different TypeExprs, the entry tracks
// the owner_canonical at insertion time and validates per-canonical only.
// ===========================================================================

#[derive(Clone)]
pub struct OwnerCollectionEntry {
    pub owner_canonical: Arc<str>,
    pub value: Option<Arc<TypeExpr>>,
    pub dep_signature: DepSignature,
}

pub type OwnerCollectionKey = (Arc<str>, Arc<str>); // (owner, name)

pub struct OwnerCollectionDb {
    entries: DashMap<OwnerCollectionKey, Arc<OwnerCollectionEntry>>,
    inflight: InflightTable<OwnerCollectionKey>,
    live_counter: Arc<AtomicU64>,
}

impl OwnerCollectionDb {
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
        }
    }

    pub fn get_or_compute<F>(
        &self,
        key: &OwnerCollectionKey,
        host: &VerterHost,
        compute: F,
    ) -> Option<Option<Arc<TypeExpr>>>
    where
        F: FnOnce() -> Option<(Option<TypeExpr>, DepSignature)>,
    {
        let owner_canonical = key.0.clone();
        let live_counter = Arc::clone(&self.live_counter);
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &OwnerCollectionEntry| {
                if dep_signature_valid_for_host(&entry.dep_signature, host) {
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    OwnerCollectionEntry {
                        owner_canonical,
                        value: value.map(Arc::new),
                        dep_signature,
                    }
                })
            },
            |entry: &OwnerCollectionEntry| entry.value.clone(),
            |entry: &OwnerCollectionEntry| dep_signature_valid_for_host(&entry.dep_signature, host),
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<OwnerCollectionKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (owner, _) = entry.key();
                if owner.as_ref() == canonical_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

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

impl Default for OwnerCollectionDb {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// 5. PreparedTargetDb — `PreparedTargetCacheKey → Option<(Arc<str>, Arc<str>)>`
// ===========================================================================

#[derive(Clone)]
pub struct PreparedTargetEntry {
    pub value: Option<(Arc<str>, Arc<str>)>,
    pub dep_signature: DepSignature,
}

pub struct PreparedTargetDb {
    entries: DashMap<PreparedTargetCacheKey, Arc<PreparedTargetEntry>>,
    inflight: InflightTable<PreparedTargetCacheKey>,
    live_counter: Arc<AtomicU64>,
}

impl PreparedTargetDb {
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
        }
    }

    /// Peek-only lookup: returns the cached value only if its
    /// dep_signature is still valid against `host`.
    pub fn peek(
        &self,
        key: &PreparedTargetCacheKey,
        host: &VerterHost,
    ) -> Option<Option<(Arc<str>, Arc<str>)>> {
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if dep_signature_valid_for_host(&entry_arc.dep_signature, host) {
            Some(entry_arc.value.clone())
        } else {
            None
        }
    }

    pub fn get_or_compute<F>(
        &self,
        key: &PreparedTargetCacheKey,
        host: &VerterHost,
        compute: F,
    ) -> Option<Option<(Arc<str>, Arc<str>)>>
    where
        F: FnOnce() -> Option<(Option<(Arc<str>, Arc<str>)>, DepSignature)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &PreparedTargetEntry| {
                if dep_signature_valid_for_host(&entry.dep_signature, host) {
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    PreparedTargetEntry {
                        value,
                        dep_signature,
                    }
                })
            },
            |entry: &PreparedTargetEntry| entry.value.clone(),
            |entry: &PreparedTargetEntry| dep_signature_valid_for_host(&entry.dep_signature, host),
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<PreparedTargetCacheKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let key = entry.key();
                if key.active_scope_canonical_id.as_ref() == canonical_id
                    || key.decl_canonical_id.as_ref() == canonical_id
                {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

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

impl Default for PreparedTargetDb {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// 6. MaterializeMemoDb — `(Arc<str>, Arc<TypeExpr>, ProjectionMode) → MaterializedTypeExpr`
// ===========================================================================

#[derive(Clone)]
pub struct MaterializeMemoEntry {
    pub value: MaterializedTypeExpr,
    pub dep_signature: DepSignature,
}

pub type MaterializeMemoKey = (Arc<str>, Arc<TypeExpr>, ProjectionMode);

pub struct MaterializeMemoDb {
    entries: DashMap<MaterializeMemoKey, Arc<MaterializeMemoEntry>>,
    inflight: InflightTable<MaterializeMemoKey>,
    live_counter: Arc<AtomicU64>,
}

impl MaterializeMemoDb {
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
        }
    }

    /// Peek-only lookup: returns the cached value only if its
    /// dep_signature is still valid against `host`.
    pub fn peek(
        &self,
        key: &MaterializeMemoKey,
        host: &VerterHost,
    ) -> Option<MaterializedTypeExpr> {
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if dep_signature_valid_for_host(&entry_arc.dep_signature, host) {
            Some(entry_arc.value.clone())
        } else {
            None
        }
    }

    pub fn get_or_compute<F>(
        &self,
        key: &MaterializeMemoKey,
        host: &VerterHost,
        compute: F,
    ) -> Option<MaterializedTypeExpr>
    where
        F: FnOnce() -> Option<(MaterializedTypeExpr, DepSignature)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &MaterializeMemoEntry| {
                if dep_signature_valid_for_host(&entry.dep_signature, host) {
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    MaterializeMemoEntry {
                        value,
                        dep_signature,
                    }
                })
            },
            |entry: &MaterializeMemoEntry| entry.value.clone(),
            |entry: &MaterializeMemoEntry| dep_signature_valid_for_host(&entry.dep_signature, host),
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<MaterializeMemoKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let (scope, _, _) = entry.key();
                if scope.as_ref() == canonical_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

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

impl Default for MaterializeMemoDb {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// 7. MaterializedMemberSurfaceDb — `MaterializedMemberSurfaceKey → TypeExpr`
// ===========================================================================

#[derive(Clone)]
pub struct MaterializedMemberSurfaceEntry {
    pub value: Arc<TypeExpr>,
    pub dep_signature: DepSignature,
}

pub struct MaterializedMemberSurfaceDb {
    entries: DashMap<MaterializedMemberSurfaceKey, Arc<MaterializedMemberSurfaceEntry>>,
    inflight: InflightTable<MaterializedMemberSurfaceKey>,
    live_counter: Arc<AtomicU64>,
}

impl MaterializedMemberSurfaceDb {
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
        }
    }

    /// Peek-only lookup for `cached_*` accessors that must not trigger
    /// a cold compute. Returns the cached value only if the entry's
    /// dep_signature is still valid against `host`; stale entries
    /// return `None` (caller falls through to compute through the
    /// regular `get_or_compute` path).
    pub fn peek(
        &self,
        key: &MaterializedMemberSurfaceKey,
        host: &VerterHost,
    ) -> Option<Arc<TypeExpr>> {
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if dep_signature_valid_for_host(&entry_arc.dep_signature, host) {
            Some(entry_arc.value.clone())
        } else {
            None
        }
    }

    pub fn get_or_compute<F>(
        &self,
        key: &MaterializedMemberSurfaceKey,
        host: &VerterHost,
        compute: F,
    ) -> Option<Arc<TypeExpr>>
    where
        F: FnOnce() -> Option<(TypeExpr, DepSignature)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &MaterializedMemberSurfaceEntry| {
                if dep_signature_valid_for_host(&entry.dep_signature, host) {
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    MaterializedMemberSurfaceEntry {
                        value: Arc::new(value),
                        dep_signature,
                    }
                })
            },
            |entry: &MaterializedMemberSurfaceEntry| entry.value.clone(),
            |entry: &MaterializedMemberSurfaceEntry| {
                dep_signature_valid_for_host(&entry.dep_signature, host)
            },
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<MaterializedMemberSurfaceKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let key = entry.key();
                if key.scope_canonical_id.as_ref() == canonical_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

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

impl Default for MaterializedMemberSurfaceDb {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// 8. PreparedSurfaceDb — `PreparedSurfaceCacheKey → PreparedSurfaceProjection`
//
// PreparedSurfaceProjection is private to the engine; we serialize the
// concrete `Arc<ProjectedSurface>` payload through a public adapter.
// ===========================================================================

/// Public projection payload mirrored from the engine's
/// `PreparedSurfaceProjection`. The engine adapter converts at the
/// edges so consumers do not depend on the engine module's private
/// enum.
#[derive(Debug, Clone)]
pub enum PreparedSurfacePayload {
    Surface(Arc<verter_semantic::analysis::type_solver::query_engine::ProjectedSurface>),
    Empty,
    Unsupported,
}

#[derive(Clone)]
pub struct PreparedSurfaceEntry {
    pub value: PreparedSurfacePayload,
    pub dep_signature: DepSignature,
}

pub struct PreparedSurfaceDb {
    entries: DashMap<PreparedSurfaceCacheKey, Arc<PreparedSurfaceEntry>>,
    inflight: InflightTable<PreparedSurfaceCacheKey>,
    live_counter: Arc<AtomicU64>,
}

impl PreparedSurfaceDb {
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
        }
    }

    /// Peek-only lookup: returns the cached payload only if its
    /// dep_signature is still valid against `host`.
    pub fn peek(
        &self,
        key: &PreparedSurfaceCacheKey,
        host: &VerterHost,
    ) -> Option<PreparedSurfacePayload> {
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if dep_signature_valid_for_host(&entry_arc.dep_signature, host) {
            Some(entry_arc.value.clone())
        } else {
            None
        }
    }

    pub fn get_or_compute<F>(
        &self,
        key: &PreparedSurfaceCacheKey,
        host: &VerterHost,
        compute: F,
    ) -> Option<PreparedSurfacePayload>
    where
        F: FnOnce() -> Option<(PreparedSurfacePayload, DepSignature)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &PreparedSurfaceEntry| {
                if dep_signature_valid_for_host(&entry.dep_signature, host) {
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    PreparedSurfaceEntry {
                        value,
                        dep_signature,
                    }
                })
            },
            |entry: &PreparedSurfaceEntry| entry.value.clone(),
            |entry: &PreparedSurfaceEntry| dep_signature_valid_for_host(&entry.dep_signature, host),
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<PreparedSurfaceCacheKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let key = entry.key();
                if key.canonical_id.as_ref() == canonical_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

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

impl Default for PreparedSurfaceDb {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// 9. PreparedMemberDb — `PreparedMemberCacheKey → Option<ProjectedMember>`
// ===========================================================================

#[derive(Clone)]
pub struct PreparedMemberEntry {
    pub value: Option<Arc<ProjectedMember>>,
    pub dep_signature: DepSignature,
}

pub struct PreparedMemberDb {
    entries: DashMap<PreparedMemberCacheKey, Arc<PreparedMemberEntry>>,
    inflight: InflightTable<PreparedMemberCacheKey>,
    live_counter: Arc<AtomicU64>,
}

impl PreparedMemberDb {
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
        }
    }

    /// Peek-only lookup: returns the cached value only if its
    /// dep_signature is still valid against `host`.
    pub fn peek(
        &self,
        key: &PreparedMemberCacheKey,
        host: &VerterHost,
    ) -> Option<Option<Arc<ProjectedMember>>> {
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if dep_signature_valid_for_host(&entry_arc.dep_signature, host) {
            Some(entry_arc.value.clone())
        } else {
            None
        }
    }

    pub fn get_or_compute<F>(
        &self,
        key: &PreparedMemberCacheKey,
        host: &VerterHost,
        compute: F,
    ) -> Option<Option<Arc<ProjectedMember>>>
    where
        F: FnOnce() -> Option<(Option<ProjectedMember>, DepSignature)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &PreparedMemberEntry| {
                if dep_signature_valid_for_host(&entry.dep_signature, host) {
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    PreparedMemberEntry {
                        value: value.map(Arc::new),
                        dep_signature,
                    }
                })
            },
            |entry: &PreparedMemberEntry| entry.value.clone(),
            |entry: &PreparedMemberEntry| dep_signature_valid_for_host(&entry.dep_signature, host),
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<PreparedMemberCacheKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let key = entry.key();
                if key.canonical_id.as_ref() == canonical_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

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

impl Default for PreparedMemberDb {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// 10. RoutedExprSurfaceDb — `RoutedExprSurfaceCacheKey → TypeExpr`
// ===========================================================================

#[derive(Clone)]
pub struct RoutedExprSurfaceEntry {
    pub value: Arc<TypeExpr>,
    pub dep_signature: DepSignature,
}

pub struct RoutedExprSurfaceDb {
    entries: DashMap<RoutedExprSurfaceCacheKey, Arc<RoutedExprSurfaceEntry>>,
    inflight: InflightTable<RoutedExprSurfaceCacheKey>,
    live_counter: Arc<AtomicU64>,
}

impl RoutedExprSurfaceDb {
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            live_counter,
        }
    }

    /// Peek-only lookup: returns the cached value only if its
    /// dep_signature is still valid against `host`.
    pub fn peek(
        &self,
        key: &RoutedExprSurfaceCacheKey,
        host: &VerterHost,
    ) -> Option<Arc<TypeExpr>> {
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if dep_signature_valid_for_host(&entry_arc.dep_signature, host) {
            Some(entry_arc.value.clone())
        } else {
            None
        }
    }

    pub fn get_or_compute<F>(
        &self,
        key: &RoutedExprSurfaceCacheKey,
        host: &VerterHost,
        compute: F,
    ) -> Option<Arc<TypeExpr>>
    where
        F: FnOnce() -> Option<(TypeExpr, DepSignature)>,
    {
        let live_counter = Arc::clone(&self.live_counter);
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            |entry: &RoutedExprSurfaceEntry| {
                if dep_signature_valid_for_host(&entry.dep_signature, host) {
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            move || {
                compute().map(|(value, dep_signature)| {
                    live_counter.fetch_add(1, Ordering::Relaxed);
                    RoutedExprSurfaceEntry {
                        value: Arc::new(value),
                        dep_signature,
                    }
                })
            },
            |entry: &RoutedExprSurfaceEntry| entry.value.clone(),
            |entry: &RoutedExprSurfaceEntry| {
                dep_signature_valid_for_host(&entry.dep_signature, host)
            },
        )
    }

    pub fn invalidate_canonical(&self, canonical_id: &str) {
        let keys: Vec<RoutedExprSurfaceCacheKey> = self
            .entries
            .iter()
            .filter_map(|entry| {
                let key = entry.key();
                if key.scope_canonical_id.as_ref() == canonical_id {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        for key in keys {
            if self.entries.remove(&key).is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
        }
    }

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

impl Default for RoutedExprSurfaceDb {
    fn default() -> Self {
        Self::new()
    }
}
