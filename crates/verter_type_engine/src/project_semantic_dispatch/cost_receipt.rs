//! Cost receipts: the exclusive cost of each semantic computation a
//! connected demand performs, kept as a shared DAG so that serving a result
//! warm charges exactly what computing it cold would have — each
//! computation once per connected demand, however many consumers share it.
//!
//! A receipt records only its OWN (exclusive) work units, construction
//! bytes and request operations, plus shared references to the receipts of
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
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use verter_execution::tasks::ProducerIdentity;

/// The revision of the logical cost model: what a work unit, a
/// construction byte and a request operation are charged for. A receipt is
/// meaningful only under the revision that recorded it, and a refusal only
/// under the [`BudgetProfile`] that refused it.
pub const COST_MODEL_REVISION: u32 = 1;

/// The exact identity of one semantic computation: the canonical demand
/// and the materialized point it answers, as the memo's prepared key
/// encodes them. Equality compares the whole identity, so two computations
/// never share a receipt by hash collision; the hash only speeds hashing
/// and the common inequality.
#[derive(Clone)]
pub struct CostIdentity {
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
            hash: key.producer_hash(),
            key,
        }
    }
}

impl CostIdentity {
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

/// A 64-bit FNV-1a digest of `bytes`.
fn digest_of(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}

/// The logical cost of work: its work units, its construction bytes and
/// the request operations (the per-request projection fuse) it spent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LogicalUsage {
    pub work: u64,
    pub bytes: u64,
    pub operations: u64,
}

impl LogicalUsage {
    /// The sum of two usages, `None` on overflow.
    pub(crate) fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            work: self.work.checked_add(other.work)?,
            bytes: self.bytes.checked_add(other.bytes)?,
            operations: self.operations.checked_add(other.operations)?,
        })
    }

    /// The sum of two usages, saturating.
    pub(crate) fn saturating_add(self, other: Self) -> Self {
        Self {
            work: self.work.saturating_add(other.work),
            bytes: self.bytes.saturating_add(other.bytes),
            operations: self.operations.saturating_add(other.operations),
        }
    }
}

/// One prerequisite a computation consumed: the prerequisite's receipt,
/// and the logical nesting at which it was consumed (0 for a result read
/// in place, 1 for one entered as a nested demand).
#[derive(Clone, Debug)]
pub struct CostDependency {
    pub receipt: Arc<DemandCostReceipt>,
    pub nesting: u16,
}

/// The sealed cost of one computation: its identity, its exclusive usage,
/// the receipts of the computations it consumed (each once), and the
/// deepest logical nesting it needs below its own entry.
#[derive(Debug)]
pub struct DemandCostReceipt {
    identity: CostIdentity,
    exclusive: LogicalUsage,
    prerequisites: Box<[CostDependency]>,
    depth: u16,
}

impl DemandCostReceipt {
    /// A receipt for `identity` with `exclusive` usage over
    /// `prerequisites`, deduplicated by identity (each keeping the deepest
    /// nesting it was consumed at). Its depth is the deepest prerequisite's
    /// depth plus the nesting that prerequisite was consumed at.
    pub fn new(
        identity: CostIdentity,
        exclusive: LogicalUsage,
        prerequisites: Vec<CostDependency>,
    ) -> Arc<Self> {
        let mut unique: Vec<CostDependency> = Vec::with_capacity(prerequisites.len());
        let mut positions: FxHashMap<CostIdentity, usize> = FxHashMap::default();
        for dependency in prerequisites {
            match positions.get(&dependency.receipt.identity) {
                Some(&position) => {
                    let kept = &mut unique[position];
                    kept.nesting = kept.nesting.max(dependency.nesting);
                }
                None => {
                    positions.insert(dependency.receipt.identity.clone(), unique.len());
                    unique.push(dependency);
                }
            }
        }
        let depth = unique
            .iter()
            .map(|dependency| dependency.receipt.depth.saturating_add(dependency.nesting))
            .max()
            .unwrap_or(0);
        Arc::new(Self {
            identity,
            exclusive,
            prerequisites: unique.into_boxed_slice(),
            depth,
        })
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
        self.depth
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

impl Drop for DemandCostReceipt {
    /// Release the prerequisite DAG iteratively: a receipt reaches the
    /// receipts of a chain as deep as the chain of computations it costs,
    /// and dropping them recursively would spend a native frame per level.
    fn drop(&mut self) {
        let mut pending: Vec<CostDependency> = std::mem::take(&mut self.prerequisites).into_vec();
        while let Some(dependency) = pending.pop() {
            if let Some(mut last) = Arc::into_inner(dependency.receipt) {
                pending.extend(std::mem::take(&mut last.prerequisites).into_vec());
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
/// while it is on top, and the prerequisites it consumed.
#[derive(Debug)]
pub struct CostScope {
    pub(super) identity: CostIdentity,
    pub(super) exclusive: LogicalUsage,
    pub(super) prerequisites: Vec<CostDependency>,
}

/// Why a replay was refused: the rail the unpaid closure would pass, with
/// nothing charged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayRefusal {
    /// The closure's work would pass the remaining work allowance.
    Work,
    /// The closure's construction bytes would pass the remaining byte
    /// allowance.
    Bytes,
    /// The closure's request operations would pass the request's remaining
    /// operation allowance.
    Operations,
    /// The receipt's nesting would pass the remaining query depth.
    Depth,
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
    pub bytes: usize,
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

/// An interned [`BudgetProfileSpec`].
#[derive(Clone, Debug)]
pub struct BudgetProfile(Arc<BudgetProfileSpec>);

impl BudgetProfile {
    /// The interned profile for `spec`.
    pub fn intern(spec: BudgetProfileSpec) -> Self {
        static PROFILES: OnceLock<Mutex<FxHashMap<BudgetProfileSpec, Arc<BudgetProfileSpec>>>> =
            OnceLock::new();
        let profiles = PROFILES.get_or_init(Default::default);
        let mut profiles = profiles.lock();
        Self(Arc::clone(
            profiles.entry(spec).or_insert_with(|| Arc::new(spec)),
        ))
    }

    /// The allowances.
    pub fn spec(&self) -> &BudgetProfileSpec {
        &self.0
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
        self.0.hash(state);
    }
}
