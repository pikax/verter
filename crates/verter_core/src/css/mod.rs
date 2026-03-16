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

pub use types::{CssError, ProcessStyleOptions, ProcessStyleResult, VBindVar};

use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};

/// Parse CSS with lightningcss and serialize back to normalize it.
///
/// This normalizes comments, strings, at-rules, and nesting so downstream
/// string-level transforms (scoped, modules) see well-formed CSS.
fn normalize_css(css: &str) -> Result<String, CssError> {
    let stylesheet = StyleSheet::parse(css, ParserOptions::default())
        .map_err(|e| CssError::Parse(e.to_string()))?;
    let result = stylesheet
        .to_css(PrinterOptions::default())
        .map_err(|e| CssError::Serialize(e.to_string()))?;
    Ok(result.code)
}

/// Process a CSS style block: apply scoping, CSS modules, and v-bind replacement.
///
/// This is the main entry point, called from:
/// - The Rust `StyleCodegenPlugin` for plain CSS blocks (inline in compileForVite)
/// - The NAPI `processStyle()` binding for preprocessed CSS (from vite-plugin)
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn process_style(
    css: &str,
    options: &ProcessStyleOptions<'_>,
) -> Result<ProcessStyleResult, CssError> {
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

    let module_name = if options.is_module {
        Some(options.module_name.unwrap_or("$style").to_string())
    } else {
        None
    };

    Ok(ProcessStyleResult {
        code: current_css,
        source_map: None, // TODO: source map support
        module_classes,
        module_name,
        v_bind_vars,
    })
}

/// Fast CSS style processing: skip lightningcss normalization.
///
/// Uses the same walker-based scoping but directly on raw CSS (after prepass).
/// Faster than [`process_style`] since it avoids the full CSS parse/serialize
/// cycle. CSS values are not normalized — colors, units, and shorthand remain
/// as authored.
///
/// **Trade-off:** CSS nesting (`&`) is not flattened. If the input uses native
/// CSS nesting, the output may contain `&` selectors that need browser support.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn process_style_fast(
    css: &str,
    options: &ProcessStyleOptions<'_>,
) -> Result<ProcessStyleResult, CssError> {
    // Step 1: Pre-pass — replace v-bind() and Vue pseudo-selectors with valid CSS
    let prepass_result = prepass::prepass(css, options.scope_id);
    let mut current_css = prepass_result.css;
    let v_bind_vars = prepass_result.v_bind_vars;

    // Step 2: Apply CSS modules directly on raw CSS (no normalization)
    let mut module_classes = Vec::new();
    if options.is_module {
        let (modules_css, mapping) =
            modules::apply_css_modules_normalized(&current_css, options.scope_id);
        current_css = modules_css;
        module_classes = mapping;
    }

    // Step 3: Apply scoped selectors directly on raw CSS (no normalization)
    if options.scoped {
        current_css = scoped::apply_scoped_raw(&current_css, options.scope_id);
    }

    let module_name = if options.is_module {
        Some(options.module_name.unwrap_or("$style").to_string())
    } else {
        None
    };

    Ok(ProcessStyleResult {
        code: current_css,
        source_map: None,
        module_classes,
        module_name,
        v_bind_vars,
    })
}

/// Extract CSS class names from a style block (lightweight scan, no lightningcss).
///
/// Scans for `.className` patterns in CSS selectors. Handles basic cases:
/// - `.btn { ... }` → `"btn"`
/// - `.card-title, .card-body { ... }` → `"card-title"`, `"card-body"`
/// - `@media (...) { .responsive { ... } }` → `"responsive"`
///
/// Skips class-like patterns inside strings, comments, and `v-bind()`.
/// May produce false positives from comments, but that's acceptable for IDE completions.
pub fn extract_css_class_names(css: &str) -> Vec<String> {
    let mut classes = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let bytes = css.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        // Skip block comments
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        // Skip line comments (SCSS)
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Skip strings
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            while i < len && bytes[i] != quote {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        // Skip rule bodies (between { and })
        if bytes[i] == b'{' {
            let mut depth = 1;
            i += 1;
            while i < len && depth > 0 {
                match bytes[i] {
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    b'"' | b'\'' => {
                        let q = bytes[i];
                        i += 1;
                        while i < len && bytes[i] != q {
                            if bytes[i] == b'\\' {
                                i += 1;
                            }
                            i += 1;
                        }
                    }
                    b'/' if i + 1 < len && bytes[i + 1] == b'*' => {
                        i += 2;
                        while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                            i += 1;
                        }
                        i += 1; // skip past '*'
                    }
                    b'.' => {
                        // Nested selector (e.g. @media { .foo { } })
                        i += 1;
                        let start = i;
                        while i < len
                            && (bytes[i].is_ascii_alphanumeric()
                                || bytes[i] == b'-'
                                || bytes[i] == b'_')
                        {
                            i += 1;
                        }
                        if i > start {
                            let name = &css[start..i];
                            if seen.insert(name.to_string()) {
                                classes.push(name.to_string());
                            }
                        }
                        continue;
                    }
                    _ => {}
                }
                i += 1;
            }
            continue;
        }
        // Detect class selector
        if bytes[i] == b'.' {
            i += 1;
            let start = i;
            while i < len
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_')
            {
                i += 1;
            }
            if i > start {
                let name = &css[start..i];
                if seen.insert(name.to_string()) {
                    classes.push(name.to_string());
                }
            }
            continue;
        }
        i += 1;
    }

    classes
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
                module_name: None,
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
                module_name: None,
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
                module_name: None,
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
                module_name: None,
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
                module_name: None,
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
                module_name: None,
                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        // Class names should be content-hashed (format: {name}_{8-hex-chars})
        assert!(
            result.code.contains("btn_")
                && !result.code.contains(".btn{")
                && !result.code.contains(".btn "),
            "btn should be hashed. Got: {}",
            result.code
        );
        assert!(
            result.code.contains("card_")
                && !result.code.contains(".card{")
                && !result.code.contains(".card "),
            "card should be hashed. Got: {}",
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
                module_name: None,
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
                module_name: None,
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
        // Hashed class names should be present (content-hash format: {name}_{8-hex-chars})
        assert!(
            result.code.contains("btn_") && !result.code.contains(".btn["),
            "Should have hashed btn. Got: {}",
            result.code
        );
        assert!(
            result.code.contains("card_") && !result.code.contains(".card["),
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
                module_name: None,
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
                module_name: None,
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
                module_name: None,
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

    /// @ai-generated - Test lightningcss normalization on grid layout CSS
    /// to verify that grid-template-areas/columns/rows are preserved
    /// and not collapsed into shorthand that could change behavior.
    #[test]
    fn test_process_style_grid_layout_normalization() {
        let css = r#"
.dashboard {
  display: grid;
  grid-template-areas:
    "header header"
    "sidebar content"
    "footer footer";
  grid-template-columns: 250px 1fr;
  grid-template-rows: auto 1fr auto;
  min-height: 100vh;
}
.stat-card {
  background: white;
  padding: 1.5rem;
  border-radius: 8px;
  box-shadow: 0 2px 4px rgba(0,0,0,0.1);
}
.badge.success {
  background: #d4edda;
  color: #155724;
}
.indicator.online {
  background: #4CAF50;
}
"#;
        let result = process_style(
            css,
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        eprintln!("=== NORMALIZED+SCOPED CSS ===\n{}", result.code);

        // grid-template-areas must be preserved (not collapsed into shorthand)
        assert!(
            result.code.contains("grid-template-areas"),
            "grid-template-areas must be preserved after normalization. Got:\n{}",
            result.code
        );
        // grid-template-columns and rows must be preserved
        assert!(
            result.code.contains("grid-template-columns"),
            "grid-template-columns must be preserved. Got:\n{}",
            result.code
        );
        assert!(
            result.code.contains("grid-template-rows"),
            "grid-template-rows must be preserved. Got:\n{}",
            result.code
        );
        // Compound selectors must be scoped correctly
        assert!(
            result.code.contains(".badge.success[data-v-a4f2eed6]"),
            "Compound selector must be scoped. Got:\n{}",
            result.code
        );
        assert!(
            result.code.contains(".indicator.online[data-v-a4f2eed6]"),
            "Compound selector must be scoped. Got:\n{}",
            result.code
        );
        // box-shadow with rgba must still be valid
        assert!(
            result.code.contains("box-shadow"),
            "box-shadow must be preserved. Got:\n{}",
            result.code
        );
    }

    // ===================================================================
    // @ai-generated - Comparison tests: process_style vs process_style_fast
    // Both paths must produce the same scoped selectors. Values may differ
    // (lightningcss normalizes colors/units), but selectors must match.
    // ===================================================================

    /// Helper: count the number of scoped attribute occurrences in CSS.
    fn count_scope_attrs(css: &str, scope_id: &str) -> usize {
        let attr = format!("[data-v-{}]", scope_id);
        css.matches(&attr).count()
    }

    /// Both paths scope simple classes identically.
    #[test]
    fn test_fast_vs_normal_simple_classes() {
        let css = ".box { color: red; } .card { display: flex; }";
        let opts = ProcessStyleOptions {
            scope_id: "a4f2eed6",
            scoped: true,
            is_module: false,
            module_name: None,
            filename: None,
            sourcemap: false,
        };
        let normal = process_style(css, &opts).unwrap();
        let fast = process_style_fast(css, &opts).unwrap();

        // Both must produce exactly 2 scoped selectors
        assert_eq!(count_scope_attrs(&normal.code, "a4f2eed6"), 2);
        assert_eq!(count_scope_attrs(&fast.code, "a4f2eed6"), 2);
        // Both must scope the same selectors
        assert!(normal.code.contains(".box[data-v-a4f2eed6]"));
        assert!(fast.code.contains(".box[data-v-a4f2eed6]"));
        assert!(normal.code.contains(".card[data-v-a4f2eed6]"));
        assert!(fast.code.contains(".card[data-v-a4f2eed6]"));
    }

    /// Both paths scope descendant selectors identically.
    #[test]
    fn test_fast_vs_normal_descendant() {
        let css = ".parent .child { color: red; } .a > .b { color: blue; }";
        let opts = ProcessStyleOptions {
            scope_id: "test1234",
            scoped: true,
            is_module: false,
            module_name: None,
            filename: None,
            sourcemap: false,
        };
        let normal = process_style(css, &opts).unwrap();
        let fast = process_style_fast(css, &opts).unwrap();

        // Both should scope only the last compound selector
        assert!(normal.code.contains(".child[data-v-test1234]"));
        assert!(fast.code.contains(".child[data-v-test1234]"));
        assert!(normal.code.contains(".b[data-v-test1234]"));
        assert!(fast.code.contains(".b[data-v-test1234]"));
        // Parent should NOT be scoped in either
        assert!(!normal.code.contains(".parent[data-v-test1234]"));
        assert!(!fast.code.contains(".parent[data-v-test1234]"));
    }

    /// Both paths scope selectors inside @media correctly.
    #[test]
    fn test_fast_vs_normal_media() {
        let css = ".top { color: red; } @media (max-width: 768px) { .inner { color: blue; } } .bottom { color: green; }";
        let opts = ProcessStyleOptions {
            scope_id: "a4f2eed6",
            scoped: true,
            is_module: false,
            module_name: None,
            filename: None,
            sourcemap: false,
        };
        let normal = process_style(css, &opts).unwrap();
        let fast = process_style_fast(css, &opts).unwrap();

        for (label, code) in [("normal", &normal.code), ("fast", &fast.code)] {
            assert!(
                code.contains(".top[data-v-a4f2eed6]"),
                "{}: .top must be scoped. Got: {}",
                label,
                code
            );
            assert!(
                code.contains(".inner[data-v-a4f2eed6]"),
                "{}: .inner inside @media must be scoped. Got: {}",
                label,
                code
            );
            assert!(
                code.contains(".bottom[data-v-a4f2eed6]"),
                "{}: .bottom must be scoped. Got: {}",
                label,
                code
            );
        }
    }

    /// Both paths skip @keyframes selectors.
    #[test]
    fn test_fast_vs_normal_keyframes() {
        let css =
            "@keyframes fade { from { opacity: 1; } to { opacity: 0; } } .box { color: red; }";
        let opts = ProcessStyleOptions {
            scope_id: "a4f2eed6",
            scoped: true,
            is_module: false,
            module_name: None,
            filename: None,
            sourcemap: false,
        };
        let normal = process_style(css, &opts).unwrap();
        let fast = process_style_fast(css, &opts).unwrap();

        for (label, code) in [("normal", &normal.code), ("fast", &fast.code)] {
            assert!(
                code.contains(".box[data-v-a4f2eed6]"),
                "{}: .box must be scoped. Got: {}",
                label,
                code
            );
            assert!(
                !code.contains("from[data-v"),
                "{}: keyframe 'from' must NOT be scoped. Got: {}",
                label,
                code
            );
            assert!(
                !code.contains("to[data-v"),
                "{}: keyframe 'to' must NOT be scoped. Got: {}",
                label,
                code
            );
        }
    }

    /// Both paths handle pseudo-classes correctly.
    #[test]
    fn test_fast_vs_normal_pseudo() {
        let css = ".btn:hover { color: red; } .item:not(.active) { opacity: 0.5; }";
        let opts = ProcessStyleOptions {
            scope_id: "a4f2eed6",
            scoped: true,
            is_module: false,
            module_name: None,
            filename: None,
            sourcemap: false,
        };
        let normal = process_style(css, &opts).unwrap();
        let fast = process_style_fast(css, &opts).unwrap();

        for (label, code) in [("normal", &normal.code), ("fast", &fast.code)] {
            assert!(
                code.contains(".btn[data-v-a4f2eed6]:hover"),
                "{}: .btn:hover must be scoped before :hover. Got: {}",
                label,
                code
            );
            assert!(
                code.contains(".item[data-v-a4f2eed6]:not(.active)"),
                "{}: .item:not must be scoped before :not. Got: {}",
                label,
                code
            );
        }
    }

    /// Both paths handle v-bind() replacement.
    #[test]
    fn test_fast_vs_normal_v_bind() {
        let css = ".box { color: v-bind(primary); font-size: v-bind('theme.size'); }";
        let opts = ProcessStyleOptions {
            scope_id: "a4f2eed6",
            scoped: true,
            is_module: false,
            module_name: None,
            filename: None,
            sourcemap: false,
        };
        let normal = process_style(css, &opts).unwrap();
        let fast = process_style_fast(css, &opts).unwrap();

        for (label, result) in [("normal", &normal), ("fast", &fast)] {
            assert!(
                result.code.contains("var(--a4f2eed6-primary)"),
                "{}: v-bind(primary) must be replaced. Got: {}",
                label,
                result.code
            );
            assert!(
                result.code.contains("[data-v-a4f2eed6]"),
                "{}: must be scoped. Got: {}",
                label,
                result.code
            );
            assert_eq!(
                result.v_bind_vars.len(),
                2,
                "{}: must have 2 v-bind vars",
                label
            );
        }
    }

    /// Both paths handle CSS modules.
    #[test]
    fn test_fast_vs_normal_modules() {
        let css = ".btn { color: red; } .card { display: flex; }";
        let opts = ProcessStyleOptions {
            scope_id: "a4f2eed6",
            scoped: false,
            is_module: true,
            module_name: None,
            filename: None,
            sourcemap: false,
        };
        let normal = process_style(css, &opts).unwrap();
        let fast = process_style_fast(css, &opts).unwrap();

        for (label, result) in [("normal", &normal), ("fast", &fast)] {
            assert_eq!(
                result.module_classes.len(),
                2,
                "{}: must have 2 module classes",
                label
            );
            // Content-hash format: {name}_{8-hex-chars}
            assert!(
                result.code.contains("btn_") && !result.code.contains(".btn{"),
                "{}: btn must be hashed. Got: {}",
                label,
                result.code
            );
            assert!(
                result.code.contains("card_") && !result.code.contains(".card{"),
                "{}: card must be hashed. Got: {}",
                label,
                result.code
            );
        }
        // Module class mappings should be identical
        assert_eq!(
            normal.module_classes, fast.module_classes,
            "Module class mappings must match"
        );
    }

    /// Both paths handle real-world template-heavy.vue CSS.
    #[test]
    fn test_fast_vs_normal_template_heavy() {
        let sfc_source =
            include_str!("../../../../packages/benchmark/src/fixtures/template-heavy.vue");
        // Extract CSS from the style block
        let style_start = sfc_source
            .find("<style scoped>")
            .expect("must have <style scoped>");
        let css_start = style_start + "<style scoped>".len();
        let css_end = sfc_source[css_start..]
            .find("</style>")
            .expect("must have </style>");
        let css = &sfc_source[css_start..css_start + css_end];

        let opts = ProcessStyleOptions {
            scope_id: "0d04bfeb",
            scoped: true,
            is_module: false,
            module_name: None,
            filename: None,
            sourcemap: false,
        };
        let normal = process_style(css, &opts).unwrap();
        let fast = process_style_fast(css, &opts).unwrap();

        let normal_count = count_scope_attrs(&normal.code, "0d04bfeb");
        let fast_count = count_scope_attrs(&fast.code, "0d04bfeb");

        eprintln!("Normal scoped attrs: {}", normal_count);
        eprintln!("Fast scoped attrs: {}", fast_count);

        assert_eq!(
            normal_count, fast_count,
            "Both paths must produce the same number of scoped attributes.\nNormal: {}\nFast: {}",
            normal_count, fast_count
        );

        // Spot-check key selectors from template-heavy.vue
        for sel in [
            ".dashboard[data-v-0d04bfeb]",
            ".stat-card[data-v-0d04bfeb]",
            ".badge.success[data-v-0d04bfeb]",
            ".indicator.online[data-v-0d04bfeb]",
        ] {
            assert!(
                normal.code.contains(sel),
                "Normal must contain {}. Got:\n{}",
                sel,
                normal.code
            );
            assert!(
                fast.code.contains(sel),
                "Fast must contain {}. Got:\n{}",
                sel,
                fast.code
            );
        }
    }

    /// @ai-generated - Selectors inside @media blocks MUST be scoped.
    /// This is a regression test for the walker bug where selectors inside
    /// @media were skipped because `last_block_end` tracking caused the
    /// inner selector text to include the @media prefix.
    #[test]
    fn test_process_style_media_query_selectors_scoped() {
        let css = r#"
.box { color: red; }
@media (min-width: 768px) {
  .box { color: blue; }
  .container { display: flex; }
}
.footer { color: green; }
"#;
        let result = process_style(
            css,
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        eprintln!("=== @media SCOPED CSS ===\n{}", result.code);

        // Top-level selectors must be scoped
        assert!(
            result.code.contains(".box[data-v-a4f2eed6]"),
            "Top-level .box must be scoped. Got:\n{}",
            result.code
        );
        assert!(
            result.code.contains(".footer[data-v-a4f2eed6]"),
            "Top-level .footer must be scoped. Got:\n{}",
            result.code
        );

        // Selectors INSIDE @media must also be scoped
        // Find the @media block and check its selectors
        let media_start = result.code.find("@media").expect("@media must be present");
        let media_section = &result.code[media_start..];
        assert!(
            media_section.contains(".box[data-v-a4f2eed6]"),
            "@media .box must be scoped. Got:\n{}",
            result.code
        );
        assert!(
            media_section.contains(".container[data-v-a4f2eed6]"),
            "@media .container must be scoped. Got:\n{}",
            result.code
        );
    }

    /// @ai-generated - Selectors inside nested @supports blocks must be scoped.
    #[test]
    fn test_process_style_supports_query_selectors_scoped() {
        let css = r#"
.box { display: grid; }
@supports (display: grid) {
  .grid-item { grid-column: span 2; }
}
"#;
        let result = process_style(
            css,
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        eprintln!("=== @supports SCOPED CSS ===\n{}", result.code);

        assert!(
            result.code.contains(".box[data-v-a4f2eed6]"),
            "Top-level .box must be scoped. Got:\n{}",
            result.code
        );

        // Selector inside @supports must also be scoped
        if result.code.contains("@supports") {
            let supports_start = result.code.find("@supports").unwrap();
            let supports_section = &result.code[supports_start..];
            assert!(
                supports_section.contains(".grid-item[data-v-a4f2eed6]"),
                "@supports .grid-item must be scoped. Got:\n{}",
                result.code
            );
        }
    }

    /// @ai-generated - @media with multiple nested selectors and combined rules.
    #[test]
    fn test_process_style_media_complex_selectors_scoped() {
        let css = r#"
.sidebar { width: 250px; }
@media (max-width: 768px) {
  .sidebar { display: none; }
  .content { width: 100%; }
  .nav .link { color: blue; }
}
@media (min-width: 1200px) {
  .sidebar { width: 300px; }
}
.content { padding: 1rem; }
"#;
        let result = process_style(
            css,
            &ProcessStyleOptions {
                scope_id: "test123",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            },
        )
        .unwrap();

        eprintln!("=== complex @media SCOPED CSS ===\n{}", result.code);

        // All selectors must be scoped, both inside and outside @media
        let scope = "[data-v-test123]";

        // Top-level
        assert!(
            result.code.contains(&format!(".sidebar{}", scope)),
            "Top-level .sidebar must be scoped. Got:\n{}",
            result.code
        );
        assert!(
            result.code.contains(&format!(".content{}", scope)),
            "Top-level .content must be scoped. Got:\n{}",
            result.code
        );

        // Count scoped selectors - should be at least 6 (2 top + 3 in first @media + 1 in second)
        let scope_count = result.code.matches(scope).count();
        assert!(
            scope_count >= 6,
            "Expected at least 6 scoped selectors, found {}. Got:\n{}",
            scope_count,
            result.code
        );
    }

    // ── extract_css_class_names tests ───────────────────────────

    #[test]
    fn extract_basic_classes() {
        let css = ".btn { color: red; } .card { padding: 1rem; }";
        let classes = extract_css_class_names(css);
        assert_eq!(classes, vec!["btn", "card"]);
    }

    #[test]
    fn extract_classes_with_media_query() {
        let css = ".mobile { display: block; } @media (min-width: 768px) { .desktop { display: block; } }";
        let classes = extract_css_class_names(css);
        assert!(classes.contains(&"mobile".to_string()));
        assert!(classes.contains(&"desktop".to_string()));
    }

    #[test]
    fn extract_deduplicates() {
        let css = ".btn { color: red; } .btn:hover { color: blue; }";
        let classes = extract_css_class_names(css);
        assert_eq!(classes.len(), 1);
        assert_eq!(classes[0], "btn");
    }

    #[test]
    fn extract_kebab_case_classes() {
        let css = ".card-title { font-weight: bold; } .card-body { padding: 1rem; }";
        let classes = extract_css_class_names(css);
        assert_eq!(classes, vec!["card-title", "card-body"]);
    }

    #[test]
    fn extract_empty_css() {
        let classes = extract_css_class_names("");
        assert!(classes.is_empty());
    }

    #[test]
    fn extract_no_classes() {
        let css = "div { color: red; } p { margin: 0; }";
        let classes = extract_css_class_names(css);
        assert!(classes.is_empty());
    }
}
