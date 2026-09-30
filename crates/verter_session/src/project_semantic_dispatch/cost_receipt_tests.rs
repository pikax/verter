//! The cost-receipt substrate: a connected demand records each cold
//! computation's exclusive cost, and serving a result warm charges the
//! unpaid part of its receipt's closure once — so a diamond of shared
//! results costs linearly, and the cost of a demand is the same cold, warm
//! or partly warm, in any order.

use std::sync::Arc;

use super::cost_receipt::{
    CostDelivery, CostIdentity, DemandCostReceipt, LogicalUsage, ReplayRefusal,
};
use super::ProjectSemanticDispatch;
use crate::{HostConfig, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn identity(name: &str) -> CostIdentity {
    CostIdentity::new(name.as_bytes().to_vec())
}

fn usage(work: u64, bytes: u64) -> LogicalUsage {
    LogicalUsage { work, bytes }
}

/// `T0 … Tn`, each `Tk` consuming `T(k-1)` twice, each costing one unit
/// and ten bytes of its own.
fn diamond(levels: usize) -> Vec<Arc<DemandCostReceipt>> {
    let mut receipts: Vec<Arc<DemandCostReceipt>> = Vec::with_capacity(levels + 1);
    for level in 0..=levels {
        let prerequisites = receipts
            .last()
            .map(|below| {
                vec![
                    super::cost_receipt::CostDependency {
                        receipt: Arc::clone(below),
                        nesting: 1,
                    },
                    super::cost_receipt::CostDependency {
                        receipt: Arc::clone(below),
                        nesting: 1,
                    },
                ]
            })
            .unwrap_or_default();
        receipts.push(DemandCostReceipt::new(
            identity(&format!("T{level}")),
            usage(1, 10),
            prerequisites,
        ));
    }
    receipts
}

/// Serving `T20` of a diamond charges each of its 21 distinct computations
/// once — 21 units and 210 bytes — never the 2^21 its expansion names.
#[test]
fn a_diamond_replays_in_linear_cost() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let receipts = diamond(20);
    let top = receipts.last().expect("a top");
    assert_eq!(
        top.prerequisites().len(),
        1,
        "a repeated prerequisite is kept once"
    );
    assert_eq!(
        top.depth(),
        20,
        "each level consumes the next as a nested demand"
    );
    let (_guard, _) = dispatch.enter_connected_demand(false);
    dispatch
        .connected_demand
        .replay_admit(top)
        .expect("21 units fit");
    assert_eq!(dispatch.connected_demand.work_used_for_tests(), 21);
    assert_eq!(dispatch.connected_demand.bytes_used_for_tests(), 210);
    for receipt in &receipts {
        assert!(dispatch.connected_demand.is_paid(receipt.identity()));
    }
    dispatch
        .connected_demand
        .replay_admit(top)
        .expect("a paid closure costs nothing more");
    assert_eq!(dispatch.connected_demand.work_used_for_tests(), 21);
}

/// A partly warm demand pays only what it has not paid, and the total is
/// the cold total: serving `T10` first, then `T20`, charges 11 and then 10
/// units — 21, as serving `T20` alone does.
#[test]
fn a_partly_paid_diamond_charges_the_rest_once() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let receipts = diamond(20);
    let (_guard, _) = dispatch.enter_connected_demand(false);
    dispatch
        .connected_demand
        .replay_admit(&receipts[10])
        .expect("11 units fit");
    assert_eq!(dispatch.connected_demand.work_used_for_tests(), 11);
    dispatch
        .connected_demand
        .replay_admit(&receipts[20])
        .expect("10 more units fit");
    assert_eq!(dispatch.connected_demand.work_used_for_tests(), 21);
}

/// Recording a diamond cold gives each receipt only its own cost: `T2`
/// computing `T1` once and reading it again paid, and `T1` doing the same
/// with `T0`, seal to receipts whose replay on a fresh demand charges what
/// the cold run charged.
#[test]
fn a_cold_diamond_records_exclusive_costs() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let ledger = &dispatch.connected_demand;
    let (top, cold_work) = {
        let (_guard, _) = dispatch.enter_connected_demand(false);
        ledger.open_cost_scope(identity("T2"));
        ledger.charge().expect("T2's own unit");
        ledger.open_cost_scope(identity("T1"));
        ledger.charge_units(2).expect("T1's own units");
        ledger.open_cost_scope(identity("T0"));
        ledger.charge_units(3).expect("T0's own units");
        ledger.reserve_bytes(30).expect("T0's own bytes");
        let t0 = ledger.seal_cost_scope().expect("T0 seals");
        ledger.record_prerequisite(&t0, 1);
        assert!(
            ledger.is_paid(t0.identity()),
            "a sealed computation is paid"
        );
        // The second read of T0 finds it paid and records it again.
        ledger.record_prerequisite(&t0, 1);
        let t1 = ledger.seal_cost_scope().expect("T1 seals");
        ledger.record_prerequisite(&t1, 1);
        ledger.record_prerequisite(&t1, 1);
        let t2 = ledger.seal_cost_scope().expect("T2 seals");
        assert_eq!(t0.exclusive(), usage(3, 30));
        assert_eq!(t1.exclusive(), usage(2, 0));
        assert_eq!(t2.exclusive(), usage(1, 0));
        assert_eq!(t2.depth(), 2);
        (t2, ledger.work_used_for_tests())
    };
    assert_eq!(cold_work, 6);
    let (_guard, _) = dispatch.enter_connected_demand(false);
    assert!(
        !ledger.is_paid(top.identity()),
        "a new connected demand has paid nothing"
    );
    ledger.replay_admit(&top).expect("six units fit");
    assert_eq!(
        ledger.work_used_for_tests(),
        cold_work,
        "served warm, the demand costs what it cost cold"
    );
    assert_eq!(ledger.bytes_used_for_tests(), 30);
}

/// A replay the remaining allowance cannot pay is refused whole: nothing is
/// charged and nothing is marked paid, so a later demand under a larger
/// allowance pays it in full.
#[test]
fn a_refused_replay_charges_and_marks_nothing() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let receipts = diamond(20);
    dispatch.set_connected_limits_for_tests(10, 24);
    let (_guard, _) = dispatch.enter_connected_demand(false);
    assert_eq!(
        dispatch.connected_demand.replay_admit(&receipts[20]),
        Err(ReplayRefusal::Work)
    );
    assert_eq!(dispatch.connected_demand.work_used_for_tests(), 0);
    assert!(receipts
        .iter()
        .all(|receipt| !dispatch.connected_demand.is_paid(receipt.identity())));
    dispatch
        .connected_demand
        .replay_admit(&receipts[9])
        .expect("ten units fit exactly");
    assert_eq!(dispatch.connected_demand.work_used_for_tests(), 10);
}

/// A receipt's nesting is checked against the remaining query depth even
/// when its cost is paid: a result first used shallowly is not free to use
/// under an arbitrarily deep consumer.
#[test]
fn a_paid_receipt_still_answers_to_the_depth_rail() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let receipts = diamond(20);
    dispatch.set_connected_limits_for_tests(1_000, 10);
    let (_guard, _) = dispatch.enter_connected_demand(false);
    assert_eq!(
        dispatch.connected_demand.replay_admit(&receipts[20]),
        Err(ReplayRefusal::Depth),
        "a depth-20 receipt does not fit a depth-10 rail"
    );
    dispatch
        .connected_demand
        .replay_admit(&receipts[10])
        .expect("depth 10 fits");
    assert_eq!(
        dispatch.connected_demand.replay_admit(&receipts[20]),
        Err(ReplayRefusal::Depth),
        "paying part of the closure does not waive the depth"
    );
}

/// Identities compare by their whole encoding.
#[test]
fn identities_compare_by_their_encoding() {
    assert_eq!(identity("T1"), identity("T1"));
    assert_ne!(identity("T1"), identity("T2"));
}

/// Distinct results sharing their descendants: at each of 20 levels two
/// results `Ak` and `Bk` each consume both `A(k-1)` and `B(k-1)`, and `C`
/// consumes `A20` and `B20`. Serving `C` charges each of its 43 distinct
/// computations once, never once per path to it.
#[test]
fn a_lattice_of_shared_results_replays_each_once() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let dependency = |receipt: &Arc<DemandCostReceipt>| super::cost_receipt::CostDependency {
        receipt: Arc::clone(receipt),
        nesting: 0,
    };
    let mut level: Vec<Arc<DemandCostReceipt>> = ["A0", "B0"]
        .into_iter()
        .map(|name| DemandCostReceipt::new(identity(name), usage(1, 0), Vec::new()))
        .collect();
    for k in 1..=20 {
        level = ["A", "B"]
            .into_iter()
            .map(|side| {
                DemandCostReceipt::new(
                    identity(&format!("{side}{k}")),
                    usage(1, 0),
                    level.iter().map(dependency).collect(),
                )
            })
            .collect();
    }
    let top = DemandCostReceipt::new(
        identity("C"),
        usage(1, 0),
        level.iter().map(dependency).collect(),
    );
    let (_guard, _) = dispatch.enter_connected_demand(false);
    dispatch
        .connected_demand
        .replay_admit(&top)
        .expect("43 units fit");
    assert_eq!(dispatch.connected_demand.work_used_for_tests(), 43);
}

/// A computation that does not complete leaves no receipt and is never
/// paid: its charges stay spent, and nothing about it is served warm.
#[test]
fn an_abandoned_computation_is_never_paid() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let ledger = &dispatch.connected_demand;
    let (_guard, _) = dispatch.enter_connected_demand(false);
    ledger.open_cost_scope(identity("X"));
    ledger.charge_units(5).expect("X's own units");
    ledger.abandon_cost_scope();
    assert!(!ledger.is_paid(&identity("X")));
    assert!(
        ledger.seal_cost_scope().is_none(),
        "nothing is left recording"
    );
    assert_eq!(ledger.work_used_for_tests(), 5, "the work stays spent");
}

/// The rule a consumer follows for a delivered result: a result computed
/// for this demand is recorded without being charged again; a result
/// served from a store is admitted by replaying its receipt, then
/// recorded. Either way the consumer's receipt owes it. A refused replay is
/// plain resource incompleteness for this consumer.
fn consume(
    ledger: &super::connected_demand::ConnectedDemandLedger<'_>,
    delivery: &CostDelivery,
) -> Result<(), ReplayRefusal> {
    let receipt = match delivery {
        CostDelivery::Fresh(receipt) => receipt,
        CostDelivery::Replayed(receipt) => {
            ledger.replay_admit(receipt)?;
            receipt
        }
    };
    ledger.record_prerequisite(receipt, 1);
    Ok(())
}

/// A result computed for this demand is not charged twice when delivered to
/// its consumer, and a result replayed from a store costs what computing it
/// did: the consumer's total is the same either way.
#[test]
fn a_fresh_delivery_is_not_charged_twice() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let ledger = &dispatch.connected_demand;
    let child = {
        let (_guard, _) = dispatch.enter_connected_demand(false);
        ledger.open_cost_scope(identity("P"));
        ledger.open_cost_scope(identity("C"));
        ledger.charge_units(4).expect("C's own units");
        let child = ledger.seal_cost_scope().expect("C seals");
        consume(ledger, &CostDelivery::Fresh(Arc::clone(&child))).expect("fresh is free");
        ledger.charge().expect("P's own unit");
        let parent = ledger.seal_cost_scope().expect("P seals");
        assert_eq!(ledger.work_used_for_tests(), 5, "C is paid once");
        assert_eq!(parent.exclusive(), usage(1, 0));
        child
    };
    let (_guard, _) = dispatch.enter_connected_demand(false);
    ledger.open_cost_scope(identity("P"));
    consume(ledger, &CostDelivery::Replayed(child)).expect("four units fit");
    ledger.charge().expect("P's own unit");
    let parent = ledger.seal_cost_scope().expect("P seals");
    assert_eq!(
        ledger.work_used_for_tests(),
        5,
        "served warm, C costs what it cost cold"
    );
    assert_eq!(
        parent.exclusive(),
        usage(1, 0),
        "a replay is not the consumer's own cost"
    );
}

/// A computation consuming 20,000 results, each twice, keeps each once:
/// deduplication is by identity, not by scanning what is kept.
#[test]
fn a_wide_fan_out_keeps_each_prerequisite_once() {
    let leaves: Vec<Arc<DemandCostReceipt>> = (0..20_000)
        .map(|i| DemandCostReceipt::new(identity(&format!("L{i}")), usage(1, 0), Vec::new()))
        .collect();
    let prerequisites = leaves
        .iter()
        .chain(leaves.iter())
        .map(|leaf| super::cost_receipt::CostDependency {
            receipt: Arc::clone(leaf),
            nesting: 0,
        })
        .collect();
    let wide = DemandCostReceipt::new(identity("W"), usage(1, 0), prerequisites);
    assert_eq!(wide.prerequisites().len(), 20_000);
}
