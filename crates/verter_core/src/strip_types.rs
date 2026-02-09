//! Standalone TypeScript stripping for `.ts`/`.tsx` files.
//!
//! Parses a TypeScript source file with oxc, strips all TypeScript-specific
//! syntax using the same visitor as the Vue SFC pipeline, and returns valid
//! JavaScript. This allows the playground to strip TypeScript from standalone
//! files without a separate `oxc-transform` WASM dependency.

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::code_transform::CodeTransform;
use crate::codegen::vue::strip_types::strip_typescript_types;

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
        code: code_transform.to_string(),
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
