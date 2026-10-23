use super::*;
use crate::tasks::TaskRegistry;
use crate::wake::{WakeRegistration, WakerSet};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::Wake;

#[derive(Default)]
struct Slot(Mutex<(Option<u64>, WakerSet)>);

struct Join(Arc<Slot>, WakeRegistration);

impl Join {
    fn new(slot: &Arc<Slot>) -> Self {
        Self(Arc::clone(slot), WakeRegistration::default())
    }
}

impl DependencyPoll for Join {
    type Output = u64;
    fn poll_dependency(&mut self, waker: &Waker) -> Poll<u64> {
        let mut slot = self.0 .0.lock();
        if let Some(value) = slot.0 {
            Poll::Ready(value)
        } else {
            slot.1.register(&mut self.1, waker);
            Poll::Pending
        }
    }
}

impl Drop for Join {
    fn drop(&mut self) {
        self.0 .0.lock().1.withdraw(&mut self.1);
    }
}

impl Slot {
    fn complete(&self, value: u64) {
        let wakes = {
            let mut slot = self.0.lock();
            slot.0 = Some(value);
            slot.1.take()
        };
        wakes.wake();
    }
}

#[derive(Default)]
struct Count(AtomicUsize);
impl Wake for Count {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn stopping_one_waiter_releases_its_edge_and_registrations_before_producer_completion() {
    fn send<T: Send>() {}
    send::<OwnedDependency<Join>>();
    let registry = TaskRegistry::default();
    let producer = registry.register_task();
    let slot = Arc::new(Slot::default());
    let cancel = CancellationToken::new();
    let mut first = OwnedDependency::new(
        Join::new(&slot),
        registry.register_task(),
        producer.clone(),
        cancel.clone(),
    );
    let mut second = OwnedDependency::new(
        Join::new(&slot),
        registry.register_task(),
        producer,
        CancellationToken::new(),
    );
    let count = Arc::new(Count::default());
    let waker = Waker::from(Arc::clone(&count));
    assert!(first.poll(&waker).is_pending());
    assert!(second.poll(&waker).is_pending());
    assert_eq!(registry.wait_graph_occupancy().edges, 2);
    cancel.cancel();
    assert_eq!(count.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        first.poll(&waker),
        Poll::Ready(Err(DependencyStop::Cancelled))
    );
    assert_eq!(registry.wait_graph_occupancy().edges, 1);
    assert_eq!(slot.0.lock().1.len(), 1);
    // A spurious wake or repoll does not complete or duplicate the sibling.
    waker.wake_by_ref();
    assert!(second.poll(&waker).is_pending());
    assert_eq!(slot.0.lock().1.len(), 1);
    slot.complete(42);
    assert_eq!(second.poll(&waker), Poll::Ready(Ok(42)));
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert!(slot.0.lock().1.is_empty());
}

#[test]
fn completed_self_dependency_is_ready_but_pending_self_dependency_is_refused() {
    let registry = TaskRegistry::default();
    let task = registry.register_task();
    let ready = Arc::new(Slot::default());
    ready.complete(7);
    let mut join = OwnedDependency::new(
        Join::new(&ready),
        task.clone(),
        task.clone(),
        CancellationToken::new(),
    );
    assert_eq!(join.poll(Waker::noop()), Poll::Ready(Ok(7)));
    let pending = Arc::new(Slot::default());
    let mut join = OwnedDependency::new(
        Join::new(&pending),
        task.clone(),
        task,
        CancellationToken::new(),
    );
    assert_eq!(
        join.poll(Waker::noop()),
        Poll::Ready(Err(DependencyStop::Cycle))
    );
    assert!(pending.0.lock().1.is_empty());
}

#[test]
fn cycles_and_cross_registry_aliases_refuse_without_stranding_registrations() {
    let registry = TaskRegistry::default();
    let first = registry.register_task();
    let second = registry.register_task();
    let first_slot = Arc::new(Slot::default());
    let second_slot = Arc::new(Slot::default());
    let mut forward = OwnedDependency::new(
        Join::new(&second_slot),
        first.clone(),
        second.clone(),
        CancellationToken::new(),
    );
    let mut backward = OwnedDependency::new(
        Join::new(&first_slot),
        second,
        first.clone(),
        CancellationToken::new(),
    );
    assert!(forward.poll(Waker::noop()).is_pending());
    assert_eq!(
        backward.poll(Waker::noop()),
        Poll::Ready(Err(DependencyStop::Cycle))
    );
    assert!(first_slot.0.lock().1.is_empty());
    drop(forward);
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert!(second_slot.0.lock().1.is_empty());
    let alien = TaskRegistry::default().register_task();
    let mut join = OwnedDependency::new(
        Join::new(&second_slot),
        first,
        alien,
        CancellationToken::new(),
    );
    assert_eq!(
        join.poll(Waker::noop()),
        Poll::Ready(Err(DependencyStop::Cycle))
    );
}

#[test]
fn stopping_or_dropping_a_wait_does_not_pin_its_tasks() {
    let registry = TaskRegistry::default();
    let slot = Arc::new(Slot::default());
    let mut join = OwnedDependency::new(
        Join::new(&slot),
        registry.register_task(),
        registry.register_task(),
        CancellationToken::new(),
    );
    assert!(join.poll(Waker::noop()).is_pending());
    drop(join);
    assert_eq!(registry.active_task_count_for_tests(), 0);
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert!(slot.0.lock().1.is_empty());
}
