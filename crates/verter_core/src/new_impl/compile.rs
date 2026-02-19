//! Orchestrator for the AST-based (`new_impl`) compilation pipeline.
//!
//! Drives the full SFC → JS compilation:
//!   1. Tokenize → `Syntax` (parse SFC structure + template AST)
//!   2. Style codegen (v-bind scan + `process_style`)
//!   3. Script codegen (macros, bindings, imports)
//!   4. Template codegen (VDOM or Vapor render function)
//!   5. Assemble results

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use oxc_allocator::Allocator;
use oxc_span::SourceType;

use crate::builder::codegen::{
    compute_scope_id, convert_diagnostics, extract_component_name, remove_inter_block_gaps,
    CodegenOptions, CompileDiagnostic,
};
use crate::code_transform::{CodeTransform, SourceMapOptions};
use crate::css::{process_style, types::ProcessStyleOptions};
use crate::new_impl::script::{generate_script, ScriptCodeGenOptions};
use crate::new_impl::style::generate_style;
use crate::new_impl::syntax::types::StyleLang;
use crate::new_impl::syntax::Syntax;
use crate::new_impl::template::code_gen::{generate_template, CodeGenMode, TemplateCodeGenOptions};
use crate::new_impl::template::oxc::parse_template_expressions;
use crate::syntax::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
use crate::tokenizer::byte::{tokenize, tokenize_with_delimiters};

/// Options specific to the `new_impl` orchestrator (on top of shared [`CodegenOptions`]).
#[derive(Default)]
pub struct VerterCompileOptions {
    /// When true, force Vapor mode output regardless of template attributes,
    /// and implicitly treat script as `<script setup>`.
    pub force_vapor: bool,
    /// When true, strip remaining TypeScript syntax (type annotations, generics)
    /// from script output to produce valid JavaScript.
    pub strip_ts: bool,
    /// When true, generate a source map for the template output.
    pub source_map: bool,
}

// ── Result types ───────────────────────────────────────────────────

pub struct VerterCompileResult {
    pub script: Option<VerterScriptBlock>,
    pub template: Option<VerterTemplateBlock>,
    pub styles: Vec<VerterStyleBlock>,
    pub custom_blocks: Vec<VerterCustomBlock>,
    pub scope_id: String,
    pub errors: Vec<CompileDiagnostic>,
    pub parse_duration_ms: f64,
    pub total_duration_ms: f64,
}

pub struct VerterScriptBlock {
    pub code: String,
    pub duration_ms: f64,
    pub source_map: String,
    pub setup: bool,
    pub attrs: Vec<(String, String)>,
}

pub struct VerterTemplateBlock {
    pub code: String,
    pub source_map: String,
    pub imports: Vec<&'static str>,
    pub duration_ms: f64,
    pub attrs: Vec<(String, String)>,
}

pub struct VerterStyleBlock {
    pub code: String,
    pub scoped: bool,
    pub lang: Option<String>,
    pub duration_ms: f64,
    pub attrs: Vec<(String, String)>,
}

pub struct VerterCustomBlock {
    pub block_type: String,
    pub content: String,
    pub attrs: Vec<(String, String)>,
}

// ── Orchestrator ───────────────────────────────────────────────────

pub fn compile(
    input: &str,
    options: &CodegenOptions,
    verter_options: &VerterCompileOptions,
    allocator: &Allocator,
) -> VerterCompileResult {
    let total_start = Instant::now();
    let bytes = input.as_bytes();

    // ── 1. Parse ──────────────────────────────────────────────────
    let parse_start = Instant::now();

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
    let ctx = SyntaxPluginContext {
        input,
        bytes,
        options: &syntax_options,
        diagnostics: Vec::new(),
    };

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

    let parse_duration_ms = parse_start.elapsed().as_secs_f64() * 1000.0;

    // Collect diagnostics from parse phase
    let mut all_diagnostics = syntax.take_diagnostics();

    // Early return on fatal errors
    if syntax.has_errors() {
        let total_duration_ms = total_start.elapsed().as_secs_f64() * 1000.0;
        return VerterCompileResult {
            script: None,
            template: None,
            styles: Vec::new(),
            custom_blocks: Vec::new(),
            scope_id: String::new(),
            errors: convert_diagnostics(&all_diagnostics),
            parse_duration_ms,
            total_duration_ms,
        };
    }

    // ── 2. Extract metadata ───────────────────────────────────────
    let component_name = options
        .filename
        .as_ref()
        .map(|f| extract_component_name(f))
        .unwrap_or_else(|| "App".to_string());

    let scope_id_bytes = if let Some(ref id) = options.component_id {
        let mut b = [b'0'; 8];
        let id_bytes = id.as_bytes();
        let len = id_bytes.len().min(8);
        b[..len].copy_from_slice(&id_bytes[..len]);
        b
    } else {
        compute_scope_id(&component_name)
    };
    let scope_id_str = std::str::from_utf8(&scope_id_bytes).unwrap_or("00000000");

    let use_vapor = verter_options.force_vapor || syntax.is_vapor();
    let has_scoped_style = syntax.has_style_scope();

    // Collect block ranges for inter-block gap removal
    let block_ranges = extract_block_ranges(&syntax, input);

    // Collect custom blocks before taking template ast
    let custom_blocks: Vec<VerterCustomBlock> = syntax
        .unknown_nodes()
        .iter()
        .map(|node| {
            let tag_name = &input[node.tag_open.start as usize..node.tag_open.name_end as usize];
            // Extract tag name (skip the '<')
            let block_type = tag_name.strip_prefix('<').unwrap_or(tag_name).to_string();
            let content = node
                .content
                .map(|span| input[span.start as usize..span.end as usize].to_string())
                .unwrap_or_default();
            let attrs = extract_attrs(&node.attributes, input);
            VerterCustomBlock {
                block_type,
                content,
                attrs,
            }
        })
        .collect();

    // ── 3. Style codegen ──────────────────────────────────────────
    let mut all_v_bind_vars = Vec::new();
    let mut style_blocks: Vec<VerterStyleBlock> = Vec::new();

    for style in syntax.style_nodes() {
        let style_start = Instant::now();
        let style_result = generate_style(style, input, allocator, scope_id_str);
        all_v_bind_vars.extend(style_result.v_bind_vars);

        // Apply v-bind overwrites to the style content
        let style_code = if let Some(content) = &style.content {
            let style_source = &input[content.start as usize..content.end as usize];
            let style_alloc = Allocator::new();
            let mut style_ct = CodeTransform::new(style_source, &style_alloc);

            // Apply v-bind overwrites (need to shift positions relative to content start)
            for (start, end, replacement) in &style_result.out.overwrites {
                let rel_start = start - content.start;
                let rel_end = end - content.start;
                let replacement = style_alloc.alloc_str(replacement);
                style_ct.overwrite(rel_start, rel_end, replacement);
            }
            let modified_css = style_ct.build_string();

            // Run CSS processing (scoped, modules)
            if matches!(style.lang, None | Some(StyleLang::Css)) {
                let process_opts = ProcessStyleOptions {
                    scope_id: scope_id_str,
                    scoped: style.scoped,
                    is_module: style.module,
                    filename: options.filename.as_deref(),
                    sourcemap: false,
                };
                match process_style(&modified_css, &process_opts) {
                    Ok(result) => result.code,
                    Err(e) => {
                        all_diagnostics.push(crate::syntax::plugin::Diagnostic {
                            severity: crate::syntax::plugin::DiagnosticSeverity::Error,
                            code: crate::syntax::plugin::CompilerErrorCode::XMissingEndTag,
                            plugin: "style",
                            message: e,
                            span: None,
                        });
                        modified_css
                    }
                }
            } else {
                modified_css
            }
        } else {
            String::new()
        };

        let style_duration_ms = style_start.elapsed().as_secs_f64() * 1000.0;
        let lang_str = style.lang.map(|l| match l {
            StyleLang::Css => "css".to_string(),
            StyleLang::Scss => "scss".to_string(),
            StyleLang::Sass => "sass".to_string(),
            StyleLang::Less => "less".to_string(),
            StyleLang::Stylus => "stylus".to_string(),
            StyleLang::Unknown => "unknown".to_string(),
        });

        style_blocks.push(VerterStyleBlock {
            code: style_code,
            scoped: style.scoped,
            lang: lang_str,
            duration_ms: style_duration_ms,
            attrs: extract_attrs(&style.attributes, input),
        });
    }

    // ── 4. Script codegen ─────────────────────────────────────────
    let script_start = Instant::now();

    let mut ct = CodeTransform::new(input, allocator);

    let script_options = ScriptCodeGenOptions {
        component_name: &component_name,
        scope_id: scope_id_str,
        keep_ts_types: !verter_options.strip_ts,
        is_production: options.is_production,
        inline_template: false,
        is_vapor: use_vapor,
        has_scoped_style,
        runtime_module_name: options.runtime_module_name.as_deref().unwrap_or("vue"),
        css_v_binds: &all_v_bind_vars,
    };

    let script_result = generate_script(
        syntax.script(),
        syntax.script_setup(),
        input,
        &mut ct,
        allocator,
        &script_options,
    );

    // Remove template and style blocks from script output
    if let Some(template_ast) = syntax.template_ast() {
        let root = &template_ast.root;
        let tpl_start = root.tag_open.start;
        let tpl_end = root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(root.tag_open.end);
        ct.remove(tpl_start, tpl_end);
    }

    for style in syntax.style_nodes() {
        let s_start = style.tag_open.start;
        let s_end = style
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(style.tag_open.end);
        ct.remove(s_start, s_end);
    }

    for node in syntax.unknown_nodes() {
        let s_start = node.tag_open.start;
        let s_end = node
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(node.tag_open.end);
        ct.remove(s_start, s_end);
    }

    // When <script setup> exists, remove the companion <script> block.
    // Its type exports are not used at runtime — the setup wrapper is the
    // sole script output.
    if syntax.script_setup().is_some() {
        if let Some(script) = syntax.script() {
            let s_start = script.tag_open.start;
            let s_end = script
                .tag_close
                .as_ref()
                .map(|tc| tc.end)
                .unwrap_or(script.tag_open.end);
            ct.remove(s_start, s_end);
        }
    }

    // Remove inter-block gaps
    remove_inter_block_gaps(&mut ct, input.len() as u32, &block_ranges);

    // Strip remaining TypeScript syntax if requested
    if verter_options.strip_ts {
        // Parse the script content with OXC and strip type annotations
        if let Some(script_setup) = syntax.script_setup() {
            if let Some(content) = &script_setup.content {
                let script_source = &input[content.start as usize..content.end as usize];
                let strip_alloc = Allocator::new();
                let source_type = SourceType::tsx();
                let parser = oxc_parser::Parser::new(&strip_alloc, script_source, source_type);
                let parse_result = parser.parse();
                crate::strip_types::typescript::strip_typescript_types(
                    &parse_result.program,
                    &mut ct,
                    content.start,
                    script_source,
                );
            }
        }
        if let Some(script) = syntax.script() {
            if let Some(content) = &script.content {
                let script_source = &input[content.start as usize..content.end as usize];
                let strip_alloc = Allocator::new();
                let source_type = SourceType::tsx();
                let parser = oxc_parser::Parser::new(&strip_alloc, script_source, source_type);
                let parse_result = parser.parse();
                crate::strip_types::typescript::strip_typescript_types(
                    &parse_result.program,
                    &mut ct,
                    content.start,
                    script_source,
                );
            }
        }
    }

    // Emit imports from script codegen
    if !script_result.imports.is_empty() {
        let runtime = options.runtime_module_name.as_deref().unwrap_or("vue");
        let specifiers: Vec<String> = script_result
            .imports
            .iter()
            .map(|name| format_import_specifier(name))
            .collect();
        let import_line = format!(
            "import {{ {} }} from \"{}\"\n",
            specifiers.join(", "),
            runtime,
        );
        ct.prepend(&import_line);
    }

    let script_code = ct.build_string();
    let script_source_map = if verter_options.source_map {
        let sm_opts = SourceMapOptions {
            source: options.filename.as_deref(),
            file: options.filename.as_deref(),
            include_content: true,
        };
        ct.generate_map_json(sm_opts)
    } else {
        String::new()
    };
    let script_duration_ms = script_start.elapsed().as_secs_f64() * 1000.0;

    let has_script_setup = syntax.script_setup().is_some();
    let script_attrs = if let Some(ss) = syntax.script_setup() {
        extract_attrs(&ss.attributes, input)
    } else if let Some(s) = syntax.script() {
        extract_attrs(&s.attributes, input)
    } else {
        Vec::new()
    };

    let script_block = if syntax.script().is_some() || syntax.script_setup().is_some() {
        Some(VerterScriptBlock {
            code: script_code,
            duration_ms: script_duration_ms,
            source_map: script_source_map,
            setup: has_script_setup,
            attrs: script_attrs,
        })
    } else {
        None
    };

    // ── 5. Template codegen ───────────────────────────────────────
    let template_block = if let Some(template_ast) = syntax.take_template_ast() {
        // Skip codegen for non-HTML template languages (e.g. Pug).
        // The AST positions are from the raw source and don't represent HTML.
        let is_non_html_lang = template_ast.root.lang.as_ref().is_some_and(|span| {
            let lang_val = &input[span.start as usize..span.end as usize];
            !lang_val.is_empty() && lang_val != "html"
        });
        if is_non_html_lang {
            None
        } else {
            let tpl_start = Instant::now();

            let source_type = SourceType::tsx();
            let oxc_ast = parse_template_expressions(&template_ast, input, allocator, source_type);

            let tpl_alloc = Allocator::new();
            // Use the full SFC input so AST positions (which are absolute) align correctly.
            // After codegen we slice out only the template region from the result.
            let mut tpl_ct = CodeTransform::new(input, &tpl_alloc);
            let tpl_tag_start = template_ast.root.tag_open.start as usize;
            let tpl_tag_end = template_ast
                .root
                .tag_close
                .as_ref()
                .map(|tc| tc.end as usize)
                .unwrap_or(
                    template_ast
                        .root
                        .content
                        .as_ref()
                        .map(|c| c.end as usize)
                        .unwrap_or(template_ast.root.tag_open.end as usize),
                );

            let tpl_options = TemplateCodeGenOptions {
                mode: if use_vapor {
                    CodeGenMode::Vapor
                } else {
                    CodeGenMode::Vdom
                },
                is_inline: false,
                is_production: options.is_production,
                comments: options.comments.unwrap_or(!options.is_production),
            };

            let imports = generate_template(
                &template_ast,
                &oxc_ast,
                input,
                &mut tpl_ct,
                &tpl_alloc,
                script_result.bindings,
                &tpl_options,
            );

            // The full output includes unchanged prefix (before <template>) and suffix
            // (after </template>). Slice out only the transformed template region.
            let full_output = tpl_ct.build_string();
            let suffix_len = input.len() - tpl_tag_end;
            let tpl_code = full_output[tpl_tag_start..full_output.len() - suffix_len].to_string();
            let tpl_source_map = if verter_options.source_map {
                let sm_opts = SourceMapOptions {
                    source: options.filename.as_deref(),
                    file: options.filename.as_deref(),
                    include_content: true,
                };
                tpl_ct.generate_map_json(sm_opts)
            } else {
                String::new()
            };
            let tpl_duration_ms = tpl_start.elapsed().as_secs_f64() * 1000.0;

            let tpl_attrs = extract_attrs(&template_ast.root.attributes, input);

            Some(VerterTemplateBlock {
                code: tpl_code,
                source_map: tpl_source_map,
                imports,
                duration_ms: tpl_duration_ms,
                attrs: tpl_attrs,
            })
        } // close `else` for is_non_html_lang
    } else {
        None
    };

    // ── 6. Assemble ───────────────────────────────────────────────
    let scope_id_full = if has_scoped_style {
        format!("data-v-{}", scope_id_str)
    } else {
        String::new()
    };

    let total_duration_ms = total_start.elapsed().as_secs_f64() * 1000.0;

    VerterCompileResult {
        script: script_block,
        template: template_block,
        styles: style_blocks,
        custom_blocks,
        scope_id: scope_id_full,
        errors: convert_diagnostics(&all_diagnostics),
        parse_duration_ms,
        total_duration_ms,
    }
}

// ── Helpers ────────────────────────────────────────────────────────

/// Format a runtime import specifier for the `import { ... } from "vue"` line.
///
/// Internal helper names use a `_` prefix (e.g., `_defineComponent`), while Vue
/// exports them without the prefix (`defineComponent`). This function produces
/// the `exportName as _localName` form when needed.
fn format_import_specifier(name: &str) -> String {
    if let Some(stripped) = name.strip_prefix('_') {
        if stripped.is_empty() {
            name.to_string()
        } else {
            format!("{} as {}", stripped, name)
        }
    } else {
        name.to_string()
    }
}

/// Extract SFC block byte ranges from root nodes for inter-block gap removal.
fn extract_block_ranges(syntax: &Syntax, _input: &str) -> Vec<(u32, u32)> {
    let mut ranges = Vec::new();

    // Template
    if let Some(ast) = syntax.template_ast() {
        let start = ast.root.tag_open.start;
        let end = ast
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(ast.root.tag_open.end);
        ranges.push((start, end));
    }

    // Script(s)
    for script in [syntax.script(), syntax.script_setup()]
        .into_iter()
        .flatten()
    {
        let start = script.tag_open.start;
        let end = script
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(script.tag_open.end);
        ranges.push((start, end));
    }

    // Styles
    for style in syntax.style_nodes() {
        let start = style.tag_open.start;
        let end = style
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(style.tag_open.end);
        ranges.push((start, end));
    }

    // Unknown blocks
    for node in syntax.unknown_nodes() {
        let start = node.tag_open.start;
        let end = node
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(node.tag_open.end);
        ranges.push((start, end));
    }

    ranges.sort_by_key(|&(s, _)| s);
    ranges
}

/// Extract attribute key-value pairs from `NodeProp` list.
fn extract_attrs(props: &[crate::new_impl::types::NodeProp], input: &str) -> Vec<(String, String)> {
    props
        .iter()
        .filter(|p| !p.is_directive)
        .map(|p| {
            let name = &input[p.start as usize..p.name_end as usize];
            let value = match (p.value_start, p.value_end) {
                (Some(vs), Some(ve)) => input[vs as usize..ve as usize].to_string(),
                _ => String::new(),
            };
            (name.to_string(), value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile_sfc(source: &str) -> VerterCompileResult {
        let alloc = Allocator::new();
        let options = CodegenOptions {
            filename: Some("App.vue".to_string()),
            ..Default::default()
        };
        let verter_opts = VerterCompileOptions {
            strip_ts: true,
            ..Default::default()
        };
        compile(source, &options, &verter_opts, &alloc)
    }

    #[test]
    fn format_import_specifier_strips_underscore_prefix() {
        assert_eq!(
            format_import_specifier("_defineComponent"),
            "defineComponent as _defineComponent"
        );
        assert_eq!(
            format_import_specifier("_useSlots"),
            "useSlots as _useSlots"
        );
        assert_eq!(
            format_import_specifier("_Fragment"),
            "Fragment as _Fragment"
        );
    }

    #[test]
    fn format_import_specifier_preserves_non_prefixed() {
        assert_eq!(format_import_specifier("vue"), "vue");
        assert_eq!(format_import_specifier("ref"), "ref");
    }

    #[test]
    fn basic_sfc_compiles() {
        let result = compile_sfc(
            r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert!(result.script.is_some());
        assert!(result.template.is_some());
    }

    #[test]
    fn script_imports_use_as_syntax() {
        let result = compile_sfc(
            r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
        );
        let script = result.script.as_ref().expect("script block");
        // The import should use "defineComponent as _defineComponent" syntax
        // because Vue exports "defineComponent" (no underscore prefix).
        assert!(
            script.code.contains("defineComponent as _defineComponent"),
            "Expected 'defineComponent as _defineComponent' in imports, got: {}",
            script.code
        );
        assert!(
            !script.code.contains("import { _defineComponent }"),
            "Should not import bare _defineComponent, got: {}",
            script.code
        );
    }

    #[test]
    fn style_block_extracted() {
        let result = compile_sfc(
            r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>

<style scoped>
.app { color: red; }
</style>
"#,
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.styles.len(), 1);
        assert!(result.styles[0].scoped);
        assert!(!result.scope_id.is_empty());
    }

    #[test]
    fn custom_blocks_extracted() {
        let result = compile_sfc(
            r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>

<i18n lang="json">
{ "en": { "hello": "Hello" } }
</i18n>
"#,
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.custom_blocks.len(), 1);
        assert_eq!(result.custom_blocks[0].block_type, "i18n");
    }

    #[test]
    fn empty_input_no_panic() {
        let result = compile_sfc("");
        // No script or template, but should not panic
        assert!(result.script.is_none());
        assert!(result.template.is_none());
    }

    #[test]
    fn template_output_contains_render_function_vdom() {
        let result = compile_sfc(
            r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let tpl = result.template.as_ref().expect("template block");
        assert!(
            tpl.code.contains("function render("),
            "Expected render function in template output, got: {}",
            tpl.code
        );
        assert!(
            !tpl.code.contains("<div>"),
            "Template output should not contain raw HTML: {}",
            tpl.code
        );
        assert!(
            !tpl.code.contains("<script"),
            "Template output should not contain script tags: {}",
            tpl.code
        );
    }

    #[test]
    fn template_output_contains_render_function_vapor() {
        let alloc = Allocator::new();
        let options = CodegenOptions {
            filename: Some("App.vue".to_string()),
            ..Default::default()
        };
        let verter_opts = VerterCompileOptions {
            strip_ts: true,
            force_vapor: true,
            ..Default::default()
        };
        let result = compile(
            r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
            &options,
            &verter_opts,
            &alloc,
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let tpl = result.template.as_ref().expect("template block");
        assert!(
            tpl.code.contains("function render("),
            "Expected render function in template output, got: {}",
            tpl.code
        );
        assert!(
            tpl.code.contains("_template("),
            "Expected _template() call in vapor output, got: {}",
            tpl.code
        );
        // Vapor legitimately has <div> inside _template("...") string literals,
        // so check there's no raw <div> OUTSIDE of string contexts.
        // A raw <div> would appear as a line starting with `<div>` or after whitespace.
        assert!(
            !tpl.code.contains("<script"),
            "Template output should not contain script tags: {}",
            tpl.code
        );
        assert!(
            !tpl.code.contains("<template>"),
            "Template output should not contain raw template tags: {}",
            tpl.code
        );
    }

    #[test]
    fn scoped_css_no_double_data_v_prefix() {
        let result = compile_sfc(
            r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div class="app">{{ msg }}</div>
</template>

<style scoped>
.app { color: red; }
</style>
"#,
        );
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        assert_eq!(result.styles.len(), 1);
        let css = &result.styles[0].code;
        assert!(
            !css.contains("data-v-data-v-"),
            "CSS should not contain double data-v- prefix: {}",
            css
        );
        assert!(
            css.contains("[data-v-"),
            "CSS should contain scoped attribute selector: {}",
            css
        );
    }

    #[test]
    fn timing_fields_populated() {
        let result = compile_sfc(
            r#"<script setup>
const x = 1
</script>
<template><div>{{ x }}</div></template>
"#,
        );
        assert!(result.parse_duration_ms >= 0.0);
        assert!(result.total_duration_ms >= 0.0);
        if let Some(ref s) = result.script {
            assert!(s.duration_ms >= 0.0);
        }
        if let Some(ref t) = result.template {
            assert!(t.duration_ms >= 0.0);
        }
    }

    /// Compile and assert template output is syntactically valid JS.
    /// Returns the template code string for further assertion.
    fn compile_and_validate_template(source: &str) -> String {
        let result = compile_sfc(source);
        assert!(result.errors.is_empty(), "compile errors: {:?}", result.errors);
        let tpl = result.template.as_ref().expect("template block");
        assert!(!tpl.code.trim().is_empty(), "template code is empty");
        // Parse with OXC to ensure valid JS
        let alloc = Allocator::new();
        let source_type = oxc_span::SourceType::mjs();
        let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
        let parsed = oxc_parser::Parser::new(&alloc, &wrapped, source_type).parse();
        assert!(
            parsed.errors.is_empty(),
            "Template JS parse error: {:?}\n--- generated code ---\n{}",
            parsed.errors.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
            tpl.code
        );
        tpl.code.clone()
    }

    // ==================== v-if / v-else-if / v-else ====================

    #[test]
    fn v_if_only_emits_comment_fallback() {
        let code = compile_and_validate_template(
            r#"<template><div><span v-if="show">yes</span></div></template>"#,
        );
        assert!(
            code.contains("_createCommentVNode"),
            "v-if without v-else should emit comment fallback\n{}",
            code
        );
    }

    #[test]
    fn v_if_v_else_no_comment_fallback() {
        let code = compile_and_validate_template(
            r#"<template><div><span v-if="show">yes</span><span v-else>no</span></div></template>"#,
        );
        // Full chain has v-else, so no comment fallback needed
        assert!(
            !code.contains("_createCommentVNode"),
            "v-if/v-else should not emit comment fallback\n{}",
            code
        );
    }

    #[test]
    fn v_if_v_else_if_no_v_else_emits_comment_fallback() {
        let code = compile_and_validate_template(
            r#"<template><div><span v-if="a">A</span><span v-else-if="b">B</span></div></template>"#,
        );
        assert!(
            code.contains("_createCommentVNode"),
            "v-if/v-else-if without v-else should emit comment fallback\n{}",
            code
        );
    }

    #[test]
    fn v_if_v_else_if_v_else_complete_chain() {
        let code = compile_and_validate_template(
            r#"<template><div><span v-if="a">A</span><span v-else-if="b">B</span><span v-else>C</span></div></template>"#,
        );
        // Complete chain, no comment fallback
        assert!(
            !code.contains("_createCommentVNode"),
            "complete v-if chain should not emit comment fallback\n{}",
            code
        );
    }

    #[test]
    fn v_if_after_sibling_has_comma_separator() {
        let code = compile_and_validate_template(
            r#"<template><div><p>text</p><span v-if="show">conditional</span></div></template>"#,
        );
        // The v-if should be separated from the previous sibling by a comma
        assert!(
            code.contains("_createCommentVNode"),
            "v-if without v-else should have comment fallback\n{}",
            code
        );
    }

    #[test]
    fn v_if_chain_after_sibling() {
        let code = compile_and_validate_template(
            r#"<template><div><p>text</p><span v-if="a">A</span><span v-else-if="b">B</span><span v-else>C</span></div></template>"#,
        );
        // Should produce valid JS with comma before the ternary
        assert!(code.contains("function render("));
    }

    #[test]
    fn v_if_chain_without_v_else_after_sibling() {
        let code = compile_and_validate_template(
            r#"<template><div><p>text</p><span v-if="a">A</span><span v-else-if="b">B</span></div></template>"#,
        );
        assert!(
            code.contains("_createCommentVNode"),
            "incomplete chain after sibling should have comment fallback\n{}",
            code
        );
    }

    #[test]
    fn v_if_as_root_single_child() {
        let code = compile_and_validate_template(
            r#"<template><div v-if="show">hello</div></template>"#,
        );
        assert!(code.contains("return "));
        assert!(
            code.contains("_createCommentVNode"),
            "root v-if should have comment fallback\n{}",
            code
        );
    }

    #[test]
    fn v_if_v_else_as_root() {
        let code = compile_and_validate_template(
            r#"<template><div v-if="show">yes</div><div v-else>no</div></template>"#,
        );
        assert!(code.contains("return "));
    }

    #[test]
    fn v_if_in_multi_root_fragment() {
        let code = compile_and_validate_template(
            r#"<template><p>first</p><div v-if="show">middle</div><p>last</p></template>"#,
        );
        assert!(code.contains("_Fragment"));
        assert!(
            code.contains("_createCommentVNode"),
            "v-if in fragment should have comment fallback\n{}",
            code
        );
    }

    #[test]
    fn multiple_v_if_chains_in_same_parent() {
        let code = compile_and_validate_template(
            r#"<template><div><span v-if="a">A</span><span v-else>notA</span><span v-if="b">B</span><span v-else>notB</span></div></template>"#,
        );
        // Two independent v-if/v-else chains in the same parent
        assert!(code.contains("function render("));
    }

    #[test]
    fn v_if_with_whitespace_between_branches() {
        // Whitespace nodes between v-if/v-else should be skipped
        let code = compile_and_validate_template(
            "<template><div>\n  <span v-if=\"a\">A</span>\n  <span v-else>B</span>\n</div></template>",
        );
        assert!(code.contains("function render("));
    }

    #[test]
    fn v_if_nested_inside_v_for() {
        let code = compile_and_validate_template(
            r#"<template><div><div v-for="item in items" :key="item"><span v-if="item.show">{{ item.name }}</span></div></div></template>"#,
        );
        assert!(code.contains("_renderList"));
        assert!(
            code.contains("_createCommentVNode"),
            "v-if inside v-for should have comment fallback\n{}",
            code
        );
    }

    #[test]
    fn script_attrs_contain_lang() {
        let result = compile_sfc(
            r#"<script setup lang="ts">
const x = 1
</script>
<template><div>{{ x }}</div></template>"#,
        );
        let script = result.script.as_ref().expect("script block");
        eprintln!("attrs: {:?}", script.attrs);
        let lang = script.attrs.iter().find(|(k, _)| k == "lang");
        assert!(
            lang.is_some(),
            "Expected 'lang' in attrs, got: {:?}",
            script.attrs
        );
        assert_eq!(lang.unwrap().1, "ts");
    }
}
