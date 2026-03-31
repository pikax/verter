use std::sync::Arc;

/// A change event applied to the workspace.
#[derive(Debug, Clone)]
pub enum WorkspaceChange {
    /// File content changed (source = None means re-read from disk in Filesystem mode).
    FileChanged {
        canonical_id: String,
        source: Option<Arc<str>>,
    },
    /// File was deleted.
    FileDeleted { canonical_id: String },
    /// Mark a directory tree dirty so the next access refreshes cached membership.
    DirectoryTreeDirty { prefix: String },
    /// Config file changed (triggers project graph rebuild).
    ConfigChanged { canonical_id: String },
    /// Set an overlay (active editor content, takes priority over disk/snapshot).
    OverlaySet {
        canonical_id: String,
        source: Arc<str>,
    },
    /// Clear an overlay (revert to snapshot/disk).
    OverlayClear { canonical_id: String },
}

/// Result of applying changes to the workspace.
#[derive(Debug, Clone, Default)]
pub struct ChangeResult {
    /// Files whose source changed — need recompilation.
    pub invalidated_files: Vec<String>,
    /// Whether the project graph was rebuilt (config change).
    pub graph_rebuilt: bool,
    /// New project graph generation (if rebuilt).
    pub generation: Option<u64>,
    /// Ownership diff (only populated when graph_rebuilt = true).
    pub ownership_diff: Option<OwnershipDiff>,
}

/// Diff of file ownership after a project graph rebuild.
#[derive(Debug, Clone, Default)]
pub struct OwnershipDiff {
    /// Files that now belong to a project (were previously unowned).
    pub newly_owned: Vec<OwnedFileInfo>,
    /// Files that lost their owning project.
    pub no_longer_owned: Vec<String>,
    /// Files whose owning project changed (different tsconfig/root).
    pub owner_changed: Vec<OwnedFileInfo>,
    /// Files whose resolver config changed (aliases, paths) without owner change.
    pub resolver_changed: Vec<OwnedFileInfo>,
}

/// Information about a file's project ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedFileInfo {
    pub canonical_id: String,
    pub project_root: String,
    pub tsconfig_path: Option<String>,
}
