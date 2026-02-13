//! Builder functions for the syntax_kai pipeline.
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

use crate::{
    code_transform::{CodeTransform, SourceMapOptions},
    syntax_kai::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxPluginOptions, SyntaxResult},
        plugins::{
            code_gen::{script::ScriptGeneratorPlugin, template::TemplateGeneratorPlugin},
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
}

/// Result of the codegen process.
pub struct CodegenResult {
    /// The generated code (VDOM or Vapor render function).
    pub code: String,
    /// The source map as JSON string.
    pub source_map: String,
    /// The transformed code with inline source map appended.
    pub code_with_source_map: String,
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
/// the (potentially transformed) event to the next plugin.
fn run_pipeline<'a>(
    events: Vec<Event<'a>>,
    plugins: &mut [&mut dyn SyntaxPlugin<'a>],
    ctx: &mut SyntaxPluginContext<'a>,
) {
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

// =============================================================================
// compile — VDOM/Vapor codegen
// =============================================================================

/// Compile a Vue SFC using the syntax_kai pipeline.
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

    let syntax_options = SyntaxPluginOptions::default();
    let mut ctx = SyntaxPluginContext {
        input,
        bytes,
        options: &syntax_options,
    };

    let mut syntax = Syntax::new(false);
    tokenize(bytes, |e| syntax.handle(&e, &ctx));

    let events = syntax.events();

    let component_name = options
        .filename
        .as_ref()
        .map(|f| extract_component_name(f))
        .unwrap_or_else(|| "App".to_string());

    let code_transform = Rc::new(RefCell::new(CodeTransform::new(input, allocator)));

    // codegen plugins
    let mut code_gen_script = ScriptGeneratorPlugin::new(
        Rc::clone(&code_transform),
        &component_name,
        false,
        false,
        false,
    );

    let mut code_gen_template =
        TemplateGeneratorPlugin::new(Rc::clone(&code_transform), options.is_production);

    {
        // transient plugins
        let mut script_ec = ElementCompilerPlugin::new();
        let mut script_oxc = OxcParserPlugin::new(allocator);

        let pipeline: &mut [&mut dyn SyntaxPlugin] = &mut [
            &mut script_ec,
            &mut script_oxc,
            &mut code_gen_script,
            &mut code_gen_template,
        ];

        run_pipeline(events, pipeline, &mut ctx);
    }

    // Flush deferred operations (e.g. batched binding patches) before reading.
    code_gen_template.finalize();

    let code = code_transform.borrow().to_string();

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

    CodegenResult {
        code,
        source_map,
        code_with_source_map,
        duration_ms,
    }
}

// =============================================================================
// compile_with_tsx — TSX codegen
// =============================================================================

/// Generate TSX from a Vue SFC using the syntax_kai pipeline.
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

    #[test]
    #[ignore = "profiling helper — run with --nocapture --ignored"]
    fn profile_pipeline_stages() {
        use crate::syntax_kai::plugin::{
            SyntaxPlugin, SyntaxPluginContext, SyntaxPluginOptions, SyntaxResult,
        };
        use crate::syntax_kai::plugins::code_gen::script::ScriptGeneratorPlugin;
        use crate::syntax_kai::plugins::code_gen::template::TemplateGeneratorPlugin;
        use crate::syntax_kai::plugins::element_compiler::element_compiler::ElementCompilerPlugin;
        use crate::syntax_kai::plugins::oxc_parser::oxc_parser::OxcParserPlugin;
        use crate::syntax_kai::syntax::Syntax;
        use crate::syntax_kai::types::Event;
        use crate::tokenizer::byte::tokenize;
        use std::cell::RefCell;
        use std::rc::Rc;

        let path = format!(
            "{}/benches/fixtures/kitchen-sink.vue",
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
            };
            let mut syntax = Syntax::new(false);
            tokenize(bytes, |e| syntax.handle(&e, &ctx));
            let events = syntax.events();

            let ct = Rc::new(RefCell::new(crate::code_transform::CodeTransform::new(
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
            };
            let mut syntax = Syntax::new(false);
            tokenize(bytes, |e| syntax.handle(&e, &ctx));
            let events = syntax.events();

            let ct = Rc::new(RefCell::new(crate::code_transform::CodeTransform::new(
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
            "{}/benches/fixtures/kitchen-sink.vue",
            env!("CARGO_MANIFEST_DIR")
        );
        let input = std::fs::read_to_string(&path).unwrap();
        let alloc = Allocator::new();

        let syntax_options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext {
            input: &input,
            bytes: input.as_bytes(),
            options: &syntax_options,
        };

        let mut syntax = crate::syntax_kai::syntax::Syntax::new(false);
        crate::tokenizer::byte::tokenize(input.as_bytes(), |e| syntax.handle(&e, &ctx));
        let events = syntax.events();
        let event_count = events.len();

        let code_transform = Rc::new(RefCell::new(CodeTransform::new(&input, &alloc)));

        let component_name = "KitchenSink";
        let mut code_gen_script =
            crate::syntax_kai::plugins::code_gen::script::ScriptGeneratorPlugin::new(
                Rc::clone(&code_transform),
                component_name,
                false,
                false,
                false,
            );
        let mut code_gen_template =
            crate::syntax_kai::plugins::code_gen::template::TemplateGeneratorPlugin::new(
                Rc::clone(&code_transform),
                false,
            );

        {
            let mut ec = crate::syntax_kai::plugins::element_compiler::element_compiler::ElementCompilerPlugin::new();
            let mut oxc =
                crate::syntax_kai::plugins::oxc_parser::oxc_parser::OxcParserPlugin::new(&alloc);
            let mut pipeline: Vec<&mut dyn crate::syntax_kai::plugin::SyntaxPlugin> = vec![
                &mut ec,
                &mut oxc,
                &mut code_gen_script,
                &mut code_gen_template,
            ];
            run_pipeline(events, &mut pipeline, &mut ctx);
        }

        let ct = code_transform.borrow();
        let chunk_count = ct.chunk_count();
        let code = ct.to_string();

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
}
