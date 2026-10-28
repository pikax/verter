use verter_semantic_source::decl_lowering::{LeaseHandoff, OwnedLoweringHandoff};

fn cannot_share(handoff: &OwnedLoweringHandoff<LeaseHandoff<usize>>) {
    handoff.poll(std::task::Waker::noop());
}

fn main() {}
