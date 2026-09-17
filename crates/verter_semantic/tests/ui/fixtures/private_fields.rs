//! C2-GAP3-PRIVATE-FIELDS: kernel payload values are minted only by the
//! gateway — outside `verter_semantic` neither construction nor field
//! writes compile.

use std::sync::Arc;

use verter_semantic::type_info::{NonFlowOperation, TypeInfoCore, VueMacroSemanticInput};

fn main() {
    // Constructing a payload directly must not compile.
    let _plan = VueMacroSemanticInput {
        owner_canonical: Arc::from("/App.vue"),
        demands: Vec::new(),
    };
    // Reading through the gateway is the sanctioned path.
    let core = TypeInfoCore::from_observation_snapshot(
        Arc::new(verter_semantic::type_info::NonFlowObservationSnapshot::new()),
        verter_semantic::resolver_core::ResolutionBasis::unbound_placeholder(),
    );
    let _ = core.attempt(&NonFlowOperation::ProjectVueMacroSemantics {
        owner_canonical: Arc::from("/App.vue"),
    });
}
