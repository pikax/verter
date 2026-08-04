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
        SkeletonExprShape::Other => panic!("site must be an object literal"),
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
