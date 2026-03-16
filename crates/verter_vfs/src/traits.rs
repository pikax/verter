use std::sync::Arc;

use crate::types::{FileKind, ParsedEdge, ProjectOwnership, ResolveRequestKind, ResolveResult};

/// Workspace access trait for the compilation host.
///
/// Provides file reads, resolution, AND edge recording.
/// Lives in verter_vfs — no dependency on verter_host types.
///
/// This is the read-only view that the host sees. Mutations like
/// `apply_changes()` and `set_exact_resolutions()` are called directly
/// on `FilesystemWorkspace` or `MemoryWorkspace` by the consumer (LSP, MCP, unplugin).
///
/// # Thread safety
///
/// This trait requires `Send + Sync` because the host may be shared across
/// threads (e.g., in the LSP server with tokio). Implementations must ensure
/// interior mutability is thread-safe (e.g., using `parking_lot::RwLock`).
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
    fn classify_file(&self, canonical_id: &str) -> FileKind;

    // ── Resolution ──

    /// Resolve an import specifier. All resolution policy is internal:
    /// exact resolutions (authoritative, suppress fallthrough) → project
    /// resolver → fallback probing. Host never does its own heuristic
    /// resolution when this returns `None`.
    fn resolve_import(
        &self,
        importer_id: &str,
        specifier: &str,
        kind: ResolveRequestKind,
    ) -> Option<ResolveResult>;

    /// Find the owning project for a file.
    fn owner_for_file(&self, canonical_id: &str) -> Option<ProjectOwnership>;

    // ── Edge recording (called by host during upsert) ──

    /// Record parsed edges from a file's imports. Eagerly resolves
    /// `Relative` and `ExternalSrc` edges via `resolve_import()`. Stores
    /// `Bare` specifiers. Clears `exact_resolutions` for the file.
    /// Replaces `resolved_deps`. Updates reverse-dep graph.
    fn record_parsed_edges(&self, canonical_id: &str, edges: &[ParsedEdge]);

    /// Query reverse deps (files that import this file).
    fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String>;

    /// Query forward deps (files this file imports).
    fn forward_deps_for(&self, canonical_id: &str) -> Vec<String>;
}
