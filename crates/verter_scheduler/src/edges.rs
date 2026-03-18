//! Edge graph: per-file dependency tracking with declarative blockers.
//!
//! The edge graph is **sharded by file** to eliminate global-lock bottlenecks.
//! Each file's dependency data lives in the [`FileNode`] itself via [`FileEdges`];
//! the only shared structures are the concurrent [`ReverseIndex`] and [`BlockerRegistry`].

use dashmap::DashMap;
use rustc_hash::FxHashMap;
use std::collections::BTreeSet;

use crate::stage::{Priority, TaskKind};

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

/// Resolution phase discriminant (mirrors `verter_vfs::types::ResolvePhase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolvePhase {
    CodegenBlocker,
    ProviderGraph,
}

/// Request kind discriminant (mirrors `verter_vfs::types::ResolveRequestKind`).
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

/// Reference to a blocker that a file is waiting on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockerRef {
    /// Canonical ID of the file being waited on.
    pub file_id: String,
    /// The stage that must complete before the waiter can proceed.
    pub required_stage: TaskKind,
}

/// State for a blocked job.
#[derive(Debug, Clone)]
pub struct BlockerState {
    /// Blockers this job is waiting on.
    pub blockers: Vec<BlockerRef>,
    /// Priority for requeue when all blockers resolve.
    pub priority: Priority,
}

/// Blocker bookkeeping — sharded via DashMap, generation-aware.
///
/// Tracks which jobs are blocked on which files, and which files
/// have waiters. When a blocker resolves, the registry returns the
/// list of jobs that should be requeued.
#[derive(Default)]
pub struct BlockerRegistry {
    /// `(blocked_file, generation, task_kind)` → blockers it waits on + requeue priority.
    pub pending: DashMap<(String, u64, TaskKind), BlockerState>,
    /// `blocker_file` → list of `(blocked_file, generation, task_kind)` waiting on it.
    pub waiters: DashMap<String, Vec<(String, u64, TaskKind)>>,
}

/// A job that was unblocked and should be requeued.
#[derive(Debug, Clone)]
pub struct UnblockedJob {
    pub file_id: String,
    pub generation: u64,
    pub task_kind: TaskKind,
    pub priority: Priority,
}

impl BlockerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register that `(blocked_file, gen, task_kind)` is blocked on `blockers`.
    pub fn register(
        &self,
        blocked_file: &str,
        generation: u64,
        task_kind: TaskKind,
        blockers: Vec<BlockerRef>,
        priority: Priority,
    ) {
        let key = (blocked_file.to_string(), generation, task_kind);
        // Register in waiters index
        for blocker in &blockers {
            self.waiters
                .entry(blocker.file_id.clone())
                .or_default()
                .push(key.clone());
        }
        self.pending
            .insert(key, BlockerState { blockers, priority });
    }

    /// Resolve a blocker: mark `completed_file` at `completed_stage` as done.
    /// Returns list of jobs that are now fully unblocked.
    pub fn resolve(&self, completed_file: &str, completed_stage: &TaskKind) -> Vec<UnblockedJob> {
        let mut unblocked = Vec::new();

        // Get all waiters for this file
        let waiter_keys: Vec<(String, u64, TaskKind)> =
            if let Some(waiters) = self.waiters.get(completed_file) {
                waiters.clone()
            } else {
                return unblocked;
            };

        for key in &waiter_keys {
            if let Some(mut state) = self.pending.get_mut(key) {
                // Remove matching blockers
                state.blockers.retain(|b| {
                    !(b.file_id == completed_file && b.required_stage == *completed_stage)
                });

                if state.blockers.is_empty() {
                    let priority = state.priority;
                    drop(state);
                    // Fully unblocked — remove from pending
                    if let Some((_, removed)) = self.pending.remove(key) {
                        unblocked.push(UnblockedJob {
                            file_id: key.0.clone(),
                            generation: key.1,
                            task_kind: key.2,
                            priority: removed.priority.min(priority),
                        });
                    }
                }
            }
        }

        // Clean up waiter entries for resolved blockers
        if let Some(mut waiters) = self.waiters.get_mut(completed_file) {
            waiters.retain(|key| self.pending.contains_key(key));
            if waiters.is_empty() {
                drop(waiters);
                self.waiters.remove(completed_file);
            }
        }

        unblocked
    }

    /// Check whether a file/generation has any unresolved blockers.
    pub fn has_pending_blockers(&self, file_id: &str, generation: u64) -> bool {
        self.pending
            .iter()
            .any(|e| e.key().0 == file_id && e.key().1 == generation)
    }

    /// Remove all blocker state where this file is the **blocked** job.
    pub fn remove_file_as_blocked(&self, file_id: &str, generation: u64) {
        let keys_to_remove: Vec<(String, u64, TaskKind)> = self
            .pending
            .iter()
            .filter(|entry| entry.key().0 == file_id && entry.key().1 == generation)
            .map(|entry| entry.key().clone())
            .collect();

        for key in keys_to_remove {
            self.pending.remove(&key);
        }

        self.waiters.alter_all(|_, mut waiters| {
            waiters.retain(|k| !(k.0 == file_id && k.1 == generation));
            waiters
        });
    }

    /// Remove a file as a **blocker** (dependency) and return dependents
    /// that are now stranded (their blocker can never resolve).
    ///
    /// Called when a dependency file is deleted. Returns the list of
    /// dependent jobs whose blockers included this file, so the caller
    /// can fail or requeue them.
    pub fn remove_file_as_blocker(&self, blocker_id: &str) -> Vec<UnblockedJob> {
        let mut stranded = Vec::new();

        // Get all pending entries that have this file as a blocker.
        let waiter_keys: Vec<(String, u64, TaskKind)> =
            if let Some(waiters) = self.waiters.get(blocker_id) {
                waiters.clone()
            } else {
                return stranded;
            };

        for key in &waiter_keys {
            if let Some(mut state) = self.pending.get_mut(key) {
                // Remove this blocker from the pending entry.
                state.blockers.retain(|b| b.file_id != blocker_id);

                if state.blockers.is_empty() {
                    // All blockers resolved/removed — unblocked.
                    let priority = state.priority;
                    drop(state);
                    if let Some((_, removed)) = self.pending.remove(key) {
                        stranded.push(UnblockedJob {
                            file_id: key.0.clone(),
                            generation: key.1,
                            task_kind: key.2,
                            priority: removed.priority.min(priority),
                        });
                    }
                }
            }
        }

        // Remove the waiter entry for this blocker.
        self.waiters.remove(blocker_id);

        stranded
    }

    /// Upgrade priority of a blocked job.
    pub fn upgrade_priority(
        &self,
        file_id: &str,
        generation: u64,
        task_kind: TaskKind,
        new_priority: Priority,
    ) -> bool {
        let key = (file_id.to_string(), generation, task_kind);
        if let Some(mut state) = self.pending.get_mut(&key) {
            state.priority = std::cmp::min(state.priority, new_priority);
            true
        } else {
            false
        }
    }

    /// Number of pending blocked jobs.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

/// Combined edge management: reverse index + blocker registry + forward deps.
pub struct EdgeManager {
    pub reverse_index: ReverseIndex,
    pub blockers: BlockerRegistry,
    /// Per-file forward dependency sets. Used to compute the diff when
    /// deps change so stale reverse edges are removed.
    pub forward_deps: DashMap<String, BTreeSet<String>>,
}

impl EdgeManager {
    pub fn new() -> Self {
        Self {
            reverse_index: ReverseIndex::new(),
            blockers: BlockerRegistry::new(),
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

    // ── BlockerRegistry ──

    #[test]
    fn blocker_register_and_resolve() {
        let reg = BlockerRegistry::new();

        // A's artifact is blocked on B's analysis
        reg.register(
            "/a.vue",
            1,
            TaskKind::Artifact { profile_hash: 0 },
            vec![BlockerRef {
                file_id: "/b.vue".to_string(),
                required_stage: TaskKind::Analysis,
            }],
            Priority::Interactive,
        );

        assert_eq!(reg.pending_count(), 1);

        // B's analysis completes
        let unblocked = reg.resolve("/b.vue", &TaskKind::Analysis);
        assert_eq!(unblocked.len(), 1);
        assert_eq!(unblocked[0].file_id, "/a.vue");
        assert_eq!(unblocked[0].generation, 1);
        assert_eq!(reg.pending_count(), 0);
    }

    #[test]
    fn blocker_multiple_blockers() {
        let reg = BlockerRegistry::new();

        // A is blocked on both B and C
        reg.register(
            "/a.vue",
            1,
            TaskKind::Artifact { profile_hash: 0 },
            vec![
                BlockerRef {
                    file_id: "/b.vue".to_string(),
                    required_stage: TaskKind::Analysis,
                },
                BlockerRef {
                    file_id: "/c.vue".to_string(),
                    required_stage: TaskKind::Analysis,
                },
            ],
            Priority::Interactive,
        );

        // B completes — A still blocked on C
        let unblocked = reg.resolve("/b.vue", &TaskKind::Analysis);
        assert!(unblocked.is_empty());
        assert_eq!(reg.pending_count(), 1);

        // C completes — A now unblocked
        let unblocked = reg.resolve("/c.vue", &TaskKind::Analysis);
        assert_eq!(unblocked.len(), 1);
        assert_eq!(unblocked[0].file_id, "/a.vue");
    }

    #[test]
    fn blocker_wrong_stage_does_not_resolve() {
        let reg = BlockerRegistry::new();

        reg.register(
            "/a.vue",
            1,
            TaskKind::Artifact { profile_hash: 0 },
            vec![BlockerRef {
                file_id: "/b.vue".to_string(),
                required_stage: TaskKind::Analysis,
            }],
            Priority::Interactive,
        );

        // B's Source completes — not sufficient, A needs Analysis
        let unblocked = reg.resolve("/b.vue", &TaskKind::Source);
        assert!(unblocked.is_empty());
        assert_eq!(reg.pending_count(), 1);
    }

    #[test]
    fn blocker_priority_upgrade() {
        let reg = BlockerRegistry::new();

        reg.register(
            "/a.vue",
            1,
            TaskKind::Artifact { profile_hash: 0 },
            vec![BlockerRef {
                file_id: "/b.vue".to_string(),
                required_stage: TaskKind::Analysis,
            }],
            Priority::Background,
        );

        assert!(reg.upgrade_priority(
            "/a.vue",
            1,
            TaskKind::Artifact { profile_hash: 0 },
            Priority::Critical,
        ));

        let unblocked = reg.resolve("/b.vue", &TaskKind::Analysis);
        assert_eq!(unblocked[0].priority, Priority::Critical);
    }

    #[test]
    fn blocker_remove_file_cleans_up() {
        let reg = BlockerRegistry::new();

        reg.register(
            "/a.vue",
            1,
            TaskKind::Artifact { profile_hash: 0 },
            vec![BlockerRef {
                file_id: "/b.vue".to_string(),
                required_stage: TaskKind::Analysis,
            }],
            Priority::Interactive,
        );

        reg.remove_file_as_blocked("/a.vue", 1);
        assert_eq!(reg.pending_count(), 0);

        // Resolving B should not produce any unblocked jobs
        let unblocked = reg.resolve("/b.vue", &TaskKind::Analysis);
        assert!(unblocked.is_empty());
    }

    #[test]
    fn blocker_cascade_multiple_waiters() {
        let reg = BlockerRegistry::new();

        // Both A and C are blocked on B
        reg.register(
            "/a.vue",
            1,
            TaskKind::Artifact { profile_hash: 0 },
            vec![BlockerRef {
                file_id: "/b.vue".to_string(),
                required_stage: TaskKind::Analysis,
            }],
            Priority::Interactive,
        );
        reg.register(
            "/c.vue",
            1,
            TaskKind::Artifact { profile_hash: 0 },
            vec![BlockerRef {
                file_id: "/b.vue".to_string(),
                required_stage: TaskKind::Analysis,
            }],
            Priority::Background,
        );

        // B completes — both A and C unblocked
        let unblocked = reg.resolve("/b.vue", &TaskKind::Analysis);
        assert_eq!(unblocked.len(), 2);
        let ids: BTreeSet<_> = unblocked.iter().map(|u| u.file_id.as_str()).collect();
        assert!(ids.contains("/a.vue"));
        assert!(ids.contains("/c.vue"));
    }

    #[test]
    fn blocker_remove_as_blocker_unblocks_dependents() {
        let reg = BlockerRegistry::new();

        // A is blocked on B's analysis
        reg.register(
            "/a.vue",
            1,
            TaskKind::Analysis,
            vec![BlockerRef {
                file_id: "/b.vue".to_string(),
                required_stage: TaskKind::Analysis,
            }],
            Priority::Interactive,
        );
        assert_eq!(reg.pending_count(), 1);

        // B is deleted — A should be released
        let stranded = reg.remove_file_as_blocker("/b.vue");
        assert_eq!(stranded.len(), 1);
        assert_eq!(stranded[0].file_id, "/a.vue");
        assert_eq!(reg.pending_count(), 0);
    }

    #[test]
    fn blocker_remove_as_blocker_with_multiple_blockers() {
        let reg = BlockerRegistry::new();

        // A blocked on both B and C
        reg.register(
            "/a.vue",
            1,
            TaskKind::Analysis,
            vec![
                BlockerRef {
                    file_id: "/b.vue".to_string(),
                    required_stage: TaskKind::Analysis,
                },
                BlockerRef {
                    file_id: "/c.vue".to_string(),
                    required_stage: TaskKind::Analysis,
                },
            ],
            Priority::Interactive,
        );

        // B is deleted — A still blocked on C
        let stranded = reg.remove_file_as_blocker("/b.vue");
        assert!(stranded.is_empty(), "A should still be blocked on C");
        assert_eq!(reg.pending_count(), 1);

        // C is deleted — A now unblocked
        let stranded = reg.remove_file_as_blocker("/c.vue");
        assert_eq!(stranded.len(), 1);
        assert_eq!(stranded[0].file_id, "/a.vue");
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
