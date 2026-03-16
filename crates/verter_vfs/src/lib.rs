//! Virtual filesystem layer for Verter.
//!
//! `verter_vfs` provides a single file-access and resolution layer that
//! centralizes file reading, import resolution, project ownership, and
//! dependency tracking. Two workspace modes are supported:
//!
//! - **Filesystem**: disk-backed with overlay and snapshot cache (LSP, MCP, bundler)
//! - **Memory**: fully in-memory, no disk fallback (playground, WASM, tests)

pub mod changes;
#[cfg(not(target_arch = "wasm32"))]
pub mod config;
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
    resolve_tsconfig_extends, strip_json_comments, ParsedTsConfig, TsConfigEntry,
};
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
    ProjectResolver, ProjectResolverReader, WorkspaceAlias,
};
pub use traits::WorkspaceAccess;
pub use types::{
    ExactResolution, ExactResolutionResult, FileKind, PackageManifest, ParsedEdge,
    ProjectOwnership, ProviderTarget, ResolutionKind, ResolvePhase, ResolveRequest,
    ResolveRequestKind, ResolveResult,
};
#[cfg(not(target_arch = "wasm32"))]
pub use vite_config::{
    analyze_vite_config, discover_vite_aliases, execute_trusted_vite_config, find_vite_config,
    get_lkg_or_empty, normalize_alias_pair, TrustedExecResult, ViteConfigAnalysis,
    ViteConfigOptions, ViteConfigTrustInfo,
};
