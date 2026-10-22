//! The scheduler DAG lock and delivery of completion wakes after unlock.

use super::*;
use std::ops::{Deref, DerefMut};
use verter_execution::wake::TakenWakers;

#[cfg(feature = "semantic-observe")]
type InnerMutex = hotpath::wrap::parking_lot::Mutex<SchedulerDag>;
#[cfg(not(feature = "semantic-observe"))]
type InnerMutex = Mutex<SchedulerDag>;
#[cfg(feature = "semantic-observe")]
type InnerGuard<'a> = hotpath::wrap::parking_lot::MutexGuard<'a, SchedulerDag>;
#[cfg(not(feature = "semantic-observe"))]
type InnerGuard<'a> = parking_lot::MutexGuard<'a, SchedulerDag>;

/// Every scheduler DAG acquisition carries its deferred completion wakes.
/// Publication still happens under the lifecycle lock; callbacks never do.
pub(crate) struct DagMutex {
    inner: InnerMutex,
    delivery: Mutex<Delivery>,
}

#[derive(Default)]
struct Delivery {
    scopes: usize,
    pending: Vec<TakenWakers>,
}

pub(super) struct DagMutexGuard<'a> {
    inner: Option<InnerGuard<'a>>,
    owner: &'a DagMutex,
}

/// Postpone callbacks across an outer scheduler lifecycle hold (reset).
/// This also covers the driver the resetting thread must join.
#[cfg(not(target_arch = "wasm32"))]
pub(super) struct WakeDeferral<'a>(&'a DagMutex);

impl DagMutex {
    pub(super) fn lock(&self) -> DagMutexGuard<'_> {
        DagMutexGuard {
            inner: Some(self.inner.lock()),
            owner: self,
        }
    }

    #[cfg(test)]
    pub(super) fn try_lock(&self) -> Option<DagMutexGuard<'_>> {
        self.inner.try_lock().map(|inner| DagMutexGuard {
            inner: Some(inner),
            owner: self,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
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

    fn deliver(&self, mut wakes: Vec<TakenWakers>) {
        if wakes.is_empty() {
            return;
        }
        {
            let mut delivery = self.delivery.lock();
            if delivery.scopes != 0 {
                delivery.pending.append(&mut wakes);
                return;
            }
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

#[cfg(not(target_arch = "wasm32"))]
impl Drop for WakeDeferral<'_> {
    fn drop(&mut self) {
        let wakes = {
            let mut delivery = self.0.delivery.lock();
            delivery.scopes -= 1;
            if delivery.scopes != 0 {
                return;
            }
            std::mem::take(&mut delivery.pending)
        };
        self.0.deliver(wakes);
    }
}

pub(super) fn new_dag_mutex(mut dag: SchedulerDag) -> DagMutex {
    dag.defer_completion_wakes();
    #[cfg(feature = "semantic-observe")]
    let inner = hotpath::mutex!(parking_lot::Mutex::new(dag), label = "scheduler_dag");
    #[cfg(not(feature = "semantic-observe"))]
    let inner = Mutex::new(dag);
    DagMutex {
        inner,
        delivery: Mutex::new(Delivery::default()),
    }
}
