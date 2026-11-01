//! Immutable index snapshots and the one publication path.
//!
//! Snapshots share structure through persistent maps, so a publication costs
//! the changed slots and their postings, not the workspace. A reader keeps the
//! snapshot it captured for as long as it holds it; a contribution's retention
//! charge is released when the last snapshot holding it is dropped.

use std::collections::BTreeSet;
use std::sync::Arc;

use arc_swap::ArcSwap;
use imbl::{OrdMap, OrdSet};
use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use verter_session_query::retention::{RetentionCharge, StoreAccount};

use super::delta::{
    CompletePartition, ContributionDelta, ExpectedPrior, ObservationOracle, PublishOutcome,
    PublishRefusal, SlotChange,
};
use super::identity::{
    ContributionId, ContributionKey, ObservationKey, PartitionId, ReadSet, ReadWitness, ScopeId,
    SourceBasis,
};
use super::records::{IndexRecord, PostingKey, Provenance, RecordSlot, TypedSubject};

/// One published or request-local contribution.
#[derive(Debug)]
pub struct Contribution {
    pub id: ContributionId,
    pub key: ContributionKey,
    pub basis: SourceBasis,
    pub provenance: Provenance,
    pub read_set: ReadSet,
    pub records: Vec<IndexRecord>,
    charge: RetentionCharge,
}

impl Contribution {
    /// Bytes this contribution holds against the retention account.
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.charge.bytes()
    }

    fn same_value(&self, partition: &CompletePartition) -> bool {
        let draft = &partition.draft;
        self.basis == draft.basis
            && self.read_set == draft.read_set
            && self.provenance == draft.provenance
            && self.records == draft.records
    }
}

/// Proof that one declared population was fully enumerated on a valid basis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageRecord {
    pub scope: ScopeId,
    /// The declared fact families this population covers.
    pub partitions: BTreeSet<PartitionId>,
    /// The owner's enumeration witness. A list of previously seen files would
    /// miss additions; this changes when the population does.
    pub witness: ReadWitness,
    /// Every slot the population holds.
    pub contributions: BTreeSet<ContributionId>,
}

/// Canonical posting order: profile claim, typed subject, slot id, then record.
/// The table is already part of the [`PostingKey`] that owns the set.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct PostingRef {
    pub claim: super::identity::ProfileClaim,
    pub subject: TypedSubject,
    pub contribution: ContributionId,
    pub slot: RecordSlot,
    pub record_index: u32,
}

/// One immutable view of the index.
#[derive(Debug, Clone, Default)]
pub struct IndexSnapshot {
    generation: u64,
    contributions: OrdMap<ContributionId, Arc<Contribution>>,
    pub(super) postings: OrdMap<PostingKey, OrdSet<PostingRef>>,
    lineages: OrdMap<ContributionKey, ContributionId>,
    by_partition: OrdMap<PartitionId, OrdSet<ContributionId>>,
    /// Which slots consumed each observation.
    dependents: OrdMap<ObservationKey, OrdSet<ContributionId>>,
    coverage: OrdMap<ScopeId, Arc<CoverageRecord>>,
    /// Which coverage records hold each slot or witness observation.
    coverage_by_contribution: OrdMap<ContributionId, OrdSet<ScopeId>>,
    coverage_by_witness: OrdMap<ObservationKey, OrdSet<ScopeId>>,
    /// Slots written only for one request's answer. They exist only in
    /// private snapshots and are never reachable from the store.
    return_only: bool,
}

impl IndexSnapshot {
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub fn contribution(&self, id: &ContributionId) -> Option<&Arc<Contribution>> {
        self.contributions.get(id)
    }

    #[must_use]
    pub fn coverage(&self, scope: &ScopeId) -> Option<&Arc<CoverageRecord>> {
        self.coverage.get(scope)
    }

    /// The slots of one declared fact family. An owner enumerating what it
    /// wrote reads this; a candidate query never does.
    pub fn contributions_in_partition(
        &self,
        partition: &PartitionId,
    ) -> impl Iterator<Item = Arc<Contribution>> + '_ {
        self.by_partition
            .get(partition)
            .into_iter()
            .flatten()
            .filter_map(|id| self.contributions.get(id).cloned())
    }

    /// Live slots.
    #[must_use]
    pub fn contribution_count(&self) -> usize {
        self.contributions.len()
    }

    /// Live posting entries across every key.
    #[must_use]
    pub fn posting_count(&self) -> usize {
        self.postings.values().map(OrdSet::len).sum()
    }

    /// Live slot-per-observation read edges.
    #[must_use]
    pub fn read_edge_count(&self) -> usize {
        self.dependents.values().map(OrdSet::len).sum()
    }

    /// `true` for a request-local snapshot holding unpublished rows.
    #[must_use]
    pub fn is_return_only(&self) -> bool {
        self.return_only
    }

    /// A private snapshot with request-local rows added and slots the walk
    /// proved gone removed. It answers one request and cannot be published.
    pub(super) fn with_return_only(
        &self,
        contributions: impl IntoIterator<Item = Contribution>,
        gone: &[ContributionId],
    ) -> Self {
        let mut next = self.clone();
        for id in gone {
            next.remove_contribution(id);
        }
        for contribution in contributions {
            next.remove_contribution(&contribution.id);
            next.insert_contribution(Arc::new(contribution));
        }
        next.return_only = true;
        next
    }

    fn insert_contribution(&mut self, contribution: Arc<Contribution>) {
        for witness in &contribution.read_set.0 {
            self.dependents
                .entry(witness.key)
                .or_default()
                .insert(contribution.id.clone());
        }
        for (index, record) in contribution.records.iter().enumerate() {
            let posting = PostingRef {
                claim: contribution.key.claim.clone(),
                subject: record.subject.clone(),
                contribution: contribution.id.clone(),
                slot: record.slot,
                record_index: u32::try_from(index).unwrap_or(u32::MAX),
            };
            for key in &record.keys {
                self.postings
                    .entry(key.clone())
                    .or_default()
                    .insert(posting.clone());
            }
        }
        self.lineages
            .insert(contribution.key.lineage(), contribution.id.clone());
        self.by_partition
            .entry(contribution.key.partition.clone())
            .or_default()
            .insert(contribution.id.clone());
        self.contributions
            .insert(contribution.id.clone(), contribution);
    }

    /// Remove a slot, its forward and reverse postings and read edges. Returns
    /// the removed value.
    fn remove_contribution(&mut self, id: &ContributionId) -> Option<Arc<Contribution>> {
        let removed = self.contributions.remove(id)?;
        for witness in &removed.read_set.0 {
            remove_from_set(&mut self.dependents, &witness.key, id);
        }
        for (index, record) in removed.records.iter().enumerate() {
            let posting = PostingRef {
                claim: removed.key.claim.clone(),
                subject: record.subject.clone(),
                contribution: removed.id.clone(),
                slot: record.slot,
                record_index: u32::try_from(index).unwrap_or(u32::MAX),
            };
            for key in &record.keys {
                remove_from_set(&mut self.postings, key, &posting);
            }
        }
        self.lineages.remove(&removed.key.lineage());
        remove_from_set(&mut self.by_partition, &removed.key.partition, id);
        Some(removed)
    }

    fn insert_coverage(&mut self, record: CoverageRecord) {
        self.remove_coverage(&record.scope);
        for id in &record.contributions {
            self.coverage_by_contribution
                .entry(id.clone())
                .or_default()
                .insert(record.scope.clone());
        }
        self.coverage_by_witness
            .entry(record.witness.key)
            .or_default()
            .insert(record.scope.clone());
        self.coverage.insert(record.scope.clone(), Arc::new(record));
    }

    fn remove_coverage(&mut self, scope: &ScopeId) {
        let Some(record) = self.coverage.remove(scope) else {
            return;
        };
        for id in &record.contributions {
            remove_from_set(&mut self.coverage_by_contribution, id, scope);
        }
        remove_from_set(&mut self.coverage_by_witness, &record.witness.key, scope);
    }

    /// Coverage cannot survive any change to a slot it holds.
    fn retire_coverage_holding(&mut self, id: &ContributionId) {
        let scopes = self.coverage_by_contribution.get(id).cloned();
        for scope in scopes.iter().flatten() {
            self.remove_coverage(scope);
        }
    }
}

fn remove_from_set<K, V>(map: &mut OrdMap<K, OrdSet<V>>, key: &K, value: &V)
where
    K: Ord + Clone,
    V: Ord + Clone,
{
    let now_empty = map.get_mut(key).is_some_and(|set| {
        set.remove(value);
        set.is_empty()
    });
    if now_empty {
        map.remove(key);
    }
}

/// What one invalidation touched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InvalidationWork {
    /// Dependent slots removed.
    pub contributions_retired: u32,
    /// Coverage records removed.
    pub coverage_retired: u32,
}

/// The store: one current snapshot and one writer at a time. Publication and
/// invalidation swap whole snapshots; readers never take the writer lock.
pub struct WorkspaceIndex {
    current: ArcSwap<IndexSnapshot>,
    writer: Mutex<()>,
    account: StoreAccount,
    /// The workspace change counters a population was last proved current at.
    /// One small entry per distinct demand; a validity shortcut, never a
    /// substitute for coverage.
    population_stamps: Mutex<FxHashMap<ScopeId, (u64, u64)>>,
}

impl std::fmt::Debug for WorkspaceIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceIndex")
            .field("generation", &self.current.load().generation)
            .finish_non_exhaustive()
    }
}

impl WorkspaceIndex {
    /// An empty store charging `account`.
    #[must_use]
    pub fn new(account: StoreAccount) -> Self {
        Self {
            current: ArcSwap::from_pointee(IndexSnapshot::default()),
            writer: Mutex::new(()),
            account,
            population_stamps: Mutex::default(),
        }
    }

    /// The account this store charges.
    #[must_use]
    pub fn account(&self) -> &StoreAccount {
        &self.account
    }

    pub(super) fn population_stamp(&self, scope: &ScopeId) -> Option<(u64, u64)> {
        self.population_stamps.lock().get(scope).copied()
    }

    pub(super) fn record_population_stamp(&self, scope: ScopeId, stamp: (u64, u64)) {
        self.population_stamps.lock().insert(scope, stamp);
    }

    /// Drop every slot and population. Readers keep what they captured.
    pub fn clear(&self) {
        let _writer = self.writer.lock();
        let generation = self.current.load().generation + 1;
        self.current.store(Arc::new(IndexSnapshot {
            generation,
            ..IndexSnapshot::default()
        }));
        self.population_stamps.lock().clear();
    }

    /// Capture the current snapshot.
    #[must_use]
    pub fn snapshot(&self) -> Arc<IndexSnapshot> {
        self.current.load_full()
    }

    /// Publish a delta as one snapshot, or commit nothing.
    ///
    /// The fence runs in order: prior slot bases, schema layouts, epochs,
    /// every consumed observation, retention, cancellation. A failure at any
    /// step leaves the current snapshot untouched, including for sibling
    /// changes in the same delta.
    pub fn publish(
        &self,
        delta: ContributionDelta,
        oracle: &dyn ObservationOracle,
        is_cancelled: &dyn Fn() -> bool,
    ) -> PublishOutcome {
        let _writer = self.writer.lock();
        if is_cancelled() {
            return PublishOutcome::Refused(PublishRefusal::Cancelled);
        }
        let current = self.current.load_full();

        if let Err(refusal) = check_epochs(&delta, oracle) {
            return PublishOutcome::Refused(refusal);
        }
        let plan = match PublicationPlan::fence(&current, delta, oracle) {
            Ok(plan) => plan,
            Err(refusal) => return PublishOutcome::Refused(refusal),
        };
        if plan.is_noop() {
            return PublishOutcome::AlreadyCurrent {
                generation: current.generation,
            };
        }
        let next = match plan.apply(&current) {
            Ok(next) => next,
            Err(refusal) => return PublishOutcome::Refused(refusal),
        };
        // Cancellation at the last boundary still discards the whole delta.
        if is_cancelled() {
            return PublishOutcome::Refused(PublishRefusal::Cancelled);
        }
        let generation = next.generation;
        self.current.store(Arc::new(next));
        PublishOutcome::Published { generation }
    }

    /// Make every slot and population that consumed `observations` ineligible
    /// immediately. Requests cannot serve them as current while a rebuild is
    /// pending; unrelated partitions stay.
    pub fn invalidate(&self, observations: &[ObservationKey]) -> InvalidationWork {
        let _writer = self.writer.lock();
        let current = self.current.load_full();
        let mut next = (*current).clone();
        let mut work = InvalidationWork::default();
        for key in observations {
            let slots = next.dependents.get(key).cloned();
            for id in slots.iter().flatten() {
                let before = next.coverage.len();
                next.retire_coverage_holding(id);
                work.coverage_retired += count_u32(before - next.coverage.len());
                if next.remove_contribution(id).is_some() {
                    work.contributions_retired += 1;
                }
            }
            let scopes = next.coverage_by_witness.get(key).cloned();
            for scope in scopes.iter().flatten() {
                next.remove_coverage(scope);
                work.coverage_retired += 1;
            }
        }
        if work != InvalidationWork::default() {
            next.generation = current.generation + 1;
            self.current.store(Arc::new(next));
        }
        work
    }
}

fn count_u32(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn check_epochs(
    delta: &ContributionDelta,
    oracle: &dyn ObservationOracle,
) -> Result<(), PublishRefusal> {
    if oracle.current_epochs() == (delta.selection_epoch, delta.demand_epoch) {
        Ok(())
    } else {
        Err(PublishRefusal::Stale)
    }
}

fn witness_current(witness: &ReadWitness, oracle: &dyn ObservationOracle) -> bool {
    oracle.current(&witness.key) == Some(witness.fingerprint)
}

/// A delta that passed the fence, with idempotent replays removed.
struct PublicationPlan {
    upserts: Vec<Box<CompletePartition>>,
    retirements: Vec<ContributionId>,
    coverage: Vec<CoverageRecord>,
}

impl PublicationPlan {
    fn fence(
        current: &IndexSnapshot,
        delta: ContributionDelta,
        oracle: &dyn ObservationOracle,
    ) -> Result<Self, PublishRefusal> {
        let mut plan = Self {
            upserts: Vec::with_capacity(delta.changes.len()),
            retirements: Vec::new(),
            coverage: Vec::new(),
        };
        let retiring: BTreeSet<ContributionId> = delta
            .changes
            .iter()
            .filter_map(|change| match change {
                SlotChange::Retire { id, .. } => Some(id.clone()),
                SlotChange::Upsert { .. } => None,
            })
            .collect();
        for change in delta.changes {
            match change {
                SlotChange::Upsert {
                    partition,
                    expected,
                } => {
                    if Self::fence_upsert(current, &partition, &expected, &retiring, oracle)? {
                        plan.upserts.push(partition);
                    }
                }
                SlotChange::Retire {
                    id,
                    expected,
                    evidence,
                } => {
                    let holds_expected = current
                        .contributions
                        .get(&id)
                        .is_some_and(|held| held.basis == *expected);
                    if !holds_expected {
                        return Err(PublishRefusal::Conflict(id));
                    }
                    if !witness_current(&evidence.witness, oracle) {
                        return Err(PublishRefusal::Stale);
                    }
                    plan.retirements.push(id);
                }
            }
        }
        for record in delta.coverage {
            if !witness_current(&record.witness, oracle) {
                return Err(PublishRefusal::Stale);
            }
            plan.check_coverage_slots(current, &record)?;
            // A change to a held slot retires the old record, so an identical
            // record must be written again in that case.
            let slot_changes = record.contributions.iter().any(|id| {
                plan.retirements.contains(id)
                    || plan.upserts.iter().any(|partition| partition.id() == *id)
            });
            let unchanged = current
                .coverage
                .get(&record.scope)
                .is_some_and(|held| **held == record);
            if slot_changes || !unchanged {
                plan.coverage.push(record);
            }
        }
        Ok(plan)
    }

    /// `true` when the partition must be written; `false` for an exact replay.
    fn fence_upsert(
        current: &IndexSnapshot,
        partition: &CompletePartition,
        expected: &ExpectedPrior,
        retiring: &BTreeSet<ContributionId>,
        oracle: &dyn ObservationOracle,
    ) -> Result<bool, PublishRefusal> {
        let id = partition.id();
        let held = current.contributions.get(&id);
        if let Some(held) = held {
            if held.same_value(partition) {
                return Ok(false);
            }
            if held.basis == partition.draft.basis {
                return Err(PublishRefusal::ReplayMismatch(id));
            }
        }
        let matches_expected = match (expected, held) {
            (ExpectedPrior::Absent, None) => true,
            (ExpectedPrior::Basis(basis), Some(held)) => held.basis == **basis,
            _ => false,
        };
        if !matches_expected {
            return Err(PublishRefusal::Conflict(id));
        }
        let lineage_holder = current.lineages.get(&partition.draft.key.lineage());
        if lineage_holder.is_some_and(|holder| *holder != id && !retiring.contains(holder)) {
            return Err(PublishRefusal::ParallelSchemaLayout(id));
        }
        if !partition
            .draft
            .read_set
            .0
            .iter()
            .all(|witness| witness_current(witness, oracle))
        {
            return Err(PublishRefusal::Stale);
        }
        Ok(true)
    }

    fn check_coverage_slots(
        &self,
        current: &IndexSnapshot,
        record: &CoverageRecord,
    ) -> Result<(), PublishRefusal> {
        for id in &record.contributions {
            let written = self.upserts.iter().any(|partition| partition.id() == *id);
            let retained = current.contributions.contains_key(id) && !self.retirements.contains(id);
            if !written && !retained {
                return Err(PublishRefusal::CoverageWithoutPartition(id.clone()));
            }
        }
        Ok(())
    }

    fn is_noop(&self) -> bool {
        self.upserts.is_empty() && self.retirements.is_empty() && self.coverage.is_empty()
    }

    fn apply(self, current: &IndexSnapshot) -> Result<IndexSnapshot, PublishRefusal> {
        let mut next = current.clone();
        for id in &self.retirements {
            next.retire_coverage_holding(id);
            next.remove_contribution(id);
        }
        for partition in self.upserts {
            let CompletePartition { draft, mut charge } = *partition;
            charge
                .try_adopt_retained()
                .map_err(|_| PublishRefusal::RetentionPressure)?;
            let id = draft.key.id();
            next.retire_coverage_holding(&id);
            next.remove_contribution(&id);
            next.insert_contribution(Arc::new(Contribution {
                id,
                key: draft.key,
                basis: draft.basis,
                provenance: draft.provenance,
                read_set: draft.read_set,
                records: draft.records,
                charge: charge.commit_retained(),
            }));
        }
        for record in self.coverage {
            next.insert_coverage(record);
        }
        next.generation = current.generation + 1;
        Ok(next)
    }
}

/// Build the request-local form of a partial walk's rows.
pub(super) fn return_only_contribution(partial: super::delta::PartialEnumeration) -> Contribution {
    let super::delta::PartialEnumeration { draft, charge, .. } = partial;
    Contribution {
        id: draft.key.id(),
        key: draft.key,
        basis: draft.basis,
        provenance: draft.provenance,
        read_set: draft.read_set,
        records: draft.records,
        charge,
    }
}
