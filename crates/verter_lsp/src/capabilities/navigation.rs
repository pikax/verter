//! Navigation capability slot: selection ranges and linked editing.

use super::CapabilityContext;
use tower_lsp_server::ls_types::{
    LinkedEditingRangeServerCapabilities, SelectionRangeProviderCapability, ServerCapabilities,
};

pub(crate) fn contribute(caps: &mut ServerCapabilities, _ctx: &CapabilityContext) {
    caps.selection_range_provider = Some(SelectionRangeProviderCapability::Simple(true));
    caps.linked_editing_range_provider = Some(LinkedEditingRangeServerCapabilities::Simple(true));
}
