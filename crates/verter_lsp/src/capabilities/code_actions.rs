//! Code-action capability slot: the advertised action kinds.

use super::CapabilityContext;
use tower_lsp_server::ls_types::{
    CodeActionKind, CodeActionOptions, CodeActionProviderCapability, ServerCapabilities,
};

pub(crate) fn contribute(caps: &mut ServerCapabilities, _ctx: &CapabilityContext) {
    caps.code_action_provider = Some(CodeActionProviderCapability::Options(CodeActionOptions {
        code_action_kinds: Some(vec![
            CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
            CodeActionKind::QUICKFIX,
            CodeActionKind::REFACTOR,
            CodeActionKind::REFACTOR_EXTRACT,
        ]),
        ..Default::default()
    }));
}
