//! Workspace file-operation capability slot: create/delete watcher filters.

use super::{carrier_watch_glob, CapabilityContext};
use tower_lsp_server::ls_types::{
    FileOperationFilter, FileOperationPattern, FileOperationRegistrationOptions,
    ServerCapabilities, WorkspaceFileOperationsServerCapabilities,
};

pub(crate) fn contribute(caps: &mut ServerCapabilities, ctx: &CapabilityContext) {
    caps.workspace
        .get_or_insert_with(Default::default)
        .file_operations = Some(WorkspaceFileOperationsServerCapabilities {
        did_create: Some(FileOperationRegistrationOptions {
            filters: vec![FileOperationFilter {
                scheme: Some("file".to_string()),
                pattern: FileOperationPattern {
                    glob: carrier_watch_glob(ctx.framework),
                    matches: None,
                    options: None,
                },
            }],
        }),
        did_delete: Some(FileOperationRegistrationOptions {
            filters: vec![FileOperationFilter {
                scheme: Some("file".to_string()),
                pattern: FileOperationPattern {
                    glob: carrier_watch_glob(ctx.framework),
                    matches: None,
                    options: None,
                },
            }],
        }),
        ..Default::default()
    });
}
