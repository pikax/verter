//! CSS style processor using lightningcss.
//!
//! Provides `process_style()` — a standalone function for processing Vue SFC style blocks.
//! Uses lightningcss for correct CSS parsing and transformation.
//!
//! ## Processing Pipeline
//!
//! 1. **Pre-pass** (`prepass.rs`): Replace Vue-specific syntax with valid CSS markers
//!    - `v-bind(expr)` → `var(--{scopeId}-{sanitized})`
//!    - `:deep(.inner)` → `[__v_deep__] .inner`
//!    - `:slotted(.inner)` → `.inner[__v_slotted__]`
//!
//! 2. **lightningcss parse + transform** (`scoped.rs`, `modules.rs`):
//!    - Scoped: insert `[data-v-{scopeId}]` attribute selectors
//!    - Modules: hash class names and build mapping
//!
//! 3. **Serialization**: lightningcss serializes the AST back to clean CSS

pub mod modules;
pub mod prepass;
pub mod scoped;
pub mod types;
mod walk;

pub use types::{ProcessStyleOptions, ProcessStyleResult, VBindVar};

use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};

/// Parse CSS with lightningcss and serialize back to normalize it.
///
/// This normalizes comments, strings, at-rules, and nesting so downstream
/// string-level transforms (scoped, modules) see well-formed CSS.
fn normalize_css(css: &str) -> Result<String, String> {
    let stylesheet = StyleSheet::parse(css, ParserOptions::default())
        .map_err(|e| format!("CSS parse error: {}", e))?;
    let result = stylesheet
        .to_css(PrinterOptions::default())
        .map_err(|e| format!("CSS serialization error: {}", e))?;
    Ok(result.code)
}

/// Process a CSS style block: apply scoping, CSS modules, and v-bind replacement.
///
/// This is the main entry point, called from:
/// - The Rust `StyleCodegenPlugin` for plain CSS blocks (inline in compileForVite)
/// - The NAPI `processStyle()` binding for preprocessed CSS (from vite-plugin)
pub fn process_style(
    css: &str,
    options: &ProcessStyleOptions<'_>,
) -> Result<ProcessStyleResult, String> {
    // Step 1: Pre-pass — replace v-bind() and Vue pseudo-selectors with valid CSS
    let prepass_result = prepass::prepass(css, options.scope_id);
    let mut current_css = prepass_result.css;
    let v_bind_vars = prepass_result.v_bind_vars;

    // Step 2: Normalize CSS once (if any transform needs it)
    let needs_transform = options.is_module || options.scoped;
    if needs_transform {
        current_css = normalize_css(&current_css)?;
    }

    // Step 3: Apply CSS modules (class name hashing) on normalized CSS
    let mut module_classes = Vec::new();
    if options.is_module {
        let (modules_css, mapping) =
            modules::apply_css_modules_normalized(&current_css, options.scope_id);
        current_css = modules_css;
        module_classes = mapping;
    }

    // Step 4: Apply scoped selectors on normalized CSS
    if options.scoped {
        current_css = scoped::apply_scoped_normalized(&current_css, options.scope_id);
    }

    Ok(ProcessStyleResult {
        code: current_css,
        source_map: None, // TODO: source map support
        module_classes,
        v_bind_vars,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_style_scoped_basic() {
        let result = process_style(
            ".box { color: red; }",
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,

                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        assert!(
            result.code.contains(".box[data-v-a4f2eed6]"),
            "Got: {}",
            result.code
        );
    }

    #[test]
    fn test_process_style_scoped_with_v_bind() {
        let result = process_style(
            ".box { color: v-bind(primary); }",
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,

                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        assert!(
            result.code.contains("var(--a4f2eed6-primary)"),
            "Got: {}",
            result.code
        );
        assert!(
            result.code.contains("[data-v-a4f2eed6]"),
            "Got: {}",
            result.code
        );
        assert_eq!(result.v_bind_vars.len(), 1);
        assert_eq!(result.v_bind_vars[0].expression, "primary");
    }

    #[test]
    fn test_process_style_deep() {
        let result = process_style(
            ":deep(.inner) { color: red; }",
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,

                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        assert!(
            result.code.contains("[data-v-a4f2eed6]"),
            "Got: {}",
            result.code
        );
        assert!(result.code.contains(".inner"), "Got: {}", result.code);
        // Inner should NOT have scope attr
        assert!(
            !result.code.contains(".inner[data-v"),
            "Inner should not be scoped. Got: {}",
            result.code
        );
    }

    #[test]
    fn test_process_style_slotted() {
        let result = process_style(
            ":slotted(.slot) { color: red; }",
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,

                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        assert!(
            result.code.contains("[data-v-a4f2eed6-s]"),
            "Got: {}",
            result.code
        );
    }

    #[test]
    fn test_process_style_global() {
        let result = process_style(
            ":global(.reset) { margin: 0; }",
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,

                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        assert!(result.code.contains(".reset"), "Got: {}", result.code);
        assert!(
            !result.code.contains("[data-v"),
            "Should not have scope attr. Got: {}",
            result.code
        );
    }

    #[test]
    fn test_process_style_modules() {
        let result = process_style(
            ".btn { color: red; } .card { display: flex; }",
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: false,
                is_module: true,

                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        assert!(
            result.code.contains("btn_a4f2eed6_"),
            "Got: {}",
            result.code
        );
        assert!(
            result.code.contains("card_a4f2eed6_"),
            "Got: {}",
            result.code
        );
        assert_eq!(result.module_classes.len(), 2);
    }

    #[test]
    fn test_process_style_no_transform() {
        let result = process_style(
            ".box { color: red; }",
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: false,
                is_module: false,

                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        // No scoping, no modules — CSS should pass through (possibly normalized)
        assert!(result.code.contains(".box"), "Got: {}", result.code);
        assert!(
            !result.code.contains("[data-v"),
            "Should not have scope attr. Got: {}",
            result.code
        );
    }

    #[test]
    fn test_process_style_scoped_and_modules() {
        let result = process_style(
            ".btn { color: red; } .card { display: flex; }",
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: true,

                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        // Classes should be hashed AND scoped
        assert_eq!(result.module_classes.len(), 2);
        assert!(
            result.code.contains("[data-v-a4f2eed6]"),
            "Should have scope attr. Got: {}",
            result.code
        );
        // Hashed class names should be present
        assert!(
            result.code.contains("btn_a4f2eed6_"),
            "Should have hashed btn. Got: {}",
            result.code
        );
        assert!(
            result.code.contains("card_a4f2eed6_"),
            "Should have hashed card. Got: {}",
            result.code
        );
    }

    #[test]
    fn test_process_style_pseudo_class_ordering() {
        let result = process_style(
            ".btn:hover { color: red; }",
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,

                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        assert!(
            result.code.contains(".btn[data-v-a4f2eed6]:hover"),
            "Scope should be before :hover. Got: {}",
            result.code
        );
    }

    #[test]
    fn test_process_style_pseudo_class_and_pseudo_element() {
        let result = process_style(
            ".btn:hover::before { content: ''; }",
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,

                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        assert!(
            result.code.contains(".btn[data-v-a4f2eed6]:hover:before")
                || result.code.contains(".btn[data-v-a4f2eed6]:hover::before"),
            "Scope should be before :hover. Got: {}",
            result.code
        );
    }

    #[test]
    fn test_process_style_pseudo_element_ordering() {
        let result = process_style(
            ".text::before { content: ''; }",
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,

                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        // lightningcss may normalize ::before to :before
        assert!(
            result.code.contains(".text[data-v-a4f2eed6]:before")
                || result.code.contains(".text[data-v-a4f2eed6]::before"),
            "Scope should be before ::before. Got: {}",
            result.code
        );
    }
}
