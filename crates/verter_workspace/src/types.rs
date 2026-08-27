use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use verter_semantic::resolver_core::{ResolvePhase, ResolveRequestKind};

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
    /// Live resolution-evidence reads: one per syscall issued by the
    /// independent evidence rail, which is the ONLY rail allowed to bypass
    /// the event-invalidated caches. Warm reuse is zero-syscall WITHIN a
    /// content generation, so a non-zero delta across repeated demands in one
    /// generation is a defect this counter makes visible.
    pub resolution_evidence_live_read_count: AtomicU64,
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
            resolution_evidence_live_read_count: self
                .resolution_evidence_live_read_count
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
        self.resolution_evidence_live_read_count
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
    pub resolution_evidence_live_read_count: u64,
}
