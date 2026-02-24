//! TSX code generation for the Vue SFC compiler.
//!
//! Generates valid `.tsx` output from a parsed SFC for TypeScript type checking.
//! Unlike the VDOM/Vapor codegen backends (which produce render functions), this
//! module produces a single file with:
//!
//! - **Script block**: preserves TypeScript types, macros, and imports
//! - **Template block**: converts Vue template syntax to valid JSX
//!
//! The TSX output is used by the LSP for hover, completions, go-to-definition,
//! and diagnostics, and by the playground's "Types" tab.
//!
//! ## Architecture
//!
//! This is a **top-level module**, not a template codegen variant. It receives
//! the full `Syntax` AST (script blocks + template AST) and generates two
//! independent output blocks with source maps. It does NOT implement
//! `TemplateCodeGen` or use the shared walker.
//!
//! ```text
//! compile() orchestrator
//!   ├── generate_script()      → JS/TS script block (existing)
//!   ├── generate_template()    → render function (existing)
//!   ├── tsx::script::generate_tsx_script()    → TSX script block (NEW)
//!   └── tsx::template::generate_tsx_template() → TSX template JSX (NEW)
//! ```

pub mod script;
pub mod template;

/// Options for TSX script generation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TsxScriptOptions<'a> {
    /// Component name extracted from filename.
    pub component_name: &'a str,
    /// Sanitized JS identifier for the component (e.g., `_123Widget` for `123-widget.vue`).
    pub js_component_name: &'a str,
    /// Scoped style ID (e.g., `"data-v-abc123"`).
    pub scope_id: &'a str,
    /// Whether any `<style scoped>` block exists.
    pub has_scoped_style: bool,
    /// Runtime module name (e.g., `"vue"`).
    pub runtime_module_name: &'a str,
    /// Whether the SFC uses Vapor mode.
    pub is_vapor: bool,
}

/// Options for TSX template generation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TsxTemplateOptions<'a> {
    /// Self-referencing component name (PascalCase).
    pub self_name: &'a str,
    /// Whether to preserve HTML comments in JSX output.
    pub comments: bool,
}

// ── Utilities ──────────────────────────────────────────────────────

/// Sanitize a filename into a valid JavaScript identifier.
///
/// Rules:
/// - Strip file extension (`.vue`, `.setup.vue`)
/// - Convert kebab-case and dot-separated to PascalCase
/// - Strip non-alphanumeric characters
/// - Prefix with `_` if starts with a digit
/// - Fallback to `"Component"` if empty
pub fn sanitize_js_identifier(filename: &str) -> String {
    // Extract basename (strip directory separators)
    let basename = filename.rsplit(['/', '\\']).next().unwrap_or(filename);

    // Strip extensions: .setup.vue, .vue, or just last extension
    let stem = basename
        .strip_suffix(".setup.vue")
        .or_else(|| basename.strip_suffix(".vue"))
        .or_else(|| basename.rsplit_once('.').map(|(stem, _)| stem))
        .unwrap_or(basename);

    if stem.is_empty() {
        return "Component".to_string();
    }

    // Convert to PascalCase: split on non-alphanumeric, capitalize each segment
    let mut result = String::with_capacity(stem.len());
    let mut capitalize_next = true;

    for ch in stem.chars() {
        if ch.is_alphanumeric() {
            if capitalize_next {
                for upper in ch.to_uppercase() {
                    result.push(upper);
                }
                capitalize_next = false;
            } else {
                result.push(ch);
            }
        } else {
            // Non-alphanumeric characters act as separators
            capitalize_next = true;
        }
    }

    if result.is_empty() {
        return "Component".to_string();
    }

    // Prefix with `_` if starts with a digit
    if result.as_bytes()[0].is_ascii_digit() {
        result.insert(0, '_');
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_basic() {
        assert_eq!(sanitize_js_identifier("my-component.vue"), "MyComponent");
    }

    #[test]
    fn sanitize_numeric_start() {
        assert_eq!(sanitize_js_identifier("123-widget.vue"), "_123Widget");
    }

    #[test]
    fn sanitize_index() {
        assert_eq!(sanitize_js_identifier("index.vue"), "Index");
    }

    #[test]
    fn sanitize_setup_extension() {
        assert_eq!(sanitize_js_identifier("my-comp.setup.vue"), "MyComp");
    }

    #[test]
    fn sanitize_special_chars() {
        assert_eq!(sanitize_js_identifier("special@chars.vue"), "SpecialChars");
    }

    #[test]
    fn sanitize_empty_stem() {
        assert_eq!(sanitize_js_identifier(".vue"), "Component");
    }

    #[test]
    fn sanitize_no_extension() {
        assert_eq!(sanitize_js_identifier("App"), "App");
    }

    #[test]
    fn sanitize_with_path() {
        assert_eq!(
            sanitize_js_identifier("src/components/my-button.vue"),
            "MyButton"
        );
    }

    #[test]
    fn sanitize_dot_separated() {
        assert_eq!(sanitize_js_identifier("my.comp.vue"), "MyComp");
    }

    #[test]
    fn sanitize_already_pascal() {
        assert_eq!(sanitize_js_identifier("MyComponent.vue"), "MyComponent");
    }

    #[test]
    fn sanitize_all_special_chars() {
        assert_eq!(sanitize_js_identifier("----.vue"), "Component");
    }
}
