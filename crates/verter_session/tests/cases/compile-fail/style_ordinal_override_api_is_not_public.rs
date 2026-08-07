//! Public consumers must use `BlockOverrideRequest`, whose entries carry a
//! sealed block token plus revision, artifact, basis, source-space, and hash
//! stamps. This fixture deliberately tries to recover the retired ordinal-only
//! style lane; it must remain a compile error.

use std::sync::Arc;

use verter_session::{CompileProfile, StyleOverrideEntry, StyleOverrideRequest, VerterHost};

fn invoke_retired_lane(host: &VerterHost) {
    let _ = host.apply_style_overrides(StyleOverrideRequest {
        canonical_id: "/src/App.vue".to_string(),
        compile_profile: CompileProfile::default(),
        overrides: vec![StyleOverrideEntry {
            index: 0,
            code: Arc::from(".root {}"),
            source_map: None,
        }],
    });
}

fn main() {}
