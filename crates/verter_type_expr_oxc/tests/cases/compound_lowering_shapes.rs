//! Shape-preservation rails for the compound lowering arms
//! (union / intersection / tuple / type-literal / template-literal /
//! type-reference / import-type).
//!
//! These arms construct their `Arc<[…]>` payloads straight from
//! exact-size iterators (single allocation) instead of `Vec` +
//! `Arc::from(vec)`. The tests pin the OBSERVABLE contract that the
//! direct-construction path must preserve: arm/element SOURCE ORDER,
//! `Array`/`ReadonlyArray` single-argument normalization (and its
//! arity gate), filtered type-literal member inventories, and the
//! shared empty type-argument slice for bare references.

use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_parser::Parser;
use oxc_span::SourceType;

use verter_type_expr::{ObjectMember, PrimitiveName, TypeExpr};
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
fn union_arms_lower_in_source_order() {
    let lowered = lower_alias(r#"type __T = "a" | number | Foo;"#);
    let TypeExpr::Union(parts) = &lowered else {
        panic!("expected Union, got {lowered:?}");
    };
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], TypeExpr::string_literal("a"));
    assert_eq!(parts[1], TypeExpr::Primitive(PrimitiveName::Number));
    assert!(
        matches!(&parts[2], TypeExpr::Ref { name, .. } if name.as_ref() == "Foo"),
        "third arm must stay the Foo reference, got {:?}",
        parts[2]
    );
}

#[test]
fn intersection_arms_lower_in_source_order() {
    let lowered = lower_alias("type __T = Foo & Bar & { c: string };");
    let TypeExpr::Intersection(parts) = &lowered else {
        panic!("expected Intersection, got {lowered:?}");
    };
    assert_eq!(parts.len(), 3);
    assert!(matches!(&parts[0], TypeExpr::Ref { name, .. } if name.as_ref() == "Foo"));
    assert!(matches!(&parts[1], TypeExpr::Ref { name, .. } if name.as_ref() == "Bar"));
    assert!(matches!(&parts[2], TypeExpr::Object(_)));
}

#[test]
fn type_literal_members_lower_in_source_order_and_count() {
    let lowered = lower_alias(
        "type __T = { a: string; m(x: number): void; [key: string]: unknown; new (): Foo };",
    );
    let TypeExpr::Object(obj) = &lowered else {
        panic!("expected Object, got {lowered:?}");
    };
    assert_eq!(obj.properties.len(), 4);
    assert!(matches!(&obj.properties[0], ObjectMember::Property(p) if p.name == "a"));
    assert!(matches!(&obj.properties[1], ObjectMember::Method(m) if m.name == "m"));
    assert!(matches!(
        &obj.properties[2],
        ObjectMember::IndexSignature(_)
    ));
    assert!(matches!(
        &obj.properties[3],
        ObjectMember::ConstructSignature(_)
    ));
}

#[test]
fn type_literal_drops_unnameable_members_without_padding() {
    // A computed-symbol key has no static name — `lower_ts_signature`
    // filters it. The surviving inventory must be EXACTLY the nameable
    // members (no placeholder / padding slots from the pre-sized buffer).
    let lowered = lower_alias("type __T = { a: string; [Symbol.iterator](): void; b: number };");
    let TypeExpr::Object(obj) = &lowered else {
        panic!("expected Object, got {lowered:?}");
    };
    assert_eq!(
        obj.properties.len(),
        2,
        "computed-symbol member must be dropped, got {:?}",
        obj.properties
    );
    assert!(matches!(&obj.properties[0], ObjectMember::Property(p) if p.name == "a"));
    assert!(matches!(&obj.properties[1], ObjectMember::Property(p) if p.name == "b"));
}

#[test]
fn tuple_elements_lower_in_source_order_with_flags() {
    let lowered = lower_alias("type __T = [string, label?: number, ...rest: boolean[]];");
    let TypeExpr::Tuple { elements, readonly } = &lowered else {
        panic!("expected Tuple, got {lowered:?}");
    };
    assert!(!readonly);
    assert_eq!(elements.len(), 3);
    assert_eq!(elements[0].ty, TypeExpr::Primitive(PrimitiveName::String));
    assert!(!elements[0].optional && !elements[0].rest);
    assert_eq!(elements[1].label.as_deref(), Some("label"));
    assert!(elements[1].optional);
    assert!(elements[2].rest);
}

#[test]
fn template_literal_keeps_quasis_and_expressions_aligned() {
    let lowered = lower_alias("type __T = `pre${string}mid${Foo}post`;");
    let TypeExpr::TemplateLiteral {
        quasis,
        expressions,
    } = &lowered
    else {
        panic!("expected TemplateLiteral, got {lowered:?}");
    };
    assert_eq!(quasis, &["pre", "mid", "post"]);
    assert_eq!(expressions.len(), 2);
    assert_eq!(expressions[0], TypeExpr::Primitive(PrimitiveName::String));
    assert!(matches!(&expressions[1], TypeExpr::Ref { name, .. } if name.as_ref() == "Foo"));
}

#[test]
fn array_single_argument_normalizes_to_array_node() {
    let lowered = lower_alias("type __T = Array<string>;");
    let TypeExpr::Array { element, readonly } = &lowered else {
        panic!("expected Array, got {lowered:?}");
    };
    assert!(!readonly);
    assert_eq!(
        element.as_ref(),
        &TypeExpr::Primitive(PrimitiveName::String)
    );
}

#[test]
fn readonly_array_single_argument_normalizes_readonly() {
    let lowered = lower_alias("type __T = ReadonlyArray<number>;");
    let TypeExpr::Array { element, readonly } = &lowered else {
        panic!("expected Array, got {lowered:?}");
    };
    assert!(readonly);
    assert_eq!(
        element.as_ref(),
        &TypeExpr::Primitive(PrimitiveName::Number)
    );
}

#[test]
fn array_wrong_arity_stays_a_reference() {
    // NEGATIVE: the Array normalization gate is EXACTLY one argument.
    let two_args = lower_alias("type __T = Array<string, number>;");
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = &two_args
    else {
        panic!("expected Ref, got {two_args:?}");
    };
    assert_eq!(name.as_ref(), "Array");
    assert_eq!(type_arguments.len(), 2);

    let bare = lower_alias("type __T = Array;");
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = &bare
    else {
        panic!("expected Ref, got {bare:?}");
    };
    assert_eq!(name.as_ref(), "Array");
    assert!(type_arguments.is_empty());
}

#[test]
fn reference_type_arguments_lower_in_source_order() {
    let lowered = lower_alias("type __T = Record<string, Foo>;");
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = &lowered
    else {
        panic!("expected Ref, got {lowered:?}");
    };
    assert_eq!(name.as_ref(), "Record");
    assert_eq!(type_arguments.len(), 2);
    assert_eq!(
        type_arguments[0],
        TypeExpr::Primitive(PrimitiveName::String)
    );
    assert!(matches!(&type_arguments[1], TypeExpr::Ref { name, .. } if name.as_ref() == "Foo"));
}

#[test]
fn bare_reference_has_empty_type_arguments() {
    let lowered = lower_alias("type __T = Foo;");
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = &lowered
    else {
        panic!("expected Ref, got {lowered:?}");
    };
    assert_eq!(name.as_ref(), "Foo");
    assert!(type_arguments.is_empty());
    // Equal to the factory-built bare reference (shared empty slice
    // semantics — structural equality, not pointer identity).
    assert_eq!(lowered, TypeExpr::named("Foo"));
}

#[test]
fn import_type_arguments_lower_in_source_order() {
    let lowered = lower_alias(r#"type __T = import("./m").Maker<string, Foo>;"#);
    let TypeExpr::ImportType {
        specifier,
        qualifier,
        typeof_query,
        type_arguments,
    } = &lowered
    else {
        panic!("expected ImportType, got {lowered:?}");
    };
    assert_eq!(specifier.as_ref(), "./m");
    assert_eq!(qualifier.len(), 1);
    assert_eq!(qualifier[0].as_ref(), "Maker");
    assert!(!typeof_query);
    assert_eq!(type_arguments.len(), 2);
    assert_eq!(
        type_arguments[0],
        TypeExpr::Primitive(PrimitiveName::String)
    );
    assert!(matches!(&type_arguments[1], TypeExpr::Ref { name, .. } if name.as_ref() == "Foo"));
}

#[test]
fn bare_import_type_has_empty_type_arguments() {
    let lowered = lower_alias(r#"type __T = import("./m");"#);
    let TypeExpr::ImportType {
        specifier,
        qualifier,
        typeof_query,
        type_arguments,
    } = &lowered
    else {
        panic!("expected ImportType, got {lowered:?}");
    };
    assert_eq!(specifier.as_ref(), "./m");
    assert!(qualifier.is_empty());
    assert!(!typeof_query);
    assert!(type_arguments.is_empty());
}

#[test]
fn single_arm_union_syntax_unwraps() {
    // `type __T = | Foo;` parses as a 1-arm union — the lowering must
    // unwrap to the bare arm, never a 1-arm Union node.
    let lowered = lower_alias("type __T = | Foo;");
    assert!(
        matches!(&lowered, TypeExpr::Ref { name, .. } if name.as_ref() == "Foo"),
        "1-arm union must unwrap, got {lowered:?}"
    );
}
