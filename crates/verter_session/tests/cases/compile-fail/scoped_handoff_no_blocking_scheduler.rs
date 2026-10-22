use verter_scheduler::scheduler::ScopedCacheNodeHandoff;

fn cannot_recover_blocking_scheduler(handoff: &ScopedCacheNodeHandoff<u64>) {
    let _ = &handoff.scheduler;
}

fn main() {}
