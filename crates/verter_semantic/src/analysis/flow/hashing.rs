//! The slice hash: a stable fold over EXACTLY the selected subgraph of
//! one [`ReturnSlicePlan`] — never the full body.
//!
//! There is deliberately NO full-body hash entry point in this module: a
//! slice hash is only ever computed FROM a plan, so a member-projection
//! demand can never be served by a whole-body hash (a whole-return
//! demand's plan simply reaches the whole return-relevant subgraph). The
//! whole-body content identity is the separate `flow_body_stable_hash`
//! (`analysis::function_program_hash`), carried on the cache KEY — the
//! slice hash identifies the SELECTION within that pinned content.
//!
//! Stability rules:
//! - **Span-free**: byte offsets never enter the fold, so cosmetic edits
//!   that only shift positions cannot perturb the hash.
//! - **Alpha-normalized locals**: bindings fold as dense ids (their
//!   declaration ordinals), never as name text — a local rename keeps
//!   the hash.
//! - **Property keys fold as TEXT**: a demanded or written property key
//!   is content, and renaming it changes the hash.
//! - **Stack-safe**: one linear pass over the sorted selection plus its
//!   internal edges — no recursion.
//!
//! [`FlowSliceHash`] is deliberately opaque: its field is private and
//! only this producer mints values, so any key that embeds a slice hash
//! (the lowered-body cache key) can only be constructed AFTER the hash
//! was computed — the hash-then-lower order is enforced by the type, not
//! by call-site discipline.

use verter_no_typeexpr::NoTypeExpr;

use crate::analysis::types::{hash_16, Hash16};

use super::flow_graph::{FlowEdgeKind, FlowNodeId, FunctionFlowGraph};
use super::flow_ir::ReturnSlicePlan;
use super::peeker::{DemandSegment, SliceOrigin};
use super::{FunctionBodySkeleton, SkeletonPathSegment, SkeletonWriteCertainty};

#[cfg(test)]
#[path = "hashing_tests.rs"]
mod hashing_tests;

const HASH_SALT: &[u8] = b"verter-flow-slice-hash:v1";
const HASH_SEP: u8 = 0;

thread_local! {
    /// Per-thread count of [`compute_flow_slice_hash`] executions — the
    /// behavioral half of the hash-then-lower guard. The opaque
    /// [`FlowSliceHash`] type-state proves a hash PRECEDES any
    /// lowered-body KEY, but `compute_flow_slice_hash` itself is a public
    /// producer, so the type system alone cannot prove a lowered-body
    /// compute performs no hash computation of its own; the guard binds
    /// that half to this counter. Thread-local (cache-runtime computes
    /// run on the demanding thread), observability only: never key
    /// material, never a fact, never exported hash bytes.
    static COMPUTE_INVOCATIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The number of [`compute_flow_slice_hash`] executions performed on the
/// CALLING thread. Guard observability only — see the thread-local's doc.
#[must_use]
pub fn compute_flow_slice_hash_thread_invocations() -> u64 {
    COMPUTE_INVOCATIONS.with(std::cell::Cell::get)
}

/// The identity of one selected slice: minted ONLY by
/// [`compute_flow_slice_hash`]. The private field makes the value
/// unforgeable — a consumer holding a `FlowSliceHash` provably ran the
/// planner + hasher first — and there is deliberately NO byte accessor:
/// a slice hash cannot be converted to raw bytes (`Debug` is redacted,
/// so even diagnostic formatting exports none), so it cannot be embedded
/// into any fact payload or warm-validity rail (the sole intra-function
/// rail stays the whole-body `flow_body_stable_hash`; slice identity
/// keys content-addressed artifacts only). NOTE the type-state pins
/// hash-BEFORE-lowered-KEY only; "the lowered compute performs no hash
/// computation" is the separate behavioral half, held by the
/// [`compute_flow_slice_hash_thread_invocations`] counter binding.
#[derive(Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct FlowSliceHash(Hash16);

impl std::fmt::Debug for FlowSliceHash {
    /// Redacted: the slice identity's bytes never leave the type, not
    /// even through diagnostic formatting of a containing carrier.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FlowSliceHash(..)")
    }
}

/// Hash exactly the selected subgraph of `plan`: the demand identity,
/// the selected node sets with their roles, and every graph edge whose
/// both endpoints are selected (with its class, source ordinal, and
/// path-write payload). Linear, span-free, alpha-normalized over locals.
#[must_use]
pub fn compute_flow_slice_hash(
    plan: &ReturnSlicePlan,
    graph: &FunctionFlowGraph,
    skeleton: &FunctionBodySkeleton,
) -> FlowSliceHash {
    COMPUTE_INVOCATIONS.with(|count| count.set(count.get().saturating_add(1)));
    let mut buf: Vec<u8> = Vec::with_capacity(256);
    buf.extend_from_slice(HASH_SALT);
    buf.push(HASH_SEP);

    for origin in plan.origins.iter() {
        match origin {
            SliceOrigin::Return(id) => {
                buf.push(b'O');
                buf.push(0);
                fold_u32(&mut buf, id.index() as u32);
            }
            SliceOrigin::Expr(id) => {
                buf.push(b'O');
                buf.push(1);
                fold_u32(&mut buf, id.index() as u32);
            }
        }
    }

    for segment in plan.demand_path.iter() {
        match segment {
            DemandSegment::Named(name) => {
                buf.push(b'D');
                fold_text(&mut buf, skeleton.name(*name));
            }
            DemandSegment::Foreign(text) => {
                buf.push(b'F');
                fold_text(&mut buf, text);
            }
        }
    }

    for node in plan.value_nodes.iter() {
        buf.push(b'V');
        fold_u32(&mut buf, node.index() as u32);
    }
    for node in plan.effect_only_nodes.iter() {
        buf.push(b'E');
        fold_u32(&mut buf, node.index() as u32);
    }

    // Internal edges of the selection, in selection order. Both node
    // sets are sorted, so the fold order is deterministic.
    let mut fold_edges = |nodes: &[FlowNodeId]| {
        for node in nodes {
            for edge in graph.out_edges(*node) {
                if !plan.is_selected(edge.to) {
                    continue;
                }
                buf.push(b'G');
                fold_u32(&mut buf, edge.from.index() as u32);
                fold_u32(&mut buf, edge.to.index() as u32);
                fold_u32(&mut buf, edge.ordinal);
                match &edge.kind {
                    FlowEdgeKind::ValueDef => buf.push(1),
                    FlowEdgeKind::PathWrite { path, certainty } => {
                        buf.push(2);
                        buf.push(match certainty {
                            SkeletonWriteCertainty::Definite => 1,
                            SkeletonWriteCertainty::Optional => 2,
                        });
                        for segment in path.iter() {
                            match segment {
                                SkeletonPathSegment::Static(name) => {
                                    buf.push(b'S');
                                    fold_text(&mut buf, skeleton.name(*name));
                                }
                                SkeletonPathSegment::Computed => buf.push(b'C'),
                            }
                        }
                    }
                    FlowEdgeKind::EvalEffect => buf.push(3),
                    FlowEdgeKind::ControlRegion => buf.push(4),
                }
            }
        }
    };
    fold_edges(&plan.value_nodes);
    fold_edges(&plan.effect_only_nodes);

    FlowSliceHash(hash_16(&buf))
}

fn fold_u32(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn fold_text(buf: &mut Vec<u8>, text: &str) {
    fold_u32(buf, u32::try_from(text.len()).unwrap_or(u32::MAX));
    buf.extend_from_slice(text.as_bytes());
    buf.push(HASH_SEP);
}
