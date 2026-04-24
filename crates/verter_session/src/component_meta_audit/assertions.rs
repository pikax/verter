//! Inherent assertions + iterative walker for [`RustAuditRecord`].
//!
//! Plan §2.8 + §3 Commit 6. Public surface:
//!
//! - [`RustAuditRecord::assert_loaded_files_exactly`] — set-equality
//!   assertion against the union of `vfs_reads` and `shared_load_reuses`
//!   canonical ids. Failure renders a unified-diff style explanation.
//! - [`RustAuditRecord::why_loaded`] — iterative backward walker
//!   producing a [`ProvenanceChain`] for the canonical.
//! - [`RustAuditRecord::why_instantiated`] — same shape, rooted at the
//!   matching [`InstantiationRecord`].
//! - [`render_chain_text`] — pure formatter; NAPI / WASM / LSP all
//!   delegate to it via Rust-walker bindings (plan §2.8 — single
//!   walker implementation).
//!
//! The walker is iterative: a `Vec<(NodeId, u16)>` work-stack, an
//! `FxHashSet<EdgeId>` visited set, a depth-256 cap, and termination
//! markers carried on the returned chain. Branch termination on
//! `SharedLoadReuse` terminates only the affected branch (plan §2.8).

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

use super::{
    DerivationEdgeRecord, EdgeId, InstantiationRecord, NodeId, OriginEdgeKind, RustAuditRecord,
    SharedLoadReuseRecord,
};
use crate::types::Hash16;

/// Maximum depth for the iterative walker. Exceeding this cap
/// terminates the affected branch with a `DepthExceeded` marker
/// (plan §2.8).
pub const WALKER_DEPTH_CAP: u16 = 256;

/// Provenance chain returned by [`RustAuditRecord::why_loaded`] /
/// [`RustAuditRecord::why_instantiated`]. Always carries a
/// [`ChainTermination`] so renderers can distinguish a complete walk
/// from a depth-capped, cycle-terminated, or shared-load-redirected
/// one.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct ProvenanceChain {
    /// In-audit `NodeId` the walk started from. `None` when the walker
    /// could not locate any matching root in the audit record.
    pub root: Option<NodeId>,
    /// Steps in BFS order. Each step is one derivation edge whose
    /// `result` is the current frontier node; `depth` records the hop
    /// count from the root.
    pub steps: Vec<ProvenanceStep>,
    /// Why the walk stopped. `Complete` means the frontier exhausted
    /// without hitting any structural termination.
    pub terminated: ChainTermination,
    /// Shared-load reuses observed for the queried canonical (only
    /// populated by `why_loaded`). Renderers display these as terminal
    /// branches per plan §2.7.
    pub shared_load_terminals: Vec<SharedLoadReuseRecord>,
}

/// One step on a [`ProvenanceChain`].
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct ProvenanceStep {
    pub edge_id: EdgeId,
    pub depth: u16,
    /// `display_label` of the edge's result node.
    pub node_label: Arc<str>,
    pub edge: DerivationEdgeRecord,
}

/// Why the walker stopped.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum ChainTermination {
    /// All reachable edges were visited without hitting any
    /// structural cap.
    Complete,
    /// The walker reached [`WALKER_DEPTH_CAP`] and terminated the
    /// affected branch.
    DepthExceeded { cap: u16 },
    /// A previously-visited [`EdgeId`] was re-encountered. The walker
    /// terminates that branch and records the cycle anchor.
    Cycle { at_edge: EdgeId },
    /// The walker's root could not be located in the audit record.
    NotFound,
}

// ──────────────────────────────────────────────────────────────────────
// Inherent assertion / walker methods on RustAuditRecord
// ──────────────────────────────────────────────────────────────────────

impl RustAuditRecord {
    /// Assert that the union of `vfs_reads` and `shared_load_reuses`
    /// canonical ids equals `expected` exactly (set equality).
    ///
    /// Returns `Err(AssertionDiff)` on mismatch — the diff renders a
    /// unified-format explanation with the symmetric difference grouped
    /// into `+expected` and `-actual` arms.
    pub fn assert_loaded_files_exactly<I, S>(&self, expected: I) -> Result<(), AssertionDiff>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let actual: Vec<Arc<str>> = self
            .footprint
            .as_ref()
            .map(super::RustSemanticFootprintAudit::loaded_files)
            .unwrap_or_default();
        let mut expected_sorted: Vec<String> = expected
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        expected_sorted.sort();
        expected_sorted.dedup();
        let actual_set: FxHashSet<&str> = actual.iter().map(|a| a.as_ref()).collect();
        let expected_set: FxHashSet<&str> = expected_sorted.iter().map(String::as_str).collect();
        if actual_set == expected_set {
            return Ok(());
        }
        let mut missing: Vec<&str> = expected_set.difference(&actual_set).copied().collect();
        let mut extra: Vec<&str> = actual_set.difference(&expected_set).copied().collect();
        missing.sort();
        extra.sort();
        Err(AssertionDiff::new_loaded_files(missing, extra))
    }

    /// Assert that the broader dependency set
    /// (`vfs_reads ∪ shared_load_reuses ∪ indexed_ready_builds`)
    /// equals `expected` exactly (set equality). Plan §3.B Commit 7.B —
    /// use this when the fixture's intent is "the request's dependency
    /// closure included these files", which is a distinct semantic
    /// claim from [`Self::assert_loaded_files_exactly`]'s "the
    /// scheduler actually read these files on behalf of this request".
    pub fn assert_declared_dependency_files_exactly<I, S>(
        &self,
        expected: I,
    ) -> Result<(), AssertionDiff>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let actual: Vec<Arc<str>> = self
            .footprint
            .as_ref()
            .map(super::RustSemanticFootprintAudit::declared_dependency_files)
            .unwrap_or_default();
        let mut expected_sorted: Vec<String> = expected
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        expected_sorted.sort();
        expected_sorted.dedup();
        let actual_set: FxHashSet<&str> = actual.iter().map(|a| a.as_ref()).collect();
        let expected_set: FxHashSet<&str> = expected_sorted.iter().map(String::as_str).collect();
        if actual_set == expected_set {
            return Ok(());
        }
        let mut missing: Vec<&str> = expected_set.difference(&actual_set).copied().collect();
        let mut extra: Vec<&str> = actual_set.difference(&expected_set).copied().collect();
        missing.sort();
        extra.sort();
        Err(AssertionDiff::new_declared_dependency_files(missing, extra))
    }

    /// Walk the derivation subgraph backward starting from any node
    /// that names `canonical_id` (via [`super::NamedIdentity`]) — or,
    /// failing that, surface the `vfs_reads` and `shared_load_reuses`
    /// records for `canonical_id` as terminals.
    pub fn why_loaded(&self, canonical_id: &str) -> ProvenanceChain {
        let Some(footprint) = self.footprint.as_ref() else {
            return ProvenanceChain {
                root: None,
                steps: Vec::new(),
                terminated: ChainTermination::NotFound,
                shared_load_terminals: Vec::new(),
            };
        };
        let shared_load_terminals: Vec<SharedLoadReuseRecord> = footprint
            .shared_load_reuses
            .iter()
            .filter(|r| r.canonical_id.as_ref() == canonical_id)
            .cloned()
            .collect();
        let root = footprint
            .derivation_subgraph
            .nodes
            .iter()
            .enumerate()
            .find_map(|(i, rec)| match rec.named_identity.as_ref() {
                Some(id) if id.canonical_id.as_ref() == canonical_id => Some(NodeId(i as u32)),
                _ => None,
            });
        let Some(root) = root else {
            return ProvenanceChain {
                root: None,
                steps: Vec::new(),
                terminated: if shared_load_terminals.is_empty() {
                    ChainTermination::NotFound
                } else {
                    ChainTermination::Complete
                },
                shared_load_terminals,
            };
        };
        let mut chain = walk_back(footprint, root);
        chain.shared_load_terminals = shared_load_terminals;
        chain
    }

    /// Walk the derivation subgraph backward from the
    /// [`InstantiationRecord`] matching
    /// `(decl_canonical_id, decl_symbol_name, args_fingerprint)`.
    pub fn why_instantiated(
        &self,
        decl_canonical_id: &str,
        decl_symbol_name: &str,
        args_fingerprint: Hash16,
    ) -> ProvenanceChain {
        let Some(footprint) = self.footprint.as_ref() else {
            return ProvenanceChain {
                root: None,
                steps: Vec::new(),
                terminated: ChainTermination::NotFound,
                shared_load_terminals: Vec::new(),
            };
        };
        let target = footprint
            .instantiations
            .iter()
            .find(|inst: &&InstantiationRecord| {
                inst.decl_canonical_id.as_ref() == decl_canonical_id
                    && inst.decl_symbol_name.as_ref() == decl_symbol_name
                    && inst.args_fingerprint == args_fingerprint
            });
        let Some(inst) = target else {
            return ProvenanceChain {
                root: None,
                steps: Vec::new(),
                terminated: ChainTermination::NotFound,
                shared_load_terminals: Vec::new(),
            };
        };
        walk_back(footprint, inst.result)
    }
}

// ──────────────────────────────────────────────────────────────────────
// Iterative backward walker
// ──────────────────────────────────────────────────────────────────────

/// Walk every derivation edge whose `result` is reachable from `root`
/// in the backward (sources-of-result) direction. BFS via a
/// `Vec<(NodeId, u16)>` work-stack so heap-deep chains do not overflow
/// the OS stack (plan §3 Commit 6 test
/// `why_loaded_iterative_walker_handles_heap_depth_1000_without_stack_overflow`).
fn walk_back(footprint: &super::RustSemanticFootprintAudit, root: NodeId) -> ProvenanceChain {
    let edges = &footprint.derivation_subgraph.edges;
    let nodes = &footprint.derivation_subgraph.nodes;

    let mut result_to_edges: FxHashMap<NodeId, Vec<EdgeId>> =
        FxHashMap::with_capacity_and_hasher(edges.len(), Default::default());
    for (i, e) in edges.iter().enumerate() {
        result_to_edges
            .entry(e.result)
            .or_default()
            .push(EdgeId(i as u32));
    }

    let mut steps: Vec<ProvenanceStep> = Vec::new();
    let mut visited: FxHashSet<EdgeId> = FxHashSet::default();
    // BFS work queue — push children, pop from front so the chain is
    // shallow-first. Implemented as `Vec<(NodeId, u16)>` with `remove(0)`;
    // edge counts are bounded by `max_derivation_edges` (default 10_000)
    // so the linear pop cost is acceptable for the audit walker.
    let mut work: Vec<(NodeId, u16)> = vec![(root, 0)];
    let mut termination = ChainTermination::Complete;
    let mut idx = 0usize;

    while idx < work.len() {
        let (current, depth) = work[idx];
        idx += 1;
        if depth > WALKER_DEPTH_CAP {
            termination = ChainTermination::DepthExceeded {
                cap: WALKER_DEPTH_CAP,
            };
            break;
        }
        let Some(edge_ids) = result_to_edges.get(&current) else {
            continue;
        };
        for &eid in edge_ids {
            if !visited.insert(eid) {
                // Cycle — record the anchor and skip (terminates only
                // this branch, not the entire walk).
                if matches!(termination, ChainTermination::Complete) {
                    termination = ChainTermination::Cycle { at_edge: eid };
                }
                continue;
            }
            let edge = &edges[eid.0 as usize];
            let label = nodes
                .get(current.0 as usize)
                .map(|r| Arc::clone(&r.display_label))
                .unwrap_or_else(|| Arc::from("<unknown>"));
            steps.push(ProvenanceStep {
                edge_id: eid,
                depth,
                node_label: label,
                edge: edge.clone(),
            });
            // Push sources for further walk (unless this edge is a
            // SharedLoadReuse — those terminate only this branch).
            if matches!(edge.kind, OriginEdgeKind::SharedLoadReuse) {
                continue;
            }
            for src in &edge.sources {
                work.push((*src, depth + 1));
            }
        }
    }

    ProvenanceChain {
        root: Some(root),
        steps,
        terminated: termination,
        shared_load_terminals: Vec::new(),
    }
}

// ──────────────────────────────────────────────────────────────────────
// Pure-formatting renderer (used by NAPI/WASM/LSP via JSON round-trip)
// ──────────────────────────────────────────────────────────────────────

/// Render a [`ProvenanceChain`] to a multi-line text representation.
/// Pure formatting — no graph access, no walker re-entry. NAPI / WASM
/// helpers delegate to this; LSP renders Markdown via a parallel
/// formatter.
pub fn render_chain_text(chain: &ProvenanceChain) -> String {
    let mut out = String::new();
    match chain.root {
        Some(root) => {
            out.push_str(&format!("# Provenance chain (root #{})\n", root.0));
        }
        None => {
            out.push_str("# Provenance chain (no root)\n");
        }
    }
    for step in &chain.steps {
        let indent = " ".repeat(step.depth.min(64) as usize * 2);
        out.push_str(&format!(
            "{indent}↳ {} via {:?}\n",
            step.node_label, step.edge.kind
        ));
    }
    for terminal in &chain.shared_load_terminals {
        if terminal.winner_audited {
            out.push_str(&format!(
                "↘ shared with audited request #{} ({})\n",
                terminal.winner_request_id, terminal.canonical_id
            ));
        } else {
            out.push_str(&format!(
                "↘ shared with unaudited request #{} ({}) — winner did not capture\n",
                terminal.winner_request_id, terminal.canonical_id
            ));
        }
    }
    match &chain.terminated {
        ChainTermination::Complete => out.push_str("(complete)\n"),
        ChainTermination::DepthExceeded { cap } => {
            out.push_str(&format!("(truncated at depth {cap})\n"));
        }
        ChainTermination::Cycle { at_edge } => {
            out.push_str(&format!("(cycle anchor: edge #{})\n", at_edge.0));
        }
        ChainTermination::NotFound => out.push_str("(not found)\n"),
    }
    out
}

// ──────────────────────────────────────────────────────────────────────
// Assertion failure diff
// ──────────────────────────────────────────────────────────────────────

/// Unified-diff style assertion failure.
#[derive(Debug, Clone)]
pub struct AssertionDiff {
    pub message: String,
}

impl AssertionDiff {
    fn new_loaded_files(missing: Vec<&str>, extra: Vec<&str>) -> Self {
        let mut out = String::from("loaded_files set mismatch:\n");
        for m in &missing {
            out.push_str(&format!("  + {m} (expected, missing from actual)\n"));
        }
        for e in &extra {
            out.push_str(&format!("  - {e} (actual, not expected)\n"));
        }
        Self { message: out }
    }

    fn new_declared_dependency_files(missing: Vec<&str>, extra: Vec<&str>) -> Self {
        let mut out = String::from("declared_dependency_files set mismatch:\n");
        for m in &missing {
            out.push_str(&format!("  + {m} (expected, missing from actual)\n"));
        }
        for e in &extra {
            out.push_str(&format!("  - {e} (actual, not expected)\n"));
        }
        Self { message: out }
    }
}

impl std::fmt::Display for AssertionDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AssertionDiff {}

// ──────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_meta_audit::{
        DerivationEdgeRecord, DerivationSubgraph, NodeRecord, OriginEdgeMetaDto, RustAuditRecord,
        RustMemoryAudit, RustSemanticFootprintAudit, RustSolverAudit, RustStoreAudit,
        RustTimingAudit, SemanticNodeKind, VfsLayer, VfsReadRecord,
    };

    fn empty_record() -> RustAuditRecord {
        RustAuditRecord {
            request_id: 1,
            canonical_id: "/x.vue".into(),
            timings: RustTimingAudit::default(),
            solver: RustSolverAudit::default(),
            store: RustStoreAudit::default(),
            memory: RustMemoryAudit::default(),
            footprint: None,
        }
    }

    fn record_with_footprint(footprint: RustSemanticFootprintAudit) -> RustAuditRecord {
        let mut r = empty_record();
        r.footprint = Some(footprint);
        r
    }

    fn primitive_node(label: &str) -> NodeRecord {
        NodeRecord {
            kind: SemanticNodeKind::Primitive,
            named_identity: None,
            structural_hash: [label.len() as u8; 16],
            display_label: Arc::from(label),
        }
    }

    fn alias_edge(result: u32, sources: &[u32], name: &str) -> DerivationEdgeRecord {
        DerivationEdgeRecord {
            result: NodeId(result),
            kind: OriginEdgeKind::AliasResolve,
            sources: sources.iter().copied().map(NodeId).collect(),
            meta: OriginEdgeMetaDto::AliasResolve {
                alias_name: Arc::from(name),
            },
        }
    }

    #[test]
    fn assert_loaded_files_exactly_passes_when_sets_match() {
        let fp = RustSemanticFootprintAudit {
            vfs_reads: vec![VfsReadRecord {
                canonical_id: Arc::from("/a.ts"),
                layer: VfsLayer::Disk,
                cache_hit: false,
                bytes_read: 1,
                request_id: 1,
            }],
            shared_load_reuses: vec![SharedLoadReuseRecord {
                canonical_id: Arc::from("/b.ts"),
                winner_request_id: 1,
                winner_audited: false,
            }],
            ..Default::default()
        };
        let r = record_with_footprint(fp);
        r.assert_loaded_files_exactly(["/a.ts", "/b.ts"])
            .expect("set match");
    }

    #[test]
    fn assert_loaded_files_exactly_renders_diff_on_mismatch() {
        let fp = RustSemanticFootprintAudit {
            vfs_reads: vec![VfsReadRecord {
                canonical_id: Arc::from("/a.ts"),
                layer: VfsLayer::Disk,
                cache_hit: false,
                bytes_read: 1,
                request_id: 1,
            }],
            ..Default::default()
        };
        let r = record_with_footprint(fp);
        let err = r
            .assert_loaded_files_exactly(["/x.ts", "/y.ts"])
            .expect_err("set must mismatch");
        assert!(err.message.contains("/x.ts"));
        assert!(err.message.contains("/y.ts"));
        assert!(err.message.contains("/a.ts"));
        assert!(err.message.contains('+'), "missing should render with +");
        assert!(err.message.contains('-'), "extra should render with -");
    }

    #[test]
    fn loaded_files_returns_exactly_vfs_reads_plus_shared_load_reuses_no_indexed_ready() {
        use crate::component_meta_audit::IndexedReadyBuildRecord;
        let fp = RustSemanticFootprintAudit {
            vfs_reads: vec![VfsReadRecord {
                canonical_id: Arc::from("/a.ts"),
                layer: VfsLayer::Disk,
                cache_hit: false,
                bytes_read: 1,
                request_id: 1,
            }],
            shared_load_reuses: vec![SharedLoadReuseRecord {
                canonical_id: Arc::from("/b.ts"),
                winner_request_id: 1,
                winner_audited: false,
            }],
            indexed_ready_builds: vec![IndexedReadyBuildRecord {
                canonical_id: Arc::from("/c.ts"),
                whole_hash: [0u8; 16],
            }],
            ..Default::default()
        };
        let loaded = fp.loaded_files();
        let set: std::collections::HashSet<&str> = loaded.iter().map(Arc::as_ref).collect();
        assert_eq!(
            set,
            ["/a.ts", "/b.ts"]
                .into_iter()
                .collect::<std::collections::HashSet<_>>(),
            "loaded_files must exclude indexed_ready_builds per plan §1.4 exactness",
        );
    }

    #[test]
    fn declared_dependency_files_returns_vfs_reads_plus_shared_load_reuses_plus_indexed_ready_builds(
    ) {
        use crate::component_meta_audit::IndexedReadyBuildRecord;
        let fp = RustSemanticFootprintAudit {
            vfs_reads: vec![VfsReadRecord {
                canonical_id: Arc::from("/a.ts"),
                layer: VfsLayer::Disk,
                cache_hit: false,
                bytes_read: 1,
                request_id: 1,
            }],
            shared_load_reuses: vec![SharedLoadReuseRecord {
                canonical_id: Arc::from("/b.ts"),
                winner_request_id: 1,
                winner_audited: false,
            }],
            indexed_ready_builds: vec![IndexedReadyBuildRecord {
                canonical_id: Arc::from("/c.ts"),
                whole_hash: [0u8; 16],
            }],
            ..Default::default()
        };
        let declared = fp.declared_dependency_files();
        let set: std::collections::HashSet<&str> = declared.iter().map(Arc::as_ref).collect();
        assert_eq!(
            set,
            ["/a.ts", "/b.ts", "/c.ts"]
                .into_iter()
                .collect::<std::collections::HashSet<_>>(),
            "declared_dependency_files must include all three lanes",
        );
    }

    #[test]
    fn loaded_files_and_declared_dependency_files_are_distinct_when_indexed_ready_has_entries_without_vfs_reads(
    ) {
        use crate::component_meta_audit::IndexedReadyBuildRecord;
        // A fresh IndexedReadyBuildRecord for `/c.ts` with no matching
        // VfsReadRecord models the "pre-request snapshot populated"
        // case: `c.ts` entered the cache earlier (scheduler prefetch or
        // shared warmup) and was observed by THIS request via the
        // dependency graph, but the request itself did not trigger a
        // read on its behalf.
        let fp = RustSemanticFootprintAudit {
            vfs_reads: vec![VfsReadRecord {
                canonical_id: Arc::from("/a.ts"),
                layer: VfsLayer::Disk,
                cache_hit: false,
                bytes_read: 1,
                request_id: 1,
            }],
            indexed_ready_builds: vec![IndexedReadyBuildRecord {
                canonical_id: Arc::from("/c.ts"),
                whole_hash: [0u8; 16],
            }],
            ..Default::default()
        };

        let loaded: Vec<Arc<str>> = fp.loaded_files();
        let declared: Vec<Arc<str>> = fp.declared_dependency_files();

        // `loaded` covers only `/a.ts`.
        let loaded_set: std::collections::HashSet<&str> = loaded.iter().map(Arc::as_ref).collect();
        assert!(
            !loaded_set.contains("/c.ts"),
            "loaded_files must NOT include `/c.ts` (no VfsReadRecord for it), got {loaded_set:?}",
        );
        assert!(loaded_set.contains("/a.ts"));

        // `declared` covers both.
        let declared_set: std::collections::HashSet<&str> =
            declared.iter().map(Arc::as_ref).collect();
        assert!(
            declared_set.contains("/c.ts"),
            "declared_dependency_files MUST include `/c.ts` (broader dependency-cache set), got {declared_set:?}",
        );
        assert!(declared_set.contains("/a.ts"));
    }

    #[test]
    fn assert_declared_dependency_files_exactly_passes_when_sets_match() {
        use crate::component_meta_audit::IndexedReadyBuildRecord;
        let fp = RustSemanticFootprintAudit {
            vfs_reads: vec![VfsReadRecord {
                canonical_id: Arc::from("/a.ts"),
                layer: VfsLayer::Disk,
                cache_hit: false,
                bytes_read: 1,
                request_id: 1,
            }],
            indexed_ready_builds: vec![IndexedReadyBuildRecord {
                canonical_id: Arc::from("/c.ts"),
                whole_hash: [0u8; 16],
            }],
            ..Default::default()
        };
        let r = record_with_footprint(fp);
        r.assert_declared_dependency_files_exactly(["/a.ts", "/c.ts"])
            .expect("set match");
    }

    #[test]
    fn assert_declared_dependency_files_exactly_renders_diff_on_mismatch() {
        use crate::component_meta_audit::IndexedReadyBuildRecord;
        let fp = RustSemanticFootprintAudit {
            indexed_ready_builds: vec![IndexedReadyBuildRecord {
                canonical_id: Arc::from("/c.ts"),
                whole_hash: [0u8; 16],
            }],
            ..Default::default()
        };
        let r = record_with_footprint(fp);
        let err = r
            .assert_declared_dependency_files_exactly(["/x.ts"])
            .expect_err("set must mismatch");
        assert!(err.message.contains("declared_dependency_files"));
        assert!(err.message.contains("/x.ts"));
        assert!(err.message.contains("/c.ts"));
    }

    #[test]
    fn why_loaded_returns_not_found_when_no_footprint() {
        let r = empty_record();
        let chain = r.why_loaded("/x.ts");
        assert!(matches!(chain.terminated, ChainTermination::NotFound));
        assert_eq!(chain.steps.len(), 0);
    }

    #[test]
    fn why_loaded_surfaces_shared_load_reuse_terminal_when_canonical_matches() {
        let fp = RustSemanticFootprintAudit {
            shared_load_reuses: vec![SharedLoadReuseRecord {
                canonical_id: Arc::from("/x.ts"),
                winner_request_id: 7,
                winner_audited: true,
            }],
            ..Default::default()
        };
        let r = record_with_footprint(fp);
        let chain = r.why_loaded("/x.ts");
        assert_eq!(chain.shared_load_terminals.len(), 1);
        assert_eq!(chain.shared_load_terminals[0].winner_request_id, 7);
    }

    #[test]
    fn why_loaded_traverses_derivation_subgraph_backward_via_node_ids() {
        // Three nodes: 0 ← 1 ← 2 via two AliasResolve hops, with node 2
        // carrying a NamedIdentity matching "/x.ts".
        use crate::component_meta_audit::NamedIdentity;
        let mut nodes = vec![primitive_node("a"), primitive_node("b")];
        nodes.push(NodeRecord {
            kind: SemanticNodeKind::Alias,
            named_identity: Some(NamedIdentity {
                canonical_id: Arc::from("/x.ts"),
                symbol_name: Arc::from("Sym"),
                args_fingerprint: [0u8; 16],
            }),
            structural_hash: [42u8; 16],
            display_label: Arc::from("Sym"),
        });
        let edges = vec![
            alias_edge(2, &[1], "two_hop"),
            alias_edge(1, &[0], "one_hop"),
        ];
        let fp = RustSemanticFootprintAudit {
            derivation_subgraph: DerivationSubgraph { nodes, edges },
            ..Default::default()
        };
        let r = record_with_footprint(fp);
        let chain = r.why_loaded("/x.ts");
        assert_eq!(chain.root, Some(NodeId(2)));
        assert_eq!(chain.steps.len(), 2);
        assert!(matches!(chain.terminated, ChainTermination::Complete));
    }

    #[test]
    fn why_loaded_iterative_walker_handles_heap_depth_1000_without_stack_overflow() {
        // 1000-deep chain — recursive walk would overflow; iterative
        // BFS handles it. Plan §2.8 / §3 Commit 6.
        use crate::component_meta_audit::NamedIdentity;
        let mut nodes: Vec<NodeRecord> = (0..1001)
            .map(|i| primitive_node(&format!("n{i}")))
            .collect();
        nodes[1000] = NodeRecord {
            kind: SemanticNodeKind::Alias,
            named_identity: Some(NamedIdentity {
                canonical_id: Arc::from("/deep.ts"),
                symbol_name: Arc::from("Deep"),
                args_fingerprint: [0u8; 16],
            }),
            structural_hash: [0xff; 16],
            display_label: Arc::from("Deep"),
        };
        let edges: Vec<DerivationEdgeRecord> = (1..=1000)
            .rev()
            .map(|i| alias_edge(i, &[i - 1], "hop"))
            .collect();
        let fp = RustSemanticFootprintAudit {
            derivation_subgraph: DerivationSubgraph { nodes, edges },
            ..Default::default()
        };
        let r = record_with_footprint(fp);
        let chain = r.why_loaded("/deep.ts");
        // Walker must terminate via the depth cap, NOT via stack
        // overflow.
        assert!(matches!(
            chain.terminated,
            ChainTermination::DepthExceeded {
                cap: WALKER_DEPTH_CAP
            }
        ));
        // Cap=256 means we record 257 steps (depths 0..256 inclusive)
        // before the cap fires.
        assert!(chain.steps.len() <= (WALKER_DEPTH_CAP as usize) + 2);
    }

    #[test]
    fn why_loaded_cycle_terminates_with_cycle_marker() {
        use crate::component_meta_audit::NamedIdentity;
        // Two nodes with a 0 → 1 → 0 cycle.
        let mut nodes = vec![primitive_node("zero"), primitive_node("one")];
        nodes[0] = NodeRecord {
            kind: SemanticNodeKind::Alias,
            named_identity: Some(NamedIdentity {
                canonical_id: Arc::from("/cycle.ts"),
                symbol_name: Arc::from("Cyc"),
                args_fingerprint: [0u8; 16],
            }),
            structural_hash: [9u8; 16],
            display_label: Arc::from("Cyc"),
        };
        let edges = vec![alias_edge(0, &[1], "fwd"), alias_edge(1, &[0], "back")];
        let fp = RustSemanticFootprintAudit {
            derivation_subgraph: DerivationSubgraph { nodes, edges },
            ..Default::default()
        };
        let r = record_with_footprint(fp);
        let chain = r.why_loaded("/cycle.ts");
        // Both edges visit once; on the second appearance of edge 0
        // (or edge 1) the visited-set blocks re-walk.
        assert_eq!(chain.steps.len(), 2);
        // Termination set on first cycle hit, not Complete.
        assert!(!matches!(chain.terminated, ChainTermination::NotFound));
    }

    #[test]
    fn why_loaded_two_derivations_of_same_node_both_visited_independently() {
        use crate::component_meta_audit::NamedIdentity;
        let mut nodes = vec![
            primitive_node("source_a"),
            primitive_node("source_b"),
            primitive_node("result"),
        ];
        nodes[2] = NodeRecord {
            kind: SemanticNodeKind::Alias,
            named_identity: Some(NamedIdentity {
                canonical_id: Arc::from("/multi.ts"),
                symbol_name: Arc::from("M"),
                args_fingerprint: [0u8; 16],
            }),
            structural_hash: [3u8; 16],
            display_label: Arc::from("M"),
        };
        let edges = vec![alias_edge(2, &[0], "via_a"), alias_edge(2, &[1], "via_b")];
        let fp = RustSemanticFootprintAudit {
            derivation_subgraph: DerivationSubgraph { nodes, edges },
            ..Default::default()
        };
        let r = record_with_footprint(fp);
        let chain = r.why_loaded("/multi.ts");
        // Both AliasResolve edges into result must appear as separate
        // steps (multi-derivation).
        assert_eq!(chain.steps.len(), 2);
        let names: Vec<Arc<str>> = chain
            .steps
            .iter()
            .map(|s| match &s.edge.meta {
                OriginEdgeMetaDto::AliasResolve { alias_name } => Arc::clone(alias_name),
                _ => Arc::from("?"),
            })
            .collect();
        assert!(names.iter().any(|n| n.as_ref() == "via_a"));
        assert!(names.iter().any(|n| n.as_ref() == "via_b"));
    }

    #[test]
    fn render_chain_text_includes_steps_and_termination_marker() {
        let mut nodes = vec![primitive_node("n0")];
        nodes.push(primitive_node("n1"));
        let edges = vec![alias_edge(1, &[0], "hop")];
        let fp = RustSemanticFootprintAudit {
            derivation_subgraph: DerivationSubgraph {
                nodes: nodes.clone(),
                edges,
            },
            ..Default::default()
        };
        let _r = record_with_footprint(fp);
        let chain = ProvenanceChain {
            root: Some(NodeId(1)),
            steps: vec![ProvenanceStep {
                edge_id: EdgeId(0),
                depth: 0,
                node_label: Arc::from("n1"),
                edge: alias_edge(1, &[0], "hop"),
            }],
            terminated: ChainTermination::Complete,
            shared_load_terminals: Vec::new(),
        };
        let text = render_chain_text(&chain);
        assert!(text.contains("Provenance chain"));
        assert!(text.contains("n1"));
        assert!(text.contains("AliasResolve"));
        assert!(text.contains("(complete)"));
    }

    #[test]
    fn render_chain_text_renders_winner_unaudited_fallback_for_shared_load_reuse() {
        let chain = ProvenanceChain {
            root: None,
            steps: Vec::new(),
            terminated: ChainTermination::Complete,
            shared_load_terminals: vec![SharedLoadReuseRecord {
                canonical_id: Arc::from("/shared.ts"),
                winner_request_id: 99,
                winner_audited: false,
            }],
        };
        let text = render_chain_text(&chain);
        assert!(text.contains("unaudited"));
        assert!(text.contains("/shared.ts"));
        assert!(text.contains("99"));
    }

    #[test]
    fn why_instantiated_returns_chain_rooted_at_matching_instantiation() {
        use crate::component_meta_audit::InstantiationRecord;
        let nodes = vec![primitive_node("decl"), primitive_node("inst_result")];
        let edges = vec![alias_edge(1, &[0], "from_decl")];
        let fp = RustSemanticFootprintAudit {
            derivation_subgraph: DerivationSubgraph { nodes, edges },
            instantiations: vec![InstantiationRecord {
                result: NodeId(1),
                decl_canonical_id: Arc::from("/decl.ts"),
                decl_symbol_name: Arc::from("Decl"),
                args_fingerprint: [7u8; 16],
                args: vec![NodeId(0)],
            }],
            ..Default::default()
        };
        let r = record_with_footprint(fp);
        let chain = r.why_instantiated("/decl.ts", "Decl", [7u8; 16]);
        assert_eq!(chain.root, Some(NodeId(1)));
        assert_eq!(chain.steps.len(), 1);
    }

    #[test]
    fn why_instantiated_returns_not_found_when_triple_does_not_match() {
        let r = record_with_footprint(RustSemanticFootprintAudit::default());
        let chain = r.why_instantiated("/missing.ts", "Nope", [0u8; 16]);
        assert!(matches!(chain.terminated, ChainTermination::NotFound));
    }
}
