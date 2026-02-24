//! Orchestrator for the AST-based compilation pipeline.
//!
//! Drives the full SFC → JS compilation:
//!   1. Tokenize → `Syntax` (parse SFC structure + template AST)
//!   2. Style codegen (v-bind scan + `process_style`)
//!   3. Script codegen (macros, bindings, imports)
//!   4. Template codegen (VDOM or Vapor render function)
//!   5. Assemble results

mod helpers;
pub mod types;

pub use helpers::*;
pub use types::*;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use oxc_allocator::Allocator;
use oxc_span::SourceType;
use rustc_hash::FxHashSet;

use crate::ast::types::{AstNodeKind, TagType};
use crate::code_transform::{CodeTransform, SourceMapOptions};
use crate::css::{process_style, types::ProcessStyleOptions};
use crate::diagnostics::{
    CompilerErrorCode, Diagnostic, DiagnosticSeverity, SyntaxPluginContext, SyntaxPluginOptions,
};
use crate::parser::types::StyleLang;
use crate::parser::Syntax;
use crate::script::{generate_script, ScriptCodeGenOptions};
use crate::style::generate_style;
use crate::template::code_gen::vdom::element::to_pascal_case;
use crate::template::code_gen::{generate_template, CodeGenMode, TemplateCodeGenOptions};
use crate::template::oxc::parse_template_expressions;
use crate::template::oxc::types::OxcParsedAst;
use crate::tokenizer::byte::{tokenize_sfc, tokenize_sfc_with_delimiters};

use helpers::{extract_attrs, extract_block_ranges};

// ── Orchestrator ───────────────────────────────────────────────────

/// Compile a Vue SFC source string into script, template, and style outputs.
///
/// Drives the full pipeline: tokenize the SFC, generate style CSS (with scoped
/// rewriting and `v-bind()` extraction), generate script JS/TS (macro expansion,
/// bindings, imports), and generate the template render function (VDOM or Vapor).
///
/// The caller-supplied [`Allocator`] is used for the main script `CodeTransform`;
/// template and style codegen create their own short-lived allocators internally.
///
/// Returns a [`VerterCompileResult`] containing the generated code for each block,
/// timing information, and any diagnostics emitted during compilation.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
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
        tokenize_sfc_with_delimiters(
            bytes,
            |e| syntax.handle(&e, &ctx),
            open.as_bytes(),
            close.as_bytes(),
        );
    } else {
        tokenize_sfc(bytes, |e| syntax.handle(&e, &ctx));
    }

    let parse_duration_ms = parse_start.elapsed().as_secs_f64() * 1000.0;

    // Collect diagnostics from parse phase
    let mut all_diagnostics = syntax.take_diagnostics();

    // Note: we continue processing even after parse errors to provide
    // partial results. Script and style blocks may still be valid even
    // when the template has errors, and returning partial results allows
    // the LSP to provide completions and diagnostics for those blocks.
    let has_parse_errors = syntax.has_errors();

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
    let scope_id_full = format!("data-v-{}", scope_id_str);

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
                    module_name: None,
                    filename: options.filename.as_deref(),
                    sourcemap: false,
                };
                match process_style(&modified_css, &process_opts) {
                    Ok(result) => result.code,
                    Err(e) => {
                        all_diagnostics.push(Diagnostic {
                            severity: DiagnosticSeverity::Error,
                            code: CompilerErrorCode::XCssParseError,
                            plugin: "style",
                            message: e.to_string(),
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

    // Parse template expressions early so we can collect the set of identifiers
    // actually used in the template (for import elision in script codegen).
    // This avoids the text-based heuristic and correctly handles TS type positions.
    let source_type = SourceType::tsx();
    let early_oxc_ast: Option<OxcParsedAst<'_>> = if !has_parse_errors {
        syntax.template_ast().map(|template_ast_ref| {
            parse_template_expressions(template_ast_ref, input, allocator, source_type)
        })
    } else {
        None
    };

    let template_used_vars: Option<FxHashSet<String>> = if let (
        Some(ref oxc_ast),
        Some(template_ast_ref),
    ) =
        (&early_oxc_ast, syntax.template_ast())
    {
        let mut vars = FxHashSet::default();

        // 1. Collect identifiers from all expression bindings
        //    (interpolations, v-if conditions, directive values, dynamic args)
        for expr in oxc_ast.iter_expressions() {
            if let Some(ref bindings) = expr.bindings {
                for name in bindings.non_ignored_binding_names() {
                    vars.insert(name.to_string());
                }
            }
        }

        // 2. Collect identifiers from v-for source expressions
        for node_data in &oxc_ast.data {
            if let crate::template::oxc::types::OxcNodeData::Element(el) = node_data {
                if let Some(ref v_for) = el.v_for {
                    for span in &v_for.parsed.references {
                        let name = &input[span.start as usize..span.end as usize];
                        vars.insert(name.to_string());
                    }
                }
            }
        }

        // 3. Collect component tag names from the template AST
        for node in &template_ast_ref.nodes {
            if let AstNodeKind::Element(el) = &node.kind {
                if el.tag_type == TagType::Component {
                    // Tag name is between '<' and name_end
                    let tag_name =
                        &input[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];
                    vars.insert(tag_name.to_string());
                    // Also add PascalCase version for kebab-case tags
                    if tag_name.contains('-') {
                        vars.insert(to_pascal_case(tag_name));
                    }
                }
            }
        }

        Some(vars)
    } else {
        None
    };

    let script_options = ScriptCodeGenOptions {
        component_name: &component_name,
        scope_id: &scope_id_full,
        keep_ts_types: !verter_options.force_js,
        is_production: options.is_production,
        inline_template: false,
        is_vapor: use_vapor,
        has_scoped_style,
        runtime_module_name: options.runtime_module_name.as_deref().unwrap_or("vue"),
        css_v_binds: &all_v_bind_vars,
        external_types: verter_options.external_types.clone(),
        template_used_vars,
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

    // When <script setup> exists, strip the companion <script> tags but
    // keep the content. Named runtime exports (export enum, export const,
    // export function, etc.) must remain in the output so importers can
    // access them. The force_js pass below handles any TS-only constructs.
    if syntax.script_setup().is_some() {
        if let Some(script) = syntax.script() {
            // Remove the <script ...> open tag
            ct.remove(script.tag_open.start, script.tag_open.end);
            // Remove the </script> close tag
            if let Some(tag_close) = &script.tag_close {
                ct.remove(tag_close.start, tag_close.end);
            }
        }
    }

    // Remove inter-block gaps
    remove_inter_block_gaps(&mut ct, input.len() as u32, &block_ranges);

    // Strip remaining TypeScript syntax if requested
    if verter_options.force_js {
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
    } else if has_scoped_style || use_vapor {
        // Template-only component with scoped styles or vapor mode:
        // Emit a synthetic script block so __scopeId / __vapor propagates
        // to consumers (playground, bundler, etc.).
        let mut code = String::with_capacity(128);
        code.push_str("const __sfc__ = {};\n");
        if has_scoped_style {
            code.push_str("__sfc__.__scopeId = \"");
            code.push_str(&scope_id_full);
            code.push_str("\";\n");
        }
        if use_vapor {
            code.push_str("__sfc__.__vapor = true;\n");
        }
        code.push_str("export default __sfc__;\n");
        Some(VerterScriptBlock {
            code,
            duration_ms: script_duration_ms,
            source_map: String::new(),
            setup: false,
            attrs: Vec::new(),
        })
    } else {
        None
    };

    // ── 5. Template codegen ───────────────────────────────────────
    let template_block = if has_parse_errors {
        // Template AST may be invalid after parse errors — skip codegen
        // but continue with script/style results.
        None
    } else if let Some(template_ast) = syntax.take_template_ast() {
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

            // Reuse early-parsed OxcParsedAst if available, otherwise parse now
            let oxc_ast = match early_oxc_ast {
                Some(ast) => ast,
                None => parse_template_expressions(&template_ast, input, allocator, source_type),
            };

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
                force_js: verter_options.force_js,
                self_name: to_pascal_case(&component_name),
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

            // Strip TypeScript syntax from template expressions when force_js is set.
            // Expressions were already parsed with OXC during parse_template_expressions(),
            // so we reuse those ASTs instead of re-parsing.
            if verter_options.force_js {
                for expr in oxc_ast.iter_expressions() {
                    if let Some(ref expression) = expr.expression {
                        crate::strip_types::typescript::strip_typescript_from_expression(
                            expression,
                            &mut tpl_ct,
                            expr.offset,
                            &input[expr.offset as usize..],
                        );
                    }
                }
            }

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
    let scope_id_result = if has_scoped_style {
        scope_id_full.clone()
    } else {
        String::new()
    };

    let total_duration_ms = total_start.elapsed().as_secs_f64() * 1000.0;

    VerterCompileResult {
        script: script_block,
        template: template_block,
        styles: style_blocks,
        custom_blocks,
        scope_id: scope_id_result,
        errors: convert_diagnostics(&all_diagnostics),
        parse_duration_ms,
        total_duration_ms,
    }
}

#[cfg(test)]
#[path = "../compile_tests.rs"]
mod tests;
