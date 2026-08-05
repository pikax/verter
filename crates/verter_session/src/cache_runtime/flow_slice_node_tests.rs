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
/// exactly the selected slice.
#[test]
fn hash_then_lower_round_trip_serves_lowered_slice_ir() {
    let function = function_key("/fixtures/my-type.ts", "myType", 1);
    let rig = rig(
        vec![(function.clone(), MYTYPE_FIXTURE)],
        FlowSliceBudget::default(),
    );
    let ctx: &dyn ResolverContext = &rig.host;

    let key = hash_key(function, &["b"]);
    let outcome = lookup(&rig.hash_node, key.clone(), ctx).expect("hash lookup");
    let planned = planned(outcome);

    let lowered_key = FlowSliceLoweredKey {
        hash_key: key.clone(),
        slice_hash: planned,
    };
    let ir = lookup(&rig.lowered_node, lowered_key, ctx).expect("lowered lookup");

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
/// exactly ONCE and only re-plan reachability.
#[test]
fn two_demands_one_function_flow_graph_build() {
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
/// while the slice identity stays selection-scoped.
#[test]
fn distinct_content_versions_key_distinct_artifacts() {
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
