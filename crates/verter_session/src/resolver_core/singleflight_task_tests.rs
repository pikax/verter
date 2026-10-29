//! Resolver flights are led and joined by execution tasks: a suspended
//! leader keeps its lane across polls, another task on the same thread
//! follows it instead of computing again, a task re-entering its own lane
//! or closing a wait cycle computes inline, and dropping a suspended
//! leader or follower releases what it held.

use std::future::Future;
use std::pin::{pin, Pin};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use verter_execution::tasks::{ExecutionScope, ExecutionTask, TaskRegistry};

use super::{SingleflightGroup, SingleflightRole, SingleflightRunResult, StoreViewCompatToken};

fn waker() -> Waker {
    Waker::noop().clone()
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

fn synchronous_cross_thread_cycle(group: Group) {
    use std::sync::mpsc;
    use std::time::Duration;

    let group = Arc::new(group);
    let parked = group.subscribe_joiner_park();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let mut releases = Vec::new();
    let mut threads = Vec::new();
    for (key, other, fallback) in [("x", "y", 20), ("y", "x", 10)] {
        let group = group.clone();
        let entered = entered_tx.clone();
        let done = done_tx.clone();
        let (release_tx, release_rx) = mpsc::channel();
        releases.push(release_tx);
        threads.push(std::thread::spawn(move || {
            let answer = group.run(key, token(), || {
                entered.send(()).map_err(|_| ())?;
                release_rx.recv().map_err(|_| ())?;
                let nested = group.run(other, token(), || Ok(fallback))?;
                Ok(*nested.value + 1)
            });
            let _ = done.send(());
            answer
        }));
    }
    let deadline = Duration::from_secs(5);
    let completed = (|| {
        entered_rx.recv_timeout(deadline).ok()?;
        entered_rx.recv_timeout(deadline).ok()?;
        releases[0].send(()).ok()?;
        parked.recv_timeout(deadline).ok()?;
        releases[1].send(()).ok()?;
        done_rx.recv_timeout(deadline).ok()?;
        done_rx.recv_timeout(deadline).ok()?;
        Some(())
    })();

    // Release every hold and retire parked claims before any assertion,
    // including when cycle detection fails and both threads are waiting.
    if completed.is_none() {
        group.clear();
    }
    drop(releases);
    let answers: Vec<_> = threads.into_iter().map(|thread| thread.join()).collect();
    assert!(completed.is_some(), "cross-thread cycle must complete");
    let values: Vec<_> = answers
        .into_iter()
        .map(|answer| {
            *answer
                .expect("worker returned")
                .expect("flight answered")
                .value
        })
        .collect();
    assert_eq!(
        values,
        [12, 11],
        "only the cycle-closing join computes inline"
    );
    assert_eq!(group.task_registry().wait_count_for_tests(), 0);
    assert_eq!(group.task_registry().active_task_count_for_tests(), 0);
    assert!(group.flights.lock().lanes.is_empty());
}

#[test]
fn default_synchronous_flights_break_cross_thread_cycles() {
    synchronous_cross_thread_cycle(Group::default());
}

#[test]
fn composed_synchronous_flights_break_cross_thread_cycles() {
    synchronous_cross_thread_cycle(Group::with_task_registry(TaskRegistry::default()));
}

#[test]
fn default_synchronous_flights_reuse_ambient_task_authority() {
    let registry = TaskRegistry::default();
    let caller = ExecutionScope::enter(&registry);
    let group = Group::default();
    let answer = group
        .run("x", token(), || {
            let producer = ExecutionScope::current_task().expect("producer task installed");
            assert_ne!(producer.id(), caller.task().id());
            let other = registry.register_task();
            let edge = other.wait_for(&producer).expect("ambient authority reused");
            drop(edge);
            Ok(7)
        })
        .unwrap();
    assert_eq!(*answer.value, 7);
    assert!(group.registry.get().is_none());
    drop(caller);
    assert_eq!(registry.wait_count_for_tests(), 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

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
    assert_eq!(group.test_flight_strong_count(&"k", token()), 0);
    let fresh = group.run("k", token(), || Ok(29)).unwrap();
    assert_eq!((fresh.role, *fresh.value), (SingleflightRole::Leader, 29));
}

#[test]
fn dropping_a_suspended_follower_releases_the_completed_value() {
    for abandon in [false, true] {
        let group = SingleflightGroup::<&str, Arc<usize>, ()>::default();
        let registry = group.task_registry();
        let leading = registry.register_task();
        let gate = Gate::default();
        let value = Arc::new(11);
        let weak = Arc::downgrade(&value);
        let mut leader = Box::pin(group.run_retaining_pollable(
            "k",
            token(),
            || async {
                gate.passed().await;
                Ok(value)
            },
            |_| true,
        ));
        assert!(poll_as(registry, &leading, leader.as_mut()).is_pending());
        let following = registry.register_task();
        let mut follower = Box::pin(group.run_retaining_pollable(
            "k",
            token(),
            || async {
                panic!("a follower must not compute");
            },
            |_| true,
        ));
        assert!(poll_as(registry, &following, follower.as_mut()).is_pending());
        if abandon {
            drop(follower);
        } else {
            gate.release();
            assert!(poll_as(registry, &leading, leader.as_mut()).is_ready());
            assert!(poll_as(registry, &following, follower.as_mut()).is_ready());
            drop(follower);
        }
        if abandon {
            gate.release();
            assert!(poll_as(registry, &leading, leader.as_mut()).is_ready());
        }
        drop(leader);
        assert!(weak.upgrade().is_none(), "abandon={abandon}");
    }
}
