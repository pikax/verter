//! Resolver flights are led and joined by execution tasks: a suspended
//! leader keeps its lane across polls, another task on the same thread
//! follows it instead of computing again, a task re-entering its own lane
//! or closing a wait cycle computes inline, and dropping a suspended
//! leader or follower releases what it held.

use std::future::Future;
use std::pin::{pin, Pin};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use verter_execution::tasks::{ExecutionScope, ExecutionTask, TaskRegistry};

use super::{SingleflightGroup, SingleflightRole, SingleflightRunResult, StoreViewCompatToken};

struct Noop;

impl Wake for Noop {
    fn wake(self: Arc<Self>) {}
}

fn waker() -> Waker {
    Waker::from(Arc::new(Noop))
}

fn token() -> StoreViewCompatToken {
    StoreViewCompatToken {
        epoch: 0,
        session: None,
        validity_fingerprint: 0,
    }
}

/// Holds a leader's compute until released.
#[derive(Clone, Default)]
struct Gate(Arc<parking_lot::Mutex<(bool, Option<Waker>)>>);

impl Gate {
    fn release(&self) {
        let waker = {
            let mut state = self.0.lock();
            state.0 = true;
            state.1.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    async fn passed(&self) {
        std::future::poll_fn(|cx| {
            let mut state = self.0.lock();
            if state.0 {
                Poll::Ready(())
            } else {
                state.1 = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await;
    }
}

type Group = SingleflightGroup<&'static str, usize, ()>;
type Answer = Result<SingleflightRunResult<usize>, ()>;

/// One flight claim computing `value` once `gate` passes, counting computes.
fn claim<'g>(
    group: &'g Group,
    key: &'static str,
    gate: Gate,
    computes: &'g AtomicUsize,
    value: usize,
) -> impl Future<Output = Answer> + 'g {
    group.run_retaining_pollable(
        key,
        token(),
        move || async move {
            gate.passed().await;
            computes.fetch_add(1, Ordering::SeqCst);
            Ok(value)
        },
        |_| true,
    )
}

/// Poll `flight` once as `task`.
fn poll_as<F: Future>(
    registry: &TaskRegistry,
    task: &ExecutionTask,
    flight: Pin<&mut F>,
) -> Poll<F::Output> {
    let _task = ExecutionScope::install(registry, task);
    flight.poll(&mut Context::from_waker(&waker()))
}

fn answered(poll: Poll<Answer>) -> (SingleflightRole, usize) {
    match poll {
        Poll::Ready(Ok(result)) => (result.role, *result.value),
        Poll::Ready(Err(())) => panic!("the flight failed"),
        Poll::Pending => panic!("the flight is still pending"),
    }
}

/// Two tasks polled on one thread share one leader: the second follows the
/// suspended first instead of computing again, and both answer its value.
#[test]
fn a_second_task_on_the_same_thread_follows_a_suspended_leader() {
    let registry = TaskRegistry::default();
    let group = Group::with_task_registry(registry.clone());
    let (leading, following) = (registry.register_task(), registry.register_task());
    let (gate, computes) = (Gate::default(), AtomicUsize::new(0));
    let mut leader = pin!(claim(&group, "k", gate.clone(), &computes, 1));
    let mut follower = pin!(claim(&group, "k", Gate::default(), &computes, 2));
    assert!(poll_as(&registry, &leading, leader.as_mut()).is_pending());
    assert!(poll_as(&registry, &following, follower.as_mut()).is_pending());
    assert_eq!(
        registry.wait_count_for_tests(),
        1,
        "the follower waits on the leader"
    );
    gate.release();
    assert_eq!(
        answered(poll_as(&registry, &leading, leader.as_mut())),
        (SingleflightRole::Leader, 1)
    );
    assert_eq!(
        answered(poll_as(&registry, &following, follower.as_mut())),
        (SingleflightRole::Follower, 1)
    );
    assert_eq!(computes.load(Ordering::SeqCst), 1);
    assert_eq!(registry.wait_count_for_tests(), 0);
}

/// The task that leads a suspended flight and claims it again — same-path
/// re-entry — computes inline as a nested leader instead of waiting on
/// itself.
#[test]
fn a_task_re_entering_its_own_suspended_flight_computes_inline() {
    let registry = TaskRegistry::default();
    let group = Group::with_task_registry(registry.clone());
    let task = registry.register_task();
    let (gate, computes) = (Gate::default(), AtomicUsize::new(0));
    let mut leader = pin!(claim(&group, "k", gate.clone(), &computes, 1));
    assert!(poll_as(&registry, &task, leader.as_mut()).is_pending());
    let open = Gate::default();
    open.release();
    let mut nested = pin!(claim(&group, "k", open, &computes, 3));
    assert_eq!(
        answered(poll_as(&registry, &task, nested.as_mut())),
        (SingleflightRole::Leader, 3)
    );
    assert_eq!(
        registry.wait_count_for_tests(),
        0,
        "a task never waits on itself"
    );
    gate.release();
    assert_eq!(
        answered(poll_as(&registry, &task, leader.as_mut())),
        (SingleflightRole::Leader, 1)
    );
}

/// A follower whose wait would close a cycle of waiting tasks computes
/// inline: the leader it would join already waits on it.
#[test]
fn a_wait_that_would_close_a_cycle_computes_inline() {
    let registry = TaskRegistry::default();
    let group = Group::with_task_registry(registry.clone());
    let (first, second) = (registry.register_task(), registry.register_task());
    let (gate, computes) = (Gate::default(), AtomicUsize::new(0));
    let mut first_leads_x = pin!(claim(&group, "x", gate.clone(), &computes, 1));
    let mut second_leads_y = pin!(claim(&group, "y", gate.clone(), &computes, 2));
    assert!(poll_as(&registry, &first, first_leads_x.as_mut()).is_pending());
    assert!(poll_as(&registry, &second, second_leads_y.as_mut()).is_pending());
    let mut first_follows_y = pin!(claim(&group, "y", Gate::default(), &computes, 3));
    assert!(poll_as(&registry, &first, first_follows_y.as_mut()).is_pending());
    let open = Gate::default();
    open.release();
    let mut second_joins_x = pin!(claim(&group, "x", open, &computes, 4));
    assert_eq!(
        answered(poll_as(&registry, &second, second_joins_x.as_mut())),
        (SingleflightRole::Leader, 4),
        "the refused wait computes inline",
    );
    assert_eq!(
        registry.wait_count_for_tests(),
        1,
        "only the first task's wait stands"
    );
    gate.release();
    assert_eq!(
        answered(poll_as(&registry, &second, second_leads_y.as_mut())),
        (SingleflightRole::Leader, 2)
    );
    assert_eq!(
        answered(poll_as(&registry, &first, first_follows_y.as_mut())),
        (SingleflightRole::Follower, 2)
    );
    assert_eq!(
        answered(poll_as(&registry, &first, first_leads_x.as_mut())),
        (SingleflightRole::Leader, 1)
    );
    assert_eq!(registry.wait_count_for_tests(), 0);
}

/// Dropping a suspended leader aborts its lane: its follower re-elects and
/// computes, and nothing is left registered.
#[test]
fn dropping_a_suspended_leader_lets_its_follower_re_elect() {
    let registry = TaskRegistry::default();
    let group = Group::with_task_registry(registry.clone());
    let (leading, following) = (registry.register_task(), registry.register_task());
    let computes = AtomicUsize::new(0);
    let mut leader = Box::pin(claim(&group, "k", Gate::default(), &computes, 1));
    assert!(poll_as(&registry, &leading, leader.as_mut()).is_pending());
    let open = Gate::default();
    open.release();
    let mut follower = pin!(claim(&group, "k", open, &computes, 2));
    assert!(poll_as(&registry, &following, follower.as_mut()).is_pending());
    drop(leader);
    assert_eq!(
        answered(poll_as(&registry, &following, follower.as_mut())),
        (SingleflightRole::Leader, 2)
    );
    assert_eq!(computes.load(Ordering::SeqCst), 1);
    assert_eq!(registry.wait_count_for_tests(), 0);
    assert_eq!(group.test_flight_strong_count(&"k", token()), 0);
}

/// Dropping a suspended follower withdraws its wait and its edge; the
/// leader completes alone.
#[test]
fn dropping_a_suspended_follower_withdraws_its_wait() {
    let registry = TaskRegistry::default();
    let group = Group::with_task_registry(registry.clone());
    let (leading, following) = (registry.register_task(), registry.register_task());
    let (gate, computes) = (Gate::default(), AtomicUsize::new(0));
    let mut leader = pin!(claim(&group, "k", gate.clone(), &computes, 1));
    assert!(poll_as(&registry, &leading, leader.as_mut()).is_pending());
    let mut follower = Box::pin(claim(&group, "k", Gate::default(), &computes, 2));
    assert!(poll_as(&registry, &following, follower.as_mut()).is_pending());
    assert_eq!(registry.wait_count_for_tests(), 1);
    drop(follower);
    assert_eq!(registry.wait_count_for_tests(), 0);
    gate.release();
    assert_eq!(
        answered(poll_as(&registry, &leading, leader.as_mut())),
        (SingleflightRole::Leader, 1)
    );
    assert_eq!(computes.load(Ordering::SeqCst), 1);
}
