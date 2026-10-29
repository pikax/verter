use verter_semantic_source::decl_lowering::{LeaseHandoff, OwnedLoweringHandoff};

fn cannot_block(mut handoff: OwnedLoweringHandoff<LeaseHandoff<usize>>) {
    handoff.wait();
}

fn main() {}
