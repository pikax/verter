//! Document-close release for the semantic substrate.
//!
//! A per-canonical EDIT ([`SemanticGraphStore::invalidate_canonical`]) drains
//! the memo entries whose carriers reference the canonical and drops the
//! arena's dedup entries for it, but leaves every node payload, every
//! per-node sidecar (`unresolved_reach`, the member-ordinal index, origin
//! edges) and every relation proof in place: the edited document is still
//! open, its next lowering re-interns fresh ids, and the retained payloads
//! are what a warm re-read of an unchanged neighbour still reaches.
//!
//! A document CLOSE is different. The editor buffer is gone, the next
//! reader reloads the file from disk and re-lowers it from scratch, and
//! nothing that was interned for the closed content can be reached again
//! except through a stale handle. [`SemanticGraphStore::release_canonical`]
//! therefore performs the edit drain AND reclaims the payload side:
//!
//! 1. the edit-path drain ([`SemanticGraphStore::invalidate_canonical`]);
//! 2. the arena tombstone ([`super::arena::NodeArena::release_canonical`])
//!    — the canonical's nodes plus every node embedding one of them, ids
//!    kept, payloads dropped, dedup entries removed;
//! 3. under the `entries` lock: every remaining candidate whose family key
//!    names a released (or any non-live) node, whose result names one, or
//!    whose validity is bound to the canonical (self-root, full fact rail,
//!    dispatch fence, compacted aggregate) is evicted, reverse index and
//!    budget kept consistent. In-flight builds are NOT aborted beyond the
//!    targeted abort the edit drain already performs: aborting every flight
//!    made each aborted requester re-run cold and call `ensure_loaded` on
//!    the freshly evicted document, which submits `close_file` + Load to
//!    the scheduler per requester per close and overflowed the pool
//!    transport in a long session. A build that read the closed content
//!    and publishes afterwards is registered under the canonical, fails
//!    validation once the reload lands a new hash, and is drained by the
//!    next close; the admission fence below keeps it off released ids;
//! 4. the per-node sidecars and the relation proof / relate-key tables drop
//!    every entry naming a released node;
//! 5. the hash-cons memos are cleared once more, after the tombstone.
//!
//! What is NOT released, and why: `Global`-scope nodes that embed no
//! released id (primitives, shared literal unions minted for the closed
//! content) — they are scope-less by design and may be shared by any file;
//! the sealed `DeferredCallable` carriers of OTHER canonicals whose
//! parameter types name a released node — their parts are unreadable
//! outside the two sanctioned consumers; memo candidates whose only link to
//! the closed canonical is an id behind an opaque interned handle (an
//! intersection recipe, a signature descriptor) — the family budget
//! reclaims those. None of these can serve a released node: the read-side
//! [`SemanticGraphStore::result_is_live`] guard rejects any warm result
//! naming a tombstoned id, and a tombstoned id reads as `Opaque(Miss)`.

use std::sync::atomic::Ordering;

use rustc_hash::FxHashSet;

use super::*;

/// What one [`SemanticGraphStore::release_canonical`] reclaimed. Counts
/// are per call; `shape_entries_released` is filled by the owning
/// [`crate::project_type_store::ProjectTypeStore`], which releases the
/// shape cache alongside the graph.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SemanticReleaseReport {
    /// Warm memo candidates evicted — by the reverse-index drain and by the
    /// released-id key / result sweep together.
    pub memo_entries_evicted: usize,
    /// Arena slots tombstoned (the canonical's own nodes plus the cascade).
    pub nodes_released: usize,
    /// `unresolved_reach` bits dropped.
    pub unresolved_reach_dropped: usize,
    /// Member-ordinal sidecar indexes dropped.
    pub member_indexes_dropped: usize,
    /// Origin-edge buckets dropped from the derivation store.
    pub derivation_buckets_dropped: usize,
    /// Relation proofs whose witness named a released node.
    pub relation_proofs_released: usize,
    /// Co-discharged relate keys whose operands named a released node.
    pub relate_keys_released: usize,
    /// Shape-cache entries dropped by the owning store (see the type doc).
    pub shape_entries_released: usize,
}

impl SemanticGraphStore {
    /// Release everything this store retained for the closed
    /// `canonical_id`. See the module docs for the exact sequence and for
    /// what deliberately stays. Idempotent: a second call for the same
    /// canonical finds nothing to release beyond the edit-path drain.
    pub fn release_canonical(&self, canonical_id: &str) -> SemanticReleaseReport {
        let drained = self.invalidate_canonical(canonical_id);
        let mut report = self.release_canonical_payloads_below(canonical_id, u64::MAX);
        report.memo_entries_evicted += drained;
        report
    }

    /// The payload half of [`Self::release_canonical`]: everything after the
    /// edit-path drain, releasing only the canonical's nodes with ids below
    /// `below` (plus the cascade). The language server applies it through
    /// [`crate::project_type_store::semantic_activity`] once no computation is in flight, with
    /// `below` set to the arena size at the close, so nodes the reload
    /// interned after the close stay.
    pub fn release_canonical_payloads_below(
        &self,
        canonical_id: &str,
        below: u64,
    ) -> SemanticReleaseReport {
        let mut report = SemanticReleaseReport::default();
        // Arm the warm-read liveness guard BEFORE any payload is dropped so
        // a warm hit racing the tombstone cannot serve a released node.
        self.released_any.store(true, Ordering::Release);
        let released_ids = self.arena.release_canonical(canonical_id, below);
        report.nodes_released = released_ids.len();
        // Possibly empty: a document that interned no node can still be
        // what a consumer's candidate is bound to, so the memo sweep and
        // the in-flight abort below run regardless.
        let dead: FxHashSet<SemanticNodeId> = released_ids.into_iter().collect();

        // Memo sweep in ONE `entries`-lock hold: the family memo's
        // consistency cluster (`entries`, `memo_budget`,
        // `canonical_to_entries`) mutates under this lock.
        {
            let mut entries = self.entries_lock_diagnosed();
            let mut victims: Vec<(FamilyKey, ModeSlot, MemoEntry)> = Vec::new();
            for (family, slots) in entries.iter_mut() {
                // A key naming ANY non-live node — this call's tombstones or
                // an earlier close's — can never be looked up again: a stale
                // holder's re-dispatch of a released ordinal is the only way
                // such a family gets minted, and its id is dead before this
                // release runs, so this call's dead set alone would miss it.
                let mut key_names_dead = family.binds_canonical(canonical_id);
                family.for_each_node_id(|id| {
                    key_names_dead |= dead.contains(&id) || !self.arena.is_live(id);
                });
                for slot in family::ALL_MODE_SLOTS {
                    slots.retain_candidates_in_slot_mut(*slot, |entry| {
                        let drop = key_names_dead
                            || result_names_dead(&entry.result, &dead)
                            || candidate_binds_canonical(entry, canonical_id);
                        if drop {
                            victims.push((family.clone(), *slot, entry.clone()));
                        }
                        !drop
                    });
                }
            }
            for (family, slot, entry) in &victims {
                reverse_index::drain_candidate_reverse_index_registrations(
                    &self.canonical_to_entries,
                    family,
                    *slot,
                    entry,
                );
            }
            report.memo_entries_evicted += victims.len();
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
        }

        {
            let mut reach = self.unresolved_reach.lock();
            let before = reach.len();
            reach.retain(|id, _| !dead.contains(id));
            report.unresolved_reach_dropped = before - reach.len();
        }
        {
            let before = self.member_ordinal_index_memo.len();
            self.member_ordinal_index_memo
                .retain(|id, _| !dead.contains(id));
            report.member_indexes_dropped = before - self.member_ordinal_index_memo.len();
            self.member_ordinal_index_fifo
                .lock()
                .retain(|id| !dead.contains(id));
        }
        report.derivation_buckets_dropped = self
            .derivation
            .lock()
            .release_nodes(&|id| dead.contains(&id));
        let (relate_keys_released, relation_proofs_released) = self.release_relation_tables(&dead);
        report.relate_keys_released = relate_keys_released;
        report.relation_proofs_released = relation_proofs_released;
        // The edit-path drain already cleared these, but a publish that
        // raced the tombstone could have landed a mapping whose value names
        // a released node; clearing again after the tombstone closes it.
        self.clear_hash_cons_memos();
        report
    }

    /// Drop every relate key whose operands name a released node, then
    /// every proof whose witness names a released node or a key released
    /// here. Slots stay allocated (ids are never reused); the dedup maps
    /// shrink to the live set. Returns `(keys released, proofs released)`.
    fn release_relation_tables(&self, dead: &FxHashSet<SemanticNodeId>) -> (usize, usize) {
        use crate::semantic_query::{RelateKeyId, RelationProof};

        let mut released_keys: FxHashSet<RelateKeyId> = FxHashSet::default();
        {
            let mut table = self.relate_key_table.lock();
            let (slots, index) = &mut *table;
            for (ordinal, slot) in slots.iter_mut().enumerate() {
                let stale = slot
                    .as_ref()
                    .is_some_and(|key| dead.contains(&key.source) || dead.contains(&key.target));
                if !stale {
                    continue;
                }
                if let Some(key) = slot.take() {
                    index.remove(&key);
                    released_keys.insert(RelateKeyId(ordinal as u32));
                }
            }
        }
        let mut released_proofs = 0usize;
        {
            let mut table = self.relation_proof_table.lock();
            let (slots, index) = &mut *table;
            for slot in slots.iter_mut() {
                let stale = slot.as_ref().is_some_and(|proof| match proof {
                    RelationProof::Assignable { witness } => witness
                        .sub_derivations
                        .iter()
                        .any(|sub| dead.contains(&sub.source) || dead.contains(&sub.target)),
                    RelationProof::NotAssignable { failing_sub, .. } => {
                        dead.contains(&failing_sub.source) || dead.contains(&failing_sub.target)
                    }
                    RelationProof::BudgetExceeded { .. } => false,
                    RelationProof::CoinductiveCycle { keys } => {
                        keys.iter().any(|key| released_keys.contains(key))
                    }
                });
                if !stale {
                    continue;
                }
                if let Some(proof) = slot.take() {
                    index.remove(&proof);
                    released_proofs += 1;
                }
            }
        }
        (released_keys.len(), released_proofs)
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
            .any(|(canonical, _)| canonical.as_ref() == canonical_id)
        || !entry.read_set_signature.aggregated_domains().is_empty()
}

/// Whether a warm candidate's result names a released node at top level.
/// Only the node-valued domains carry an arena id there; the other value
/// domains are reclaimed through the reverse index and the family budget.
fn result_names_dead(
    result: &QueryResult<SemanticQueryValue>,
    dead: &FxHashSet<SemanticNodeId>,
) -> bool {
    match result {
        QueryResult::Value(SemanticQueryValue::TypeNode(id)) | QueryResult::Recursive(id) => {
            dead.contains(id)
        }
        _ => false,
    }
}
