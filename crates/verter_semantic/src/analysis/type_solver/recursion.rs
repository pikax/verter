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

use std::hash::{Hash, Hasher};

use rustc_hash::{FxHashMap, FxHashSet};

use super::arena::{ConditionalFrameSnapshot, Node, NodeId, QueryArena};

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
// Structural recursion fingerprint
// ---------------------------------------------------------------------------

/// Whether the current reentry is happening under a conditional branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StructuralRecursionMode {
    /// No conditional branch is active.
    Plain,
    /// At least one conditional frame is active.
    Conditional,
}

/// A bounded runtime fingerprint for structural recursion detection.
///
/// Combines the current conditional context summary and applied argument summary
/// so that same-branch/different-args and different-branch/same-args get separate
/// structural budgets.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StructuralRecursionFingerprint {
    pub mode: StructuralRecursionMode,
    pub combined_fingerprint: u64,
}

/// Budget limits for fingerprint computation to avoid expensive walks.
const FINGERPRINT_DEPTH_CAP: usize = 3;
const FINGERPRINT_NODE_CAP: usize = 50;

/// Compute a bounded structural fingerprint from type arguments and conditional context.
pub fn compute_structural_fingerprint(
    arena: &QueryArena,
    args: &[NodeId],
    conditional_context: &[ConditionalFrameSnapshot],
) -> StructuralRecursionFingerprint {
    let mode = if conditional_context.is_empty() {
        StructuralRecursionMode::Plain
    } else {
        StructuralRecursionMode::Conditional
    };

    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    // Hash argument shapes (bounded)
    let mut visited = 0usize;
    for &arg in args {
        hash_node_shape(arena, arg, &mut hasher, 0, &mut visited);
    }

    // Hash conditional context shape
    for frame in conditional_context {
        frame.branch.hash(&mut hasher);
        frame.decided.hash(&mut hasher);
        hash_node_shape(arena, frame.check, &mut hasher, 0, &mut visited);
        hash_node_shape(arena, frame.extends, &mut hasher, 0, &mut visited);
    }

    StructuralRecursionFingerprint {
        mode,
        combined_fingerprint: hasher.finish(),
    }
}

/// Bounded hash of a node's shape (kind + shallow structure).
fn hash_node_shape(
    arena: &QueryArena,
    node: NodeId,
    hasher: &mut impl Hasher,
    depth: usize,
    visited: &mut usize,
) {
    if depth > FINGERPRINT_DEPTH_CAP || *visited > FINGERPRINT_NODE_CAP || node.is_unresolved() {
        // Opaque marker — too deep or budget exceeded
        255u8.hash(hasher);
        return;
    }
    *visited += 1;

    match arena.get(node) {
        Node::Primitive(kind) => {
            0u8.hash(hasher);
            (*kind as u8).hash(hasher);
        }
        Node::Literal(lit) => {
            1u8.hash(hasher);
            lit.hash(hasher);
        }
        Node::Union(members) => {
            2u8.hash(hasher);
            members.len().hash(hasher);
            for &m in members {
                hash_node_shape(arena, m, hasher, depth + 1, visited);
            }
        }
        Node::Intersection(members) => {
            3u8.hash(hasher);
            members.len().hash(hasher);
            for &m in members {
                hash_node_shape(arena, m, hasher, depth + 1, visited);
            }
        }
        Node::Array { element, readonly } => {
            4u8.hash(hasher);
            readonly.hash(hasher);
            hash_node_shape(arena, *element, hasher, depth + 1, visited);
        }
        Node::Ref {
            name,
            type_arguments,
        } => {
            5u8.hash(hasher);
            name.hash(hasher);
            type_arguments.len().hash(hasher);
            for &a in type_arguments {
                hash_node_shape(arena, a, hasher, depth + 1, visited);
            }
        }
        Node::TypeParam {
            name,
            constraint,
            default,
        } => {
            6u8.hash(hasher);
            node.hash(hasher);
            name.hash(hasher);
            if let Some(constraint) = constraint {
                hash_node_shape(arena, *constraint, hasher, depth + 1, visited);
            }
            if let Some(default) = default {
                hash_node_shape(arena, *default, hasher, depth + 1, visited);
            }
        }
        Node::Object(obj) => {
            7u8.hash(hasher);
            obj.properties.len().hash(hasher);
            for p in &obj.properties {
                p.name.hash(hasher);
                p.optional.hash(hasher);
                p.readonly.hash(hasher);
                hash_node_shape(arena, p.ty, hasher, depth + 1, visited);
            }
            obj.index_signatures.len().hash(hasher);
            for signature in &obj.index_signatures {
                signature.readonly.hash(hasher);
                hash_node_shape(arena, signature.key_type, hasher, depth + 1, visited);
                hash_node_shape(arena, signature.value_type, hasher, depth + 1, visited);
            }
        }
        Node::Function(func) => {
            8u8.hash(hasher);
            func.signatures.len().hash(hasher);
            if let Some(sig) = func.signatures.first() {
                sig.parameters.len().hash(hasher);
                for param in &sig.parameters {
                    param.name.hash(hasher);
                    param.optional.hash(hasher);
                    param.rest.hash(hasher);
                    hash_node_shape(arena, param.ty, hasher, depth + 1, visited);
                }
                hash_node_shape(arena, sig.return_type, hasher, depth + 1, visited);
            }
        }
        Node::Conditional { check, extends, .. } => {
            9u8.hash(hasher);
            hash_node_shape(arena, *check, hasher, depth + 1, visited);
            hash_node_shape(arena, *extends, hasher, depth + 1, visited);
        }
        Node::RecursiveRef { symbol_name, .. } => {
            10u8.hash(hasher);
            symbol_name.hash(hasher);
        }
        Node::Infer { name } => {
            11u8.hash(hasher);
            node.hash(hasher);
            name.hash(hasher);
        }
        Node::Tuple { elements, readonly } => {
            12u8.hash(hasher);
            readonly.hash(hasher);
            elements.len().hash(hasher);
            for element in elements {
                element.label.hash(hasher);
                element.optional.hash(hasher);
                element.rest.hash(hasher);
                hash_node_shape(arena, element.ty, hasher, depth + 1, visited);
            }
        }
        _ => {
            // Catch-all for less common node kinds
            254u8.hash(hasher);
        }
    }
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
    /// Per-symbol per-fingerprint reentry counts for tiered structural limits.
    fingerprint_counts: FxHashMap<(String, String, u64), usize>,

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
    /// Hard ceiling: same symbol across ALL fingerprints.
    const MAX_SYMBOL_REENTRY: usize = 10;
    /// Soft ceiling: same symbol + same fingerprint (plain mode bails fast).
    const MAX_FINGERPRINT_REENTRY_PLAIN: usize = 2;
    /// Soft ceiling: same symbol + same fingerprint (conditional mode gets more room).
    const MAX_FINGERPRINT_REENTRY_CONDITIONAL: usize = 4;

    pub fn new() -> Self {
        Self::default()
    }

    /// Enter a recursive resolution. Returns `Some(placeholder_id)` if this
    /// key is already being resolved (cycle detected).
    ///
    /// Three tiers:
    /// 1. Exact key match → immediate placeholder reuse
    /// 2. Same symbol + same structural fingerprint → bail at soft ceiling
    /// 3. Same symbol across all fingerprints → bail at hard ceiling
    pub fn enter(
        &mut self,
        key: RecursionKey,
        fingerprint: Option<&StructuralRecursionFingerprint>,
    ) -> Option<NodeId> {
        // Tier 1: exact key match → immediate reuse
        if let Some(&placeholder) = self.active.get(&key) {
            return Some(placeholder);
        }

        let symbol_key = key.symbol_key();
        let symbol_depth = self
            .symbol_depth
            .get(&symbol_key)
            .copied()
            .unwrap_or_default();

        // Tier 3: hard ceiling on total symbol reentry
        if symbol_depth >= Self::MAX_SYMBOL_REENTRY {
            return self
                .symbol_placeholders
                .get(&symbol_key)
                .and_then(|placeholders| placeholders.last().copied());
        }

        // Tier 2: fingerprint-based soft ceiling (only when symbol is already active)
        if symbol_depth > 0 {
            if let Some(fp) = fingerprint {
                let fp_key = (
                    symbol_key.0.clone(),
                    symbol_key.1.clone(),
                    fp.combined_fingerprint,
                );
                let fp_count = self.fingerprint_counts.get(&fp_key).copied().unwrap_or(0);
                let soft_limit = match fp.mode {
                    StructuralRecursionMode::Plain => Self::MAX_FINGERPRINT_REENTRY_PLAIN,
                    StructuralRecursionMode::Conditional => {
                        Self::MAX_FINGERPRINT_REENTRY_CONDITIONAL
                    }
                };
                if fp_count >= soft_limit {
                    return self
                        .symbol_placeholders
                        .get(&symbol_key)
                        .and_then(|placeholders| placeholders.last().copied());
                }
            }
        }

        None
    }

    /// Record that resolution of `key` is in progress, associated with
    /// the given placeholder node.
    pub fn push(
        &mut self,
        key: RecursionKey,
        placeholder: NodeId,
        fingerprint: Option<&StructuralRecursionFingerprint>,
    ) {
        let symbol_key = key.symbol_key();
        self.active.insert(key, placeholder);
        *self.symbol_depth.entry(symbol_key.clone()).or_default() += 1;
        self.symbol_placeholders
            .entry(symbol_key.clone())
            .or_default()
            .push(placeholder);
        if let Some(fp) = fingerprint {
            let fp_key = (symbol_key.0, symbol_key.1, fp.combined_fingerprint);
            *self.fingerprint_counts.entry(fp_key).or_default() += 1;
        }
        self.max_depth = self.max_depth.max(self.active.len());
    }

    /// Mark resolution of `key` as complete.
    pub fn pop(
        &mut self,
        key: &RecursionKey,
        fingerprint: Option<&StructuralRecursionFingerprint>,
    ) {
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
        if let Some(fp) = fingerprint {
            let fp_key = (
                symbol_key.0.clone(),
                symbol_key.1.clone(),
                fp.combined_fingerprint,
            );
            if let Some(count) = self.fingerprint_counts.get_mut(&fp_key) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    self.fingerprint_counts.remove(&fp_key);
                }
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

    /// Whether the same symbol (any args) is currently being resolved.
    pub fn is_symbol_active(&self, key: &RecursionKey) -> bool {
        let symbol_key = key.symbol_key();
        self.symbol_depth
            .get(&symbol_key)
            .copied()
            .unwrap_or_default()
            > 0
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
    use crate::analysis::type_solver::arena::{ObjectNode, PrimitiveKind, PropertyNode};

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

        assert!(tracker.enter(key.clone(), None).is_none());
        tracker.push(key.clone(), NodeId(42), None);
        assert!(tracker.is_active(&key));
        assert_eq!(tracker.depth(), 1);

        // Second entry detects the cycle
        assert_eq!(tracker.enter(key.clone(), None), Some(NodeId(42)));

        tracker.pop(&key, None);
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

        tracker.push(k1.clone(), NodeId(0), None);
        tracker.push(k2.clone(), NodeId(1), None);
        assert_eq!(tracker.depth(), 2);
        assert_eq!(tracker.max_depth(), 2);

        tracker.pop(&k2, None);
        assert_eq!(tracker.depth(), 1);
        assert_eq!(tracker.max_depth(), 2); // max preserved

        tracker.pop(&k1, None);
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
                tracker.enter(key.clone(), None).is_none(),
                "entry {} should succeed",
                args_hash
            );
            tracker.push(key, NodeId(args_hash as u32), None);
        }

        // The 11th reentry (different args_hash) should return the last placeholder.
        let structural_reentry = RecursionKey {
            canonical_id: "/types.ts".into(),
            symbol_name: "NestedItem".into(),
            args_hash: 99,
        };
        assert_eq!(
            tracker.enter(structural_reentry, None),
            Some(NodeId(9)),
            "11th reentry should return last placeholder"
        );
    }

    #[test]
    fn plain_structural_reentry_bails_fast() {
        let mut tracker = RecursionTracker::new();
        let plain_fp = StructuralRecursionFingerprint {
            mode: StructuralRecursionMode::Plain,
            combined_fingerprint: 42,
        };

        // Push 2 entries with same fingerprint — should succeed
        for i in 0..2 {
            let key = RecursionKey {
                canonical_id: "/types.ts".into(),
                symbol_name: "Tree".into(),
                args_hash: i,
            };
            assert!(tracker.enter(key.clone(), Some(&plain_fp)).is_none());
            tracker.push(key, NodeId(i as u32), Some(&plain_fp));
        }

        // 3rd entry with same fingerprint should bail (plain budget = 2)
        let key3 = RecursionKey {
            canonical_id: "/types.ts".into(),
            symbol_name: "Tree".into(),
            args_hash: 99,
        };
        assert!(
            tracker.enter(key3, Some(&plain_fp)).is_some(),
            "plain mode should bail at 2 same-fingerprint reentries"
        );
    }

    #[test]
    fn conditional_structural_reentry_gets_higher_budget() {
        let mut tracker = RecursionTracker::new();
        let cond_fp = StructuralRecursionFingerprint {
            mode: StructuralRecursionMode::Conditional,
            combined_fingerprint: 42,
        };

        // Push 4 entries — conditional mode has budget of 4
        for i in 0..4 {
            let key = RecursionKey {
                canonical_id: "/types.ts".into(),
                symbol_name: "Flatten".into(),
                args_hash: i,
            };
            assert!(
                tracker.enter(key.clone(), Some(&cond_fp)).is_none(),
                "entry {} should succeed under conditional budget",
                i
            );
            tracker.push(key, NodeId(i as u32), Some(&cond_fp));
        }

        // 5th entry should bail
        let key5 = RecursionKey {
            canonical_id: "/types.ts".into(),
            symbol_name: "Flatten".into(),
            args_hash: 99,
        };
        assert!(
            tracker.enter(key5, Some(&cond_fp)).is_some(),
            "conditional mode should bail at 4 same-fingerprint reentries"
        );
    }

    #[test]
    fn different_arg_shape_fingerprints_do_not_collapse_together() {
        let mut tracker = RecursionTracker::new();
        let fp_a = StructuralRecursionFingerprint {
            mode: StructuralRecursionMode::Conditional,
            combined_fingerprint: 100,
        };
        let fp_b = StructuralRecursionFingerprint {
            mode: StructuralRecursionMode::Conditional,
            combined_fingerprint: 200,
        };

        // Push 2 with fp_a, 2 with fp_b — each gets its own budget
        for i in 0..2 {
            let key = RecursionKey {
                canonical_id: "/types.ts".into(),
                symbol_name: "Bar".into(),
                args_hash: i,
            };
            tracker.push(key, NodeId(i as u32), Some(&fp_a));
        }
        for i in 10..12 {
            let key = RecursionKey {
                canonical_id: "/types.ts".into(),
                symbol_name: "Bar".into(),
                args_hash: i,
            };
            tracker.push(key, NodeId(i as u32), Some(&fp_b));
        }

        // A third fp_a entry should still be allowed (4 budget, only 2 used)
        let key_a3 = RecursionKey {
            canonical_id: "/types.ts".into(),
            symbol_name: "Bar".into(),
            args_hash: 50,
        };
        assert!(
            tracker.enter(key_a3, Some(&fp_a)).is_none(),
            "fp_a should still have room"
        );
    }

    #[test]
    fn fingerprint_distinguishes_same_named_infer_nodes_by_origin() {
        let mut arena = QueryArena::new();
        let infer_a = arena.alloc(Node::Infer { name: "X".into() });
        let infer_b = arena.alloc(Node::Infer { name: "X".into() });

        let fp_a = compute_structural_fingerprint(&arena, &[infer_a], &[]);
        let fp_b = compute_structural_fingerprint(&arena, &[infer_b], &[]);

        assert_ne!(
            fp_a, fp_b,
            "distinct infer binders with the same textual name must not share a fingerprint"
        );
    }

    #[test]
    fn fingerprint_distinguishes_object_property_value_shapes() {
        let mut arena = QueryArena::new();
        let string_ty = arena.primitive(PrimitiveKind::String);
        let number_ty = arena.primitive(PrimitiveKind::Number);

        let object_string = arena.alloc(Node::Object(ObjectNode {
            properties: vec![PropertyNode {
                name: "value".into(),
                ty: string_ty,
                optional: false,
                readonly: false,
                is_method: false,
            }],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        }));
        let object_number = arena.alloc(Node::Object(ObjectNode {
            properties: vec![PropertyNode {
                name: "value".into(),
                ty: number_ty,
                optional: false,
                readonly: false,
                is_method: false,
            }],
            index_signatures: vec![],
            call_signatures: vec![],
            construct_signatures: vec![],
        }));

        let fp_string = compute_structural_fingerprint(&arena, &[object_string], &[]);
        let fp_number = compute_structural_fingerprint(&arena, &[object_number], &[]);

        assert_ne!(
            fp_string, fp_number,
            "object fingerprints must distinguish nested property value types"
        );
    }
}
