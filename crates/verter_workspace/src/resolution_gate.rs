//! The workspace publication gate. Its synchronous entry drives the same
//! readiness subscription; a resolver never parks while polling this gate.

use crate::resolution_handoff::ResolutionError;
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;
use std::task::{Poll, Waker};
use verter_execution::tasks::{ExecutionScope, ExecutionTask, TaskRegistry, WaitEdge};
use verter_execution::wake::{WakeRegistration, WakerSet};

/// Shared composition-root authority, including standalone workspace use.
#[derive(Clone, Default)]
pub(crate) struct Authority(Arc<RwLock<Option<TaskRegistry>>>);
impl Authority {
    pub(crate) fn bind(&self, registry: TaskRegistry) {
        let mut current = self.0.write();
        if let Some(existing) = current.as_ref() {
            existing.share_authority_with(&registry);
        }
        *current = Some(registry);
    }
    pub(crate) fn registry(&self) -> TaskRegistry {
        if let Some(registry) = self.0.read().as_ref() {
            return registry.clone();
        }
        self.0
            .write()
            .get_or_insert_with(TaskRegistry::default)
            .clone()
    }
    pub(crate) fn current(&self) -> ExecutionTask {
        // A production continuation already owns the host's authority. An
        // external compatibility call uses the authority injected at attach.
        let registry = self.registry();
        ExecutionScope::current(&registry).unwrap_or_else(|| registry.register_task())
    }
}
struct State {
    owner: Option<ExecutionTask>,
    wakers: WakerSet,
}
pub(crate) struct PublicationGate {
    state: Arc<Mutex<State>>,
    authority: Authority,
}
pub(crate) struct PublicationGuard {
    state: Arc<Mutex<State>>,
    scope: Option<ExecutionScope>,
    // A publication guard never crosses a suspension or worker migration.
    _thread: std::marker::PhantomData<*mut ()>,
}
impl PublicationGate {
    pub(crate) fn new(authority: Authority) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                owner: None,
                wakers: WakerSet::default(),
            })),
            authority,
        }
    }
    pub(crate) fn try_lock(&self) -> Option<PublicationGuard> {
        let mut state = self.state.lock();
        if state.owner.is_some() {
            return None;
        }
        let task = self.authority.current();
        state.owner = Some(task.clone());
        Some(PublicationGuard {
            state: self.state.clone(),
            scope: Some(ExecutionScope::install(task.registry(), &task)),
            _thread: std::marker::PhantomData,
        })
    }
    pub(crate) fn lock(&self) -> PublicationGuard {
        let mut subscription = self.subscribe(self.authority.current());
        verter_execution::blocking::block_on(
            verter_execution::blocking::WaitFamily::WorkspaceResolution,
            None,
            |waker| subscription.poll(waker),
        )
        .expect("publication writers do not recursively acquire their gate")
    }
    pub(crate) fn subscribe(&self, waiter: ExecutionTask) -> GateSubscription {
        GateSubscription {
            state: self.state.clone(),
            waiter,
            registration: WakeRegistration::default(),
            edge: None,
        }
    }
}
impl Drop for PublicationGuard {
    fn drop(&mut self) {
        let (owner, wakes) = {
            let mut state = self.state.lock();
            (state.owner.take(), state.wakers.take())
        };
        self.scope = None;
        drop(owner);
        wakes.wake();
    }
}
pub(crate) struct GateSubscription {
    state: Arc<Mutex<State>>,
    waiter: ExecutionTask,
    registration: WakeRegistration,
    edge: Option<WaitEdge>,
}
impl GateSubscription {
    pub(crate) fn poll(
        &mut self,
        waker: &Waker,
    ) -> Poll<Result<PublicationGuard, ResolutionError>> {
        let mut state = self.state.lock();
        self.edge = None;
        if let Some(owner) = &state.owner {
            let edge = match self.waiter.wait_for(owner) {
                Ok(edge) => edge,
                Err(_) => {
                    state.wakers.withdraw(&mut self.registration);
                    return Poll::Ready(Err(ResolutionError::Cycle));
                }
            };
            self.edge = Some(edge);
            state.wakers.register(&mut self.registration, waker);
            Poll::Pending
        } else {
            state.wakers.withdraw(&mut self.registration);
            state.owner = Some(self.waiter.clone());
            Poll::Ready(Ok(PublicationGuard {
                state: self.state.clone(),
                scope: Some(ExecutionScope::install(
                    self.waiter.registry(),
                    &self.waiter,
                )),
                _thread: std::marker::PhantomData,
            }))
        }
    }
    pub(crate) async fn acquire(mut self) -> Result<PublicationGuard, ResolutionError> {
        std::future::poll_fn(|cx| self.poll(cx.waker())).await
    }
}
impl Drop for GateSubscription {
    fn drop(&mut self) {
        self.edge = None;
        self.state.lock().wakers.withdraw(&mut self.registration);
    }
}
