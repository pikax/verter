use std::sync::Arc;

use crate::types::{
    ExactResolution, ExactResolutionResult, FileKind, ParsedEdge, ProjectOwnership,
    ResolutionContext, ResolveResult,
};

/// Single workspace trait — sole authority for file access and resolution.
///
/// All workspace I/O (reads, writes, walks, resolution) goes through this
/// trait. There is no separate `ProjectResolverReader` or `ConfigFileReader`.
/// The resolver, config parser, and host all take `&dyn WorkspaceAccess`.
///
/// # Implementors
///
/// - [`FilesystemWorkspace`] — disk-backed with overlay/snapshot cache
/// - [`MemoryWorkspace`] — fully in-memory (tests, WASM, playground)
/// - Lightweight adapters (LSP readers) that delegate to a host's workspace
///
/// # Minimal implementation
///
/// Only [`read_file`], [`file_exists`], and [`realpath`] are required for
/// a lightweight adapter (e.g., for the project resolver). All other methods
/// have default implementations.
///
/// # Thread safety
///
/// Requires `Send + Sync` — implementations must use interior mutability
/// (e.g., `parking_lot::RwLock`) for thread-safe state.
pub trait WorkspaceAccess: Send + Sync {
    // ── File reads ──

    /// Read file content. Returns overlay content if set, otherwise
    /// snapshot/disk content. Returns `None` if the file doesn't exist.
    fn read_file(&self, canonical_id: &str) -> Option<Arc<str>>;

    /// Check whether a file exists. In Filesystem mode, probes disk on miss.
    fn file_exists(&self, canonical_id: &str) -> bool;

    /// Resolve symlinks to real path.
    fn realpath(&self, canonical_id: &str) -> Option<String>;

    /// Classify a file by extension.
    /// Default: classifies `.vue` as `VueSfc`, everything else as `NonSfc`.
    fn classify_file(&self, canonical_id: &str) -> FileKind {
        if canonical_id.ends_with(".vue") {
            FileKind::VueSfc
        } else {
            FileKind::NonSfc
        }
    }

    // ── Resolution ──

    /// Resolve an import specifier with full context.
    ///
    /// The `ctx` determines which target a specifier resolves to. Different
    /// `(phase, kind)` combinations produce different package.json condition
    /// chains and legacy-field lookups. Host never does its own heuristic
    /// resolution when this returns `None`.
    ///
    /// Default: `None` (no resolution).
    fn resolve_import(
        &self,
        _importer_id: &str,
        _specifier: &str,
        _ctx: ResolutionContext,
    ) -> Option<ResolveResult> {
        None
    }

    /// Find the owning project for a file.
    /// Default: `None` (no project ownership).
    fn owner_for_file(&self, _canonical_id: &str) -> Option<ProjectOwnership> {
        None
    }

    /// Compute the preferred alias-based import specifier for a target file.
    ///
    /// Returns the shortest tsconfig-path or workspace-alias specifier that
    /// round-trips back to `target_id` via `resolve_import()`. Returns `None`
    /// if no alias matches or the importer is unowned.
    fn preferred_specifier(&self, _importer_id: &str, _target_id: &str) -> Option<String> {
        None
    }

    // ── Edge recording (called by host during upsert) ──

    /// Record parsed edges from a file's imports. Eagerly resolves
    /// `Relative` and `ExternalSrc` edges via `resolve_import()`. Stores
    /// `Bare` specifiers. Clears `exact_resolutions` for the file.
    /// Replaces `resolved_deps`. Updates reverse-dep graph.
    /// Default: no-op.
    fn record_parsed_edges(&self, _canonical_id: &str, _edges: &[ParsedEdge]) {}

    /// Query reverse deps (files that import this file).
    /// Default: empty.
    fn reverse_deps_for(&self, _canonical_id: &str) -> Vec<String> {
        Vec::new()
    }

    /// Query forward deps (files this file imports).
    /// Default: empty.
    fn forward_deps_for(&self, _canonical_id: &str) -> Vec<String> {
        Vec::new()
    }

    // ── Mutation methods (called by host for backward compat) ──

    /// Set exact resolutions for a file (authoritative specifier->canonical_id).
    /// Called by the host when `set_import_dependencies()` is used.
    /// Default: no-op. Concrete workspaces override to delegate to EdgeStore.
    fn set_exact_resolutions(
        &self,
        _canonical_id: &str,
        _resolutions: Vec<ExactResolution>,
    ) -> ExactResolutionResult {
        ExactResolutionResult::default()
    }

    /// Notify the workspace that a file was upserted into the host.
    ///
    /// Sets an overlay so the VFS resolver can find open/in-memory files that
    /// may not yet exist on disk. Called by the host during `upsert()`.
    /// Default: no-op (MemoryWorkspace manages its own snapshot).
    fn notify_upsert(&self, _canonical_id: &str, _source: Arc<str>) {}

    /// Notify the workspace that an editor buffer was closed.
    ///
    /// Clears the overlay AND invalidates the snapshot cache so the next
    /// read falls through to disk (picking up any saves made while the
    /// overlay was active). Default: no-op.
    fn notify_close(&self, _canonical_id: &str) {}

    /// Notify the workspace that a file was deleted.
    ///
    /// Clears overlay, removes snapshot, and removes edge-store data so
    /// the file is no longer resolvable or tracked. Default: no-op.
    fn notify_delete(&self, _canonical_id: &str) {}

    /// Configure the project resolver from a list of project configs.
    /// Called by the host when `configure_projects()` is used.
    /// Default: no-op. Concrete workspaces override to rebuild the resolver.
    fn configure_resolver(&self, _projects: Vec<crate::resolver::IdeProjectConfig>) {}

    // ── Directory and mutation operations ──

    /// List entries in a directory.
    /// Default: `Err(UnsupportedOperation)`.
    fn read_dir(&self, _dir: &str) -> Result<Vec<crate::error::DirEntry>, crate::error::VfsError> {
        Err(crate::error::VfsError::UnsupportedOperation("read_dir"))
    }

    /// Recursively walk a directory tree, filtering directories and files.
    /// Returns canonical paths of matching files.
    /// Default: `Err(UnsupportedOperation)`.
    fn walk(
        &self,
        _root: &str,
        _filter_dir: &dyn Fn(&str) -> bool,
        _filter_file: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<String>, crate::error::VfsError> {
        Err(crate::error::VfsError::UnsupportedOperation("walk"))
    }

    /// Write file content. Creates parent directories as needed.
    /// Default: `Err(UnsupportedOperation)`.
    fn write_file(&self, _path: &str, _content: &str) -> Result<(), crate::error::VfsError> {
        Err(crate::error::VfsError::UnsupportedOperation("write_file"))
    }

    /// Create a directory and all parent directories.
    /// Default: `Err(UnsupportedOperation)`.
    fn create_dir_all(&self, _path: &str) -> Result<(), crate::error::VfsError> {
        Err(crate::error::VfsError::UnsupportedOperation(
            "create_dir_all",
        ))
    }

    /// Delete a file.
    /// Default: `Err(UnsupportedOperation)`.
    fn delete_file(&self, _path: &str) -> Result<(), crate::error::VfsError> {
        Err(crate::error::VfsError::UnsupportedOperation("delete_file"))
    }

    /// Delete a directory and all its contents.
    /// Default: `Err(UnsupportedOperation)`.
    fn delete_dir_all(&self, _path: &str) -> Result<(), crate::error::VfsError> {
        Err(crate::error::VfsError::UnsupportedOperation(
            "delete_dir_all",
        ))
    }

    /// Copy a file from `src` to `dst`.
    /// Default: `Err(UnsupportedOperation)`.
    fn copy_file(&self, _src: &str, _dst: &str) -> Result<(), crate::error::VfsError> {
        Err(crate::error::VfsError::UnsupportedOperation("copy_file"))
    }

    /// Check whether a path is a directory.
    fn is_dir(&self, _path: &str) -> bool {
        false
    }
}
