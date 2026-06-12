use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Classification of a file by its role in the VFS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// Vue Single File Component (.vue).
    VueSfc,
    /// Non-Vue source file (.ts, .tsx, .js, .jsx, .d.ts, etc.).
    NonSfc,
}

/// Ownership information for a file within a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectOwnership {
    pub project_root: String,
    pub tsconfig_path: Option<String>,
}

/// Resolution kind for an import or external source request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolveRequestKind {
    EsmImport,
    TypeImport,
    RequireCall,
    SfcSrcAttr,
}

/// Which dependency graph is asking for resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolvePhase {
    CodegenBlocker,
    ProviderGraph,
}

/// Resolution context — determines which target a specifier resolves to.
///
/// Different `(phase, kind)` combinations produce different results for the
/// same specifier. For example:
/// - `(CodegenBlocker, EsmImport)` → runtime entry (`index.js`, `"import"` condition)
/// - `(CodegenBlocker, TypeImport)` → type entry (`index.d.ts`, `"types"` condition)
/// - `(ProviderGraph, *)` → type entry (`"types"` condition)
/// - `(*, RequireCall)` → CJS entry (`"require"` condition)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResolutionContext {
    pub phase: ResolvePhase,
    pub kind: ResolveRequestKind,
}

/// Where the resolved file should be exposed to the provider layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTarget {
    SourceFile,
    VuePublicApi,
    ShadowSourceFile,
}

/// High-level category describing how the specifier resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionKind {
    Relative,
    TsConfigPath,
    ProjectReference,
    NodeModules,
    PackageExports,
    PackageImports,
    WorkspaceAlias,
    Bundler,
    PlaygroundMap,
}

/// Input for import resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveRequest {
    pub importer_id: String,
    pub specifier: String,
    pub kind: ResolveRequestKind,
    pub phase: ResolvePhase,
}

/// Output from import resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveResult {
    pub source_id: String,
    pub provider_id: String,
    pub provider_specifier: String,
    pub provider_target: ProviderTarget,
    pub resolution_kind: ResolutionKind,
    pub owner_tsconfig_path: Option<String>,
}

/// A parsed edge from a file's imports, recorded during upsert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedEdge {
    /// Relative import (./foo, ../bar) — resolved eagerly via resolve_import().
    Relative {
        specifier: String,
        kind: ResolveRequestKind,
    },
    /// Bare import (@/foo, vue, lodash) — stored, resolved later.
    Bare {
        specifier: String,
        kind: ResolveRequestKind,
    },
    /// External src block — resolved eagerly via resolve_import() (project-aware).
    ExternalSrc {
        specifier: String,
        resolved_path: Option<String>,
    },
}

/// An exact resolution override injected by bundler or LSP.
///
/// Keyed by `(specifier, phase, kind)` in the edge store, so different
/// contexts can resolve the same specifier to different targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactResolution {
    pub specifier: String,
    pub phase: ResolvePhase,
    pub kind: ResolveRequestKind,
    pub resolved_canonical_id: Option<String>,
    pub possible_canonical_ids: Vec<String>,
}

/// Result of setting exact resolutions.
#[derive(Debug, Clone, Default)]
pub struct ExactResolutionResult {
    /// Canonical IDs of files that were newly added to the dependency graph.
    pub newly_resolved: Vec<String>,
    /// Whether the stored exact-resolution table actually changed. `false`
    /// means the supplied snapshot was value-identical to the stored one
    /// and the call performed no write — callers use this to skip
    /// invalidation cascades for steady-state re-pushes.
    pub changed: bool,
}

/// Parsed package.json manifest fields (cached by PackageIndex).
#[derive(Debug, Clone, Default)]
pub struct PackageManifest {
    pub name: Option<String>,
    pub version: Option<String>,
    pub main: Option<String>,
    pub module: Option<String>,
    pub types: Option<String>,
    pub typings: Option<String>,
    pub exports: Option<serde_json::Value>,
    pub imports: Option<serde_json::Value>,
    /// Raw source for re-parsing if needed.
    pub raw: Option<Arc<str>>,
}

#[derive(Debug, Default)]
pub struct VfsProvenance {
    pub import_resolution_cache_hit_count: AtomicU64,
    pub import_resolution_cache_miss_count: AtomicU64,
    pub dir_index_hit_count: AtomicU64,
    pub dir_index_refresh_count: AtomicU64,
    pub dir_index_dirty_rescan_count: AtomicU64,
    pub native_fs_read_dir_count: AtomicU64,
    pub native_fs_read_file_miss_count: AtomicU64,
}

impl VfsProvenance {
    pub fn snapshot(&self) -> VfsProvenanceSnapshot {
        VfsProvenanceSnapshot {
            import_resolution_cache_hit_count: self
                .import_resolution_cache_hit_count
                .load(Ordering::Relaxed),
            import_resolution_cache_miss_count: self
                .import_resolution_cache_miss_count
                .load(Ordering::Relaxed),
            dir_index_hit_count: self.dir_index_hit_count.load(Ordering::Relaxed),
            dir_index_refresh_count: self.dir_index_refresh_count.load(Ordering::Relaxed),
            dir_index_dirty_rescan_count: self.dir_index_dirty_rescan_count.load(Ordering::Relaxed),
            native_fs_read_dir_count: self.native_fs_read_dir_count.load(Ordering::Relaxed),
            native_fs_read_file_miss_count: self
                .native_fs_read_file_miss_count
                .load(Ordering::Relaxed),
        }
    }

    pub fn reset(&self) {
        self.import_resolution_cache_hit_count
            .store(0, Ordering::Relaxed);
        self.import_resolution_cache_miss_count
            .store(0, Ordering::Relaxed);
        self.dir_index_hit_count.store(0, Ordering::Relaxed);
        self.dir_index_refresh_count.store(0, Ordering::Relaxed);
        self.dir_index_dirty_rescan_count
            .store(0, Ordering::Relaxed);
        self.native_fs_read_dir_count.store(0, Ordering::Relaxed);
        self.native_fs_read_file_miss_count
            .store(0, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VfsProvenanceSnapshot {
    pub import_resolution_cache_hit_count: u64,
    pub import_resolution_cache_miss_count: u64,
    pub dir_index_hit_count: u64,
    pub dir_index_refresh_count: u64,
    pub dir_index_dirty_rescan_count: u64,
    pub native_fs_read_dir_count: u64,
    pub native_fs_read_file_miss_count: u64,
}
