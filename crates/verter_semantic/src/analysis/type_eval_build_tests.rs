use super::type_eval::*;
use super::type_eval_build::parse_and_build_env;
use crate::analysis::type_eval_build::{
    expand_macro_types_impl_with_expander, FieldExpansionContext, FieldKind, MacroExpansionScope,
    PathSegment,
};
use crate::analysis::type_expand::{ExpandedNormalizedExpr, ExpansionResult};
use crate::analysis::types::{
    AnalyzedEmitField, AnalyzedMacro, AnalyzedMacroKind, AnalyzedPropField, AnalyzedSlotField,
    AnalyzedSlotFieldBinding, TypeResolutionSource,
};
use std::sync::Arc;
use verter_type_expr::*;

// =============================================================================
// Type alias extraction
// =============================================================================

#[test]
fn extracts_type_alias() {
    let env = parse_and_build_env("type Color = \"red\" | \"blue\" | \"green\"");
    assert!(env.type_symbols.contains_key("Color"));
    let decl = env.type_symbols["Color"].primary();
    assert_eq!(decl.kind, TypeDeclKind::Alias);
    assert!(decl.type_parameters.is_empty());
    match &decl.body {
        TypeExpr::Union(types) => {
            assert_eq!(types.len(), 3);
            assert!(types.contains(&TypeExpr::string_literal("red")));
        }
        _ => panic!("expected union, got {:?}", decl.body),
    }
}

#[test]
fn extracts_generic_type_alias() {
    let env = parse_and_build_env("type Box<T> = { value: T }");
    let decl = env.type_symbols["Box"].primary();
    assert_eq!(decl.type_parameters.len(), 1);
    assert_eq!(decl.type_parameters[0].name, "T");
}

#[test]
fn parse_type_parameter_clause_preserves_constraint_and_default() {
    let params =
        super::type_eval_build::parse_type_parameter_clause("T extends Item = DefaultItem, U");

    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "T");
    assert!(matches!(
        params[0].constraint.as_deref(),
        Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "Item"
    ));
    assert!(matches!(
        params[0].default.as_deref(),
        Some(TypeExpr::Ref { name, .. }) if name.as_ref() == "DefaultItem"
    ));
    assert_eq!(params[1].name, "U");
    assert!(params[1].constraint.is_none());
    assert!(params[1].default.is_none());
}

#[test]
fn parse_and_build_env_assigns_stable_type_declaration_ids_for_unchanged_source() {
    let env_a = parse_and_build_env("type Box<T> = { value: T }\ninterface User { id: number }");
    let env_b = parse_and_build_env("type Box<T> = { value: T }\ninterface User { id: number }");

    assert_eq!(
        env_a.type_declaration_id("Box"),
        env_b.type_declaration_id("Box")
    );
    assert_eq!(
        env_a.type_declaration_id("User"),
        env_b.type_declaration_id("User")
    );
    assert_eq!(
        env_a.type_symbols["Box"].primary().declaration_id,
        env_b.type_symbols["Box"].primary().declaration_id
    );
    assert_eq!(
        env_a.type_symbols["User"].primary().declaration_id,
        env_b.type_symbols["User"].primary().declaration_id
    );
    assert_ne!(env_a.type_symbols["Box"].primary().declaration_id, 0);
    assert_ne!(env_a.type_symbols["User"].primary().declaration_id, 0);
}

#[test]
fn parse_and_build_env_assigns_stable_value_declaration_ids_for_unchanged_source() {
    let env_a =
        parse_and_build_env("const count: number = 1\nfunction greet(): string { return '' }");
    let env_b =
        parse_and_build_env("const count: number = 1\nfunction greet(): string { return '' }");

    assert_eq!(
        env_a.value_declaration_id("count"),
        env_b.value_declaration_id("count")
    );
    assert_eq!(
        env_a.value_declaration_id("greet"),
        env_b.value_declaration_id("greet")
    );
    assert_eq!(
        env_a.value_symbols["count"].primary().declaration_id,
        env_b.value_symbols["count"].primary().declaration_id
    );
    assert_eq!(
        env_a.value_symbols["greet"].primary().declaration_id,
        env_b.value_symbols["greet"].primary().declaration_id
    );
    assert_ne!(env_a.value_symbols["count"].primary().declaration_id, 0);
    assert_ne!(env_a.value_symbols["greet"].primary().declaration_id, 0);
}

// =============================================================================
// Interface extraction
// =============================================================================

#[test]
fn extracts_interface() {
    let env = parse_and_build_env("interface User { id: number; name: string; email?: string }");
    assert!(env.type_symbols.contains_key("User"));
    let decl = env.type_symbols["User"].primary();
    assert_eq!(decl.kind, TypeDeclKind::Interface);

    match &decl.body {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 3);
            // Check optional property
            let email = obj.properties.iter().find_map(|m| match m {
                ObjectMember::Property(p) if p.name == "email" => Some(p),
                _ => None,
            });
            assert!(email.is_some());
            assert!(email.unwrap().optional);
        }
        _ => panic!("expected object, got {:?}", decl.body),
    }
}

#[test]
fn extracts_interface_with_extends() {
    let env = parse_and_build_env(
        r#"
        interface Base { id: number }
        interface User extends Base { name: string }
        "#,
    );
    assert!(env.type_symbols.contains_key("Base"));
    assert!(env.type_symbols.contains_key("User"));

    let user = env.type_symbols["User"].primary();
    // Should be intersection of Base & { name: string }
    match &user.body {
        TypeExpr::Intersection(parts) => {
            assert_eq!(parts.len(), 2);
            assert_eq!(parts[0], TypeExpr::named("Base"));
            assert!(matches!(&parts[1], TypeExpr::Object(_)));
        }
        _ => panic!("expected intersection, got {:?}", user.body),
    }
}

#[test]
fn extracts_interface_with_methods() {
    let env =
        parse_and_build_env("interface Logger { log(msg: string): void; warn(msg: string): void }");
    let decl = env.type_symbols["Logger"].primary();
    match &decl.body {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
            for member in &obj.properties {
                assert!(matches!(member, ObjectMember::Method(_)));
            }
        }
        _ => panic!("expected object, got {:?}", decl.body),
    }
}

// =============================================================================
// Class member visibility (B4.5) — `extract_class` RECORDS non-public class
// members with their declared accessibility on the shared IR surface, instead
// of dropping them. Static members and the constructor are NOT surface
// members. Interface members are always Public.
// =============================================================================

/// Find a property member by name in a `TypeExpr::Object` body.
fn class_property<'a>(body: &'a TypeExpr, name: &str) -> Option<&'a ObjectProperty> {
    let TypeExpr::Object(obj) = body else {
        return None;
    };
    obj.properties.iter().find_map(|m| match m {
        ObjectMember::Property(p) if p.name == name => Some(p),
        _ => None,
    })
}

/// Find a method member by name in a `TypeExpr::Object` body.
fn class_method<'a>(body: &'a TypeExpr, name: &str) -> Option<&'a MethodSignature> {
    let TypeExpr::Object(obj) = body else {
        return None;
    };
    obj.properties.iter().find_map(|m| match m {
        ObjectMember::Method(mm) if mm.name == name => Some(mm),
        _ => None,
    })
}

#[test]
fn extract_class_records_non_public_members_with_visibility() {
    // The producer-level discriminator: pre-change `extract_class` DROPS
    // `b`/`c` (only `a` survives, all Public); post-change it RECORDS all three
    // instance members with their declared accessibility. Static members and
    // the constructor are excluded from the surface entirely.
    let env = parse_and_build_env(
        r#"
        class C {
            public a: string = "";
            protected b: number = 0;
            private c: boolean = false;
            static s: string = "";
            constructor() {}
        }
        "#,
    );
    let decl = env.type_symbols["C"].primary();
    let body = &decl.body;

    let a = class_property(body, "a").expect("public field `a` must be recorded");
    assert_eq!(a.visibility, MemberVisibility::Public);

    let b = class_property(body, "b").expect("protected field `b` must be RECORDED (not dropped)");
    assert_eq!(b.visibility, MemberVisibility::Protected);

    let c = class_property(body, "c").expect("private field `c` must be RECORDED (not dropped)");
    assert_eq!(c.visibility, MemberVisibility::Private);

    // Static member is NOT a surface member.
    assert!(
        class_property(body, "s").is_none(),
        "static field `s` must be excluded from the instance surface"
    );
    // The constructor is NOT a surface member (no `constructor` property/method).
    assert!(
        class_property(body, "constructor").is_none()
            && class_method(body, "constructor").is_none(),
        "the constructor must not appear as a surface member"
    );
}

#[test]
fn extract_class_default_accessibility_is_public() {
    // A field with no accessibility modifier is Public (mirrors
    // `None | Some(Public) => Public`).
    let env = parse_and_build_env(r#"class C { a: string = ""; }"#);
    let body = &env.type_symbols["C"].primary().body;
    let a = class_property(body, "a").expect("field `a` must be recorded");
    assert_eq!(a.visibility, MemberVisibility::Public);
}

#[test]
fn extract_class_records_non_public_methods_with_visibility() {
    let env = parse_and_build_env(
        r#"
        class C {
            public pub(): void {}
            protected prot(): void {}
            private priv(): void {}
            static stat(): void {}
        }
        "#,
    );
    let body = &env.type_symbols["C"].primary().body;

    assert_eq!(
        class_method(body, "pub")
            .expect("public method recorded")
            .visibility,
        MemberVisibility::Public
    );
    assert_eq!(
        class_method(body, "prot")
            .expect("protected method must be RECORDED (not dropped)")
            .visibility,
        MemberVisibility::Protected
    );
    assert_eq!(
        class_method(body, "priv")
            .expect("private method must be RECORDED (not dropped)")
            .visibility,
        MemberVisibility::Private
    );
    assert!(
        class_method(body, "stat").is_none(),
        "static method must be excluded from the instance surface"
    );
}

#[test]
fn extract_class_interface_members_stay_public() {
    // Interface members have no accessibility — always Public.
    let env = parse_and_build_env("interface I { a: string; m(): void }");
    let body = &env.type_symbols["I"].primary().body;
    assert_eq!(
        class_property(body, "a")
            .expect("interface field")
            .visibility,
        MemberVisibility::Public
    );
    assert_eq!(
        class_method(body, "m")
            .expect("interface method")
            .visibility,
        MemberVisibility::Public
    );
}

#[test]
fn extract_class_with_heritage_records_non_public_own_members() {
    // `class C extends Base { protected own }` folds to `Base & { own }`; the
    // own-body Object arm records `own` with its accessibility.
    let env = parse_and_build_env(
        r#"
        class Base { x: number = 0; }
        class C extends Base {
            public a: string = "";
            private secret: number = 0;
        }
        "#,
    );
    let body = &env.type_symbols["C"].primary().body;
    let TypeExpr::Intersection(parts) = body else {
        panic!("expected intersection (heritage fold), got {body:?}");
    };
    assert_eq!(parts[0], TypeExpr::named("Base"));
    let own = &parts[1];
    assert_eq!(
        class_property(own, "a")
            .expect("public own field")
            .visibility,
        MemberVisibility::Public
    );
    assert_eq!(
        class_property(own, "secret")
            .expect("private own field must be RECORDED")
            .visibility,
        MemberVisibility::Private
    );
}

// =============================================================================
// Class STATIC members ride INSIDE the value-side constructor-shape ObjectExpr
// (the `typeof C` constructor-object model). Own static props/methods are
// recorded as ObjectMember::Property / ObjectMember::Method WITH their declared
// visibility alongside the ConstructSignature. The instance surface stays
// unchanged (statics excluded there); accessors still drop; `#private` statics
// never appear; decorated classes lower normally.
// =============================================================================

/// Find a property member by name in an `ObjectExpr`.
fn shape_property<'a>(shape: &'a ObjectExpr, name: &str) -> Option<&'a ObjectProperty> {
    shape.properties.iter().find_map(|m| match m {
        ObjectMember::Property(p) if p.name == name => Some(p),
        _ => None,
    })
}

/// Find a method member by name in an `ObjectExpr`.
fn shape_method<'a>(shape: &'a ObjectExpr, name: &str) -> Option<&'a MethodSignature> {
    shape.properties.iter().find_map(|m| match m {
        ObjectMember::Method(mm) if mm.name == name => Some(mm),
        _ => None,
    })
}

#[test]
fn extract_class_folds_static_members_into_constructor_shape() {
    let env = parse_and_build_env(
        r#"
        class C {
            a: string = "";
            static initial: string = "0";
            static describe(): string { return "c"; }
            protected static hidden: number = 0;
            private static secret: boolean = false;
            constructor(id: string) {}
        }
        "#,
    );
    let value = env.value_symbols["C"].primary();
    let shape = value
        .object_shape
        .as_ref()
        .expect("class value must carry the constructor-object shape");

    // The construct signature is still present (the `new C(...)` half).
    assert!(
        shape
            .properties
            .iter()
            .any(|m| matches!(m, ObjectMember::ConstructSignature(_))),
        "constructor shape must keep its ConstructSignature"
    );

    // Own static field with declared type and visibility.
    let initial = shape_property(shape, "initial").expect("static field `initial` must be folded");
    assert_eq!(initial.ty, TypeExpr::Primitive(PrimitiveName::String));
    assert_eq!(initial.visibility, MemberVisibility::Public);

    // Own static method with its signature.
    let describe =
        shape_method(shape, "describe").expect("static method `describe` must be folded");
    assert_eq!(describe.visibility, MemberVisibility::Public);
    assert_eq!(
        describe.function.return_type.as_deref(),
        Some(&TypeExpr::Primitive(PrimitiveName::String))
    );

    // Non-public statics are RECORDED with their declared visibility.
    assert_eq!(
        shape_property(shape, "hidden")
            .expect("protected static must be recorded")
            .visibility,
        MemberVisibility::Protected
    );
    assert_eq!(
        shape_property(shape, "secret")
            .expect("private static must be recorded")
            .visibility,
        MemberVisibility::Private
    );

    // NEGATIVE: instance members never leak into the constructor shape.
    assert!(
        shape_property(shape, "a").is_none(),
        "instance field `a` must NOT appear on the constructor shape"
    );
    // NEGATIVE: the constructor itself is not a named member.
    assert!(
        shape_property(shape, "constructor").is_none()
            && shape_method(shape, "constructor").is_none(),
        "the constructor must not appear as a named member of the shape"
    );

    // NEGATIVE: the instance surface still excludes statics.
    let body = &env.type_symbols["C"].primary().body;
    assert!(
        class_property(body, "initial").is_none() && class_method(body, "describe").is_none(),
        "static members must stay excluded from the instance surface"
    );
}

#[test]
fn extract_class_static_private_hash_and_accessor_members_stay_excluded() {
    // `static #tag` has a PrivateIdentifier key (no public name) and the
    // `accessor` keyword produces an AccessorProperty — neither is folded
    // into the constructor shape, and the instance accessor stays off the
    // instance surface (current producer contract: accessors drop).
    let env = parse_and_build_env(
        r#"
        class C {
            static #tag: number = 0;
            static accessor sv: string = "";
            accessor v: string = "";
            static visible: string = "";
        }
        "#,
    );
    let value = env.value_symbols["C"].primary();
    let shape = value.object_shape.as_ref().expect("constructor shape");

    assert!(
        shape_property(shape, "visible").is_some(),
        "plain static still folds (control)"
    );
    assert!(
        shape_property(shape, "#tag").is_none() && shape_property(shape, "tag").is_none(),
        "static #private field must not be folded"
    );
    assert!(
        shape_property(shape, "sv").is_none(),
        "static accessor must not be folded (accessor lowering is out of scope)"
    );
    let body = &env.type_symbols["C"].primary().body;
    assert!(
        class_property(body, "v").is_none(),
        "instance accessor must stay off the instance surface"
    );
}

#[test]
fn extract_class_static_only_class_keeps_synthesized_construct_signature() {
    // A class with no explicit constructor still synthesizes the implicit
    // `new () => C` construct signature next to its folded statics.
    let env = parse_and_build_env(
        r#"
        class GenericStatic {
            static make(value: string): { wrapped: string } { return { wrapped: value }; }
        }
        "#,
    );
    let value = env.value_symbols["GenericStatic"].primary();
    let shape = value.object_shape.as_ref().expect("constructor shape");
    assert!(
        shape
            .properties
            .iter()
            .any(|m| matches!(m, ObjectMember::ConstructSignature(_))),
        "implicit constructor must still synthesize a ConstructSignature"
    );
    let make = shape_method(shape, "make").expect("static method `make` folded");
    assert!(
        matches!(
            make.function.return_type.as_deref(),
            Some(TypeExpr::Object(_))
        ),
        "static method return type must be lowered"
    );
}

#[test]
fn extract_class_decorated_class_lowers_normally() {
    // Decorator-capture checkpoint: a decorated class flows through
    // `extract_class` exactly like an undecorated one — surfaces are
    // decorator-invariant; decorators are ignored, never a lowering failure.
    let env = parse_and_build_env(
        r#"
        function logged(ctor: any, ctx: any) { return ctor; }
        @logged
        class LoggedItem {
            id: string = "";
            static version: string = "1";
            label(): string { return "label"; }
        }
        "#,
    );
    let decl = env.type_symbols.get("LoggedItem");
    assert!(
        decl.is_some(),
        "decorated class must register a type symbol"
    );
    let body = &decl.unwrap().primary().body;
    assert!(
        class_property(body, "id").is_some() && class_method(body, "label").is_some(),
        "decorated class instance members must lower normally"
    );
    let value = env.value_symbols["LoggedItem"].primary();
    let shape = value.object_shape.as_ref().expect("constructor shape");
    assert!(
        shape_property(shape, "version").is_some(),
        "decorated class static members must fold normally"
    );
}

#[test]
fn extracts_namespace_qualified_interfaces() {
    let env = parse_and_build_env(
        r#"
        interface NativeElements {
          div: { id?: string }
        }

        declare namespace JSX {
          interface IntrinsicElements extends NativeElements {}
          interface ElementChildrenAttribute {
            children: {}
          }
        }
        "#,
    );

    assert!(
        env.type_symbols.contains_key("JSX.IntrinsicElements"),
        "namespace interfaces should be registered under their qualified name"
    );
    assert!(
        env.type_symbols
            .contains_key("JSX.ElementChildrenAttribute"),
        "nested namespace members should remain addressable from the eval env"
    );

    let decl = env.type_symbols["JSX.IntrinsicElements"].primary();
    match &decl.body {
        TypeExpr::Intersection(parts) => {
            assert_eq!(
                parts[0],
                TypeExpr::named("NativeElements"),
                "namespace interfaces should preserve their extends clauses"
            );
            assert!(
                matches!(parts[1], TypeExpr::Object(_)),
                "qualified namespace interfaces should still lower their local members structurally"
            );
        }
        other => panic!("expected namespace interface intersection, got {other:?}"),
    }
}

#[test]
fn extracts_declare_global_namespace_jsx_into_global_augmentation_scope() {
    // `declare global { namespace JSX { interface IntrinsicElements {...} } }`
    // registers the inner interfaces under their QUALIFIED `JSX.X` names in the
    // GLOBAL augmentation scope — never file-scope `type_symbols` — so a later
    // `JSX.IntrinsicElements["div"]` / `JSX.Element` lookup resolves through the
    // global-augmentation fallback. Discriminating: were the nested namespace not
    // registered under its qualified name, the `(Global, "JSX.IntrinsicElements")`
    // key would be absent and this assertion would fail.
    let env = parse_and_build_env(
        r#"
        export {};
        declare global {
          namespace JSX {
            interface IntrinsicElements {
              div: { id?: string; className?: string };
              span: { title?: string };
            }
            interface Element {
              __element_brand__: true;
            }
          }
        }
        "#,
    );

    let key_intrinsic = (
        AugmentationScopeKind::Global,
        "JSX.IntrinsicElements".to_string(),
    );
    let key_element = (AugmentationScopeKind::Global, "JSX.Element".to_string());

    assert!(
        env.augmentation_scopes.contains_key(&key_intrinsic),
        "nested `namespace JSX` interfaces must register under their qualified \
         name in the global augmentation scope"
    );
    assert!(
        env.augmentation_scopes.contains_key(&key_element),
        "`JSX.Element` must register under the global augmentation scope"
    );

    // The members are global-augmentation declarations, NOT file-surface
    // symbols — they must not leak into file-scope `type_symbols`.
    assert!(
        !env.type_symbols.contains_key("JSX.IntrinsicElements"),
        "declare-global namespace members must NOT enter file-scope type_symbols"
    );
    assert!(
        !env.type_symbols.contains_key("JSX.Element"),
        "declare-global namespace members must NOT enter file-scope type_symbols"
    );

    // A single block contributes exactly one decl whose body lowers the two
    // intrinsic members structurally.
    let group = &env.augmentation_scopes[&key_intrinsic];
    assert_eq!(
        group.contributors.len(),
        1,
        "a single declare-global block contributes one decl"
    );
    match &group.primary().body {
        TypeExpr::Object(obj) => {
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|m| match m {
                    ObjectMember::Property(p) => Some(p.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"div"), "div member present, got {names:?}");
            assert!(
                names.contains(&"span"),
                "span member present, got {names:?}"
            );
        }
        other => panic!("expected object body for JSX.IntrinsicElements, got {other:?}"),
    }
}

#[test]
fn merges_repeated_declare_global_namespace_jsx_intrinsic_elements() {
    // Two `declare global { namespace JSX { interface IntrinsicElements {...} } }`
    // declarations fold into ONE ordered group under the same `(Global,
    // "JSX.IntrinsicElements")` key, so the existing `MergedDecl` peer-merge
    // unions the `div` member from the first declaration with the `customCard`
    // member from the second downstream. Discriminating: a walk that drops the
    // nested namespace would leave the key absent entirely (zero contributors).
    let env = parse_and_build_env(
        r#"
        export {};
        declare global {
          namespace JSX {
            interface IntrinsicElements {
              div: { id?: string };
            }
          }
        }
        declare global {
          namespace JSX {
            interface IntrinsicElements {
              customCard: { variant?: "primary" | "secondary" };
            }
          }
        }
        "#,
    );

    let key = (
        AugmentationScopeKind::Global,
        "JSX.IntrinsicElements".to_string(),
    );
    let group = &env.augmentation_scopes[&key];
    assert_eq!(
        group.contributors.len(),
        2,
        "two declare-global blocks targeting JSX.IntrinsicElements must append \
         to ONE ordered group (got {})",
        group.contributors.len()
    );

    // The ordered group must carry BOTH blocks' members so the downstream
    // `MergedDecl` peer-merge can union them: the first block's `div` and the
    // second block's `customCard`. Asserting only `len() == 2` would pass even
    // if both contributors carried identical (or empty) bodies — collect every
    // contributor's object members and require the union surface.
    let union_members: Vec<&str> = group
        .contributors
        .iter()
        .flat_map(|decl| match &decl.body {
            TypeExpr::Object(obj) => obj
                .properties
                .iter()
                .filter_map(|m| match m {
                    ObjectMember::Property(p) => Some(p.name.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect();
    assert!(
        union_members.contains(&"div"),
        "the first block's `div` member must be retained in the ordered group, \
         got {union_members:?}"
    );
    assert!(
        union_members.contains(&"customCard"),
        "the second block's `customCard` member must be retained in the ordered \
         group, got {union_members:?}"
    );
}

/// Build the shallow declaration-header index for `source` (mirrors the
/// private `index_for` in `decl_headers_tests`), so a body-builder test can
/// assert header↔body parity on the VALUE-augmentation table without reaching
/// into a sibling test module.
fn header_index_for(source: &str) -> crate::analysis::decl_headers::DeclHeaderIndex {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    assert!(!ret.panicked, "fixture must parse");
    crate::analysis::decl_headers::build_decl_header_index(&ret.program, source)
}

#[test]
fn extracts_declare_global_namespace_jsx_value_into_global_value_augmentation_scope() {
    // `declare global { namespace JSX { export const VERSION: string } }`
    // registers the inner EXPORTED value member under its QUALIFIED `JSX.VERSION`
    // name in the GLOBAL value-augmentation scope — never file-scope
    // `value_symbols` — mirroring the type-side interface/alias path. The JSX
    // typeinfo fixture is interface-only, so this is the discriminating cover for
    // the VALUE arm (`ExportNamedDeclaration` → `VariableDeclaration` in
    // `extract_namespaced_declaration_into_augmentation`, plus its header-index
    // mirror): were that arm removed, `(Global, "JSX.VERSION")` would be absent
    // and these assertions would fail.
    let source = r#"
        export {};
        declare global {
          namespace JSX {
            export const VERSION: string;
          }
        }
        "#;
    let env = parse_and_build_env(source);

    let key = (AugmentationScopeKind::Global, "JSX.VERSION".to_string());

    // (a) registers in the global VALUE-augmentation scope, with the const's
    //     declared body type intact.
    let group = env.augmentation_value_scopes.get(&key).expect(
        "declare-global namespace exported VALUE member must register under its \
         qualified name in the global value-augmentation scope",
    );
    let decl = group.primary();
    assert_eq!(decl.kind, ValueDeclKind::Const);
    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::String)),
        "the augmented const must retain its declared `string` body"
    );

    // (b) does NOT leak into the file-scope value surface (neither the
    //     qualified nor the bare member name).
    assert!(
        !env.value_symbols.contains_key("JSX.VERSION"),
        "declare-global namespace VALUE members must NOT enter file-scope value_symbols"
    );
    assert!(
        !env.value_symbols.contains_key("VERSION"),
        "the unqualified member name must NOT enter file-scope value_symbols either"
    );

    // (c) header↔body parity on the VALUE side: the shallow header index
    //     inventories EXACTLY the same value-augmentation name the whole-env walk
    //     registers — non-trivially (the singleton `JSX.VERSION`, not an empty
    //     set). Mirrors `assert_name_parity`'s augmentation-VALUE table and
    //     discriminates the header-index value mirror independently of the body
    //     builder.
    let index = header_index_for(source);
    let mut env_value_aug: Vec<(String, &str)> = env
        .augmentation_value_scopes
        .keys()
        .map(|(scope, name)| (format!("{scope:?}"), name.as_str()))
        .collect();
    let mut index_value_aug: Vec<(String, &str)> = index
        .augmentation_value_headers
        .iter()
        .flat_map(|(scope, names)| {
            names
                .keys()
                .map(move |name| (format!("{scope:?}"), name.as_str()))
        })
        .collect();
    env_value_aug.sort();
    index_value_aug.sort();
    assert_eq!(
        env_value_aug,
        vec![("Global".to_string(), "JSX.VERSION")],
        "the value-augmentation env walk must be exactly the JSX.VERSION singleton"
    );
    assert_eq!(
        env_value_aug, index_value_aug,
        "augmentation VALUE names must match between the env walk and the header index"
    );
}

// =============================================================================
// Function extraction
// =============================================================================

#[test]
fn extracts_function_declaration() {
    let env =
        parse_and_build_env("function greet(name: string, age?: number): string { return name }");
    assert!(env.value_symbols.contains_key("greet"));
    let decl = env.value_symbols["greet"].primary();
    assert_eq!(decl.kind, ValueDeclKind::Function);
    assert!(decl.signatures.first().is_some());

    let sig = decl.signatures.first().unwrap();
    assert_eq!(sig.parameters.len(), 2);
    assert_eq!(sig.parameters[0].name.as_deref(), Some("name"));
    assert_eq!(
        sig.parameters[0].ty,
        TypeExpr::Primitive(PrimitiveName::String)
    );
    assert!(!sig.parameters[0].optional);
    assert!(sig.parameters[1].optional);
    assert_eq!(
        sig.return_type,
        Some(TypeExpr::Primitive(PrimitiveName::String))
    );
}

#[test]
fn extracts_async_function() {
    let env = parse_and_build_env("async function fetchData(): Promise<string> { return '' }");
    let decl = env.value_symbols["fetchData"].primary();
    assert_eq!(decl.kind, ValueDeclKind::AsyncFunction);
}

// =============================================================================
// Variable extraction
// =============================================================================

#[test]
fn extracts_const_with_type_annotation() {
    let env = parse_and_build_env("const MAX_SIZE: number = 100");
    assert!(env.value_symbols.contains_key("MAX_SIZE"));
    let decl = env.value_symbols["MAX_SIZE"].primary();
    assert_eq!(decl.kind, ValueDeclKind::Const);
    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Number))
    );
}

#[test]
fn extracts_const_arrow_function() {
    let env = parse_and_build_env("const add = (a: number, b: number): number => a + b");
    let decl = env.value_symbols["add"].primary();
    assert!(decl.signatures.first().is_some());
    let sig = decl.signatures.first().unwrap();
    assert_eq!(sig.parameters.len(), 2);
    assert_eq!(
        sig.return_type,
        Some(TypeExpr::Primitive(PrimitiveName::Number))
    );
}

#[test]
fn extracts_const_object_literal() {
    let env = parse_and_build_env(r#"const defaults = { theme: "dark", debug: false }"#);
    let decl = env.value_symbols["defaults"].primary();
    assert!(decl.object_shape.is_some());
    let shape = decl.object_shape.as_ref().unwrap();
    assert_eq!(shape.properties.len(), 2);
}

#[test]
fn extracts_const_asserted_object_literal_without_degrading_to_unknown_const() {
    let env = parse_and_build_env(r#"const theme = { color: { primary: "" } } as const"#);
    let decl = env.value_symbols["theme"].primary();

    assert!(
        decl.object_shape.is_some(),
        "const assertions should preserve the underlying object literal shape"
    );
    assert!(
        matches!(decl.type_annotation, Some(TypeExpr::Object(_))),
        "const assertions should infer the object literal type instead of an opaque const marker, got {:?}",
        decl.type_annotation
    );
}

#[test]
fn extracts_let_variable() {
    let env = parse_and_build_env("let count: number = 0");
    let decl = env.value_symbols["count"].primary();
    assert_eq!(decl.kind, ValueDeclKind::Let);
}

#[test]
fn infers_non_empty_array_element_types() {
    let env = parse_and_build_env("const items = [1, 2, 3]");
    let decl = env.value_symbols["items"].primary();
    let Some(TypeExpr::Array { element, .. }) = decl.type_annotation.as_ref() else {
        panic!(
            "expected inferred array type, got {:?}",
            decl.type_annotation
        );
    };

    assert!(
        !matches!(element.as_ref(), TypeExpr::Primitive(PrimitiveName::Any)),
        "non-empty arrays should not infer Array<any>"
    );
    match element.as_ref() {
        TypeExpr::Primitive(PrimitiveName::Number) => {}
        TypeExpr::Literal(LiteralValue::Number(_)) => {}
        TypeExpr::Union(members) => {
            assert!(
                members.iter().all(|member| matches!(
                    member,
                    TypeExpr::Literal(LiteralValue::Number(_))
                        | TypeExpr::Primitive(PrimitiveName::Number)
                )),
                "array element union should stay numeric, got {members:?}"
            );
        }
        other => panic!("expected numeric element type, got {other:?}"),
    }
}

#[test]
fn infers_mixed_array_element_union() {
    let env = parse_and_build_env(r#"const mixed = [1, "hello", true]"#);
    let decl = env.value_symbols["mixed"].primary();
    let Some(TypeExpr::Array { element, .. }) = decl.type_annotation.as_ref() else {
        panic!(
            "expected inferred array type, got {:?}",
            decl.type_annotation
        );
    };

    let TypeExpr::Union(members) = element.as_ref() else {
        panic!("mixed arrays should infer a union element type, got {element:?}");
    };
    assert!(
        members.iter().any(|member| matches!(
            member,
            TypeExpr::Literal(LiteralValue::Number(_)) | TypeExpr::Primitive(PrimitiveName::Number)
        )),
        "mixed array should include a numeric branch"
    );
    assert!(
        members.iter().any(|member| matches!(
            member,
            TypeExpr::Literal(LiteralValue::String(value)) if value == "hello"
        ) || matches!(
            member,
            TypeExpr::Primitive(PrimitiveName::String)
        )),
        "mixed array should include a string branch"
    );
    assert!(
        members.iter().any(|member| matches!(
            member,
            TypeExpr::Literal(LiteralValue::Boolean(true))
                | TypeExpr::Primitive(PrimitiveName::Boolean)
        )),
        "mixed array should include a boolean branch"
    );
    assert!(
        !members
            .iter()
            .any(|member| matches!(member, TypeExpr::Primitive(PrimitiveName::Any))),
        "mixed arrays should not keep any once element types are known"
    );
}

#[test]
fn infers_array_spread_literal_element_types() {
    let env = parse_and_build_env(r#"const mixed = [...[1, 2], "hello"]"#);
    let decl = env.value_symbols["mixed"].primary();
    let Some(TypeExpr::Array { element, .. }) = decl.type_annotation.as_ref() else {
        panic!(
            "expected inferred array type, got {:?}",
            decl.type_annotation
        );
    };

    let TypeExpr::Union(members) = element.as_ref() else {
        panic!("array spread literal should infer a union element type, got {element:?}");
    };
    assert!(
        members.iter().any(|member| matches!(
            member,
            TypeExpr::Literal(LiteralValue::Number(_)) | TypeExpr::Primitive(PrimitiveName::Number)
        )),
        "spread literal array should contribute numeric element types"
    );
    assert!(
        members.iter().any(|member| matches!(
            member,
            TypeExpr::Literal(LiteralValue::String(value)) if value == "hello"
        ) || matches!(
            member,
            TypeExpr::Primitive(PrimitiveName::String)
        )),
        "array literal should retain the non-spread string branch"
    );
}

#[test]
fn empty_array_stays_any_array() {
    let env = parse_and_build_env("const empty = []");
    let decl = env.value_symbols["empty"].primary();

    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Array {
            element: Arc::new(TypeExpr::Primitive(PrimitiveName::Any)),
            readonly: false,
        })
    );
}

#[test]
fn infers_template_literal_with_expressions_as_string() {
    let env = parse_and_build_env(r#"const name = "world"; const label = `hello ${name}`"#);
    let decl = env.value_symbols["label"].primary();

    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::String))
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Any)),
        "template literals with expressions should not fall back to any"
    );
}

#[test]
fn const_preserves_literal_initializer_type() {
    let env = parse_and_build_env(r#"const greeting = "hello""#);
    let decl = env.value_symbols["greeting"].primary();

    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::string_literal("hello"))
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::String)),
        "const literal initializers should remain literal types"
    );
}

#[test]
fn let_widens_string_literal_initializer() {
    let env = parse_and_build_env(r#"let greeting = "hello""#);
    let decl = env.value_symbols["greeting"].primary();

    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::String))
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::string_literal("hello")),
        "let string initializers should widen away from literal types"
    );
}

#[test]
fn let_widens_number_literal_initializer() {
    let env = parse_and_build_env("let count = 42");
    let decl = env.value_symbols["count"].primary();

    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Number))
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::number_literal(42.0)),
        "let number initializers should widen away from literal types"
    );
}

#[test]
fn let_widens_boolean_literal_initializer() {
    let env = parse_and_build_env("let enabled = true");
    let decl = env.value_symbols["enabled"].primary();

    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Boolean))
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::boolean_literal(true)),
        "let boolean initializers should widen away from literal types"
    );
}

#[test]
fn var_widens_string_literal_initializer() {
    let env = parse_and_build_env(r#"var greeting = "hello""#);
    let decl = env.value_symbols["greeting"].primary();

    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::String))
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::string_literal("hello")),
        "var string initializers should widen away from literal types"
    );
}

/// A `let` whose initializer is a constructor-type assertion
/// (`value as new () => { kind: "x" }`) must widen the literal members
/// of the constructor's return type EXACTLY as the function-type
/// equivalent does — and must PRESERVE the `ConstructorType` variant
/// (not flatten it to `Function`).
///
/// `widen_literal_type` runs on analyzer-side lowered IR (the `as`
/// expression lowers via `lower_ts_type`), BEFORE the dispatch lower
/// collapses `Function`/`ConstructorType` to `SemanticNodeData::Function`.
/// Pre-fix the catch-all `_ => expr` arm forwarded the whole
/// `ConstructorType` untouched, so the inner `kind: "x"` literal was
/// silently NOT widened. Discriminator: the inner `kind` member must be
/// `string`, never the `"x"` literal — and the outer node must remain a
/// `ConstructorType`.
#[test]
fn let_widens_constructor_type_return_literal_members() {
    let env = parse_and_build_env(r#"let C = value as new () => { kind: "x" }"#);
    let decl = env.value_symbols["C"].primary();

    let Some(TypeExpr::ConstructorType(function)) = decl.type_annotation.as_ref() else {
        panic!(
            "constructor-type assertion must preserve the ConstructorType variant (never flatten to Function), got {:?}",
            decl.type_annotation
        );
    };
    let Some(return_type) = function.return_type.as_ref() else {
        panic!("constructor type must carry a return type");
    };
    let TypeExpr::Object(obj) = return_type.as_ref() else {
        panic!(
            "constructor return type must remain an object, got {:?}",
            return_type
        );
    };
    let kind_ty = obj.properties.iter().find_map(|member| match member {
        ObjectMember::Property(prop) if prop.name == "kind" => Some(&prop.ty),
        _ => None,
    });
    assert_eq!(
        kind_ty,
        Some(&TypeExpr::Primitive(PrimitiveName::String)),
        "let-bound constructor-return literal member must widen to `string`"
    );
    assert_ne!(
        kind_ty,
        Some(&TypeExpr::string_literal("x")),
        "let-bound constructor-return literal member must NOT stay the `\"x\"` literal"
    );
}

/// Parity guard: the FUNCTION-type equivalent of
/// `let_widens_constructor_type_return_literal_members` must widen the
/// same return literal identically. The two carry the same `FunctionExpr`
/// payload; only the variant tag differs, so widening behaviour must be
/// identical. This pins that the ConstructorType arm mirrors the Function
/// arm rather than diverging.
#[test]
fn let_widens_function_type_return_literal_members_parity() {
    let env = parse_and_build_env(r#"let F = value as () => { kind: "x" }"#);
    let decl = env.value_symbols["F"].primary();

    let Some(TypeExpr::Function(function)) = decl.type_annotation.as_ref() else {
        panic!(
            "function-type assertion must remain a Function, got {:?}",
            decl.type_annotation
        );
    };
    let return_type = function.return_type.as_ref().expect("function return type");
    let TypeExpr::Object(obj) = return_type.as_ref() else {
        panic!("function return type must remain an object, got {return_type:?}");
    };
    let kind_ty = obj.properties.iter().find_map(|member| match member {
        ObjectMember::Property(prop) if prop.name == "kind" => Some(&prop.ty),
        _ => None,
    });
    assert_eq!(
        kind_ty,
        Some(&TypeExpr::Primitive(PrimitiveName::String)),
        "let-bound function-return literal member must widen to `string`"
    );
}

#[test]
fn let_widens_nested_object_literal_properties() {
    let env = parse_and_build_env(r#"let settings = { mode: "dark", nested: { count: 1 } }"#);
    let decl = env.value_symbols["settings"].primary();
    let Some(TypeExpr::Object(obj)) = decl.type_annotation.as_ref() else {
        panic!(
            "expected object type for let object initializer, got {:?}",
            decl.type_annotation
        );
    };

    let mode_ty = obj.properties.iter().find_map(|member| match member {
        ObjectMember::Property(prop) if prop.name == "mode" => Some(&prop.ty),
        _ => None,
    });
    assert_eq!(mode_ty, Some(&TypeExpr::Primitive(PrimitiveName::String)));

    let nested_ty = obj.properties.iter().find_map(|member| match member {
        ObjectMember::Property(prop) if prop.name == "nested" => Some(&prop.ty),
        _ => None,
    });
    let Some(TypeExpr::Object(nested)) = nested_ty else {
        panic!("expected nested object property, got {nested_ty:?}");
    };
    let count_ty = nested.properties.iter().find_map(|member| match member {
        ObjectMember::Property(prop) if prop.name == "count" => Some(&prop.ty),
        _ => None,
    });
    assert_eq!(count_ty, Some(&TypeExpr::Primitive(PrimitiveName::Number)));
}

#[test]
fn let_widens_array_element_literals() {
    let env = parse_and_build_env("let flags = [true, false]");
    let decl = env.value_symbols["flags"].primary();
    let Some(TypeExpr::Array { element, .. }) = decl.type_annotation.as_ref() else {
        panic!(
            "expected array type for let array initializer, got {:?}",
            decl.type_annotation
        );
    };

    assert_eq!(
        element.as_ref(),
        &TypeExpr::Primitive(PrimitiveName::Boolean)
    );
}

// =============================================================================
// satisfies expression inference
// =============================================================================

#[test]
fn satisfies_preserves_underlying_value_type() {
    let env = parse_and_build_env(
        r#"const config = { x: 1, y: "hello" } satisfies { x: number; y: string }"#,
    );
    let decl = env.value_symbols["config"].primary();
    let Some(TypeExpr::Object(obj)) = decl.type_annotation.as_ref() else {
        panic!(
            "satisfies should infer the underlying object literal type, got {:?}",
            decl.type_annotation
        );
    };

    // The value type should have literal/inferred properties from the expression,
    // not abstract types from the satisfies annotation
    let x_prop = obj.properties.iter().find_map(|member| match member {
        ObjectMember::Property(p) if p.name == "x" => Some(&p.ty),
        _ => None,
    });
    assert!(
        x_prop.is_some(),
        "satisfies result should include x property from the value"
    );

    // x should be a number literal (1), not just `number`
    assert!(
        matches!(x_prop.unwrap(), TypeExpr::Literal(LiteralValue::Number(_))),
        "satisfies should preserve literal types from the value expression, got {:?}",
        x_prop,
    );
}

#[test]
fn satisfies_does_not_use_annotation_type() {
    // When using satisfies, the expression type should win, not the annotation
    let env = parse_and_build_env(r#"const label = "hello" satisfies string"#);
    let decl = env.value_symbols["label"].primary();

    // Should be the literal "hello", not widened string
    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::string_literal("hello")),
        "satisfies should preserve the value's literal type"
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::String)),
        "satisfies should not widen to the annotation type"
    );
}

// =============================================================================
// Object spread in extract_object_literal
// =============================================================================

#[test]
fn object_spread_identifier_produces_intersection() {
    let env = parse_and_build_env(r#"const extended = { ...base, extra: true }"#);
    let decl = env.value_symbols["extended"].primary();

    // Should not lose the spread source — at minimum, the explicit props must be present
    // AND the spread source should be represented (as typeof base in an intersection)
    match decl.type_annotation.as_ref() {
        Some(TypeExpr::Intersection(members)) => {
            assert!(
                members.iter().any(|m| matches!(m, TypeExpr::TypeOf(_))),
                "spread identifier should produce a typeof reference in the intersection"
            );
            assert!(
                members.iter().any(|m| matches!(m, TypeExpr::Object(_))),
                "explicit properties should be present in the intersection"
            );
        }
        Some(TypeExpr::Object(obj)) => {
            // At minimum, if we flatten, the explicit property must exist
            assert!(
                obj.properties.iter().any(|member| matches!(
                    member,
                    ObjectMember::Property(p) if p.name == "extra"
                )),
                "explicit property 'extra' must be present"
            );
            panic!(
                "spread source was lost — expected intersection with typeof base, got plain object"
            );
        }
        other => panic!("expected intersection or object, got {other:?}"),
    }
}

#[test]
fn object_spread_object_literal_merges_properties() {
    let env = parse_and_build_env(r#"const merged = { ...{ a: 1, b: 2 }, c: 3 }"#);
    let decl = env.value_symbols["merged"].primary();

    let Some(TypeExpr::Object(obj)) = decl.type_annotation.as_ref() else {
        panic!(
            "expected object type for merged spread, got {:?}",
            decl.type_annotation
        );
    };

    let names: Vec<&str> = obj
        .properties
        .iter()
        .filter_map(|m| match m {
            ObjectMember::Property(p) => Some(p.name.as_str()),
            _ => None,
        })
        .collect();

    assert!(
        names.contains(&"a"),
        "spread object literal property 'a' should be merged"
    );
    assert!(
        names.contains(&"b"),
        "spread object literal property 'b' should be merged"
    );
    assert!(
        names.contains(&"c"),
        "explicit property 'c' should be present"
    );
    assert_eq!(
        names.len(),
        3,
        "should have exactly 3 properties after merge"
    );
}

#[test]
fn object_spread_later_property_overrides_spread_property() {
    let env = parse_and_build_env(r#"const merged = { ...{ a: 1 }, a: "override" }"#);
    let decl = env.value_symbols["merged"].primary();

    let Some(TypeExpr::Object(obj)) = decl.type_annotation.as_ref() else {
        panic!(
            "expected object type for merged spread override, got {:?}",
            decl.type_annotation
        );
    };

    let props: Vec<_> = obj
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(prop) if prop.name == "a" => Some(&prop.ty),
            _ => None,
        })
        .collect();

    assert_eq!(
        props.len(),
        1,
        "later explicit properties should replace earlier spread properties"
    );
    // The later explicit `a: "override"` wins (a STRING, not the spread's
    // number `a: 1`) and — as a fresh object-literal property with no
    // `as const` — widens to its primitive `string` (TS object-literal
    // widening). The string-vs-number discrimination still proves override
    // precedence.
    assert_eq!(props[0], &TypeExpr::Primitive(PrimitiveName::String));
}

#[test]
fn object_spread_later_spread_overrides_earlier_property() {
    let env = parse_and_build_env(r#"const merged = { a: 1, ...{ a: "override" } }"#);
    let decl = env.value_symbols["merged"].primary();

    let Some(TypeExpr::Object(obj)) = decl.type_annotation.as_ref() else {
        panic!(
            "expected object type for merged spread override, got {:?}",
            decl.type_annotation
        );
    };

    let props: Vec<_> = obj
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(prop) if prop.name == "a" => Some(&prop.ty),
            _ => None,
        })
        .collect();

    assert_eq!(
        props.len(),
        1,
        "later spread properties should replace earlier explicit properties"
    );
    // The later spread's `a: "override"` wins (a STRING, not the earlier
    // explicit number `a: 1`) and — as a fresh object-literal property with no
    // `as const` — widens to its primitive `string` (TS object-literal
    // widening). The string-vs-number discrimination still proves override
    // precedence.
    assert_eq!(props[0], &TypeExpr::Primitive(PrimitiveName::String));
}

// =============================================================================
// MemberExpression inference
// =============================================================================

#[test]
fn static_member_expression_infers_typeof_path() {
    let env = parse_and_build_env(r#"const value = obj.foo"#);
    let decl = env.value_symbols["value"].primary();

    match decl.type_annotation.as_ref() {
        Some(TypeExpr::TypeOf(vr)) => {
            assert_eq!(
                vr.path,
                vec!["obj".to_string(), "foo".to_string()],
                "static member expression should produce typeof with dotted path"
            );
        }
        other => panic!("expected TypeOf with path [obj, foo], got {other:?}"),
    }
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Any)),
        "member expression should not degrade to any"
    );
}

#[test]
fn nested_member_expression_infers_deep_typeof_path() {
    let env = parse_and_build_env(r#"const value = a.b.c"#);
    let decl = env.value_symbols["value"].primary();

    match decl.type_annotation.as_ref() {
        Some(TypeExpr::TypeOf(vr)) => {
            assert_eq!(
                vr.path,
                vec!["a".to_string(), "b".to_string(), "c".to_string()],
                "nested member expression should produce typeof with full path"
            );
        }
        other => panic!("expected TypeOf with path [a, b, c], got {other:?}"),
    }
}

#[test]
fn member_on_call_expression_degrades_to_any() {
    // fn().prop — the root is a CallExpression, not an Identifier, so we can't build a simple path
    let env = parse_and_build_env(r#"const value = getObj().prop"#);
    let decl = env.value_symbols["value"].primary();

    // Should not produce a broken partial path like ["prop"] without the root
    // Any or None is acceptable — the key assertion is no broken partial path.
    if let Some(TypeExpr::TypeOf(vr)) = decl.type_annotation.as_ref() {
        panic!(
            "call-rooted member path should not produce TypeOf, got path {:?}",
            vr.path
        );
    }
}

// =============================================================================
// CallExpression inference
// =============================================================================

#[test]
fn simple_call_expression_does_not_degrade_to_any() {
    let env = parse_and_build_env(r#"const result = someFunction()"#);
    let decl = env.value_symbols["result"].primary();

    // For unknown function calls, should produce ReturnType<typeof someFunction>
    // rather than degrading to Any
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Any)),
        "call expression should not degrade to any — should produce ReturnType<typeof fn>"
    );
    // Should be some kind of structured type reference
    assert!(
        decl.type_annotation.is_some(),
        "call expression should produce a type annotation"
    );
}

#[test]
fn method_call_expression_does_not_degrade_to_any() {
    let env = parse_and_build_env(r#"const result = obj.create()"#);
    let decl = env.value_symbols["result"].primary();

    assert!(
        decl.type_annotation.is_some(),
        "method call expression should produce a type, not None (filtered-out Any)"
    );
    assert_ne!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Any)),
        "method call expression should not degrade to any"
    );
}

// =============================================================================
// Class extraction
// =============================================================================

#[test]
fn extracts_class_as_type_and_value() {
    let env = parse_and_build_env(
        r#"
        class Widget {
            readonly id: number;
            name?: string;
            constructor(id: number) {}
            render(): void {}
        }
        "#,
    );
    // Should be in both type and value symbols
    assert!(env.type_symbols.contains_key("Widget"));
    assert!(env.value_symbols.contains_key("Widget"));

    let type_decl = env.type_symbols["Widget"].primary();
    assert_eq!(type_decl.kind, TypeDeclKind::Class);
    match &type_decl.body {
        TypeExpr::Object(obj) => {
            // id, name, render (constructor is not a member)
            assert_eq!(obj.properties.len(), 3);
            let id_prop = obj.properties.iter().find_map(|m| match m {
                ObjectMember::Property(p) if p.name == "id" => Some(p),
                _ => None,
            });
            assert!(id_prop.unwrap().readonly);
        }
        _ => panic!("expected object, got {:?}", type_decl.body),
    }

    let value_decl = env.value_symbols["Widget"].primary();
    assert_eq!(value_decl.kind, ValueDeclKind::Class);
    assert!(value_decl.signatures.first().is_some()); // constructor
}

// =============================================================================
// Export declarations
// =============================================================================

#[test]
fn extracts_exported_types() {
    let env = parse_and_build_env("export type Status = \"active\" | \"inactive\"");
    assert!(env.type_symbols.contains_key("Status"));
}

#[test]
fn extracts_exported_functions() {
    let env = parse_and_build_env("export function helper(): void {}");
    assert!(env.value_symbols.contains_key("helper"));
}

#[test]
fn extracts_exported_interfaces() {
    let env = parse_and_build_env("export interface Config { debug: boolean }");
    assert!(env.type_symbols.contains_key("Config"));
}

#[test]
fn extracts_export_default_object_expression_as_default_value() {
    let env = parse_and_build_env(
        r#"
        export default {
            item: "item",
            body: "body",
        }
        "#,
    );

    let decl = env
        .value_symbols
        .get("default")
        .expect("export default object should register a synthetic default value")
        .primary();

    let ty = decl
        .type_annotation
        .as_ref()
        .expect("default export should preserve a lowered type annotation");
    let TypeExpr::Object(obj) = ty else {
        panic!("expected default export type to be an object, got {ty:?}");
    };

    let names: Vec<&str> = obj
        .properties
        .iter()
        .filter_map(|member| match member {
            ObjectMember::Property(prop) => Some(prop.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(names.contains(&"item"));
    assert!(names.contains(&"body"));
}

// =============================================================================
// Negative tests
// =============================================================================

#[test]
fn no_type_symbols_for_plain_variables() {
    let env = parse_and_build_env("const x = 42");
    assert!(!env.type_symbols.contains_key("x"));
    assert!(env.value_symbols.contains_key("x"));
}

#[test]
fn no_value_symbols_for_type_aliases() {
    let env = parse_and_build_env("type Foo = string");
    assert!(env.type_symbols.contains_key("Foo"));
    assert!(!env.value_symbols.contains_key("Foo"));
}

#[test]
fn parse_and_build_env_preserves_union_type_aliases_with_local_interface_refs() {
    let env = parse_and_build_env(
        r#"
export interface St { path: string }
export interface vt { name: string }
type RouteLocationRaw = string | St | vt
export { RouteLocationRaw as Lt, St, vt }
"#,
    );

    let route = env
        .type_symbols
        .get("RouteLocationRaw")
        .expect("RouteLocationRaw alias should be registered")
        .primary();
    let TypeExpr::Union(types) = &route.body else {
        panic!(
            "RouteLocationRaw should stay a union before evaluation, got {:?}",
            route.body
        );
    };
    assert!(
        types
            .iter()
            .any(|ty| matches!(ty, TypeExpr::Primitive(PrimitiveName::String))),
        "RouteLocationRaw should preserve its string branch, got {:?}",
        route.body
    );
    assert!(
        types.iter().any(|ty| {
            matches!(ty, TypeExpr::Ref { name, .. } if name.as_ref() == "St")
                || matches!(
                    ty,
                    TypeExpr::Object(shape)
                        if shape.properties.iter().any(|member| matches!(member, ObjectMember::Property(property) if property.name == "path"))
                )
        }),
        "RouteLocationRaw should preserve its path-like branch, got {:?}",
        route.body
    );
    assert!(
        types.iter().any(|ty| {
            matches!(ty, TypeExpr::Ref { name, .. } if name.as_ref() == "vt")
                || matches!(
                    ty,
                    TypeExpr::Object(shape)
                        if shape.properties.iter().any(|member| matches!(member, ObjectMember::Property(property) if property.name == "name"))
                )
        }),
        "RouteLocationRaw should preserve its name-like branch, got {:?}",
        route.body
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Discriminating invariant: `expand_macro_types_impl_with_expander`
// reads `field.type_expr` / `field.payload_expr` / `binding.binding_expr`
// directly, never reparsing `type_annotation` text. The probe is to
// populate every typed field with a structural shape that the matching
// raw `*_annotation` text does NOT describe — if expansion ever falls
// back to reparsing the text it would produce the WRONG shape; the
// expected behaviour is that it walks the typed form and the closure
// receives the producer-supplied expression unchanged.
// ───────────────────────────────────────────────────────────────────────────
fn passthrough_expander(
) -> impl FnMut(FieldExpansionContext, &TypeExpr) -> ExpansionResult<ExpandedNormalizedExpr> {
    |_ctx, expr| ExpansionResult::exact(ExpandedNormalizedExpr { expr: expr.clone() })
}

fn make_synth_typed_prop(name: &str, typed: TypeExpr) -> AnalyzedPropField {
    AnalyzedPropField {
        name: name.to_string(),
        is_optional: false,
        span: verter_span::Span::default(),
        // `type_annotation` text deliberately does NOT describe the typed
        // shape: a regression that reparses the text would produce a
        // different structure; the typed form must survive end-to-end.
        type_annotation: Some("garbage<<<unparseable".to_string()),
        type_expr: Some(typed),
        type_expr_scope: Some(TypeExprScope::new("test:fixture")),
        description: None,
        tags: Vec::new(),
        resolution_source: TypeResolutionSource::Rust,
        resolution_error: None,
        declared_in_macro_type_arg: false,
    }
}

fn make_synth_typed_emit(name: &str, typed: TypeExpr) -> AnalyzedEmitField {
    AnalyzedEmitField {
        name: name.to_string(),
        span: verter_span::Span::default(),
        payload_type: Some("garbage<<<unparseable".to_string()),
        payload_expr: Some(typed),
        payload_expr_scope: Some(TypeExprScope::new("test:fixture")),
        description: None,
        tags: Vec::new(),
    }
}

fn make_synth_typed_slot(
    name: &str,
    binding_name: &str,
    binding_typed: TypeExpr,
) -> AnalyzedSlotField {
    AnalyzedSlotField {
        name: name.to_string(),
        is_required: false,
        span: verter_span::Span::default(),
        bindings: vec![AnalyzedSlotFieldBinding {
            name: binding_name.to_string(),
            type_annotation: Some("garbage<<<unparseable".to_string()),
            span: verter_span::Span::default(),
            binding_expr: Some(binding_typed),
            binding_expr_scope: Some(TypeExprScope::new("test:fixture")),
        }],
        return_type: None,
        description: None,
        tags: Vec::new(),
        return_expr: None,
        return_expr_scope: None,
    }
}

fn make_synth_macro(
    kind: AnalyzedMacroKind,
    props: Vec<AnalyzedPropField>,
    emits: Vec<AnalyzedEmitField>,
    slots: Vec<AnalyzedSlotField>,
) -> AnalyzedMacro {
    AnalyzedMacro {
        kind,
        is_type_based: true,
        type_references: Vec::new(),
        binding_name: None,
        model_name: None,
        has_inherit_attrs_false: false,
        prop_fields: props,
        emit_fields: emits,
        slot_fields: slots,
        default_keys: Vec::new(),
        default_values: Vec::new(),
        expose_fields: Vec::new(),
        resolved_local_types: Vec::new(),
        parsed_type_argument: None,
        parsed_type_argument_scope: None,
        span: verter_span::Span::default(),
    }
}

#[test]
fn expand_macro_types_reads_prop_field_type_expr_directly_without_reparse() {
    // A shape the producer captured (e.g. via cross-file external
    // resolution) that text reparsing cannot reproduce.
    let typed_indexed_access = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::Ref {
            name: "ImportedAlias".into(),
            type_arguments: Vec::<TypeExpr>::new().into(),
        }),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String("a".to_string()))),
    };

    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineProps,
        vec![make_synth_typed_prop("foo", typed_indexed_access.clone())],
        Vec::new(),
        Vec::new(),
    )];

    let result = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        passthrough_expander(),
    );

    assert_eq!(
        result.props.len(),
        1,
        "the prop field's typed expression should drive expansion, got {result:?}"
    );
    assert_eq!(
        result.props[0].r#type, typed_indexed_access,
        "expand_macro_types_impl_with_expander must consume field.type_expr directly, not reparse type_annotation"
    );
    assert_eq!(
        result.props[0].raw_type.as_deref(),
        Some("garbage<<<unparseable"),
        "raw_type passthrough should preserve the original annotation text"
    );

    // Negative discrimination: prove the typed shape differs from what
    // the text parser would have produced. If they happened to coincide,
    // the test would not be characterising the typed-read change.
    let from_text =
        crate::analysis::jsdoc::parse_jsdoc_tag_type_payload("garbage<<<unparseable", None);
    assert_ne!(
        from_text, typed_indexed_access,
        "annotation text MUST NOT round-trip back to the typed shape; otherwise the test does not discriminate"
    );
}

#[test]
fn expand_macro_types_reads_emit_payload_expr_directly_without_reparse() {
    let typed_tuple = TypeExpr::Tuple {
        elements: vec![TupleElement {
            label: Some("payload".to_string()),
            ty: TypeExpr::Ref {
                name: "ImportedPayload".into(),
                type_arguments: Vec::<TypeExpr>::new().into(),
            },
            optional: false,
            rest: false,
        }]
        .into(),
        readonly: false,
    };

    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineEmits,
        Vec::new(),
        vec![make_synth_typed_emit("update", typed_tuple.clone())],
        Vec::new(),
    )];

    let result = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        passthrough_expander(),
    );

    assert_eq!(
        result.emits.len(),
        1,
        "emit should expand from payload_expr"
    );
    assert_eq!(
        result.emits[0].r#type, typed_tuple,
        "expand_macro_types_impl_with_expander must consume field.payload_expr directly"
    );

    let from_text =
        crate::analysis::jsdoc::parse_jsdoc_tag_type_payload("garbage<<<unparseable", None);
    assert_ne!(from_text, typed_tuple);
}

#[test]
fn expand_macro_types_reads_slot_binding_expr_directly_without_reparse() {
    let typed_indexed_access = TypeExpr::IndexedAccess {
        object: Arc::new(TypeExpr::Ref {
            name: "SlotProps".into(),
            type_arguments: Vec::<TypeExpr>::new().into(),
        }),
        index: Arc::new(TypeExpr::Literal(LiteralValue::String("item".to_string()))),
    };

    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineSlots,
        Vec::new(),
        Vec::new(),
        vec![make_synth_typed_slot(
            "default",
            "item",
            typed_indexed_access.clone(),
        )],
    )];

    let result = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        passthrough_expander(),
    );

    assert_eq!(
        result.slot_bindings.len(),
        1,
        "slot binding should expand from binding_expr"
    );
    assert_eq!(result.slot_bindings[0].name, "default.item");
    assert_eq!(
        result.slot_bindings[0].r#type, typed_indexed_access,
        "expand_macro_types_impl_with_expander must consume binding.binding_expr directly"
    );

    let from_text =
        crate::analysis::jsdoc::parse_jsdoc_tag_type_payload("garbage<<<unparseable", None);
    assert_ne!(from_text, typed_indexed_access);
}

#[test]
fn expand_macro_types_skips_field_when_typed_form_is_absent_or_unknown() {
    // Producer left `type_expr` unset — the function does NOT fall back
    // to reparsing `type_annotation` text. The expansion vector stays
    // empty.
    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineProps,
        vec![AnalyzedPropField {
            name: "foo".to_string(),
            is_optional: false,
            span: verter_span::Span::default(),
            type_annotation: Some("string".to_string()),
            type_expr: None,
            type_expr_scope: None,
            description: None,
            tags: Vec::new(),
            resolution_source: TypeResolutionSource::Rust,
            resolution_error: None,
            declared_in_macro_type_arg: false,
        }],
        Vec::new(),
        Vec::new(),
    )];

    let result = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        passthrough_expander(),
    );

    assert!(
        result.props.is_empty(),
        "no typed form ⇒ no expansion (would have been non-empty if reparse fallback existed); got {result:?}"
    );
}

#[test]
fn expand_macro_types_threads_field_kind_and_path_through_closure() {
    let typed_string = TypeExpr::Primitive(PrimitiveName::String);
    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineProps,
        vec![make_synth_typed_prop("alpha", typed_string.clone())],
        Vec::new(),
        Vec::new(),
    )];

    let mut captured: Vec<(FieldKind, Vec<String>)> = Vec::new();
    let _ = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        |ctx, expr| {
            let path: Vec<String> = ctx
                .output_path
                .iter()
                .map(|seg| match seg {
                    PathSegment::Member(name) => name.to_string(),
                })
                .collect();
            captured.push((ctx.kind, path));
            ExpansionResult::exact(ExpandedNormalizedExpr { expr: expr.clone() })
        },
    );

    assert_eq!(captured, vec![(FieldKind::Prop, vec!["alpha".to_string()])]);
}

// ── W1.1c: `ExpandedField.shallow_type_expr` carries the analyzer-side
//          shallow typed sidecar through the expander ──
//
// The producer at `expand_macro_types_impl_with_expander` reads
// `field.type_expr` / `field.payload_expr` / `binding.binding_expr`
// (shallow, analyzer-populated) and stamps each onto
// `ExpandedField.shallow_type_expr` (+ paired scope). Pre-W1.1c the
// field did not exist; consumers fell back to reparsing `raw_type`.
// Post-W1.1c the bare alias `Ref` is preserved alongside the
// (potentially distinct) post-expansion `r#type`.

#[test]
fn expand_macro_types_props_publish_shallow_type_expr_from_prop_field_typed_form() {
    // The analyzer captured a bare `Ref` for `foo: ImportedAlias`. The
    // passthrough expander leaves `r#type` equal to the input, but the
    // discriminating assertion is that `shallow_type_expr` is
    // independently populated from the producer's analyzer-side
    // `field.type_expr` and survives all the way to the `ExpandedField`.
    let bare_ref = TypeExpr::Ref {
        name: "ImportedAlias".into(),
        type_arguments: Vec::<TypeExpr>::new().into(),
    };
    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineProps,
        vec![make_synth_typed_prop("foo", bare_ref.clone())],
        Vec::new(),
        Vec::new(),
    )];

    let result = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        passthrough_expander(),
    );

    assert_eq!(result.props.len(), 1);
    // Discriminator: pre-W1.1c the field did not exist; post-W1.1c it
    // carries the bare alias `Ref` from `field.type_expr`.
    assert_eq!(
        result.props[0].shallow_type_expr.as_ref(),
        Some(&bare_ref),
        "shallow_type_expr must surface the analyzer-side bare alias Ref directly"
    );
    // Pairing invariant: scope present iff expr present.
    assert!(
        result.props[0].shallow_type_expr_scope.is_some(),
        "shallow_type_expr_scope must be populated when shallow_type_expr is Some"
    );
    assert_eq!(
        result.props[0]
            .shallow_type_expr_scope
            .as_ref()
            .map(|s| s.as_str()),
        Some("test:fixture"),
        "shallow_type_expr_scope must inherit the analyzer field's scope"
    );
}

#[test]
fn expand_macro_types_emits_publish_shallow_type_expr_from_emit_field_typed_form() {
    let bare_ref = TypeExpr::Ref {
        name: "ImportedPayload".into(),
        type_arguments: Vec::<TypeExpr>::new().into(),
    };
    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineEmits,
        Vec::new(),
        vec![make_synth_typed_emit("update", bare_ref.clone())],
        Vec::new(),
    )];

    let result = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        passthrough_expander(),
    );

    assert_eq!(result.emits.len(), 1);
    assert_eq!(
        result.emits[0].shallow_type_expr.as_ref(),
        Some(&bare_ref),
        "shallow_type_expr must surface the analyzer-side bare alias Ref for emits"
    );
    assert!(result.emits[0].shallow_type_expr_scope.is_some());
    assert_eq!(
        result.emits[0]
            .shallow_type_expr_scope
            .as_ref()
            .map(|s| s.as_str()),
        Some("test:fixture")
    );
}

/// R21-F1 c3 discriminating test for the
/// `expand_macro_types_impl_with_expander` propagation step
/// (`AnalyzedPropField → ExpandedField`).
///
/// Constructs an `AnalyzedMacro` with two `AnalyzedPropField` entries:
/// one with `declared_in_macro_type_arg = true` (the parser-side
/// fact for an own-body member of the macro T) and one with `false`
/// (a member reached via heritage from outside the macro T body).
///
/// Asserts the resulting `ExpandedField` entries carry the SAME
/// per-field fact through the expander. If the prop-field push site
/// in `expand_macro_types_impl_with_expander` is reverted to
/// hardcode `declared_in_macro_type_arg: false`, the own-body field's
/// flag collapses to `false` and the assertion FAILS — the test
/// discriminates the c3 fix.
#[test]
fn r21_c3_expand_macro_types_propagates_declared_in_macro_type_arg_per_field() {
    let typed_string = TypeExpr::Primitive(PrimitiveName::String);

    // Build two AnalyzedPropFields differing only in the structural
    // own-body flag.
    let mut own_body = make_synth_typed_prop("own_body", typed_string.clone());
    own_body.declared_in_macro_type_arg = true;

    let mut heritage = make_synth_typed_prop("heritage", typed_string.clone());
    heritage.declared_in_macro_type_arg = false;

    let macros = vec![make_synth_macro(
        AnalyzedMacroKind::DefineProps,
        vec![own_body, heritage],
        Vec::new(),
        Vec::new(),
    )];

    let result = expand_macro_types_impl_with_expander(
        &macros,
        None,
        &[],
        None,
        MacroExpansionScope::Full,
        passthrough_expander(),
    );

    assert_eq!(
        result.props.len(),
        2,
        "both AnalyzedPropField inputs must reach the ExpandedField output"
    );
    let own_body_field = result
        .props
        .iter()
        .find(|f| f.name == "own_body")
        .expect("own_body ExpandedField present");
    let heritage_field = result
        .props
        .iter()
        .find(|f| f.name == "heritage")
        .expect("heritage ExpandedField present");

    assert!(
        own_body_field.declared_in_macro_type_arg,
        "own_body's declared_in_macro_type_arg=true MUST propagate from \
         AnalyzedPropField to ExpandedField. Discriminator: reverting the \
         prop push in expand_macro_types_impl_with_expander to hardcode \
         `false` flips this to false."
    );
    assert!(
        !heritage_field.declared_in_macro_type_arg,
        "heritage's declared_in_macro_type_arg=false MUST propagate \
         unchanged — the expander must not synthesize true for fields the \
         analyzer marked as heritage."
    );
}

// =============================================================================
// Enum extraction (dual-space: value-side member name→value pairs + type-side value union)
// =============================================================================

fn enum_member_value<'a>(env: &'a EvalEnv, enum_name: &str, member: &str) -> &'a TypeExpr {
    let value = env.value_symbols[enum_name].primary();
    assert_eq!(
        value.kind,
        ValueDeclKind::Enum,
        "value decl must be an enum"
    );
    // The member inventory carries the NAME for every member plus an
    // `EnumMemberValue` (`Folded` = a statically folded literal, `Deferred` =
    // degraded); this helper asserts the member has a FOLDABLE value, so it
    // unwraps through `folded_literal`.
    value
        .enum_members
        .as_ref()
        .expect("enum value decl carries the ordered member inventory")
        .iter()
        .find(|(name, _)| name == member)
        .and_then(|(_, value)| value.folded_literal())
        .unwrap_or_else(|| panic!("enum {enum_name} has no foldable member {member}"))
}

/// The FOLDABLE member NAMES of an enum, in source order — the names whose
/// value Verter statically folded (`EnumMemberValue::Folded`). A member whose
/// value is deferred is OMITTED here, so this is the authority for which members
/// survived value folding. (The FULL member-NAME set — including deferred-value
/// members — is the presence rail; see `merged_enum_member_names`.)
fn enum_member_names<'a>(env: &'a EvalEnv, enum_name: &str) -> Vec<&'a str> {
    env.value_symbols[enum_name]
        .primary()
        .enum_members
        .as_ref()
        .expect("enum value decl carries the ordered member inventory")
        .iter()
        .filter(|(_, value)| value.folded_literal().is_some())
        .map(|(name, _)| name.as_str())
        .collect()
}

#[test]
fn extracts_numeric_enum_member_map_with_auto_increment() {
    let env = parse_and_build_env("export enum Color { Red, Green, Blue }");
    assert!(
        env.value_symbols.contains_key("Color"),
        "enum registers a VALUE symbol (the eval-env enum arm is the producer)"
    );
    let members = env.value_symbols["Color"]
        .primary()
        .enum_members
        .as_ref()
        .expect("numeric enum carries enum_members");
    assert_eq!(members.len(), 3);
    assert_eq!(
        *enum_member_value(&env, "Color", "Red"),
        TypeExpr::number_literal(0.0)
    );
    assert_eq!(
        *enum_member_value(&env, "Color", "Green"),
        TypeExpr::number_literal(1.0)
    );
    assert_eq!(
        *enum_member_value(&env, "Color", "Blue"),
        TypeExpr::number_literal(2.0)
    );
}

#[test]
fn extracts_string_enum_member_map() {
    let env = parse_and_build_env(
        "export enum Status { Idle = \"idle\", Active = \"active\", Done = \"done\" }",
    );
    assert_eq!(
        *enum_member_value(&env, "Status", "Idle"),
        TypeExpr::string_literal("idle")
    );
    assert_eq!(
        *enum_member_value(&env, "Status", "Active"),
        TypeExpr::string_literal("active")
    );
    assert_eq!(
        *enum_member_value(&env, "Status", "Done"),
        TypeExpr::string_literal("done")
    );
}

#[test]
fn const_enum_members_lower_identically_to_a_regular_enum() {
    let env = parse_and_build_env("export const enum Direction { Up = \"UP\", Down = \"DOWN\" }");
    // The `const` modifier does NOT change the type-level value.
    assert_eq!(
        *enum_member_value(&env, "Direction", "Up"),
        TypeExpr::string_literal("UP")
    );
    assert_eq!(
        *enum_member_value(&env, "Direction", "Down"),
        TypeExpr::string_literal("DOWN")
    );
}

#[test]
fn numeric_enum_resumes_auto_increment_after_explicit_value() {
    // TS rule: a bare member is `previous numeric + 1`; an explicit numeric
    // reseeds the counter. So A=5, B=6, C=10, D=11.
    let env = parse_and_build_env("export enum E { A = 5, B, C = 10, D }");
    assert_eq!(
        *enum_member_value(&env, "E", "A"),
        TypeExpr::number_literal(5.0)
    );
    assert_eq!(
        *enum_member_value(&env, "E", "B"),
        TypeExpr::number_literal(6.0)
    );
    assert_eq!(
        *enum_member_value(&env, "E", "C"),
        TypeExpr::number_literal(10.0)
    );
    assert_eq!(
        *enum_member_value(&env, "E", "D"),
        TypeExpr::number_literal(11.0)
    );
}

#[test]
fn enum_type_space_is_union_of_member_value_literals() {
    let env = parse_and_build_env("export enum Color { Red, Green, Blue }");
    // An enum is dual-space: it still registers a TYPE symbol (kind Alias) so
    // it resolves through the shared type demand path.
    assert!(
        env.type_symbols.contains_key("Color"),
        "enum registers a TYPE symbol"
    );
    let decl = env.type_symbols["Color"].primary();
    assert_eq!(decl.kind, TypeDeclKind::Alias);
    // The eager eval-env type body is a NON-SERVED placeholder, NOT the
    // value-literal union: a per-declaration walk cannot see same-name merged
    // contributors, so the union is derived ONCE from the value members by
    // `ValueDeclGroup::enum_type_union` (the single source of truth) and the
    // lazily-served body memo defers to it. (An eager body union here would be
    // last-wins for merged enums — a divergence trap this placeholder avoids.)
    assert!(
        !matches!(decl.body, TypeExpr::Union(_)),
        "the eager enum type body must be a non-served placeholder, not the value-literal \
         union; got {:?}",
        decl.body
    );

    // The PRODUCTION type-space derivation (the body the memo actually
    // serves): the union of the member value literals, from the SAME merged
    // value-space member set.
    let union = env.value_symbols["Color"]
        .enum_type_union()
        .expect("enum value group derives a type union");
    match &union {
        TypeExpr::Union(types) => {
            assert_eq!(types.len(), 3);
            assert!(types.contains(&TypeExpr::number_literal(0.0)));
            assert!(types.contains(&TypeExpr::number_literal(1.0)));
            assert!(types.contains(&TypeExpr::number_literal(2.0)));
            // Negative assertion: the union is the member VALUES, NOT the
            // member NAMES (a value-from-name confusion would put "Red" here).
            assert!(!types.contains(&TypeExpr::string_literal("Red")));
        }
        other => panic!("expected value-literal union, got {other:?}"),
    }
}

#[test]
fn string_enum_type_space_is_union_of_string_value_literals() {
    let env = parse_and_build_env(
        "export enum Status { Idle = \"idle\", Active = \"active\", Done = \"done\" }",
    );
    // The eager type body is the non-served placeholder; the served type union
    // is derived from the value members by `ValueDeclGroup::enum_type_union`.
    assert!(
        !matches!(
            env.type_symbols["Status"].primary().body,
            TypeExpr::Union(_)
        ),
        "the eager enum type body must be a non-served placeholder, not the union"
    );
    let union = env.value_symbols["Status"]
        .enum_type_union()
        .expect("enum value group derives a type union");
    match &union {
        TypeExpr::Union(types) => {
            assert_eq!(types.len(), 3);
            assert!(types.contains(&TypeExpr::string_literal("idle")));
            assert!(types.contains(&TypeExpr::string_literal("active")));
            assert!(types.contains(&TypeExpr::string_literal("done")));
        }
        other => panic!("expected string value-literal union, got {other:?}"),
    }
}

#[test]
fn negative_numeric_enum_initializer_folds_signed_literal_and_reseeds_auto_increment() {
    // TS represents `A = -1` as a unary `-` over a numeric literal. The
    // member must fold to `number_literal(-1)` (NOT be skipped), and the
    // auto-increment must continue from the signed value: after `B = -3`, a
    // bare `C` is `-3 + 1 = -2`.
    let env = parse_and_build_env("export enum E { A = -1, B = -3, C }");
    assert_eq!(
        *enum_member_value(&env, "E", "A"),
        TypeExpr::number_literal(-1.0),
        "negative initializer must fold to the signed literal, not be skipped"
    );
    assert_eq!(
        *enum_member_value(&env, "E", "B"),
        TypeExpr::number_literal(-3.0)
    );
    assert_eq!(
        *enum_member_value(&env, "E", "C"),
        TypeExpr::number_literal(-2.0),
        "auto-increment resumes from the previous (negative) numeric"
    );
    // A leading unary `+` is also a numeric initializer.
    let env_plus = parse_and_build_env("export enum P { A = +5, B }");
    assert_eq!(
        *enum_member_value(&env_plus, "P", "A"),
        TypeExpr::number_literal(5.0)
    );
    assert_eq!(
        *enum_member_value(&env_plus, "P", "B"),
        TypeExpr::number_literal(6.0)
    );
}

#[test]
fn enum_members_preserve_source_declaration_order() {
    // TS enum members are ordered. A non-alphabetical declaration order must
    // be preserved verbatim (the value-space `enum_members` is a source-order
    // `Vec`, not a hash-ordered map) so `typeof Enum` surfaces members in
    // declaration order and auto-increment positions stay correct.
    let env = parse_and_build_env("export enum E { Zed, Alpha, Mid }");
    let members = env.value_symbols["E"]
        .primary()
        .enum_members
        .as_ref()
        .expect("enum carries enum_members");
    let order: Vec<&str> = members.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        order,
        vec!["Zed", "Alpha", "Mid"],
        "enum members must be in source declaration order, not sorted/hashed"
    );
}

#[test]
fn merged_same_name_enum_unions_all_contributor_members_in_both_spaces() {
    // TS declaration merging: two same-name `enum E` bodies contribute to one
    // enum. The resolver-visible member set MUST be the UNION of all
    // contributors — using only the last (last-wins `primary()`) contributor
    // would drop the earlier declaration's members (`A`, `B`).
    let env = parse_and_build_env("export enum E { A = 1, B = 2 }\nexport enum E { C = 3, D = 4 }");

    // Discriminator: `primary()` (the last contributor) sees ONLY its own
    // members — the merge is exactly what recovers `A`, `B` from the earlier
    // declaration.
    let primary_only: Vec<&str> = env.value_symbols["E"]
        .primary()
        .enum_members
        .as_ref()
        .expect("enum")
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(
        primary_only,
        vec!["C", "D"],
        "primary() alone is last-contributor-only — the merge is what unions all four"
    );

    // Value space: the merged member set is the union, in source order.
    let merged = env.value_symbols["E"]
        .merged_enum_members()
        .expect("merged enum members");
    let merged_names: Vec<&str> = merged.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        merged_names,
        vec!["A", "B", "C", "D"],
        "merged value-space enum_members must contain ALL contributors' members in source order"
    );
    assert_eq!(merged[0].1, TypeExpr::number_literal(1.0));
    assert_eq!(merged[3].1, TypeExpr::number_literal(4.0));

    // Type space: assert on the PRODUCTION derivation — `enum_type_union`, the
    // single source of truth the lazily-served body memo serves — NOT a union
    // rebuilt from `merged` inside the test, which would be tautological (it
    // would exercise `TypeExpr::union` + the test's own data, not the
    // production fold, and would still pass if `enum_type_union` were broken).
    // The derived union must carry every contributor's value literal, never
    // just the last declaration's.
    let type_union = env.value_symbols["E"]
        .enum_type_union()
        .expect("merged enum value group derives a type union");
    match &type_union {
        TypeExpr::Union(types) => {
            assert_eq!(
                types.len(),
                4,
                "type union must carry all 4 member literals"
            );
            for literal in [1.0, 2.0, 3.0, 4.0] {
                assert!(
                    types.contains(&TypeExpr::number_literal(literal)),
                    "type union must contain literal {literal}; got {types:?}"
                );
            }
        }
        other => panic!("expected a 4-arm value-literal union, got {other:?}"),
    }
}

#[test]
fn unsupported_initializer_makes_running_auto_increment_unknown_not_fabricated() {
    // `1 << 2` is a computed (binary) initializer Verter cannot statically
    // fold, so `A`'s value is DEFERRED AND the running auto-increment value
    // becomes UNKNOWN. A bare member with an unknown running value (`B`) is
    // therefore ALSO deferred — its foldable value must NOT be fabricated as
    // `0` off a stale counter. An explicit literal (`C = 5`) RESEEDS the
    // counter to KNOWN, so the following bare member `D` folds to `6`. The
    // FOLDABLE rail (`enum_member_names`) therefore holds only `C`/`D`.
    let env = parse_and_build_env("export enum E { A = 1 << 2, B, C = 5, D }");
    let names = enum_member_names(&env, "E");
    assert_eq!(
        names,
        vec!["C", "D"],
        "A (unfoldable) and B (bare after an unknown running value) are deferred, not folded; \
         C/D fold because C = 5 reseeds the counter"
    );
    // Negative: B must NOT be present in the FOLDABLE rail with a fabricated-0
    // literal — its value is deferred, not invented.
    assert!(
        !names.contains(&"B"),
        "a bare member after an unknown running value must NOT be fabricated into the foldable rail"
    );
    assert_eq!(
        *enum_member_value(&env, "E", "C"),
        TypeExpr::number_literal(5.0)
    );
    assert_eq!(
        *enum_member_value(&env, "E", "D"),
        TypeExpr::number_literal(6.0),
        "auto-increment resumes from the explicit C = 5, not from the deferred A/B"
    );
}

#[test]
fn bitwise_not_initializer_defers_member_and_following_bare_members() {
    // `~1` is a bitwise (unary, non-`±`) initializer — unfoldable. `B`'s value
    // is deferred and the running value becomes UNKNOWN, so the following bare
    // `C` is ALSO deferred: its value would be `previous + 1`, and the previous
    // member's value is unknown. Only `A = 0` folds, so the FOLDABLE rail holds
    // `A` alone; a deferred member's foldable value is never fabricated off a
    // stale counter.
    let env = parse_and_build_env("export enum F { A = 0, B = ~1, C }");
    let names = enum_member_names(&env, "F");
    assert_eq!(
        names,
        vec!["A"],
        "B (~1 unfoldable) and C (bare after an unknown running value) are both deferred, not folded"
    );
    assert!(
        !names.contains(&"C"),
        "a bare member after an unknown running value must NOT be fabricated into the foldable rail"
    );
    assert_eq!(
        *enum_member_value(&env, "F", "A"),
        TypeExpr::number_literal(0.0)
    );
}

#[test]
fn computed_string_enum_member_names_resolve_via_static_name_and_fold_values() {
    // A computed STRING member name (`["A"]`) carries a STATIC identity,
    // resolved through the SAME `static_name` helper the production
    // `index_enum` header walk uses (name resolution is SHARED, not forked, so
    // the two construction paths can never disagree on the member-NAME set).
    // With a foldable value it projects `E.A == 1` / `E.B == 2` — a computed
    // name is recorded, never dropped, so the eval-env walk and `index_enum`
    // agree on the member-NAME set.
    let env = parse_and_build_env("export enum E { [\"A\"] = 1, [\"B\"] = 2 }");
    let names = enum_member_names(&env, "E");
    assert_eq!(
        names,
        vec!["A", "B"],
        "computed-string member names resolve to their static identity (NOT dropped)"
    );
    assert_eq!(
        *enum_member_value(&env, "E", "A"),
        TypeExpr::number_literal(1.0)
    );
    assert_eq!(
        *enum_member_value(&env, "E", "B"),
        TypeExpr::number_literal(2.0)
    );
}

#[test]
fn computed_template_enum_member_name_resolves_to_static_single_quasi() {
    // Template-name variant: a computed TEMPLATE member name with a single
    // no-substitution quasi (`` [`X`] ``) ALSO carries a static identity via
    // `static_name`, so it folds `T.X == 7` exactly as `index_enum` records it.
    let env = parse_and_build_env("export enum T { [`X`] = 7 }");
    assert_eq!(enum_member_names(&env, "T"), vec!["X"]);
    assert_eq!(
        *enum_member_value(&env, "T", "X"),
        TypeExpr::number_literal(7.0)
    );
}

#[test]
fn enum_member_inventory_records_all_static_names_folding_or_degrading_each_value() {
    // The member inventory carries the NAME of EVERY statically-named member —
    // including ones whose VALUE is deferred — so the name rail is the SUPERSET
    // the foldable rail filters. For `enum E { A = 1 << 2, B, C = 5, D }`: all
    // four NAMES are recorded; `A` (unfoldable `1 << 2`) and `B` (bare after an
    // unknown running value) are `Deferred`, each carrying the degraded `number`
    // domain; `C = 5` reseeds and `D = 6` fold.
    let env = parse_and_build_env("export enum E { A = 1 << 2, B, C = 5, D }");
    let inventory = env.value_symbols["E"]
        .primary()
        .enum_members
        .as_ref()
        .expect("enum carries the member inventory");
    let all_names: Vec<&str> = inventory.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        all_names,
        vec!["A", "B", "C", "D"],
        "every statically-named member is recorded, even with a deferred value"
    );
    assert_eq!(
        inventory[0].1,
        EnumMemberValue::Deferred(TypeExpr::Primitive(PrimitiveName::Number)),
        "A's value is DEFERRED and degrades to `number` (the numeric `1 << 2`)"
    );
    assert_eq!(
        inventory[1].1,
        EnumMemberValue::Deferred(TypeExpr::Primitive(PrimitiveName::Number)),
        "B is DEFERRED (bare after an unknown running value) and degrades to `number`"
    );
    assert_eq!(
        inventory[2].1,
        EnumMemberValue::Folded(TypeExpr::number_literal(5.0)),
        "C = 5 folds and reseeds the counter"
    );
    assert_eq!(
        inventory[3].1,
        EnumMemberValue::Folded(TypeExpr::number_literal(6.0)),
        "D resumes auto-increment from the reseeded C"
    );

    // The rails derive consistently from this single inventory: the FOLDABLE
    // rail (`merged_enum_members`) is the `Folded` subset, the NAME rail
    // (`merged_enum_member_names`) is the full superset.
    let value_rail = env.value_symbols["E"]
        .merged_enum_members()
        .expect("foldable rail");
    let value_names: Vec<&str> = value_rail.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        value_names,
        vec!["C", "D"],
        "the foldable rail filters out deferred members"
    );
    let presence_names = env.value_symbols["E"]
        .merged_enum_member_names()
        .expect("name rail");
    assert_eq!(
        presence_names,
        vec!["A", "B", "C", "D"],
        "the name rail is the full member-name superset"
    );
}

#[test]
fn all_deferred_numeric_enum_type_union_degrades_to_number_not_never() {
    // `enum Flags { A = 1 << 0, B = 1 << 1 }` — BOTH members carry a computed
    // (deferred) value, so NO member folds to a literal. The enum's TYPE body
    // must NOT collapse to `never` (a confident-wrong bottom type) just because
    // no value folded: a non-empty enum is inhabited. Each deferred
    // numeric-expression member degrades to its narrowest sound domain
    // (`number`), so the deduped union is exactly `number`.
    let env = parse_and_build_env("export enum Flags { A = 1 << 0, B = 1 << 1 }");
    let union = env.value_symbols["Flags"]
        .enum_type_union()
        .expect("enum value group derives a type union");
    assert_ne!(
        union,
        TypeExpr::Primitive(PrimitiveName::Never),
        "an all-deferred NON-EMPTY enum must NOT resolve to `never`"
    );
    assert_eq!(
        union,
        TypeExpr::Primitive(PrimitiveName::Number),
        "two numeric-expression members both degrade to `number`; the deduped \
         union is `number`, not `never` and not the empty bottom"
    );
}

#[test]
fn partial_deferred_enum_type_union_keeps_degraded_arm_for_deferred_member() {
    // `enum P { A = "x", B = someFn() }` — `A` folds to `"x"`, `B`'s value is a
    // computed call (deferred). The TYPE body must NOT narrow to just `"x"`
    // (silently dropping `B` is false absence). It carries the folded `"x"` arm
    // PLUS a DEGRADED arm for `B` — an unclassifiable call initializer proves no
    // narrower domain than `unknown`.
    let env = parse_and_build_env("export enum P { A = \"x\", B = someFn() }");
    let union = env.value_symbols["P"]
        .enum_type_union()
        .expect("enum value group derives a type union");
    match &union {
        TypeExpr::Union(arms) => {
            assert!(
                arms.contains(&TypeExpr::string_literal("x")),
                "the folded `A` literal must remain an arm: {arms:?}"
            );
            assert!(
                arms.iter()
                    .any(|arm| matches!(arm, TypeExpr::Primitive(PrimitiveName::Unknown))),
                "the deferred `B` must contribute a DEGRADED arm, never vanish: {arms:?}"
            );
        }
        other => panic!(
            "the type body must NOT narrow to the single folded literal `\"x\"`; \
             expected a union carrying a degraded arm for `B`, got {other:?}"
        ),
    }
}

#[test]
fn deferred_members_degrade_into_enum_type_union_alongside_folded_literals() {
    // `enum E { A = 1 << 2, B, C = 5, D }` — `A` (computed) and `B` (bare after
    // a deferred running value) are deferred; `C = 5`, `D = 6` fold. The TYPE
    // union must carry the folded literals `5`/`6` AND a degraded `number` arm
    // for the deferred members — never drop `A`/`B`.
    let env = parse_and_build_env("export enum E { A = 1 << 2, B, C = 5, D }");
    let union = env.value_symbols["E"]
        .enum_type_union()
        .expect("enum value group derives a type union");
    let arms = match &union {
        TypeExpr::Union(arms) => arms.clone(),
        other => panic!("expected a union of literal + degraded arms, got {other:?}"),
    };
    assert!(
        arms.contains(&TypeExpr::number_literal(5.0)),
        "the folded `C = 5` literal arm must be present: {arms:?}"
    );
    assert!(
        arms.contains(&TypeExpr::number_literal(6.0)),
        "the folded `D = 6` literal arm must be present: {arms:?}"
    );
    assert!(
        arms.iter()
            .any(|arm| matches!(arm, TypeExpr::Primitive(PrimitiveName::Number))),
        "the deferred `A`/`B` members degrade to a `number` arm: {arms:?}"
    );
}

#[test]
fn tagged_template_enum_member_degrades_to_unknown_not_string() {
    // A TAGGED template (`tag`...``) is a CALL — `tag` is invoked with the
    // template parts and can return ANY type — so `string` is NOT a sound upper
    // bound for its degraded domain; the narrowest sound bound is `unknown`. A
    // PLAIN template literal (no tag) IS a string expression and keeps `string`.
    // Both members are VALUE-deferred (neither folds to a literal), so
    // `degraded_member_domain` classifies each from its initializer kind.
    let env = parse_and_build_env("export enum E { A = tag`x`, B = `y` }");
    let inventory = env.value_symbols["E"]
        .primary()
        .enum_members
        .as_ref()
        .expect("enum carries the member inventory");
    let all_names: Vec<&str> = inventory.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        all_names,
        vec!["A", "B"],
        "both statically-named members are recorded with deferred values"
    );
    assert_eq!(
        inventory[0].1,
        EnumMemberValue::Deferred(TypeExpr::Primitive(PrimitiveName::Unknown)),
        "a TAGGED template is a call returning an arbitrary type; `string` would \
         under-approximate, so `A` degrades to the sound `unknown`"
    );
    assert_eq!(
        inventory[1].1,
        EnumMemberValue::Deferred(TypeExpr::Primitive(PrimitiveName::String)),
        "a PLAIN template literal (no tag) is a string expression; `B` keeps the \
         `string` domain — the soundness fix touches only the tagged case"
    );
}
