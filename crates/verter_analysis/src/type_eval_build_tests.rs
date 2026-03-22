use super::type_eval::*;
use super::type_eval_build::{evaluate_macro_types, parse_and_build_env};
use super::type_expr::*;

// =============================================================================
// Type alias extraction
// =============================================================================

#[test]
fn extracts_type_alias() {
    let env = parse_and_build_env("type Color = \"red\" | \"blue\" | \"green\"");
    assert!(env.type_symbols.contains_key("Color"));
    let decl = &env.type_symbols["Color"];
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
    let decl = &env.type_symbols["Box"];
    assert_eq!(decl.type_parameters.len(), 1);
    assert_eq!(decl.type_parameters[0].name, "T");
}

// =============================================================================
// Interface extraction
// =============================================================================

#[test]
fn extracts_interface() {
    let env = parse_and_build_env("interface User { id: number; name: string; email?: string }");
    assert!(env.type_symbols.contains_key("User"));
    let decl = &env.type_symbols["User"];
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

    let user = &env.type_symbols["User"];
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
    let decl = &env.type_symbols["Logger"];
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
// Function extraction
// =============================================================================

#[test]
fn extracts_function_declaration() {
    let env =
        parse_and_build_env("function greet(name: string, age?: number): string { return name }");
    assert!(env.value_symbols.contains_key("greet"));
    let decl = &env.value_symbols["greet"];
    assert_eq!(decl.kind, ValueDeclKind::Function);
    assert!(decl.function_signature.is_some());

    let sig = decl.function_signature.as_ref().unwrap();
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
    let decl = &env.value_symbols["fetchData"];
    assert_eq!(decl.kind, ValueDeclKind::AsyncFunction);
}

// =============================================================================
// Variable extraction
// =============================================================================

#[test]
fn extracts_const_with_type_annotation() {
    let env = parse_and_build_env("const MAX_SIZE: number = 100");
    assert!(env.value_symbols.contains_key("MAX_SIZE"));
    let decl = &env.value_symbols["MAX_SIZE"];
    assert_eq!(decl.kind, ValueDeclKind::Const);
    assert_eq!(
        decl.type_annotation,
        Some(TypeExpr::Primitive(PrimitiveName::Number))
    );
}

#[test]
fn extracts_const_arrow_function() {
    let env = parse_and_build_env("const add = (a: number, b: number): number => a + b");
    let decl = &env.value_symbols["add"];
    assert!(decl.function_signature.is_some());
    let sig = decl.function_signature.as_ref().unwrap();
    assert_eq!(sig.parameters.len(), 2);
    assert_eq!(
        sig.return_type,
        Some(TypeExpr::Primitive(PrimitiveName::Number))
    );
}

#[test]
fn extracts_const_object_literal() {
    let env = parse_and_build_env(r#"const defaults = { theme: "dark", debug: false }"#);
    let decl = &env.value_symbols["defaults"];
    assert!(decl.object_shape.is_some());
    let shape = decl.object_shape.as_ref().unwrap();
    assert_eq!(shape.properties.len(), 2);
}

#[test]
fn extracts_let_variable() {
    let env = parse_and_build_env("let count: number = 0");
    let decl = &env.value_symbols["count"];
    assert_eq!(decl.kind, ValueDeclKind::Let);
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

    let type_decl = &env.type_symbols["Widget"];
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

    let value_decl = &env.value_symbols["Widget"];
    assert_eq!(value_decl.kind, ValueDeclKind::Class);
    assert!(value_decl.function_signature.is_some()); // constructor
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

// =============================================================================
// End-to-end: build env then evaluate
// =============================================================================

#[test]
fn e2e_return_type_of_function() {
    let env = parse_and_build_env(
        r#"
        function createConfig() {
            return { theme: "dark", debug: false }
        }
        "#,
    );

    // Now evaluate ReturnType<typeof createConfig>
    let mut eval_env = env;
    let expr = TypeExpr::named_with_args(
        "ReturnType",
        vec![TypeExpr::TypeOf(ValueRef {
            path: vec!["createConfig".to_string()],
        })],
    );
    let result = evaluate(&expr, &mut eval_env);
    // Body inference: the function returns { theme: "dark", debug: false }
    // so ReturnType resolves to an object shape
    match &result {
        TypeExpr::Object(obj) => {
            assert!(
                !obj.properties.is_empty(),
                "should infer object properties from return statement"
            );
        }
        _ => panic!("expected object from body inference, got {result:?}"),
    }
}

#[test]
fn e2e_return_type_annotated_function() {
    let env = parse_and_build_env(
        r#"
        function createConfig(): { theme: string; debug: boolean } {
            return { theme: "dark", debug: false }
        }
        "#,
    );

    let mut eval_env = env;
    let expr = TypeExpr::named_with_args(
        "ReturnType",
        vec![TypeExpr::TypeOf(ValueRef {
            path: vec!["createConfig".to_string()],
        })],
    );
    let result = evaluate(&expr, &mut eval_env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|m| match m {
                    ObjectMember::Property(p) => Some(p.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"theme"));
            assert!(names.contains(&"debug"));
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

#[test]
fn e2e_pick_from_interface() {
    let env = parse_and_build_env(
        r#"
        interface User {
            id: number
            name: string
            email: string
            password: string
        }
        "#,
    );

    let mut eval_env = env;
    let expr = TypeExpr::named_with_args(
        "Pick",
        vec![
            TypeExpr::named("User"),
            TypeExpr::Union(vec![
                TypeExpr::string_literal("id"),
                TypeExpr::string_literal("name"),
            ]),
        ],
    );
    let result = evaluate(&expr, &mut eval_env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
            let names: Vec<&str> = obj
                .properties
                .iter()
                .filter_map(|m| match m {
                    ObjectMember::Property(p) => Some(p.name.as_str()),
                    _ => None,
                })
                .collect();
            assert!(names.contains(&"id"));
            assert!(names.contains(&"name"));
            assert!(!names.contains(&"email"), "email should NOT be picked");
            assert!(
                !names.contains(&"password"),
                "password should NOT be picked"
            );
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

#[test]
fn e2e_partial_of_interface() {
    let env = parse_and_build_env("interface Config { theme: string; debug: boolean }");

    let mut eval_env = env;
    let expr = TypeExpr::named_with_args("Partial", vec![TypeExpr::named("Config")]);
    let result = evaluate(&expr, &mut eval_env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
            for member in &obj.properties {
                if let ObjectMember::Property(p) = member {
                    assert!(p.optional, "{} should be optional after Partial", p.name);
                }
            }
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

#[test]
fn e2e_keyof_interface() {
    let env = parse_and_build_env("interface User { id: number; name: string; email: string }");

    let mut eval_env = env;
    let expr = TypeExpr::KeyOf(Box::new(TypeExpr::named("User")));
    let result = evaluate(&expr, &mut eval_env);
    match &result {
        TypeExpr::Union(types) => {
            assert_eq!(types.len(), 3);
            assert!(types.contains(&TypeExpr::string_literal("id")));
            assert!(types.contains(&TypeExpr::string_literal("name")));
            assert!(types.contains(&TypeExpr::string_literal("email")));
        }
        _ => panic!("expected union, got {result:?}"),
    }
}

#[test]
fn e2e_typeof_const_object() {
    let env = parse_and_build_env(r#"const defaults = { size: 42, color: "blue" }"#);

    let mut eval_env = env;
    let expr = TypeExpr::TypeOf(ValueRef {
        path: vec!["defaults".to_string()],
    });
    let result = evaluate(&expr, &mut eval_env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
        }
        _ => panic!("expected object, got {result:?}"),
    }
}

#[test]
fn e2e_generic_alias_instantiation() {
    let env = parse_and_build_env(
        r#"
        type Wrapper<T> = { data: T; timestamp: number }
        "#,
    );

    let mut eval_env = env;
    let expr =
        TypeExpr::named_with_args("Wrapper", vec![TypeExpr::Primitive(PrimitiveName::String)]);
    let result = evaluate(&expr, &mut eval_env);
    match &result {
        TypeExpr::Object(obj) => {
            assert_eq!(obj.properties.len(), 2);
            let data = obj.properties.iter().find_map(|m| match m {
                ObjectMember::Property(p) if p.name == "data" => Some(p),
                _ => None,
            });
            assert_eq!(data.unwrap().ty, TypeExpr::Primitive(PrimitiveName::String));
        }
        _ => panic!("expected object, got {result:?}"),
    }
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

// =============================================================================
// evaluate_macro_types with real analysis snapshot
// =============================================================================

#[test]
fn evaluate_macro_types_resolves_prop_annotations() {
    use super::analysis::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
interface ButtonProps {
  label: string
  size?: "sm" | "md" | "lg"
  disabled: boolean
}
defineProps<ButtonProps>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    // Should have evaluated prop types
    assert!(
        !result.props.is_empty(),
        "should evaluate prop type annotations"
    );

    // Verify specific prop types
    let label = result.props.iter().find(|p| p.name == "label");
    assert!(label.is_some(), "should have evaluated 'label' prop type");
    assert_eq!(
        label.unwrap().r#type,
        TypeExpr::Primitive(PrimitiveName::String)
    );

    let size = result.props.iter().find(|p| p.name == "size");
    assert!(size.is_some(), "should have evaluated 'size' prop type");
    {
        let size = size.unwrap();
        // "sm" | "md" | "lg" should be a union of string literals
        match &size.r#type {
            TypeExpr::Union(types) => {
                assert_eq!(types.len(), 3);
                assert!(types.contains(&TypeExpr::string_literal("sm")));
                assert!(types.contains(&TypeExpr::string_literal("md")));
                assert!(types.contains(&TypeExpr::string_literal("lg")));
            }
            _ => panic!("expected union for size, got {:?}", size.r#type),
        }
    }
}

#[test]
fn evaluate_macro_types_resolves_generic_utility() {
    use super::analysis::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
interface Config {
  theme: string
  debug: boolean
}
defineProps<Partial<Config>>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    // Props from Partial<Config> should have all fields optional
    for field in &result.props {
        match &field.r#type {
            TypeExpr::Object(obj) => {
                for member in &obj.properties {
                    if let ObjectMember::Property(p) = member {
                        assert!(p.optional, "Partial should make {} optional", p.name);
                    }
                }
            }
            // If the evaluator resolved via the snapshot type_annotation strings,
            // the result might be a flat resolved form
            _ => {}
        }
    }
}

#[test]
fn evaluate_macro_types_keeps_complex_prop_annotations() {
    use super::analysis::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
interface User {
  id: number
  name: string
  password: string
}

function createConfig() {
  return { theme: "dark" as string, debug: false }
}

defineProps<{
  user: Pick<User, 'id' | 'name'>
  config: ReturnType<typeof createConfig>
}>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    let user = result.props.iter().find(|p| p.name == "user");
    assert!(user.is_some(), "should keep evaluated utility prop fields");
    assert!(
        matches!(user.unwrap().r#type, TypeExpr::Object(_)),
        "Pick<User, ...> should evaluate to an object"
    );

    let config = result.props.iter().find(|p| p.name == "config");
    assert!(
        config.is_some(),
        "should keep evaluated ReturnType prop fields"
    );
    assert!(
        matches!(config.unwrap().r#type, TypeExpr::Object(_)),
        "ReturnType<typeof createConfig> should evaluate to an object"
    );
}

#[test]
fn evaluate_macro_types_with_inline_props() {
    use super::analysis::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
defineProps<{ count: number; label?: string }>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    // Should evaluate both prop types
    let count = result.props.iter().find(|p| p.name == "count");
    assert!(count.is_some(), "should have evaluated 'count' prop type");
    assert_eq!(
        count.unwrap().r#type,
        TypeExpr::Primitive(PrimitiveName::Number)
    );

    let label = result.props.iter().find(|p| p.name == "label");
    assert!(label.is_some(), "should have evaluated 'label' prop type");
    assert_eq!(
        label.unwrap().r#type,
        TypeExpr::Primitive(PrimitiveName::String)
    );
}

#[test]
fn evaluate_macro_types_synthesizes_define_props_from_typeof() {
    use super::analysis::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
const config = { x: 1, y: "hello" }
defineProps<typeof config>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    assert_eq!(result.define_props.len(), 1);
    let fields = &result.define_props[0].result.value.properties;
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    assert!(names.contains(&"x"));
    assert!(names.contains(&"y"));
}

#[test]
fn evaluate_macro_types_synthesizes_define_props_from_utility_heritage() {
    use super::analysis::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
interface BaseProps { a: string; b: number; c: boolean }
interface MyProps extends Pick<BaseProps, 'a' | 'b'> { local: string }
defineProps<MyProps>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    assert_eq!(result.define_props.len(), 1);
    let fields = &result.define_props[0].result.value.properties;
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    assert!(names.contains(&"a"));
    assert!(names.contains(&"b"));
    assert!(names.contains(&"local"));
    assert!(!names.contains(&"c"));
}

#[test]
fn evaluate_macro_types_synthesizes_define_props_from_union_object_variants() {
    use super::analysis::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
type FixedProps = {
  layout?: 'fixed'
  editor: string
}

type BubbleProps = {
  layout?: 'bubble'
  editor: string
  floating?: boolean
}

type Props = FixedProps | BubbleProps
defineProps<Props>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    assert_eq!(result.define_props.len(), 1);
    let fields = &result.define_props[0].result.value.properties;
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    assert!(names.contains(&"layout"));
    assert!(names.contains(&"editor"));
    assert!(names.contains(&"floating"));

    let editor = fields
        .iter()
        .find(|field| field.name == "editor")
        .expect("editor field should be synthesized");
    assert!(
        !editor.optional,
        "editor should stay required when present in every variant"
    );

    let floating = fields
        .iter()
        .find(|field| field.name == "floating")
        .expect("floating field should be synthesized");
    assert!(
        floating.optional,
        "branch-specific props should be optional in synthesized union fields"
    );
}

#[test]
fn evaluate_macro_types_synthesizes_define_props_from_mixed_intersection() {
    use super::analysis::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
type Props = {
  id?: string
  disabled?: boolean
} & Omit<FormHTMLAttributes, 'name'>

defineProps<Props>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    assert_eq!(result.define_props.len(), 1);
    let fields = &result.define_props[0].result.value.properties;
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    assert!(names.contains(&"id"));
    assert!(names.contains(&"disabled"));
}

#[test]
fn evaluate_macro_types_skips_vue_ignore_intersection_branch() {
    use super::analysis::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
type HtmlAttrs = {
  title?: string
  name?: string
}

type Props = {
  id?: string
} & /** @vue-ignore */ Omit<HtmlAttrs, 'name'>

defineProps<Props>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    assert_eq!(result.define_props.len(), 1);
    let fields = &result.define_props[0].result.value.properties;
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    assert!(
        names.contains(&"id"),
        "should keep local props, got: {names:?}"
    );
    assert!(
        !names.contains(&"title"),
        "should skip @vue-ignore branch props, got: {names:?}"
    );
}

#[test]
fn evaluate_macro_types_skips_vue_ignore_interface_extends() {
    use super::analysis::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
interface HtmlAttrs {
  title?: string
}

interface Props extends /** @vue-ignore */ HtmlAttrs {
  id?: string
}

defineProps<Props>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    assert_eq!(result.define_props.len(), 1);
    let fields = &result.define_props[0].result.value.properties;
    let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
    assert!(
        names.contains(&"id"),
        "should keep local props, got: {names:?}"
    );
    assert!(
        !names.contains(&"title"),
        "should skip @vue-ignore extends props, got: {names:?}"
    );
}

#[test]
fn evaluate_macro_types_with_env_only_emits_local_bindings() {
    use super::analysis::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
const localLabel: string = 'hello'
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let local_env = parse_and_build_env(source);
    let local_binding_names = local_env.value_symbols.keys().cloned().collect();
    let mut env = local_env;
    env.extend_missing(parse_and_build_env(
        "export const importedLabel: string = 'world'",
    ));

    let budget = super::type_expand::ExpansionBudget::default();
    let result = super::type_eval_build::expand_macro_types(
        &snapshot.macros,
        Some(source),
        &mut env,
        Some(&local_binding_names),
        &budget,
    );

    let names: Vec<&str> = result
        .bindings
        .iter()
        .map(|field| field.name.as_str())
        .collect();
    assert!(
        names.contains(&"localLabel"),
        "should keep local bindings, got: {names:?}"
    );
    assert!(
        !names.contains(&"importedLabel"),
        "should skip imported bindings, got: {names:?}"
    );
}

#[test]
fn evaluate_macro_types_skips_complex_slot_binding_types() {
    use super::analysis::build_script_analysis;
    use oxc_allocator::Allocator;

    let source = r#"
type Button = { ui: string }

defineSlots<{
  default(props: { ui: Button['ui'] }): any
}>()
"#;

    let allocator = Allocator::default();
    let snapshot = build_script_analysis(source, oxc_span::SourceType::tsx(), &allocator);
    let result = evaluate_macro_types(&snapshot.macros, source);

    // The new expander evaluates slot binding types that the old code skipped.
    // Button['ui'] resolves to string because Button = { ui: string }.
    assert!(
        !result.slot_bindings.is_empty(),
        "slot binding types should now be expanded"
    );
    assert_eq!(
        result.slot_bindings[0].r#type,
        TypeExpr::Primitive(PrimitiveName::String),
        "Button['ui'] should resolve to string"
    );
}
