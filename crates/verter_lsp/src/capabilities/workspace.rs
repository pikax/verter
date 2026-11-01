//! Workspace-folders capability slot: folder support and change
//! notifications.

use super::CapabilityContext;
use tower_lsp_server::ls_types::{OneOf, ServerCapabilities, WorkspaceFoldersServerCapabilities};

pub(crate) fn contribute(caps: &mut ServerCapabilities, _ctx: &CapabilityContext) {
    caps.workspace
        .get_or_insert_with(Default::default)
        .workspace_folders = Some(WorkspaceFoldersServerCapabilities {
        supported: Some(true),
        change_notifications: Some(OneOf::Left(true)),
    });
}
