use std::sync::Arc;

use crate::ambient_lib::{AmbientLibError, AmbientLibSpec, AmbientLibsByProject, AmbientSymbolHit};
use crate::project_key::ProjectStableKey;
use crate::types::{
    ExactResolution, ExactResolutionResult, FileKind, PackageManifest, ParsedEdge,
    ProjectOwnership, ResolutionContext, ResolveResult,
};
use crate::workspace_snapshot::ProjectId;

/// Lightweight resource snapshot for first-class Rust audit.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceResourceSnapshot {
    pub overlay_entries: usize,
    pub overlay_bytes: u64,
    pub snapshot_entries: usize,
    pub snapshot_bytes: u64,
    pub edge_file_count: usize,
    pub reverse_dep_bucket_count: usize,
    pub package_manifest_count: usize,
    pub published_project_count: usize,
}

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

    /// Trace-only detail about the most recent `read_file()` on the current
    /// thread for this canonical path, if the workspace can provide it.
    ///
    /// This is intended for high-level trace events that want to preserve the
    /// concrete VFS layer/cache result without re-reading the file.
    fn take_last_read_file_trace_detail(&self, _canonical_id: &str) -> Option<String> {
        None
    }

    /// Check whether a file exists. In Filesystem mode, probes disk on miss.
    fn file_exists(&self, canonical_id: &str) -> bool;

    /// Resolve symlinks to real path.
    fn realpath(&self, canonical_id: &str) -> Option<String>;

    /// Read and parse a `package.json` manifest.
    ///
    /// Concrete workspaces can override this to add caching. The default
    /// implementation reads the file through `read_file()` and parses it
    /// directly.
    fn read_package_manifest(&self, canonical_id: &str) -> Option<PackageManifest> {
        let source = self.read_file(canonical_id)?;
        Some(crate::package_index::parse_package_json(&source))
    }

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

    /// Resolve an import specifier against an explicit owning project.
    ///
    /// This is used for project-scoped lookups that are not naturally rooted at
    /// a real source file, such as resolving `vue/jsx` for fallthrough
    /// intrinsics. Implementations should honor the same project-level tsconfig,
    /// alias, and package resolution rules as `resolve_import()`, without
    /// fabricating an importer path.
    fn resolve_import_for_project(
        &self,
        _owner: &ProjectOwnership,
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

    /// Monotonic content generation. Bumped when workspace file content or
    /// overlays change, so long-lived consumers can invalidate cached reads.
    fn content_generation(&self) -> u64 {
        0
    }

    /// Point-in-time VFS provenance counters for observability and benchmarks.
    fn vfs_provenance_snapshot(&self) -> crate::types::VfsProvenanceSnapshot {
        crate::types::VfsProvenanceSnapshot::default()
    }

    /// Reset VFS provenance counters.
    fn reset_vfs_provenance(&self) {}

    /// Point-in-time resource snapshot for native audit.
    fn resource_snapshot(&self) -> WorkspaceResourceSnapshot {
        WorkspaceResourceSnapshot::default()
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

    // ── Audit sink registry (plan §2.4 / Commit 4) ──

    /// Register a VFS audit sink. The returned handle is deregister-able
    /// via [`deregister_audit_sink`]. Default: `NotSupported`.
    /// Concrete workspaces override to maintain a per-sink registry.
    fn register_audit_sink(
        &self,
        _sink: Arc<dyn crate::audit_sink::VfsAuditSink>,
    ) -> Result<crate::audit_sink::SinkHandle, crate::audit_sink::AuditSinkError> {
        Err(crate::audit_sink::AuditSinkError::NotSupported)
    }

    /// Deregister a previously-registered VFS audit sink. Default:
    /// `NotSupported`. Concrete workspaces override to complete the
    /// RAII-style registration lifecycle.
    fn deregister_audit_sink(
        &self,
        _handle: crate::audit_sink::SinkHandle,
    ) -> Result<(), crate::audit_sink::AuditSinkError> {
        Err(crate::audit_sink::AuditSinkError::NotSupported)
    }

    // ── Ambient TypeScript lib registry (Phase 5 §6 — A1) ──
    //
    // All methods live on the base trait so `VerterHost::workspace()` can reach
    // them through `Arc<dyn WorkspaceAccess>`. Default impls return
    // `NotBootstrapped` / `None` for backends without ambient lib support.

    /// Register an ambient TypeScript lib (e.g. `lib.es5.d.ts`) for a project.
    ///
    /// Idempotent on `(project, canonical_id, content_hash)`. New content for
    /// the same canonical_id replaces the existing entry and bumps the content
    /// generation so dep-fact validators re-execute.
    ///
    /// Default: `Err(NotBootstrapped)`.
    fn register_ambient_lib(&self, _spec: AmbientLibSpec) -> Result<(), AmbientLibError> {
        Err(AmbientLibError::NotBootstrapped)
    }

    /// Unregister an ambient lib by `(stable_key, canonical_id)`.
    /// Default: `Err(NotBootstrapped)`.
    fn unregister_ambient_lib(
        &self,
        _stable_key: ProjectStableKey,
        _canonical_id: &str,
    ) -> Result<(), AmbientLibError> {
        Err(AmbientLibError::NotBootstrapped)
    }

    /// Read an ambient lib's source by `(stable_key, canonical_id)`.
    ///
    /// Returns `None` when the canonical_id is shadowed by a non-ambient user
    /// file (overlay or snapshot) — see A5 user-wins shadowing.
    ///
    /// Default: `None`.
    fn read_ambient_lib(
        &self,
        _stable_key: ProjectStableKey,
        _canonical_id: &str,
    ) -> Option<Arc<str>> {
        None
    }

    /// Compute the project-scoped ambient virtual canonical id for a
    /// `(stable_key, canonical_id)` pair (`ambient:/<tag>/<canonical>`).
    ///
    /// Default falls through to the helper in [`crate::ambient_lib`] so all
    /// backends produce the same id for the same inputs.
    fn ambient_virtual_canonical_id(
        &self,
        stable_key: ProjectStableKey,
        canonical_id: &str,
    ) -> Arc<str> {
        crate::ambient_lib::ambient_virtual_canonical_id(stable_key, canonical_id)
    }

    /// Record a session-side reverse-dep edge from a consumer file to the
    /// ambient virtual id. Re-registration of the lib bumps the content
    /// generation so `HostFenceValidator` invalidates downstream caches.
    /// Default: no-op.
    fn record_ambient_dependency(&self, _consumer: &str, _virtual_id: &str) {}

    /// Resolve a `ProjectId` (snapshot index) to its stable key for ambient
    /// lookups. Returns `None` when the workspace is not yet published or the
    /// id is unknown. Default: `None`.
    fn project_stable_key(&self, _project_id: ProjectId) -> Option<ProjectStableKey> {
        None
    }

    /// O(1) symbol-name lookup against the registered ambient libs for a
    /// given consumer project. Used by the bare-name resolver as a fallback
    /// when a symbol is not present in scope or in the import graph.
    ///
    /// Default: `None`.
    fn lookup_ambient_symbol(
        &self,
        _consumer_project: ProjectStableKey,
        _symbol: &str,
    ) -> Option<AmbientSymbolHit> {
        None
    }

    /// Lock-free read of the engine's ambient lib registry, used by host-side
    /// validators (e.g., `HostFenceValidator`'s ambient `WholeHash` arm).
    /// Backends that don't support ambient libs return an empty registry.
    fn ambient_libs_view(&self) -> Arc<AmbientLibsByProject> {
        Arc::new(AmbientLibsByProject::default())
    }
}

// ── Scheduler-oriented traits ──

/// Read-only file loading interface for the scheduler's I/O pool.
///
/// Implementations check overlay first, then fall back to disk (or memory).
/// All methods are sync — they run on the scheduler's bounded I/O pool,
/// isolated from the CPU pool.
///
/// Unlike [`WorkspaceAccess`], this trait has no resolution, edge recording,
/// or mutation methods. It is the minimal interface needed for the scheduler's
/// Source stage to load file content.
pub trait SourceLoader: Send + Sync {
    /// Load file content by canonical ID. Returns `None` if the file doesn't exist.
    fn load(&self, canonical_id: &str) -> Option<Arc<str>>;

    /// Check whether a file exists.
    fn exists(&self, canonical_id: &str) -> bool;

    /// Classify a file by extension.
    fn classify(&self, canonical_id: &str) -> FileKind;

    /// Resolve symlinks to real path.
    fn realpath(&self, canonical_id: &str) -> Option<String>;
}

/// Read-only snapshot of project resolution state.
///
/// Provides import resolution and project ownership queries without
/// mutation capabilities. The scheduler holds this via `ArcSwap` so it
/// can be atomically replaced when project configuration changes.
///
/// Implementors: [`ProjectResolver`](crate::resolver::ProjectResolver),
/// `EmptyResolverSnapshot` (for standalone/test hosts).
pub trait ResolverSnapshot: Send + Sync {
    /// Resolve an import specifier in context.
    fn resolve_import(
        &self,
        importer: &str,
        specifier: &str,
        ctx: ResolutionContext,
    ) -> Option<ResolveResult>;

    /// Compute the preferred alias-based specifier for auto-imports.
    fn preferred_specifier(&self, importer: &str, target: &str) -> Option<String>;

    /// Find the owning project for a file.
    fn owner_for_file(&self, id: &str) -> Option<ProjectOwnership>;

    /// Monotonic generation counter. Bumped when project configuration changes.
    fn generation(&self) -> u64;
}

/// Empty resolver that resolves nothing. Used by standalone hosts and tests.
pub struct EmptyResolverSnapshot;

impl ResolverSnapshot for EmptyResolverSnapshot {
    fn resolve_import(
        &self,
        _importer: &str,
        _specifier: &str,
        _ctx: ResolutionContext,
    ) -> Option<ResolveResult> {
        None
    }

    fn preferred_specifier(&self, _importer: &str, _target: &str) -> Option<String> {
        None
    }

    fn owner_for_file(&self, _id: &str) -> Option<ProjectOwnership> {
        None
    }

    fn generation(&self) -> u64 {
        0
    }
}

#[cfg(test)]
mod ambient_default_tests {
    //! Phase 5 §6.1 / A1 default-trait surface tests.
    //!
    //! These confirm that `WorkspaceAccess` has the ambient lib registration
    //! API and that backends without ambient support return
    //! `Err(NotBootstrapped)` / `None` from the defaults. These are
    //! discriminating: pre-change tree (no ambient methods on the trait) does
    //! not even compile.
    use std::sync::Arc;

    use super::WorkspaceAccess;
    use crate::ambient_lib::{AmbientLibError, AmbientLibSpec};
    use crate::project_key::ProjectStableKey;

    /// Minimal backend that opts out of ambient lib support — exercises the
    /// trait defaults.
    struct StubWs;

    impl WorkspaceAccess for StubWs {
        fn read_file(&self, _id: &str) -> Option<Arc<str>> {
            None
        }
        fn file_exists(&self, _id: &str) -> bool {
            false
        }
        fn realpath(&self, _id: &str) -> Option<String> {
            None
        }
    }

    #[test]
    fn default_register_ambient_lib_returns_not_bootstrapped() {
        let ws = StubWs;
        let spec = AmbientLibSpec {
            project_id: None,
            canonical_id: Arc::from("lib.es5.d.ts"),
            source: Arc::from("export {};"),
        };
        let err = ws.register_ambient_lib(spec).unwrap_err();
        assert_eq!(
            err,
            AmbientLibError::NotBootstrapped,
            "default impl MUST surface NotBootstrapped"
        );
    }

    #[test]
    fn default_read_ambient_lib_returns_none() {
        let ws = StubWs;
        let key = ProjectStableKey::Configured([0u8; 16]);
        assert!(
            ws.read_ambient_lib(key, "lib.es5.d.ts").is_none(),
            "default impl MUST return None"
        );
    }

    #[test]
    fn default_lookup_ambient_symbol_returns_none() {
        let ws = StubWs;
        let key = ProjectStableKey::Configured([0u8; 16]);
        assert!(
            ws.lookup_ambient_symbol(key, "Pick").is_none(),
            "default impl MUST return None"
        );
    }

    #[test]
    fn default_ambient_libs_view_is_empty() {
        let ws = StubWs;
        let view = ws.ambient_libs_view();
        assert!(
            view.by_project.is_empty(),
            "default impl MUST return empty registry"
        );
    }

    #[test]
    fn default_ambient_virtual_canonical_id_uses_helper() {
        let ws = StubWs;
        let key = ProjectStableKey::Configured([0xAB; 16]);
        let virt = ws.ambient_virtual_canonical_id(key, "lib.es5.d.ts");
        let s: &str = &virt;
        assert!(s.starts_with("ambient:/C"), "got {s}");
        assert!(s.ends_with("/lib.es5.d.ts"), "got {s}");
    }
}
