//! Owned inline execution: polling observes a reply, external work owns CPU.

/// Independent pure lowering work. Running it contains operation panic and
/// publishes an owned reply; dropping it settles the demand as abandoned.
pub struct LoweringWork {
    run: Box<dyn FnOnce() + Send>,
}
impl LoweringWork {
    pub fn run(self) {
        (self.run)();
    }
}
impl std::fmt::Debug for LoweringWork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoweringWork").finish_non_exhaustive()
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
use std::{
    any::Any,
    sync::Arc,
    task::{Poll, Waker},
};
#[cfg(any(test, target_arch = "wasm32"))]
use verter_execution::wake::{WakeRegistration, WakerSet};

#[cfg(any(test, target_arch = "wasm32"))]
type Operation<T> = Box<dyn FnOnce() -> T + Send>;
#[cfg(any(test, target_arch = "wasm32"))]
struct Reply<T> {
    answer: Option<Result<T, Box<dyn Any + Send>>>,
    wakers: WakerSet,
}
#[cfg(any(test, target_arch = "wasm32"))]
pub(super) struct InlineAbandoned;

/// One reply owner and one initially retained operation, with no arena borrow.
#[cfg(any(test, target_arch = "wasm32"))]
pub(super) struct InlineWork<T> {
    reply: Arc<parking_lot::Mutex<Reply<T>>>,
    operation: Option<Operation<T>>,
    registration: WakeRegistration,
    finished: bool,
}
#[cfg(any(test, target_arch = "wasm32"))]
impl<T> InlineWork<T> {
    pub(super) fn new(operation: Operation<T>) -> Self {
        Self {
            reply: Arc::new(parking_lot::Mutex::new(Reply {
                answer: None,
                wakers: WakerSet::default(),
            })),
            operation: Some(operation),
            registration: WakeRegistration::default(),
            finished: false,
        }
    }
    #[cfg(target_arch = "wasm32")]
    pub(super) fn ready(answer: T) -> Self {
        Self {
            reply: Arc::new(parking_lot::Mutex::new(Reply {
                answer: Some(Ok(answer)),
                wakers: WakerSet::default(),
            })),
            operation: None,
            registration: WakeRegistration::default(),
            finished: false,
        }
    }
    #[cfg(target_arch = "wasm32")]
    pub(super) fn install_task(&mut self, task: verter_execution::tasks::ExecutionTask)
    where
        T: 'static,
    {
        if let Some(operation) = self.operation.take() {
            self.operation = Some(Box::new(move || {
                let _scope =
                    verter_execution::tasks::ExecutionScope::install(task.registry(), &task);
                let _arena = task.open(Arc::new(super::LoweringShardIdentity {}));
                operation()
            }));
        }
    }
    pub(super) fn poll(&mut self, waker: &Waker) -> Poll<T> {
        assert!(!self.finished, "inline lowering polled after answering");
        let answer = {
            let mut reply = self.reply.lock();
            match reply.answer.take() {
                Some(answer) => {
                    reply.wakers.withdraw(&mut self.registration);
                    Some(answer)
                }
                None => {
                    reply.wakers.register(&mut self.registration, waker);
                    None
                }
            }
        };
        self.finished = answer.is_some();
        match answer {
            None => Poll::Pending,
            Some(Ok(answer)) => Poll::Ready(answer),
            Some(Err(panic)) => std::panic::resume_unwind(panic),
        }
    }
}
#[cfg(any(test, target_arch = "wasm32"))]
impl<T: Send + 'static> InlineWork<T> {
    pub(super) fn take_work(&mut self) -> Option<LoweringWork> {
        let operation = self.operation.take()?;
        let producer = Producer {
            operation: Some(operation),
            reply: self.reply.clone(),
            settled: false,
        };
        Some(LoweringWork {
            run: Box::new(move || producer.run()),
        })
    }
}
#[cfg(any(test, target_arch = "wasm32"))]
impl<T> Drop for InlineWork<T> {
    fn drop(&mut self) {
        self.reply.lock().wakers.withdraw(&mut self.registration);
    }
}
#[cfg(any(test, target_arch = "wasm32"))]
struct Producer<T> {
    operation: Option<Operation<T>>,
    reply: Arc<parking_lot::Mutex<Reply<T>>>,
    settled: bool,
}
#[cfg(any(test, target_arch = "wasm32"))]
impl<T> Producer<T> {
    fn publish(&mut self, answer: Result<T, Box<dyn Any + Send>>) {
        self.settled = true;
        let wakes = {
            let mut reply = self.reply.lock();
            reply.answer = Some(answer);
            reply.wakers.take()
        };
        wakes.wake();
    }
    fn run(mut self) {
        let operation = self.operation.take().expect("exclusive inline producer");
        // The operation's execution scope and arena borrow leave before wakes.
        let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation));
        self.publish(answer);
    }
}
#[cfg(any(test, target_arch = "wasm32"))]
impl<T> Drop for Producer<T> {
    fn drop(&mut self) {
        if !self.settled {
            drop(self.operation.take());
            self.publish(Err(Box::new(InlineAbandoned)));
        }
    }
}
