use verter_semantic_source::decl_lowering::{
    LeaseHandoff, LeasedBatchHandoff, LeasedRunHandoff, LoweringWork, OwnedLoweringHandoff,
};
use verter_session::{StoreViewHandoff, StoreViewProducer};

fn send<T: Send>() {}

fn main() {
    send::<StoreViewHandoff>();
    send::<StoreViewProducer>();
    send::<OwnedLoweringHandoff<LeaseHandoff<usize>>>();
    send::<OwnedLoweringHandoff<LeasedRunHandoff<usize>>>();
    send::<OwnedLoweringHandoff<LeasedBatchHandoff<usize>>>();
    send::<LoweringWork>();
}
