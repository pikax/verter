use std::process::Command;
use std::sync::Arc;

use super::locator_view::{LocatorViewInputs, ViewMemo};
use super::walk::ShallowDiagnostic;
use super::ProjectSemanticDispatch;
use crate::request_context::{current_cold_compute_completeness, ColdComputeCompletenessScope};
use crate::resolver_core::scope_shadowing::ScopeShadowing;
use crate::semantic_query::{
    InstantiateContext, InstantiateKey, NodeScopeId, PartialReasonSet, PrimitiveKind,
    ProjectionMode, ProjectionReductionContext, QueryResult, ResultCompleteness, SemanticNodeData,
    SemanticQueryKey,
};
use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodyPathStep, TypeBodySlot,
};

const CHILD_MARKER: &str = "VERTER_PROJECTION_STACK_CHILD";
const AUTHORED_DEPTH: usize = 200;
const STRUCTURAL_VALIDATION_DEPTH: usize = 4_096;
const CHILD_STACK_BYTES: usize = 2 * 1024 * 1024;
const SHAPE_SETUP_STACK_BYTES: usize = 32 * 1024 * 1024;

fn nested_singleton_tuple_source(depth: usize) -> String {
    let mut source = String::with_capacity(depth * 2 + 32);
    source.push_str("export type Deep = ");
    source.extend(std::iter::repeat_n('[', depth));
    source.push_str("string");
    source.extend(std::iter::repeat_n(']', depth));
    source.push_str(";\n");
    source
}

fn nested_closed_conditional_source(depth: usize) -> String {
    let mut source = String::with_capacity(depth * 40 + 32);
    source.push_str("export type Deep = ");
    // bounded-loop: authored finite fixture depth supplied by the test.
    for _ in 0..depth {
        source.push_str("string extends string ? (");
    }
    source.push_str("string");
    // bounded-loop: closes the same authored finite fixture depth.
    for _ in 0..depth {
        source.push_str(") : never");
    }
    source.push_str(";\n");
    source
}

fn build_host(source: String) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _update = host
        .upsert(UpsertRequest {
            canonical_id: Some("/deep.ts".to_string()),
            input_id: "/deep.ts".to_string(),
            source: Arc::from(source),
            file_language: crate::LanguageRegistry::global()
                .classify_static("/deep.ts")
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("deep fixture must parse and index on the parent test stack");
    host.shallow_file_state("/deep.ts")
        .expect("deep fixture must have shallow state before entering the 2 MiB worker");
    let setup_host = Arc::clone(&host);
    std::thread::Builder::new()
        .name("projection-shape-setup".to_string())
        .stack_size(SHAPE_SETUP_STACK_BYTES)
        .spawn(move || {
            let dispatch = ProjectSemanticDispatch::new(setup_host.as_ref());
            let locator = AuthoredBodyLocator::DeclBody(TypeBodySlot {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::from("/deep.ts"),
                    symbol: Arc::from("Deep"),
                    space: LocatorSymbolSpace::Type,
                },
                path: Arc::from(Vec::<TypeBodyPathStep>::new().into_boxed_slice()),
            });
            assert!(
                matches!(dispatch.lower_locator(locator), QueryResult::Value(_)),
                "role-free deep shape must prewarm before the controlled-stack projection"
            );
        })
        .expect("deep-shape setup worker must spawn")
        .join()
        .expect("deep-shape setup must complete on its explicit setup stack");
    host
}

fn run_deep_tuple_projection_child() {
    let host = build_host(nested_singleton_tuple_source(AUTHORED_DEPTH));
    let worker_host = Arc::clone(&host);
    let (result, diagnostics, completeness) = std::thread::Builder::new()
        .name("projection-stack-2mib".to_string())
        .stack_size(CHILD_STACK_BYTES)
        .spawn(move || {
            let _scope = ColdComputeCompletenessScope::enter();
            let dispatch = ProjectSemanticDispatch::new(worker_host.as_ref());
            let read = dispatch.execute_read(SemanticQueryKey::Instantiate(InstantiateKey::new(
                crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
                    Arc::from("/deep.ts"),
                    Arc::from("Deep"),
                ),
                Arc::from(Vec::new().into_boxed_slice()),
                InstantiateContext::non_file(
                    ProjectionReductionContext::published(ProjectionMode::Expanded),
                    Default::default(),
                    super::BodySourceWitness::mint_for_unit_tests(),
                ),
            )));
            (
                read.value,
                read.walker_diagnostics,
                current_cold_compute_completeness(),
            )
        })
        .expect("2 MiB projection worker must spawn")
        .join()
        .expect("2 MiB projection worker must return without panic or stack overflow");

    assert_eq!(
        completeness,
        ResultCompleteness::Complete,
        "a finite 200-deep authored structural body must remain Complete"
    );
    assert!(
        diagnostics.is_empty(),
        "a finite deep type must not receive an operational diagnostic: {diagnostics:?}"
    );
    let QueryResult::Value(value) = result else {
        panic!("deep authored tuple must resolve to a value, got {result:?}");
    };

    let graph = host.project_type_store().semantic_graph();
    let mut cursor = value;
    for level in 0..AUTHORED_DEPTH {
        let data = graph
            .node_data(cursor)
            .unwrap_or_else(|| panic!("missing graph node at tuple level {level}"));
        let SemanticNodeData::Tuple { elements, .. } = data.as_ref() else {
            panic!("expected singleton tuple at level {level}, got {data:?}");
        };
        assert_eq!(elements.len(), 1, "tuple level {level} must stay singleton");
        cursor = elements[0].value;
    }
    assert!(
        matches!(
            graph.node_data(cursor).as_deref(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::String))
        ),
        "the 200-deep tuple leaf must remain string"
    );
}

fn run_deep_conditional_projection_child() {
    let host = build_host(nested_closed_conditional_source(AUTHORED_DEPTH));
    let worker_host = Arc::clone(&host);
    let (result, diagnostics, completeness) = std::thread::Builder::new()
        .name("projection-conditional-stack-2mib".to_string())
        .stack_size(CHILD_STACK_BYTES)
        .spawn(move || {
            let _scope = ColdComputeCompletenessScope::enter();
            let dispatch = ProjectSemanticDispatch::new(worker_host.as_ref());
            let read = dispatch.execute_read(SemanticQueryKey::Instantiate(InstantiateKey::new(
                crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
                    Arc::from("/deep.ts"),
                    Arc::from("Deep"),
                ),
                Arc::from(Vec::new().into_boxed_slice()),
                InstantiateContext::non_file(
                    ProjectionReductionContext::published(ProjectionMode::Expanded),
                    Default::default(),
                    super::BodySourceWitness::mint_for_unit_tests(),
                ),
            )));
            (
                read.value,
                read.walker_diagnostics,
                current_cold_compute_completeness(),
            )
        })
        .expect("2 MiB conditional projection worker must spawn")
        .join()
        .expect("2 MiB conditional projection worker must return without stack overflow");

    assert_eq!(
        completeness,
        ResultCompleteness::Complete,
        "a finite 200-deep closed conditional body must remain Complete"
    );
    assert!(
        diagnostics.is_empty(),
        "a finite deep conditional must not receive an operational diagnostic: {diagnostics:?}"
    );
    let QueryResult::Value(value) = result else {
        panic!("deep closed conditional must resolve to a value, got {result:?}");
    };
    assert!(
        matches!(
            host.project_type_store()
                .semantic_graph()
                .node_data(value)
                .as_deref(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::String))
        ),
        "200 nested true conditionals must reduce exactly to string"
    );
}

fn run_deep_structural_validation_child() {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let graph = host.project_type_store().semantic_graph();
    let leaf = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let mut root = leaf;
    // bounded-loop: fixed finite structural-validation fixture depth.
    for _ in 0..STRUCTURAL_VALIDATION_DEPTH {
        root = graph.intern_node(SemanticNodeData::Intersection(Arc::from(
            vec![leaf, root].into_boxed_slice(),
        )));
    }

    let worker_host = Arc::clone(&host);
    let validated = std::thread::Builder::new()
        .name("structural-validation-stack-2mib".to_string())
        .stack_size(CHILD_STACK_BYTES)
        .spawn(move || {
            let _scope = ColdComputeCompletenessScope::enter();
            ProjectSemanticDispatch::new(worker_host.as_ref()).demand_validated_structural_node(
                root,
                ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
            )
        })
        .expect("2 MiB structural-validation worker must spawn")
        .join()
        .expect("2 MiB structural-validation worker must return without stack overflow");

    assert!(
        validated.is_some(),
        "a finite deeply nested composite must validate without an operational partial"
    );
}

#[test]
fn deep_finite_structural_validation_completes_on_2_mib_stack() {
    if std::env::var(CHILD_MARKER).as_deref() == Ok("structural-validation") {
        run_deep_structural_validation_child();
        return;
    }

    let exe = std::env::current_exe().expect("current unit-test executable");
    let status = Command::new(exe)
        .arg("--exact")
        .arg(
            "project_semantic_dispatch::projection_stack_safety_tests::deep_finite_structural_validation_completes_on_2_mib_stack",
        )
        .arg("--nocapture")
        .env(CHILD_MARKER, "structural-validation")
        .env_remove("RUST_MIN_STACK")
        .status()
        .expect("spawn isolated structural-validation child");

    assert!(
        status.success(),
        "the isolated 2 MiB structural-validation child must exit cleanly; status={status}"
    );
}

#[test]
fn authored_200_deep_tuple_projects_complete_on_2_mib_stack() {
    if std::env::var(CHILD_MARKER).as_deref() == Ok("tuple") {
        run_deep_tuple_projection_child();
        return;
    }

    let exe = std::env::current_exe().expect("current unit-test executable");
    let status = Command::new(exe)
        .arg("--exact")
        .arg(
            "project_semantic_dispatch::projection_stack_safety_tests::authored_200_deep_tuple_projects_complete_on_2_mib_stack",
        )
        .arg("--nocapture")
        .env(CHILD_MARKER, "tuple")
        .env_remove("RUST_MIN_STACK")
        .status()
        .expect("spawn isolated projection-stack child");

    assert!(
        status.success(),
        "the isolated 2 MiB projection child must exit cleanly; status={status}"
    );
}

#[test]
fn authored_200_deep_closed_conditionals_project_complete_on_2_mib_stack() {
    if std::env::var(CHILD_MARKER).as_deref() == Ok("conditional") {
        run_deep_conditional_projection_child();
        return;
    }

    let exe = std::env::current_exe().expect("current unit-test executable");
    let status = Command::new(exe)
        .arg("--exact")
        .arg(
            "project_semantic_dispatch::projection_stack_safety_tests::authored_200_deep_closed_conditionals_project_complete_on_2_mib_stack",
        )
        .arg("--nocapture")
        .env(CHILD_MARKER, "conditional")
        .env_remove("RUST_MIN_STACK")
        .status()
        .expect("spawn isolated conditional projection-stack child");

    assert!(
        status.success(),
        "the isolated 2 MiB conditional projection child must exit cleanly; status={status}"
    );
}

fn projection_inputs<'a>(
    env: &'a rustc_hash::FxHashMap<String, crate::semantic_query::SemanticNodeId>,
    scope: &'a NodeScopeId,
    names: &'a rustc_hash::FxHashMap<
        std::sync::Arc<str>,
        verter_semantic::analysis::type_solver::host::ResolvedRootIdentity,
    >,
    shadowing: &'a ScopeShadowing,
) -> LocatorViewInputs<'a> {
    LocatorViewInputs {
        env,
        scope,
        name_resolution: names,
        scope_payload: None,
        shadowing,
    }
}

#[test]
fn projection_work_limit_allows_last_step_and_refuses_the_next_without_memoizing_root() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let leaf = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let mut root = leaf;
    const ARRAY_DEPTH: usize = 4;
    // bounded-loop: fixed finite work-boundary fixture depth.
    for _ in 0..ARRAY_DEPTH {
        root = graph.intern_node(SemanticNodeData::Array {
            element: root,
            readonly: false,
        });
    }
    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let env = rustc_hash::FxHashMap::default();
    let names = rustc_hash::FxHashMap::default();
    let scope = NodeScopeId::Global;
    let shadowing = ScopeShadowing::empty();
    let inputs = projection_inputs(&env, &scope, &names, &shadowing);
    let mut substitutions = Vec::new();

    // One charge for the root plus one for each of its four descendants.
    let exact_limit = ARRAY_DEPTH + 1;
    dispatch.set_connected_limits_for_tests(exact_limit, 24);
    let mut exact_memo = ViewMemo::default();
    let exact = dispatch.project_view_node_worklist(
        root,
        context,
        &inputs,
        &mut substitutions,
        &mut exact_memo,
    );
    assert_eq!(exact.completeness, ResultCompleteness::Complete);
    assert_eq!(exact.node, root, "unchanged arrays must preserve identity");
    assert_eq!(exact_memo.get(&(root, context)), Some(&root));

    dispatch.set_connected_limits_for_tests(exact_limit - 1, 24);
    let mut over_memo = ViewMemo::default();
    let over = dispatch.project_view_node_worklist(
        root,
        context,
        &inputs,
        &mut substitutions,
        &mut over_memo,
    );
    let ResultCompleteness::Partial(reasons) = over.completeness else {
        panic!("one step over the connected work limit must be Partial");
    };
    assert!(reasons.contains(PartialReasonSet::PROJECTION_WORK_LIMIT));
    assert_eq!(over.node, root, "a work trip must carrier-stop at the root");
    assert!(
        !over_memo.contains_key(&(root, context)),
        "an unfinished projection root must never enter the view memo"
    );
}

#[test]
fn active_identity_cycle_keeps_recursive_sentinel_at_exhausted_work_boundary() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    dispatch.set_connected_limits_for_tests(0, 24);
    let graph = host.project_type_store().semantic_graph();
    let recursive = graph.intern_node(SemanticNodeData::DeclRef {
        identity: crate::semantic_query::DeclIdentity {
            canonical_id: Arc::from("/recursive.ts"),
            whole_hash: Default::default(),
            decl_name: Arc::from("Recursive"),
        },
    });
    assert!(dispatch.push_instantiate_active((Arc::from("/recursive.ts"), Arc::from("Recursive"),)));
    let (_demand_guard, initial_trip) = dispatch.enter_connected_demand(false);
    assert_eq!(initial_trip, None);
    let tripped = dispatch
        .charge_connected_work()
        .expect_err("the zero-work connected demand must already be tripped");
    assert!(tripped.contains(PartialReasonSet::PROJECTION_WORK_LIMIT));

    let context = ProjectionReductionContext::published(ProjectionMode::Expanded);
    let env = rustc_hash::FxHashMap::default();
    let names = rustc_hash::FxHashMap::default();
    let scope = NodeScopeId::Global;
    let shadowing = ScopeShadowing::empty();
    let inputs = projection_inputs(&env, &scope, &names, &shadowing);
    let mut substitutions = Vec::new();
    let mut memo = ViewMemo::default();
    let outcome = dispatch.project_view_node_worklist(
        recursive,
        context,
        &inputs,
        &mut substitutions,
        &mut memo,
    );
    dispatch.pop_instantiate_active();

    assert_eq!(outcome.completeness, ResultCompleteness::Complete);
    assert!(matches!(
        graph.node_data(outcome.node).as_deref(),
        Some(SemanticNodeData::Opaque(
            crate::semantic_query::QueryError::RecursiveRef { name }
        )) if name.as_ref() == "Recursive"
    ));
    assert_eq!(memo.get(&(recursive, context)), Some(&outcome.node));
}

#[test]
fn exact_memo_identity_cycle_precedes_both_operational_limits() {
    let host = build_host("export type Deep = string;\n".to_string());
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    dispatch.set_connected_limits_for_tests(0, 0);
    let graph = host.project_type_store().semantic_graph();
    let key = SemanticQueryKey::Instantiate(InstantiateKey::new(
        crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from("/deep.ts"),
            Arc::from("Deep"),
        ),
        Arc::from([]),
        InstantiateContext::non_file(
            ProjectionReductionContext::published(ProjectionMode::Expanded),
            Default::default(),
            super::BodySourceWitness::mint_for_unit_tests(),
        ),
    ));

    let before = graph.stats_snapshot();
    let read =
        graph.with_same_path_inflight_for_test(key.clone(), || dispatch.execute_read(key.clone()));
    let after = graph.stats_snapshot();

    assert!(
        matches!(read.value, QueryResult::Recursive(_)),
        "the exact in-flight memo identity must retain the established recursion sentinel"
    );
    assert!(
        read.walker_diagnostics.is_empty(),
        "an exact identity cycle must not be mislabeled as either operational limit: {:?}",
        read.walker_diagnostics
    );
    assert_eq!(
        after.same_path_sentinel_returns,
        before.same_path_sentinel_returns + 1,
        "the cooperative memo, not an operational budget branch, must own the result"
    );
}

fn enter_nested_query_boundaries(
    dispatch: &ProjectSemanticDispatch<'_>,
    remaining: usize,
) -> Option<PartialReasonSet> {
    if remaining == 0 {
        return None;
    }
    let (_guard, trip) = dispatch.enter_connected_demand(true);
    trip.or_else(|| enter_nested_query_boundaries(dispatch, remaining - 1))
}

#[test]
fn connected_query_depth_limit_allows_boundary_and_trips_at_plus_one() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    dispatch.set_connected_limits_for_tests(128, 3);
    assert_eq!(enter_nested_query_boundaries(&dispatch, 3), None);
    let reasons = enter_nested_query_boundaries(&dispatch, 4)
        .expect("the fourth nested boundary must exceed a depth limit of three");
    assert!(reasons.contains(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT));
    assert!(
        !reasons.contains(PartialReasonSet::PROJECTION_WORK_LIMIT),
        "the depth rail must stay distinct from the work rail"
    );
}

#[test]
fn runaway_generic_returns_work_partial_one_root_diagnostic_and_recomputes() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _update = host
        .upsert(UpsertRequest {
            canonical_id: Some("/runaway.ts".to_string()),
            input_id: "/runaway.ts".to_string(),
            source: Arc::from("export type Runaway<T> = Runaway<[T]>;\n"),
            file_language: crate::LanguageRegistry::global()
                .classify_static("/runaway.ts")
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("runaway fixture must parse and index");
    let dispatch = ProjectSemanticDispatch::new(&host);
    // Give genuine cross-query recursion ample headroom so fresh semantic
    // identity growth is stopped specifically by the connected work rail.
    dispatch.set_connected_limits_for_tests(64, 512);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    // A signature utility is a real structural demand point. It repeatedly
    // settles the residual carrier after each Instantiate result, so every
    // `Runaway<[...T]>` level has a fresh node identity and no exact in-flight
    // key cycle can turn it into the legitimate recursive sentinel case.
    let runaway_carrier = graph.intern_node(SemanticNodeData::InstantiationRef {
        base: crate::semantic_query::DeclIdentity {
            canonical_id: Arc::from("/runaway.ts"),
            whole_hash: Default::default(),
            decl_name: Arc::from("Runaway"),
        },
        args: Arc::from([string]),
    });
    let key = SemanticQueryKey::Instantiate(InstantiateKey::new(
        crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from("/w/lib.ts"),
            Arc::from("ReturnType"),
        ),
        Arc::from([runaway_carrier]),
        InstantiateContext::non_file(
            ProjectionReductionContext::published(ProjectionMode::Navigate),
            Default::default(),
            super::BodySourceWitness::mint_for_unit_tests(),
        ),
    ));

    let run = || {
        let _scope = ColdComputeCompletenessScope::enter();
        let read = dispatch.execute_read(key.clone());
        (read, current_cold_compute_completeness())
    };

    let before = graph.stats_snapshot();
    let (first, first_completeness) = run();
    let after_first = graph.stats_snapshot();
    let ResultCompleteness::Partial(first_reasons) = first_completeness else {
        panic!("a connected work trip must preserve typed Partial completeness");
    };
    assert!(first_reasons.contains(PartialReasonSet::PROJECTION_WORK_LIMIT));
    assert!(
        !first_reasons.contains(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT),
        "fresh-identity growth must stop on the work rail, not query depth"
    );
    assert!(first.result_is_partial);
    assert!(first.cache_suppress);
    let QueryResult::Value(_carrier) = first.value else {
        panic!("a work trip must preserve a safe typed carrier");
    };
    let [ShallowDiagnostic::ProjectionWorkLimit {
        root: diagnostic_root,
    }] = first.walker_diagnostics.as_ref()
    else {
        panic!(
            "the runaway root must surface exactly one machine-typed work-limit diagnostic, got {:?}",
            first.walker_diagnostics
        );
    };

    let (second, second_completeness) = run();
    let after_second = graph.stats_snapshot();
    assert!(matches!(
        second_completeness,
        ResultCompleteness::Partial(reasons)
            if reasons.contains(PartialReasonSet::PROJECTION_WORK_LIMIT)
    ));
    assert_eq!(
        second.walker_diagnostics.as_ref(),
        &[ShallowDiagnostic::ProjectionWorkLimit {
            root: *diagnostic_root,
        }]
    );
    assert!(
        after_first.misses > before.misses && after_second.misses > after_first.misses,
        "a limited result must never enter the warm query memo; each request must recompute"
    );
}

#[test]
fn connected_query_depth_limit_is_distinct_diagnostic_and_is_not_cached() {
    let host = build_host("export type Deep = keyof { x: string };\n".to_string());
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    dispatch.set_connected_limits_for_tests(128, 1);
    let graph = host.project_type_store().semantic_graph();
    let key = SemanticQueryKey::Instantiate(InstantiateKey::new(
        crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from("/deep.ts"),
            Arc::from("Deep"),
        ),
        Arc::from([]),
        InstantiateContext::non_file(
            ProjectionReductionContext::published(ProjectionMode::Expanded),
            Default::default(),
            super::BodySourceWitness::mint_for_unit_tests(),
        ),
    ));
    let run = || {
        let _scope = ColdComputeCompletenessScope::enter();
        let read = dispatch.execute_read(key.clone());
        (read, current_cold_compute_completeness())
    };

    let before = graph.stats_snapshot();
    let (first, first_completeness) = run();
    let after_first = graph.stats_snapshot();
    let ResultCompleteness::Partial(reasons) = first_completeness else {
        panic!("nested host-query exhaustion must remain a typed Partial");
    };
    assert!(reasons.contains(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT));
    assert!(!reasons.contains(PartialReasonSet::PROJECTION_WORK_LIMIT));
    let QueryResult::Value(_carrier) = first.value else {
        panic!("query-depth exhaustion must preserve a safe carrier");
    };
    let [ShallowDiagnostic::ConnectedQueryDepthLimit {
        root: diagnostic_root,
    }] = first.walker_diagnostics.as_ref()
    else {
        panic!(
            "the root demand must surface exactly one query-depth diagnostic, got {:?}",
            first.walker_diagnostics
        );
    };

    let (second, second_completeness) = run();
    let after_second = graph.stats_snapshot();
    assert!(matches!(
        second_completeness,
        ResultCompleteness::Partial(reasons)
            if reasons.contains(PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT)
                && !reasons.contains(PartialReasonSet::PROJECTION_WORK_LIMIT)
    ));
    assert_eq!(
        second.walker_diagnostics.as_ref(),
        &[ShallowDiagnostic::ConnectedQueryDepthLimit {
            root: *diagnostic_root,
        }]
    );
    assert!(
        after_first.misses > before.misses && after_second.misses > after_first.misses,
        "query-depth-limited outcomes must stay out of the warm memo"
    );
}

#[test]
fn legitimate_recursive_type_uses_recursive_carrier_without_limit_diagnostic() {
    let host = build_host("export type Deep = { children: Deep[] };\n".to_string());
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let _scope = ColdComputeCompletenessScope::enter();
    let read = dispatch.execute_read(SemanticQueryKey::Instantiate(InstantiateKey::new(
        crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from("/deep.ts"),
            Arc::from("Deep"),
        ),
        Arc::from([]),
        InstantiateContext::non_file(
            ProjectionReductionContext::published(ProjectionMode::Expanded),
            Default::default(),
            super::BodySourceWitness::mint_for_unit_tests(),
        ),
    )));

    assert_eq!(
        current_cold_compute_completeness(),
        ResultCompleteness::Complete,
        "a semantic recursion back-edge is a complete recursive carrier, not operational partiality"
    );
    let QueryResult::Value(value) = read.value else {
        panic!("a legitimate recursive type must resolve to a value carrier");
    };
    assert!(
        read.walker_diagnostics.is_empty(),
        "legitimate recursive types must not be diagnosed as an operational limit: {:?}",
        read.walker_diagnostics
    );
    let raised = dispatch
        .materialize_output_type_expr_for_test(value)
        .expect("recursive graph result must remain materializable");
    let verter_type_expr::TypeExpr::Object(ref object) = raised else {
        panic!("recursive type root must remain an object carrier");
    };
    let children = object
        .properties
        .iter()
        .find_map(|member| match member {
            verter_type_expr::ObjectMember::Property(property) if property.name == "children" => {
                Some(&property.ty)
            }
            _ => None,
        })
        .expect("recursive object must retain its children property");
    let verter_type_expr::TypeExpr::Array { element, .. } = children else {
        panic!("recursive children property must remain an array");
    };
    assert!(
        matches!(
            element.as_ref(),
            verter_type_expr::TypeExpr::RecursiveRef { name, .. } if name.as_ref() == "Deep"
        ),
        "the exact in-flight identity cycle must retain its RecursiveRef sentinel, got {element:?}"
    );
}

#[test]
fn stable_unresolved_reference_is_complete_carrier_without_limit_diagnostic() {
    let host = build_host("export type Deep = MissingType;\n".to_string());
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let _scope = ColdComputeCompletenessScope::enter();
    let read = dispatch.execute_read(SemanticQueryKey::Instantiate(InstantiateKey::new(
        crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            Arc::from("/deep.ts"),
            Arc::from("Deep"),
        ),
        Arc::from([]),
        InstantiateContext::non_file(
            ProjectionReductionContext::published(ProjectionMode::Expanded),
            Default::default(),
            super::BodySourceWitness::mint_for_unit_tests(),
        ),
    )));

    assert_eq!(
        current_cold_compute_completeness(),
        ResultCompleteness::Complete,
        "an unresolved authored reference is a stable Complete carrier"
    );
    let QueryResult::Value(value) = read.value else {
        panic!("a stable unresolved reference must return its value carrier");
    };
    assert!(
        read.walker_diagnostics.is_empty(),
        "stable unresolved references must not receive budget diagnostics: {:?}",
        read.walker_diagnostics
    );
    assert!(
        matches!(
            host.project_type_store()
                .semantic_graph()
                .node_data(value)
                .as_deref(),
            Some(SemanticNodeData::Opaque(
                crate::semantic_query::QueryError::Miss
            ))
        ),
        "the unresolved authored symbol must remain the stable Complete miss carrier"
    );
}
