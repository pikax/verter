use verter_execution::dependency::OwnedDependency;
use verter_scheduler::job::CompletionSubscription;

fn cannot_block(mut handoff: OwnedDependency<CompletionSubscription<u64>>) {
    handoff.wait();
}

fn main() {}
