//! The workspace lane's resolution slots: who owns them, what they cost,
//! and when they go.
//!
//! Every slot belongs to its importer. A slot holds up to
//! [`CANDIDATE_CAP`] candidates, and each retained candidate carries the
//! retention charge its bytes were admitted under, released when the
//! candidate leaves. Ownership decides retirement:
//!
//! - an importer's slots retire together when the importer leaves the
//!   workspace (deleted, renamed away, or under a removed subtree), in the
//!   same world mutation that retires its edges;
//! - independently of ownership, the store holds at most `slot_cap`
//!   slots, oldest admitted first out, so importers that never retire —
//!   paths the workspace does not know, or a long churn — cannot grow it
//!   without bound. The per-slot candidate cap bounds one key, never the
//!   number of distinct owners.
//!
//! Every removal reports the queries that no longer have a candidate
//! behind them, so the caller removes their decision nodes under the same
//! fence: a decision never outlives the candidate that serves it.

use std::collections::{BTreeMap, VecDeque};

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use verter_session_query::facts::fact_cache::CANDIDATE_CAP;
use verter_session_query::facts::resolution::ResolutionQueryKey;
use verter_session_query::resolution::ResolutionPopulation;
use verter_session_query::retention::resolution_charge::ResolutionRetentionCharge;

use super::{LazyResolutionCacheEntry, LazyResolutionCacheKey};

/// Default bound on the workspace lane's slot count.
pub(crate) const WORKSPACE_LANE_SLOT_CAP: usize = 1 << 17;

/// Estimated bytes one decision edge holds: its key in the node's forward
/// set and the node in the dependency's reverse set.
pub(crate) const DECISION_EDGE_BYTES: usize =
    2 * (std::mem::size_of::<verter_session_query::facts::resolution::ResolutionFactKey>() + 64);

pub(crate) struct WorkspaceResolutionSlots {
    slots: FxHashMap<LazyResolutionCacheKey, Slot>,
    /// Normalized importer → its slot keys. Ordered, so a subtree
    /// retirement is one range seek rather than a scan of every owner.
    owners: BTreeMap<String, FxHashSet<LazyResolutionCacheKey>>,
    /// Keys in admission order, each with the generation of the slot it
    /// admitted. A retired slot leaves its entry behind; eviction skips
    /// it, and the queue is compacted once stale entries dominate.
    order: VecDeque<(LazyResolutionCacheKey, u64)>,
    next_generation: u64,
    candidates: usize,
    slot_cap: usize,
}

struct Slot {
    generation: u64,
    owner: String,
    candidates: SmallVec<[RetainedCandidate; CANDIDATE_CAP]>,
}

struct RetainedCandidate {
    entry: LazyResolutionCacheEntry,
    _charge: Option<ResolutionRetentionCharge>,
}

/// Occupancy of the workspace lane.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SlotResidency {
    pub(crate) slots: usize,
    pub(crate) candidates: usize,
    pub(crate) owners: usize,
}

impl WorkspaceResolutionSlots {
    pub(crate) fn new(slot_cap: usize) -> Self {
        Self {
            slots: FxHashMap::default(),
            owners: BTreeMap::new(),
            order: VecDeque::new(),
            next_generation: 0,
            candidates: 0,
            slot_cap: slot_cap.max(1),
        }
    }

    /// The candidates `key` holds, oldest first.
    pub(crate) fn candidates(
        &self,
        key: &LazyResolutionCacheKey,
    ) -> impl Iterator<Item = &LazyResolutionCacheEntry> + '_ {
        self.slots
            .get(key)
            .into_iter()
            .flat_map(|slot| slot.candidates.iter().map(|retained| &retained.entry))
    }

    /// Retain `entry` under `key` with the charge its bytes were admitted
    /// under. The slot keeps its newest [`CANDIDATE_CAP`] candidates and
    /// the store its newest `slot_cap` slots.
    ///
    /// Returns the queries left with no candidate: an aged-out candidate
    /// whose query no remaining sibling serves, and every query of an
    /// evicted slot.
    pub(crate) fn admit(
        &mut self,
        key: LazyResolutionCacheKey,
        entry: LazyResolutionCacheEntry,
        charge: Option<ResolutionRetentionCharge>,
    ) -> Vec<ResolutionQueryKey> {
        if !self.slots.contains_key(&key) {
            let generation = self.next_generation;
            self.next_generation += 1;
            let owner = verter_session_query::resolution::normalize_canonical_id(&key.importer_id);
            self.owners
                .entry(owner.clone())
                .or_default()
                .insert(key.clone());
            self.slots.insert(
                key.clone(),
                Slot {
                    generation,
                    owner,
                    candidates: SmallVec::new(),
                },
            );
            self.order.push_back((key.clone(), generation));
        }
        let slot = self.slots.get_mut(&key).expect("the slot was just ensured");
        let mut aged_out = Vec::new();
        while slot.candidates.len() >= CANDIDATE_CAP {
            aged_out.push(slot.candidates.remove(0).entry.query);
            self.candidates -= 1;
        }
        slot.candidates.push(RetainedCandidate {
            entry,
            _charge: charge,
        });
        self.candidates += 1;
        // An aged-out query that a remaining candidate (the incoming one
        // included) still serves keeps its decision.
        aged_out.retain(|query| {
            !slot
                .candidates
                .iter()
                .any(|retained| retained.entry.query == *query)
        });
        aged_out.sort();
        aged_out.dedup();
        self.enforce_slot_cap(&mut aged_out);
        aged_out
    }

    /// Retire every slot `owner` holds whose population `retire` accepts.
    /// Returns their queries.
    pub(crate) fn retire_owner(
        &mut self,
        owner: &str,
        retire: impl Fn(&str, ResolutionPopulation) -> bool,
    ) -> Vec<ResolutionQueryKey> {
        let owner = verter_session_query::resolution::normalize_canonical_id(owner);
        let mut queries = Vec::new();
        self.retire_owners(std::iter::once(owner), &retire, &mut queries);
        queries
    }

    /// [`Self::retire_owner`] for every owner at or under `prefix`.
    pub(crate) fn retire_owners_under(
        &mut self,
        prefix: &str,
        retire: impl Fn(&str, ResolutionPopulation) -> bool,
    ) -> Vec<ResolutionQueryKey> {
        let prefix = verter_session_query::resolution::normalize_canonical_id(prefix);
        let base = prefix.trim_end_matches('/');
        let directory = format!("{base}/");
        let owners: Vec<String> = self
            .owners
            .range(base.to_owned()..)
            .map(|(owner, _)| owner)
            .take_while(|owner| owner.starts_with(base))
            .filter(|owner| owner.as_str() == base || owner.starts_with(&directory))
            .cloned()
            .collect();
        let mut queries = Vec::new();
        self.retire_owners(owners.into_iter(), &retire, &mut queries);
        queries
    }

    pub(crate) fn residency(&self) -> SlotResidency {
        SlotResidency {
            slots: self.slots.len(),
            candidates: self.candidates,
            owners: self.owners.len(),
        }
    }

    /// Entries in the admission queue, stale ones included.
    #[cfg(test)]
    pub(crate) fn queue_len(&self) -> usize {
        self.order.len()
    }

    /// Drop every slot without retiring a decision: a test seam that forces
    /// the next demand cold while the decision graph stays as it was.
    #[cfg(test)]
    pub(crate) fn clear(&mut self) {
        self.slots.clear();
        self.owners.clear();
        self.order.clear();
        self.candidates = 0;
    }

    #[cfg(test)]
    pub(crate) fn set_slot_cap_for_test(&mut self, slot_cap: usize) {
        self.slot_cap = slot_cap.max(1);
    }

    fn retire_owners(
        &mut self,
        owners: impl Iterator<Item = String>,
        retire: &impl Fn(&str, ResolutionPopulation) -> bool,
        queries: &mut Vec<ResolutionQueryKey>,
    ) {
        for owner in owners {
            let Some(keys) = self.owners.get(&owner) else {
                continue;
            };
            let retired: Vec<LazyResolutionCacheKey> = keys
                .iter()
                .filter(|key| retire(&owner, key.population))
                .cloned()
                .collect();
            for key in retired {
                self.remove_slot(&key, queries);
            }
        }
        self.compact_order();
    }

    fn remove_slot(&mut self, key: &LazyResolutionCacheKey, queries: &mut Vec<ResolutionQueryKey>) {
        let Some(slot) = self.slots.remove(key) else {
            return;
        };
        if let Some(keys) = self.owners.get_mut(&slot.owner) {
            keys.remove(key);
            if keys.is_empty() {
                self.owners.remove(&slot.owner);
            }
        }
        self.candidates -= slot.candidates.len();
        // A query names its slot, so only candidates of this one slot can
        // repeat it.
        let mut slot_queries: Vec<ResolutionQueryKey> = slot
            .candidates
            .into_iter()
            .map(|retained| retained.entry.query)
            .collect();
        slot_queries.sort();
        slot_queries.dedup();
        queries.extend(slot_queries);
    }

    fn enforce_slot_cap(&mut self, queries: &mut Vec<ResolutionQueryKey>) {
        while self.slots.len() > self.slot_cap {
            let Some((key, generation)) = self.order.pop_front() else {
                break;
            };
            if self
                .slots
                .get(&key)
                .is_some_and(|slot| slot.generation == generation)
            {
                self.remove_slot(&key, queries);
            }
        }
        self.compact_order();
    }

    /// Drop the queue's stale entries once they outnumber the live ones,
    /// so the queue stays proportional to the store.
    fn compact_order(&mut self) {
        if self.order.len() > 2 * self.slots.len() + 64 {
            let slots = &self.slots;
            self.order.retain(|(key, generation)| {
                slots
                    .get(key)
                    .is_some_and(|slot| slot.generation == *generation)
            });
        }
    }
}

impl std::fmt::Debug for WorkspaceResolutionSlots {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceResolutionSlots")
            .field("slots", &self.slots.len())
            .field("owners", &self.owners.len())
            .field("candidates", &self.candidates)
            .finish_non_exhaustive()
    }
}
