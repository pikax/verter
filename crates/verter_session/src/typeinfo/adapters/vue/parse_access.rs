//! The blessed Vue parse-carrier accessor.
//!
//! [`vue_parse`] is the ONE way session code reaches a Vue
//! [`ParsedSfc`] out of the framework-neutral
//! [`FrameworkParseArtifact`]: it opens the artifact through the Vue
//! adapter's own registered-projector opener
//! (`vue_bridge::open_vue_carrier`) and downcasts the erased carrier —
//! no capability token, since the opener only ever opens a Vue-adapter
//! artifact.

use std::sync::Arc;

use verter_compiler::framework_common::vue_bridge::{open_vue_carrier, VueParseCarrier};
use verter_compiler::framework_common::FrameworkParseArtifact;
use verter_compiler::parser::types::ParsedSfc;

/// Typed access to a Vue parse artifact's [`ParsedSfc`].
///
/// Returns `None` for a non-Vue artifact (the opener only opens Vue
/// artifacts) or when the erased payload is not a `VueParseCarrier`. The ONE
/// accessor every session `ParsedSfc` reader routes through.
pub(crate) fn vue_parse(artifact: &FrameworkParseArtifact) -> Option<Arc<ParsedSfc>> {
    let carrier = open_vue_carrier(artifact)?
        .__verter_as_any_arc()
        .downcast::<VueParseCarrier>()
        .ok()?;
    Some(Arc::clone(carrier.parsed_arc()))
}
