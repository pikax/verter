use std::sync::Arc;

use verter_semantic::type_info::{NonFlowObservationSnapshot, NonFlowOperation, TypeInfoCore};

fn main() {
    let core = TypeInfoCore::from_observation_snapshot(
        Arc::new(NonFlowObservationSnapshot::new()),
        verter_session_query::resolution::ResolutionBasis::unbound_placeholder(),
    );
    let _ = core.attempt(&NonFlowOperation::ProjectVueMacroSemantics {
        owner_canonical: Arc::from("/App.vue"),
    });
}
