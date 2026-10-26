//! The scheduler DAG lock and delivery of wakes after lifecycle unlock.

use super::*;
use std::ops::{Deref, DerefMut};
use std::task::{Wake, Waker};
use verter_execution::wake::{TakenWakers, WakeRegistration, WakerSet};

#[cfg(feature = "semantic-observe")]
type InnerMutex = hotpath::wrap::parking_lot::Mutex<SchedulerDag>;
#[cfg(not(feature = "semantic-observe"))]
type InnerMutex = Mutex<SchedulerDag>;
#[cfg(feature = "semantic-observe")]
type InnerGuard<'a> = hotpath::wrap::parking_lot::MutexGuard<'a, SchedulerDag>;
#[cfg(not(feature = "semantic-observe"))]
type InnerGuard<'a> = parking_lot::MutexGuard<'a, SchedulerDag>;

/// Every scheduler lifecycle acquisition carries its deferred wakes.
/// Publication happens under the lock; callbacks never do.
pub(crate) struct DagMutex {
    inner: InnerMutex,
    delivery: Mutex<Delivery>,
    inbox: crossbeam_channel::Sender<Submission>,
}

#[derive(Default)]
struct Delivery {
    scopes: usize,
    pending: Vec<TakenWakers>,
    progress: bool,
    waiters: WakerSet,
}

pub(super) struct DagMutexGuard<'a> {
    inner: Option<InnerGuard<'a>>,
    owner: &'a DagMutex,
    _wakes: WakeDeferral<'a>,
}

/// Postpone callbacks across an outer scheduler lifecycle hold (reset).
/// This also covers the driver the resetting thread must join.
pub(super) struct WakeDeferral<'a>(&'a DagMutex);

/// Interest in scheduler readiness or capacity transitions for a cooperative
/// driver. Register before checking the dependency and pumping; a transition
/// racing that check then schedules another poll. Drop withdraws only this
/// driver's registration. The subscription never drives work itself.
pub struct SchedulerProgressSubscription {
    owner: Arc<DagMutex>,
    registration: WakeRegistration,
}

impl SchedulerProgressSubscription {
    pub(super) fn new(owner: Arc<DagMutex>) -> Self {
        Self {
            owner,
            registration: WakeRegistration::default(),
        }
    }

    /// Subscribe before rechecking readiness. Replaces an older executor waker.
    pub fn register(&mut self, waker: &Waker) {
        self.owner
            .delivery
            .lock()
            .waiters
            .register(&mut self.registration, waker);
    }
}

impl Drop for SchedulerProgressSubscription {
    fn drop(&mut self) {
        self.owner
            .delivery
            .lock()
            .waiters
            .withdraw(&mut self.registration);
    }
}

/// Reservations carry this weak callback so the final share can return
/// capacity outside a DAG operation without retaining the scheduler.
struct CapacityWake(std::sync::Weak<DagMutex>);

impl Wake for CapacityWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if let Some(owner) = self.0.upgrade() {
            owner.deliver_transition(Vec::new(), true);
        }
    }
}

impl DagMutex {
    pub(super) fn lock(&self) -> DagMutexGuard<'_> {
        let wakes = self.defer_wakes();
        DagMutexGuard {
            inner: Some(self.inner.lock()),
            owner: self,
            _wakes: wakes,
        }
    }

    #[cfg(test)]
    pub(super) fn try_lock(&self) -> Option<DagMutexGuard<'_>> {
        let wakes = self.defer_wakes();
        self.inner.try_lock().map(|inner| DagMutexGuard {
            inner: Some(inner),
            owner: self,
            _wakes: wakes,
        })
    }

    pub(super) fn defer_wakes(&self) -> WakeDeferral<'_> {
        self.delivery.lock().scopes += 1;
        WakeDeferral(self)
    }

    pub(super) fn send_completion(
        &self,
        sender: &CompletionSender<RequestResult>,
        state: CompletionState<RequestResult>,
    ) {
        if let Some(wakers) = sender.send_deferred(state) {
            if !wakers.is_empty() {
                self.deliver(vec![wakers]);
            }
        }
    }

    pub(super) fn deliver(&self, wakes: Vec<TakenWakers>) {
        self.deliver_transition(wakes, false);
    }

    fn deliver_transition(&self, mut wakes: Vec<TakenWakers>, progress: bool) {
        let (wakes, progress) = {
            let mut delivery = self.delivery.lock();
            delivery.pending.append(&mut wakes);
            delivery.progress |= progress;
            if delivery.scopes != 0 {
                return;
            }
            let progress = std::mem::take(&mut delivery.progress);
            if progress {
                let waiters = delivery.waiters.take();
                if !waiters.is_empty() {
                    delivery.pending.push(waiters);
                }
            }
            (std::mem::take(&mut delivery.pending), progress)
        };
        if progress {
            // Full already means the driver has inbox work; never block a
            // completion or a final permit release on bounded inbox capacity.
            let _ = self.inbox.try_send(Submission::Wake);
        }
        for wakers in wakes {
            wakers.wake();
        }
    }
}

impl Deref for DagMutexGuard<'_> {
    type Target = SchedulerDag;
    fn deref(&self) -> &Self::Target {
        self.inner.as_deref().expect("live DAG guard")
    }
}

impl DerefMut for DagMutexGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.as_deref_mut().expect("live DAG guard")
    }
}

impl Drop for DagMutexGuard<'_> {
    fn drop(&mut self) {
        let mut inner = self.inner.take().expect("DAG guard releases once");
        let wakes = inner.take_completion_wakes();
        drop(inner);
        self.owner.deliver(wakes);
    }
}

impl Drop for WakeDeferral<'_> {
    fn drop(&mut self) {
        {
            let mut delivery = self.0.delivery.lock();
            delivery.scopes -= 1;
            if delivery.scopes != 0 {
                return;
            }
        }
        self.0.deliver(Vec::new());
    }
}

pub(super) fn new_dag_mutex(
    mut dag: SchedulerDag,
    inbox: crossbeam_channel::Sender<Submission>,
) -> Arc<DagMutex> {
    dag.defer_completion_wakes();
    Arc::new_cyclic(|weak| {
        dag.set_progress_waker(Waker::from(Arc::new(CapacityWake(weak.clone()))));
        #[cfg(feature = "semantic-observe")]
        let inner = hotpath::mutex!(
            hotpath_parking_lot::Mutex::new(dag),
            label = "scheduler_dag"
        );
        #[cfg(not(feature = "semantic-observe"))]
        let inner = Mutex::new(dag);
        DagMutex {
            inner,
            delivery: Mutex::new(Delivery::default()),
            inbox,
        }
    })
}
