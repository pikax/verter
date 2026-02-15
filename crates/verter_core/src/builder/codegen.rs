//! Builder functions for the syntax pipeline.
//!
//! Two builder functions:
//! - `compile()` — VDOM/Vapor template codegen (production compiler output)
//! - `compile_with_tsx()` — TSX codegen for IDE type checking

use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
use std::{cell::RefCell, rc::Rc};
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

pub use crate::syntax::plugins::code_gen::css::CssStyleOutput;
use crate::{
    code_transform::{CodeTransform, SourceMapOptions},
    syntax::{
        pipeline::Syntax,
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxPluginOptions, SyntaxResult},
        plugins::{
            code_gen::{
                css::CssGeneratorPlugin, script::ScriptGeneratorPlugin,
                template::TemplateGeneratorPlugin,
            },
            css_parser::CssParserPlugin,
            element_compiler::element_compiler::ElementCompilerPlugin,
            oxc_parser::oxc_parser::OxcParserPlugin,
        },
        types::*,
    },
    tokenizer::byte::{tokenize, tokenize_with_delimiters},
};

// =============================================================================
// Options and Result Types
// =============================================================================

/// Whitespace handling strategy for template compilation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhitespaceStrategy {
    /// Condense whitespace (Vue default): collapse consecutive whitespace to a single space,
    /// remove whitespace-only text nodes between elements.
    Condense,
    /// Preserve all whitespace as-is.
    Preserve,
}

/// Options for the codegen process.
#[derive(Debug, Clone, Default)]
pub struct CodegenOptions {
    /// The filename for source map generation and component name extraction.
    pub filename: Option<String>,
    /// Production mode — affects component ID generation and optimizations.
    pub is_production: bool,
    /// Custom component ID (overrides auto-generation from filename).
    pub component_id: Option<String>,
    /// When true, include the TSX codegen plugin in the pipeline.
    /// When false, the pipeline still runs (producing compiled CSS) but skips TSX generation.
    pub include_tsx: bool,
    /// When true, skip source map generation and base64 encoding.
    /// Returns empty strings for `source_map` and `code_with_source_map`.
    pub skip_source_map: bool,

    // -- Vue compiler parity options --
    /// Custom interpolation delimiters. Default: `("{{", "}}")`.
    pub delimiters: Option<(String, String)>,
    /// Tag name prefixes treated as custom elements (skip component resolution).
    /// E.g. `["ion-", "my-"]` matches `<ion-button>`, `<my-card>`.
    pub custom_elements: Option<Vec<String>>,
    /// Whether to preserve HTML comments in output.
    /// `None` = `!is_production` (comments in dev, stripped in prod).
    pub comments: Option<bool>,
    /// Runtime module name to import helpers from. Default: `"vue"`.
    pub runtime_module_name: Option<String>,
    /// Hoist static VNodes/props to constants outside the render function.
    /// `None` = `true`.
    pub hoist_static: Option<bool>,
    /// Whitespace handling strategy. `None` = `Condense`.
    pub whitespace: Option<WhitespaceStrategy>,
    /// Cache event handler expressions. `None` = `false`.
    pub cache_handlers: Option<bool>,
    /// Inline the render function inside `setup()`.
    /// `None` = `is_production`.
    pub inline: Option<bool>,
    /// Indicates the SFC uses `:slotted()` in styles.
    /// `None` = `true`.
    pub slotted: Option<bool>,
    /// Add `_ctx.`/`$setup.` prefix to template identifiers.
    /// `None` = `true`.
    pub prefix_identifiers: Option<bool>,
}

impl CodegenOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    pub fn with_production(mut self, is_production: bool) -> Self {
        self.is_production = is_production;
        self
    }

    // -- Resolved accessors (apply defaults) --

    /// Whether to preserve HTML comments in output.
    pub fn resolve_comments(&self) -> bool {
        self.comments.unwrap_or(!self.is_production)
    }

    /// Whether to hoist static VNodes/props.
    pub fn resolve_hoist_static(&self) -> bool {
        self.hoist_static.unwrap_or(true)
    }

    /// Whitespace handling strategy.
    pub fn resolve_whitespace(&self) -> WhitespaceStrategy {
        self.whitespace.unwrap_or(WhitespaceStrategy::Condense)
    }

    /// Whether to cache event handlers.
    pub fn resolve_cache_handlers(&self) -> bool {
        self.cache_handlers.unwrap_or(false)
    }

    /// Whether to inline the render function.
    pub fn resolve_inline(&self) -> bool {
        self.inline.unwrap_or(self.is_production)
    }

    /// Whether the SFC uses `:slotted()`.
    pub fn resolve_slotted(&self) -> bool {
        self.slotted.unwrap_or(true)
    }

    /// Whether to prefix template identifiers.
    pub fn resolve_prefix_identifiers(&self) -> bool {
        self.prefix_identifiers.unwrap_or(true)
    }

    /// Runtime module name for helper imports.
    pub fn resolve_runtime_module_name(&self) -> &str {
        self.runtime_module_name.as_deref().unwrap_or("vue")
    }
}

/// Severity level for a compilation diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileDiagnosticSeverity {
    Error,
    Warning,
    Info,
}

/// A structured diagnostic emitted during compilation.
#[derive(Debug, Clone)]
pub struct CompileDiagnostic {
    /// Severity level.
    pub severity: CompileDiagnosticSeverity,
    /// Vue-compatible error code string (e.g., "X_MISSING_END_TAG").
    pub code: String,
    /// Human-readable message.
    pub message: String,
    /// Optional source span (byte offsets into original input).
    pub span: Option<crate::common::Span>,
}

/// Result of the codegen process.
pub struct CodegenResult {
    /// The generated code (VDOM or Vapor render function).
    pub code: String,
    /// The source map as JSON string.
    pub source_map: String,
    /// The transformed code with inline source map appended.
    pub code_with_source_map: String,
    /// Compiled CSS blocks from `<style>` tags (scoped, v-bind, modules applied).
    pub styles: Vec<CssStyleOutput>,
    /// Scope ID for scoped styles (e.g., `"data-v-a4f2eed6"`).
    /// Empty string when no `<style scoped>` blocks exist.
    pub scope_id: String,
    /// Diagnostics (errors, warnings, info) emitted during compilation.
    pub errors: Vec<CompileDiagnostic>,
    /// Time taken in milliseconds.
    pub duration_ms: f64,
}

/// Result of the TSX codegen process.
pub struct TsxCodegenResult {
    /// The generated TSX code (all blocks: script content + template JSX + commented styles).
    pub tsx: String,
    /// Compiled CSS (from processed style blocks — scoped selectors applied, v-bind replaced).
    pub css: String,
    /// CSS processing errors (e.g., lightningcss parse failures).
    pub css_errors: Vec<String>,
    /// Time taken in milliseconds.
    pub duration_ms: f64,
}

// =============================================================================
// Pipeline runner
// =============================================================================

/// Run events through a sequence of plugins, discarding results.
///
/// Each event is passed through all plugins in order. If any plugin drops
/// the event, subsequent plugins don't see it. Replace and Keep both forward
/// the (potentially transformed) event to the next plugin. Stop halts the
/// entire pipeline — no further events are processed.
fn run_pipeline<'a>(
    events: Vec<Event<'a>>,
    plugins: &mut [&mut dyn SyntaxPlugin<'a>],
    ctx: &mut SyntaxPluginContext<'a>,
) {
    'outer: for event in events {
        let mut current = Some(event);
        for plugin in plugins.iter_mut() {
            if let Some(ev) = current.take() {
                match plugin.process_event(ev, ctx) {
                    SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => current = Some(e),
                    SyntaxResult::Drop => break,
                    SyntaxResult::Stop => break 'outer,
                }
            }
        }
    }
}

// =============================================================================
// Helpers
// =============================================================================

/// SHA-256 hash → 8 hex chars (first 4 bytes).
pub(crate) fn get_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..4])
}

/// Extract component name from a filename.
fn extract_component_name(filename: &str) -> String {
    let name = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    let name = name.strip_suffix(".vue").unwrap_or(name);
    let name = name.strip_suffix(".ts").unwrap_or(name);
    let name = name.strip_suffix(".js").unwrap_or(name);
    name.to_string()
}

/// Compute scope_id as 8 hex chars from component name.
fn compute_scope_id(component_name: &str) -> [u8; 8] {
    let hash = get_hash(component_name);
    let hash_bytes = hash.as_bytes();
    let mut scope_id = [0u8; 8];
    scope_id.copy_from_slice(&hash_bytes[..8.min(hash_bytes.len())]);
    scope_id
}

/// Convert plugin diagnostics to public `CompileDiagnostic` structs.
fn convert_diagnostics(
    diagnostics: &[crate::syntax::plugin::Diagnostic],
) -> Vec<CompileDiagnostic> {
    diagnostics
        .iter()
        .map(|d| CompileDiagnostic {
            severity: match d.severity {
                crate::syntax::plugin::DiagnosticSeverity::Error => {
                    CompileDiagnosticSeverity::Error
                }
                crate::syntax::plugin::DiagnosticSeverity::Warning => {
                    CompileDiagnosticSeverity::Warning
                }
                crate::syntax::plugin::DiagnosticSeverity::Info => CompileDiagnosticSeverity::Info,
            },
            code: format!("{:?}", d.code),
            message: d.message.clone(),
            span: d.span,
        })
        .collect()
}

// =============================================================================
// compile — VDOM/Vapor codegen
// =============================================================================

/// Compile a Vue SFC using the syntax pipeline.
///
/// Pipeline:
/// 1. Tokenize input
/// 2. Syntax → events
/// 3. Pipeline: element_compiler → oxc_parser → code_gen_script → code_gen_template
/// 4. Generate source map
/// 5. Return compiled code with source map
pub fn compile(
    input: &str,
    options: &CodegenOptions,
    allocator: &oxc_allocator::Allocator,
) -> CodegenResult {
    let start = Instant::now();
    let bytes = input.as_bytes();

    let syntax_options = if let Some(ref prefixes) = options.custom_elements {
        let prefixes = prefixes.clone();
        SyntaxPluginOptions {
            is_custom_element: Box::new(move |tag_name: &[u8]| {
                prefixes
                    .iter()
                    .any(|prefix| tag_name.starts_with(prefix.as_bytes()))
            }),
            ..SyntaxPluginOptions::default()
        }
    } else {
        SyntaxPluginOptions::default()
    };
    let mut ctx = SyntaxPluginContext {
        input,
        bytes,
        options: &syntax_options,
        diagnostics: Vec::new(),
    };

    let component_name = options
        .filename
        .as_ref()
        .map(|f| extract_component_name(f))
        .unwrap_or_else(|| "App".to_string());

    let mut syntax = Syntax::new(false);
    if let Some((ref open, ref close)) = options.delimiters {
        tokenize_with_delimiters(
            bytes,
            |e| syntax.handle(&e, &ctx),
            open.as_bytes(),
            close.as_bytes(),
        );
    } else {
        tokenize(bytes, |e| syntax.handle(&e, &ctx));
    }

    // Finalize: detect unclosed elements → X_MISSING_END_TAG errors.
    syntax.finalize(bytes);

    // Collect syntax-phase diagnostics into the pipeline context
    ctx.diagnostics.extend(syntax.take_diagnostics());

    // If fatal errors were found (missing/invalid end tags), stop early.
    if syntax.has_fatal_error() {
        let duration_ms = start.elapsed().as_secs_f64() * 1000.0;
        return CodegenResult {
            code: String::new(),
            source_map: String::new(),
            code_with_source_map: String::new(),
            styles: Vec::new(),
            scope_id: String::new(),
            errors: convert_diagnostics(&ctx.diagnostics),
            duration_ms,
        };
    }

    let events = syntax.events();

    // Detect if the SFC has <script setup>. Inline render mode is only valid
    // for script setup components (the render arrow is returned from setup()).
    // For Options API components, we must always use function render() form.
    // We check the raw input since OxcScript events aren't created until the pipeline runs.
    let has_script_setup = input
        .as_bytes()
        .windows(b"<script".len())
        .enumerate()
        .any(|(i, w)| {
            if !w.eq_ignore_ascii_case(b"<script") {
                return false;
            }
            // Find the end of this <script ...> tag
            let rest = &input[i + w.len()..];
            if let Some(gt) = rest.find('>') {
                let attrs = &rest[..gt];
                // Check for `setup` as a standalone word in the tag attributes
                attrs.split_whitespace().any(|a| a == "setup")
            } else {
                false
            }
        });
    let effective_inline = options.resolve_inline() && has_script_setup;

    // Pre-scan for <template vapor> to inform the script codegen plugin.
    let has_vapor_template = input
        .as_bytes()
        .windows(b"<template".len())
        .enumerate()
        .any(|(i, w)| {
            if !w.eq_ignore_ascii_case(b"<template") {
                return false;
            }
            let rest = &input[i + w.len()..];
            if let Some(gt) = rest.find('>') {
                let attrs = &rest[..gt];
                attrs.split_whitespace().any(|a| a == "vapor")
            } else {
                false
            }
        });

    let code_transform = Rc::new(RefCell::new(CodeTransform::new(input, allocator)));

    // CSS plugins — scope_id from custom component_id or hashed component name
    let scope_id = if let Some(ref id) = options.component_id {
        let mut bytes = [b'0'; 8];
        let id_bytes = id.as_bytes();
        let len = id_bytes.len().min(8);
        bytes[..len].copy_from_slice(&id_bytes[..len]);
        bytes
    } else {
        compute_scope_id(&component_name)
    };
    // Parser plugins
    let mut css_parser = CssParserPlugin::new();

    // Code generation plugins
    let mut code_gen_css = CssGeneratorPlugin::new(Rc::clone(&code_transform), scope_id);
    let mut code_gen_script = ScriptGeneratorPlugin::new(
        Rc::clone(&code_transform),
        &component_name,
        false,
        options.is_production,
    )
    .with_scope_id(scope_id)
    .with_inline_template(effective_inline)
    .with_runtime_module_name(options.resolve_runtime_module_name().to_string())
    .with_vapor(has_vapor_template);

    use crate::syntax::plugins::code_gen::template::TemplateOptions;
    let template_options = TemplateOptions {
        is_production: options.is_production,
        inline: effective_inline,
        comments: options.resolve_comments(),
        hoist_static: options.resolve_hoist_static(),
        cache_handlers: options.resolve_cache_handlers(),
        runtime_module_name: options.resolve_runtime_module_name().to_string(),
        prefix_identifiers: options.resolve_prefix_identifiers(),
    };
    let mut code_gen_template =
        TemplateGeneratorPlugin::with_options(Rc::clone(&code_transform), template_options);

    {
        // transient plugins
        let mut script_ec = ElementCompilerPlugin::new();
        let mut script_oxc = OxcParserPlugin::new(allocator);

        // Pipeline: parsers first, code_gen last (code_gen order independent)
        let pipeline: &mut [&mut dyn SyntaxPlugin] = &mut [
            &mut script_ec,
            &mut css_parser,
            &mut script_oxc,
            &mut code_gen_css,
            &mut code_gen_script,
            &mut code_gen_template,
        ];

        run_pipeline(events, pipeline, &mut ctx);
    }

    // Flush deferred operations (e.g. batched binding patches) before reading.
    code_gen_template.finalize();

    // Emit generated import statements (template helpers + script helpers).
    code_gen_template.emit_imports();
    code_gen_script.end(&ctx);

    let code = code_transform.borrow().build_string();
    let styles = code_gen_css.take_styles();

    // Scoped styles: wrap component in __sfc__ variable and set __scopeId
    let has_scoped = styles.iter().any(|s| s.scoped);
    let scope_id_string = if has_scoped {
        let hex = std::str::from_utf8(&scope_id).unwrap_or("00000000");
        format!("data-v-{}", hex)
    } else {
        String::new()
    };
    let code = if has_scoped {
        let scope_id_hex = std::str::from_utf8(&scope_id).unwrap_or("00000000");
        let code = code.replacen("export default ", "const __sfc__ = ", 1);
        format!(
            "{}\n__sfc__.__scopeId = \"data-v-{}\";\nexport default __sfc__;\n",
            code, scope_id_hex
        )
    } else {
        code
    };

    let (source_map, code_with_source_map) = if options.skip_source_map {
        (String::new(), String::new())
    } else {
        // Generate source map
        let source_map_options = SourceMapOptions::new()
            .with_source(
                options
                    .filename
                    .clone()
                    .unwrap_or_else(|| "input.vue".to_string()),
            )
            .with_file(
                options
                    .filename
                    .as_ref()
                    .map(|f| format!("{}.js", f))
                    .unwrap_or_else(|| "output.js".to_string()),
            );

        let sm = code_transform
            .borrow()
            .generate_map_json(source_map_options);

        // Create inline source map
        let source_map_base64 = STANDARD.encode(&sm);
        let cwsm = format!(
            "{}\n//# sourceMappingURL=data:application/json;charset=utf-8;base64,{}",
            code, source_map_base64
        );

        (sm, cwsm)
    };

    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

    let errors = convert_diagnostics(&ctx.diagnostics);

    CodegenResult {
        code,
        source_map,
        code_with_source_map,
        styles,
        scope_id: scope_id_string,
        errors,
        duration_ms,
    }
}

// =============================================================================
// compile_with_tsx — TSX codegen
// =============================================================================

/// Generate TSX from a Vue SFC using the syntax pipeline.
///
/// Pipeline:
/// 1. Tokenize input
/// 2. Syntax → events
/// 3. Script pipeline: element_compiler → oxc_parser → code_gen_script
/// 4. Template pipeline: element_compiler → css_style → oxc_parser → code_gen_tsx
/// 5. Return generated TSX
pub fn compile_with_tsx(
    _input: &str,
    _options: &CodegenOptions,
    _allocator: &oxc_allocator::Allocator,
) -> TsxCodegenResult {
    // TODO: TSX codegen not yet implemented
    TsxCodegenResult {
        tsx: String::new(),
        css: String::new(),
        css_errors: Vec::new(),
        duration_ms: 0.0,
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;

    fn gen_result(input: &str) -> CodegenResult {
        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        compile(input, &options, &allocator)
    }

    // ==================== diagnostics ====================

    #[test]
    fn test_compile_returns_empty_errors_on_valid_input() {
        let input = r#"<template><div>hello</div></template>"#;
        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");

        let result = compile(input, &options, &allocator);

        assert!(
            result.errors.is_empty(),
            "Valid input should produce no errors, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_pipeline_stop_halts_processing() {
        use crate::syntax::plugin::*;
        use crate::syntax::types::Event;

        // A plugin that always stops
        struct StopPlugin;
        impl<'a> SyntaxPlugin<'a> for StopPlugin {
            fn name(&self) -> &str {
                "stop-plugin"
            }
            fn process_event(
                &mut self,
                _event: Event<'a>,
                ctx: &mut SyntaxPluginContext<'a>,
            ) -> SyntaxResult<Event<'a>> {
                ctx.error(
                    "stop-plugin",
                    crate::syntax::plugin::CompilerErrorCode::XInvalidExpression,
                );
                SyntaxResult::Stop
            }
        }

        // A plugin that tracks whether it saw any events
        struct CountPlugin {
            count: usize,
        }
        impl<'a> SyntaxPlugin<'a> for CountPlugin {
            fn name(&self) -> &str {
                "count-plugin"
            }
            fn process_event(
                &mut self,
                event: Event<'a>,
                _ctx: &mut SyntaxPluginContext<'a>,
            ) -> SyntaxResult<Event<'a>> {
                self.count += 1;
                SyntaxResult::Keep(event)
            }
        }

        let opts = SyntaxPluginOptions::default();
        let input = "";
        let mut ctx = SyntaxPluginContext {
            input,
            bytes: input.as_bytes(),
            options: &opts,
            diagnostics: Vec::new(),
        };

        let mut stop = StopPlugin;
        let mut count = CountPlugin { count: 0 };

        // Two events — Stop on first should prevent second from reaching count
        let events = vec![
            Event::Text(crate::syntax::types::Text {
                parent_id: 0,
                start: 0,
                end: 3,
                has_entity: false,
            }),
            Event::Text(crate::syntax::types::Text {
                parent_id: 0,
                start: 3,
                end: 6,
                has_entity: false,
            }),
        ];

        let plugins: &mut [&mut dyn SyntaxPlugin] = &mut [
            &mut stop as &mut dyn SyntaxPlugin,
            &mut count as &mut dyn SyntaxPlugin,
        ];
        run_pipeline(events, plugins, &mut ctx);

        assert_eq!(
            count.count, 0,
            "Stop should prevent events from reaching later plugins"
        );
        assert!(ctx.has_errors(), "Stop plugin should have added an error");
        assert_eq!(ctx.diagnostics.len(), 1);
    }

    // ==================== compile ====================

    #[test]
    fn test_full_pipeline_simple() {
        let input = r#"<template><div>hello</div></template>"#;
        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");

        let result = compile(input, &options, &allocator);

        assert!(!result.code.is_empty(), "Should produce some output");
        assert!(result.duration_ms >= 0.0);
        assert!(!result.source_map.is_empty(), "Should produce source map");
        assert!(
            result.code_with_source_map.contains("sourceMappingURL"),
            "Should have inline source map"
        );
    }

    #[test]
    fn test_pipeline_interpolation() {
        let input = r#"<template><div>{{ msg }}</div></template>"#;
        let allocator = Allocator::new();
        let options = CodegenOptions::new();

        let result = compile(input, &options, &allocator);

        assert!(
            result.code.contains("_toDisplayString"),
            "Should have toDisplayString for interpolation, got: {}",
            result.code
        );
    }

    #[test]
    fn test_pipeline_binding_flow() {
        let input = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new();

        let result = compile(input, &options, &allocator);

        // Binding metadata should flow from script to template
        assert!(
            result.code.contains("_toDisplayString"),
            "Should have toDisplayString, got: {}",
            result.code
        );
    }

    #[test]
    fn test_pipeline_vdom_default() {
        let input = r#"<template><div>text</div></template>"#;
        let allocator = Allocator::new();
        let options = CodegenOptions::new();

        let result = compile(input, &options, &allocator);

        assert!(
            result.code.contains("_createElementBlock"),
            "Root VDOM should use _createElementBlock, got: {}",
            result.code
        );
    }

    #[test]
    fn test_pipeline_with_scoped_style() {
        let input = r#"<template><div class="box">hi</div></template>
<style scoped>.box { color: red; }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("Scoped.vue");

        let result = compile(input, &options, &allocator);

        assert!(!result.code.is_empty(), "Should produce output");
        assert_eq!(result.styles.len(), 1, "Should have one style block");
        assert!(result.styles[0].scoped, "Style should be scoped");
        assert!(
            result.styles[0].code.contains("[data-v-"),
            "Scoped CSS should contain [data-v-] attribute selector, got: {}",
            result.styles[0].code
        );
        assert!(
            result.styles[0].errors.is_empty(),
            "Should have no CSS errors"
        );
    }

    #[test]
    fn test_pipeline_plain_style() {
        let input = r#"<template><div>hi</div></template>
<style>.box { color: red; }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("Plain.vue");

        let result = compile(input, &options, &allocator);

        assert_eq!(result.styles.len(), 1, "Should have one style block");
        assert!(!result.styles[0].scoped, "Style should not be scoped");
        assert!(
            result.styles[0].code.contains(".box"),
            "Plain CSS should preserve selectors, got: {}",
            result.styles[0].code
        );
    }

    #[test]
    fn test_pipeline_multiple_style_blocks() {
        let input = r#"<template><div>hi</div></template>
<style scoped>.a { color: red; }</style>
<style>.b { color: blue; }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("Multi.vue");

        let result = compile(input, &options, &allocator);

        assert_eq!(result.styles.len(), 2, "Should have two style blocks");
        assert!(result.styles[0].scoped, "First block should be scoped");
        assert!(
            !result.styles[1].scoped,
            "Second block should not be scoped"
        );
    }

    #[test]
    fn test_pipeline_scoped_v_bind() {
        let input = r#"<template><div>hi</div></template>
<style scoped>.box { color: v-bind(color); }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("VBind.vue");

        let result = compile(input, &options, &allocator);

        assert_eq!(result.styles.len(), 1);
        assert!(
            result.styles[0].code.contains("var(--"),
            "v-bind should be replaced with CSS variable, got: {}",
            result.styles[0].code
        );
    }

    #[test]
    fn test_pipeline_css_modules() {
        let input = r#"<template><div>hi</div></template>
<style module>.btn { color: red; }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("Modules.vue");

        let result = compile(input, &options, &allocator);

        assert_eq!(result.styles.len(), 1);
        assert!(result.styles[0].module.is_some(), "Should have module info");
        let module = result.styles[0].module.as_ref().unwrap();
        assert!(!module.classes.is_empty(), "Should have class mappings");
        assert!(
            result.styles[0].code.contains(".btn_"),
            "Module classes should be hashed, got: {}",
            result.styles[0].code
        );
    }

    #[test]
    fn test_pipeline_no_style() {
        let input = r#"<template><div>hi</div></template>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new();

        let result = compile(input, &options, &allocator);

        assert!(result.styles.is_empty(), "Should have no style blocks");
    }

    // ==================== code_gen/css: Style tag removal ====================

    /// @ai-generated — Style tags should be removed from JS output
    #[test]
    fn test_codegen_css_removes_style_tags() {
        let input = r#"<script setup>const x = 1</script>
<template><div>hi</div></template>
<style scoped>.box { color: red; }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            !result.code.contains("<style"),
            "Style tags should be removed from JS output, got:\n{}",
            result.code
        );
        assert!(
            !result.code.contains("</style>"),
            "Style closing tags should be removed from JS output, got:\n{}",
            result.code
        );
        assert!(
            !result.code.contains(".box"),
            "CSS content should not appear in JS output, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — Style tags removed but CSS preserved in styles field
    #[test]
    fn test_codegen_css_styles_field_preserved() {
        let input = r#"<script setup>const x = 1</script>
<template><div>hi</div></template>
<style scoped>.box { color: red; }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert_eq!(
            result.styles.len(),
            1,
            "Should have one style in styles field"
        );
        assert!(result.styles[0].scoped, "Style should be marked scoped");
        assert!(
            result.styles[0].code.contains("[data-v-"),
            "CSS should have scope attribute, got: {}",
            result.styles[0].code
        );
    }

    /// @ai-generated — Multiple style blocks all removed from JS
    #[test]
    fn test_codegen_css_multiple_styles_removed() {
        let input = r#"<script setup>const x = 1</script>
<template><div>hi</div></template>
<style scoped>.a { color: red; }</style>
<style>.b { color: blue; }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            !result.code.contains("<style"),
            "All style tags should be removed, got:\n{}",
            result.code
        );
        assert_eq!(
            result.styles.len(),
            2,
            "Should have two styles in styles field"
        );
    }

    /// @ai-generated — JS output is valid after style tag removal
    #[test]
    fn test_codegen_css_valid_js_after_removal() {
        let input = r#"<script setup>const x = 1</script>
<template><div>{{ x }}</div></template>
<style scoped>.box { color: red; }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        // Validate JS syntax
        let js_allocator = Allocator::default();
        let source_type = oxc_span::SourceType::mjs();
        let parser_result =
            oxc_parser::Parser::new(&js_allocator, &result.code, source_type).parse();
        assert!(
            parser_result.errors.is_empty(),
            "JS output should be valid after style removal.\nErrors: {:?}\nCode:\n{}",
            parser_result.errors,
            result.code
        );
    }

    /// @ai-generated — :deep() transformation preserved in styles output
    #[test]
    fn test_codegen_css_deep_transform() {
        let input = r#"<template><div>hi</div></template>
<style scoped>:deep(.inner) { color: red; }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert_eq!(result.styles.len(), 1);
        assert!(
            result.styles[0].code.contains(".inner"),
            ":deep inner selector should be present, got: {}",
            result.styles[0].code
        );
        assert!(
            result.styles[0].code.contains("[data-v-"),
            "Scope attribute should be present, got: {}",
            result.styles[0].code
        );
        // Inner should NOT have scope attr
        assert!(
            !result.styles[0].code.contains(".inner[data-v-"),
            ":deep inner should not be scoped, got: {}",
            result.styles[0].code
        );
    }

    // ==================== useCssVars: Script injection ====================

    /// @ai-generated — v-bind in CSS injects useCssVars in script
    #[test]
    fn test_use_css_vars_single() {
        let input = r#"<script setup>
import { ref } from 'vue'
const color = ref('red')
</script>
<template><div class="box">hi</div></template>
<style scoped>.box { color: v-bind(color); }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            result.code.contains("_useCssVars"),
            "Should inject useCssVars call, got:\n{}",
            result.code
        );
        assert!(
            result.code.contains("color.value"),
            "Should reference color with .value (it's a ref), got:\n{}",
            result.code
        );
        assert!(
            !result.code.contains("_ctx.color"),
            "Should NOT use _ctx.color for setup ref bindings, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — Multiple v-bind expressions in useCssVars
    #[test]
    fn test_use_css_vars_multiple() {
        let input = r#"<script setup>
const color = 'red'
const size = '16px'
</script>
<template><div>hi</div></template>
<style scoped>.box { color: v-bind(color); font-size: v-bind(size); }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            result.code.contains("_useCssVars"),
            "Should inject useCssVars, got:\n{}",
            result.code
        );
        // color and size are plain const (SetupConst), so direct access (no _ctx.)
        assert!(
            result.code.contains("\"): (color)") || result.code.contains("\": (color)"),
            "Should reference color directly (SetupConst), got:\n{}",
            result.code
        );
        assert!(
            result.code.contains("\"): (size)") || result.code.contains("\": (size)"),
            "Should reference size directly (SetupConst), got:\n{}",
            result.code
        );
    }

    /// @ai-generated — v-bind with dotted expression
    #[test]
    fn test_use_css_vars_dotted() {
        let input = r#"<script setup>
const theme = { color: 'red' }
</script>
<template><div>hi</div></template>
<style scoped>.box { color: v-bind('theme.color'); }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            result.code.contains("_useCssVars"),
            "Should inject useCssVars, got:\n{}",
            result.code
        );
        assert!(
            result.code.contains("_ctx.theme.color"),
            "Should reference dotted expression, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — useCssVars adds import statement
    #[test]
    fn test_use_css_vars_import() {
        let input = r#"<script setup>
const color = 'red'
</script>
<template><div>hi</div></template>
<style scoped>.box { color: v-bind(color); }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            result.code.contains("useCssVars as _useCssVars"),
            "Should import useCssVars, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — No v-bind means no useCssVars injection
    #[test]
    fn test_use_css_vars_not_injected_without_v_bind() {
        let input = r#"<script setup>
const x = 1
</script>
<template><div>hi</div></template>
<style scoped>.box { color: red; }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            !result.code.contains("_useCssVars"),
            "Should NOT inject useCssVars without v-bind, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — v-bind in non-scoped style still injects useCssVars
    #[test]
    fn test_use_css_vars_non_scoped() {
        let input = r#"<script setup>
const color = 'red'
</script>
<template><div>hi</div></template>
<style>.box { color: v-bind(color); }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            result.code.contains("_useCssVars"),
            "v-bind in non-scoped style should still inject useCssVars, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — useCssVars output is valid JS
    #[test]
    fn test_use_css_vars_valid_js() {
        let input = r#"<script setup>
const color = 'red'
const size = '16px'
</script>
<template><div>{{ color }}</div></template>
<style scoped>.box { color: v-bind(color); font-size: v-bind(size); }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        let js_allocator = Allocator::default();
        let source_type = oxc_span::SourceType::mjs();
        let parser_result =
            oxc_parser::Parser::new(&js_allocator, &result.code, source_type).parse();
        assert!(
            parser_result.errors.is_empty(),
            "useCssVars output should be valid JS.\nErrors: {:?}\nCode:\n{}",
            parser_result.errors,
            result.code
        );
    }

    #[test]
    fn test_pipeline_empty_template() {
        let input = r#"<template></template>"#;
        let allocator = Allocator::new();
        let options = CodegenOptions::new();

        let result = compile(input, &options, &allocator);

        // Should not crash, output can be empty
        assert!(result.duration_ms >= 0.0);
    }

    #[test]
    fn test_pipeline_script_only() {
        let input = r#"<script setup>
const x = 'hello'
</script>"#;
        let allocator = Allocator::new();
        let options = CodegenOptions::new();

        let result = compile(input, &options, &allocator);

        // No template, so output may be empty but shouldn't crash
        assert!(result.duration_ms >= 0.0);
    }

    // ==================== compile_with_tsx ====================

    fn tsx_options() -> CodegenOptions {
        CodegenOptions {
            include_tsx: true,
            ..CodegenOptions::new()
        }
    }

    #[test]
    #[ignore = "compile_with_tsx is not yet implemented"]
    fn test_tsx_pipeline_simple() {
        let input = r#"<template><div>hello</div></template>"#;
        let allocator = Allocator::new();
        let options = tsx_options();

        let result = compile_with_tsx(input, &options, &allocator);

        assert!(!result.tsx.is_empty(), "Should produce TSX output");
        assert!(
            result.tsx.contains("<div>"),
            "TSX should have standard JSX <div>, got: {}",
            result.tsx
        );
    }

    #[test]
    #[ignore = "compile_with_tsx is not yet implemented"]
    fn test_tsx_pipeline_interpolation() {
        let input = r#"<template><div>{{ msg }}</div></template>"#;
        let allocator = Allocator::new();
        let options = tsx_options();

        let result = compile_with_tsx(input, &options, &allocator);

        assert!(
            result.tsx.contains("_ctx.msg"),
            "TSX should have _ctx.msg for unbound variable, got: {}",
            result.tsx
        );
    }

    #[test]
    #[ignore = "compile_with_tsx is not yet implemented"]
    fn test_tsx_pipeline_binding_flow() {
        let input = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#;

        let allocator = Allocator::new();
        let options = tsx_options();

        let result = compile_with_tsx(input, &options, &allocator);

        assert!(
            result.tsx.contains("count"),
            "TSX should reference count variable, got: {}",
            result.tsx
        );
    }

    #[test]
    #[ignore = "compile_with_tsx is not yet implemented"]
    fn test_tsx_pipeline_with_style() {
        let input = r#"<template><div>hi</div></template>
<style scoped>.box { color: red; }</style>"#;

        let allocator = Allocator::new();
        let mut options = tsx_options();
        options.filename = Some("Styled.vue".to_string());

        let result = compile_with_tsx(input, &options, &allocator);

        assert!(!result.tsx.is_empty());
    }

    // ==================== Helpers ====================

    #[test]
    fn test_extract_component_name_basic() {
        assert_eq!(extract_component_name("App.vue"), "App");
        assert_eq!(extract_component_name("my-component.vue"), "my-component");
        assert_eq!(
            extract_component_name("src/components/MyComp.vue"),
            "MyComp"
        );
    }

    #[test]
    fn test_compute_scope_id_deterministic() {
        let id1 = compute_scope_id("App");
        let id2 = compute_scope_id("App");
        assert_eq!(id1, id2);

        let id3 = compute_scope_id("Other");
        assert_ne!(id1, id3);
    }

    // ==================== Compiler Options Tests ====================

    /// @ai-generated — Custom delimiters compile interpolations correctly
    #[test]
    fn test_option_delimiters() {
        let input = r#"<template><div>{{{ msg }}}</div></template>"#;
        let allocator = Allocator::new();
        let options = CodegenOptions {
            delimiters: Some(("{{{".to_string(), "}}}".to_string())),
            ..CodegenOptions::new()
        };

        let result = compile(input, &options, &allocator);

        assert!(
            result.code.contains("_toDisplayString"),
            "Custom delimiters should trigger interpolation, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — Default delimiters (None) still work
    #[test]
    fn test_option_delimiters_default() {
        let input = r#"<template><div>{{ msg }}</div></template>"#;
        let allocator = Allocator::new();
        let options = CodegenOptions::new();

        let result = compile(input, &options, &allocator);

        assert!(
            result.code.contains("_toDisplayString"),
            "Default delimiters should work, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — Custom elements skip _resolveComponent
    #[test]
    fn test_option_custom_elements() {
        let input = r#"<template><ion-button>click</ion-button></template>"#;
        let allocator = Allocator::new();

        // Without custom_elements: treated as component, emits _resolveComponent
        let result_default = compile(input, &CodegenOptions::new(), &allocator);

        let allocator2 = Allocator::new();
        // With custom_elements: treated as native element, no _resolveComponent
        let options = CodegenOptions {
            custom_elements: Some(vec!["ion-".to_string()]),
            ..CodegenOptions::new()
        };
        let result_custom = compile(input, &options, &allocator2);

        assert!(
            result_default.code.contains("_resolveComponent"),
            "Default should resolve ion-button as component, got:\n{}",
            result_default.code
        );
        assert!(
            !result_custom.code.contains("_resolveComponent"),
            "With custom_elements, ion-button should NOT resolve as component, got:\n{}",
            result_custom.code
        );
    }

    /// @ai-generated — comments: false strips HTML comments
    #[test]
    fn test_option_comments_false() {
        let input = r#"<template><div><!-- hello --><span>text</span></div></template>"#;
        let allocator = Allocator::new();
        let options = CodegenOptions {
            comments: Some(false),
            ..CodegenOptions::new()
        };

        let result = compile(input, &options, &allocator);

        assert!(
            !result.code.contains("_createCommentVNode"),
            "comments: false should strip comment VNodes, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — comments: true preserves HTML comments
    #[test]
    fn test_option_comments_true() {
        let input = r#"<template><div><!-- hello --><span>text</span></div></template>"#;
        let allocator = Allocator::new();
        let options = CodegenOptions {
            comments: Some(true),
            ..CodegenOptions::new()
        };

        let result = compile(input, &options, &allocator);

        assert!(
            result.code.contains("_createCommentVNode"),
            "comments: true should preserve comment VNodes, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — runtime_module_name changes the import source
    #[test]
    fn test_option_runtime_module_name() {
        let input = r#"<script setup>const x = 1</script><template><div>{{ x }}</div></template>"#;
        let allocator = Allocator::new();
        let options = CodegenOptions {
            runtime_module_name: Some("vue/dist/vue.esm-bundler.js".to_string()),
            ..CodegenOptions::new()
        };

        let result = compile(input, &options, &allocator);

        assert!(
            result.code.contains("from 'vue/dist/vue.esm-bundler.js'"),
            "Should import from custom module name, got:\n{}",
            result.code
        );
        assert!(
            !result.code.contains("from 'vue';"),
            "Should NOT import from default 'vue', got:\n{}",
            result.code
        );
    }

    /// @ai-generated — hoist_static: false prevents static prop hoisting
    #[test]
    fn test_option_hoist_static_false() {
        let input = r#"<template><div><span id="foo">text</span></div></template>"#;
        let allocator = Allocator::new();
        let options = CodegenOptions {
            hoist_static: Some(false),
            ..CodegenOptions::new()
        };

        let result = compile(input, &options, &allocator);

        assert!(
            !result.code.contains("_hoisted_"),
            "hoist_static: false should NOT hoist, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — hoist_static: true (default) hoists static props
    #[test]
    fn test_option_hoist_static_true() {
        let input = r#"<template><div><span id="foo">text</span></div></template>"#;
        let allocator = Allocator::new();
        let options = CodegenOptions::new();

        let result = compile(input, &options, &allocator);

        assert!(
            result.code.contains("_hoisted_"),
            "Default should hoist static props, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — inline option decouples from is_production
    #[test]
    fn test_option_inline_explicit() {
        let input = r#"<script setup>const x = 1</script><template><div>{{ x }}</div></template>"#;

        // inline: true in dev mode
        let allocator = Allocator::new();
        let options = CodegenOptions {
            inline: Some(true),
            is_production: false,
            ..CodegenOptions::new()
        };
        let result = compile(input, &options, &allocator);
        assert!(
            result.code.contains("(_ctx,_cache) => {"),
            "inline: true should produce arrow function even in dev, got:\n{}",
            result.code
        );
    }

    #[test]
    #[ignore = "profiling helper — run with --nocapture --ignored"]
    fn profile_pipeline_stages() {
        use crate::syntax::pipeline::Syntax;
        use crate::syntax::plugin::{
            SyntaxPlugin, SyntaxPluginContext, SyntaxPluginOptions, SyntaxResult,
        };
        use crate::syntax::plugins::element_compiler::element_compiler::ElementCompilerPlugin;
        use crate::syntax::plugins::oxc_parser::oxc_parser::OxcParserPlugin;
        use crate::syntax::types::Event;
        use crate::tokenizer::byte::tokenize;
        use std::cell::RefCell;
        use std::rc::Rc;

        let path = format!(
            "{}/tests/fixtures/kitchen-sink.vue",
            env!("CARGO_MANIFEST_DIR")
        );
        let input = std::fs::read_to_string(&path).unwrap();
        let n = 200u32;

        // Helper to run pipeline
        fn run_pipeline_local<'a>(
            events: Vec<Event<'a>>,
            plugins: &mut [&mut dyn SyntaxPlugin<'a>],
            ctx: &mut SyntaxPluginContext<'a>,
        ) -> Vec<Event<'a>> {
            let mut result = Vec::with_capacity(events.len());
            for event in events {
                let mut current = Some(event);
                for plugin in plugins.iter_mut() {
                    if let Some(ev) = current.take() {
                        match plugin.process_event(ev, ctx) {
                            SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => current = Some(e),
                            SyntaxResult::Drop | SyntaxResult::Stop => break,
                        }
                    }
                }
                if let Some(ev) = current {
                    result.push(ev);
                }
            }
            result
        }

        // Stage 1: Tokenize + Syntax only
        let mut total_tok = std::time::Duration::ZERO;
        for _ in 0..n {
            let t = Instant::now();
            let bytes = input.as_bytes();
            let opts = SyntaxPluginOptions::default();
            let ctx = SyntaxPluginContext {
                input: &input,
                bytes,
                options: &opts,
                diagnostics: Vec::new(),
            };
            let mut syntax = Syntax::new(false);
            tokenize(bytes, |e| syntax.handle(&e, &ctx));
            let _ = std::hint::black_box(syntax.events());
            total_tok += t.elapsed();
        }

        // Stage 2: Tokenize + Syntax + ElementCompiler only
        let mut total_ec = std::time::Duration::ZERO;
        for _ in 0..n {
            let alloc = Allocator::new();
            let t = Instant::now();
            let bytes = input.as_bytes();
            let opts = SyntaxPluginOptions::default();
            let mut ctx = SyntaxPluginContext {
                input: &input,
                bytes,
                options: &opts,
                diagnostics: Vec::new(),
            };
            let mut syntax = Syntax::new(false);
            tokenize(bytes, |e| syntax.handle(&e, &ctx));
            let events = syntax.events();

            let _ct = Rc::new(RefCell::new(crate::code_transform::CodeTransform::new(
                &input, &alloc,
            )));
            let mut ec = ElementCompilerPlugin::new();
            let mut pipeline = vec![&mut ec as &mut dyn SyntaxPlugin];
            let _ = std::hint::black_box(run_pipeline_local(events, &mut pipeline, &mut ctx));
            total_ec += t.elapsed();
        }

        // Stage 3: Tokenize + Syntax + EC + OXC parser only
        let mut total_oxc = std::time::Duration::ZERO;
        for _ in 0..n {
            let alloc = Allocator::new();
            let t = Instant::now();
            let bytes = input.as_bytes();
            let opts = SyntaxPluginOptions::default();
            let mut ctx = SyntaxPluginContext {
                input: &input,
                bytes,
                options: &opts,
                diagnostics: Vec::new(),
            };
            let mut syntax = Syntax::new(false);
            tokenize(bytes, |e| syntax.handle(&e, &ctx));
            let events = syntax.events();

            let _ct = Rc::new(RefCell::new(crate::code_transform::CodeTransform::new(
                &input, &alloc,
            )));
            let mut ec = ElementCompilerPlugin::new();
            let mut oxc = OxcParserPlugin::new(&alloc);
            let mut pipeline = vec![
                &mut ec as &mut dyn SyntaxPlugin,
                &mut oxc as &mut dyn SyntaxPlugin,
            ];
            let _ = std::hint::black_box(run_pipeline_local(events, &mut pipeline, &mut ctx));
            total_oxc += t.elapsed();
        }

        // Stage 4: Full pipeline (no source map)
        let mut total_full = std::time::Duration::ZERO;
        for _ in 0..n {
            let alloc = Allocator::new();
            let t = Instant::now();
            let opts = CodegenOptions {
                skip_source_map: true,
                filename: Some("kitchen-sink.vue".to_string()),
                ..Default::default()
            };
            let r = compile(&input, &opts, &alloc);
            std::hint::black_box(&r.code);
            total_full += t.elapsed();
        }

        let tok = total_tok / n;
        let ec = (total_ec / n).saturating_sub(tok);
        let oxc = (total_oxc / n).saturating_sub(total_ec / n);
        let codegen = (total_full / n).saturating_sub(total_oxc / n);
        let full = total_full / n;

        eprintln!(
            "\n=== Pipeline Stages (kitchen-sink.vue, avg of {} runs) ===",
            n
        );
        eprintln!(
            "  Tokenize + Syntax:  {:>7.1}µs  ({:.0}%)",
            tok.as_nanos() as f64 / 1000.0,
            tok.as_nanos() as f64 / full.as_nanos() as f64 * 100.0
        );
        eprintln!(
            "  ElementCompiler:    {:>7.1}µs  ({:.0}%)",
            ec.as_nanos() as f64 / 1000.0,
            ec.as_nanos() as f64 / full.as_nanos() as f64 * 100.0
        );
        eprintln!(
            "  OXC Parser:         {:>7.1}µs  ({:.0}%)",
            oxc.as_nanos() as f64 / 1000.0,
            oxc.as_nanos() as f64 / full.as_nanos() as f64 * 100.0
        );
        eprintln!(
            "  Script + Template:  {:>7.1}µs  ({:.0}%)",
            codegen.as_nanos() as f64 / 1000.0,
            codegen.as_nanos() as f64 / full.as_nanos() as f64 * 100.0
        );
        eprintln!(
            "  Total (no srcmap):  {:>7.1}µs",
            full.as_nanos() as f64 / 1000.0
        );
    }

    #[test]
    #[ignore = "profiling helper — run with --nocapture --ignored"]
    fn profile_chunk_count() {
        let path = format!(
            "{}/tests/fixtures/kitchen-sink.vue",
            env!("CARGO_MANIFEST_DIR")
        );
        let input = std::fs::read_to_string(&path).unwrap();
        let alloc = Allocator::new();

        let syntax_options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext {
            input: &input,
            bytes: input.as_bytes(),
            options: &syntax_options,
            diagnostics: Vec::new(),
        };

        let mut syntax = crate::syntax::pipeline::Syntax::new(false);
        crate::tokenizer::byte::tokenize(input.as_bytes(), |e| syntax.handle(&e, &ctx));
        let events = syntax.events();
        let event_count = events.len();

        let code_transform = Rc::new(RefCell::new(CodeTransform::new(&input, &alloc)));

        let component_name = "KitchenSink";
        let mut code_gen_script =
            crate::syntax::plugins::code_gen::script::ScriptGeneratorPlugin::new(
                Rc::clone(&code_transform),
                component_name,
                false,
                false,
            );
        let mut code_gen_template =
            crate::syntax::plugins::code_gen::template::TemplateGeneratorPlugin::new(
                Rc::clone(&code_transform),
                false,
            );

        {
            let mut ec = crate::syntax::plugins::element_compiler::element_compiler::ElementCompilerPlugin::new();
            let mut oxc =
                crate::syntax::plugins::oxc_parser::oxc_parser::OxcParserPlugin::new(&alloc);
            let mut pipeline: Vec<&mut dyn crate::syntax::plugin::SyntaxPlugin> = vec![
                &mut ec,
                &mut oxc,
                &mut code_gen_script,
                &mut code_gen_template,
            ];
            run_pipeline(events, &mut pipeline, &mut ctx);
        }

        let ct = code_transform.borrow();
        let chunk_count = ct.chunk_count();
        let code = ct.build_string();

        eprintln!("\n=== Chunk Diagnostics (kitchen-sink.vue) ===");
        eprintln!("  Input size:     {} bytes", input.len());
        eprintln!("  Output size:    {} bytes", code.len());
        eprintln!("  Events:         {}", event_count);
        eprintln!("  Final chunks:   {}", chunk_count);
        eprintln!(
            "  Bytes/chunk:    {:.1}",
            code.len() as f64 / chunk_count.max(1) as f64
        );
    }

    #[test]
    #[ignore] // Run manually: cargo test --release -p verter_core -- profile_template_heavy --nocapture
    fn profile_template_heavy_phases() {
        let input = include_str!("../../../../packages/benchmark/src/fixtures/template-heavy.vue");
        let iterations = 5000;

        // Warmup
        for _ in 0..100 {
            let alloc = Allocator::new();
            let mut opts = CodegenOptions::new().with_filename("template-heavy.vue");
            opts.skip_source_map = true;
            let _ = compile(input, &opts, &alloc);
        }

        // Phase 1: Tokenize + Syntax only
        let t = Instant::now();
        for _ in 0..iterations {
            let bytes = input.as_bytes();
            let syntax_options = SyntaxPluginOptions::default();
            let ctx = SyntaxPluginContext {
                input,
                bytes,
                options: &syntax_options,
                diagnostics: Vec::new(),
            };
            let mut syntax = Syntax::new(false);
            tokenize(bytes, |e| syntax.handle(&e, &ctx));
            std::hint::black_box(syntax.events());
        }
        let tokenize_us = t.elapsed().as_micros() as f64 / iterations as f64;

        // Phase 2a: Tokenize + EC + OXC only (no codegen)
        let t = Instant::now();
        for _ in 0..iterations {
            let alloc = Allocator::new();
            let bytes = input.as_bytes();
            let syntax_options = SyntaxPluginOptions::default();
            let mut ctx = SyntaxPluginContext {
                input,
                bytes,
                options: &syntax_options,
                diagnostics: Vec::new(),
            };
            let mut syntax = Syntax::new(false);
            tokenize(bytes, |e| syntax.handle(&e, &ctx));
            let events = syntax.events();

            let code_transform = Rc::new(RefCell::new(CodeTransform::new(input, &alloc)));
            let mut cgs =
                ScriptGeneratorPlugin::new(Rc::clone(&code_transform), "App", false, false);
            let mut ec = ElementCompilerPlugin::new();
            let mut oxc = OxcParserPlugin::new(&alloc);
            let pipeline: &mut [&mut dyn SyntaxPlugin] = &mut [&mut ec, &mut oxc, &mut cgs];
            run_pipeline(events, pipeline, &mut ctx);
            std::hint::black_box(&ec);
        }
        let ec_oxc_us = t.elapsed().as_micros() as f64 / iterations as f64;
        let ec_oxc_only = ec_oxc_us - tokenize_us;

        // Phase 2b: Full pipeline (tokenize + EC + OXC + codegen)
        let t = Instant::now();
        for _ in 0..iterations {
            let alloc = Allocator::new();
            let bytes = input.as_bytes();
            let syntax_options = SyntaxPluginOptions::default();
            let mut ctx = SyntaxPluginContext {
                input,
                bytes,
                options: &syntax_options,
                diagnostics: Vec::new(),
            };
            let mut syntax = Syntax::new(false);
            tokenize(bytes, |e| syntax.handle(&e, &ctx));
            let events = syntax.events();

            let code_transform = Rc::new(RefCell::new(CodeTransform::new(input, &alloc)));
            let mut cgs =
                ScriptGeneratorPlugin::new(Rc::clone(&code_transform), "App", false, false);
            let mut cgt = TemplateGeneratorPlugin::new(Rc::clone(&code_transform), false);
            let mut ec = ElementCompilerPlugin::new();
            let mut oxc = OxcParserPlugin::new(&alloc);
            let pipeline: &mut [&mut dyn SyntaxPlugin] =
                &mut [&mut ec, &mut oxc, &mut cgs, &mut cgt];
            run_pipeline(events, pipeline, &mut ctx);
            std::hint::black_box(&cgt);
        }
        let pipeline_us = t.elapsed().as_micros() as f64 / iterations as f64;
        let run_pipeline_us = pipeline_us - tokenize_us;
        let codegen_only = run_pipeline_us - ec_oxc_only;

        // Phase 3: Full compile (tokenize + pipeline + finalize + to_string)
        let t = Instant::now();
        for _ in 0..iterations {
            let alloc = Allocator::new();
            let mut opts = CodegenOptions::new().with_filename("template-heavy.vue");
            opts.skip_source_map = true;
            let result = compile(input, &opts, &alloc);
            std::hint::black_box(&result.code);
        }
        let total_us = t.elapsed().as_micros() as f64 / iterations as f64;
        let finalize_tostring_us = total_us - pipeline_us;

        eprintln!("\n=== Template-Heavy Phase Breakdown ({iterations} iterations) ===");
        eprintln!(
            "  Tokenize + Syntax:    {tokenize_us:.1}μs ({:.0}%)",
            tokenize_us / total_us * 100.0
        );
        eprintln!(
            "  EC + OXC:             {ec_oxc_only:.1}μs ({:.0}%)",
            ec_oxc_only / total_us * 100.0
        );
        eprintln!(
            "  Template Codegen:     {codegen_only:.1}μs ({:.0}%)",
            codegen_only / total_us * 100.0
        );
        eprintln!(
            "  finalize + to_string: {finalize_tostring_us:.1}μs ({:.0}%)",
            finalize_tostring_us / total_us * 100.0
        );
        eprintln!("  Total:                {total_us:.1}μs");
    }

    // ==================== Scoped styles: __scopeId ====================

    /// @ai-generated — Scoped styles: compiler emits __sfc__.__scopeId in generated code
    #[test]
    fn test_scoped_styles_emit_scope_id() {
        let input = r#"<script setup>
const x = 1
</script>
<template><div>hi</div></template>
<style scoped>.box { color: red; }</style>"#;

        let result = gen_result(input);

        assert!(
            result.code.contains("const __sfc__ = "),
            "Scoped SFC should use intermediate __sfc__ variable, got:\n{}",
            result.code
        );
        assert!(
            result.code.contains("__sfc__.__scopeId = \"data-v-"),
            "Scoped SFC should emit __sfc__.__scopeId assignment, got:\n{}",
            result.code
        );
        assert!(
            result.code.contains("export default __sfc__"),
            "Scoped SFC should re-export __sfc__, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — Non-scoped styles: no __sfc__ or __scopeId in code
    #[test]
    fn test_no_scoped_styles_no_scope_id() {
        let input = r#"<script setup>
const x = 1
</script>
<template><div>hi</div></template>
<style>.box { color: red; }</style>"#;

        let result = gen_result(input);

        assert!(
            !result.code.contains("__sfc__"),
            "Non-scoped SFC should NOT have __sfc__, got:\n{}",
            result.code
        );
        assert!(
            !result.code.contains("__scopeId"),
            "Non-scoped SFC should NOT have __scopeId, got:\n{}",
            result.code
        );
        assert!(
            result.code.contains("export default "),
            "Non-scoped SFC should use normal export default, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — scope_id field in result is populated for scoped styles
    #[test]
    fn test_scope_id_in_result() {
        let input = r#"<template><div>hi</div></template>
<style scoped>.box { color: red; }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("Scoped.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            !result.scope_id.is_empty(),
            "scope_id should be populated when scoped styles exist"
        );
        assert!(
            result.scope_id.starts_with("data-v-"),
            "scope_id should start with 'data-v-', got: {}",
            result.scope_id
        );
        assert_eq!(
            result.scope_id.len(),
            "data-v-".len() + 8,
            "scope_id should have 8 hex chars after prefix, got: {}",
            result.scope_id
        );
    }

    /// @ai-generated — scope_id is empty when no scoped styles
    #[test]
    fn test_scope_id_empty_without_scoped() {
        let input = r#"<template><div>hi</div></template>
<style>.box { color: red; }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("Plain.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            result.scope_id.is_empty(),
            "scope_id should be empty when no scoped styles, got: {}",
            result.scope_id
        );
    }

    /// @ai-generated — scope_id is deterministic (same input → same output)
    #[test]
    fn test_scope_id_deterministic() {
        let input = r#"<template><div>hi</div></template>
<style scoped>.box { color: red; }</style>"#;

        let allocator1 = Allocator::new();
        let allocator2 = Allocator::new();
        let options = CodegenOptions::new().with_filename("Det.vue");

        let result1 = compile(input, &options, &allocator1);
        let result2 = compile(input, &options, &allocator2);

        assert_eq!(
            result1.scope_id, result2.scope_id,
            "scope_id should be deterministic"
        );
    }

    /// @ai-generated — scope_id matches CSS scoped selectors
    #[test]
    fn test_scope_id_matches_css() {
        let input = r#"<template><div>hi</div></template>
<style scoped>.box { color: red; }</style>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("Match.vue");
        let result = compile(input, &options, &allocator);

        assert!(!result.scope_id.is_empty());
        assert_eq!(result.styles.len(), 1);
        let expected_attr = format!("[{}]", result.scope_id);
        assert!(
            result.styles[0].code.contains(&expected_attr),
            "CSS should contain scope selector.\nscope_id: {}\nCSS: {}",
            result.scope_id,
            result.styles[0].code
        );
    }

    /// @ai-generated — Scoped __sfc__ output is valid JS
    #[test]
    fn test_scoped_styles_valid_js() {
        let input = r#"<script setup>
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
<style scoped>.box { color: red; }</style>"#;

        let result = gen_result(input);

        let js_allocator = Allocator::default();
        let source_type = oxc_span::SourceType::mjs();
        let parser_result =
            oxc_parser::Parser::new(&js_allocator, &result.code, source_type).parse();
        assert!(
            parser_result.errors.is_empty(),
            "Scoped output should be valid JS.\nErrors: {:?}\nCode:\n{}",
            parser_result.errors,
            result.code
        );
    }

    // ==================== E2E error detection ====================

    #[test]
    fn test_compile_missing_end_tag_returns_error() {
        let input = r#"<template><div></template>"#;
        let result = gen_result(input);

        assert!(
            result.code.is_empty(),
            "Fatal error should produce empty code"
        );
        assert!(
            !result.errors.is_empty(),
            "Missing end tag should produce errors"
        );
        let has_missing = result
            .errors
            .iter()
            .any(|e| e.code == "XMissingEndTag" || e.code == "XInvalidEndTag");
        assert!(
            has_missing,
            "Should have XMissingEndTag or XInvalidEndTag error, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_compile_invalid_end_tag_returns_error() {
        let input = r#"<template><div></span></div></template>"#;
        let result = gen_result(input);

        let has_invalid = result.errors.iter().any(|e| e.code == "XInvalidEndTag");
        assert!(
            has_invalid,
            "Mismatched end tag should produce XInvalidEndTag, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_compile_duplicate_v_if_produces_diagnostic() {
        // Two v-if on the same element — should produce a diagnostic
        let input = r#"<template><div v-if="a" v-if="b">text</div></template>"#;
        let result = gen_result(input);

        let has_dup = result
            .errors
            .iter()
            .any(|e| e.message.contains("Duplicate v-if"));
        assert!(
            has_dup,
            "Duplicate v-if should produce a diagnostic, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_compile_duplicate_v_for_produces_diagnostic() {
        let input = r#"<template><div v-for="a in list" v-for="b in list">text</div></template>"#;
        let result = gen_result(input);

        let has_dup = result
            .errors
            .iter()
            .any(|e| e.message.contains("Duplicate v-for"));
        assert!(
            has_dup,
            "Duplicate v-for should produce a diagnostic, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_compile_duplicate_v_else_produces_diagnostic() {
        // v-else twice on the same element — should produce a diagnostic
        let input = r#"<template><div v-if="a">a</div><div v-else v-else>b</div></template>"#;
        let result = gen_result(input);

        let has_dup = result
            .errors
            .iter()
            .any(|e| e.message.contains("Duplicate v-else"));
        assert!(
            has_dup,
            "Duplicate v-else should produce a diagnostic, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_compile_duplicate_v_slot_produces_diagnostic() {
        let input = r#"<template><MyComp v-slot="a" v-slot="b">text</MyComp></template>"#;
        let result = gen_result(input);

        let has_dup = result
            .errors
            .iter()
            .any(|e| e.message.contains("Duplicate v-slot"));
        assert!(
            has_dup,
            "Duplicate v-slot should produce a diagnostic, got: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_compile_valid_directives_no_errors() {
        // Single v-if, v-for, v-slot — no duplicate errors
        let input = r#"<script setup>
import { ref } from 'vue'
const list = ref([1, 2, 3])
const show = ref(true)
</script>
<template>
  <div v-if="show">
    <span v-for="item in list" :key="item">{{ item }}</span>
  </div>
</template>"#;
        let result = gen_result(input);

        let has_dup_error = result.errors.iter().any(|e| {
            e.message.contains("Duplicate v-if")
                || e.message.contains("Duplicate v-for")
                || e.message.contains("Duplicate v-slot")
                || e.message.contains("Duplicate v-else")
        });
        assert!(
            !has_dup_error,
            "Valid directives should not produce duplicate errors, got: {:?}",
            result.errors
        );
    }
}
