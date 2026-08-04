//! @ai-generated - `FunctionFlowGraph` typed-edge discrimination tests:
//! skeleton-only construction, value-provider vs effect edge families,
//! value-dead siblings keeping their evaluation-effect edges, region
//! membership / nesting, arena-freedom, and determinism.

use super::*;
use crate::analysis::flow::{
    build_function_body_skeleton, FunctionBodySkeleton, FunctionBodySource, SkeletonBindingId,
    SkeletonPathSegment, SkeletonRegionKind, SkeletonReturnSiteId, SkeletonWriteCertainty,
};

fn return_site_id(skeleton: &FunctionBodySkeleton, ordinal: usize) -> SkeletonReturnSiteId {
    assert!(ordinal < skeleton.return_sites.len(), "return site exists");
    SkeletonReturnSiteId::from_index(ordinal as u32)
}

fn single_binding(skeleton: &FunctionBodySkeleton, name: &str) -> SkeletonBindingId {
    let id = skeleton
        .name_id(name)
        .unwrap_or_else(|| panic!("name `{name}` must be interned"));
    let mut bindings = skeleton.bindings_named(id);
    let binding = bindings
        .next()
        .unwrap_or_else(|| panic!("`{name}` must be bound"));
    assert!(bindings.next().is_none(), "`{name}` binds exactly once");
    binding
}

fn out_edges_of_class(
    graph: &FunctionFlowGraph,
    from: FlowNodeId,
    class: FlowEdgeClass,
) -> Vec<&FlowEdge> {
    graph
        .out_edges(from)
        .iter()
        .filter(|edge| edge.kind.class() == class)
        .collect()
}

fn has_edge_class(
    graph: &FunctionFlowGraph,
    from: FlowNodeId,
    to: FlowNodeId,
    class: FlowEdgeClass,
) -> bool {
    graph
        .out_edges(from)
        .iter()
        .any(|edge| edge.to == to && edge.kind.class() == class)
}

fn out_path_writes(graph: &FunctionFlowGraph, from: FlowNodeId) -> Vec<&FlowEdge> {
    out_edges_of_class(graph, from, FlowEdgeClass::PathWrite)
}

fn path_write_path(edge: &FlowEdge) -> Vec<SkeletonPathSegment> {
    let FlowEdgeKind::PathWrite { path, .. } = &edge.kind else {
        panic!("edge must be a path write");
    };
    path.to_vec()
}

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

#[test]
fn flow_graph_builds_typed_edges_from_skeleton_alone() {
    let skeleton =
        skeleton_of("function myType() { const a = new Mytype(); const b = 1; return { a, b } }");
    let graph = build_function_flow_graph(&skeleton);
    assert_eq!(graph.region_kind, ExecutableRegionKind::Function);

    // Return site → object argument (value-def).
    let return_node = graph.return_site_node(return_site_id(&skeleton, 0));
    let object_site = skeleton.return_sites[0].argument.expect("argument");
    let object_node = graph.expr_site_node(object_site);
    assert!(has_edge_class(
        &graph,
        return_node,
        object_node,
        FlowEdgeClass::ValueDef
    ));

    // Object → per-key value sites (path-writes), authored order.
    let path_writes = out_path_writes(&graph, object_node);
    assert_eq!(path_writes.len(), 2);
    let a_name = skeleton.name_id("a").expect("a interned");
    let b_name = skeleton.name_id("b").expect("b interned");
    assert_eq!(
        path_write_path(path_writes[0]),
        vec![SkeletonPathSegment::Static(a_name)]
    );
    assert_eq!(
        path_write_path(path_writes[1]),
        vec![SkeletonPathSegment::Static(b_name)]
    );

    // Shorthand `a` value site reads binding `a`; the binding's definition
    // hub provides its initializer.
    let a_value_node = path_writes[0].to;
    let a_binding = single_binding(&skeleton, "a");
    assert!(has_edge_class(
        &graph,
        a_value_node,
        graph.binding_node(a_binding),
        FlowEdgeClass::ValueDef
    ));
    let a_init = skeleton.binding(a_binding).initializer.expect("init");
    assert!(has_edge_class(
        &graph,
        graph.binding_node(a_binding),
        graph.expr_site_node(a_init),
        FlowEdgeClass::ValueDef
    ));

    // `Mytype` stays a structural name: it binds nothing, so the `a`
    // initializer's construct call produces NO effect edge and NO binding
    // node — nothing to materialize through this storage.
    assert!(skeleton.name_id("Mytype").is_some());
    let init_node = graph.expr_site_node(a_init);
    assert!(out_edges_of_class(&graph, init_node, FlowEdgeClass::EvalEffect).is_empty());
    assert!(
        skeleton
            .name_id("Mytype")
            .into_iter()
            .all(|id| skeleton.bindings_named(id).next().is_none()),
        "`Mytype` must not bind in this frame"
    );
    // The object container evaluates no effectful child here.
    assert!(out_edges_of_class(&graph, object_node, FlowEdgeClass::EvalEffect).is_empty());
}

#[test]
fn flow_graph_keeps_effect_edges_for_value_dead_siblings() {
    let skeleton =
        skeleton_of(r#"function f(x: string) { return { a: (x = "s"), b: x.toUpperCase() } }"#);
    let graph = build_function_flow_graph(&skeleton);

    let object_site = skeleton.return_sites[0].argument.expect("argument");
    let object_node = graph.expr_site_node(object_site);
    let path_writes = out_path_writes(&graph, object_node);
    assert_eq!(path_writes.len(), 2);
    let a_value_node = path_writes[0].to;
    let b_value_node = path_writes[1].to;
    let x_node = graph.binding_node(single_binding(&skeleton, "x"));

    // `a`'s value site carries the evaluation effect on `x` — an EFFECT
    // edge, not a value edge.
    assert!(has_edge_class(
        &graph,
        a_value_node,
        x_node,
        FlowEdgeClass::EvalEffect
    ));

    // The container's evaluation reaches BOTH effectful children.
    assert!(has_edge_class(
        &graph,
        object_node,
        a_value_node,
        FlowEdgeClass::EvalEffect
    ));
    assert!(has_edge_class(
        &graph,
        object_node,
        b_value_node,
        FlowEdgeClass::EvalEffect
    ));

    // `b` reads `x` (value-def), and `x`'s hub provides the sibling
    // write's right-hand side.
    assert!(has_edge_class(
        &graph,
        b_value_node,
        x_node,
        FlowEdgeClass::ValueDef
    ));
    let x_defs = out_edges_of_class(&graph, x_node, FlowEdgeClass::ValueDef);
    assert_eq!(x_defs.len(), 1, "x's only definition is the sibling write");
    let rhs_site = skeleton.writes[0].value.expect("assignment value site");
    assert_eq!(x_defs[0].to, graph.expr_site_node(rhs_site));

    // Class discipline: the `a` provisioning edge is a PATH-WRITE, never
    // an effect edge, and the hub carries no path-writes here.
    assert_eq!(path_writes[0].kind.class(), FlowEdgeClass::PathWrite);
    assert!(out_path_writes(&graph, x_node).is_empty());
}

#[test]
fn flow_graph_duplicate_key_definite_write_keeps_earlier_entry_edges() {
    let skeleton = skeleton_of("function f2(x: number) { return { a: (x = 1), a: 2, b: x } }");
    let graph = build_function_flow_graph(&skeleton);

    let object_site = skeleton.return_sites[0].argument.expect("argument");
    let object_node = graph.expr_site_node(object_site);
    let a_name = skeleton.name_id("a").expect("a interned");

    // BOTH `a` entries keep their path-write edges, in authored order —
    // a later definite write never prunes the earlier entry.
    let a_writes: Vec<&FlowEdge> = out_path_writes(&graph, object_node)
        .into_iter()
        .filter(|edge| path_write_path(edge) == vec![SkeletonPathSegment::Static(a_name)])
        .collect();
    assert_eq!(a_writes.len(), 2);
    assert!(a_writes[0].ordinal < a_writes[1].ordinal);

    // The value-dead first entry keeps its evaluation-effect edges.
    let first_a_value = a_writes[0].to;
    let x_node = graph.binding_node(single_binding(&skeleton, "x"));
    assert!(has_edge_class(
        &graph,
        first_a_value,
        x_node,
        FlowEdgeClass::EvalEffect
    ));
    assert!(has_edge_class(
        &graph,
        object_node,
        first_a_value,
        FlowEdgeClass::EvalEffect
    ));
}

#[test]
fn flow_graph_spread_entries_are_optional_unknown_path_writes() {
    let skeleton = skeleton_of("function s(rest: object) { return { ...rest, b: 1 } }");
    let graph = build_function_flow_graph(&skeleton);
    let object_site = skeleton.return_sites[0].argument.expect("argument");
    let path_writes = out_path_writes(&graph, graph.expr_site_node(object_site));
    assert_eq!(path_writes.len(), 2);
    let FlowEdgeKind::PathWrite { path, certainty } = &path_writes[0].kind else {
        panic!("spread entry is a path write");
    };
    assert_eq!(path.as_ref(), &[SkeletonPathSegment::Computed]);
    assert_eq!(*certainty, SkeletonWriteCertainty::Optional);
    let FlowEdgeKind::PathWrite {
        certainty: b_certainty,
        ..
    } = &path_writes[1].kind
    else {
        panic!("static entry is a path write");
    };
    assert_eq!(*b_certainty, SkeletonWriteCertainty::Definite);
}

#[test]
fn flow_graph_region_membership_and_nesting() {
    let skeleton = skeleton_of("function f3(c: boolean) { if (c) { return 1; } return 2; }");
    let graph = build_function_flow_graph(&skeleton);

    // The arm return belongs to the block region nested in the consequent
    // region nested in the function body — via control-region edges.
    let arm_return = graph.return_site_node(return_site_id(&skeleton, 0));
    let arm_region = skeleton.return_sites[0].region;
    assert!(has_edge_class(
        &graph,
        arm_return,
        graph.region_node(arm_region),
        FlowEdgeClass::ControlRegion
    ));
    assert_eq!(skeleton.region(arm_region).kind, SkeletonRegionKind::Block);
    let consequent = skeleton.region(arm_region).parent.expect("parent");
    assert_eq!(
        skeleton.region(consequent).kind,
        SkeletonRegionKind::IfConsequent
    );
    assert!(has_edge_class(
        &graph,
        graph.region_node(arm_region),
        graph.region_node(consequent),
        FlowEdgeClass::ControlRegion
    ));
    let root = skeleton.region(consequent).parent.expect("root");
    assert_eq!(skeleton.region(root).kind, SkeletonRegionKind::FunctionBody);

    // The condition site is the consequent's control input and reads `c`.
    let condition = skeleton
        .region(consequent)
        .control_input
        .expect("condition input");
    let c_node = graph.binding_node(single_binding(&skeleton, "c"));
    assert!(has_edge_class(
        &graph,
        graph.expr_site_node(condition),
        c_node,
        FlowEdgeClass::ValueDef
    ));

    // The trailing return belongs to the root region directly.
    let trailing = graph.return_site_node(return_site_id(&skeleton, 1));
    assert!(has_edge_class(
        &graph,
        trailing,
        graph.region_node(root),
        FlowEdgeClass::ControlRegion
    ));
}

#[test]
fn flow_graph_is_arena_free_send_sync_static() {
    fn assert_arena_free<T: Send + Sync + 'static + verter_no_typeexpr::NoTypeExpr>() {}
    assert_arena_free::<FunctionFlowGraph>();
    assert_arena_free::<FlowEdge>();
    assert_arena_free::<FlowEdgeKind>();
    assert_arena_free::<ExecutableRegionKind>();
}

#[test]
fn flow_graph_build_is_deterministic_over_one_skeleton() {
    let source = r#"
function d(a: number, flag: boolean) {
  let out = a;
  if (flag) { out = a + 1; } else { out += 2; }
  for (const step of [1, 2]) { out += step; }
  return { out, tag: "d" };
}
"#;
    let skeleton = skeleton_of(source);
    let first = build_function_flow_graph(&skeleton);
    let second = build_function_flow_graph(&skeleton);
    assert_eq!(first, second);
    // Same content version reproduces the same skeleton AND the same graph
    // through an independent parse.
    let reparsed = skeleton_of(source);
    assert_eq!(skeleton, reparsed);
    assert_eq!(first, build_function_flow_graph(&reparsed));
}

#[test]
fn flow_graph_csr_out_edges_are_from_consistent() {
    let skeleton =
        skeleton_of("function e(a: number) { let b = a; b = a + 1; if (a) { b++; } return b; }");
    let graph = build_function_flow_graph(&skeleton);
    let mut total = 0usize;
    for index in 0..graph.node_count() {
        let node = FlowNodeId::from_index(index as u32);
        for edge in graph.out_edges(node) {
            assert_eq!(edge.from, node);
            total += 1;
        }
    }
    assert_eq!(total, graph.edges().len());
    assert!(total > 0, "the fixture produces edges");
}
