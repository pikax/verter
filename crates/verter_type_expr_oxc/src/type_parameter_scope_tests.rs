//! Which declaration a type-parameter reference names when nested generic
//! function types declare the same name.

use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_parser::Parser;
use oxc_span::SourceType;
use verter_type_expr::{PrimitiveName, TypeExpr};

use super::lower_ts_type;

/// The lowered right-hand side of the file's last type alias.
fn lowered_alias(source: &str) -> TypeExpr {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(parsed.errors.is_empty(), "the fixture parses");
    let Some(Statement::TSTypeAliasDeclaration(alias)) = parsed.program.body.last() else {
        panic!("the fixture ends in a type alias");
    };
    lower_ts_type(&alias.type_annotation, source)
}

/// A nested generic function type's own `T` shadows the enclosing one:
/// the inner signature's parameter and return name the inner declaration.
/// Each function type rewrites its own references first, so the enclosing
/// one's rewrite finds them already bound.
///
/// Measured on TypeScript 7.0.2: over `type F = <T extends string>(x: T) =>
/// <T extends number>(y: T) => T`, `ReturnType<ReturnType<F>>` and
/// `Parameters<ReturnType<F>>[0]` are `number`, the inner constraint.
#[test]
fn a_nested_same_name_type_parameter_names_the_innermost_declaration() {
    let lowered =
        lowered_alias("type F = <T extends string>(x: T) => <T extends number>(y: T) => T;\n");
    let TypeExpr::Function(outer) = &lowered else {
        panic!("a function type: {lowered:?}");
    };
    let Some(TypeExpr::Function(inner)) = outer.return_type.as_deref() else {
        panic!("a function type returning one: {lowered:?}");
    };
    let constraint = |ty: &TypeExpr| match ty {
        TypeExpr::TypeParameter(param) => param.constraint.as_deref().cloned(),
        other => panic!("a type parameter: {other:?}"),
    };
    let number = Some(TypeExpr::Primitive(PrimitiveName::Number));
    assert_eq!(constraint(&inner.parameters[0].ty), number);
    assert_eq!(
        constraint(inner.return_type.as_deref().expect("a return type")),
        number
    );
}
