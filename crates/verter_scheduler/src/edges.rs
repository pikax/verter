//! Edge graph: per-file dependency tracking.
//!
//! The edge graph is **sharded by file** to eliminate global-lock bottlenecks.
//! Each file's dependency data lives in the [`FileNode`] itself via
//! [`FileEdges`]; the only shared structure is the concurrent
//! [`ReverseIndex`].
//!
//! Cross-file dependency gating (the blocker semantics that used to
//! live here) now flows through `SchedulerDag` — the single readiness
//! authority. The edge module retains only the immutable resolution
//! state (forward/reverse deps, exact resolutions, bare specifiers).

use dashmap::DashMap;
use rustc_hash::FxHashMap;
use std::collections::BTreeSet;

/// Per-file edge state, stored inside FileNode.
///
/// No global lock needed for reads/writes to own file's edges.
#[derive(Debug, Default, Clone)]
pub struct FileEdges {
    /// Forward dependencies (canonical IDs of files this file imports).
    pub forward_deps: BTreeSet<String>,
    /// Exact resolutions keyed by `(specifier, phase, kind)`.
    pub exact_resolutions: FxHashMap<ExactResKey, ExactResValue>,
    /// Bare specifiers not yet resolved.
    pub bare_specifiers: Vec<(String, ResolveRequestKind)>,
}

/// Key for exact resolution lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExactResKey {
    pub specifier: String,
    pub phase: ResolvePhase,
    pub kind: ResolveRequestKind,
}

/// Value for exact resolution entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactResValue {
    pub resolved_canonical_id: Option<String>,
    pub possible_canonical_ids: Vec<String>,
}

/// Resolution phase discriminant (mirrors `verter_workspace::types::ResolvePhase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolvePhase {
    CodegenBlocker,
    ProviderGraph,
}

/// Request kind discriminant (mirrors `verter_workspace::types::ResolveRequestKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolveRequestKind {
    EsmImport,
    TypeImport,
    RequireCall,
    SfcSrcAttr,
}

/// Global reverse dependency index — DashMap for concurrent per-key access.
///
/// Maps `dep_file → set of files that depend on it`.
#[derive(Default)]
pub struct ReverseIndex {
    pub inner: DashMap<String, BTreeSet<String>>,
}

impl ReverseIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get reverse deps for a file.
    pub fn get(&self, dep_id: &str) -> Vec<String> {
        self.inner
            .get(dep_id)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Add `dependent` as a reverse dep of `dependency`.
    pub fn add(&self, dependency: &str, dependent: &str) {
        self.inner
            .entry(dependency.to_string())
            .or_default()
            .insert(dependent.to_string());
    }

    /// Remove `dependent` from reverse deps of `dependency`.
    pub fn remove(&self, dependency: &str, dependent: &str) {
        if let Some(mut set) = self.inner.get_mut(dependency) {
            set.remove(dependent);
            if set.is_empty() {
                drop(set);
                self.inner.remove(dependency);
            }
        }
    }

    /// Update reverse deps when a file's forward deps change.
    pub fn update(&self, file_id: &str, old_deps: &BTreeSet<String>, new_deps: &BTreeSet<String>) {
        for old in old_deps {
            if !new_deps.contains(old) {
                self.remove(old, file_id);
            }
        }
        for new in new_deps {
            if !old_deps.contains(new) {
                self.add(new, file_id);
            }
        }
    }

    /// Remove a file from all reverse dep sets (called on file deletion).
    pub fn remove_file(&self, file_id: &str, forward_deps: &BTreeSet<String>) {
        for dep in forward_deps {
            self.remove(dep, file_id);
        }
        self.inner.remove(file_id);
    }
}

/// Edge state: reverse-index plus per-file forward dependency
/// snapshots.
///
/// Cross-file dependency gating runs through `SchedulerDag`; this
/// module only tracks the immutable resolution structure.
pub struct EdgeManager {
    pub reverse_index: ReverseIndex,
    /// Per-file forward dependency sets. Used to compute the diff when
    /// deps change so stale reverse edges are removed.
    pub forward_deps: DashMap<String, BTreeSet<String>>,
}

impl EdgeManager {
    pub fn new() -> Self {
        Self {
            reverse_index: ReverseIndex::new(),
            forward_deps: DashMap::new(),
        }
    }

    /// Record forward deps for a file and update the reverse index with the diff.
    ///
    /// Replaces the stored forward-dep set, computes added/removed edges,
    /// and applies the diff to the reverse index.
    pub fn record_forward_deps(&self, file_id: &str, new_deps: BTreeSet<String>) {
        let old_deps = self
            .forward_deps
            .insert(file_id.to_string(), new_deps.clone())
            .unwrap_or_default();
        self.reverse_index.update(file_id, &old_deps, &new_deps);
    }

    /// Get the current forward deps for a file.
    pub fn get_forward_deps(&self, file_id: &str) -> BTreeSet<String> {
        self.forward_deps
            .get(file_id)
            .map(|v| v.clone())
            .unwrap_or_default()
    }

    /// Remove all edge state for a file.
    pub fn remove_file(&self, file_id: &str) {
        if let Some((_, old_deps)) = self.forward_deps.remove(file_id) {
            self.reverse_index.remove_file(file_id, &old_deps);
        }
    }
}

impl Default for EdgeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ReverseIndex ──

    #[test]
    fn reverse_index_add_and_get() {
        let idx = ReverseIndex::new();
        idx.add("/dep.ts", "/a.vue");
        idx.add("/dep.ts", "/b.vue");

        let deps = idx.get("/dep.ts");
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&"/a.vue".to_string()));
        assert!(deps.contains(&"/b.vue".to_string()));
    }

    #[test]
    fn reverse_index_remove() {
        let idx = ReverseIndex::new();
        idx.add("/dep.ts", "/a.vue");
        idx.add("/dep.ts", "/b.vue");

        idx.remove("/dep.ts", "/a.vue");
        let deps = idx.get("/dep.ts");
        assert_eq!(deps.len(), 1);
        assert!(deps.contains(&"/b.vue".to_string()));
    }

    #[test]
    fn reverse_index_remove_cleans_empty_sets() {
        let idx = ReverseIndex::new();
        idx.add("/dep.ts", "/a.vue");
        idx.remove("/dep.ts", "/a.vue");
        assert!(idx.get("/dep.ts").is_empty());
        // Internal cleanup: entry should be removed
        assert!(!idx.inner.contains_key("/dep.ts"));
    }

    #[test]
    fn reverse_index_update() {
        let idx = ReverseIndex::new();
        let old = BTreeSet::from(["/x.ts".to_string(), "/y.ts".to_string()]);
        let new = BTreeSet::from(["/y.ts".to_string(), "/z.ts".to_string()]);

        // Set up initial state
        for dep in &old {
            idx.add(dep, "/a.vue");
        }

        idx.update("/a.vue", &old, &new);

        // /x.ts should be removed, /z.ts added, /y.ts unchanged
        assert!(idx.get("/x.ts").is_empty());
        assert!(idx.get("/y.ts").contains(&"/a.vue".to_string()));
        assert!(idx.get("/z.ts").contains(&"/a.vue".to_string()));
    }

    #[test]
    fn reverse_index_remove_file() {
        let idx = ReverseIndex::new();
        let deps = BTreeSet::from(["/x.ts".to_string(), "/y.ts".to_string()]);
        for dep in &deps {
            idx.add(dep, "/a.vue");
        }
        idx.add("/x.ts", "/b.vue");

        idx.remove_file("/a.vue", &deps);

        // /a.vue removed from /x.ts (but /b.vue remains)
        let x_deps = idx.get("/x.ts");
        assert!(!x_deps.contains(&"/a.vue".to_string()));
        assert!(x_deps.contains(&"/b.vue".to_string()));

        // /a.vue removed from /y.ts (now empty)
        assert!(idx.get("/y.ts").is_empty());
    }

    // ── FileEdges ──

    #[test]
    fn file_edges_default_empty() {
        let edges = FileEdges::default();
        assert!(edges.forward_deps.is_empty());
        assert!(edges.exact_resolutions.is_empty());
        assert!(edges.bare_specifiers.is_empty());
    }

    #[test]
    fn file_edges_exact_resolution_lookup() {
        let mut edges = FileEdges::default();
        let key = ExactResKey {
            specifier: "./types".to_string(),
            phase: ResolvePhase::CodegenBlocker,
            kind: ResolveRequestKind::EsmImport,
        };
        edges.exact_resolutions.insert(
            key.clone(),
            ExactResValue {
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: vec![],
            },
        );

        assert!(edges.exact_resolutions.contains_key(&key));
        assert_eq!(
            edges.exact_resolutions[&key].resolved_canonical_id,
            Some("/src/types.ts".to_string())
        );

        // Different context key should not match
        let other_key = ExactResKey {
            specifier: "./types".to_string(),
            phase: ResolvePhase::ProviderGraph,
            kind: ResolveRequestKind::EsmImport,
        };
        assert!(!edges.exact_resolutions.contains_key(&other_key));
    }

    // ── EdgeManager ──

    #[test]
    fn edge_manager_record_and_remove() {
        let mgr = EdgeManager::new();

        let deps = BTreeSet::from(["/x.ts".to_string(), "/y.ts".to_string()]);
        mgr.record_forward_deps("/a.vue", deps);

        assert!(mgr
            .reverse_index
            .get("/x.ts")
            .contains(&"/a.vue".to_string()));
        assert!(mgr
            .reverse_index
            .get("/y.ts")
            .contains(&"/a.vue".to_string()));

        // Remove file — reverse edges cleaned up
        mgr.remove_file("/a.vue");
        assert!(mgr.reverse_index.get("/x.ts").is_empty());
        assert!(mgr.reverse_index.get("/y.ts").is_empty());
        assert!(mgr.get_forward_deps("/a.vue").is_empty());
    }

    #[test]
    fn edge_manager_record_replaces_old_deps() {
        let mgr = EdgeManager::new();

        // First: depends on X and Y
        mgr.record_forward_deps(
            "/a.vue",
            BTreeSet::from(["/x.ts".to_string(), "/y.ts".to_string()]),
        );
        assert!(mgr
            .reverse_index
            .get("/x.ts")
            .contains(&"/a.vue".to_string()));

        // Second: depends only on Z — X and Y should be removed
        mgr.record_forward_deps("/a.vue", BTreeSet::from(["/z.ts".to_string()]));

        assert!(
            mgr.reverse_index.get("/x.ts").is_empty(),
            "old dep /x.ts should be removed"
        );
        assert!(
            mgr.reverse_index.get("/y.ts").is_empty(),
            "old dep /y.ts should be removed"
        );
        assert!(mgr
            .reverse_index
            .get("/z.ts")
            .contains(&"/a.vue".to_string()));
    }

    #[test]
    fn edge_manager_empty_deps_clears_all() {
        let mgr = EdgeManager::new();
        mgr.record_forward_deps("/a.vue", BTreeSet::from(["/x.ts".to_string()]));
        assert!(!mgr.reverse_index.get("/x.ts").is_empty());

        // Record empty deps — all reverse edges should be removed
        mgr.record_forward_deps("/a.vue", BTreeSet::new());
        assert!(
            mgr.reverse_index.get("/x.ts").is_empty(),
            "reverse edge should be removed when deps become empty"
        );
    }
}
