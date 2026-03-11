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
                resolved: resolve_type_elements(&alias.type_annotation, 0),
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
                let resolved = resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx);
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
                let resolved = resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx);
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
                let resolved = resolve_type_elements_with_ctx_ref(&alias.type_annotation, 0, &ctx);
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
                let resolved = resolve_type_elements_with_ctx_ref(&alias.type_annotation, 0, &ctx);
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
fn test_format_runtime_types_filters_unknown() {
    // Unknown types should be filtered out
    assert_eq!(
        format_runtime_types(&[RuntimeType::String, RuntimeType::Unknown]),
        "String"
    );
    assert_eq!(format_runtime_types(&[RuntimeType::Unknown]), "null");
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
        type_span: None,
        map_local: true,
        span_is_absolute: false,
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
        type_span: None,
        map_local: false,
        span_is_absolute: false,
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
    base.emits.push(ResolvedEmit {
        span: Span::new(0, 0),
        name: "submit".to_string(),
        name_span: None,
        signature: ResolvedEmitSignature::Call {
            params_text: "payload: string".to_string(),
        },
        map_local: false,
        span_is_absolute: false,
    });
    companion_types.insert("BaseEmits".to_string(), base);

    let resolved =
        resolve_external_type_with_companion("Emits", dep, &companion_types, &alloc).unwrap();
    assert_eq!(
        resolved.emits.len(),
        2,
        "Emits should include imported and local emits entries"
    );
    assert!(resolved.emits.iter().any(|emit| emit.name == "submit"));
    assert!(resolved.emits.iter().any(|emit| emit.name == "confirm"));
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
    assert_eq!(result3.bindings[0].local_name, "Base");
    assert_eq!(result3.bindings[0].source, "./base");
    assert_eq!(result3.bindings[1].local_name, "Foo");
    assert_eq!(result3.bindings[1].source, "./foo");
    assert_eq!(
        result3.wildcard_reexport_sources,
        vec!["./utils"],
        "should have one wildcard"
    );
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
            let _ = resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx0);
        }
    }
    let span0 = ctx0.diagnostics[0].span;

    // Now resolve with base_offset = 100
    let mut ctx100 = build_type_context(&result.program, source.as_bytes(), 0);
    for stmt in &result.program.body {
        if let Statement::TSTypeAliasDeclaration(alias) = stmt {
            let _ = resolve_type_elements_with_ctx(&alias.type_annotation, 100, &mut ctx100);
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
                let _ = resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx);
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
                let _ = resolve_type_elements_with_ctx(&alias.type_annotation, 0, &mut ctx);
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
        type_span: None,
        map_local: false,
        span_is_absolute: false,
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
        type_span: None,
        map_local: false,
        span_is_absolute: false,
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
