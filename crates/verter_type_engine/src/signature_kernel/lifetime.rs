//! Epoch ownership and retirement.
//!
//! Roots are live project state, intentionally retained results, and live
//! readers. Coherent epoch replacement retires obsolete tables: old pinned
//! readers finish against their epoch; new requests use the replacement.
//! A stale handle (wrong epoch, or epoch 0) is rejected.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock, Weak};

use arc_swap::ArcSwap;
use parking_lot::Mutex;

use crate::semantic_query::{CanonicalTypeSubstitution, SemanticNodeId};

use super::provenance::{ConstituentSequence, SignatureProvenance};
use super::records::{
    AppliedResult, BinderSpace, BinderSpaceId, BodyLocatorId, CallSubstitutionId,
    DeclarationInstantiationId, GraphEpoch, ParameterLayout, ParameterLayoutId, ParameterSlot,
    ParameterSlotId, SignatureCandidate, SignatureDescriptor, SignatureDescriptorId,
    SignatureInputShape, SignatureInputShapeId, SignatureProvenanceId, SignatureResultRecipe,
    SignatureResultRecipeId, SignatureSet, SignatureSetId, SignatureSetRef, SignatureTemplate,
    SignatureTemplateId, SpellingId,
};
use super::storage::{AppendInterner, EpochStorage, InternError};
use super::substitution::{
    compose_canonical, CallSubstitution, SubstError, SubstTerm, MAX_SUBSTITUTION_CHAIN_DEPTH,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    Cancelled,
    Overflow,
    Panicked,
    StaleHandle,
    Missing,
    EscapingInferenceVar,
    WrongBinderSpace,
    InvalidBinderToken,
    /// Two logical signature identities reduced to one binder-space key.
    BinderKeyCollision,
    UnresolvedCompose,
}

impl From<InternError> for StoreError {
    fn from(err: InternError) -> Self {
        match err {
            InternError::Cancelled => Self::Cancelled,
            InternError::Overflow => Self::Overflow,
            InternError::Panicked => Self::Panicked,
        }
    }
}

impl From<SubstError> for StoreError {
    fn from(err: SubstError) -> Self {
        match err {
            SubstError::EscapingInferenceVar => Self::EscapingInferenceVar,
            SubstError::WrongBinderSpace => Self::WrongBinderSpace,
            SubstError::UnresolvedCompose => Self::UnresolvedCompose,
        }
    }
}

pub(super) struct EpochInner {
    pub epoch: GraphEpoch,
    /// Every table's and side map's storage, pinned until the epoch's last
    /// holder drops (see [`EpochStorage`]).
    pub storage: Arc<EpochStorage>,
    pub shapes: AppendInterner<SignatureInputShape>,
    pub templates: AppendInterner<SignatureTemplate>,
    pub descriptors: AppendInterner<SignatureDescriptor>,
    pub recipes: AppendInterner<SignatureResultRecipe>,
    pub provenances: AppendInterner<SignatureProvenance>,
    pub substitutions: AppendInterner<CallSubstitution>,
    pub layouts: AppendInterner<ParameterLayout>,
    pub slots: AppendInterner<ParameterSlot>,
    pub spaces: AppendInterner<BinderSpace>,
    pub sets: AppendInterner<SignatureSet>,
    pub results: AppendInterner<AppliedResult>,
    pub strings: AppendInterner<Arc<str>>,
    pub sequences: AppendInterner<ConstituentSequence>,
    pub environments: AppendInterner<CanonicalTypeSubstitution>,
    pub locators: AppendInterner<u64>,
    pub type_tokens: AppendInterner<SemanticNodeId>,
    /// Each type token's graph root, or the mark a close left on a token
    /// whose root it released. The stranded count changes only under this
    /// lock, with the entry it describes.
    graph_roots: Mutex<rustc_hash::FxHashMap<u32, TokenRoot>>,
    /// Authored source node of a descriptor (graph `Signature` node token).
    /// Epoch-local, like every handle it is keyed by.
    /// Logical identity that owns each binder-space key, so a truncated key
    /// collision fails closed instead of merging unrelated binder regions.
    pub space_key_owners: Mutex<rustc_hash::FxHashMap<u64, u128>>,
    pub descriptor_sources: Mutex<rustc_hash::FxHashMap<u32, super::records::TypeToken>>,
    pub live_readers: AtomicU64,
    /// Shared across replacement epochs so `live_reader_count` includes
    /// still-pinned retired views.
    pub store_live_readers: Arc<AtomicU64>,
    pub descriptor_chain_walks: AtomicU64,
    pub apply_count: AtomicU64,
    /// Type tokens of this epoch whose graph root a close released
    /// ([`SignatureStore::note_released_nodes`]) and no intern has rooted
    /// again: every record reached through one of them is garbage no live
    /// request can name again. Exactly the [`TokenRoot::Stranded`] entries
    /// of [`Self::graph_roots`], updated under that lock.
    pub stranded_tokens: AtomicUsize,
    /// Read views and leases of this epoch alive now. Each one may read any
    /// of the epoch's tokens back to its node, so while one is alive a
    /// close moves the store's root on a closed token into
    /// [`Self::released_roots`] instead of dropping it.
    holders: AtomicU64,
    /// Roots a close took from the store while a holder of this epoch was
    /// alive, keyed by the token they rooted, so a token holds at most one
    /// root whatever its close/re-intern history: an intern of a stranded
    /// token takes its deferred root back instead of acquiring another.
    /// They are dropped when the last holder drops, or by the next close
    /// that finds none; the epoch's storage stays charged for them until
    /// then. Inserted and reclaimed only under [`Self::graph_roots`], so a
    /// [`TokenRoot::Rooted`] token never also has a deferred root.
    released_roots: Mutex<ReleasedRoots>,
    /// The graph whose nodes this epoch's tokens root; shared with the store.
    graph: Arc<OnceLock<Weak<crate::semantic_query_memo::arena::NodeArenaCore>>>,
}

/// The roots a close deferred for an epoch's holders, with the bucket
/// backing the epoch's storage is charged for. A removal can lower
/// `HashMap::capacity` through tombstones without freeing the allocation,
/// so the charged backing moves only on growth or an explicit geometric
/// shrink, never with the reported capacity.
#[derive(Default)]
struct ReleasedRoots {
    roots: rustc_hash::FxHashMap<u32, crate::semantic_query_memo::node_roots::NodeRootSet>,
    backing: usize,
}

impl ReleasedRoots {
    const ENTRY: usize =
        std::mem::size_of::<(u32, crate::semantic_query_memo::node_roots::NodeRootSet)>();

    /// Bytes `buckets` charged buckets stand for, control bytes included.
    fn backing_bytes(buckets: usize) -> usize {
        buckets * (Self::ENTRY + 1)
    }

    /// Charge `storage` for backing the table grew past what it is charged
    /// for.
    fn settle_growth(&mut self, storage: &super::storage::EpochStorage) {
        let capacity = self.roots.capacity();
        if capacity > self.backing {
            storage.add(Self::backing_bytes(capacity - self.backing));
            self.backing = capacity;
        }
    }

    /// Free the table once empty, or rebuild it at twice its entries once
    /// its backing exceeds four times them, and settle the charge. The
    /// rebuilt table holds no tombstone, so its capacity is its backing.
    fn settle_shrink(&mut self, storage: &super::storage::EpochStorage) {
        let len = self.roots.len();
        if len == 0 {
            self.roots = rustc_hash::FxHashMap::default();
        } else if self.backing > len.saturating_mul(4) {
            let mut rebuilt = rustc_hash::FxHashMap::with_capacity_and_hasher(
                len.saturating_mul(2),
                Default::default(),
            );
            rebuilt.extend(self.roots.drain());
            self.roots = rebuilt;
        } else {
            return;
        }
        let capacity = self.roots.capacity();
        if capacity < self.backing {
            storage.sub(Self::backing_bytes(self.backing - capacity));
        } else {
            storage.add(Self::backing_bytes(capacity - self.backing));
        }
        self.backing = capacity;
    }
}

/// A type token's entry in its epoch's root table.
enum TokenRoot {
    /// The root the store holds on the token's node.
    Rooted(crate::semantic_query_memo::node_roots::NodeRootSet),
    /// A close released the root; an intern of the same node roots it
    /// again.
    Stranded,
}

impl EpochInner {
    fn new(
        epoch: GraphEpoch,
        store_live_readers: Arc<AtomicU64>,
        account: &Arc<OnceLock<Arc<verter_session_query::retention::SemanticRetentionAccount>>>,
        graph: &Arc<OnceLock<Weak<crate::semantic_query_memo::arena::NodeArenaCore>>>,
    ) -> Self {
        let storage = EpochStorage::new(Arc::clone(account));
        Self {
            epoch,
            shapes: AppendInterner::charged(epoch, &storage),
            templates: AppendInterner::charged(epoch, &storage),
            descriptors: AppendInterner::charged(epoch, &storage),
            recipes: AppendInterner::charged(epoch, &storage),
            provenances: AppendInterner::charged(epoch, &storage),
            substitutions: AppendInterner::charged(epoch, &storage),
            layouts: AppendInterner::charged(epoch, &storage),
            slots: AppendInterner::charged(epoch, &storage),
            spaces: AppendInterner::charged(epoch, &storage),
            sets: AppendInterner::charged(epoch, &storage),
            results: AppendInterner::charged(epoch, &storage),
            strings: AppendInterner::charged(epoch, &storage),
            sequences: AppendInterner::charged(epoch, &storage),
            environments: AppendInterner::charged(epoch, &storage),
            locators: AppendInterner::charged(epoch, &storage),
            type_tokens: AppendInterner::charged(epoch, &storage),
            storage,
            graph_roots: Mutex::new(rustc_hash::FxHashMap::default()),
            space_key_owners: Mutex::new(rustc_hash::FxHashMap::default()),
            descriptor_sources: Mutex::new(rustc_hash::FxHashMap::default()),
            live_readers: AtomicU64::new(0),
            store_live_readers,
            descriptor_chain_walks: AtomicU64::new(0),
            apply_count: AtomicU64::new(0),
            stranded_tokens: AtomicUsize::new(0),
            holders: AtomicU64::new(0),
            released_roots: Mutex::new(ReleasedRoots::default()),
            graph: Arc::clone(graph),
        }
    }

    /// A read view or lease of this epoch now exists.
    pub(super) fn hold(&self) {
        self.holders.fetch_add(1, Ordering::SeqCst);
    }

    /// A read view or lease of this epoch dropped; the last one releases
    /// the roots a close deferred for it.
    pub(super) fn release_hold(&self) {
        if self.holders.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.drain_released_roots();
        }
    }

    /// Keep `roots`, taken from the store by a close, until no holder of
    /// this epoch can read their tokens back. The caller holds
    /// [`Self::graph_roots`] and has just marked each token stranded, so no
    /// token already has a deferred root; should one, the duplicate is
    /// returned for the caller to drop outside its locks, never kept.
    fn defer_released_roots(
        &self,
        roots: Vec<(u32, crate::semantic_query_memo::node_roots::NodeRootSet)>,
    ) -> Vec<crate::semantic_query_memo::node_roots::NodeRootSet> {
        let mut duplicates = Vec::new();
        if roots.is_empty() {
            return duplicates;
        }
        let mut released = self.released_roots.lock();
        for (index, root) in roots {
            match released.roots.entry(index) {
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(root);
                }
                std::collections::hash_map::Entry::Occupied(_) => {
                    self.storage.sub(root.heap_bytes());
                    duplicates.push(root);
                }
            }
        }
        released.settle_growth(&self.storage);
        duplicates
    }

    /// Take back the root a close deferred on token `index`, if no final
    /// holder drop has dropped it yet. The caller holds
    /// [`Self::graph_roots`] and re-roots the token with it.
    fn reclaim_released_root(
        &self,
        index: u32,
    ) -> Option<crate::semantic_query_memo::node_roots::NodeRootSet> {
        let mut released = self.released_roots.lock();
        let root = released.roots.remove(&index)?;
        released.settle_shrink(&self.storage);
        Some(root)
    }

    /// Drop the deferred roots once no holder of this epoch is alive. The
    /// holder count is read under the lock a close defers under, so a close
    /// and a final release racing each other leave nothing behind.
    fn drain_released_roots(&self) {
        let drained = {
            let mut released = self.released_roots.lock();
            if self.holders.load(Ordering::SeqCst) != 0 || released.backing == 0 {
                return;
            }
            std::mem::take(&mut *released)
        };
        let block = ReleasedRoots::backing_bytes(drained.backing);
        let heap: usize = drained.roots.values().map(|roots| roots.heap_bytes()).sum();
        // The roots drop here, outside the lock.
        drop(drained);
        self.storage.sub(heap + block);
    }

    pub(super) fn shard_lock_acquires(&self) -> u64 {
        self.shapes.shard_lock_acquires()
            + self.templates.shard_lock_acquires()
            + self.descriptors.shard_lock_acquires()
            + self.recipes.shard_lock_acquires()
            + self.provenances.shard_lock_acquires()
            + self.substitutions.shard_lock_acquires()
            + self.layouts.shard_lock_acquires()
            + self.slots.shard_lock_acquires()
            + self.spaces.shard_lock_acquires()
            + self.sets.shard_lock_acquires()
            + self.results.shard_lock_acquires()
            + self.strings.shard_lock_acquires()
            + self.sequences.shard_lock_acquires()
            + self.environments.shard_lock_acquires()
            + self.locators.shard_lock_acquires()
            + self.type_tokens.shard_lock_acquires()
    }

    fn record_count(&self) -> usize {
        self.shapes.record_count()
            + self.templates.record_count()
            + self.descriptors.record_count()
            + self.recipes.record_count()
            + self.provenances.record_count()
            + self.substitutions.record_count()
            + self.layouts.record_count()
            + self.slots.record_count()
            + self.spaces.record_count()
            + self.sets.record_count()
            + self.results.record_count()
            + self.strings.record_count()
            + self.sequences.record_count()
            + self.environments.record_count()
            + self.locators.record_count()
            + self.type_tokens.record_count()
    }
}

/// Records the current epoch's interners may hold before the edit path
/// replaces the epoch ([`SignatureStore::replace_epoch_if_over`]).
///
/// Every record is epoch-local and append-only, and type tokens name graph
/// nodes, so an edited project keeps interning records for content no
/// reader can reach again. Replacing the epoch is the only reclamation;
/// this cap bounds the garbage between two replacements. A document close
/// releases the epoch's roots on tokens naming the closed content
/// ([`SignatureStore::note_released_nodes`]), and the epoch is then
/// replaced once its stranded tokens outnumber the rest
/// ([`SignatureStore::replace_epoch_if_mostly_stranded`]), which keeps the
/// tables proportional to the live set; the cap is the bound where nothing
/// is released.
///
/// Measured on the signature-kernel benchmark corpus with every witness
/// answered: the working set is about 100 records per module (468 at 8
/// modules of depth 4, 2,438 at 24 and 9,710 at 96 modules of depth 8).
/// Reverting an edit interns nothing, since records are content-interned;
/// a UNIQUE edit of one module interns 41 to 69 records, and the editor's
/// open/edit/close churn lane measured about 11 records (about 0.5 KiB)
/// per cycle. 262,144 records, about 12 MiB at that rate, is 27 times the
/// 96-module working set: a demanded working set stays far below it, so a
/// replacement comes thousands of unique edits apart and never thrashes,
/// while a long-lived editor's kernel tables stop growing at a fixed size.
pub const EPOCH_RECORD_CAP: usize = 1 << 18;

/// Intentionally retained result: holds the epoch it was published in.
struct RetainedRoot {
    epoch: SignatureEpochLease,
    result: AppliedResult,
}

/// High bit marks binder tokens so they never collide with interned graph nodes.
const BINDER_TOKEN_NAMESPACE: u64 = 1 << 63;
const MAX_BINDER_SPACE_KEY: u64 = (1 << 31) - 1;

/// Epoch-safe signature store. Replacement publishes a new empty epoch;
/// retained results and live readers keep the old tables alive. The live
/// `ArcSwap` slot is the live-project-state root.
pub struct SignatureStore {
    /// The graph every epoch's tokens root, bound once
    /// ([`Self::bind_graph`]) and cloned into each epoch as `account` is.
    graph: Arc<OnceLock<Weak<crate::semantic_query_memo::arena::NodeArenaCore>>>,
    /// The account every epoch's storage is pinned against: the owning
    /// store's, bound before the first record is interned
    /// ([`Self::bind_retention_account`]), or the process-local one.
    account: Arc<OnceLock<Arc<verter_session_query::retention::SemanticRetentionAccount>>>,
    current: ArcSwap<EpochInner>,
    retained_results: Mutex<Vec<RetainedRoot>>,
    next_epoch: AtomicU64,
    epoch_publish: Mutex<()>,
    bodies_forced: AtomicU64,
    /// Test-only override of [`EPOCH_RECORD_CAP`]; zero keeps the constant.
    #[cfg(any(test, feature = "test-support"))]
    record_cap_for_tests: std::sync::atomic::AtomicUsize,
}

impl SignatureStore {
    #[must_use]
    pub fn new() -> Self {
        let account = Arc::new(OnceLock::new());
        let graph = Arc::new(OnceLock::new());
        let inner = Arc::new(EpochInner::new(
            GraphEpoch::FIRST,
            Arc::new(AtomicU64::new(0)),
            &account,
            &graph,
        ));
        Self {
            graph,
            account,
            current: ArcSwap::from(inner),
            retained_results: Mutex::new(Vec::new()),
            next_epoch: AtomicU64::new(GraphEpoch::FIRST.as_u32() as u64 + 1),
            epoch_publish: Mutex::new(()),
            bodies_forced: AtomicU64::new(0),
            #[cfg(any(test, feature = "test-support"))]
            record_cap_for_tests: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// The record count past which the edit path replaces the epoch.
    #[must_use]
    pub fn record_cap(&self) -> usize {
        #[cfg(any(test, feature = "test-support"))]
        {
            let cap = self.record_cap_for_tests.load(Ordering::Relaxed);
            if cap != 0 {
                return cap;
            }
        }
        EPOCH_RECORD_CAP
    }

    /// Lower the record cap so a test reaches a replacement in a few edits.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_record_cap_for_tests(&self, cap: usize) {
        self.record_cap_for_tests.store(cap, Ordering::Relaxed);
    }

    #[must_use]
    pub fn epoch(&self) -> GraphEpoch {
        self.current.load().epoch
    }

    /// Record that a body recipe was forced. Enumeration never calls this.
    pub fn note_body_forced(&self) {
        self.bodies_forced.fetch_add(1, Ordering::Relaxed);
    }

    /// How many body recipes have been forced through this store.
    #[must_use]
    pub fn bodies_forced(&self) -> u64 {
        self.bodies_forced.load(Ordering::Relaxed)
    }

    #[must_use]
    pub(super) fn pin(&self) -> Arc<EpochInner> {
        let inner = self.current.load_full();
        inner.live_readers.fetch_add(1, Ordering::Relaxed);
        inner.store_live_readers.fetch_add(1, Ordering::Relaxed);
        inner
    }

    pub(super) fn unpin(inner: &EpochInner) {
        inner.live_readers.fetch_sub(1, Ordering::Relaxed);
        inner.store_live_readers.fetch_sub(1, Ordering::Relaxed);
    }

    /// Live readers of the current epoch plus still-pinned retired epochs.
    #[must_use]
    pub fn live_reader_count(&self) -> u64 {
        self.current
            .load()
            .store_live_readers
            .load(Ordering::Relaxed)
    }

    /// Records the current epoch's interners hold, summed over every table.
    /// Retired epochs a pinned reader or retained result still holds are
    /// not counted: they are released with their last root.
    #[must_use]
    pub fn interned_len(&self) -> usize {
        self.current.load().record_count()
    }

    construction_ordinal_seam! {
        /// Release the current epoch's roots on the type tokens naming
        /// `released` — closed content no new request reaches again — so the
        /// store no longer keeps those nodes alive; every record reached
        /// through such a token is stranded. A read view or lease of the
        /// epoch alive now may still read those tokens back, so the roots
        /// are dropped only once the last one drops. A token is counted
        /// once per release of its root; re-rooting it uncounts it. Returns
        /// how many of `released` the current epoch held a root for.
        fn note_released_nodes(&self, released: impl IntoIterator<Item = SemanticNodeId>) -> usize {
            let inner = self.current.load();
            let indexes: Vec<u32> = released
                .into_iter()
                .filter_map(|node| inner.type_tokens.lookup(&node))
                .map(super::records::handle_index)
                .collect();
            let mut freed = Vec::new();
            let (stranded, duplicates) = {
                let mut roots = inner.graph_roots.lock();
                for index in indexes {
                    let Some(entry) = roots.get_mut(&index) else {
                        continue;
                    };
                    if let TokenRoot::Rooted(root) =
                        std::mem::replace(entry, TokenRoot::Stranded)
                    {
                        freed.push((index, root));
                    }
                }
                // Counted with the marks, under the lock an intern re-roots
                // under, so a concurrent re-root never misses its uncount;
                // deferred under it too, so that re-root finds the root to
                // take back.
                inner.stranded_tokens.fetch_add(freed.len(), Ordering::Relaxed);
                (freed.len(), inner.defer_released_roots(freed))
            };
            // Any duplicate drops here, outside the locks.
            drop(duplicates);
            inner.drain_released_roots();
            stranded
        }
    }

    /// The graph nodes the current epoch's type tokens hold roots on: the
    /// candidates a document close checks against the closed content.
    pub(crate) fn rooted_token_nodes(&self) -> Vec<SemanticNodeId> {
        self.current
            .load()
            .graph_roots
            .lock()
            .values()
            .filter_map(|entry| match entry {
                TokenRoot::Rooted(roots) => roots.operands().first().copied(),
                TokenRoot::Stranded => None,
            })
            .collect()
    }

    /// Type tokens of the current epoch whose graph roots a close released
    /// ([`Self::note_released_nodes`]).
    #[must_use]
    pub fn stranded_token_count(&self) -> usize {
        self.current.load().stranded_tokens.load(Ordering::Relaxed)
    }

    /// Replace the current epoch once more of its type tokens name released
    /// nodes than name live ones. The tables are append-only within an
    /// epoch, so replacement is the only reclamation, and this trigger
    /// keeps what they hold proportional to the live working set: a
    /// replacement discards at least as many stranded tokens as the live
    /// ones the next requests re-intern, so the rebuild is paid for by the
    /// garbage it drops and a replacement never follows another without
    /// that much release in between. The check repeats under the
    /// publication gate, like [`Self::replace_epoch_if_over`].
    pub fn replace_epoch_if_mostly_stranded(&self) -> Option<GraphEpoch> {
        let mostly_stranded = |inner: &EpochInner| {
            let stranded = inner.stranded_tokens.load(Ordering::Relaxed);
            stranded > 0 && stranded * 2 > inner.type_tokens.record_count()
        };
        if !mostly_stranded(&self.current.load()) {
            return None;
        }
        let _gate = self.epoch_publish.lock();
        if !mostly_stranded(&self.current.load()) {
            return None;
        }
        self.publish_next_epoch().ok()
    }

    /// Replace the current epoch. Old pinned readers keep their `Arc`.
    /// Publication is serialized so concurrent callers cannot leave a
    /// lower-numbered epoch current.
    pub fn replace_epoch(&self) -> Result<GraphEpoch, StoreError> {
        let _gate = self.epoch_publish.lock();
        self.publish_next_epoch()
    }

    /// Replace the current epoch only while its interners hold more than
    /// `cap` records. The check repeats under the publication gate, so
    /// triggers that race past the first check retire one epoch, not one
    /// each. `None`: at or under the cap, or the epoch space is exhausted
    /// (the tables then keep growing, which is retention and never a stale
    /// read).
    pub fn replace_epoch_if_over(&self, cap: usize) -> Option<GraphEpoch> {
        if self.interned_len() <= cap {
            return None;
        }
        let _gate = self.epoch_publish.lock();
        if self.interned_len() <= cap {
            return None;
        }
        self.publish_next_epoch().ok()
    }

    /// Publish a new empty epoch. The caller holds `epoch_publish`.
    fn publish_next_epoch(&self) -> Result<GraphEpoch, StoreError> {
        let next_raw = self.next_epoch.fetch_add(1, Ordering::Relaxed);
        let next = u32::try_from(next_raw).map_err(|_| StoreError::Overflow)?;
        if next == 0 {
            return Err(StoreError::Overflow);
        }
        let epoch = GraphEpoch::from_raw(next);
        let store_live_readers = Arc::clone(&self.current.load().store_live_readers);
        self.current.store(Arc::new(EpochInner::new(
            epoch,
            store_live_readers,
            &self.account,
            &self.graph,
        )));
        Ok(epoch)
    }

    /// Pin `result` as a lifetime root of the epoch it was published in.
    pub fn retain_result(&self, result: AppliedResult) -> Result<(), StoreError> {
        let inner = self.current.load_full();
        inner.check_applied(&result)?;
        self.retained_results.lock().push(RetainedRoot {
            epoch: SignatureEpochLease::from_epoch(inner),
            result,
        });
        Ok(())
    }

    pub fn drain_retained(&self) {
        self.retained_results.lock().clear();
    }

    #[must_use]
    pub fn retained_len(&self) -> usize {
        self.retained_results.lock().len()
    }

    /// Read a retained result's descriptor against its pinned epoch.
    pub fn retained_descriptor(&self, index: usize) -> Result<SignatureDescriptor, StoreError> {
        let guard = self.retained_results.lock();
        let entry = guard.get(index).ok_or(StoreError::Missing)?;
        check_epoch(entry.epoch.inner.epoch, entry.result.descriptor.epoch())?;
        entry
            .epoch
            .inner
            .descriptors
            .get(entry.result.descriptor.index())
            .copied()
            .ok_or(StoreError::Missing)
    }

    pub fn retained_result(&self, index: usize) -> Result<AppliedResult, StoreError> {
        let guard = self.retained_results.lock();
        let entry = guard.get(index).ok_or(StoreError::Missing)?;
        Ok(entry.result.clone())
    }

    fn inner(&self) -> arc_swap::Guard<Arc<EpochInner>> {
        self.current.load()
    }

    /// Intern `node`'s token record without its root: the state a
    /// concurrent [`Self::intern_type_token`] leaves between interning the
    /// record and rooting it.
    #[cfg(test)]
    pub(crate) fn intern_unrooted_type_token_for_tests(&self, node: SemanticNodeId) {
        self.inner()
            .type_tokens
            .intern(node, None)
            .expect("a test token interns");
    }

    /// The epoch current now, held open for reading and publication.
    #[must_use]
    pub fn bind_current(&self) -> EpochBinding {
        EpochBinding {
            view: super::read_view::SemanticReadView::pin(self),
        }
    }

    /// `epoch` held open for reading and publication, while it is the
    /// current one; a retired epoch is a stale handle, as for
    /// [`Self::lease_epoch`].
    pub fn bind_epoch(&self, epoch: GraphEpoch) -> Result<EpochBinding, StoreError> {
        let inner = self.current.load_full();
        check_epoch(inner.epoch, epoch)?;
        Ok(EpochBinding {
            view: super::read_view::SemanticReadView::from_epoch(inner),
        })
    }

    pub fn intern_spelling(
        &self,
        s: &str,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SpellingId, StoreError> {
        self.inner().intern_spelling(s, cancelled)
    }

    pub fn intern_body_locator(
        &self,
        locator: u64,
        cancelled: Option<&AtomicBool>,
    ) -> Result<BodyLocatorId, StoreError> {
        self.inner().intern_body_locator(locator, cancelled)
    }

    construction_ordinal_seam! {
        /// Mint the kernel token standing for graph node `node`. Tokens live in
        /// the kernel's own numbering space: the raw value is an epoch-qualified
        /// intern handle, never a node ordinal. Minting roots `node`, so it
        /// belongs to signature discovery inside the engine; a bare ordinal
        /// never mints an owning token from outside it.
        fn intern_type_token(
            &self,
            node: SemanticNodeId,
            cancelled: Option<&AtomicBool>,
        ) -> Result<super::records::TypeToken, StoreError> {
            self.inner().intern_type_token(node, cancelled)
        }
    }

    /// The construction ordinal a token stands for. A token from another
    /// epoch is stale. Tokens are kernel-internal shape identity: nothing
    /// outside the engine resolves one, and test builds expose the ordinal
    /// as a seam.
    #[cfg(any(test, feature = "test-support"))]
    pub fn type_token_node(
        &self,
        token: super::records::TypeToken,
    ) -> Result<SemanticNodeId, StoreError> {
        self.inner().type_token_ordinal(token)
    }

    /// See the test-build variant.
    #[cfg(not(any(test, feature = "test-support")))]
    pub(crate) fn type_token_node(
        &self,
        token: super::records::TypeToken,
    ) -> Result<SemanticNodeId, StoreError> {
        self.inner().type_token_ordinal(token)
    }

    /// Record the graph node a descriptor was authored from. Idempotent:
    /// the same descriptor always maps to the same node token.
    pub fn record_descriptor_source(
        &self,
        descriptor: SignatureDescriptorId,
        source: super::records::TypeToken,
    ) -> Result<(), StoreError> {
        self.inner().record_descriptor_source(descriptor, source)
    }

    pub fn descriptor_source(
        &self,
        descriptor: SignatureDescriptorId,
    ) -> Result<Option<super::records::TypeToken>, StoreError> {
        self.inner().descriptor_source(descriptor)
    }

    pub fn intern_slot(
        &self,
        slot: ParameterSlot,
        cancelled: Option<&AtomicBool>,
    ) -> Result<ParameterSlotId, StoreError> {
        self.inner().intern_slot(slot, cancelled)
    }

    pub fn intern_layout(
        &self,
        layout: ParameterLayout,
        cancelled: Option<&AtomicBool>,
    ) -> Result<ParameterLayoutId, StoreError> {
        self.inner().intern_layout(layout, cancelled)
    }

    pub fn intern_binder_space(
        &self,
        space: BinderSpace,
        cancelled: Option<&AtomicBool>,
    ) -> Result<BinderSpaceId, StoreError> {
        self.inner().intern_binder_space(space, cancelled)
    }

    /// Claim `key` for the logical signature identity `identity`. A second,
    /// different identity under the same key is a collision.
    pub fn claim_space_key(&self, key: u64, identity: u128) -> Result<(), StoreError> {
        self.inner().claim_space_key(key, identity)
    }

    pub fn intern_environment(
        &self,
        env: CanonicalTypeSubstitution,
        cancelled: Option<&AtomicBool>,
    ) -> Result<DeclarationInstantiationId, StoreError> {
        self.inner().intern_environment(env, cancelled)
    }

    pub fn intern_shape(
        &self,
        shape: SignatureInputShape,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureInputShapeId, StoreError> {
        self.inner().intern_shape(shape, cancelled)
    }

    pub fn intern_recipe(
        &self,
        recipe: SignatureResultRecipe,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureResultRecipeId, StoreError> {
        self.inner().intern_recipe(recipe, cancelled)
    }

    pub fn intern_template(
        &self,
        template: SignatureTemplate,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureTemplateId, StoreError> {
        self.inner().intern_template(template, cancelled)
    }

    pub fn intern_descriptor(
        &self,
        descriptor: SignatureDescriptor,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureDescriptorId, StoreError> {
        self.inner().intern_descriptor(descriptor, cancelled)
    }

    pub fn intern_provenance(
        &self,
        provenance: SignatureProvenance,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureProvenanceId, StoreError> {
        self.inner().intern_provenance(provenance, cancelled)
    }

    pub fn intern_sequence(
        &self,
        sequence: ConstituentSequence,
        cancelled: Option<&AtomicBool>,
    ) -> Result<super::records::ConstituentSequenceId, StoreError> {
        self.inner().intern_sequence(sequence, cancelled)
    }

    pub fn intern_set(
        &self,
        candidates: Box<[SignatureCandidate]>,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureSetId, StoreError> {
        self.inner().intern_set(candidates, cancelled)
    }

    pub fn intern_substitution(
        &self,
        subst: CallSubstitution,
        cancelled: Option<&AtomicBool>,
    ) -> Result<CallSubstitutionId, StoreError> {
        self.inner().intern_substitution(subst, cancelled)
    }

    pub fn intern_with_shape<F>(
        &self,
        cancelled: Option<&AtomicBool>,
        build: F,
    ) -> Result<SignatureInputShapeId, StoreError>
    where
        F: FnOnce() -> SignatureInputShape,
    {
        if cancelled.is_some_and(|c| c.load(Ordering::Acquire)) {
            return Err(StoreError::Cancelled);
        }
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(build)) {
            Ok(shape) => self.intern_shape(shape, cancelled),
            Err(_) => Err(StoreError::Panicked),
        }
    }

    /// `compose_after(first, second)` interned. Identity is elided; chains
    /// deeper than [`MAX_SUBSTITUTION_CHAIN_DEPTH`] flatten. Requires
    /// `first.codomain == second.domain`.
    pub fn compose_after(
        &self,
        first: CallSubstitutionId,
        second: CallSubstitutionId,
        cancelled: Option<&AtomicBool>,
    ) -> Result<CallSubstitutionId, StoreError> {
        let inner = self.inner();
        let a = inner.subst(first)?;
        let b = inner.subst(second)?;
        if a.codomain() != b.domain() {
            return Err(StoreError::WrongBinderSpace);
        }
        if a.is_same_space_identity() {
            return Ok(second);
        }
        if b.is_same_space_identity() {
            return Ok(first);
        }
        let depth = compose_depth(a).max(compose_depth(b)).saturating_add(1);
        if depth >= MAX_SUBSTITUTION_CHAIN_DEPTH {
            let flat = inner.flatten_subst(first, second)?;
            return self.intern_substitution(flat, cancelled);
        }
        self.intern_substitution(
            CallSubstitution::Compose {
                domain: a.domain(),
                codomain: b.codomain(),
                first,
                second,
                depth,
            },
            cancelled,
        )
    }

    /// Apply `subst` to `term`. Compose nodes apply first then second.
    pub fn apply(
        &self,
        subst: CallSubstitutionId,
        term: &SubstTerm,
    ) -> Result<SubstTerm, StoreError> {
        let inner = self.inner();
        inner.apply_count.fetch_add(1, Ordering::Relaxed);
        Self::apply_inner(&inner, subst, term)
    }

    pub(super) fn apply_inner(
        inner: &EpochInner,
        subst: CallSubstitutionId,
        term: &SubstTerm,
    ) -> Result<SubstTerm, StoreError> {
        check_epoch(inner.epoch, subst.epoch())?;
        let node = inner
            .substitutions
            .get(subst.index())
            .ok_or(StoreError::Missing)?;
        match node {
            CallSubstitution::Compose { first, second, .. } => {
                inner.descriptor_chain_walks.fetch_add(1, Ordering::Relaxed);
                let mid = Self::apply_inner(inner, *first, term)?;
                Self::apply_inner(inner, *second, &mid)
            }
            other => Ok(other.apply_term(term)?),
        }
    }

    /// Publish a result already in call space. Warm reads do not re-apply.
    pub fn publish_result(
        &self,
        result: AppliedResult,
        cancelled: Option<&AtomicBool>,
    ) -> Result<u64, StoreError> {
        self.inner().publish_result(result, cancelled)
    }

    /// Look up a previously published result. Does not intern on miss.
    pub fn lookup_result(&self, result: &AppliedResult) -> Result<Option<u64>, StoreError> {
        let inner = self.inner();
        inner.check_applied(result)?;
        Ok(inner.results.lookup(result))
    }

    /// The whole substitution as one canonical map (composes flatten).
    pub fn flatten_substitution(
        &self,
        id: CallSubstitutionId,
    ) -> Result<CanonicalTypeSubstitution, StoreError> {
        self.inner().flatten_substitution(id)
    }

    /// The published result record behind a handle.
    pub fn applied_result(
        &self,
        id: super::records::AppliedResultId,
    ) -> Result<AppliedResult, StoreError> {
        let inner = self.inner();
        check_epoch(inner.epoch, id.epoch())?;
        inner
            .results
            .get(id.index())
            .cloned()
            .ok_or(StoreError::Missing)
    }

    pub fn substitution(&self, id: CallSubstitutionId) -> Result<CallSubstitution, StoreError> {
        let inner = self.inner();
        Ok(inner.subst(id)?.clone())
    }

    #[must_use]
    pub fn apply_count(&self) -> u64 {
        self.inner().apply_count.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn descriptor_chain_walks(&self) -> u64 {
        self.inner().descriptor_chain_walks.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn shard_lock_acquires(&self) -> u64 {
        self.inner().shard_lock_acquires()
    }

    pub fn candidate(
        &self,
        signature: SignatureDescriptorId,
        provenance: SignatureProvenanceId,
    ) -> Result<SignatureCandidate, StoreError> {
        self.inner().candidate(signature, provenance)
    }

    #[must_use]
    pub fn set_ref_one(&self, candidate: SignatureCandidate) -> SignatureSetRef {
        SignatureSetRef::One(candidate)
    }

    pub fn set_ref_many(
        &self,
        candidates: Box<[SignatureCandidate]>,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureSetRef, StoreError> {
        self.inner().set_ref_many(candidates, cancelled)
    }

    /// Whether `node` lives in the kernel's binder-token namespace.
    #[must_use]
    pub fn is_binder_token(node: SemanticNodeId) -> bool {
        node.0 & BINDER_TOKEN_NAMESPACE != 0
    }

    /// Decode an ordinal only through the binder-token namespace boundary.
    #[must_use]
    pub fn binder_token_ordinal(node: SemanticNodeId) -> Option<u32> {
        Self::is_binder_token(node).then_some(node.0 as u32)
    }

    /// Logical binder token from a space key and ordinal. Independent of
    /// intern order. High bit is a dedicated namespace so tokens never
    /// collide with interned graph nodes. Rejects keys that do not fit.
    pub fn binder_token(space_key: u64, ordinal: u32) -> Result<SemanticNodeId, StoreError> {
        if space_key > MAX_BINDER_SPACE_KEY {
            return Err(StoreError::InvalidBinderToken);
        }
        Ok(SemanticNodeId(
            BINDER_TOKEN_NAMESPACE | (space_key << 32) | u64::from(ordinal),
        ))
    }

    /// Binder token for an interned space: identity is the space's logical
    /// key plus ordinal, not the intern slot.
    pub fn binder_token_for(
        &self,
        space: BinderSpaceId,
        ordinal: u32,
    ) -> Result<SemanticNodeId, StoreError> {
        let inner = self.inner();
        inner.require_id(space.epoch(), space.index(), &inner.spaces)?;
        let rec = inner.spaces.get(space.index()).ok_or(StoreError::Missing)?;
        Self::binder_token(rec.key, ordinal)
    }
}

/// Interning and reads against one epoch's tables, whichever epoch the
/// store has made current since: [`SignatureStore`] delegates here with its
/// current epoch, [`EpochBinding`] with the epoch it holds.
impl EpochInner {
    #[inline]
    fn require_id<T>(
        &self,
        epoch: GraphEpoch,
        index: u32,
        table: &AppendInterner<T>,
    ) -> Result<(), StoreError> {
        check_epoch(self.epoch, epoch)?;
        if table.get(index).is_none() {
            Err(StoreError::Missing)
        } else {
            Ok(())
        }
    }

    fn check_applied(&self, result: &AppliedResult) -> Result<(), StoreError> {
        self.require_id(
            result.descriptor.epoch(),
            result.descriptor.index(),
            &self.descriptors,
        )?;
        self.require_id(
            result.substitution.epoch(),
            result.substitution.index(),
            &self.substitutions,
        )?;
        self.require_id(result.recipe.epoch(), result.recipe.index(), &self.recipes)?;
        for token in result
            .return_type
            .into_iter()
            .chain(result.effects.and_then(|effect| effect.ty))
        {
            let raw = token.as_u64();
            self.require_id(
                super::records::handle_epoch(raw),
                super::records::handle_index(raw),
                &self.type_tokens,
            )?;
        }
        Ok(())
    }

    fn check_slot(&self, slot: &ParameterSlot) -> Result<(), StoreError> {
        let raw = slot.ty.as_u64();
        self.require_id(
            super::records::handle_epoch(raw),
            super::records::handle_index(raw),
            &self.type_tokens,
        )?;
        match slot.name {
            Some(name) => self.require_id(name.epoch(), name.index(), &self.strings),
            None => Ok(()),
        }
    }

    fn check_shape(&self, shape: &SignatureInputShape) -> Result<(), StoreError> {
        self.require_id(
            shape.binder_declarations.epoch(),
            shape.binder_declarations.index(),
            &self.spaces,
        )?;
        self.require_id(
            shape.parameter_layout.epoch(),
            shape.parameter_layout.index(),
            &self.layouts,
        )?;
        if let Some(slot) = shape.this_parameter {
            self.require_id(slot.epoch(), slot.index(), &self.slots)?;
        }
        Ok(())
    }

    fn check_template(&self, template: &SignatureTemplate) -> Result<(), StoreError> {
        self.require_id(
            template.input_shape.epoch(),
            template.input_shape.index(),
            &self.shapes,
        )?;
        self.require_id(
            template.result_recipe.epoch(),
            template.result_recipe.index(),
            &self.recipes,
        )
    }

    fn check_descriptor(&self, descriptor: &SignatureDescriptor) -> Result<(), StoreError> {
        self.require_id(
            descriptor.template.epoch(),
            descriptor.template.index(),
            &self.templates,
        )?;
        self.require_id(
            descriptor.declaration_environment.epoch(),
            descriptor.declaration_environment.index(),
            &self.environments,
        )?;
        self.require_id(
            descriptor.residual_binders.epoch(),
            descriptor.residual_binders.index(),
            &self.spaces,
        )
    }

    fn check_candidate(&self, candidate: &SignatureCandidate) -> Result<(), StoreError> {
        self.require_id(
            candidate.signature.epoch(),
            candidate.signature.index(),
            &self.descriptors,
        )?;
        self.require_id(
            candidate.provenance.epoch(),
            candidate.provenance.index(),
            &self.provenances,
        )
    }

    fn check_subst_node(&self, subst: &CallSubstitution) -> Result<(), StoreError> {
        match subst {
            CallSubstitution::Compose {
                domain,
                codomain,
                first,
                second,
                ..
            } => {
                self.require_id(first.epoch(), first.index(), &self.substitutions)?;
                self.require_id(second.epoch(), second.index(), &self.substitutions)?;
                self.require_id(domain.epoch(), domain.index(), &self.spaces)?;
                self.require_id(codomain.epoch(), codomain.index(), &self.spaces)
            }
            CallSubstitution::Identity { domain, codomain }
            | CallSubstitution::Map {
                domain, codomain, ..
            } => {
                self.require_id(domain.epoch(), domain.index(), &self.spaces)?;
                self.require_id(codomain.epoch(), codomain.index(), &self.spaces)
            }
        }
    }

    #[inline]
    pub(super) fn intern_spelling(
        &self,
        s: &str,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SpellingId, StoreError> {
        let raw = self.strings.intern(Arc::<str>::from(s), cancelled)?;
        Ok(SpellingId::from_raw(raw))
    }

    #[inline]
    pub(super) fn intern_body_locator(
        &self,
        locator: u64,
        cancelled: Option<&AtomicBool>,
    ) -> Result<BodyLocatorId, StoreError> {
        let raw = self.locators.intern(locator, cancelled)?;
        Ok(BodyLocatorId::from_raw(raw))
    }

    /// Mint the token standing for graph node `node` in this epoch,
    /// rooting `node` here.
    pub(super) fn intern_type_token(
        &self,
        node: SemanticNodeId,
        cancelled: Option<&AtomicBool>,
    ) -> Result<super::records::TypeToken, StoreError> {
        let root = self
            .graph
            .get()
            .and_then(Weak::upgrade)
            .map(|arena| {
                let nodes = if SignatureStore::is_binder_token(node) {
                    crate::semantic_query_memo::node_roots::RootIds::new()
                } else {
                    smallvec::smallvec![node]
                };
                crate::semantic_query_memo::node_roots::NodeRootSet::acquire(arena, nodes)
                    .map_err(|_| StoreError::Missing)
            })
            .transpose()?;
        let raw = self.type_tokens.intern(node, cancelled)?;
        if let Some(root) = root {
            // The root this intern does not keep drops after the lock.
            let mut spare = None;
            let mut roots = self.graph_roots.lock();
            let before = roots.capacity();
            match roots.entry(super::records::handle_index(raw)) {
                std::collections::hash_map::Entry::Vacant(slot) => {
                    self.storage.add(root.heap_bytes());
                    slot.insert(TokenRoot::Rooted(root));
                }
                std::collections::hash_map::Entry::Occupied(mut slot) => {
                    if matches!(slot.get(), TokenRoot::Stranded) {
                        // A close stranded this token; rooting it again
                        // makes it live, decided under the lock the
                        // close marked it under. A root the close
                        // deferred for a holder is taken back, so
                        // repeated close/re-intern never stacks roots.
                        let kept = match self.reclaim_released_root(*slot.key()) {
                            Some(deferred) => {
                                spare = Some(root);
                                deferred
                            }
                            None => {
                                self.storage.add(root.heap_bytes());
                                root
                            }
                        };
                        slot.insert(TokenRoot::Rooted(kept));
                        self.stranded_tokens.fetch_sub(1, Ordering::Relaxed);
                    }
                }
            }
            self.storage.map_growth(
                before,
                roots.capacity(),
                std::mem::size_of::<(u32, TokenRoot)>(),
            );
            drop(roots);
            drop(spare);
        }
        Ok(super::records::TypeToken::from_raw(raw))
    }

    /// The construction ordinal a token of this epoch stands for.
    #[inline]
    pub(super) fn type_token_ordinal(
        &self,
        token: super::records::TypeToken,
    ) -> Result<SemanticNodeId, StoreError> {
        let raw = token.as_u64();
        check_epoch(self.epoch, super::records::handle_epoch(raw))?;
        self.type_tokens
            .get(super::records::handle_index(raw))
            .copied()
            .ok_or(StoreError::Missing)
    }

    pub(super) fn record_descriptor_source(
        &self,
        descriptor: SignatureDescriptorId,
        source: super::records::TypeToken,
    ) -> Result<(), StoreError> {
        self.require_id(descriptor.epoch(), descriptor.index(), &self.descriptors)?;
        let raw = source.as_u64();
        self.require_id(
            super::records::handle_epoch(raw),
            super::records::handle_index(raw),
            &self.type_tokens,
        )?;
        let mut sources = self.descriptor_sources.lock();
        let before = sources.capacity();
        sources.entry(descriptor.index()).or_insert(source);
        self.storage.map_growth(
            before,
            sources.capacity(),
            std::mem::size_of::<(u32, super::records::TypeToken)>(),
        );
        Ok(())
    }

    pub(super) fn descriptor_source(
        &self,
        descriptor: SignatureDescriptorId,
    ) -> Result<Option<super::records::TypeToken>, StoreError> {
        check_epoch(self.epoch, descriptor.epoch())?;
        let found = self
            .descriptor_sources
            .lock()
            .get(&descriptor.index())
            .copied();
        Ok(found)
    }

    #[inline]
    pub(super) fn intern_slot(
        &self,
        slot: ParameterSlot,
        cancelled: Option<&AtomicBool>,
    ) -> Result<ParameterSlotId, StoreError> {
        self.check_slot(&slot)?;
        let raw = self.slots.intern(slot, cancelled)?;
        Ok(ParameterSlotId::from_raw(raw))
    }

    pub(super) fn intern_layout(
        &self,
        layout: ParameterLayout,
        cancelled: Option<&AtomicBool>,
    ) -> Result<ParameterLayoutId, StoreError> {
        for slot in layout.parameters.iter().chain(
            layout
                .rest
                .iter()
                .flat_map(|r| std::iter::once(&r.slot).chain(r.tail.iter())),
        ) {
            self.check_slot(slot)?;
        }
        let raw = self.layouts.intern(layout, cancelled)?;
        Ok(ParameterLayoutId::from_raw(raw))
    }

    pub(super) fn intern_binder_space(
        &self,
        space: BinderSpace,
        cancelled: Option<&AtomicBool>,
    ) -> Result<BinderSpaceId, StoreError> {
        for binder in space.binders.iter() {
            self.require_id(
                binder.spelling.epoch(),
                binder.spelling.index(),
                &self.strings,
            )?;
        }
        let raw = self.spaces.intern(space, cancelled)?;
        Ok(BinderSpaceId::from_raw(raw))
    }

    pub(super) fn claim_space_key(&self, key: u64, identity: u128) -> Result<(), StoreError> {
        let mut owners = self.space_key_owners.lock();
        let before = owners.capacity();
        let owner = *owners.entry(key).or_insert(identity);
        self.storage.map_growth(
            before,
            owners.capacity(),
            std::mem::size_of::<(u64, u128)>(),
        );
        match owner {
            owner if owner == identity => Ok(()),
            _ => Err(StoreError::BinderKeyCollision),
        }
    }

    /// The logical identity that claimed binder-space `key` in this epoch.
    pub(super) fn space_key_owner(&self, key: u64) -> Option<u128> {
        self.space_key_owners.lock().get(&key).copied()
    }

    #[inline]
    pub(super) fn intern_environment(
        &self,
        env: CanonicalTypeSubstitution,
        cancelled: Option<&AtomicBool>,
    ) -> Result<DeclarationInstantiationId, StoreError> {
        let raw = self.environments.intern(env, cancelled)?;
        Ok(DeclarationInstantiationId::from_raw(raw))
    }

    #[inline]
    pub(super) fn intern_shape(
        &self,
        shape: SignatureInputShape,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureInputShapeId, StoreError> {
        self.check_shape(&shape)?;
        let raw = self.shapes.intern(shape, cancelled)?;
        Ok(SignatureInputShapeId::from_raw(raw))
    }

    pub(super) fn intern_recipe(
        &self,
        recipe: SignatureResultRecipe,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureResultRecipeId, StoreError> {
        match recipe {
            SignatureResultRecipe::Body {
                return_obligation_key,
            } => {
                self.require_id(
                    return_obligation_key.body_locator.epoch(),
                    return_obligation_key.body_locator.index(),
                    &self.locators,
                )?;
            }
            SignatureResultRecipe::UnionCommon { constituents, .. }
            | SignatureResultRecipe::UnionSynthesized { constituents, .. }
            | SignatureResultRecipe::IntersectionConstruct {
                mixins: constituents,
                ..
            } => {
                self.require_id(constituents.epoch(), constituents.index(), &self.sequences)?;
            }
            SignatureResultRecipe::Declared { .. } => {}
        }
        let raw = self.recipes.intern(recipe, cancelled)?;
        Ok(SignatureResultRecipeId::from_raw(raw))
    }

    #[inline]
    pub(super) fn intern_template(
        &self,
        template: SignatureTemplate,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureTemplateId, StoreError> {
        self.check_template(&template)?;
        let raw = self.templates.intern(template, cancelled)?;
        Ok(SignatureTemplateId::from_raw(raw))
    }

    #[inline]
    pub(super) fn intern_descriptor(
        &self,
        descriptor: SignatureDescriptor,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureDescriptorId, StoreError> {
        self.check_descriptor(&descriptor)?;
        let raw = self.descriptors.intern(descriptor, cancelled)?;
        Ok(SignatureDescriptorId::from_raw(raw))
    }

    pub(super) fn intern_provenance(
        &self,
        provenance: SignatureProvenance,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureProvenanceId, StoreError> {
        match provenance.origin {
            super::provenance::OriginRelation::Synthesized { from }
            | super::provenance::OriginRelation::Instantiated { from } => {
                self.require_id(from.epoch(), from.index(), &self.provenances)?;
            }
            super::provenance::OriginRelation::Authored => {}
        }
        let raw = self.provenances.intern(provenance, cancelled)?;
        Ok(SignatureProvenanceId::from_raw(raw))
    }

    pub(super) fn intern_sequence(
        &self,
        sequence: ConstituentSequence,
        cancelled: Option<&AtomicBool>,
    ) -> Result<super::records::ConstituentSequenceId, StoreError> {
        for edge in sequence.edges.iter() {
            self.require_id(
                edge.declaration.epoch(),
                edge.declaration.index(),
                &self.descriptors,
            )?;
            self.require_id(
                edge.residual.epoch(),
                edge.residual.index(),
                &self.descriptors,
            )?;
            self.require_id(
                edge.arm.contributor.epoch(),
                edge.arm.contributor.index(),
                &self.descriptors,
            )?;
        }
        let raw = self.sequences.intern(sequence, cancelled)?;
        Ok(super::records::ConstituentSequenceId::from_raw(raw))
    }

    pub(super) fn intern_set(
        &self,
        candidates: Box<[SignatureCandidate]>,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureSetId, StoreError> {
        for candidate in candidates.iter() {
            self.check_candidate(candidate)?;
        }
        let raw = self.sets.intern(SignatureSet { candidates }, cancelled)?;
        Ok(SignatureSetId::from_raw(raw))
    }

    pub(super) fn set_ref_many(
        &self,
        candidates: Box<[SignatureCandidate]>,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureSetRef, StoreError> {
        Ok(SignatureSetRef::Many(
            self.intern_set(candidates, cancelled)?,
        ))
    }

    pub(super) fn candidate(
        &self,
        signature: SignatureDescriptorId,
        provenance: SignatureProvenanceId,
    ) -> Result<SignatureCandidate, StoreError> {
        check_epoch(self.epoch, signature.epoch())?;
        check_epoch(self.epoch, provenance.epoch())?;
        if self.descriptors.get(signature.index()).is_none() {
            return Err(StoreError::Missing);
        }
        if self.provenances.get(provenance.index()).is_none() {
            return Err(StoreError::Missing);
        }
        Ok(SignatureCandidate {
            signature,
            provenance,
        })
    }

    pub(super) fn intern_substitution(
        &self,
        subst: CallSubstitution,
        cancelled: Option<&AtomicBool>,
    ) -> Result<CallSubstitutionId, StoreError> {
        let subst = self.canonicalize_subst(subst)?;
        self.check_subst_node(&subst)?;
        let raw = self.substitutions.intern(subst, cancelled)?;
        Ok(CallSubstitutionId::from_raw(raw))
    }

    /// Recompute Compose depth/domain/codomain from operands so a
    /// hand-built node cannot bypass chain-depth control or embed a
    /// stale space. Identity children are elided; chains past the bound
    /// flatten. Same-space identity elision matches `compose_after`.
    fn canonicalize_subst(&self, subst: CallSubstitution) -> Result<CallSubstitution, StoreError> {
        match subst {
            CallSubstitution::Compose { first, second, .. } => {
                self.require_id(first.epoch(), first.index(), &self.substitutions)?;
                self.require_id(second.epoch(), second.index(), &self.substitutions)?;
                let a = self.subst(first)?;
                let b = self.subst(second)?;
                if a.codomain() != b.domain() {
                    return Err(StoreError::WrongBinderSpace);
                }
                if a.is_same_space_identity() {
                    return Ok(b.clone());
                }
                if b.is_same_space_identity() {
                    return Ok(a.clone());
                }
                let depth = compose_depth(a).max(compose_depth(b)).saturating_add(1);
                if depth >= MAX_SUBSTITUTION_CHAIN_DEPTH {
                    return self.flatten_subst(first, second);
                }
                Ok(CallSubstitution::Compose {
                    domain: a.domain(),
                    codomain: b.codomain(),
                    first,
                    second,
                    depth,
                })
            }
            other => Ok(other),
        }
    }

    fn subst(&self, id: CallSubstitutionId) -> Result<&CallSubstitution, StoreError> {
        check_epoch(self.epoch, id.epoch())?;
        self.substitutions
            .get(id.index())
            .ok_or(StoreError::Missing)
    }

    fn flatten_subst(
        &self,
        first: CallSubstitutionId,
        second: CallSubstitutionId,
    ) -> Result<CallSubstitution, StoreError> {
        let a = self.flatten_to_map(first)?;
        let b = self.flatten_to_map(second)?;
        self.descriptor_chain_walks.fetch_add(1, Ordering::Relaxed);
        Ok(CallSubstitution::map_across(
            a.domain(),
            b.codomain(),
            compose_canonical(a_map(&a), a_map(&b)),
        ))
    }

    fn flatten_to_map(&self, id: CallSubstitutionId) -> Result<CallSubstitution, StoreError> {
        let node = self.subst(id)?;
        match node {
            CallSubstitution::Identity { domain, codomain } => {
                Ok(CallSubstitution::identity_across(*domain, *codomain))
            }
            CallSubstitution::Map {
                domain,
                codomain,
                map,
            } => Ok(CallSubstitution::map_across(
                *domain,
                *codomain,
                map.clone(),
            )),
            CallSubstitution::Compose { first, second, .. } => self.flatten_subst(*first, *second),
        }
    }

    pub(super) fn publish_result(
        &self,
        result: AppliedResult,
        cancelled: Option<&AtomicBool>,
    ) -> Result<u64, StoreError> {
        self.check_applied(&result)?;
        Ok(self.results.intern(result, cancelled)?)
    }

    pub(super) fn flatten_substitution(
        &self,
        id: CallSubstitutionId,
    ) -> Result<CanonicalTypeSubstitution, StoreError> {
        let flat = self.flatten_to_map(id)?;
        Ok(a_map(&flat).clone())
    }

    /// The arena of the graph this epoch's tokens root, which issues every
    /// node operand its records name.
    pub(super) fn graph_arena(
        &self,
    ) -> Option<Arc<crate::semantic_query_memo::arena::NodeArenaCore>> {
        self.graph.get().and_then(Weak::upgrade)
    }
}

/// One epoch of the store, held open for reading and publication. Every
/// record read or interned through it belongs to this epoch, whichever
/// epoch the store has made current since: a computation binds once and
/// works in that epoch for its whole life, so a replacement mid-way never
/// leaves it with handles of two epochs. Holding a binding keeps its epoch's
/// tables readable, as a read view does.
pub struct EpochBinding {
    view: super::read_view::SemanticReadView,
}

impl std::fmt::Debug for EpochBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("EpochBinding").field(&self.epoch()).finish()
    }
}

impl EpochBinding {
    /// The bound epoch's read view.
    #[must_use]
    pub fn view(&self) -> &super::read_view::SemanticReadView {
        &self.view
    }

    /// The bound epoch.
    #[must_use]
    pub fn epoch(&self) -> GraphEpoch {
        self.view.epoch()
    }

    /// A lease of the bound epoch, readable after it retires.
    #[must_use]
    pub fn lease(&self) -> SignatureEpochLease {
        self.view.lease()
    }

    #[inline]
    fn inner(&self) -> &EpochInner {
        self.view.epoch_inner()
    }

    pub fn intern_spelling(
        &self,
        s: &str,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SpellingId, StoreError> {
        self.inner().intern_spelling(s, cancelled)
    }

    pub fn intern_body_locator(
        &self,
        locator: u64,
        cancelled: Option<&AtomicBool>,
    ) -> Result<BodyLocatorId, StoreError> {
        self.inner().intern_body_locator(locator, cancelled)
    }

    construction_ordinal_seam! {
        /// Mint the token standing for graph node `node` in the bound
        /// epoch, rooting `node` there ([`SignatureStore::intern_type_token`]).
        fn intern_type_token(
            &self,
            node: SemanticNodeId,
            cancelled: Option<&AtomicBool>,
        ) -> Result<super::records::TypeToken, StoreError> {
            self.inner().intern_type_token(node, cancelled)
        }
    }

    /// The construction ordinal a token of the bound epoch stands for.
    #[cfg(any(test, feature = "test-support"))]
    pub fn type_token_node(
        &self,
        token: super::records::TypeToken,
    ) -> Result<SemanticNodeId, StoreError> {
        self.inner().type_token_ordinal(token)
    }

    /// See the test-build variant.
    #[cfg(not(any(test, feature = "test-support")))]
    pub(crate) fn type_token_node(
        &self,
        token: super::records::TypeToken,
    ) -> Result<SemanticNodeId, StoreError> {
        self.inner().type_token_ordinal(token)
    }

    pub fn record_descriptor_source(
        &self,
        descriptor: SignatureDescriptorId,
        source: super::records::TypeToken,
    ) -> Result<(), StoreError> {
        self.inner().record_descriptor_source(descriptor, source)
    }

    pub fn descriptor_source(
        &self,
        descriptor: SignatureDescriptorId,
    ) -> Result<Option<super::records::TypeToken>, StoreError> {
        self.inner().descriptor_source(descriptor)
    }

    pub fn intern_slot(
        &self,
        slot: ParameterSlot,
        cancelled: Option<&AtomicBool>,
    ) -> Result<ParameterSlotId, StoreError> {
        self.inner().intern_slot(slot, cancelled)
    }

    pub fn intern_layout(
        &self,
        layout: ParameterLayout,
        cancelled: Option<&AtomicBool>,
    ) -> Result<ParameterLayoutId, StoreError> {
        self.inner().intern_layout(layout, cancelled)
    }

    pub fn intern_binder_space(
        &self,
        space: BinderSpace,
        cancelled: Option<&AtomicBool>,
    ) -> Result<BinderSpaceId, StoreError> {
        self.inner().intern_binder_space(space, cancelled)
    }

    pub fn claim_space_key(&self, key: u64, identity: u128) -> Result<(), StoreError> {
        self.inner().claim_space_key(key, identity)
    }

    pub fn intern_environment(
        &self,
        env: CanonicalTypeSubstitution,
        cancelled: Option<&AtomicBool>,
    ) -> Result<DeclarationInstantiationId, StoreError> {
        self.inner().intern_environment(env, cancelled)
    }

    pub fn intern_shape(
        &self,
        shape: SignatureInputShape,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureInputShapeId, StoreError> {
        self.inner().intern_shape(shape, cancelled)
    }

    pub fn intern_recipe(
        &self,
        recipe: SignatureResultRecipe,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureResultRecipeId, StoreError> {
        self.inner().intern_recipe(recipe, cancelled)
    }

    pub fn intern_template(
        &self,
        template: SignatureTemplate,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureTemplateId, StoreError> {
        self.inner().intern_template(template, cancelled)
    }

    pub fn intern_descriptor(
        &self,
        descriptor: SignatureDescriptor,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureDescriptorId, StoreError> {
        self.inner().intern_descriptor(descriptor, cancelled)
    }

    pub fn intern_provenance(
        &self,
        provenance: SignatureProvenance,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureProvenanceId, StoreError> {
        self.inner().intern_provenance(provenance, cancelled)
    }

    pub fn intern_sequence(
        &self,
        sequence: ConstituentSequence,
        cancelled: Option<&AtomicBool>,
    ) -> Result<super::records::ConstituentSequenceId, StoreError> {
        self.inner().intern_sequence(sequence, cancelled)
    }

    pub fn intern_set(
        &self,
        candidates: Box<[SignatureCandidate]>,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureSetId, StoreError> {
        self.inner().intern_set(candidates, cancelled)
    }

    pub fn set_ref_many(
        &self,
        candidates: Box<[SignatureCandidate]>,
        cancelled: Option<&AtomicBool>,
    ) -> Result<SignatureSetRef, StoreError> {
        self.inner().set_ref_many(candidates, cancelled)
    }

    pub fn candidate(
        &self,
        signature: SignatureDescriptorId,
        provenance: SignatureProvenanceId,
    ) -> Result<SignatureCandidate, StoreError> {
        self.inner().candidate(signature, provenance)
    }

    pub fn intern_substitution(
        &self,
        subst: CallSubstitution,
        cancelled: Option<&AtomicBool>,
    ) -> Result<CallSubstitutionId, StoreError> {
        self.inner().intern_substitution(subst, cancelled)
    }

    /// The whole substitution as one canonical map (composes flatten).
    pub fn flatten_substitution(
        &self,
        id: CallSubstitutionId,
    ) -> Result<CanonicalTypeSubstitution, StoreError> {
        self.inner().flatten_substitution(id)
    }

    /// Publish a result already in call space into the bound epoch.
    pub fn publish_result(
        &self,
        result: AppliedResult,
        cancelled: Option<&AtomicBool>,
    ) -> Result<u64, StoreError> {
        self.inner().publish_result(result, cancelled)
    }

    /// The arena of the graph the bound epoch's tokens root.
    pub(crate) fn graph_arena(
        &self,
    ) -> Option<Arc<crate::semantic_query_memo::arena::NodeArenaCore>> {
        self.inner().graph_arena()
    }
}

/// Owns the tables used by one published result without adding it to a
/// store-global retained-results list. Replacement never changes this lease,
/// and a close never drops the graph roots of the tokens it can read.
pub struct SignatureEpochLease {
    inner: Arc<EpochInner>,
}

impl Clone for SignatureEpochLease {
    fn clone(&self) -> Self {
        Self::from_epoch(Arc::clone(&self.inner))
    }
}

impl Drop for SignatureEpochLease {
    fn drop(&mut self) {
        self.inner.release_hold();
    }
}

impl std::fmt::Debug for SignatureEpochLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SignatureEpochLease")
            .field(&self.inner.epoch)
            .finish()
    }
}

impl SignatureEpochLease {
    pub(super) fn from_epoch(inner: Arc<EpochInner>) -> Self {
        inner.hold();
        Self { inner }
    }
    pub fn epoch(&self) -> GraphEpoch {
        self.inner.epoch
    }
    pub fn read_view(&self) -> super::read_view::SemanticReadView {
        super::read_view::SemanticReadView::from_epoch(Arc::clone(&self.inner))
    }
    /// The lease's own epoch, current or retired, held open for reading
    /// and publication.
    #[must_use]
    pub fn bind(&self) -> EpochBinding {
        EpochBinding {
            view: self.read_view(),
        }
    }
}

impl SignatureStore {
    pub(crate) fn bind_graph(&self, graph: &Arc<crate::semantic_query_memo::arena::NodeArenaCore>) {
        let _ = self.graph.set(Arc::downgrade(graph));
    }

    /// Pin every epoch's storage against `account` — the owning store's.
    /// Bound once, before the first record is interned; a later binding is
    /// ignored, so a charge is never moved between accounts.
    pub(crate) fn bind_retention_account(
        &self,
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) {
        let _ = self.account.set(Arc::clone(account));
    }

    /// Bytes the current epoch's storage is charged for.
    #[must_use]
    pub fn current_epoch_storage_charge(&self) -> usize {
        self.current.load().storage.charged()
    }

    /// Exact bytes the current epoch's storage accounts, before rounding to
    /// the charged granule.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn current_epoch_storage_used(&self) -> usize {
        self.current.load().storage.used()
    }
    /// The arena of the graph this kernel is bound to, which issues every
    /// node operand its records name.
    pub(crate) fn graph_arena(
        &self,
    ) -> Option<Arc<crate::semantic_query_memo::arena::NodeArenaCore>> {
        self.inner().graph_arena()
    }
    pub fn lease_epoch(&self, epoch: GraphEpoch) -> Result<SignatureEpochLease, StoreError> {
        let inner = self.current.load_full();
        check_epoch(inner.epoch, epoch)?;
        Ok(SignatureEpochLease::from_epoch(inner))
    }
}

impl Default for SignatureStore {
    fn default() -> Self {
        Self::new()
    }
}

pub(super) fn check_epoch(live: GraphEpoch, handle: GraphEpoch) -> Result<(), StoreError> {
    if !handle.is_issued() || handle != live {
        Err(StoreError::StaleHandle)
    } else {
        Ok(())
    }
}

fn compose_depth(node: &CallSubstitution) -> u8 {
    match node {
        CallSubstitution::Compose { depth, .. } => *depth,
        _ => 0,
    }
}

fn a_map(node: &CallSubstitution) -> &CanonicalTypeSubstitution {
    static EMPTY: std::sync::OnceLock<CanonicalTypeSubstitution> = std::sync::OnceLock::new();
    match node {
        CallSubstitution::Map { map, .. } => map,
        _ => EMPTY.get_or_init(CanonicalTypeSubstitution::empty),
    }
}
