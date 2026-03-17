//! Virtual filesystem layer for Verter — sole authority for workspace access.
//!
//! # Architecture
//!
//! `verter_vfs` is the **single entry point** for all workspace filesystem
//! access and import resolution. No code outside `NativeFs` touches `std::fs`.
//! The host, LSP, bundler plugins, and Node.js consumers all go through the
//! [`WorkspaceAccess`] trait.
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

pub mod changes;
#[cfg(not(target_arch = "wasm32"))]
pub mod config;
pub mod error;
pub mod exact_resolution;
pub mod filesystem;
pub mod memory;
#[cfg(not(target_arch = "wasm32"))]
pub mod native_fs;
pub mod overlay;
pub mod package_index;
pub mod project_graph;
pub mod resolver;
pub mod traits;
pub mod types;
#[cfg(not(target_arch = "wasm32"))]
pub mod vite_config;

mod engine;

// ── Public re-exports ──

pub use changes::{ChangeResult, OwnedFileInfo, OwnershipDiff, WorkspaceChange};
#[cfg(not(target_arch = "wasm32"))]
pub use config::{
    discover_tsconfigs, has_solution_style_tsconfig, load_compiler_options,
    load_project_membership, load_project_references, normalize_path_buf, parse_tsconfig_json,
    raw_paths_json, resolve_tsconfig_extends, strip_json_comments, ParsedTsConfig, TsConfigEntry,
};
pub use error::{DirEntry, VfsError};
pub use exact_resolution::EdgeStore;
pub use filesystem::{FilesystemOptions, FilesystemWorkspace};
pub use memory::{MemoryOptions, MemorySnapshot, MemoryWorkspace};
pub use overlay::OverlayStore;
pub use package_index::PackageIndex;
#[cfg(not(target_arch = "wasm32"))]
pub use project_graph::ProjectGraphBuildResult;
pub use project_graph::{ProjectGraph, ProjectRank, VfsProjectConfig};
pub use resolver::{
    IdeProjectCompilerOptions, IdeProjectConfig, NativeProjectResolver, ProjectMembership,
    ProjectResolver, WorkspaceAlias,
};
pub use traits::WorkspaceAccess;
pub use types::{
    ExactResolution, ExactResolutionResult, FileKind, PackageManifest, ParsedEdge,
    ProjectOwnership, ProviderTarget, ResolutionContext, ResolutionKind, ResolvePhase,
    ResolveRequest, ResolveRequestKind, ResolveResult,
};
#[cfg(not(target_arch = "wasm32"))]
pub use vite_config::{
    analyze_vite_config, discover_vite_aliases, execute_trusted_vite_config, find_vite_config,
    get_lkg_or_empty, normalize_alias_pair, TrustedExecResult, ViteConfigAnalysis,
    ViteConfigOptions, ViteConfigTrustInfo,
};
