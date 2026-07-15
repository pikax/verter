//! Lowering tests for `typeof` type queries carrying an instantiation
//! expression (`typeof C.make<string>` — `TSTypeQuery.type_arguments`).
//!
//! The query's type arguments are SEMANTIC meaning (they select the generic
//! instantiation of the referenced value), so the lowering must carry them on
//! [`ValueRef::type_args`] rather than dropping them. A bare `typeof a.b.c`
//! keeps an EMPTY `type_args` — these tests discriminate both directions.

use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_parser::Parser;
use oxc_span::SourceType;

use verter_type_expr::{PrimitiveName, TypeExpr};
use verter_type_expr_oxc::lower_ts_type;

/// Parse `source` (which MUST declare a `type __T = <annotation>;` alias) and
/// lower the alias's annotation to a [`TypeExpr`].
fn lower_alias(source: &str) -> TypeExpr {
    let allocator = Allocator::default();
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

    lower_ts_type(alias, source)
}

#[test]
fn typeof_instantiation_expression_lowers_type_args() {
    let lowered = lower_alias("type __T = typeof GenericStatic.make<string>;");
    let TypeExpr::TypeOf(value_ref) = &lowered else {
        panic!("expected TypeOf, got {lowered:?}");
    };
    assert_eq!(value_ref.path, vec!["GenericStatic", "make"]);
    assert_eq!(
        value_ref.type_args,
        vec![TypeExpr::Primitive(PrimitiveName::String)],
        "the instantiation-expression type argument must be lowered onto type_args"
    );
}

#[test]
fn typeof_instantiation_expression_lowers_multiple_and_compound_args() {
    let lowered = lower_alias("type __T = typeof pair<string, { a: number }>;");
    let TypeExpr::TypeOf(value_ref) = &lowered else {
        panic!("expected TypeOf, got {lowered:?}");
    };
    assert_eq!(value_ref.path, vec!["pair"]);
    assert_eq!(value_ref.type_args.len(), 2);
    assert_eq!(
        value_ref.type_args[0],
        TypeExpr::Primitive(PrimitiveName::String)
    );
    assert!(
        matches!(&value_ref.type_args[1], TypeExpr::Object(_)),
        "compound type argument must lower structurally, got {:?}",
        value_ref.type_args[1]
    );
}

#[test]
fn bare_typeof_path_keeps_empty_type_args() {
    // NEGATIVE: a plain `typeof a.b.c` must NOT grow phantom type args.
    let lowered = lower_alias("type __T = typeof a.b.c;");
    let TypeExpr::TypeOf(value_ref) = &lowered else {
        panic!("expected TypeOf, got {lowered:?}");
    };
    assert_eq!(value_ref.path, vec!["a", "b", "c"]);
    assert!(
        value_ref.type_args.is_empty(),
        "bare typeof must keep an empty type_args"
    );
}
