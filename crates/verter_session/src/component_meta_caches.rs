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

use crate::cooperative_admission::{cooperative_get_or_insert, InflightTable};
use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;
use crate::resolver_core::cache_keys::{
    PreparedMemberCacheKey, PreparedSurfaceCacheKey, PreparedTargetCacheKey,
    RoutedExprSurfaceCacheKey,
};
use crate::resolver_core::component_meta_query_engine::ResolvedImportedRegistrySymbol;
use crate::resolver_core::{ResolvedTypeDeclaration, ResolverContext};
use crate::semantic_query::{DepSignature, ProjectionMode};

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

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &ImportedRegistryKey,
        ctx: &dyn ResolverContext,
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
                if ctx.validate_dep_signature(&entry.dep_signature) {
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
            |entry: &ImportedRegistryEntry| ctx.validate_dep_signature(&entry.dep_signature),
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

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &DeclarationLookupKey,
        ctx: &dyn ResolverContext,
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
                if ctx.validate_dep_signature(&entry.dep_signature) {
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
            |entry: &DeclarationLookupEntry| ctx.validate_dep_signature(&entry.dep_signature),
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

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &ResolvabilityKey,
        ctx: &dyn ResolverContext,
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
                if ctx.validate_dep_signature(&entry.dep_signature) {
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
            |entry: &ResolvabilityEntry| ctx.validate_dep_signature(&entry.dep_signature),
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

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &OwnerCollectionKey,
        ctx: &dyn ResolverContext,
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
                if ctx.validate_dep_signature(&entry.dep_signature) {
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
            |entry: &OwnerCollectionEntry| ctx.validate_dep_signature(&entry.dep_signature),
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
    pub(crate) fn peek(
        &self,
        key: &PreparedTargetCacheKey,
        ctx: &dyn ResolverContext,
    ) -> Option<Option<(Arc<str>, Arc<str>)>> {
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if ctx.validate_dep_signature(&entry_arc.dep_signature) {
            Some(entry_arc.value.clone())
        } else {
            None
        }
    }

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &PreparedTargetCacheKey,
        ctx: &dyn ResolverContext,
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
                if ctx.validate_dep_signature(&entry.dep_signature) {
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
            |entry: &PreparedTargetEntry| ctx.validate_dep_signature(&entry.dep_signature),
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
    pub(crate) fn peek(
        &self,
        key: &MaterializeMemoKey,
        ctx: &dyn ResolverContext,
    ) -> Option<MaterializedTypeExpr> {
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if ctx.validate_dep_signature(&entry_arc.dep_signature) {
            Some(entry_arc.value.clone())
        } else {
            None
        }
    }

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &MaterializeMemoKey,
        ctx: &dyn ResolverContext,
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
                if ctx.validate_dep_signature(&entry.dep_signature) {
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
            |entry: &MaterializeMemoEntry| ctx.validate_dep_signature(&entry.dep_signature),
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
    pub(crate) fn peek(
        &self,
        key: &PreparedSurfaceCacheKey,
        ctx: &dyn ResolverContext,
    ) -> Option<PreparedSurfacePayload> {
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if ctx.validate_dep_signature(&entry_arc.dep_signature) {
            Some(entry_arc.value.clone())
        } else {
            None
        }
    }

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &PreparedSurfaceCacheKey,
        ctx: &dyn ResolverContext,
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
                if ctx.validate_dep_signature(&entry.dep_signature) {
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
            |entry: &PreparedSurfaceEntry| ctx.validate_dep_signature(&entry.dep_signature),
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
    pub(crate) fn peek(
        &self,
        key: &PreparedMemberCacheKey,
        ctx: &dyn ResolverContext,
    ) -> Option<Option<Arc<ProjectedMember>>> {
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if ctx.validate_dep_signature(&entry_arc.dep_signature) {
            Some(entry_arc.value.clone())
        } else {
            None
        }
    }

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &PreparedMemberCacheKey,
        ctx: &dyn ResolverContext,
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
                if ctx.validate_dep_signature(&entry.dep_signature) {
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
            |entry: &PreparedMemberEntry| ctx.validate_dep_signature(&entry.dep_signature),
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
    pub(crate) fn peek(
        &self,
        key: &RoutedExprSurfaceCacheKey,
        ctx: &dyn ResolverContext,
    ) -> Option<Arc<TypeExpr>> {
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if ctx.validate_dep_signature(&entry_arc.dep_signature) {
            Some(entry_arc.value.clone())
        } else {
            None
        }
    }

    pub(crate) fn get_or_compute<F>(
        &self,
        key: &RoutedExprSurfaceCacheKey,
        ctx: &dyn ResolverContext,
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
                if ctx.validate_dep_signature(&entry.dep_signature) {
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
            |entry: &RoutedExprSurfaceEntry| ctx.validate_dep_signature(&entry.dep_signature),
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

// ===========================================================================
// Plan §1.5 / Phase 8 — MaterializeStructureDb
// ===========================================================================

use crate::component_meta_materialize::{MaterializeOutcome, MaterializeStructureCacheKey};

/// Plan §1.5 — entry stored in `MaterializeStructureDb`. Carries the
/// cacheable `MaterializeOutcome` (`Value` or `Miss` only — `Recursive`
/// and `Tainted` are non-cacheable per-call sentinels) plus the
/// `dep_signature` that produced it.
#[derive(Clone)]
pub struct MaterializeStructureEntry {
    /// The cached outcome. ONLY `Value` or `Miss` may be stored here.
    /// The materialiser's publish path enforces this with
    /// `debug_assert!`.
    pub outcome: MaterializeOutcome,
    /// `dep_signature` observed during the cold build. Used by
    /// `peek` and the cooperative-admission post-publish revalidation
    /// to detect stale entries after canonical invalidation.
    pub dep_signature: DepSignature,
}

/// Plan §1.5 / §10.1 — final-result cache for the structural
/// materialiser. Reverse-index `canonical_to_keys` enables
/// `Arc::ptr_eq`-based invalidation cleanup; cooperative-admission's
/// `post_publish` callback wires the registration.
pub struct MaterializeStructureDb {
    entries: DashMap<MaterializeStructureCacheKey, Arc<MaterializeStructureEntry>>,
    inflight: InflightTable<MaterializeStructureCacheKey>,
    /// Per-canonical reverse index: maps each canonical id to the set
    /// of cache keys whose `dep_signature` references it, paired with
    /// the registered `dep_signature` `Arc`. `invalidate_for_canonical`
    /// drains this map and uses `Arc::ptr_eq` to discriminate stale
    /// entries from fresh post-publish writes.
    canonical_to_keys: DashMap<
        Arc<str>,
        parking_lot::Mutex<rustc_hash::FxHashMap<MaterializeStructureCacheKey, DepSignature>>,
    >,
    live_counter: Arc<AtomicU64>,
}

impl MaterializeStructureDb {
    /// Construct a fresh cache.
    #[must_use]
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            canonical_to_keys: DashMap::new(),
            live_counter,
        }
    }

    /// Plan §6.10 sub-task 8 — read-only test accessor for the shared
    /// live_counter. Used by `materialize_publish_after_invalidation_*`
    /// tests to verify that revalidation failures do NOT increment the
    /// counter (entries are removed without inflating the live count).
    #[cfg(test)]
    #[allow(
        dead_code,
        reason = "B1's validated_at_generation field + the test that consumes this accessor land in WT5/R; this accessor is reserved for that wiring per plan §6.10 sub-task 8"
    )]
    pub(crate) fn live_counter_for_test(&self) -> u64 {
        self.live_counter.load(Ordering::Relaxed)
    }

    /// Read-only peek with proactive stale-entry removal. Plan §1.5:
    /// when the entry's `dep_signature` is stale, remove it (orphan
    /// reaping) and return `None`.
    ///
    /// Plan R8-5 — successful stale removal must decrement the shared
    /// `live_counter` so it tracks live entries (not lifetime inserts).
    /// Without this, every stale peek inflates the shared counter
    /// permanently.
    pub(crate) fn peek(
        &self,
        key: &MaterializeStructureCacheKey,
        ctx: &dyn ResolverContext,
    ) -> Option<crate::semantic_query::CacheRead<MaterializeOutcome>> {
        let entry_arc = self.entries.get(key).map(|e| e.clone())?;
        if !ctx.validate_dep_signature(&entry_arc.dep_signature) {
            let removed = self
                .entries
                .remove_if(key, |_, e| Arc::ptr_eq(e, &entry_arc));
            if removed.is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
            return None;
        }
        crate::host_manage::record_materialize_structure_cache_hit();
        Some(crate::semantic_query::CacheRead {
            value: entry_arc.outcome.clone(),
            dep_signature: entry_arc.dep_signature.clone(),
        })
    }

    /// Drop every cache entry whose `dep_signature` references
    /// `canonical_id`. Plan §1.5 — uses the `canonical_to_keys`
    /// reverse index to find affected keys; uses `Arc::ptr_eq` to
    /// discriminate "our entry" from concurrent fresh writes.
    pub fn invalidate_for_canonical(&self, canonical_id: &str) {
        let drained: Vec<(MaterializeStructureCacheKey, DepSignature)> =
            match self.canonical_to_keys.remove(canonical_id) {
                Some((_, mutex)) => mutex.lock().drain().collect(),
                None => return,
            };
        for (key, registered_sig) in &drained {
            let registered = Arc::clone(registered_sig);
            let removed = self.entries.remove_if(key, move |_, entry_arc| {
                Arc::ptr_eq(&entry_arc.dep_signature, &registered)
            });
            if removed.is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
                // Cross-canonical cleanup with ptr_eq — drop the
                // matching registration in every other canonical's
                // shard.
                for (other_canonical, _) in registered_sig.iter() {
                    if other_canonical.as_ref() == canonical_id {
                        continue;
                    }
                    if let Some(shard) = self.canonical_to_keys.get(other_canonical) {
                        let mut map = shard.lock();
                        if let Some(existing_sig) = map.get(key) {
                            if Arc::ptr_eq(existing_sig, registered_sig) {
                                map.remove(key);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Drop every cache entry. Used on project-generation bumps.
    ///
    /// Plan R8-5 — saturating-subtract pattern (NOT `store(0)`) because
    /// `live_counter` is shared via `Arc<AtomicU64>` across every typed DB
    /// in `ProjectTypeStore` (`component_meta_cache_live`). A per-DB
    /// `store(0)` would corrupt other DBs' contributions to the shared
    /// sum. Mirrors the existing `ImportedRegistryDb::invalidate_all`
    /// pattern — subtract only this DB's entry count, capped at the
    /// counter's current value to prevent underflow under
    /// concurrent invalidation.
    pub fn invalidate_all(&self) {
        let n = self.entries.len() as u64;
        self.entries.clear();
        self.canonical_to_keys.clear();
        self.live_counter.fetch_sub(
            n.min(self.live_counter.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
    }

    /// Number of warm entries.
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.entries.len()
    }

    /// Internal — register a `(key, dep_signature)` pair under every
    /// canonical in the dep_signature. Called from the materialiser's
    /// `post_publish` callback.
    pub(crate) fn register_post_publish(
        &self,
        key: MaterializeStructureCacheKey,
        dep_signature: DepSignature,
    ) {
        for (canonical, _) in dep_signature.iter() {
            crate::host_manage::record_family_map_lock_acquisition();
            let shard = self
                .canonical_to_keys
                .entry(Arc::clone(canonical))
                .or_insert_with(|| parking_lot::Mutex::new(rustc_hash::FxHashMap::default()));
            let mut map = shard.value().lock();
            map.insert(key.clone(), Arc::clone(&dep_signature));
        }
    }

    /// Internal — get the inflight table. Used by the materialiser
    /// for the cooperative-admission write path.
    pub(crate) fn inflight(&self) -> &InflightTable<MaterializeStructureCacheKey> {
        &self.inflight
    }

    /// Internal — get the entries map. Used by the materialiser for
    /// the cooperative-admission write path.
    pub(crate) fn entries(
        &self,
    ) -> &DashMap<MaterializeStructureCacheKey, Arc<MaterializeStructureEntry>> {
        &self.entries
    }

    /// Internal — bump the live counter. Called from the
    /// materialiser's compute closure on successful publish.
    pub(crate) fn bump_live_counter(&self) {
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for MaterializeStructureDb {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Plan §4.8 / Phase C / Commit R — RefCycleResultDb
// ===========================================================================

use crate::cooperative_admission::cooperative_get_or_insert_with_post_publish;
use crate::semantic_query::DeclIdentity;

/// Plan §4.8 / Phase C — entry stored in `RefCycleResultDb`. Carries the
/// boolean BFS result, the dep-signature recorded during the cold BFS
/// compute, and a `validated_at_generation` field used by `peek`'s
/// generation-local fast path.
///
/// No `Clone` derive — `AtomicU64` is non-Clone. Entries are wrapped in
/// `Arc<RefCycleEntry>`; cloning the `Arc` cheaply shares the entry
/// across cache hits.
pub struct RefCycleEntry {
    /// `true` when the BFS root reaches a transitive cycle through a
    /// complex helper surface.
    pub result: bool,
    /// `dep_signature` recorded during the cold BFS compute. Used by
    /// `peek`'s slow path to revalidate against `HostFenceValidator`
    /// when `validated_at_generation` is stale.
    pub dep_signature: DepSignature,
    /// Plan §4.9 — generation-local validity field. Updated to the
    /// current `workspace().content_generation()` on:
    ///   - cold publish (initial value = current generation);
    ///   - successful slow-path revalidation in `peek`.
    ///
    /// `peek`'s fast path returns immediately when `cached ==
    /// current_gen` without walking the dep_signature. Race contract:
    /// `Relaxed` ordering — under heavy invalidation, a thread may see
    /// a stale cached_gen and re-walk the slow path even when the
    /// entry is still valid. This is correct (re-walking catches
    /// genuine staleness) but may briefly reduce cache effectiveness.
    pub validated_at_generation: AtomicU64,
}

/// Plan §4.8 / §4.9 / Commit R — host-owned cache for transitive
/// cycle BFS results.
///
/// Mirrors [`MaterializeStructureDb`]'s reverse-index pattern:
///   - Entries keyed by `DeclIdentity`.
/// - `canonical_to_keys` reverse-index drains under `invalidate_for_canonical`.
/// - `Arc::ptr_eq` discriminates "our entry" from concurrent fresh writes.
///   - `live_counter` shared via `Arc<AtomicU64>` with all sibling DBs;
///     uses the saturating-subtract pattern on `invalidate_all` to
///     preserve other DBs' contributions.
///
/// Cooperative-admission integration: cold-path BFS runs inside
/// [`cooperative_get_or_insert_with_post_publish`], whose `compute`
/// closure runs synchronously on the caller's thread (see
/// `cooperative_admission.rs:278` synchronous-compute contract).
/// Borrow-capture of `&dyn ResolverContext` in the BFS compute closure
/// is safe because no thread-hop occurs.
pub struct RefCycleResultDb {
    entries: DashMap<DeclIdentity, Arc<RefCycleEntry>>,
    inflight: InflightTable<DeclIdentity>,
    /// Per-canonical reverse index — maps each canonical id to the set
    /// of cache keys whose dep_signature references it.
    canonical_to_keys:
        DashMap<Arc<str>, parking_lot::Mutex<rustc_hash::FxHashMap<DeclIdentity, DepSignature>>>,
    live_counter: Arc<AtomicU64>,
}

impl RefCycleResultDb {
    /// Construct with a fresh, unshared `live_counter`. Tests-only.
    #[must_use]
    pub fn new() -> Self {
        Self::with_counter(Arc::new(AtomicU64::new(0)))
    }

    /// Construct with a shared `live_counter` borrowed from
    /// `ProjectTypeStoreCounters::component_meta_cache_live`.
    pub(crate) fn with_counter(live_counter: Arc<AtomicU64>) -> Self {
        Self {
            entries: DashMap::new(),
            inflight: InflightTable::new(),
            canonical_to_keys: DashMap::new(),
            live_counter,
        }
    }

    /// Internal — get the entries map. Used by the BFS-cache compute
    /// closure for `cooperative_get_or_insert_with_post_publish`.
    pub(crate) fn entries(&self) -> &DashMap<DeclIdentity, Arc<RefCycleEntry>> {
        &self.entries
    }

    /// Internal — get the inflight table. Used by the BFS-cache compute
    /// closure for `cooperative_get_or_insert_with_post_publish`.
    pub(crate) fn inflight(&self) -> &InflightTable<DeclIdentity> {
        &self.inflight
    }

    /// Internal — bump the live counter. Called from the BFS-cache
    /// compute closure's `post_publish` callback on successful publish.
    pub(crate) fn bump_live_counter(&self) {
        self.live_counter.fetch_add(1, Ordering::Relaxed);
    }

    /// Plan §6.10 sub-task 8 — read-only test accessor for the shared
    /// `live_counter`. Used by R's invalidation tests to verify that
    /// `invalidate_for_canonical` and `invalidate_all` correctly
    /// decrement the counter without corrupting sibling DBs'
    /// contributions to the shared sum.
    #[cfg(test)]
    pub(crate) fn live_counter_for_test(&self) -> u64 {
        self.live_counter.load(Ordering::Relaxed)
    }

    /// Plan §4.8 / §10.1 — register the reverse-index after a successful
    /// publish. Per-canonical mutex acquisition pattern matches
    /// `MaterializeStructureDb`. Bounded by `dep_signature.len() ≤ 64`
    /// (BFS hop cap from A0).
    pub(crate) fn register_post_publish(&self, key: DeclIdentity, dep_signature: DepSignature) {
        for (canonical, _) in dep_signature.iter() {
            crate::host_manage::record_family_map_lock_acquisition();
            let shard = self
                .canonical_to_keys
                .entry(Arc::clone(canonical))
                .or_insert_with(|| parking_lot::Mutex::new(rustc_hash::FxHashMap::default()));
            let mut map = shard.value().lock();
            map.insert(key.clone(), Arc::clone(&dep_signature));
        }
    }

    /// Plan §4.9 — generation-local validity peek.
    ///
    /// Fast path: if the entry's `validated_at_generation` matches the
    /// host's current `content_generation`, return the cached value
    /// without walking the dep_signature.
    ///
    /// Slow path: revalidate against `HostFenceValidator`; on success,
    /// update `validated_at_generation` and return; on failure, remove
    /// the stale entry (with `live_counter` decrement per R8-5) and
    /// return `None`.
    pub(crate) fn peek(
        &self,
        id: &DeclIdentity,
        ctx: &dyn ResolverContext,
    ) -> Option<crate::semantic_query::CacheRead<bool>> {
        let entry_arc = self.entries.get(id).map(|e| Arc::clone(&*e))?;
        let current_gen = ctx.workspace_content_generation();
        let cached_gen = entry_arc.validated_at_generation.load(Ordering::Relaxed);
        if cached_gen == current_gen {
            return Some(crate::semantic_query::CacheRead {
                value: entry_arc.result,
                dep_signature: Arc::clone(&entry_arc.dep_signature),
            });
        }
        if !ctx.validate_dep_signature(&entry_arc.dep_signature) {
            // Plan R8-5 — decrement live_counter on stale removal so
            // the shared counter tracks live entries, not stale ones.
            let removed = self
                .entries
                .remove_if(id, |_, e| Arc::ptr_eq(e, &entry_arc));
            if removed.is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
            }
            return None;
        }
        entry_arc
            .validated_at_generation
            .store(current_gen, Ordering::Relaxed);
        Some(crate::semantic_query::CacheRead {
            value: entry_arc.result,
            dep_signature: Arc::clone(&entry_arc.dep_signature),
        })
    }

    /// Plan §4.8 / R8-5 — drop every cache entry whose `dep_signature`
    /// references `canonical_id`. Uses the `canonical_to_keys` reverse
    /// index to find affected keys; `Arc::ptr_eq` discriminates "our
    /// entry" from concurrent fresh writes.
    pub fn invalidate_for_canonical(&self, canonical_id: &str) {
        let drained: Vec<(DeclIdentity, DepSignature)> =
            match self.canonical_to_keys.remove(canonical_id) {
                Some((_, mutex)) => mutex.lock().drain().collect(),
                None => return,
            };
        for (key, registered_sig) in &drained {
            let registered = Arc::clone(registered_sig);
            let removed = self.entries.remove_if(key, move |_, entry_arc| {
                Arc::ptr_eq(&entry_arc.dep_signature, &registered)
            });
            if removed.is_some() {
                self.live_counter.fetch_sub(1, Ordering::Relaxed);
                // Cross-canonical cleanup with ptr_eq — drop the
                // matching registration in every other canonical's
                // shard so subsequent invalidations do not double-free.
                for (other_canonical, _) in registered_sig.iter() {
                    if other_canonical.as_ref() == canonical_id {
                        continue;
                    }
                    if let Some(shard) = self.canonical_to_keys.get(other_canonical) {
                        let mut map = shard.lock();
                        if let Some(existing_sig) = map.get(key) {
                            if Arc::ptr_eq(existing_sig, registered_sig) {
                                map.remove(key);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Plan §4.8 / R8-5 — saturating-subtract pattern (NOT `store(0)`)
    /// because `live_counter` is shared via `Arc<AtomicU64>` across all
    /// typed DBs in `ProjectTypeStore`. A per-DB `store(0)` would
    /// corrupt sibling DBs' contributions to the shared sum.
    pub fn invalidate_all(&self) {
        let n = self.entries.len() as u64;
        self.entries.clear();
        self.canonical_to_keys.clear();
        self.live_counter.fetch_sub(
            n.min(self.live_counter.load(Ordering::Relaxed)),
            Ordering::Relaxed,
        );
    }
}

impl Default for RefCycleResultDb {
    fn default() -> Self {
        Self::new()
    }
}

/// Plan §4.8 — public hook to consult the BFS cache from
/// `meta_resolve::ref_root_reaches_transitive_cycle_node`.
///
/// Returns `Some(read)` on a generation-local fast hit OR a
/// successful slow-path revalidation. Returns `None` on a true cache
/// miss or a stale entry (the caller falls through to BFS compute).
pub(crate) fn ref_cycle_db_peek(
    db: &RefCycleResultDb,
    id: &DeclIdentity,
    ctx: &dyn ResolverContext,
) -> Option<crate::semantic_query::CacheRead<bool>> {
    db.peek(id, ctx)
}

/// Plan §4.8 / §4.20 — the cooperative-admission wrapper invoked by
/// `meta_resolve::ref_root_reaches_transitive_cycle_node` on the cold
/// path. The `compute` closure runs synchronously on the caller's
/// thread (per cooperative_admission's synchronous-compute contract),
/// so capturing `&dyn ResolverContext` and `&DeclIdentity` directly is safe.
///
/// On cooperative-admission success: bumps `live_counter`, registers
/// the reverse-index, and returns `Some(CacheRead)`. On revalidation
/// failure or compute returning `None`: returns `None` and the caller
/// falls back to an uncached recompute.
pub(crate) fn ref_cycle_db_get_or_compute<C>(
    db: &RefCycleResultDb,
    id: &DeclIdentity,
    ctx: &dyn ResolverContext,
    compute_bfs: C,
) -> Option<crate::semantic_query::CacheRead<bool>>
where
    C: FnOnce(&mut Vec<(Arc<str>, crate::semantic_query::DepVersion)>) -> bool,
{
    let key_for_register = id.clone();
    let current_gen = ctx.workspace_content_generation();
    cooperative_get_or_insert_with_post_publish(
        db.entries(),
        db.inflight(),
        id.clone(),
        // validate(&Entry) -> Option<V>
        |entry: &RefCycleEntry| {
            if ctx.validate_dep_signature(&entry.dep_signature) {
                Some(crate::semantic_query::CacheRead {
                    value: entry.result,
                    dep_signature: Arc::clone(&entry.dep_signature),
                })
            } else {
                None
            }
        },
        // compute() -> Option<Entry>
        || -> Option<RefCycleEntry> {
            let mut compute_fence: Vec<(Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
            let result = compute_bfs(&mut compute_fence);
            Some(RefCycleEntry {
                result,
                dep_signature: Arc::from(compute_fence.into_boxed_slice()),
                validated_at_generation: AtomicU64::new(current_gen),
            })
        },
        // project(&Entry) -> V
        |entry: &RefCycleEntry| crate::semantic_query::CacheRead {
            value: entry.result,
            dep_signature: Arc::clone(&entry.dep_signature),
        },
        // revalidate_after_compute(&Entry) -> bool
        |entry: &RefCycleEntry| ctx.validate_dep_signature(&entry.dep_signature),
        // post_publish(&Arc<Entry>, &K)
        move |entry_arc: &Arc<RefCycleEntry>, _k: &DeclIdentity| {
            db.bump_live_counter();
            db.register_post_publish(
                key_for_register.clone(),
                Arc::clone(&entry_arc.dep_signature),
            );
        },
    )
}
