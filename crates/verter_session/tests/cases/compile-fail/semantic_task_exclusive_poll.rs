//! A polled root has one poller at a time: its task is lent only through an
//! exclusive borrow of its entry, so the entry cannot be polled while its
//! task is out, and a task reached through a shared reference cannot be
//! polled at all.
use std::task::Waker;

use verter_type_engine::project_semantic_dispatch::{SemanticQueryPoll, SemanticTask};
use verter_type_engine::resolver_core::ResolverCapabilities;

fn entry_while_its_task_is_out<C: ResolverCapabilities>(
    entry: &mut SemanticQueryPoll<'_, '_, C>,
    waker: &Waker,
) {
    let Some(task) = entry.task() else {
        return;
    };
    let _ = entry.poll(waker);
    let _ = task.poll(waker);
}

fn shared_task<C: ResolverCapabilities>(task: &SemanticTask<'_, '_, C>, waker: &Waker) {
    let _ = task.poll(waker);
}

fn main() {}
