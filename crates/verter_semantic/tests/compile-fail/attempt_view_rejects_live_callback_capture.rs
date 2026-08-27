//! The sanctioned resolver attempt view is immutable data. Its construction
//! surface must not accept a callback that can retain and observe live host
//! state after the attempt begins.

use std::sync::Arc;

use verter_semantic::resolver_core::{AttemptOutcome, ResolverAttemptView};

struct HostShapedState {
    generation: u64,
}

fn main() {
    let host = Arc::new(HostShapedState { generation: 7 });
    let _view = ResolverAttemptView::new().with_project_generation(move || {
        AttemptOutcome::Complete(host.generation)
    });
}
