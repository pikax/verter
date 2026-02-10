//! Builder functions for the syntax_kai pipeline.
//!
//! Two builder functions:
//! - `generate_kai()` — VDOM/Vapor template codegen (production compiler output)
//! - `generate_with_tsx_kai()` — TSX codegen for IDE type checking

use sha2::{Digest, Sha256};

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use crate::{
    cursor::ScriptDetector,
    syntax_kai::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxPluginOptions, SyntaxResult},
        plugins::{
            code_gen_script::code_gen_script::CodeGenScriptPlugin,
            code_gen_template::code_gen_template::VdomTemplateCodegenPlugin,
            code_gen_template_vapor::code_gen_template_vapor::VaporTemplateCodegenPlugin,
            code_gen_tsx::code_gen_tsx::TsxCodegenPlugin, css_parser::css_parser::CssParserPlugin,
            css_style::css_style::CssStylePlugin,
            element_compiler::element_compiler::ElementCompilerPlugin,
            oxc_parser::oxc_parser::OxcParserPlugin,
        },
        syntax::Syntax,
        types::*,
    },
    tokenizer::byte::tokenize,
};

// =============================================================================
// Options and Result Types
// =============================================================================

/// Options for the kai codegen process.
#[derive(Debug, Clone, Default)]
pub struct KaiCodegenOptions {
    /// The filename for source map generation and component name extraction.
    pub filename: Option<String>,
    /// Production mode — affects component ID generation and optimizations.
    pub is_production: bool,
    /// Custom component ID (overrides auto-generation from filename).
    pub component_id: Option<String>,
    /// When true, include the TSX codegen plugin in the pipeline.
    /// When false, the pipeline still runs (producing compiled CSS) but skips TSX generation.
    pub include_tsx: bool,
}

impl KaiCodegenOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }
}

/// Result of the kai codegen process.
pub struct KaiCodegenResult {
    /// The generated code (VDOM or Vapor render function).
    pub code: String,
    /// Time taken in milliseconds.
    pub duration_ms: f64,
    /// Whether Vapor mode was used.
    pub is_vapor: bool,
}

/// Result of the kai TSX codegen process.
pub struct KaiTsxCodegenResult {
    /// The generated TSX code (all blocks: script content + template JSX + commented styles).
    pub tsx: String,
    /// Compiled CSS (from processed style blocks — scoped selectors applied, v-bind replaced).
    pub css: String,
    /// Time taken in milliseconds.
    pub duration_ms: f64,
}

// =============================================================================
// Pipeline runner
// =============================================================================

/// Run events through a sequence of plugins.
///
/// Each event is passed through all plugins in order. If any plugin drops
/// the event, it is removed from the output. Replace and Keep both forward
/// the (potentially transformed) event to the next plugin.
fn run_pipeline<'a>(
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
                    SyntaxResult::Drop => break,
                }
            }
        }
        if let Some(ev) = current {
            result.push(ev);
        }
    }
    result
}

// =============================================================================
// Helpers
// =============================================================================

/// SHA-256 hash → 8 hex chars (first 4 bytes).
fn get_hash(text: &str) -> String {
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

/// Quick byte scan for `<style ... scoped ...>`.
fn has_scoped_style(source: &[u8]) -> bool {
    let style_tag = b"<style";
    let scoped = b"scoped";
    let close = b">";

    let mut pos = 0;
    while pos + style_tag.len() < source.len() {
        if let Some(style_start) = find_bytes(&source[pos..], style_tag) {
            let style_pos = pos + style_start;
            if let Some(close_offset) = find_bytes(&source[style_pos..], close) {
                let tag_content = &source[style_pos..style_pos + close_offset];
                if find_bytes(tag_content, scoped).is_some() {
                    return true;
                }
                pos = style_pos + close_offset + 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    false
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Compute scope_id as 8 hex chars from component name.
fn compute_scope_id(component_name: &str) -> [u8; 8] {
    let hash = get_hash(component_name);
    let hash_bytes = hash.as_bytes();
    let mut scope_id = [0u8; 8];
    scope_id.copy_from_slice(&hash_bytes[..8.min(hash_bytes.len())]);
    scope_id
}

/// Extract ScriptBindings event from pipeline output (if any).
fn extract_script_bindings<'a>(
    events: &[Event<'a>],
) -> Option<crate::syntax_kai::binding_types::BindingMetadata> {
    for event in events {
        if let Event::ScriptBindings(ref metadata) = event {
            return Some(metadata.clone());
        }
    }
    None
}

/// Detect if any template start event has the vapor attribute set.
fn detect_vapor<'a>(events: &[Event<'a>]) -> bool {
    for event in events {
        if let Event::CompiledTemplateStart(ref start) = event {
            return start.vapor.is_some();
        }
    }
    false
}

// =============================================================================
// generate_kai — VDOM/Vapor codegen
// =============================================================================

/// Generate code from a Vue SFC using the syntax_kai pipeline.
///
/// Pipeline:
/// 1. Tokenize input
/// 2. Syntax → events + root_script_events
/// 3. Script pipeline: element_compiler → oxc_parser → code_gen_script
/// 4. Detect vapor mode from CompiledTemplateStart
/// 5. Template pipeline: element_compiler → css_style → oxc_parser → code_gen_template
/// 6. Return generated code
pub fn generate_kai(
    input: &str,
    options: &KaiCodegenOptions,
    allocator: &oxc_allocator::Allocator,
) -> KaiCodegenResult {
    let start = Instant::now();
    let bytes = input.as_bytes();

    let syntax_options = SyntaxPluginOptions::default();
    let mut ctx = SyntaxPluginContext {
        input,
        bytes,
        options: &syntax_options,
    };

    // 1. Tokenize and run Syntax to produce events
    let mut tokenizer_events = Vec::new();
    tokenize(bytes, |e| tokenizer_events.push(e));

    let mut events_storage: Vec<Event<'_>> = Vec::new();
    let ptr = &mut events_storage as *mut Vec<Event<'_>>;
    let root_script_events;
    {
        // SAFETY: Decouples the mutable borrow lifetime from the Event lifetime.
        // Syntax writes into the vec during handle() calls, then is dropped at scope end.
        let mut syntax = Syntax::new(unsafe { &mut *ptr }, false);
        for event in &tokenizer_events {
            syntax.handle(event, &mut ctx);
        }
        root_script_events = syntax.take_root_script_events();
    }
    let events = events_storage;

    // Detect script language for oxc parser
    let script_detector = ScriptDetector::new();
    let detected = script_detector.detect(bytes);

    // 2. Compute scope_id if needed
    let component_name = options
        .filename
        .as_ref()
        .map(|f| extract_component_name(f))
        .unwrap_or_else(|| "App".to_string());

    let scope_id = if has_scoped_style(bytes) {
        Some(compute_scope_id(&component_name))
    } else {
        None
    };
    let component_id = compute_scope_id(&component_name);

    // 3. Script pipeline: element_compiler → oxc_parser → code_gen_script
    let mut script_ec = ElementCompilerPlugin::new();
    let mut script_oxc = OxcParserPlugin::new(allocator);
    script_oxc.set_source_type(detected.language.to_source_type());
    let mut code_gen_script = CodeGenScriptPlugin::new();

    let script_output = run_pipeline(
        root_script_events,
        &mut [&mut script_ec, &mut script_oxc, &mut code_gen_script],
        &mut ctx,
    );

    // Extract binding metadata from script pipeline
    let _binding_metadata = extract_script_bindings(&script_output);

    // 4. Detect vapor mode from template events
    // The element_compiler in the template pipeline will produce CompiledTemplateStart,
    // but we need to run it first. Let's run element_compiler on events first to detect.
    let mut template_ec = ElementCompilerPlugin::new();
    let events_after_ec = run_pipeline(events, &mut [&mut template_ec], &mut ctx);

    let is_vapor = detect_vapor(&events_after_ec);

    // 5. Prepend ScriptBindings to template events
    let mut template_events = Vec::with_capacity(script_output.len() + events_after_ec.len());
    for event in &script_output {
        if let Event::ScriptBindings(ref meta) = event {
            template_events.push(Event::ScriptBindings(meta.clone()));
        }
    }
    template_events.extend(events_after_ec);

    // 6. Template pipeline: css_parser → css_style → oxc_parser → code_gen_template (VDOM or Vapor)
    let mut css_parser = CssParserPlugin::new();
    let mut css_style = CssStylePlugin::new(allocator);
    if let Some(sid) = scope_id {
        css_style.set_scope_id(sid);
    }
    css_style.set_component_id(component_id);

    let mut template_oxc = OxcParserPlugin::new(allocator);
    template_oxc.set_source_type(detected.language.to_source_type());

    let code = if is_vapor {
        let mut vapor_codegen = VaporTemplateCodegenPlugin::new();
        if let Some(sid) = scope_id {
            vapor_codegen.set_scope_id(sid);
        }

        let _output = run_pipeline(
            template_events,
            &mut [
                &mut css_parser,
                &mut css_style,
                &mut template_oxc,
                &mut vapor_codegen,
            ],
            &mut ctx,
        );

        vapor_codegen.take_output()
    } else {
        let mut vdom_codegen = VdomTemplateCodegenPlugin::new();
        if let Some(sid) = scope_id {
            vdom_codegen.set_scope_id(sid);
        }

        let _output = run_pipeline(
            template_events,
            &mut [
                &mut css_parser,
                &mut css_style,
                &mut template_oxc,
                &mut vdom_codegen,
            ],
            &mut ctx,
        );

        vdom_codegen.take_output()
    };

    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

    KaiCodegenResult {
        code,
        duration_ms,
        is_vapor,
    }
}

// =============================================================================
// generate_with_tsx_kai — TSX codegen
// =============================================================================

/// Generate TSX from a Vue SFC using the syntax_kai pipeline.
///
/// Pipeline:
/// 1. Tokenize input
/// 2. Syntax → events + root_script_events
/// 3. Script pipeline: element_compiler → oxc_parser → code_gen_script
/// 4. Template pipeline: element_compiler → css_style → oxc_parser → code_gen_tsx
/// 5. Return generated TSX
pub fn generate_with_tsx_kai(
    input: &str,
    options: &KaiCodegenOptions,
    allocator: &oxc_allocator::Allocator,
) -> KaiTsxCodegenResult {
    let start = Instant::now();
    let bytes = input.as_bytes();

    let syntax_options = SyntaxPluginOptions::default();
    let mut ctx = SyntaxPluginContext {
        input,
        bytes,
        options: &syntax_options,
    };

    // 1. Tokenize
    let mut tokenizer_events = Vec::new();
    tokenize(bytes, |e| tokenizer_events.push(e));

    let mut events_storage: Vec<Event<'_>> = Vec::new();
    let ptr = &mut events_storage as *mut Vec<Event<'_>>;
    let root_script_events;
    {
        let mut syntax = Syntax::new(unsafe { &mut *ptr }, false);
        for event in &tokenizer_events {
            syntax.handle(event, &mut ctx);
        }
        root_script_events = syntax.take_root_script_events();
    }
    let events = events_storage;

    let script_detector = ScriptDetector::new();
    let detected = script_detector.detect(bytes);

    // 2. Compute IDs
    let component_name = options
        .filename
        .as_ref()
        .map(|f| extract_component_name(f))
        .unwrap_or_else(|| "App".to_string());

    let scope_id = if has_scoped_style(bytes) {
        Some(compute_scope_id(&component_name))
    } else {
        None
    };
    let component_id = compute_scope_id(&component_name);

    // 3. Script pipeline
    let mut script_ec = ElementCompilerPlugin::new();
    let mut script_oxc = OxcParserPlugin::new(allocator);
    script_oxc.set_source_type(detected.language.to_source_type());
    let mut code_gen_script = CodeGenScriptPlugin::new();

    let script_output = run_pipeline(
        root_script_events,
        &mut [&mut script_ec, &mut script_oxc, &mut code_gen_script],
        &mut ctx,
    );

    // 4. Extract script block content from script pipeline output events
    let mut script_content = String::new();
    for event in &script_output {
        if let Event::CompiledScriptStart(ref start_ev) = event {
            // Comment the script open tag
            let tag_bytes =
                &bytes[start_ev.tag_open.start as usize..start_ev.tag_open.end as usize];
            script_content.push_str("// ");
            script_content.push_str(&String::from_utf8_lossy(tag_bytes));
            script_content.push('\n');
        }
        if let Event::CompiledScriptEnd(ref end_ev) = event {
            // Include the script content as-is
            if let Some(content_span) = end_ev.content {
                let content_bytes = &bytes[content_span.start as usize..content_span.end as usize];
                let content_str = String::from_utf8_lossy(content_bytes);
                // Trim leading/trailing newlines but preserve internal formatting
                let trimmed = content_str.trim();
                if !trimmed.is_empty() {
                    script_content.push_str(trimmed);
                    script_content.push('\n');
                }
            }
            // Comment the script close tag
            if let Some(close_span) = end_ev.tag_close {
                let close_bytes = &bytes[close_span.start as usize..close_span.end as usize];
                script_content.push_str("// ");
                script_content.push_str(&String::from_utf8_lossy(close_bytes));
                script_content.push('\n');
            }
        }
    }

    // 5. Template pipeline: element_compiler → css_style → oxc_parser → code_gen_tsx
    let mut template_ec = ElementCompilerPlugin::new();
    let events_after_ec = run_pipeline(events, &mut [&mut template_ec], &mut ctx);

    // Prepend ScriptBindings
    let mut template_events = Vec::with_capacity(script_output.len() + events_after_ec.len());
    for event in &script_output {
        if let Event::ScriptBindings(ref meta) = event {
            template_events.push(Event::ScriptBindings(meta.clone()));
        }
    }
    template_events.extend(events_after_ec);

    let mut css_parser = CssParserPlugin::new();
    let mut css_style = CssStylePlugin::new(allocator);
    if let Some(sid) = scope_id {
        css_style.set_scope_id(sid);
    }
    css_style.set_component_id(component_id);

    let mut template_oxc = OxcParserPlugin::new(allocator);
    template_oxc.set_source_type(detected.language.to_source_type());

    // Conditionally include the TSX codegen plugin
    let mut tsx_codegen = TsxCodegenPlugin::new();
    let pipeline_output = if options.include_tsx {
        run_pipeline(
            template_events,
            &mut [
                &mut css_parser,
                &mut css_style,
                &mut template_oxc,
                &mut tsx_codegen,
            ],
            &mut ctx,
        )
    } else {
        // Run pipeline without TSX plugin — still produces CSS from ProcessedStyle events
        run_pipeline(
            template_events,
            &mut [&mut css_parser, &mut css_style, &mut template_oxc],
            &mut ctx,
        )
    };

    // 6. Extract compiled CSS from ProcessedStyle events
    let mut css = String::new();
    for event in &pipeline_output {
        if let Event::ProcessedStyle(ref ps) = event {
            if let Some(ref transformed) = ps.transformed_css {
                if !css.is_empty() {
                    css.push('\n');
                }
                css.push_str(&String::from_utf8_lossy(transformed));
            } else if let Some(content_span) = ps.compiled_end.content {
                // No transformation (plain unscoped style) — use raw content
                if !css.is_empty() {
                    css.push('\n');
                }
                let raw = &bytes[content_span.start as usize..content_span.end as usize];
                css.push_str(&String::from_utf8_lossy(raw));
            }
        }
    }

    // 7. Combine: script content + template TSX (which includes commented styles)
    let tsx = if options.include_tsx {
        let template_tsx = tsx_codegen.take_output();
        let mut tsx = String::with_capacity(script_content.len() + template_tsx.len() + 1);
        if !script_content.is_empty() {
            tsx.push_str(&script_content);
            tsx.push('\n');
        }
        tsx.push_str(&template_tsx);
        tsx
    } else {
        String::new()
    };

    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

    KaiTsxCodegenResult {
        tsx,
        css,
        duration_ms,
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;

    // ==================== generate_kai ====================

    #[test]
    fn test_full_pipeline_simple() {
        let input = r#"<template><div>hello</div></template>"#;
        let allocator = Allocator::new();
        let options = KaiCodegenOptions::new().with_filename("test.vue");

        let result = generate_kai(input, &options, &allocator);

        assert!(!result.code.is_empty(), "Should produce some output");
        assert!(!result.is_vapor, "Default should be VDOM mode");
        assert!(result.duration_ms >= 0.0);
    }

    #[test]
    fn test_pipeline_interpolation() {
        let input = r#"<template><div>{{ msg }}</div></template>"#;
        let allocator = Allocator::new();
        let options = KaiCodegenOptions::new();

        let result = generate_kai(input, &options, &allocator);

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
        let options = KaiCodegenOptions::new();

        let result = generate_kai(input, &options, &allocator);

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
        let options = KaiCodegenOptions::new();

        let result = generate_kai(input, &options, &allocator);

        assert!(!result.is_vapor, "Should default to VDOM");
        assert!(
            result.code.contains("_createElementVNode"),
            "VDOM should use _createElementVNode, got: {}",
            result.code
        );
    }

    #[test]
    fn test_pipeline_with_scoped_style() {
        let input = r#"<template><div class="box">hi</div></template>
<style scoped>.box { color: red; }</style>"#;

        let allocator = Allocator::new();
        let options = KaiCodegenOptions::new().with_filename("Scoped.vue");

        let result = generate_kai(input, &options, &allocator);

        assert!(!result.code.is_empty(), "Should produce output");
    }

    #[test]
    fn test_pipeline_empty_template() {
        let input = r#"<template></template>"#;
        let allocator = Allocator::new();
        let options = KaiCodegenOptions::new();

        let result = generate_kai(input, &options, &allocator);

        // Should not crash, output can be empty
        assert!(result.duration_ms >= 0.0);
    }

    #[test]
    fn test_pipeline_script_only() {
        let input = r#"<script setup>
const x = 'hello'
</script>"#;
        let allocator = Allocator::new();
        let options = KaiCodegenOptions::new();

        let result = generate_kai(input, &options, &allocator);

        // No template, so output may be empty but shouldn't crash
        assert!(result.duration_ms >= 0.0);
    }

    // ==================== generate_with_tsx_kai ====================

    fn tsx_options() -> KaiCodegenOptions {
        KaiCodegenOptions {
            include_tsx: true,
            ..KaiCodegenOptions::new()
        }
    }

    #[test]
    fn test_tsx_pipeline_simple() {
        let input = r#"<template><div>hello</div></template>"#;
        let allocator = Allocator::new();
        let options = tsx_options();

        let result = generate_with_tsx_kai(input, &options, &allocator);

        assert!(!result.tsx.is_empty(), "Should produce TSX output");
        // TSX should use standard JSX syntax
        assert!(
            result.tsx.contains("<div>"),
            "TSX should have standard JSX <div>, got: {}",
            result.tsx
        );
    }

    #[test]
    fn test_tsx_pipeline_interpolation() {
        let input = r#"<template><div>{{ msg }}</div></template>"#;
        let allocator = Allocator::new();
        let options = tsx_options();

        let result = generate_with_tsx_kai(input, &options, &allocator);

        // TSX uses {expr} syntax
        assert!(
            result.tsx.contains("_ctx.msg"),
            "TSX should have _ctx.msg for unbound variable, got: {}",
            result.tsx
        );
    }

    #[test]
    fn test_tsx_pipeline_binding_flow() {
        let input = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#;

        let allocator = Allocator::new();
        let options = tsx_options();

        let result = generate_with_tsx_kai(input, &options, &allocator);

        // Bindings should flow: setup ref in inline mode → count.value or $setup.count
        assert!(
            result.tsx.contains("count"),
            "TSX should reference count variable, got: {}",
            result.tsx
        );
    }

    #[test]
    fn test_tsx_pipeline_with_style() {
        let input = r#"<template><div>hi</div></template>
<style scoped>.box { color: red; }</style>"#;

        let allocator = Allocator::new();
        let mut options = tsx_options();
        options.filename = Some("Styled.vue".to_string());

        let result = generate_with_tsx_kai(input, &options, &allocator);

        // Should produce TSX without crashing
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
    fn test_has_scoped_style_true() {
        assert!(has_scoped_style(
            b"<template></template><style scoped>.a{}</style>"
        ));
    }

    #[test]
    fn test_has_scoped_style_false() {
        assert!(!has_scoped_style(
            b"<template></template><style>.a{}</style>"
        ));
    }

    #[test]
    fn test_compute_scope_id_deterministic() {
        let id1 = compute_scope_id("App");
        let id2 = compute_scope_id("App");
        assert_eq!(id1, id2);

        let id3 = compute_scope_id("Other");
        assert_ne!(id1, id3);
    }
}
