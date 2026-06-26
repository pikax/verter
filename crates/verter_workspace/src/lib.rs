//! Virtual filesystem layer for Verter — sole authority for workspace access.
//!
//! # Scope
//!
//! Workspace source/config reads only. Tool/output I/O remains on `std::fs`.
//!
//! [`WorkspaceAccess`] (and the read-only [`WorkspaceRead`]) is the single
//! workspace trait; it is the authority for reading user `.vue`/`.ts`/`.tsx`/
//! `.js`/`.jsx`/`.cjs`/`.mjs`/`.d.ts` source, `tsconfig*.json`,
//! `vite.config.*`, and `package.json` for module resolution.
//!
//! Files that are NOT workspace source/config — trace artifacts, MCP
//! baseline outputs, profiler dumps, temp test setup files, TS-runtime
//! tool-cache or shim files, etc. — stay on `std::fs`. The architecture
//! guard `no_std_fs_in_semantic_session_paths` consults
//! `tool-output-allowlist.toml` to enumerate those exceptions.
//!
//! Ambient TypeScript SDK declarations (`lib*.d.ts`) read from the
//! installed `typescript` package go through the dedicated
//! [`IntrinsicLibraryAccess`] trait, NOT [`WorkspaceAccess`], so SDK
//! reads do not flow through the user-content overlay.
//!
//! # Architecture
//!
//! `verter_workspace` is the **single entry point** for all workspace filesystem
//! access and import resolution. No code outside `NativeFs` touches `std::fs`
//! for workspace reads. The host, LSP, bundler plugins, and Node.js consumers
//! all go through the [`WorkspaceAccess`] trait.
//!
//! ## Key invariants
//!
//! - **`NativeFs`** (`native_fs.rs`) is the sole disk boundary. All `std::fs`
//!   calls are concentrated here.
//! - **`WorkspaceAccess`** is the single workspace trait. There is no separate
//!   `ProjectResolverReader` or `ConfigFileReader`.
//! - **Context-aware resolution**: every `resolve_import` call carries a
//!   [`ResolutionContext`] `{ phase, kind }` that determines which package.json
//!   export conditions and legacy fields are used.
//! - **No host-side fallback**: the host calls `ws.resolve_import()` and
//!   accepts the answer. No extension guessing, no basename matching.
//!
//! ## Workspace modes
//!
//! - **Filesystem** ([`FilesystemWorkspace`]): disk-backed with overlay and
//!   snapshot cache. Used by LSP, MCP, bundler plugins.
//! - **Memory** ([`MemoryWorkspace`]): fully in-memory. Used by playground,
//!   WASM, and tests.
//!
//! ## Resolution priority
//!
//! 1. Exact resolutions (authoritative, injected by bundler/LSP)
//! 2. Project resolver (tsconfig paths, workspace aliases, node_modules)
//! 3. `None` — no heuristic fallback
//!
//! ## File read priority (FilesystemWorkspace)
//!
//! 1. Overlay (active editor buffer)
//! 2. Snapshot (cached content)
//! 3. Disk via `NativeFs`

pub mod ambient_lib;
#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "ambient_lib_tests.rs"]
mod ambient_lib_tests;
#[cfg(not(target_arch = "wasm32"))]
pub mod ambient_parse;
pub mod audit_sink;
pub mod canonical_path;
pub mod changes;
#[cfg(not(target_arch = "wasm32"))]
pub mod config;
pub(crate) mod dir_index;
pub mod env_hash;
pub mod error;
pub mod exact_resolution;
pub mod filesystem;
pub mod intrinsic_library;
pub mod membership;
pub mod memory;
pub mod module_resolution;
#[cfg(not(target_arch = "wasm32"))]
pub mod native_fs;
pub mod normalized_glob;
pub mod overlay;
#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "package_classification_tests.rs"]
mod package_classification_tests;
pub mod package_index;
pub(crate) mod project_graph;
pub mod project_key;
pub mod published_state;
pub mod relative_path;
pub mod resolver;
pub mod snapshot_builder;
pub mod traits;
pub mod types;
#[cfg(not(target_arch = "wasm32"))]
pub mod vite_config;
pub mod workspace_snapshot;

mod engine;

/// Check whether `path` is equal to or nested under `prefix`.
///
/// Handles optional trailing `/` on `prefix` so callers don't need to
/// normalize before calling.
pub(crate) fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    let prefix = prefix.strip_suffix('/').unwrap_or(prefix);
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

// ── Public re-exports ──

pub use ambient_lib::{
    ambient_virtual_canonical_id, compute_ambient_hash16,
    normalize_canonical_id as normalize_ambient_canonical_id, AmbientLibEntry, AmbientLibError,
    AmbientLibSpec, AmbientLibsByProject, AmbientSymbolHit, ProjectAmbientLibs,
};
pub use canonical_path::{canonicalize_path, CanonicalPath};
pub use changes::{ChangeResult, OwnedFileInfo, OwnershipDiff, WorkspaceChange};
#[cfg(not(target_arch = "wasm32"))]
pub use config::{
    discover_tsconfigs, has_solution_style_tsconfig, load_compiler_options,
    load_project_membership, load_project_references, normalize_path_buf, parse_tsconfig_json,
    raw_paths_json, resolve_tsconfig_extends, strip_json_comments, ParsedTsConfig, TsConfigEntry,
};
pub use engine::SVELTE_RUNE_AMBIENT_PARSER_FLAG;
pub use error::{DirEntry, VfsError};
pub use exact_resolution::{DependencySnapshotView, EdgeStore};
pub use filesystem::{FilesystemOptions, FilesystemWorkspace};
#[cfg(not(target_arch = "wasm32"))]
pub use intrinsic_library::NativeIntrinsicLibrary;
pub use intrinsic_library::{InMemoryIntrinsicLibrary, IntrinsicLibraryAccess};
pub use membership::{
    typescript_default_excludes, ConfiguredMembership, FallbackMembership, StaticMembershipSpec,
};
pub use memory::{MemoryOptions, MemorySnapshot, MemoryWorkspace};
pub use normalized_glob::NormalizedGlob;
pub use overlay::OverlayStore;
pub use package_index::PackageIndex;
pub use project_key::ProjectStableKey;
pub use published_state::{ProjectEnvHashArray, PublishedRoot};
pub use resolver::{
    carrier_api_provider_path, carrier_ide_provider_path, path_is_carrier, strip_carrier_extension,
    IdeProjectCompilerOptions, IdeProjectConfig, NativeProjectResolver, ProjectMembership,
    ProjectResolver, WorkspaceAlias, CARRIER_API_VIRTUAL_SUFFIX,
};
pub use snapshot_builder::build_workspace_snapshot_simple;
#[cfg(not(target_arch = "wasm32"))]
pub use snapshot_builder::{build_workspace_snapshot, membership_to_spec, SnapshotBuildResult};
pub use traits::{
    EmptyResolverSnapshot, ResolverSnapshot, SourceLoader, WorkspaceAccess, WorkspaceRead,
    WorkspaceResourceSnapshot,
};
pub use types::{
    ExactResolution, ExactResolutionResult, PackageManifest, ParsedEdge, ProjectOwnership,
    ProviderTarget, ResolutionContext, ResolutionKind, ResolvePhase, ResolveRequest,
    ResolveRequestKind, ResolveResult, VfsProvenanceSnapshot,
};
#[cfg(not(target_arch = "wasm32"))]
pub use vite_config::{
    analyze_vite_config, discover_vite_aliases, execute_trusted_vite_config, find_vite_config,
    get_lkg_or_empty, normalize_alias_pair, vite_config_is_trusted, TrustedExecResult,
    ViteConfigAnalysis, ViteConfigOptions, ViteConfigTrustInfo,
};
pub use workspace_snapshot::{
    ConfiguredOwnerResolution, OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration,
    WorkspaceSnapshot,
};

// ── Deprecated re-exports (transitional) ──
// These types are pub(crate) in project_graph but still referenced by
// verter_lsp, verter_session tests, and verter_bench. They will be removed
// once callers migrate to the snapshot-based ownership types.

#[deprecated(note = "use OwnershipProject / WorkspaceSnapshot instead")]
pub use project_graph::ProjectGraph;

#[deprecated(note = "use OwnershipProject / WorkspaceSnapshot instead")]
pub use project_graph::ProjectRank;

#[deprecated(note = "use OwnershipProject / WorkspaceSnapshot instead")]
pub use project_graph::VfsProjectConfig;

#[cfg(not(target_arch = "wasm32"))]
#[deprecated(note = "use OwnershipProject / WorkspaceSnapshot instead")]
pub use project_graph::ProjectGraphBuildResult;
