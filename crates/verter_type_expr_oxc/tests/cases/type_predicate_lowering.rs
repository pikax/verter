//! Every type-predicate spelling lowers into the typed IR as a predicate
//! carried BESIDE the signature's return, never as an unsupported-syntax
//! fallback.
//!
//! TypeScript models a predicate signature as a `TypePredicate` record plus
//! a return of `boolean` (a type predicate) or `void` (an assertion) —
//! measured on 7.0.2: `ReturnType<typeof isFoo>` over `isFoo(x: unknown): x
//! is Foo` is `boolean`, and over `assertBar(x: unknown): asserts x is Bar`
//! is `void`. These tests FAIL against a lowering that drops the predicate
//! or leaves the return as a raw `x is Foo` fallback.

use std::sync::Arc;

use oxc_ast::ast::{Statement, TSType};
use oxc_parser::Parser;
use oxc_span::SourceType;

use verter_type_expr::{
    FunctionExpr, ObjectMember, PrimitiveName, TypeExpr, TypePredicate, TypePredicateSubject,
};
use verter_type_expr_oxc::{lower_return_annotation, lower_ts_type};

/// Parse `source` (which MUST declare a `type __T = <annotation>;` alias) and
/// hand the alias annotation to `read`.
fn with_alias<R>(source: &str, read: impl FnOnce(&TSType<'_>) -> R) -> R {
    let allocator = oxc_allocator::Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!ret.panicked, "OXC parser panicked on `{source}`");
    let alias = ret
        .program
        .body
        .iter()
        .find_map(|stmt| match stmt {
            Statement::TSTypeAliasDeclaration(alias) if alias.id.name == "__T" => {
                Some(&alias.type_annotation)
            }
            _ => None,
        })
        .expect("test wrapper missing `__T` alias");
    read(alias)
}

fn lower_alias(source: &str) -> TypeExpr {
    with_alias(source, |alias| lower_ts_type(alias, source))
}

fn only_function(expr: &TypeExpr) -> &FunctionExpr {
    match expr {
        TypeExpr::Function(function) => function,
        other => panic!("expected a function type, got {other:?}"),
    }
}

fn only_member_function(expr: &TypeExpr) -> &FunctionExpr {
    let TypeExpr::Object(object) = expr else {
        panic!("expected an object type, got {expr:?}");
    };
    match object.properties.as_slice() {
        [ObjectMember::Method(method)] => &method.function,
        [ObjectMember::CallSignature(function)] => function,
        other => panic!("expected one method or call signature, got {other:?}"),
    }
}

fn named(name: &str) -> Option<Arc<TypeExpr>> {
    Some(Arc::new(TypeExpr::named(name)))
}

fn predicate(
    subject: TypePredicateSubject,
    asserts: bool,
    ty: Option<Arc<TypeExpr>>,
) -> TypePredicate {
    TypePredicate {
        subject,
        asserts,
        ty,
    }
}

fn parameter(name: &str) -> TypePredicateSubject {
    TypePredicateSubject::Parameter(Arc::from(name))
}

/// Assert `function` returns the checker's predicate return and carries
/// exactly `expected`.
fn assert_predicate(function: &FunctionExpr, expected: TypePredicate) {
    let ret = if expected.asserts {
        PrimitiveName::Void
    } else {
        PrimitiveName::Boolean
    };
    assert_eq!(
        function.return_type.as_deref(),
        Some(&TypeExpr::Primitive(ret)),
        "a predicate signature returns the checker's boolean / void"
    );
    assert_eq!(function.predicate.as_deref(), Some(&expected));
}

#[test]
fn every_parameter_predicate_form_lowers_beside_its_return() {
    for (annotation, expected) in [
        (
            "(x: unknown) => x is Foo",
            predicate(parameter("x"), false, named("Foo")),
        ),
        (
            "(x: unknown) => asserts x is Foo",
            predicate(parameter("x"), true, named("Foo")),
        ),
        (
            "(x: unknown) => asserts x",
            predicate(parameter("x"), true, None),
        ),
        (
            "(a: string, b: unknown) => b is Foo",
            predicate(parameter("b"), false, named("Foo")),
        ),
    ] {
        let source = format!("type __T = {annotation};");
        let lowered = lower_alias(&source);
        assert_predicate(only_function(&lowered), expected);
    }
}

#[test]
fn every_receiver_predicate_form_lowers_on_method_and_call_signatures() {
    for (annotation, expected) in [
        (
            "{ is(): this is Foo }",
            predicate(TypePredicateSubject::This, false, named("Foo")),
        ),
        (
            "{ check(): asserts this is Foo }",
            predicate(TypePredicateSubject::This, true, named("Foo")),
        ),
        (
            "{ check(): asserts this }",
            predicate(TypePredicateSubject::This, true, None),
        ),
        (
            "{ (x: unknown): x is Foo }",
            predicate(parameter("x"), false, named("Foo")),
        ),
    ] {
        let source = format!("type __T = {annotation};");
        let lowered = lower_alias(&source);
        assert_predicate(only_member_function(&lowered), expected);
    }
}

#[test]
fn a_generic_predicate_target_resolves_to_the_signatures_own_binder() {
    let lowered = lower_alias("type __T = <T>(x: unknown) => x is T;");
    let function = only_function(&lowered);
    let target = function
        .predicate
        .as_deref()
        .and_then(|predicate| predicate.ty.as_deref())
        .expect("a type predicate carries its target");
    assert!(
        matches!(target, TypeExpr::TypeParameter(param) if param.name == "T"),
        "`x is T` names the signature's own `T` binder, exactly like its return would: \
         {target:?}"
    );
}

#[test]
fn a_standalone_predicate_annotation_is_its_return_type_never_a_raw_fallback() {
    // The return annotation of `(x: unknown) => x is Foo`, read on its own:
    // TypeScript's `getTypeFromTypeNode` gives a predicate node `boolean`
    // and an assertion node `void`.
    for (annotation, expected) in [
        ("(x: unknown) => x is Foo", PrimitiveName::Boolean),
        ("(x: unknown) => asserts x", PrimitiveName::Void),
    ] {
        let source = format!("type __T = {annotation};");
        with_alias(&source, |alias| {
            let TSType::TSFunctionType(function) = alias else {
                panic!("expected a function type annotation");
            };
            let returned = &function.return_type.type_annotation;
            assert_eq!(
                lower_ts_type(returned, &source),
                TypeExpr::Primitive(expected)
            );
            let (return_type, predicate) = lower_return_annotation(returned, &source);
            assert_eq!(return_type, TypeExpr::Primitive(expected));
            assert!(predicate.is_some(), "the return reader keeps the predicate");
        });
    }
}
