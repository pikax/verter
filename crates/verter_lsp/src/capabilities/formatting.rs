//! Whole-document formatting capability slot.

use super::CapabilityContext;
use tower_lsp_server::ls_types::{OneOf, ServerCapabilities};

pub(crate) fn contribute(caps: &mut ServerCapabilities, _ctx: &CapabilityContext) {
    caps.document_formatting_provider = Some(OneOf::Left(true));
}
