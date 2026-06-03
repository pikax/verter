use std::collections::BTreeSet;
use std::sync::Arc;

use verter_audit::files::{FileAudit, FileRole};
use verter_audit::origin_graph::VfsLayer;
use verter_audit::payloads::WorkspacePayload;
use verter_audit::{
    RequestAuditRecord, RequestKind, RequestKindPayload, RequestMemoryAudit, RequestStoreAudit,
    RequestTimingAudit, WorkspaceOp,
};

use verter_scheduler::invalidation::Hash16;

use crate::ambient_lib::{AmbientLibError, AmbientLibSpec, AmbientLibsByProject, AmbientSymbolHit};
use crate::exact_resolution::DependencySnapshotView;
use crate::project_key::ProjectStableKey;
use crate::published_state::ProjectEnvHashArray;
use crate::types::{
    ExactResolution, ExactResolutionResult, FileKind, PackageManifest, ParsedEdge,
    ProjectOwnership, ResolutionContext, ResolvePhase, ResolveRequestKind, ResolveResult,
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

/// Read-only view of the workspace authority.
///
/// `WorkspaceRead` carries every method that does NOT mutate workspace state
/// (file reads, resolution, ownership, generation, queries, ambient-lib
/// lookups). It is the public surface of the workspace exposed to external
/// crates via [`VerterHost::workspace_read`](`crate::WorkspaceRead`); the
/// mutator surface lives on [`WorkspaceAccess`] (which extends
/// `WorkspaceRead`) and is gated behind `pub(crate) workspace()`.
///
/// **Trait-upcasting requires Rust 1.86.** The workspace `Cargo.toml`
/// declares `rust-version = "1.86"`; this lets `Arc<dyn WorkspaceAccess>`
/// upcast to `Arc<dyn WorkspaceRead>` without a manual `as_read_arc`
/// conversion.
///
/// # Implementation note
///
/// Concrete workspaces (`FilesystemWorkspace`, `MemoryWorkspace`, etc.)
/// implement BOTH traits. The split is mechanical: read-method bodies live
/// on `impl WorkspaceRead for X`, mutator-method bodies on
/// `impl WorkspaceAccess for X`. The `WorkspaceAccess: WorkspaceRead`
/// supertrait bound ensures any `&dyn WorkspaceAccess` can be implicitly
/// used as a `&dyn WorkspaceRead`.
pub trait WorkspaceRead: Send + Sync {
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

    /// Whether `canonical_id` is a workspace-owned source file.
    ///
    /// Routes through the resolver's existing ownership classification:
    /// - True when the file (or its `realpath` resolution) sits inside
    ///   any registered project's `root`. This includes:
    ///   - regular workspace packages,
    ///   - workspace packages that happen to live under `node_modules/`,
    ///   - pnpm-symlink hops where `realpath()` resolves a
    ///     `node_modules/.pnpm/...` path back to a workspace location.
    /// - False otherwise (third-party `node_modules` packages, paths
    ///   outside every registered project).
    ///
    /// Callers MUST NOT substitute `path.contains("/node_modules/")`
    /// for this method — that heuristic mis-classifies every
    /// pnpm-symlink and workspace-inside-node_modules case.
    ///
    /// Default: `false` (no project ownership).
    fn is_workspace_owned(&self, _canonical_id: &str) -> bool {
        false
    }

    /// Whether `canonical_id` is backed by a third-party package
    /// installation (i.e., reachable through `node_modules` and NOT
    /// claimed by any registered workspace project).
    ///
    /// Routes through the resolver's existing ownership classification:
    /// - True when the realpath sits under `node_modules/` AND no
    ///   registered project root claims the file.
    /// - False for workspace sources, pnpm-symlink hops that resolve
    ///   into a workspace project, and paths outside any
    ///   `node_modules` directory.
    ///
    /// Default: `false` (nothing is package-backed without a resolver).
    fn is_package_backed(&self, _canonical_id: &str) -> bool {
        false
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

    /// Query reverse deps (files that import this file). Returns the union
    /// of canonical-axis and stem-axis hits, with the queried target
    /// stripped longest-suffix-first against the workspace's configured
    /// extension list.
    fn reverse_deps_for(&self, canonical_id: &str) -> Vec<String>;

    /// Query forward deps (files this file imports). Union of all
    /// canonical-axis dep classes (parsed + exact + lazy + ambient +
    /// semantic_transitive). Stems are NOT included.
    fn forward_deps_for(&self, canonical_id: &str) -> Vec<String>;

    /// R22 contract: transitive importers of `edited`. The reverse
    /// import graph serves reachability GC + LSP affected-files
    /// reporting + diagnostics; it is **never** wired to cache
    /// invalidation. This BFS walks the canonical reverse axis and
    /// returns the transitive closure of files that (directly or
    /// indirectly) import `edited`, sorted for stable order.
    ///
    /// Default implementation walks via [`Self::reverse_deps_for`]. The
    /// `edited` file itself is NOT included in the result; cycles
    /// terminate via the visited set.
    fn affected_canonicals(&self, edited: &str) -> Vec<String> {
        let mut out: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut frontier: Vec<String> = self.reverse_deps_for(edited);
        while let Some(next) = frontier.pop() {
            if next == edited {
                continue;
            }
            if !out.insert(next.clone()) {
                continue;
            }
            for parent in self.reverse_deps_for(&next) {
                if parent != edited && !out.contains(&parent) {
                    frontier.push(parent);
                }
            }
        }
        out.into_iter().collect()
    }

    /// Inspection: snapshot of an owner's dependency state.
    fn dependency_snapshot(&self, canonical_id: &str) -> Option<DependencySnapshotView>;

    // ── Directory queries ──

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

    /// Check whether a path is a directory.
    fn is_dir(&self, _path: &str) -> bool {
        false
    }

    // ── Ambient TypeScript lib reads ──

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
    /// fact validators that resolve an ambient virtual id's current
    /// `FileWholeHash` (e.g., the `WholeHash` arm of the
    /// `validates_fact_signature` walk on the `StoreView`).
    /// Backends that don't support ambient libs return an empty registry.
    fn ambient_libs_view(&self) -> Arc<AmbientLibsByProject> {
        Arc::new(AmbientLibsByProject::default())
    }

    /// Currently-published workspace root, if any.
    ///
    /// Returns `Some` once `Engine::new()`'s bootstrap publication runs;
    /// returns `None` for adapter backends that do not maintain a
    /// published snapshot (e.g., test stubs). Session-side consumers
    /// read `published_root()` to map canonical ids to owning projects
    /// via `snapshot.owners_for_file(canonical).first()`.
    ///
    /// Default: `None`.
    fn published_root(&self) -> Option<Arc<crate::published_state::PublishedRoot>> {
        None
    }
}

/// Mutating view of the workspace authority — extends [`WorkspaceRead`]
/// with edge recording, exact-resolution writes, overlay notifications,
/// audit-sink registry, and ambient-lib mutators.
///
/// All workspace I/O (reads, writes, walks, resolution) goes through this
/// trait hierarchy. There is no separate `ProjectResolverReader` or
/// `ConfigFileReader`. The resolver, config parser, and host all take
/// `&dyn WorkspaceAccess` (or the narrower `&dyn WorkspaceRead` for
/// read-only consumers).
///
/// # Implementors
///
/// - [`FilesystemWorkspace`] — disk-backed with overlay/snapshot cache
/// - [`MemoryWorkspace`] — fully in-memory (tests, WASM, playground)
/// - Lightweight adapters (LSP readers) that delegate to a host's workspace
///
/// # sub-
///
/// `WorkspaceAccess` is no longer the public read API for external crates;
/// it is gated behind `pub(crate) VerterHost::workspace()`. Read consumers
/// outside `verter_session` use `VerterHost::workspace_read()` →
/// `Arc<dyn WorkspaceRead>`. Mutators (`notify_close`, `notify_upsert`,
/// `set_exact_resolutions`, `configure_resolver`) are reachable only via
/// host wrappers that run the cache-cascade discipline.
pub trait WorkspaceAccess: WorkspaceRead {
    // ── Reverse-graph authority methods (R6: NO DEFAULTS) ──
    //
    // Every WorkspaceAccess impl MUST explicitly implement these. A future
    // impl that forgets to override would have silently dropped edges under
    // a default-no-op design; R6's compile-time enforcement makes that
    // impossible.

    /// Record parsed edges from a file's imports. Eagerly resolves
    /// `Relative` and `ExternalSrc` edges via the parsed-edge resolver
    /// (which bypasses `exact_resolutions` per R5). Stores `Bare` specifiers.
    /// Per R4 lifecycle: clears `exact_resolutions`, `exact_resolved`,
    /// `lazy_resolved`, and `semantic_transitive` for the file. **Does NOT
    /// clear `ambient_resolved` (F1.5).**
    fn record_parsed_edges(&self, canonical_id: &str, edges: &[ParsedEdge]);

    /// Replace bundler-injected exact resolutions for a file. The active
    /// stem set is recomputed AFTER the exact mutation; parsed-unresolved
    /// entries are NOT destroyed (F18 active-stem model).
    fn set_exact_resolutions(
        &self,
        canonical_id: &str,
        resolutions: Vec<ExactResolution>,
    ) -> ExactResolutionResult;

    /// Replace owner's transitive-semantic dep set. Always fires regardless
    /// of `cc.dependencies` union equality (closes F15).
    fn replace_semantic_transitive(&self, canonical_id: &str, deps: BTreeSet<String>);

    /// Set the workspace's reverse-dep-stripping extension list. Merges
    /// with `probe_extensions()` and sorts longest-first at set-time (F4).
    fn set_default_resolve_extensions(&self, host_extensions: Vec<String>);

    /// Reset VFS provenance counters.
    fn reset_vfs_provenance(&self) {}

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

    // ── Package-backed types-entry resolution ──

    /// Locate the manifest `types` / `typings` entry for a package-backed
    /// runtime-script target.
    ///
    /// Returns the canonical path of the resolved types entry when
    /// `canonical_id` is package-backed AND its effective target is a
    /// runtime script (`.js`, `.cjs`, `.mjs`, `.jsx`). Returns `None` for
    /// workspace-owned files, for declaration files (`.d.ts`, `.d.cts`,
    /// `.d.mts`), for TypeScript sources (`.ts`, `.tsx`), and when the
    /// package manifest declares no `types` / `typings` entry.
    ///
    /// Concrete workspaces may override this to add caching. The default
    /// implementation walks up to the package root (the segment immediately
    /// after the last `node_modules/` boundary, expanded for scoped packages),
    /// reads `package.json`, and resolves the `types` or `typings` field
    /// against the package directory.
    fn manifest_types_entry_for(&self, canonical_id: &str) -> Option<String> {
        if !self.is_package_backed(canonical_id) {
            return None;
        }
        if !is_runtime_script_target(canonical_id) {
            return None;
        }
        let package_dir = package_dir_for_resolved_target(canonical_id)?;
        let package_json_path = format!("{package_dir}/package.json");
        let manifest = self.read_package_manifest(&package_json_path)?;
        let type_targets = [manifest.types.clone(), manifest.typings.clone()];
        type_targets.into_iter().flatten().find_map(|target| {
            let candidate = if let Some(rest) = target.strip_prefix("./") {
                format!("{package_dir}/{rest}")
            } else if target.starts_with('/') {
                target
            } else {
                format!("{package_dir}/{target}")
            };
            if !self.file_exists(&candidate) {
                return None;
            }
            Some(self.realpath(&candidate).unwrap_or(candidate))
        })
    }

    // ── Audit sink registry ──

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

    // ── Ambient TypeScript lib registry mutations ──

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

    /// Record a session-side reverse-dep edge from a consumer file to the
    /// ambient virtual id. Routes to the dedicated `ambient_resolved`
    /// dep class (F1.5: ambient deps survive parse re-records).
    /// Re-registration of the lib bumps the content generation so the
    /// fact-rail self-root validators reject downstream caches that
    /// pinned the prior content.
    /// **R6: no default; every workspace impl must override.**
    fn record_ambient_dependency(&self, consumer: &str, virtual_id: &str);

    // ── Project-scoped env-hash API ──
    //
    // Five-dimensional env-hash composition (R21) is keyed by `ProjectId`,
    // not by canonical id, so workspaces with overlapping projects can
    // hold distinct cache identities for a file claimed by multiple
    // projects. Session-side queries map canonical → ProjectId via
    // `WorkspaceSnapshot::owners_for_file(canonical).first()`.
    //
    // The tables live inside the published `PublishedRoot` snapshot (see
    // `crate::published_state::PublishedRoot::env_hashes_by_project` and
    // `project_identity_hashes`) so the snapshot and its env-hash tables
    // swap atomically on `ArcSwapOption<PublishedRoot>` republish. Lookup
    // is `O(1)` map access; the tables are computed ONCE at snapshot-build
    // time in `engine.rs::rebuild_and_publish()`.

    /// Env-hash array `[parse, resolve, type_, lib]` for a published
    /// project.
    ///
    /// Returns `None` when `project_id` is not present in the currently
    /// published snapshot (e.g., dropped on workspace bump, or no snapshot
    /// has been published yet). Callers fall back to
    /// [`Self::workspace_default_env_hash_array`] for canonicals with no
    /// owning project.
    ///
    /// Default body returns `None` — concrete workspaces override to read
    /// from their published snapshot.
    fn env_hash_array_for_project(&self, _project_id: ProjectId) -> Option<ProjectEnvHashArray> {
        None
    }

    /// Project-identity hash for a published project.
    ///
    /// Session callers wrap the returned `Hash16` as
    /// `verter_session::ProjectIdentity`. Returns `None` when the project
    /// is not present in the currently published snapshot.
    ///
    /// Default body returns `None` — concrete workspaces override.
    fn project_identity_hash_for_project(&self, _project_id: ProjectId) -> Option<Hash16> {
        None
    }

    /// Workspace-wide default env-hash array for canonicals with no
    /// owning project (e.g., cross-project sweeps over scratch / ambient
    /// canonicals).
    ///
    /// The default body returns all-zero `Hash16`s; concrete workspaces
    /// override to mix workspace-config + SDK fingerprint into a stable
    /// non-zero default. Session-side validators that observe an all-zero
    /// project identity treat it as "no owning project" rather than
    /// "default project".
    fn workspace_default_env_hash_array(&self) -> ProjectEnvHashArray {
        [[0u8; 16]; 4]
    }

    /// Workspace-wide default project-identity hash for canonicals with
    /// no owning project. See [`Self::workspace_default_env_hash_array`]
    /// for the rationale on the all-zero default.
    fn workspace_default_project_identity_hash(&self) -> Hash16 {
        [0u8; 16]
    }

    // ── Audit producer ──

    /// Drive a workspace [`WorkspaceOp`] under audit and produce a
    /// [`RequestAuditRecord`] describing the work.
    ///
    /// The default body executes the operation through this trait's
    /// own read methods (`resolve_import`, `forward_deps_for`,
    /// `resolve_import_for_project`) so every concrete backend
    /// inherits a real producer that walks live workspace state —
    /// it is NOT a stub.
    ///
    /// **Reachable-only invariant.** The traversal uses ONLY the
    /// `from`-importer's resolution surface (for `AuditResolve`),
    /// the BFS root's forward-dep edges (for `DepGraphTraverse`),
    /// or the project-scoped resolver (for `ResolverWalk`). Files
    /// outside the requested operation's reach do NOT appear in
    /// `record.files` — this enforces the macro-traversal
    /// MUST-NOT-walk-unrelated-imports invariant
    /// (see `CLAUDE.md` "Macro Type Traversal Rule").
    ///
    /// **TLS install.** The session-level callsite (`VerterHost`)
    /// wraps `audit_op` with an `AuditRequestRegistration::new`
    /// (`Active` / `Noop`) so the consumer-filter / records-store
    /// lifecycle is honored. The trait method itself is purely a
    /// producer: it does not enter the active-request registry.
    /// Per-request id is read from
    /// [`verter_scheduler::request_context::current_request_id`]
    /// so a registration installed by the host will already be
    /// visible when the trait method runs.
    fn audit_op(&self, op: WorkspaceOp) -> RequestAuditRecord {
        let request_id = verter_scheduler::request_context::current_request_id().unwrap_or(0);
        let canonical_id = match &op {
            WorkspaceOp::AuditResolve { from, .. } => from.clone(),
            WorkspaceOp::DepGraphTraverse { root } => root.clone(),
            WorkspaceOp::ResolverWalk { .. } => String::new(),
        };

        let start = std::time::Instant::now();
        let mut files: Vec<FileAudit> = Vec::new();
        let mut dep_edges_traversed: u64 = 0;

        match &op {
            WorkspaceOp::AuditResolve { specifier, from } => {
                let ctx = ResolutionContext {
                    phase: ResolvePhase::CodegenBlocker,
                    kind: ResolveRequestKind::EsmImport,
                };
                if let Some(result) = self.resolve_import(from, specifier, ctx) {
                    files.push(workspace_audit_file_entry(
                        &result.source_id,
                        FileRole::DirectImport,
                    ));
                }
            }
            WorkspaceOp::DepGraphTraverse { root } => {
                let mut visited: BTreeSet<String> = BTreeSet::new();
                let mut frontier: Vec<String> = vec![root.clone()];
                while let Some(current) = frontier.pop() {
                    if !visited.insert(current.clone()) {
                        continue;
                    }
                    let role = if current == *root {
                        FileRole::Entry
                    } else {
                        FileRole::TransitiveImport
                    };
                    files.push(workspace_audit_file_entry(&current, role));
                    let forward = self.forward_deps_for(&current);
                    dep_edges_traversed += forward.len() as u64;
                    for dep in forward {
                        if !visited.contains(&dep) {
                            frontier.push(dep);
                        }
                    }
                }
            }
            WorkspaceOp::ResolverWalk { specifier } => {
                // Project-scoped resolution: walk the workspace's
                // resolver surface for the specifier. The default
                // body uses the bare `resolve_import` surface with an
                // empty importer; backends that publish a project
                // graph hit `resolve_import_for_project` for each
                // owner via `is_workspace_owned`.
                let ctx = ResolutionContext {
                    phase: ResolvePhase::CodegenBlocker,
                    kind: ResolveRequestKind::EsmImport,
                };
                if let Some(result) = self.resolve_import("", specifier, ctx) {
                    files.push(workspace_audit_file_entry(
                        &result.source_id,
                        FileRole::ResolverWalk,
                    ));
                }
            }
        }

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
        let files_touched: u32 = files.len().min(u32::MAX as usize) as u32;
        let payload = WorkspacePayload {
            op: op.clone(),
            files_touched,
            ms: elapsed_ms,
            dep_edges_traversed,
        };

        RequestAuditRecord {
            request_id,
            canonical_id,
            kind: RequestKind::Workspace { op },
            parent_request_id: None,
            from_cache: false,
            timings: RequestTimingAudit {
                total_ms: elapsed_ms,
                ..RequestTimingAudit::default()
            },
            memory: RequestMemoryAudit::default(),
            store: RequestStoreAudit::default(),
            footprint: None,
            scheduler: None,
            files,
            waits: None,
            kind_payload: RequestKindPayload::Workspace(payload),
            capture_state: verter_audit::AuditCaptureState::ActiveStored,
            trace_id: String::new(),
        }
    }
}

/// Construct a [`FileAudit`] entry recording a workspace-side touch
/// of `canonical_id` with the given role. Used by the default body
/// of [`WorkspaceAccess::audit_op`] to attribute every file the
/// workspace operation visited. Bytes/timing are zero because
/// `audit_op` does not load file content; the bytes/timing surfaces
/// belong to the read-loop producers (Slices 3.A/3.B/3.C).
fn workspace_audit_file_entry(canonical_id: &str, role: FileRole) -> FileAudit {
    FileAudit {
        canonical_id: canonical_id.to_string(),
        role,
        layer: VfsLayer::Snapshot,
        bytes_read: 0,
        cache_hit: true,
        triggered_by_this_request: false,
        read_ms: None,
        parse_ms: None,
        lower_ms: None,
    }
}

/// Whether `canonical_id`'s effective target is a runtime-script extension
/// (`.js`, `.cjs`, `.mjs`, `.jsx`).
///
/// Module-private helper for [`WorkspaceAccess::manifest_types_entry_for`].
fn is_runtime_script_target(canonical_id: &str) -> bool {
    canonical_id.ends_with(".js")
        || canonical_id.ends_with(".jsx")
        || canonical_id.ends_with(".mjs")
        || canonical_id.ends_with(".cjs")
}

/// Locate the package directory for a canonical_id that lives inside
/// `node_modules/`. Walks back from the last `/node_modules/` boundary
/// to capture the package segment, expanding scoped packages
/// (`@scope/name`) into two segments.
///
/// Returns `None` if `canonical_id` does not contain a `/node_modules/`
/// segment.
///
/// Module-private helper for [`WorkspaceAccess::manifest_types_entry_for`].
fn package_dir_for_resolved_target(canonical_id: &str) -> Option<String> {
    let normalized = canonical_id.replace('\\', "/");
    let marker = "/node_modules/";
    let marker_index = normalized.rfind(marker)?;
    let package_start = marker_index + marker.len();
    let package_path = &normalized[package_start..];
    let mut segments = package_path.split('/');
    let first = segments.next()?;
    let package_suffix = if first.starts_with('@') {
        format!("{first}/{}", segments.next()?)
    } else {
        first.to_string()
    };
    Some(format!("{}{package_suffix}", &normalized[..package_start]))
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
    //! / A1 default-trait surface tests.
    //!
    //! These confirm that `WorkspaceAccess` has the ambient lib registration
    //! API and that backends without ambient support return
    //! `Err(NotBootstrapped)` / `None` from the defaults. These are
    //! discriminating: pre-change tree (no ambient methods on the trait) does
    //! not even compile.
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use super::{DependencySnapshotView, WorkspaceAccess, WorkspaceRead};
    use crate::ambient_lib::{AmbientLibError, AmbientLibSpec};
    use crate::project_key::ProjectStableKey;
    use crate::types::{ExactResolution, ExactResolutionResult, ParsedEdge};

    /// Minimal backend that opts out of ambient lib support — exercises the
    /// trait defaults.
    struct StubWs;

    impl WorkspaceRead for StubWs {
        fn read_file(&self, _id: &str) -> Option<Arc<str>> {
            None
        }
        fn file_exists(&self, _id: &str) -> bool {
            false
        }
        fn realpath(&self, _id: &str) -> Option<String> {
            None
        }
        fn reverse_deps_for(&self, _id: &str) -> Vec<String> {
            Vec::new()
        }
        fn forward_deps_for(&self, _id: &str) -> Vec<String> {
            Vec::new()
        }
        fn dependency_snapshot(&self, _id: &str) -> Option<DependencySnapshotView> {
            None
        }
    }

    impl WorkspaceAccess for StubWs {
        // Reader-only stub overrides (R6/R7). Rationale (§2.16b):
        // `StubWs` lives inside a `#[cfg(test)]` ambient_default_tests module;
        // constructed only by trait-default coverage tests that don't invoke
        // VerterHost or any dep-flow path.
        fn record_parsed_edges(&self, _id: &str, _edges: &[ParsedEdge]) {}
        fn set_exact_resolutions(
            &self,
            _id: &str,
            _resolutions: Vec<ExactResolution>,
        ) -> ExactResolutionResult {
            ExactResolutionResult::default()
        }
        fn replace_semantic_transitive(&self, _id: &str, _deps: BTreeSet<String>) {}
        fn set_default_resolve_extensions(&self, _host_extensions: Vec<String>) {}
        fn record_ambient_dependency(&self, _consumer: &str, _virtual_id: &str) {}
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
