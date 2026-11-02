//! Document-link capability slot.

use super::CapabilityContext;
use tower_lsp_server::ls_types::{DocumentLinkOptions, ServerCapabilities};

pub(crate) fn contribute(caps: &mut ServerCapabilities, _ctx: &CapabilityContext) {
    caps.document_link_provider = Some(DocumentLinkOptions {
        resolve_provider: Some(false),
        work_done_progress_options: Default::default(),
    });
}
