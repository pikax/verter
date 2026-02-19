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

/// A custom block extracted from the SFC (e.g., `<i18n>`, `<docs>`).
pub struct CustomBlock {
    /// The tag name (e.g., "i18n", "docs").
    pub block_type: String,
    /// Raw content between open and close tags (empty string for self-closing).
    pub content: String,
    /// Attributes as key-value pairs (e.g., `[("lang", "json")]`).
    /// Boolean attributes (no value) have an empty string value.
    pub attrs: Vec<(String, String)>,
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
    /// Custom blocks (e.g., `<i18n>`, `<docs>`).
    pub custom_blocks: Vec<CustomBlock>,
    /// Scope ID for scoped styles (e.g., `"data-v-a4f2eed6"`).
    /// Empty string when no `<style scoped>` blocks exist.
    pub scope_id: String,
    /// Diagnostics (errors, warnings, info) emitted during compilation.
    pub errors: Vec<CompileDiagnostic>,
    /// Time taken in milliseconds.
    pub duration_ms: f64,
    /// Whether the output contains a standalone `function render()` that must be
    /// attached to the component via `_sfc_main.render = render`.
    /// `false` when the render is inlined inside `setup()` (production mode with
    /// `<script setup>`), or when there is no `<template>` block.
    pub has_render: bool,
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
// Custom block collector plugin
// =============================================================================

/// A lightweight pipeline plugin that collects `CompiledUnknownStart/End` events
/// into `CustomBlock` structs. Events pass through unchanged.
struct CustomBlockCollector {
    /// Pending start event data (tag name + attrs), waiting for the matching End.
    pending: Option<(String, Vec<(String, String)>)>,
    /// Collected custom blocks.
    blocks: Vec<CustomBlock>,
}

impl CustomBlockCollector {
    fn new() -> Self {
        Self {
            pending: None,
            blocks: Vec::new(),
        }
    }

    fn take_blocks(&mut self) -> Vec<CustomBlock> {
        std::mem::take(&mut self.blocks)
    }

    /// Extract attribute name and value from a `Prop` using source byte offsets.
    /// The value span already excludes quotes (pipeline handles this).
    fn extract_attr(prop: &Prop, input: &str) -> (String, String) {
        let name = &input[prop.start as usize..prop.name_end as usize];
        let value = prop
            .value
            .as_ref()
            .map(|span| &input[span.start as usize..span.end as usize])
            .unwrap_or("");
        (name.to_string(), value.to_string())
    }
}

impl<'a> SyntaxPlugin<'a> for CustomBlockCollector {
    fn name(&self) -> &str {
        "custom-block-collector"
    }

    fn process_event(
        &mut self,
        event: Event<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<Event<'a>> {
        match &event {
            Event::CompiledUnknownStart(start) => {
                // Extract tag name: source between '<' and name_end
                let tag_name =
                    &ctx.input[(start.tag_open_event.start + 1) as usize..start.name_end as usize];
                let attrs: Vec<(String, String)> = start
                    .attributes
                    .iter()
                    .map(|prop| Self::extract_attr(prop, ctx.input))
                    .collect();

                if start.tag_open_end_event.is_self_closing {
                    // Self-closing: no CompiledUnknownEnd will follow
                    self.blocks.push(CustomBlock {
                        block_type: tag_name.to_string(),
                        content: String::new(),
                        attrs,
                    });
                } else {
                    self.pending = Some((tag_name.to_string(), attrs));
                }
            }
            Event::CompiledUnknownEnd(end) => {
                if let Some((block_type, attrs)) = self.pending.take() {
                    let content = end
                        .content
                        .as_ref()
                        .map(|span| &ctx.input[span.start as usize..span.end as usize])
                        .unwrap_or("");
                    self.blocks.push(CustomBlock {
                        block_type,
                        content: content.to_string(),
                        attrs,
                    });
                }
            }
            _ => {}
        }
        SyntaxResult::Keep(event)
    }
}

// =============================================================================
// Helpers
// =============================================================================

/// Extract root-level SFC block byte ranges from parsed pipeline events.
///
/// Returns a sorted, non-overlapping vec of `(start, end)` byte offsets for
/// each top-level block (`<script>`, `<template>`, `<style>`, custom blocks).
/// Any source content outside these ranges is inter-block "gap" content
/// (whitespace, HTML comments, stray text) that should be removed from JS output.
///
/// This uses events produced by the tokenizer (which correctly handles RCDATA
/// mode for `<script>`/`<style>` blocks), avoiding the need to re-scan raw bytes
/// and the risk of confusing string literal content with SFC block boundaries.
fn extract_sfc_block_ranges(events: &[Event]) -> Vec<(u32, u32)> {
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    // Stack of open block starts: (kind, start_pos)
    let mut open_stack: Vec<(RootNodeKind, u32)> = Vec::new();

    for event in events {
        match event {
            Event::RootOpenTagEnd(e) => {
                if e.is_self_closing {
                    ranges.push((e.start, e.end));
                } else {
                    open_stack.push((e.kind.clone(), e.start));
                }
            }
            Event::RootCloseTag(e) => {
                if let Some((_kind, start)) = open_stack.pop() {
                    ranges.push((start, e.end));
                }
            }
            _ => {}
        }
    }

    ranges.sort_by_key(|&(s, _)| s);
    ranges
}

/// Remove inter-block gaps from the code transform.
///
/// SFC source may contain content between root-level blocks (e.g., HTML comments,
/// whitespace). This content is not valid JS and must be blanked out.
pub(crate) fn remove_inter_block_gaps(
    code_transform: &mut crate::code_transform::CodeTransform,
    input_len: u32,
    ranges: &[(u32, u32)],
) {
    if ranges.is_empty() {
        return;
    }

    // Remove gap before first block
    if ranges[0].0 > 0 {
        code_transform.remove(0, ranges[0].0);
    }

    // Remove gaps between consecutive blocks
    for i in 0..ranges.len() - 1 {
        let gap_start = ranges[i].1;
        let gap_end = ranges[i + 1].0;
        if gap_start < gap_end {
            code_transform.remove(gap_start, gap_end);
        }
    }

    // Remove gap after last block
    let last_end = ranges[ranges.len() - 1].1;
    if last_end < input_len {
        code_transform.remove(last_end, input_len);
    }
}

/// SHA-256 hash → 8 hex chars (first 4 bytes).
pub(crate) fn get_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..4])
}

/// Extract component name from a filename.
pub(crate) fn extract_component_name(filename: &str) -> String {
    let name = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    let name = name.strip_suffix(".vue").unwrap_or(name);
    let name = name.strip_suffix(".ts").unwrap_or(name);
    let name = name.strip_suffix(".js").unwrap_or(name);
    name.to_string()
}

/// Compute scope_id as 8 hex chars from component name.
pub(crate) fn compute_scope_id(component_name: &str) -> [u8; 8] {
    let hash = get_hash(component_name);
    let hash_bytes = hash.as_bytes();
    let mut scope_id = [0u8; 8];
    scope_id.copy_from_slice(&hash_bytes[..8.min(hash_bytes.len())]);
    scope_id
}

/// Convert plugin diagnostics to public `CompileDiagnostic` structs.
pub(crate) fn convert_diagnostics(
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
            custom_blocks: Vec::new(),
            scope_id: String::new(),
            errors: convert_diagnostics(&ctx.diagnostics),
            duration_ms,
            has_render: false,
        };
    }

    // Read SFC metadata from Syntax (tracked during tokenization).
    let script_setup_pos = syntax.script_setup_start().map(|s| s as usize);
    let has_script_setup = script_setup_pos.is_some();
    let has_template = syntax.has_template();
    let has_vapor_template = syntax.has_vapor_template();
    let template_block = syntax.template_block();
    let script_block_end = syntax.script_block_end();

    let events = syntax.events();

    // Extract SFC block byte ranges from pipeline events (tokenizer-derived, RCDATA-aware).
    let block_ranges = extract_sfc_block_ranges(&events);

    let effective_inline = options.resolve_inline() && has_script_setup;

    // When <template> appears before <script setup> and inline mode is active,
    // the in-place CodeTransform would emit `return (_ctx,_cache) => {` at the
    // template position (before the setup() wrapper). We detect this and later
    // use move_slice to relocate the template block inside setup().
    let template_before_script_range = if effective_inline {
        match (template_block, script_setup_pos, script_block_end) {
            (Some((tpl_start, tpl_end)), Some(s), Some(se)) if (tpl_start as usize) < s => {
                Some((tpl_start, tpl_end, se))
            }
            _ => None,
        }
    } else {
        None
    };

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
    // Compute PascalCase self_name for recursive self-reference detection.
    // Vue uses `capitalize(camelize(filename_without_ext))` which is equivalent
    // to camelize_capitalize_into.
    let self_name = {
        use crate::syntax::plugins::code_gen::template::shared::helper::camelize_capitalize_into;
        let mut buf = String::with_capacity(component_name.len());
        camelize_capitalize_into(&component_name, &mut buf);
        buf
    };
    let template_options = TemplateOptions {
        is_production: options.is_production,
        inline: effective_inline,
        comments: options.resolve_comments(),
        hoist_static: options.resolve_hoist_static(),
        cache_handlers: options.resolve_cache_handlers(),
        runtime_module_name: options.resolve_runtime_module_name().to_string(),
        prefix_identifiers: options.resolve_prefix_identifiers(),
        self_name,
    };
    let mut code_gen_template =
        TemplateGeneratorPlugin::with_options(Rc::clone(&code_transform), template_options);

    let mut custom_block_collector = CustomBlockCollector::new();

    {
        // transient plugins
        let mut script_ec = ElementCompilerPlugin::new();
        let mut script_oxc = OxcParserPlugin::new(allocator);

        // Pipeline: parsers first, code_gen last (code_gen order independent).
        // custom_block_collector sits after element_compiler to capture
        // CompiledUnknownStart/End events before they're dropped by codegen plugins.
        let pipeline: &mut [&mut dyn SyntaxPlugin] = &mut [
            &mut script_ec,
            &mut custom_block_collector,
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

    // When <template> appears before <script setup> in inline mode, the template
    // content (return (_ctx,_cache) => { ... }) was emitted at the template's source
    // position, which precedes the setup() wrapper. Move the entire template block
    // to after </script> (which was replaced with "\n", leaving setup() open).
    // This places the render function inside setup() where it belongs.
    if let Some((tpl_start, tpl_end, script_close_end)) = template_before_script_range {
        code_transform
            .borrow_mut()
            .move_slice(tpl_start, tpl_end, script_close_end);
    }

    // Remove inter-block gaps: HTML comments, whitespace, and stray text between
    // root-level SFC blocks (e.g., between </script> and <template>). These are
    // not valid JS and must not leak into the output.
    remove_inter_block_gaps(
        &mut code_transform.borrow_mut(),
        input.len() as u32,
        &block_ranges,
    );

    let code = code_transform.borrow().build_string();
    let styles = code_gen_css.take_styles();

    // Scoped styles: __sfc__.__scopeId and export default __sfc__ are now emitted
    // by ScriptGeneratorPlugin::end() using AST-level position info, avoiding fragile
    // string-based detection of "export default". The scope_id_string is still computed
    // for the CodegenResult metadata.
    let has_scoped = styles.iter().any(|s| s.scoped);
    let scope_id_string = if has_scoped {
        let hex = std::str::from_utf8(&scope_id).unwrap_or("00000000");
        format!("data-v-{}", hex)
    } else {
        String::new()
    };

    let (source_map, code_with_source_map) = if options.skip_source_map {
        (String::new(), String::new())
    } else {
        // Generate source map
        let source_name = options
            .filename
            .clone()
            .unwrap_or_else(|| "input.vue".to_string());
        let file_name = options
            .filename
            .as_ref()
            .map(|f| format!("{}.js", f))
            .unwrap_or_else(|| "output.js".to_string());
        let source_map_options = SourceMapOptions::new()
            .with_source(&source_name)
            .with_file(&file_name);

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

    // A standalone `function render()` exists when:
    // - There is a template AND the render was NOT inlined (VDOM dev / Options API), OR
    // - There is a vapor template (vapor always emits standalone render)
    let has_render = has_template && (!effective_inline || has_vapor_template);

    let custom_blocks = custom_block_collector.take_blocks();

    CodegenResult {
        code,
        source_map,
        code_with_source_map,
        styles,
        custom_blocks,
        scope_id: scope_id_string,
        errors,
        duration_ms,
        has_render,
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
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    fn gen_result(input: &str) -> CodegenResult {
        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        compile(input, &options, &allocator)
    }

    fn assert_valid_js(code: &str, context: &str) {
        let allocator = Allocator::default();
        let source_type = SourceType::mjs();
        let parser_result = Parser::new(&allocator, code, source_type).parse();
        assert!(
            parser_result.errors.is_empty(),
            "Generated code is NOT valid JavaScript!\n\
             Context: {}\n\
             Parse Errors: {:?}\n\
             Generated Code:\n{}",
            context,
            parser_result.errors,
            code
        );
    }

    fn gen_and_validate(input: &str) -> CodegenResult {
        let result = gen_result(input);
        assert_valid_js(&result.code, input);
        result
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

    /// @ai-generated — Scoped LESS/SCSS/Sass/Stylus style should NOT be processed
    /// by lightningcss. Non-CSS langs are preprocessed by Vite BEFORE calling
    /// processStyle(). The inline Rust pipeline should pass them through raw.
    #[test]
    fn test_pipeline_scoped_less_no_css_error() {
        let input = "<template><div>hi</div></template>\n\
<style scoped lang=\"less\">\n\
#AllMessage {\n\
  position: fixed;\n\
  font-size: 14rem;\n\
\n\
  .center {\n\
    display: flex;\n\
\n\
    img {\n\
      width: 15rem;\n\
      transform: rotate(180deg);\n\
    }\n\
  }\n\
\n\
  .type-dialog {\n\
    z-index: 9;\n\
    position: fixed;\n\
    height: calc(var(--vh, 1vh) * 100);\n\
\n\
    .dialog-content {\n\
      border-radius: 0 0 4rem 4rem;\n\
\n\
      img {\n\
        width: 18rem;\n\
      }\n\
    }\n\
\n\
    .mask {\n\
      height: calc(var(--vh, 1vh) * 100);\n\
    }\n\
  }\n\
\n\
  .messages {\n\
    .message {\n\
      display: flex;\n\
\n\
      &:first-child {\n\
        margin-top: 20rem;\n\
      }\n\
\n\
      .left {\n\
        .avatar {\n\
          width: 58rem;\n\
          border-radius: 50%;\n\
        }\n\
      }\n\
    }\n\
  }\n\
}\n\
</style>";

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("Less.vue");

        let result = compile(input, &options, &allocator);

        assert_eq!(result.styles.len(), 1, "Should have one style block");
        assert!(result.styles[0].scoped, "Style should be scoped");
        assert!(
            result.styles[0].errors.is_empty(),
            "LESS style should NOT produce CSS parse errors, got: {:?}",
            result.styles[0].errors
        );
    }

    /// @ai-generated — Scoped SCSS also should not be processed by lightningcss
    #[test]
    fn test_pipeline_scoped_scss_no_css_error() {
        let input = "<template><div>hi</div></template>\n\
<style scoped lang=\"scss\">\n\
$primary: red;\n\
.box {\n\
  color: $primary;\n\
  &__inner {\n\
    font-size: 14px;\n\
  }\n\
}\n\
</style>";

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("Scss.vue");

        let result = compile(input, &options, &allocator);

        assert_eq!(result.styles.len(), 1, "Should have one style block");
        assert!(result.styles[0].scoped, "Style should be scoped");
        assert!(
            result.styles[0].errors.is_empty(),
            "SCSS style should NOT produce CSS parse errors, got: {:?}",
            result.styles[0].errors
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

    /// @ai-generated — Non-scoped styles: __sfc__ pattern used but no __scopeId
    #[test]
    fn test_no_scoped_styles_no_scope_id() {
        let input = r#"<script setup>
const x = 1
</script>
<template><div>hi</div></template>
<style>.box { color: red; }</style>"#;

        let result = gen_result(input);

        // All SFCs now use the __sfc__ pattern (AST-based export default)
        assert!(
            result.code.contains("const __sfc__ ="),
            "Should use const __sfc__ pattern, got:\n{}",
            result.code
        );
        assert!(
            !result.code.contains("__scopeId"),
            "Non-scoped SFC should NOT have __scopeId, got:\n{}",
            result.code
        );
        assert!(
            result.code.contains("export default __sfc__"),
            "Non-scoped SFC should export __sfc__, got:\n{}",
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

    /// @ai-generated - v-for with computed member expression iterable should not panic.
    ///
    /// Regression test: `collections[activeIndex].data` has two references
    /// (`collections` and `activeIndex`) that may come out of the FxHashSet in
    /// arbitrary order. `prefix_vfor_references_into` assumed ascending order,
    /// causing a slice panic when `activeIndex` (higher offset) was iterated
    /// before `collections` (lower offset).
    #[test]
    fn test_vfor_computed_member_expression_does_not_panic() {
        let input = r#"<script setup>
import { ref, computed } from 'vue'
const activeIndex = ref(0)
const collections = computed(() => [
  { id: 'a', data: [1, 2] },
  { id: 'b', data: [3, 4] },
])
</script>
<template>
  <div v-for="item in collections[activeIndex].data" :key="item">
    {{ item }}
  </div>
</template>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            result.errors.is_empty(),
            "Should compile without errors, got: {:?}",
            result.errors
        );
        assert!(!result.code.is_empty(), "Should produce output code");
        assert!(
            result.code.contains("_renderList"),
            "Should contain _renderList for v-for, got: {}",
            result.code
        );
    }

    /// @ai-generated - Directive with shorthand object properties should not panic.
    ///
    /// Regression test: `v-tooltip="{ content, offset }"` with shorthand props
    /// causes an overflow in `build_prefixed_value_into` when `b.span.start < val_start`.
    #[test]
    fn test_directive_shorthand_object_props_does_not_panic() {
        let input = r#"<script setup>
import { ref } from 'vue'
const content = ref('hello')
const offset = ref(10)
</script>
<template>
  <div v-tooltip="{ content, offset, placement: 'right' }">hover</div>
</template>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            result.errors.is_empty(),
            "Should compile without errors, got: {:?}",
            result.errors
        );
        assert!(!result.code.is_empty(), "Should produce output code");
    }

    /// @ai-generated - Options API component with directive using shorthand object properties.
    ///
    /// Regression test: Options API (defineComponent) with directive value like
    /// `v-tooltip="{ content, offset }"` where content/offset come from props.
    #[test]
    fn test_options_api_directive_shorthand_does_not_panic() {
        let input = r#"<script lang="ts">
import { defineComponent } from 'vue'
export default defineComponent({
  props: {
    content: { type: String, required: true },
    offset: { type: Array, default: () => [0, 15] },
  },
})
</script>
<template>
  <div v-tooltip="{ content, offset, placement: 'right' }">hover</div>
</template>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            result.errors.is_empty(),
            "Should compile without errors, got: {:?}",
            result.errors
        );
        assert!(!result.code.is_empty(), "Should produce output code");
    }

    /// @ai-generated - Vue SFC with custom `<docs>` block containing multi-byte UTF-8
    /// and shorthand object properties in the template.
    ///
    /// Regression test: custom blocks with Chinese characters (3-byte UTF-8) before
    /// `<template>` combined with shorthand object properties could cause a char
    /// boundary panic in `CodeTransform::build_string()` because byte positions
    /// land in the middle of multi-byte characters.
    #[test]
    fn test_custom_block_with_multibyte_utf8_does_not_panic() {
        // Matches 7109_clickable.vue from ant-design-vue
        let input = r#"<docs>
---
order: 9
title:
  zh-CN: 可点击
  en-US: Clickable
---

## zh-CN

设置 `v-model` 后，Steps 变为可点击状态。

## en-US

Setting `v-model` makes Steps clickable.
</docs>

<template>
  <div>
    <a-steps
      v-model:current="current"
      :items="[
        {
          title: 'Step 1',
          description,
        },
        {
          title: 'Step 2',
          description,
        },
        {
          title: 'Step 3',
          description,
        },
      ]"
    ></a-steps>
  </div>
</template>
<script lang="ts" setup>
import { ref } from 'vue';
const current = ref<number>(0);
const description = 'This is a description.';
</script>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            result.errors.is_empty(),
            "Should compile without errors, got: {:?}",
            result.errors
        );
        assert!(!result.code.is_empty(), "Should produce output code");
    }

    /// @ai-generated - Vue SFC with `<docs>` block, scoped slots, and multi-byte UTF-8.
    ///
    /// Regression test: file from ant-design-vue (tree-transfer.vue) with Chinese
    /// chars in `<docs>` block, scoped slots with destructuring, and spread operators.
    #[test]
    fn test_docs_block_with_scoped_slots_does_not_panic() {
        let input = r#"<docs>
---
order: 7
title:
  zh-CN: 树穿梭框
  en-US: Tree Transfer
---

## zh-CN

使用 Tree 组件作为自定义渲染列表。

## en-US

Customize render list with Tree component.

</docs>

<template>
  <div>
    <a-transfer
      v-model:target-keys="targetKeys"
      class="tree-transfer"
      :data-source="dataSource"
      :render="item => item.title"
      :show-select-all="false"
    >
      <template #children="{ direction, selectedKeys, onItemSelect }">
        <a-tree
          v-if="direction === 'left'"
          block-node
          checkable
          check-strictly
          default-expand-all
          :checked-keys="[...selectedKeys, ...targetKeys]"
          :tree-data="treeData"
          @check="
            (_, props) => {
              onChecked(props, [...selectedKeys, ...targetKeys], onItemSelect);
            }
          "
        />
      </template>
    </a-transfer>
  </div>
</template>
<script lang="ts" setup>
import { computed, ref } from 'vue';
const targetKeys = ref<string[]>([]);
const dataSource = ref([]);
const treeData = computed(() => []);
const onChecked = (e: any, checkedKeys: string[], onItemSelect: (n: any, c: boolean) => void) => {};
</script>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            result.errors.is_empty(),
            "Should compile without errors, got: {:?}",
            result.errors
        );
        assert!(!result.code.is_empty(), "Should produce output code");
    }

    /// @ai-generated - Verify that custom blocks are extracted with correct
    /// block_type, content, and attributes.
    #[test]
    fn test_custom_blocks_extracted() {
        let input = r#"<i18n lang="json" locale="en">
{"hello": "world"}
</i18n>

<template>
  <div>{{ $t('hello') }}</div>
</template>

<script setup>
</script>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            result.errors.is_empty(),
            "Should compile without errors, got: {:?}",
            result.errors
        );
        assert_eq!(
            result.custom_blocks.len(),
            1,
            "Should have one custom block"
        );

        let block = &result.custom_blocks[0];
        assert_eq!(block.block_type, "i18n");
        assert_eq!(block.content, "\n{\"hello\": \"world\"}\n");

        // Check attributes
        assert_eq!(block.attrs.len(), 2);
        assert!(
            block
                .attrs
                .contains(&("lang".to_string(), "json".to_string())),
            "Should have lang=json attribute, got: {:?}",
            block.attrs
        );
        assert!(
            block
                .attrs
                .contains(&("locale".to_string(), "en".to_string())),
            "Should have locale=en attribute, got: {:?}",
            block.attrs
        );
    }

    /// @ai-generated - Multiple custom blocks of different types are all extracted.
    #[test]
    fn test_multiple_custom_blocks() {
        let input = r#"<i18n lang="json">
{"hello": "world"}
</i18n>

<docs>
# My Component
</docs>

<template>
  <div>hello</div>
</template>

<script setup>
</script>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            result.errors.is_empty(),
            "Should compile without errors, got: {:?}",
            result.errors
        );
        assert_eq!(
            result.custom_blocks.len(),
            2,
            "Should have two custom blocks"
        );

        assert_eq!(result.custom_blocks[0].block_type, "i18n");
        assert_eq!(result.custom_blocks[1].block_type, "docs");
        assert!(result.custom_blocks[1].content.contains("# My Component"));
    }

    /// @ai-generated - Self-closing custom blocks have empty content.
    #[test]
    fn test_self_closing_custom_block() {
        let input = r#"<i18n src="./en.json" />

<template>
  <div>hello</div>
</template>

<script setup>
</script>"#;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        let result = compile(input, &options, &allocator);

        assert!(
            result.errors.is_empty(),
            "Should compile without errors, got: {:?}",
            result.errors
        );
        assert_eq!(
            result.custom_blocks.len(),
            1,
            "Should have one custom block"
        );

        let block = &result.custom_blocks[0];
        assert_eq!(block.block_type, "i18n");
        assert_eq!(block.content, "");
        assert!(
            block
                .attrs
                .contains(&("src".to_string(), "./en.json".to_string())),
            "Should have src attribute, got: {:?}",
            block.attrs
        );
    }

    // ==================== regression: script tags in string literals ====================

    /// @ai-generated — Regression test: `<script` inside a template literal in
    /// `<script setup>` must NOT confuse the block range detection.
    /// The event-based `extract_sfc_block_ranges` must not treat `<script` inside
    /// JS strings as SFC block boundaries.
    #[test]
    fn test_script_tag_inside_template_literal() {
        let input = r#"<script setup lang="ts">
import srcdocTemplate from "./srcdoc.html?raw";

const srcdoc = computed(() => {
  const importMapScript = `<script type="importmap">${JSON.stringify(props.store.importMap)}<\/script>`;
  return srcdocTemplate.replace("</head>", `${importMapScript}\n  </head>`);
});
</script>
<template><div>hello</div></template>"#;

        let result = gen_result(input);
        assert!(
            result.errors.is_empty(),
            "Script tag inside template literal should not cause errors, got: {:?}",
            result.errors
        );
        assert!(
            result.code.contains("importMapScript"),
            "Output should preserve the importMapScript variable, got: {}",
            result.code
        );
    }

    // ==================== hyphenated slot names ====================

    /// @ai-generated — Slot names containing hyphens must be quoted as JS object keys.
    /// `pool-summary` is not a valid bare JS identifier (interpreted as `pool - summary`),
    /// so it must be emitted as `"pool-summary"`.
    #[test]
    fn test_hyphenated_slot_name() {
        let input = r#"<script setup>
import Comp from './Comp.vue'
import Child from './Child.vue'
</script>
<template>
  <Comp>
    <template #pool-summary>
      <Child />
    </template>
  </Comp>
</template>"#;
        let result = gen_and_validate(input);
        assert!(
            result.code.contains("\"pool-summary\""),
            "Slot name with hyphen should be quoted: {}",
            result.code
        );
    }

    /// @ai-generated — Slot names containing colons must be quoted as JS object keys.
    #[test]
    fn test_colon_slot_name() {
        let input = r#"<script setup>
import Comp from './Comp.vue'
import Child from './Child.vue'
</script>
<template>
  <Comp>
    <template #slot:name>
      <Child />
    </template>
  </Comp>
</template>"#;
        let result = gen_and_validate(input);
        assert!(
            result.code.contains("\"slot:name\""),
            "Slot name with colon should be quoted: {}",
            result.code
        );
    }

    /// @ai-generated — Simple slot names (valid JS identifiers) should NOT be quoted.
    #[test]
    fn test_simple_slot_name_not_quoted() {
        let input = r#"<script setup>
import Comp from './Comp.vue'
import Child from './Child.vue'
</script>
<template>
  <Comp>
    <template #header>
      <Child />
    </template>
  </Comp>
</template>"#;
        let result = gen_and_validate(input);
        assert!(
            result.code.contains("header: _withCtx"),
            "Simple slot name should not be quoted: {}",
            result.code
        );
        assert!(
            !result.code.contains("\"header\""),
            "Simple slot name should not have quotes: {}",
            result.code
        );
    }

    /// @ai-generated — Named slot template inside a component (parent-level slot_params path).
    /// Hyphenated slot names on component children must also be quoted.
    #[test]
    fn test_hyphenated_slot_name_with_slot_params() {
        let input = r#"<script setup>
import Comp from './Comp.vue'
</script>
<template>
  <Comp>
    <template #pool-summary="{ data }">
      {{ data }}
    </template>
  </Comp>
</template>"#;
        let result = gen_and_validate(input);
        assert!(
            result.code.contains("\"pool-summary\""),
            "Slot name with hyphen and params should be quoted: {}",
            result.code
        );
    }

    // ==================== v-once ====================

    #[test]
    fn test_v_once_with_v_if_self_closing_component() {
        // v-once + v-if on a self-closing component.
        // The _createCommentVNode must be INSIDE the ternary (the else branch),
        // not dangling after the cache block close.
        //
        // Vue official compiler output:
        //   _cache[0] || (
        //     _setBlockTracking(-1, true),
        //     (_cache[0] = (show.value)
        //       ? (_openBlock(), _createBlock(Comp, { key: 0 }))
        //       : _createCommentVNode("v-if", true)).cacheIndex = 0,
        //     _setBlockTracking(1),
        //     _cache[0]
        //   )
        let input = r#"<script setup>
import Comp from './Comp.vue'
const show = true
</script>
<template>
  <div>
    <Comp v-if="show" v-once />
    <div>other content</div>
  </div>
</template>"#;
        let result = gen_and_validate(input);
        // The ternary else must NOT be empty (`: )` pattern)
        assert!(
            !result.code.contains(": )"),
            "ternary else should not be empty — _createCommentVNode must be inside the ternary: {}",
            result.code
        );
        // The comment node must appear inside the cache block
        assert!(
            result.code.contains("_createCommentVNode"),
            "should have _createCommentVNode for v-if else branch: {}",
            result.code
        );
        // Verify the comment node is BEFORE the cache close, not after
        let cache_close = ").cacheIndex = ";
        let comment = "_createCommentVNode(";
        if let (Some(comment_pos), Some(cache_pos)) =
            (result.code.find(comment), result.code.find(cache_close))
        {
            assert!(
                comment_pos < cache_pos,
                "_createCommentVNode (at {}) must appear before .cacheIndex (at {}) in: {}",
                comment_pos,
                cache_pos,
                result.code
            );
        }
    }

    #[test]
    fn test_v_once_with_v_if_non_self_closing_element() {
        // v-once + v-if on a non-self-closing element.
        // Same requirement: _createCommentVNode must be inside the ternary.
        let input = r#"<script setup>
const show = true
</script>
<template>
  <div>
    <span v-if="show" v-once>text</span>
    <div>other content</div>
  </div>
</template>"#;
        let result = gen_and_validate(input);
        assert!(
            !result.code.contains(": )"),
            "ternary else should not be empty: {}",
            result.code
        );
        assert!(
            result.code.contains("_createCommentVNode"),
            "should have _createCommentVNode: {}",
            result.code
        );
        let cache_close = ").cacheIndex = ";
        let comment = "_createCommentVNode(";
        if let (Some(comment_pos), Some(cache_pos)) =
            (result.code.find(comment), result.code.find(cache_close))
        {
            assert!(
                comment_pos < cache_pos,
                "_createCommentVNode (at {}) must appear before .cacheIndex (at {}) in: {}",
                comment_pos,
                cache_pos,
                result.code
            );
        }
    }

    #[test]
    fn test_ref_with_class_and_vbind_spread() {
        // ref="xxx" matching a setup binding + :class + v-bind="$attrs" + events.
        // In inline mode, ref matching a binding emits ref_key + ref (variable reference).
        // When _mergeProps is used, the ref prop must be followed by a comma before
        // subsequent props like class.
        let input = r#"<script setup>
import { ref } from 'vue'
const activator = ref()
const handleMouseEnter = () => {}
const handleMouseLeave = () => {}
</script>
<template>
  <button
    ref="activator"
    :class="['leading-none', { 'cursor-default': false }]"
    v-bind="$attrs"
    @mouseenter="handleMouseEnter"
    @mouseleave="handleMouseLeave"
  >
    content
  </button>
</template>"#;
        let allocator = Allocator::new();
        let options = CodegenOptions {
            inline: Some(true),
            ..CodegenOptions::new().with_filename("test.vue")
        };
        let result = compile(input, &options, &allocator);
        assert_valid_js(&result.code, input);
        // Must NOT have "activatorclass" — that means a comma is missing between
        // `ref: activator` and `class: _normalizeClass(...)`.
        assert!(
            !result.code.contains("activatorclass"),
            "Missing comma between ref and class props: {}",
            result.code
        );
        // Verify the correct pattern: `ref: activator, class:`
        assert!(
            result.code.contains("ref: activator, class:"),
            "Should have proper comma separation between ref and class: {}",
            result.code
        );
    }

    // ==================== shorthand property expansion ====================

    fn gen_result_prod(input: &str) -> CodegenResult {
        let allocator = Allocator::new();
        let options = CodegenOptions {
            is_production: true,
            ..CodegenOptions::new().with_filename("test.vue")
        };
        compile(input, &options, &allocator)
    }

    fn gen_and_validate_prod(input: &str) -> CodegenResult {
        let result = gen_result_prod(input);
        assert_valid_js(&result.code, input);
        result
    }

    /// @ai-generated — Shorthand property with .value suffix must expand to key: value form
    /// (production/inline mode where SetupRef gets .value suffix)
    #[test]
    fn test_shorthand_property_with_ref_value_prod() {
        let input = r#"<script setup>
import { computed } from 'vue'
import Comp from './Comp.vue'
const subsetTokens = computed(() => ['a', 'b'])
</script>
<template>
  <Comp :tokenSelectProps="{ ignoreBalances: true, subsetTokens }" />
</template>"#;
        let result = gen_and_validate_prod(input);
        // In production (inline) mode, computed refs get .value suffix
        // Shorthand must be expanded to key: value form
        assert!(
            result.code.contains("subsetTokens: subsetTokens.value"),
            "Shorthand with .value must be expanded to key: value form: {}",
            result.code
        );
    }

    /// @ai-generated — Shorthand property with _ctx. prefix must expand to key: value form
    #[test]
    fn test_shorthand_property_with_ctx_prefix() {
        let input = r#"<script setup>
import Comp from './Comp.vue'
</script>
<template>
  <Comp :data="{ someFlag: true, unknownVar }" />
</template>"#;
        let result = gen_and_validate(input);
        // unknownVar is not declared in script setup, so it should get _ctx. prefix
        // and shorthand must be expanded
        assert!(
            result.code.contains("unknownVar: _ctx.unknownVar"),
            "Shorthand with _ctx. prefix must be expanded to key: value form: {}",
            result.code
        );
    }

    /// @ai-generated — Shorthand property with ref() in production mode also needs expansion
    #[test]
    fn test_shorthand_property_with_ref_prod() {
        let input = r#"<script setup>
import { ref } from 'vue'
import Comp from './Comp.vue'
const count = ref(0)
</script>
<template>
  <Comp :data="{ count }" />
</template>"#;
        let result = gen_and_validate_prod(input);
        // count is a ref, should get .value suffix and shorthand must be expanded
        assert!(
            result.code.contains("count: count.value"),
            "Shorthand ref must be expanded to key: value form: {}",
            result.code
        );
    }

    /// @ai-generated — v-show with v-bind spread must not leave empty leading prop in _mergeProps
    #[test]
    fn test_vshow_with_vbind_spread() {
        let input = r#"<script setup>
import { ref } from 'vue'
const loaded = ref(false)
const onLoaded = () => { loaded.value = true }
</script>
<template>
  <div>
    <img v-show="loaded" :width="'auto'" :height="'auto'" v-bind="$attrs" @load="onLoaded" />
  </div>
</template>"#;
        let result = gen_and_validate(input);
        assert!(
            !result.code.contains("{,"),
            "Should not have empty leading prop in _mergeProps object: {}",
            result.code
        );
        // Vue official compiler groups non-spread props before and after v-bind="$attrs":
        // _mergeProps({ width: ..., height: ... }, _ctx.$attrs, { onLoad: ... })
        assert!(
            result.code.contains("{width:"),
            "First _mergeProps group should start with width (no empty slot): {}",
            result.code
        );
    }

    /// @ai-generated — Shorthand property with $setup. prefix (non-inline mode) must expand
    #[test]
    fn test_shorthand_property_with_setup_prefix() {
        let input = r#"<script setup>
import { computed } from 'vue'
import Comp from './Comp.vue'
const subsetTokens = computed(() => ['a', 'b'])
</script>
<template>
  <Comp :data="{ ignoreBalances: true, subsetTokens }" />
</template>"#;
        let result = gen_and_validate(input);
        // In non-inline mode, setup refs get $setup. prefix
        assert!(
            result.code.contains("subsetTokens: $setup.subsetTokens"),
            "Shorthand with $setup. prefix must be expanded to key: value form: {}",
            result.code
        );
    }

    // ==================== recursive self-referencing components ====================

    fn gen_result_with_filename(input: &str, filename: &str) -> CodegenResult {
        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename(filename);
        compile(input, &options, &allocator)
    }

    fn gen_and_validate_with_filename(input: &str, filename: &str) -> CodegenResult {
        let result = gen_result_with_filename(input, filename);
        assert_valid_js(&result.code, input);
        result
    }

    /// @ai-generated — Recursive component self-reference should use _resolveComponent with true flag
    #[test]
    fn test_recursive_component_self_reference() {
        let input = r#"<script setup>
const props = defineProps(['level'])
</script>
<template>
  <div>Level {{ level }}</div>
  <TokenBreakdown v-if="level < 3" :level="level + 1" />
</template>"#;
        let result = gen_and_validate_with_filename(input, "TokenBreakdown.vue");
        // Should use _resolveComponent("TokenBreakdown", true) — the true flag signals
        // a possible self-reference to the Vue runtime.
        assert!(
            result
                .code
                .contains("_resolveComponent(\"TokenBreakdown\", true)"),
            "Recursive component should use _resolveComponent with true flag: {}",
            result.code
        );
        // Should NOT have _resolveComponent("TokenBreakdown") without the true flag
        assert!(
            !result
                .code
                .contains("_resolveComponent(\"TokenBreakdown\")"),
            "Recursive component should NOT use _resolveComponent without true flag: {}",
            result.code
        );
    }

    /// @ai-generated — Non-self-referencing component should NOT have the true flag
    #[test]
    fn test_non_recursive_component_no_self_flag() {
        let input = r#"<script setup>
const x = 1
</script>
<template>
  <SomeOtherComponent />
</template>"#;
        let result = gen_and_validate_with_filename(input, "MyComponent.vue");
        // Non-self-referencing component should use _resolveComponent without true flag
        assert!(
            result
                .code
                .contains("_resolveComponent(\"SomeOtherComponent\")"),
            "Non-recursive component should use plain _resolveComponent: {}",
            result.code
        );
        assert!(
            !result.code.contains(", true)"),
            "Non-recursive component should NOT have true flag: {}",
            result.code
        );
    }

    /// @ai-generated — Recursive component with kebab-case tag name
    #[test]
    fn test_recursive_component_kebab_case() {
        let input = r#"<script setup>
const props = defineProps(['level'])
</script>
<template>
  <div>Level {{ level }}</div>
  <my-tree v-if="level < 3" :level="level + 1" />
</template>"#;
        let result = gen_and_validate_with_filename(input, "MyTree.vue");
        // kebab-case <my-tree> should match PascalCase filename MyTree.vue
        assert!(
            result.code.contains("_resolveComponent(\"my-tree\", true)"),
            "Kebab-case recursive component should have true flag: {}",
            result.code
        );
    }

    /// @ai-generated — Imported component with same name as SFC should use setup binding, not self-ref
    #[test]
    fn test_imported_component_overrides_self_reference() {
        let input = r#"<script setup>
import TokenBreakdown from './other/TokenBreakdown.vue'
</script>
<template>
  <TokenBreakdown />
</template>"#;
        let result = gen_and_validate_with_filename(input, "TokenBreakdown.vue");
        // When the component IS in setup bindings (imported), it should use $setup["TokenBreakdown"]
        // rather than _resolveComponent, regardless of the self-name match.
        assert!(
            !result.code.contains("_resolveComponent"),
            "Imported component should NOT use _resolveComponent: {}",
            result.code
        );
    }

    // ==================== defineProps / withDefaults variable assignment ====================

    /// @ai-generated — const props = defineProps([...]) must produce const props = __props
    #[test]
    fn test_define_props_array_preserves_variable_assignment() {
        let input = r#"<script setup>
const props = defineProps(['disabled'])
function check() { return props.disabled }
</script>
<template><button :disabled="props.disabled" @click="check">x</button></template>"#;
        let result = gen_and_validate(input);
        assert!(
            result.code.contains("const props = __props"),
            "defineProps with array arg should produce `const props = __props`, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — const props = defineProps({...}) must produce const props = __props
    #[test]
    fn test_define_props_object_preserves_variable_assignment() {
        let input = r#"<script setup>
const props = defineProps({ disabled: Boolean })
function check() { return props.disabled }
</script>
<template><button :disabled="props.disabled" @click="check">x</button></template>"#;
        let result = gen_and_validate(input);
        assert!(
            result.code.contains("const props = __props"),
            "defineProps with object arg should produce `const props = __props`, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — const props = withDefaults(defineProps<T>(), {...}) must produce const props = __props
    #[test]
    fn test_with_defaults_type_params_preserves_variable_assignment() {
        let input = r#"<script setup lang="ts">
const props = withDefaults(defineProps<{ disabled?: boolean }>(), {
  disabled: false,
})
function check() { return props.disabled }
</script>
<template><button :disabled="props.disabled" @click="check">x</button></template>"#;
        let result = gen_and_validate(input);
        assert!(
            result.code.contains("const props = __props"),
            "withDefaults should produce `const props = __props`, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — TypeScript enum in script setup must be preserved (converted to JS)
    #[test]
    fn test_enum_preserved_in_script_setup() {
        let input = r#"<script setup lang="ts">
enum BtnStates { Default, Init, Confirming }
const state = ref(BtnStates.Default)
</script>
<template><div>{{ state }}</div></template>"#;
        let result = gen_result(input);
        eprintln!("=== ENUM OUTPUT ===\n{}\n=== END ===", result.code);
        // TypeScript enum should either be converted to JS or preserved for downstream tools.
        // It should NOT be silently removed (which would cause ReferenceError at runtime).
        assert!(
            result.code.contains("BtnStates"),
            "TypeScript enum should be preserved (converted to JS), not stripped:\n{}",
            result.code
        );
        // If the enum is still TypeScript syntax, it's NOT valid JS
        if result.code.contains("enum BtnStates") {
            eprintln!("WARNING: Enum is preserved as raw TypeScript — not valid JS!");
        }
    }

    /// @ai-generated — const props = defineProps<T>() must produce const props = __props (type-only)
    #[test]
    fn test_define_props_type_only_preserves_variable_assignment() {
        let input = r#"<script setup lang="ts">
const props = defineProps<{ disabled?: boolean }>()
function check() { return props.disabled }
</script>
<template><button :disabled="props.disabled" @click="check">x</button></template>"#;
        let result = gen_and_validate(input);
        assert!(
            result.code.contains("const props = __props"),
            "defineProps<T>() should produce `const props = __props`, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — const props = withDefaults(defineProps<Props>(), {...}) with external interface
    /// must produce const props = __props (not strip the assignment)
    #[test]
    fn test_with_defaults_external_type_preserves_variable_assignment() {
        let input = r#"<script setup lang="ts">
type Props = {
  disabled?: boolean;
};
const props = withDefaults(defineProps<Props>(), {
  disabled: false,
})
function doSomething() {
  console.log(props.disabled)
}
</script>
<template><button :disabled="props.disabled" @click="doSomething">Click</button></template>"#;
        let result = gen_and_validate(input);
        println!("OUTPUT:\n{}", result.code);
        assert!(
            result.code.contains("const props = __props"),
            "withDefaults with external type should produce `const props = __props`, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — const props = defineProps<Props>() with external interface
    /// must produce const props = __props
    #[test]
    fn test_define_props_external_type_preserves_variable_assignment() {
        let input = r#"<script setup lang="ts">
interface Props {
  disabled?: boolean;
}
const props = defineProps<Props>()
function check() { return props.disabled }
</script>
<template><button :disabled="props.disabled" @click="check">x</button></template>"#;
        let result = gen_and_validate(input);
        println!("OUTPUT:\n{}", result.code);
        assert!(
            result.code.contains("const props = __props"),
            "defineProps<ExternalInterface>() should produce `const props = __props`, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — Exact repro from props-interface.vue benchmark file
    #[test]
    fn test_props_interface_benchmark_file() {
        let input = r#"<template>
  <div class="flex flex-col items-center justify-center">
    <div class="mt-[29px] text-[15px] text-white">{{ props.describeText }}</div>
    <div v-if="props.isTimeFilter" class="filter-button">选择时间</div>
  </div>
</template>
<script setup lang="ts">
interface Props {
  isTimeFilter?: boolean;
  describeText?: string;
}
const props = withDefaults(defineProps<Props>(), {
  isTimeFilter: false,
  describeText: "...",
});
</script>"#;
        let result = gen_and_validate(input);
        println!("BENCHMARK FILE OUTPUT:\n{}", result.code);
        assert!(
            result.code.contains("const props = __props"),
            "Props should be assigned to __props in benchmark file, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — withDefaults with imported type (unresolvable external type)
    #[test]
    fn test_with_defaults_imported_type_preserves_variable_assignment() {
        let input = r#"<script lang="ts" setup>
import type { AffixProps } from './affix'
const props = withDefaults(defineProps<AffixProps>(), {
  zIndex: 100,
  target: '',
  offset: 0,
  position: 'top',
})
function check() { return props.zIndex }
</script>
<template><div>{{ props.zIndex }}</div></template>"#;
        let result = gen_and_validate(input);
        println!("IMPORTED TYPE OUTPUT:\n{}", result.code);
        assert!(
            result.code.contains("const props = __props"),
            "withDefaults with imported type should produce `const props = __props`, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — Production mode: const props = withDefaults(defineProps<Props>(), {...})
    /// with external interface must produce const props = __props
    #[test]
    fn test_with_defaults_external_type_prod_preserves_variable_assignment() {
        let input = r#"<script setup lang="ts">
type Props = {
  disabled?: boolean;
};
const props = withDefaults(defineProps<Props>(), {
  disabled: false,
})
function doSomething() {
  console.log(props.disabled)
}
</script>
<template><button :disabled="props.disabled" @click="doSomething">Click</button></template>"#;
        let result = gen_and_validate_prod(input);
        println!("PROD OUTPUT:\n{}", result.code);
        assert!(
            result.code.contains("const props = __props"),
            "Production withDefaults with external type should produce `const props = __props`, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — defineEmits with declarator should produce const emit = __emit
    #[test]
    fn test_define_emits_preserves_variable_assignment() {
        let input = r#"<script setup>
const emit = defineEmits(['change'])
function trigger() { emit('change') }
</script>
<template><button @click="trigger">x</button></template>"#;
        let result = gen_and_validate(input);
        println!("EMITS OUTPUT:\n{}", result.code);
        assert!(
            result.code.contains("const emit = __emit"),
            "defineEmits should produce `const emit = __emit`, got:\n{}",
            result.code
        );
    }

    /// @ai-generated — defineProps without assignment should NOT produce any variable assignment
    #[test]
    fn test_define_props_no_assignment() {
        let input = r#"<script setup>
defineProps(['disabled'])
</script>
<template><button :disabled="disabled">x</button></template>"#;
        let result = gen_and_validate(input);
        // Should not have "const  = __props" or similar
        assert!(
            !result.code.contains("const  = __props"),
            "defineProps without assignment should not create bogus variable: {}",
            result.code
        );
    }

    // ==================== whitespace condensation ====================

    /// @ai-generated — Whitespace-only text between elements with newlines is removed entirely.
    /// Vue condense mode: text that is ALL whitespace AND contains a newline → removed.
    #[test]
    fn test_whitespace_condense_removes_newline_only_text() {
        let input = r#"<template>
  <div>
    <span>a</span>
    <span>b</span>
  </div>
</template>"#;
        let result = gen_and_validate(input);
        // The whitespace between <span>a</span> and <span>b</span> should be removed,
        // not appear as a text child. Only the two <span> elements should be children.
        assert!(
            !result.code.contains("\\n"),
            "Whitespace-only text with newlines should be removed, got: {}",
            result.code
        );
    }

    /// @ai-generated — Multi-line text content is condensed: newlines+indentation → single space.
    /// This is the exact bug from the issue: `\n  Total APR\n  2.71%` → `Total APR 2.71%`
    #[test]
    fn test_whitespace_condense_multiline_text() {
        let input = r#"<template>
  <div data-testid="total-apr">
    Total APR
    2.71%
  </div>
</template>"#;
        let result = gen_and_validate(input);
        // The text should be condensed: leading/trailing whitespace with newlines removed,
        // internal newline+spaces → single space.
        // Note: the whitespace-only nodes before/after the text are separate Text events
        // that get removed. The text "Total APR\n    2.71%" itself has its whitespace condensed.
        assert!(
            result.code.contains("Total APR"),
            "Should contain 'Total APR', got: {}",
            result.code
        );
        assert!(
            !result.code.contains("\\n"),
            "Newlines should be condensed to spaces, got: {}",
            result.code
        );
    }

    /// @ai-generated — Simple text without extra whitespace is preserved as-is.
    #[test]
    fn test_whitespace_condense_simple_text_preserved() {
        let input = r#"<template><div>hello world</div></template>"#;
        let result = gen_and_validate(input);
        assert!(
            result.code.contains("\"hello world\""),
            "Simple text should be preserved: {}",
            result.code
        );
    }

    /// @ai-generated — Single space between elements (no newline) is dropped by the tokenizer
    /// (whitespace-only text before a `<` is not emitted). This is tokenizer-level behavior.
    #[test]
    fn test_whitespace_condense_single_space_between_elements_kept() {
        let input = r#"<template><div><span>a</span> <span>b</span></div></template>"#;
        let result = gen_and_validate(input);
        // Vue condense mode: space (no newline) between two elements → kept as single space.
        // Only whitespace WITH newlines between two elements is removed.
        assert!(
            result.code.contains("_createTextVNode(\" \")"),
            "Space between elements (no newline) should be kept: {}",
            result.code
        );
    }

    /// @ai-generated — Multiple spaces (no newline) condense to single space.
    #[test]
    fn test_whitespace_condense_multiple_spaces() {
        let input = "<template><div>a   b</div></template>";
        let result = gen_and_validate(input);
        assert!(
            result.code.contains("\"a b\""),
            "Multiple spaces should condense to single space: {}",
            result.code
        );
    }

    /// @ai-generated — Tab characters are treated as whitespace and condensed.
    #[test]
    fn test_whitespace_condense_tabs() {
        let input = "<template><div>a\t\tb</div></template>";
        let result = gen_and_validate(input);
        assert!(
            result.code.contains("\"a b\""),
            "Tabs should condense to single space: {}",
            result.code
        );
    }

    /// @ai-generated — Text with interpolation: whitespace between text and interpolation is condensed.
    #[test]
    fn test_whitespace_condense_with_interpolation() {
        let input = r#"<script setup>
const msg = 'hello'
</script>
<template>
  <div>
    prefix {{ msg }} suffix
  </div>
</template>"#;
        let result = gen_and_validate(input);
        // The text nodes around the interpolation should have their whitespace condensed.
        // " prefix " and " suffix " are separate text events from the tokenizer.
        assert!(
            !result.code.contains("\\n"),
            "No raw newlines should remain in text: {}",
            result.code
        );
    }

    /// Vue condense mode: multiple spaces (no newline) between elements → condense to single space.
    #[test]
    fn test_whitespace_condense_spaces_only_no_newline_between_elements_kept() {
        let input = "<template><div><span>a</span>   <span>b</span></div></template>";
        let result = gen_and_validate(input);
        // Vue condense mode: multiple spaces (no newline) → condense to single space, kept.
        assert!(
            result.code.contains("_createTextVNode(\" \")"),
            "Multiple spaces between elements (no newline) should condense to single space: {}",
            result.code
        );
    }

    /// @ai-generated — Mixed whitespace (spaces + newlines) within text content.
    #[test]
    fn test_whitespace_condense_mixed_content() {
        let input = "<template><div>hello\n    world</div></template>";
        let result = gen_and_validate(input);
        assert!(
            result.code.contains("\"hello world\""),
            "Newline + spaces in text should condense to single space: {}",
            result.code
        );
    }

    /// @ai-generated — Real-world GameAccess pattern: multi-line text in nested elements.
    #[test]
    fn test_whitespace_condense_real_world_total_apr() {
        let input = r#"<script setup>
const apr = '2.71%'
</script>
<template>
  <div>
    <span>
      Total APR:
      {{ apr }}
    </span>
  </div>
</template>"#;
        let result = gen_and_validate(input);
        // No raw newlines should appear in the output
        assert!(
            !result.code.contains("\\n"),
            "No raw newlines in condensed output: {}",
            result.code
        );
    }

    /// Vue condense mode: whitespace between element and interpolation (with newline)
    /// should be kept as a single space, NOT removed.
    ///
    /// Template: `<div><span>APR</span>\n  {{ value }}\n</div>`
    /// In Vue's compiler:
    ///   - Whitespace between </span> and {{ value }} has a newline → but it's between
    ///     an element and an interpolation, so it becomes " " (single space).
    ///   - Whitespace after {{ value }} at end → removed (last child).
    ///   - Whitespace before <span> at start → removed (first child).
    /// Result: text concat = `"APR"`, `" "`, `_toDisplayString(value)` → "APR 15.22%"
    #[test]
    fn test_whitespace_condense_element_interpolation_keeps_space() {
        let input = r#"<script setup>
const value = '15.22%'
</script>
<template>
  <div>
    <span>APR</span>
    {{ value }}
  </div>
</template>"#;
        let result = gen_and_validate(input);
        // The whitespace between </span> and {{ value }} should produce a " " text node.
        // This means the children should include a text concatenation with a space:
        // _createTextVNode(" " + _toDisplayString(...))  or similar with the space
        assert!(
            result.code.contains("\" \"")
                || result.code.contains("\" \" +")
                || result.code.contains("+ \" \""),
            "Whitespace between element and interpolation should be kept as space: {}",
            result.code
        );
    }

    /// Vue condense mode: whitespace between two elements (with newline) → removed.
    /// Template: `<div>\n  <span>A</span>\n  <span>B</span>\n</div>`
    /// The whitespace between the two spans has a newline and both siblings are elements → removed.
    #[test]
    fn test_whitespace_condense_between_elements_removed() {
        let input = r#"<template>
  <div>
    <span>A</span>
    <span>B</span>
  </div>
</template>"#;
        let result = gen_and_validate(input);
        // No text node should exist between the two spans.
        // The children array should have exactly the two spans.
        assert!(
            !result.code.contains("\" \""),
            "Whitespace between two elements should be removed, not kept as space: {}",
            result.code
        );
    }

    /// Vue condense mode: whitespace between interpolation and element (with newline)
    /// should be kept as a single space.
    #[test]
    fn test_whitespace_condense_interpolation_element_keeps_space() {
        let input = r#"<script setup>
const msg = 'hello'
</script>
<template>
  <div>
    {{ msg }}
    <span>world</span>
  </div>
</template>"#;
        let result = gen_and_validate(input);
        // The whitespace between {{ msg }} and <span> should produce a text node.
        assert!(
            result.code.contains("\" \"")
                || result.code.contains("\" \" +")
                || result.code.contains("+ \" \""),
            "Whitespace between interpolation and element should be kept as space: {}",
            result.code
        );
    }

    /// Real-world APRTooltip pattern: <span>label</span>\n{{ value }}
    /// The space between the label span and the value interpolation must be preserved.
    #[test]
    fn test_whitespace_condense_apr_tooltip_pattern() {
        let input = r#"<script setup>
const label = 'Swap fees APR'
const value = '15.22%'
</script>
<template>
  <div>
    <span>{{ label }}</span>
    {{ value }}
  </div>
</template>"#;
        let result = gen_and_validate(input);
        // Between the <span> and {{ value }}, the newline whitespace should become a space.
        // This ensures "Swap fees APR 15.22%" not "Swap fees APR15.22%".
        assert!(
            result.code.contains("\" \"")
                || result.code.contains("\" \" +")
                || result.code.contains("+ \" \""),
            "APRTooltip pattern: space between element and interpolation must be preserved: {}",
            result.code
        );
    }

    /// Self-closing component followed by div (multi-root with whitespace).
    #[test]
    fn test_whitespace_condense_self_closing_then_element() {
        let input = r#"<template>
  <Comp />
  <div>hello</div>
</template>"#;
        let result = gen_and_validate(input);
        assert!(
            !result.code.contains("\\n"),
            "No raw newlines: {}",
            result.code
        );
    }

    /// v-if self-closing component followed by div (multi-root).
    #[test]
    fn test_whitespace_condense_vif_self_closing_then_element() {
        let input = r#"<script setup>
const show = true
</script>
<template>
  <Comp v-if="show" />
  <div>hello</div>
</template>"#;
        let result = gen_and_validate(input);
        assert!(
            !result.code.contains("\\n"),
            "No raw newlines: {}",
            result.code
        );
    }

    /// Self-closing component with dynamic props followed by div (multi-root).
    /// Tests the patch flag path in handle_element_close_self_closing.
    #[test]
    fn test_whitespace_condense_self_closing_with_props() {
        let input = r#"<script setup>
const msg = 'hi'
</script>
<template>
  <Comp :msg="msg" />
  <div>hello</div>
</template>"#;
        let result = gen_and_validate(input);
        assert!(
            !result.code.contains("\\n"),
            "No raw newlines: {}",
            result.code
        );
    }

    /// v-if self-closing component with dynamic props followed by div (multi-root).
    /// Tests v-if + patch flag path interaction with whitespace removal.
    #[test]
    fn test_whitespace_condense_vif_self_closing_with_props() {
        let input = r#"<script setup>
const show = true
const msg = 'hi'
</script>
<template>
  <Comp v-if="show" :msg="msg" />
  <div>hello</div>
</template>"#;
        let result = gen_and_validate(input);
        assert!(
            !result.code.contains("\\n"),
            "No raw newlines: {}",
            result.code
        );
    }

    /// Focused layout pattern: multi-root with v-if + nested component slots.
    /// Reproduces build failure from FocussedLayout.vue.
    #[test]
    fn test_whitespace_condense_focused_layout() {
        let input = r#"<script setup>
import AppNavAlert from './AppNavAlert.vue'
const currentAlert = null
const isFakeModal = true
function getReturnRoute() { return '/' }
</script>
<template>
  <AppNavAlert v-if="currentAlert" :alert="currentAlert" />
  <div class="pb-16">
    <div class="h-screen" :class="{ 'bg-gray-850': isFakeModal }">
      <div class="mb-12 layout-header">
        <div />
        <BalBtn tag="router-link" :to="getReturnRoute()" color="white" circle>
          <BalIcon name="x" size="lg" />
        </BalBtn>
      </div>
      <slot />
    </div>
  </div>
</template>"#;
        let result = gen_and_validate(input);
        assert!(
            !result.code.contains("\\n"),
            "No raw newlines in output: {}",
            result.code
        );
    }

    /// v-for with array destructuring: `v-for="([address, amount], index) in entries"`
    /// Must preserve destructuring in renderList callback params.
    #[test]
    fn test_vfor_array_destructuring() {
        let input = r#"<script setup>
const entries = [['0x123', 100], ['0x456', 200]]
</script>
<template>
  <div v-for="([address, amount], index) in entries" :key="index">
    {{ address }} {{ amount }}
  </div>
</template>"#;
        let result = gen_and_validate(input);
        // Must have ([address, amount], index) in the callback, not (address, amount, index)
        assert!(
            result.code.contains("([address, amount], index)"),
            "v-for array destructuring must be preserved: {}",
            result.code
        );
    }

    /// v-for with object destructuring: `v-for="({ address, weight }, i) in tokens"`
    /// Must preserve destructuring in renderList callback params.
    #[test]
    fn test_vfor_object_destructuring() {
        let input = r#"<script setup>
const tokens = [{ address: '0x123', weight: 50 }]
</script>
<template>
  <div v-for="({ address, weight }, i) in tokens" :key="i">
    {{ address }} {{ weight }}
  </div>
</template>"#;
        let result = gen_and_validate(input);
        // Must have ({ address, weight }, i) in the callback, not (address, weight, i)
        assert!(
            result.code.contains("({ address, weight }, i)"),
            "v-for object destructuring must be preserved: {}",
            result.code
        );
    }

    /// v-for with simple pattern (no destructuring): `v-for="item in list"`
    /// Should still work correctly.
    #[test]
    fn test_vfor_simple_pattern() {
        let input = r#"<script setup>
const list = [1, 2, 3]
</script>
<template>
  <div v-for="item in list" :key="item">{{ item }}</div>
</template>"#;
        let result = gen_and_validate(input);
        assert!(
            result.code.contains("(item)"),
            "Simple v-for param must be wrapped in parens: {}",
            result.code
        );
    }

    /// v-for with (item, index) pattern.
    #[test]
    fn test_vfor_item_index_pattern() {
        let input = r#"<script setup>
const list = [1, 2, 3]
</script>
<template>
  <div v-for="(item, index) in list" :key="index">{{ item }}</div>
</template>"#;
        let result = gen_and_validate(input);
        assert!(
            result.code.contains("(item, index)"),
            "v-for (item, index) pattern must be preserved: {}",
            result.code
        );
    }

    /// Component with mixed slot content: element + interpolation.
    /// Reproduces APRTooltip StakingBreakdown pattern.
    /// Component with mixed slot content: element + interpolation.
    /// Text/interpolation runs inside component slots must be wrapped in
    /// _createTextVNode — bare strings are not valid VNodes.
    /// Reproduces APRTooltip StakingBreakdown pattern.
    #[test]
    fn test_component_mixed_slot_element_interpolation() {
        let input = r#"<script setup>
const value = '42%'
</script>
<template>
  <Comp justify="between">
    <span>Label</span>
    {{ value }}
  </Comp>
</template>"#;
        let result = gen_and_validate(input);
        // Text+interpolation in mixed component slot content must be wrapped
        // in _createTextVNode, not emitted as bare strings/toDisplayString.
        assert!(
            result.code.contains("_createTextVNode"),
            "Mixed slot text+interp must use _createTextVNode: {}",
            result.code
        );
    }

    /// Component with v-for containing mixed slot content.
    /// Reproduces StakingBreakdown pattern: BalHStack with span + interpolation.
    /// The interpolation value (amount) must render in the output.
    #[test]
    fn test_component_vfor_mixed_slot_element_interpolation() {
        let input = r#"<script setup>
import { ref } from 'vue'
const items = ref([['Min BAL', '0.44%'], ['Max BAL', '5.67%']])
function fmt(v) { return v }
</script>
<template>
  <div>
    <Comp v-for="([label, amount], i) in items" :key="i" justify="between">
      <span>{{ label }}</span>
      {{ fmt(amount) }}
    </Comp>
  </div>
</template>"#;
        let result = gen_and_validate(input);
        // The interpolation `{{ fmt(amount) }}` must be wrapped in _createTextVNode
        assert!(
            result.code.contains("_createTextVNode"),
            "v-for component mixed slot must use _createTextVNode: {}",
            result.code
        );
        // The fmt() call must appear in the output (not dropped)
        assert!(
            result.code.contains("fmt(amount)"),
            "fmt(amount) interpolation must be present in output: {}",
            result.code
        );
    }

    /// Exact reproduction of StakingBreakdown v-for breakdown pattern.
    /// The span has `{{ label }} {{ suffix }} </span>` — the trailing space
    /// after the last interpolation is a separate whitespace-only text node.
    /// Vue removes it because it's the last child of the span.
    /// Without this fix, the trailing space + condensed whitespace between
    /// </span> and {{ fNum(...) }} creates a double-space in the output.
    #[test]
    fn test_staking_breakdown_vfor_pattern() {
        let input = r#"<script setup>
import { ref } from 'vue'
const breakdownItems = ref([])
function fNum(v, f) { return v }
const FNumFormats = { bp: 'bp' }
const suffix = 'APR'
</script>
<template>
  <div data-testid="staking-apr">
    <BalHStack justify="between" class="font-bold">
      <span>Staking APR</span>
      {{ '0.44% - 5.67%' }}
    </BalHStack>
    <BalVStack spacing="xs" class="mt-1">
      <BalHStack
        v-for="([label, amount], i) in breakdownItems"
        :key="i"
        justify="between"
        class="text-gray-500"
      >
        <span class="ml-2">{{ label }} {{ suffix }} </span>
        {{ fNum(amount, FNumFormats.bp) }}
      </BalHStack>
    </BalVStack>
  </div>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("GENERATED:\n{}", result.code);
        // Both component slots must have _createTextVNode wrapping
        assert!(
            result.code.contains("_createTextVNode"),
            "Mixed slot text+interp must use _createTextVNode: {}",
            result.code
        );
        // The fNum call must be present
        assert!(
            result.code.contains("fNum(amount"),
            "fNum interpolation must be present: {}",
            result.code
        );
        // The span's trailing space (last child, whitespace-only) must be removed.
        // The span text should be: _toDisplayString(label) + " " + _toDisplayString(suffix)
        // NOT: _toDisplayString(label) + " " + _toDisplayString(suffix) + " "
        // Verify no trailing space before the span's closing paren:
        assert!(
            result.code.contains("_toDisplayString( $setup.suffix )"),
            "suffix interpolation must be present: {}",
            result.code
        );
    }

    /// Props from defineProps must be accessible in template without $props. prefix.
    /// Vue's compiler resolves `tag` in `:is="tag"` as `$props.tag` when `tag`
    /// is a prop from defineProps. Verter must do the same.
    /// Reproduces BalLink.vue pattern where <component :is="tag"> renders as <a>.
    #[test]
    fn test_define_props_accessible_in_template() {
        let input = r#"<script lang="ts" setup>
type Props = {
  tag?: string;
  external?: boolean;
};
const props = withDefaults(defineProps<Props>(), {
  tag: 'a',
  external: false,
});
</script>
<template>
  <component :is="tag">
    <slot />
  </component>
</template>"#;
        let result = gen_and_validate(input);
        // `tag` must resolve to $props.tag, NOT _ctx.tag
        assert!(
            !result.code.contains("_ctx.tag"),
            "tag must NOT use _ctx prefix (should be $props.tag): {}",
            result.code
        );
    }

    /// Dual script blocks: <script> with inheritAttrs + <script setup> with defineProps.
    /// Reproduces BalLink.vue pattern. The component :is="tag" must still resolve.
    #[test]
    fn test_dual_script_blocks_component_is() {
        let input = r#"<script lang="ts">
export default {
  inheritAttrs: false,
};
</script>
<script lang="ts" setup>
type Props = {
  tag?: string;
  external?: boolean;
};
const props = withDefaults(defineProps<Props>(), {
  tag: 'a',
  external: false,
});
const attrs = useAttrs();
</script>
<template>
  <component :is="tag" v-bind="attrs">
    <slot />
  </component>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("DUAL SCRIPT GENERATED:\n{}", result.code);
        // Must still resolve tag to $props.tag
        assert!(
            !result.code.contains("_ctx.tag"),
            "tag must NOT use _ctx prefix in dual-script: {}",
            result.code
        );
        // Must have inheritAttrs: false in the output
        assert!(
            result.code.contains("inheritAttrs: false")
                || result.code.contains("inheritAttrs:false"),
            "inheritAttrs must be preserved: {}",
            result.code
        );
    }

    /// Object shorthand with prop that gets $props. prefix must expand to full key-value.
    /// `{ as }` where `as` is a prop → `{ as: $props.as }` (not `{ $props.as }`)
    /// Reproduces oku-primitives Label.vue pattern.
    #[test]
    fn test_object_shorthand_prop_expansion() {
        let input = r#"<script setup lang="ts">
interface Props { as?: string }
withDefaults(defineProps<Props>(), { as: 'label' })
</script>
<template>
  <Primitive v-bind="normalizeAttrs(fn([$attrs, { as }]))">
    <slot />
  </Primitive>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("SHORTHAND PROP OUTPUT:\n{}", result.code);
        // Must NOT contain { $props.as } — that's invalid JS
        assert!(
            !result.code.contains("{ $props.as }"),
            "shorthand {{ $props.as }} is invalid, must expand to {{ as: $props.as }}: {}",
            result.code
        );
        // Must NOT contain { _ctx.as } either
        assert!(
            !result.code.contains("{ _ctx.as }"),
            "shorthand {{ _ctx.as }} is invalid, must expand: {}",
            result.code
        );
        // Must contain the expanded form
        assert!(
            result.code.contains("as: $props.as"),
            "must expand to {{ as: $props.as }}: {}",
            result.code
        );
    }

    /// Custom directive on a component (v-c-placeholder).
    /// Reproduces coreui Placeholders.vue: CButton with v-c-placeholder directive.
    #[test]
    fn test_custom_directive_on_component() {
        let input = r##"<script setup>
</script>
<template>
  <CButton
    v-c-placeholder="{ xs: 6 }"
    color="primary"
    aria-hidden="true"
    disabled
    href="#"
    tabindex="-1"
  ></CButton>
</template>"##;
        let result = gen_and_validate(input);
        eprintln!("CUSTOM DIRECTIVE OUTPUT:\n{}", result.code);
        assert!(
            result.code.contains("_withDirectives"),
            "must use _withDirectives: {}",
            result.code
        );
    }

    /// Custom directive on component WITH children.
    /// Reproduces coreui: <CCardTitle v-c-placeholder="..."><CPlaceholder /></CCardTitle>
    #[test]
    fn test_custom_directive_on_component_with_children() {
        let input = r#"<script setup>
</script>
<template>
  <CCardTitle v-c-placeholder="{ animation: 'glow', xs: 7 }">
    <CPlaceholder :xs="6" />
  </CCardTitle>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("DIRECTIVE WITH CHILDREN:\n{}", result.code);
        assert!(
            result.code.contains("_withDirectives"),
            "must use _withDirectives: {}",
            result.code
        );
    }

    /// Reproduces oku-primitives Label.vue exactly: defineProps with imported type,
    /// defineOptions, defineEmits, and shorthand `{ as }` in template expression.
    /// KNOWN LIMITATION: When LabelProps is imported from another file, Verter can't
    /// cross-file resolve the type to extract prop names. This causes `as` to be
    /// resolved as `_ctx.as` instead of `$props.as`. Requires cross-file type resolution.
    #[test]
    #[ignore = "requires cross-file type resolution for defineProps<ImportedType>()"]
    fn test_oku_label_pattern_imported_type() {
        let input = r#"<script setup lang="ts">
import type { EmitsToHookProps } from '../shared/index.ts'
import type { LabelProps, LabelEmits } from './Label.ts'
import { Primitive } from '../primitive/index.ts'
import { normalizeAttrs } from '../shared/index.ts'
import { DEFAULT_LABEL_PROPS, useLabel } from './Label.ts'

defineOptions({
  name: 'RadixLabel',
  inheritAttrs: false,
})

withDefaults(defineProps<LabelProps>(), DEFAULT_LABEL_PROPS)
const emit = defineEmits<LabelEmits>()

const label = useLabel({
  onMousedown(event) {
    emit('mousedown', event)
  },
} satisfies Required<EmitsToHookProps<LabelEmits>>)
</script>
<template>
  <Primitive v-bind="normalizeAttrs(label.attrs([$attrs, { as }]))">
    <slot />
  </Primitive>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("OKU LABEL OUTPUT:\n{}", result.code);
        // `as` must NOT use _ctx prefix — it's a prop
        assert!(
            !result.code.contains("_ctx.as"),
            "as must NOT use _ctx prefix (should be $props.as): {}",
            result.code
        );
    }

    /// Scoped slot with hyphenated prop name.
    /// `:handle-keydown="onKeydown"` must produce a valid object key.
    /// Reproduces element-plus focus-trap.vue pattern.
    #[test]
    fn test_slot_hyphenated_prop() {
        let input = r#"<script>
import { defineComponent } from 'vue'
export default defineComponent({
  setup() {
    const onKeydown = () => {}
    return { onKeydown }
  }
})
</script>
<template>
  <slot :handle-keydown="onKeydown" />
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("SLOT HYPHENATED PROP:\n{}", result.code);
        // The key "handle-keydown" must be quoted or camelized
        assert!(
            !result.code.contains("handle-keydown:"),
            "hyphenated key must be quoted or camelized: {}",
            result.code
        );
    }

    /// v-show on component with complex children (el-overlay pattern).
    /// Reproduces element-plus drawer.vue: v-show on el-overlay with nested slots.
    #[test]
    fn test_vshow_component_with_nested_children() {
        let input = r#"<script setup>
import { ref } from 'vue'
const visible = ref(true)
const ns = { b: () => 'overlay', e: (s) => s, is: (s,v) => s }
const handleClick = () => {}
</script>
<template>
  <ElOverlay
    v-show="visible"
    :mask="true"
    @click="handleClick"
  >
    <div ref="drawerRef" @click.stop>
      <header>
        <slot name="header" :close="handleClick">
          <span>Title</span>
        </slot>
      </header>
      <div>
        <slot />
      </div>
    </div>
  </ElOverlay>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("V-SHOW COMPONENT WITH CHILDREN:\n{}", result.code);
        assert!(
            result.code.contains("_withDirectives"),
            "must use _withDirectives for v-show: {}",
            result.code
        );
    }

    /// Computed property names in :class object binding.
    /// `[ns.bm('group', 'append')]: $slots.append` must produce valid JS.
    /// Reproduces element-plus input.vue pattern.
    #[test]
    fn test_class_computed_property_name() {
        let input = r#"<script setup>
const ns = { bm: (a, b) => a + b }
</script>
<template>
  <div :class="[containerKls, { [ns.bm('group', 'append')]: $slots.append }]">
    <slot />
  </div>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("COMPUTED PROP NAME:\n{}", result.code);
    }

    /// HTML comment between v-if/v-else branches breaks ternary chain.
    #[test]
    fn test_comment_between_vif_velse_branches() {
        let input = r#"<script setup>
const show = true
</script>
<template>
  <div>
    <span v-if="show">A</span>
    <!-- comment -->
    <span v-else>B</span>
  </div>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("COMMENT BETWEEN IF/ELSE:\n{}", result.code);
        // The comment should NOT break the ternary chain
    }

    /// HTML comment between v-else-if branches breaks ternary chain.
    #[test]
    fn test_comment_between_velseif_branches() {
        let input = r#"<script setup>
const a = true
const b = false
</script>
<template>
  <div>
    <span v-if="a">A</span>
    <span v-else-if="b">B</span>
    <!-- eslint-disable -->
    <span v-else>C</span>
  </div>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("COMMENT BETWEEN ELSE-IF/ELSE:\n{}", result.code);
    }

    /// Reproduces element-plus cascader-panel/menu.vue — exact template.
    #[test]
    fn test_element_plus_cascader_menu() {
        let input = r##"<template>
  <el-scrollbar
    :key="menuId"
    tag="ul"
    role="menu"
    :class="ns.b()"
    :wrap-class="ns.e('wrap')"
    :view-class="[ns.e('list'), ns.is('empty', isEmpty)]"
    @mousemove="handleMouseMove"
    @mouseleave="clearHoverZone"
  >
    <el-cascader-node
      v-for="node in nodes"
      :key="node.uid"
      :node="node"
      :menu-id="menuId"
      @expand="handleExpand"
    />
    <div v-if="isLoading" :class="ns.e('empty-text')">
      <el-icon size="14" :class="ns.is('loading')">
        <loading />
      </el-icon>
      {{ t('el.cascader.loading') }}
    </div>
    <div v-else-if="isEmpty" :class="ns.e('empty-text')">
      <slot name="empty">{{ t('el.cascader.noData') }}</slot>
    </div>
    <!-- eslint-disable vue/html-self-closing -->
    <svg
      v-else-if="panel?.isHoverMenu"
      ref="hoverZone"
      :class="ns.e('hover-zone')"
    ></svg>
    <!-- eslint-enable vue/html-self-closing -->
  </el-scrollbar>
</template>
<script setup>
import { computed, getCurrentInstance, inject, ref } from 'vue'
import { Loading } from '@element-plus/icons-vue'
defineOptions({ name: 'ElCascaderMenu' })
const props = defineProps({
  nodes: { type: Array, required: true },
  index: { type: Number, required: true },
})
const ns = { b: () => '', e: (s) => s, is: (a, b) => a }
const t = (s) => s
const panel = inject('key')
const hoverZone = ref(null)
const isEmpty = computed(() => !props.nodes.length)
const isLoading = computed(() => false)
const menuId = computed(() => `test-${props.index}`)
const handleExpand = (e) => {}
const handleMouseMove = (e) => {}
const clearHoverZone = () => {}
</script>"##;
        let result = gen_and_validate(input);
        eprintln!("CASCADER MENU:\n{}", result.code);
    }

    /// Reproduces element-plus input/input.vue — exact template.
    #[test]
    fn test_element_plus_input() {
        let input = r##"<template>
  <div
    :class="[
      containerKls,
      {
        [nsInput.bm('group', 'append')]: $slots.append,
        [nsInput.bm('group', 'prepend')]: $slots.prepend,
      },
    ]"
    :style="containerStyle"
    @mouseenter="handleMouseEnter"
    @mouseleave="handleMouseLeave"
  >
    <!-- input -->
    <template v-if="type !== 'textarea'">
      <!-- prepend slot -->
      <div v-if="$slots.prepend" :class="nsInput.be('group', 'prepend')">
        <slot name="prepend" />
      </div>

      <div ref="wrapperRef" :class="wrapperKls">
        <!-- prefix slot -->
        <span v-if="$slots.prefix || prefixIcon" :class="nsInput.e('prefix')">
          <span :class="nsInput.e('prefix-inner')">
            <slot name="prefix" />
            <el-icon v-if="prefixIcon" :class="nsInput.e('icon')">
              <component :is="prefixIcon" />
            </el-icon>
          </span>
        </span>

        <input
          :id="inputId"
          ref="input"
          :class="nsInput.e('inner')"
          v-bind="attrs"
          :name="name"
          :minlength="minlength"
          :maxlength="maxlength"
          :type="showPassword ? (passwordVisible ? 'text' : 'password') : type"
          :disabled="inputDisabled"
          :readonly="readonly"
          :autocomplete="autocomplete"
          :tabindex="tabindex"
          :aria-label="ariaLabel"
          :placeholder="placeholder"
          :style="inputStyle"
          :form="form"
          :autofocus="autofocus"
          :role="containerRole"
          :inputmode="inputmode"
          @compositionstart="handleCompositionStart"
          @compositionupdate="handleCompositionUpdate"
          @compositionend="handleCompositionEnd"
          @input="handleInput"
          @change="handleChange"
          @keydown="handleKeydown"
        />

        <!-- suffix slot -->
        <span v-if="suffixVisible" :class="nsInput.e('suffix')">
          <span :class="nsInput.e('suffix-inner')">
            <template
              v-if="!showClear || !showPwdVisible || !isWordLimitVisible"
            >
              <slot name="suffix" />
              <el-icon v-if="suffixIcon" :class="nsInput.e('icon')">
                <component :is="suffixIcon" />
              </el-icon>
            </template>
            <el-icon
              v-if="showClear"
              :class="[nsInput.e('icon'), nsInput.e('clear')]"
              @mousedown.prevent="NOOP"
              @click="clear"
            >
              <component :is="clearIcon" />
            </el-icon>
            <el-icon
              v-if="showPwdVisible"
              :class="[nsInput.e('icon'), nsInput.e('password')]"
              @click="handlePasswordVisible"
              @mousedown.prevent="NOOP"
              @mouseup.prevent="NOOP"
            >
              <component :is="passwordIcon" />
            </el-icon>
            <span
              v-if="isWordLimitVisible"
              :class="[
                nsInput.e('count'),
                nsInput.is('outside', wordLimitPosition === 'outside'),
              ]"
            >
              <span :class="nsInput.e('count-inner')">
                {{ textLength }} / {{ maxlength }}
              </span>
            </span>
            <el-icon
              v-if="validateState && validateIcon && needStatusIcon"
              :class="[
                nsInput.e('icon'),
                nsInput.e('validateIcon'),
                nsInput.is('loading', validateState === 'validating'),
              ]"
            >
              <component :is="validateIcon" />
            </el-icon>
          </span>
        </span>
      </div>

      <!-- append slot -->
      <div v-if="$slots.append" :class="nsInput.be('group', 'append')">
        <slot name="append" />
      </div>
    </template>

    <!-- textarea -->
    <template v-else>
      <textarea
        :id="inputId"
        ref="textarea"
        :class="[nsTextarea.e('inner'), nsInput.is('focus', isFocused)]"
        v-bind="attrs"
        :name="name"
        :minlength="minlength"
        :maxlength="maxlength"
        :tabindex="tabindex"
        :disabled="inputDisabled"
        :readonly="readonly"
        :autocomplete="autocomplete"
        :style="textareaStyle"
        :aria-label="ariaLabel"
        :placeholder="placeholder"
        :form="form"
        :autofocus="autofocus"
        :rows="rows"
        :role="containerRole"
        @compositionstart="handleCompositionStart"
        @compositionupdate="handleCompositionUpdate"
        @compositionend="handleCompositionEnd"
        @input="handleInput"
        @focus="handleFocus"
        @blur="handleBlur"
        @change="handleChange"
        @keydown="handleKeydown"
      />
      <span
        v-if="isWordLimitVisible"
        :style="countStyle"
        :class="[
          nsInput.e('count'),
          nsInput.is('outside', wordLimitPosition === 'outside'),
        ]"
      >
        {{ textLength }} / {{ maxlength }}
      </span>
    </template>
  </div>
</template>
<script lang="ts" setup>
import { computed, ref, shallowRef, toRef, useAttrs as useRawAttrs, useSlots } from 'vue'
const NOOP = () => {}
defineOptions({ name: 'ElInput', inheritAttrs: false })
const props = defineProps({
  type: { type: String, default: 'text' },
  modelValue: { type: [String, Number], default: '' },
  name: String,
  minlength: Number,
  maxlength: Number,
  disabled: Boolean,
  readonly: Boolean,
  autocomplete: String,
  tabindex: [String, Number],
  placeholder: String,
  form: String,
  autofocus: Boolean,
  rows: { type: Number, default: 2 },
  resize: String,
  inputmode: String,
  clearable: Boolean,
  showPassword: Boolean,
  showWordLimit: Boolean,
  suffixIcon: [String, Object],
  prefixIcon: [String, Object],
  validateEvent: { type: Boolean, default: true },
})
const emit = defineEmits(['update:modelValue', 'input', 'change', 'clear', 'mouseenter', 'mouseleave', 'keydown'])
const nsInput = { b: () => '', e: (s) => s, m: (s) => s, bm: (a, b) => a, be: (a, b) => a, is: (a, b) => a }
const nsTextarea = { b: () => '', e: (s) => s }
const attrs = {}
const containerKls = computed(() => [])
const wrapperKls = computed(() => [])
const containerStyle = computed(() => ({}))
const inputStyle = computed(() => ({}))
const textareaStyle = computed(() => ({}))
const countStyle = ref({})
const inputId = ref('id')
const inputDisabled = ref(false)
const suffixVisible = ref(true)
const showClear = ref(false)
const showPwdVisible = ref(false)
const isWordLimitVisible = ref(false)
const passwordVisible = ref(false)
const hovering = ref(false)
const isFocused = ref(false)
const validateState = ref('')
const validateIcon = ref(null)
const needStatusIcon = ref(false)
const textLength = ref(0)
const clearIcon = ref(null)
const passwordIcon = ref(null)
const ariaLabel = ref('')
const containerRole = ref('')
const wordLimitPosition = ref('outside')
const input = shallowRef(null)
const textarea = shallowRef(null)
const handleMouseEnter = () => {}
const handleMouseLeave = () => {}
const handleCompositionStart = () => {}
const handleCompositionUpdate = () => {}
const handleCompositionEnd = () => {}
const handleInput = () => {}
const handleChange = () => {}
const handleKeydown = () => {}
const handleFocus = () => {}
const handleBlur = () => {}
const handlePasswordVisible = () => {}
const clear = () => {}
</script>"##;
        let result = gen_and_validate(input);
        eprintln!("INPUT:\n{}", result.code);
    }

    /// Multi-line HTML comment inside a component child.
    #[test]
    fn test_multiline_comment_inside_component() {
        let input = r#"<script setup>
const checked = true
</script>
<template>
  <Radio v-model="checked">
    <!--
      Multi-line comment
      inside a component
    -->
    <span />
  </Radio>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("MULTILINE COMMENT:\n{}", result.code);
    }

    /// Conditional named slot with v-if on template #slotname.
    #[test]
    fn test_conditional_named_slot() {
        let input = r#"<script setup>
const inputValue = ''
</script>
<template>
  <ElInput v-model="inputValue">
    <template v-if="$slots.prefix" #prefix>
      <slot name="prefix" />
    </template>
    <template #suffix>
      <Icon v-if="clearBtnVisible" @click.stop="handleClear">
        <component :is="clearIcon" />
      </Icon>
      <Icon v-else @click.stop="togglePopperVisible()">
        <arrow-down />
      </Icon>
    </template>
  </ElInput>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("CONDITIONAL NAMED SLOT:\n{}", result.code);
    }

    /// @click.stop with no handler (empty event modifier).
    #[test]
    fn test_empty_event_handler_with_stop_modifier() {
        let input = r#"<script setup>
const handler = () => {}
</script>
<template>
  <Checkbox
    :model-value="true"
    @click.stop
    @update:model-value="handler"
  />
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("EMPTY HANDLER STOP:\n{}", result.code);
    }

    /// Reproduces cascader-panel/node.vue pattern (v-if chain with @click.stop and @update:model-value).
    #[test]
    fn test_cascader_node_pattern() {
        let input = r##"<script setup>
import { computed, inject } from 'vue'
const props = defineProps({
  node: { type: Object, required: true },
  menuId: String,
})
const ns = { b: () => '', e: (s) => s, is: (a, b) => a }
const multiple = computed(() => false)
const checkStrictly = computed(() => false)
const showPrefix = computed(() => true)
const checkedNodeId = computed(() => 1)
const isDisabled = computed(() => false)
const isLeaf = computed(() => false)
const expandable = computed(() => true)
const inExpandingPath = computed(() => false)
const inCheckedPath = computed(() => false)
const handleHoverExpand = () => {}
const handleClick = () => {}
const handleSelectCheck = () => {}
</script>
<template>
  <li
    :id="`${menuId}-${node.uid}`"
    role="menuitem"
    :class="[ns.b(), ns.is('active', node.checked)]"
    @mouseenter="handleHoverExpand"
    @click="handleClick"
  >
    <!-- prefix -->
    <Checkbox
      v-if="multiple && showPrefix"
      :model-value="node.checked"
      :disabled="isDisabled"
      @click.stop
      @update:model-value="handleSelectCheck"
    />
    <Radio
      v-else-if="checkStrictly && showPrefix"
      :model-value="checkedNodeId"
      :label="node.uid"
      :disabled="isDisabled"
      @update:model-value="handleSelectCheck"
      @click.stop
    >
      <!--
        Add an empty element to avoid render label
      -->
      <span />
    </Radio>
    <Icon v-else-if="isLeaf && node.checked" :class="ns.e('prefix')">
      <check />
    </Icon>
  </li>
</template>"##;
        let result = gen_and_validate(input);
        eprintln!("CASCADER NODE:\n{}", result.code);
    }

    /// Custom directive with dynamic argument using bracket syntax: v-click-outside:[triggerRef]
    /// This is the pattern from element-plus color-picker.vue that causes "Expected ) but found ]"
    #[test]
    fn test_custom_directive_dynamic_arg_bracket() {
        let input = r#"<script setup>
import { ClickOutside as vClickOutside } from '@element-plus/directives'
const triggerRef = ref()
const handleClickOutside = () => {}
</script>
<template>
  <div v-click-outside:[triggerRef]="handleClickOutside">content</div>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("DIRECTIVE DYNAMIC ARG:\n{}", result.code);
        // Ensure the directive is actually used in _withDirectives
        assert!(
            result.code.contains("_directive_click_outside"),
            "Directive should be referenced in output"
        );
    }

    /// Custom directive with dynamic arg on a COMPONENT (not element) — this is the real pattern
    /// from color-picker.vue where v-click-outside:[triggerRef] is on a component with named slots
    #[test]
    fn test_custom_directive_dynamic_arg_on_component() {
        let input = r#"<script setup>
import { ClickOutside as vClickOutside } from '@element-plus/directives'
const triggerRef = ref()
const handleClickOutside = () => {}
const panelProps = {}
</script>
<template>
  <MyComp v-bind="panelProps" v-click-outside:[triggerRef]="handleClickOutside" :border="false">
    <template #footer>
      <div>footer</div>
    </template>
  </MyComp>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("DIRECTIVE ON COMPONENT:\n{}", result.code);
        assert!(
            result.code.contains("_directive_click_outside"),
            "Directive should be referenced in output"
        );
    }

    /// Full color-picker.vue pattern: el-tooltip with named slots, v-click-outside:[triggerRef],
    /// v-bind="$attrs", v-show, v-if conditions — tests bracket handling in directives
    #[test]
    fn test_element_plus_color_picker_pattern() {
        let input = r#"<script setup>
import { ClickOutside as vClickOutside } from '@element-plus/directives'
const triggerRef = ref()
const showPicker = ref(false)
const panelProps = computed(() => ({}))
const handleClickOutside = () => {}
const handleEsc = (e) => {}
const clearable = ref(true)
const btnKls = computed(() => [])
const buttonId = ref('')
const modelValue = ref('')
const showPanelColor = ref(false)
const showAlpha = ref(false)
const displayedColor = computed(() => '')
const ns = { be: (a, b) => '', is: (a, b) => '' }
</script>
<template>
  <el-tooltip
    ref="popper"
    :visible="showPicker"
    :show-arrow="false"
    trigger="click"
  >
    <template #content>
      <el-color-picker-panel
        ref="pickerPanelRef"
        v-bind="panelProps"
        v-click-outside:[triggerRef]="handleClickOutside"
        :border="false"
        @keydown.esc="handleEsc"
      >
        <template #footer>
          <div>
            <el-button
              v-if="clearable"
              :class="ns.be('footer', 'link-btn')"
              text
              size="small"
              @click="clear"
            >
              Clear
            </el-button>
            <el-button
              plain
              size="small"
              :class="ns.be('footer', 'btn')"
              @click="confirmValue"
            >
              Confirm
            </el-button>
          </div>
        </template>
      </el-color-picker-panel>
    </template>
    <template #default>
      <div
        :id="buttonId"
        ref="triggerRef"
        v-bind="$attrs"
        :class="btnKls"
        role="button"
      >
        <div :class="ns.be('picker', 'trigger')">
          <span :class="[ns.be('picker', 'color'), ns.is('alpha', showAlpha)]">
            <span
              :class="ns.be('picker', 'color-inner')"
              :style="{ backgroundColor: displayedColor }"
            >
              <el-icon
                v-show="modelValue || showPanelColor"
                :class="[ns.be('picker', 'icon'), ns.is('icon-arrow-down')]"
              >
                <arrow-down />
              </el-icon>
              <el-icon
                v-show="!modelValue && !showPanelColor"
                :class="[ns.be('picker', 'empty'), ns.is('icon-close')]"
              >
                <close />
              </el-icon>
            </span>
          </span>
        </div>
      </div>
    </template>
  </el-tooltip>
</template>"#;
        let result = gen_result(input);
        std::fs::write("color_picker_output.js", &result.code).unwrap();
        assert_valid_js(&result.code, "color-picker pattern");
    }

    /// Full switch.vue pattern from element-plus — tests reserved word filename + complex template
    #[test]
    fn test_element_plus_switch_full() {
        let input = r#"<script setup>
import { computed, ref, shallowRef } from 'vue'
import { switchEmits } from './switch'

const COMPONENT_NAME = 'ElSwitch'
defineOptions({ name: COMPONENT_NAME })

const props = defineProps({
  modelValue: { default: false },
  disabled: { default: undefined },
  activeText: { default: '' },
  inactiveText: { default: '' },
  name: { default: '' },
})
const emit = defineEmits(switchEmits)
const ns = { b: () => '', m: (s) => '', e: (s) => '', em: (a, b) => '', is: (a, b) => '' }
const inputId = ref('')
const input = shallowRef()
const switchDisabled = ref(false)
const checked = computed(() => true)
const switchKls = computed(() => [])
const labelLeftKls = computed(() => [])
const labelRightKls = computed(() => [])
const coreStyle = computed(() => ({}))
const inlinePrompt = ref(false)
const loading = ref(false)

const handleChange = () => {}
const switchValue = () => {}
</script>
<template>
  <div :class="switchKls" @click.prevent="switchValue">
    <input
      :id="inputId"
      ref="input"
      :class="ns.e('input')"
      type="checkbox"
      role="switch"
      :aria-checked="checked"
      :aria-disabled="switchDisabled"
      :name="name"
      :disabled="switchDisabled"
      @change="handleChange"
      @keydown.enter="switchValue"
    />
    <span
      v-if="!inlinePrompt && (inactiveIcon || inactiveText || $slots.inactive)"
      :class="labelLeftKls"
    >
      <slot name="inactive">
        <el-icon v-if="inactiveIcon">
          <component :is="inactiveIcon" />
        </el-icon>
        <span v-if="!inactiveIcon && inactiveText" :aria-hidden="checked">{{
          inactiveText
        }}</span>
      </slot>
    </span>
    <span :class="ns.e('core')" :style="coreStyle">
      <div v-if="inlinePrompt" :class="ns.e('inner')">
        <div v-if="!checked" :class="ns.e('inner-wrapper')">
          <slot name="inactive">
            <el-icon v-if="inactiveIcon">
              <component :is="inactiveIcon" />
            </el-icon>
            <span v-if="!inactiveIcon && inactiveText">{{ inactiveText }}</span>
          </slot>
        </div>
        <div v-else :class="ns.e('inner-wrapper')">
          <slot name="active">
            <el-icon v-if="activeIcon">
              <component :is="activeIcon" />
            </el-icon>
            <span v-if="!activeIcon && activeText">{{ activeText }}</span>
          </slot>
        </div>
      </div>
      <div :class="ns.e('action')">
        <el-icon v-if="loading" :class="ns.is('loading')">
          <loading />
        </el-icon>
        <slot v-else-if="checked" name="active-action">
          <el-icon v-if="activeActionIcon">
            <component :is="activeActionIcon" />
          </el-icon>
        </slot>
        <slot v-else-if="!checked" name="inactive-action">
          <el-icon v-if="inactiveActionIcon">
            <component :is="inactiveActionIcon" />
          </el-icon>
        </slot>
      </div>
    </span>
    <span
      v-if="!inlinePrompt && (activeIcon || activeText || $slots.active)"
      :class="labelRightKls"
    >
      <slot name="active">
        <el-icon v-if="activeIcon">
          <component :is="activeIcon" />
        </el-icon>
        <span v-if="!activeIcon && activeText" :aria-hidden="!checked">{{
          activeText
        }}</span>
      </slot>
    </span>
  </div>
</template>"#;
        let result = gen_and_validate_with_filename(input, "switch.vue");
        eprintln!("SWITCH FULL:\n{}", result.code);
    }

    /// Conditional named slots: <template v-if="cond" #name> inside a component
    /// must use _createSlots or conditionally include the slot, not inline ternary
    #[test]
    fn test_conditional_named_slot_v_if_v_else_if() {
        let input = r#"<script setup>
const loading = ref(false)
const items = ref([])
</script>
<template>
  <MyComp :data="items">
    <template v-if="loading" #loading>
      <div>Loading...</div>
    </template>
    <template v-else-if="items.length === 0" #empty>
      <div>No data</div>
    </template>
  </MyComp>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("CONDITIONAL SLOTS:\n{}", result.code);
    }

    /// Simpler case: single conditional named slot with v-if
    #[test]
    fn test_single_conditional_named_slot() {
        let input = r#"<script setup>
const show = ref(true)
</script>
<template>
  <MyComp>
    <template v-if="show" #header>
      <div>Header</div>
    </template>
    <template #default>
      <div>Default</div>
    </template>
  </MyComp>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("SINGLE CONDITIONAL SLOT:\n{}", result.code);
    }

    /// Static named slot closes before conditional sibling sets any_dynamic_slots.
    /// The static slot must be retroactively patched to { name, fn } format.
    /// Reproduces dropdown.vue pattern: <template #content> then <template v-if #default>.
    #[test]
    fn test_static_slot_before_conditional_sibling() {
        let input = r#"<script setup>
const splitButton = ref(false)
</script>
<template>
  <MyTooltip trigger="click">
    <template #content>
      <div>Scrollbar content</div>
    </template>
    <template v-if="!splitButton" #default>
      <div>Trigger element</div>
    </template>
  </MyTooltip>
</template>"#;
        let result = gen_and_validate(input);
        let code = &result.code;
        eprintln!("STATIC SLOT BEFORE CONDITIONAL:\n{}", code);

        // Both slots should use { name, fn } format inside _createSlots
        assert!(
            code.contains("_createSlots("),
            "Should use _createSlots for mixed static/conditional slots"
        );
        assert!(
            code.contains(r#"{ name: "content", fn: _withCtx"#),
            "Static #content slot should be patched to {{ name, fn }} format"
        );
        assert!(
            code.contains(r#"{ name: "default", fn: _withCtx"#),
            "Conditional #default slot should use {{ name, fn }} format"
        );
    }

    /// select-v2.vue: v-click-outside on native element, v-for inside slot,
    /// inline event expressions, @keydown.delete.stop, v-text, conditional named slots
    #[test]
    fn test_element_plus_select_v2_pattern() {
        let input = r#"<script setup>
import { ClickOutside as vClickOutside } from '@element-plus/directives'
const popperRef = ref()
const handleClickOutside = () => {}
const states = reactive({ inputHovering: false, inputValue: '', isBeforeHide: false })
const selectSize = ref('default')
const nsSelect = { b: () => '', m: (s) => '', e: (s) => '', is: (a, b) => '', be: (a, b) => '' }
const dropdownMenuVisible = ref(false)
const filterable = ref(false)
const selectDisabled = ref(false)
const expanded = ref(false)
const isFocused = ref(false)
const multiple = ref(false)
const loading = ref(false)
const showTagList = ref([])
const filteredOptions = ref([])
const emptyText = ref('')
const toggleMenu = () => {}
const handleMenuEnter = () => {}
const onInput = () => {}
const handleCompositionStart = () => {}
const handleCompositionUpdate = () => {}
const handleCompositionEnd = () => {}
const onKeyboardNavigate = (d) => {}
const onKeyboardSelect = () => {}
const handleEsc = () => {}
const handleDel = () => {}
const deleteTag = (e, i) => {}
const getValueKey = (v) => v
const getValue = (i) => i
const getLabel = (i) => i
const getDisabled = (i) => false
</script>
<template>
  <div
    ref="selectRef"
    v-click-outside:[popperRef]="handleClickOutside"
    :class="[nsSelect.b(), nsSelect.m(selectSize)]"
    @mouseenter="states.inputHovering = true"
    @mouseleave="states.inputHovering = false"
  >
    <el-tooltip
      ref="tooltipRef"
      :visible="dropdownMenuVisible"
      trigger="click"
      @before-show="handleMenuEnter"
      @hide="states.isBeforeHide = false"
    >
      <template #default>
        <div ref="wrapperRef" :class="nsSelect.e('wrapper')" @click.prevent="toggleMenu">
          <div ref="selectionRef" :class="nsSelect.e('selection')">
            <slot v-if="multiple" name="tag" :data="states.cachedOptions" :delete-tag="deleteTag">
              <div v-for="item in showTagList" :key="getValueKey(getValue(item))" :class="nsSelect.e('selected-item')">
                <el-tag :closable="!selectDisabled && !getDisabled(item)" @close="deleteTag($event, item)">
                  <span :class="nsSelect.e('tags-text')">
                    <slot name="label" :label="getLabel(item)" :value="getValue(item)">
                      {{ getLabel(item) }}
                    </slot>
                  </span>
                </el-tag>
              </div>
            </slot>
            <div :class="nsSelect.e('selected-item')">
              <input
                ref="inputRef"
                :value="states.inputValue"
                :aria-expanded="expanded"
                :disabled="selectDisabled"
                role="combobox"
                type="text"
                @input="onInput"
                @compositionstart="handleCompositionStart"
                @compositionupdate="handleCompositionUpdate"
                @compositionend="handleCompositionEnd"
                @keydown.up.stop.prevent="onKeyboardNavigate('backward')"
                @keydown.down.stop.prevent="onKeyboardNavigate('forward')"
                @keydown.enter.stop.prevent="onKeyboardSelect"
                @keydown.esc.stop.prevent="handleEsc"
                @keydown.delete.stop="handleDel"
                @click.stop="toggleMenu"
              />
              <span
                v-if="filterable"
                ref="calculatorRef"
                aria-hidden="true"
                :class="nsSelect.e('input-calculator')"
                v-text="states.inputValue"
              />
            </div>
          </div>
        </div>
      </template>
      <template #content>
        <el-select-menu :id="'content'" ref="menuRef" :data="filteredOptions">
          <template v-if="$slots.header" #header>
            <div :class="nsSelect.be('dropdown', 'header')" @click.stop>
              <slot name="header" />
            </div>
          </template>
          <template #default="scope">
            <slot v-bind="scope" />
          </template>
          <template v-if="$slots.loading && loading" #loading>
            <div :class="nsSelect.be('dropdown', 'loading')">
              <slot name="loading" />
            </div>
          </template>
          <template v-else-if="loading || filteredOptions.length === 0" #empty>
            <div :class="nsSelect.be('dropdown', 'empty')">
              <slot name="empty">
                <span>{{ emptyText }}</span>
              </slot>
            </div>
          </template>
          <template v-if="$slots.footer" #footer>
            <div :class="nsSelect.be('dropdown', 'footer')" @click.stop>
              <slot name="footer" />
            </div>
          </template>
        </el-select-menu>
      </template>
    </el-tooltip>
  </div>
</template>"#;
        let result = gen_and_validate(input);
        eprintln!("SELECT-V2 len:{}", result.code.len());
    }

    /// Reserved word as filename (switch.vue) must not produce invalid JS.
    #[test]
    fn test_reserved_word_filename_switch() {
        let input = r#"<script setup>
defineOptions({ name: 'ElSwitch' })
const checked = true
</script>
<template>
  <div @click.prevent="switchValue">
    <input type="checkbox" role="switch" />
  </div>
</template>"#;
        let result = gen_and_validate_with_filename(input, "switch.vue");
        eprintln!("SWITCH:\n{}", result.code);
        // __name should not produce bare `switch` keyword
    }

    #[test]
    fn test_root_level_comment_before_script() {
        let input = r#"<!--
 Root-level comment before the script block.
 This should be stripped from the output.
-->
<script lang="ts" setup>
import { computed } from 'vue'
const x = computed(() => 1)
</script>
<template>
  <div>{{ x }}</div>
</template>"#;
        let result = gen_and_validate(input);
        // The comment text should NOT appear in the JS output
        assert!(
            !result.code.contains("Root-level comment"),
            "Comment leaked into output: {}",
            result.code
        );
    }

    #[test]
    fn test_html_entities_decoded_in_attribute_values() {
        // HTML entities like &quot; &amp; &lt; &gt; in attribute values must be
        // decoded to their literal characters in the JS output.
        let input = r#"<script setup>
const x = 1
</script>
<template>
  <DemoBox :data="{&quot;title&quot;:&quot;hello&quot;,&quot;desc&quot;:&quot;a &amp; b&quot;}">
    <div>content</div>
  </DemoBox>
</template>"#;
        let result = gen_and_validate(input);
        // &quot; should become " and &amp; should become &
        // The output should NOT contain raw HTML entities
        assert!(
            !result.code.contains("&quot;"),
            "Raw &quot; in output:\n{}",
            result.code
        );
        assert!(
            !result.code.contains("&amp;"),
            "Raw &amp; in output:\n{}",
            result.code
        );
    }
}
