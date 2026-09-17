//! C2-GAP3-ALTERNATE-ENTRY: `TypeInfoCore::attempt` is the only gateway
//! entry — no bypass, raw dispatch, or construction from outside.

use std::sync::Arc;

use verter_semantic::type_info::{
    NonFlowObservationSnapshot, NonFlowOperation, NonFlowPayload, TypeInfoCore,
};

fn main() {
    let core = TypeInfoCore::from_observation_snapshot(
        Arc::new(NonFlowObservationSnapshot::new()),
        verter_semantic::resolver_core::ResolutionBasis::unbound_placeholder(),
    );
    // An alternate dispatch entry must not exist.
    let _ = core.attempt_unchecked(&NonFlowOperation::ProjectExposeSurface {
        owner_canonical: Arc::from("/App.vue"),
        macro_index: 0,
    });
    // Neither may a caller synthesize an outcome outside the kernel.
    let _ = NonFlowPayload::from_operation(&NonFlowOperation::ProjectExposeSurface {
        owner_canonical: Arc::from("/App.vue"),
        macro_index: 0,
    });
}
