use super::*;
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

/// Result of parsing a type string, includes source for key extraction
struct ParsedType {
    source: String,
    resolved: ResolvedElements,
}

impl ParsedType {
    /// Get the key name from a prop by extracting from source
    fn key_name(&self, prop: &ResolvedProp) -> &str {
        &self.source[prop.key.start as usize..prop.key.end as usize]
    }

    /// Find a prop by key name
    fn find_prop(&self, name: &str) -> Option<&ResolvedProp> {
        self.resolved
            .props
            .iter()
            .find(|p| self.key_name(p) == name)
    }
}

/// Helper to parse a type string and return the result with source
fn parse_type(type_str: &str) -> Option<ParsedType> {
    let allocator = Allocator::default();
    // Wrap in a type alias to parse
    let source = format!("type T = {}", type_str);
    let source_type = SourceType::ts();
    let parser = Parser::new(&allocator, &source, source_type);
    let result = parser.parse();

    if !result.errors.is_empty() {
        return None;
    }

    // Find the type alias declaration
    for stmt in &result.program.body {
        if let Statement::TSTypeAliasDeclaration(alias) = stmt {
            return Some(ParsedType {
                source: source.clone(),
                resolved: resolve_type_elements(&alias.type_annotation, 0, true),
            });
        }
    }
    None
}

/// Helper to infer runtime types from a type string
fn infer_type(type_str: &str) -> Vec<RuntimeType> {
    let allocator = Allocator::default();
    let source = format!("type T = {}", type_str);
    let source_type = SourceType::ts();
    let parser = Parser::new(&allocator, &source, source_type);
    let result = parser.parse();

    if !result.errors.is_empty() {
        return vec![RuntimeType::Unknown];
    }

    for stmt in &result.program.body {
        if let Statement::TSTypeAliasDeclaration(alias) = stmt {
            return infer_runtime_type(&alias.type_annotation);
        }
    }
    vec![RuntimeType::Unknown]
}

/// Helper to resolve a `type Test = ...` declaration using context, returning resolved elements
/// and collected diagnostics.
fn resolve_with_ctx(source: &str) -> (ResolvedElements, Vec<ResolutionDiagnostic>) {
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let parser = Parser::new(&allocator, source, source_type);
    let result = parser.parse();
    assert!(
        result.errors.is_empty(),
        "Source should parse without errors: {:?}",
        result.errors
    );

    let mut ctx = build_type_context(&result.program, source.as_bytes(), 0);

    for stmt in &result.program.body {
        if let Statement::TSTypeAliasDeclaration(alias) = stmt {
            if alias.id.name.as_str() == "Test" {
                let resolved =
                    resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx, true);
                return (resolved, ctx.diagnostics);
            }
        }
    }
    panic!("No `type Test = ...` declaration found in source");
}

/// Helper to resolve with context and companion types.
fn resolve_with_ctx_and_companions(
    source: &str,
    companions: FxHashMap<String, ResolvedElements>,
) -> (ResolvedElements, Vec<ResolutionDiagnostic>) {
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let parser = Parser::new(&allocator, source, source_type);
    let result = parser.parse();
    assert!(
        result.errors.is_empty(),
        "Source should parse without errors: {:?}",
        result.errors
    );

    let mut ctx = build_type_context(&result.program, source.as_bytes(), 0);
    ctx.companion_types = companions;

    for stmt in &result.program.body {
        if let Statement::TSTypeAliasDeclaration(alias) = stmt {
            if alias.id.name.as_str() == "Test" {
                let resolved =
                    resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx, true);
                return (resolved, ctx.diagnostics);
            }
        }
    }
    panic!("No `type Test = ...` declaration found in source");
}

/// Helper to resolve via the immutable `_ref` path, returning resolved elements
/// and the diagnostics on the context (which the `_ref` path should NOT modify).
fn resolve_with_ctx_ref(source: &str) -> (ResolvedElements, Vec<ResolutionDiagnostic>) {
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let parser = Parser::new(&allocator, source, source_type);
    let result = parser.parse();
    assert!(
        result.errors.is_empty(),
        "Source should parse without errors: {:?}",
        result.errors
    );

    let ctx = build_type_context(&result.program, source.as_bytes(), 0);

    for stmt in &result.program.body {
        if let Statement::TSTypeAliasDeclaration(alias) = stmt {
            if alias.id.name.as_str() == "Test" {
                let resolved =
                    resolve_type_elements_with_ctx_ref(&alias.type_annotation, 0, &ctx, true);
                return (resolved, ctx.diagnostics);
            }
        }
    }
    panic!("No `type Test = ...` declaration found in source");
}

/// Helper to resolve via the immutable `_ref` path with companion types.
fn resolve_with_ctx_ref_and_companions(
    source: &str,
    companions: FxHashMap<String, ResolvedElements>,
) -> (ResolvedElements, Vec<ResolutionDiagnostic>) {
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let parser = Parser::new(&allocator, source, source_type);
    let result = parser.parse();
    assert!(
        result.errors.is_empty(),
        "Source should parse without errors: {:?}",
        result.errors
    );

    let mut ctx = build_type_context(&result.program, source.as_bytes(), 0);
    ctx.companion_types = companions;

    for stmt in &result.program.body {
        if let Statement::TSTypeAliasDeclaration(alias) = stmt {
            if alias.id.name.as_str() == "Test" {
                let resolved =
                    resolve_type_elements_with_ctx_ref(&alias.type_annotation, 0, &ctx, true);
                return (resolved, ctx.diagnostics);
            }
        }
    }
    panic!("No `type Test = ...` declaration found in source");
}

// =========================================================================
// Existing tests — infer_runtime_type
// =========================================================================

#[test]
fn test_primitive_types() {
    assert_eq!(infer_type("string"), vec![RuntimeType::String]);
    assert_eq!(infer_type("number"), vec![RuntimeType::Number]);
    assert_eq!(infer_type("boolean"), vec![RuntimeType::Boolean]);
    assert_eq!(infer_type("symbol"), vec![RuntimeType::Symbol]);
    assert_eq!(infer_type("null"), vec![RuntimeType::Null]);
    assert_eq!(infer_type("bigint"), vec![RuntimeType::Number]);
}

#[test]
fn test_literal_types() {
    assert_eq!(infer_type("'hello'"), vec![RuntimeType::String]);
    assert_eq!(infer_type("42"), vec![RuntimeType::Number]);
    assert_eq!(infer_type("true"), vec![RuntimeType::Boolean]);
    assert_eq!(infer_type("false"), vec![RuntimeType::Boolean]);
}

#[test]
fn test_array_types() {
    assert_eq!(infer_type("string[]"), vec![RuntimeType::Array]);
    assert_eq!(infer_type("Array<number>"), vec![RuntimeType::Array]);
    assert_eq!(infer_type("[string, number]"), vec![RuntimeType::Array]);
}

#[test]
fn test_function_types() {
    assert_eq!(infer_type("() => void"), vec![RuntimeType::Function]);
    assert_eq!(
        infer_type("(x: number) => string"),
        vec![RuntimeType::Function]
    );
    assert_eq!(infer_type("Function"), vec![RuntimeType::Function]);
}

#[test]
fn test_object_types() {
    assert_eq!(infer_type("{ foo: string }"), vec![RuntimeType::Object]);
    assert_eq!(infer_type("object"), vec![RuntimeType::Object]);
    assert_eq!(infer_type("Object"), vec![RuntimeType::Object]);
}

#[test]
fn test_union_types() {
    let types = infer_type("string | number");
    assert!(types.contains(&RuntimeType::String));
    assert!(types.contains(&RuntimeType::Number));
    assert_eq!(types.len(), 2);
}

#[test]
fn test_builtin_types() {
    assert_eq!(
        infer_type("Date"),
        vec![RuntimeType::BuiltIn("Date".to_string())]
    );
    assert_eq!(
        infer_type("Map<string, number>"),
        vec![RuntimeType::BuiltIn("Map".to_string())]
    );
    assert_eq!(
        infer_type("Set<string>"),
        vec![RuntimeType::BuiltIn("Set".to_string())]
    );
    assert_eq!(
        infer_type("Promise<void>"),
        vec![RuntimeType::BuiltIn("Promise".to_string())]
    );
}

#[test]
fn test_utility_types() {
    assert_eq!(
        infer_type("Partial<{ foo: string }>"),
        vec![RuntimeType::Object]
    );
    assert_eq!(
        infer_type("Required<{ foo?: string }>"),
        vec![RuntimeType::Object]
    );
    assert_eq!(
        infer_type("Parameters<() => void>"),
        vec![RuntimeType::Array]
    );
}

// =========================================================================
// Existing tests — resolve_type_elements
// =========================================================================

#[test]
fn test_resolve_type_literal() {
    let parsed = parse_type("{ title: string; count: number }").unwrap();
    assert_eq!(parsed.resolved.props.len(), 2);

    let title = parsed.find_prop("title").unwrap();
    assert_eq!(title.types, vec![RuntimeType::String]);
    assert!(!title.optional);

    let count = parsed.find_prop("count").unwrap();
    assert_eq!(count.types, vec![RuntimeType::Number]);
    assert!(!count.optional);
}

#[test]
fn test_resolve_optional_props() {
    let parsed = parse_type("{ required: string; optional?: number }").unwrap();
    assert_eq!(parsed.resolved.props.len(), 2);

    let required = parsed.find_prop("required").unwrap();
    assert!(!required.optional);

    let optional = parsed.find_prop("optional").unwrap();
    assert!(optional.optional);
}

#[test]
fn test_resolve_method_signatures() {
    let parsed = parse_type("{ onClick(): void; onChange(value: string): void }").unwrap();
    assert_eq!(parsed.resolved.props.len(), 2);

    for prop in &parsed.resolved.props {
        assert_eq!(prop.types, vec![RuntimeType::Function]);
    }
}

#[test]
fn test_resolve_union_prop_types() {
    let parsed = parse_type("{ value: string | number }").unwrap();
    assert_eq!(parsed.resolved.props.len(), 1);

    let value = &parsed.resolved.props[0];
    assert!(value.types.contains(&RuntimeType::String));
    assert!(value.types.contains(&RuntimeType::Number));
}

#[test]
fn test_resolve_call_signature() {
    let parsed = parse_type("{ (): void }").unwrap();
    assert!(parsed.resolved.has_call_signature);
}

#[test]
fn test_complex_props_type() {
    let parsed = parse_type(
        r#"{
            title: string;
            count?: number;
            items: string[];
            metadata: { key: string };
            onClick: () => void;
            onUpdate(value: string): void;
        }"#,
    )
    .unwrap();

    assert_eq!(parsed.resolved.props.len(), 6);

    let title = parsed.find_prop("title").unwrap();
    assert_eq!(title.types, vec![RuntimeType::String]);
    assert!(!title.optional);

    let count = parsed.find_prop("count").unwrap();
    assert_eq!(count.types, vec![RuntimeType::Number]);
    assert!(count.optional);

    let items = parsed.find_prop("items").unwrap();
    assert_eq!(items.types, vec![RuntimeType::Array]);

    let metadata = parsed.find_prop("metadata").unwrap();
    assert_eq!(metadata.types, vec![RuntimeType::Object]);

    let onclick = parsed.find_prop("onClick").unwrap();
    assert_eq!(onclick.types, vec![RuntimeType::Function]);

    let onupdate = parsed.find_prop("onUpdate").unwrap();
    assert_eq!(onupdate.types, vec![RuntimeType::Function]);
}

// =========================================================================
// Existing tests — format_runtime_types
// =========================================================================

#[test]
fn test_format_runtime_types_single() {
    assert_eq!(format_runtime_types(&[RuntimeType::String]), "String");
    assert_eq!(format_runtime_types(&[RuntimeType::Number]), "Number");
    assert_eq!(format_runtime_types(&[RuntimeType::Boolean]), "Boolean");
    assert_eq!(format_runtime_types(&[RuntimeType::Array]), "Array");
    assert_eq!(format_runtime_types(&[RuntimeType::Function]), "Function");
    assert_eq!(format_runtime_types(&[RuntimeType::Object]), "Object");
}

#[test]
fn test_format_runtime_types_multiple() {
    assert_eq!(
        format_runtime_types(&[RuntimeType::String, RuntimeType::Number]),
        "[String, Number]"
    );
    assert_eq!(
        format_runtime_types(&[
            RuntimeType::String,
            RuntimeType::Number,
            RuntimeType::Boolean
        ]),
        "[String, Number, Boolean]"
    );
}

#[test]
fn test_format_runtime_types_unknown_union_matches_official() {
    // Official @vue/compiler-sfc rule: an unresolvable union member forces
    // `null` (accept anything, skip validation) UNLESS Boolean is present
    // (the boolean cast needs the declared constructor) or a default
    // exists alongside Function (a function default value must not be
    // treated as a factory).
    assert_eq!(
        format_runtime_types(&[RuntimeType::String, RuntimeType::Unknown]),
        "null",
        "string | Unresolved must skip validation like official, not warn as String"
    );
    assert_eq!(format_runtime_types(&[RuntimeType::Unknown]), "null");
    assert_eq!(
        format_runtime_types(&[RuntimeType::Boolean, RuntimeType::Unknown]),
        "Boolean"
    );
    assert_eq!(
        format_runtime_types(&[
            RuntimeType::String,
            RuntimeType::Boolean,
            RuntimeType::Unknown
        ]),
        "[String, Boolean]"
    );
    // Function survives Unknown only WITH a default present.
    assert_eq!(
        format_runtime_types_with_default(&[RuntimeType::Function, RuntimeType::Unknown], true),
        "Function"
    );
    assert_eq!(
        format_runtime_types_with_default(&[RuntimeType::Function, RuntimeType::Unknown], false),
        "null"
    );
    // The unresolved-DateValue shape: Unknown + Array + null → null.
    assert_eq!(
        format_runtime_types(&[RuntimeType::Unknown, RuntimeType::Array, RuntimeType::Null]),
        "null"
    );
    // No unknown member → the concrete list is untouched.
    assert_eq!(
        format_runtime_types(&[RuntimeType::String, RuntimeType::Number]),
        "[String, Number]"
    );
}

#[test]
fn test_format_runtime_types_builtin() {
    assert_eq!(
        format_runtime_types(&[RuntimeType::BuiltIn("Date".to_string())]),
        "Date"
    );
    assert_eq!(
        format_runtime_types(&[RuntimeType::BuiltIn("Map".to_string()), RuntimeType::Null]),
        "[Map, null]"
    );
}

// =========================================================================
// Existing tests — TypeResolutionContext
// =========================================================================

#[test]
fn test_build_type_context_collects_type_aliases() {
    let allocator = Allocator::default();
    let source = r#"type Props = { foo: string };
type Options = { bar: number };"#;
    let source_type = SourceType::ts();
    let parser = Parser::new(&allocator, source, source_type);
    let result = parser.parse();

    let ctx = build_type_context(&result.program, source.as_bytes(), 0);

    assert_eq!(ctx.type_aliases.len(), 2);
    // Check that we can find Props
    assert!(ctx.find_type_alias(b"Props").is_some());
    assert!(ctx.find_type_alias(b"Options").is_some());
    assert!(ctx.find_type_alias(b"Unknown").is_none());
}

#[test]
fn test_build_type_context_collects_interfaces() {
    let allocator = Allocator::default();
    let source = r#"interface Props { foo: string }
interface Options { bar: number }"#;
    let source_type = SourceType::ts();
    let parser = Parser::new(&allocator, source, source_type);
    let result = parser.parse();

    let ctx = build_type_context(&result.program, source.as_bytes(), 0);

    assert_eq!(ctx.interfaces.len(), 2);
    assert!(ctx.find_interface(b"Props").is_some());
    assert!(ctx.find_interface(b"Options").is_some());
    assert!(ctx.find_interface(b"Unknown").is_none());
}

#[test]
fn test_resolve_type_alias_with_context() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"type Props = { foo: string; bar: number };
type Test = Props;"#,
    );
    assert_eq!(
        resolved.props.len(),
        2,
        "Should resolve Props type alias with 2 props"
    );
    assert!(diagnostics.is_empty(), "Should have no diagnostics");
}

#[test]
fn test_resolve_interface_with_context() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface Props { foo: string; bar: number }
type Test = Props;"#,
    );
    assert_eq!(
        resolved.props.len(),
        2,
        "Should resolve Props interface with 2 props"
    );
    assert!(diagnostics.is_empty(), "Should have no diagnostics");
}

#[test]
fn test_unresolved_type_emits_diagnostic() {
    let (_, diagnostics) = resolve_with_ctx("type Test = UnknownType;");
    assert_eq!(diagnostics.len(), 1, "Should have 1 diagnostic");
    assert_eq!(
        diagnostics[0].kind,
        ResolutionDiagnosticKind::UnresolvedTypeReference
    );
    assert_eq!(
        diagnostics[0].location,
        DiagnosticLocation::TypeResolution,
        "Diagnostic should come from TypeResolution"
    );
}

#[test]
fn test_resolve_intersection_with_context() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"type A = { foo: string };
type B = { bar: number };
type Test = A & B;"#,
    );
    assert_eq!(
        resolved.props.len(),
        2,
        "Should resolve intersection with 2 props"
    );
    assert!(diagnostics.is_empty(), "Should have no diagnostics");
}

// ═══════════════════════════════════════════════════════════
// Cross-file type resolution (Tier 3)
// ═══════════════════════════════════════════════════════════

#[test]
fn resolve_external_type_interface() {
    let alloc = Allocator::default();
    let dep = "export interface Props { foo: string; bar: number }";
    let resolved = resolve_external_type("Props", dep, &alloc).unwrap();
    assert_eq!(resolved.props.len(), 2);
}

#[test]
fn resolve_external_type_alias() {
    let alloc = Allocator::default();
    let dep = "export type Props = { count: number; label?: string }";
    let resolved = resolve_external_type("Props", dep, &alloc).unwrap();
    assert_eq!(resolved.props.len(), 2);
    let optional_count = resolved.props.iter().filter(|p| p.optional).count();
    assert_eq!(optional_count, 1);
}

#[test]
fn resolve_external_type_alias_preserves_primitive_root_runtime_type() {
    let alloc = Allocator::default();
    let dep = "export type Props = string";
    let resolved = resolve_external_type("Props", dep, &alloc).unwrap();

    assert_eq!(resolved.props.len(), 0);
    assert_eq!(resolved.root_runtime_types, vec![RuntimeType::String]);
}

#[test]
fn resolve_external_type_empty_interface_is_object_like() {
    let alloc = Allocator::default();
    let dep = "export interface Props {}";
    let resolved = resolve_external_type("Props", dep, &alloc).unwrap();

    assert_eq!(resolved.props.len(), 0);
    assert_eq!(resolved.root_runtime_types, vec![RuntimeType::Object]);
}

#[test]
fn resolve_external_type_not_found() {
    let alloc = Allocator::default();
    let dep = "export interface Other { x: string }";
    assert!(resolve_external_type("Props", dep, &alloc).is_none());
}

#[test]
fn resolve_external_type_non_exported_still_found() {
    let alloc = Allocator::default();
    // build_type_context collects both exported and non-exported declarations
    let dep = "interface Props { name: string }";
    let resolved = resolve_external_type("Props", dep, &alloc).unwrap();
    assert_eq!(resolved.props.len(), 1);
}

#[test]
fn resolve_external_type_with_intersection() {
    let alloc = Allocator::default();
    let dep = r#"
type A = { foo: string };
type B = { bar: number };
export type Props = A & B;
"#;
    let resolved = resolve_external_type("Props", dep, &alloc).unwrap();
    assert_eq!(resolved.props.len(), 2);
}

#[test]
fn resolve_external_type_parse_error_returns_none() {
    let alloc = Allocator::default();
    let dep = "export interface { broken syntax";
    // Should not panic, just return None
    assert!(resolve_external_type("Props", dep, &alloc).is_none());
}

#[test]
fn hash_resolved_type_stable_across_formatting() {
    let alloc1 = Allocator::default();
    let dep1 = "export interface Props { foo: string; bar: number }";
    let resolved1 = resolve_external_type("Props", dep1, &alloc1).unwrap();
    let hash1 = hash_resolved_type(&resolved1, dep1.as_bytes());

    let alloc2 = Allocator::default();
    // Same interface with different whitespace
    let dep2 = "export interface Props {\n  foo: string;\n  bar: number;\n}";
    let resolved2 = resolve_external_type("Props", dep2, &alloc2).unwrap();
    let hash2 = hash_resolved_type(&resolved2, dep2.as_bytes());

    assert_eq!(hash1, hash2, "Same prop shape should produce same hash");
}

#[test]
fn hash_resolved_type_differs_on_prop_added() {
    let alloc1 = Allocator::default();
    let dep1 = "export interface Props { foo: string }";
    let resolved1 = resolve_external_type("Props", dep1, &alloc1).unwrap();
    let hash1 = hash_resolved_type(&resolved1, dep1.as_bytes());

    let alloc2 = Allocator::default();
    let dep2 = "export interface Props { foo: string; bar: number }";
    let resolved2 = resolve_external_type("Props", dep2, &alloc2).unwrap();
    let hash2 = hash_resolved_type(&resolved2, dep2.as_bytes());

    assert_ne!(
        hash1, hash2,
        "Different prop count should produce different hash"
    );
}

#[test]
fn hash_resolved_type_differs_on_type_changed() {
    let alloc1 = Allocator::default();
    let dep1 = "export interface Props { foo: string }";
    let resolved1 = resolve_external_type("Props", dep1, &alloc1).unwrap();
    let hash1 = hash_resolved_type(&resolved1, dep1.as_bytes());

    let alloc2 = Allocator::default();
    let dep2 = "export interface Props { foo: number }";
    let resolved2 = resolve_external_type("Props", dep2, &alloc2).unwrap();
    let hash2 = hash_resolved_type(&resolved2, dep2.as_bytes());

    assert_ne!(hash1, hash2, "Different type should produce different hash");
}

#[test]
fn hash_resolved_type_differs_on_optional_changed() {
    let alloc1 = Allocator::default();
    let dep1 = "export interface Props { foo: string }";
    let resolved1 = resolve_external_type("Props", dep1, &alloc1).unwrap();
    let hash1 = hash_resolved_type(&resolved1, dep1.as_bytes());

    let alloc2 = Allocator::default();
    let dep2 = "export interface Props { foo?: string }";
    let resolved2 = resolve_external_type("Props", dep2, &alloc2).unwrap();
    let hash2 = hash_resolved_type(&resolved2, dep2.as_bytes());

    assert_ne!(
        hash1, hash2,
        "Optional change should produce different hash"
    );
}

// ═══════════════════════════════════════════════════════════
// Interface extends / heritage clause tests
// ═══════════════════════════════════════════════════════════

/// @ai-generated - interface B extends A should include A's props
#[test]
fn interface_extends_single() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface A { foo: string }
interface B extends A { bar: number }
type Test = B;"#,
    );
    assert_eq!(
        resolved.props.len(),
        2,
        "B extends A should have 2 props (foo + bar)"
    );
    assert!(diagnostics.is_empty());
}

/// @ai-generated - interface extends multiple bases
#[test]
fn interface_extends_multiple() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface A { foo: string }
interface B { bar: number }
interface C extends A, B { baz: boolean }
type Test = C;"#,
    );
    assert_eq!(
        resolved.props.len(),
        3,
        "C extends A, B should have 3 props (foo + bar + baz)"
    );
    assert!(diagnostics.is_empty());
}

/// @ai-generated - deep interface extends chain: C extends B extends A
#[test]
fn interface_extends_deep_chain() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface A { a: string }
interface B extends A { b: number }
interface C extends B { c: boolean }
type Test = C;"#,
    );
    assert_eq!(
        resolved.props.len(),
        3,
        "C extends B extends A should have 3 props (a + b + c)"
    );
    assert!(diagnostics.is_empty());
}

#[test]
fn interface_extends_generic_base_preserves_bound_members() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface Base<T> { value: T }
interface Child extends Base<string> { count: number }
type Test = Child;"#,
    );
    assert_eq!(
        resolved.props.len(),
        2,
        "Child should include the inherited generic member and its own member",
    );
    let names: Vec<_> = resolved
        .props
        .iter()
        .filter_map(|prop| prop.key_name.as_deref())
        .collect();
    assert!(
        names.contains(&"value"),
        "inherited generic member should resolve"
    );
    assert!(names.contains(&"count"), "local member should resolve");
    assert!(diagnostics.is_empty());
}

/// @ai-generated - interface extends with companion types
#[test]
fn interface_extends_companion() {
    let source = r#"interface Local extends Base { own: string }
type Test = Local;"#;

    let mut companions = FxHashMap::default();
    let mut base_resolved = ResolvedElements::default();
    base_resolved.props.push(ResolvedProp {
        span: Span { start: 0, end: 0 },
        key: Span { start: 0, end: 0 },
        key_name: Some("baseField".to_string()),
        optional: false,
        types: vec![RuntimeType::String],
        visibility: ResolvedMemberVisibility::Public,
        type_span: None,
        type_text: None,
        map_local: true,
        span_is_absolute: false,
        declared_in_macro_type_arg: false,
    });
    companions.insert("Base".to_string(), base_resolved);

    let (resolved, diagnostics) = resolve_with_ctx_and_companions(source, companions);
    assert_eq!(
        resolved.props.len(),
        2,
        "Local extends Base should have 2 props (baseField + own)"
    );
    assert!(diagnostics.is_empty());
}

/// @ai-generated - resolve_external_type handles interface extends within same file
#[test]
fn resolve_external_type_interface_extends() {
    let alloc = Allocator::default();
    let dep = r#"
export interface Base { foo: string }
export interface Props extends Base { bar: number }
"#;
    let resolved = resolve_external_type("Props", dep, &alloc).unwrap();
    assert_eq!(
        resolved.props.len(),
        2,
        "Props extends Base should have 2 props"
    );
}

/// @ai-generated - resolve_external_type handles deep extends chain
#[test]
fn resolve_external_type_deep_extends() {
    let alloc = Allocator::default();
    let dep = r#"
interface A { a: string }
interface B extends A { b: number }
export interface Props extends B { c: boolean }
"#;
    let resolved = resolve_external_type("Props", dep, &alloc).unwrap();
    assert_eq!(
        resolved.props.len(),
        3,
        "Props extends B extends A should have 3 props"
    );
}

/// @ai-generated - resolve_external_type_with_companion supports imported aliases.
#[test]
fn resolve_external_type_with_companion_import_alias() {
    let alloc = Allocator::default();
    let dep = r#"
import type { BaseAction as LocalBase } from './base'

export interface Props extends LocalBase {
  label: string
}
"#;
    let mut companion_types = rustc_hash::FxHashMap::default();
    let mut base = ResolvedElements::default();
    base.props.push(ResolvedProp {
        span: Span::new(0, 0),
        key: Span::new(0, 0),
        key_name: Some("id".to_string()),
        optional: false,
        types: vec![RuntimeType::String],
        visibility: ResolvedMemberVisibility::Public,
        type_span: None,
        type_text: None,
        map_local: false,
        span_is_absolute: false,
        declared_in_macro_type_arg: false,
    });
    companion_types.insert("LocalBase".to_string(), base);

    let resolved =
        resolve_external_type_with_companion("Props", dep, &companion_types, &alloc).unwrap();
    assert_eq!(
        resolved.props.len(),
        2,
        "Props should include both imported base props and local props"
    );
    assert!(resolved
        .props
        .iter()
        .any(|prop| prop.key_name.as_deref() == Some("id")));
    assert!(resolved
        .props
        .iter()
        .any(|prop| prop.key_name.as_deref() == Some("label")));
}

/// @ai-generated - imported companion aliases should support extends but must not
/// be treated as exports of the current file.
#[test]
fn resolve_external_type_with_companion_does_not_export_imported_alias() {
    let alloc = Allocator::default();
    let dep = r#"
import type { BaseAction as LocalBase } from './base'

export interface Props extends LocalBase {
  label: string
}
"#;
    let mut companion_types = rustc_hash::FxHashMap::default();
    let mut base = ResolvedElements::default();
    base.props.push(ResolvedProp {
        span: Span::new(0, 0),
        key: Span::new(0, 0),
        key_name: Some("id".to_string()),
        optional: false,
        types: vec![RuntimeType::String],
        visibility: ResolvedMemberVisibility::Public,
        type_span: None,
        type_text: None,
        map_local: false,
        span_is_absolute: false,
        declared_in_macro_type_arg: false,
    });
    companion_types.insert("LocalBase".to_string(), base);

    let resolved = resolve_external_type_with_companion("LocalBase", dep, &companion_types, &alloc);
    assert!(
        resolved.is_none(),
        "imported companion aliases are not exported declarations of this file"
    );
}

/// @ai-generated - resolve_external_type_with_companion supports transitive imported emits shapes.
#[test]
fn resolve_external_type_with_companion_transitive_emits_shape() {
    let alloc = Allocator::default();
    let dep = r#"
import type { BaseEmits } from './base'

export interface Emits extends BaseEmits {
  confirm: [id: number]
}
"#;
    let mut companion_types = rustc_hash::FxHashMap::default();
    let mut base = ResolvedElements::default();
    base.call_signatures.push(ResolvedNamedCallSignature {
        span: Span::new(0, 0),
        name: "submit".to_string(),
        name_span: None,
        signature: ResolvedCallPayloadForm::Call {
            params_text: "payload: string".to_string(),
        },
        map_local: false,
        span_is_absolute: false,
    });
    companion_types.insert("BaseEmits".to_string(), base);

    let resolved =
        resolve_external_type_with_companion("Emits", dep, &companion_types, &alloc).unwrap();
    assert_eq!(
        resolved.call_signatures.len(),
        2,
        "Emits should include imported and local emits entries"
    );
    assert!(resolved
        .call_signatures
        .iter()
        .any(|emit| emit.name == "submit"));
    assert!(resolved
        .call_signatures
        .iter()
        .any(|emit| emit.name == "confirm"));
}

#[test]
fn resolve_external_type_interface_emits_shape() {
    let alloc = Allocator::default();
    let dep = r#"
export interface AccordionRootEmits {
  openChange: [value: boolean]
}
"#;

    let resolved = resolve_external_type_with_companion(
        "AccordionRootEmits",
        dep,
        &FxHashMap::default(),
        &alloc,
    )
    .expect("interface emits shape should resolve");

    assert_eq!(
        resolved.call_signatures.len(),
        1,
        "expected one resolved emit"
    );
    assert_eq!(resolved.call_signatures[0].name, "openChange");
}

/// @ai-generated - extract_companion_types handles interface extends
#[test]
fn companion_types_interface_extends() {
    let allocator = Allocator::default();
    let source = r#"interface Base { base: string }
interface Extended extends Base { own: number }"#;
    let source_type = SourceType::ts();
    let parser = Parser::new(&allocator, source, source_type);
    let result = parser.parse();

    let types = extract_companion_types(&result.program, source.as_bytes(), 0);

    let extended = types.get("Extended").unwrap();
    assert_eq!(
        extended.props.len(),
        2,
        "Extended should include base + own props"
    );
}

// ═══════════════════════════════════════════════════════════
// Utility types in extends clauses (Pick, Omit, Partial, etc.)
// ═══════════════════════════════════════════════════════════

/// @ai-generated - interface extends Pick<Companion, 'key'> resolves selected prop
#[test]
fn interface_extends_pick_companion() {
    let mut companions = FxHashMap::default();
    let bar = parse_type("{ x: number; z: boolean }").unwrap();
    companions.insert("Bar".to_string(), bar.resolved);

    let (resolved, diags) = resolve_with_ctx_ref_and_companions(
        "interface Foo extends Pick<Bar, 'x'> { y: string }; type Test = Foo",
        companions,
    );
    assert!(diags.is_empty(), "should not emit diagnostics");
    assert_eq!(
        resolved.props.len(),
        2,
        "expected 2 props (x from Pick + y from body)"
    );
    let names: Vec<&str> = resolved
        .props
        .iter()
        .filter_map(|p| p.key_name.as_deref())
        .collect();
    assert!(names.contains(&"x"), "should have 'x' from Pick<Bar, 'x'>");
    assert!(names.contains(&"y"), "should have 'y' from interface body");
    assert!(
        !names.contains(&"z"),
        "should NOT have 'z' (excluded by Pick)"
    );
}

/// @ai-generated - interface extends Omit<Companion, 'key'> excludes specified prop
#[test]
fn interface_extends_omit_companion() {
    let mut companions = FxHashMap::default();
    let bar = parse_type("{ x: number; z: boolean }").unwrap();
    companions.insert("Bar".to_string(), bar.resolved);

    let (resolved, diags) = resolve_with_ctx_ref_and_companions(
        "interface Foo extends Omit<Bar, 'z'> { y: string }; type Test = Foo",
        companions,
    );
    assert!(diags.is_empty());
    assert_eq!(
        resolved.props.len(),
        2,
        "expected 2 props (x from Omit + y from body)"
    );
    let names: Vec<&str> = resolved
        .props
        .iter()
        .filter_map(|p| p.key_name.as_deref())
        .collect();
    assert!(names.contains(&"x"), "should have 'x' (not omitted)");
    assert!(names.contains(&"y"), "should have 'y' from interface body");
    assert!(!names.contains(&"z"), "should NOT have 'z' (omitted)");
}

/// @ai-generated - interface extends Partial<Companion> makes all inherited props optional
#[test]
fn interface_extends_partial_companion() {
    let mut companions = FxHashMap::default();
    let bar = parse_type("{ x: number }").unwrap();
    companions.insert("Bar".to_string(), bar.resolved);

    let (resolved, _) = resolve_with_ctx_ref_and_companions(
        "interface Foo extends Partial<Bar> { y: string }; type Test = Foo",
        companions,
    );
    assert_eq!(
        resolved.props.len(),
        2,
        "expected 2 props (x from Partial + y from body)"
    );
    let x = resolved
        .props
        .iter()
        .find(|p| p.key_name.as_deref() == Some("x"))
        .unwrap();
    let y = resolved
        .props
        .iter()
        .find(|p| p.key_name.as_deref() == Some("y"))
        .unwrap();
    assert!(x.optional, "'x' from Partial<Bar> should be optional");
    assert!(!y.optional, "'y' from interface body should be required");
}

/// @ai-generated - interface with multiple utility type extends
#[test]
fn interface_extends_multi_utility() {
    let mut companions = FxHashMap::default();
    let a = parse_type("{ a: string; b: number }").unwrap();
    let b = parse_type("{ b: number; c: boolean }").unwrap();
    companions.insert("A".to_string(), a.resolved);
    companions.insert("B".to_string(), b.resolved);

    let (resolved, _) = resolve_with_ctx_ref_and_companions(
        "interface Foo extends Pick<A, 'a'>, Omit<B, 'c'> { d: string }; type Test = Foo",
        companions,
    );
    let names: Vec<&str> = resolved
        .props
        .iter()
        .filter_map(|p| p.key_name.as_deref())
        .collect();
    assert_eq!(
        resolved.props.len(),
        3,
        "expected 3 props: a, b, d. Got: {:?}",
        names
    );
    assert!(names.contains(&"a"), "should have 'a' from Pick<A, 'a'>");
    assert!(
        names.contains(&"b"),
        "should have 'b' from Omit<B, 'c'> (b not omitted)"
    );
    assert!(names.contains(&"d"), "should have 'd' from interface body");
    assert!(
        !names.contains(&"c"),
        "should NOT have 'c' (omitted from B)"
    );
}

// ═══════════════════════════════════════════════════════════
// Union/Intersection deduplication tests
// ═══════════════════════════════════════════════════════════

/// @ai-generated - intersection of types with shared props should deduplicate
#[test]
fn resolve_intersection_deduplicates_shared_props() {
    let parsed = parse_type("{ x: string; y: number } & { x: string; z: boolean }").unwrap();
    // x appears in both branches — should appear only once
    let x_count = parsed
        .resolved
        .props
        .iter()
        .filter(|p| parsed.key_name(p) == "x")
        .count();
    assert_eq!(
        x_count, 1,
        "Intersection should deduplicate shared prop 'x'"
    );
    assert_eq!(
        parsed.resolved.props.len(),
        3,
        "Should have 3 unique props: x, y, z"
    );
}

/// @ai-generated - union of types with shared props should deduplicate
#[test]
fn resolve_union_deduplicates_shared_props() {
    let parsed = parse_type("{ x: string; y: number } | { x: string; z: boolean }").unwrap();
    let x_count = parsed
        .resolved
        .props
        .iter()
        .filter(|p| parsed.key_name(p) == "x")
        .count();
    assert_eq!(x_count, 1, "Union should deduplicate shared prop 'x'");
    assert_eq!(
        parsed.resolved.props.len(),
        3,
        "Should have 3 unique props: x, y, z"
    );
}

/// @ai-generated - mixed union and intersection with overlapping props
#[test]
fn resolve_intersection_union_combo_deduplicates() {
    let parsed =
        parse_type("({ a: string } | { a: number; b: boolean }) & { a: string; c: number }")
            .unwrap();
    let a_count = parsed
        .resolved
        .props
        .iter()
        .filter(|p| parsed.key_name(p) == "a")
        .count();
    assert_eq!(
        a_count, 1,
        "Combined union+intersection should deduplicate shared prop 'a'"
    );
}

/// @ai-generated - intersection dedup with context (type references)
#[test]
fn resolve_intersection_dedup_with_context() {
    let (resolved, _) = resolve_with_ctx(
        r#"type A = { x: string; y: number };
type B = { x: string; z: boolean };
type Test = A & B;"#,
    );
    assert_eq!(
        resolved.props.len(),
        3,
        "A & B with shared 'x' should have 3 unique props"
    );
}

/// @ai-generated - circular extends doesn't cause infinite recursion
#[test]
fn interface_extends_circular_no_panic() {
    let alloc = Allocator::default();
    // This is invalid TS but shouldn't crash the resolver
    let dep = r#"
interface A extends B { a: string }
interface B extends A { b: number }
"#;
    // Should return without panicking
    let resolved = resolve_external_type("A", dep, &alloc).unwrap();
    // Should have at least A's own prop
    assert!(
        !resolved.props.is_empty(),
        "Should resolve at least some props without crashing"
    );
}

#[test]
fn extract_bindings_handles_export_star_and_named_reexports() {
    let allocator = Allocator::new();

    // export * from './Drawer'
    let source1 = "export * from './Drawer'";
    let result1 = extract_imported_type_bindings(source1, &allocator);
    assert!(result1.bindings.is_empty(), "no named bindings");
    assert!(
        result1.reexport_bindings.is_empty(),
        "wildcard export should not fabricate named re-exports"
    );
    assert_eq!(
        result1.wildcard_reexport_sources,
        vec!["./Drawer"],
        "should extract wildcard re-export source"
    );

    let allocator2 = Allocator::new();
    // export { DrawerEmits } from './src/index.vue'
    let source2 = "export type { DrawerEmits, DrawerProps } from './src/index.vue'";
    let result2 = extract_imported_type_bindings(source2, &allocator2);
    assert_eq!(
        result2.bindings.len(),
        2,
        "should have 2 named re-export bindings"
    );
    assert_eq!(
        result2.reexport_bindings.len(),
        2,
        "named re-export bindings should be tracked separately"
    );
    assert_eq!(result2.bindings[0].local_name, "DrawerEmits");
    assert_eq!(result2.bindings[0].imported_name, "DrawerEmits");
    assert_eq!(result2.bindings[0].source, "./src/index.vue");
    assert_eq!(result2.bindings[1].local_name, "DrawerProps");
    assert!(
        result2.wildcard_reexport_sources.is_empty(),
        "no wildcard re-exports"
    );

    let allocator3 = Allocator::new();
    // Mixed: import + export * + export {}
    let source3 =
        "import { Base } from './base';\nexport * from './utils';\nexport { Foo } from './foo';";
    let result3 = extract_imported_type_bindings(source3, &allocator3);
    assert_eq!(result3.bindings.len(), 2, "Base import + Foo re-export");
    assert_eq!(
        result3.reexport_bindings.len(),
        1,
        "plain imports must not be treated as direct re-exports"
    );
    assert_eq!(result3.bindings[0].local_name, "Base");
    assert_eq!(result3.bindings[0].source, "./base");
    assert_eq!(result3.bindings[1].local_name, "Foo");
    assert_eq!(result3.bindings[1].source, "./foo");
    assert_eq!(result3.reexport_bindings[0].local_name, "Foo");
    assert_eq!(result3.reexport_bindings[0].source, "./foo");
    assert_eq!(
        result3.wildcard_reexport_sources,
        vec!["./utils"],
        "should have one wildcard"
    );
}

#[test]
fn extract_bindings_follows_import_alias_then_export_local() {
    let allocator = Allocator::new();
    let source = "import type { Foo as Bar } from './dep'; export { Bar };";
    let result = extract_imported_type_bindings(source, &allocator);

    assert!(
        result
            .bindings
            .iter()
            .any(|binding| binding.local_name == "Bar"
                && binding.imported_name == "Foo"
                && binding.source == "./dep"),
        "import alias should keep the original imported symbol: {:?}",
        result.bindings
    );
    assert!(
        result
            .reexport_bindings
            .iter()
            .any(|binding| binding.local_name == "Bar"
                && binding.imported_name == "Foo"
                && binding.source == "./dep"),
        "re-exporting the aliased local should preserve the original symbol: {:?}",
        result.reexport_bindings
    );
}

#[test]
fn extract_bindings_follows_plain_import_alias_then_export_local() {
    let allocator = Allocator::new();
    let source = "import { foo as bar } from './dep'; export { bar };";
    let result = extract_imported_type_bindings(source, &allocator);

    assert!(
        result
            .bindings
            .iter()
            .any(|binding| binding.local_name == "bar"
                && binding.imported_name == "foo"
                && binding.source == "./dep"),
        "plain import alias should keep the original imported symbol: {:?}",
        result.bindings
    );
    assert!(
        result
            .reexport_bindings
            .iter()
            .any(|binding| binding.local_name == "bar"
                && binding.imported_name == "foo"
                && binding.source == "./dep"),
        "re-exporting the aliased plain import should preserve the original symbol: {:?}",
        result.reexport_bindings
    );
}

#[test]
fn extract_bindings_follows_default_import_then_export_local() {
    let allocator = Allocator::new();
    let source = "import Foo from './dep'; export { Foo as Bar };";
    let result = extract_imported_type_bindings(source, &allocator);

    assert!(
        result
            .bindings
            .iter()
            .any(|binding| binding.local_name == "Foo"
                && binding.imported_name == "default"
                && binding.source == "./dep"),
        "default import should preserve the default export symbol: {:?}",
        result.bindings
    );
    assert!(
        result
            .reexport_bindings
            .iter()
            .any(|binding| binding.local_name == "Bar"
                && binding.imported_name == "default"
                && binding.source == "./dep"),
        "re-exporting a default-imported local should preserve the default symbol: {:?}",
        result.reexport_bindings
    );
}

#[test]
fn analyze_external_type_source_tracks_local_export_symbol_targets_and_stats() {
    let allocator = Allocator::new();
    let source = "\
import type { Foo as LocalFoo } from './dep';\n\
type Inner = { label: string };\n\
export interface DirectProps { value: string }\n\
export { LocalFoo as Props, Inner as Alias };\n\
export { RemoteFoo as RemoteProps } from './remote';\n\
export * from './barrel';\n";
    let analysis = analyze_external_type_source(source, &allocator);
    let stats = analysis.stats();

    assert_eq!(
        analysis.local_export_symbol_target("Props"),
        Some("LocalFoo")
    );
    assert_eq!(
        analysis.local_export_symbol_target("DirectProps"),
        Some("DirectProps")
    );
    assert_eq!(analysis.local_export_symbol_target("Alias"), Some("Inner"));
    assert_eq!(analysis.local_symbol_target_name("Alias"), "Inner");
    assert!(analysis.has_local_symbol_target("Alias"));
    assert!(analysis.local_symbol_span("Inner").is_some());
    assert!(
        analysis
            .exported_local_type_names()
            .any(|name| name == "DirectProps"),
        "direct exported local declarations should stay available for shallow registry publication",
    );
    assert!(
        analysis
            .exported_local_symbol_names()
            .any(|name| name == "Props"),
        "local export aliases should stay available for shallow registry publication",
    );
    assert!(
        analysis
            .direct_reexport_entries()
            .any(|(name, source, imported)| {
                name == "RemoteProps" && source == "./remote" && imported == "RemoteFoo"
            }),
        "direct reexports should stay available for shallow registry publication",
    );
    assert_eq!(analysis.wildcard_reexport_sources(), ["./barrel"]);
    assert_eq!(stats.top_level_statement_count, 6);
    assert_eq!(stats.binding_count, 2);
    assert_eq!(stats.direct_reexport_count, 2);
    assert_eq!(stats.wildcard_reexport_count, 1);
    assert_eq!(stats.local_export_symbol_count, 3);
}

#[test]
fn analyze_external_type_source_builds_shallow_symbol_graph_once() {
    let allocator = Allocator::new();
    let source = "\
import type { Foo as ImportedFoo } from './dep';\n\
type Inner = ImportedFoo & { local: string };\n\
export interface Props extends Inner { id: string }\n";
    let analysis = analyze_external_type_source(source, &allocator);
    let inner = analysis
        .local_type_symbol("Inner")
        .expect("inner symbol should be cached");
    let props = analysis
        .local_type_symbol("Props")
        .expect("props symbol should be cached");

    assert_eq!(inner.kind, AnalyzedExternalTypeSymbolKind::TypeAlias);
    assert!(
        inner.dependency_names.contains("ImportedFoo"),
        "inner should keep shallow imported refs, got {:?}",
        inner.dependency_names
    );
    assert!(
        inner.structural_dependency_names.contains("ImportedFoo"),
        "structural deps should retain imported bases that affect the exported shape, got {:?}",
        inner.structural_dependency_names
    );
    assert!(
        props.dependency_names.contains("Inner"),
        "props should keep shallow local refs, got {:?}",
        props.dependency_names
    );
    assert!(
        props.structural_dependency_names.contains("Inner"),
        "structural deps should retain local alias-chain refs that affect the exported shape, got {:?}",
        props.structural_dependency_names
    );
    assert_eq!(
        analysis.local_import_symbol_target("ImportedFoo"),
        Some(("./dep", "Foo"))
    );
}

#[test]
fn resolve_external_type_with_analyzed_symbol_companion_resolves_local_symbol_chain() {
    let allocator = Allocator::new();
    let source = r#"
import type { ImportedBase } from './dep'

type Inner = ImportedBase & {
  localLabel: string
}

export interface Props extends Inner {
  id: string
}
"#;
    let analysis = analyze_external_type_source(source, &allocator);
    let mut companion_types = FxHashMap::default();
    companion_types.insert(
        "ImportedBase".to_string(),
        parse_type("{ imported: number }")
            .expect("companion type should parse")
            .resolved,
    );

    let program_alloc = Allocator::new();
    let parsed = Parser::new(&program_alloc, source, SourceType::ts()).parse();

    let resolved = resolve_external_type_in_program_with_analyzed_symbol_companion(
        "Props",
        &parsed.program,
        source.as_bytes(),
        &analysis,
        &companion_types,
    )
    .expect("local symbol targeted resolution should succeed");

    let prop_names = resolved
        .props
        .iter()
        .map(|prop| prop.key_name.clone().unwrap_or_default())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(prop_names.contains("id"));
    assert!(prop_names.contains("localLabel"));
    assert!(prop_names.contains("imported"));
}

#[test]
fn extract_bindings_keeps_namespace_type_imports() {
    let allocator = Allocator::new();
    let source = "import type * as Types from './types';";
    let result = extract_imported_type_bindings(source, &allocator);

    assert!(
        result
            .bindings
            .iter()
            .any(|binding| binding.local_name == "Types"
                && binding.source == "./types"
                && binding.is_namespace),
        "namespace type import should be preserved for lazy raw-source resolution: {:?}",
        result.bindings
    );
}

#[test]
fn collect_required_import_names_for_external_type_skips_leaf_prop_aliases() {
    let allocator = Allocator::new();
    let source = r#"
import { computed, toValue } from 'vue'
import type { MaybeRefOrGetter } from 'vue'
import { useAppConfig } from '#imports'
import type { AvatarProps, IconProps } from '../types'

export interface UseComponentIconsProps {
  icon?: IconProps['name']
  avatar?: AvatarProps
}

export function useComponentIcons(componentProps: MaybeRefOrGetter<UseComponentIconsProps>) {
  const appConfig = useAppConfig()
  const props = computed(() => toValue(componentProps))
  return { appConfig, props }
}
"#;

    let required = collect_required_import_names_for_external_type(
        "UseComponentIconsProps",
        source,
        &allocator,
    );
    let analysis = analyze_external_type_source(source, &allocator);
    let props = analysis
        .local_type_symbol("UseComponentIconsProps")
        .expect("UseComponentIconsProps should be tracked");

    assert_eq!(
        required.len(),
        1,
        "leaf prop aliases should stay symbolic instead of becoming required companions"
    );
    assert!(required.contains("IconProps"));
    assert!(!required.contains("AvatarProps"));
    assert!(
        props.dependency_names.contains("AvatarProps"),
        "the shallow symbol graph should still remember all refs for local bookkeeping"
    );
    assert!(
        !props.structural_dependency_names.contains("AvatarProps"),
        "leaf prop aliases must not become structural import requirements"
    );
    assert!(props.structural_dependency_names.contains("IconProps"));
    assert!(!required.contains("computed"));
    assert!(!required.contains("toValue"));
    assert!(!required.contains("MaybeRefOrGetter"));
    assert!(!required.contains("useAppConfig"));
}

#[test]
fn collect_required_import_names_for_external_type_follows_local_alias_chain() {
    let allocator = Allocator::new();
    let source = r#"
import type { Base } from './base'
import { computed } from 'vue'

type Local = Base & { label: string }

export interface Props extends Local {
  id: string
}

export function setup() {
  return computed(() => 1)
}
"#;

    let required = collect_required_import_names_for_external_type("Props", source, &allocator);

    assert_eq!(
        required.len(),
        1,
        "only the import used through the local alias chain should be followed"
    );
    assert!(required.contains("Base"));
    assert!(!required.contains("computed"));
}

#[test]
fn collect_required_import_names_for_external_type_ignores_slot_return_only_imports() {
    let allocator = Allocator::new();
    let source = r#"
import type { VNode } from 'vue'
import type { SlotBindings } from './slot-bindings'

export interface Slots {
  default?(props: SlotBindings): VNode[]
  title?(props?: {}): VNode[]
}
"#;

    let required = collect_required_import_names_for_external_type("Slots", source, &allocator);

    assert_eq!(
        required.len(),
        1,
        "slot return types should not drag framework-only imports into the required set"
    );
    assert!(required.contains("SlotBindings"));
    assert!(!required.contains("VNode"));
}

// ═══════════════════════════════════════════════════════════════════════════
// NEW: Unresolved Type Reference Variants (Step 2)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn unresolved_union_both_unknown() {
    let (resolved, diagnostics) = resolve_with_ctx("type Test = UnknownA | UnknownB;");
    assert_eq!(diagnostics.len(), 2, "Should have 2 diagnostics");
    assert!(
        diagnostics
            .iter()
            .all(|d| d.kind == ResolutionDiagnosticKind::UnresolvedTypeReference),
        "Both diagnostics should be UnresolvedTypeReference"
    );
    assert!(
        resolved.props.is_empty(),
        "No props should be resolved from unknown types"
    );
}

#[test]
fn unresolved_intersection_both_unknown() {
    let (resolved, diagnostics) = resolve_with_ctx("type Test = UnknownA & UnknownB;");
    assert_eq!(diagnostics.len(), 2, "Should have 2 diagnostics");
    assert!(
        diagnostics
            .iter()
            .all(|d| d.kind == ResolutionDiagnosticKind::UnresolvedTypeReference),
        "Both diagnostics should be UnresolvedTypeReference"
    );
    assert!(
        resolved.props.is_empty(),
        "No props should be resolved from unknown types"
    );
}

#[test]
fn unresolved_partial_union() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"type Known = { a: string };
type Test = Known | Unknown;"#,
    );
    assert_eq!(diagnostics.len(), 1, "Should have 1 diagnostic for Unknown");
    assert_eq!(
        diagnostics[0].kind,
        ResolutionDiagnosticKind::UnresolvedTypeReference
    );
    assert_eq!(resolved.props.len(), 1, "Should have 1 prop from Known");
}

#[test]
fn unresolved_partial_intersection() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"type Known = { a: string };
type Test = Known & Unknown;"#,
    );
    assert_eq!(diagnostics.len(), 1, "Should have 1 diagnostic for Unknown");
    assert_eq!(
        diagnostics[0].kind,
        ResolutionDiagnosticKind::UnresolvedTypeReference
    );
    assert_eq!(resolved.props.len(), 1, "Should have 1 prop from Known");
}

#[test]
fn unresolved_nested_in_union_branch() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"type A = { a: string };
type Test = A | (Unknown1 & Unknown2);"#,
    );
    assert_eq!(
        diagnostics.len(),
        2,
        "Should have 2 diagnostics for Unknown1 and Unknown2"
    );
    assert!(diagnostics
        .iter()
        .all(|d| d.kind == ResolutionDiagnosticKind::UnresolvedTypeReference),);
    assert_eq!(resolved.props.len(), 1, "Should have 1 prop from A");
}

// ═══════════════════════════════════════════════════════════════════════════
// NEW: Unresolved in Extends — Document Behavior (Step 3)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn extends_unknown_base_silently_ignored() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface X extends UnknownBase { own: string }
type Test = X;"#,
    );
    // resolve_interface_with_extends_ctx silently drops unknown bases — no diagnostic
    assert!(
        diagnostics.is_empty(),
        "Unknown extends bases are silently dropped, no diagnostic"
    );
    assert_eq!(
        resolved.props.len(),
        1,
        "Should have only the 'own' prop from X"
    );
}

#[test]
fn extends_partially_unknown_bases() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface A { a: string }
interface X extends A, UnknownBase { own: string }
type Test = X;"#,
    );
    assert!(
        diagnostics.is_empty(),
        "Unknown extends bases are silently dropped, no diagnostic"
    );
    assert_eq!(
        resolved.props.len(),
        2,
        "Should have 'a' from A and 'own' from X"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// NEW: Span Accuracy (Step 4)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diagnostic_span_matches_reference() {
    let source = "type Test = UnknownType;";
    let (_, diagnostics) = resolve_with_ctx(source);
    assert_eq!(diagnostics.len(), 1);

    let span = diagnostics[0].span;
    let name = &source[span.start as usize..span.end as usize];
    assert_eq!(
        name, "UnknownType",
        "Diagnostic span should cover the exact type reference name"
    );
}

#[test]
fn diagnostic_span_with_base_offset() {
    let source = "type Test = UnknownType;";
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let parser = Parser::new(&allocator, source, source_type);
    let result = parser.parse();

    // First resolve with offset 0 to get the base span
    let mut ctx0 = build_type_context(&result.program, source.as_bytes(), 0);
    for stmt in &result.program.body {
        if let Statement::TSTypeAliasDeclaration(alias) = stmt {
            let _ = resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx0, true);
        }
    }
    let span0 = ctx0.diagnostics[0].span;

    // Now resolve with base_offset = 100
    let mut ctx100 = build_type_context(&result.program, source.as_bytes(), 0);
    for stmt in &result.program.body {
        if let Statement::TSTypeAliasDeclaration(alias) = stmt {
            let _ = resolve_type_elements_with_ctx(&alias.type_annotation, 100, &mut ctx100, true);
        }
    }
    let span100 = ctx100.diagnostics[0].span;

    assert_eq!(
        span100.start,
        span0.start + 100,
        "Span start should be offset by base_offset"
    );
    assert_eq!(
        span100.end,
        span0.end + 100,
        "Span end should be offset by base_offset"
    );
}

#[test]
fn diagnostic_span_in_union() {
    let source = "type Known = { a: string };\ntype Test = Known | UnknownType | Known;";
    let (_, diagnostics) = resolve_with_ctx(source);
    assert_eq!(diagnostics.len(), 1, "Should have 1 diagnostic");

    let span = diagnostics[0].span;
    let name = &source[span.start as usize..span.end as usize];
    assert_eq!(
        name, "UnknownType",
        "Span should cover only UnknownType, not the union"
    );
}

#[test]
fn diagnostic_spans_multiple_are_distinct() {
    let source = "type Test = UnknownA & UnknownB;";
    let (_, diagnostics) = resolve_with_ctx(source);
    assert_eq!(diagnostics.len(), 2);

    let span_a = diagnostics[0].span;
    let span_b = diagnostics[1].span;

    assert_ne!(
        span_a, span_b,
        "Diagnostics for different type references should have distinct spans"
    );

    let name_a = &source[span_a.start as usize..span_a.end as usize];
    let name_b = &source[span_b.start as usize..span_b.end as usize];
    assert_eq!(name_a, "UnknownA");
    assert_eq!(name_b, "UnknownB");
}

// ═══════════════════════════════════════════════════════════════════════════
// NEW: Diagnostic Accumulation (Step 5)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn diagnostics_accumulate_across_union_branches() {
    let (_, diagnostics) = resolve_with_ctx("type Test = UnknownA | UnknownB | UnknownC;");
    assert_eq!(
        diagnostics.len(),
        3,
        "Should accumulate 3 diagnostics for 3 unknown union branches"
    );
}

#[test]
fn diagnostics_accumulate_across_intersection_branches() {
    let (_, diagnostics) = resolve_with_ctx("type Test = UnknownA & UnknownB & UnknownC;");
    assert_eq!(
        diagnostics.len(),
        3,
        "Should accumulate 3 diagnostics for 3 unknown intersection branches"
    );
}

#[test]
fn diagnostics_append_across_resolutions() {
    let allocator = Allocator::default();
    let source = r#"type Test1 = UnknownA;
type Test2 = UnknownB;"#;
    let source_type = SourceType::ts();
    let parser = Parser::new(&allocator, source, source_type);
    let result = parser.parse();

    let mut ctx = build_type_context(&result.program, source.as_bytes(), 0);

    // Resolve Test1
    for stmt in &result.program.body {
        if let Statement::TSTypeAliasDeclaration(alias) = stmt {
            if alias.id.name.as_str() == "Test1" {
                let _ = resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx, true);
            }
        }
    }
    assert_eq!(
        ctx.diagnostics.len(),
        1,
        "After first resolution: 1 diagnostic"
    );

    // Resolve Test2 on the same context
    for stmt in &result.program.body {
        if let Statement::TSTypeAliasDeclaration(alias) = stmt {
            if alias.id.name.as_str() == "Test2" {
                let _ = resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx, true);
            }
        }
    }
    assert_eq!(
        ctx.diagnostics.len(),
        2,
        "After second resolution: 2 total diagnostics (appended)"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// NEW: Immutable (_ref) Path Comparison (Step 6)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ctx_ref_resolves_same_as_ctx_mut() {
    let source = r#"type A = { a: string };
type Test = A;"#;
    let (resolved_mut, _) = resolve_with_ctx(source);
    let (resolved_ref, _) = resolve_with_ctx_ref(source);

    assert_eq!(
        resolved_mut.props.len(),
        resolved_ref.props.len(),
        "Both paths should produce the same number of props"
    );
}

#[test]
fn ctx_ref_does_not_collect_diagnostics() {
    let source = "type Test = UnknownType;";
    let (_, diag_ref) = resolve_with_ctx_ref(source);
    let (_, diag_mut) = resolve_with_ctx(source);

    assert!(
        diag_ref.is_empty(),
        "The _ref path should not collect diagnostics (immutable ctx)"
    );
    assert_eq!(
        diag_mut.len(),
        1,
        "The mut path should collect 1 diagnostic"
    );
}

#[test]
fn ctx_ref_companion_fallback() {
    let source = "type Test = CompanionType;";
    let mut companions = FxHashMap::default();
    let mut comp = ResolvedElements::default();
    comp.props.push(ResolvedProp {
        span: Span::new(0, 0),
        key: Span::new(0, 0),
        key_name: Some("compProp".to_string()),
        optional: false,
        types: vec![RuntimeType::String],
        visibility: ResolvedMemberVisibility::Public,
        type_span: None,
        type_text: None,
        map_local: false,
        span_is_absolute: false,
        declared_in_macro_type_arg: false,
    });
    companions.insert("CompanionType".to_string(), comp);

    let (resolved, diagnostics) = resolve_with_ctx_ref_and_companions(source, companions);
    assert_eq!(
        resolved.props.len(),
        1,
        "Should resolve companion type prop via _ref path"
    );
    assert!(
        diagnostics.is_empty(),
        "No diagnostics should be collected via _ref path"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// NEW: Dead Diagnostic Kinds — Document Current Behavior (Step 7)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn empty_type_literal_no_diagnostic() {
    let (resolved, diagnostics) = resolve_with_ctx("type Test = {};");
    assert!(
        diagnostics.is_empty(),
        "Empty type literal should NOT emit EmptyTypeLiteral diagnostic (dead code path)"
    );
    assert!(resolved.props.is_empty(), "No props in empty literal");
}

#[test]
fn type_alias_to_empty_literal_no_diagnostic() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"type E = {};
type Test = E;"#,
    );
    assert!(
        diagnostics.is_empty(),
        "Alias to empty literal should not emit diagnostic"
    );
    assert!(resolved.props.is_empty());
}

#[test]
fn empty_intersection_no_diagnostic() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"type A = {};
type B = {};
type Test = A & B;"#,
    );
    assert!(
        diagnostics.is_empty(),
        "Intersection of empty types should NOT emit EmptyIntersection diagnostic (dead code path)"
    );
    assert!(resolved.props.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// NEW: Edge Cases That Should NOT Error (Step 8)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn never_type_no_props_no_diagnostic() {
    let (resolved, diagnostics) = resolve_with_ctx("type Test = never;");
    assert!(resolved.props.is_empty(), "never type has no props");
    assert!(
        diagnostics.is_empty(),
        "never type should not emit diagnostic"
    );
}

#[test]
fn parenthesized_type_resolves_correctly() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"type A = { a: string };
type Test = (A);"#,
    );
    assert_eq!(
        resolved.props.len(),
        1,
        "Parenthesized type should resolve through to A's props"
    );
    assert!(diagnostics.is_empty());
}

#[test]
fn function_type_call_signature_no_diagnostic() {
    let parsed = parse_type("{ (): void }").unwrap();
    assert!(
        parsed.resolved.has_call_signature,
        "Should detect call signature"
    );
    // No diagnostics in the simple parse_type path (no ctx)
}

#[test]
fn index_signature_not_a_prop() {
    let parsed = parse_type("{ [key: string]: number }").unwrap();
    // Index signatures are not regular props
    assert!(
        parsed.resolved.props.is_empty(),
        "Index signature should not be treated as a named prop"
    );
}

#[test]
fn typeof_query_with_companion() {
    let source = "type Test = typeof myVar;";
    let mut companions = FxHashMap::default();
    let mut comp = ResolvedElements::default();
    comp.props.push(ResolvedProp {
        span: Span::new(0, 0),
        key: Span::new(0, 0),
        key_name: Some("x".to_string()),
        optional: false,
        types: vec![RuntimeType::Number],
        visibility: ResolvedMemberVisibility::Public,
        type_span: None,
        type_text: None,
        map_local: false,
        span_is_absolute: false,
        declared_in_macro_type_arg: false,
    });
    companions.insert("myVar".to_string(), comp);

    let (resolved, diagnostics) = resolve_with_ctx_and_companions(source, companions);
    assert_eq!(
        resolved.props.len(),
        1,
        "typeof with companion should resolve to companion props"
    );
    assert!(
        diagnostics.is_empty(),
        "No diagnostic for typeof with companion"
    );
}

#[test]
fn typeof_query_without_companion_no_diagnostic() {
    let (resolved, diagnostics) = resolve_with_ctx("type Test = typeof unknownVar;");
    assert!(
        resolved.props.is_empty(),
        "typeof without companion should have no props"
    );
    assert!(
        diagnostics.is_empty(),
        "typeof without companion should NOT emit UnresolvedTypeReference"
    );
}

#[test]
fn diagnostic_kind_messages_not_empty() {
    let kinds = [
        ResolutionDiagnosticKind::UnresolvedTypeReference,
        ResolutionDiagnosticKind::EmptyTypeLiteral,
        ResolutionDiagnosticKind::EmptyIntersection,
    ];
    for kind in &kinds {
        let msg = kind.message();
        assert!(
            !msg.is_empty(),
            "{:?} should have a non-empty message",
            kind
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// NEW: External Type Resolution Error Paths (Step 9)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn resolve_external_empty_file_returns_none() {
    let alloc = Allocator::default();
    let dep = "";
    assert!(
        resolve_external_type("Props", dep, &alloc).is_none(),
        "Empty file should return None"
    );
}

#[test]
fn resolve_external_only_imports_returns_none() {
    let alloc = Allocator::default();
    let dep = "import { X } from './y';";
    assert!(
        resolve_external_type("Props", dep, &alloc).is_none(),
        "File with only imports and no matching type should return None"
    );
}

#[test]
fn indexed_access_emit_property_types_resolve_to_call_signatures() {
    let source = r#"
type LayerEmits = {
  escapeKeydown: [event: KeyboardEvent]
  pointerdownOutside: [event: PointerEvent]
}
type Test = {
  escapeKeydown: LayerEmits['escapeKeydown']
  pointerdownOutside: LayerEmits['pointerdownOutside']
}
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");
    assert_eq!(
        resolved.call_signatures.len(),
        2,
        "indexed-access property types should become emit signatures, got: {:?}",
        resolved
            .call_signatures
            .iter()
            .map(|e| &e.name)
            .collect::<Vec<_>>()
    );
    // The payload must be the TUPLE form carrying the target tuple's text —
    // never a degraded call form — and the members must NOT double as props.
    for sig in &resolved.call_signatures {
        match &sig.signature {
            ResolvedCallPayloadForm::Tuple { tuple_text } => {
                assert!(
                    tuple_text.starts_with('[') && tuple_text.contains("event:"),
                    "tuple payload must carry the target tuple text, got {tuple_text:?}"
                );
            }
            other => panic!("expected Tuple payload form, got {other:?}"),
        }
    }
    assert!(
        resolved.props.is_empty(),
        "emit-shorthand members must not also surface as props: {:?}",
        resolved
            .props
            .iter()
            .map(|p| p.key_name.as_deref())
            .collect::<Vec<_>>()
    );
}

/// Indexed access whose OBJECT is an interface (not a type alias).
#[test]
fn indexed_access_emit_via_interface_object_resolves_to_call_signature() {
    let source = r#"
interface LayerEmits {
  escapeKeydown: [event: KeyboardEvent]
}
type Test = {
  escapeKeydown: LayerEmits['escapeKeydown']
}
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");
    assert_eq!(resolved.call_signatures.len(), 1);
    assert_eq!(resolved.call_signatures[0].name, "escapeKeydown");
    match &resolved.call_signatures[0].signature {
        ResolvedCallPayloadForm::Tuple { tuple_text } => {
            assert_eq!(tuple_text, "[event: KeyboardEvent]");
        }
        other => panic!("expected Tuple payload form, got {other:?}"),
    }
}

/// Indexed access whose object reference goes through an alias chain.
#[test]
fn indexed_access_emit_through_alias_chain_resolves() {
    let source = r#"
type LayerEmits = {
  escapeKeydown: [event: KeyboardEvent]
}
type Renamed = LayerEmits
type Test = {
  escapeKeydown: Renamed['escapeKeydown']
}
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");
    assert_eq!(resolved.call_signatures.len(), 1);
}

/// Indexed-access member whose target member is NOT a tuple stays a prop.
#[test]
fn indexed_access_non_tuple_member_stays_prop() {
    let source = r#"
type Obj = { name: string }
type Test = { name: Obj['name'] }
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");
    assert!(
        resolved.call_signatures.is_empty(),
        "non-tuple indexed access must NOT become an emit: {:?}",
        resolved
            .call_signatures
            .iter()
            .map(|e| &e.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(resolved.props.len(), 1);
    assert_eq!(resolved.props[0].key_name.as_deref(), Some("name"));
}

/// Indexed access into an unresolvable object name stays a prop.
#[test]
fn indexed_access_unresolvable_object_stays_prop() {
    let source = "type Test = { foo: Missing['foo'] }\n";
    let (resolved, _diagnostics) = resolve_with_ctx(source);
    assert!(resolved.call_signatures.is_empty());
    assert_eq!(resolved.props.len(), 1);
}

/// Non-string-literal index (a reference) is out of scope and stays a prop.
#[test]
fn indexed_access_non_literal_index_stays_prop() {
    let source = r#"
type K = 'a'
type Obj = { a: [x: number] }
type Test = { a: Obj[K] }
"#;
    let (resolved, _diagnostics) = resolve_with_ctx(source);
    assert!(
        resolved.call_signatures.is_empty(),
        "reference-typed index is out of scope for the emit shorthand"
    );
    assert_eq!(resolved.props.len(), 1);
}

/// A cyclic alias chain behind the indexed access terminates as a prop.
#[test]
fn indexed_access_alias_cycle_terminates_as_prop() {
    let source = r#"
type A = B
type B = A
type Test = { foo: A['foo'] }
"#;
    let (resolved, _diagnostics) = resolve_with_ctx(source);
    assert!(resolved.call_signatures.is_empty());
    assert_eq!(resolved.props.len(), 1);
}

/// Interface OWN members take the same ctx-aware emit path (the
/// `build_interface_resolution_plan` / `resolve_interface_with_extends_*`
/// call sites).
#[test]
fn interface_member_indexed_access_emit_resolves() {
    let source = r#"
type LayerEmits = {
  escapeKeydown: [event: KeyboardEvent]
}
interface TestShape {
  escapeKeydown: LayerEmits['escapeKeydown']
}
type Test = TestShape
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");
    assert_eq!(
        resolved.call_signatures.len(),
        1,
        "interface own-member indexed access must resolve to an emit, got props {:?}",
        resolved
            .props
            .iter()
            .map(|p| p.key_name.as_deref())
            .collect::<Vec<_>>()
    );
}

/// A direct tuple member is unaffected by ctx availability (same behavior
/// as the source-only path).
#[test]
fn direct_tuple_member_with_ctx_still_resolves_as_emit() {
    let source = "type Test = { change: [id: number] }\n";
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty());
    assert_eq!(resolved.call_signatures.len(), 1);
    match &resolved.call_signatures[0].signature {
        ResolvedCallPayloadForm::Tuple { tuple_text } => {
            assert_eq!(tuple_text, "[id: number]");
        }
        other => panic!("expected Tuple payload form, got {other:?}"),
    }
}

// --- Utility type resolution (Omit, Pick, Partial, Required, Readonly) ---

#[test]
fn omit_filters_emits_by_key() {
    // Omit<BaseEmits, 'entryFocus'> should exclude the 'entryFocus' emit
    let source = r#"
type BaseEmits = {
  entryFocus: [event: Event]
  escapeKeyDown: [event: KeyboardEvent]
  closeAutoFocus: [event: Event]
}
type Test = Omit<BaseEmits, 'entryFocus'>
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(
        diagnostics.is_empty(),
        "No unresolved diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        resolved.call_signatures.len(),
        2,
        "Should have 2 emits after omitting entryFocus"
    );
    assert!(resolved
        .call_signatures
        .iter()
        .any(|e| e.name == "escapeKeyDown"));
    assert!(resolved
        .call_signatures
        .iter()
        .any(|e| e.name == "closeAutoFocus"));
    assert!(
        !resolved
            .call_signatures
            .iter()
            .any(|e| e.name == "entryFocus"),
        "entryFocus must be omitted"
    );
}

#[test]
fn omit_filters_multiple_keys() {
    // Omit<T, 'a' | 'b'> should exclude both
    let source = r#"
type BaseEmits = {
  entryFocus: [event: Event]
  openAutoFocus: [event: Event]
  escapeKeyDown: [event: KeyboardEvent]
  closeAutoFocus: [event: Event]
}
type Test = Omit<BaseEmits, 'entryFocus' | 'openAutoFocus'>
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");
    assert_eq!(resolved.call_signatures.len(), 2);
    assert!(resolved
        .call_signatures
        .iter()
        .any(|e| e.name == "escapeKeyDown"));
    assert!(resolved
        .call_signatures
        .iter()
        .any(|e| e.name == "closeAutoFocus"));
    assert!(!resolved
        .call_signatures
        .iter()
        .any(|e| e.name == "entryFocus"));
    assert!(!resolved
        .call_signatures
        .iter()
        .any(|e| e.name == "openAutoFocus"));
}

#[test]
fn omit_filters_props_by_key() {
    let source = r#"
type Base = { foo: string; bar: number; baz: boolean }
type Test = Omit<Base, 'bar'>
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");
    assert_eq!(resolved.props.len(), 2);
    assert!(resolved
        .props
        .iter()
        .any(|p| p.key_name.as_deref() == Some("foo")));
    assert!(resolved
        .props
        .iter()
        .any(|p| p.key_name.as_deref() == Some("baz")));
    assert!(!resolved
        .props
        .iter()
        .any(|p| p.key_name.as_deref() == Some("bar")));
}

#[test]
fn pick_keeps_only_selected_keys() {
    let source = r#"
type BaseEmits = {
  entryFocus: [event: Event]
  escapeKeyDown: [event: KeyboardEvent]
  closeAutoFocus: [event: Event]
}
type Test = Pick<BaseEmits, 'escapeKeyDown'>
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");
    assert_eq!(resolved.call_signatures.len(), 1);
    assert_eq!(resolved.call_signatures[0].name, "escapeKeyDown");
}

#[test]
fn pick_keeps_multiple_keys() {
    let source = r#"
type Base = { a: string; b: number; c: boolean; d: object }
type Test = Pick<Base, 'a' | 'c'>
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");
    assert_eq!(resolved.props.len(), 2);
    assert!(resolved
        .props
        .iter()
        .any(|p| p.key_name.as_deref() == Some("a")));
    assert!(resolved
        .props
        .iter()
        .any(|p| p.key_name.as_deref() == Some("c")));
}

#[test]
fn omit_with_intersection_and_alias_chain() {
    // Simulates the reka-ui DropdownMenuContentEmits pattern:
    // type DismissableLayerEmits = { escapeKeyDown: [...]; pointerDownOutside: [...] }
    // type RovingFocusGroupEmits = { entryFocus: [...]; 'update:currentTabStopId': [...] }
    // type MenuContentImplEmits = DismissableLayerEmits & Omit<RovingFocusGroupEmits, 'update:currentTabStopId'> & { openAutoFocus: [...]; closeAutoFocus: [...] }
    // type MenuContentEmits = Omit<MenuContentImplEmits, 'entryFocus' | 'openAutoFocus'>
    // type DropdownMenuContentEmits = MenuContentEmits
    let source = r#"
type DismissableLayerEmits = {
  escapeKeyDown: [event: KeyboardEvent]
  pointerDownOutside: [event: PointerEvent]
}
type RovingFocusGroupEmits = {
  entryFocus: [event: Event]
  'update:currentTabStopId': [value: string | null]
}
type MenuContentImplEmits = DismissableLayerEmits & Omit<RovingFocusGroupEmits, 'update:currentTabStopId'> & {
  openAutoFocus: [event: Event]
  closeAutoFocus: [event: Event]
}
type MenuContentEmits = Omit<MenuContentImplEmits, 'entryFocus' | 'openAutoFocus'>
type DropdownMenuContentEmits = MenuContentEmits
type Test = DropdownMenuContentEmits
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");
    // Should have: escapeKeyDown, pointerDownOutside, closeAutoFocus
    // Should NOT have: entryFocus, openAutoFocus, update:currentTabStopId
    assert_eq!(
        resolved.call_signatures.len(),
        3,
        "emits: {:?}",
        resolved
            .call_signatures
            .iter()
            .map(|e| &e.name)
            .collect::<Vec<_>>()
    );
    assert!(resolved
        .call_signatures
        .iter()
        .any(|e| e.name == "escapeKeyDown"));
    assert!(resolved
        .call_signatures
        .iter()
        .any(|e| e.name == "pointerDownOutside"));
    assert!(resolved
        .call_signatures
        .iter()
        .any(|e| e.name == "closeAutoFocus"));
    assert!(
        !resolved
            .call_signatures
            .iter()
            .any(|e| e.name == "entryFocus"),
        "entryFocus must be omitted"
    );
    assert!(
        !resolved
            .call_signatures
            .iter()
            .any(|e| e.name == "openAutoFocus"),
        "openAutoFocus must be omitted"
    );
    assert!(
        !resolved
            .call_signatures
            .iter()
            .any(|e| e.name == "update:currentTabStopId"),
        "update:currentTabStopId must be omitted"
    );
}

#[test]
fn resolve_external_type_omit_emits() {
    // Test via resolve_external_type path (used by host for .d.ts files)
    let alloc = Allocator::default();
    let dep = r#"
type BaseEmits = {
  escapeKeyDown: [event: KeyboardEvent]
  entryFocus: [event: Event]
}
export type Emits = Omit<BaseEmits, 'entryFocus'>
"#;
    let resolved = resolve_external_type("Emits", dep, &alloc).unwrap();
    assert_eq!(resolved.call_signatures.len(), 1);
    assert_eq!(resolved.call_signatures[0].name, "escapeKeyDown");
    assert!(!resolved
        .call_signatures
        .iter()
        .any(|e| e.name == "entryFocus"));
}

#[test]
fn resolve_external_type_follows_local_export_alias_chain() {
    let alloc = Allocator::default();
    let dep = r#"
type Foo = { label: string }
export { Foo as Bar }
"#;

    let resolved =
        resolve_external_type("Bar", dep, &alloc).expect("local export alias should resolve");
    let names: Vec<&str> = resolved
        .props
        .iter()
        .map(|prop| prop.key_name.as_deref().unwrap_or(""))
        .collect();

    assert!(
        names.contains(&"label"),
        "local export aliases should resolve to the underlying declaration: {names:?}"
    );
}

#[test]
fn resolve_external_type_default_export_class_by_default_name() {
    let alloc = Allocator::default();
    let dep = r#"
export default class Props {
  label!: string
  protected hidden!: boolean
}
"#;

    let resolved = resolve_external_type("default", dep, &alloc)
        .expect("default-exported class should resolve");
    let names: Vec<&str> = resolved
        .props
        .iter()
        .map(|prop| prop.key_name.as_deref().unwrap_or(""))
        .collect();

    assert!(
        names.contains(&"label"),
        "default-exported class should resolve its public members: {names:?}"
    );
    assert!(
        names.contains(&"hidden"),
        "native default-exported class resolution should preserve protected members: {names:?}"
    );
}

#[test]
fn partial_preserves_all_members() {
    // Partial<T> should keep all props/emits (just makes them optional)
    let source = r#"
type Base = { foo: string; bar: number }
type Test = Partial<Base>
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");
    assert_eq!(resolved.props.len(), 2);
}

#[test]
fn required_preserves_all_members() {
    let source = r#"
type Base = { foo?: string; bar?: number }
type Test = Required<Base>
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");
    assert_eq!(resolved.props.len(), 2);
}

#[test]
fn readonly_preserves_all_members() {
    let source = r#"
type Base = { foo: string; bar: number }
type Test = Readonly<Base>
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");
    assert_eq!(resolved.props.len(), 2);
}

// =========================================================================
// Phase 8: interface extends Pick<Imported, ...> + generic wrappers
// =========================================================================

#[test]
fn interface_extends_pick_of_imported_type_resolves_inherited_fields() {
    let source = r#"
interface BaseProps { a: string; b: number; c: boolean; d: object }
interface MyProps extends Pick<BaseProps, 'a' | 'b'> { local: string }
type Test = MyProps
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");

    let names: Vec<&str> = resolved
        .props
        .iter()
        .map(|p| &source[p.key.start as usize..p.key.end as usize])
        .collect();

    // Assert+: inherited + local fields are present
    assert!(
        names.contains(&"a"),
        "should have 'a' from Pick, got: {names:?}"
    );
    assert!(
        names.contains(&"b"),
        "should have 'b' from Pick, got: {names:?}"
    );
    assert!(
        names.contains(&"local"),
        "should have 'local', got: {names:?}"
    );

    // Assert-: excluded fields must not be present
    assert!(
        !names.contains(&"c"),
        "should NOT have 'c' (not in Pick), got: {names:?}"
    );
    assert!(
        !names.contains(&"d"),
        "should NOT have 'd' (not in Pick), got: {names:?}"
    );
}

#[test]
fn generic_wrapper_over_utility_type_resolves_fields() {
    let source = r#"
interface BaseProps { a: string; b: number; c: boolean }
type WithExtra<T> = Partial<T> & { extra: string }
type Test = WithExtra<BaseProps>
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    assert!(diagnostics.is_empty(), "No diagnostics: {diagnostics:?}");

    let names: Vec<&str> = resolved
        .props
        .iter()
        .map(|p| &source[p.key.start as usize..p.key.end as usize])
        .collect();

    // Assert+: all BaseProps + extra
    assert!(
        names.contains(&"a"),
        "should have 'a' from Partial<BaseProps>, got: {names:?}"
    );
    assert!(
        names.contains(&"b"),
        "should have 'b' from Partial<BaseProps>, got: {names:?}"
    );
    assert!(
        names.contains(&"c"),
        "should have 'c' from Partial<BaseProps>, got: {names:?}"
    );
    assert!(
        names.contains(&"extra"),
        "should have 'extra' from intersection, got: {names:?}"
    );

    // Assert-: no missing props
    assert_eq!(
        resolved.props.len(),
        4,
        "should have exactly 4 props (a, b, c, extra)"
    );
}

#[test]
fn type_resolution_context_prefers_latest_type_param_binding() {
    let allocator = Allocator::default();
    let source = r#"
type Wrapper<T> = T
type StringAlias = string
type NumberAlias = number
"#;
    let source_type = SourceType::ts();
    let parser = Parser::new(&allocator, source, source_type);
    let result = parser.parse();
    assert!(result.errors.is_empty(), "Source should parse cleanly");

    let mut ctx = build_type_context(&result.program, source.as_bytes(), 0);
    let mut wrapper_param_span = None;
    let mut string_alias = None;
    let mut number_alias = None;

    for stmt in &result.program.body {
        if let Statement::TSTypeAliasDeclaration(alias) = stmt {
            match alias.id.name.as_str() {
                "Wrapper" => {
                    wrapper_param_span = alias
                        .type_parameters
                        .as_ref()
                        .and_then(|params| params.params.first())
                        .map(|param| Span::from(param.name.span));
                }
                "StringAlias" => {
                    string_alias = Some(&alias.type_annotation);
                }
                "NumberAlias" => {
                    number_alias = Some(&alias.type_annotation);
                }
                _ => {}
            }
        }
    }

    let wrapper_param_span = wrapper_param_span.expect("Wrapper<T> param span should exist");
    let string_alias = string_alias.expect("StringAlias body should exist");
    let number_alias = number_alias.expect("NumberAlias body should exist");

    ctx.type_param_bindings
        .push((wrapper_param_span, string_alias));
    ctx.type_param_bindings
        .push((wrapper_param_span, number_alias));

    let resolved = ctx
        .find_type_param(b"T")
        .expect("latest binding for T should be found");
    assert!(
        matches!(resolved, TSType::TSNumberKeyword(_)),
        "latest nested binding should shadow the outer one"
    );
}

#[test]
fn resolve_external_type_with_companion_nested_generic_shadowing_uses_inner_binding() {
    let alloc = Allocator::default();
    let dep = r#"
import type { Base } from './base'

type Prettify<T> = { [K in keyof T]: T[K] } & {}
export type Inner = Prettify<Base>
export type Outer = Prettify<Inner>
"#;
    let mut companion_types = rustc_hash::FxHashMap::default();
    let mut base = ResolvedElements::default();
    for name in ["a", "b"] {
        base.props.push(ResolvedProp {
            span: Span::new(0, 0),
            key: Span::new(0, 0),
            key_name: Some(name.to_string()),
            optional: true,
            types: vec![RuntimeType::String],
            visibility: ResolvedMemberVisibility::Public,
            type_span: None,
            type_text: None,
            map_local: false,
            span_is_absolute: false,
            declared_in_macro_type_arg: false,
        });
    }
    companion_types.insert("Base".to_string(), base);

    let resolved = resolve_external_type_with_companion("Outer", dep, &companion_types, &alloc)
        .expect("nested imported generic alias should resolve without recursion");
    let names: Vec<&str> = resolved
        .props
        .iter()
        .filter_map(|prop| prop.key_name.as_deref())
        .collect();

    assert_eq!(
        resolved.props.len(),
        2,
        "expected only inherited Base props"
    );
    assert!(names.contains(&"a"), "expected prop 'a', got: {names:?}");
    assert!(names.contains(&"b"), "expected prop 'b', got: {names:?}");
}

#[test]
fn resolve_external_type_skips_vue_ignore_interface_extends() {
    let alloc = Allocator::default();
    let dep = r#"
interface HtmlAttrs {
  title?: string
}

export interface Props extends /** @vue-ignore */ HtmlAttrs {
  id?: string
}
"#;

    let resolved = resolve_external_type("Props", dep, &alloc).unwrap();
    let names: Vec<&str> = resolved
        .props
        .iter()
        .map(|p| p.key_name.as_deref().unwrap_or(""))
        .collect();

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
fn resolve_external_type_class_public_members_and_heritage() {
    let alloc = Allocator::default();
    let dep = r#"
interface Implemented {
  from_implements: number
}

class BaseProps {
  from_base!: string
  protected hidden!: boolean
  private internal!: number
  static ignored = true
}

export class Props extends BaseProps implements Implemented {
  own?: boolean
  from_implements!: number
  method(): void {}
}
"#;

    let resolved = resolve_external_type("Props", dep, &alloc).unwrap();
    let names: Vec<&str> = resolved
        .props
        .iter()
        .map(|p| p.key_name.as_deref().unwrap_or(""))
        .collect();

    assert!(
        names.contains(&"from_base"),
        "should include base members, got: {names:?}"
    );
    assert!(
        names.contains(&"from_implements"),
        "should include public class members on implemented shapes, got: {names:?}"
    );
    assert!(
        names.contains(&"own"),
        "should include own members, got: {names:?}"
    );
    assert!(
        names.contains(&"method"),
        "should include public methods as function props, got: {names:?}"
    );
    assert!(
        names.contains(&"hidden"),
        "native resolver should preserve protected members, got: {names:?}"
    );
    assert!(
        names.contains(&"internal"),
        "native resolver should preserve private members, got: {names:?}"
    );
    assert!(
        !names.contains(&"ignored"),
        "should not expose static members, got: {names:?}"
    );

    let hidden = resolved
        .props
        .iter()
        .find(|prop| prop.key_name.as_deref() == Some("hidden"))
        .expect("protected member should be present");
    assert_eq!(hidden.visibility, ResolvedMemberVisibility::Protected);
    assert_eq!(
        hidden.type_text.as_deref(),
        Some("boolean"),
        "class property annotations should be retained for native provenance"
    );

    let internal = resolved
        .props
        .iter()
        .find(|prop| prop.key_name.as_deref() == Some("internal"))
        .expect("private member should be present");
    assert_eq!(internal.visibility, ResolvedMemberVisibility::Private);

    let method = resolved
        .props
        .iter()
        .find(|prop| prop.key_name.as_deref() == Some("method"))
        .expect("method should be present");
    assert_eq!(
        method.type_text.as_deref(),
        Some("() => void"),
        "class methods should retain a callable raw type for downstream provenance"
    );
}

#[test]
fn class_references_stay_object_like_at_root() {
    let (resolved, diagnostics) = resolve_with_ctx_ref(
        r#"
class Props {
  label!: string
}

type Test = Props
"#,
    );

    assert!(
        diagnostics.is_empty(),
        "class references should not produce diagnostics: {diagnostics:?}"
    );
    assert_eq!(
        resolved.root_runtime_types,
        vec![RuntimeType::Object],
        "class references should stay object-like for downstream validation"
    );
}

#[test]
fn collect_required_import_names_for_external_class_includes_heritage_imports() {
    let alloc = Allocator::default();
    let dep = r#"
import { BaseProps } from './base'
import type { Implemented } from './iface'

export class Props extends BaseProps implements Implemented {
  own?: boolean
}
"#;

    let required = collect_required_import_names_for_external_type("Props", dep, &alloc);

    assert!(
        required.contains("BaseProps"),
        "class extends imports should be followed, got: {required:?}"
    );
    assert!(
        required.contains("Implemented"),
        "class implements imports should be followed, got: {required:?}"
    );
}

// ===========================================================================
// Edge case: Pick/Partial applied to class types
// ===========================================================================

#[test]
fn pick_on_class_type_selects_named_members() {
    let alloc = Allocator::default();
    let dep = r#"
class BaseClass {
  label!: string
  count!: number
  protected secret!: boolean
}
export type Props = Pick<BaseClass, 'label'>
"#;
    let resolved =
        resolve_external_type("Props", dep, &alloc).expect("Pick<Class, 'label'> should resolve");
    let names: Vec<&str> = resolved
        .props
        .iter()
        .filter_map(|p| p.key_name.as_deref())
        .collect();
    assert!(
        names.contains(&"label"),
        "Pick should include 'label': {names:?}"
    );
    assert!(
        !names.contains(&"count"),
        "Pick should NOT include 'count': {names:?}"
    );
    assert!(
        !names.contains(&"secret"),
        "Pick should NOT include 'secret': {names:?}"
    );
}

#[test]
fn partial_on_class_type_keeps_all_members() {
    let alloc = Allocator::default();
    let dep = r#"
class BaseClass {
  label!: string
  count!: number
}
export type Props = Partial<BaseClass>
"#;
    let resolved =
        resolve_external_type("Props", dep, &alloc).expect("Partial<Class> should resolve");
    let names: Vec<&str> = resolved
        .props
        .iter()
        .filter_map(|p| p.key_name.as_deref())
        .collect();
    // Assert+: all members should be present
    assert!(
        names.contains(&"label"),
        "Partial should keep 'label': {names:?}"
    );
    assert!(
        names.contains(&"count"),
        "Partial should keep 'count': {names:?}"
    );
    // Assert-: no extra members should appear
    assert_eq!(names.len(), 2, "should have exactly 2 props: {names:?}");
}

// ===========================================================================
// Edge case: abstract class members
// ===========================================================================

#[test]
fn abstract_class_members_are_resolved() {
    let alloc = Allocator::default();
    let dep = r#"
export abstract class BaseWidget {
  abstract label: string
  count!: number
}
"#;
    let resolved =
        resolve_external_type("BaseWidget", dep, &alloc).expect("abstract class should resolve");
    let names: Vec<&str> = resolved
        .props
        .iter()
        .filter_map(|p| p.key_name.as_deref())
        .collect();
    // Abstract members should still appear as props (they define the shape)
    assert!(
        names.contains(&"label"),
        "abstract member 'label' should be resolved: {names:?}"
    );
    assert!(
        names.contains(&"count"),
        "'count' should be resolved: {names:?}"
    );
}

// ===========================================================================
// Edge case: callable_signature_text with no return type annotation
// ===========================================================================

#[test]
fn method_without_return_type_defaults_to_void() {
    let alloc = Allocator::default();
    let dep = r#"
export interface Slots {
  default(props: { row: string }): void
  noReturn(props: { item: number })
}
"#;
    // The second method has no return type annotation;
    // callable_signature_text should default to "void" not "unknown"
    let resolved =
        resolve_external_type("Slots", dep, &alloc).expect("interface with methods should resolve");
    // Both should be present as props (method signatures on interfaces)
    let names: Vec<&str> = resolved
        .props
        .iter()
        .filter_map(|p| p.key_name.as_deref())
        .collect();
    assert!(
        names.contains(&"default"),
        "should have 'default': {names:?}"
    );
    assert!(
        names.contains(&"noReturn"),
        "should have 'noReturn': {names:?}"
    );
}

// ── extract_export_surface tests ────────────────────────────────────────

#[test]
fn extract_export_surface_collects_exported_declarations() {
    let alloc = Allocator::default();
    let source = r#"
export interface Foo {}
export type Bar = string
export enum Baz { A, B }
export class Qux {}
export const CONSTANT = 42
export function helper() {}
interface NotExported {}
type AlsoNotExported = number
"#;
    let surface = extract_export_surface(source, &alloc);
    assert!(surface.exported_names.contains("Foo"), "should have Foo");
    assert!(surface.exported_names.contains("Bar"), "should have Bar");
    assert!(surface.exported_names.contains("Baz"), "should have Baz");
    assert!(surface.exported_names.contains("Qux"), "should have Qux");
    assert!(
        surface.exported_names.contains("CONSTANT"),
        "should have CONSTANT"
    );
    assert!(
        surface.exported_names.contains("helper"),
        "should have helper"
    );
    assert!(
        !surface.exported_names.contains("NotExported"),
        "bare interface without export should not be collected"
    );
    assert!(
        !surface.exported_names.contains("AlsoNotExported"),
        "bare type without export should not be collected"
    );
    assert!(
        surface.wildcard_reexport_sources.is_empty(),
        "no wildcard re-exports"
    );
}

#[test]
fn extract_export_surface_collects_named_reexports() {
    let alloc = Allocator::default();
    let source = r#"
export { Foo } from './foo'
export { Bar as PublicBar } from './bar'
export type { Baz } from './baz'
export type { Qux as PublicQux } from './qux'
"#;
    let surface = extract_export_surface(source, &alloc);
    assert!(surface.exported_names.contains("Foo"), "should have Foo");
    assert!(
        surface.exported_names.contains("PublicBar"),
        "should have PublicBar (aliased)"
    );
    assert!(
        !surface.exported_names.contains("Bar"),
        "local name Bar should not be in exports"
    );
    assert!(surface.exported_names.contains("Baz"), "should have Baz");
    assert!(
        surface.exported_names.contains("PublicQux"),
        "should have PublicQux (aliased type re-export)"
    );
}

#[test]
fn extract_export_surface_collects_local_reexports() {
    let alloc = Allocator::default();
    let source = r#"
interface Foo {}
type Bar = string
export { Foo }
export { Bar as PublicBar }
"#;
    let surface = extract_export_surface(source, &alloc);
    assert!(surface.exported_names.contains("Foo"), "should have Foo");
    assert!(
        surface.exported_names.contains("PublicBar"),
        "should have PublicBar (aliased local re-export)"
    );
    assert!(
        !surface.exported_names.contains("Bar"),
        "local name Bar should not be in exports"
    );
}

#[test]
fn extract_export_surface_collects_wildcard_reexports() {
    let alloc = Allocator::default();
    let source = r#"
export * from './types'
export * from '../components/Button.vue'
export { Foo } from './foo'
"#;
    let surface = extract_export_surface(source, &alloc);
    assert_eq!(surface.wildcard_reexport_sources.len(), 2);
    assert_eq!(surface.wildcard_reexport_sources[0], "./types");
    assert_eq!(
        surface.wildcard_reexport_sources[1],
        "../components/Button.vue"
    );
    assert!(surface.exported_names.contains("Foo"), "should have Foo");
}

#[test]
fn extract_export_surface_collects_export_default() {
    let alloc = Allocator::default();
    let source = r#"
export default function main() {}
"#;
    let surface = extract_export_surface(source, &alloc);
    assert!(
        surface.exported_names.contains("default"),
        "should have default"
    );
}

#[test]
fn extract_export_surface_handles_empty_source() {
    let alloc = Allocator::default();
    let surface = extract_export_surface("", &alloc);
    assert!(surface.exported_names.is_empty());
    assert!(surface.wildcard_reexport_sources.is_empty());
}

#[test]
fn extract_export_surface_handles_mixed_barrel() {
    let alloc = Allocator::default();
    // Simulate a Nuxt UI types/index.ts barrel
    let source = r#"
export * from '../components/Accordion.vue'
export * from '../components/Alert.vue'
export * from '../components/Button.vue'
export * from './utils'
export * from './tv'
"#;
    let surface = extract_export_surface(source, &alloc);
    assert_eq!(
        surface.wildcard_reexport_sources.len(),
        5,
        "should have 5 wildcard sources"
    );
    assert!(
        surface.exported_names.is_empty(),
        "pure barrel should have no direct exports"
    );
}

// ---------------------------------------------------------------------------
// Inherited props/emits/slots through companion types.
// These tests verify that extends + Omit/Pick chains resolve correctly
// when the base type comes from a companion (cross-file import).
// ---------------------------------------------------------------------------

/// Props extends Omit<CompanionType, keys> should preserve inherited members
/// minus the omitted keys, plus local members.
///
/// Simulates: DashboardSidebarCollapse → Omit<ButtonProps, LinkPropsKeys | ...>
#[test]
fn companion_extends_omit_preserves_inherited_props() {
    let alloc = Allocator::default();

    // Simulate ButtonProps resolved from another file (companion)
    let mut button_props = ResolvedElements::default();
    for (name, optional) in [
        ("icon", true),
        ("avatar", true),
        ("label", true),
        ("color", true),
        ("variant", true),
        ("size", true),
        ("onClick", true),
        ("class", true),
        ("as", true),
        ("type", true),
        ("disabled", true),
        ("href", true),   // from LinkProps (should be omitted)
        ("target", true), // from LinkProps (should be omitted)
        ("active", true), // from LinkProps (should be omitted)
    ] {
        button_props.props.push(ResolvedProp {
            span: Span::new(0, 0),
            key: Span::new(0, 0),
            key_name: Some(name.to_string()),
            optional,
            types: vec![RuntimeType::String],
            visibility: ResolvedMemberVisibility::Public,
            type_span: None,
            type_text: None,
            map_local: false,
            span_is_absolute: false,
            declared_in_macro_type_arg: false,
        });
    }

    let mut companion_types = rustc_hash::FxHashMap::default();
    companion_types.insert("ButtonProps".to_string(), button_props);

    let dep = r#"
import type { ButtonProps } from '../types'

type LinkPropsKeys = 'href' | 'target' | 'active'

export interface DashboardSidebarCollapseProps extends Omit<ButtonProps, LinkPropsKeys | 'color' | 'variant'> {
  color?: string
  variant?: string
  side?: string
  ui?: object
}
"#;

    let resolved = resolve_external_type_with_companion(
        "DashboardSidebarCollapseProps",
        dep,
        &companion_types,
        &alloc,
    )
    .expect("should resolve DashboardSidebarCollapseProps");

    let names: Vec<&str> = resolved
        .props
        .iter()
        .filter_map(|p| p.key_name.as_deref())
        .collect();

    // Assert+: inherited props surviving Omit
    assert!(
        names.contains(&"icon"),
        "inherited 'icon' should survive Omit, got: {names:?}"
    );
    assert!(
        names.contains(&"avatar"),
        "inherited 'avatar' should survive Omit, got: {names:?}"
    );
    assert!(
        names.contains(&"label"),
        "inherited 'label' should survive Omit, got: {names:?}"
    );
    assert!(
        names.contains(&"size"),
        "inherited 'size' should survive Omit, got: {names:?}"
    );
    assert!(
        names.contains(&"onClick"),
        "inherited 'onClick' should survive Omit, got: {names:?}"
    );
    assert!(
        names.contains(&"class"),
        "inherited 'class' should survive Omit, got: {names:?}"
    );
    assert!(
        names.contains(&"as"),
        "inherited 'as' should survive Omit, got: {names:?}"
    );
    assert!(
        names.contains(&"type"),
        "inherited 'type' should survive Omit, got: {names:?}"
    );
    assert!(
        names.contains(&"disabled"),
        "inherited 'disabled' should survive Omit, got: {names:?}"
    );

    // Assert+: local re-declared props
    assert!(
        names.contains(&"color"),
        "local 'color' should be present, got: {names:?}"
    );
    assert!(
        names.contains(&"variant"),
        "local 'variant' should be present, got: {names:?}"
    );
    assert!(
        names.contains(&"side"),
        "local 'side' should be present, got: {names:?}"
    );
    assert!(
        names.contains(&"ui"),
        "local 'ui' should be present, got: {names:?}"
    );

    // Assert-: omitted props must NOT be present
    assert!(
        !names.contains(&"href"),
        "Omit'd 'href' should NOT be present, got: {names:?}"
    );
    assert!(
        !names.contains(&"target"),
        "Omit'd 'target' should NOT be present, got: {names:?}"
    );
    assert!(
        !names.contains(&"active"),
        "Omit'd 'active' should NOT be present, got: {names:?}"
    );
}

/// Emits interface extending a companion emits type should preserve
/// all inherited event names and payloads.
///
/// Simulates: ContextMenuContentEmits = Omit<MenuContentImplEmits, 'entryFocus' | 'openAutoFocus'>
#[test]
fn companion_extends_omit_preserves_inherited_emits() {
    let alloc = Allocator::default();

    // Simulate DismissableLayerEmits + closeAutoFocus resolved from another file
    let mut base_emits = ResolvedElements::default();
    for name in [
        "escapeKeyDown",
        "pointerDownOutside",
        "focusOutside",
        "interactOutside",
        "openAutoFocus",
        "closeAutoFocus",
        "entryFocus",
    ] {
        base_emits.call_signatures.push(ResolvedNamedCallSignature {
            span: Span::new(0, 0),
            name: name.to_string(),
            name_span: None,
            signature: ResolvedCallPayloadForm::Call {
                params_text: "event: Event".to_string(),
            },
            map_local: false,
            span_is_absolute: false,
        });
    }

    let mut companion_types = rustc_hash::FxHashMap::default();
    companion_types.insert("MenuContentImplEmits".to_string(), base_emits);

    let dep = r#"
import type { MenuContentImplEmits } from './base'

type MenuContentEmits = Omit<MenuContentImplEmits, 'entryFocus' | 'openAutoFocus'>

export interface ContextMenuContentEmits extends MenuContentEmits {}
"#;

    let resolved = resolve_external_type_with_companion(
        "ContextMenuContentEmits",
        dep,
        &companion_types,
        &alloc,
    )
    .expect("should resolve ContextMenuContentEmits");

    let emit_names: Vec<&str> = resolved
        .call_signatures
        .iter()
        .map(|e| e.name.as_str())
        .collect();

    // Assert+: inherited events surviving Omit
    assert!(
        emit_names.contains(&"escapeKeyDown"),
        "inherited 'escapeKeyDown' should survive Omit, got: {emit_names:?}"
    );
    assert!(
        emit_names.contains(&"pointerDownOutside"),
        "inherited 'pointerDownOutside' should survive, got: {emit_names:?}"
    );
    assert!(
        emit_names.contains(&"focusOutside"),
        "inherited 'focusOutside' should survive, got: {emit_names:?}"
    );
    assert!(
        emit_names.contains(&"interactOutside"),
        "inherited 'interactOutside' should survive, got: {emit_names:?}"
    );
    assert!(
        emit_names.contains(&"closeAutoFocus"),
        "inherited 'closeAutoFocus' should survive, got: {emit_names:?}"
    );

    // Assert-: omitted events must NOT be present
    assert!(
        !emit_names.contains(&"openAutoFocus"),
        "Omit'd 'openAutoFocus' should NOT be present, got: {emit_names:?}"
    );
    assert!(
        !emit_names.contains(&"entryFocus"),
        "Omit'd 'entryFocus' should NOT be present, got: {emit_names:?}"
    );

    // Assert: exactly 5 events
    assert_eq!(
        resolved.call_signatures.len(),
        5,
        "should have exactly 5 events after Omit, got: {emit_names:?}"
    );
}

/// Mapped type slots over a companion type should expand to the companion's
/// concrete keys. Dynamic/unbounded branches should NOT produce concrete slots.
///
/// Simulates: PricingPlansSlots = { [K in keyof PricingPlanSlots]?: ... } & { default: ... }
#[test]
fn companion_mapped_type_slots_expand_concrete_keys() {
    let source = r#"
interface PricingPlanSlots {
  badge(props: {}): any
  title(props: {}): any
  description(props: {}): any
  footer(props: {}): any
}

type PricingPlansSlots = {
  [K in keyof PricingPlanSlots]?: (props: { plan: any }) => any
} & {
  default?(props?: {}): any
}

type Test = PricingPlansSlots
"#;
    let (resolved, diagnostics) = resolve_with_ctx(source);
    // Allow diagnostics (mapped types may produce some) but check output
    let _ = diagnostics;

    let names: Vec<&str> = resolved
        .props
        .iter()
        .map(|p| &source[p.key.start as usize..p.key.end as usize])
        .collect();

    // Assert+: all concrete slot names from the mapped type
    assert!(
        names.contains(&"badge"),
        "mapped 'badge' slot should be present, got: {names:?}"
    );
    assert!(
        names.contains(&"title"),
        "mapped 'title' slot should be present, got: {names:?}"
    );
    assert!(
        names.contains(&"description"),
        "mapped 'description' slot should be present, got: {names:?}"
    );
    assert!(
        names.contains(&"footer"),
        "mapped 'footer' slot should be present, got: {names:?}"
    );
    assert!(
        names.contains(&"default"),
        "explicit 'default' slot should be present, got: {names:?}"
    );

    // Assert: should have 5 slots total
    assert_eq!(
        resolved.props.len(),
        5,
        "should have 5 slots (4 mapped + default), got: {names:?}"
    );
}

/// Dynamic slot branches (Record<string, ...>) should NOT synthesize concrete slot names.
/// Only explicitly named slots should appear.
#[test]
fn dynamic_slot_branches_do_not_synthesize_default() {
    let source = r#"
type TableSlots = {
  expanded?(props: { row: any }): any
  empty?(props?: {}): any
  loading?(props?: {}): any
} & Record<string, (props: any) => any>

type Test = TableSlots
"#;
    let (resolved, _diagnostics) = resolve_with_ctx(source);

    let names: Vec<&str> = resolved
        .props
        .iter()
        .map(|p| &source[p.key.start as usize..p.key.end as usize])
        .collect();

    // Assert+: explicitly named slots are present
    assert!(
        names.contains(&"expanded"),
        "named 'expanded' slot should be present, got: {names:?}"
    );
    assert!(
        names.contains(&"empty"),
        "named 'empty' slot should be present, got: {names:?}"
    );
    assert!(
        names.contains(&"loading"),
        "named 'loading' slot should be present, got: {names:?}"
    );

    // Assert-: Record<string, ...> should NOT produce a 'default' slot
    assert!(
        !names.contains(&"default"),
        "dynamic Record<string,...> should NOT synthesize 'default', got: {names:?}"
    );
}

#[test]
fn resolution_depth_is_bounded_per_call_chain() {
    // Discriminating invariant: the parser cap
    // `PARSER_SYNTACTIC_DEPTH_LIMIT = 256` enforces syntactic
    // stack-safety, not a semantic budget.
    //
    // Depth-as-argument refactor replaced the `Rc<Cell<u16>>` in
    // `TypeResolutionContext` with a module-local thread-local. Deeply
    // nested generics must still bail at `PARSER_SYNTACTIC_DEPTH_LIMIT`
    // rather than stack-overflowing.
    //
    // Synthesises a `Foo<Foo<Foo<...>>>` chain of depth 100 and asserts
    // that resolution terminates cleanly. 100 stays well under the 256
    // cap so resolution runs through the full inner body.
    let mut source = String::from(
        "interface Leaf { value: string }\ninterface Foo<T> { inner: T }\ntype Test = ",
    );
    let depth = 100;
    for _ in 0..depth {
        source.push_str("Foo<");
    }
    source.push_str("Leaf");
    for _ in 0..depth {
        source.push('>');
    }
    source.push('\n');

    let (_resolved, _diagnostics) = resolve_with_ctx(&source);
    // If we reach this line without stack overflow, the depth guard is
    // working. The invariant is termination, not payload completeness —
    // 100 < 256 so resolution should run all the way through.
}

#[test]
fn parser_syntactic_depth_limit_blocks_excessive_chain_cleanly() {
    // Assert the 256 cap holds: a chain of
    // `Foo<Foo<...>>` with depth 300 (> PARSER_SYNTACTIC_DEPTH_LIMIT)
    // must terminate cleanly without stack overflow. Resolution is
    // allowed to truncate; the invariant is termination + no panic.
    let mut source = String::from(
        "interface Leaf { value: string }\ninterface Foo<T> { inner: T }\ntype Test = ",
    );
    let depth = 300;
    for _ in 0..depth {
        source.push_str("Foo<");
    }
    source.push_str("Leaf");
    for _ in 0..depth {
        source.push('>');
    }
    source.push('\n');

    let (_resolved, _diagnostics) = resolve_with_ctx(&source);
}

#[test]
fn parser_syntactic_depth_limit_records_structured_failure_shape() {
    // The depth guard emits a
    // structured `ResolutionBudgetExceeded { limit, actual, context }`
    // record (NOT a silent `None` followed by an `Applied` stub from the
    // retired solver). A deep type-alias chain forces
    // `resolve_type_elements_inner_with_ctx` to re-enter the guard
    // depth-limit-plus times; the last cap-trip is observable via
    // `take_last_resolution_budget_exceeded`.
    let _ = take_last_resolution_budget_exceeded();

    let chain_depth = (PARSER_SYNTACTIC_DEPTH_LIMIT as usize) + 20;
    let mut source = String::from("interface Leaf { value: string }\n");
    for i in 0..chain_depth {
        source.push_str(&format!("type A{i} = A{next};\n", next = i + 1));
    }
    source.push_str(&format!("type A{chain_depth} = Leaf;\n"));
    source.push_str("type Test = A0;\n");

    let (_resolved, _diagnostics) = resolve_with_ctx(&source);

    let record = take_last_resolution_budget_exceeded()
        .expect("ResolutionBudgetExceeded must be recorded when the cap trips");
    assert_eq!(record.limit, PARSER_SYNTACTIC_DEPTH_LIMIT);
    assert!(
        record.actual >= PARSER_SYNTACTIC_DEPTH_LIMIT,
        "ResolutionBudgetExceeded.actual must be >= limit at cap-trip; got {}",
        record.actual
    );
    assert!(!record.context.is_empty());
}

#[test]
fn resolved_elements_supports_deep_partial_eq() {
    // Prerequisite for the `parser_cache_audit` feature: `PartialEq`
    // on `ResolvedElements`, `ResolvedProp`, and `ResolvedNamedCallSignature` must
    // be structural so cache-hit / recomputed-slow-path equality can
    // be asserted.
    let (resolved_a, _) =
        resolve_with_ctx("interface Props { label: string; count?: number }\ntype Test = Props\n");
    let (resolved_b, _) =
        resolve_with_ctx("interface Props { label: string; count?: number }\ntype Test = Props\n");
    assert_eq!(
        resolved_a, resolved_b,
        "identical source must produce structurally equal `ResolvedElements`"
    );
}

/// Discriminating test for `declared_in_macro_type_arg`:
/// own-body literal members must be distinguished from
/// heritage-injected members reaching the surface via
/// `Omit<Imported, K>`.
///
/// The reference shape:
///
/// ```text
/// interface Bar { x: number; kept: number }
/// interface Foo extends Omit<Bar, 'x'> { y: string }
/// ```
///
/// Expected facts after resolving `Foo` from a macro-T root
/// (`defineProps<Foo>()` semantics):
///
/// - `y.declared_in_macro_type_arg == true`  — `y` is the user's
///   own literal body in `Foo`.
/// - `kept.declared_in_macro_type_arg == false` — `kept` enters via
///   `Omit<Bar, 'x'>` heritage descent; the carrier interface body
///   never named it literally.
/// - `x` should NOT appear on the surface — `Omit<Bar, 'x'>` excluded
///   it.
///
/// **Discrimination contract**: if the `from_root_body` threading at
/// `resolve_interface_with_extends_ctx_ref` is reverted (the
/// own-body call site stamps `false` instead of propagating the
/// caller's flag), the `y` assertion below FLIPS to `false` and the
/// test FAILS.
#[test]
fn declared_in_macro_type_arg_true_for_own_body_false_for_omit_heritage() {
    let alloc = Allocator::default();
    let dep = r#"
import type { Bar } from './bar'

export interface Foo extends Omit<Bar, 'x'> {
  y: string
}
"#;

    // Simulate `Bar` resolved from another file: { x: number, kept: number }.
    // Companion fixture mimics the post-leaf state stamped under
    // `from_root_body = false` (heritage descent inside `Foo`'s
    // extends), so the heritage flip is unambiguously the
    // production code under test rather than the fixture stamp.
    let mut companion_types = rustc_hash::FxHashMap::default();
    let mut bar = ResolvedElements::default();
    for name in ["x", "kept"] {
        bar.props.push(ResolvedProp {
            span: Span::new(0, 0),
            key: Span::new(0, 0),
            key_name: Some(name.to_string()),
            optional: false,
            types: vec![RuntimeType::Number],
            visibility: ResolvedMemberVisibility::Public,
            type_span: None,
            type_text: None,
            map_local: false,
            span_is_absolute: false,
            declared_in_macro_type_arg: false,
        });
    }
    companion_types.insert("Bar".to_string(), bar);

    let resolved = resolve_external_type_with_companion("Foo", dep, &companion_types, &alloc)
        .expect("Foo should resolve");

    let by_name: std::collections::HashMap<&str, bool> = resolved
        .props
        .iter()
        .filter_map(|p| {
            p.key_name
                .as_deref()
                .map(|n| (n, p.declared_in_macro_type_arg))
        })
        .collect();

    // Own-body literal member: declared in Foo's literal body — must be `true`.
    assert_eq!(
        by_name.get("y").copied(),
        Some(true),
        "own-body literal `y` should have declared_in_macro_type_arg == true; got {by_name:?}"
    );

    // Heritage member surviving Omit: came from Bar via heritage descent —
    // must remain `false` (Foo's extends body is a heritage boundary).
    assert_eq!(
        by_name.get("kept").copied(),
        Some(false),
        "heritage `kept` (survives Omit<Bar, 'x'>) must have declared_in_macro_type_arg == false; got {by_name:?}"
    );

    // 'x' was explicitly excluded by Omit — it should NOT appear at all.
    assert!(
        !by_name.contains_key("x"),
        "Omit<Bar, 'x'> should exclude `x`; got {by_name:?}"
    );
}

/// Discriminating test for the companion-root restamping fix at
/// `resolve_named_local_type_with_ctx_ref_inner` (`decl.rs`):
/// when a companion type's resolved members include heritage-injected
/// members (e.g. via `extends Omit<Vendor, K>`), consuming the
/// companion at the macro-T root MUST preserve the heritage `false`
/// for inherited members. Blanket-stamping every prop to `true`
/// (the inverse-revert) drops the structural distinction between
/// own-body and heritage-injected members.
///
/// The reference shape:
///
/// ```text
/// // Companion (pre-resolved with per-prop provenance):
/// type Foo = { y: own-body, kept: heritage-injected }
/// // Setup script consumes Foo at root:
/// type Test = Foo
/// ```
///
/// **Discrimination contract**: if the consumer's heritage-aware
/// branch at `decl.rs` is reverted to the inverse blanket-stamp
/// (every prop forced `true` when `from_root_body == true`), the
/// `kept` assertion below FLIPS from `false` to `true` and the
/// test FAILS.
#[test]
fn declared_in_macro_type_arg_companion_root_preserves_inherited_heritage_false() {
    let mut companion_types = FxHashMap::default();
    let mut foo = ResolvedElements::default();

    // Own-body literal member of the companion — marked `true` by the
    // post-fix `extract_companion_types` producer (which resolves with
    // `from_root_body = true`).
    foo.props.push(ResolvedProp {
        span: Span::new(0, 0),
        key: Span::new(0, 0),
        key_name: Some("y".to_string()),
        optional: false,
        types: vec![RuntimeType::Number],
        visibility: ResolvedMemberVisibility::Public,
        type_span: None,
        type_text: None,
        map_local: false,
        span_is_absolute: false,
        declared_in_macro_type_arg: true,
    });
    // Heritage-injected member — marked `false` by the producer
    // (heritage descent inside the companion's `extends` forces
    // `from_root_body = false`).
    foo.props.push(ResolvedProp {
        span: Span::new(0, 0),
        key: Span::new(0, 0),
        key_name: Some("kept".to_string()),
        optional: false,
        types: vec![RuntimeType::Number],
        visibility: ResolvedMemberVisibility::Public,
        type_span: None,
        type_text: None,
        map_local: false,
        span_is_absolute: false,
        declared_in_macro_type_arg: false,
    });
    companion_types.insert("Foo".to_string(), foo);

    let (resolved, _diags) =
        resolve_with_ctx_ref_and_companions("type Test = Foo", companion_types);

    let by_name: std::collections::HashMap<&str, bool> = resolved
        .props
        .iter()
        .filter_map(|p| {
            p.key_name
                .as_deref()
                .map(|n| (n, p.declared_in_macro_type_arg))
        })
        .collect();

    // Own-body literal: must stay `true` at root-body consumption.
    assert_eq!(
        by_name.get("y").copied(),
        Some(true),
        "own-body literal `y` must carry declared_in_macro_type_arg == true \
         at root-body consumption; got {by_name:?}",
    );

    // Heritage-injected: must stay `false` at root-body consumption —
    // the bug case is the inverse blanket-stamp re-flipping this to `true`.
    assert_eq!(
        by_name.get("kept").copied(),
        Some(false),
        "heritage-injected `kept` must carry declared_in_macro_type_arg == false \
         at root-body consumption; got {by_name:?}",
    );
}

/// Companion at heritage descent (`from_root_body = false`) flips every
/// resolved prop's `declared_in_macro_type_arg` to `false`. This guards
/// the symmetric side of the companion consumer contract: when a
/// carrier accesses the companion through its own `extends`, every
/// member of the companion crosses a heritage boundary on the way to
/// the carrier's surface.
#[test]
fn declared_in_macro_type_arg_companion_at_heritage_descent_flips_to_false() {
    let mut companion_types = FxHashMap::default();
    let mut foo = ResolvedElements::default();

    // Own-body literal member of the companion — declared at the
    // companion's macro-T root (true).
    foo.props.push(ResolvedProp {
        span: Span::new(0, 0),
        key: Span::new(0, 0),
        key_name: Some("y".to_string()),
        optional: false,
        types: vec![RuntimeType::Number],
        visibility: ResolvedMemberVisibility::Public,
        type_span: None,
        type_text: None,
        map_local: false,
        span_is_absolute: false,
        declared_in_macro_type_arg: true,
    });
    companion_types.insert("Foo".to_string(), foo);

    // Carrier interface `Carrier` extends the companion `Foo`. The
    // carrier's heritage clause descends into `Foo` at
    // `from_root_body = false`, so the companion's `y` reaches the
    // carrier's surface via heritage and must flip to `false`.
    let (resolved, _diags) = resolve_with_ctx_ref_and_companions(
        "interface Carrier extends Foo { own_member: string }\n\
         type Test = Carrier",
        companion_types,
    );

    let by_name: std::collections::HashMap<&str, bool> = resolved
        .props
        .iter()
        .filter_map(|p| {
            p.key_name
                .as_deref()
                .map(|n| (n, p.declared_in_macro_type_arg))
        })
        .collect();

    // Carrier's own literal body member — stays `true`.
    assert_eq!(
        by_name.get("own_member").copied(),
        Some(true),
        "carrier own-body `own_member` must remain true at root consumption; got {by_name:?}",
    );

    // Companion-inherited member — must flip to false on the heritage hop.
    assert_eq!(
        by_name.get("y").copied(),
        Some(false),
        "companion-inherited `y` must flip to false on the heritage hop \
         (carrier extends Foo); got {by_name:?}",
    );
}

/// Discriminating producer-side test for the `extract_companion_types`
/// behavior at `type_surface/mod.rs`: the producer resolves every
/// companion type with `from_root_body = true` so own-body literal
/// members are emitted with `declared_in_macro_type_arg = true`
/// directly out of the producer, while the heritage-descent boundary
/// inside `resolve_interface_with_extends_ctx_ref` overrides the
/// flag to `false` for `extends`-named-target lookups.
///
/// The two paired `declared_in_macro_type_arg_companion_*` tests
/// above synthesise `companion_types` manually with hard-coded
/// provenance values and therefore discriminate only the consumer-
/// side restamping at `decl.rs`. This test calls
/// `extract_companion_types` directly so a regression at the
/// producer (flipping `from_root_body` back to `false`) is caught
/// at this layer.
///
/// **Discrimination contract**: if `let from_root_body = true;` in
/// `extract_companion_types` is changed to `false`, the own-body
/// `y` assertion below FLIPS from `true` to `false` and the test
/// FAILS. The heritage-injected `kept` assertion is a co-asserted
/// structural invariant (heritage descent forces `false`
/// independently of the producer-side flag) and is not the
/// discriminating signal.
#[test]
fn extract_companion_types_resolves_with_root_body_provenance_true() {
    let allocator = Allocator::default();
    // Mixed companion: one interface with both an own-body literal
    // member (`y`) and a heritage-injected member (`kept`, brought in
    // via `extends Omit<Vendor, 'removed'>`).
    let source = r#"interface Vendor { kept: number; removed: boolean }
interface Foo extends Omit<Vendor, 'removed'> { y: string }"#;
    let source_type = SourceType::ts();
    let parser = Parser::new(&allocator, source, source_type);
    let result = parser.parse();
    assert!(
        result.errors.is_empty(),
        "companion source should parse without errors: {:?}",
        result.errors
    );

    let companion_types = extract_companion_types(&result.program, source.as_bytes(), 0);

    let foo = companion_types
        .get("Foo")
        .expect("`Foo` must be resolved as a companion type");

    let by_name: std::collections::HashMap<&str, bool> = foo
        .props
        .iter()
        .filter_map(|p| {
            p.key_name
                .as_deref()
                .map(|n| (n, p.declared_in_macro_type_arg))
        })
        .collect();

    // Discriminating assertion: own-body literal `y` carries
    // `declared_in_macro_type_arg == true` out of
    // `extract_companion_types`. Reverting the producer to
    // `from_root_body = false` flips this to `false` and FAILS the
    // test.
    assert_eq!(
        by_name.get("y").copied(),
        Some(true),
        "own-body literal `y` must carry declared_in_macro_type_arg == true \
         out of `extract_companion_types`; got {by_name:?}",
    );

    // Co-asserted invariant: the heritage-descent boundary inside
    // `resolve_interface_with_extends_ctx_ref` forces
    // `declared_in_macro_type_arg == false` for the heritage-
    // injected member regardless of the producer-side flag. This
    // assertion holds in both the fixed and the inverted producer
    // state and is therefore not the discriminating signal — it
    // exists to lock the heritage-descent invariant into the
    // producer-side surface.
    assert_eq!(
        by_name.get("kept").copied(),
        Some(false),
        "heritage-injected `kept` must carry declared_in_macro_type_arg == false \
         out of `extract_companion_types` (heritage-descent boundary forces it); \
         got {by_name:?}",
    );
}

/// The `ResolvedElements` member DTOs carry EXACTLY the codegen-survivor
/// field set — no typed-IR sidecar.
///
/// The exhaustive (no `..` rest pattern) destructurings below are the
/// compile-time half of the pin: re-adding a `type_expr` /
/// `type_expr_scope` field (or any other field) to `ResolvedProp` or
/// `ResolvedNamedCallSignature` makes both patterns non-exhaustive and this
/// test FAILS TO COMPILE — the perturbation discriminator. The runtime
/// half proves a real resolution still populates every survivor field
/// (spans, key naming, optionality, runtime types, visibility,
/// type_span/type_text, map_local, span_is_absolute,
/// declared_in_macro_type_arg, and the emit payload signature).
#[test]
fn resolved_elements_members_carry_exactly_the_codegen_survivor_fields() {
    let allocator = Allocator::default();
    let source = r#"
interface Props {
  label?: string
  (e: 'save', id: number): void
}
type Test = Props;
"#;
    let parser = Parser::new(&allocator, source, SourceType::ts());
    let result = parser.parse();
    assert!(result.errors.is_empty(), "fixture must parse");

    let ctx = build_type_context(&result.program, source.as_bytes(), 0);
    let mut guard = vec!["Props".to_string()];
    let resolved = resolve_named_local_type_with_ctx_ref("Props", None, 0, &ctx, true, &mut guard)
        .expect("Props should resolve");

    let prop = resolved
        .props
        .iter()
        .find(|p| {
            p.key_name.as_deref() == Some("label")
                || &source.as_bytes()[p.key.start as usize..p.key.end as usize] == b"label"
        })
        .expect("the `label` prop must be on the surface");

    // Compile-time exhaustive-field pin (ResolvedProp).
    let ResolvedProp {
        span,
        key,
        key_name: _,
        optional,
        types,
        visibility,
        type_span,
        type_text: _,
        map_local,
        span_is_absolute,
        declared_in_macro_type_arg,
    } = prop.clone();

    // Runtime survivor-field assertions.
    assert!(span.end > span.start, "prop span must cover the signature");
    assert!(key.end > key.start, "prop key span must cover the name");
    assert!(optional, "`label?` must resolve optional");
    assert_eq!(types, vec![RuntimeType::String]);
    assert!(visibility.is_public());
    assert!(
        type_span.is_some(),
        "an annotated property keeps its type_span"
    );
    assert!(map_local, "a local prop maps locally");
    assert!(!span_is_absolute, "base_offset 0 keeps spans relative");
    assert!(
        declared_in_macro_type_arg,
        "own-body member resolved with from_root_body=true"
    );

    let emit = resolved
        .call_signatures
        .first()
        .expect("the call-signature emit must be on the surface");

    // Compile-time exhaustive-field pin (ResolvedNamedCallSignature).
    let ResolvedNamedCallSignature {
        span: emit_span,
        name,
        name_span,
        signature,
        map_local: emit_map_local,
        span_is_absolute: emit_span_is_absolute,
    } = emit.clone();

    assert!(emit_span.end > emit_span.start);
    assert_eq!(name, "save");
    assert!(
        name_span.is_some(),
        "string-literal event names carry a span"
    );
    match signature {
        ResolvedCallPayloadForm::Call { params_text } => {
            assert_eq!(params_text, "id: number");
        }
        other => panic!("call-signature emits keep the Call payload form, got {other:?}"),
    }
    assert!(emit_map_local);
    assert!(!emit_span_is_absolute);
}

// ═══════════════════════════════════════════════════════════
// Local re-export / empty-body interface extends (radix Separator)
// ═══════════════════════════════════════════════════════════

/// Empty-body local interface that re-exports a companion base surface.
#[test]
fn empty_body_interface_extends_companion_inherits_all_members() {
    let source = r#"interface Local extends Base {}
type Test = Local;"#;
    let mut companions = FxHashMap::default();
    let mut base = ResolvedElements::default();
    for (name, ty) in [
        ("orientation", RuntimeType::String),
        ("decorative", RuntimeType::Boolean),
        ("asChild", RuntimeType::Boolean),
        ("as", RuntimeType::String),
    ] {
        base.props.push(ResolvedProp {
            span: Span { start: 0, end: 0 },
            key: Span { start: 0, end: 0 },
            key_name: Some(name.to_string()),
            optional: true,
            types: vec![ty],
            visibility: ResolvedMemberVisibility::Public,
            type_span: None,
            type_text: None,
            map_local: true,
            span_is_absolute: false,
            declared_in_macro_type_arg: false,
        });
    }
    companions.insert("Base".to_string(), base);
    let (resolved, diagnostics) = resolve_with_ctx_and_companions(source, companions);
    assert!(diagnostics.is_empty(), "diags: {diagnostics:?}");
    assert_eq!(
        resolved.props.len(),
        4,
        "empty-body Local extends Base must inherit all 4 members, got {:?}",
        resolved
            .props
            .iter()
            .filter_map(|p| p.key_name.as_deref())
            .collect::<Vec<_>>()
    );
}

/// Empty-body interface extends a local interface that itself extends another.
#[test]
fn empty_body_interface_extends_local_chain() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface PrimitiveProps { asChild?: boolean; as?: string }
interface BaseSeparatorProps extends PrimitiveProps {
  orientation?: string
  decorative?: boolean
}
interface SeparatorProps extends BaseSeparatorProps {}
type Test = SeparatorProps;"#,
    );
    assert!(diagnostics.is_empty(), "diags: {diagnostics:?}");
    assert_eq!(
        resolved.props.len(),
        4,
        "SeparatorProps must expand full heritage, got {:?}",
        resolved
            .props
            .iter()
            .filter_map(|p| p.key_name.as_deref())
            .collect::<Vec<_>>()
    );
}

/// type alias re-export of an interface surface.
#[test]
fn type_alias_equals_interface_with_extends() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface Base { foo: string; bar: number }
type Props = Base;
type Test = Props;"#,
    );
    assert!(diagnostics.is_empty());
    assert_eq!(resolved.props.len(), 2);
}

/// Empty-body interface extends type alias of object type.
#[test]
fn empty_body_interface_extends_type_alias_object() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"type Base = { foo: string; bar?: number }
interface Props extends Base {}
type Test = Props;"#,
    );
    assert!(diagnostics.is_empty(), "diags: {diagnostics:?}");
    assert_eq!(
        resolved.props.len(),
        2,
        "Props extends type alias object must have 2 members"
    );
}

/// Intersection of empty-body extends and local members.
#[test]
fn empty_body_extends_plus_intersection() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface Base { a: string }
interface Mid extends Base {}
type Test = Mid & { b: number };"#,
    );
    assert!(diagnostics.is_empty());
    assert_eq!(resolved.props.len(), 2);
}

/// Multiple empty re-exports in a chain.
#[test]
fn empty_body_extends_chain_of_three() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface A { a: string }
interface B extends A {}
interface C extends B {}
interface D extends C {}
type Test = D;"#,
    );
    assert!(diagnostics.is_empty());
    assert_eq!(resolved.props.len(), 1);
    assert_eq!(resolved.props[0].key_name.as_deref(), Some("a"));
}

/// Empty-body extends multiple bases (multi-extends).
#[test]
fn empty_body_extends_multiple_bases() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface A { a: string }
interface B { b: number }
interface C extends A, B {}
type Test = C;"#,
    );
    assert!(diagnostics.is_empty(), "diags: {diagnostics:?}");
    assert_eq!(
        resolved.props.len(),
        2,
        "multi-extends must inherit both members, got {:?}",
        resolved
            .props
            .iter()
            .filter_map(|p| p.key_name.as_deref())
            .collect::<Vec<_>>()
    );
}

/// Empty-body interface extends intersection type alias.
#[test]
fn empty_body_extends_intersection_type_alias() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"type Left = { a: string }
type Right = { b: number }
type Mid = Left & Right
interface Props extends Mid {}
type Test = Props;"#,
    );
    assert!(diagnostics.is_empty(), "diags: {diagnostics:?}");
    assert_eq!(
        resolved.props.len(),
        2,
        "extends intersection alias must have 2 members, got {:?}",
        resolved
            .props
            .iter()
            .filter_map(|p| p.key_name.as_deref())
            .collect::<Vec<_>>()
    );
}

/// Non-empty local interface + empty re-export layer.
#[test]
fn non_empty_interface_plus_empty_reexport_layer() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface Base { base: string }
interface Mid extends Base { mid: number }
interface Props extends Mid {}
type Test = Props;"#,
    );
    assert!(diagnostics.is_empty(), "diags: {diagnostics:?}");
    assert_eq!(
        resolved.props.len(),
        2,
        "must keep own + heritage, got {:?}",
        resolved
            .props
            .iter()
            .filter_map(|p| p.key_name.as_deref())
            .collect::<Vec<_>>()
    );
}

/// Type alias of empty-body interface extends chain.
#[test]
fn type_alias_of_empty_body_extends() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface Base { x: boolean; y: string }
interface Empty extends Base {}
type Props = Empty;
type Test = Props;"#,
    );
    assert!(diagnostics.is_empty(), "diags: {diagnostics:?}");
    assert_eq!(resolved.props.len(), 2);
}

/// External companion empty-body re-export of multi-member surface.
#[test]
fn empty_body_extends_external_companion_multi_member() {
    let source = r#"interface Local extends External {}
type Test = Local;"#;
    let mut companions = FxHashMap::default();
    let mut external = ResolvedElements::default();
    for (name, ty) in [
        ("one", RuntimeType::String),
        ("two", RuntimeType::Number),
        ("three", RuntimeType::Boolean),
    ] {
        external.props.push(ResolvedProp {
            span: Span { start: 0, end: 0 },
            key: Span { start: 0, end: 0 },
            key_name: Some(name.to_string()),
            optional: true,
            types: vec![ty],
            visibility: ResolvedMemberVisibility::Public,
            type_span: None,
            type_text: None,
            map_local: true,
            span_is_absolute: false,
            declared_in_macro_type_arg: false,
        });
    }
    companions.insert("External".to_string(), external);
    let (resolved, diagnostics) = resolve_with_ctx_and_companions(source, companions);
    assert!(diagnostics.is_empty(), "diags: {diagnostics:?}");
    assert_eq!(resolved.props.len(), 3);
}

/// Generic interface empty extends of non-generic base members only.
#[test]
fn empty_body_extends_preserves_optional_flags() {
    let (resolved, diagnostics) = resolve_with_ctx(
        r#"interface Base {
  required: string
  optional?: number
}
interface Props extends Base {}
type Test = Props;"#,
    );
    assert!(diagnostics.is_empty(), "diags: {diagnostics:?}");
    assert_eq!(resolved.props.len(), 2);
    let required = resolved
        .props
        .iter()
        .find(|p| p.key_name.as_deref() == Some("required"))
        .expect("required");
    let optional = resolved
        .props
        .iter()
        .find(|p| p.key_name.as_deref() == Some("optional"))
        .expect("optional");
    assert!(!required.optional, "required must stay required");
    assert!(optional.optional, "optional must stay optional");
}

#[test]
fn companion_type_alias_to_external_emits_keeps_call_signatures() {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    let alloc = Allocator::default();
    let external = resolve_external_type(
        "RootEmits",
        "export type RootEmits = { 'update:open': [value: boolean] }",
        &alloc,
    )
    .expect("external");
    assert!(
        !external.call_signatures.is_empty(),
        "external must have call_signatures"
    );
    let mut ext = rustc_hash::FxHashMap::default();
    ext.insert("RootEmits".to_string(), external);

    let source = "export type AlertEmits = RootEmits\n";
    let ret = Parser::new(&alloc, source, SourceType::ts()).parse();
    let program = alloc.alloc(ret.program);
    let types = extract_companion_types_with_externals(program, source.as_bytes(), 0, Some(&ext));
    let alert = types.get("AlertEmits").expect("AlertEmits");
    assert!(
        !alert.call_signatures.is_empty(),
        "companion alias to external emits must keep call_signatures, got props={:?} calls={:?}",
        alert
            .props
            .iter()
            .map(|p| p.key_name.as_deref())
            .collect::<Vec<_>>(),
        alert
            .call_signatures
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
    );
}

#[test]
fn alias_to_companion_emits_via_named_local() {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    let alloc = Allocator::default();
    let mut companion = rustc_hash::FxHashMap::default();
    let external = resolve_external_type(
        "RootEmits",
        "export type RootEmits = { 'update:open': [value: boolean] }",
        &alloc,
    )
    .unwrap();
    assert!(!external.call_signatures.is_empty());
    companion.insert("RootEmits".to_string(), external);

    let source = "type A = RootEmits\n";
    let ret = Parser::new(&alloc, source, SourceType::ts()).parse();
    let program = alloc.alloc(ret.program);
    let mut ctx = build_type_context(program, source.as_bytes(), 0);
    ctx.extend_companion_types(&companion);
    let mut guard = vec![];
    let resolved = resolve_named_local_type_with_ctx_ref("A", None, 0, &ctx, true, &mut guard)
        .expect("A should resolve");
    assert!(
        !resolved.call_signatures.is_empty(),
        "alias A = RootEmits must get call_signatures from companion, got {:?}",
        resolved
            .call_signatures
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
    );
}

// =========================================================================
// Generic type-parameter runtime prop-type resolution
//
// A member whose type is a generic type parameter must resolve its runtime
// prop type through the parameter's binding: an explicit instantiation
// argument (`Foo<string>`) or the declared `extends` constraint
// (`T extends number`). A type-parameter DEFAULT (`T = boolean`) is never a
// runtime bound — it must NOT leak a `Boolean` prop constructor.
// =========================================================================

#[test]
fn type_param_explicit_arg_resolves_member_runtime_type() {
    let (resolved, diagnostics) = resolve_with_ctx(
        "interface Foo<T> { value: T }\ntype Test = Foo<string>;",
    );
    let value = resolved
        .props
        .iter()
        .find(|p| p.key_name.as_deref() == Some("value"))
        .expect("value prop should resolve");
    assert_eq!(
        format_runtime_types(&value.types),
        "String",
        "explicit `Foo<string>` must resolve member `value: T` to String, got {:?}",
        value.types
    );
    assert!(
        !value.types.contains(&RuntimeType::Unknown),
        "explicit generic argument must not leave the member Unknown"
    );
    assert!(diagnostics.is_empty());
}

#[test]
fn type_param_explicit_arg_resolves_member_runtime_type_ref_path() {
    let (resolved, _) = resolve_with_ctx_ref(
        "interface Foo<T> { value: T }\ntype Test = Foo<number>;",
    );
    let value = resolved
        .props
        .iter()
        .find(|p| p.key_name.as_deref() == Some("value"))
        .expect("value prop should resolve");
    assert_eq!(
        format_runtime_types(&value.types),
        "Number",
        "explicit `Foo<number>` (ref path) must resolve `value: T` to Number, got {:?}",
        value.types
    );
}

#[test]
fn type_param_constraint_resolves_member_runtime_type() {
    // A generic referenced without an explicit argument falls back to the
    // declared `extends` constraint for its runtime prop type.
    let (resolved, _) = resolve_with_ctx(
        "interface Foo<T extends string> { value: T }\ntype Test = Foo;",
    );
    let value = resolved
        .props
        .iter()
        .find(|p| p.key_name.as_deref() == Some("value"))
        .expect("value prop should resolve");
    assert_eq!(
        format_runtime_types(&value.types),
        "String",
        "`T extends string` must resolve `value: T` to String, got {:?}",
        value.types
    );
}

#[test]
fn type_param_default_does_not_leak_boolean_runtime_type() {
    // A type-parameter DEFAULT is not a runtime constructor: `trueValue: T`
    // (T defaulting to boolean) must stay `null`, while a directly-declared
    // `boolean` member stays `Boolean`.
    let (resolved, _) = resolve_with_ctx(
        "interface Foo<T = boolean> { trueValue?: T; rounded?: boolean }\ntype Test = Foo;",
    );
    let true_value = resolved
        .props
        .iter()
        .find(|p| p.key_name.as_deref() == Some("trueValue"))
        .expect("trueValue prop should resolve");
    assert_eq!(
        format_runtime_types(&true_value.types),
        "null",
        "type-param default `T = boolean` must NOT leak a Boolean runtime type, got {:?}",
        true_value.types
    );
    assert!(
        !true_value.types.contains(&RuntimeType::Boolean),
        "defaulted generic member must not become a Boolean prop"
    );
    let rounded = resolved
        .props
        .iter()
        .find(|p| p.key_name.as_deref() == Some("rounded"))
        .expect("rounded prop should resolve");
    assert_eq!(
        format_runtime_types(&rounded.types),
        "Boolean",
        "directly-declared boolean member stays Boolean, got {:?}",
        rounded.types
    );
}

#[test]
fn type_param_constraint_default_prefers_constraint_runtime_type() {
    // With both a constraint and a default, the constraint is the runtime
    // bound (the default is ignored), so `value: T` resolves to Number.
    let (resolved, _) = resolve_with_ctx(
        "interface Foo<T extends number = 3> { value: T }\ntype Test = Foo;",
    );
    let value = resolved
        .props
        .iter()
        .find(|p| p.key_name.as_deref() == Some("value"))
        .expect("value prop should resolve");
    assert_eq!(
        format_runtime_types(&value.types),
        "Number",
        "constraint `extends number` is the runtime bound even with a default, got {:?}",
        value.types
    );
}

#[test]
fn type_param_heritage_generic_resolves_member_runtime_type() {
    // reka-ui heritage pattern: `interface Child extends Base<string>`.
    // The production resolution path (immutable `_ref`, the one the Vue
    // macro pipeline uses) instantiates the heritage type argument, so the
    // inherited `value: T` resolves to String.
    let (resolved, _) = resolve_with_ctx_ref(
        "interface Base<T> { value: T }\ninterface Child extends Base<string> { count: number }\ntype Test = Child;",
    );
    let value = resolved
        .props
        .iter()
        .find(|p| p.key_name.as_deref() == Some("value"))
        .expect("inherited generic member should resolve");
    assert_eq!(
        format_runtime_types(&value.types),
        "String",
        "heritage `extends Base<string>` must resolve inherited `value: T` to String, got {:?}",
        value.types
    );
    assert!(resolved
        .props
        .iter()
        .any(|p| p.key_name.as_deref() == Some("count")));
}

#[test]
fn type_param_transitive_binding_resolves_member_runtime_type() {
    // Nested instantiation through heritage on the production `_ref` path:
    // `Outer<string>` binds `T = string`, forwarded into `Inner<T>` so the
    // inherited `value: U` resolves to String.
    let (resolved, _) = resolve_with_ctx_ref(
        "interface Inner<U> { value: U }\ninterface Outer<T> extends Inner<T> { count: number }\ntype Test = Outer<string>;",
    );
    let value = resolved
        .props
        .iter()
        .find(|p| p.key_name.as_deref() == Some("value"))
        .expect("inherited generic member should resolve");
    assert_eq!(
        format_runtime_types(&value.types),
        "String",
        "transitive generic binding must resolve inherited `value: U` to String, got {:?}",
        value.types
    );
}

// =========================================================================
// Props-vs-emits surface discrimination for tuple-shaped member values
//
// A tuple / indexed-access-to-tuple member VALUE is the Vue emit shorthand
// ONLY on an emits surface. On a props surface it is a genuine prop and
// must NOT be reclassified into `call_signatures` (where the props consumer
// would drop it). With no surface set, the legacy reclassifying behavior is
// preserved.
// =========================================================================

/// Resolve `type Test = ...` on the immutable `_ref` path with an explicit
/// resolution surface set on the context.
fn resolve_with_surface(
    source: &str,
    surface: Option<BlockedTypeSurface>,
) -> ResolvedElements {
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let parser = Parser::new(&allocator, source, source_type);
    let result = parser.parse();
    assert!(
        result.errors.is_empty(),
        "Source should parse without errors: {:?}",
        result.errors
    );
    let mut ctx = build_type_context(&result.program, source.as_bytes(), 0);
    ctx.current_surface = surface;
    for stmt in &result.program.body {
        if let Statement::TSTypeAliasDeclaration(alias) = stmt {
            if alias.id.name.as_str() == "Test" {
                return resolve_type_elements_with_ctx_ref(&alias.type_annotation, 0, &ctx, true);
            }
        }
    }
    panic!("No `type Test = ...` declaration found in source");
}

#[test]
fn indexed_access_tuple_stays_prop_on_props_surface() {
    let resolved = resolve_with_surface(
        "interface LayerEmits { close: [] }\ninterface Props { onClose: LayerEmits['close'] }\ntype Test = Props;",
        Some(BlockedTypeSurface::DefineProps),
    );
    assert!(
        resolved
            .props
            .iter()
            .any(|p| p.key_name.as_deref() == Some("onClose")),
        "on a props surface `onClose: LayerEmits['close']` must stay a prop, props: {:?}",
        resolved
            .props
            .iter()
            .filter_map(|p| p.key_name.as_deref())
            .collect::<Vec<_>>()
    );
    assert!(
        resolved.call_signatures.is_empty(),
        "props-surface member must NOT be reclassified as an emit, got call_signatures: {:?}",
        resolved
            .call_signatures
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn direct_tuple_stays_prop_on_props_surface() {
    let resolved = resolve_with_surface(
        "interface Props { tags: [string, number] }\ntype Test = Props;",
        Some(BlockedTypeSurface::DefineProps),
    );
    assert!(
        resolved
            .props
            .iter()
            .any(|p| p.key_name.as_deref() == Some("tags")),
        "a tuple-typed prop on a props surface stays a prop"
    );
    assert!(resolved.call_signatures.is_empty());
}

#[test]
fn indexed_access_tuple_reclassifies_on_emits_surface() {
    // Regression guard: the reka-ui emit-forwarding pattern must still
    // reclassify on an emits surface.
    let resolved = resolve_with_surface(
        "interface LayerEmits { close: [] }\ninterface Emits { onClose: LayerEmits['close'] }\ntype Test = Emits;",
        Some(BlockedTypeSurface::DefineEmits),
    );
    assert!(
        resolved
            .call_signatures
            .iter()
            .any(|c| c.name == "onClose"),
        "on an emits surface the indexed-access-to-tuple member must become an emit"
    );
    assert!(
        !resolved
            .props
            .iter()
            .any(|p| p.key_name.as_deref() == Some("onClose")),
        "the emit member must not also remain a prop"
    );
}

#[test]
fn indexed_access_tuple_reclassifies_when_surface_unset() {
    // Legacy default (no surface): reclassification is preserved.
    let resolved = resolve_with_surface(
        "interface LayerEmits { close: [] }\ninterface Emits { onClose: LayerEmits['close'] }\ntype Test = Emits;",
        None,
    );
    assert!(
        resolved.call_signatures.iter().any(|c| c.name == "onClose"),
        "with no surface set the legacy reclassifying behavior is preserved"
    );
}

// =========================================================================
// Emit-tuple detection is TSType-node driven, not text-driven: `readonly`
// and parenthesized tuples are still the emit shorthand, and the payload
// text is the inner tuple. An array type (`string[]`) is never an emit.
// =========================================================================

#[test]
fn readonly_tuple_member_is_emit_shorthand() {
    let (resolved, _) =
        resolve_with_ctx("interface Emits { escapeKeydown: readonly [ev: string] }\ntype Test = Emits;");
    let emit = resolved
        .call_signatures
        .iter()
        .find(|c| c.name == "escapeKeydown")
        .expect("`readonly [ev: string]` must be detected as an emit");
    match &emit.signature {
        ResolvedCallPayloadForm::Tuple { tuple_text } => assert_eq!(
            tuple_text, "[ev: string]",
            "emit payload text is the inner tuple, not the `readonly` wrapper"
        ),
        other => panic!("expected tuple payload, got {:?}", other),
    }
    assert!(
        !resolved
            .props
            .iter()
            .any(|p| p.key_name.as_deref() == Some("escapeKeydown")),
        "the emit member must not also remain a prop"
    );
}

#[test]
fn parenthesized_tuple_member_is_emit_shorthand() {
    let (resolved, _) =
        resolve_with_ctx("interface Emits { wrapped: ([ev: number]) }\ntype Test = Emits;");
    assert!(
        resolved.call_signatures.iter().any(|c| c.name == "wrapped"),
        "a parenthesized tuple `([ev: number])` must be detected as an emit, call_sigs={:?}",
        resolved
            .call_signatures
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn readonly_indexed_access_tuple_member_is_emit_shorthand() {
    // The context path (indexed-access-to-tuple) also unwraps `readonly`.
    let (resolved, _) = resolve_with_ctx(
        "interface LayerEmits { close: readonly [ev: string] }\ninterface Emits { onClose: LayerEmits['close'] }\ntype Test = Emits;",
    );
    assert!(
        resolved.call_signatures.iter().any(|c| c.name == "onClose"),
        "indexed access to a `readonly` tuple must resolve as an emit"
    );
}

#[test]
fn array_type_member_stays_prop_not_emit() {
    // Negative guard: an array type is NOT the emit tuple shorthand.
    let (resolved, _) =
        resolve_with_ctx("interface Emits { list: string[] }\ntype Test = Emits;");
    assert!(
        resolved
            .props
            .iter()
            .any(|p| p.key_name.as_deref() == Some("list")),
        "an array-typed member stays a prop"
    );
    assert!(
        resolved.call_signatures.is_empty(),
        "an array type must not be reclassified as an emit"
    );
}
