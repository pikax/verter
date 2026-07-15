//! Discriminating tests for the parse-time `RawSourceSurface` capture (design
//! item G). Each test asserts a SPECIFIC erased fact is captured AND a clean
//! peer is captured WITHOUT the fact — an empty/broken capture would fail.

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

use super::*;
use verter_type_expr::MemberVisibility;

fn capture_all(src: &str) -> Vec<CapturedSurface> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, src, SourceType::ts()).parse();
    assert!(!ret.panicked, "fixture must parse: {src}");
    let captured: Vec<CapturedSurface> = ret
        .program
        .body
        .iter()
        .flat_map(capture_statement_surfaces)
        .collect();
    merge_overload_groups(captured)
}

fn surface<'a>(
    caps: &'a [CapturedSurface],
    name: &str,
    space: SymbolSpace,
) -> &'a RawSourceSurface {
    &caps
        .iter()
        .find(|c| c.name == name && c.symbol_space == space)
        .unwrap_or_else(|| panic!("no captured surface for {name} in {space:?}"))
        .surface
}

#[test]
fn clean_type_alias_object_records_static_keys_and_no_erased_facts() {
    let caps = capture_all("type T = { a: string; b: number };");
    let s = surface(&caps, "T", SymbolSpace::Type);
    assert_eq!(s.decl_kind, RawDeclKind::TypeAlias);
    // Both keys captured AND both Static.
    assert_eq!(s.raw_member_keys.len(), 2, "two members captured");
    assert!(
        s.raw_member_keys
            .iter()
            .all(|k| matches!(k, RawKey::Static(_))),
        "clean object has only static keys: {:?}",
        s.raw_member_keys
    );
    // Negative: a clean object carries NONE of the erased facts.
    assert!(s.unique_symbol_ops.is_empty());
    assert!(!s.this_type_or_param);
    assert!(s
        .member_kinds
        .iter()
        .all(|k| matches!(k, RawMemberKind::Property | RawMemberKind::IndexSignature)));
    assert!(s
        .member_visibility
        .iter()
        .all(|v| *v == MemberVisibility::Public));
}

#[test]
fn computed_key_captured_as_non_static() {
    // A clean static-keyed class peer vs a computed-keyed one.
    let caps = capture_all("class C { ['a' + 'b']: number = 1; plain: number = 2; }");
    let s = surface(&caps, "C", SymbolSpace::Type);
    assert!(
        s.raw_member_keys
            .iter()
            .any(|k| !matches!(k, RawKey::Static(_))),
        "computed key recorded as non-static: {:?}",
        s.raw_member_keys
    );
    assert!(
        s.raw_member_keys
            .iter()
            .any(|k| matches!(k, RawKey::Static(_))),
        "the plain peer is still static"
    );
}

#[test]
fn symbol_keyed_member_captured() {
    let caps = capture_all("class C { [Symbol.iterator]: number = 1; }");
    let s = surface(&caps, "C", SymbolSpace::Type);
    assert!(
        s.raw_member_keys.contains(&RawKey::SymbolKeyed),
        "Symbol.* key recorded as symbol-keyed: {:?}",
        s.raw_member_keys
    );
}

#[test]
fn unique_symbol_in_member_type_captured() {
    let with = capture_all("type T = { x: unique symbol };");
    assert!(
        !surface(&with, "T", SymbolSpace::Type)
            .unique_symbol_ops
            .is_empty(),
        "unique symbol operator captured"
    );
    // Negative: a plain `symbol` member captures NO unique-symbol op.
    let without = capture_all("type U = { x: symbol };");
    assert!(surface(&without, "U", SymbolSpace::Type)
        .unique_symbol_ops
        .is_empty());
}

#[test]
fn class_member_visibility_captured() {
    let caps = capture_all(
        "class C { private a: number = 1; protected b: number = 2; public c: number = 3; }",
    );
    let s = surface(&caps, "C", SymbolSpace::Type);
    assert!(s.member_visibility.contains(&MemberVisibility::Private));
    assert!(s.member_visibility.contains(&MemberVisibility::Protected));
    assert!(s.member_visibility.contains(&MemberVisibility::Public));
}

#[test]
fn class_accessor_kinds_captured() {
    let caps = capture_all("class C { get x(): number { return 1; } set x(v: number) {} }");
    let s = surface(&caps, "C", SymbolSpace::Type);
    assert!(
        s.member_kinds.contains(&RawMemberKind::Getter),
        "getter kind: {:?}",
        s.member_kinds
    );
    assert!(s.member_kinds.contains(&RawMemberKind::Setter));
    // Negative: a plain data property is NOT an accessor.
    let plain = capture_all("class P { x: number = 1; }");
    let ps = surface(&plain, "P", SymbolSpace::Type);
    assert!(!ps.member_kinds.contains(&RawMemberKind::Getter));
    assert!(!ps.member_kinds.contains(&RawMemberKind::Setter));
}

#[test]
fn abstract_class_flag_captured() {
    let abs = capture_all("abstract class A {}");
    assert!(surface(&abs, "A", SymbolSpace::Type).abstract_ctor);
    // Negative.
    let plain = capture_all("class B {}");
    assert!(!surface(&plain, "B", SymbolSpace::Type).abstract_ctor);
}

#[test]
fn type_param_const_modifier_captured() {
    let caps = capture_all("function f<const T>(x: T): T { return x; }");
    let s = surface(&caps, "f", SymbolSpace::Value);
    assert!(
        s.type_param_modifiers.iter().any(|m| m.is_const),
        "const type-param modifier captured: {:?}",
        s.type_param_modifiers
    );
    assert!(s.type_param_modifiers.iter().any(|m| m.is_present()));
}

#[test]
fn type_param_variance_captured() {
    let caps = capture_all("type T<in U, out V> = [U, V];");
    let s = surface(&caps, "T", SymbolSpace::Type);
    assert!(
        s.type_param_modifiers.iter().any(|m| m.variance_in),
        "in variance captured: {:?}",
        s.type_param_modifiers
    );
    assert!(s.type_param_modifiers.iter().any(|m| m.variance_out));
    // Negative: a plain type param has no modifiers.
    let plain = capture_all("type P<W> = W;");
    assert!(surface(&plain, "P", SymbolSpace::Type)
        .type_param_modifiers
        .iter()
        .all(|m| !m.is_present()));
}

#[test]
fn function_overload_group_merged() {
    let caps = capture_all(
        "function f(a: number): void; function f(a: string): void; function f(a: any): void {}",
    );
    let s = surface(&caps, "f", SymbolSpace::Value);
    assert!(
        s.overload_signatures.len() >= 2,
        "overload SET captured (len {}): {:?}",
        s.overload_signatures.len(),
        s.overload_signatures
    );
    // Negative: a lone function is NOT an overload set.
    let lone = capture_all("function g(a: number): void {}");
    assert_eq!(
        surface(&lone, "g", SymbolSpace::Value)
            .overload_signatures
            .len(),
        1
    );
}

#[test]
fn as_const_provenance_captured() {
    let with = capture_all("const x = { a: 1 } as const;");
    assert_eq!(
        surface(&with, "x", SymbolSpace::Value).value_const_assertion,
        Some(true)
    );
    // Negative: a plain value is Some(false), NOT None and NOT true.
    let without = capture_all("const y = 5;");
    assert_eq!(
        surface(&without, "y", SymbolSpace::Value).value_const_assertion,
        Some(false)
    );
}

#[test]
fn tuple_element_shapes_captured() {
    let opt = capture_all("type O = [number, string?];");
    let os = surface(&opt, "O", SymbolSpace::Type);
    assert!(os
        .tuple_element_shape
        .contains(&TupleElementShape::Optional));

    let lab = capture_all("type L = [first: number, second: string];");
    let ls = surface(&lab, "L", SymbolSpace::Type);
    assert!(ls
        .tuple_element_shape
        .contains(&TupleElementShape::Labelled));

    let rest = capture_all("type R = [number, ...string[]];");
    let rs = surface(&rest, "R", SymbolSpace::Type);
    assert!(rs.tuple_element_shape.contains(&TupleElementShape::Rest));

    // Negative: a plain tuple has only Plain shapes.
    let plain = capture_all("type P = [number, string];");
    let ps = surface(&plain, "P", SymbolSpace::Type);
    assert!(ps
        .tuple_element_shape
        .iter()
        .all(|s| *s == TupleElementShape::Plain));
}

#[test]
fn this_param_captured() {
    let caps = capture_all("function f(this: number, x: number): void {}");
    assert!(surface(&caps, "f", SymbolSpace::Value).this_type_or_param);
    // Negative: an ordinary function has no `this`.
    let plain = capture_all("function g(x: number): void {}");
    assert!(!surface(&plain, "g", SymbolSpace::Value).this_type_or_param);
}

#[test]
fn this_type_in_annotation_captured() {
    let caps = capture_all("type T = { self: this };");
    assert!(surface(&caps, "T", SymbolSpace::Type).this_type_or_param);
}

#[test]
fn typeof_referent_captured() {
    let caps = capture_all("const base = { a: 1 }; type T = typeof base;");
    let s = surface(&caps, "T", SymbolSpace::Type);
    assert!(
        s.transitive_referents
            .iter()
            .any(|r| r.reference_name == "base"),
        "typeof referent captured: {:?}",
        s.transitive_referents
    );
}

#[test]
fn exported_declaration_captured() {
    let caps = capture_all("export interface E { a: string }");
    let s = surface(&caps, "E", SymbolSpace::Type);
    assert_eq!(s.decl_kind, RawDeclKind::Interface);
    assert_eq!(s.raw_member_keys.len(), 1);
}

#[test]
fn type_and_value_space_keyed_independently() {
    // `Foo` exists as BOTH a type alias and a value const — two surfaces.
    let caps = capture_all("type Foo = string; const Foo = 1;");
    let t = surface(&caps, "Foo", SymbolSpace::Type);
    assert_eq!(t.decl_kind, RawDeclKind::TypeAlias);
    let v = surface(&caps, "Foo", SymbolSpace::Value);
    assert_eq!(v.decl_kind, RawDeclKind::Variable);
}

#[test]
fn callable_member_kind_captured() {
    let caps = capture_all("type T = { (): number; new (): object; m(): void };");
    let s = surface(&caps, "T", SymbolSpace::Type);
    assert!(s.member_kinds.contains(&RawMemberKind::CallSignature));
    assert!(s.member_kinds.contains(&RawMemberKind::ConstructSignature));
    assert!(s.member_kinds.contains(&RawMemberKind::Method));
}

#[test]
fn lowered_body_rejectable_variant_discriminates() {
    use std::sync::Arc;
    use verter_type_expr::{PrimitiveName, TypeExpr};
    // A clean primitive body has no rejectable non-erased variant.
    let clean = TypeExpr::Primitive(PrimitiveName::String);
    assert_eq!(lowered_body_rejectable_variant(&clean), None);
    // A keyof body does.
    let keyof = TypeExpr::KeyOf(Arc::new(clean));
    assert_eq!(lowered_body_rejectable_variant(&keyof), Some("keyof"));
}

#[test]
fn capture_is_infallible_on_empty_and_exotic() {
    // Empty program → no surfaces, no panic.
    assert!(capture_all("").is_empty());
    // A namespace / module declaration contributes nothing but must not panic.
    let caps = capture_all("namespace N { export const x = 1; }");
    // No top-level capturable surface (namespaces are out of the closed set).
    assert!(caps.iter().all(|c| c.name != "N"));
}

#[test]
fn class_value_space_surface_captures_static_half() {
    // A class is BOTH a type and a value: `typeof C` walks the VALUE-space
    // declaration, so the capture must emit a VALUE surface carrying the
    // STATIC half's facts (keys, kinds, visibility) plus the abstract flag —
    // while the TYPE surface keeps the instance half only.
    let caps = capture_all(
        "class C { x: number = 1; static s: string = \"\"; protected static h: number = 0; \
         static describe(): string { return \"\"; } constructor(id: string) {} }",
    );
    let value = surface(&caps, "C", SymbolSpace::Value);
    assert!(
        value
            .raw_member_keys
            .contains(&RawKey::Static("s".to_string())),
        "static field on the VALUE surface: {:?}",
        value.raw_member_keys
    );
    assert!(
        value
            .raw_member_keys
            .contains(&RawKey::Static("describe".to_string())),
        "static method on the VALUE surface"
    );
    assert!(
        value
            .member_visibility
            .contains(&MemberVisibility::Protected),
        "static visibility carried on the VALUE surface"
    );
    // NEGATIVE: the instance member stays OFF the value surface, and the
    // statics stay OFF the type surface.
    assert!(
        !value
            .raw_member_keys
            .contains(&RawKey::Static("x".to_string())),
        "instance member must not leak onto the VALUE surface"
    );
    let ty = surface(&caps, "C", SymbolSpace::Type);
    assert!(
        !ty.raw_member_keys
            .contains(&RawKey::Static("s".to_string())),
        "static member must not leak onto the TYPE surface"
    );

    // A static ACCESSOR's kind is captured on the VALUE surface (lossy fact).
    let acc = capture_all("class D { static get x(): number { return 1; } }");
    let acc_value = surface(&acc, "D", SymbolSpace::Value);
    assert!(acc_value.member_kinds.contains(&RawMemberKind::Getter));

    // The abstract flag rides BOTH halves.
    let abs = capture_all("abstract class A {}");
    assert!(surface(&abs, "A", SymbolSpace::Value).abstract_ctor);
}
