use verter_semantic_source::decl_lowering::{LeaseHandoff, OwnedLoweringHandoff};

fn cannot_extract(handoff: OwnedLoweringHandoff<LeaseHandoff<usize>>) {
    let _ = handoff.dependency;
}

fn main() {}
