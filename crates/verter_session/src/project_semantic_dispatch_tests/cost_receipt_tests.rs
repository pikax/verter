//! The cost-receipt substrate: a connected demand records each cold
//! computation's exclusive cost, and serving a result warm charges the
//! unpaid part of its receipt's closure once — so a diamond of shared
//! results costs linearly, and the cost of a demand is the same cold, warm
//! or partly warm, in any order.

use std::sync::Arc;

use super::cost_receipt::{
    BudgetProfile, BudgetProfileSpec, CostIdentity, DemandCostReceipt, LogicalUsage, Nesting,
    ReplayRefusal, COST_MODEL_REVISION,
};
use super::ProjectSemanticDispatch;
use crate::{HostConfig, VerterHost};
use verter_type_engine::request_budget::RequestBudget;

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn identity(name: &str) -> CostIdentity {
    CostIdentity::new(name.as_bytes().to_vec())
}

fn usage(work: u64, bytes: u64) -> LogicalUsage {
    LogicalUsage {
        work,
        bytes,
        ..LogicalUsage::default()
    }
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
                        nesting: Nesting::Entered,
                    },
                    super::cost_receipt::CostDependency {
                        receipt: Arc::clone(below),
                        nesting: Nesting::Entered,
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
        .connected_demand()
        .replay_admit(top, None, Nesting::Entered)
        .expect("21 units fit");
    assert_eq!(dispatch.connected_demand().work_used_for_tests(), 21);
    assert_eq!(dispatch.connected_demand().bytes_used_for_tests(), 210);
    for receipt in &receipts {
        assert!(dispatch.connected_demand().is_paid(receipt.identity()));
    }
    dispatch
        .connected_demand()
        .replay_admit(top, None, Nesting::Entered)
        .expect("a paid closure costs nothing more");
    assert_eq!(dispatch.connected_demand().work_used_for_tests(), 21);
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
        .connected_demand()
        .replay_admit(&receipts[10], None, Nesting::Entered)
        .expect("11 units fit");
    assert_eq!(dispatch.connected_demand().work_used_for_tests(), 11);
    dispatch
        .connected_demand()
        .replay_admit(&receipts[20], None, Nesting::Entered)
        .expect("10 more units fit");
    assert_eq!(dispatch.connected_demand().work_used_for_tests(), 21);
}

/// Recording a diamond cold gives each receipt only its own cost: `T2`
/// computing `T1` once and reading it again paid, and `T1` doing the same
/// with `T0`, seal to receipts whose replay on a fresh demand charges what
/// the cold run charged.
#[test]
fn a_cold_diamond_records_exclusive_costs() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let ledger = &dispatch.connected_demand();
    let (top, cold_work) = {
        let (_guard, _) = dispatch.enter_connected_demand(false);
        ledger.open_cost_scope(identity("T2"));
        ledger.charge().expect("T2's own unit");
        ledger.open_cost_scope(identity("T1"));
        ledger.charge_units(2).expect("T1's own units");
        ledger.open_cost_scope(identity("T0"));
        ledger.charge_units(3).expect("T0's own units");
        ledger.reserve_bytes(30).expect("T0's own bytes");
        let t0 = ledger.seal_cost_scope(None).expect("T0 seals");
        ledger.record_prerequisite(&t0, Nesting::Entered);
        assert!(
            ledger.is_paid(t0.identity()),
            "a sealed computation is paid"
        );
        // The second read of T0 finds it paid and records it again.
        ledger.record_prerequisite(&t0, Nesting::Entered);
        let t1 = ledger.seal_cost_scope(None).expect("T1 seals");
        ledger.record_prerequisite(&t1, Nesting::Entered);
        ledger.record_prerequisite(&t1, Nesting::Entered);
        let t2 = ledger.seal_cost_scope(None).expect("T2 seals");
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
    ledger
        .replay_admit(&top, None, Nesting::Entered)
        .expect("six units fit");
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
        dispatch
            .connected_demand()
            .replay_admit(&receipts[20], None, Nesting::Entered),
        Err(ReplayRefusal::Work)
    );
    assert_eq!(dispatch.connected_demand().work_used_for_tests(), 0);
    assert!(receipts
        .iter()
        .all(|receipt| !dispatch.connected_demand().is_paid(receipt.identity())));
    dispatch
        .connected_demand()
        .replay_admit(&receipts[9], None, Nesting::Entered)
        .expect("ten units fit exactly");
    assert_eq!(dispatch.connected_demand().work_used_for_tests(), 10);
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
        dispatch
            .connected_demand()
            .replay_admit(&receipts[20], None, Nesting::Entered),
        Err(ReplayRefusal::Depth),
        "a depth-20 receipt does not fit a depth-10 rail"
    );
    dispatch
        .connected_demand()
        .replay_admit(&receipts[10], None, Nesting::Entered)
        .expect("depth 10 fits");
    {
        let (_nested, trip) = dispatch.enter_connected_demand(true);
        assert!(trip.is_none(), "the nested consumer itself fits");
        assert_eq!(
            dispatch
                .connected_demand()
                .replay_admit(&receipts[10], None, Nesting::Entered),
            Err(ReplayRefusal::Depth),
            "a fully paid root still needs its footprint at the new entry depth"
        );
    }
    assert_eq!(
        dispatch
            .connected_demand()
            .replay_admit(&receipts[20], None, Nesting::Entered),
        Err(ReplayRefusal::Depth),
        "paying part of the closure does not waive the depth"
    );
}

/// Construction bytes are charged on replay but never refuse it: a
/// complete result is not rejected at handoff for the bytes it took to
/// build, so serving it never turns a complete answer into a memory stop.
#[test]
fn a_replay_charges_bytes_but_never_refuses_on_them() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let receipts = diamond(20);
    dispatch.connected_demand().set_byte_limit_for_tests(50);
    let (_guard, _) = dispatch.enter_connected_demand(false);
    dispatch
        .connected_demand()
        .replay_admit(&receipts[20], None, Nesting::Entered)
        .expect("210 bytes past a 50-byte allowance still serve");
    assert_eq!(dispatch.connected_demand().bytes_used_for_tests(), 210);
    assert_eq!(dispatch.connected_demand().work_used_for_tests(), 21);
}

/// A receipt names the operation allowances its cold run needed, and a
/// replay answers to the reader's: its instantiation frames against the
/// instantiation budget at the reader's frame depth, and its tail runs
/// against the tail budget — even when its cost is already paid.
#[test]
fn a_replay_answers_to_the_operation_footprint_its_cold_run_needed() {
    let host = host();
    // Three instantiations, each needed by the frame above it.
    let leaf = DemandCostReceipt::new(identity("I0"), usage(1, 0), Vec::new());
    let middle = DemandCostReceipt::new(
        identity("I1"),
        usage(1, 0),
        vec![super::cost_receipt::CostDependency {
            receipt: Arc::clone(&leaf),
            nesting: Nesting::Frame { depth: 2 },
        }],
    );
    let top = DemandCostReceipt::new(
        identity("I2"),
        usage(1, 0),
        vec![super::cost_receipt::CostDependency {
            receipt: Arc::clone(&middle),
            nesting: Nesting::Frame { depth: 2 },
        }],
    );
    assert_eq!(top.footprint().instantiation_height, 2);
    {
        let _budget = super::connected_demand::InstantiationBudgetForTests::install(4);
        let dispatch = ProjectSemanticDispatch::new(&host);
        let (_guard, _) = dispatch.enter_connected_demand(false);
        let ledger = dispatch.connected_demand();
        ledger
            .replay_admit(&top, None, Nesting::Frame { depth: 2 })
            .expect("frames 2, 3 and 4 fit a budget of 4");
        assert_eq!(
            ledger.replay_admit(&top, None, Nesting::Frame { depth: 3 }),
            Err(ReplayRefusal::InstantiationDepth),
            "one frame deeper its cold run passes the budget, paid or not"
        );
        assert_eq!(
            ledger.replay_admit(&top, None, Nesting::Entered),
            Ok(()),
            "a synchronous read runs no frames"
        );
    }

    let tail = DemandCostReceipt::with_tail_steps(identity("T"), usage(1, 0), Vec::new(), 5);
    let consumer = DemandCostReceipt::new(
        identity("C"),
        usage(1, 0),
        vec![super::cost_receipt::CostDependency {
            receipt: Arc::clone(&tail),
            nesting: Nesting::Entered,
        }],
    );
    for (budget, admitted) in [(5, false), (6, true)] {
        let _budget = super::connected_demand::TailBudgetForTests::install(budget);
        let dispatch = ProjectSemanticDispatch::new(&host);
        let (_guard, _) = dispatch.enter_connected_demand(false);
        assert_eq!(
            dispatch
                .connected_demand()
                .replay_admit(&consumer, None, Nesting::Entered)
                .is_ok(),
            admitted,
            "a run of 5 tail steps under a tail budget of {budget}"
        );
    }
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
        nesting: Nesting::InPlace,
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
        .connected_demand()
        .replay_admit(&top, None, Nesting::Entered)
        .expect("43 units fit");
    assert_eq!(dispatch.connected_demand().work_used_for_tests(), 43);
}

/// A computation that does not complete leaves no receipt and is never
/// paid: its charges stay spent, and are its consumer's own cost — a
/// consumer's receipt owes what it spent on a computation it then had to
/// do without, so serving the consumer warm charges what its cold run did.
#[test]
fn an_abandoned_computation_is_its_consumers_cost() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let ledger = &dispatch.connected_demand();
    let (_guard, _) = dispatch.enter_connected_demand(false);
    ledger.open_cost_scope(identity("P"));
    ledger.open_cost_scope(identity("X"));
    ledger.charge_units(5).expect("X's own units");
    ledger.abandon_cost_scope();
    assert!(!ledger.is_paid(&identity("X")));
    ledger.charge().expect("P's own unit");
    let parent = ledger.seal_cost_scope(None).expect("P seals");
    assert_eq!(ledger.work_used_for_tests(), 6, "the work stays spent");
    assert_eq!(
        parent.exclusive(),
        usage(6, 0),
        "the abandoned computation's charges are the consumer's"
    );
    assert!(
        ledger.seal_cost_scope(None).is_none(),
        "nothing is left recording"
    );
}

/// A chain of computations that each fail — every level consuming one
/// result, then abandoned into the level below — folds what each level
/// consumed into its consumer as one failed-prefix receipt per level, not
/// as every prerequisite the levels above already folded: the surviving
/// consumer's receipt names a bounded list. It still owes everything the
/// failed levels spent and consumed, and its footprint is what the levels
/// needed: a result consumed by a frame below any of them keeps its frame.
#[test]
fn failed_computations_fold_one_prefix_per_level() {
    const LEVELS: usize = 64;
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let ledger = &dispatch.connected_demand();
    let (_guard, _) = dispatch.enter_connected_demand(false);
    ledger.open_cost_scope(identity("root"));
    for level in 0..LEVELS {
        ledger.open_cost_scope(identity(&format!("L{level}")));
    }
    for level in (0..LEVELS).rev() {
        let consumed =
            DemandCostReceipt::new(identity(&format!("R{level}")), usage(1, 0), Vec::new());
        let nesting = if level == LEVELS / 2 {
            Nesting::Frame { depth: 1 }
        } else {
            Nesting::InPlace
        };
        ledger.record_prerequisite(&consumed, nesting);
        ledger.charge().expect("the level's own unit");
        ledger.abandon_cost_scope();
    }
    let root = ledger.seal_cost_scope(None).expect("the root seals");
    assert!(
        root.prerequisites().len() <= 2,
        "{} prerequisites folded into the surviving consumer",
        root.prerequisites().len()
    );
    assert_eq!(
        root.exclusive(),
        usage(LEVELS as u64, 0),
        "the failed levels' own units"
    );
    assert_eq!(
        root.closure_usage(),
        usage(2 * LEVELS as u64, 0),
        "the failed levels' units and every result they consumed"
    );
    assert_eq!(
        root.footprint().instantiation_height,
        1,
        "the frame a failed level consumed a result through is the consumer's"
    );
}

/// A computation run again in the same demand — its result could not be
/// served — is charged again: the demand pays for the work it does, so a
/// demand that recomputes a result is bounded by its allowance however
/// often it recomputes. Each run's receipt costs the computation.
#[test]
fn a_recomputation_is_charged_as_the_work_it_is() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let ledger = &dispatch.connected_demand();
    let (_guard, _) = dispatch.enter_connected_demand(false);
    ledger.open_cost_scope(identity("P"));
    for _ in 0..2 {
        ledger.open_cost_scope(identity("C"));
        ledger.charge_units(4).expect("C's own units");
        ledger.reserve_bytes(8).expect("C's own bytes");
        let run = ledger.seal_cost_scope(None).expect("C seals");
        assert_eq!(run.exclusive(), usage(4, 8), "each run's receipt costs C");
        ledger.record_prerequisite(&run, Nesting::Entered);
    }
    let parent = ledger.seal_cost_scope(None).expect("P seals");
    assert_eq!(ledger.work_used_for_tests(), 8, "both runs are charged");
    assert_eq!(ledger.bytes_used_for_tests(), 16);
    assert_eq!(parent.closure_usage(), usage(4, 8), "C is one computation");
}

/// The rule a consumer follows for a delivered result: a result computed
/// for this demand is recorded without being charged again; a result
/// served from a store is admitted by replaying its receipt, then
/// recorded. Either way the consumer's receipt owes it. A refused replay is
/// plain resource incompleteness for this consumer.
enum Delivery {
    Fresh(Arc<DemandCostReceipt>),
    Replayed(Arc<DemandCostReceipt>),
}

fn consume(
    ledger: &super::connected_demand::ConnectedDemandLedger<'_>,
    delivery: &Delivery,
) -> Result<(), ReplayRefusal> {
    let receipt = match delivery {
        Delivery::Fresh(receipt) => receipt,
        Delivery::Replayed(receipt) => {
            ledger.replay_admit(receipt, None, Nesting::Entered)?;
            receipt
        }
    };
    ledger.record_prerequisite(receipt, Nesting::Entered);
    Ok(())
}

/// A result computed for this demand is not charged twice when delivered to
/// its consumer, and a result replayed from a store costs what computing it
/// did: the consumer's total is the same either way.
#[test]
fn a_fresh_delivery_is_not_charged_twice() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let ledger = &dispatch.connected_demand();
    let child = {
        let (_guard, _) = dispatch.enter_connected_demand(false);
        ledger.open_cost_scope(identity("P"));
        ledger.open_cost_scope(identity("C"));
        ledger.charge_units(4).expect("C's own units");
        let child = ledger.seal_cost_scope(None).expect("C seals");
        consume(ledger, &Delivery::Fresh(Arc::clone(&child))).expect("fresh is free");
        ledger.charge().expect("P's own unit");
        let parent = ledger.seal_cost_scope(None).expect("P seals");
        assert_eq!(ledger.work_used_for_tests(), 5, "C is paid once");
        assert_eq!(parent.exclusive(), usage(1, 0));
        child
    };
    let (_guard, _) = dispatch.enter_connected_demand(false);
    ledger.open_cost_scope(identity("P"));
    consume(ledger, &Delivery::Replayed(child)).expect("four units fit");
    ledger.charge().expect("P's own unit");
    let parent = ledger.seal_cost_scope(None).expect("P seals");
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
            nesting: Nesting::InPlace,
        })
        .collect();
    let wide = DemandCostReceipt::new(identity("W"), usage(1, 0), prerequisites);
    assert_eq!(wide.prerequisites().len(), 20_000);
}

/// A receipt chain as deep as the computations it costs is released
/// without a native frame per level: a 100,000-deep chain drops on a
/// 256 KiB stack.
#[test]
fn a_deep_receipt_chain_drops_in_constant_stack() {
    std::thread::Builder::new()
        .stack_size(256 << 10)
        .spawn(|| {
            let mut top = DemandCostReceipt::new(identity("D0"), usage(1, 0), Vec::new());
            for level in 1..100_000 {
                top = DemandCostReceipt::new(
                    identity(&format!("D{level}")),
                    usage(1, 0),
                    vec![super::cost_receipt::CostDependency {
                        receipt: top,
                        nesting: Nesting::InPlace,
                    }],
                );
            }
            drop(top);
        })
        .expect("spawn the small-stack thread")
        .join()
        .expect("the chain drops without overflowing");
}

/// A receipt's request operations are charged to the request's projection
/// fuse once per request — across its connected demands — and a replay the
/// fuse cannot pay is refused whole, spending and marking nothing.
#[test]
fn a_replay_spends_request_operations_once_per_request() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let ledger = &dispatch.connected_demand();
    let request = RequestBudget::new(5);
    let operations = |name: &str, count: u64| {
        DemandCostReceipt::new(
            identity(name),
            LogicalUsage {
                work: 1,
                operations: count,
                ..LogicalUsage::default()
            },
            Vec::new(),
        )
    };
    let first = operations("A", 3);
    {
        let (_guard, _) = dispatch.enter_connected_demand(false);
        ledger
            .replay_admit(&first, Some(&request), Nesting::Entered)
            .expect("three operations fit five");
    }
    assert_eq!(request.projection_ops_executed_count(), 3);
    {
        let (_guard, _) = dispatch.enter_connected_demand(false);
        ledger
            .replay_admit(&first, Some(&request), Nesting::Entered)
            .expect("a paid computation spends no operations again");
        assert_eq!(
            ledger.work_used_for_tests(),
            1,
            "the new demand still pays its work"
        );
    }
    assert_eq!(request.projection_ops_executed_count(), 3);
    let (_guard, _) = dispatch.enter_connected_demand(false);
    assert_eq!(
        ledger.replay_admit(&operations("B", 3), Some(&request), Nesting::Entered),
        Err(ReplayRefusal::Operations),
        "three more operations do not fit the two left"
    );
    assert_eq!(request.projection_ops_executed_count(), 3);
    assert_eq!(
        ledger.work_used_for_tests(),
        0,
        "the refusal charged nothing"
    );
}

/// Allowances intern to one profile identity: equal allowances are one
/// profile, any differing allowance — an operation cap, a connected cap,
/// a cost-model revision — another.
#[test]
fn a_budget_profile_is_one_interned_identity() {
    let spec = BudgetProfileSpec {
        work: 100,
        bytes: 1_000,
        construction_byte_refusal: false,
        query_depth: 24,
        instantiation_depth: 10,
        tail_steps: 10,
        request_operations: 2_000,
        relation_comparisons: 600,
        cost_model_revision: COST_MODEL_REVISION,
    };
    assert_eq!(BudgetProfile::intern(spec), BudgetProfile::intern(spec));
    for other in [
        BudgetProfileSpec { work: 101, ..spec },
        BudgetProfileSpec {
            request_operations: 2_001,
            ..spec
        },
        BudgetProfileSpec {
            cost_model_revision: COST_MODEL_REVISION + 1,
            ..spec
        },
    ] {
        assert_ne!(BudgetProfile::intern(spec), BudgetProfile::intern(other));
    }
}

// ── The public boundary ──────────────────────────────────────────────────

const BENCH_ROOT: &str = "/bench";
const BENCH_SCENARIO: &str = "/bench/scenario.ts";

/// `type S = { p0: 0 } | …` against `type T = { p(n-1): number } | …`:
/// every arm of `S` fits an arm of `T`, but never at its own position.
fn reversed_unions(arms: usize) -> String {
    let source: Vec<String> = (0..arms).map(|i| format!("{{ p{i}: {i} }}")).collect();
    let target: Vec<String> = (0..arms)
        .rev()
        .map(|i| format!("{{ p{i}: number }}"))
        .collect();
    format!(
        "type S = {};\ntype T = {};\ntype __Probe = [S] extends [T] ? 1 : 2;\nexport {{}};\n",
        source.join(" | "),
        target.join(" | ")
    )
}

/// A host configured the way the semantic benchmark configures one: a
/// project whose library is a root file under `noLib`, and a per-request
/// projection fuse of `projection_op_budget`.
fn bench_host(projection_op_budget: usize, scenario: &str) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone_with_tsconfig_projects(
        HostConfig {
            projection_op_budget,
            ..HostConfig::default()
        },
        &[(
            BENCH_ROOT,
            r#"{ "compilerOptions": { "strict": true, "noLib": true }, "files": ["lib.bench.d.ts", "scenario.ts"] }"#,
        )],
    ));
    let lib = "/bench/lib.bench.d.ts";
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        lib,
        "interface Object { toString(): string; }\n",
        crate::LanguageRegistry::global()
            .classify_static(lib)
            .static_resolution(),
    );
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        BENCH_SCENARIO,
        scenario,
        crate::FileLanguage::script_ts(),
    );
    host
}

/// The answer one request for `__Probe` gives: its wire bytes, or why
/// there are none.
fn probe_answer(host: &VerterHost) -> Result<Vec<u8>, String> {
    let (outcome, _) = host
        .resolve_named_symbol_with_audit(BENCH_SCENARIO, "__Probe", None)
        .into_parts();
    match outcome {
        Ok(Some(node)) => host
            .project_node_to_type_expr_json_bytes(&node)
            .ok_or_else(|| "the answer did not materialise".to_owned()),
        Ok(None) => Err("miss".to_owned()),
        Err(fault) => Err(format!("{fault:?}")),
    }
}

fn semantic_misses(host: &VerterHost) -> u64 {
    host.project_type_store()
        .semantic_graph()
        .stats_snapshot()
        .misses
}

/// The benchmark request envelope reaches the checker comparison limit,
/// including its resource recovery, rather than stopping at construction bytes.
fn assert_reversed_checker_answer(count: usize, expected: u8) {
    use verter_type_engine::semantic_query::PartialReasonSet;
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    {
        let host = bench_host(1_000_000, &reversed_unions(count));
        let (key, [source, target]) = reversed_conditional_key(&host, count);
        let read = || {
            let _request = fresh_request(100);
            root_read(&host, &key, full)
        };
        let before_reductions = ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::relation_reductions_for_tests();
        let before_visits = ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::relation_union_arm_visits_for_tests();
        let cold = read();
        let reductions = ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::relation_reductions_for_tests() - before_reductions;
        let visits = ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::relation_union_arm_visits_for_tests() - before_visits;
        assert!(
            visits <= 12 * count + 32,
            "{count} arms: linear union traversal, got {visits}"
        );
        assert!(
            reductions <= 4 * count + 8,
            "{count} arms: linear structural queries, got {reductions}"
        );
        assert_eq!(
            cold.answer,
            format!("literal Number({expected}.0)"),
            "{count} arms: checker answer and charged envelope: {cold:?}"
        );
        let recovered = count >= 2100;
        assert_eq!(
            cold.partial, recovered,
            "{count} arms: resource recovery class"
        );
        assert_eq!(
            cold.reasons,
            if recovered {
                PartialReasonSet::OPERATION_BUDGET
            } else {
                PartialReasonSet::empty()
            }
        );
        assert_eq!(
            cold.refusals,
            if recovered {
                vec![super::walk::ShallowDiagnostic::RelationTooComplex { source, target }]
            } else {
                Vec::new()
            },
            "{count} arms: TS2859 subjects"
        );
        let before_repeat = semantic_misses(&host);
        assert_eq!(read(), cold, "{count} arms: exact repeat");
        assert_eq!(
            semantic_misses(&host),
            before_repeat,
            "{count} arms: no recomputation"
        );
        println!(
            "reversed-{count}: {} partial={} work={} bytes={} reductions={reductions} visits={visits} repeat_misses=0",
            cold.answer, cold.partial, cold.work, cold.bytes
        );
    }
}

#[test]
fn reversed_object_unions_reach_the_checker_answer_200() {
    assert_reversed_checker_answer(200, 1);
}
#[test]
fn reversed_object_unions_reach_the_checker_answer_600() {
    assert_reversed_checker_answer(600, 1);
}
#[test]
fn reversed_object_unions_reach_the_checker_answer_1800() {
    assert_reversed_checker_answer(1800, 1);
}
#[test]
fn reversed_object_unions_reach_the_checker_answer_2100() {
    assert_reversed_checker_answer(2100, 2);
}
#[test]
fn reversed_object_unions_reach_the_checker_answer_3200() {
    assert_reversed_checker_answer(3200, 2);
}

#[test]
fn cold_query_entries_fit_within_the_checker_relation_envelope() {
    use verter_type_engine::semantic_query::checker_policy::with_relation_comparisons_for_tests;
    use verter_type_engine::semantic_query::PartialReasonSet;
    let host = bench_host(1_000_000, &reversed_unions(200));
    let (key, [source, target]) = reversed_conditional_key(&host, 200);
    let branch_proofs = || {
        host.project_type_store()
            .semantic_graph()
            .export_all_origin_edges()
            .iter()
            .filter(|(_, kind, edge)| {
                *kind == verter_type_engine::semantic_query::OriginEdgeKind::ConditionalSelect
                    && edge.sources.as_ref() == [source, target]
                    && matches!(
                        edge.meta,
                        verter_type_engine::semantic_query::OriginMeta::Branch(
                            verter_type_engine::semantic_query::BranchSelection::True
                                | verter_type_engine::semantic_query::BranchSelection::False
                        )
                    )
            })
            .count()
    };
    let before_recovery = branch_proofs();
    let comparisons = 20_000;
    let reducer_only =
        comparisons as usize * super::connected_demand::RELATION_UNITS_PER_COMPARISON;
    // One hundredth of the pinned checker's comparison allowance and
    // production work envelope keeps this rail discriminator small.
    let mapped = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK / 100;
    let read = |work| {
        let _request = fresh_request(100);
        with_relation_comparisons_for_tests(comparisons, || root_read(&host, &key, work))
    };
    let premature = read(reducer_only);
    let recovered = read(mapped);
    println!("reduced checker envelope: reducer={premature:?}; mapped={recovered:?}");
    assert_eq!(premature.work, reducer_only, "the work rail was spent");
    assert!(premature.partial);
    assert_eq!(premature.answer, "refusal carrier TypeNode");
    // The raw conditional exposes its undecided shell at this operational
    // stop, without attributing the stop to the checker's comparison budget.
    assert_eq!(premature.reasons, PartialReasonSet::UNDECIDED_CONDITIONAL);
    assert!(
        premature.refusals.is_empty(),
        "the checker was never reached"
    );
    assert_eq!(recovered.answer, "literal Number(2.0)", "{recovered:?}");
    assert_eq!(recovered.reasons, PartialReasonSet::OPERATION_BUDGET);
    assert!(recovered.partial);
    assert_eq!(
        recovered.refusals,
        vec![super::walk::ShallowDiagnostic::RelationTooComplex { source, target }]
    );
    assert_eq!(
        branch_proofs(),
        before_recovery,
        "a checker recovery must not publish a branch-selection proof"
    );
    let before_repeat = semantic_misses(&host);
    let before_reductions = ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::relation_reductions_for_tests();
    assert_eq!(read(mapped), recovered);
    assert_eq!(semantic_misses(&host), before_repeat);
    assert_eq!(
        ProjectSemanticDispatch::<crate::resolver_core::HostCapabilities>::relation_reductions_for_tests(),
        before_reductions,
        "the sealed operation refusal performs no structural comparisons"
    );
}

/// A request repeated warm answers exactly as it answered cold. Its
/// sub-results were charged to the request as they were computed; served
/// warm, their receipts charge the same, and a result the cold request
/// already computed is read again rather than recomputed, so the
/// projection fuse cannot trip on one and not the other. Reversed object
/// unions are the shape that showed it: the cold request ran out of
/// operations recomputing a library interface its warm repeat read once.
#[test]
fn a_warm_repeat_answers_as_its_cold_request() {
    let host = bench_host(200, &reversed_unions(40));
    let cold = probe_answer(&host);
    for repeat in 1..=3 {
        assert_eq!(
            probe_answer(&host),
            cold,
            "warm repeat {repeat} answers as the cold request did"
        );
    }
    let rendered = cold.ok().map(|bytes| {
        let expr: verter_type_expr::TypeExpr =
            serde_json::from_slice(&bytes).expect("the answer decodes");
        verter_type_expr::render_type_expr_display(&expr)
            .expect("the answer renders")
            .text
    });
    assert_eq!(
        rendered.as_deref(),
        Some("1"),
        "every arm of S fits an arm of T, as the checker relates them"
    );
}

/// An isolated root that runs out of its allowance seals its refusal: an
/// exact repeat under the same allowances answers it again without
/// evaluating anything, and an edit to what it read evaluates afresh.
#[test]
fn a_sealed_refusal_answers_its_repeat_without_evaluating() {
    let host = bench_host(1, &reversed_unions(4));
    let _scope = host
        .project_type_store()
        .semantic_graph()
        .enter_node_root_scope();
    let refused = probe_answer(&host).expect("the refusal is a typed partial value");
    assert!(
        host.project_type_store()
            .semantic_graph()
            .refusal_summary_count_for_tests()
            >= 1,
        "the refused root sealed its refusal"
    );
    let misses = semantic_misses(&host);
    assert_eq!(
        probe_answer(&host),
        Ok(refused.clone()),
        "the repeat answers the sealed refusal"
    );
    assert_eq!(
        semantic_misses(&host),
        misses,
        "the repeat evaluated nothing"
    );
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        BENCH_SCENARIO,
        &reversed_unions(4).replace("p0: 0", "p0: 10"),
        crate::FileLanguage::script_ts(),
    );
    let _ = probe_answer(&host);
    assert!(
        semantic_misses(&host) > misses,
        "an edit to what the refusal read evaluates afresh"
    );
}

/// Imported modules whose relation [`wide_refusal_root`] reads: enough that
/// the refusal's proving prefix is wider than `WIDE_SIGNATURE_WIDTH` while
/// every fact domain stays precise.
const WIDE_PREFIX_MODULES: usize = 300;

/// Work units that refuse [`wide_refusal_root`] only after it has read every
/// module.
const WIDE_PREFIX_WORK: usize = 2_000;

/// A host whose probe relates `S`, a union of one type imported from each
/// of [`WIDE_PREFIX_MODULES`] sibling modules, to the reversed union `T`,
/// and that relation as a root key.
fn wide_refusal_root() -> (
    Arc<VerterHost>,
    verter_type_engine::semantic_query::SemanticQueryKey,
) {
    let names: Vec<String> = (0..WIDE_PREFIX_MODULES)
        .map(|i| format!("m{i:04}.ts"))
        .collect();
    let sources: Vec<String> = (0..WIDE_PREFIX_MODULES)
        .map(|i| format!("export type A{i} = {{ p{i}: {i} }};\n"))
        .collect();
    let files: Vec<(&str, &str)> = names
        .iter()
        .map(String::as_str)
        .zip(sources.iter().map(String::as_str))
        .collect();
    let project = super::checker_probe_lane_tests::ProbeProject {
        files: &files,
        ..Default::default()
    };
    let host = super::checker_probe_lane_tests::probe_host(project);
    let imports: String = (0..WIDE_PREFIX_MODULES)
        .map(|i| format!("import type {{ A{i} }} from \"./m{i:04}\";\n"))
        .collect();
    let source_arms: Vec<String> = (0..WIDE_PREFIX_MODULES).map(|i| format!("A{i}")).collect();
    let target_arms: Vec<String> = (0..WIDE_PREFIX_MODULES)
        .rev()
        .map(|i| format!("{{ p{i}: number }}"))
        .collect();
    let source = format!(
        "{imports}type S = {};\ntype T = {};\n",
        source_arms.join(" | "),
        target_arms.join(" | ")
    );
    let key = super::checker_probe_lane_tests::with_probe_on_host(
        &host,
        project,
        &source,
        "[S, T]",
        |dispatch, node| {
            let elements = match dispatch.graph().node_data(node).as_deref() {
                Some(verter_type_engine::semantic_query::SemanticNodeData::Tuple {
                    elements,
                    ..
                }) => elements
                    .iter()
                    .map(|element| element.value)
                    .collect::<Vec<_>>(),
                other => panic!("the probe reads the pair [S, T], got {other:?}"),
            };
            dispatch
                .relate_key_for(elements[0], elements[1])
                .to_query_key()
        },
    );
    (host, key)
}

/// A refusal whose proving prefix is wider than a wide signature is sealed
/// whole and replayed like a narrow one: the root, out of work after reading
/// every imported module, seals a refusal whose carrier holds every fact it
/// read; the sealed refusal keeps the carrier's storage charged; an exact
/// repeat answers it without evaluating; a follower parked on a producer's
/// flight answers as the root does alone; and a change to the carrier's one
/// project-generation fact — which sorts after every file fact, at the end
/// of the canonical order — misses the refusal, so the root evaluates
/// afresh.
#[test]
fn a_wide_refusal_is_replayed_whole_and_missed_by_an_edit_at_its_end() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use verter_session_query::facts::fact_cache::FactVersionRef;
    use verter_session_query::facts::fact_read_set::WIDE_SIGNATURE_WIDTH;

    let (host, key) = wide_refusal_root();
    let refused = root_read(&host, &key, WIDE_PREFIX_WORK);
    assert!(
        refused.partial,
        "{WIDE_PREFIX_WORK} units cannot relate {WIDE_PREFIX_MODULES} reversed arms"
    );
    let store = Arc::clone(host.project_type_store().semantic_graph());
    let carriers = store.refusal_carriers_for_tests();
    assert_eq!(carriers.len(), 1, "the refused root sealed its refusal");
    let carrier = &carriers[0];
    assert!(
        carrier.facts.len() > WIDE_SIGNATURE_WIDTH,
        "the proving prefix is wider than a wide signature: {} facts",
        carrier.facts.len()
    );
    let storage = carrier.facts.tree_footprint();
    assert!(
        storage.nodes > 0 && storage.charged_bytes > 0,
        "the carrier completes a wide traced set, so it is held in tree storage: {storage:?}"
    );
    assert_eq!(
        (storage.pinned_bytes, storage.retained_bytes),
        (0, storage.charged_bytes),
        "sealing the refusal claimed every node of its carrier into a retained reservation"
    );
    let is_generation =
        |fact: &FactVersionRef| matches!(fact, FactVersionRef::ProjectGeneration { .. });
    assert_eq!(
        carrier
            .facts
            .iter()
            .filter(|fact| is_generation(fact))
            .count(),
        1,
        "the carrier reads the project generation once"
    );
    assert!(
        carrier
            .facts
            .iter()
            .rev()
            .take_while(|fact| !is_generation(fact))
            .all(|fact| !matches!(
                fact,
                FactVersionRef::FileWholeHash { .. }
                    | FactVersionRef::DerivedFactHash { .. }
                    | FactVersionRef::Parse(_)
                    | FactVersionRef::ResolveImports(_)
            )),
        "the project generation sorts after every file fact of the carrier"
    );

    let misses = semantic_misses(&host);
    assert_eq!(
        root_read(&host, &key, WIDE_PREFIX_WORK),
        refused,
        "the repeat answers the sealed refusal, charged as the refused root"
    );
    assert_eq!(semantic_misses(&host), misses, "and evaluates nothing");

    host.project_type_store().bump_project_generation();
    let _ = root_read(&host, &key, WIDE_PREFIX_WORK);
    assert!(
        semantic_misses(&host) > misses,
        "a change to the carrier's last file-independent fact misses the refusal"
    );

    store.invalidate_all();
    let outliving = carrier.facts.tree_footprint();
    assert_eq!(
        (outliving.nodes, outliving.charged_bytes),
        (storage.nodes, storage.charged_bytes),
        "storage outliving the refusal that claimed it stays charged while a holder lives"
    );
    drop(carriers);

    // A follower parked on a producer's flight answers as the root alone.
    let (host, key) = wide_refusal_root();
    let store = Arc::clone(host.project_type_store().semantic_graph());
    let claimed = Arc::new(AtomicBool::new(false));
    let held = Arc::new(AtomicBool::new(false));
    let joined_before = store.test_joiner_on_condvar_count();
    let hook: verter_type_engine::semantic_query_memo::test_support::ProduceHookForTests = {
        let (key, claimed, held, store) = (
            key.clone(),
            claimed.clone(),
            held.clone(),
            Arc::clone(&store),
        );
        Arc::new(move |claimed_key, _| {
            if claimed_key != &key || held.swap(true, Ordering::SeqCst) {
                return;
            }
            claimed.store(true, Ordering::SeqCst);
            wait_until("the follower to park on the producer's flight", || {
                store.test_joiner_on_condvar_count() > joined_before
            });
        })
    };
    let _hook = ProduceHookGuard::install(&store, hook);
    let (producer, follower) = std::thread::scope(|scope| {
        let producer = scope.spawn(|| root_read(&host, &key, WIDE_PREFIX_WORK));
        wait_until("the producer to claim the root", || {
            claimed.load(Ordering::SeqCst)
        });
        let follower = scope.spawn(|| root_read(&host, &key, WIDE_PREFIX_WORK));
        (
            producer.join().expect("the producer completes"),
            follower.join().expect("the follower completes"),
        )
    });
    assert!(
        store.test_joiner_on_condvar_count() > joined_before,
        "the follower joined the producer's flight"
    );
    assert_eq!(producer, refused, "the producer answers as the root alone");
    assert_eq!(follower, refused, "the follower answers as the root alone");
}

/// The relation `S` to `T` of [`reversed_unions`] with `arms` arms, as a
/// root query key on `host`.
fn reversed_relation_key(
    host: &Arc<VerterHost>,
    arms: usize,
    reversed: bool,
) -> verter_type_engine::semantic_query::SemanticQueryKey {
    super::checker_probe_lane_tests::with_probe_on_host(
        host,
        Default::default(),
        &reversed_unions(arms),
        "[S, T]",
        |dispatch, node| {
            let elements = match dispatch.graph().node_data(node).as_deref() {
                Some(verter_type_engine::semantic_query::SemanticNodeData::Tuple {
                    elements,
                    ..
                }) => elements
                    .iter()
                    .map(|element| element.value)
                    .collect::<Vec<_>>(),
                other => panic!("the probe reads the pair [S, T], got {other:?}"),
            };
            let (source, target) = if reversed {
                (elements[1], elements[0])
            } else {
                (elements[0], elements[1])
            };
            dispatch.relate_key_for(source, target).to_query_key()
        },
    )
}

/// What one root demand answered and what it was charged.
#[derive(Debug, PartialEq, Eq)]
struct RootAnswer {
    /// The relation's outcome, or the class of the refusal it answered.
    answer: String,
    status: verter_type_engine::semantic_query::FactStatus,
    has_value: bool,
    partial: bool,
    reasons: verter_type_engine::semantic_query::PartialReasonSet,
    work: usize,
    bytes: u64,
    /// The operation refusals the read reports.
    refusals: Vec<super::walk::ShallowDiagnostic>,
}

/// Run `key` as an isolated root demand with `work` units over a fresh
/// view of `host`, in whatever request is installed on this thread.
fn root_read(
    host: &Arc<VerterHost>,
    key: &verter_type_engine::semantic_query::SemanticQueryKey,
    work: usize,
) -> RootAnswer {
    root_read_with_context(host, key, work, 24, 0)
}

fn root_read_with_context(
    host: &Arc<VerterHost>,
    key: &verter_type_engine::semantic_query::SemanticQueryKey,
    work: usize,
    depth: u16,
    preceding: usize,
) -> RootAnswer {
    root_read_configured(host, key, preceding, |dispatch| {
        dispatch.set_connected_limits_for_tests(work, depth);
    })
}

/// [`root_read_with_context`] with the dispatcher's allowances set by
/// `configure` before the demand enters.
fn root_read_configured(
    host: &Arc<VerterHost>,
    key: &verter_type_engine::semantic_query::SemanticQueryKey,
    preceding: usize,
    configure: impl FnOnce(&ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>),
) -> RootAnswer {
    use verter_type_engine::semantic_query::{QueryResult, SemanticQueryValue};
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx =
        crate::resolver_core::HostResolverContext::<'_, verter_audit::observe::Uncaptured>::new(
            host,
            &store_view,
            overlay,
        );
    let dispatch = ProjectSemanticDispatch::new(&ctx);
    configure(&dispatch);
    let _connected = (preceding != 0).then(|| dispatch.enter_connected_demand(false).0);
    if preceding != 0 {
        dispatch.connected_demand().charge_units(preceding).unwrap();
    }
    let read = dispatch.execute_via_cold_build_helper(key.clone());
    let answer = match &read.value() {
        QueryResult::Value(SemanticQueryValue::TypeNode(node)) => {
            match dispatch.graph().node_data(*node).as_deref() {
                Some(verter_type_engine::semantic_query::SemanticNodeData::Literal(literal)) => {
                    format!("literal {literal:?}")
                }
                _ => "refusal carrier TypeNode".to_owned(),
            }
        }
        QueryResult::Value(SemanticQueryValue::Relation(payload)) => {
            format!("{:?}", payload.outcome)
        }
        QueryResult::Value(other) => format!("refusal carrier {:?}", other.tag()),
        QueryResult::Recursive(_) => "recursive".to_owned(),
        QueryResult::Error(error) => format!("error {error:?}"),
    };
    RootAnswer {
        answer,
        status: *read.fact_status(),
        has_value: matches!(read.value().clone(), QueryResult::Value(_)),
        partial: read.result_is_partial(),
        reasons: read.partial_reason_classes(),
        work: dispatch.connected_demand().work_used_for_tests(),
        bytes: dispatch.connected_demand().bytes_used_for_tests(),
        refusals: read
            .walker_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.is_operation_refusal())
            .map(super::walk::ShallowDiagnostic::copy_for_tests)
            .collect(),
    }
}

#[test]
fn operation_refusal_isolates_depth_and_preceding_connected_work() {
    use super::connected_demand::InstantiationBudgetForTests;
    use verter_type_engine::semantic_query::checker_policy::with_relation_comparisons_for_tests;
    let host = super::checker_probe_lane_tests::default_probe_host();
    let key = reversed_relation_key(&host, 40, false);
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    with_relation_comparisons_for_tests(50, || {
        let solo = root_read(&host, &key, full);
        assert!(solo.partial);
        let before = semantic_misses(&host);
        assert_eq!(root_read(&host, &key, full), solo);
        assert_eq!(
            semantic_misses(&host),
            before,
            "the exact isolated repeat is sealed"
        );
        for (depth, preceding) in [(23, 0), (24, 1)] {
            let before = semantic_misses(&host);
            let altered = root_read_with_context(&host, &key, full, depth, preceding);
            assert_eq!(
                (altered.answer, altered.partial, altered.reasons),
                (solo.answer.clone(), solo.partial, solo.reasons)
            );
            assert!(
                semantic_misses(&host) > before,
                "a different depth or non-isolated ledger must evaluate the check"
            );
        }
        let before = semantic_misses(&host);
        {
            let _depth = InstantiationBudgetForTests::install(9999);
            assert!(root_read(&host, &key, full).partial);
        }
        assert!(
            semantic_misses(&host) > before,
            "checker depth participates in refusal identity"
        );
        let before = semantic_misses(&host);
        assert_eq!(root_read(&host, &key, full), solo);
        assert_eq!(
            semantic_misses(&host),
            before,
            "the original isolated profile remains usable"
        );
    });
}

/// An exhausted comparison refuses before its apparent Object query enters,
/// even when index preparation would exhaust work before the first alternative
/// or the next alternative fits the last unit of connected work. Preserve the
/// recovery outcome as well as partiality, named subjects and exact repeats.
#[test]
fn object_union_comparison_refusal_precedes_apparent_read() {
    use super::checker_probe_lane_tests::{
        probe_host, with_probe_on_host, ProbeProject, PROBE_FILE,
    };
    use verter_type_engine::semantic_query::checker_policy::with_relation_comparisons_for_tests;
    use verter_type_engine::semantic_query::{
        PartialReasonSet, ProjectionMode, ProjectionReductionContext, SemanticNodeData,
        SemanticQueryKey,
    };

    let observe = |aliased, ambient, comparisons, work| {
        let project = ProbeProject {
            compiler_options: Some(r#"{"strict": true, "noLib": true}"#),
            files: if ambient {
                &[("lib.object.d.ts", "interface Object { z: number; }")]
            } else {
                &[]
            },
            ..Default::default()
        };
        let host = probe_host(project);
        // Missing keys never read these values. Aliases disable the index
        // without changing which structured comparisons the relation enters.
        let source = if aliased {
            "type V0 = 0; type V1 = 1; type V2 = 2; type V3 = 3; \
             type S = {p0: V0} | {p1: V1} | {p2: V2} | {p3: V3};"
        } else {
            "type S = {p0: 0} | {p1: 1} | {p2: 2} | {p3: 3};"
        };
        let source = format!("{source} type T = {{q2: number}} | {{q1: number}} | {{q0: number}};");
        let key = with_probe_on_host(&host, project, &source, "[S, T]", |dispatch, node| {
            assert_eq!(
                matches!(
                    dispatch.global_wrapper_surface("Object", &[], PROBE_FILE),
                    super::apparent_type::GlobalWrapper::Surface(_)
                ),
                ambient,
                "the apparent Object interface matches the control"
            );
            let data = dispatch.graph().node_data(node).expect("the tuple exists");
            let SemanticNodeData::Tuple { elements, .. } = data.as_ref() else {
                panic!("the probe reads [S, T]: {data:?}");
            };
            let normalize = |node| {
                dispatch
                    .normalize_node_for_structural_fact_demand(
                        node,
                        ProjectionReductionContext::published(ProjectionMode::Expanded),
                    )
                    .into_complete_node()
                    .expect("the union is prepared")
            };
            dispatch
                .relate_key_for(normalize(elements[0].value), normalize(elements[1].value))
                .to_query_key()
        });
        let SemanticQueryKey::Relate { source, target, .. } = &key else {
            unreachable!("a relation key")
        };
        with_relation_comparisons_for_tests(comparisons, || {
            let read = || {
                let _request = verter_type_engine::request_context::RequestContextGuard::install(
                    verter_type_engine::request_context::RequestContext::with_kind_timing_and_projection_budget(
                        1, Arc::from(PROBE_FILE), verter_audit::RequestKind::ComponentMeta, false, false, None, 1_000_000));
                root_read(&host, &key, work)
            };
            let cold = read();
            assert_eq!(read(), cold, "exact repeat, aliased={aliased}");
            let expected_refusals = if comparisons == 100 {
                vec![]
            } else {
                vec![super::walk::ShallowDiagnostic::RelationTooComplex {
                    source: *source,
                    target: *target,
                }]
            };
            assert_eq!(
                cold.refusals, expected_refusals,
                "TS2859 subjects: {cold:?}"
            );
            (
                cold.answer,
                cold.partial,
                cold.reasons,
                cold.status,
                cold.has_value,
            )
        })
    };
    for (ambient, comparisons, work) in [
        (false, 1, 5),
        (true, 2, 12),
        (true, 3, 20),
        (false, 100, 1_000),
        (true, 100, 1_000),
    ] {
        let ordinary = observe(true, ambient, comparisons, work);
        if comparisons <= 2 {
            assert!(
                matches!(
                    ordinary.3,
                    verter_type_engine::semantic_query::FactStatus::Unavailable(_)
                ),
                "an exhausted comparison has no scalar judgement: {ordinary:?}"
            );
            assert!(
                !ordinary.4,
                "the refusal must carry no usable payload: {ordinary:?}"
            );
        } else {
            assert_eq!(ordinary.0, "NotAssignable", "the exact ordinary verdict");
        }
        if comparisons == 100 {
            assert_eq!(
                ordinary,
                (
                    "NotAssignable".to_owned(),
                    false,
                    PartialReasonSet::empty(),
                    verter_type_engine::semantic_query::FactStatus::Complete,
                    true
                )
            );
        } else {
            assert!(ordinary.1, "the checker refusal is partial");
            assert_eq!(ordinary.2, PartialReasonSet::OPERATION_BUDGET);
        }
        assert_eq!(
            observe(false, ambient, comparisons, work),
            ordinary,
            "indexed and ordinary refusal classes: ambient={ambient}, comparisons={comparisons}, work={work}"
        );
    }
}

/// Install a fresh request with an operation fuse no relation here reaches.
fn fresh_request(id: u64) -> verter_type_engine::request_context::RequestContextGuard {
    verter_type_engine::request_context::RequestContextGuard::install(
        verter_type_engine::request_context::RequestContext::with_kind_timing_and_projection_budget(
            id,
            Arc::from(BENCH_SCENARIO),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            1_000_000,
        ),
    )
}

/// A refusal is sealed under the allowances it was decided by and answers
/// only those: a demand with any other allowance evaluates for itself —
/// with enough, it completes, and the completed result is kept for any
/// demand that can pay for it — while the original allowances keep
/// answering their refusal without evaluating, even once a complete result
/// is stored.
#[test]
fn a_refusal_answers_only_its_own_allowances() {
    let host = super::checker_probe_lane_tests::default_probe_host();
    let key = reversed_relation_key(&host, 40, false);
    let misses = || semantic_misses(&host);
    // Each read is its own request-less root demand over a fresh view, as
    // a repeat request would be.
    let read = |work: usize| {
        let answer = root_read(&host, &key, work);
        (answer.answer, answer.partial)
    };

    let refused = read(64);
    assert!(refused.1, "64 units cannot relate 40 arms");
    let before = misses();
    assert_eq!(read(64), refused, "the same allowances answer the refusal");
    assert_eq!(misses(), before, "and evaluate nothing");

    let other = read(65);
    assert!(other.1, "65 units cannot relate 40 arms either");
    assert!(misses() > before, "another allowance evaluates for itself");

    let complete = read(super::connected_demand::MAX_CONNECTED_PROJECTION_WORK);
    assert!(!complete.1, "the production allowance relates them");
    let healed = misses();
    assert_eq!(
        read(super::connected_demand::MAX_CONNECTED_PROJECTION_WORK),
        complete,
        "the completed relation is kept"
    );
    assert_eq!(misses(), healed, "for a demand that can pay for it");

    assert_eq!(
        read(64),
        refused,
        "a stored complete result never answers a demand that cannot pay for it"
    );
}

/// The path projection `A` → `path` on `host`, navigated, as a root key.
fn path_key(
    host: &Arc<VerterHost>,
    path: &[&str],
) -> verter_type_engine::semantic_query::SemanticQueryKey {
    use verter_type_engine::semantic_query::{
        PathSegment, ProjectionMode, ProjectionReductionContext, PropertyKey, SemanticQueryKey,
    };
    let path: verter_type_expr::OwnedSlice<PathSegment> = path
        .iter()
        .map(|member| PathSegment::Member(PropertyKey::identifier(Arc::from(*member))))
        .collect();
    super::checker_probe_lane_tests::with_probe_on_host(
        host,
        Default::default(),
        "type A = { x: X }; type X = { y: Y }; type Y = { z: Z }; type Z = { w: W }; type W = { v: number };",
        "A",
        |_, base| SemanticQueryKey::ProjectPath {
            base,
            path,
            context: ProjectionReductionContext::published(ProjectionMode::Navigate),
        },
    )
}

/// A prefix a longer path walk materialized and kept is charged what
/// reaching it costs — the hops up to it — never the whole walk it was
/// reached on the way to: served after the longer path, it costs what it
/// costs computed alone.
#[test]
fn a_backfilled_prefix_costs_what_computing_it_alone_costs() {
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let alone = {
        let host = super::checker_probe_lane_tests::default_probe_host();
        let prefix = path_key(&host, &["x"]);
        root_read(&host, &prefix, full)
    };
    assert!(!alone.partial, "the prefix projects");

    let host = super::checker_probe_lane_tests::default_probe_host();
    let longer = path_key(&host, &["x", "y", "z", "w"]);
    let walked = root_read(&host, &longer, full);
    assert!(!walked.partial, "the longer path projects");
    let prefix = path_key(&host, &["x"]);
    let misses = semantic_misses(&host);
    let served = root_read(&host, &prefix, full);
    assert_eq!(
        semantic_misses(&host),
        misses,
        "the prefix is served from the longer walk's backfill"
    );
    assert_eq!(served, alone, "and charged as it is alone");
    assert!(
        served.work < walked.work,
        "never the whole walk ({} of {})",
        served.work,
        walked.work
    );
}

/// An operation that exhausts its own allowance at an isolated root — a
/// relation past its structured-comparison allowance, TS2859 — refuses as
/// a function of the root, its inputs and its allowances alone, without
/// tripping the connected demand. Its refusal is sealed like a demand's: an
/// exact repeat answers it without evaluating, and a larger allowance is
/// another profile that evaluates and completes.
#[test]
fn an_operation_refusal_answers_its_repeat_without_evaluating() {
    use verter_type_engine::semantic_query::checker_policy::with_relation_comparisons_for_tests;
    use verter_type_engine::semantic_query::PartialReasonSet;
    let host = super::checker_probe_lane_tests::default_probe_host();
    let key = reversed_relation_key(&host, 40, false);
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let limited = || with_relation_comparisons_for_tests(50, || root_read(&host, &key, full));

    let refused = limited();
    assert!(
        refused.partial && refused.reasons.contains(PartialReasonSet::OPERATION_BUDGET),
        "50 comparisons cannot relate 40 reversed arms: {refused:?}"
    );
    let before = semantic_misses(&host);
    assert_eq!(limited(), refused, "the repeat answers the sealed refusal");
    assert_eq!(semantic_misses(&host), before, "and evaluates nothing");

    let complete = root_read(&host, &key, full);
    assert!(
        !complete.partial,
        "the checker's comparison allowance relates them"
    );
    assert!(semantic_misses(&host) > before, "another profile evaluates");
}

/// `[S] extends [T] ? 1 : 2` over [`reversed_unions`] with `arms` arms, as
/// a root query key on `host`, with the check and extends types it relates.
fn reversed_conditional_key(
    host: &Arc<VerterHost>,
    arms: usize,
) -> (
    verter_type_engine::semantic_query::SemanticQueryKey,
    [verter_type_engine::semantic_query::SemanticNodeId; 2],
) {
    super::checker_probe_lane_tests::with_probe_on_host(
        host,
        Default::default(),
        &reversed_unions(arms),
        "[[S], [T], 1, 2]",
        |dispatch, node| {
            let elements = match dispatch.graph().node_data(node).as_deref() {
                Some(verter_type_engine::semantic_query::SemanticNodeData::Tuple {
                    elements,
                    ..
                }) if elements.len() == 4 => elements
                    .iter()
                    .map(|element| element.value)
                    .collect::<Vec<_>>(),
                other => panic!("the probe reads [[S], [T], 1, 2], got {other:?}"),
            };
            (
                verter_type_engine::semantic_query::SemanticQueryKey::Conditional {
                    check: elements[0],
                    extends: elements[1],
                    true_branch: elements[2],
                    false_branch: elements[3],
                    distributive: false,
                    pending: None,
                },
                [elements[0], elements[1]],
            )
        },
    )
}

/// A relation refused at its comparison allowance (TS2859) names the two
/// types its check compared, and the refusal rides with every read
/// composed over it: the conditional consuming the check answers its false
/// branch as a resource partial and reports the same refusal, naming the
/// check and extends types, on the cold read and on its sealed repeat
/// alike. Under the checker's allowance nothing is refused and nothing is
/// reported.
#[test]
fn a_relation_refusal_names_its_subjects_through_its_consumer() {
    use super::walk::ShallowDiagnostic;
    use verter_type_engine::semantic_query::checker_policy::with_relation_comparisons_for_tests;
    use verter_type_engine::semantic_query::{PartialReasonSet, SemanticQueryKey};
    let host = super::checker_probe_lane_tests::default_probe_host();
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let limited = |key: &SemanticQueryKey| {
        with_relation_comparisons_for_tests(50, || root_read(&host, key, full))
    };

    let relation = reversed_relation_key(&host, 40, false);
    let SemanticQueryKey::Relate { source, target, .. } = &relation else {
        panic!("the relation key relates a pair, got {relation:?}");
    };
    let named = vec![ShallowDiagnostic::RelationTooComplex {
        source: *source,
        target: *target,
    }];
    let refused = limited(&relation);
    assert!(
        refused.reasons.contains(PartialReasonSet::OPERATION_BUDGET),
        "50 comparisons cannot relate 40 reversed arms: {refused:?}"
    );
    assert_eq!(
        refused.refusals, named,
        "the refusal names the check's pair"
    );
    let before_repeat = semantic_misses(&host);
    assert_eq!(
        limited(&relation),
        refused,
        "its sealed repeat reports it too"
    );
    assert_eq!(
        semantic_misses(&host),
        before_repeat,
        "no comparisons recomputed"
    );
    assert!(
        root_read(&host, &relation, full).refusals.is_empty(),
        "the checker's allowance refuses nothing"
    );

    let (conditional, [check, extends]) = reversed_conditional_key(&host, 40);
    let consumed = limited(&conditional);
    assert_eq!(
        consumed.answer, "literal Number(2.0)",
        "the conditional recovers the checker refusal as false: {consumed:?}"
    );
    assert_eq!(
        (consumed.partial, consumed.reasons),
        (true, PartialReasonSet::OPERATION_BUDGET),
        "the conditional is a resource partial of the operation budget alone"
    );
    assert_eq!(
        consumed.refusals,
        vec![ShallowDiagnostic::RelationTooComplex {
            source: check,
            target: extends,
        }],
        "the conditional reports the refusal of its check"
    );
    let before_repeat = semantic_misses(&host);
    assert_eq!(limited(&conditional), consumed, "cold and repeat agree");
    assert_eq!(
        semantic_misses(&host),
        before_repeat,
        "the consumer refusal is replayed"
    );
    let decided = root_read(&host, &conditional, full);
    assert!(
        !decided.partial && decided.refusals.is_empty(),
        "the checker's allowance decides the conditional: {decided:?}"
    );
}

/// Each relation check starts with the whole allowance: two checks of one
/// demand, each within it but not together, both relate. Forty reversed
/// arms record about 820 comparisons; an allowance of 1,000 holds each check
/// and not their sum.
#[test]
fn each_relation_check_records_against_a_fresh_allowance() {
    use verter_type_engine::semantic_query::checker_policy::with_relation_comparisons_for_tests;
    let arms = |prefix: &str| {
        let source: Vec<String> = (0..40).map(|i| format!("{{ {prefix}{i}: {i} }}")).collect();
        let target: Vec<String> = (0..40)
            .rev()
            .map(|i| format!("{{ {prefix}{i}: number }}"))
            .collect();
        (source.join(" | "), target.join(" | "))
    };
    let (s, t) = arms("p");
    let (u, v) = arms("q");
    let source = format!("type S = {s};\ntype T = {t};\ntype U = {u};\ntype V = {v};\n");
    let probe = "[[S] extends [T] ? 1 : 2, [U] extends [V] ? 1 : 2]";
    with_relation_comparisons_for_tests(1_000, || {
        let failures = super::checker_probe_lane_tests::mismatches(&source, &[(probe, "[1, 1]")]);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    });
    with_relation_comparisons_for_tests(500, || {
        super::checker_probe_lane_tests::with_recovered_probe(&source, probe, |_, _| {});
    });
}

/// A relation warmed under the checker's comparison allowance never
/// answers a demand under a smaller one: its receipt carries the structured
/// comparisons its cold run recorded, so a demand that could not record
/// them is refused the replay and relates the pair itself — stopping where
/// a cold run under that allowance stops, on a fresh host as on a warm one.
#[test]
fn a_warm_relation_answers_to_the_comparison_allowance_its_cold_run_needed() {
    use verter_type_engine::semantic_query::checker_policy::with_relation_comparisons_for_tests;
    use verter_type_engine::semantic_query::PartialReasonSet;
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let cold_limited = {
        let host = super::checker_probe_lane_tests::default_probe_host();
        let key = reversed_relation_key(&host, 40, false);
        with_relation_comparisons_for_tests(50, || root_read(&host, &key, full))
    };
    assert!(
        cold_limited.partial
            && cold_limited
                .reasons
                .contains(PartialReasonSet::OPERATION_BUDGET),
        "50 comparisons cannot relate 40 reversed arms cold: {cold_limited:?}"
    );

    let host = super::checker_probe_lane_tests::default_probe_host();
    let key = reversed_relation_key(&host, 40, false);
    assert!(
        !root_read(&host, &key, full).partial,
        "the checker's allowance relates them, and keeps the relation"
    );
    let warm_limited = with_relation_comparisons_for_tests(50, || root_read(&host, &key, full));
    assert_eq!(
        (
            warm_limited.answer,
            warm_limited.partial,
            warm_limited.reasons
        ),
        (
            cold_limited.answer,
            cold_limited.partial,
            cold_limited.reasons
        ),
        "a smaller allowance answers as its cold run does, never from the warm relation"
    );
}

/// The same boundary through the public named-symbol request: a request
/// under a small comparison allowance answers its conditional as a cold
/// request under it does, whatever a request under the checker's allowance
/// warmed before it.
#[test]
fn a_named_symbol_under_a_smaller_comparison_allowance_answers_as_cold() {
    use verter_type_engine::semantic_query::checker_policy::with_relation_comparisons_for_tests;
    let limited =
        |host: &VerterHost| with_relation_comparisons_for_tests(50, || probe_answer(host));
    let cold = limited(&bench_host(1_000_000, &reversed_unions(40)));
    let host = bench_host(1_000_000, &reversed_unions(40));
    let warmed = probe_answer(&host);
    assert_ne!(
        warmed, cold,
        "the checker's allowance relates the arms, a smaller one does not"
    );
    assert_eq!(
        limited(&host),
        cold,
        "after a warm request at the full allowance"
    );
}

/// A refusal is sealed at the request state its root entered: the
/// operations the request had spent and the computations it had paid. A
/// root entering a request at that state answers from it whatever runs
/// after it; the same root run after another root of its request has paid
/// for computations is not that refusal and evaluates for itself — so the
/// order of a request's roots never lets one inherit a refusal decided
/// under another.
#[test]
fn a_refusal_answers_only_the_request_state_it_entered_at() {
    let host = super::checker_probe_lane_tests::default_probe_host();
    let key = reversed_relation_key(&host, 40, false);
    let other = reversed_relation_key(&host, 40, true);
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let misses = || semantic_misses(&host);

    let refused = {
        let _request = fresh_request(1);
        root_read(&host, &key, 64)
    };
    assert!(refused.partial, "64 units cannot relate 40 arms");

    // The root first in its request: the refusal answers it.
    {
        let _request = fresh_request(2);
        let before = misses();
        assert_eq!(root_read(&host, &key, 64), refused);
        assert_eq!(
            misses(),
            before,
            "a root entering a fresh request evaluates nothing"
        );
        assert!(!root_read(&host, &other, full).partial);
    }

    // The same root after another root paid for computations in its
    // request: a different entry state, which evaluates.
    {
        let _request = fresh_request(3);
        assert!(!root_read(&host, &other, full).partial);
        let before = misses();
        let after_other = root_read(&host, &key, 64);
        assert!(
            misses() > before,
            "a root entering a request with paid computations evaluates for itself"
        );
        assert_eq!(
            (after_other.answer, after_other.partial, after_other.reasons),
            (refused.answer.clone(), refused.partial, refused.reasons),
            "and is refused where the evaluation stops, not where a sealed refusal did"
        );
    }
}

/// A cancelled request never answers from a sealed refusal: an exact
/// repeat of a sealed root under an otherwise identical request that is
/// cancelled takes the cancellation terminal — as it would with no summary
/// kept — and spends nothing of its request.
#[test]
fn a_cancelled_repeat_never_answers_from_a_sealed_refusal() {
    use verter_type_engine::semantic_query::PartialReasonSet;
    let host = super::checker_probe_lane_tests::default_probe_host();
    let key = reversed_relation_key(&host, 40, false);
    let refused = {
        let _request = fresh_request(1);
        root_read(&host, &key, 64)
    };
    assert!(refused.partial && !refused.reasons.contains(PartialReasonSet::CANCELLED));
    {
        let _request = fresh_request(2);
        assert_eq!(
            root_read(&host, &key, 64),
            refused,
            "an uncancelled repeat answers the sealed refusal"
        );
    }

    let cancelled =
        verter_type_engine::request_context::RequestContext::with_kind_timing_and_projection_budget(
            3,
            Arc::from(BENCH_SCENARIO),
            verter_audit::RequestKind::ComponentMeta,
            false,
            false,
            None,
            1_000_000,
        );
    cancelled.cancel();
    let _request = verter_type_engine::request_context::RequestContextGuard::install(cancelled);
    let answer = root_read(&host, &key, 64);
    assert!(
        answer.partial && answer.reasons.contains(PartialReasonSet::CANCELLED),
        "a cancelled repeat is answered as cancelled: {answer:?}"
    );
    assert_eq!(
        (answer.work, answer.bytes),
        (0, 0),
        "and charged nothing of the sealed evaluation"
    );
    assert_eq!(
        verter_type_engine::request_context::current_request_budget()
            .expect("the cancelled request is installed")
            .projection_ops_executed_count(),
        0,
        "and spent none of its request's operations"
    );
}

/// A demand served the sub-results an earlier demand computed before its
/// allowance ran out is charged exactly as its cold run, wherever the
/// earlier demand stopped: a stored evaluation's receipt carries everything
/// computing it charged, including the step its consumer pays to enter it.
#[test]
fn a_partly_warm_demand_is_charged_as_its_cold_run() {
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let cold = {
        let host = super::checker_probe_lane_tests::default_probe_host();
        let key = reversed_relation_key(&host, 2, false);
        root_read(&host, &key, full)
    };
    assert!(!cold.partial, "the production allowance relates them");
    for stopped_at in 1..cold.work {
        let host = super::checker_probe_lane_tests::default_probe_host();
        let key = reversed_relation_key(&host, 2, false);
        let _ = root_read(&host, &key, stopped_at);
        assert_eq!(
            root_read(&host, &key, full),
            cold,
            "after a demand that stopped at {stopped_at} units"
        );
    }
}

/// Waits until `ready` holds, failing the test past a deadline rather than
/// hanging it.
fn wait_until(what: &str, ready: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !ready() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Removes a store's producer hook however the test ends.
struct ProduceHookGuard<'s>(&'s verter_type_engine::semantic_query_memo::SemanticGraphStore);

impl<'s> ProduceHookGuard<'s> {
    fn install(
        store: &'s verter_type_engine::semantic_query_memo::SemanticGraphStore,
        hook: verter_type_engine::semantic_query_memo::test_support::ProduceHookForTests,
    ) -> Self {
        store.set_produce_hook_for_tests(Some(hook));
        Self(store)
    }
}

impl Drop for ProduceHookGuard<'_> {
    fn drop(&mut self) {
        self.0.set_produce_hook_for_tests(None);
    }
}

/// Followers that join another demand's producer under different
/// allowances answer and are charged exactly as they are alone and cold.
/// The producer, under the production allowance, is held once it claims the
/// root until both followers are parked on its flight; released, it
/// publishes a complete, priced result. The follower that can pay replays
/// its receipt and is charged as its cold run; the one that cannot is
/// refused the joined result and computes the root itself, stopping where
/// its cold run stops.
#[test]
fn concurrent_profiles_and_followers_are_charged_as_alone() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let alone = |work: usize| {
        let host = super::checker_probe_lane_tests::default_probe_host();
        let key = reversed_relation_key(&host, 24, false);
        root_read(&host, &key, work)
    };
    let refused_alone = alone(48);
    let complete_alone = alone(full);
    assert!(refused_alone.partial, "48 units cannot relate 24 arms");
    assert!(
        !complete_alone.partial,
        "the production allowance relates them"
    );

    let host = super::checker_probe_lane_tests::default_probe_host();
    let key = reversed_relation_key(&host, 24, false);
    let store = Arc::clone(host.project_type_store().semantic_graph());
    let claimed = Arc::new(AtomicBool::new(false));
    let held = Arc::new(AtomicBool::new(false));
    let joined_before = store.test_joiner_on_condvar_count();
    let hook: verter_type_engine::semantic_query_memo::test_support::ProduceHookForTests = {
        let (key, claimed, held, store) = (
            key.clone(),
            claimed.clone(),
            held.clone(),
            Arc::clone(&store),
        );
        Arc::new(move |claimed_key, _| {
            if claimed_key != &key || held.swap(true, Ordering::SeqCst) {
                return;
            }
            claimed.store(true, Ordering::SeqCst);
            wait_until("both followers to park on the producer's flight", || {
                store.test_joiner_on_condvar_count() >= joined_before + 2
            });
        })
    };
    let _hook = ProduceHookGuard::install(&store, hook);
    let (producer, followers) = std::thread::scope(|scope| {
        let producer = scope.spawn(|| root_read(&host, &key, full));
        wait_until("the producer to claim the root", || {
            claimed.load(Ordering::SeqCst)
        });
        let followers: Vec<_> = [48, full]
            .into_iter()
            .map(|work| {
                let (host, key) = (&host, &key);
                (work, scope.spawn(move || root_read(host, key, work)))
            })
            .collect();
        let producer = producer.join().expect("the producer completes");
        let followers: Vec<_> = followers
            .into_iter()
            .map(|(work, follower)| (work, follower.join().expect("a follower completes")))
            .collect();
        (producer, followers)
    });
    assert!(
        store.test_joiner_on_condvar_count() >= joined_before + 2,
        "both followers joined the producer's flight"
    );
    assert_eq!(
        producer, complete_alone,
        "the producer answers as it does alone"
    );
    for (work, answer) in followers {
        let expected = if work == full {
            &complete_alone
        } else {
            &refused_alone
        };
        assert_eq!(
            &answer, expected,
            "a follower with {work} units answers and is charged as it is alone"
        );
    }
}

/// A root whose evaluation a cross-task wait cycle cut never seals its
/// refusal: the read the cycle answered with a recursion carrier is work
/// another task's schedule withheld, so where the root stopped is not its
/// own. Two tasks each produce one key and need the other's; the root then
/// runs out of its allowance. No refusal is kept, and the root evaluated
/// alone afterwards answers as a solo cold run.
#[test]
fn a_root_cut_by_a_wait_cycle_never_seals_its_refusal() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let solo = {
        let host = super::checker_probe_lane_tests::default_probe_host();
        let key = reversed_relation_key(&host, 40, false);
        root_read(&host, &key, 64)
    };
    assert!(solo.partial, "64 units cannot relate 40 arms");

    let host = super::checker_probe_lane_tests::default_probe_host();
    let k = reversed_relation_key(&host, 40, false);
    let j = reversed_relation_key(&host, 40, true);
    let store = Arc::clone(host.project_type_store().semantic_graph());
    let k_claimed = Arc::new(AtomicBool::new(false));
    let j_claimed = Arc::new(AtomicBool::new(false));
    let cut = Arc::new(AtomicBool::new(false));
    let hook: verter_type_engine::semantic_query_memo::test_support::ProduceHookForTests = {
        let (k, j) = (k.clone(), j.clone());
        let (k_claimed, j_claimed, cut, store) = (
            k_claimed.clone(),
            j_claimed.clone(),
            cut.clone(),
            Arc::clone(&store),
        );
        Arc::new(move |claimed, nested| {
            if claimed == &k && !k_claimed.swap(true, Ordering::SeqCst) {
                // K's producer: once J's producer waits on K, need J.
                wait_until("J's producer to claim J", || {
                    j_claimed.load(Ordering::SeqCst)
                });
                wait_until("J's producer to wait on K", || {
                    store.wait_graph_counts_for_tests().1 >= 1
                });
                nested(j.clone());
                cut.store(true, Ordering::SeqCst);
            } else if claimed == &j && !j_claimed.load(Ordering::SeqCst) {
                // J's producer: need K, which K's producer holds.
                wait_until("K's producer to claim K", || {
                    k_claimed.load(Ordering::SeqCst)
                });
                j_claimed.store(true, Ordering::SeqCst);
                nested(k.clone());
            }
        })
    };
    let hook = ProduceHookGuard::install(&store, hook);
    let refused = std::thread::scope(|scope| {
        let refused = scope.spawn(|| root_read(&host, &k, 64));
        let other = scope.spawn(|| root_read(&host, &j, full));
        let refused = refused.join().expect("K's root completes");
        other.join().expect("J's root completes");
        refused
    });
    drop(hook);
    assert!(
        cut.load(Ordering::SeqCst),
        "K's producer needed J across the cycle"
    );
    assert!(refused.partial, "K's root still runs out of its allowance");
    assert_eq!(
        store.refusal_summary_count_for_tests(),
        0,
        "a refusal a wait cycle cut is never sealed"
    );
    let misses = semantic_misses(&host);
    let repeat = root_read(&host, &k, 64);
    assert!(
        semantic_misses(&host) > misses,
        "the repeat evaluates for itself"
    );
    assert_eq!(
        (repeat.answer, repeat.partial, repeat.reasons),
        (solo.answer, solo.partial, solo.reasons),
        "and answers as the root does alone"
    );
}

/// The refusal table is bounded: past its cap the oldest refusal is
/// dropped (its root evaluates again), the newest kept. An evaluation the
/// project moved under is torn, and a cancelled one stopped for a reason
/// that is not its allowance: neither ever becomes a refusal.
#[test]
fn the_refusal_table_keeps_the_newest_and_refuses_torn_evaluations() {
    use verter_type_engine::semantic_query::{
        CacheRead, PartialReasonSet, QueryResult, ResolveDeclKey, ScopeId, SemanticQueryKey,
        SemanticQueryValue,
    };
    use verter_type_engine::semantic_query_memo::refusal_summary::REFUSAL_SUMMARY_CAP;

    let host = host();
    let store = verter_type_engine::semantic_query_memo::SemanticGraphStore::new();
    let profile = BudgetProfile::intern(BudgetProfileSpec {
        work: 64,
        bytes: 1_024,
        construction_byte_refusal: false,
        query_depth: 24,
        instantiation_depth: 10,
        tail_steps: 10,
        request_operations: 0,
        relation_comparisons: 600,
        cost_model_revision: COST_MODEL_REVISION,
    });
    let key = |name: usize| {
        SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: ScopeId {
                canonical_id: verter_type_expr::OwnedStr::from("/refusals.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                local_scope: None,
                binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
            },
            name: verter_type_expr::OwnedStr::from(format!("R{name}")),
        })
    };
    let refusal = || {
        CacheRead::unleased_for_tests(
            QueryResult::<SemanticQueryValue>::Error(
                verter_type_engine::semantic_query::QueryError::Miss,
            ),
            verter_type_engine::semantic_query_memo::empty_signature(),
            verter_type_engine::project_semantic_dispatch::walk::WalkerDiagnostics::default(),
            true,
            verter_type_engine::semantic_query::FactStatus::unavailable(
                PartialReasonSet::PROJECTION_WORK_LIMIT,
            ),
            verter_type_engine::semantic_query::ReadReceipt::Unpriced,
        )
    };
    let carrier =
        || verter_session_query::facts::fact_cache::ReadSetSignature::new(Arc::from(Vec::new()));
    let entry = store.refusal_entry_for_tests(&host);
    for name in 0..=REFUSAL_SUMMARY_CAP {
        assert!(store.seal_refusal_for_tests(
            &host,
            key(name),
            profile.clone(),
            entry,
            refusal(),
            carrier(),
        ));
    }
    assert_eq!(store.refusal_summary_count_for_tests(), REFUSAL_SUMMARY_CAP);
    assert!(
        !store.has_sealed_refusal_for_tests(&host, key(0), &profile),
        "the oldest refusal was dropped"
    );
    assert!(store.has_sealed_refusal_for_tests(&host, key(REFUSAL_SUMMARY_CAP), &profile));
    assert!(
        !store.has_sealed_refusal_for_tests(
            &host,
            key(REFUSAL_SUMMARY_CAP),
            &BudgetProfile::intern(BudgetProfileSpec {
                work: 65,
                ..*profile.spec()
            }),
        ),
        "another profile never inherits the refusal"
    );

    let torn = key(REFUSAL_SUMMARY_CAP + 1);
    assert!(
        !store.seal_refusal_for_tests(
            &host,
            torn.clone(),
            profile.clone(),
            verter_type_engine::semantic_query_memo::refusal_summary::RefusalEntryState {
                generation: entry.generation.wrapping_sub(1),
                ..entry
            },
            refusal(),
            carrier(),
        ),
        "an evaluation the project moved under is never sealed"
    );
    assert!(!store.has_sealed_refusal_for_tests(&host, torn, &profile));

    let cancelled_key = key(REFUSAL_SUMMARY_CAP + 2);
    let cancelled = verter_type_engine::request_context::RequestContext::new(
        1,
        Arc::from("/refusals.ts"),
        false,
        None,
    );
    cancelled.cancel();
    {
        let _request = verter_type_engine::request_context::RequestContextGuard::install(cancelled);
        assert!(
            !store.seal_refusal_for_tests(
                &host,
                cancelled_key.clone(),
                profile.clone(),
                entry,
                refusal(),
                carrier(),
            ),
            "a cancelled evaluation is never sealed"
        );
    }
    assert!(!store.has_sealed_refusal_for_tests(&host, cancelled_key, &profile));

    // A refusal is retained only with a reservation on the process
    // retention account: an account with no room keeps none.
    let exhausted = verter_type_engine::semantic_query_memo::SemanticGraphStore::with_account(
        verter_session_query::retention::StoreAccount::new(
            verter_session_query::retention::SemanticRetentionAccount::new(
                verter_session_query::retention::RetentionLimits {
                    aggregate_ceiling_bytes: 0,
                    ..verter_session_query::retention::RetentionLimits::defaults()
                },
            ),
        ),
    );
    assert!(
        !exhausted.seal_refusal_for_tests(
            &host,
            key(0),
            profile.clone(),
            exhausted.refusal_entry_for_tests(&host),
            refusal(),
            carrier(),
        ),
        "a refusal the account declines is returned, never sealed"
    );
    assert_eq!(exhausted.refusal_summary_count_for_tests(), 0);
    assert!(store.retention_account().snapshot().retained_bytes > 0);

    // An evaluation that entered before the table was cleared finishes
    // after the clear: its refusal, decided against what the clear
    // dropped, is never inserted. One entering after the clear is.
    let before_clear = store.refusal_entry_for_tests(&host);
    store.invalidate_all();
    assert_eq!(store.refusal_summary_count_for_tests(), 0);
    assert!(
        !store.seal_refusal_for_tests(
            &host,
            key(0),
            profile.clone(),
            before_clear,
            refusal(),
            carrier(),
        ),
        "a refusal entered before a clear is never sealed after it"
    );
    assert!(!store.has_sealed_refusal_for_tests(&host, key(0), &profile));
    assert!(store.seal_refusal_for_tests(
        &host,
        key(0),
        profile.clone(),
        store.refusal_entry_for_tests(&host),
        refusal(),
        carrier(),
    ));
}

/// `[{ a: 0 } | … | { a: n - 1 }, { b: 0 } | … | { b: n - 1 }]`: two object
/// unions whose intersection's cross product the checker builds whole.
fn object_union_pair(n: usize) -> String {
    let union = |key: &str| {
        (0..n)
            .map(|i| format!("{{ {key}: {i} }}"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    format!("[{}, {}]", union("a"), union("b"))
}

/// The intersection of the two unions `probe` reads, as a root query key
/// on `host` over `source`.
fn intersection_root_key(
    host: &Arc<VerterHost>,
    source: &str,
    probe: &str,
) -> verter_type_engine::semantic_query::SemanticQueryKey {
    super::checker_probe_lane_tests::with_probe_on_host(
        host,
        Default::default(),
        source,
        probe,
        |dispatch, node| {
            let arms: Vec<_> = match dispatch.graph().node_data(node).as_deref() {
                Some(verter_type_engine::semantic_query::SemanticNodeData::Tuple {
                    elements,
                    ..
                }) => elements.iter().map(|element| element.value).collect(),
                other => panic!("the probe reads a pair of unions, got {other:?}"),
            };
            verter_type_engine::semantic_query::SemanticQueryKey::reduce_intersection_operands(
                dispatch.graph().store(),
                Arc::from(arms),
            )
        },
    )
}

/// An intersection's distribution is paid for whole before its first
/// constituent is built, so a root whose allowance cannot pay refuses as a
/// function of the root and its allowance alone, and seals that refusal:
/// an exact repeat answers it without evaluating; the same root entered
/// inside another demand, or under another allowance, never inherits it
/// and evaluates for itself; the production allowance distributes all 900
/// constituents; and an edit to the file the arms are written in misses
/// the refusal.
#[test]
fn an_unpaid_distribution_seals_its_refusal_under_its_own_allowance() {
    use verter_type_engine::semantic_query::PartialReasonSet;
    let host = super::checker_probe_lane_tests::default_probe_host();
    // The key names the probe's nodes; the scope keeps them for every read.
    let _scope = host
        .project_type_store()
        .semantic_graph()
        .enter_node_root_scope();
    let source = "export type Marker = 1;\n";
    let key = intersection_root_key(&host, source, &object_union_pair(30));
    let allowance = 500;
    let limited = || root_read(&host, &key, allowance);

    let refused = limited();
    assert!(
        refused.partial
            && refused
                .reasons
                .contains(PartialReasonSet::PROJECTION_WORK_LIMIT),
        "500 units cannot pay for a 900-constituent product: {refused:?}"
    );
    assert!(
        refused.work <= allowance,
        "the unpaid product charged nothing past the allowance: {refused:?}"
    );
    assert!(
        host.project_type_store()
            .semantic_graph()
            .refusal_summary_count_for_tests()
            >= 1,
        "the refused root sealed its refusal"
    );
    let misses = semantic_misses(&host);
    assert_eq!(limited(), refused, "the repeat answers the sealed refusal");
    assert_eq!(semantic_misses(&host), misses, "and evaluates nothing");

    let nested = root_read_with_context(&host, &key, allowance + 10, 24, 10);
    assert!(nested.partial, "{nested:?}");
    let after_nested = semantic_misses(&host);
    assert!(
        after_nested > misses,
        "a root inside another demand evaluates for itself"
    );
    let other = root_read(&host, &key, allowance + 1);
    assert!(other.partial, "{other:?}");
    assert!(
        semantic_misses(&host) > after_nested,
        "another allowance is another profile, evaluated afresh"
    );

    let complete = root_read(
        &host,
        &key,
        super::connected_demand::MAX_CONNECTED_PROJECTION_WORK,
    );
    assert!(
        !complete.partial,
        "the production allowance distributes: {complete:?}"
    );
    assert!(
        complete.work > 900,
        "every constituent was paid for: {complete:?}"
    );

    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        super::checker_probe_lane_tests::PROBE_FILE,
        &crate::u6_flow_shape_corpus_tests::module_script("export type Marker = 2;\n"),
        crate::FileLanguage::script_ts(),
    );
    let before_edit = semantic_misses(&host);
    let _ = limited();
    assert!(
        semantic_misses(&host) > before_edit,
        "an edit to what the refusal read evaluates afresh"
    );
}

/// An intersection's distribution charges its construction footprint to
/// the demand whatever the byte allowance; the advisory construction-byte
/// rail is off unless enabled, so an allowance half that footprint still
/// distributes all 900 constituents, complete. Enabled, the same allowance
/// refuses the construction before it builds, a partial of the memory
/// rail. Each read is cold, on a host of its own.
#[test]
fn construction_byte_refusal_is_opt_in() {
    use verter_type_engine::semantic_query::PartialReasonSet;
    let work = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let cold_read = |bytes: u64, refusal: bool| {
        let host = super::checker_probe_lane_tests::default_probe_host();
        let _scope = host
            .project_type_store()
            .semantic_graph()
            .enter_node_root_scope();
        let key = intersection_root_key(&host, "export type Marker = 1;\n", &object_union_pair(30));
        root_read_configured(&host, &key, 0, |dispatch| {
            dispatch.set_connected_limits_for_tests(work, 24);
            dispatch.set_construction_byte_limit_for_tests(bytes);
            dispatch
                .connected_demand()
                .set_construction_byte_refusal_for_tests(refusal);
        })
    };
    let unlimited = cold_read(u64::MAX, false);
    assert!(!unlimited.partial, "{unlimited:?}");
    let allowance = unlimited.bytes / 2;
    let default_off = cold_read(allowance, false);
    assert!(
        !default_off.partial && default_off.answer == unlimited.answer,
        "the rail is off by default: the distribution completes: {default_off:?}"
    );
    assert!(
        default_off.bytes > allowance,
        "its footprint is still charged: {default_off:?}"
    );
    let enabled = cold_read(allowance, true);
    assert!(
        enabled.partial
            && enabled
                .reasons
                .contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT),
        "the enabled rail refuses the construction: {enabled:?}"
    );
}

/// An explicit invalidation of a canonical drops every sealed refusal whose
/// failed prefix consumed a result reading it — through receipts no cache
/// holds — together with the memo candidates those receipts reach, and
/// leaves refusals and candidates that never read it. The last release
/// drains both reverse indexes.
#[test]
fn invalidating_a_canonical_drops_the_refusals_its_failed_prefix_reached() {
    use verter_session_query::facts::fact_cache::{
        FactVersionRef, ReadSetSignature, ResultReceipt,
    };
    use verter_type_engine::semantic_query::{
        CacheRead, LiteralValue, PartialReasonSet, QueryResult, RelateMemoKey, RelationContext,
        RelationOutcome, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticQueryKey,
        SemanticQueryValue,
    };
    use verter_type_engine::semantic_query_memo::ReverseIndexOccupancy;

    let host = host();
    let store = verter_type_engine::semantic_query_memo::SemanticGraphStore::new();
    let profile = BudgetProfile::intern(BudgetProfileSpec {
        work: 64,
        bytes: 1_024,
        construction_byte_refusal: false,
        query_depth: 24,
        instantiation_depth: 10,
        tail_steps: 10,
        request_operations: 0,
        relation_comparisons: 600,
        cost_model_revision: COST_MODEL_REVISION,
    });
    let key = |name: &str| {
        SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: ScopeId {
                canonical_id: verter_type_expr::OwnedStr::from("/refusals.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                local_scope: None,
                binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
            },
            name: verter_type_expr::OwnedStr::from(name),
        })
    };
    let refusal = || {
        CacheRead::unleased_for_tests(
            QueryResult::<SemanticQueryValue>::Error(
                verter_type_engine::semantic_query::QueryError::Miss,
            ),
            verter_type_engine::semantic_query_memo::empty_signature(),
            verter_type_engine::project_semantic_dispatch::walk::WalkerDiagnostics::default(),
            true,
            verter_type_engine::semantic_query::FactStatus::unavailable(
                PartialReasonSet::PROJECTION_WORK_LIMIT,
            ),
            verter_type_engine::semantic_query::ReadReceipt::Unpriced,
        )
    };
    let file = |canonical: &str| FactVersionRef::FileWholeHash {
        canonical_id: canonical.into(),
        hash: [9; 16],
    };
    // The failed prefix consumed `mid`, which consumed `base`; neither is
    // cached.
    let base = ResultReceipt::new(vec![file("/w/base.ts")]);
    let mid = ResultReceipt::new(vec![file("/w/mid.ts"), FactVersionRef::Receipt(base)]);
    let through_prefix =
        || ReadSetSignature::new(Arc::from(vec![FactVersionRef::Receipt(mid.clone())]));
    let elsewhere = ReadSetSignature::new(Arc::from(vec![file("/w/other.ts")]));

    let entry = store.refusal_entry_for_tests(&host);
    assert!(store.seal_refusal_for_tests(
        &host,
        key("Prefixed"),
        profile.clone(),
        entry,
        refusal(),
        through_prefix(),
    ));
    assert!(store.seal_refusal_for_tests(
        &host,
        key("Elsewhere"),
        profile.clone(),
        entry,
        refusal(),
        elsewhere,
    ));
    let source = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "source".into(),
    )));
    let target = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "target".into(),
    )));
    store.insert_relation_payload_for_tests(
        RelateMemoKey::assignable(source, target, RelationContext::default()),
        through_prefix(),
        Arc::from(Vec::new()),
        store.relation_payload_for_tests(RelationOutcome::Assignable),
        0,
    );
    assert_eq!(store.refusal_summary_count_for_tests(), 2);
    assert_eq!(store.canonical_to_entries_count("/w/base.ts"), 1);

    assert_eq!(store.invalidate_canonical("/w/base.ts"), 1);
    let kept = store.refusal_carriers_for_tests();
    assert_eq!(kept.len(), 1, "only the refusal that read the edit leaves");
    assert!(kept[0]
        .facts
        .iter()
        .any(|fact| fact.canonical_id() == Some("/w/other.ts")));
    assert_eq!(
        store.reverse_index_occupancy(),
        ReverseIndexOccupancy::default()
    );
    assert_eq!(store.refusal_index_occupancy_for_tests().canonicals, 1);

    assert_eq!(store.invalidate_canonical("/w/other.ts"), 0);
    assert_eq!(store.refusal_summary_count_for_tests(), 0);
    assert_eq!(
        store.refusal_index_occupancy_for_tests(),
        ReverseIndexOccupancy::default()
    );

    // An evaluation in flight across an explicit invalidation read what the
    // invalidation dropped, before the index could hold its summary: its
    // refusal is never sealed. One entering after the invalidation is.
    let before_invalidation = store.refusal_entry_for_tests(&host);
    assert_eq!(store.invalidate_canonical("/w/base.ts"), 0);
    assert!(
        !store.seal_refusal_for_tests(
            &host,
            key("Prefixed"),
            profile.clone(),
            before_invalidation,
            refusal(),
            through_prefix(),
        ),
        "a refusal entered before an invalidation is never sealed after it"
    );
    assert_eq!(store.refusal_summary_count_for_tests(), 0);
    assert_eq!(
        store.refusal_index_occupancy_for_tests(),
        ReverseIndexOccupancy::default()
    );
    assert!(store.seal_refusal_for_tests(
        &host,
        key("Prefixed"),
        profile.clone(),
        store.refusal_entry_for_tests(&host),
        refusal(),
        through_prefix(),
    ));
}

/// `{ ...a, ...b }` over two `arms`-member object unions, and an index
/// target every alternative relates to.
fn spread_unions(arms: usize) -> String {
    let union = |prefix: &str| {
        (0..arms)
            .map(|i| format!("{{ {prefix}{i}: {i} }}"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    format!(
        "type A = {};\ntype B = {};\ndeclare const a: A;\ndeclare const b: B;\n\
         const s = {{ ...a, ...b }};\ntype T = {{ [key: string]: number }};\n",
        union("a"),
        union("b")
    )
}

/// The relation of the spread `s` of [`spread_unions`] to `T`, as a root key.
fn spread_relation_key(
    host: &Arc<VerterHost>,
    source: &str,
) -> verter_type_engine::semantic_query::SemanticQueryKey {
    super::checker_probe_lane_tests::with_probe_on_host(
        host,
        Default::default(),
        source,
        "[typeof s, T]",
        |dispatch, node| {
            let elements = match dispatch.graph().node_data(node).as_deref() {
                Some(verter_type_engine::semantic_query::SemanticNodeData::Tuple {
                    elements,
                    ..
                }) => elements
                    .iter()
                    .map(|element| element.value)
                    .collect::<Vec<_>>(),
                other => panic!("the probe reads the pair [typeof s, T], got {other:?}"),
            };
            assert!(
                matches!(
                    dispatch.graph().node_data(elements[0]).as_deref(),
                    Some(
                        verter_type_engine::semantic_query::SemanticNodeData::ObjectSpreadProgram(
                            _
                        )
                    )
                ),
                "the source is the factored construction program"
            );
            dispatch
                .relate_key_for(elements[0], elements[1])
                .to_query_key()
        },
    )
}

/// A demand that cannot pay to read a spread's correlated alternatives
/// stops as a typed work refusal, never as a smaller or empty product. The
/// refusal is sealed for its isolated root and allowances only: an exact
/// repeat answers it without reconstructing anything, a demand inside a
/// different enclosing ledger evaluates for itself, a higher allowance
/// completes, and an edit to the construction evaluates afresh.
#[test]
fn a_spread_work_refusal_is_sealed_per_isolated_root_and_heals() {
    use verter_type_engine::semantic_query::PartialReasonSet;
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let source = spread_unions(20);
    let measured = {
        let host = super::checker_probe_lane_tests::default_probe_host();
        let key = spread_relation_key(&host, &source);
        root_read(&host, &key, full)
    };
    assert!(
        !measured.partial,
        "the full allowance relates every alternative: {measured:?}"
    );

    let host = super::checker_probe_lane_tests::default_probe_host();
    let key = spread_relation_key(&host, &source);
    let limited = measured.work / 2;
    let refused = root_read(&host, &key, limited);
    assert!(
        refused.partial
            && refused
                .reasons
                .contains(PartialReasonSet::PROJECTION_WORK_LIMIT),
        "half the work cannot read every alternative: {refused:?}"
    );
    assert_ne!(
        refused.answer, measured.answer,
        "a refusal never answers as the complete product"
    );

    let before = semantic_misses(&host);
    assert_eq!(root_read(&host, &key, limited), refused);
    assert_eq!(
        semantic_misses(&host),
        before,
        "the exact repeat answers its sealed refusal without reconstruction"
    );

    let before = semantic_misses(&host);
    let enclosed = root_read_with_context(&host, &key, limited, 24, 1);
    assert!(enclosed.partial);
    assert!(
        semantic_misses(&host) > before,
        "a demand inside another ledger never inherits the refusal"
    );

    let healed = root_read(&host, &key, full);
    assert_eq!(
        (healed.answer.as_str(), healed.partial),
        (measured.answer.as_str(), false),
        "a higher allowance recomputes and completes"
    );

    let _ = spread_relation_key(&host, &source.replace("a0: 0", "a0: 10"));
    let before = semantic_misses(&host);
    let _ = root_read(&host, &key, limited);
    assert!(
        semantic_misses(&host) > before,
        "an edit to the construction evaluates afresh"
    );
}

/// Unions `A` of 100 and `B` of 80 string literals.
fn template_unions() -> String {
    let union = |name: &str, prefix: &str, count: usize| {
        let arms: Vec<String> = (0..count).map(|i| format!("\"{prefix}{i}\"")).collect();
        format!("type {name} = {};\n", arms.join(" | "))
    };
    format!("{}{}", union("A", "a", 100), union("B", "b", 80))
}

/// The reduction of `${A}-${B}` over [`template_unions`] as a root key.
fn template_reduction_key(
    host: &Arc<VerterHost>,
    source: &str,
) -> verter_type_engine::semantic_query::SemanticQueryKey {
    super::checker_probe_lane_tests::with_probe_on_host(
        host,
        Default::default(),
        source,
        "[A, B]",
        |dispatch, node| {
            let args = match dispatch.graph().node_data(node).as_deref() {
                Some(verter_type_engine::semantic_query::SemanticNodeData::Tuple {
                    elements,
                    ..
                }) => elements
                    .iter()
                    .map(|element| element.value)
                    .collect::<Vec<_>>(),
                other => panic!("the probe reads the pair [A, B], got {other:?}"),
            };
            verter_type_engine::semantic_query::SemanticQueryKey::TemplateLiteralReduce {
                pattern: verter_type_expr::OwnedSlice::<verter_type_expr::OwnedStr>::from([
                    verter_type_expr::OwnedStr::from(""),
                    verter_type_expr::OwnedStr::from("-"),
                    verter_type_expr::OwnedStr::from(""),
                ]),
                args: Arc::from(args),
                context: dispatch.template_literal_reduce_context(),
            }
        },
    )
}

/// A template whose product the work allowance cannot pay for stops as a
/// typed work refusal, never as a smaller or empty union. The refusal is
/// sealed for its isolated root and allowances only: an exact repeat
/// answers it without reconstructing anything, a demand inside a different
/// enclosing ledger evaluates for itself, a higher allowance completes the
/// 8,000-arm union, and an edit to a span evaluates afresh.
#[test]
fn a_template_work_refusal_is_sealed_per_isolated_root_and_heals() {
    use verter_type_engine::semantic_query::PartialReasonSet;
    let full = super::connected_demand::MAX_CONNECTED_PROJECTION_WORK;
    let source = template_unions();
    let measured = {
        let host = super::checker_probe_lane_tests::default_probe_host();
        let key = template_reduction_key(&host, &source);
        root_read(&host, &key, full)
    };
    assert!(
        !measured.partial && measured.work >= 8_000,
        "the full allowance builds all 8,000 concatenations: {measured:?}"
    );

    let host = super::checker_probe_lane_tests::default_probe_host();
    let key = template_reduction_key(&host, &source);
    let limited = measured.work / 2;
    let refused = root_read(&host, &key, limited);
    assert!(
        refused.partial
            && refused
                .reasons
                .contains(PartialReasonSet::PROJECTION_WORK_LIMIT),
        "half the work cannot build every concatenation: {refused:?}"
    );

    let before = semantic_misses(&host);
    assert_eq!(root_read(&host, &key, limited), refused);
    assert_eq!(
        semantic_misses(&host),
        before,
        "the exact repeat answers its sealed refusal without reconstruction"
    );

    let before = semantic_misses(&host);
    let enclosed = root_read_with_context(&host, &key, limited, 24, 1);
    assert!(enclosed.partial);
    assert!(
        semantic_misses(&host) > before,
        "a demand inside another ledger never inherits the refusal"
    );

    let healed = root_read(&host, &key, full);
    assert!(
        !healed.partial && healed.work >= 8_000,
        "a higher allowance recomputes and completes: {healed:?}"
    );

    let edited = template_reduction_key(&host, &source.replace("\"a0\"", "\"z0\""));
    let before = semantic_misses(&host);
    let _ = root_read(&host, &edited, limited);
    assert!(
        semantic_misses(&host) > before,
        "an edit to a span evaluates afresh"
    );
}

/// `{ ...a, ...b }` over 33- and 33-member object unions has 1,089
/// correlated constituents — each pairs one `a` arm with one `b` arm, never
/// an `a` key with another arm's value — and inference over the spread
/// reads every constituent.
#[test]
fn a_33_by_33_spread_publishes_every_correlated_constituent_and_infers_from_each() {
    let union = |prefix: &str| {
        (0..33)
            .map(|i| format!("{{ {prefix}{i}: {i} }}"))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let declarations = format!(
        "type A = {};\ntype B = {};\ndeclare const a: A;\ndeclare const b: B;\n\
         const s = {{ ...a, ...b }};\n",
        union("a"),
        union("b")
    );
    let render = |probe: &str| {
        let host = bench_host(
            0,
            &format!(
                "{declarations}type Values<T> = [T] extends [{{ [key: string]: infer V }}] ? V : never;\n\
                 type __Probe = {probe};\nexport {{}};\n"
            ),
        );
        let bytes = probe_answer(&host).expect("the probe answers");
        let expr: verter_type_expr::TypeExpr =
            serde_json::from_slice(&bytes).expect("the answer decodes");
        verter_type_expr::render_type_expr_display(&expr)
            .expect("the answer renders")
            .text
    };

    let whole = render("typeof s");
    let mut pairs = std::collections::BTreeSet::new();
    for arm in whole.split(" | ") {
        let members: Vec<(String, String)> = arm
            .trim_matches(|c: char| c == '{' || c == '}' || c.is_whitespace())
            .split(';')
            .map(str::trim)
            .filter(|member| !member.is_empty())
            .map(|member| {
                let (name, value) = member.split_once(':').expect("a property");
                (name.trim().to_owned(), value.trim().to_owned())
            })
            .collect();
        let [(a_name, a_value), (b_name, b_value)] = members.as_slice() else {
            panic!("each constituent copies one arm of each union: {arm}");
        };
        assert_eq!(a_name, &format!("a{a_value}"), "correlated a in {arm}");
        assert_eq!(b_name, &format!("b{b_value}"), "correlated b in {arm}");
        pairs.insert((a_value.clone(), b_value.clone()));
    }
    assert_eq!(
        pairs.len(),
        33 * 33,
        "every pair of arms is one constituent"
    );

    let inferred = render("Values<typeof s>");
    let values: std::collections::BTreeSet<String> =
        inferred.split(" | ").map(str::to_owned).collect();
    assert_eq!(
        values,
        (0..33).map(|i: usize| i.to_string()).collect(),
        "inference reads every value of every constituent: {inferred}"
    );
}

/// A stress-size spread also reads through an inference (one test per
/// size, so each stays inside the 20-second speed gate). `Values<typeof
/// s>` unions the values of every constituent, and a later direct write
/// over one arm's key replaces that arm's value in every constituent:
/// the inferred set is exactly the surviving arm values plus the write's
/// literal, so a lost constituent, a missed write or a write that
/// appended instead of replacing all show up here.
fn assert_stress_size_spread_infers_every_constituent_and_the_later_write(widths: &[usize]) {
    let declarations = widths
        .iter()
        .zip(["A", "B", "C"].iter().zip(["a", "b", "c"]))
        .map(|(width, (type_name, name))| {
            let union = (0..*width)
                .map(|i| format!("{{ {name}{i}: {i} }}"))
                .collect::<Vec<_>>()
                .join(" | ");
            format!("type {type_name} = {union};\ndeclare const {name}: {type_name};\n")
        })
        .collect::<String>();
    let spreads = ["a", "b", "c"]
        .iter()
        .take(widths.len())
        .map(|name| format!("...{name}"))
        .chain([format!("a{}: 4242 as const", widths[0] - 1)])
        .collect::<Vec<_>>()
        .join(", ");
    let host = bench_host(
        0,
        &format!(
            "{declarations}const s = {{ {spreads} }};\n\
             type Values<T> = [T] extends [{{ [key: string]: infer V }}] ? V : never;\n\
             type __Probe = Values<typeof s>;\nexport {{}};\n"
        ),
    );
    let bytes = probe_answer(&host).expect("the stress probe answers");
    let expr: verter_type_expr::TypeExpr =
        serde_json::from_slice(&bytes).expect("the answer decodes");
    let inferred = verter_type_expr::render_type_expr_display(&expr)
        .expect("the answer renders")
        .text;

    let values: std::collections::BTreeSet<String> =
        inferred.split(" | ").map(str::to_owned).collect();
    let overwritten = widths[0] - 1;
    let mut expected: std::collections::BTreeSet<String> = widths
        .iter()
        .flat_map(|width| (0..*width).map(|i| i.to_string()).collect::<Vec<_>>())
        .collect();
    // The write replaces the arm's own value everywhere; it leaves the
    // inferred set only when no other union writes the same value.
    if !widths[1..].iter().any(|width| overwritten < *width) {
        expected.remove(&overwritten.to_string());
    }
    expected.insert("4242".to_owned());
    assert_eq!(
        values, expected,
        "{widths:?}: inference reads every surviving value and the later write: {inferred}"
    );
}

#[test]
fn a_369_by_271_spread_infers_every_constituent_and_the_later_write() {
    assert_stress_size_spread_infers_every_constituent_and_the_later_write(&[369, 271]);
}

#[test]
fn a_50_by_50_by_39_spread_infers_every_constituent_and_the_later_write() {
    assert_stress_size_spread_infers_every_constituent_and_the_later_write(&[50, 50, 39]);
}
