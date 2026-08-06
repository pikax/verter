//! @ai-generated - flow-slice node discriminators: the hash-then-lower
//! round trip, the once-per-content-version graph build (share-vs-split
//! check 4's discriminating fixture), the three-layer budget
//! non-admission, warm-hit identity, content-version keying, and the
//! empty-fact-rail pin (no slice identity in `ReadSetSignature.facts`).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use verter_semantic::analysis::flow::flow_ir::{FlowExprShape, FlowObjectEntry, FlowObjectKey};
use verter_semantic::analysis::flow::peeker::{FlowSliceBudget, FlowSliceBudgetAxis};
use verter_semantic::analysis::flow::{
    build_function_body_skeleton, FunctionBodySkeleton, FunctionBodySource,
};
use verter_semantic::analysis::function_program::{FunctionDeclarationRef, FunctionProgramKey};
use verter_semantic::facts::SymbolSpace;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::TopLevelOwnerId;

use super::*;
use crate::cache_runtime::lookup;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::SemanticQueryApi as _;
use crate::types::HostConfig;
use crate::VerterHost;

fn skeleton_of(source: &str) -> FunctionBodySkeleton {
    let allocator = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::ts();
    let ret = oxc_parser::Parser::new(&allocator, source, source_type).parse();
    assert!(
        ret.errors.is_empty(),
        "fixture must parse: {:?}",
        ret.errors
    );
    for statement in &ret.program.body {
        if let oxc_ast::ast::Statement::FunctionDeclaration(function) = statement {
            if let Some(body_source) = FunctionBodySource::from_function(function) {
                return build_function_body_skeleton(&body_source);
            }
        }
    }
    panic!("fixture must contain a bodied function declaration");
}

/// A skeleton source over pinned `(key, source-text)` fixtures, counting
/// every build so the once-per-content-version fixtures can assert on
/// it.
struct FixtureSkeletonSource {
    fixtures: Vec<(FlowSliceFunctionKey, &'static str)>,
    builds: AtomicUsize,
}

impl FixtureSkeletonSource {
    fn new(fixtures: Vec<(FlowSliceFunctionKey, &'static str)>) -> Self {
        Self {
            fixtures,
            builds: AtomicUsize::new(0),
        }
    }

    fn build_calls(&self) -> usize {
        self.builds.load(Ordering::SeqCst)
    }
}

impl FlowBodySkeletonSource for FixtureSkeletonSource {
    fn build_skeleton(
        &self,
        key: &FlowSliceFunctionKey,
        _resolver: &dyn ResolverContext,
    ) -> Option<FunctionBodySkeleton> {
        let source = self
            .fixtures
            .iter()
            .find(|(fixture_key, _)| fixture_key == key)
            .map(|(_, source)| *source)?;
        self.builds.fetch_add(1, Ordering::SeqCst);
        Some(skeleton_of(source))
    }
}

fn function_key(canonical: &str, name: &str, body_hash_tag: u8) -> FlowSliceFunctionKey {
    FlowSliceFunctionKey {
        canonical_id: Arc::from(canonical),
        function: FunctionProgramKey {
            declaration: FunctionDeclarationRef {
                owner: TopLevelOwnerId::ordinary_file(),
                name: Arc::from(name),
                space: SymbolSpace::Value,
            },
            part: FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        },
        flow_body_stable_hash: [body_hash_tag; 16],
        flow_body_exact_hash: [body_hash_tag; 16],
        parse_env_hash: [0u8; 16],
        parser_version: 1,
    }
}

fn hash_key(function: FlowSliceFunctionKey, path: &[&str]) -> FlowSliceHashKey {
    FlowSliceHashKey {
        function,
        demand: FlowSliceDemandIdentity {
            projection_path: Arc::from(
                path.iter()
                    .map(|segment| Arc::from(*segment))
                    .collect::<Vec<Arc<str>>>()
                    .into_boxed_slice(),
            ),
        },
    }
}

const MYTYPE_FIXTURE: &str =
    "function myType() { const a = new Mytype(); const b = 1; return { a, b } }";

struct Rig {
    host: VerterHost,
    graphs: Arc<FunctionFlowGraphStore>,
    source: Arc<FixtureSkeletonSource>,
    hash_node: FlowSliceHashNode,
    lowered_node: FlowSliceLoweredBodyNode,
}

fn rig(fixtures: Vec<(FlowSliceFunctionKey, &'static str)>, budget: FlowSliceBudget) -> Rig {
    let host = VerterHost::new_standalone(HostConfig::default());
    let graphs = Arc::new(FunctionFlowGraphStore::new());
    let source = Arc::new(FixtureSkeletonSource::new(fixtures));
    let skeletons: Arc<dyn FlowBodySkeletonSource> = Arc::clone(&source) as _;
    let budget: FlowSliceBudgetCell = Arc::new(parking_lot::RwLock::new(budget));
    let hash_node = FlowSliceHashNode::new(
        Arc::clone(&graphs),
        Arc::clone(&skeletons),
        Arc::clone(&budget),
    );
    let lowered_node =
        FlowSliceLoweredBodyNode::new(Arc::clone(&graphs), Arc::clone(&skeletons), budget);
    Rig {
        host,
        graphs,
        source,
        hash_node,
        lowered_node,
    }
}

fn planned(outcome: FlowSliceHashOutcome) -> FlowSliceHash {
    match outcome {
        FlowSliceHashOutcome::Planned(slice_hash) => slice_hash,
        FlowSliceHashOutcome::BudgetExceeded(exceeded) => {
            panic!("expected a planned slice, got budget refusal {exceeded:?}")
        }
    }
}

/// The hash-then-lower round trip: the hash node plans + hashes; the
/// lowered key is built FROM the minted hash (the opaque
/// `FlowSliceHash` has no other producer, so the hash structurally
/// precedes the lowered lookup); the lowered node serves the IR of
/// exactly the selected slice. Registry-live guard
/// (`GuardId::FlowSliceLoweredBodyDoesNotComputeSliceHash`) — TWO rails:
///
/// - STRUCTURAL (hash-before-lowered-KEY): the lowered node's key
///   REQUIRES a minted `FlowSliceHash`, which has no byte export and no
///   constructor outside `compute_flow_slice_hash` — the lowered store
///   is unaddressable until the hasher ran.
/// - BEHAVIORAL (the lowered COMPUTE hashes nothing): the type-state
///   alone cannot pin this half — `compute_flow_slice_hash` is a public
///   producer any compute could call — so the guard binds it to the
///   per-thread invocation counter: the lowered-node lookup below
///   performs ZERO hash computations (a `compute_flow_slice_hash` call
///   inserted into `FlowSliceLoweredBodyNode::compute` flips exactly
///   this assertion).
#[test]
pub(crate) fn hash_then_lower_round_trip_serves_lowered_slice_ir() {
    use verter_semantic::analysis::flow::hashing::compute_flow_slice_hash_thread_invocations;

    let function = function_key("/fixtures/my-type.ts", "myType", 1);
    let rig = rig(
        vec![(function.clone(), MYTYPE_FIXTURE)],
        FlowSliceBudget::default(),
    );
    let ctx: &dyn ResolverContext = &rig.host;

    let key = hash_key(function, &["b"]);
    let invocations_before_hash = compute_flow_slice_hash_thread_invocations();
    let outcome = lookup(&rig.hash_node, key.clone(), ctx).expect("hash lookup");
    let planned = planned(outcome);
    assert_eq!(
        compute_flow_slice_hash_thread_invocations(),
        invocations_before_hash + 1,
        "the hash node's cold compute performs exactly one slice-hash \
         computation (the counter binding is live, not vacuous)"
    );

    let lowered_key = FlowSliceLoweredKey {
        hash_key: key.clone(),
        slice_hash: planned,
    };
    let invocations_before_lowered = compute_flow_slice_hash_thread_invocations();
    let ir = lookup(&rig.lowered_node, lowered_key, ctx).expect("lowered lookup");
    assert_eq!(
        compute_flow_slice_hash_thread_invocations(),
        invocations_before_lowered,
        "the lowered-body compute performs ZERO slice-hash computations \
         (hash-then-lower: it re-plans and lowers only — it never \
         re-derives the slice identity)"
    );

    // The IR covers exactly the demanded member: one `b` entry, one
    // elided sibling, and no slot for `a`.
    let (entries, elided) = ir
        .exprs
        .iter()
        .find_map(|expr| match &expr.shape {
            FlowExprShape::ObjectLiteral {
                entries,
                elided_entries,
            } => Some((entries, *elided_entries)),
            FlowExprShape::Opaque { .. } => None,
        })
        .expect("the returned object lowers");
    assert_eq!(entries.len(), 1);
    assert_eq!(elided, 1);
    assert!(matches!(
        &entries[0],
        FlowObjectEntry::Property {
            key: FlowObjectKey::Named(name),
            ..
        } if name.as_ref() == "b"
    ));
    assert!(ir.slots.iter().all(|slot| slot.name.as_ref() != "a"));

    assert_eq!(rig.hash_node.entry_count(), 1);
    assert_eq!(rig.lowered_node.entry_count(), 1);
}

/// Share-vs-split check 4 (`function_flow_graph_built_once_per_function_skeleton`),
/// the discriminating fixture: two demands against the same function
/// content version — through BOTH nodes — build the skeleton + graph
/// exactly ONCE and only re-plan reachability. Registry-live guard
/// (`GuardId::FunctionFlowGraphBuiltOncePerFunctionSkeleton`).
#[test]
pub(crate) fn two_demands_one_function_flow_graph_build() {
    let function = function_key("/fixtures/my-type.ts", "myType", 1);
    let rig = rig(
        vec![(function.clone(), MYTYPE_FIXTURE)],
        FlowSliceBudget::default(),
    );
    let ctx: &dyn ResolverContext = &rig.host;

    let key_a = hash_key(function.clone(), &["a"]);
    let key_b = hash_key(function, &["b"]);
    let planned_a = planned(lookup(&rig.hash_node, key_a.clone(), ctx).expect("a"));
    let planned_b = planned(lookup(&rig.hash_node, key_b.clone(), ctx).expect("b"));
    assert_ne!(
        planned_a, planned_b,
        "distinct demands select distinct slices"
    );

    let _ir_a = lookup(
        &rig.lowered_node,
        FlowSliceLoweredKey {
            hash_key: key_a,
            slice_hash: planned_a,
        },
        ctx,
    )
    .expect("lowered a");
    let _ir_b = lookup(
        &rig.lowered_node,
        FlowSliceLoweredKey {
            hash_key: key_b,
            slice_hash: planned_b,
        },
        ctx,
    )
    .expect("lowered b");

    assert_eq!(
        rig.source.build_calls(),
        1,
        "the skeleton is produced once per function content version"
    );
    assert_eq!(
        rig.graphs.build_count(),
        1,
        "the FunctionFlowGraph is built once; every further demand re-plans only"
    );
    assert_eq!(rig.hash_node.entry_count(), 2);
    assert_eq!(rig.lowered_node.entry_count(), 2);
}

/// Budget non-admission at every layer this substrate owns: the planner
/// returns the TYPED refusal (not a panic, not a truncated plan); the
/// hash node routes it through `ReturnOnly` — returned to the caller,
/// with NO entry published on any store; and the lowered store is not
/// even addressable, because a refused plan mints no slice hash. A
/// retry recomputes cold and still publishes nothing.
#[test]
fn budget_exceeded_admits_nothing_at_any_layer() {
    let function = function_key("/fixtures/my-type.ts", "myType", 1);
    let tiny = FlowSliceBudget {
        max_return_sites: 256,
        max_selected_nodes: 1,
    };
    let rig = rig(vec![(function.clone(), MYTYPE_FIXTURE)], tiny);
    let ctx: &dyn ResolverContext = &rig.host;

    let key = hash_key(function, &["b"]);
    for _ in 0..2 {
        let outcome = lookup(&rig.hash_node, key.clone(), ctx)
            .expect("a budget refusal is RETURNED, never a silent None");
        let FlowSliceHashOutcome::BudgetExceeded(exceeded) = outcome else {
            panic!("a one-node budget cannot hold this slice");
        };
        assert_eq!(exceeded.axis, FlowSliceBudgetAxis::SelectedNodes);
        assert_eq!(exceeded.limit, 1);

        assert_eq!(
            rig.hash_node.entry_count(),
            0,
            "ReturnOnly never publishes a hash entry"
        );
        assert!(rig.hash_node.published_entry(&key).is_none());
        assert_eq!(
            rig.lowered_node.entry_count(),
            0,
            "no slice hash exists, so the lowered store is unaddressable and empty"
        );
    }
}

/// A warm hash hit serves the SAME planned value (Arc identity) with no
/// recompute and no second graph build.
#[test]
fn warm_hash_hit_reuses_planned_value_without_recompute() {
    let function = function_key("/fixtures/my-type.ts", "myType", 1);
    let rig = rig(
        vec![(function.clone(), MYTYPE_FIXTURE)],
        FlowSliceBudget::default(),
    );
    let ctx: &dyn ResolverContext = &rig.host;

    let key = hash_key(function, &["b"]);
    let first = planned(lookup(&rig.hash_node, key.clone(), ctx).expect("cold"));
    let second = planned(lookup(&rig.hash_node, key, ctx).expect("warm"));
    assert_eq!(
        first, second,
        "the warm hit serves the published slice identity, not a recompute"
    );
    assert_eq!(rig.source.build_calls(), 1);
    assert_eq!(rig.hash_node.entry_count(), 1);
}

/// Content-version keying: two keys differing only in
/// `flow_body_stable_hash` are distinct artifacts (separate graph
/// builds, separate lowered entries) even when the SLICE hash
/// coincides — a return-literal edit re-keys through the content pin
/// while the slice identity stays selection-scoped. Registry-live guard
/// (`GuardId::FlowSliceKeysOnBodySensitiveHashNotParseStableHash`): the
/// `return { b: 1 }` / `return { b: 2 }` pair is exactly the contract's
/// body-sensitive fixture, and the key field IS `flow_body_stable_hash`
/// (`FlowSliceFunctionKey` carries no `parse_stable_hash`).
#[test]
pub(crate) fn distinct_content_versions_key_distinct_artifacts() {
    let v1 = function_key("/fixtures/lit.ts", "lit", 1);
    let v2 = function_key("/fixtures/lit.ts", "lit", 2);
    let rig = rig(
        vec![
            (v1.clone(), "function lit() { return { b: 1 } }"),
            (v2.clone(), "function lit() { return { b: 2 } }"),
        ],
        FlowSliceBudget::default(),
    );
    let ctx: &dyn ResolverContext = &rig.host;

    let key_v1 = hash_key(v1, &["b"]);
    let key_v2 = hash_key(v2, &["b"]);
    let planned_v1 = planned(lookup(&rig.hash_node, key_v1.clone(), ctx).expect("v1"));
    let planned_v2 = planned(lookup(&rig.hash_node, key_v2.clone(), ctx).expect("v2"));

    // The slice SELECTION is identical across the literal edit — the
    // content difference is pinned by `flow_body_stable_hash` in the
    // KEY, exactly why the lowered key carries both.
    assert_eq!(planned_v1, planned_v2);

    let lowered_v1 = FlowSliceLoweredKey {
        hash_key: key_v1,
        slice_hash: planned_v1,
    };
    let lowered_v2 = FlowSliceLoweredKey {
        hash_key: key_v2,
        slice_hash: planned_v2,
    };
    assert_ne!(lowered_v1, lowered_v2, "content versions never collide");
    let _ = lookup(&rig.lowered_node, lowered_v1, ctx).expect("v1 IR");
    let _ = lookup(&rig.lowered_node, lowered_v2, ctx).expect("v2 IR");
    assert_eq!(rig.graphs.build_count(), 2, "one graph per content version");
    assert_eq!(rig.lowered_node.entry_count(), 2);
}

/// The published entries carry an EMPTY fact rail: these are
/// content-addressed artifacts (key identity is validity), and no slice
/// hash or selected-ID set ever enters `ReadSetSignature.facts` — slice
/// identity is never a warm-validity oracle.
#[test]
fn published_entries_carry_empty_fact_rail() {
    let function = function_key("/fixtures/my-type.ts", "myType", 1);
    let rig = rig(
        vec![(function.clone(), MYTYPE_FIXTURE)],
        FlowSliceBudget::default(),
    );
    let ctx: &dyn ResolverContext = &rig.host;

    let key = hash_key(function, &["b"]);
    let _ = lookup(&rig.hash_node, key.clone(), ctx).expect("hash");
    let entry = rig.hash_node.published_entry(&key).expect("published");
    assert!(
        entry.signature.facts.is_empty(),
        "no fact rail — and in particular no slice-identity fact — rides the signature"
    );
    assert!(entry.self_root_canonicals.is_empty());
}

/// `remove_canonical` evicts the memoized graph bundles of one
/// canonical; the next demand rebuilds once.
#[test]
fn graph_store_remove_canonical_evicts_bundles() {
    let function = function_key("/fixtures/my-type.ts", "myType", 1);
    let rig = rig(
        vec![(function.clone(), MYTYPE_FIXTURE)],
        FlowSliceBudget::default(),
    );
    let ctx: &dyn ResolverContext = &rig.host;

    let key = hash_key(function, &["b"]);
    let _ = lookup(&rig.hash_node, key.clone(), ctx).expect("cold");
    assert_eq!(rig.graphs.build_count(), 1);
    rig.graphs.remove_canonical("/fixtures/my-type.ts");
    // A different demand misses the hash store and rebuilds the bundle
    // once more.
    let key2 = hash_key(key.function.clone(), &["a"]);
    let _ = lookup(&rig.hash_node, key2, ctx).expect("recold");
    assert_eq!(rig.graphs.build_count(), 2);
}

/// Share-vs-split check 5 through the PRODUCTION storage: the
/// `ReturnType<typeof myType>['b']`-class member-slice demand, driven
/// through the `ProjectTypeStore`-homed nodes over the production
/// retained-snapshot skeleton source against a REAL host file,
/// materializes NO sibling (`a` is elided from the lowered slice; no
/// slot for `a`; `Mytype` appears nowhere in the slice IR) and records
/// NO `Mytype` fact (the fact tracer bracketing the whole
/// hash-then-lower chain observes no fact naming `Mytype`, and the
/// published artifacts carry EMPTY fact rails).
#[test]
fn mytype_member_slice_via_production_store_materializes_no_sibling_and_no_mytype_fact() {
    use crate::types::UpsertRequest;
    let host = VerterHost::new_standalone(HostConfig::default());
    let canonical = "/ws/mytype.ts";
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(
            "export class Mytype { tag = 1 }\nexport function myType() { const a = new Mytype(); const b = 1; return { a, b }; }\n",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
    let ctx: &dyn ResolverContext = &host;
    let serve = ctx
        .ensure_indexed_ready_serve(canonical)
        .expect("the fixture file is served");
    let index = serve
        .indexed
        .shallow_state
        .decl_bodies()
        .function_program_index();
    let entry = index
        .entries
        .iter()
        .find(|entry| entry.key.declaration.name.as_ref() == "myType")
        .expect("myType is a served function position");
    let env = host.host_view_env_hashes_for(canonical);
    let key = FlowSliceHashKey {
        function: FlowSliceFunctionKey {
            canonical_id: Arc::from(canonical),
            function: entry.key.clone(),
            flow_body_stable_hash: entry.flow_body_stable_hash,
            flow_body_exact_hash: entry
                .flow_body_exact_hash
                .expect("a served function position addresses its own bytes"),
            parse_env_hash: env.parse_env_hash,
            parser_version: crate::file_artifact_store::CURRENT_PARSER_VERSION,
        },
        demand: FlowSliceDemandIdentity {
            projection_path: Arc::from(vec![Arc::<str>::from("b")].into_boxed_slice()),
        },
    };
    let stores = ctx.project_type_store().flow_slice();

    // Bracket the WHOLE hash-then-lower chain with a REAL fact tracer:
    // the slice path must observe no fact naming `Mytype` (no
    // class-surface, `TypeOf`, constructor, import, or route fact for
    // the sibling's type).
    let (ir, finalise) = crate::fact_signature_helpers::install_fact_tracer(&host, || {
        let outcome = lookup(stores.hash_node(), key.clone(), ctx).expect("hash lookup");
        let slice_hash = planned(outcome);
        lookup(
            stores.lowered_node(),
            FlowSliceLoweredKey {
                hash_key: key.clone(),
                slice_hash,
            },
            ctx,
        )
        .expect("lowered lookup")
    });
    let observed: Vec<String> = match &finalise {
        crate::resolver_core::FactReadSetFinalise::Ok(facts)
        | crate::resolver_core::FactReadSetFinalise::NonCacheable(facts) => {
            facts.iter().map(|fact| format!("{fact:?}")).collect()
        }
        crate::resolver_core::FactReadSetFinalise::Overflow => {
            panic!("the slice chain must not overflow the fact tracer")
        }
    };
    assert!(
        observed.iter().all(|fact| !fact.contains("Mytype")),
        "no fact names the elided sibling's type: {observed:?}"
    );

    // Sibling non-materialization: the lowered slice carries ONLY the
    // demanded `b` entry; `a` is elided (counted, never lowered), no
    // slot exists for `a`, and `Mytype` appears nowhere in the IR.
    let (entries, elided) = ir
        .exprs
        .iter()
        .find_map(|expr| match &expr.shape {
            FlowExprShape::ObjectLiteral {
                entries,
                elided_entries,
            } => Some((entries, *elided_entries)),
            FlowExprShape::Opaque { .. } => None,
        })
        .expect("the returned object lowers");
    assert_eq!(entries.len(), 1, "only the demanded member is lowered");
    assert_eq!(elided, 1, "the sibling is counted, never lowered");
    assert!(matches!(
        &entries[0],
        FlowObjectEntry::Property {
            key: FlowObjectKey::Named(name),
            ..
        } if name.as_ref() == "b"
    ));
    assert!(ir.slots.iter().all(|slot| slot.name.as_ref() != "a"));
    assert!(
        !format!("{ir:?}").contains("Mytype"),
        "the sibling's type never enters the lowered slice"
    );

    // The production source built the skeleton from the retained
    // snapshot exactly once, and the published artifacts carry EMPTY
    // fact rails (key identity is validity — no slice identity in any
    // signature).
    assert_eq!(stores.graphs().build_count(), 1);
    let published = stores
        .hash_node()
        .published_entry(&key)
        .expect("the hash artifact published");
    assert!(published.signature.facts.is_empty());
}

// ---------------------------------------------------------------------------
// Registry-live substrate guards (U6.FLOW_RETURN_SUBSTRATE required guards,
// bound through `LIB_LIVE_GUARD_BINDINGS`)
// ---------------------------------------------------------------------------

/// Registry-live guard (`GuardId::FlowSliceIsGraphReachabilityNotProceduralWalk`).
///
/// The demand slice is graph REACHABILITY over the `FunctionFlowGraph`,
/// never a procedural statement / mini-CFG walk. Two rails:
///
/// - STRUCTURAL: the skeleton is DROPPED before planning — the planner
///   type holds only `&FunctionFlowGraph` (`ReturnPathPeeker::new(graph)`),
///   so a statement-list re-walk is unrepresentable at compile time (the
///   statements live on the dropped skeleton).
/// - BEHAVIORAL: a statement PROCEDURALLY BETWEEN the demanded
///   contributors (`dead`, declared between `b`'s definition and the
///   return) is graph-disconnected from the `['b']` demand and stays
///   unselected by EITHER frontier, while the same graph serves an
///   EXPRESSION-SITE origin — the planner is origin-general reachability,
///   not a return-statement walker.
#[test]
pub(crate) fn flow_slice_is_graph_reachability_not_procedural_walk() {
    use verter_semantic::analysis::flow::peeker::{ReturnPathPeeker, SliceDemand};

    let skeleton = skeleton_of(
        "function f(u: number) { const b = 1; const dead = mystery(u); return { a: dead, b } }",
    );
    let graph = verter_semantic::analysis::flow::flow_graph::build_function_flow_graph(&skeleton);

    // Resolve every asserted node id BEFORE the skeleton is dropped.
    let object_site = skeleton.return_sites[0].argument.expect("return argument");
    let object_node = graph.expr_site_node(object_site);
    let b_key = skeleton.name_id("b").expect("b interned");
    let a_key = skeleton.name_id("a").expect("a interned");
    let entry_node = |key| {
        graph
            .out_edges(object_node)
            .iter()
            .find_map(|edge| match &edge.kind {
                verter_semantic::analysis::flow::flow_graph::FlowEdgeKind::PathWrite {
                    path,
                    ..
                } if path.as_ref()
                    == [verter_semantic::analysis::flow::SkeletonPathSegment::Static(key)] =>
                {
                    Some(edge.to)
                }
                _ => None,
            })
            .expect("object literal provisions the key")
    };
    let a_entry = entry_node(a_key);
    let b_entry = entry_node(b_key);
    let binding_node = |name: &str| {
        let id = skeleton.name_id(name).expect("name interned");
        let binding = skeleton.bindings_named(id).next().expect("bound");
        graph.binding_node(binding)
    };
    let dead_hub = binding_node("dead");
    let b_hub = binding_node("b");
    let dead_binding = skeleton
        .bindings_named(skeleton.name_id("dead").expect("dead"))
        .next()
        .expect("dead bound");
    let dead_init = skeleton.binding(dead_binding).initializer.expect("init");
    let dead_init_node = graph.expr_site_node(dead_init);
    let b_binding = skeleton.bindings_named(b_key).next().expect("b bound");
    let b_init = skeleton.binding(b_binding).initializer.expect("b init");

    let return_demand = SliceDemand::for_return_projection(&skeleton, &[Arc::<str>::from("b")]);
    let expr_demand = SliceDemand::for_expression_site(&skeleton, b_init, &[]);

    // The structural rail: NOTHING below can consult the statement list.
    drop(skeleton);

    let planner = ReturnPathPeeker::new(&graph);
    let plan = planner
        .plan(&return_demand, &FlowSliceBudget::default())
        .expect("plan within default budget");
    assert!(plan.is_value(b_entry), "b's entry value is selected");
    assert!(plan.is_value(b_hub), "b's binding hub is selected");
    assert!(
        !plan.is_selected(dead_hub),
        "the procedurally-interleaved `dead` binding is graph-disconnected \
         from the ['b'] demand — a statement walker would have visited it"
    );
    assert!(!plan.is_selected(dead_init_node), "`dead`'s call stays out");
    assert!(
        !plan.is_selected(a_entry),
        "the sibling entry (a plain binding read — no eval effect) is \
         reached by NEITHER frontier"
    );

    // Expression-site origin over the SAME graph: reachability from b's
    // initializer selects that site and still never reaches `dead`.
    let expr_plan = planner
        .plan(&expr_demand, &FlowSliceBudget::default())
        .expect("expression-site plan");
    assert!(expr_plan.is_value(graph.expr_site_node(b_init)));
    assert!(!expr_plan.is_selected(dead_hub));
}

/// Registry-live guard (`GuardId::FlowGraphEffectEdgesStayLivePastValueWrites`)
/// — the block contract's discriminating fixture: demanding `['b']` of
/// `return { a: (x = "s"), b: x.toUpperCase() }` reaches `a`'s
/// eval-effect edge (the assignment mutates `x`, which `b` reads) but
/// never materializes `a`'s VALUE; value-provider edges stop at the
/// definite write while the effect family stays live past it.
#[test]
pub(crate) fn flow_graph_effect_edges_stay_live_past_value_writes() {
    use verter_semantic::analysis::flow::peeker::{ReturnPathPeeker, SliceDemand};

    let skeleton =
        skeleton_of(r#"function f(x: string) { return { a: (x = "s"), b: x.toUpperCase() } }"#);
    let graph = verter_semantic::analysis::flow::flow_graph::build_function_flow_graph(&skeleton);
    let demand = SliceDemand::for_return_projection(&skeleton, &[Arc::<str>::from("b")]);
    let plan = ReturnPathPeeker::new(&graph)
        .plan(&demand, &FlowSliceBudget::default())
        .expect("plan within default budget");

    let object_site = skeleton.return_sites[0].argument.expect("return argument");
    let object_node = graph.expr_site_node(object_site);
    let entry_node = |name: &str| {
        let key = skeleton.name_id(name).expect("key interned");
        graph
            .out_edges(object_node)
            .iter()
            .find_map(|edge| match &edge.kind {
                verter_semantic::analysis::flow::flow_graph::FlowEdgeKind::PathWrite {
                    path,
                    ..
                } if path.as_ref()
                    == [verter_semantic::analysis::flow::SkeletonPathSegment::Static(key)] =>
                {
                    Some(edge.to)
                }
                _ => None,
            })
            .expect("object literal provisions the key")
    };
    let a_entry = entry_node("a");
    let b_entry = entry_node("b");
    let x_id = skeleton.name_id("x").expect("x interned");
    let x_hub = graph.binding_node(skeleton.bindings_named(x_id).next().expect("x bound"));

    assert!(plan.is_value(b_entry), "b's value is demanded");
    assert!(
        plan.is_effect_only(a_entry),
        "a's eval-effect edge stays LIVE past the definite value write — \
         the two-frontier soundness as a typed-edge invariant"
    );
    assert!(!plan.is_value(a_entry), "a's VALUE is never materialized");
    assert!(plan.is_value(x_hub), "x is read by the selected path");
    // The sibling write's RHS is x's reaching definition — value-selected.
    let rhs_site = skeleton.writes[0].value.expect("assignment value site");
    assert!(plan.is_value(graph.expr_site_node(rhs_site)));
}

/// Registry-live guard
/// (`GuardId::FlowGraphBuildIsShallowInternedNoLoweringLazyRegions`) — the
/// PART 1 §6.2 perf-hardening build-path pin: skeleton + graph construction
/// for a large body lowers NO type and produces NO resolution dispatch,
/// route lookup, or imported-fact observation. Rails:
///
/// - STRUCTURAL: `build_function_body_skeleton(&FunctionBodySource)` and
///   `build_function_flow_graph(&FunctionBodySkeleton)` take no resolver,
///   no store view, and no dispatch handle — resolution is unreachable
///   from the build path at compile time; and both artifacts are
///   `NoTypeExpr` (asserted below), so no lowered type can be STORED.
/// - BEHAVIORAL: a REAL host fact tracer brackets the whole build over a
///   type-heavy body and observes ZERO facts.
///
/// Build timing / region-materialization strategy is deliberately NOT
/// constrained here (eager and lazy are both conforming); the
/// per-query-rebuild rejection is `two_demands_one_function_flow_graph_build`.
#[test]
pub(crate) fn flow_graph_build_is_shallow_interned_no_lowering_lazy_regions() {
    fn assert_arena_free_no_type_expr<T: Send + Sync + 'static + verter_no_typeexpr::NoTypeExpr>() {
    }
    assert_arena_free_no_type_expr::<FunctionBodySkeleton>();
    assert_arena_free_no_type_expr::<verter_semantic::analysis::flow::flow_graph::FunctionFlowGraph>(
    );

    let host = VerterHost::new_standalone(HostConfig::default());
    let source = r#"function big(input: Widget, flags: Map<string, Set<number>>) {
        const a: Widget = makeWidget(input);
        const b = { deep: { deeper: { deepest: [1, 2, 3] } } };
        const c: Promise<Array<Record<string, Widget>>> = load(a, flags);
        const d = c;
        const e = { a, b, d };
        return { big: e, tiny: 1 };
    }"#;
    let ((), finalise) = crate::fact_signature_helpers::install_fact_tracer(&host, || {
        let skeleton = skeleton_of(source);
        let graph =
            verter_semantic::analysis::flow::flow_graph::build_function_flow_graph(&skeleton);
        assert!(graph.node_count() > 0, "the build produced a real graph");
    });
    match finalise {
        crate::resolver_core::FactReadSetFinalise::Ok(facts)
        | crate::resolver_core::FactReadSetFinalise::NonCacheable(facts) => {
            assert!(
                facts.is_empty(),
                "skeleton + graph construction must observe ZERO facts \
                 (no resolution dispatch, no route lookup, no imported-fact \
                 production), got {facts:?}"
            );
        }
        crate::resolver_core::FactReadSetFinalise::Overflow => {
            panic!("the build path must not overflow the fact tracer")
        }
    }
}

/// Registry-live guard (`GuardId::FlowSliceIrDetachesFromOxcArena`) —
/// `FlowSliceIR` (and every carrier on the hash-then-lower chain) is
/// `Send + Sync + 'static` and `NoTypeExpr`: no transitive
/// `&'arena T` / `oxc_allocator::Box<'arena, T>` field survives into the
/// host-owned stores. The runtime half is inherent to the fixture rig:
/// `skeleton_of`'s OXC allocator drops at the end of that helper, and the
/// IR served below is read AFTER that arena is gone.
#[test]
pub(crate) fn flow_slice_ir_detaches_from_oxc_arena() {
    fn assert_detached<T: Send + Sync + 'static + verter_no_typeexpr::NoTypeExpr>() {}
    assert_detached::<verter_semantic::analysis::flow::flow_ir::FlowSliceIR>();
    assert_detached::<verter_semantic::analysis::flow::flow_ir::ReturnSlicePlan>();
    assert_detached::<verter_semantic::analysis::flow::hashing::FlowSliceHash>();

    let function = function_key("/fixtures/my-type.ts", "myType", 1);
    let rig = rig(
        vec![(function.clone(), MYTYPE_FIXTURE)],
        FlowSliceBudget::default(),
    );
    let ctx: &dyn ResolverContext = &rig.host;
    let key = hash_key(function, &["b"]);
    let slice_hash = planned(lookup(&rig.hash_node, key.clone(), ctx).expect("hash"));
    let ir = lookup(
        &rig.lowered_node,
        FlowSliceLoweredKey {
            hash_key: key,
            slice_hash,
        },
        ctx,
    )
    .expect("lowered IR");
    // The parse arena that produced this IR dropped inside `skeleton_of`;
    // reading the IR here is the runtime detach witness.
    assert!(!ir.exprs.is_empty(), "the detached IR carries owned data");
}

// ---------------------------------------------------------------------------
// Content-addressing: a span-bearing artifact under a span-free key
// ---------------------------------------------------------------------------

/// Two functions, so an edit that moves the file can leave one of them
/// structurally untouched while shifting its absolute source positions.
const SHIFT_FIXTURE: &str = r#"export function shiftedFirst() {
  const aa = 1;
  return { m: aa };
}

export function shiftedSecond() {
  const bb = "s";
  return { m: bb };
}
"#;

/// The SAME file with a leading blank line: no function's own text
/// changes, every function's absolute source positions shift by one.
fn shifted_source() -> String {
    format!("\n{SHIFT_FIXTURE}")
}

/// The SAME file with `shiftedSecond`'s local ALPHA-RENAMED to a
/// different-LENGTH name. `flow_body_stable_hash` alpha-normalizes
/// binding/reference identifiers, so `shiftedSecond`'s structural hash
/// is UNCHANGED — while every source position inside its body shifts,
/// including positions measured RELATIVE to the function's own start.
fn alpha_renamed_source() -> String {
    SHIFT_FIXTURE.replace("bb", "bbbbbb")
}

/// Evaluate one function's whole flow return through the production
/// dispatch, as a projected type.
fn flow_return_of(
    host: &Arc<VerterHost>,
    canonical: &str,
    name: &str,
    stage: &str,
) -> verter_type_expr::TypeExpr {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(&host_ctx);
    let key = crate::semantic_query::FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(canonical),
            TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(canonical),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
    };
    match dispatch.execute(crate::semantic_query::SemanticQueryKey::FlowReturn(
        Box::new(key),
    )) {
        crate::semantic_query::QueryResult::Value(crate::semantic_query::SemanticQueryOutput {
            value: crate::semantic_query::SemanticQueryValue::FlowReturn(result),
            ..
        }) => host
            .project_node_to_type_expr_for_test(result.return_type)
            .expect("a flow return value projects"),
        other => panic!("[{stage}] {name} must produce a flow return value, got {other:?}"),
    }
}

fn upsert_source(host: &Arc<VerterHost>, canonical: &str, source: &str) {
    use crate::types::UpsertRequest;
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

/// The expected whole-return shape of `shiftedSecond`: `{ m: string }`.
#[track_caller]
fn assert_second_is_string(ty: &verter_type_expr::TypeExpr, label: &str) {
    let verter_type_expr::TypeExpr::Object(object) = ty else {
        panic!("{label}: expected an object return, got {ty:?}");
    };
    let member = object
        .properties
        .iter()
        .find_map(|member| match member {
            verter_type_expr::ObjectMember::Property(property)
                if property.key.as_string() == Some("m") =>
            {
                Some(property)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{label}: member `m` must be present in {ty:?}"));
    assert_eq!(
        member.ty,
        verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
        "{label}: `m` reads a widening-literal local, so it publishes `string`"
    );
}

/// The per-function flow bundle is CONTENT-addressed, so a reuse of it
/// must be sound for every content the key admits.
///
/// The bundle (`FunctionBodySkeleton` + `FunctionFlowGraph`) is the
/// substrate's source-position authority: the content lowering matches
/// live OXC spans against the plan's selected spans to decide what
/// lowers. `FlowSliceFunctionKey`'s only content axis is
/// `flow_body_stable_hash`, which is an AST fold — it is blind to
/// absolute file position (an edit ABOVE the function) AND, because it
/// alpha-normalizes binding/reference identifiers, blind to a local
/// rename that shifts every position INSIDE the function. So one key
/// admits contents whose positions differ, and reuse hands the plan
/// positions that no longer address the code they were computed from.
///
/// Both directions are exercised, because they need DIFFERENT halves of
/// the fix and either one alone leaves the other broken:
///
/// - a leading blank line moves the whole file; the function's own bytes
///   are untouched, so it is not distinguishable by any per-function
///   content hash — only positions stored RELATIVE to the function's own
///   anchor survive it;
/// - an alpha-rename to a different-LENGTH name shifts positions INSIDE
///   one function while leaving its structural hash identical — relative
///   positions do NOT survive it, only an EXACT per-function content
///   axis in the key does.
///
/// Discrimination is against a COLD host built directly on each edited
/// content: the cold answer is what the substrate computes with no
/// reuse at all, so an equal warm answer is reuse that was sound and an
/// unequal one is reuse that was not.
///
/// Mutation recipes:
///
/// - dropping the relative-anchor rebase (storing absolute skeleton
///   spans again) makes the LEADING-BLANK-LINE arm fail: the shifted
///   file reuses `shiftedSecond`'s stale bundle and the demand selects
///   nothing;
/// - dropping `flow_body_exact_hash` from `FlowSliceFunctionKey` makes
///   the ALPHA-RENAME arm fail the same way, and leaves the first arm
///   green.
#[test]
fn flow_bundle_reuse_is_sound_for_every_content_its_key_admits() {
    let canonical = "/ws/flow-shift.ts";

    // ── Arm 1: an edit ABOVE the function (whole-file shift) ──────────
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_source(&host, canonical, SHIFT_FIXTURE);
    assert_second_is_string(
        &flow_return_of(&host, canonical, "shiftedSecond", "base"),
        "base",
    );

    upsert_source(&host, canonical, &shifted_source());
    let warm_shifted = flow_return_of(&host, canonical, "shiftedSecond", "warm shifted");

    let cold = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_source(&cold, canonical, &shifted_source());
    let cold_shifted = flow_return_of(&cold, canonical, "shiftedSecond", "cold shifted");
    assert_second_is_string(&cold_shifted, "cold shifted");
    assert_eq!(
        warm_shifted, cold_shifted,
        "a leading blank line changes no function's own text, so every function's \
         bundle must still serve — and must serve the SAME answer a cold host \
         computes on exactly this content"
    );

    // ── Arm 2: an ALPHA-RENAME inside the function ────────────────────
    let renamed_host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_source(&renamed_host, canonical, SHIFT_FIXTURE);
    assert_second_is_string(
        &flow_return_of(
            &renamed_host,
            canonical,
            "shiftedSecond",
            "base (rename arm)",
        ),
        "base (rename arm)",
    );

    upsert_source(&renamed_host, canonical, &alpha_renamed_source());
    let warm_renamed = flow_return_of(&renamed_host, canonical, "shiftedSecond", "warm renamed");

    let cold_renamed_host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert_source(&cold_renamed_host, canonical, &alpha_renamed_source());
    let cold_renamed = flow_return_of(
        &cold_renamed_host,
        canonical,
        "shiftedSecond",
        "cold renamed",
    );
    assert_second_is_string(&cold_renamed, "cold renamed");
    assert_eq!(
        warm_renamed, cold_renamed,
        "an alpha-rename keeps `flow_body_stable_hash` identical while shifting \
         every position inside the body, so the key must carry an EXACT content \
         axis — relative positions alone do not survive it"
    );
}

/// The fixture invariant the test above depends on: the two edits are
/// exactly the two blind spots claimed, and neither is distinguishable
/// by `flow_body_stable_hash`.
#[test]
fn the_two_shift_edits_are_invisible_to_flow_body_stable_hash() {
    let canonical = "/ws/flow-shift-invariant.ts";
    let hash_of = |source: &str| {
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        upsert_source(&host, canonical, source);
        let ctx: &dyn ResolverContext = host.as_ref();
        let serve = ctx
            .ensure_indexed_ready_serve(canonical)
            .expect("the fixture file is served");
        let index = serve
            .indexed
            .shallow_state
            .decl_bodies()
            .function_program_index();
        index
            .entries
            .iter()
            .find(|entry| entry.key.declaration.name.as_ref() == "shiftedSecond")
            .expect("shiftedSecond is a served function position")
            .flow_body_stable_hash
    };
    let base = hash_of(SHIFT_FIXTURE);
    assert_eq!(
        base,
        hash_of(&shifted_source()),
        "a leading blank line must not change the structural body hash"
    );
    assert_eq!(
        base,
        hash_of(&alpha_renamed_source()),
        "an alpha-rename must not change the structural body hash — this is what \
         makes relative positions insufficient on their own"
    );
}
