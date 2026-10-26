//! What a receipt reverse index keeps allocated, against what its account
//! and its candidates' owners are charged for it.
//!
//! The allocator counts every block the index holds by its real layout,
//! costed by the retention account's declared heap-block convention, so the
//! figure is independent of how the index estimates its own storage. At
//! every stage — full, partially drained, churned — the index's charge plus
//! the reservations its candidates' owners make must cover that figure, must
//! not exceed it by more than the charge granule's rounding, and must reach
//! zero with the last candidate, a clear or a drop.
//!
//! Fixtures are built before the count starts and live until it ends, so
//! the count holds only the index's own allocations.

use std::sync::Arc;

use verter_session_query::facts::fact_cache::{FactVersionRef, ResultReceipt};
use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount};
use verter_type_engine::semantic_query_memo::ReceiptReverseIndex;

use super::{live_block_bytes, reset_live_block_bytes};

type Index = ReceiptReverseIndex<u32, ()>;

/// The index's charge settles in whole 4 KiB granules: never below what it
/// holds and less than two granules above.
const GRANULE_SLACK: i64 = 2 * 4096;

fn file(canonical: &str) -> FactVersionRef {
    FactVersionRef::FileWholeHash {
        canonical_id: canonical.into(),
        hash: [3; 16],
    }
}

fn carrier(receipt: &ResultReceipt) -> verter_session_query::facts::fact_set::FactSet {
    verter_session_query::facts::fact_set::FactSet::from(vec![FactVersionRef::Receipt(
        receipt.clone(),
    )])
}

/// `n` results, each reading its own file and consuming the one before.
fn chain(n: usize) -> Vec<ResultReceipt> {
    let mut receipts: Vec<ResultReceipt> = Vec::with_capacity(n);
    for i in 0..n {
        let mut facts = vec![file(&format!("/w/chain{i}.ts"))];
        if let Some(below) = receipts.last() {
            facts.push(FactVersionRef::Receipt(below.clone()));
        }
        receipts.push(ResultReceipt::new(facts));
    }
    receipts
}

/// One index under measurement, and what its candidates' owners reserve
/// for their own registrations when the index leaves that share to them.
struct Measured {
    account: Arc<SemanticRetentionAccount>,
    index: Index,
    owner_charges_candidates: bool,
    owner_reserved: i64,
}

impl Measured {
    /// Built before the count starts: the index itself is inline. A
    /// throwaway index on the same account runs one full registration
    /// cycle first, so whatever that path sets up once per thread is
    /// already allocated: it belongs to no index.
    fn new(owner_charges_candidates: bool) -> Self {
        let account = SemanticRetentionAccount::new(RetentionLimits::defaults());
        let warm = ReceiptReverseIndex::<u32, ()>::charging_registrations(&account);
        warm.register(
            0,
            (),
            &verter_session_query::facts::fact_set::FactSet::empty(),
            ["/w/warm.ts"],
        );
        assert_eq!(warm.unregister(&0), Some(()));
        drop(warm);
        let index = if owner_charges_candidates {
            Index::charged(&account)
        } else {
            Index::charging_registrations(&account)
        };
        Self {
            account,
            index,
            owner_charges_candidates,
            owner_reserved: 0,
        }
    }

    fn owner_share(&self, extras: usize, evidence: usize) -> i64 {
        if self.owner_charges_candidates {
            Index::candidate_bytes(extras, evidence) as i64
        } else {
            0
        }
    }

    fn register(
        &mut self,
        key: u32,
        carrier: &verter_session_query::facts::fact_set::FactSet,
        extras: &[&str],
    ) {
        self.index
            .register(key, (), carrier, extras.iter().copied());
        self.owner_reserved += self.owner_share(extras.len(), usize::from(!carrier.is_empty()));
    }

    fn unregister(&mut self, key: u32, extras: usize, carrier_has_facts: bool) {
        assert_eq!(self.index.unregister(&key), Some(()));
        self.owner_reserved -= self.owner_share(extras, usize::from(carrier_has_facts));
    }

    /// The index's charge and its owners' reservations cover every block
    /// it holds, by less than the granule's rounding above, and the
    /// account carries exactly the index's charge.
    fn check(&self, stage: &str, at: u32) {
        let held = live_block_bytes();
        let charge = self.index.storage_charge_bytes() as i64;
        let charged = charge + self.owner_reserved;
        assert!(
            charged >= held,
            "{stage} {at}: the index holds {held} bytes of blocks but {charge} are charged to its \
             account and {} reserved by its candidates' owners",
            self.owner_reserved
        );
        assert!(
            charged < held + GRANULE_SLACK,
            "{stage} {at}: {charged} bytes charged and reserved for {held} bytes of blocks held"
        );
        assert_eq!(
            self.account.snapshot().pinned_bytes as i64,
            charge,
            "{stage} {at}: the account carries the index's charge"
        );
    }

    /// Nothing held, nothing charged, nothing reserved.
    fn check_drained(&self, stage: &str, at: u32) {
        assert_eq!(live_block_bytes(), 0, "{stage} {at}: blocks left allocated");
        assert_eq!(
            self.index.storage_charge_bytes(),
            0,
            "{stage} {at}: charge left"
        );
        assert_eq!(
            self.owner_reserved, 0,
            "{stage} {at}: owner reservation left"
        );
        assert_eq!(
            self.account.snapshot().pinned_bytes,
            0,
            "{stage} {at}: account"
        );
    }
}

/// Thousands of candidates each naming its own extra canonical: every one
/// holds a one-element extras list and a one-element holder set, the
/// smallest allocations the index makes.
#[test]
fn reverse_index_singleton_extras_and_holder_sets_are_charged_by_their_blocks() {
    const N: u32 = 4096;
    for owner_charges_candidates in [false, true] {
        let names: Vec<String> = (0..N).map(|i| format!("/w/extra{i}.ts")).collect();
        let empty = verter_session_query::facts::fact_set::FactSet::empty();
        let mut measured = Measured::new(owner_charges_candidates);
        reset_live_block_bytes();
        for (key, name) in names.iter().enumerate() {
            measured.register(key as u32, &empty, &[name.as_str()]);
        }
        measured.check("full", 0);
        for key in 0..N / 2 {
            measured.unregister(key, 1, false);
        }
        measured.check("half drained", 0);
        for key in N / 2..N - 1 {
            measured.unregister(key, 1, false);
        }
        measured.check("one left", 0);
        measured.unregister(N - 1, 1, false);
        measured.check_drained("drained", 0);
        drop((names, empty));
    }
}

/// Thousands of candidates each over its own receipt: every evidence node
/// holds a one-element parent set.
#[test]
fn reverse_index_singleton_parent_sets_are_charged_by_their_blocks() {
    const N: u32 = 2048;
    for owner_charges_candidates in [false, true] {
        let carriers: Vec<verter_session_query::facts::fact_set::FactSet> = (0..N)
            .map(|i| carrier(&ResultReceipt::new(vec![file(&format!("/w/own{i}.ts"))])))
            .collect();
        let mut measured = Measured::new(owner_charges_candidates);
        reset_live_block_bytes();
        for (key, carrier) in carriers.iter().enumerate() {
            measured.register(key as u32, carrier, &[]);
        }
        measured.check("full", 0);
        for key in (0..N).step_by(3) {
            measured.unregister(key, 0, true);
        }
        measured.check("a third drained", 0);
        for key in (0..N).filter(|key| key % 3 != 0) {
            measured.unregister(key, 0, true);
        }
        measured.check_drained("drained", 0);
        drop(carriers);
    }
}

/// Candidates naming one of two shared canonicals in a scrambled order,
/// drained in sequence: removals leave tombstones in the shared holder
/// sets, which keep their blocks until rebuilt or freed.
#[test]
fn reverse_index_drain_through_tombstones_stays_covered_and_releases_everything() {
    const N: u32 = 3500;
    for owner_charges_candidates in [false, true] {
        let empty = verter_session_query::facts::fact_set::FactSet::empty();
        let mut random = 0x1234_5678_u64;
        let fences: Vec<&str> = (0..N)
            .map(|_| {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                if random & 1 == 0 {
                    "/w/shared.ts"
                } else {
                    "/w/other.ts"
                }
            })
            .collect();
        let mut measured = Measured::new(owner_charges_candidates);
        reset_live_block_bytes();
        for (key, fence) in fences.iter().enumerate() {
            measured.register(key as u32, &empty, &[fence]);
        }
        measured.check("full", 0);
        for key in 0..N {
            measured.unregister(key, 1, false);
            if key % 500 == 0 {
                measured.check("after", key);
            }
        }
        measured.check_drained("drained", 0);
        drop((fences, empty));
    }
}

/// Rounds of registration and release over one shared chain accumulate
/// no charge and hold no block between rounds.
#[test]
fn reverse_index_repeated_churn_accumulates_no_phantom_charge() {
    const ROUNDS: u32 = 12;
    const PER_ROUND: u32 = 600;
    for owner_charges_candidates in [false, true] {
        let receipts = chain(40);
        let carriers: Vec<verter_session_query::facts::fact_set::FactSet> =
            receipts.iter().map(carrier).collect();
        let names: Vec<String> = (0..PER_ROUND).map(|i| format!("/w/self{i}.ts")).collect();
        let mut measured = Measured::new(owner_charges_candidates);
        reset_live_block_bytes();
        for round in 0..ROUNDS {
            for key in 0..PER_ROUND {
                let carrier = &carriers[((key + round) as usize * 7) % carriers.len()];
                let extras: &[&str] = if key % 4 == 0 {
                    &[names[key as usize].as_str()]
                } else {
                    &[]
                };
                measured.register(key, carrier, extras);
            }
            measured.check("full in round", round);
            for key in (0..PER_ROUND).rev() {
                measured.unregister(key, usize::from(key % 4 == 0), true);
                if key == PER_ROUND / 2 {
                    measured.check("half in round", round);
                }
            }
            measured.check_drained("drained in round", round);
        }
        drop((receipts, carriers, names));
    }
}

/// A clear and a drop give back every block and every charged byte.
#[test]
fn reverse_index_clear_and_drop_restore_the_account_baseline() {
    let receipts = chain(64);
    let carriers: Vec<verter_session_query::facts::fact_set::FactSet> =
        receipts.iter().map(carrier).collect();
    let names: Vec<String> = (0..256).map(|i| format!("/w/kept{i}.ts")).collect();

    let measured = Measured::new(false);
    reset_live_block_bytes();
    for (key, name) in names.iter().enumerate() {
        measured.index.register(
            key as u32,
            (),
            &carriers[key % carriers.len()],
            [name.as_str()],
        );
    }
    measured.check("full", 0);
    measured.index.clear();
    measured.check_drained("cleared", 0);

    for (key, name) in names.iter().enumerate() {
        measured.index.register(
            key as u32,
            (),
            &carriers[key % carriers.len()],
            [name.as_str()],
        );
    }
    measured.check("refilled", 0);
    let Measured { account, index, .. } = measured;
    drop(index);
    assert_eq!(live_block_bytes(), 0, "a dropped index frees every block");
    assert_eq!(
        account.snapshot().pinned_bytes,
        0,
        "a dropped index releases its charge"
    );
    drop((receipts, carriers, names));
}
