//! @ai-generated - `FunctionBodySkeleton` structural-index discrimination
//! tests: binding / return / region / write indexing, object-literal
//! footprint path-precision, arena-freedom, and per-content-version
//! determinism.

use super::*;

fn parse_and_build<T>(
    source: &str,
    select: impl for<'a, 'ast> Fn(&'a oxc_ast::ast::Program<'ast>) -> FunctionBodySource<'a, 'ast>,
    check: impl Fn(FunctionBodySkeleton) -> T,
) -> T {
    let allocator = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::ts();
    let ret = oxc_parser::Parser::new(&allocator, source, source_type).parse();
    assert!(
        ret.errors.is_empty(),
        "fixture must parse: {:?}",
        ret.errors
    );
    let body_source = select(&ret.program);
    check(build_function_body_skeleton(&body_source))
}

fn first_function<'a, 'ast>(
    program: &'a oxc_ast::ast::Program<'ast>,
) -> FunctionBodySource<'a, 'ast> {
    for statement in &program.body {
        if let Statement::FunctionDeclaration(function) = statement {
            if let Some(source) = FunctionBodySource::from_function(function) {
                return source;
            }
        }
    }
    panic!("fixture must contain a bodied function declaration");
}

fn first_arrow<'a, 'ast>(program: &'a oxc_ast::ast::Program<'ast>) -> FunctionBodySource<'a, 'ast> {
    for statement in &program.body {
        if let Statement::VariableDeclaration(declaration) = statement {
            for declarator in &declaration.declarations {
                if let Some(Expression::ArrowFunctionExpression(arrow)) = declarator.init.as_ref() {
                    return FunctionBodySource::from_arrow(arrow);
                }
            }
        }
    }
    panic!("fixture must contain an arrow initializer");
}

fn skeleton_of(source: &str) -> FunctionBodySkeleton {
    parse_and_build(source, first_function, |skeleton| skeleton)
}

pub(super) fn indexed_skeleton_of(source: &str) -> FunctionBodySkeleton {
    indexed_structure_of(source).into_parts().0
}

fn indexed_structure_of(source: &str) -> PreparedFunctionBodySkeleton {
    use crate::analysis::function_program::build_function_program_index;
    use crate::analysis::top_level_owners::TopLevelOwnerTable;
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
    assert!(parsed.errors.is_empty());
    let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
    let index =
        build_function_program_index(&parsed.program, source, &owners, Arc::from("/graph.ts"));
    let function = parsed
        .program
        .body
        .iter()
        .find_map(|statement| match statement {
            Statement::FunctionDeclaration(function) if function.body.is_some() => Some(function),
            _ => None,
        })
        .expect("fixture function");
    let entry = index
        .matches_named(function.id.as_ref().unwrap().name.as_str())
        .next()
        .unwrap()
        .entry();
    build_indexed_function_body_skeleton(
        &FunctionBodySource::from_function(function).unwrap(),
        entry,
    )
    .unwrap()
}

#[test]
fn source_type_queries_retain_separate_exact_lexical_occurrences() {
    let source = "function f<T>(arg=accept(0 as typeof later)) { let later=1; { let twin=1; accept(0 as typeof twin); } { let twin=2; accept(0 as typeof twin); } accept(0 as typeof T); return accept(0 as typeof outside); }";
    let prepared = indexed_structure_of(source);
    let bindings = prepared.bindings();
    let queries: Vec<_> = prepared
        .skeleton()
        .expr_sites
        .iter()
        .flat_map(|site| site.source_type_queries.iter())
        .collect();
    assert_eq!(queries.len(), 5);
    let mut twins = Vec::new();
    for query in queries {
        assert_eq!(
            bindings.occurrence(query.span),
            FlowBindingOccurrence::Missing,
            "query is not a runtime read"
        );
        match bindings.source_type_query_occurrence(query.span) {
            FlowBindingOccurrence::Resolved(FlowBindingRef::Local(binding)) => twins.push(*binding),
            FlowBindingOccurrence::Free => assert!(query.binding.is_none()),
            other => panic!("unexpected query disposition {other:?}"),
        }
    }
    assert_eq!(
        twins.len(),
        2,
        "default sees outer environment; type binder has no value identity"
    );
    assert_ne!(
        twins[0], twins[1],
        "same-name sibling scopes remain distinct"
    );
    assert_eq!(
        bindings.source_type_query_occurrence(FrameSpan::rebase(0, verter_span::Span::new(0, 1))),
        FlowBindingOccurrence::Missing
    );
}

#[test]
// @ai-generated - Calls without an enclosing expression site retain source-only query bindings.
fn enum_initializer_call_retains_source_type_query_on_its_call_site() {
    let source = "function flow(queried: unknown, other: unknown) { enum Local { Value = accept(other as typeof queried), } return Local.Value; }";
    let prepared = indexed_structure_of(source);
    let skeleton = prepared.skeleton();
    let call_site = skeleton
        .expr_sites
        .iter()
        .find(|site| !site.calls.is_empty())
        .expect("enum initializer call is indexed");
    assert_eq!(call_site.calls.len(), 1);
    assert_eq!(call_site.source_type_queries.len(), 1);
    let query = &call_site.source_type_queries[0];
    assert!(matches!(query.binding, Some(FlowBindingRef::Local(_))));
    assert_eq!(
        prepared.bindings().source_type_query_occurrence(query.span),
        FlowBindingOccurrence::Resolved(query.binding.as_ref().unwrap())
    );
    assert_eq!(
        prepared.bindings().occurrence(query.span),
        FlowBindingOccurrence::Missing,
        "a source type query is not a runtime read"
    );
    assert_eq!(
        skeleton
            .expr_sites
            .iter()
            .map(|site| site.source_type_queries.len())
            .sum::<usize>(),
        1,
        "the query belongs to the existing call site exactly once"
    );
}

#[test]
fn source_type_query_inventory_and_observer_share_wrapper_precedence() {
    for (input, count) in [
        ("other as typeof queried", 1),
        ("(((other as typeof queried)))", 1),
        ("<const>((other as typeof queried) satisfies unknown)", 1),
        ("(other as typeof queried) satisfies unknown", 0),
        ("(other as typeof queried) as const", 0),
        ("other as (typeof queried)", 0),
        ("other as typeof queried.member", 0),
        ("other as typeof queried<string>", 0),
        ("other as { value: typeof queried }", 0),
    ] {
        let source = format!("function f(other, queried) {{ return accept({input}); }}");
        let prepared = indexed_structure_of(&source);
        assert_eq!(
            prepared
                .skeleton()
                .expr_sites
                .iter()
                .map(|site| site.source_type_queries.len())
                .sum::<usize>(),
            count,
            "{source}"
        );
    }
}

#[test]
fn skeleton_records_whole_declarator_annotation_presence_without_lowering() {
    let source = "function f() { let marked: Widget; var plain = 1; let {part}: Shape = other; return marked; }";
    let skeleton = skeleton_of(source);
    let start = source.find(": Widget").unwrap() as u32;
    assert_eq!(
        binding(&skeleton, "marked").annotation_span,
        Some(FrameSpan::rebase(
            0,
            verter_span::Span::new(start, start + 8)
        ))
    );
    assert!(binding(&skeleton, "plain").annotation_span.is_none());
    assert!(
        binding(&skeleton, "part").destructured,
        "pattern captures cannot claim whole-declarator authority"
    );
}

#[test]
fn indexed_skeleton_retains_exact_nested_capture_paths_and_runtime_aliases() {
    use crate::analysis::function_program::build_function_program_index;
    use crate::analysis::top_level_owners::TopLevelOwnerTable;
    let source =
        "function f(x) { {var x;} let unused = 0; { let x = {a: 1}; return () => () => x.a; } }";
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
    assert!(parsed.errors.is_empty());
    let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
    let index =
        build_function_program_index(&parsed.program, source, &owners, Arc::from("/capture.ts"));
    let entry = index.matches_named("f").next().unwrap().entry();
    let prepared =
        build_indexed_function_body_skeleton(&first_function(&parsed.program), entry).unwrap();
    let skeleton = &prepared.skeleton;
    let xs: Vec<_> = skeleton
        .bindings_named(skeleton.name_id("x").unwrap())
        .collect();
    assert_eq!(xs.len(), 3);
    let exact = prepared.bindings.identity(xs[0]).unwrap();
    assert_eq!(prepared.bindings.local(exact), Some(xs[0]));
    let mut stale_name = exact.clone();
    stale_name.name = Arc::from("replaced");
    let mut stale_kind = exact.clone();
    stale_kind.kind = crate::analysis::function_program::FunctionBindingKind::Let;
    for stale in [&stale_name, &stale_kind] {
        assert_eq!(stale, exact, "display metadata is not semantic identity");
        assert_eq!(
            prepared.bindings.local(stale),
            None,
            "map admission still requires exact indexed metadata"
        );
    }
    assert_ne!(
        prepared.bindings.identity(xs[0]),
        prepared.bindings.identity(xs[1])
    );
    assert_eq!(
        skeleton.binding(xs[0]).runtime_binding,
        skeleton.binding(xs[1]).runtime_binding
    );
    assert_ne!(
        skeleton.binding(xs[0]).runtime_binding,
        skeleton.binding(xs[2]).runtime_binding
    );
    let closure = skeleton
        .expr_sites
        .iter()
        .find(|site| !site.capture_bindings.is_empty())
        .unwrap();
    assert_eq!(
        closure.capture_bindings.as_ref(),
        &[FlowBindingRef::Local(xs[2])]
    );
    assert_eq!(closure.reads.len(), 1);
    assert_eq!(closure.reads[0].binding, Some(FlowBindingRef::Local(xs[2])));
    assert_eq!(
        closure.reads[0].path.as_ref(),
        &[SkeletonPathSegment::Static(skeleton.name_id("a").unwrap())]
    );
    let start = source.rfind("x.a").unwrap() as u32;
    assert_eq!(
        closure.reads[0].span,
        FrameSpan::rebase(0, verter_span::Span::new(start, start + 1))
    );
    assert_eq!(
        skeleton.return_sites.len(),
        1,
        "nested frame bodies are never walked"
    );
}

#[test]
fn prepared_graph_does_not_resolve_free_parameter_inputs_as_body_bindings() {
    use crate::analysis::function_program::build_function_program_index;
    use crate::analysis::top_level_owners::TopLevelOwnerTable;
    for source in [
        "const seed = 'outer'; function f(arg = seed) { const seed = 1; return arg; }",
        "const seed = 'outer'; function f(arg = seed) { var seed = 1; return arg; }",
        "const seed = 'outer'; function f(arg = seed()) { const seed = 1; return arg; }",
        "const seed = 'outer'; function f(arg = seed()) { var seed = 1; return arg; }",
    ] {
        let allocator = oxc_allocator::Allocator::default();
        let parsed =
            oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
        assert!(parsed.errors.is_empty());
        let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
        let index = build_function_program_index(
            &parsed.program,
            source,
            &owners,
            Arc::from("/default.ts"),
        );
        let entry = index.matches_named("f").next().unwrap().entry();
        let reference = entry
            .references
            .iter()
            .find(|read| read.name.as_ref() == "seed")
            .unwrap_or_else(|| panic!("default read is indexed: {:?}", entry.references));
        assert!(
            reference.binding == crate::analysis::function_program::FunctionReferenceBinding::Free,
            "the default sees the outer environment"
        );
        let prepared =
            build_indexed_function_body_skeleton(&first_function(&parsed.program), entry).unwrap();
        let skeleton = &prepared.skeleton;
        let seed = skeleton
            .bindings_named(skeleton.name_id("seed").unwrap())
            .next()
            .unwrap();
        let graph = flow_graph::build_function_flow_graph(&prepared);
        let plan = peeker::ReturnPathPeeker::new(&graph)
            .plan(
                &peeker::SliceDemand::for_return_projection(skeleton, &[]),
                &peeker::FlowSliceBudget::default(),
            )
            .unwrap();
        assert!(
            !plan.is_selected(graph.binding_node(seed)),
            "known-free input must not bind a same-name body declaration"
        );
    }
}

#[test]
fn indexed_skeleton_retains_write_only_capture_subjects_without_value_reads() {
    use crate::analysis::function_program::build_function_program_index;
    use crate::analysis::top_level_owners::TopLevelOwnerTable;
    for (source, captures_outer) in [
        ("function f(x) { return () => { x = 1; return 0; }; }", true),
        (
            "function f(x) { return () => () => { x = 1; return 0; }; }",
            true,
        ),
        (
            "function f(x) { return () => { let x = 0; return () => { x = 1; return 0; }; }; }",
            false,
        ),
    ] {
        let allocator = oxc_allocator::Allocator::default();
        let parsed =
            oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
        assert!(parsed.errors.is_empty());
        let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
        let index = build_function_program_index(
            &parsed.program,
            source,
            &owners,
            Arc::from("/capture.ts"),
        );
        let entry = index.matches_named("f").next().unwrap().entry();
        let prepared =
            build_indexed_function_body_skeleton(&first_function(&parsed.program), entry).unwrap();
        let skeleton = &prepared.skeleton;
        let x = skeleton
            .bindings_named(skeleton.name_id("x").unwrap())
            .next()
            .unwrap();
        let closure = skeleton
            .expr_sites
            .iter()
            .find(|site| !site.capture_bindings.is_empty());
        if !captures_outer {
            assert!(
                closure.is_none(),
                "an intervening local owns its descendant write"
            );
            continue;
        }
        let closure = closure.expect("a write-only capture remains an exact closure subject");
        assert_eq!(
            closure.capture_bindings.as_ref(),
            &[FlowBindingRef::Local(x)]
        );
        assert!(
            closure.reads.is_empty(),
            "writing a cell is not reading its value"
        );
        let graph = flow_graph::build_function_flow_graph(&prepared);
        let plan = peeker::ReturnPathPeeker::new(&graph)
            .plan(
                &peeker::SliceDemand::for_return_projection(skeleton, &[]),
                &peeker::FlowSliceBudget::default(),
            )
            .unwrap();
        assert!(plan.is_effect_only(graph.binding_node(x)));
    }
}

fn binding<'a>(skeleton: &'a FunctionBodySkeleton, name: &str) -> &'a SkeletonBinding {
    let id = skeleton
        .name_id(name)
        .unwrap_or_else(|| panic!("name `{name}` must be interned"));
    let binding_id = skeleton
        .bindings_named(id)
        .next()
        .unwrap_or_else(|| panic!("`{name}` must be bound"));
    skeleton.binding(binding_id)
}

fn has_binding(skeleton: &FunctionBodySkeleton, name: &str) -> bool {
    skeleton
        .name_id(name)
        .is_some_and(|id| skeleton.bindings_named(id).next().is_some())
}

fn writes_of<'a>(skeleton: &'a FunctionBodySkeleton, name: &str) -> Vec<&'a SkeletonWrite> {
    let Some(id) = skeleton.name_id(name) else {
        return Vec::new();
    };
    skeleton
        .writes
        .iter()
        .filter(|write| write.target == SkeletonWriteTarget::Named(id))
        .collect()
}

fn region_chain_kinds(
    skeleton: &FunctionBodySkeleton,
    mut region: SkeletonRegionId,
) -> Vec<SkeletonRegionKind> {
    let mut kinds = vec![skeleton.region(region).kind];
    while let Some(parent) = skeleton.region(region).parent {
        kinds.push(skeleton.region(parent).kind);
        region = parent;
    }
    kinds
}

fn object_entries(
    skeleton: &FunctionBodySkeleton,
    site: SkeletonExprSiteId,
) -> Vec<SkeletonObjectEntry> {
    match &skeleton.expr_site(site).shape {
        SkeletonExprShape::ObjectLiteral { entries } => entries.to_vec(),
        SkeletonExprShape::BranchJoin { .. } | SkeletonExprShape::Other => {
            panic!("site must be an object literal")
        }
    }
}

fn site_reads_name(skeleton: &FunctionBodySkeleton, site: SkeletonExprSiteId, name: &str) -> bool {
    let Some(id) = skeleton.name_id(name) else {
        return false;
    };
    skeleton
        .expr_site(site)
        .reads
        .iter()
        .any(|read| read.name == id)
}

#[test]
fn skeleton_indexes_bindings_returns_regions_and_writes() {
    let skeleton = skeleton_of(
        r#"
function f(p: number, { q }: { q: string }) {
  const c = p + 1;
  let l;
  var v = 2;
  function nested() { const hidden = 3; return hidden; }
  class K {}
  if (p) { return c; } else { l = p; }
  while (p) { v++; }
  l ||= 5;
  return { c };
}
"#,
    );

    // Lexical binding index.
    assert_eq!(binding(&skeleton, "p").kind, SkeletonBindingKind::Param);
    assert_eq!(binding(&skeleton, "q").kind, SkeletonBindingKind::Param);
    assert_eq!(binding(&skeleton, "c").kind, SkeletonBindingKind::Const);
    assert_eq!(binding(&skeleton, "l").kind, SkeletonBindingKind::Let);
    assert_eq!(binding(&skeleton, "v").kind, SkeletonBindingKind::Var);
    assert_eq!(
        binding(&skeleton, "nested").kind,
        SkeletonBindingKind::NestedFunction
    );
    assert_eq!(binding(&skeleton, "K").kind, SkeletonBindingKind::Class);
    // Nested function bodies are their own frames: nothing from them.
    assert!(!has_binding(&skeleton, "hidden"));

    // Declarator initializers.
    assert!(binding(&skeleton, "c").initializer.is_some());
    assert!(binding(&skeleton, "l").initializer.is_none());
    assert!(binding(&skeleton, "v").initializer.is_some());

    // Return-site index: the arm return + the trailing return; the nested
    // function's return never contributes.
    assert_eq!(skeleton.return_sites.len(), 2);
    let arm_return = &skeleton.return_sites[0];
    let chain = region_chain_kinds(&skeleton, arm_return.region);
    assert!(chain.contains(&SkeletonRegionKind::IfConsequent));
    assert_eq!(
        *chain.last().expect("chain reaches the root"),
        SkeletonRegionKind::FunctionBody
    );
    let trailing = &skeleton.return_sites[1];
    assert_eq!(
        skeleton.region(trailing.region).kind,
        SkeletonRegionKind::FunctionBody
    );

    // has_return marks exactly the regions a return sits in.
    let consequent = skeleton
        .regions
        .iter()
        .find(|region| region.kind == SkeletonRegionKind::IfConsequent)
        .expect("consequent region");
    assert!(consequent.has_return);
    let alternate = skeleton
        .regions
        .iter()
        .find(|region| region.kind == SkeletonRegionKind::IfAlternate)
        .expect("alternate region");
    assert!(!alternate.has_return);
    let loop_region = skeleton
        .regions
        .iter()
        .find(|region| region.kind == SkeletonRegionKind::Loop)
        .expect("loop region");
    assert!(!loop_region.has_return);
    assert!(loop_region.control_input.is_some());

    // Assignment / kill summary.
    let l_writes = writes_of(&skeleton, "l");
    assert_eq!(l_writes.len(), 2);
    assert!(l_writes[0].path.is_empty());
    assert_eq!(l_writes[0].certainty, SkeletonWriteCertainty::Definite);
    assert!(l_writes[0].value.is_some());
    assert_eq!(l_writes[1].certainty, SkeletonWriteCertainty::Optional);
    let v_writes = writes_of(&skeleton, "v");
    assert_eq!(v_writes.len(), 1);
    assert!(
        v_writes[0].value.is_none(),
        "update writes have no value site"
    );
    // No write escaped from the nested frame.
    assert!(writes_of(&skeleton, "hidden").is_empty());
}

#[test]
fn skeleton_member_writes_carry_projection_paths() {
    let skeleton = skeleton_of(
        r#"
function f(obj: { a: { b: number }; c: number }, key: string) {
  obj.a.b = 1;
  obj[key] = 2;
  return obj;
}
"#,
    );
    let writes = writes_of(&skeleton, "obj");
    assert_eq!(writes.len(), 2);
    let a_name = skeleton.name_id("a").expect("a interned");
    let b_name = skeleton.name_id("b").expect("b interned");
    assert_eq!(
        writes[0].path.as_ref(),
        &[
            SkeletonPathSegment::Static(a_name),
            SkeletonPathSegment::Static(b_name)
        ]
    );
    assert_eq!(writes[0].certainty, SkeletonWriteCertainty::Definite);
    assert_eq!(writes[1].path.as_ref(), &[SkeletonPathSegment::Computed]);
}

#[test]
fn skeleton_object_literal_footprint_is_path_precise() {
    let skeleton = skeleton_of(
        r#"
function g(x: string, rest: object, k: () => string) {
  return { a: (x = "s"), b: x.toUpperCase(), ...rest, [k()]: 1, m() { return 99; } };
}
"#,
    );

    // One return site whose argument is the object literal; the method
    // body's return does not contribute.
    assert_eq!(skeleton.return_sites.len(), 1);
    let object_site = skeleton.return_sites[0]
        .argument
        .expect("return has an argument");
    let entries = object_entries(&skeleton, object_site);
    assert_eq!(entries.len(), 5);

    // The object site's OWN footprint is empty — reads / writes / calls
    // attribute to the child sites (path precision).
    let object = skeleton.expr_site(object_site);
    assert!(object.reads.is_empty());
    assert!(object.calls.is_empty());
    assert!(
        skeleton
            .writes
            .iter()
            .all(|write| write.site != object_site),
        "no write attributes to the object container site"
    );

    let a_name = skeleton.name_id("a").expect("a interned");
    let b_name = skeleton.name_id("b").expect("b interned");
    let SkeletonObjectEntry::Property {
        key: SkeletonObjectKey::Static(key_a),
        value: a_value,
        kind: SkeletonPropertyKind::Init,
    } = entries[0]
    else {
        panic!("entry 0 is the static `a` init property");
    };
    assert_eq!(key_a, a_name);
    // `a`'s value site carries the `x` write; the write's value is a child
    // site of `a`'s value site.
    let x_writes = writes_of(&skeleton, "x");
    assert_eq!(x_writes.len(), 1);
    assert_eq!(x_writes[0].site, a_value);
    let rhs = x_writes[0].value.expect("assignment has a value site");
    assert_eq!(skeleton.expr_site(rhs).parent, Some(a_value));

    let SkeletonObjectEntry::Property {
        key: SkeletonObjectKey::Static(key_b),
        value: b_value,
        kind: SkeletonPropertyKind::Init,
    } = entries[1]
    else {
        panic!("entry 1 is the static `b` init property");
    };
    assert_eq!(key_b, b_name);
    assert!(site_reads_name(&skeleton, b_value, "x"));
    let b_site = skeleton.expr_site(b_value);
    assert_eq!(b_site.calls.len(), 1);
    let SkeletonCallee::Path(path) = &b_site.calls[0].callee else {
        panic!("`x.toUpperCase()` is a path callee");
    };
    assert_eq!(path.first().copied(), skeleton.name_id("x"));

    let SkeletonObjectEntry::Spread { source } = entries[2] else {
        panic!("entry 2 is the spread");
    };
    assert!(site_reads_name(&skeleton, source, "rest"));

    let SkeletonObjectEntry::Property {
        key: SkeletonObjectKey::Computed(key_site),
        kind: SkeletonPropertyKind::Init,
        ..
    } = entries[3]
    else {
        panic!("entry 3 is the computed-key property");
    };
    let key_footprint = skeleton.expr_site(key_site);
    assert_eq!(key_footprint.calls.len(), 1);
    assert_eq!(
        key_footprint.calls[0].callee,
        SkeletonCallee::Named(skeleton.name_id("k").expect("k interned"))
    );

    let SkeletonObjectEntry::Property {
        key: SkeletonObjectKey::Static(key_m),
        value: m_value,
        kind: SkeletonPropertyKind::Method,
    } = entries[4]
    else {
        panic!("entry 4 is the method property");
    };
    assert_eq!(Some(key_m), skeleton.name_id("m"));
    // The method body is its own frame: no footprint, no return site.
    let m_site = skeleton.expr_site(m_value);
    assert!(m_site.reads.is_empty());
    assert!(m_site.calls.is_empty());
}

#[test]
fn skeleton_arrow_expression_body_records_implicit_return() {
    let skeleton = parse_and_build("const h = (y: number) => y + 1;", first_arrow, |skeleton| {
        skeleton
    });
    assert_eq!(skeleton.return_sites.len(), 1);
    let site = &skeleton.return_sites[0];
    assert!(site.implicit);
    let argument = site.argument.expect("implicit return has an argument");
    assert!(site_reads_name(&skeleton, argument, "y"));
    assert_eq!(binding(&skeleton, "y").kind, SkeletonBindingKind::Param);
    assert!(skeleton.regions[0].has_return);
}

#[test]
fn skeleton_type_positions_are_not_value_footprint() {
    let skeleton = skeleton_of(
        r#"
function t(v: SomeTypeName) {
  const w = v as OtherTypeName;
  return w satisfies ThirdTypeName;
}
"#,
    );
    // Type names never enter the read footprint or the name-driven write /
    // call surface — they are type positions, not value reads.
    for type_name in ["SomeTypeName", "OtherTypeName", "ThirdTypeName"] {
        let read = skeleton.name_id(type_name).is_some_and(|id| {
            skeleton
                .expr_sites
                .iter()
                .any(|site| site.reads.iter().any(|read| read.name == id))
        });
        assert!(!read, "`{type_name}` must not be a value read");
    }
    // The value reads are still exact.
    assert!(skeleton.name_id("v").is_some());
    assert!(skeleton.name_id("w").is_some());
}

#[test]
fn skeleton_is_arena_free_send_sync_static() {
    fn assert_arena_free<T: Send + Sync + 'static + verter_no_typeexpr::NoTypeExpr>() {}
    assert_arena_free::<FunctionBodySkeleton>();
    assert_arena_free::<SkeletonRegion>();
    assert_arena_free::<SkeletonBinding>();
    assert_arena_free::<SkeletonExprSite>();
    assert_arena_free::<SkeletonReturnSite>();
    assert_arena_free::<SkeletonWrite>();
}

#[test]
fn skeleton_static_member_reads_preserve_the_full_projection_path() {
    let skeleton = skeleton_of(
        r#"
function f(o: { x: number; y: { z: string } }) {
  const x = o.x;
  return o.y.z;
}
"#,
    );
    let o = skeleton.name_id("o").expect("o is interned");
    let x = skeleton.name_id("x").expect("x is interned");
    let y = skeleton.name_id("y").expect("y is interned");
    let z = skeleton.name_id("z").expect("z is interned");
    let paths: Vec<Vec<SkeletonPathSegment>> = skeleton
        .expr_sites
        .iter()
        .flat_map(|site| site.reads.iter())
        .filter(|read| read.name == o)
        .map(|read| read.path.to_vec())
        .collect();
    assert!(paths.contains(&vec![SkeletonPathSegment::Static(x)]));
    assert!(paths.contains(&vec![
        SkeletonPathSegment::Static(y),
        SkeletonPathSegment::Static(z),
    ]));
    assert!(
        !paths.contains(&Vec::new()),
        "a fully static member read must not collapse to its root"
    );
}

#[test]
fn skeleton_build_is_deterministic_per_content_version() {
    let source = r#"
function d(a: number, b: string) {
  let out = { first: a, second: b.length };
  if (a) { out = { first: a + 1, second: 0 }; }
  for (const item of [a]) { out.first = item; }
  return out;
}
"#;
    let first = skeleton_of(source);
    let second = skeleton_of(source);
    assert_eq!(first, second);
}

/// The skeleton is a pure function of the FUNCTION's content: moving the
/// whole function through the file changes NOTHING in it.
///
/// The skeleton is memoized per function content version and reused for
/// any file content that key admits, so an absolute file offset stored
/// anywhere inside it makes the cached artifact depend on something the
/// key cannot see — a blank line above the function is invisible to the
/// key and moves every absolute offset. Whole-artifact equality covers
/// EVERY span-bearing family at once (regions, bindings, expression
/// sites, return sites, writes, and the call footprint), rather than the
/// five a hand-written per-family rebase pass remembered; the read
/// footprint carries no span at all, because a coordinate with no
/// consumer is exactly where a stale one hides.
///
/// Mutation recipe: storing the call footprint's ABSOLUTE span
/// (`FrameSpan::rebase(0, span)` in `SkeletonBuilder::push_call`) flips
/// this and both `lower_tests` anchor rows, and leaves every other
/// skeleton row green.
#[test]
fn skeleton_is_invariant_under_the_function_position() {
    let body = r#"
function d(a: number, b: string) {
  let out = { first: a, second: b.length };
  if (a) { out = { first: a + 1, second: 0 }; }
  for (const item of [a]) { out.first = item; }
  g(a);
  return out;
}
"#;
    let padded = format!("const pad = 0;\nconst pad2 = \"a longer padding statement\";\n{body}");
    assert_eq!(
        skeleton_of(body),
        skeleton_of(&padded),
        "the same function body indexes identically wherever it sits"
    );
}

#[test]
fn prepared_occurrences_distinguish_free_shadowed_and_captured_targets() {
    use crate::analysis::flow::{FlowBindingOccurrence, FlowBindingRef};
    use crate::analysis::function_program::{
        build_function_program_index, resolve_function_node, FunctionNode,
    };
    use crate::analysis::top_level_owners::TopLevelOwnerTable;
    let source = "const seed='global'; function root(arg=seed) { const seed=1; let value=0; { let twin=1; consume(twin); } { let twin=2; consume(twin); } return () => { value=2; return value; }; }";
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
    let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
    let index = build_function_program_index(
        &parsed.program,
        source,
        &owners,
        Arc::from("/occurrences.ts"),
    );
    let entries: Vec<_> = index.matches_named("root").collect();
    for matched in entries {
        let entry = matched.entry();
        let node = resolve_function_node(&parsed.program, &entry.locator)
            .unwrap()
            .node;
        let body = match node {
            FunctionNode::Function(function) => {
                FunctionBodySource::from_function(function).unwrap()
            }
            FunctionNode::Arrow(arrow) => FunctionBodySource::from_arrow(arrow),
        };
        let prepared = build_indexed_function_body_skeleton(&body, entry).unwrap();
        let bindings = prepared.bindings();
        let frame_span = |start: u32, len: u32| {
            FrameSpan::rebase(entry.span.start, verter_span::Span::new(start, start + len))
        };
        assert_eq!(
            bindings.occurrence(frame_span(entry.span.start, 1)),
            FlowBindingOccurrence::Missing
        );
        if entry.lexical_parent.is_none() {
            let default = source.find("arg=seed").unwrap() as u32 + 4;
            assert_eq!(
                bindings.occurrence(frame_span(default, 4)),
                FlowBindingOccurrence::Free,
                "a body-local declaration must not capture a parameter-default read"
            );
            let mut twins = Vec::new();
            for (position, _) in source.match_indices("consume(twin)") {
                let FlowBindingOccurrence::Resolved(FlowBindingRef::Local(binding)) =
                    bindings.occurrence(frame_span(position as u32 + 8, 4))
                else {
                    panic!("exact local occurrence");
                };
                twins.push(*binding);
            }
            assert_eq!(twins.len(), 2);
            assert_ne!(
                twins[0], twins[1],
                "sibling shadows retain distinct exact references"
            );
        } else {
            let write = source.find("value=2").unwrap() as u32;
            let FlowBindingOccurrence::Resolved(FlowBindingRef::Captured(identity)) =
                bindings.occurrence(frame_span(write, 5))
            else {
                panic!("write-only LHS is an indexed captured occurrence");
            };
            assert_eq!(identity.name.as_ref(), "value");
            assert_ne!(identity.defining_function, entry.key);
            let read = source.rfind("return value").unwrap() as u32 + 7;
            assert_eq!(
                bindings.occurrence(frame_span(read, 5)),
                bindings.occurrence(frame_span(write, 5))
            );
        }
    }
}

#[test]
fn prepared_class_occurrences_distinguish_outer_free_and_static_local_bindings() {
    use crate::analysis::function_program::build_function_program_index_with_nodes;
    use crate::analysis::top_level_owners::TopLevelOwnerTable;
    let source = "function f(x) { class C { static { touch(); var touch; { let x; x = 1; } x = 2; try {} catch (caught) { caught = 3; } for (let item of []) { item = 4; } } static { touch(); x = 5; } method() { deferred(); } p = deferredInit(); static p = immediate(); } const named = class own { static { own(); } }; return x; }";
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
    assert!(parsed.errors.is_empty(), "{:?}", parsed.errors);
    let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
    let (index, nodes) = build_function_program_index_with_nodes(
        &parsed.program,
        source,
        &owners,
        Arc::from("/class.ts"),
    );
    let entry = index.matches_named("f").next().unwrap().entry();
    let prepared =
        build_indexed_function_body_skeleton(&first_function(&parsed.program), entry).unwrap();
    let occurrence = |needle: &str, name: &str| {
        let start = source.find(needle).unwrap() as u32;
        prepared.bindings().occurrence(FrameSpan::rebase(
            entry.span.start,
            verter_span::Span::new(start, start + name.len() as u32),
        ))
    };
    for (needle, name) in [
        ("touch(); var", "touch"),
        ("x = 1", "x"),
        ("caught = 3", "caught"),
        ("item = 4", "item"),
        ("own();", "own"),
    ] {
        assert_eq!(
            occurrence(needle, name),
            FlowBindingOccurrence::UnmodeledLocal,
            "{needle}"
        );
    }
    assert_eq!(
        occurrence("touch(); x", "touch"),
        FlowBindingOccurrence::Free
    );
    assert!(matches!(
        occurrence("x = 2", "x"),
        FlowBindingOccurrence::Resolved(FlowBindingRef::Local(_))
    ));
    assert_eq!(occurrence("x = 2", "x"), occurrence("x = 5", "x"));
    assert_eq!(
        occurrence("deferred();", "deferred"),
        FlowBindingOccurrence::Missing
    );
    assert_eq!(
        occurrence("deferredInit();", "deferredInit"),
        FlowBindingOccurrence::Missing
    );
    assert_eq!(
        occurrence("immediate();", "immediate"),
        FlowBindingOccurrence::Free
    );
    for text in ["touch()", "own()", "immediate()"] {
        let start = source.find(text).unwrap() as u32;
        assert!(
            nodes
                .call(verter_span::Span::new(start, start + text.len() as u32))
                .is_some(),
            "evaluated call has retained address: {text}"
        );
    }
    assert!(entry
        .bindings
        .iter()
        .all(|binding| !["touch", "caught", "item", "own"].contains(&binding.name.as_ref())));
}

#[test]
fn runtime_shape_preserves_parameter_var_and_pattern_alias_boundaries() {
    use crate::analysis::function_program::build_function_program_index;
    use crate::analysis::top_level_owners::TopLevelOwnerTable;
    let source = "function f(p,q,r) { var p; var [q] = []; { let q = 0; q; } return r; }";
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::ts()).parse();
    assert!(parsed.errors.is_empty());
    let owners = TopLevelOwnerTable::ordinary_file(parsed.program.body.len());
    let index =
        build_function_program_index(&parsed.program, source, &owners, Arc::from("/runtime.ts"));
    let entry = index.matches_named("f").next().unwrap().entry();
    let prepared =
        build_indexed_function_body_skeleton(&first_function(&parsed.program), entry).unwrap();
    for (ordinal, binding) in prepared.skeleton().bindings.iter().enumerate() {
        let local = SkeletonBindingId::from_index(ordinal as u32);
        let shape = prepared.bindings().runtime_shape(local);
        let name = prepared.bindings().identity(local).unwrap().name.as_ref();
        if name == "p" {
            assert_eq!(
                shape,
                FlowRuntimeBindingShape {
                    has_var: true,
                    has_destructured_var: false
                }
            );
        } else if name == "q" && binding.kind != SkeletonBindingKind::Let {
            assert_eq!(
                shape,
                FlowRuntimeBindingShape {
                    has_var: true,
                    has_destructured_var: true
                }
            );
        } else {
            assert_eq!(shape, FlowRuntimeBindingShape::default());
        }
    }
}

#[test]
fn declaration_span_queries_do_not_scan_unrelated_parameters_or_aliases() {
    for count in [32, 128, 512] {
        let parameters = (0..count)
            .map(|index| format!("p{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let declarations = "var shared = 0;".repeat(count);
        let source = format!("function f({parameters}) {{ {declarations} return 0; }}");
        let prepared = indexed_structure_of(&source);
        let skeleton = prepared.skeleton();
        let bindings = prepared.bindings();
        assert_eq!(skeleton.bindings.len(), count * 2);
        binding::take_declaration_span_comparisons();
        for (ordinal, declaration) in skeleton.bindings.iter().enumerate() {
            assert_eq!(
                bindings.declaration_at_span(declaration.span),
                Some(SkeletonBindingId::from_index(ordinal as u32))
            );
        }
        let missing = FrameSpan::rebase(
            0,
            verter_span::Span::new(source.len() as u32, source.len() as u32 + 1),
        );
        assert_eq!(bindings.declaration_at_span(missing), None);
        let comparisons = binding::take_declaration_span_comparisons();
        eprintln!(
            "{count} parameters + {count} aliases: {comparisons} declaration span comparisons"
        );
        assert!(comparisons <= 8 * (count * 2 + 1), "{count} parameters and {count} aliases used {comparisons} declaration span comparisons");
    }
}

#[test]
fn declaration_span_index_preserves_authored_alias_and_shadow_identities() {
    let source = "function f(shared, {part: renamed}, ...rest) { var shared = 0; let local = 1; { let local = 2; } type Label = string; return 0; }";
    let prepared = indexed_structure_of(source);
    let skeleton = prepared.skeleton();
    let bindings = prepared.bindings();
    for (ordinal, declaration) in skeleton.bindings.iter().enumerate() {
        assert_eq!(
            bindings.declaration_at_span(declaration.span),
            Some(SkeletonBindingId::from_index(ordinal as u32))
        );
    }
    let shared = skeleton
        .bindings_named(skeleton.name_id("shared").unwrap())
        .collect::<Vec<_>>();
    assert_eq!(shared.len(), 2);
    assert_ne!(shared[0], shared[1]);
    assert_eq!(
        bindings.canonical_local(shared[0]),
        bindings.canonical_local(shared[1])
    );
    let locals = skeleton
        .bindings_named(skeleton.name_id("local").unwrap())
        .collect::<Vec<_>>();
    assert_eq!(locals.len(), 2);
    assert_ne!(
        bindings.canonical_local(locals[0]),
        bindings.canonical_local(locals[1])
    );
    let type_binding = skeleton
        .bindings_named(skeleton.name_id("Label").unwrap())
        .next()
        .unwrap();
    assert!(
        bindings.identity(type_binding).is_none(),
        "declaration addressability never invents a value identity for a type-only binder"
    );
}

/// A site is not a callback identity. Several callables share one
/// expression site whenever the site is a compound the skeleton does not
/// open per element — a call's argument list, an array literal — so the
/// site-level capture union answers "is this cell retained here" and can
/// never answer "which callback retains it". The per-callable inventory
/// is the partition that can: exactly one record per authored callable,
/// in authored order, each carrying only its OWN captures. Without it a
/// two-callback call is one undifferentiated capture set, and a
/// capture-free callback is indistinguishable from no callback at all.
#[test]
fn each_callable_sharing_one_site_retains_its_own_capture_partition() {
    for source in [
        "function root() { const a = 1; const b = 2; sink(() => a, () => b); return 1; }",
        "function root() { const a = 1; const b = 2; return [() => a, () => b]; }",
    ] {
        let prepared = indexed_structure_of(source);
        let skeleton = prepared.skeleton();
        let a = single_binding_named(skeleton, "a");
        let b = single_binding_named(skeleton, "b");
        let site = skeleton
            .expr_sites
            .iter()
            .find(|site| site.closures.len() > 1)
            .unwrap_or_else(|| panic!("both callables share one site: {source}"));
        assert_eq!(
            site.capture_bindings.as_ref(),
            &[FlowBindingRef::Local(a), FlowBindingRef::Local(b)],
            "the site union deliberately merges both callbacks"
        );
        let partition: Vec<_> = site
            .closures
            .iter()
            .map(|closure| (closure.correlation, closure.captures.as_ref()))
            .collect();
        assert_eq!(
            partition,
            vec![
                (
                    SkeletonClosureCorrelation::Exact,
                    &[FlowBindingRef::Local(a)][..]
                ),
                (
                    SkeletonClosureCorrelation::Exact,
                    &[FlowBindingRef::Local(b)][..]
                ),
            ],
            "each callback keeps exactly its own capture, in authored order: {source}"
        );
        assert!(
            site.closures[0].span.to_absolute(0).start < site.closures[1].span.to_absolute(0).start,
            "authored order is the record order"
        );
    }
}

/// The three outcomes a consumer must be able to tell apart at one call
/// site: a callback that provably captures nothing, a callback whose only
/// free read binds no declaration at all (a global — never a capture),
/// and a callback the indexed program does not serve (a parameter
/// default), which asserts NOTHING and must fail closed rather than read
/// as capture-free.
#[test]
fn capture_free_globals_only_and_uncorrelated_callables_stay_distinct() {
    let prepared = indexed_structure_of("function root() { sink(() => 1); return 1; }");
    let closures = sole_closure_inventory(prepared.skeleton());
    assert_eq!(
        closures.len(),
        1,
        "a capture-free callback is still recorded"
    );
    assert_eq!(closures[0].correlation, SkeletonClosureCorrelation::Exact);
    assert!(
        closures[0].captures.is_empty(),
        "proved capture-free, not unknown"
    );

    let prepared = indexed_structure_of("function root() { sink(() => globalThing); return 1; }");
    let closures = sole_closure_inventory(prepared.skeleton());
    assert_eq!(closures[0].correlation, SkeletonClosureCorrelation::Exact);
    assert!(
        closures[0].captures.is_empty(),
        "a read that binds no declaration is free, never a capture"
    );

    let prepared = indexed_structure_of("function root(p, q = () => p) { return q; }");
    let closures = sole_closure_inventory(prepared.skeleton());
    assert_eq!(
        closures[0].correlation,
        SkeletonClosureCorrelation::Uncorrelated,
        "a parameter-default callable the index does not serve is a typed unknown"
    );
    assert!(
        closures[0].captures.is_empty(),
        "an uncorrelated record asserts no capture set"
    );
}

/// Capture identity is the binding, never the name: the inner `a` shadows
/// the outer one, and a callable capturing through an intervening
/// callable reaches the same exact declaration the direct capture does.
#[test]
fn per_callable_captures_are_shadow_exact_and_transitive() {
    let prepared = indexed_structure_of(
        "function root() { const a = 1; { const a = 2; sink(() => a); } return a; }",
    );
    let skeleton = prepared.skeleton();
    let shadowed: Vec<_> = skeleton
        .bindings_named(skeleton.name_id("a").unwrap())
        .collect();
    assert_eq!(shadowed.len(), 2);
    let closures = sole_closure_inventory(skeleton);
    assert_eq!(
        closures[0].captures.as_ref(),
        &[FlowBindingRef::Local(shadowed[1])],
        "the inner declaration is the captured cell"
    );

    let prepared =
        indexed_structure_of("function root() { const a = 1; sink(() => () => a); return 1; }");
    let skeleton = prepared.skeleton();
    let a = single_binding_named(skeleton, "a");
    let closures = sole_closure_inventory(skeleton);
    assert_eq!(
        closures[0].captures.as_ref(),
        &[FlowBindingRef::Local(a)],
        "a capture through an intervening callable names the same declaration"
    );
}

/// The indexed program serves no class member body or field initializer
/// and no parameter-list callable, so a cell retained there is named by
/// no index record. A class evaluated at a site is therefore an unserved
/// callable, and a SERVED callable whose body creates one only knows a
/// lower bound of its captures. Neither may read as an exact capture set:
/// an exact empty set is a capture-free proof, and every fixture here
/// really retains `a`.
#[test]
fn callables_the_index_cannot_serve_never_read_as_an_exact_capture_set() {
    use SkeletonClosureCorrelation::{Partial, Uncorrelated};
    for (source, expected) in [
        (
            "function root() { const a = 1; sink(class { m() { return a; } }); return 1; }",
            Uncorrelated,
        ),
        (
            "function root() { const a = 1; sink(class { m = () => a; }); return 1; }",
            Uncorrelated,
        ),
        (
            "function root() { const a = 1; sink(() => class { m() { return a; } }); return 1; }",
            Partial,
        ),
        (
            "function root() { const a = 1; sink(() => { function g(q = () => a) { return q; } return g; }); return 1; }",
            Partial,
        ),
    ] {
        let prepared = indexed_structure_of(source);
        let closures = sole_closure_inventory(prepared.skeleton());
        assert_eq!(closures.len(), 1, "one authored callable at the site: {source}");
        assert_eq!(
            closures[0].correlation, expected,
            "an unserved capture is never an exact set: {source}"
        );
    }
}

fn single_binding_named(skeleton: &FunctionBodySkeleton, name: &str) -> SkeletonBindingId {
    let id = skeleton
        .name_id(name)
        .unwrap_or_else(|| panic!("`{name}` must be interned"));
    let mut bindings = skeleton.bindings_named(id);
    let binding = bindings
        .next()
        .unwrap_or_else(|| panic!("`{name}` must be bound"));
    assert!(bindings.next().is_none(), "`{name}` binds exactly once");
    binding
}

fn sole_closure_inventory(skeleton: &FunctionBodySkeleton) -> &[SkeletonClosure] {
    let mut sites = skeleton
        .expr_sites
        .iter()
        .filter(|site| !site.closures.is_empty());
    let site = sites.next().expect("the fixture authors one callable");
    assert!(sites.next().is_none(), "exactly one site holds a callable");
    &site.closures
}
