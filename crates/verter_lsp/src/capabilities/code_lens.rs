//! Code-lens capability slot.

use super::CapabilityContext;
use tower_lsp_server::ls_types::{CodeLensOptions, ServerCapabilities};

pub(crate) fn contribute(caps: &mut ServerCapabilities, _ctx: &CapabilityContext) {
    caps.code_lens_provider = Some(CodeLensOptions {
        resolve_provider: Some(false),
    });
}
