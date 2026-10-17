//! Document close and destroyed-node sidecar cleanup.
//!
//! Graph lifetime is counted (`arena.rs`, **Lifetime**): a node lives while
//! any owner holds it and is destroyed by its last release. Nothing here
//! destroys a node or revokes a holder. A per-canonical EDIT
//! ([`SemanticGraphStore::invalidate_canonical`]) drains the memo entries
//! whose carriers reference the canonical and drops the arena's dedup entries
//! for it, so the edited content re-interns fresh ids.
//!
//! A document CLOSE ([`SemanticGraphStore::close_canonical`]) goes further:
//! the editor buffer is gone and the next reader re-lowers the file from
//! disk, so no lookup reaches the closed content again. The close therefore
//! evicts every CACHE owner bound to that content, and the content's nodes
//! are destroyed as soon as no caller holds them:
//!
//! 1. the edit-path drain ([`SemanticGraphStore::invalidate_canonical`]);
//! 2. under the `entries` lock, every remaining candidate whose family key
//!    binds the canonical or names closed content, whose result names closed
//!    content, or whose validity is bound to the canonical (self-root, full
//!    fact rail, dispatch fence, compacted aggregate) is evicted, reverse
//!    index and budget kept consistent. In-flight builds are not aborted
//!    beyond the edit drain's targeted abort;
//! 3. the union member views over closed content;
//! 4. the hash-cons, substitute and deferred-evaluation memos;
//! 5. the signature kernel's roots on type tokens naming closed content —
//!    the epoch is replaced once such stranded tokens dominate it, or once
//!    its tables outgrow their cap;
//! 6. the per-node sidecar entries of every node destroyed so far
//!    ([`SemanticGraphStore::reclaim_destroyed_sidecars`]).
//!
//! "Closed content" is decided per cached node, never by sweeping the arena:
//! a node interned in the closed file's scope, one whose payload binds the
//! closed content's identity, or one reaching such a node through its
//! retained children ([`super::arena::NodeArenaCore::reaches_canonical_content`]).
//! A caller holding a closed node keeps it readable until the caller drops
//! it; the close only stops the caches from keeping it alive.
//!
//! The per-node sidecars (structure bits, member-ordinal indexes, origin
//! edges, stable-key classes) own nothing. Each marks the slot it describes,
//! and a marked slot's destruction is logged for cleanup, so the pending log
//! never outgrows the stale entries it removes. The log is drained by every
//! close, by any sidecar write that finds it past
//! [`DESTROYED_DRAIN_THRESHOLD`], and by the release that grows it past that
//! threshold ([`SidecarSink`]). A release that finds a sidecar lock held
//! leaves the drain to that lock's holder, which runs it as it unlocks
//! ([`SidecarMutex`]), so the pending log and the stale sidecar entries it
//! describes stay bounded at rest even when a holder's final drop comes
//! after the last close, under contention, and no later work runs.

use rustc_hash::FxHashMap;

use super::*;

/// Destroyed ids the pending log may hold before a sidecar write or a
/// release drains it.
pub(super) const DESTROYED_DRAIN_THRESHOLD: usize = 1024;

/// A per-node sidecar's lock. Its guard settles any cleanup a release
/// could not run because a sidecar lock was held: the release records the
/// obligation ([`SidecarSink::settle`]) and the holder that blocked it
/// drains the log as it unlocks, so the pending log never waits on later,
/// unrelated work.
pub(crate) struct SidecarMutex<T> {
    lock: Mutex<T>,
    binding: std::sync::OnceLock<SidecarBinding>,
}

/// What a sidecar guard settles on unlock: the owning arena's cleanup
/// obligation.
struct SidecarBinding {
    owed: Arc<std::sync::atomic::AtomicBool>,
    arena: std::sync::Weak<super::arena::NodeArenaCore>,
}

impl<T: Default> Default for SidecarMutex<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> SidecarMutex<T> {
    pub(super) fn new(value: T) -> Self {
        Self {
            lock: Mutex::new(value),
            binding: std::sync::OnceLock::new(),
        }
    }

    /// Lock the sidecar; the guard settles a cleanup obligation recorded
    /// while it was held.
    pub(super) fn lock(&self) -> SidecarGuard<'_, T> {
        SidecarGuard {
            guard: Some(self.lock.lock()),
            binding: self.binding.get(),
        }
    }

    /// The raw lock, for the drain itself: it settles nothing on unlock.
    fn try_lock_raw(&self) -> Option<parking_lot::MutexGuard<'_, T>> {
        self.lock.try_lock()
    }

    fn bind(
        &self,
        owed: &Arc<std::sync::atomic::AtomicBool>,
        arena: &Arc<super::arena::NodeArenaCore>,
    ) {
        let _ = self.binding.set(SidecarBinding {
            owed: Arc::clone(owed),
            arena: Arc::downgrade(arena),
        });
    }
}

/// A held sidecar lock (see [`SidecarMutex`]).
pub(crate) struct SidecarGuard<'a, T> {
    guard: Option<parking_lot::MutexGuard<'a, T>>,
    binding: Option<&'a SidecarBinding>,
}

impl<T> std::ops::Deref for SidecarGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.guard.as_ref().expect("a held sidecar guard")
    }
}

impl<T> std::ops::DerefMut for SidecarGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.guard.as_mut().expect("a held sidecar guard")
    }
}

impl<T> Drop for SidecarGuard<'_, T> {
    fn drop(&mut self) {
        drop(self.guard.take());
        let Some(binding) = self.binding else {
            return;
        };
        // Pairs with the fence in `SidecarSink::settle_owed`: a release that
        // found this lock held recorded its obligation before trying it, so
        // either it drained after this unlock or this load sees the
        // obligation.
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        if binding.owed.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(arena) = binding.arena.upgrade() {
                arena.settle_destroyed_log();
            }
        }
    }
}

/// Weak handles on a store's per-node sidecars, bound into the store's arena
/// so the release that grows the destroyed log past
/// [`DESTROYED_DRAIN_THRESHOLD`] drains it itself, and the cleanup
/// obligation a held sidecar lock leaves to its holder.
pub(super) struct SidecarSink {
    unresolved_reach: std::sync::Weak<
        SidecarMutex<FxHashMap<SemanticNodeId, super::unresolved_reach::NodeStructureBits>>,
    >,
    member_ordinal_index_memo:
        std::sync::Weak<DashMap<SemanticNodeId, Arc<super::member_index::MemberOrdinalIndex>>>,
    member_ordinal_index_fifo:
        std::sync::Weak<SidecarMutex<std::collections::VecDeque<SemanticNodeId>>>,
    derivation: std::sync::Weak<SidecarMutex<DerivationStore>>,
    key_classes: std::sync::Weak<SidecarMutex<crate::semantic_query::stable_key::KeyClasses>>,
    /// Set from the release that leaves the destroyed log past its threshold
    /// until a drain empties it.
    owed: Arc<std::sync::atomic::AtomicBool>,
}

impl SidecarSink {
    /// Drain the destroyed log, now or through the holder of a sidecar lock
    /// that blocks it. Never waits for a sidecar lock: one held elsewhere —
    /// on this thread too, when the release runs inside a sidecar write —
    /// leaves the obligation recorded, and that holder's unlock settles it.
    pub(super) fn settle(&self, arena: &super::arena::NodeArenaCore) {
        self.owed.store(true, std::sync::atomic::Ordering::SeqCst);
        self.settle_owed(arena);
    }

    /// Settle a recorded obligation, if any.
    pub(super) fn settle_owed(&self, arena: &super::arena::NodeArenaCore) {
        loop {
            // Pairs with the fence in a sidecar guard's unlock: an
            // obligation recorded while this drain held the locks is seen
            // here, and one recorded before a holder unlocked is seen there.
            std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
            if !self.owed.load(std::sync::atomic::Ordering::Relaxed) || !self.try_drain(arena) {
                return;
            }
        }
    }

    /// Drop the sidecar entries of every logged destroyed id, clearing the
    /// obligation under the sidecar locks. `false` when a sidecar lock is
    /// held elsewhere. A store already gone has no entries left to drop: its
    /// log is emptied.
    fn try_drain(&self, arena: &super::arena::NodeArenaCore) -> bool {
        let (Some(reach), Some(memo), Some(fifo), Some(derivation), Some(classes)) = (
            self.unresolved_reach.upgrade(),
            self.member_ordinal_index_memo.upgrade(),
            self.member_ordinal_index_fifo.upgrade(),
            self.derivation.upgrade(),
            self.key_classes.upgrade(),
        ) else {
            self.owed.store(false, std::sync::atomic::Ordering::SeqCst);
            drop(arena.drain_destroyed());
            return true;
        };
        let (Some(mut reach), Some(mut fifo), Some(mut derivation), Some(mut classes)) = (
            reach.try_lock_raw(),
            fifo.try_lock_raw(),
            derivation.try_lock_raw(),
            classes.try_lock_raw(),
        ) else {
            return false;
        };
        self.owed.store(false, std::sync::atomic::Ordering::SeqCst);
        let destroyed = arena.drain_destroyed();
        for id in &destroyed {
            reach.remove(id);
            // A member-index shard is held only to read or write one entry,
            // never across a release, so waiting for it cannot deadlock.
            memo.remove(id);
        }
        let dead: rustc_hash::FxHashSet<SemanticNodeId> = destroyed.into_iter().collect();
        fifo.retain(|id| !dead.contains(id));
        derivation.release_nodes(&|id| dead.contains(&id));
        classes.release_nodes(|id| dead.contains(&id));
        true
    }
}

/// What one [`SemanticGraphStore::close_canonical`] evicted. Counts are per
/// call; `shape_entries_released` is filled by the owning
/// [`crate::project_type_store::ProjectTypeStore`], which releases the shape
/// cache alongside the graph.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SemanticCloseReport {
    /// Warm memo candidates evicted — by the reverse-index drain and by the
    /// closed-content sweep of the memo together.
    pub memo_entries_evicted: usize,
    /// Union member views over closed content evicted.
    pub union_views_released: usize,
    /// Signature-kernel type-token roots the store released for closed
    /// content. A root a live reader or lease of the epoch may still read
    /// back is dropped when the epoch's last holder drops, so this counts
    /// roots taken from the store, not nodes freed by this close.
    pub signature_tokens_released: usize,
    /// Whether this close replaced the signature kernel's epoch.
    pub signature_epoch_replaced: bool,
    /// Per-node sidecar entries of destroyed nodes dropped.
    pub sidecar_entries_dropped: usize,
    /// Shape-cache entries dropped by the owning store (see the type doc).
    pub shape_entries_released: usize,
    /// Wall time of the close, in microseconds.
    pub elapsed_micros: u64,
}

/// The verdicts of one close: which cached nodes are closed content.
pub struct ClosedContent<'a> {
    graph: &'a SemanticGraphStore,
    canonical_id: &'a str,
    memo: FxHashMap<u64, bool>,
}

impl ClosedContent<'_> {
    /// Whether `node` is the closed document's content, or reaches it.
    pub(crate) fn names(&mut self, node: SemanticNodeId) -> bool {
        self.graph
            .arena
            .reaches_canonical_content(self.canonical_id, node, &mut self.memo)
    }
}

impl SemanticGraphStore {
    /// The closed-content verdicts of one close of `canonical_id`.
    pub(crate) fn closed_content<'a>(&'a self, canonical_id: &'a str) -> ClosedContent<'a> {
        ClosedContent {
            graph: self,
            canonical_id,
            memo: FxHashMap::default(),
        }
    }

    /// Close `canonical_id`: evict every cache owner bound to its content
    /// and drop the sidecar entries of nodes already destroyed. See the
    /// module docs for the sequence. A caller's held result is never
    /// revoked. Idempotent: a second call finds nothing further to evict.
    pub fn close_canonical(&self, canonical_id: &str) -> SemanticCloseReport {
        self.close_canonical_with(canonical_id, |_| ())
    }

    /// [`Self::close_canonical`], letting `also` evict an owning cache of
    /// the caller's against the same closed-content verdicts before the
    /// destroyed-node cleanup runs.
    pub(crate) fn close_canonical_with(
        &self,
        canonical_id: &str,
        also: impl FnOnce(&mut ClosedContent<'_>),
    ) -> SemanticCloseReport {
        let started = std::time::Instant::now();
        let mut report = SemanticCloseReport {
            memo_entries_evicted: self.invalidate_canonical(canonical_id),
            ..SemanticCloseReport::default()
        };
        let mut closed = self.closed_content(canonical_id);

        // Memo sweep in ONE `entries`-lock hold: the family memo's
        // consistency cluster (`entries`, `memo_budget`,
        // `canonical_to_entries`) mutates under this lock. The evicted
        // candidates are dropped after the lock is released.
        let victims = {
            let mut entries = self.entries_lock_diagnosed();
            let mut victims: Vec<(ModeSlot, MemoEntry)> = Vec::new();
            for (family, slots) in entries.iter_mut() {
                let mut key_names_closed = family.binds_canonical(canonical_id);
                if !key_names_closed {
                    family.for_each_node_id(|id| {
                        key_names_closed = key_names_closed || closed.names(id);
                    });
                }
                for slot in family::ALL_MODE_SLOTS {
                    slots.retain_candidates_in_slot_mut(*slot, |entry| {
                        let drop = key_names_closed
                            || candidate_binds_canonical(entry, canonical_id)
                            || result_names_closed(&entry.result, &mut closed);
                        if drop {
                            victims.push((*slot, entry.clone()));
                        }
                        !drop
                    });
                }
            }
            for (slot, entry) in &victims {
                reverse_index::drain_candidate_reverse_index_registrations(
                    &self.canonical_to_entries,
                    *slot,
                    entry,
                );
            }
            // A family that lost its last candidate leaves the map and the
            // budget ledger together — sound under the held `entries`
            // lock, exactly as in `invalidate_canonical`.
            entries.retain(|family, slots| {
                if slots.populated_count() > 0 {
                    true
                } else {
                    self.memo_budget.forget_key_under_exclusive_lock(family);
                    false
                }
            });
            victims
        };
        report.memo_entries_evicted += victims.len();
        drop(victims);

        report.union_views_released = {
            let taken = self.union_views.lock().take_where(|key, members| {
                closed.names(key.union()) || members.iter().any(|member| closed.names(*member))
            });
            // The views, their charges and their roots drop here, outside
            // the table's lock.
            taken.len()
        };
        self.clear_hash_cons_memos();

        let closed_tokens: Vec<SemanticNodeId> = self
            .signatures
            .rooted_token_nodes()
            .into_iter()
            .filter(|node| closed.names(*node))
            .collect();
        report.signature_tokens_released = closed_tokens.len();
        report.signature_epoch_replaced = self
            .release_signature_records_naming(closed_tokens)
            .is_some()
            || self.compact_signature_store_if_over_cap().is_some();

        also(&mut closed);
        drop(closed);
        report.sidecar_entries_dropped = self.reclaim_destroyed_sidecars();
        report.elapsed_micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        report
    }

    /// Bind this store's sidecars into its arena, so a release that grows
    /// the destroyed log past [`DESTROYED_DRAIN_THRESHOLD`] drains it.
    pub(super) fn bind_destroyed_sidecar_sink(&self) {
        let owed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.unresolved_reach.bind(&owed, &self.arena.core);
        self.member_ordinal_index_fifo.bind(&owed, &self.arena.core);
        self.derivation.bind(&owed, &self.arena.core);
        self.key_classes.bind(&owed, &self.arena.core);
        self.arena.core.bind_destroyed_sink(SidecarSink {
            unresolved_reach: Arc::downgrade(&self.unresolved_reach),
            member_ordinal_index_memo: Arc::downgrade(&self.member_ordinal_index_memo),
            member_ordinal_index_fifo: Arc::downgrade(&self.member_ordinal_index_fifo),
            derivation: Arc::downgrade(&self.derivation),
            key_classes: Arc::downgrade(&self.key_classes),
            owed,
        });
    }

    /// Drop the per-node sidecar entries of every destroyed node the arena
    /// logged. Returns the entries dropped.
    pub fn reclaim_destroyed_sidecars(&self) -> usize {
        let destroyed = self.arena.drain_destroyed();
        if destroyed.is_empty() {
            return 0;
        }
        let dead: rustc_hash::FxHashSet<SemanticNodeId> = destroyed.into_iter().collect();
        let mut dropped = 0usize;
        {
            let mut reach = self.unresolved_reach.lock();
            let before = reach.len();
            reach.retain(|id, _| !dead.contains(id));
            dropped += before - reach.len();
        }
        {
            let before = self.member_ordinal_index_memo.len();
            self.member_ordinal_index_memo
                .retain(|id, _| !dead.contains(id));
            dropped += before - self.member_ordinal_index_memo.len();
            self.member_ordinal_index_fifo
                .lock()
                .retain(|id| !dead.contains(id));
        }
        dropped += self
            .derivation
            .lock()
            .release_nodes(&|id| dead.contains(&id));
        dropped += self
            .key_classes
            .lock()
            .release_nodes(|id| dead.contains(&id));
        dropped
    }

    /// Mark `node` as described by a sidecar entry just written, draining
    /// the destroyed log first when it has grown past
    /// [`DESTROYED_DRAIN_THRESHOLD`]. `false` when `node` was already
    /// destroyed: the caller removes the entry it wrote.
    pub(super) fn track_sidecar(&self, node: SemanticNodeId) -> bool {
        if self.arena.pending_destroyed() > DESTROYED_DRAIN_THRESHOLD {
            self.reclaim_destroyed_sidecars();
        }
        self.arena.track_sidecar(node)
    }
}

/// Whether a warm candidate's VALIDITY is bound to `canonical_id` — by a
/// self-root, by any fact on its rail (the exhaustive per-variant walk,
/// which reaches the `ProgramAnalysis` / `FileSourceEnv` / `DerivedFactHash`
/// facts too), by its dispatch fence, or by a compacted domain aggregate.
///
/// The reverse index registers a candidate only under the canonicals
/// `ReadSetSignature::canonical_ids` reports, which is a documented
/// UNDER-approximation: a `DomainAggregate` / `ProjectScalar` fact names no
/// canonical, so a candidate whose dependency on the closed document was
/// folded into an aggregate is never found by the edit-path drain. A close
/// is the moment every candidate the document's content validated stops
/// being reusable — the reload re-lowers it — so this sweep is exact where
/// the index is not: the full rail is walked, and an aggregated carrier
/// ("rejects on ANY movement in its domain", and the reload IS movement) is
/// dropped rather than left for the budget.
fn candidate_binds_canonical(entry: &MemoEntry, canonical_id: &str) -> bool {
    entry
        .self_root_canonicals
        .iter()
        .any(|root| root.as_ref() == canonical_id)
        || carrier_facts_reference_canonical(&entry.read_set_signature.facts, canonical_id)
        || entry
            .dispatch_dep_signature
            .iter()
            .any(|crate::semantic_query::DepEntry(canonical, _)| canonical.as_ref() == canonical_id)
        || !entry.read_set_signature.aggregated_domains().is_empty()
}

/// Whether a warm candidate's result names closed content at top level.
/// Only the node-valued domains carry a graph node there; the other value
/// domains are reclaimed through the reverse index and the family budget.
fn result_names_closed(
    result: &QueryResult<SemanticQueryValue>,
    closed: &mut ClosedContent<'_>,
) -> bool {
    match result {
        QueryResult::Value(SemanticQueryValue::TypeNode(id)) | QueryResult::Recursive(id) => {
            closed.names(*id)
        }
        _ => false,
    }
}
