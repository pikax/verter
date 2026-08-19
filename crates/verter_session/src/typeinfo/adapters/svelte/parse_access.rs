//! The blessed Svelte parse-carrier accessor.
//!
//! [`svelte_parse`] is the ONE way session code reaches a
//! [`ParsedSvelte`](verter_compiler::svelte::ParsedSvelte) out of the
//! framework-neutral [`FrameworkParseArtifact`]: it opens the artifact
//! through the Svelte adapter's own registered-projector opener
//! (`svelte::carrier::open_svelte_carrier`) and downcasts the erased
//! carrier — no capability token, since the opener only ever opens a
//! Svelte-adapter artifact.

use std::sync::Arc;

use verter_compiler::framework_common::FrameworkParseArtifact;
use verter_compiler::svelte::carrier::{open_svelte_carrier, SvelteParseCarrier};
use verter_compiler::svelte::ParsedSvelte;

/// Typed access to a Svelte parse artifact's [`ParsedSvelte`].
///
/// Returns `None` for a non-Svelte artifact (the opener only opens Svelte
/// artifacts) or when the erased payload is not a `SvelteParseCarrier`. The
/// ONE accessor every session `ParsedSvelte` reader routes through.
pub(crate) fn svelte_parse(artifact: &FrameworkParseArtifact) -> Option<Arc<ParsedSvelte>> {
    let carrier = open_svelte_carrier(artifact)?
        .__verter_as_any_arc()
        .downcast::<SvelteParseCarrier>()
        .ok()?;
    Some(Arc::clone(carrier.parsed_arc()))
}
