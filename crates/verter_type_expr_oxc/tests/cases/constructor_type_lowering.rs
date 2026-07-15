//! Lowering tests for the `TypeExpr::ConstructorType` variant and the
//! operator-aware signed-literal lowering in `lower_ts_type`.
//!
//! Two distinctions are load-bearing and pinned here:
//!
//! 1. A bare constructor TYPE `new (...) => R` (a `TSConstructorType`) lowers
//!    to the dedicated [`TypeExpr::ConstructorType`] variant, while a
//!    type-LITERAL carrying a construct signature `{ new (): R }` (a
//!    `TSTypeLiteral`) still lowers to [`TypeExpr::Object`] with an
//!    [`ObjectMember::ConstructSignature`]. Vue runtime-constructor inference
//!    maps the former to `Function` and the latter to `Object`, so the producer
//!    must keep them apart — these tests FAIL against a tree that collapses a
//!    bare constructor type into `Function` or into `Object`.
//!
//! 2. A signed numeric / bigint literal type carries its sign on the wrapping
//!    `UnaryExpression`, NOT on the inner literal. The lowering must be
//!    operator-aware: `+1` / `+1n` keep their (positive) magnitude while `-1` /
//!    `-1n` are negated. A regressed lowering that unconditionally negates turns
//!    `+1n` into the wrong literal `-1n` — these tests catch exactly that.

use oxc_allocator::Allocator;
use oxc_ast::ast::{BigintBase, Statement, TSType, UnaryOperator};
use oxc_ast::AstBuilder;
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};

use verter_type_expr::{LiteralValue, ObjectMember, TypeExpr};
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

/// Lower a SYNTHETIC unary-literal type built directly with [`AstBuilder`].
///
/// A unary-`+` literal type (`+1`, `+1n`) is REJECTED by TypeScript and by the
/// OXC parser ("a unary expression with the '+' operator is not allowed"), so it
/// cannot be produced from source — `lower_alias` would hit `ret.panicked`. The
/// lowerer must nonetheless map the `UnaryOperator` faithfully if such a node
/// ever reaches it, so the `UnaryPlus` arm is exercised here by constructing the
/// `TSLiteralType { UnaryExpression { operator, <numeric|bigint> } }` AST node
/// directly. `is_bigint` selects a `BigIntLiteral` argument (`Nn`) vs a
/// `NumericLiteral` argument (`N`). Returns the lowered [`TypeExpr`].
fn lower_synthetic_unary_literal(
    operator: UnaryOperator,
    magnitude: &str,
    is_bigint: bool,
) -> TypeExpr {
    let allocator = Allocator::default();
    let builder = AstBuilder::new(&allocator);
    let argument = if is_bigint {
        builder.expression_big_int_literal(Span::default(), magnitude, None, BigintBase::Decimal)
    } else {
        let value: f64 = magnitude
            .parse()
            .expect("numeric magnitude must parse as f64");
        builder.expression_numeric_literal(
            Span::default(),
            value,
            None,
            oxc_ast::ast::NumberBase::Decimal,
        )
    };
    let literal = builder.ts_literal_unary_expression(Span::default(), operator, argument);
    let ts_type = builder.ts_type_literal_type(Span::default(), literal);
    let TSType::TSLiteralType(_) = &ts_type else {
        panic!("builder must produce a TSLiteralType");
    };
    // The synthetic node has no real source text, so `lower_ts_type`'s
    // `span_text` fallback (only reached on the `Unknown` arm) is never needed
    // for a recognised numeric/bigint literal; an empty source is sufficient.
    lower_ts_type(&ts_type, "")
}

// ---------------------------------------------------------------------------
// Constructor type vs construct-signature type literal
// ---------------------------------------------------------------------------

#[test]
fn bare_constructor_type_lowers_to_constructor_type_variant() {
    // `new () => Foo` is a `TSConstructorType`. It must lower to the dedicated
    // `ConstructorType` variant (NOT `Function`, NOT `Object`).
    let lowered = lower_alias("type __T = new () => Foo;");

    let TypeExpr::ConstructorType(func) = &lowered else {
        panic!("expected TypeExpr::ConstructorType, got {lowered:?}");
    };

    // Negative: it is NOT a plain function type — that is the precise collapse
    // the dedicated variant prevents.
    assert!(
        !matches!(lowered, TypeExpr::Function(_)),
        "a bare constructor type must NOT lower to TypeExpr::Function",
    );
    assert!(
        !matches!(lowered, TypeExpr::Object(_)),
        "a bare constructor type must NOT lower to TypeExpr::Object",
    );

    // The carried signature mirrors a construct signature: no parameters, a
    // `Foo` return.
    assert!(
        func.parameters.is_empty(),
        "`new () => Foo` has no parameters, got {:?}",
        func.parameters,
    );
    let return_type = func
        .return_type
        .as_deref()
        .expect("`new () => Foo` has a return type");
    match return_type {
        TypeExpr::Ref { name, .. } => {
            assert_eq!(name.as_ref(), "Foo", "constructor return must be `Foo`")
        }
        other => panic!("expected the constructor return to be `Ref(Foo)`, got {other:?}"),
    }
}

#[test]
fn constructor_type_with_params_preserves_signature() {
    // `new (x: number) => Foo` — params survive on the ConstructorType payload.
    let lowered = lower_alias("type __T = new (x: number) => Foo;");
    let TypeExpr::ConstructorType(func) = &lowered else {
        panic!("expected TypeExpr::ConstructorType, got {lowered:?}");
    };
    assert_eq!(
        func.parameters.len(),
        1,
        "`new (x: number) => Foo` has exactly one parameter, got {:?}",
        func.parameters,
    );
    assert!(
        matches!(
            func.parameters[0].ty,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
        ),
        "the constructor parameter type must be `number`, got {:?}",
        func.parameters[0].ty,
    );
}

#[test]
fn construct_signature_type_literal_lowers_to_object_not_constructor_type() {
    // `{ new (): Foo }` is a `TSTypeLiteral` carrying a construct signature. It
    // must STILL lower to `Object` with an `ObjectMember::ConstructSignature` —
    // NOT to the new `ConstructorType` variant. This is the distinction the
    // shared reducer relies on (type literal => Object, bare ctor type =>
    // Function).
    let lowered = lower_alias("type __T = { new (): Foo };");

    let TypeExpr::Object(object) = &lowered else {
        panic!("expected TypeExpr::Object for a construct-signature type literal, got {lowered:?}");
    };
    assert!(
        !matches!(lowered, TypeExpr::ConstructorType(_)),
        "a `{{ new (): Foo }}` type literal must NOT lower to TypeExpr::ConstructorType",
    );

    assert_eq!(
        object.properties.len(),
        1,
        "the type literal has exactly one member, got {:?}",
        object.properties,
    );
    assert!(
        matches!(object.properties[0], ObjectMember::ConstructSignature(_)),
        "the single member must be a ConstructSignature, got {:?}",
        object.properties[0],
    );
}

// ---------------------------------------------------------------------------
// Operator-aware signed literal lowering (the `+1n` -> `-1n` regression)
// ---------------------------------------------------------------------------

#[test]
fn negative_bigint_literal_type_keeps_its_sign() {
    // `-1n` lowers to a `BigInt` literal whose stored value carries the `-`.
    let lowered = lower_alias("type __T = -1n;");
    match &lowered {
        TypeExpr::Literal(LiteralValue::BigInt(value)) => {
            assert_eq!(value, "-1", "`-1n` must lower to the bigint literal `-1`");
        }
        other => panic!("expected a BigInt literal, got {other:?}"),
    }
}

#[test]
fn positive_bigint_literal_node_is_not_negated() {
    // `+1n` is rejected by the parser, so this exercises the `UnaryPlus` arm via
    // a synthetic AST node. It must lower to the bare magnitude `1` — NOT the
    // negated `-1`. The WIP's unconditional `format!("-{}", b.value)` produced
    // `-1` here; the operator-aware fix produces `1`. This test FAILS against
    // the unconditional-negation regression and PASSES against the fix.
    let lowered = lower_synthetic_unary_literal(UnaryOperator::UnaryPlus, "1", true);
    match &lowered {
        TypeExpr::Literal(LiteralValue::BigInt(value)) => {
            assert_eq!(
                value, "1",
                "`+1n` must lower to the bigint literal `1`, NOT the negated `-1`",
            );
            assert_ne!(
                value, "-1",
                "`+1n` must NOT be turned into `-1n` (the operator-sign regression)",
            );
        }
        other => panic!("expected a BigInt literal, got {other:?}"),
    }
}

#[test]
fn negative_numeric_literal_type_keeps_its_sign() {
    // `-1` lowers to a numeric literal whose value is `-1.0`.
    let lowered = lower_alias("type __T = -1;");
    match &lowered {
        TypeExpr::Literal(LiteralValue::Number(value)) => {
            assert_eq!(*value, -1.0, "`-1` must lower to the numeric literal -1.0");
        }
        other => panic!("expected a numeric literal, got {other:?}"),
    }
}

#[test]
fn positive_numeric_literal_node_is_not_negated() {
    // `+1` is rejected by the parser, so this exercises the `UnaryPlus` arm via
    // a synthetic AST node. The sign comes from the wrapping `UnaryExpression`,
    // so a `UnaryPlus` must preserve the magnitude (`1.0`); the WIP's
    // unconditional `-n.value` negation yields `-1.0`, which this test FAILS on.
    let lowered = lower_synthetic_unary_literal(UnaryOperator::UnaryPlus, "1", false);
    match &lowered {
        TypeExpr::Literal(LiteralValue::Number(value)) => {
            assert_eq!(
                *value, 1.0,
                "`+1` must lower to the numeric literal 1.0, NOT the negated -1.0",
            );
            assert_ne!(*value, -1.0, "`+1` must NOT be negated to -1.0");
        }
        other => panic!("expected a numeric literal, got {other:?}"),
    }
}

#[test]
fn synthetic_negative_unary_nodes_match_the_parsed_negative_forms() {
    // Cross-check the synthetic-node path against the parsed path for the
    // REACHABLE negative forms: a `UnaryNegation` node built by the builder must
    // lower to the same signed literal as `type __T = -1n;` / `-1;` parsed from
    // source. This pins that `lower_synthetic_unary_literal` itself is faithful
    // (so the `UnaryPlus` assertions above are trustworthy).
    let synthetic_bigint = lower_synthetic_unary_literal(UnaryOperator::UnaryNegation, "1", true);
    assert_eq!(synthetic_bigint, lower_alias("type __T = -1n;"));

    let synthetic_number = lower_synthetic_unary_literal(UnaryOperator::UnaryNegation, "1", false);
    assert_eq!(synthetic_number, lower_alias("type __T = -1;"));
}
