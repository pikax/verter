//! Unified dead-operand deep-work matrix — the cross-operator table
//! proving the seven attributable deep-work classes stay at zero for every
//! dead operand class now that the sealed forcing boundary is the only
//! selectable evaluation entrance.
//!
//! Dead operand classes (one matrix row each):
//! 1. a dead conditional branch (the unselected arm of a decided check);
//! 2. a projection sibling member (the member next to the demanded one);
//! 3. a proven non-contributing intersection arm (the arm that cannot
//!    answer the demanded path);
//! 4. an unrelated mapped key (a produced member nobody demanded);
//! 5. a remap-dropped value (an `as`-remap the key domain drops).
//!
//! Deep-work classes and how each row pins them:
//! - **forcing attempts** — per-key `dispatch_cold_for`/`dispatch_warm_for`
//!   over the row's dead keys (exactly 0) plus the whole-family
//!   `DISPATCH_TRACE` class window (exactly the table's expected multiset,
//!   cold and warm);
//! - **locator dereferences** — the `LowerLocator` count in the class
//!   window;
//! - **substitutions** — `substitute_memo_misses` delta over the window;
//! - **nested value/body dispatches** — `instantiate_count` delta;
//! - **relation reads** — `relation_check_count` delta;
//! - **semantic allocations / origin writes** — interned-node delta plus
//!   `origin_edges_emitted` delta;
//! - **deferred-shell dereferences** — `evaluate_deferred_memo_misses`
//!   delta;
//! - **semantic dependency fact reads** — every dead operand's declaration
//!   lives in a SECOND file; the cold force runs under the fact tracer and
//!   its finalised read set names no fact of that file (a positive control
//!   proves demanding a dead operand DOES read it).
//!
//! Every row also asserts the ANSWER (a counter-only assertion would pass
//! for an implementation that skipped the work by skipping the semantics)
//! and the warm-repeat leg (identical warm demand adds zero nodes, zero
//! candidates, and enters exactly the warm hit's family).
//!
//! The conditional / mapped rows carry their operator-attributable
//! counters (`conditional_decided_count`, `branch_selections_true/false`,
//! `mapped_per_k_materializations`) as ordinary columns of the same
//! table — the matrix crosses operators with ONE assertion vocabulary.

use std::collections::BTreeMap;
use std::sync::Arc;

use verter_type_expr::locators::{AuthoredBodyLocator, LocatorSymbolSpace, TypeBodyPathStep};

use crate::project_semantic_dispatch::raise::{
    dispatch_cold_for, dispatch_warm_for, enable_dispatch_trace_for_test,
};
use crate::semantic_query::operand::{
    ForcedSemanticOperand, SemanticOperand, SemanticOperandForceProjection,
    SemanticOperandForceRequest,
};
use crate::semantic_query::{
    PathSegment, PrimitiveKind, ProjectionMode, ProjectionReductionContext, PropertyKey,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};
use crate::VerterHost;

use super::semantic_operand_tests::{
    assert_primitive, dispatch_classes_since, force_key_at, force_projecting, interned_nodes,
    locator_at, make_host, mint, trace_len, upsert_at,
};
use super::ProjectSemanticDispatch;

const OWNER: &str = "/w/operand/deadwork.ts";

/// The file every DEAD operand's declaration lives in. A fact of this file
/// in a cold force's read set is a dead-operand dependency read.
const DEAD: &str = "/w/operand/deadwork_dead.ts";

const SOURCE: &str = "\
import type { DeadCold, DeadExtra, DeadHeavy, DeadDropped, DeadB, DeadC } from './deadwork_dead';\n\
export type ColdShell = DeadCold;\n\
export type ExtraShell = DeadExtra;\n\
export type HeavyShell = DeadHeavy;\n\
export type DroppedShell = DeadDropped;\n\
export type BShell = DeadB;\n\
export type CShell = DeadC;\n\
export type Deep = { wanted: string; cold: ColdShell };\n\
export type Named = { name: string; extra: ExtraShell };\n\
export type Tagged = { tag: number; heavy: HeavyShell };\n\
export type Both = Named & Tagged;\n\
export type Cond<T> = T extends string ? { picked: \"yes\" } : DroppedShell;\n\
export type Text = string;\n\
export type Flat = { a: string; b: BShell; c: CShell };\n\
export type Boxed = { [K in keyof Flat]: { boxed: Flat[K] } };\n\
export type KeptOnly = { [K in keyof Flat as K extends \"a\" ? \"kept\" : never]: { boxed: Flat[K] } };\n\
";

/// Every dead operand's DECLARATION lives here, behind a local alias shell
/// in the owner file: emitting the shell (a local `DeclRef`) records no fact
/// of this file — only FORCING the dead operand resolves the alias through
/// the import and reads it.
const DEAD_SOURCE: &str = "\
export type DeadCold = { p: \"c0\"; q: \"c1\"; r: \"c2\" };\n\
export type DeadExtra = { p: \"e0\"; q: \"e1\" };\n\
export type DeadHeavy = { h0: \"h0\"; h1: \"h1\" };\n\
export type DeadDropped = { dropped: \"no\" };\n\
export type DeadB = number;\n\
export type DeadC = string[];\n\
";

/// The store counters one matrix row pins. Every row's expected vector is
/// a fixed-arity array over this SAME vocabulary — a row cannot silently
/// stop asserting a counter — and the runner additionally checks each
/// variant appears exactly once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Counter {
    SubstituteMisses,
    RelationChecks,
    Instantiations,
    DeferredMisses,
    OriginEdges,
    ConditionalDecided,
    BranchTrue,
    BranchFalse,
    MappedPerK,
}

/// The full counter vocabulary every row must pin, in one canonical order.
const COUNTER_VOCABULARY: [Counter; 9] = [
    Counter::SubstituteMisses,
    Counter::RelationChecks,
    Counter::Instantiations,
    Counter::DeferredMisses,
    Counter::OriginEdges,
    Counter::ConditionalDecided,
    Counter::BranchTrue,
    Counter::BranchFalse,
    Counter::MappedPerK,
];

fn counter_delta(
    before: &crate::semantic_query::SemanticGraphStats,
    after: &crate::semantic_query::SemanticGraphStats,
    counter: Counter,
) -> u64 {
    match counter {
        Counter::SubstituteMisses => after.substitute_memo_misses - before.substitute_memo_misses,
        Counter::RelationChecks => after.relation_check_count - before.relation_check_count,
        Counter::Instantiations => after.instantiate_count - before.instantiate_count,
        Counter::DeferredMisses => {
            after.evaluate_deferred_memo_misses - before.evaluate_deferred_memo_misses
        }
        Counter::OriginEdges => after.origin_edges_emitted - before.origin_edges_emitted,
        Counter::ConditionalDecided => {
            after.conditional_decided_count - before.conditional_decided_count
        }
        Counter::BranchTrue => after.branch_selections_true - before.branch_selections_true,
        Counter::BranchFalse => after.branch_selections_false - before.branch_selections_false,
        Counter::MappedPerK => {
            after.mapped_per_k_materializations - before.mapped_per_k_materializations
        }
    }
}

fn stats(host: &VerterHost) -> crate::semantic_query::SemanticGraphStats {
    host.project_type_store().semantic_graph().stats_snapshot()
}

fn graph_node_data(host: &VerterHost, node: SemanticNodeId) -> Arc<SemanticNodeData> {
    host.project_type_store()
        .semantic_graph()
        .node_data(node)
        .expect("matrix answer node must have graph data")
}

/// The published member names of an evaluated object surface, sorted.
fn member_names(host: &VerterHost, surface: SemanticNodeId) -> Vec<String> {
    match graph_node_data(host, surface).as_ref() {
        SemanticNodeData::Object(view) => {
            let mut names: Vec<String> = view
                .positive_members()
                .iter()
                .filter_map(|m| m.string_name().map(str::to_string))
                .collect();
            names.sort();
            names
        }
        other => panic!("matrix row expected an Object surface, got {other:?}"),
    }
}

fn host_with_source() -> VerterHost {
    let host = make_host();
    upsert_at(&host, DEAD, DEAD_SOURCE);
    upsert_at(&host, OWNER, SOURCE);
    host.set_import_dependencies(
        OWNER,
        vec![crate::types::DependencyResolution {
            specifier: "./deadwork_dead".to_string(),
            resolved_canonical_id: Some(DEAD.to_string()),
            possible_canonical_ids: Vec::new(),
        }],
    );
    host
}

/// Run `f` under the host's fact tracer and count the finalised facts that
/// name the DEAD file — the seventh deep-work class. A non-cacheable or
/// overflowed tracer is a fixture fault, never a zero.
fn dead_file_fact_reads<R>(host: &VerterHost, f: impl FnOnce() -> R) -> (R, usize) {
    use crate::resolver_core::{FactReadSetFinalise, FactVersionRef};
    let (value, read_set) =
        host.with_fact_tracer(verter_workspace::AggregateBasisSeed::Unvouched, f);
    let facts = match read_set.finalise() {
        FactReadSetFinalise::Ok(facts) => facts,
        FactReadSetFinalise::NonCacheable(reason) => {
            panic!("matrix tracer unexpectedly non-cacheable: {reason:?}")
        }
        FactReadSetFinalise::Overflow | FactReadSetFinalise::MutationUnstable => {
            panic!("matrix tracer overflowed on a tiny fixture")
        }
    };
    let dead = facts
        .iter()
        .filter(|fact| match fact {
            FactVersionRef::FileWholeHash { canonical_id, .. } => canonical_id == DEAD,
            _ => false,
        })
        .count();
    (value, dead)
}

fn locator(symbol: &str) -> AuthoredBodyLocator {
    locator_at(OWNER, symbol, LocatorSymbolSpace::Type, Arc::from([]))
}

fn whole(symbol: &str) -> AuthoredBodyLocator {
    locator(symbol)
}

fn member_value(symbol: &str, ordinal: u32) -> AuthoredBodyLocator {
    locator_at(
        OWNER,
        symbol,
        LocatorSymbolSpace::Type,
        Arc::from([
            TypeBodyPathStep::Member { ordinal },
            TypeBodyPathStep::MemberValue,
        ]),
    )
}

fn arm_locator(symbol: &str, ordinal: u32) -> AuthoredBodyLocator {
    locator_at(
        OWNER,
        symbol,
        LocatorSymbolSpace::Type,
        Arc::from([TypeBodyPathStep::IntersectionArm { ordinal }]),
    )
}

fn path_of(names: &[&str]) -> Arc<[PathSegment]> {
    Arc::from(
        names
            .iter()
            .map(|name| PathSegment::Member(PropertyKey::identifier(Arc::from(*name))))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

/// One matrix row: everything the common runner needs to drive the
/// selective evaluation and pin the full deep-work vector.
struct MatrixRow {
    label: &'static str,
    /// The selective demand. `None` = whole-surface force.
    path: Option<Arc<[PathSegment]>>,
    /// Mint the row's operand (may itself assert the mint succeeded).
    operand: fn(&ProjectSemanticDispatch<'_>) -> SemanticOperand,
    /// Check the ANSWER the evaluation owed the demand.
    answer: fn(&VerterHost, &ForcedSemanticOperand),
    /// Build the dead-target keys this row forbids any forcing attempt on.
    dead_keys: fn(&ProjectSemanticDispatch<'_>) -> Vec<SemanticQueryKey>,
    /// The exact per-family dispatch multiset of the COLD window.
    expected_cold_classes: &'static [(&'static str, usize)],
    /// The exact per-family dispatch multiset of the warm repeat.
    expected_warm_classes: &'static [(&'static str, usize)],
    /// Upper bound on interned nodes across the COLD window (the warm
    /// leg pins exactly 0).
    cold_node_bound: usize,
    /// The exact expected counter deltas over the COLD window — one entry
    /// per `Counter` variant (arity enforced by the array type).
    expected_counters: [(Counter, u64); COUNTER_VOCABULARY.len()],
}

fn classes(pairs: &[(&'static str, usize)]) -> BTreeMap<&'static str, usize> {
    pairs.iter().copied().collect()
}

/// The common assertion spine: cold window, dead keys, counter vector,
/// warm repeat, zero warm growth. Every row runs through this unchanged.
fn run_matrix_row(row: &MatrixRow) {
    let host = host_with_source();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    let operand = (row.operand)(&dispatch);
    // The dead keys and the whole-surface key are derived BEFORE the cold
    // window opens so their derivation cannot hide work inside the window.
    let dead_keys = (row.dead_keys)(&dispatch);

    // ── COLD window ────────────────────────────────────────────────────
    let window = trace_len();
    let nodes_before = interned_nodes(&host);
    let stats_before = stats(&host);
    let (forced, dead_fact_reads) = dead_file_fact_reads(&host, || match &row.path {
        Some(path) => force_projecting(&dispatch, &operand, context, Arc::clone(path)),
        None => match dispatch
            .force_semantic_operand(&operand, SemanticOperandForceRequest::new(context))
        {
            crate::semantic_query::QueryResult::Value(forced) => forced,
            other => panic!(
                "row {}: whole-surface force must resolve, got {other:?}",
                row.label
            ),
        },
    });
    let cold_classes = dispatch_classes_since(window);
    let cold_nodes = interned_nodes(&host) - nodes_before;
    let stats_after = stats(&host);
    (row.answer)(&host, &forced);

    // Forcing attempts: the cold window enters exactly the table's family
    // multiset — every other family (the families a whole-surface-then-
    // narrow or an eager-sibling route would spend its dead work in)
    // stays at zero.
    assert_eq!(
        cold_classes,
        classes(row.expected_cold_classes),
        "row {}: cold family window",
        row.label
    );

    // Semantic dependency fact reads: the cold force never read the file
    // every dead operand's declaration lives in.
    assert_eq!(
        dead_fact_reads, 0,
        "row {}: the cold force read {dead_fact_reads} fact(s) of the dead-operand file",
        row.label
    );

    // Forcing attempts on the DEAD targets: zero cold AND zero warm.
    for key in &dead_keys {
        assert_eq!(
            dispatch_cold_for(key) + dispatch_warm_for(key),
            0,
            "row {}: a dead operand target was forced",
            row.label
        );
    }

    // Substitutions / nested dispatches / relation reads / deferred
    // dereferences / origin writes / operator-attributable counters. The
    // row must name every vocabulary counter exactly once — a dropped or
    // duplicated class is a table defect, not a passing row.
    for counter in COUNTER_VOCABULARY {
        assert_eq!(
            row.expected_counters
                .iter()
                .filter(|(named, _)| *named == counter)
                .count(),
            1,
            "row {}: counter {:?} must be pinned exactly once",
            row.label,
            counter
        );
    }
    for (counter, expected) in &row.expected_counters {
        let observed = counter_delta(&stats_before, &stats_after, *counter);
        assert_eq!(
            observed, *expected,
            "row {}: counter {:?} delta",
            row.label, counter
        );
    }

    assert!(
        cold_nodes <= row.cold_node_bound,
        "row {}: cold window interned {cold_nodes} nodes, bound {}",
        row.label,
        row.cold_node_bound
    );

    // ── WARM repeat ────────────────────────────────────────────────────
    let warm_window = trace_len();
    let warm_nodes_before = interned_nodes(&host);
    let warm_forced = match &row.path {
        Some(path) => force_projecting(&dispatch, &operand, context, Arc::clone(path)),
        None => match dispatch
            .force_semantic_operand(&operand, SemanticOperandForceRequest::new(context))
        {
            crate::semantic_query::QueryResult::Value(forced) => forced,
            other => panic!(
                "row {}: warm whole-surface force must resolve, got {other:?}",
                row.label
            ),
        },
    };
    let warm_classes = dispatch_classes_since(warm_window);
    let warm_nodes = interned_nodes(&host) - warm_nodes_before;
    assert_eq!(
        warm_forced.node(),
        forced.node(),
        "row {}: warm node",
        row.label
    );

    assert_eq!(
        warm_classes,
        classes(row.expected_warm_classes),
        "row {}: warm family window — an identical warm demand must not \
         enter any other family",
        row.label
    );
    assert_eq!(
        warm_nodes, 0,
        "row {}: an identical warm demand must allocate no semantic nodes",
        row.label
    );
    for key in &dead_keys {
        let candidates = host
            .project_type_store()
            .semantic_graph()
            .slot_candidate_count_for_tests(key);
        assert_eq!(
            candidates, 0,
            "row {}: a dead target must never publish a candidate",
            row.label
        );
    }
}

fn dead_key_for(
    dispatch: &ProjectSemanticDispatch<'_>,
    locator: AuthoredBodyLocator,
    path: Option<Arc<[PathSegment]>>,
) -> SemanticQueryKey {
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let projection = match path {
        Some(path) => SemanticOperandForceProjection::Path(path),
        None => SemanticOperandForceProjection::WholeSurface,
    };
    let operand = mint(dispatch, locator);
    force_key_at(dispatch, &operand, context, projection)
}

// ─── Row 1: projection sibling member ───────────────────────────────────

fn row_projection_sibling() -> MatrixRow {
    MatrixRow {
        label: "projection-sibling-member",
        path: None,
        operand: |dispatch| mint(dispatch, member_value("Deep", 0)),
        answer: |host, forced| {
            assert_primitive(host, forced.node(), PrimitiveKind::String);
        },
        dead_keys: |dispatch| {
            vec![
                // The sibling member's own force family...
                dead_key_for(dispatch, member_value("Deep", 1), None),
                // ...and the whole-surface family a narrow-then-filter
                // route would have dispatched.
                dead_key_for(dispatch, whole("Deep"), None),
            ]
        },
        expected_cold_classes: &[("Instantiate", 1), ("LowerLocator", 1), ("ProjectPath", 1)],
        expected_warm_classes: &[("Instantiate", 1)],
        cold_node_bound: 4,
        expected_counters: [
            (Counter::OriginEdges, 0),
            (Counter::SubstituteMisses, 0),
            (Counter::RelationChecks, 0),
            (Counter::Instantiations, 0),
            (Counter::DeferredMisses, 0),
            (Counter::ConditionalDecided, 0),
            (Counter::BranchTrue, 0),
            (Counter::BranchFalse, 0),
            (Counter::MappedPerK, 0),
        ],
    }
}

// ─── Row 2: dead conditional branch ─────────────────────────────────────

fn row_conditional_dead_branch() -> MatrixRow {
    MatrixRow {
        label: "conditional-dead-branch",
        path: None,
        operand: |dispatch| {
            // The check operand `Text` (= string) is forced OUTSIDE the
            // row so the window observes only the conditional's own work.
            let check = match dispatch.force_semantic_operand(
                &mint(dispatch, whole("Text")),
                SemanticOperandForceRequest::new(ProjectionReductionContext::published(
                    ProjectionMode::Identity,
                )),
            ) {
                crate::semantic_query::QueryResult::Value(forced) => forced,
                other => panic!("check operand must force, got {other:?}"),
            };
            let check_operand = dispatch
                .mint_node_semantic_operand(&check)
                .expect("check node operand must mint");
            dispatch
                .mint_authored_semantic_operand(locator("Cond"), Arc::from([check_operand]))
                .expect("substituted conditional operand must mint")
        },
        answer: |host, forced| {
            assert_eq!(
                member_names(host, forced.node()),
                vec!["picked".to_string()],
                "the decided conditional publishes exactly the selected branch — \
                 the dead branch's `dropped` member must be absent"
            );
        },
        dead_keys: |dispatch| vec![dead_key_for(dispatch, arm_locator("Cond", 1), None)],
        expected_cold_classes: &[("Conditional", 1), ("Instantiate", 1), ("LowerLocator", 1)],
        expected_warm_classes: &[("Instantiate", 1)],
        cold_node_bound: 8,
        expected_counters: [
            (Counter::OriginEdges, 3),
            (Counter::ConditionalDecided, 1),
            (Counter::BranchTrue, 1),
            (Counter::BranchFalse, 0),
            // The conditional's OWN live substitution (T → string into the
            // check, extends, and both branch shells) — the dead branch
            // contributes no substitution beyond these.
            (Counter::SubstituteMisses, 6),
            // The force family entry itself (authored Instantiate).
            (Counter::Instantiations, 1),
            (Counter::DeferredMisses, 0),
            (Counter::MappedPerK, 0),
            // The decidability check `string extends string` is one relation
            // read; the dead branch adds none.
            (Counter::RelationChecks, 1),
        ],
    }
}

// ─── Row 3: proven non-contributing intersection arm ────────────────────

fn row_intersection_non_contributing_arm() -> MatrixRow {
    MatrixRow {
        label: "intersection-non-contributing-arm",
        path: Some(path_of(&["name"])),
        operand: |dispatch| mint(dispatch, whole("Both")),
        answer: |host, forced| {
            assert_primitive(host, forced.node(), PrimitiveKind::String);
        },
        dead_keys: |dispatch| {
            vec![
                // The heavy member lives ONLY in the non-contributing arm.
                dead_key_for(dispatch, whole("Both"), Some(path_of(&["heavy"]))),
                // Whole-surface demand nobody made.
                dead_key_for(dispatch, whole("Both"), None),
                // The sibling member in the contributing arm.
                dead_key_for(dispatch, whole("Both"), Some(path_of(&["extra"]))),
            ]
        },
        expected_cold_classes: &[
            ("Instantiate", 3),
            ("LowerLocator", 3),
            ("ProjectPath", 1),
            ("ResolveDecl", 2),
        ],
        expected_warm_classes: &[("Instantiate", 1)],
        cold_node_bound: 24,
        expected_counters: [
            (Counter::OriginEdges, 4),
            (Counter::SubstituteMisses, 0),
            (Counter::RelationChecks, 0),
            (Counter::Instantiations, 2),
            (Counter::DeferredMisses, 0),
            (Counter::ConditionalDecided, 0),
            (Counter::BranchTrue, 0),
            (Counter::BranchFalse, 0),
            (Counter::MappedPerK, 0),
        ],
    }
}

// ─── Row 4: unrelated mapped key ────────────────────────────────────────

/// The narrow mapped rail: the demanded produced name is
/// answered over the LOWERED carrier (a node operand), never by
/// resolving the whole mapped surface first.
fn row_unrelated_mapped_key() -> MatrixRow {
    MatrixRow {
        label: "unrelated-mapped-key",
        path: Some(path_of(&["a"])),
        operand: |dispatch| {
            // Force the declaration at Identity OUTSIDE the row's demand
            // to obtain its carrier node, then mint the NODE operand the
            // selective demand is actually addressed to.
            let carrier = match dispatch.force_semantic_operand(
                &mint(dispatch, whole("Boxed")),
                SemanticOperandForceRequest::new(ProjectionReductionContext::published(
                    ProjectionMode::Identity,
                )),
            ) {
                crate::semantic_query::QueryResult::Value(forced) => forced,
                other => panic!("mapped carrier must force, got {other:?}"),
            };
            dispatch
                .mint_node_semantic_operand(&carrier)
                .expect("carrier node operand must mint")
        },
        answer: |host, forced| {
            assert_eq!(
                member_names(host, forced.node()),
                vec!["boxed".to_string()],
                "the demanded produced member must answer with its value surface"
            );
        },
        dead_keys: |dispatch| {
            let carrier = match dispatch.force_semantic_operand(
                &mint(dispatch, whole("Boxed")),
                SemanticOperandForceRequest::new(ProjectionReductionContext::published(
                    ProjectionMode::Identity,
                )),
            ) {
                crate::semantic_query::QueryResult::Value(forced) => forced,
                other => panic!("mapped carrier must force, got {other:?}"),
            };
            let base = carrier.node();
            let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
            vec![
                // The whole mapped surface nobody demanded.
                SemanticQueryKey::ProjectPath {
                    base,
                    path: Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
                    context,
                },
                // The unrelated produced members.
                SemanticQueryKey::ProjectPath {
                    base,
                    path: path_of(&["b"]),
                    context,
                },
                SemanticQueryKey::ProjectPath {
                    base,
                    path: path_of(&["c"]),
                    context,
                },
            ]
        },
        expected_cold_classes: &[("ProjectPath", 1)],
        expected_warm_classes: &[("ProjectPath", 1)],
        cold_node_bound: 8,
        expected_counters: [
            (Counter::OriginEdges, 2),
            // The narrow rail answers the demanded produced member through
            // its substitution rail — the per-K materialiser never runs
            // for a single-key demand (zero for unrelated keys AND on
            // the demanded key's own per-K path).
            (Counter::MappedPerK, 0),
            (Counter::SubstituteMisses, 0),
            (Counter::RelationChecks, 0),
            (Counter::Instantiations, 0),
            (Counter::ConditionalDecided, 0),
            (Counter::BranchTrue, 0),
            (Counter::BranchFalse, 0),
            // The demanded produced member is answered over the lowered
            // carrier; the unrelated keys' value shells are never dereferenced.
            (Counter::DeferredMisses, 0),
        ],
    }
}

// ─── Row 5: remap-dropped value ─────────────────────────────────────────

fn row_remap_dropped_value() -> MatrixRow {
    MatrixRow {
        label: "remap-dropped-value",
        path: None,
        operand: |dispatch| mint(dispatch, whole("KeptOnly")),
        answer: |host, forced| {
            assert_eq!(
                member_names(host, forced.node()),
                vec!["kept".to_string()],
                "the remap drops every key but `a`; the dropped keys' values \
                 must be absent from the published surface"
            );
        },
        dead_keys: |_dispatch| {
            // The dropped keys produce no members, so their dead targets
            // are not addressable as published points — the row's
            // discriminator is `mapped_per_k_materializations == 1`
            // (three-key source, one survivor) plus the surface names.
            Vec::new()
        },
        expected_cold_classes: &[
            ("Conditional", 5),
            ("Instantiate", 2),
            ("KeyOf", 1),
            ("LowerLocator", 2),
            ("MappedType", 1),
            ("ProjectPath", 1),
            ("ResolveDecl", 1),
        ],
        expected_warm_classes: &[("Instantiate", 1)],
        cold_node_bound: 36,
        expected_counters: [
            // Three-key source, one remap survivor: exactly ONE per-K
            // value materialisation — the two dropped keys' values are
            // never forced.
            (Counter::MappedPerK, 1),
            (Counter::OriginEdges, 9),
            // The remap key domain and the survivor's value body
            // substitute their binders — the DROPPED keys' value bodies
            // contribute none of these substitutions.
            (Counter::SubstituteMisses, 19),
            (Counter::Instantiations, 2),
            (Counter::DeferredMisses, 12),
            (Counter::ConditionalDecided, 3),
            (Counter::BranchTrue, 1),
            (Counter::BranchFalse, 2),
            // The remap's per-key `K extends "a"` decidability checks —
            // key-domain classification, not dead-value work.
            (Counter::RelationChecks, 6),
        ],
    }
}

/// The unified cross-operator dead-work matrix. Each row is a separate
/// discriminating proof; the table shape keeps the assertion vocabulary
/// identical across operators so a new row cannot weaken an old class.
#[test]
fn unified_dead_operand_matrix_zero_deep_work() {
    let rows = [
        row_projection_sibling(),
        row_conditional_dead_branch(),
        row_intersection_non_contributing_arm(),
        row_unrelated_mapped_key(),
        row_remap_dropped_value(),
    ];
    for row in &rows {
        run_matrix_row(row);
    }
}

/// Positive control for the matrix's zero assertions: the counters and
/// dead-key probes are LIVE — driving one dead target's evaluation moves
/// it. Without this, an all-zero row could pass because the probe itself
/// was wired to nothing.
#[test]
fn matrix_dead_key_probe_discriminates() {
    let host = host_with_source();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    // The sibling member nobody demanded in row 1...
    let sibling = mint(&dispatch, member_value("Deep", 1));
    let sibling_key = force_key_at(
        &dispatch,
        &sibling,
        context,
        SemanticOperandForceProjection::WholeSurface,
    );
    let stats_before = stats(&host);
    let nodes_before = interned_nodes(&host);
    assert_eq!(
        dispatch_cold_for(&sibling_key) + dispatch_warm_for(&sibling_key),
        0,
        "control precondition: the sibling is untouched before the probe"
    );
    // ...becomes live work the moment it IS demanded.
    match dispatch.force_semantic_operand(&sibling, SemanticOperandForceRequest::new(context)) {
        crate::semantic_query::QueryResult::Value(forced) => {
            // Shallow-by-default: the sibling's value publishes as its local
            // `ColdShell` alias carrier, never the dead file's body.
            match graph_node_data(&host, forced.node()).as_ref() {
                SemanticNodeData::DeclRef { identity } => {
                    assert_eq!(identity.decl_name.as_ref(), "ColdShell");
                    assert_eq!(identity.canonical_id.as_ref(), OWNER);
                }
                other => {
                    panic!("the sibling member publishes its `ColdShell` carrier, got {other:?}")
                }
            }
        }
        other => panic!("control probe must force, got {other:?}"),
    }
    assert!(
        dispatch_cold_for(&sibling_key) + dispatch_warm_for(&sibling_key) > 0,
        "demanding the sibling moves its cold/warm probe — the matrix's \
         zero assertions discriminate"
    );
    let after = stats(&host);
    assert!(
        after.origin_edges_emitted > stats_before.origin_edges_emitted
            || after.mapped_per_k_materializations > stats_before.mapped_per_k_materializations
            || after.instantiate_count > stats_before.instantiate_count
            || interned_nodes(&host) > nodes_before,
        "demanding live work moves at least one attributable counter or \
         interns at least one node"
    );
}

/// Positive control for the matrix's fact-read class: demanding a dead
/// operand whose declaration lives in the dead file DOES put that file's
/// fact into the finalised read set — so a zero in the matrix is a real
/// zero, not a probe wired to nothing.
#[test]
fn matrix_dead_file_fact_probe_discriminates() {
    let host = host_with_source();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);

    // Project INTO row 1's dead sibling (`Deep.cold.p`): answering `p`
    // forces the `ColdShell` alias through the import to the dead file's
    // `DeadCold` declaration, so the read set must name the dead file. The
    // matrix asserts zero such facts when only `Deep.wanted` is forced.
    let deep = mint(&dispatch, whole("Deep"));
    let (forced, dead_fact_reads) = dead_file_fact_reads(&host, || {
        force_projecting(&dispatch, &deep, context, path_of(&["cold", "p"]))
    });
    match graph_node_data(&host, forced.node()).as_ref() {
        SemanticNodeData::Literal(crate::semantic_query::LiteralValue::String(value)) => {
            assert_eq!(value, "c0", "`Deep.cold.p` answers the dead file's literal");
        }
        other => panic!("`Deep.cold.p` must answer a string literal, got {other:?}"),
    }
    assert!(
        dead_fact_reads >= 1,
        "demanding the dead-file operand must read at least one fact of the dead file — \
         the matrix's zero fact-read assertions discriminate"
    );
}

/// One-authority convergence for the applicable-utility operators: the
/// builtin utility route is the ordinary `Instantiate` family — two
/// INDEPENDENTLY constructed canonical keys with identical
/// `(slot, args, context)` converge on one warm entry and publish exactly
/// one candidate. A wrapper-shaped second entrance would have to
/// reconstruct this same family or fork it — forking shows up here as a
/// second candidate / a cold miss on the second construction.
#[test]
fn builtin_utility_route_is_one_instantiate_family() {
    use crate::semantic_query::{InstantiateKey, LiteralValue, SemanticQueryApi};

    let host = host_with_source();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let _trace = enable_dispatch_trace_for_test();

    let graph = host.project_type_store().semantic_graph();
    let base = graph.intern_node(SemanticNodeData::Object(crate::test_surface_view! {
        members: Arc::from(
            vec![crate::semantic_query::SurfaceMember {
                excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                visibility: verter_type_expr::MemberVisibility::Public,
                key: crate::semantic_query::AuthoredPropertyKey::string("foo"),
                value: graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String)),
                optional: false,
                readonly: false,
                method_kind: None,
                has_implementation_body: false,
                declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                spans: Default::default(),
                declaration_origin: None,
            }]
            .into_boxed_slice(),
        ),
        call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        index_signatures: Arc::from(
            Vec::<crate::semantic_query::IndexSignature>::new().into_boxed_slice(),
        ),
        keyspace: None,
        has_index_signature: false,
    }));

    let build_key = |name: &str| {
        let key_set = graph.intern_node(SemanticNodeData::Union(
            crate::semantic_query::composite::CompositeList::query_subject(Arc::from(
                vec![
                    graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                        "foo".to_string(),
                    ))),
                ]
                .into_boxed_slice(),
            )),
        ));
        SemanticQueryKey::Instantiate(InstantiateKey::new(
            dispatch.builtin_type_slot(name),
            Arc::from(vec![base, key_set].into_boxed_slice()),
            dispatch.instantiate_context_for(
                "__builtin__",
                ProjectionReductionContext::published(ProjectionMode::Expanded),
            ),
        ))
    };

    let pick = build_key("Pick");
    let pick_again = build_key("Pick");
    assert_eq!(pick, pick_again, "canonical constructions agree on the key");

    let first = match dispatch.execute_type_node(pick.clone()) {
        crate::semantic_query::QueryResult::Value(output) => output.value,
        other => panic!("Pick utility must evaluate, got {other:?}"),
    };
    let second = match dispatch.execute_type_node(pick_again) {
        crate::semantic_query::QueryResult::Value(output) => output.value,
        other => panic!("second Pick construction must evaluate, got {other:?}"),
    };
    assert_eq!(
        first, second,
        "two independent canonical constructions converge on one warm node"
    );
    assert_eq!(
        dispatch_cold_for(&pick),
        1,
        "exactly one cold evaluation for the family"
    );
    assert_eq!(
        host.project_type_store()
            .semantic_graph()
            .slot_candidate_count_for_tests(&pick),
        1,
        "the utility family publishes exactly one candidate — no wrapper-\
         shaped dual route forked a second entry"
    );
}
