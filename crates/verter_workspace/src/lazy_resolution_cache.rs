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
//! fence: a decision never outlives the candidate that serves it. It also
//! reports the resolved dependencies no remaining candidate of the same
//! importer still answers, so the caller retracts the importer's
//! resolution-owned dependency edge with them.
//!
//! A candidate's charge is shared with its decision node: the candidate and
//! every resolution root that still holds the node keep it, so the bytes stay
//! charged until the last of them — a held snapshot included — drops.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use verter_session_query::facts::fact_cache::CANDIDATE_CAP;
use verter_session_query::facts::resolution::ResolutionQueryKey;
use verter_session_query::resolution::ResolutionPopulation;
use verter_session_query::retention::resolution_charge::ResolutionRetentionCharge;

use super::{LazyResolutionCacheEntry, LazyResolutionCacheKey};

/// Default bound on the workspace lane's slot count.
pub(crate) const WORKSPACE_LANE_SLOT_CAP: usize = 1 << 17;

/// Backing capacity a drained lane may keep without giving it back: the
/// slack that stops a lane hovering near empty from reallocating on every
/// retirement.
const SHRINK_FLOOR: usize = 256;

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
    _charge: Arc<ResolutionRetentionCharge>,
}

/// Occupancy of the workspace lane: what it holds and the storage it keeps
/// allocated for it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SlotResidency {
    pub(crate) slots: usize,
    pub(crate) candidates: usize,
    pub(crate) owners: usize,
    /// Admission-queue entries, stale ones (of retired slots) included.
    pub(crate) queue_entries: usize,
    /// Slots the slot table can hold without reallocating.
    pub(crate) slot_capacity: usize,
    /// Entries the admission queue can hold without reallocating.
    pub(crate) queue_capacity: usize,
}

/// What one admission or retirement took out of the lane.
#[derive(Debug, Default)]
pub(crate) struct SlotRetirement {
    /// Queries left with no candidate: their decision nodes go.
    pub(crate) queries: Vec<ResolutionQueryKey>,
    /// `(importer, dependency)` pairs no remaining candidate of that
    /// importer resolves to: their resolution-owned dependency edges go.
    pub(crate) dependencies: Vec<(String, String)>,
}

impl SlotRetirement {
    pub(crate) fn is_empty(&self) -> bool {
        self.queries.is_empty() && self.dependencies.is_empty()
    }
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

    /// Whether `key` holds a candidate for `query` — under the publication
    /// gate, whether that query's decision node is live.
    pub(crate) fn serves(&self, key: &LazyResolutionCacheKey, query: &ResolutionQueryKey) -> bool {
        self.candidates(key).any(|entry| entry.query == *query)
    }

    /// Retain `entry` under `key` with the charge its bytes were admitted
    /// under, shared with the decision node it publishes. The slot keeps its
    /// newest [`CANDIDATE_CAP`] candidates and the store its newest
    /// `slot_cap` slots.
    ///
    /// Returns what left: the queries left with no candidate (an aged-out
    /// candidate whose query no remaining sibling serves, and every query
    /// of an evicted slot) and the dependencies no remaining candidate of
    /// their importer resolves to.
    pub(crate) fn admit(
        &mut self,
        key: LazyResolutionCacheKey,
        entry: LazyResolutionCacheEntry,
        charge: Arc<ResolutionRetentionCharge>,
    ) -> SlotRetirement {
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
            aged_out.push(slot.candidates.remove(0).entry);
            self.candidates -= 1;
        }
        slot.candidates.push(RetainedCandidate {
            entry,
            _charge: charge,
        });
        self.candidates += 1;
        // An aged-out query that a remaining candidate (the incoming one
        // included) still serves keeps its decision.
        let mut retirement = SlotRetirement::default();
        for entry in aged_out {
            if !slot
                .candidates
                .iter()
                .any(|retained| retained.entry.query == entry.query)
            {
                retirement.queries.push(entry.query);
            }
            if let Some(result) = entry.result {
                retirement
                    .dependencies
                    .push((key.importer_id.clone(), result.source_id));
            }
        }
        retirement.queries.sort();
        retirement.queries.dedup();
        self.enforce_slot_cap(&mut retirement);
        self.keep_served_dependencies(&mut retirement.dependencies);
        retirement
    }

    /// Retire every slot `owner` holds whose population `retire` accepts.
    /// Returns their queries.
    pub(crate) fn retire_owner(
        &mut self,
        owner: &str,
        retire: impl Fn(&str, ResolutionPopulation) -> bool,
    ) -> SlotRetirement {
        let owner = verter_session_query::resolution::normalize_canonical_id(owner);
        let mut retirement = SlotRetirement::default();
        self.retire_owners(std::iter::once(owner), &retire, &mut retirement);
        retirement
    }

    /// [`Self::retire_owner`] for every owner at or under `prefix`. Also
    /// returns those owners: every importer the lane held under `prefix`.
    pub(crate) fn retire_owners_under(
        &mut self,
        prefix: &str,
        retire: impl Fn(&str, ResolutionPopulation) -> bool,
    ) -> (Vec<String>, SlotRetirement) {
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
        let mut retirement = SlotRetirement::default();
        self.retire_owners(owners.iter().cloned(), &retire, &mut retirement);
        (owners, retirement)
    }

    pub(crate) fn residency(&self) -> SlotResidency {
        SlotResidency {
            slots: self.slots.len(),
            candidates: self.candidates,
            owners: self.owners.len(),
            queue_entries: self.order.len(),
            slot_capacity: self.slots.capacity(),
            queue_capacity: self.order.capacity(),
        }
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
        retirement: &mut SlotRetirement,
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
                self.remove_slot(&key, retirement);
            }
        }
        self.compact_order();
        self.keep_served_dependencies(&mut retirement.dependencies);
    }

    fn remove_slot(&mut self, key: &LazyResolutionCacheKey, retirement: &mut SlotRetirement) {
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
        let mut slot_queries: Vec<ResolutionQueryKey> = Vec::with_capacity(slot.candidates.len());
        for retained in slot.candidates {
            slot_queries.push(retained.entry.query);
            if let Some(result) = retained.entry.result {
                retirement
                    .dependencies
                    .push((key.importer_id.clone(), result.source_id));
            }
        }
        slot_queries.sort();
        slot_queries.dedup();
        retirement.queries.extend(slot_queries);
    }

    /// Drop every `(importer, dependency)` pair a remaining candidate of
    /// that importer still resolves to; the rest name dependency edges no
    /// retained answer backs any more.
    fn keep_served_dependencies(&self, dependencies: &mut Vec<(String, String)>) {
        dependencies.sort();
        dependencies.dedup();
        dependencies.retain(|(importer, dependency)| {
            let owner = verter_session_query::resolution::normalize_canonical_id(importer);
            let Some(keys) = self.owners.get(&owner) else {
                return true;
            };
            !keys
                .iter()
                .filter(|key| key.importer_id == *importer)
                .filter_map(|key| self.slots.get(key))
                .flat_map(|slot| slot.candidates.iter())
                .any(|retained| {
                    retained
                        .entry
                        .result
                        .as_ref()
                        .is_some_and(|result| result.source_id == *dependency)
                })
        });
    }

    fn enforce_slot_cap(&mut self, retirement: &mut SlotRetirement) {
        while self.slots.len() > self.slot_cap {
            let Some((key, generation)) = self.order.pop_front() else {
                break;
            };
            if self
                .slots
                .get(&key)
                .is_some_and(|slot| slot.generation == generation)
            {
                self.remove_slot(&key, retirement);
            }
        }
        self.compact_order();
    }

    /// Drop the queue's stale entries once they outnumber the live ones,
    /// so the queue stays proportional to the store, and give back backing
    /// storage a drained lane no longer needs.
    fn compact_order(&mut self) {
        if self.order.len() > 2 * self.slots.len() + 64 {
            let slots = &self.slots;
            self.order.retain(|(key, generation)| {
                slots
                    .get(key)
                    .is_some_and(|slot| slot.generation == *generation)
            });
        }
        if self.order.capacity() > 4 * self.order.len() + SHRINK_FLOOR {
            self.order.shrink_to(2 * self.order.len());
        }
        if self.slots.capacity() > 4 * self.slots.len() + SHRINK_FLOOR {
            self.slots.shrink_to(2 * self.slots.len());
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
