//! @ai-generated - `FunctionFlowGraph` typed-edge discrimination tests:
//! skeleton-only construction, value-provider vs effect edge families,
//! value-dead siblings keeping their evaluation-effect edges, region
//! membership / nesting, arena-freedom, and determinism.

use super::build_function_flow_graph_for_test as build_function_flow_graph;
use super::*;
use crate::analysis::flow::{
    FunctionBodySkeleton, FunctionBodySource, SkeletonBindingId, SkeletonPathSegment,
    SkeletonRegionKind, SkeletonReturnSiteId, SkeletonWriteCertainty,
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
    crate::analysis::flow::skeleton_tests::indexed_skeleton_of(source)
}

#[test]
fn write_only_closure_capture_selects_an_effect_subject_without_a_value_read() {
    use crate::analysis::flow::peeker::{FlowSliceBudget, ReturnPathPeeker, SliceDemand};
    let skeleton = indexed_returned_arrow(
        "function root(value) { return () => () => { value = 1; return 0; }; }",
    );
    let graph = build_function_flow_graph(&skeleton);
    let plan = ReturnPathPeeker::new(&graph)
        .plan(
            &SliceDemand::for_return_projection(&skeleton, &[]),
            &FlowSliceBudget::default(),
        )
        .unwrap();
    let hub = (0..graph.node_count())
        .map(|index| FlowNodeId::from_index(index as u32))
        .find(|node| matches!(graph.node_kind(*node), FlowNodeKind::CapturedBinding(_)))
        .expect("a captured cell has a graph-owned subject even without a value read");
    assert!(plan.is_effect_only(hub));
    assert!(skeleton.expr_sites.iter().all(|site| site.reads.is_empty()));
    let closure = skeleton
        .expr_sites
        .iter()
        .position(|site| !site.capture_bindings.is_empty())
        .unwrap();
    let from = graph.expr_site_node(SkeletonExprSiteId::from_index(closure as u32));
    assert!(has_edge_class(&graph, from, hub, FlowEdgeClass::EvalEffect));
    assert!(!has_edge_class(&graph, from, hub, FlowEdgeClass::ValueDef));
}

#[test]
fn captured_reads_select_own_frame_writes_and_their_control_inputs() {
    use crate::analysis::flow::peeker::{FlowSliceBudget, ReturnPathPeeker, SliceDemand};
    use crate::analysis::flow::FlowBindingRef;
    let source = "function root(value, flag) { return () => { if (flag) value = 'b'; { let value = 0; value = 2; } return value; }; }";
    let skeleton = indexed_returned_arrow(source);
    let graph = build_function_flow_graph(&skeleton);
    let plan = ReturnPathPeeker::new(&graph)
        .plan(
            &SliceDemand::for_return_projection(&skeleton, &[]),
            &FlowSliceBudget::default(),
        )
        .unwrap();
    let captured = skeleton
        .writes
        .iter()
        .find(|write| matches!(write.binding, Some(FlowBindingRef::Captured(_))))
        .unwrap();
    assert!(
        plan.is_value(graph.expr_site_node(captured.value.unwrap())),
        "a captured read must select its own-frame written value"
    );
    assert!(
        plan.is_selected(graph.expr_site_node(captured.site)),
        "the write execution site stays selected"
    );
    let condition = skeleton.regions[captured.region.index()]
        .control_input
        .or_else(|| {
            skeleton
                .regions
                .iter()
                .find_map(|region| region.control_input)
        })
        .unwrap();
    assert!(
        plan.is_value(graph.expr_site_node(condition)),
        "the governing control input stays value-selected"
    );
    let shadow = skeleton
        .writes
        .iter()
        .find(|write| matches!(write.binding, Some(FlowBindingRef::Local(_))))
        .unwrap();
    assert!(
        !plan.is_selected(graph.expr_site_node(shadow.site)),
        "a same-named local write is not a captured definition"
    );
}

fn indexed_returned_arrow(source: &str) -> FunctionBodySkeleton {
    use crate::analysis::flow::build_indexed_function_body_skeleton;
    use crate::analysis::function_program::{
        build_function_program_index, resolve_function_node, FunctionNode,
    };
    use crate::analysis::top_level_owners::TopLevelOwnerTable;
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
    let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
    let index =
        build_function_program_index(&parsed.program, source, &owners, Arc::from("/capture.ts"));
    let root = index.matches_named("root").next().unwrap().entry();
    let child = index
        .nested_at(
            &root.key,
            verter_span::Span::new(
                source.find("() =>").unwrap() as u32,
                (source.rfind("; }").unwrap()) as u32,
            ),
        )
        .unwrap()
        .entry();
    let FunctionNode::Arrow(arrow) = resolve_function_node(&parsed.program, &child.locator)
        .unwrap()
        .node
    else {
        panic!("arrow fixture");
    };
    let prepared =
        build_indexed_function_body_skeleton(&FunctionBodySource::from_arrow(arrow), child)
            .unwrap();
    prepared.skeleton
}

#[test]
fn captured_computation_inputs_do_not_grow_result_projection_cycles() {
    use crate::analysis::flow::peeker::{FlowSliceBudget, ReturnPathPeeker, SliceDemand};
    let skeleton = indexed_returned_arrow(
        "function root(value) { return () => { value=value.trim(); return value; }; }",
    );
    let graph = build_function_flow_graph(&skeleton);
    let plan = ReturnPathPeeker::new(&graph)
        .plan(
            &SliceDemand::for_return_projection(&skeleton, &[]),
            &FlowSliceBudget {
                max_value_states: 64,
                ..FlowSliceBudget::default()
            },
        )
        .expect("captured computation inputs have the same bounded path transfer as local inputs");
    let write = skeleton.writes.first().unwrap();
    assert!(plan.is_value(graph.expr_site_node(write.value.unwrap())));
}

#[test]
fn captured_member_reads_compose_projection_before_selecting_write_members() {
    use crate::analysis::flow::peeker::{FlowSliceBudget, ReturnPathPeeker, SliceDemand};
    use crate::analysis::flow::{FlowBindingRef, FrameSpan};
    let source = "function root() { let x = {a: {b: 'old'}, b: 'other'}; return () => { x = {a: {b: 'wanted'}, b: 'sibling'}; return x.a; }; }";
    let skeleton = indexed_returned_arrow(source);
    let graph = build_function_flow_graph(&skeleton);
    let plan = ReturnPathPeeker::new(&graph)
        .plan(
            &SliceDemand::for_return_projection(&skeleton, &[Arc::from("b")]),
            &FlowSliceBudget::default(),
        )
        .unwrap();
    assert!(skeleton
        .expr_sites
        .iter()
        .flat_map(|site| site.reads.iter())
        .any(
            |read| matches!(read.binding, Some(FlowBindingRef::Captured(_)))
                && !read.path.is_empty()
        ));
    for (literal, selected) in [("'wanted'", true), ("'sibling'", false)] {
        let start = source.find(literal).unwrap() as u32;
        let span = FrameSpan::rebase(
            source.find("() =>").unwrap() as u32,
            verter_span::Span::new(start, start + literal.len() as u32),
        );
        let site = skeleton
            .expr_sites
            .iter()
            .position(|site| site.span == span)
            .unwrap();
        assert_eq!(
            plan.is_value(graph.expr_site_node(SkeletonExprSiteId::from_index(site as u32))),
            selected,
            "{literal}"
        );
    }
}

#[test]
fn hoisted_runtime_aliases_keep_access_edges_linear() {
    use crate::analysis::flow::peeker::{FlowSliceBudget, ReturnPathPeeker, SliceDemand};
    let edges_for = |count| {
        let mut source = String::from("function f(value) {");
        for _ in 0..count {
            source.push_str("var value;");
        }
        for ordinal in 0..count {
            source.push_str(&format!("value={ordinal}; const read{ordinal}=value;"));
        }
        source.push_str("return value; }");
        let skeleton = skeleton_of(&source);
        let graph = build_function_flow_graph(&skeleton);
        let plan = ReturnPathPeeker::new(&graph)
            .plan(
                &SliceDemand::for_return_projection(&skeleton, &[]),
                &FlowSliceBudget::default(),
            )
            .unwrap();
        let name = skeleton.name_id("value").unwrap();
        let declarations: Vec<_> = skeleton.bindings_named(name).collect();
        assert_eq!(declarations.len(), count + 1);
        for declaration in declarations {
            assert!(
                plan.is_value(graph.binding_node(declaration)),
                "each exact authored declaration remains selected evidence"
            );
        }
        graph.edges().len()
    };
    let small = edges_for(32);
    let large = edges_for(64);
    assert!(
        large <= small * 2 + 8,
        "runtime alias access edges must be linear: {small} -> {large}"
    );
}

#[test]
fn captured_binding_hubs_keep_many_reads_and_writes_linear() {
    let graph_for = |count| {
        let mut source = String::from("function root(x) { return () => {");
        for ordinal in 0..count {
            source.push_str(&format!("let value{ordinal} = x; x = {ordinal};"));
        }
        source.push_str("return [");
        for ordinal in 0..count {
            source.push_str(&format!("value{ordinal},"));
        }
        source.push_str("]; }; }");
        let skeleton = indexed_returned_arrow(&source);
        let graph = build_function_flow_graph(&skeleton);
        let captured: Vec<_> = (0..graph.node_count())
            .filter_map(|ordinal| {
                let node = FlowNodeId::from_index(ordinal as u32);
                match graph.node_kind(node) {
                    FlowNodeKind::CapturedBinding(id) => Some((node, id)),
                    _ => None,
                }
            })
            .collect();
        assert_eq!(
            captured.len(),
            1,
            "all occurrences share one exact captured variable"
        );
        assert_eq!(graph.captured_binding(captured[0].1).name.as_ref(), "x");
        assert_eq!(graph.captured_binding_node(captured[0].1), captured[0].0);
        assert_eq!(
            skeleton.bindings.len(),
            count,
            "the captured variable is not a fabricated local declaration"
        );
        graph.edges().len()
    };
    let small = graph_for(32);
    let large = graph_for(64);
    assert!(
        large <= small * 2 + 8,
        "captured access edges grow linearly: {small} -> {large}"
    );
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
    let FlowEdgeKind::PathWrite {
        path, certainty, ..
    } = &path_writes[0].kind
    else {
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

/// Lexical binding identity (defect D): a read resolves to the binding of
/// its NEAREST enclosing region — a shadowed same-named OUTER binding gets
/// NO read edge, so an irrelevant outer initializer can never enter a
/// demand slice through name-keyed fan-out. A hoisted `var` declared in a
/// non-enclosing region still resolves (the conservative hoisting
/// fallback).
#[test]
fn flow_graph_read_edges_resolve_nearest_enclosing_binding() {
    let skeleton =
        skeleton_of("function fD(notFn: number) { const x = notFn(); { const x = 1; return x; } }");
    let graph = build_function_flow_graph(&skeleton);

    let x_name = skeleton.name_id("x").expect("x interned");
    let bindings: Vec<SkeletonBindingId> = skeleton.bindings_named(x_name).collect();
    assert_eq!(bindings.len(), 2, "two shadowing x bindings");
    // Declaration order: the outer `const x = notFn()` first, the inner
    // block's `const x = 1` second.
    let (outer, inner) = (bindings[0], bindings[1]);
    assert_ne!(
        skeleton.binding(outer).region,
        skeleton.binding(inner).region,
        "the fixture shadows across regions"
    );

    // The return argument's read site: `return x` inside the block.
    let read_site = skeleton.return_sites[0].argument.expect("argument");
    let read_node = graph.expr_site_node(read_site);
    assert!(
        has_edge_class(
            &graph,
            read_node,
            graph.binding_node(inner),
            FlowEdgeClass::ValueDef
        ),
        "the inner-block read binds the inner (nearest-enclosing) x"
    );
    assert!(
        !has_edge_class(
            &graph,
            read_node,
            graph.binding_node(outer),
            FlowEdgeClass::ValueDef
        ),
        "the shadowed outer x gets NO read edge — the defect-D fan-out"
    );

    // Hoisting fallback: a `var` declared in a sibling block still serves
    // a root-region read (the enclosing chain misses; the conservative
    // all-same-name fallback keeps the hoisted binding reachable).
    let hoisted = skeleton_of("function fh() { { var y = 1; } return y; }");
    let hoisted_graph = build_function_flow_graph(&hoisted);
    let y_name = hoisted.name_id("y").expect("y interned");
    let y_binding = hoisted.bindings_named(y_name).next().expect("y binds once");
    let hoisted_read = hoisted.return_sites[0].argument.expect("argument");
    assert!(
        has_edge_class(
            &hoisted_graph,
            hoisted_graph.expr_site_node(hoisted_read),
            hoisted_graph.binding_node(y_binding),
            FlowEdgeClass::ValueDef
        ),
        "a hoisted var in a non-enclosing region stays reachable"
    );
}

/// R1 — the write loop emits BOTH effect directions: `site → hub`
/// (evaluating the site affects the slot) AND `hub → write site` (a
/// slice selecting the hub must select the write site's evaluation).
/// The planner's effect frontier walks out-edges only, so without the
/// reverse edge a standalone preceding write (`x = "s"; return x`) is
/// never selected and its effect obligation silently drops at lowering.
#[test]
fn flow_graph_write_hub_carries_reverse_effect_edge_to_write_site() {
    let skeleton = skeleton_of(r#"function r1(x: string | number) { x = "s"; return x; }"#);
    let graph = build_function_flow_graph(&skeleton);
    let x_hub = graph.binding_node(single_binding(&skeleton, "x"));
    let write_site = graph.expr_site_node(skeleton.writes[0].site);
    assert!(
        has_edge_class(&graph, write_site, x_hub, FlowEdgeClass::EvalEffect),
        "the forward site → hub effect edge stays"
    );
    assert!(
        has_edge_class(&graph, x_hub, write_site, FlowEdgeClass::EvalEffect),
        "the hub carries the reverse effect edge to its write site — the \
         effect frontier can only reach the site through out-edges"
    );
}

/// R4 — the all-same-name fallback is the HOISTING arm only: `var` and
/// nested function declarations hoist past non-enclosing regions;
/// block-scoped `let` / `const` / class / catch-param bindings never
/// do. A root-region read of `q` whose only same-name binding is a
/// sibling-block `let q` gets NO binding edge (the name resolves as a
/// free file-scope name instead of value-selecting the block hub).
#[test]
fn flow_graph_fallback_binds_hoisting_kinds_only() {
    let skeleton = skeleton_of("function fq() { { let q = 0; } return q; }");
    let graph = build_function_flow_graph(&skeleton);
    let q_name = skeleton.name_id("q").expect("q interned");
    let q_binding = skeleton.bindings_named(q_name).next().expect("q binds");
    assert_eq!(
        skeleton.binding(q_binding).kind,
        crate::analysis::flow::SkeletonBindingKind::Let
    );
    let read_site = skeleton.return_sites[0].argument.expect("argument");
    assert!(
        !has_edge_class(
            &graph,
            graph.expr_site_node(read_site),
            graph.binding_node(q_binding),
            FlowEdgeClass::ValueDef
        ),
        "a block-scoped `let` never enters the hoisting fallback"
    );
}

/// The function-scope frame is the parameters PLUS every hoisting-kind
/// binding of the name, wherever it is written: a `var` REDECLARES a
/// same-named parameter (they share one slot), so a root-region read
/// reaches BOTH the parameter and a `var` declared in a non-enclosing
/// block. Resolving to the parameter alone left the block declarator
/// unselected, so its value never entered the slice and the parameter's
/// declared type published as the reaching definition.
#[test]
fn flow_graph_root_read_unions_parameter_with_hoisted_var_redeclaration() {
    let skeleton = skeleton_of("function fx(x: string | number) { { var x = \"s\"; } return x; }");
    let graph = build_function_flow_graph(&skeleton);
    let x_name = skeleton.name_id("x").expect("x interned");
    let bindings: Vec<SkeletonBindingId> = skeleton.bindings_named(x_name).collect();
    assert_eq!(bindings.len(), 2, "the parameter and the `var` both bind x");
    let read_site = skeleton.return_sites[0].argument.expect("argument");
    use crate::analysis::flow::peeker::{FlowSliceBudget, ReturnPathPeeker, SliceDemand};
    let plan = ReturnPathPeeker::new(&graph)
        .plan(
            &SliceDemand::for_return_projection(&skeleton, &[]),
            &FlowSliceBudget::default(),
        )
        .unwrap();
    assert_eq!(
        out_edges_of_class(
            &graph,
            graph.expr_site_node(read_site),
            FlowEdgeClass::ValueDef
        )
        .len(),
        1,
        "one read names one runtime variable"
    );
    for binding in bindings {
        assert!(
            plan.is_value(graph.binding_node(binding)),
            "the parameter and authored var declaration both retain their evidence"
        );
        if let Some(initializer) = skeleton.binding(binding).initializer {
            assert!(
                plan.is_value(graph.expr_site_node(initializer)),
                "the hoisted var's definition remains a value dependency"
            );
        }
    }
    // The block-scoped shadowing rail is untouched: an inner `let` of a
    // DIFFERENT name in a sibling block still resolves exactly.
    let shadowed = skeleton_of("function fy(y: number) { { const y = 1; return y; } }");
    let shadow_graph = build_function_flow_graph(&shadowed);
    let y_name = shadowed.name_id("y").expect("y interned");
    let inner = shadowed
        .bindings_named(y_name)
        .find(|binding| {
            shadowed.binding(*binding).kind == crate::analysis::flow::SkeletonBindingKind::Const
        })
        .expect("the inner const binds");
    let param = shadowed
        .bindings_named(y_name)
        .find(|binding| {
            shadowed.binding(*binding).kind == crate::analysis::flow::SkeletonBindingKind::Param
        })
        .expect("the parameter binds");
    let inner_read = shadowed.return_sites[0].argument.expect("argument");
    assert!(
        has_edge_class(
            &shadow_graph,
            shadow_graph.expr_site_node(inner_read),
            shadow_graph.binding_node(inner),
            FlowEdgeClass::ValueDef
        ),
        "the block read binds its own `const`"
    );
    assert!(
        !has_edge_class(
            &shadow_graph,
            shadow_graph.expr_site_node(inner_read),
            shadow_graph.binding_node(param),
            FlowEdgeClass::ValueDef
        ),
        "a block-scoped declaration still shadows the parameter exactly — \
         the union applies to the FUNCTION-scope frame only"
    );
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
    for node in graph.nodes() {
        for edge in graph.out_edges(node) {
            assert_eq!(edge.from, node);
            total += 1;
        }
    }
    assert_eq!(total, graph.edges().len());
    assert!(total > 0, "the fixture produces edges");
}

#[test]
fn flow_graph_enumerates_every_node_family_and_empty_graphs() {
    let populated = skeleton_of(
        "function enumerate(x: number) { const y = x + 1; if (x) { return y; } return x; }",
    );
    let empty = FunctionBodySkeleton {
        names: Arc::from([]),
        regions: Arc::from([]),
        bindings: Arc::from([]),
        expr_sites: Arc::from([]),
        return_sites: Arc::from([]),
        writes: Arc::from([]),
        nested_function_sites: Arc::default(),
    };
    let captured = indexed_returned_arrow("function root() { const x = 1; return () => x; }");
    for (skeleton, captures) in [(populated, 0), (empty, 0), (captured, 1)] {
        let graph = build_function_flow_graph(&skeleton);
        let mut nodes = graph.nodes();
        let expected = [
            skeleton.bindings.len(),
            skeleton.expr_sites.len(),
            skeleton.return_sites.len(),
            skeleton.regions.len(),
            captures,
        ];
        let total: usize = expected.iter().sum();
        assert_eq!(nodes.len(), total);
        let mut families = [0usize; 5];
        for index in 0..total {
            let node = nodes.next().expect("every structural node is enumerated");
            assert_eq!(node.index(), index, "enumeration is unique and ordered");
            assert_eq!(nodes.len(), total - index - 1);
            let reminted = match graph.node_kind(node) {
                FlowNodeKind::Binding(id) => {
                    families[0] += 1;
                    graph.binding_node(id)
                }
                FlowNodeKind::ExprSite(id) => {
                    families[1] += 1;
                    graph.expr_site_node(id)
                }
                FlowNodeKind::ReturnSite(id) => {
                    families[2] += 1;
                    graph.return_site_node(id)
                }
                FlowNodeKind::Region(id) => {
                    families[3] += 1;
                    graph.region_node(id)
                }
                FlowNodeKind::CapturedBinding(id) => {
                    families[4] += 1;
                    assert_eq!(graph.captured_binding(id).name.as_ref(), "x");
                    graph.captured_binding_node(id)
                }
            };
            assert_eq!(node, reminted);
            assert!(graph.out_edges(node).iter().all(|edge| edge.from == node));
        }
        assert_eq!(families, expected);
        assert_eq!(nodes.next(), None);
    }
}
