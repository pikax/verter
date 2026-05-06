//! Loop 5 — performance instrumentation counters for ChatMessage cold-path
//! investigation. Atomics live at module scope and are bumped at named call
//! sites identified in the loop-5 brief. The counters are inert in production
//! (no behavior change) and are dumped via [`dump_loop5_instrumentation_counters`]
//! after the bench `materialize_ms` window so the audit pipeline can emit
//! them as sidecar JSON.
//!
//! Hypothesis attribution mapping (orchestrator memory):
//!
//! - (a) Tree-shape — `MAX_TYPE_EXPR_OPERATOR_NODE_COUNT` /
//!   `TYPE_EXPR_OPERATOR_NODE_COUNT_SUM` measure how many operator nodes
//!   the lowered TypeExpr presents to `raise_and_reduce`.
//! - (b) Cache-key mismatch — `MATERIALIZE_MEMO_HITS` /
//!   `MATERIALIZE_MEMO_PEEKS` measures the host-memo hit rate; the ratio
//!   reveals whether equivalent surface forms collide.
//! - (c) Per-dispatch overhead — `EXECUTE_COOPERATIVE_BUILD_NS_TOTAL` /
//!   `EXECUTE_COOPERATIVE_COLD_BUILDS` measures the average ns per cold
//!   build. Expensive builds dominate at high cold counts.
//! - (d) Mostly cache HITS still paying admission cost — comparing
//!   `EXECUTE_COOPERATIVE_WARM_HITS` / `EXECUTE_COOPERATIVE_CALLS` shows
//!   the warm-fraction; if the hit rate is high but total time stays
//!   high, admission overhead dominates.

use std::sync::atomic::{AtomicU64, Ordering};

/// Outer entries to `materialize_component_meta_macro_shape_member_type_expr`.
/// One increment per request × per macro member walked.
pub static MACRO_MEMBER_WALK_OUTER_CALLS: AtomicU64 = AtomicU64::new(0);

/// Inner entries to `ProjectSemanticDispatch::reduce_graph_node_iterative`.
/// Each call walks the operator topology of one root SemanticNodeId.
pub static RAISE_REDUCE_GRAPH_NODE_ITERATIVE_CALLS: AtomicU64 = AtomicU64::new(0);

/// Entries to `ProjectSemanticDispatch::dispatch_operator_with_recurse`.
/// Each call dispatches one operator node through `execute_cooperative`.
pub static DISPATCH_OPERATOR_WITH_RECURSE_CALLS: AtomicU64 = AtomicU64::new(0);

/// Entries to `SemanticGraphStore::execute_cooperative`. Counts every
/// logical call regardless of warm/cold disposition.
pub static EXECUTE_COOPERATIVE_CALLS: AtomicU64 = AtomicU64::new(0);

/// Cold-build invocations of the build closure inside
/// `SemanticGraphStore::execute_cooperative`.
pub static EXECUTE_COOPERATIVE_COLD_BUILDS: AtomicU64 = AtomicU64::new(0);

/// Warm-hit returns from `SemanticGraphStore::execute_cooperative`
/// (path 1 in the cooperative semantics — `self.get(&key).is_some()`).
pub static EXECUTE_COOPERATIVE_WARM_HITS: AtomicU64 = AtomicU64::new(0);

/// Peeks (read attempts) into the host-owned `MaterializeMemoDb`
/// inside `materialize_component_meta_type_expr_until_stable_full`.
pub static MATERIALIZE_MEMO_PEEKS: AtomicU64 = AtomicU64::new(0);

/// Hits on the host-owned `MaterializeMemoDb`. Increments on the
/// branch that returns the cached value via `host_db.peek(...)`.
pub static MATERIALIZE_MEMO_HITS: AtomicU64 = AtomicU64::new(0);

/// Publishes (write-throughs) into the host-owned `MaterializeMemoDb`
/// after a fresh raise+reduce produces a new MaterializedTypeExpr.
pub static MATERIALIZE_MEMO_PUBLISHES: AtomicU64 = AtomicU64::new(0);

/// Family-memo (SemanticGraphStore) cold/warm splits as observed at
/// the `execute_cooperative` entry. Synonymous with
/// WARM_HITS/COLD_BUILDS but kept separate for the report schema.
pub static FAMILY_MEMO_HITS: AtomicU64 = AtomicU64::new(0);
pub static FAMILY_MEMO_MISSES: AtomicU64 = AtomicU64::new(0);

/// Maximum (single-call) operator-node count observed for any
/// TypeExpr passed into
/// `materialize_component_meta_macro_shape_member_type_expr`.
/// Tracked via fetch_max so the value is the high-water mark.
pub static MAX_TYPE_EXPR_OPERATOR_NODE_COUNT: AtomicU64 = AtomicU64::new(0);

/// Running sum of operator-node counts for every `(member, mode)` walk.
/// Divide by `MACRO_MEMBER_WALK_OUTER_CALLS` to get the mean
/// per-outer-call operator-node load.
pub static TYPE_EXPR_OPERATOR_NODE_COUNT_SUM: AtomicU64 = AtomicU64::new(0);

/// Running sum of nanoseconds spent inside the
/// `SemanticGraphStore::execute_cooperative` build closure. Divided by
/// `EXECUTE_COOPERATIVE_COLD_BUILDS` gives the mean ns per cold build.
pub static EXECUTE_COOPERATIVE_BUILD_NS_TOTAL: AtomicU64 = AtomicU64::new(0);

/// Count "operator-bearing" nodes inside a TypeExpr. Operator-bearing
/// shapes are the ones that turn into a `dispatch_operator_with_recurse`
/// call after lowering: `Ref`, `IndexedAccess`, `Conditional`, `Mapped`,
/// `TypeOf`, `KeyOf`, `Infer`. Composite shapes (`Object`, `Union`,
/// `Intersection`, `Array`, `Tuple`, `Function`) are recursed into but
/// don't themselves count. Terminal shapes (`Primitive`, `Literal`,
/// `TypeParameter`, `Unknown`, `RecursiveRef`) and `Rest`/`Parenthesized`
/// (which the lowering walks through) are ignored.
pub fn count_operator_nodes(expr: &verter_semantic::analysis::type_expr::TypeExpr) -> u64 {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    fn walk(expr: &TypeExpr, acc: &mut u64) {
        match expr {
            // operator-bearing
            TypeExpr::Ref { type_arguments, .. } => {
                *acc += 1;
                for ta in type_arguments.iter() {
                    walk(ta, acc);
                }
            }
            TypeExpr::IndexedAccess { object, index } => {
                *acc += 1;
                walk(object, acc);
                walk(index, acc);
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                *acc += 1;
                walk(check, acc);
                walk(extends, acc);
                walk(true_type, acc);
                walk(false_type, acc);
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                *acc += 1;
                walk(source, acc);
                walk(value, acc);
                if let Some(n) = name_type.as_deref() {
                    walk(n, acc);
                }
            }
            TypeExpr::TypeOf(_) => *acc += 1,
            TypeExpr::KeyOf(inner) => {
                *acc += 1;
                walk(inner, acc);
            }
            TypeExpr::Infer { .. } => *acc += 1,

            // composite - recurse but don't count
            TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => {
                for a in arms.iter() {
                    walk(a, acc);
                }
            }
            TypeExpr::Array { element, .. } => walk(element, acc),
            TypeExpr::Tuple { elements, .. } => {
                for el in elements.iter() {
                    walk(&el.ty, acc);
                }
            }
            TypeExpr::Object(obj) => {
                for member in obj.properties.iter() {
                    match member {
                        ObjectMember::Property(p) => walk(&p.ty, acc),
                        ObjectMember::Method(m) => {
                            for p in m.function.parameters.iter() {
                                walk(&p.ty, acc);
                            }
                            if let Some(rt) = m.function.return_type.as_deref() {
                                walk(rt, acc);
                            }
                        }
                        ObjectMember::IndexSignature(s) => {
                            walk(&s.key_type, acc);
                            walk(&s.value_type, acc);
                        }
                        ObjectMember::CallSignature(f) | ObjectMember::ConstructSignature(f) => {
                            for p in f.parameters.iter() {
                                walk(&p.ty, acc);
                            }
                            if let Some(rt) = f.return_type.as_deref() {
                                walk(rt, acc);
                            }
                        }
                    }
                }
            }
            TypeExpr::Function(f) => {
                for p in f.parameters.iter() {
                    walk(&p.ty, acc);
                }
                if let Some(rt) = f.return_type.as_deref() {
                    walk(rt, acc);
                }
            }
            TypeExpr::TemplateLiteral { expressions, .. } => {
                for e in expressions.iter() {
                    walk(e, acc);
                }
            }
            TypeExpr::Rest(inner) | TypeExpr::Parenthesized(inner) => walk(inner, acc),

            // terminals
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::Unknown { .. } => {}
        }
    }
    let mut count: u64 = 0;
    walk(expr, &mut count);
    count
}

/// Record one outer macro-member walk's TypeExpr operator-node count.
/// Updates both the running sum and the high-water mark via fetch_max.
pub fn record_outer_call_type_expr(expr: &verter_semantic::analysis::type_expr::TypeExpr) {
    let n = count_operator_nodes(expr);
    TYPE_EXPR_OPERATOR_NODE_COUNT_SUM.fetch_add(n, Ordering::Relaxed);
    MAX_TYPE_EXPR_OPERATOR_NODE_COUNT.fetch_max(n, Ordering::Relaxed);
}

/// Reset every counter to zero. Used between bench passes if the caller
/// wants per-pass attribution. Not invoked by default; the bench dumps
/// cumulative values.
pub fn reset_all() {
    MACRO_MEMBER_WALK_OUTER_CALLS.store(0, Ordering::Relaxed);
    RAISE_REDUCE_GRAPH_NODE_ITERATIVE_CALLS.store(0, Ordering::Relaxed);
    DISPATCH_OPERATOR_WITH_RECURSE_CALLS.store(0, Ordering::Relaxed);
    EXECUTE_COOPERATIVE_CALLS.store(0, Ordering::Relaxed);
    EXECUTE_COOPERATIVE_COLD_BUILDS.store(0, Ordering::Relaxed);
    EXECUTE_COOPERATIVE_WARM_HITS.store(0, Ordering::Relaxed);
    MATERIALIZE_MEMO_PEEKS.store(0, Ordering::Relaxed);
    MATERIALIZE_MEMO_HITS.store(0, Ordering::Relaxed);
    MATERIALIZE_MEMO_PUBLISHES.store(0, Ordering::Relaxed);
    FAMILY_MEMO_HITS.store(0, Ordering::Relaxed);
    FAMILY_MEMO_MISSES.store(0, Ordering::Relaxed);
    MAX_TYPE_EXPR_OPERATOR_NODE_COUNT.store(0, Ordering::Relaxed);
    TYPE_EXPR_OPERATOR_NODE_COUNT_SUM.store(0, Ordering::Relaxed);
    EXECUTE_COOPERATIVE_BUILD_NS_TOTAL.store(0, Ordering::Relaxed);
}

/// Snapshot of every loop-5 counter at the moment of the call. Returned
/// as a JSON-shaped string so the bench can write it as a sidecar file
/// alongside the audit JSON.
pub fn dump_loop5_instrumentation_counters() -> String {
    let macro_member_walk_outer_calls = MACRO_MEMBER_WALK_OUTER_CALLS.load(Ordering::Relaxed);
    let raise_reduce_graph_node_iterative_calls =
        RAISE_REDUCE_GRAPH_NODE_ITERATIVE_CALLS.load(Ordering::Relaxed);
    let dispatch_operator_with_recurse_calls =
        DISPATCH_OPERATOR_WITH_RECURSE_CALLS.load(Ordering::Relaxed);
    let execute_cooperative_calls = EXECUTE_COOPERATIVE_CALLS.load(Ordering::Relaxed);
    let execute_cooperative_cold_builds = EXECUTE_COOPERATIVE_COLD_BUILDS.load(Ordering::Relaxed);
    let execute_cooperative_warm_hits = EXECUTE_COOPERATIVE_WARM_HITS.load(Ordering::Relaxed);
    let materialize_memo_peeks = MATERIALIZE_MEMO_PEEKS.load(Ordering::Relaxed);
    let materialize_memo_hits = MATERIALIZE_MEMO_HITS.load(Ordering::Relaxed);
    let materialize_memo_publishes = MATERIALIZE_MEMO_PUBLISHES.load(Ordering::Relaxed);
    let family_memo_hits = FAMILY_MEMO_HITS.load(Ordering::Relaxed);
    let family_memo_misses = FAMILY_MEMO_MISSES.load(Ordering::Relaxed);
    let max_type_expr_operator_node_count =
        MAX_TYPE_EXPR_OPERATOR_NODE_COUNT.load(Ordering::Relaxed);
    let type_expr_operator_node_count_sum =
        TYPE_EXPR_OPERATOR_NODE_COUNT_SUM.load(Ordering::Relaxed);
    let execute_cooperative_build_ns_total =
        EXECUTE_COOPERATIVE_BUILD_NS_TOTAL.load(Ordering::Relaxed);

    format!(
        "{{\n  \"MACRO_MEMBER_WALK_OUTER_CALLS\": {macro_member_walk_outer_calls},\n  \
         \"RAISE_REDUCE_GRAPH_NODE_ITERATIVE_CALLS\": {raise_reduce_graph_node_iterative_calls},\n  \
         \"DISPATCH_OPERATOR_WITH_RECURSE_CALLS\": {dispatch_operator_with_recurse_calls},\n  \
         \"EXECUTE_COOPERATIVE_CALLS\": {execute_cooperative_calls},\n  \
         \"EXECUTE_COOPERATIVE_COLD_BUILDS\": {execute_cooperative_cold_builds},\n  \
         \"EXECUTE_COOPERATIVE_WARM_HITS\": {execute_cooperative_warm_hits},\n  \
         \"MATERIALIZE_MEMO_PEEKS\": {materialize_memo_peeks},\n  \
         \"MATERIALIZE_MEMO_HITS\": {materialize_memo_hits},\n  \
         \"MATERIALIZE_MEMO_PUBLISHES\": {materialize_memo_publishes},\n  \
         \"FAMILY_MEMO_HITS\": {family_memo_hits},\n  \
         \"FAMILY_MEMO_MISSES\": {family_memo_misses},\n  \
         \"MAX_TYPE_EXPR_OPERATOR_NODE_COUNT\": {max_type_expr_operator_node_count},\n  \
         \"TYPE_EXPR_OPERATOR_NODE_COUNT_SUM\": {type_expr_operator_node_count_sum},\n  \
         \"EXECUTE_COOPERATIVE_BUILD_NS_TOTAL\": {execute_cooperative_build_ns_total}\n}}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn dump_emits_all_keys() {
        let json = dump_loop5_instrumentation_counters();
        for key in [
            "MACRO_MEMBER_WALK_OUTER_CALLS",
            "RAISE_REDUCE_GRAPH_NODE_ITERATIVE_CALLS",
            "DISPATCH_OPERATOR_WITH_RECURSE_CALLS",
            "EXECUTE_COOPERATIVE_CALLS",
            "EXECUTE_COOPERATIVE_COLD_BUILDS",
            "EXECUTE_COOPERATIVE_WARM_HITS",
            "MATERIALIZE_MEMO_PEEKS",
            "MATERIALIZE_MEMO_HITS",
            "MATERIALIZE_MEMO_PUBLISHES",
            "FAMILY_MEMO_HITS",
            "FAMILY_MEMO_MISSES",
            "MAX_TYPE_EXPR_OPERATOR_NODE_COUNT",
            "TYPE_EXPR_OPERATOR_NODE_COUNT_SUM",
            "EXECUTE_COOPERATIVE_BUILD_NS_TOTAL",
        ] {
            assert!(
                json.contains(key),
                "dump_loop5_instrumentation_counters missing key {key}: {json}"
            );
        }
    }

    #[test]
    fn count_operator_nodes_terminal_zero() {
        use verter_semantic::analysis::type_expr::{PrimitiveName, TypeExpr};
        let expr = TypeExpr::Primitive(PrimitiveName::String);
        assert_eq!(count_operator_nodes(&expr), 0);
    }

    #[test]
    fn count_operator_nodes_indexed_access_three() {
        // IndexedAccess(Ref<A>, Ref<B>) → 1 indexed-access + 2 refs = 3
        use verter_semantic::analysis::type_expr::TypeExpr;
        let make_ref = |name: &str| TypeExpr::Ref {
            name: Arc::from(name),
            type_arguments: Arc::from(Vec::<TypeExpr>::new()),
        };
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(make_ref("A")),
            index: Arc::new(make_ref("B")),
        };
        assert_eq!(count_operator_nodes(&expr), 3);
    }
}
