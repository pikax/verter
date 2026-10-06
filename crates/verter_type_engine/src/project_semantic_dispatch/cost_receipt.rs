//! Cost receipts: the exclusive cost of each semantic computation a
//! connected demand performs, kept as a shared DAG so that serving a result
//! warm charges exactly what computing it cold would have — each
//! computation once per connected demand, however many consumers share it.
//!
//! A receipt records only its OWN (exclusive) work units and construction
//! bytes, plus shared references to the receipts of the computations it
//! consumed. The cost of demanding a result is the sum of the exclusive
//! costs over the DISTINCT computations its receipt reaches: a set sum, so
//! neither sharing, warmth nor the order siblings are visited in can change
//! it, and a diamond of shared sub-results costs linearly, not
//! exponentially.
//!
//! The ledger owns the recording: a cold computation opens a
//! [`CostScope`], every charge the ledger takes while the scope is on top
//! accrues to it, each consumed result is recorded as a prerequisite, and
//! sealing the scope yields the receipt and marks it paid for the demand.
//! Serving a result warm ([`ConnectedDemandLedger::replay_admit`]) charges
//! the unpaid part of its receipt's closure in one admission, or nothing.
//!
//! [`ConnectedDemandLedger::replay_admit`]: super::connected_demand::ConnectedDemandLedger::replay_admit

// The memo and continuation carriers consume this substrate; until they are
// threaded, only its own tests do.
#![cfg_attr(not(test), allow(dead_code))]

use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// The exact identity of one semantic computation: the canonical demand,
/// the materialized point it answers, its input version and the cost-model
/// revision, as the carrier encodes them. Equality compares the whole
/// encoding, so two computations never share a receipt by hash collision;
/// the digest only speeds hashing and the common inequality.
#[derive(Clone, Debug)]
pub struct CostIdentity {
    digest: u128,
    key: Arc<[u8]>,
}

impl CostIdentity {
    /// The identity of the computation `key` encodes exactly.
    pub fn new(key: impl Into<Arc<[u8]>>) -> Self {
        let key: Arc<[u8]> = key.into();
        Self {
            digest: digest_of(&key),
            key,
        }
    }
}

impl PartialEq for CostIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.digest == other.digest && self.key == other.key
    }
}

impl Eq for CostIdentity {}

impl Hash for CostIdentity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.digest.hash(state);
    }
}

/// A 128-bit FNV-1a digest of `bytes`.
fn digest_of(bytes: &[u8]) -> u128 {
    const OFFSET: u128 = 0x6c62272e07bb014262b821756295c58d;
    const PRIME: u128 = 0x0000000001000000000000000000013b;
    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u128::from(*byte)).wrapping_mul(PRIME)
    })
}

/// The logical cost of work: its work units and its construction bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LogicalUsage {
    pub work: u64,
    pub bytes: u64,
}

impl LogicalUsage {
    /// The sum of two usages, `None` on overflow.
    pub(crate) fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            work: self.work.checked_add(other.work)?,
            bytes: self.bytes.checked_add(other.bytes)?,
        })
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
        let mut positions: rustc_hash::FxHashMap<CostIdentity, usize> =
            rustc_hash::FxHashMap::default();
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
}

/// How a result reached its consumer: computed for this connected demand
/// (its cost already charged as it ran), or served from a store and to be
/// charged by replay. A result with no receipt is neither, and is
/// recomputed rather than served free.
#[derive(Clone, Debug)]
pub enum CostDelivery {
    Fresh(Arc<DemandCostReceipt>),
    Replayed(Arc<DemandCostReceipt>),
}

/// An open cold computation's recording: its identity, the usage accrued
/// while it is on top, and the prerequisites it consumed.
#[derive(Debug)]
pub(super) struct CostScope {
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
    /// The receipt's nesting would pass the remaining query depth.
    Depth,
    /// The demand already tripped, or its request was cancelled.
    Tripped,
}
