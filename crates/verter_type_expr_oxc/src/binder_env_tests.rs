//! Which binder each generic-name reference resolves to, and how the work
//! of resolving them grows with the number of binders in scope.

use std::sync::Arc;

use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_parser::Parser;
use oxc_span::SourceType;
use verter_type_expr::{FunctionExpr, PrimitiveName, TypeExpr, TypeParam};

use super::binder_env::work::{measure, BinderWork};
use super::lower_ts_type;

/// The lowered right-hand side of the file's last type alias.
fn lowered_alias(source: &str) -> TypeExpr {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(parsed.diagnostics.is_empty(), "the fixture parses");
    let Some(Statement::TSTypeAliasDeclaration(alias)) = parsed.program.body.last() else {
        panic!("the fixture ends in a type alias");
    };
    lower_ts_type(&alias.type_annotation, source)
}

fn function(ty: &TypeExpr) -> &FunctionExpr {
    match ty {
        TypeExpr::Function(func) => func,
        other => panic!("a function type: {other:?}"),
    }
}

fn returned(func: &FunctionExpr) -> &TypeExpr {
    func.return_type.as_deref().expect("a return type")
}

/// Whether `ty` is a reference bound to exactly `binder`: the same name,
/// constraint and default the declaration carries, so a binder of the same
/// spelling elsewhere does not match.
fn names_binder(ty: &TypeExpr, binder: &TypeParam) -> bool {
    let same = |a: &Option<Arc<TypeExpr>>, b: &Option<Arc<TypeExpr>>| match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
        _ => false,
    };
    matches!(ty, TypeExpr::TypeParameter(param)
        if param.name == binder.name
            && same(&param.constraint, &binder.constraint)
            && same(&param.default, &binder.default))
}

fn primitive(name: PrimitiveName) -> Option<TypeExpr> {
    Some(TypeExpr::Primitive(name))
}

/// Each binder's constraint and default see the binders before it, and the
/// signature sees all three.
///
/// Measured on TypeScript 7.0.2: over `type F = <A extends string, B extends
/// A = A, C extends B = B>(a: A, b: B, c: C) => C`, `Parameters<F>` is
/// `[a: string, b: string, c: string]` and `ReturnType<F>` is `string`.
#[test]
fn three_binders_see_their_predecessors_in_constraints_and_defaults() {
    let lowered = lowered_alias(
        "type F = <A extends string, B extends A = A, C extends B = B>(a: A, b: B, c: C) => C;\n",
    );
    let func = function(&lowered);
    let [a, b, c] = func.type_parameters.as_slice() else {
        panic!("three binders: {func:?}");
    };
    assert_eq!(
        a.constraint.as_deref().cloned(),
        primitive(PrimitiveName::String)
    );
    assert!(names_binder(b.constraint.as_deref().unwrap(), a));
    assert!(names_binder(b.default.as_deref().unwrap(), a));
    assert!(names_binder(c.constraint.as_deref().unwrap(), b));
    assert!(names_binder(c.default.as_deref().unwrap(), b));
    for (param, binder) in func.parameters.iter().zip([a, b, c]) {
        assert!(
            names_binder(&param.ty, binder),
            "{param:?} names {binder:?}"
        );
    }
    assert!(names_binder(returned(func), c));
}

/// A nested signature's own `T` shadows the enclosing one in its body, and
/// its sibling binder stays its own.
///
/// Measured on TypeScript 7.0.2: over `type G = <T extends string>(x: T) =>
/// <U extends number, T extends boolean>(y: U, z: T) => T`,
/// `Parameters<ReturnType<G>>` is `[y: number, z: boolean]` and
/// `ReturnType<ReturnType<G>>` is `boolean`.
#[test]
fn a_nested_binder_shadows_the_enclosing_one_of_its_name() {
    let lowered = lowered_alias(
        "type G = <T extends string>(x: T) => <U extends number, T extends boolean>(y: U, z: T) => T;\n",
    );
    let outer = function(&lowered);
    let inner = function(returned(outer));
    let [outer_t] = outer.type_parameters.as_slice() else {
        panic!("one outer binder: {outer:?}");
    };
    let [inner_u, inner_t] = inner.type_parameters.as_slice() else {
        panic!("two inner binders: {inner:?}");
    };
    assert_eq!(
        inner_t.constraint.as_deref().cloned(),
        primitive(PrimitiveName::Boolean)
    );
    assert!(names_binder(&outer.parameters[0].ty, outer_t));
    assert!(names_binder(&inner.parameters[0].ty, inner_u));
    assert!(names_binder(&inner.parameters[1].ty, inner_t));
    assert!(names_binder(returned(inner), inner_t));
}

/// In a nested body, a shadowed name names the nested binder while a name
/// only the enclosing signature declares names the enclosing binder.
///
/// Measured on TypeScript 7.0.2: over `type H = <A extends string, B extends
/// A>(a: A, b: B) => <B extends number>(b: B, a: A) => B`,
/// `Parameters<ReturnType<H>>` is `[b: number, a: string]` and
/// `Parameters<H>` is `[a: string, b: string]`.
#[test]
fn a_nested_body_sees_shadowing_and_enclosing_binders() {
    let lowered = lowered_alias(
        "type H = <A extends string, B extends A>(a: A, b: B) => <B extends number>(b: B, a: A) => B;\n",
    );
    let outer = function(&lowered);
    let inner = function(returned(outer));
    let [outer_a, outer_b] = outer.type_parameters.as_slice() else {
        panic!("two outer binders: {outer:?}");
    };
    let [inner_b] = inner.type_parameters.as_slice() else {
        panic!("one inner binder: {inner:?}");
    };
    assert!(names_binder(
        outer_b.constraint.as_deref().unwrap(),
        outer_a
    ));
    assert!(names_binder(&outer.parameters[0].ty, outer_a));
    assert!(names_binder(&outer.parameters[1].ty, outer_b));
    assert!(names_binder(&inner.parameters[0].ty, inner_b));
    assert!(names_binder(&inner.parameters[1].ty, outer_a));
    assert!(names_binder(returned(inner), inner_b));
}

/// A constraint sees only the binders declared before it in its own list:
/// a later binder of its list, and an enclosing signature's binder, stay
/// unbound references. This is the lowering's existing visibility, narrower
/// than the checker's; the indexed environment neither widens nor narrows it.
#[test]
fn a_constraint_sees_neither_later_nor_enclosing_binders() {
    let unbound = |ty: Option<&TypeExpr>, name: &str| {
        assert!(
            matches!(ty, Some(TypeExpr::Ref { name: spelled, type_arguments })
                if spelled.as_ref() == name && type_arguments.is_empty()),
            "an unbound `{name}`: {ty:?}"
        );
    };
    let forward = lowered_alias("type F = <A extends C, B, C>(a: A) => A;\n");
    unbound(
        function(&forward).type_parameters[0].constraint.as_deref(),
        "C",
    );

    let enclosing = lowered_alias("type F = <T>(x: T) => <U extends T>(u: U) => U;\n");
    let inner = function(returned(function(&enclosing)));
    unbound(inner.type_parameters[0].constraint.as_deref(), "T");
}

/// Two binders of one spelling in one list: a reference names the first.
#[test]
fn a_repeated_spelling_names_its_first_binder() {
    let lowered = lowered_alias("type F = <T extends string, T extends number>(x: T) => T;\n");
    let func = function(&lowered);
    let first = &func.type_parameters[0];
    assert!(names_binder(&func.parameters[0].ty, first));
    assert!(names_binder(returned(func), first));
}

/// `<T0, T1 extends T0, …, Tn-1 extends Tn-2>`, optionally declared by a
/// signature nested in a generic one.
fn predecessor_chain(binders: usize, nested: bool) -> String {
    let list = (0..binders)
        .map(|i| match i {
            0 => "T0".to_owned(),
            _ => format!("T{i} extends T{}", i - 1),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let chain = format!("<{list}>(x: T{}) => T0", binders - 1);
    if nested {
        format!("type F = <Z>(z: Z) => {chain};\n")
    } else {
        format!("type F = {chain};\n")
    }
}

fn total(work: BinderWork) -> usize {
    work.introductions + work.lookups + work.positions_examined
}

const SIZES: [usize; 4] = [128, 256, 512, 1024];

/// Each binder of a predecessor-constrained list is introduced once and each
/// reference costs one lookup over its own name's entry — no list prefix is
/// copied or searched again per binder — and every constraint names its
/// predecessor binder exactly.
#[test]
fn a_predecessor_chain_resolves_each_reference_once() {
    for binders in SIZES {
        let (lowered, work) = measure(|| lowered_alias(&predecessor_chain(binders, false)));
        // `binders - 1` constraints, the parameter and the return.
        let references = binders + 1;
        assert_eq!(
            work,
            BinderWork {
                introductions: binders,
                lookups: references,
                positions_examined: references,
            },
            "{binders} binders"
        );
        let func = function(&lowered);
        let declared = &func.type_parameters;
        assert_eq!(declared.len(), binders);
        for (i, binder) in declared.iter().enumerate().skip(1) {
            assert!(
                names_binder(binder.constraint.as_deref().unwrap(), &declared[i - 1]),
                "T{i}'s constraint names T{}",
                i - 1
            );
        }
        assert!(names_binder(&func.parameters[0].ty, &declared[binders - 1]));
        assert!(names_binder(returned(func), &declared[0]));
    }
}

/// The same chain declared by a nested signature, which the enclosing
/// signature's normalization walks again: the work still at most doubles
/// as the chain doubles.
#[test]
fn a_nested_predecessor_chain_grows_linearly() {
    let work: Vec<usize> = SIZES
        .iter()
        .map(|&binders| {
            let (lowered, work) = measure(|| lowered_alias(&predecessor_chain(binders, true)));
            let outer = function(&lowered);
            let inner = function(returned(outer));
            let declared = &inner.type_parameters;
            for (i, binder) in declared.iter().enumerate().skip(1) {
                assert!(names_binder(
                    binder.constraint.as_deref().unwrap(),
                    &declared[i - 1]
                ));
            }
            assert!(names_binder(
                &outer.parameters[0].ty,
                &outer.type_parameters[0]
            ));
            total(work)
        })
        .collect();
    for pair in work.windows(2) {
        assert!(pair[1] <= 2 * pair[0], "work per size: {work:?}");
    }
}
