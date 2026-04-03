//! SCC discovery, cycle classes, and fixed-point handling for recursive types.
//!
//! Recursive types are first-class in the solver. This module detects strongly
//! connected components in the declaration/application graph and provides the
//! scaffolding for fixed-point iteration when needed.
//!
//! The solver does not rely on arbitrary unroll depth. Instead:
//! - SCCs are detected in the declaration/application graph
//! - Recursive groups are solved with memoized placeholders
//! - Fixed-point iteration is used only where needed
//! - Recursive references are preserved in exact symbolic output

use rustc_hash::{FxHashMap, FxHashSet};

use super::arena::NodeId;

// ---------------------------------------------------------------------------
// Cycle classification
// ---------------------------------------------------------------------------

/// How a recursive cycle was classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CycleClass {
    /// A single declaration that references itself (e.g. `type Tree<T> = { children: Tree<T>[] }`).
    SelfRecursive,
    /// Two or more declarations that form a mutual cycle.
    MutuallyRecursive,
}

/// A strongly connected component in the declaration graph.
#[derive(Debug, Clone)]
pub struct SccGroup {
    /// The member identities in this SCC (indices into the SCC input).
    pub members: Vec<usize>,
    /// Classification of the cycle.
    pub cycle_class: CycleClass,
}

// ---------------------------------------------------------------------------
// Tarjan's SCC algorithm
// ---------------------------------------------------------------------------

/// Tarjan's algorithm state for SCC detection.
struct TarjanState {
    index_counter: usize,
    stack: Vec<usize>,
    on_stack: FxHashSet<usize>,
    indices: FxHashMap<usize, usize>,
    lowlinks: FxHashMap<usize, usize>,
    sccs: Vec<Vec<usize>>,
}

impl TarjanState {
    fn new() -> Self {
        Self {
            index_counter: 0,
            stack: Vec::new(),
            on_stack: FxHashSet::default(),
            indices: FxHashMap::default(),
            lowlinks: FxHashMap::default(),
            sccs: Vec::new(),
        }
    }

    fn strongconnect(&mut self, v: usize, adj: &FxHashMap<usize, Vec<usize>>) {
        self.indices.insert(v, self.index_counter);
        self.lowlinks.insert(v, self.index_counter);
        self.index_counter += 1;
        self.stack.push(v);
        self.on_stack.insert(v);

        if let Some(neighbors) = adj.get(&v) {
            for &w in neighbors {
                if !self.indices.contains_key(&w) {
                    self.strongconnect(w, adj);
                    let w_low = self.lowlinks[&w];
                    let v_low = self.lowlinks.get_mut(&v).unwrap();
                    if w_low < *v_low {
                        *v_low = w_low;
                    }
                } else if self.on_stack.contains(&w) {
                    let w_idx = self.indices[&w];
                    let v_low = self.lowlinks.get_mut(&v).unwrap();
                    if w_idx < *v_low {
                        *v_low = w_idx;
                    }
                }
            }
        }

        if self.lowlinks[&v] == self.indices[&v] {
            let mut scc = Vec::new();
            loop {
                let w = self.stack.pop().unwrap();
                self.on_stack.remove(&w);
                scc.push(w);
                if w == v {
                    break;
                }
            }
            self.sccs.push(scc);
        }
    }
}

/// Find all strongly connected components in a directed graph.
///
/// `nodes` — set of node indices.
/// `adjacency` — for each node, the list of nodes it has edges to.
///
/// Returns SCCs in reverse topological order (dependencies before dependents).
pub fn find_sccs(nodes: &[usize], adjacency: &FxHashMap<usize, Vec<usize>>) -> Vec<SccGroup> {
    let mut state = TarjanState::new();
    for &v in nodes {
        if !state.indices.contains_key(&v) {
            state.strongconnect(v, adjacency);
        }
    }

    state
        .sccs
        .into_iter()
        .map(|members| {
            let cycle_class = if members.len() == 1 {
                // Self-recursive if the node has an edge to itself
                let v = members[0];
                if adjacency.get(&v).is_some_and(|edges| edges.contains(&v)) {
                    CycleClass::SelfRecursive
                } else {
                    // Trivial SCC (no self-edge) — still report as self-recursive
                    // for consistency, though it's effectively non-recursive.
                    CycleClass::SelfRecursive
                }
            } else {
                CycleClass::MutuallyRecursive
            };
            SccGroup {
                members,
                cycle_class,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Recursion tracker
// ---------------------------------------------------------------------------

/// Tracks active recursive resolution to detect cycles and install placeholders.
///
/// When the solver encounters a type reference that is already being resolved
/// (i.e., on the active stack), it creates a `RecursiveRef` placeholder node
/// that will be patched during fixed-point iteration.
#[derive(Debug, Default)]
pub struct RecursionTracker {
    /// Currently active resolutions: (canonical_id, symbol_name, args_hash) -> NodeId placeholder.
    active: FxHashMap<RecursionKey, NodeId>,
    /// Active reentry depth per declaration symbol, independent of the applied args.
    ///
    /// This catches structural recursion where each reentry manufactures fresh
    /// infer/application nodes and therefore never repeats the exact args hash.
    symbol_depth: FxHashMap<(String, String), usize>,
    /// Placeholder stack for each active symbol. When structural recursion is
    /// detected, the innermost active placeholder is reused.
    symbol_placeholders: FxHashMap<(String, String), Vec<NodeId>>,

    /// Maximum depth observed (for diagnostics).
    max_depth: usize,
}

/// Key for tracking active recursive resolution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RecursionKey {
    pub canonical_id: String,
    pub symbol_name: String,
    /// Hash of the applied type arguments (0 for non-generic).
    pub args_hash: u64,
}

impl RecursionTracker {
    const MAX_SYMBOL_REENTRY: usize = 10;

    pub fn new() -> Self {
        Self::default()
    }

    /// Enter a recursive resolution. Returns `Some(placeholder_id)` if this
    /// key is already being resolved (cycle detected).
    pub fn enter(&mut self, key: RecursionKey) -> Option<NodeId> {
        if let Some(&placeholder) = self.active.get(&key) {
            return Some(placeholder);
        }
        let symbol_key = key.symbol_key();
        if self
            .symbol_depth
            .get(&symbol_key)
            .copied()
            .unwrap_or_default()
            >= Self::MAX_SYMBOL_REENTRY
        {
            return self
                .symbol_placeholders
                .get(&symbol_key)
                .and_then(|placeholders| placeholders.last().copied());
        }
        None
    }

    /// Record that resolution of `key` is in progress, associated with
    /// the given placeholder node.
    pub fn push(&mut self, key: RecursionKey, placeholder: NodeId) {
        let symbol_key = key.symbol_key();
        self.active.insert(key, placeholder);
        *self.symbol_depth.entry(symbol_key.clone()).or_default() += 1;
        self.symbol_placeholders
            .entry(symbol_key)
            .or_default()
            .push(placeholder);
        self.max_depth = self.max_depth.max(self.active.len());
    }

    /// Mark resolution of `key` as complete.
    pub fn pop(&mut self, key: &RecursionKey) {
        self.active.remove(key);
        let symbol_key = key.symbol_key();
        if let Some(depth) = self.symbol_depth.get_mut(&symbol_key) {
            *depth = depth.saturating_sub(1);
            if *depth == 0 {
                self.symbol_depth.remove(&symbol_key);
            }
        }
        if let Some(placeholders) = self.symbol_placeholders.get_mut(&symbol_key) {
            placeholders.pop();
            if placeholders.is_empty() {
                self.symbol_placeholders.remove(&symbol_key);
            }
        }
    }

    /// Current active depth.
    pub fn depth(&self) -> usize {
        self.active.len()
    }

    /// Maximum depth observed during this query.
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Whether we're currently resolving this key.
    pub fn is_active(&self, key: &RecursionKey) -> bool {
        self.active.contains_key(key)
    }
}

impl RecursionKey {
    fn symbol_key(&self) -> (String, String) {
        (self.canonical_id.clone(), self.symbol_name.clone())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_sccs_linear_graph() {
        // A -> B -> C (no cycles)
        let nodes = vec![0, 1, 2];
        let mut adj = FxHashMap::default();
        adj.insert(0, vec![1]);
        adj.insert(1, vec![2]);

        let sccs = find_sccs(&nodes, &adj);
        assert_eq!(sccs.len(), 3);
        // Each is a trivial SCC
        for scc in &sccs {
            assert_eq!(scc.members.len(), 1);
        }
    }

    #[test]
    fn find_sccs_self_loop() {
        // A -> A
        let nodes = vec![0];
        let mut adj = FxHashMap::default();
        adj.insert(0, vec![0]);

        let sccs = find_sccs(&nodes, &adj);
        assert_eq!(sccs.len(), 1);
        assert_eq!(sccs[0].members, vec![0]);
        assert_eq!(sccs[0].cycle_class, CycleClass::SelfRecursive);
    }

    #[test]
    fn find_sccs_mutual_cycle() {
        // A -> B -> A
        let nodes = vec![0, 1];
        let mut adj = FxHashMap::default();
        adj.insert(0, vec![1]);
        adj.insert(1, vec![0]);

        let sccs = find_sccs(&nodes, &adj);
        assert_eq!(sccs.len(), 1);
        assert_eq!(sccs[0].cycle_class, CycleClass::MutuallyRecursive);
        assert_eq!(sccs[0].members.len(), 2);
    }

    #[test]
    fn find_sccs_diamond_with_cycle() {
        // A -> B, A -> C, B -> D, C -> D, D -> A
        let nodes = vec![0, 1, 2, 3];
        let mut adj = FxHashMap::default();
        adj.insert(0, vec![1, 2]);
        adj.insert(1, vec![3]);
        adj.insert(2, vec![3]);
        adj.insert(3, vec![0]);

        let sccs = find_sccs(&nodes, &adj);
        // All nodes form one SCC due to the D -> A backedge
        assert_eq!(sccs.len(), 1);
        assert_eq!(sccs[0].members.len(), 4);
        assert_eq!(sccs[0].cycle_class, CycleClass::MutuallyRecursive);
    }

    #[test]
    fn find_sccs_two_separate_cycles() {
        // {A -> B -> A} and {C -> D -> C}
        let nodes = vec![0, 1, 2, 3];
        let mut adj = FxHashMap::default();
        adj.insert(0, vec![1]);
        adj.insert(1, vec![0]);
        adj.insert(2, vec![3]);
        adj.insert(3, vec![2]);

        let sccs = find_sccs(&nodes, &adj);
        assert_eq!(sccs.len(), 2);
        for scc in &sccs {
            assert_eq!(scc.members.len(), 2);
            assert_eq!(scc.cycle_class, CycleClass::MutuallyRecursive);
        }
    }

    #[test]
    fn recursion_tracker_detects_cycle() {
        let mut tracker = RecursionTracker::new();
        let key = RecursionKey {
            canonical_id: "/types.ts".into(),
            symbol_name: "Tree".into(),
            args_hash: 0,
        };

        assert!(tracker.enter(key.clone()).is_none());
        tracker.push(key.clone(), NodeId(42));
        assert!(tracker.is_active(&key));
        assert_eq!(tracker.depth(), 1);

        // Second entry detects the cycle
        assert_eq!(tracker.enter(key.clone()), Some(NodeId(42)));

        tracker.pop(&key);
        assert!(!tracker.is_active(&key));
        assert_eq!(tracker.depth(), 0);
        assert_eq!(tracker.max_depth(), 1);
    }

    #[test]
    fn recursion_tracker_nested_depth() {
        let mut tracker = RecursionTracker::new();
        let k1 = RecursionKey {
            canonical_id: "/a.ts".into(),
            symbol_name: "A".into(),
            args_hash: 0,
        };
        let k2 = RecursionKey {
            canonical_id: "/b.ts".into(),
            symbol_name: "B".into(),
            args_hash: 0,
        };

        tracker.push(k1.clone(), NodeId(0));
        tracker.push(k2.clone(), NodeId(1));
        assert_eq!(tracker.depth(), 2);
        assert_eq!(tracker.max_depth(), 2);

        tracker.pop(&k2);
        assert_eq!(tracker.depth(), 1);
        assert_eq!(tracker.max_depth(), 2); // max preserved

        tracker.pop(&k1);
        assert_eq!(tracker.depth(), 0);
    }

    #[test]
    fn recursion_tracker_limits_structural_symbol_reentry() {
        let mut tracker = RecursionTracker::new();

        // Push MAX_SYMBOL_REENTRY (10) distinct args_hash entries — all should succeed.
        for args_hash in 0..10 {
            let key = RecursionKey {
                canonical_id: "/types.ts".into(),
                symbol_name: "NestedItem".into(),
                args_hash,
            };
            assert!(
                tracker.enter(key.clone()).is_none(),
                "entry {} should succeed",
                args_hash
            );
            tracker.push(key, NodeId(args_hash as u32));
        }

        // The 11th reentry (different args_hash) should return the last placeholder.
        let structural_reentry = RecursionKey {
            canonical_id: "/types.ts".into(),
            symbol_name: "NestedItem".into(),
            args_hash: 99,
        };
        assert_eq!(
            tracker.enter(structural_reentry),
            Some(NodeId(9)),
            "11th reentry should return last placeholder"
        );
    }
}
