//! Cost receipts: the exclusive cost of each semantic computation a
//! connected demand performs, kept as a shared DAG so that serving a result
//! warm charges exactly what computing it cold would have — each
//! computation once per connected demand, however many consumers share it.
//!
//! A receipt records only its OWN (exclusive) work units, construction
//! bytes, request operations and structured relation comparisons, plus shared references to the receipts of
//! the computations it consumed. The cost of demanding a result is the sum
//! of the exclusive costs over the DISTINCT computations its receipt
//! reaches: a set sum, so neither sharing, warmth nor the order siblings are
//! visited in can change it, and a diamond of shared sub-results costs
//! linearly, not exponentially.
//!
//! The ledger owns the recording: a cold computation opens a
//! [`CostScope`], every charge the ledger takes while the scope is on top
//! accrues to it, each consumed result is recorded as a prerequisite, and
//! sealing the scope yields the receipt and marks it paid for the demand.
//! Serving a result warm ([`ConnectedDemandLedger::replay_admit`]) charges
//! the unpaid part of its receipt's closure in one admission, or nothing.
//!
//! Every stored semantic result carries its receipt: the memo cannot admit
//! a candidate without one, so no read can deliver a stored value without
//! paying for it.
//!
//! [`ConnectedDemandLedger::replay_admit`]: super::connected_demand::ConnectedDemandLedger::replay_admit

use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock, Weak};

use parking_lot::Mutex;
use rustc_hash::{FxHashMap, FxHashSet};
use verter_execution::tasks::ProducerIdentity;
use verter_session_query::retention::{
    ChargeClass, RetentionAdmission, RetentionCharge, RetentionRefusal, SemanticRetentionAccount,
};

/// The revision of the logical cost model: what a work unit, a
/// construction byte, a request operation and a relation comparison are
/// charged for. A receipt is meaningful only under the revision that
/// recorded it, and a refusal only under the [`BudgetProfile`] that refused
/// it.
pub const COST_MODEL_REVISION: u32 = 3;

/// The exact identity of one semantic computation: the canonical demand
/// and the materialized point it answers, as the memo's prepared key
/// encodes them. Equality compares the whole identity, so two computations
/// never share a receipt by hash collision; the hash only speeds hashing
/// and the common inequality.
#[derive(Clone)]
pub struct CostIdentity {
    roots: crate::semantic_query_memo::node_roots::NodeRootSet,
    hash: u64,
    key: Arc<dyn ProducerIdentity>,
}

impl CostIdentity {
    /// The identity of the computation `key` encodes exactly.
    pub fn new(key: impl Into<Arc<[u8]>>) -> Self {
        let key: Arc<[u8]> = key.into();
        let identity = EncodedIdentity {
            hash: digest_of(&key),
            key,
        };
        Self::of_producer(Arc::new(identity))
    }

    /// The identity of a computation keyed by `key` alone, distinct from
    /// every computation keyed by another type.
    pub(crate) fn of_key<K>(key: K) -> Self
    where
        K: Hash + Eq + Send + Sync + 'static,
    {
        let hash = std::hash::BuildHasher::hash_one(&rustc_hash::FxBuildHasher, &key);
        Self::of_producer(Arc::new(KeyedIdentity { hash, key }))
    }

    /// The identity of the computation the producer `key` names: a
    /// prepared query identity, shared rather than copied.
    pub(crate) fn of_producer(key: Arc<dyn ProducerIdentity>) -> Self {
        Self {
            roots: Default::default(),
            hash: key.producer_hash(),
            key,
        }
    }

    pub(crate) fn with_roots(
        mut self,
        roots: crate::semantic_query_memo::node_roots::NodeRootSet,
    ) -> Self {
        self.roots = roots;
        self
    }
}

impl CostIdentity {
    /// What the identity keeps reachable, once per `aliases`: its operand
    /// roots and its producer key's block, with the heap that key reaches.
    /// An encoded key owns its bytes; a prepared query identity owns its
    /// key's and family's slices, names and records (see
    /// [`crate::semantic_query_memo::prepared::PreparedKeyHandle::producer_heap_bytes`]);
    /// a typed key is inline in its block.
    pub(crate) fn owned_heap_bytes(
        &self,
        aliases: &mut verter_session_query::retention::allocation::AliasSet,
    ) -> usize {
        use crate::semantic_query_memo::prepared::PreparedKeyHandle;
        use verter_session_query::retention::allocation::{arc_block_bytes, arc_slice_block_bytes};
        let key = if aliases.first(&self.key) {
            let block = arc_block_bytes(std::mem::size_of_val::<dyn ProducerIdentity>(
                self.key.as_ref(),
            ));
            let heap = match self.key.downcast_ref::<EncodedIdentity>() {
                Some(encoded) => arc_slice_block_bytes(&encoded.key, aliases).unwrap_or(0),
                None => {
                    PreparedKeyHandle::producer_heap_bytes(self.key.as_ref(), aliases).unwrap_or(0)
                }
            };
            block + heap
        } else {
            0
        };
        key + self.roots.owned_heap_bytes(aliases)
    }

    /// The identity's hash: equal identities hash equal.
    pub(crate) fn hash_value(&self) -> u64 {
        self.hash
    }
}

impl std::fmt::Debug for CostIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("CostIdentity").field(&self.hash).finish()
    }
}

impl PartialEq for CostIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
            && (Arc::ptr_eq(&self.key, &other.key) || self.key.same_producer(other.key.as_ref()))
    }
}

impl Eq for CostIdentity {}

impl Hash for CostIdentity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

/// The identity of one failed-prefix receipt
/// ([`DemandCostReceipt::failed_prefix`]): its own allocation, equal only
/// to itself. The receipt holding it keeps the allocation, and so the
/// address its hash is taken from, alive.
struct FailedPrefixIdentity;

impl ProducerIdentity for FailedPrefixIdentity {
    fn producer_hash(&self) -> u64 {
        std::ptr::from_ref(self) as usize as u64
    }

    fn same_producer(&self, other: &dyn ProducerIdentity) -> bool {
        other
            .downcast_ref::<Self>()
            .is_some_and(|other| std::ptr::eq(self, other))
    }
}

/// A computation identified by an exact byte encoding.
struct EncodedIdentity {
    hash: u64,
    key: Arc<[u8]>,
}

impl ProducerIdentity for EncodedIdentity {
    fn producer_hash(&self) -> u64 {
        self.hash
    }

    fn same_producer(&self, other: &dyn ProducerIdentity) -> bool {
        other
            .downcast_ref::<Self>()
            .is_some_and(|other| self.key == other.key)
    }
}

/// A computation identified by a typed key.
struct KeyedIdentity<K> {
    hash: u64,
    key: K,
}

impl<K: Eq + Send + Sync + 'static> ProducerIdentity for KeyedIdentity<K> {
    fn producer_hash(&self) -> u64 {
        self.hash
    }

    fn same_producer(&self, other: &dyn ProducerIdentity) -> bool {
        other
            .downcast_ref::<Self>()
            .is_some_and(|other| self.key == other.key)
    }
}

/// The 64-bit digest of `bytes`, by the project's content hasher.
fn digest_of(bytes: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(bytes)
}

/// The logical cost of work: its work units, its construction bytes, the
/// request operations (the per-request projection fuse) it spent, and the
/// structured comparisons its relation checks recorded (the checker's
/// relation-complexity count).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LogicalUsage {
    pub work: u64,
    pub bytes: u64,
    pub operations: u64,
    pub comparisons: u64,
}

impl LogicalUsage {
    /// The sum of two usages, `None` on overflow.
    pub(crate) fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            work: self.work.checked_add(other.work)?,
            bytes: self.bytes.checked_add(other.bytes)?,
            operations: self.operations.checked_add(other.operations)?,
            comparisons: self.comparisons.checked_add(other.comparisons)?,
        })
    }

    /// The sum of two usages, saturating.
    pub(crate) fn saturating_add(self, other: Self) -> Self {
        Self {
            work: self.work.saturating_add(other.work),
            bytes: self.bytes.saturating_add(other.bytes),
            operations: self.operations.saturating_add(other.operations),
            comparisons: self.comparisons.saturating_add(other.comparisons),
        }
    }
}

/// How a computation reached a result it consumed: the nested query level
/// the read entered, and how the result's own instantiation frames sit
/// below the consumer's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nesting {
    /// Read in place: no nested query level.
    InPlace,
    /// Entered as a nested synchronous query: one nested query level.
    Entered,
    /// Entered as the root of a continuation drive: one nested query
    /// level, the result's instantiation frames counted from the drive's
    /// first frame.
    Drive,
    /// A continuation frame's need, whose own frame runs at instantiation
    /// depth `depth` — one below the needing frame.
    Frame { depth: u32 },
    /// The prerequisites of a recording that did not complete, folded into
    /// its consumer as one failed-prefix receipt
    /// ([`DemandCostReceipt::failed_prefix`]): they are the consumer's own,
    /// so the receipt's footprint is the consumer's as it stands.
    Folded,
}

impl Nesting {
    /// The nested query levels the read enters.
    pub fn query_levels(self) -> u16 {
        match self {
            Self::Entered | Self::Drive => 1,
            Self::InPlace | Self::Frame { .. } | Self::Folded => 0,
        }
    }

    /// The deeper of two ways one result was consumed.
    fn deeper(self, other: Self) -> Self {
        let rank = |nesting: Self| match nesting {
            Self::InPlace | Self::Folded => 0,
            Self::Entered => 1,
            Self::Drive => 2,
            Self::Frame { .. } => 3,
        };
        if rank(other) > rank(self) {
            other
        } else {
            self
        }
    }
}

/// One prerequisite a computation consumed: the prerequisite's receipt,
/// and how it was consumed.
#[derive(Clone, Debug)]
pub struct CostDependency {
    pub receipt: Arc<DemandCostReceipt>,
    pub nesting: Nesting,
}

/// The allowances a computation's cold run needed besides its charges:
/// what a replay must find room for so that serving the result warm never
/// passes an allowance its cold evaluation would have stopped at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OperationFootprint {
    /// The deepest nested query level its closure needs below its entry.
    pub query_depth: u16,
    /// The deepest nested query level a continuation frame of its closure
    /// needs below that frame's own entry. A frame runs from its drive,
    /// never nested in the frame that needed it, so its depth is not
    /// counted below whatever its result's reader was evaluating.
    pub frame_query_depth: u16,
    /// The deepest instantiation frame its closure runs, counted from its
    /// own frame (0 when it runs no frame below its own).
    pub instantiation_height: u16,
    /// The least instantiation allowance its closure's own continuation
    /// drives need, wherever it is entered.
    pub instantiation_required: u32,
    /// The most steps any one conditional tail run in its closure took.
    pub tail_steps: u32,
}

/// The sealed cost of one computation: its identity, its exclusive usage,
/// the receipts of the computations it consumed (each once), and the
/// operation footprint its closure needs below its own entry.
#[derive(Debug)]
pub struct DemandCostReceipt {
    identity: CostIdentity,
    exclusive: LogicalUsage,
    prerequisites: Box<[CostDependency]>,
    footprint: OperationFootprint,
    /// This receipt's own reservation against the aggregate retention
    /// account, taken the first time a store retains a result it costs
    /// ([`Self::reserve_retention`]) and released when the receipt drops —
    /// however many stores, candidates and consumers' receipts share it.
    retention: OnceLock<RetentionCharge>,
}

impl DemandCostReceipt {
    /// A receipt for `identity` with `exclusive` usage over
    /// `prerequisites`, its own tail runs taking no step.
    pub fn new(
        identity: CostIdentity,
        exclusive: LogicalUsage,
        prerequisites: Vec<CostDependency>,
    ) -> Arc<Self> {
        Self::with_tail_steps(identity, exclusive, prerequisites, 0)
    }

    /// A receipt for `identity` with `exclusive` usage over
    /// `prerequisites`, deduplicated by identity (each keeping the deepest
    /// way it was consumed), whose own tail runs took at most `tail_steps`
    /// steps. Its footprint is its prerequisites' footprints, each moved by
    /// how it was consumed.
    pub fn with_tail_steps(
        identity: CostIdentity,
        exclusive: LogicalUsage,
        prerequisites: Vec<CostDependency>,
        tail_steps: u32,
    ) -> Arc<Self> {
        let mut unique: Vec<CostDependency> = Vec::with_capacity(prerequisites.len());
        if prerequisites.len() <= LINEAR_DEDUP_LIMIT {
            // A short list dedups by scanning: no identity is cloned into a
            // table, and the stored hash rejects most pairs at once.
            for dependency in prerequisites {
                let mut kept_at = None;
                for (position, kept) in unique.iter().enumerate() {
                    if kept.receipt.identity == dependency.receipt.identity {
                        kept_at = Some(position);
                        break;
                    }
                }
                match kept_at {
                    Some(position) => {
                        let kept = &mut unique[position];
                        kept.nesting = kept.nesting.deeper(dependency.nesting);
                    }
                    None => unique.push(dependency),
                }
            }
        } else {
            let mut positions: FxHashMap<CostIdentity, usize> = FxHashMap::default();
            for dependency in prerequisites {
                match positions.get(&dependency.receipt.identity) {
                    Some(&position) => {
                        let kept = &mut unique[position];
                        kept.nesting = kept.nesting.deeper(dependency.nesting);
                    }
                    None => {
                        positions.insert(dependency.receipt.identity.clone(), unique.len());
                        unique.push(dependency);
                    }
                }
            }
        }
        let mut footprint = OperationFootprint {
            tail_steps,
            ..OperationFootprint::default()
        };
        for dependency in &unique {
            let below = dependency.receipt.footprint;
            let levels = dependency.nesting.query_levels();
            footprint.instantiation_required = footprint
                .instantiation_required
                .max(below.instantiation_required);
            footprint.tail_steps = footprint.tail_steps.max(below.tail_steps);
            match dependency.nesting {
                // The result's frames run one level below the consumer's,
                // each from the drive: its own nesting is a frame's.
                Nesting::Frame { .. } => {
                    footprint.instantiation_height = footprint
                        .instantiation_height
                        .max(below.instantiation_height.saturating_add(1));
                    footprint.frame_query_depth = footprint
                        .frame_query_depth
                        .max(below.query_depth)
                        .max(below.frame_query_depth);
                }
                // The result's frames run in a drive of their own, from
                // its first frame, entered where the consumer is: every
                // frame of it nests below the consumer's entry.
                Nesting::Drive => {
                    footprint.instantiation_required = footprint
                        .instantiation_required
                        .max(u32::from(below.instantiation_height) + 1);
                    footprint.query_depth = footprint.query_depth.max(
                        below
                            .query_depth
                            .max(below.frame_query_depth)
                            .saturating_add(levels),
                    );
                }
                // A failed prefix's frames are its consumer's own.
                Nesting::Folded => {
                    footprint.instantiation_height = footprint
                        .instantiation_height
                        .max(below.instantiation_height);
                }
                // A result read synchronously is evaluated without frames
                // inside a running drive; frames its closure ran keep
                // running from the drive.
                Nesting::InPlace | Nesting::Entered => {
                    footprint.query_depth = footprint
                        .query_depth
                        .max(below.query_depth.saturating_add(levels));
                    footprint.frame_query_depth =
                        footprint.frame_query_depth.max(below.frame_query_depth);
                }
            }
        }
        Arc::new(Self {
            identity,
            exclusive,
            prerequisites: unique.into_boxed_slice(),
            footprint,
            retention: OnceLock::new(),
        })
    }

    /// The failed-prefix receipt of `prerequisites`: what a recording that
    /// did not complete consumed, sealed as one receipt its consumer owes in
    /// their place ([`Nesting::Folded`]). Its identity is its own
    /// allocation, so it is never confused with a computation that
    /// completed, never paid on its own and never shared; it owes nothing
    /// of its own, the recording's usage having been folded into the
    /// consumer's. A chain of recordings that each fail folds one receipt
    /// per level into the next, never the whole closure again.
    pub(crate) fn failed_prefix(prerequisites: Vec<CostDependency>) -> Arc<Self> {
        Self::new(
            CostIdentity::of_producer(Arc::new(FailedPrefixIdentity)),
            LogicalUsage::default(),
            prerequisites,
        )
    }

    /// The computation this receipt costs.
    pub fn identity(&self) -> &CostIdentity {
        &self.identity
    }

    /// Its own usage, without its prerequisites'.
    pub fn exclusive(&self) -> LogicalUsage {
        self.exclusive
    }

    /// The receipts it consumed.
    pub fn prerequisites(&self) -> &[CostDependency] {
        &self.prerequisites
    }

    /// The deepest logical nesting it needs below its own entry.
    pub fn depth(&self) -> u16 {
        self.footprint.query_depth
    }

    /// The allowances its closure's cold run needed.
    pub fn footprint(&self) -> OperationFootprint {
        self.footprint
    }

    /// The bytes this receipt keeps alive of its own: its block, its
    /// prerequisite list's backing and its own identity, with the owned
    /// names its identity reaches (each charged by its own cell, held by the
    /// receipt). A prerequisite's identity lives in the prerequisite's
    /// receipt, which charges it.
    fn retained_footprint(&self) -> verter_session_query::retention::Footprint {
        use verter_session_query::retention::allocation::{
            arc_block_bytes, heap_block_bytes, AliasSet,
        };
        let mut aliases = AliasSet::new();
        let bytes = arc_block_bytes(std::mem::size_of::<Self>())
            + heap_block_bytes(std::mem::size_of_val::<[CostDependency]>(
                &self.prerequisites,
            ))
            + self.identity.owned_heap_bytes(&mut aliases);
        verter_session_query::retention::Footprint {
            bytes,
            handles: aliases.take_handles(),
        }
    }

    /// Reserve this receipt, and every receipt of its closure not yet
    /// reserved, against `account`: a store is about to retain a result it
    /// costs. Each receipt is reserved once, by whichever store retains it
    /// first, and its reservation lives exactly as long as the receipt —
    /// so a closure kept alive by a surviving consumer stays charged after
    /// the candidates that first retained it are evicted. A receipt is
    /// reserved only after its prerequisites are, so a reserved receipt's
    /// whole closure is reserved and the walk stops there.
    ///
    /// `Err` is the account's refusal: the store keeps nothing, and the
    /// receipts already reserved stay charged for as long as they live.
    pub(crate) fn reserve_retention(
        self: &Arc<Self>,
        account: &Arc<SemanticRetentionAccount>,
    ) -> Result<(), RetentionRefusal> {
        if self.retention.get().is_some() {
            return Ok(());
        }
        // Most receipts are retained after every receipt they consumed
        // already is: reserve the receipt alone, without walking.
        let mut closure_reserved = true;
        for dependency in self.prerequisites.iter() {
            if dependency.receipt.retention.get().is_none() {
                closure_reserved = false;
                break;
            }
        }
        if closure_reserved {
            let footprint = self.retained_footprint();
            return match account.reserve(ChargeClass::Retained, footprint.bytes) {
                RetentionAdmission::Admitted(charge) => {
                    drop(self.retention.set(footprint.handles.retained_with(
                        charge,
                        verter_session_query::retention::owned::ChargeCtx::new(account),
                    )?));
                    Ok(())
                }
                RetentionAdmission::Refused(refusal) => Err(refusal),
            };
        }
        let mut visited: FxHashSet<*const Self> = FxHashSet::default();
        let mut stack: Vec<(&Self, bool)> = vec![(self.as_ref(), false)];
        while let Some((next, expanded)) = stack.pop() {
            if next.retention.get().is_some() {
                continue;
            }
            if expanded {
                let footprint = next.retained_footprint();
                match account.reserve(ChargeClass::Retained, footprint.bytes) {
                    // A concurrent store that reserved it first keeps its
                    // charge; this one releases on drop.
                    RetentionAdmission::Admitted(charge) => {
                        drop(next.retention.set(footprint.handles.retained_with(
                            charge,
                            verter_session_query::retention::owned::ChargeCtx::new(account),
                        )?))
                    }
                    RetentionAdmission::Refused(refusal) => return Err(refusal),
                }
                continue;
            }
            if !visited.insert(std::ptr::from_ref(next)) {
                continue;
            }
            stack.push((next, true));
            stack.extend(
                next.prerequisites
                    .iter()
                    .filter(|dependency| dependency.receipt.retention.get().is_none())
                    .map(|dependency| (dependency.receipt.as_ref(), false)),
            );
        }
        Ok(())
    }

    /// The usage of its whole closure, each distinct computation once.
    pub fn closure_usage(&self) -> LogicalUsage {
        let mut seen: rustc_hash::FxHashSet<&CostIdentity> = rustc_hash::FxHashSet::default();
        let mut total = LogicalUsage::default();
        let mut stack: Vec<&DemandCostReceipt> = vec![self];
        while let Some(next) = stack.pop() {
            if !seen.insert(&next.identity) {
                continue;
            }
            total = total.saturating_add(next.exclusive);
            stack.extend(
                next.prerequisites
                    .iter()
                    .map(|dependency| dependency.receipt.as_ref()),
            );
        }
        total
    }
}

/// The prerequisite count up to which a receipt dedups its prerequisites
/// by scanning rather than through a table.
const LINEAR_DEDUP_LIMIT: usize = 16;

impl Drop for DemandCostReceipt {
    /// Release the prerequisite DAG iteratively: a receipt reaches the
    /// receipts of a chain as deep as the chain of computations it costs,
    /// and dropping them recursively would spend a native frame per level.
    fn drop(&mut self) {
        if self.prerequisites.is_empty() {
            return;
        }
        let mut pending: Vec<CostDependency> = std::mem::take(&mut self.prerequisites).into_vec();
        while let Some(dependency) = pending.pop() {
            if let Some(mut last) = Arc::into_inner(dependency.receipt) {
                if !last.prerequisites.is_empty() {
                    pending.extend(std::mem::take(&mut last.prerequisites).into_vec());
                }
            }
        }
    }
}

/// The cost a read's value carries, part of every read envelope
/// ([`CacheRead`](crate::semantic_query::CacheRead)): every constructor
/// states it, so no read can deliver a computed value with its cost
/// detached.
#[derive(Clone, Debug)]
pub enum ReadReceipt {
    /// A stored, joined or freshly built result and what computing it
    /// cost. The consumer replays it before using the value and records it
    /// as a prerequisite of its own computation.
    Priced(Arc<DemandCostReceipt>),
    /// No computed result to pay for: a recursion carrier, a cancellation,
    /// a typed refusal, or a build that sealed no receipt (which the memo
    /// never admits).
    Unpriced,
}

impl ReadReceipt {
    /// The receipt to replay and record, if the read is priced.
    pub fn priced(&self) -> Option<&Arc<DemandCostReceipt>> {
        match self {
            Self::Priced(receipt) => Some(receipt),
            Self::Unpriced => None,
        }
    }
}

impl From<Option<Arc<DemandCostReceipt>>> for ReadReceipt {
    fn from(receipt: Option<Arc<DemandCostReceipt>>) -> Self {
        receipt.map_or(Self::Unpriced, Self::Priced)
    }
}

/// An open cold computation's recording: its identity, the usage accrued
/// while it is on top, the prerequisites it consumed, and the most steps a
/// conditional tail run of its own took.
#[derive(Debug)]
pub struct CostScope {
    pub(super) identity: CostIdentity,
    pub(super) exclusive: LogicalUsage,
    pub(super) prerequisites: Vec<CostDependency>,
    pub(super) tail_steps: u32,
}

/// Why a replay was refused: the rail the unpaid closure would pass, or
/// the allowance its cold run needed that the reader lacks, with nothing
/// charged. Construction bytes never refuse a replay: a complete result is
/// never rejected at handoff for the bytes it took to build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayRefusal {
    /// The closure's work would pass the remaining work allowance.
    Work,
    /// The closure's request operations would pass the request's remaining
    /// operation allowance.
    Operations,
    /// The receipt's nesting would pass the remaining query depth.
    Depth,
    /// The closure's instantiation frames would pass the instantiation
    /// allowance at the reader's frame depth.
    InstantiationDepth,
    /// A tail run of the closure took more steps than the reader's tail
    /// allowance admits.
    TailSteps,
    /// The closure's structured comparisons would pass what remains of the
    /// relation-complexity allowance of the check the reader is in (all of
    /// it, outside any check).
    RelationComparisons,
    /// The demand already tripped, or its request was cancelled.
    Tripped,
}

/// The immutable allowances a refusal was decided under: every operation
/// allowance a connected demand answers to, and the cost model those
/// allowances are counted in. Interned, so two demands under the same
/// allowances share one profile and compare by pointer; a refusal recorded
/// under one profile never answers a demand under another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BudgetProfileSpec {
    /// The connected demand's work-unit allowance.
    pub work: usize,
    /// The connected demand's construction-byte allowance.
    pub bytes: u64,
    /// The connected demand's nested query-depth allowance.
    pub query_depth: u16,
    /// The continuation runtime's instantiation-depth allowance.
    pub instantiation_depth: u32,
    /// A conditional tail run's step allowance.
    pub tail_steps: u32,
    /// The request's projection-operation allowance (the effective cap).
    pub request_operations: usize,
    /// The checker's relation-complexity allowance.
    pub relation_comparisons: u32,
    /// The cost-model revision the allowances are counted in.
    pub cost_model_revision: u32,
}

impl BudgetProfileSpec {
    /// The profile a certified divergence stores. Certified divergence
    /// ignores allowances, so this is not a budget a demand could have.
    #[must_use]
    pub const fn ignored() -> Self {
        Self {
            work: 0,
            bytes: 0,
            query_depth: 0,
            instantiation_depth: 0,
            tail_steps: 0,
            request_operations: 0,
            relation_comparisons: 0,
            cost_model_revision: 0,
        }
    }
}

/// An interned [`BudgetProfileSpec`]: one identity per spec for as long as
/// anything holds it. The interner keeps only weak references, and a
/// profile's last holder removes its entry, so the interner holds exactly
/// the live profiles — a profile no request, ledger or sealed refusal
/// names any more is reclaimed, and interning the same spec again mints a
/// fresh identity no stale entry was keyed by.
///
/// A profile is a self-charged allocation: its block is pinned against the
/// process retention account for exactly as long as it lives, and the
/// interner charges its own table by capacity, giving the table back as
/// profiles drain.
#[derive(Clone, Debug)]
pub struct BudgetProfile(Arc<InternedProfile>);

/// The interned allowances, removed from the interner when the last
/// profile naming them drops, and the charge of their own block.
#[derive(Debug)]
pub struct InternedProfile {
    spec: BudgetProfileSpec,
    _charge: verter_session_query::retention::RetentionCharge,
}

/// The live profiles and the pinned charge of the table holding them.
#[derive(Default)]
struct ProfileInterner {
    profiles: FxHashMap<BudgetProfileSpec, Weak<InternedProfile>>,
    backing: Option<verter_session_query::retention::granular::GranularPin>,
}

/// Granule the profile table's backing is charged in.
const PROFILE_TABLE_GRANULE: usize = 1024;

impl ProfileInterner {
    /// Settle the table's backing charge with its capacity.
    fn settle(&mut self) {
        let bytes = self.profiles.capacity()
            * (std::mem::size_of::<(BudgetProfileSpec, Weak<InternedProfile>)>() + 1);
        let backing = self.backing.get_or_insert_with(|| {
            verter_session_query::retention::granular::GranularPin::new(
                &verter_session_query::retention::SemanticRetentionAccount::process_local(),
                PROFILE_TABLE_GRANULE,
            )
        });
        if bytes > backing.used() {
            backing.add(bytes - backing.used());
        } else {
            backing.sub(backing.used() - bytes);
        }
    }
}

fn profile_interner() -> &'static Mutex<ProfileInterner> {
    static PROFILES: OnceLock<Mutex<ProfileInterner>> = OnceLock::new();
    PROFILES.get_or_init(Default::default)
}

impl BudgetProfile {
    /// The interned profile for `spec`.
    pub fn intern(spec: BudgetProfileSpec) -> Self {
        let mut interner = profile_interner().lock();
        if let Some(live) = interner.profiles.get(&spec).and_then(Weak::upgrade) {
            return Self(live);
        }
        let profile = Arc::new(InternedProfile {
            spec,
            _charge: verter_session_query::retention::SemanticRetentionAccount::process_local()
                .pin(
                    verter_session_query::retention::allocation::arc_block_bytes(
                        std::mem::size_of::<InternedProfile>(),
                    ),
                ),
        });
        interner.profiles.insert(spec, Arc::downgrade(&profile));
        interner.settle();
        Self(profile)
    }

    /// The allowances.
    pub fn spec(&self) -> &BudgetProfileSpec {
        &self.0.spec
    }

    /// Whether a live profile for `spec` is interned now.
    #[cfg(any(test, feature = "test-support"))]
    pub fn is_interned(spec: &BudgetProfileSpec) -> bool {
        profile_interner().lock().profiles.contains_key(spec)
    }

    /// Bytes this profile is charged.
    #[cfg(any(test, feature = "test-support"))]
    pub fn charge_bytes_for_tests(&self) -> usize {
        self.0._charge.bytes()
    }

    /// The profile table's bucket capacity and the bytes its backing is
    /// charged.
    #[cfg(any(test, feature = "test-support"))]
    pub fn interner_backing_for_tests() -> (usize, usize) {
        let interner = profile_interner().lock();
        (
            interner.profiles.capacity(),
            interner.backing.as_ref().map_or(
                0,
                verter_session_query::retention::granular::GranularPin::charged,
            ),
        )
    }
}

impl Drop for InternedProfile {
    fn drop(&mut self) {
        // The last holder is gone, so no profile can upgrade this entry; a
        // spec interned again since then holds a live entry of its own,
        // which stays.
        let mut interner = profile_interner().lock();
        if interner
            .profiles
            .get(&self.spec)
            .is_some_and(|entry| entry.strong_count() == 0)
        {
            interner.profiles.remove(&self.spec);
            let len = interner.profiles.len();
            if len == 0 {
                interner.profiles = FxHashMap::default();
            } else if len * 4 <= interner.profiles.capacity() {
                interner.profiles.shrink_to(len * 2);
            }
            interner.settle();
        }
    }
}

impl PartialEq for BudgetProfile {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for BudgetProfile {}

impl Hash for BudgetProfile {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.spec().hash(state);
    }
}

#[cfg(test)]
#[path = "cost_receipt_retention_tests.rs"]
mod cost_receipt_retention_tests;
