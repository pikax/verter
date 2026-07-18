//! Standalone TypeScript stripping for `.ts`/`.tsx` files.
//!
//! Parses a TypeScript source file with oxc, strips all TypeScript-specific
//! syntax using the same visitor as the Vue SFC pipeline, and returns valid
//! JavaScript. This allows the playground to strip TypeScript from standalone
//! files without a separate `oxc-transform` WASM dependency.

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

pub(crate) mod typescript;

use crate::code_transform::CodeTransform;
use typescript::strip_typescript_types;

/// Result of stripping TypeScript from a standalone file.
pub struct StripTypesResult {
    /// The JavaScript output with TypeScript syntax removed.
    pub code: String,
    /// Any parse errors encountered (non-fatal).
    pub errors: Vec<String>,
}

/// Strip TypeScript syntax from a standalone `.ts`/`.tsx` source file.
///
/// Parses the source as TSX, walks the AST to remove all TypeScript-specific
/// constructs (type annotations, interfaces, type aliases, enums → JS IIFE),
/// and returns the resulting JavaScript code.
///
/// # Arguments
/// * `source` - The TypeScript source code
/// * `allocator` - The oxc allocator (must outlive the parse result)
///
/// # Returns
/// A `StripTypesResult` with the stripped JavaScript code and any parse errors.
pub fn strip_types<'a>(source: &'a str, allocator: &'a Allocator) -> StripTypesResult {
    let source_type = SourceType::tsx();
    let parser = Parser::new(allocator, source, source_type);
    let parse_result = parser.parse();

    let errors: Vec<String> = parse_result.errors.iter().map(|e| e.to_string()).collect();

    let mut code_transform = CodeTransform::new(source, allocator);

    // base_offset is 0 for standalone files (spans are already absolute)
    strip_typescript_types(&parse_result.program, &mut code_transform, 0, source);

    StripTypesResult {
        code: code_transform.build_string(),
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(source: &str) -> String {
        let allocator = Allocator::new();
        let result = strip_types(source, &allocator);
        assert!(
            result.errors.is_empty(),
            "Parse errors: {:?}",
            result.errors
        );
        result.code
    }

    #[test]
    fn test_plain_js_passthrough() {
        let input = "const x = 1;\nfunction foo() { return 42; }\n";
        assert_eq!(strip(input), input);
    }

    #[test]
    fn test_strip_variable_type_annotation() {
        assert_eq!(strip("const x: number = 1;"), "const x = 1;");
    }

    #[test]
    fn test_strip_function_params_and_return_type() {
        let result = strip("function foo(a: string, b: number): boolean { return true; }");
        assert_eq!(result, "function foo(a, b) { return true; }");
    }

    #[test]
    fn test_strip_interface() {
        let result = strip("interface Foo { bar: string; }\nconst x = 1;");
        assert_eq!(result, "\nconst x = 1;");
    }

    #[test]
    fn test_strip_type_alias() {
        let result = strip("type Foo = string | number;\nconst x = 1;");
        assert_eq!(result, "\nconst x = 1;");
    }

    #[test]
    fn test_strip_generics() {
        assert_eq!(
            strip("function foo<T>(a: T): T { return a; }"),
            "function foo(a) { return a; }"
        );
    }

    #[test]
    fn test_strip_as_expression() {
        assert_eq!(strip("const x = y as string;"), "const x = y;");
    }

    #[test]
    fn test_strip_as_in_optional_chaining() {
        assert_eq!(
            strip("const x = (el as HTMLElement)?.focus();"),
            "const x = (el)?.focus();"
        );
    }

    #[test]
    fn test_strip_as_in_optional_member_chain() {
        assert_eq!(
            strip("const x = (event.currentTarget as HTMLElement)?.contains(event.target as HTMLElement);"),
            "const x = (event.currentTarget)?.contains(event.target);"
        );
    }

    #[test]
    fn test_strip_non_null_in_optional_chain() {
        assert_eq!(strip("const x = obj!.foo?.bar;"), "const x = obj.foo?.bar;");
    }

    #[test]
    fn test_strip_generic_call_in_optional_chain() {
        assert_eq!(
            strip("const x = arr?.find<string>(v => v === 'a');"),
            "const x = arr?.find(v => v === 'a');"
        );
    }

    #[test]
    fn test_strip_nested_optional_chain_with_as() {
        assert_eq!(strip("const x = (a as B)?.c?.d;"), "const x = (a)?.c?.d;");
    }

    #[test]
    fn test_strip_computed_member_in_chain() {
        assert_eq!(
            strip("const x = (obj as Record<string, any>)?.[key];"),
            "const x = (obj)?.[key];"
        );
    }

    #[test]
    fn test_strip_satisfies() {
        assert_eq!(strip("const x = y satisfies Foo;"), "const x = y;");
    }

    #[test]
    fn test_strip_non_null_assertion() {
        assert_eq!(strip("const x = y!;"), "const x = y;");
    }

    #[test]
    fn test_strip_as_const() {
        assert_eq!(strip("const x = { a: 1 } as const;"), "const x = { a: 1 };");
    }

    #[test]
    fn test_strip_import_type() {
        assert_eq!(strip("import type { Foo } from 'bar';"), "");
    }

    #[test]
    fn test_strip_per_specifier_type_import() {
        let result = strip("import { type Foo, bar } from 'baz';");
        assert!(result.contains("bar"));
        assert!(!result.contains("Foo"));
    }

    #[test]
    fn test_enum_to_js() {
        let result = strip("enum Color { Red, Green, Blue }");
        assert!(result.contains("var Color"));
        assert!(result.contains("Color[Color[\"Red\"] = 0] = \"Red\""));
        assert!(result.contains("Color[Color[\"Green\"] = 1] = \"Green\""));
        assert!(result.contains("Color[Color[\"Blue\"] = 2] = \"Blue\""));
    }

    #[test]
    fn test_string_enum_to_js() {
        let result = strip("enum Dir { Up = \"UP\", Down = \"DOWN\" }");
        assert!(result.contains("var Dir"));
        assert!(result.contains("Dir[\"Up\"] = \"UP\""));
        assert!(result.contains("Dir[\"Down\"] = \"DOWN\""));
    }

    #[test]
    fn test_arrow_function_types() {
        assert_eq!(
            strip("const fn = (x: number): string => String(x);"),
            "const fn = (x) => String(x);"
        );
    }

    /// TS overload signatures (no body) must be removed entirely. Leaving them
    /// as `function scrollTo(x, y)` without a body is a JS parse error
    /// (element-plus scrollbar.vue).
    #[test]
    fn test_strip_function_overload_signatures() {
        let input = "\
function scrollTo(xCord: number, yCord?: number): void
function scrollTo(options: ScrollToOptions): void
function scrollTo(arg1: unknown, arg2?: number) {
  wrap.scrollTo(arg1, arg2)
}
";
        let result = strip(input);
        assert!(
            !result.contains("function scrollTo(xCord")
                && !result.contains("function scrollTo(options"),
            "overload signatures must be removed, got:\n{result}"
        );
        assert!(
            result.contains("function scrollTo(arg1, arg2)")
                && result.contains("wrap.scrollTo(arg1, arg2)"),
            "implementation must remain, got:\n{result}"
        );
        assert!(
            result.contains('{') && result.contains('}'),
            "implementation body must remain, got:\n{result}"
        );
    }

    /// An EXPORTED overload signature must also remove the `export` keyword and
    /// the trailing `;` — leaving `export ;` is invalid JS.
    #[test]
    fn test_strip_exported_overload_signature() {
        let result = strip(
            "export function f(): void;\nexport function f(a: string): void;\nexport function f(a?: string) { return a }\n",
        );
        assert!(
            !result.contains("export function f();") && !result.contains(": void"),
            "exported overload signatures must be removed cleanly, got:\n{result}"
        );
        assert!(
            !result.contains("export ;") && !result.contains("export  ;"),
            "no dangling export, got:\n{result}"
        );
        assert!(
            result.contains("export function f(a) { return a }"),
            "the exported implementation must remain, got:\n{result}"
        );
    }

    /// An ambient `declare function` must be removed entirely (keyword +
    /// signature + trailing `;`) — it has no runtime body.
    #[test]
    fn test_strip_declare_function() {
        let result = strip("declare function f(a: number): void;\nconst x = 1;\n");
        assert!(
            !result.contains("declare") && !result.contains("function f"),
            "declare function must be removed, got:\n{result}"
        );
        assert!(
            result.contains("const x = 1;"),
            "surrounding runtime code must remain, got:\n{result}"
        );
    }

    /// Optional parameter markers (`name?`) are TypeScript-only and must be
    /// stripped. Leaving `oldPropString?` is a JS parse error (element-plus form.vue).
    #[test]
    fn test_strip_optional_parameter_marker() {
        let result = strip(
            "const removeField = (field: Field, oldPropString?: string) => {\n  return field\n}\n",
        );
        assert!(
            !result.contains("oldPropString?"),
            "optional `?` must be stripped, got:\n{result}"
        );
        assert!(
            result.contains("oldPropString") && result.contains("=>"),
            "param name must remain, got:\n{result}"
        );
    }

    /// An optional parameter marker with NO type annotation (and one preceded by
    /// a comment) must still be stripped — `(a?)` / `(a /*c*/?)` are invalid JS.
    #[test]
    fn test_strip_optional_marker_without_annotation_and_with_comment() {
        let bare = strip("const g = (a?) => a;\n");
        assert!(
            !bare.contains("a?"),
            "bare optional `?` must strip, got:\n{bare}"
        );
        assert!(bare.contains("(a)"), "param must remain, got:\n{bare}");

        let commented = strip("const h = (a /*c*/?) => a;\n");
        assert!(
            !commented.contains('?'),
            "optional `?` after a comment must strip, got:\n{commented}"
        );
        assert!(
            commented.contains("/*c*/"),
            "the comment must be preserved, got:\n{commented}"
        );
    }

    #[test]
    fn test_class_with_ts_features() {
        let result = strip(
            "class Foo extends Bar implements Baz {\n  public x: number = 1;\n  private y: string;\n}",
        );
        assert!(!result.contains("implements"));
        assert!(!result.contains("public"));
        assert!(!result.contains("private"));
        assert!(!result.contains(": number"));
        assert!(!result.contains(": string"));
    }
}
