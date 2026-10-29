use verter_scheduler::scheduler::OwnedSchedulerRequest;

fn cannot_block(mut handoff: OwnedSchedulerRequest) {
    handoff.wait();
}

fn main() {}
