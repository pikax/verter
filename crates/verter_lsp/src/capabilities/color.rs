//! Colour-presentation capability slot.

use super::CapabilityContext;
use tower_lsp_server::ls_types::{ColorProviderCapability, ServerCapabilities};

pub(crate) fn contribute(caps: &mut ServerCapabilities, _ctx: &CapabilityContext) {
    caps.color_provider = Some(ColorProviderCapability::Simple(true));
}
