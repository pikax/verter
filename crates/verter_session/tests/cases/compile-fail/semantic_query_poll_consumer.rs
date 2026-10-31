//! A consumer that must not block drives a root query through the polled
//! entry alone: it polls the entry on its own thread and, while the entry is
//! pending, advances its `Send` task on another thread. The entry is also a
//! future, for an executor to drive.
use std::future::Future;
use std::task::Waker;

use verter_type_engine::project_semantic_dispatch::{SemanticQueryPoll, SemanticTask};
use verter_type_engine::resolver_core::ResolverCapabilities;

fn send<T: Send>(_: &T) {}

fn awaitable<F: Future + Unpin>(_: &F) {}

#[allow(dead_code)]
fn consume<C: ResolverCapabilities>(mut entry: SemanticQueryPoll<'_, '_, C>, waker: &Waker) -> bool {
    awaitable(&entry);
    loop {
        if entry.poll(waker).is_ready() {
            return true;
        }
        let Some(task) = entry.task() else {
            return false;
        };
        let task: &mut SemanticTask<'_, '_, C> = task;
        send(task);
        let advanced = std::thread::scope(|scope| scope.spawn(|| task.poll(waker).is_ready()).join());
        if advanced.is_err() {
            return false;
        }
    }
}

fn main() {}
