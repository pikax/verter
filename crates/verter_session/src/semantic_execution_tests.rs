//! The continuation runtime on synthetic programs: stack-independent depth,
//! execution-local reuse, cycles, tail transitions, outside producers,
//! failures, stops and deterministic order.

use std::rc::Rc;

use super::{DemandState, EvalStep, External, Outcome, Program, SemanticExecution, Start};

/// A frame of the chain program: `n` sums the values of the demands it
/// needs, one at a time, then completes.
struct ChainFrame {
    n: u64,
    needs: Vec<u64>,
    next: usize,
    total: u64,
    _alive: Option<Rc<()>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Structural,
}

/// A demand `n` needs the demands `edges(n)` in order and completes with
/// `n + sum(values)`. Cycles answer `CYCLE`.
struct ChainProgram<E: Fn(u64) -> Vec<u64>> {
    edges: E,
    reusable: bool,
    trace: Vec<(u64, Option<Result<u64, &'static str>>)>,
    starts: Vec<u64>,
    cycles: Vec<u64>,
    stop_after_steps: Option<u64>,
    steps: u64,
    alive: Option<Rc<()>>,
    fail: Option<u64>,
    outside: Option<u64>,
    outside_restarts: u32,
    answered: Option<u64>,
}

const CYCLE: u64 = 1_000_000_000;

impl<E: Fn(u64) -> Vec<u64>> ChainProgram<E> {
    fn new(edges: E) -> Self {
        Self {
            edges,
            reusable: true,
            trace: Vec::new(),
            starts: Vec::new(),
            cycles: Vec::new(),
            stop_after_steps: None,
            steps: 0,
            alive: None,
            fail: None,
            outside: None,
            outside_restarts: 0,
            answered: None,
        }
    }
}

impl<E: Fn(u64) -> Vec<u64>> Program for ChainProgram<E> {
    type Demand = u64;
    type Frame = ChainFrame;
    type Value = u64;
    type Failure = &'static str;
    type Role = Role;
    type Subscription = u64;

    fn start(&mut self, demand: &u64) -> Start<Self> {
        self.starts.push(*demand);
        if self.outside == Some(*demand) {
            return Start::Subscribe(*demand);
        }
        if self.answered == Some(*demand) {
            return Start::Answer(Ok(*demand * 7));
        }
        Start::Produce(ChainFrame {
            n: *demand,
            needs: (self.edges)(*demand),
            next: 0,
            total: *demand,
            _alive: self.alive.clone(),
        })
    }

    fn step(&mut self, frame: &mut ChainFrame, delivery: Option<Outcome<Self>>) -> EvalStep<Self> {
        self.steps += 1;
        if self.trace.len() < 64 {
            self.trace.push((frame.n, delivery));
        }
        match delivery {
            Some(Ok(value)) => frame.total += value,
            Some(Err(failure)) => return EvalStep::Incomplete(failure),
            None => {}
        }
        if self.fail == Some(frame.n) {
            return EvalStep::Incomplete("failed");
        }
        if let Some(&demand) = frame.needs.get(frame.next) {
            frame.next += 1;
            return EvalStep::Need {
                demand,
                role: Role::Structural,
            };
        }
        EvalStep::Complete(frame.total)
    }

    fn close_cycle(&mut self, demand: &u64, role: Role) -> Outcome<Self> {
        assert_eq!(role, Role::Structural);
        self.cycles.push(*demand);
        Ok(CYCLE)
    }

    fn reusable(&self, _demand: &u64, _value: &u64) -> bool {
        self.reusable
    }

    fn wait(&mut self, subscription: u64) -> External<Self> {
        if self.outside_restarts > 0 {
            self.outside_restarts -= 1;
            self.outside = None;
            return External::Restart;
        }
        External::Complete(Ok(subscription * 10))
    }

    fn stop(&self) -> Option<&'static str> {
        self.stop_after_steps
            .filter(|limit| self.steps >= *limit)
            .map(|_| "stopped")
    }
}

fn on_small_stack<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(body)
        .expect("spawn a 1 MiB thread")
        .join()
        .expect("the 1 MiB thread completed")
}

/// A chain of 200 000 demands, each needing the next, completes on a 1 MiB
/// thread: dependency depth is frame state, never native stack.
#[test]
fn a_deep_dependency_chain_runs_on_a_one_mebibyte_stack() {
    let (outcome, counters) = on_small_stack(|| {
        const DEPTH: u64 = 200_000;
        let mut program = ChainProgram::new(|n| if n < DEPTH { vec![n + 1] } else { vec![] });
        let mut execution = SemanticExecution::new();
        let outcome = execution.drive(&mut program, 0);
        (outcome, execution.counters())
    });
    let depth: u64 = 200_000;
    assert_eq!(outcome, Ok(depth * (depth + 1) / 2));
    assert_eq!(counters.starts, depth + 1);
    assert_eq!(counters.peak_frames, (depth + 1) as usize);
    assert_eq!(counters.cycles, 0);
}

/// Every level needs the level below twice: the second need is answered from
/// the execution's completed result, so the work is linear, not exponential.
#[test]
fn a_shared_demand_completes_once_per_execution() {
    let mut program = ChainProgram::new(|n| if n < 20 { vec![n + 1, n + 1] } else { vec![] });
    let mut execution = SemanticExecution::new();
    let outcome = execution.drive(&mut program, 0);
    // value(20) = 20; value(n) = n + 2 * value(n + 1).
    let mut expected = 20u64;
    for n in (0..20).rev() {
        expected = n + 2 * expected;
    }
    assert_eq!(outcome, Ok(expected));
    assert_eq!(execution.counters().starts, 21);
    assert_eq!(execution.counters().reuses, 20);
    assert_eq!(execution.demand_state(&7), Some(DemandState::Complete));
}

/// A completion the program does not declare reusable is delivered and
/// forgotten: the next need starts the demand again.
#[test]
fn a_completion_that_is_not_reusable_is_started_again() {
    let mut program = ChainProgram::new(|n| if n < 3 { vec![n + 1, n + 1] } else { vec![] });
    program.reusable = false;
    let mut execution = SemanticExecution::new();
    let outcome = execution.drive(&mut program, 0);
    assert_eq!(outcome, Ok(2 * (1 + 2 * (2 + 2 * 3))));
    // 1 + 2 + 4 + 8 starts: every need recomputes.
    assert_eq!(execution.counters().starts, 15);
    assert_eq!(execution.counters().reuses, 0);
    assert_eq!(execution.demand_state(&0), None);
}

/// A demand the program answers when it starts completes without a frame,
/// and its reusable answer serves later needs in the execution.
#[test]
fn a_demand_answered_at_its_start_completes_without_a_frame() {
    let mut program = ChainProgram::new(|n| if n < 3 { vec![3, 3] } else { vec![] });
    program.answered = Some(3);
    let mut execution = SemanticExecution::new();
    assert_eq!(execution.drive(&mut program, 0), Ok(42));
    assert_eq!(program.starts, vec![0, 3]);
    assert_eq!(execution.counters().reuses, 1);
    assert_eq!(execution.counters().peak_frames, 1);
}

/// A need of an open ancestor closes a cycle: the program answers the needing
/// frame, and the ancestor keeps evaluating to its own result.
#[test]
fn a_need_of_an_open_ancestor_closes_a_cycle_for_the_needing_frame_only() {
    let mut program = ChainProgram::new(|n| match n {
        0 => vec![1],
        1 => vec![2],
        2 => vec![0],
        _ => vec![],
    });
    let mut execution = SemanticExecution::new();
    let outcome = execution.drive(&mut program, 0);
    assert_eq!(outcome, Ok(1 + 2 + CYCLE));
    assert_eq!(program.cycles, vec![0]);
    assert_eq!(program.starts, vec![0, 1, 2]);
}

/// A frame that needs its own demand closes a cycle on itself.
#[test]
fn a_need_of_the_frames_own_demand_closes_a_cycle() {
    let mut program = ChainProgram::new(|n| if n == 5 { vec![5] } else { vec![] });
    let mut execution = SemanticExecution::new();
    assert_eq!(execution.drive(&mut program, 5), Ok(5 + CYCLE));
    assert_eq!(program.cycles, vec![5]);
}

/// An operational failure stops the failing demand and reaches its waiters
/// as a failure delivery; nothing about it is kept for reuse.
#[test]
fn a_failure_reaches_every_waiter_and_is_not_kept() {
    let mut program = ChainProgram::new(|n| if n < 10 { vec![n + 1] } else { vec![] });
    program.fail = Some(6);
    let mut execution = SemanticExecution::new();
    assert_eq!(execution.drive(&mut program, 0), Err("failed"));
    assert_eq!(execution.demand_state(&6), None);
    assert_eq!(execution.demand_state(&0), None);
    assert_eq!(execution.live_frames(), 0);
}

/// A stop abandons every open demand and drops every frame; completed
/// results stay and answer a later drive.
#[test]
fn a_stop_drops_every_frame_and_keeps_completed_results() {
    let alive = Rc::new(());
    let mut program = ChainProgram::new(|n| match n {
        0 => vec![100, 1],
        1 => vec![2],
        2 => vec![3],
        _ => vec![],
    });
    program.alive = Some(Rc::clone(&alive));
    // Steps: 0, 100, 0(resumed), 1, 2, 3 — stop before 3 completes.
    program.stop_after_steps = Some(5);
    let mut execution = SemanticExecution::new();
    assert_eq!(execution.drive(&mut program, 0), Err("stopped"));
    assert_eq!(execution.live_frames(), 0);
    program.alive = None;
    assert_eq!(Rc::strong_count(&alive), 1, "every frame was dropped");
    assert_eq!(execution.demand_state(&100), Some(DemandState::Complete));
    assert_eq!(execution.demand_state(&1), None);
    program.stop_after_steps = None;
    assert_eq!(execution.drive(&mut program, 100), Ok(100));
    assert_eq!(execution.drive(&mut program, 0), Ok(100 + 1 + 2 + 3));
}

/// A demand owned by an outside producer parks its waiter until the drive
/// has nothing else to run; a producer that goes away restarts the demand.
#[test]
fn an_outside_producer_is_awaited_and_a_vanished_one_restarts_the_demand() {
    let mut program = ChainProgram::new(|n| if n < 4 { vec![n + 1] } else { vec![] });
    program.outside = Some(3);
    let mut execution = SemanticExecution::new();
    assert_eq!(execution.drive(&mut program, 0), Ok(1 + 2 + 30));
    assert_eq!(execution.counters().external_waits, 1);

    let mut program = ChainProgram::new(|n| if n < 4 { vec![n + 1] } else { vec![] });
    program.outside = Some(3);
    program.outside_restarts = 1;
    let mut execution = SemanticExecution::new();
    assert_eq!(execution.drive(&mut program, 0), Ok(1 + 2 + 3 + 4));
    assert_eq!(program.starts, vec![0, 1, 2, 3, 3, 4]);
}

/// Tail transitions replace the running frame: a million replacements keep
/// one frame alive.
#[test]
fn a_tail_transition_replaces_the_frame_in_place() {
    struct Tail;
    impl Program for Tail {
        type Demand = ();
        type Frame = u32;
        type Value = u32;
        type Failure = ();
        type Role = ();
        type Subscription = ();
        fn start(&mut self, _: &()) -> Start<Self> {
            Start::Produce(0)
        }
        fn step(&mut self, frame: &mut u32, _: Option<Outcome<Self>>) -> EvalStep<Self> {
            if *frame == 1_000_000 {
                EvalStep::Complete(*frame)
            } else if frame.is_multiple_of(2) {
                EvalStep::Replace(*frame + 1)
            } else {
                *frame += 1;
                EvalStep::Yield
            }
        }
        fn close_cycle(&mut self, _: &(), _: ()) -> Outcome<Self> {
            unreachable!()
        }
        fn reusable(&self, _: &(), _: &u32) -> bool {
            true
        }
        fn wait(&mut self, _: ()) -> External<Self> {
            unreachable!()
        }
        fn stop(&self) -> Option<()> {
            None
        }
    }
    let mut execution = SemanticExecution::new();
    assert_eq!(execution.drive(&mut Tail, ()), Ok(1_000_000));
    assert_eq!(execution.counters().peak_frames, 1);
    assert_eq!(execution.counters().steps, 1_000_001);
}

/// The same program drives through the same steps with the same deliveries,
/// in the same order, every time.
#[test]
fn the_step_order_is_deterministic() {
    let run = || {
        let mut program = ChainProgram::new(|n| match n {
            0 => vec![1, 2, 1],
            1 => vec![3],
            2 => vec![3, 0],
            _ => vec![],
        });
        let mut execution = SemanticExecution::new();
        let outcome = execution.drive(&mut program, 0);
        (outcome, program.trace, program.starts, program.cycles)
    };
    let first = run();
    assert_eq!(first, run());
    assert_eq!(first.0, Ok((1 + 3) + (2 + 3 + CYCLE) + (1 + 3)));
    assert_eq!(first.2, vec![0, 1, 3, 2]);
    assert_eq!(first.3, vec![0]);
}

/// A frame's step never drives: a nested drive would put evaluation back on
/// the native stack.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "a frame step must not drive a nested execution")]
fn a_drive_inside_a_step_is_refused() {
    struct Nesting;
    impl Program for Nesting {
        type Demand = u8;
        type Frame = ();
        type Value = ();
        type Failure = ();
        type Role = ();
        type Subscription = ();
        fn start(&mut self, _: &u8) -> Start<Self> {
            Start::Produce(())
        }
        fn step(&mut self, _: &mut (), _: Option<Outcome<Self>>) -> EvalStep<Self> {
            let mut nested = SemanticExecution::<Nesting>::new();
            let _ = nested.drive(&mut Nesting, 1);
            EvalStep::Complete(())
        }
        fn close_cycle(&mut self, _: &u8, _: ()) -> Outcome<Self> {
            unreachable!()
        }
        fn reusable(&self, _: &u8, _: &()) -> bool {
            true
        }
        fn wait(&mut self, _: ()) -> External<Self> {
            unreachable!()
        }
        fn stop(&self) -> Option<()> {
            None
        }
    }
    let mut execution = SemanticExecution::new();
    let _ = execution.drive(&mut Nesting, 0);
}

/// A frame whose private children are spawned, not needed: `children`
/// copies of a child one level shallower, summed, plus one.
struct Spawner {
    children: u32,
    starts: u64,
}

struct SpawnFrame {
    depth: u32,
    spawned: u32,
    total: u64,
}

impl Program for Spawner {
    type Demand = u32;
    type Frame = SpawnFrame;
    type Value = u64;
    type Failure = ();
    type Role = ();
    type Subscription = ();
    fn start(&mut self, depth: &u32) -> Start<Self> {
        self.starts += 1;
        Start::Produce(SpawnFrame {
            depth: *depth,
            spawned: 0,
            total: 1,
        })
    }
    fn step(&mut self, frame: &mut SpawnFrame, delivery: Option<Outcome<Self>>) -> EvalStep<Self> {
        if let Some(outcome) = delivery {
            frame.total += outcome.expect("children complete");
        }
        if frame.depth > 0 && frame.spawned < self.children {
            frame.spawned += 1;
            return EvalStep::Spawn(SpawnFrame {
                depth: frame.depth - 1,
                spawned: 0,
                total: 1,
            });
        }
        EvalStep::Complete(frame.total)
    }
    fn close_cycle(&mut self, _: &u32, _: ()) -> Outcome<Self> {
        unreachable!("private children never close a cycle")
    }
    fn reusable(&self, _: &u32, _: &u64) -> bool {
        true
    }
    fn wait(&mut self, _: ()) -> External<Self> {
        unreachable!()
    }
    fn stop(&self) -> Option<()> {
        None
    }
}

/// Spawned children are the caller's own sub-computations held as frames: a
/// chain of 200 000 of them completes on a 1 MiB thread, and equal children
/// are computed each time — never shared, never started as demands.
#[test]
fn spawned_children_are_private_frames_that_never_touch_the_native_stack() {
    let (chain, chain_counters) = on_small_stack(|| {
        let mut program = Spawner {
            children: 1,
            starts: 0,
        };
        let mut execution = SemanticExecution::new();
        let outcome = execution.drive(&mut program, 200_000);
        (outcome, execution.counters())
    });
    assert_eq!(chain, Ok(200_001));
    assert_eq!(chain_counters.starts, 1);
    assert_eq!(chain_counters.peak_frames, 200_001);

    let mut program = Spawner {
        children: 2,
        starts: 0,
    };
    let mut execution = SemanticExecution::new();
    assert_eq!(execution.drive(&mut program, 10), Ok((1 << 11) - 1));
    assert_eq!(program.starts, 1, "only the root is a demand");
    assert_eq!(execution.counters().reuses, 0);
    assert_eq!(execution.live_frames(), 0);
}

/// A program whose root needs `joins` demands in turn, each owned by a
/// producer outside the execution: every one is joined, and a join may
/// first find its producer gone `restarts` times, restarting the demand.
struct Joins {
    joins: u64,
    restarts: u32,
    left: std::collections::HashMap<u64, u32>,
    /// Stop the drive once this many subscriptions have been started.
    stop_after_subscribes: Option<u64>,
    subscribes: u64,
    /// Held by every live subscription.
    alive: Rc<()>,
}

impl Joins {
    fn new(joins: u64, restarts: u32) -> Self {
        Self {
            joins,
            restarts,
            left: Default::default(),
            stop_after_subscribes: None,
            subscribes: 0,
            alive: Rc::new(()),
        }
    }
}

struct JoinFrame {
    next: u64,
    total: u64,
}

impl Program for Joins {
    type Demand = u64;
    type Frame = JoinFrame;
    type Value = u64;
    type Failure = ();
    type Role = ();
    type Subscription = (u64, Rc<()>);

    fn start(&mut self, demand: &u64) -> Start<Self> {
        if demand.is_multiple_of(1_000_000) {
            return Start::Produce(JoinFrame {
                next: *demand + 1,
                total: 0,
            });
        }
        self.subscribes += 1;
        Start::Subscribe((*demand, Rc::clone(&self.alive)))
    }

    fn step(&mut self, frame: &mut JoinFrame, delivery: Option<Outcome<Self>>) -> EvalStep<Self> {
        if let Some(value) = delivery {
            frame.total += value.expect("a join completes");
        }
        if frame.next % 1_000_000 <= self.joins {
            let demand = frame.next;
            frame.next += 1;
            return EvalStep::Need { demand, role: () };
        }
        EvalStep::Complete(frame.total)
    }

    fn close_cycle(&mut self, _: &u64, _: ()) -> Outcome<Self> {
        unreachable!("joins never close a cycle")
    }

    fn reusable(&self, _: &u64, _: &u64) -> bool {
        true
    }

    fn wait(&mut self, (subscription, _alive): (u64, Rc<()>)) -> External<Self> {
        let left = self.left.entry(subscription).or_insert(self.restarts);
        if *left > 0 {
            *left -= 1;
            return External::Restart;
        }
        External::Complete(Ok(1))
    }

    fn stop(&self) -> Option<()> {
        self.stop_after_subscribes
            .is_some_and(|limit| self.subscribes >= limit)
            .then_some(())
    }
}

/// Joining outside producers in turn inspects one subscription per wait,
/// however many joins came before: at most one subscription is live (the
/// chain's tip waits on it), and a finished one leaves nothing to skip. So
/// do restarted joins within one execution, and drives after the first on
/// the same execution.
#[test]
fn each_join_of_an_outside_producer_inspects_one_subscription() {
    for joins in [128, 256, 512, 1_024] {
        // Successive joins within one execution.
        let mut program = Joins::new(joins, 0);
        let mut execution = SemanticExecution::new();
        assert_eq!(execution.drive(&mut program, 0), Ok(joins));
        let counters = execution.counters();
        assert_eq!(counters.external_waits, joins);
        assert_eq!(
            counters.subscription_inspections, joins,
            "{joins} joins in one execution"
        );

        // Each join first restarted twice: repeated waits on one demand.
        let mut program = Joins::new(joins, 2);
        let mut execution = SemanticExecution::new();
        assert_eq!(execution.drive(&mut program, 0), Ok(joins));
        let counters = execution.counters();
        assert_eq!(counters.external_waits, 3 * joins);
        assert_eq!(
            counters.subscription_inspections,
            3 * joins,
            "{joins} restarted joins in one execution"
        );

        // Four drives on one execution, each joining its own producers.
        let mut program = Joins::new(joins, 0);
        let mut execution = SemanticExecution::new();
        for drive in 0..4 {
            assert_eq!(execution.drive(&mut program, drive * 1_000_000), Ok(joins));
        }
        let counters = execution.counters();
        assert_eq!(counters.external_waits, 4 * joins);
        assert_eq!(
            counters.subscription_inspections,
            4 * joins,
            "{joins} joins in each of four drives on one execution"
        );

        // A fresh execution per drive is reset: the same count each time.
        for drive in 0..4 {
            let mut execution = SemanticExecution::new();
            assert_eq!(execution.drive(&mut program, drive * 1_000_000), Ok(joins));
            assert_eq!(execution.counters().subscription_inspections, joins);
        }
    }
}

/// A stop while the chain waits on an outside producer drops that
/// subscription with every frame; the next drive on the execution joins
/// its own producers from a clean slate, and the demand the stop abandoned
/// is started again, not answered from a stale subscription.
#[test]
fn a_stop_while_joining_drops_the_subscription() {
    let mut program = Joins::new(8, 0);
    // Stop with the fourth join subscribed and not yet waited on.
    program.stop_after_subscribes = Some(4);
    let mut execution = SemanticExecution::new();
    assert_eq!(execution.drive(&mut program, 0), Err(()));
    assert_eq!(execution.live_frames(), 0);
    assert_eq!(
        Rc::strong_count(&program.alive),
        1,
        "no subscription outlives the stop"
    );
    assert_eq!(
        execution.demand_state(&4),
        None,
        "the subscribed demand is abandoned"
    );
    assert_eq!(execution.demand_state(&3), Some(DemandState::Complete));

    program.stop_after_subscribes = None;
    assert_eq!(execution.drive(&mut program, 0), Ok(8));
    let counters = execution.counters();
    assert_eq!(counters.external_waits, 3 + 5);
    assert_eq!(counters.subscription_inspections, counters.external_waits);
}
