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

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::instant::Instant;

/// Outer entries to the macro-member materialiser pass.
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

/// Warm-hit returns served by `SemanticGraphStore::execute_cooperative`'s
/// fast-path (single non-diagnosed `entries.lock()` + slot read, no
/// inflight-table touch, no capture-token TLS, no second
/// `entries_lock_diagnosed()` call). Bumped once per fast-path return.
/// Strictly a subset of `EXECUTE_COOPERATIVE_WARM_HITS`: the two
/// counters sum to all warm hits, with `WARM_HIT_FAST_PATH_HITS`
/// counting the cheap branch and any residual on the slow branch
/// counted only by `EXECUTE_COOPERATIVE_WARM_HITS`.
pub static WARM_HIT_FAST_PATH_HITS: AtomicU64 = AtomicU64::new(0);

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
/// TypeExpr passed into the macro-member materialiser pass.
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

/// Outer-call counter for `Instantiate { body_mode: Expanded }`
/// dispatch.
///
/// Incremented at the entry of
/// `ProjectSemanticDispatch::build_instantiate` whenever the requested
/// `body_mode` is `Expanded`. Cold dispatches bump the counter; warm
/// cache hits skip the build path entirely and leave the counter
/// unchanged.
///
/// The slot-binding regression
/// `shallow_instantiation_ref_warm_pass_o1` reads this counter to
/// assert that warm second-pass empty-path Shallow queries do not
/// re-issue an `Expanded` Instantiate. The component-meta regression
/// `enrich_does_not_eagerly_instantiate_carrier` asserts the
/// graph-native slot-binding synthesis walk stays in `Navigate` mode
/// and never dispatches an `Expanded` Instantiate over the
/// slot-binding carrier. Default: 0.
pub static SLOT_BINDING_EXPANDED_INSTANTIATE_CALLS: AtomicU64 = AtomicU64::new(0);

/// Count "operator-bearing" nodes inside a TypeExpr. Operator-bearing
/// shapes are the ones that turn into a `dispatch_operator_with_recurse`
/// call after lowering: `Ref`, `IndexedAccess`, `Conditional`, `Mapped`,
/// `TypeOf`, `KeyOf`, `Infer`. Composite shapes (`Object`, `Union`,
/// `Intersection`, `Array`, `Tuple`, `Function`) are recursed into but
/// don't themselves count. Terminal shapes (`Primitive`, `Literal`,
/// `TypeParameter`, `Unknown`, `RecursiveRef`) and `Rest`/`Parenthesized`
/// (which the lowering walks through) are ignored.
pub fn count_operator_nodes(expr: &verter_type_expr::TypeExpr) -> u64 {
    use verter_type_expr::{ObjectMember, TypeExpr};

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
            // A constructor type's signature is walked identically to a function
            // type's (same `FunctionExpr` payload).
            TypeExpr::Function(f) | TypeExpr::ConstructorType(f) => {
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
            // Synthetic carriers are intrinsic terminal leaves with no
            // operator-bearing dispatch.
            | TypeExpr::SyntheticSlotBinding(_)
            | TypeExpr::Unknown { .. } => {}
        }
    }
    let mut count: u64 = 0;
    walk(expr, &mut count);
    count
}

/// Record one outer macro-member walk's TypeExpr operator-node count.
/// Updates both the running sum and the high-water mark via fetch_max.
pub fn record_outer_call_type_expr(expr: &verter_type_expr::TypeExpr) {
    let n = count_operator_nodes(expr);
    TYPE_EXPR_OPERATOR_NODE_COUNT_SUM.fetch_add(n, Ordering::Relaxed);
    MAX_TYPE_EXPR_OPERATOR_NODE_COUNT.fetch_max(n, Ordering::Relaxed);
}

/// Per-`SemanticQueryKey`-variant timing for
/// `dispatch_operator_with_recurse` — loop 7. Each call to
/// `dispatch_operator_with_recurse` is wrapped in a wall-clock timer
/// keyed on the variant of the dispatched `SemanticQueryKey`. The
/// counters together account for the entire inner-dispatch wall-clock
/// time on the hot path and let us answer "which operator kind is
/// the dominant cost?" for ChatMessage's 31-min cold path.
///
/// Index mapping (kept in sync with `kind_index_for_key`):
///   0 = ResolveDecl
///   1 = Instantiate
///   2 = ProjectMember
///   3 = IndexedAccess
///   4 = KeyOf
///   5 = MappedType
///   6 = Conditional
///   7 = TypeOf
///   8 = NormalizeUnion
///   9 = NormalizeIntersection
///  10 = ProjectPath
///  11 = ResolvedNamedType
///  12 = Relate
///  13 = ResolveMacroPayload
///  14 = ResolveClassSurface
///  15 = ResolveAmbientNamespace
///  16 = ResolveEnum
///  17 = ResolveOverloadSet
///  18 = ApparentType
///  19 = TemplateLiteralReduce
pub const DISPATCH_OPERATOR_KIND_COUNT: usize = 20;

/// Human-readable labels for each operator-kind index. Kept in sync
/// with the comment on `DISPATCH_OPERATOR_KIND_COUNT` and with the
/// `kind_index_for_key` helper so dump JSON keys are obvious.
pub const DISPATCH_OPERATOR_KIND_LABELS: [&str; DISPATCH_OPERATOR_KIND_COUNT] = [
    "ResolveDecl",
    "Instantiate",
    "ProjectMember",
    "IndexedAccess",
    "KeyOf",
    "MappedType",
    "Conditional",
    "TypeOf",
    "NormalizeUnion",
    "NormalizeIntersection",
    "ProjectPath",
    "ResolvedNamedType",
    "Relate",
    "ResolveMacroPayload",
    "ResolveClassSurface",
    "ResolveAmbientNamespace",
    "ResolveEnum",
    "ResolveOverloadSet",
    "ApparentType",
    "TemplateLiteralReduce",
];

/// Per-kind call counts. `dispatch_operator_with_recurse` increments
/// the matching index on entry. Sum across all indices equals
/// `DISPATCH_OPERATOR_WITH_RECURSE_CALLS`.
pub static DISPATCH_OPERATOR_KIND_CALLS: [AtomicU64; DISPATCH_OPERATOR_KIND_COUNT] = [
    // 18 = ApparentType, 19 = TemplateLiteralReduce (all zero-initialised;
    // order within the array is immaterial — `kind_index_for_key` keys it).
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Per-kind nanoseconds spent inside `dispatch_operator_with_recurse`
/// (including the body's `execute_read` call AND any recursive
/// `reduce_one` follow-up reductions). Wall-clock measured at the
/// function entry / exit. Sum across all indices is approximately
/// `DISPATCH_OPERATOR_TOTAL_NS`.
pub static DISPATCH_OPERATOR_KIND_NS: [AtomicU64; DISPATCH_OPERATOR_KIND_COUNT] = [
    // 18 = ApparentType, 19 = TemplateLiteralReduce (all zero-initialised;
    // order within the array is immaterial — `kind_index_for_key` keys it).
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];

/// Total nanoseconds spent inside `dispatch_operator_with_recurse`.
/// Sanity check against the per-kind sum.
pub static DISPATCH_OPERATOR_TOTAL_NS: AtomicU64 = AtomicU64::new(0);

// ===== Loop 8 — broadened materialize_ms instrumentation =====
//
// Call-count + wall-clock-ns pairs for the 10 top-level functions
// inside `materialize_ms`. Loop 7 falsified the
// `dispatch_operator_with_recurse` hypothesis (37 ms total of a
// 25.6-min request); the unaccounted 99.7% lives in these bodies.
//
// All timers are INCLUSIVE — a parent's NS includes any children's
// NS. Subtraction yields the exclusive cost.
//
// The `TimerGuard` RAII helper used to wrap each function body
// increments the calls counter on `new` and adds the elapsed ns
// on `Drop`. Multiple early-return sites are handled implicitly.

pub static MATERIALIZE_TYPE_EXPR_UNTIL_STABLE_CALLS: AtomicU64 = AtomicU64::new(0);
pub static MATERIALIZE_TYPE_EXPR_UNTIL_STABLE_NS: AtomicU64 = AtomicU64::new(0);

pub static MATERIALIZE_STRUCTURE_CALLS: AtomicU64 = AtomicU64::new(0);
pub static MATERIALIZE_STRUCTURE_NS: AtomicU64 = AtomicU64::new(0);

pub static RAISE_AND_REDUCE_CALLS: AtomicU64 = AtomicU64::new(0);
pub static RAISE_AND_REDUCE_NS: AtomicU64 = AtomicU64::new(0);

/// Calls counter for `reduce_graph_node_iterative` already exists
/// as `RAISE_REDUCE_GRAPH_NODE_ITERATIVE_CALLS` (Loop 5).
pub static REDUCE_GRAPH_NODE_ITERATIVE_NS: AtomicU64 = AtomicU64::new(0);

pub static RAISE_NODE_TO_TYPE_EXPR_CALLS: AtomicU64 = AtomicU64::new(0);
pub static RAISE_NODE_TO_TYPE_EXPR_NS: AtomicU64 = AtomicU64::new(0);

pub static PRODUCE_MACRO_OBJECT_SHAPES_CALLS: AtomicU64 = AtomicU64::new(0);
pub static PRODUCE_MACRO_OBJECT_SHAPES_NS: AtomicU64 = AtomicU64::new(0);

pub static WALK_MACRO_MEMBER_TYPES_CALLS: AtomicU64 = AtomicU64::new(0);
pub static WALK_MACRO_MEMBER_TYPES_NS: AtomicU64 = AtomicU64::new(0);

pub static APPEND_REGISTRY_ENTRIES_CALLS: AtomicU64 = AtomicU64::new(0);
pub static APPEND_REGISTRY_ENTRIES_NS: AtomicU64 = AtomicU64::new(0);

/// Every call to `lowered_root_reaches_transitive_cycle` that takes
/// the TypeExpr-walk fast path (no dispatch lowering). The deep-lower
/// path was only useful for `Ref` / `RecursiveRef` shapes that
/// directly carried a route-root identity in their lowered form;
/// `IndexedAccess` shapes always fell through the post-lowering match
/// to `_ => return false` after paying for the lowering recursion.
/// The walk path constructs a `DeclIdentity` from the outermost
/// `Ref`/`RecursiveRef` of the TypeExpr structure without triggering
/// any third-party shallow-file loads. Inert in production.
pub static LOWERED_ROOT_CYCLE_FAST_PATH_HITS: AtomicU64 = AtomicU64::new(0);

// Walk-macro-member-types sub-blocks: per-iteration counters across
// the outer `for (macro_index, mac) in snapshot.macros.iter()` loop.

/// DefineProps arm — entry checks (member-projection shape probe via
/// `expr_needs_projection_rescue`).
pub static WALK_DEFINE_PROPS_CHECKS_CALLS: AtomicU64 = AtomicU64::new(0);
pub static WALK_DEFINE_PROPS_CHECKS_NS: AtomicU64 = AtomicU64::new(0);

/// DefineProps arm — projection block (only fires when the member-
/// projection probe fires AND `properties.is_empty()`).
pub static WALK_DEFINE_PROPS_PROJECTION_CALLS: AtomicU64 = AtomicU64::new(0);
pub static WALK_DEFINE_PROPS_PROJECTION_NS: AtomicU64 = AtomicU64::new(0);

/// DefineProps arm — per-property `for property in &mut
/// define_props.result.value.properties` loop calling the
/// macro-member materialiser pass.
pub static WALK_DEFINE_PROPS_PROPERTY_LOOP_CALLS: AtomicU64 = AtomicU64::new(0);
pub static WALK_DEFINE_PROPS_PROPERTY_LOOP_NS: AtomicU64 = AtomicU64::new(0);

/// DefineEmits arm whole (small fan-out; counter increments once
/// per macro iter that lands in this arm).
pub static WALK_DEFINE_EMITS_ARM_CALLS: AtomicU64 = AtomicU64::new(0);
pub static WALK_DEFINE_EMITS_ARM_NS: AtomicU64 = AtomicU64::new(0);

/// DefineSlots arm whole (slot binding scan + projection +
/// per-property loop).
pub static WALK_DEFINE_SLOTS_ARM_CALLS: AtomicU64 = AtomicU64::new(0);
pub static WALK_DEFINE_SLOTS_ARM_NS: AtomicU64 = AtomicU64::new(0);

/// RAII timer that bumps a `calls` counter on construction and adds
/// elapsed nanoseconds to an `ns` counter on drop. Designed for
/// bodies with multiple early-return sites where wrapping every
/// `return` would be tedious. The two atomics must outlive the
/// guard (typically `&'static AtomicU64` from this module).
pub struct TimerGuard {
    started: Instant,
    ns_counter: &'static AtomicU64,
}

impl TimerGuard {
    /// Increment `calls` immediately and capture the start time;
    /// on drop, add `elapsed_ns` to `ns_counter`.
    pub fn new(calls_counter: &'static AtomicU64, ns_counter: &'static AtomicU64) -> Self {
        calls_counter.fetch_add(1, Ordering::Relaxed);
        Self {
            started: Instant::now(),
            ns_counter,
        }
    }

    /// Variant that does NOT bump a calls counter — used for
    /// `reduce_graph_node_iterative` where the calls counter is
    /// already incremented elsewhere by Loop 5.
    pub fn new_ns_only(ns_counter: &'static AtomicU64) -> Self {
        Self {
            started: Instant::now(),
            ns_counter,
        }
    }
}

impl Drop for TimerGuard {
    fn drop(&mut self) {
        let elapsed_ns = self.started.elapsed().as_nanos() as u64;
        self.ns_counter.fetch_add(elapsed_ns, Ordering::Relaxed);
    }
}

/// Map a `SemanticQueryKey` to its kind index. Kept in lockstep with
/// `DISPATCH_OPERATOR_KIND_LABELS` — adding a new variant requires
/// extending `DISPATCH_OPERATOR_KIND_COUNT`, the labels array, both
/// counter arrays, this match, and the test below.
pub fn kind_index_for_key(key: &crate::semantic_query::SemanticQueryKey) -> usize {
    use crate::semantic_query::SemanticQueryKey;
    match key {
        SemanticQueryKey::ResolveDecl(_) => 0,
        SemanticQueryKey::Instantiate { .. } => 1,
        SemanticQueryKey::ProjectMember { .. } => 2,
        SemanticQueryKey::IndexedAccess { .. } => 3,
        SemanticQueryKey::KeyOf { .. } => 4,
        SemanticQueryKey::MappedType { .. } => 5,
        SemanticQueryKey::Conditional { .. } => 6,
        SemanticQueryKey::TypeOf { .. } => 7,
        SemanticQueryKey::NormalizeUnion { .. } => 8,
        SemanticQueryKey::NormalizeIntersection { .. } => 9,
        SemanticQueryKey::ProjectPath { .. } => 10,
        SemanticQueryKey::ResolvedNamedType { .. } => 11,
        SemanticQueryKey::Relate { .. } => 12,
        SemanticQueryKey::ResolveMacroPayload { .. } => 13,
        SemanticQueryKey::ResolveClassSurface { .. } => 14,
        SemanticQueryKey::ResolveAmbientNamespace { .. } => 15,
        SemanticQueryKey::ResolveEnum { .. } => 16,
        SemanticQueryKey::ResolveOverloadSet { .. } => 17,
        SemanticQueryKey::ApparentType { .. } => 18,
        SemanticQueryKey::TemplateLiteralReduce { .. } => 19,
    }
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
    WARM_HIT_FAST_PATH_HITS.store(0, Ordering::Relaxed);
    MATERIALIZE_MEMO_PEEKS.store(0, Ordering::Relaxed);
    MATERIALIZE_MEMO_HITS.store(0, Ordering::Relaxed);
    MATERIALIZE_MEMO_PUBLISHES.store(0, Ordering::Relaxed);
    FAMILY_MEMO_HITS.store(0, Ordering::Relaxed);
    FAMILY_MEMO_MISSES.store(0, Ordering::Relaxed);
    MAX_TYPE_EXPR_OPERATOR_NODE_COUNT.store(0, Ordering::Relaxed);
    TYPE_EXPR_OPERATOR_NODE_COUNT_SUM.store(0, Ordering::Relaxed);
    EXECUTE_COOPERATIVE_BUILD_NS_TOTAL.store(0, Ordering::Relaxed);
    DISPATCH_OPERATOR_TOTAL_NS.store(0, Ordering::Relaxed);
    for slot in DISPATCH_OPERATOR_KIND_CALLS.iter() {
        slot.store(0, Ordering::Relaxed);
    }
    for slot in DISPATCH_OPERATOR_KIND_NS.iter() {
        slot.store(0, Ordering::Relaxed);
    }
    // Loop 8 — broadened materialize_ms counters.
    MATERIALIZE_TYPE_EXPR_UNTIL_STABLE_CALLS.store(0, Ordering::Relaxed);
    MATERIALIZE_TYPE_EXPR_UNTIL_STABLE_NS.store(0, Ordering::Relaxed);
    MATERIALIZE_STRUCTURE_CALLS.store(0, Ordering::Relaxed);
    MATERIALIZE_STRUCTURE_NS.store(0, Ordering::Relaxed);
    RAISE_AND_REDUCE_CALLS.store(0, Ordering::Relaxed);
    RAISE_AND_REDUCE_NS.store(0, Ordering::Relaxed);
    REDUCE_GRAPH_NODE_ITERATIVE_NS.store(0, Ordering::Relaxed);
    RAISE_NODE_TO_TYPE_EXPR_CALLS.store(0, Ordering::Relaxed);
    RAISE_NODE_TO_TYPE_EXPR_NS.store(0, Ordering::Relaxed);
    PRODUCE_MACRO_OBJECT_SHAPES_CALLS.store(0, Ordering::Relaxed);
    PRODUCE_MACRO_OBJECT_SHAPES_NS.store(0, Ordering::Relaxed);
    WALK_MACRO_MEMBER_TYPES_CALLS.store(0, Ordering::Relaxed);
    WALK_MACRO_MEMBER_TYPES_NS.store(0, Ordering::Relaxed);
    APPEND_REGISTRY_ENTRIES_CALLS.store(0, Ordering::Relaxed);
    APPEND_REGISTRY_ENTRIES_NS.store(0, Ordering::Relaxed);
    WALK_DEFINE_PROPS_CHECKS_CALLS.store(0, Ordering::Relaxed);
    WALK_DEFINE_PROPS_CHECKS_NS.store(0, Ordering::Relaxed);
    WALK_DEFINE_PROPS_PROJECTION_CALLS.store(0, Ordering::Relaxed);
    WALK_DEFINE_PROPS_PROJECTION_NS.store(0, Ordering::Relaxed);
    WALK_DEFINE_PROPS_PROPERTY_LOOP_CALLS.store(0, Ordering::Relaxed);
    WALK_DEFINE_PROPS_PROPERTY_LOOP_NS.store(0, Ordering::Relaxed);
    WALK_DEFINE_EMITS_ARM_CALLS.store(0, Ordering::Relaxed);
    WALK_DEFINE_EMITS_ARM_NS.store(0, Ordering::Relaxed);
    WALK_DEFINE_SLOTS_ARM_CALLS.store(0, Ordering::Relaxed);
    WALK_DEFINE_SLOTS_ARM_NS.store(0, Ordering::Relaxed);
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
    let warm_hit_fast_path_hits = WARM_HIT_FAST_PATH_HITS.load(Ordering::Relaxed);
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
    let dispatch_operator_total_ns = DISPATCH_OPERATOR_TOTAL_NS.load(Ordering::Relaxed);

    // Loop 8 — broadened materialize_ms counters.
    let materialize_type_expr_until_stable_calls =
        MATERIALIZE_TYPE_EXPR_UNTIL_STABLE_CALLS.load(Ordering::Relaxed);
    let materialize_type_expr_until_stable_ns =
        MATERIALIZE_TYPE_EXPR_UNTIL_STABLE_NS.load(Ordering::Relaxed);
    let materialize_structure_calls = MATERIALIZE_STRUCTURE_CALLS.load(Ordering::Relaxed);
    let materialize_structure_ns = MATERIALIZE_STRUCTURE_NS.load(Ordering::Relaxed);
    let raise_and_reduce_calls = RAISE_AND_REDUCE_CALLS.load(Ordering::Relaxed);
    let raise_and_reduce_ns = RAISE_AND_REDUCE_NS.load(Ordering::Relaxed);
    let reduce_graph_node_iterative_ns = REDUCE_GRAPH_NODE_ITERATIVE_NS.load(Ordering::Relaxed);
    let raise_node_to_type_expr_calls = RAISE_NODE_TO_TYPE_EXPR_CALLS.load(Ordering::Relaxed);
    let raise_node_to_type_expr_ns = RAISE_NODE_TO_TYPE_EXPR_NS.load(Ordering::Relaxed);
    let produce_macro_object_shapes_calls =
        PRODUCE_MACRO_OBJECT_SHAPES_CALLS.load(Ordering::Relaxed);
    let produce_macro_object_shapes_ns = PRODUCE_MACRO_OBJECT_SHAPES_NS.load(Ordering::Relaxed);
    let walk_macro_member_types_calls = WALK_MACRO_MEMBER_TYPES_CALLS.load(Ordering::Relaxed);
    let walk_macro_member_types_ns = WALK_MACRO_MEMBER_TYPES_NS.load(Ordering::Relaxed);
    let append_registry_entries_calls = APPEND_REGISTRY_ENTRIES_CALLS.load(Ordering::Relaxed);
    let append_registry_entries_ns = APPEND_REGISTRY_ENTRIES_NS.load(Ordering::Relaxed);

    let walk_define_props_checks_calls = WALK_DEFINE_PROPS_CHECKS_CALLS.load(Ordering::Relaxed);
    let walk_define_props_checks_ns = WALK_DEFINE_PROPS_CHECKS_NS.load(Ordering::Relaxed);
    let walk_define_props_projection_calls =
        WALK_DEFINE_PROPS_PROJECTION_CALLS.load(Ordering::Relaxed);
    let walk_define_props_projection_ns = WALK_DEFINE_PROPS_PROJECTION_NS.load(Ordering::Relaxed);
    let walk_define_props_property_loop_calls =
        WALK_DEFINE_PROPS_PROPERTY_LOOP_CALLS.load(Ordering::Relaxed);
    let walk_define_props_property_loop_ns =
        WALK_DEFINE_PROPS_PROPERTY_LOOP_NS.load(Ordering::Relaxed);
    let walk_define_emits_arm_calls = WALK_DEFINE_EMITS_ARM_CALLS.load(Ordering::Relaxed);
    let walk_define_emits_arm_ns = WALK_DEFINE_EMITS_ARM_NS.load(Ordering::Relaxed);
    let walk_define_slots_arm_calls = WALK_DEFINE_SLOTS_ARM_CALLS.load(Ordering::Relaxed);
    let walk_define_slots_arm_ns = WALK_DEFINE_SLOTS_ARM_NS.load(Ordering::Relaxed);

    let mut per_kind_calls = String::new();
    let mut per_kind_ns = String::new();
    for (idx, label) in DISPATCH_OPERATOR_KIND_LABELS.iter().enumerate() {
        let calls = DISPATCH_OPERATOR_KIND_CALLS[idx].load(Ordering::Relaxed);
        let ns = DISPATCH_OPERATOR_KIND_NS[idx].load(Ordering::Relaxed);
        if idx > 0 {
            per_kind_calls.push_str(",\n    ");
            per_kind_ns.push_str(",\n    ");
        } else {
            per_kind_calls.push_str("\n    ");
            per_kind_ns.push_str("\n    ");
        }
        per_kind_calls.push_str(&format!("\"{label}\": {calls}"));
        per_kind_ns.push_str(&format!("\"{label}\": {ns}"));
    }

    format!(
        "{{\n  \"MACRO_MEMBER_WALK_OUTER_CALLS\": {macro_member_walk_outer_calls},\n  \
         \"RAISE_REDUCE_GRAPH_NODE_ITERATIVE_CALLS\": {raise_reduce_graph_node_iterative_calls},\n  \
         \"DISPATCH_OPERATOR_WITH_RECURSE_CALLS\": {dispatch_operator_with_recurse_calls},\n  \
         \"EXECUTE_COOPERATIVE_CALLS\": {execute_cooperative_calls},\n  \
         \"EXECUTE_COOPERATIVE_COLD_BUILDS\": {execute_cooperative_cold_builds},\n  \
         \"EXECUTE_COOPERATIVE_WARM_HITS\": {execute_cooperative_warm_hits},\n  \
         \"WARM_HIT_FAST_PATH_HITS\": {warm_hit_fast_path_hits},\n  \
         \"MATERIALIZE_MEMO_PEEKS\": {materialize_memo_peeks},\n  \
         \"MATERIALIZE_MEMO_HITS\": {materialize_memo_hits},\n  \
         \"MATERIALIZE_MEMO_PUBLISHES\": {materialize_memo_publishes},\n  \
         \"FAMILY_MEMO_HITS\": {family_memo_hits},\n  \
         \"FAMILY_MEMO_MISSES\": {family_memo_misses},\n  \
         \"MAX_TYPE_EXPR_OPERATOR_NODE_COUNT\": {max_type_expr_operator_node_count},\n  \
         \"TYPE_EXPR_OPERATOR_NODE_COUNT_SUM\": {type_expr_operator_node_count_sum},\n  \
         \"EXECUTE_COOPERATIVE_BUILD_NS_TOTAL\": {execute_cooperative_build_ns_total},\n  \
         \"DISPATCH_OPERATOR_TOTAL_NS\": {dispatch_operator_total_ns},\n  \
         \"MATERIALIZE_TYPE_EXPR_UNTIL_STABLE_CALLS\": {materialize_type_expr_until_stable_calls},\n  \
         \"MATERIALIZE_TYPE_EXPR_UNTIL_STABLE_NS\": {materialize_type_expr_until_stable_ns},\n  \
         \"MATERIALIZE_STRUCTURE_CALLS\": {materialize_structure_calls},\n  \
         \"MATERIALIZE_STRUCTURE_NS\": {materialize_structure_ns},\n  \
         \"RAISE_AND_REDUCE_CALLS\": {raise_and_reduce_calls},\n  \
         \"RAISE_AND_REDUCE_NS\": {raise_and_reduce_ns},\n  \
         \"REDUCE_GRAPH_NODE_ITERATIVE_NS\": {reduce_graph_node_iterative_ns},\n  \
         \"RAISE_NODE_TO_TYPE_EXPR_CALLS\": {raise_node_to_type_expr_calls},\n  \
         \"RAISE_NODE_TO_TYPE_EXPR_NS\": {raise_node_to_type_expr_ns},\n  \
         \"PRODUCE_MACRO_OBJECT_SHAPES_CALLS\": {produce_macro_object_shapes_calls},\n  \
         \"PRODUCE_MACRO_OBJECT_SHAPES_NS\": {produce_macro_object_shapes_ns},\n  \
         \"WALK_MACRO_MEMBER_TYPES_CALLS\": {walk_macro_member_types_calls},\n  \
         \"WALK_MACRO_MEMBER_TYPES_NS\": {walk_macro_member_types_ns},\n  \
         \"APPEND_REGISTRY_ENTRIES_CALLS\": {append_registry_entries_calls},\n  \
         \"APPEND_REGISTRY_ENTRIES_NS\": {append_registry_entries_ns},\n  \
         \"WALK_DEFINE_PROPS_CHECKS_CALLS\": {walk_define_props_checks_calls},\n  \
         \"WALK_DEFINE_PROPS_CHECKS_NS\": {walk_define_props_checks_ns},\n  \
         \"WALK_DEFINE_PROPS_PROJECTION_CALLS\": {walk_define_props_projection_calls},\n  \
         \"WALK_DEFINE_PROPS_PROJECTION_NS\": {walk_define_props_projection_ns},\n  \
         \"WALK_DEFINE_PROPS_PROPERTY_LOOP_CALLS\": {walk_define_props_property_loop_calls},\n  \
         \"WALK_DEFINE_PROPS_PROPERTY_LOOP_NS\": {walk_define_props_property_loop_ns},\n  \
         \"WALK_DEFINE_EMITS_ARM_CALLS\": {walk_define_emits_arm_calls},\n  \
         \"WALK_DEFINE_EMITS_ARM_NS\": {walk_define_emits_arm_ns},\n  \
         \"WALK_DEFINE_SLOTS_ARM_CALLS\": {walk_define_slots_arm_calls},\n  \
         \"WALK_DEFINE_SLOTS_ARM_NS\": {walk_define_slots_arm_ns},\n  \
         \"DISPATCH_OPERATOR_KIND_CALLS\": {{{per_kind_calls}\n  }},\n  \
         \"DISPATCH_OPERATOR_KIND_NS\": {{{per_kind_ns}\n  }}\n}}"
    )
}

// =====================================================================
// Watchdog backtrace dumper
// =====================================================================
//
// Hang-detection infrastructure for the cold-path investigation. The
// bench harness (or any caller) spawns a background watchdog thread
// that samples a "progress beat" counter at a regular interval. If the
// counter has not advanced past a threshold of stalls, the watchdog
// flips `WATCHDOG_DUMP_BACKTRACE_NOW` to true. The next call into a
// hot-path function that has been wired with `watchdog_check_and_dump`
// captures `std::backtrace::Backtrace::force_capture()` and prints it
// to stderr — i.e., a self-backtrace from inside the hung recursion.
//
// This is an in-process replacement for an external sampling
// debugger (`cdb` / `windbg` / `samply --record` are all unavailable
// on the dev Windows host the bench is currently run on).
//
// Wiring contract:
//
// 1. Hot-path functions (e.g. `shallow_lower_type_expr`) call
//    [`watchdog_beat`] on every entry to advance
//    `WATCHDOG_PROGRESS_BEAT`. They also call
//    [`watchdog_check_and_dump`] which checks the
//    `WATCHDOG_DUMP_BACKTRACE_NOW` flag and emits a self-backtrace
//    if set. Both functions are inert (single relaxed atomic load,
//    no allocation, no syscall) when the watchdog is not active —
//    i.e., the wiring has zero hot-path cost in production builds
//    that never start the watchdog thread.
//
// 2. The bench harness (or test) calls
//    [`spawn_watchdog`] before driving the workload. Pass the stall
//    threshold (seconds with no `watchdog_beat`) and the sample
//    interval (poll period of the watchdog thread).
//
// 3. On hang detection the watchdog sets
//    `WATCHDOG_DUMP_BACKTRACE_NOW`. The next hot-path entry sees the
//    flag and dumps. After a successful dump the flag is reset, but
//    the stall counter is NOT reset — i.e. on a sustained hang the
//    watchdog keeps re-arming and we get periodic stack samples.

/// Per-call progress counter. `watchdog_beat()` increments this
/// atomically (relaxed) on every hot-path entry. The watchdog thread
/// reads it on each tick to detect hang (no advancement between
/// ticks).
pub static WATCHDOG_PROGRESS_BEAT: AtomicU64 = AtomicU64::new(0);

/// Set true by the watchdog thread when it detects a stall. Read +
/// cleared by `watchdog_check_and_dump()` on the next hot-path entry.
pub static WATCHDOG_DUMP_BACKTRACE_NOW: AtomicBool = AtomicBool::new(false);

/// Serial number used by the watchdog thread to label dump windows
/// for cross-referencing the resulting `[WATCHDOG_DUMP]` lines.
pub static WATCHDOG_DUMP_SERIAL: AtomicU64 = AtomicU64::new(0);

/// Whether the watchdog has been spawned for this process. Used by
/// `watchdog_beat()` to avoid touching the atomic counter when no
/// watchdog is listening.
pub static WATCHDOG_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Hot-path entry helper. Bumps the progress counter when the
/// watchdog is active; otherwise no-op. Inline + `#[cold]` on the
/// load path keeps the inert cost to a single relaxed load.
#[inline]
pub fn watchdog_beat() {
    if WATCHDOG_ACTIVE.load(Ordering::Relaxed) {
        WATCHDOG_PROGRESS_BEAT.fetch_add(1, Ordering::Relaxed);
    }
}

/// Hot-path checkpoint. If the watchdog signalled a stall, capture
/// `std::backtrace::Backtrace::force_capture()` and emit it to stderr
/// tagged with `label`. The `force_capture` call ignores
/// `RUST_BACKTRACE` and always produces a stack — necessary because
/// release builds default to no backtrace.
#[inline]
pub fn watchdog_check_and_dump(label: &'static str) {
    if WATCHDOG_DUMP_BACKTRACE_NOW.load(Ordering::Relaxed)
        && WATCHDOG_DUMP_BACKTRACE_NOW
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    {
        watchdog_capture_and_emit(label);
    }
}

#[cold]
#[inline(never)]
fn watchdog_capture_and_emit(label: &'static str) {
    let serial = WATCHDOG_DUMP_SERIAL.load(Ordering::Relaxed);
    let bt = std::backtrace::Backtrace::force_capture();
    eprintln!(
        "[WATCHDOG_DUMP] serial={} label={} backtrace=\n{}",
        serial, label, bt
    );
}

/// Watchdog mode — controls when the watchdog requests a backtrace
/// dump.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchdogMode {
    /// Stall mode — request a dump only when `WATCHDOG_PROGRESS_BEAT`
    /// has not advanced for the threshold window. Useful for true
    /// hangs where the beat counter goes silent.
    Stall,
    /// Sample mode — request a dump every `sample_interval_ms`,
    /// regardless of beat progress. Useful for slow recursive work
    /// where the beat counter advances rapidly but the call is
    /// stuck inside a single deep recursion. Each dump shows the
    /// CURRENT recursion path so consecutive samples reveal the
    /// hot path.
    Sample,
}

/// Spawn the watchdog thread. Returns a handle the caller can drop
/// to detach (the thread keeps running until the process exits). Idempotent —
/// calling twice replaces the active state but does not stop the
/// prior thread (the prior thread keeps polling but the new thread
/// drives the new thresholds).
///
/// `stall_threshold_ms` — number of milliseconds with no
/// `watchdog_beat()` advance before a dump is requested (Stall mode
/// only).
/// `sample_interval_ms` — how often the watchdog wakes. In Sample
/// mode each tick triggers a dump request; in Stall mode each tick
/// checks the beat counter against the threshold.
pub fn spawn_watchdog_with_mode(
    mode: WatchdogMode,
    stall_threshold_ms: u64,
    sample_interval_ms: u64,
) {
    WATCHDOG_ACTIVE.store(true, Ordering::Relaxed);
    let stall_threshold = std::time::Duration::from_millis(stall_threshold_ms);
    let sample_interval = std::time::Duration::from_millis(sample_interval_ms);
    std::thread::Builder::new()
        .name("verter-watchdog".to_string())
        .spawn(move || match mode {
            WatchdogMode::Stall => watchdog_stall_loop(stall_threshold, sample_interval),
            WatchdogMode::Sample => watchdog_sample_loop(sample_interval),
        })
        .expect("spawn watchdog thread");
}

/// Backwards-compatible alias for [`spawn_watchdog_with_mode`] in
/// `Stall` mode. New callers should pick the explicit mode.
pub fn spawn_watchdog(stall_threshold_ms: u64, sample_interval_ms: u64) {
    spawn_watchdog_with_mode(WatchdogMode::Stall, stall_threshold_ms, sample_interval_ms);
}

fn watchdog_stall_loop(stall_threshold: std::time::Duration, sample_interval: std::time::Duration) {
    let mut last_beat = WATCHDOG_PROGRESS_BEAT.load(Ordering::Relaxed);
    let mut last_advance = Instant::now();
    loop {
        std::thread::sleep(sample_interval);
        let current_beat = WATCHDOG_PROGRESS_BEAT.load(Ordering::Relaxed);
        if current_beat != last_beat {
            last_beat = current_beat;
            last_advance = Instant::now();
            continue;
        }
        if last_advance.elapsed() >= stall_threshold {
            let serial = WATCHDOG_DUMP_SERIAL.fetch_add(1, Ordering::Relaxed) + 1;
            eprintln!(
                "[WATCHDOG_STALL] serial={} stalled_for_ms={:.0} beat={}",
                serial,
                last_advance.elapsed().as_secs_f64() * 1000.0,
                current_beat,
            );
            WATCHDOG_DUMP_BACKTRACE_NOW.store(true, Ordering::Relaxed);
            last_advance = Instant::now();
        }
    }
}

fn watchdog_sample_loop(sample_interval: std::time::Duration) {
    loop {
        std::thread::sleep(sample_interval);
        let serial = WATCHDOG_DUMP_SERIAL.fetch_add(1, Ordering::Relaxed) + 1;
        let beat = WATCHDOG_PROGRESS_BEAT.load(Ordering::Relaxed);
        eprintln!("[WATCHDOG_SAMPLE] serial={} beat={}", serial, beat);
        WATCHDOG_DUMP_BACKTRACE_NOW.store(true, Ordering::Relaxed);
    }
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
            "WARM_HIT_FAST_PATH_HITS",
            "MATERIALIZE_MEMO_PEEKS",
            "MATERIALIZE_MEMO_HITS",
            "MATERIALIZE_MEMO_PUBLISHES",
            "FAMILY_MEMO_HITS",
            "FAMILY_MEMO_MISSES",
            "MAX_TYPE_EXPR_OPERATOR_NODE_COUNT",
            "TYPE_EXPR_OPERATOR_NODE_COUNT_SUM",
            "EXECUTE_COOPERATIVE_BUILD_NS_TOTAL",
            "DISPATCH_OPERATOR_TOTAL_NS",
            "DISPATCH_OPERATOR_KIND_CALLS",
            "DISPATCH_OPERATOR_KIND_NS",
            "MATERIALIZE_TYPE_EXPR_UNTIL_STABLE_CALLS",
            "MATERIALIZE_TYPE_EXPR_UNTIL_STABLE_NS",
            "MATERIALIZE_STRUCTURE_CALLS",
            "MATERIALIZE_STRUCTURE_NS",
            "RAISE_AND_REDUCE_CALLS",
            "RAISE_AND_REDUCE_NS",
            "REDUCE_GRAPH_NODE_ITERATIVE_NS",
            "RAISE_NODE_TO_TYPE_EXPR_CALLS",
            "RAISE_NODE_TO_TYPE_EXPR_NS",
            "PRODUCE_MACRO_OBJECT_SHAPES_CALLS",
            "PRODUCE_MACRO_OBJECT_SHAPES_NS",
            "WALK_MACRO_MEMBER_TYPES_CALLS",
            "WALK_MACRO_MEMBER_TYPES_NS",
            "APPEND_REGISTRY_ENTRIES_CALLS",
            "APPEND_REGISTRY_ENTRIES_NS",
            "WALK_DEFINE_PROPS_CHECKS_CALLS",
            "WALK_DEFINE_PROPS_CHECKS_NS",
            "WALK_DEFINE_PROPS_PROJECTION_CALLS",
            "WALK_DEFINE_PROPS_PROJECTION_NS",
            "WALK_DEFINE_PROPS_PROPERTY_LOOP_CALLS",
            "WALK_DEFINE_PROPS_PROPERTY_LOOP_NS",
            "WALK_DEFINE_EMITS_ARM_CALLS",
            "WALK_DEFINE_EMITS_ARM_NS",
            "WALK_DEFINE_SLOTS_ARM_CALLS",
            "WALK_DEFINE_SLOTS_ARM_NS",
        ] {
            assert!(
                json.contains(key),
                "dump_loop5_instrumentation_counters missing key {key}: {json}"
            );
        }
        for label in DISPATCH_OPERATOR_KIND_LABELS {
            assert!(
                json.contains(&format!("\"{label}\":")),
                "dump_loop5_instrumentation_counters missing operator-kind label {label}: {json}"
            );
        }
    }

    #[test]
    fn kind_index_for_key_distinct_for_each_variant() {
        use crate::semantic_query::{DeclKey, IndexKey, ResolveDeclKey, ScopeId, SemanticQueryKey};
        use crate::ProjectionMode;

        let dummy_id: Arc<str> = Arc::from("/x");
        let scope = ScopeId {
            canonical_id: Arc::clone(&dummy_id),
            local_scope: None,
        };
        let identity = DeclKey {
            canonical_id: Arc::clone(&dummy_id),
            decl_name: Arc::from("X"),
        };
        let dummy_node = crate::semantic_query::SemanticNodeId(1);

        let resolve_decl = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
            scope: scope.clone(),
            name: Arc::from("X"),
        });
        let instantiate = SemanticQueryKey::Instantiate {
            base: identity.clone(),
            args: Arc::from(Vec::new().into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Skeleton,
            ),
        };
        let project_member = SemanticQueryKey::ProjectMember {
            base: dummy_node,
            member: Arc::from("p"),
            mode: ProjectionMode::Navigate,
        };
        let indexed_access = SemanticQueryKey::IndexedAccess {
            base: dummy_node,
            index: IndexKey::String(Arc::from("k")),
            mode: ProjectionMode::Navigate,
        };
        let key_of = SemanticQueryKey::KeyOf {
            base: dummy_node,
            context: crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Expanded,
            ),
        };
        let conditional = SemanticQueryKey::Conditional {
            check: dummy_node,
            extends: dummy_node,
            true_branch: dummy_node,
            false_branch: dummy_node,
            distributive: false,
        };
        let normalize_union = SemanticQueryKey::NormalizeUnion {
            members: Arc::from(Vec::new().into_boxed_slice()),
        };
        let normalize_intersection = SemanticQueryKey::NormalizeIntersection {
            members: Arc::from(Vec::new().into_boxed_slice()),
        };
        let project_path = SemanticQueryKey::ProjectPath {
            base: dummy_node,
            path: Arc::from(Vec::new().into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Navigate,
            ),
        };
        let relate = SemanticQueryKey::Relate {
            source: dummy_node,
            target: dummy_node,
        };
        let slot = crate::semantic_query::ResolvedDeclSlotIdentity::type_slot(
            Arc::clone(&dummy_id),
            Arc::from("X"),
            0,
            Default::default(),
            Default::default(),
        );
        let class_surface = SemanticQueryKey::ResolveClassSurface {
            decl_slot: slot.clone(),
            type_args: Arc::from(Vec::new().into_boxed_slice()),
            side: crate::semantic_query::ClassSurfaceSide::Instance,
            context: crate::semantic_query::ClassSurfaceContext {
                resolve_env_hash: Default::default(),
                mode: ProjectionMode::Shallow,
            },
        };
        let ambient_namespace = SemanticQueryKey::ResolveAmbientNamespace {
            namespace_slot: slot
                .with_symbol_space(crate::semantic_query::SemanticSymbolSpace::Namespace),
            type_args: Arc::from(Vec::new().into_boxed_slice()),
            context: crate::semantic_query::AmbientNamespaceContext {
                resolve_env_hash: Default::default(),
                mode: ProjectionMode::Shallow,
            },
        };
        let resolve_enum = SemanticQueryKey::ResolveEnum {
            enum_slot: slot.clone(),
            context: crate::semantic_query::EnumContext {
                resolve_env_hash: Default::default(),
            },
        };
        let overload_set = SemanticQueryKey::ResolveOverloadSet {
            callee: dummy_node,
            type_args: Arc::from(Vec::new().into_boxed_slice()),
            context: crate::semantic_query::OverloadSetContext {
                resolve_env_hash: Default::default(),
            },
        };
        let apparent_type = SemanticQueryKey::ApparentType {
            base: dummy_node,
            context: crate::semantic_query::ApparentTypeContext {
                type_env_hash: Default::default(),
                lib_env_hash: Default::default(),
                project_identity: 0,
            },
        };
        let template_literal_reduce = SemanticQueryKey::TemplateLiteralReduce {
            pattern: Arc::from(Vec::new().into_boxed_slice()),
            args: Arc::from(Vec::new().into_boxed_slice()),
            context: crate::semantic_query::TemplateLiteralReduceContext {
                resolve_env_hash: Default::default(),
                type_env_hash: Default::default(),
                lib_env_hash: Default::default(),
                project_identity: 0,
            },
        };

        // Discriminating: each of these MUST hit a distinct index in
        // the kind table; if any two collide the test fails.
        let observed = [
            kind_index_for_key(&resolve_decl),
            kind_index_for_key(&instantiate),
            kind_index_for_key(&project_member),
            kind_index_for_key(&indexed_access),
            kind_index_for_key(&key_of),
            kind_index_for_key(&conditional),
            kind_index_for_key(&normalize_union),
            kind_index_for_key(&normalize_intersection),
            kind_index_for_key(&project_path),
            kind_index_for_key(&relate),
            kind_index_for_key(&class_surface),
            kind_index_for_key(&ambient_namespace),
            kind_index_for_key(&resolve_enum),
            kind_index_for_key(&overload_set),
            kind_index_for_key(&apparent_type),
            kind_index_for_key(&template_literal_reduce),
        ];
        let expected = [0usize, 1, 2, 3, 4, 6, 8, 9, 10, 12, 14, 15, 16, 17, 18, 19];
        assert_eq!(observed, expected);
        // No off-by-one in the static label table:
        assert_eq!(
            DISPATCH_OPERATOR_KIND_LABELS.len(),
            DISPATCH_OPERATOR_KIND_COUNT
        );
        assert_eq!(
            DISPATCH_OPERATOR_KIND_CALLS.len(),
            DISPATCH_OPERATOR_KIND_COUNT
        );
        assert_eq!(
            DISPATCH_OPERATOR_KIND_NS.len(),
            DISPATCH_OPERATOR_KIND_COUNT
        );
    }

    #[test]
    fn count_operator_nodes_terminal_zero() {
        use verter_type_expr::{PrimitiveName, TypeExpr};
        let expr = TypeExpr::Primitive(PrimitiveName::String);
        assert_eq!(count_operator_nodes(&expr), 0);
    }

    #[test]
    fn count_operator_nodes_indexed_access_three() {
        // IndexedAccess(Ref<A>, Ref<B>) → 1 indexed-access + 2 refs = 3
        use verter_type_expr::TypeExpr;
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

    #[test]
    fn timer_guard_increments_calls_immediately_and_records_ns_on_drop() {
        // Discriminating: a brand-new TimerGuard must bump `calls` by
        // exactly 1 on `new`, leave `ns` at 0 until drop, and add a
        // strictly-positive `ns` on drop. Verifies the guard's
        // contract — without this, the guard could trivially "pass"
        // by being a no-op.
        static CALLS: AtomicU64 = AtomicU64::new(0);
        static NS: AtomicU64 = AtomicU64::new(0);
        CALLS.store(0, Ordering::Relaxed);
        NS.store(0, Ordering::Relaxed);

        let calls_before = CALLS.load(Ordering::Relaxed);
        let ns_before = NS.load(Ordering::Relaxed);
        {
            let _guard = TimerGuard::new(&CALLS, &NS);
            // Inside the guard's scope: calls already incremented,
            // ns still untouched (on-drop semantics).
            assert_eq!(CALLS.load(Ordering::Relaxed), calls_before + 1);
            assert_eq!(NS.load(Ordering::Relaxed), ns_before);
            // Burn a few microseconds so the elapsed window is
            // measurable. `Instant::elapsed` in CI can be 0 ns for
            // tight no-ops on Windows; spinning on a small atomic
            // arithmetic guarantees strictly-positive ns.
            let mut sink: u64 = 1;
            for i in 0..10_000u64 {
                sink = sink.wrapping_add(i ^ 0x5555);
            }
            // Force the optimizer to keep the loop.
            std::hint::black_box(sink);
        }
        // After drop: ns must be strictly larger than before.
        let ns_after = NS.load(Ordering::Relaxed);
        assert!(
            ns_after > ns_before,
            "TimerGuard::drop must record positive ns; ns_before={ns_before} ns_after={ns_after}"
        );
    }

    #[test]
    fn timer_guard_ns_only_does_not_bump_calls() {
        // `new_ns_only` is for `reduce_graph_node_iterative` which
        // already increments its calls counter elsewhere (Loop 5).
        // Discriminating: the variant must record ns on drop but NOT
        // touch `calls` — if it accidentally touched a calls counter
        // we'd double-count.
        static SHOULD_NOT_TOUCH: AtomicU64 = AtomicU64::new(0);
        static NS: AtomicU64 = AtomicU64::new(0);
        SHOULD_NOT_TOUCH.store(7, Ordering::Relaxed);
        NS.store(0, Ordering::Relaxed);

        {
            let _guard = TimerGuard::new_ns_only(&NS);
            let mut sink: u64 = 1;
            for i in 0..10_000u64 {
                sink = sink.wrapping_add(i ^ 0xaaaa);
            }
            std::hint::black_box(sink);
        }

        assert_eq!(
            SHOULD_NOT_TOUCH.load(Ordering::Relaxed),
            7,
            "TimerGuard::new_ns_only must NOT touch any calls counter"
        );
        assert!(
            NS.load(Ordering::Relaxed) > 0,
            "TimerGuard::new_ns_only must still record ns on drop"
        );
    }
}
