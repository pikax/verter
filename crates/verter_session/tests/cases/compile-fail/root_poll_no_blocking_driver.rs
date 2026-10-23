use verter_scheduler::scheduler::Scheduler;
use verter_session::{CompileRequestRoot, ComponentMetaOutputRoot};

fn cannot_block_on_a_meta_root(root: ComponentMetaOutputRoot<'_>, scheduler: &Scheduler) {
    let _ = root.drive(scheduler);
}

fn cannot_block_on_a_compile_root(root: CompileRequestRoot<'_>, scheduler: &Scheduler) {
    let _ = root.drive(scheduler);
}

fn main() {}
