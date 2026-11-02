//! On-type-formatting capability slot.

use super::CapabilityContext;
use tower_lsp_server::ls_types::{DocumentOnTypeFormattingOptions, ServerCapabilities};

/// Proactive tag auto-close fires on the `>` that closes an open tag.
/// The handler (`auto_close_tag`) requires a `>` immediately before the
/// cursor, so `>` is the only trigger it can act on — no `/` more-trigger.
pub(crate) fn contribute(caps: &mut ServerCapabilities, _ctx: &CapabilityContext) {
    caps.document_on_type_formatting_provider = Some(DocumentOnTypeFormattingOptions {
        first_trigger_character: ">".to_string(),
        more_trigger_character: None,
    });
}
