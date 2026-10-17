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
//! close and by any sidecar write that finds it past
//! [`DESTROYED_DRAIN_THRESHOLD`].

use rustc_hash::FxHashMap;

use super::*;

/// Destroyed ids the pending log may hold before a sidecar write drains it.
pub(super) const DESTROYED_DRAIN_THRESHOLD: usize = 1024;

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
    /// Signature-kernel type-token roots released for closed content.
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
