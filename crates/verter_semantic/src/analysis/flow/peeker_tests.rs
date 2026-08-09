//! @ai-generated - `ReturnPathPeeker` demand-planner tests: reachability
//! selection precision (the acceptance non-materialization case), the
//! two-frontier rule (effect edges live past definite value writes), the
//! right-to-left definite-write stop, spread/unknown-key reachability,
//! multi-origin unions, typed budget refusal, and arena-freedom.

use std::sync::Arc;

use super::*;
use crate::analysis::flow::flow_graph::{
    build_function_flow_graph, FlowEdgeKind, FlowNodeId, FlowNodeKind, FunctionFlowGraph,
};
use crate::analysis::flow::flow_ir::ReturnSlicePlan;
use crate::analysis::flow::{
    build_function_body_skeleton, FunctionBodySkeleton, FunctionBodySource, SkeletonPathSegment,
    SkeletonReturnSiteId, SkeletonWriteCertainty,
};

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

fn names(path: &[&str]) -> Vec<Arc<str>> {
    path.iter().map(|name| Arc::from(*name)).collect()
}

fn plan_return(
    skeleton: &FunctionBodySkeleton,
    graph: &FunctionFlowGraph,
    path: &[&str],
) -> ReturnSlicePlan {
    let demand = SliceDemand::for_return_projection(skeleton, &names(path));
    ReturnPathPeeker::new(graph)
        .plan(&demand, &FlowSliceBudget::default())
        .expect("plan within default budget")
}

/// The value site provisioning static key `key` of the object returned
/// at return site 0.
fn entry_value_node(
    skeleton: &FunctionBodySkeleton,
    graph: &FunctionFlowGraph,
    key: &str,
    occurrence: usize,
) -> FlowNodeId {
    let object_site = skeleton.return_sites[0].argument.expect("argument");
    let object_node = graph.expr_site_node(object_site);
    let key_id = skeleton.name_id(key).expect("key interned");
    let mut seen = 0usize;
    for edge in graph.out_edges(object_node) {
        if let FlowEdgeKind::PathWrite { path, .. } = &edge.kind {
            if path.as_ref() == [SkeletonPathSegment::Static(key_id)] {
                if seen == occurrence {
                    return edge.to;
                }
                seen += 1;
            }
        }
    }
    panic!("object literal must provision `{key}` (occurrence {occurrence})");
}

fn binding_node(
    skeleton: &FunctionBodySkeleton,
    graph: &FunctionFlowGraph,
    name: &str,
) -> FlowNodeId {
    let id = skeleton.name_id(name).expect("name interned");
    let binding = skeleton
        .bindings_named(id)
        .next()
        .unwrap_or_else(|| panic!("`{name}` must be bound"));
    graph.binding_node(binding)
}

/// The acceptance non-materialization case: demanding `['b']` of
/// `return { a: new Mytype(), b: 1 }` (via locals) selects only `b`'s
/// provider chain. `a`'s hub, `a`'s initializer (the `new Mytype()`
/// construct), and `a`'s entry value site are reached by NEITHER
/// frontier.
#[test]
fn planner_selects_only_demanded_member_value() {
    let skeleton =
        skeleton_of("function myType() { const a = new Mytype(); const b = 1; return { a, b } }");
    let graph = build_function_flow_graph(&skeleton);
    let plan = plan_return(&skeleton, &graph, &["b"]);

    let b_entry = entry_value_node(&skeleton, &graph, "b", 0);
    let b_hub = binding_node(&skeleton, &graph, "b");
    assert!(plan.is_value(b_entry), "b's entry value is value-selected");
    assert!(plan.is_value(b_hub), "b's binding hub is value-selected");

    let a_entry = entry_value_node(&skeleton, &graph, "a", 0);
    let a_hub = binding_node(&skeleton, &graph, "a");
    let a_binding = skeleton
        .bindings_named(skeleton.name_id("a").expect("a"))
        .next()
        .expect("a bound");
    let a_init = skeleton.binding(a_binding).initializer.expect("a init");
    let a_init_node = graph.expr_site_node(a_init);

    assert!(!plan.is_selected(a_entry), "a's entry value stays out");
    assert!(!plan.is_selected(a_hub), "a's binding hub stays out");
    assert!(
        !plan.is_selected(a_init_node),
        "`new Mytype()` is never reached — nothing to materialize"
    );

    // b's initializer (`1`) is value-selected through the hub.
    let b_binding = skeleton
        .bindings_named(skeleton.name_id("b").expect("b"))
        .next()
        .expect("b bound");
    let b_init = skeleton.binding(b_binding).initializer.expect("b init");
    assert!(plan.is_value(graph.expr_site_node(b_init)));
}

/// The two-frontier soundness case: `return { a: (x = "s"), b:
/// x.toUpperCase() }` demanding `['b']` must NOT value-select sibling
/// `a`, but MUST keep `a`'s evaluation effect reachable (its initializer
/// retypes `x`, which `b` reads), and the retyping write's right-hand
/// side must be VALUE-selected through `x`'s hub.
#[test]
fn planner_keeps_effect_edges_past_definite_value_writes() {
    let skeleton =
        skeleton_of(r#"function f(x: string) { return { a: (x = "s"), b: x.toUpperCase() } }"#);
    let graph = build_function_flow_graph(&skeleton);
    let plan = plan_return(&skeleton, &graph, &["b"]);

    let a_entry = entry_value_node(&skeleton, &graph, "a", 0);
    let b_entry = entry_value_node(&skeleton, &graph, "b", 0);
    let x_hub = binding_node(&skeleton, &graph, "x");

    assert!(plan.is_value(b_entry), "b's value is demanded");
    assert!(
        plan.is_effect_only(a_entry),
        "a's site stays reachable on the EFFECT frontier only"
    );
    assert!(!plan.is_value(a_entry), "a's value is never materialized");
    assert!(
        plan.is_value(x_hub),
        "x is read by the selected path — value-selected"
    );

    // The sibling write's right-hand side (`"s"`) is x's reaching
    // definition — value-selected.
    let rhs_site = skeleton.writes[0].value.expect("assignment value site");
    assert!(plan.is_value(graph.expr_site_node(rhs_site)));
}

/// The effect frontier never materializes values: an effect-only site's
/// value-def out-edges (its reads) are NOT followed.
#[test]
fn planner_effect_frontier_does_not_follow_value_edges() {
    let skeleton = skeleton_of("function g(x: number, u: number) { return { a: h(u), b: x } }");
    let graph = build_function_flow_graph(&skeleton);
    let plan = plan_return(&skeleton, &graph, &["b"]);

    let a_entry = entry_value_node(&skeleton, &graph, "a", 0);
    let u_hub = binding_node(&skeleton, &graph, "u");
    assert!(
        plan.is_effect_only(a_entry),
        "the call-bearing sibling is effect-reachable"
    );
    assert!(
        !plan.is_selected(u_hub),
        "effect reachability must not follow the sibling's VALUE reads"
    );
    assert!(plan.is_value(binding_node(&skeleton, &graph, "x")));
}

/// Duplicate keys: the right-to-left scan stops at the LAST
/// definite-present write for the demanded head; the earlier value-dead
/// entry keeps only its evaluation effect.
#[test]
fn planner_duplicate_key_stops_at_last_definite_write() {
    let skeleton = skeleton_of("function f2(x: number) { return { a: (x = 1), a: 2, b: x } }");
    let graph = build_function_flow_graph(&skeleton);
    let plan = plan_return(&skeleton, &graph, &["a"]);

    let first_a = entry_value_node(&skeleton, &graph, "a", 0);
    let last_a = entry_value_node(&skeleton, &graph, "a", 1);
    assert!(plan.is_value(last_a), "the overwriting entry provides `a`");
    assert!(
        !plan.is_value(first_a),
        "the overwritten entry is value-suppressed by the definite write"
    );
    assert!(
        plan.is_effect_only(first_a),
        "the overwritten entry's evaluation effect (x = 1) survives"
    );
}

/// Spread reachability: a spread BEFORE the demanded static key stays
/// out (the scan stops at the later definite write); a spread AFTER the
/// demanded key stays reachable as a candidate provider, and the earlier
/// static entry remains reachable past it.
#[test]
fn planner_spread_optional_writes_stay_reachable_without_stopping() {
    // Spread first, then the definite `b`: scan right-to-left stops at
    // `b`; the spread is never value-reached.
    let skeleton = skeleton_of("function s(rest: object) { return { ...rest, b: 1 } }");
    let graph = build_function_flow_graph(&skeleton);
    let plan = plan_return(&skeleton, &graph, &["b"]);
    let object_site = skeleton.return_sites[0].argument.expect("argument");
    let object_node = graph.expr_site_node(object_site);
    let spread_source = graph
        .out_edges(object_node)
        .iter()
        .find_map(|edge| match &edge.kind {
            FlowEdgeKind::PathWrite { certainty, .. }
                if *certainty == SkeletonWriteCertainty::Optional =>
            {
                Some(edge.to)
            }
            _ => None,
        })
        .expect("spread edge");
    assert!(
        !plan.is_selected(spread_source),
        "a spread before the definite demanded write is fully suppressed"
    );

    // Spread last: it stays a candidate provider (optional write), and
    // the earlier static entry remains reachable past it.
    let skeleton2 = skeleton_of("function s2(rest: object) { return { b: 1, ...rest } }");
    let graph2 = build_function_flow_graph(&skeleton2);
    let plan2 = plan_return(&skeleton2, &graph2, &["b"]);
    let object2 = graph2.expr_site_node(skeleton2.return_sites[0].argument.expect("argument"));
    let mut spread2 = None;
    let mut b2 = None;
    for edge in graph2.out_edges(object2) {
        if let FlowEdgeKind::PathWrite { certainty, .. } = &edge.kind {
            if *certainty == SkeletonWriteCertainty::Optional {
                spread2 = Some(edge.to);
            } else {
                b2 = Some(edge.to);
            }
        }
    }
    assert!(
        plan2.is_value(spread2.expect("spread edge")),
        "a trailing spread stays a candidate provider for the demand"
    );
    assert!(
        plan2.is_value(b2.expect("b edge")),
        "the earlier static entry stays reachable past the optional write"
    );
}

/// A whole-value demand (empty path) selects every entry contributor.
#[test]
fn planner_whole_value_demand_selects_all_contributors() {
    let skeleton = skeleton_of("function w() { const a = 1; const b = 2; return { a, b } }");
    let graph = build_function_flow_graph(&skeleton);
    let plan = plan_return(&skeleton, &graph, &[]);
    assert!(plan.is_value(entry_value_node(&skeleton, &graph, "a", 0)));
    assert!(plan.is_value(entry_value_node(&skeleton, &graph, "b", 0)));
    assert!(plan.is_value(binding_node(&skeleton, &graph, "a")));
    assert!(plan.is_value(binding_node(&skeleton, &graph, "b")));
}

/// Conditional returns: the whole-return demand is a multi-source
/// reachability over every return site; regions arrive through
/// control-region edges, and the branch condition site (no
/// narrowing-predicate edges yet) stays out of the VALUE selection.
#[test]
fn planner_multi_origin_unions_conditional_returns() {
    let skeleton =
        skeleton_of("function c(flag: boolean) { if (flag) { return { b: 1 } } return { b: 2 } }");
    let graph = build_function_flow_graph(&skeleton);
    let plan = plan_return(&skeleton, &graph, &["b"]);

    assert_eq!(plan.origins.len(), 2, "both return sites are origins");
    for (index, _site) in skeleton.return_sites.iter().enumerate() {
        let node = graph.return_site_node(SkeletonReturnSiteId::from_index(index as u32));
        assert!(plan.is_value(node), "return site {index} is selected");
    }
    // Regions are effect-selected through control-region edges.
    let arm_region = skeleton.return_sites[0].region;
    assert!(plan.is_effect_only(graph.region_node(arm_region)));

    // The condition expression is not a value provider of `b`.
    let consequent_parent = skeleton.region(arm_region).parent.expect("consequent");
    let condition = skeleton
        .region(consequent_parent)
        .control_input
        .expect("condition input");
    assert!(
        !plan.is_value(graph.expr_site_node(condition)),
        "no narrowing-predicate edges exist yet — the condition is not value-selected"
    );
}

/// An expression-site origin plans reachability from that site.
#[test]
fn planner_expression_site_origin_plans_reachability() {
    let skeleton = skeleton_of("function e(k: number) { const v = k + 1; return v }");
    let graph = build_function_flow_graph(&skeleton);
    let v_binding = skeleton
        .bindings_named(skeleton.name_id("v").expect("v"))
        .next()
        .expect("v bound");
    let init = skeleton.binding(v_binding).initializer.expect("init");
    let demand = SliceDemand::for_expression_site(&skeleton, init, &[]);
    let plan = ReturnPathPeeker::new(&graph)
        .plan(&demand, &FlowSliceBudget::default())
        .expect("plan");
    assert!(plan.is_value(graph.expr_site_node(init)));
    assert!(plan.is_value(binding_node(&skeleton, &graph, "k")));
    // The return site is NOT an origin here.
    let return_node = graph.return_site_node(SkeletonReturnSiteId::from_index(0));
    assert!(!plan.is_selected(return_node));
}

/// A demanded key the body never mentions resolves to a Foreign segment:
/// no static entry matches, so no entry value is selected.
#[test]
fn planner_foreign_demand_key_matches_no_static_entry() {
    let skeleton = skeleton_of("function f3() { const a = 1; return { a, b: 2 } }");
    let graph = build_function_flow_graph(&skeleton);
    let demand = SliceDemand::for_return_projection(&skeleton, &names(&["zzz"]));
    assert!(matches!(demand.path[0], DemandSegment::Foreign(_)));
    let plan = ReturnPathPeeker::new(&graph)
        .plan(&demand, &FlowSliceBudget::default())
        .expect("plan");
    assert!(!plan.is_value(entry_value_node(&skeleton, &graph, "a", 0)));
    assert!(!plan.is_value(entry_value_node(&skeleton, &graph, "b", 0)));
}

/// Budget refusal is TYPED and total: a tripped node cap returns
/// `FlowSliceBudgetExceeded` (never a panic, never a truncated plan).
#[test]
fn planner_budget_exceeded_is_typed_refusal() {
    let skeleton = skeleton_of("function b1() { const a = 1; const b = a + 1; return { a, b } }");
    let graph = build_function_flow_graph(&skeleton);
    let demand = SliceDemand::for_return_projection(&skeleton, &[]);
    let tiny = FlowSliceBudget {
        max_return_sites: 256,
        max_selected_nodes: 1,
    };
    let refused = ReturnPathPeeker::new(&graph)
        .plan(&demand, &tiny)
        .expect_err("one node cannot hold this slice");
    assert_eq!(refused.axis, FlowSliceBudgetAxis::SelectedNodes);
    assert_eq!(refused.limit, 1);
    assert!(refused.observed > 1);

    // The return-site axis trips before any traversal.
    let no_returns = FlowSliceBudget {
        max_return_sites: 0,
        max_selected_nodes: 4096,
    };
    let refused = ReturnPathPeeker::new(&graph)
        .plan(&demand, &no_returns)
        .expect_err("zero return-site budget");
    assert_eq!(refused.axis, FlowSliceBudgetAxis::ReturnSites);
}

/// Planning is deterministic: the same demand yields an identical plan.
#[test]
fn planner_is_deterministic_over_one_graph() {
    let source = r#"
function d(a: number, flag: boolean) {
  let out = a;
  if (flag) { out = a + 1; } else { out += 2; }
  return { out, tag: "d" };
}
"#;
    let skeleton = skeleton_of(source);
    let graph = build_function_flow_graph(&skeleton);
    let first = plan_return(&skeleton, &graph, &["out"]);
    let second = plan_return(&skeleton, &graph, &["out"]);
    assert_eq!(first, second);
    assert!(!first.value_nodes.is_empty());
}

/// The planner's input type is the structural proof it plans over the
/// graph alone: constructing a `ReturnPathPeeker` requires ONLY a
/// `&FunctionFlowGraph` — no skeleton, no AST, no statement list is
/// reachable from it, so a procedural body walk is impossible by
/// construction. The plan's node sets are disjoint and sorted.
#[test]
fn planner_holds_only_the_graph_and_emits_disjoint_sorted_sets() {
    fn peeker_input_is_graph_only<'g>(graph: &'g FunctionFlowGraph) -> ReturnPathPeeker<'g> {
        ReturnPathPeeker::new(graph)
    }
    let skeleton =
        skeleton_of(r#"function f(x: string) { return { a: (x = "s"), b: x.toUpperCase() } }"#);
    let graph = build_function_flow_graph(&skeleton);
    let peeker = peeker_input_is_graph_only(&graph);
    let demand = SliceDemand::for_return_projection(&skeleton, &names(&["b"]));
    let plan = peeker
        .plan(&demand, &FlowSliceBudget::default())
        .expect("plan");

    // Every loop below is vacuous on an empty plan, so the plan being
    // non-empty is the precondition that makes them mean anything: this
    // fixture selects `b`, whose value reads `x`, and records the `x = "s"`
    // write as an effect.
    assert!(
        !plan.value_nodes.is_empty(),
        "the demanded member's value providers must be selected"
    );
    assert!(
        !plan.effect_only_nodes.is_empty(),
        "the parameter write must be selected as an effect-only node"
    );

    for window in plan.value_nodes.windows(2) {
        assert!(window[0].index() < window[1].index(), "sorted, no dups");
    }
    for window in plan.effect_only_nodes.windows(2) {
        assert!(window[0].index() < window[1].index(), "sorted, no dups");
    }
    for node in plan.effect_only_nodes.iter() {
        assert!(!plan.is_value(*node), "role sets are disjoint");
    }
    for node in plan.value_nodes.iter() {
        assert!(plan.is_value(*node), "role sets are disjoint");
    }
    // Every selected node addresses a real graph node — `node_kind` is
    // total over the graph's own ids, so a selected id outside it is the
    // failure this catches.
    for node in plan.value_nodes.iter().chain(plan.effect_only_nodes.iter()) {
        assert!(
            node.index() < graph.node_count(),
            "a selected node must address a real graph node"
        );
    }
    // A control REGION carries no value: it can be selected for its
    // EFFECTS, never as a value provider.
    for node in plan.value_nodes.iter() {
        let kind: FlowNodeKind = graph.node_kind(*node);
        assert!(
            matches!(
                kind,
                FlowNodeKind::Binding(_) | FlowNodeKind::ExprSite(_) | FlowNodeKind::ReturnSite(_)
            ),
            "a control REGION is never a value provider, got {kind:?}"
        );
    }
}

/// Every planner carrier is arena-free and `TypeExpr`-free.
#[test]
fn planner_carriers_are_arena_free_send_sync_static() {
    fn assert_arena_free<T: Send + Sync + 'static + verter_no_typeexpr::NoTypeExpr>() {}
    assert_arena_free::<SliceDemand>();
    assert_arena_free::<SliceOrigin>();
    assert_arena_free::<DemandSegment>();
    assert_arena_free::<FlowSliceBudget>();
    assert_arena_free::<FlowSliceBudgetExceeded>();
    assert_arena_free::<ReturnSlicePlan>();
}
