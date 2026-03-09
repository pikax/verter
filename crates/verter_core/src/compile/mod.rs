//! Orchestrator for the AST-based compilation pipeline.
//!
//! Drives the full SFC → JS compilation:
//!   1. Tokenize → `Syntax` (parse SFC structure + template AST)
//!   2. Style codegen (v-bind scan + `process_style`) — if `CompileTarget::STYLE`
//!   3. Script codegen (macros, bindings, imports) — if `CompileTarget::needs_script()`
//!   4. Template codegen (VDOM or Vapor render function) — if `CompileTarget::TEMPLATE`
//!   5. TSX codegen (valid JSX for LSP type checking) — if `CompileTarget::TSX`
//!   6. TSC codegen (minimal TS declarations) — if `CompileTarget::TSC`
//!   7. Assemble results
//!
//! Use [`CompileTarget`] bitflags to control which steps run. Presets:
//! - [`CompileTarget::BUNDLER`] — style + script + template (runtime output)
//! - [`CompileTarget::IDE`] — TSX only (LSP type checking, independent of steps 2-4)
//! - [`CompileTarget::ANALYSIS`] — script + template data (MCP static analysis)

mod helpers;
pub mod template_data;
pub mod types;

pub use helpers::*;
pub use template_data::*;
pub use types::*;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashSet;

use crate::ast::types::{AstNodeKind, TagType};
use crate::code_transform::{CodeTransform, SourceMapOptions};
use crate::css::{process_style, types::ProcessStyleOptions};
use crate::diagnostics::{
    CompilerErrorCode, Diagnostic, DiagnosticSeverity, SyntaxPluginContext, SyntaxPluginOptions,
};
use crate::ide;
use crate::parser::types::{ParsedSfc, StyleLang};
use crate::parser::Syntax;
use crate::script::{generate_script, ScriptCodeGenOptions};
use crate::style::generate_style;
use crate::template::code_gen::vdom::element::to_pascal_case;
use crate::template::code_gen::{generate_template, CodeGenMode, TemplateCodeGenOptions};
use crate::template::oxc::parse_template_expressions;
use crate::template::oxc::types::OxcParsedAst;
use crate::tokenizer::byte::{tokenize_sfc, tokenize_sfc_with_delimiters};
use crate::tsc;
use crate::utils::oxc::vue::{
    extract_companion_types, parse_script_with_companion, MacroTypeParams, ResolvedElements,
    RuntimeType, ScriptItem, ScriptMacro, ScriptMode,
};

use helpers::{extract_attrs, extract_block_ranges};

// ── Orchestrator ───────────────────────────────────────────────────

/// Parse an SFC source string into a [`ParsedSfc`].
///
/// Tokenizes in SFC mode with optional custom delimiters and custom element
/// prefixes. The result can be cached and passed to [`compile_from_parsed`]
/// to avoid re-parsing the same source.
pub fn parse_sfc(
    input: &str,
    delimiters: Option<(&str, &str)>,
    custom_elements: Option<&[String]>,
) -> ParsedSfc {
    let bytes = input.as_bytes();

    let syntax_options = if let Some(prefixes) = custom_elements {
        let prefixes = prefixes.to_vec();
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
    if let Some((open, close)) = delimiters {
        tokenize_sfc_with_delimiters(
            bytes,
            |e| syntax.handle(&e, &ctx),
            open.as_bytes(),
            close.as_bytes(),
        );
    } else {
        tokenize_sfc(bytes, |e| syntax.handle(&e, &ctx));
    }

    syntax.into_parsed_sfc()
}

fn is_simple_type_reference(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first == '$' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric())
}

fn collect_imported_type_names<'a>(items: &'a [ScriptItem<'a>]) -> FxHashSet<&'a str> {
    let mut imported = FxHashSet::default();
    for item in items {
        let ScriptItem::Import(import) = item else {
            continue;
        };
        for binding in &import.bindings {
            if import.is_type_only || binding.is_type_only {
                imported.insert(binding.name);
            }
        }
    }
    imported
}

fn props_type_is_object_like(type_params: &MacroTypeParams) -> bool {
    !type_params.resolved.props.is_empty()
        || type_params
            .resolved
            .root_runtime_types
            .iter()
            .any(|ty| matches!(ty, RuntimeType::Object))
}

fn push_invalid_macro_type_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    message: String,
    type_params: &MacroTypeParams,
) {
    diagnostics.push(
        Diagnostic::error_with_message("script", CompilerErrorCode::XInvalidMacroType, message)
            .with_span(type_params.type_span),
    );
}

fn validate_imported_macro_type(
    macro_name: &str,
    type_params: &MacroTypeParams,
    type_text: &str,
    imported_type_names: &FxHashSet<&str>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !is_simple_type_reference(type_text) || !imported_type_names.contains(type_text) {
        return;
    }

    if type_params.unresolved_type_ref {
        push_invalid_macro_type_diagnostic(
            diagnostics,
            format!(
                "{}() type argument '{}' could not be resolved.",
                macro_name, type_text
            ),
            type_params,
        );
        return;
    }

    match macro_name {
        "defineProps" => {
            if !props_type_is_object_like(type_params) {
                push_invalid_macro_type_diagnostic(
                    diagnostics,
                    format!(
                        "defineProps() type argument '{}' must resolve to an object-like props type.",
                        type_text
                    ),
                    type_params,
                );
            }
        }
        "defineEmits" => {
            if type_params.resolved.emits.is_empty() {
                push_invalid_macro_type_diagnostic(
                    diagnostics,
                    format!(
                        "defineEmits() type argument '{}' must resolve to emit call signatures or a named-tuple emits object.",
                        type_text
                    ),
                    type_params,
                );
            }
        }
        _ => {}
    }
}

fn collect_invalid_macro_type_diagnostics(
    input: &str,
    parsed: &ParsedSfc,
    external_types: Option<&rustc_hash::FxHashMap<String, ResolvedElements>>,
) -> Vec<Diagnostic> {
    let Some(script_setup) = parsed.script_setup() else {
        return Vec::new();
    };
    let Some(content_span) = script_setup.content else {
        return Vec::new();
    };

    let content_str = &input[content_span.start as usize..content_span.end as usize];
    let companion_types = if let Some(script) = parsed.script() {
        if let Some(script_content) = script.content {
            let script_source = &input[script_content.start as usize..script_content.end as usize];
            let alloc = Allocator::default();
            let parse_result = Parser::new(&alloc, script_source, SourceType::ts()).parse();
            Some(extract_companion_types(
                &parse_result.program,
                script_source.as_bytes(),
                script_content.start,
            ))
        } else {
            None
        }
    } else {
        None
    };

    let companion_types = match (companion_types, external_types) {
        (Some(mut companion), Some(external)) => {
            for (key, value) in external {
                companion
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
            Some(companion)
        }
        (Some(companion), None) => Some(companion),
        (None, Some(external)) => Some(external.clone()),
        (None, None) => None,
    };

    let alloc = Allocator::default();
    let parse_result = Parser::new(&alloc, content_str, SourceType::ts()).parse();
    let parsed_script = parse_script_with_companion(
        &parse_result.program,
        ScriptMode::Setup,
        0,
        content_str,
        companion_types,
    );
    let imported_type_names = collect_imported_type_names(&parsed_script.items);
    let mut diagnostics = Vec::new();

    for item in &parsed_script.items {
        let ScriptItem::Macro(mac) = item else {
            continue;
        };

        match mac {
            ScriptMacro::DefineProps { type_params, .. } => {
                if let Some(type_params) = type_params {
                    let type_text = content_str
                        [type_params.type_span.start as usize..type_params.type_span.end as usize]
                        .trim();
                    validate_imported_macro_type(
                        "defineProps",
                        type_params,
                        type_text,
                        &imported_type_names,
                        &mut diagnostics,
                    );
                }
            }
            ScriptMacro::WithDefaults {
                define_props_type_params,
                ..
            } => {
                if let Some(type_params) = define_props_type_params {
                    let type_text = content_str
                        [type_params.type_span.start as usize..type_params.type_span.end as usize]
                        .trim();
                    validate_imported_macro_type(
                        "defineProps",
                        type_params,
                        type_text,
                        &imported_type_names,
                        &mut diagnostics,
                    );
                }
            }
            ScriptMacro::DefineEmits { type_params, .. } => {
                if let Some(type_params) = type_params {
                    let type_text = content_str
                        [type_params.type_span.start as usize..type_params.type_span.end as usize]
                        .trim();
                    validate_imported_macro_type(
                        "defineEmits",
                        type_params,
                        type_text,
                        &imported_type_names,
                        &mut diagnostics,
                    );
                }
            }
            _ => {}
        }
    }

    diagnostics
}

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
    let parse_start = Instant::now();
    let parsed = parse_sfc(
        input,
        options
            .delimiters
            .as_ref()
            .map(|(o, c)| (o.as_str(), c.as_str())),
        options.custom_elements.as_deref(),
    );
    let parse_duration_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
    compile_inner(
        input,
        &parsed,
        options,
        verter_options,
        allocator,
        parse_duration_ms,
    )
}

/// Compile a pre-parsed SFC. Skips tokenization and parsing.
///
/// The `parsed` must have been produced from the same `input` string.
/// Parse-affecting options (delimiters, custom_elements) must match those
/// used to create the [`ParsedSfc`] — the caller is responsible for cache
/// key correctness.
pub fn compile_from_parsed(
    input: &str,
    parsed: &ParsedSfc,
    options: &CodegenOptions,
    verter_options: &VerterCompileOptions,
    allocator: &Allocator,
) -> VerterCompileResult {
    compile_inner(input, parsed, options, verter_options, allocator, 0.0)
}

/// Internal compilation driver. Borrows a pre-parsed [`ParsedSfc`] — no cloning
/// of template AST, script nodes, or style nodes.
fn compile_inner(
    input: &str,
    parsed: &ParsedSfc,
    options: &CodegenOptions,
    verter_options: &VerterCompileOptions,
    allocator: &Allocator,
    parse_duration_ms: f64,
) -> VerterCompileResult {
    let total_start = Instant::now();

    // Merge legacy `extract_template_data` flag into the compile target.
    let options = if verter_options.extract_template_data && !options.target.needs_template_data() {
        let mut opts = options.clone();
        opts.target |= CompileTarget::TEMPLATE_DATA;
        std::borrow::Cow::Owned(opts)
    } else {
        std::borrow::Cow::Borrowed(options)
    };
    let options = &*options;

    // Clone diagnostics — this is the only clone needed from ParsedSfc.
    let mut all_diagnostics = parsed.clone_diagnostics();
    let has_parse_errors = parsed.has_errors();
    all_diagnostics.extend(collect_invalid_macro_type_diagnostics(
        input,
        parsed,
        verter_options.external_types.as_ref(),
    ));

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

    let use_vapor = verter_options.force_vapor || parsed.is_vapor();
    let has_scoped_style = parsed.has_style_scope();

    // Collect block ranges for inter-block gap removal
    let block_ranges = extract_block_ranges(parsed, input);

    // Collect custom blocks before taking template ast
    let custom_blocks: Vec<VerterCustomBlock> = parsed
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

    if options.target.needs_style() {
        for style in parsed.style_nodes() {
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
    } // end if needs_style

    // ── 4. Script codegen ─────────────────────────────────────────
    // Script codegen is skipped when the target only needs TSX or TSC,
    // since those paths have their own independent codegen.
    let source_type = SourceType::tsx();
    let mut early_oxc_ast: Option<OxcParsedAst<'_>> = None;
    let mut script_bindings: rustc_hash::FxHashMap<
        &str,
        crate::template::code_gen::binding::BindingType,
    > = rustc_hash::FxHashMap::default();
    let mut script_block: Option<VerterScriptBlock> = None;

    if options.target.needs_script() {
        let script_start = Instant::now();

        let mut ct = CodeTransform::new(input, allocator);

        // Parse template expressions early so we can collect the set of identifiers
        // actually used in the template (for import elision in script codegen).
        // This avoids the text-based heuristic and correctly handles TS type positions.
        early_oxc_ast = if !has_parse_errors {
            parsed.template_ast().map(|template_ast_ref| {
                parse_template_expressions(template_ast_ref, input, allocator, source_type)
            })
        } else {
            None
        };

        let template_used_vars: Option<FxHashSet<String>> =
            if let (Some(ref oxc_ast), Some(template_ast_ref)) =
                (&early_oxc_ast, parsed.template_ast())
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
                            let tag_name = &input
                                [(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];
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
            inline_template: false,
            is_vapor: use_vapor,
            ssr: verter_options.ssr,
            has_scoped_style,
            css_v_binds: &all_v_bind_vars,
            external_types: verter_options.external_types.clone(),
            template_used_vars,
        };

        let script_result = generate_script(
            parsed.script(),
            parsed.script_setup(),
            input,
            &mut ct,
            allocator,
            &script_options,
        );

        // Save bindings for template codegen (borrow/move happens later)
        script_bindings = script_result.bindings;

        // Remove template and style blocks from script output
        if let Some(template_ast) = parsed.template_ast() {
            let root = &template_ast.root;
            let tpl_start = root.tag_open.start;
            let tpl_end = root
                .tag_close
                .as_ref()
                .map(|tc| tc.end)
                .unwrap_or(root.tag_open.end);
            ct.remove(tpl_start, tpl_end);
        }

        for style in parsed.style_nodes() {
            let s_start = style.tag_open.start;
            let s_end = style
                .tag_close
                .as_ref()
                .map(|tc| tc.end)
                .unwrap_or(style.tag_open.end);
            ct.remove(s_start, s_end);
        }

        for node in parsed.unknown_nodes() {
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
        if parsed.script_setup().is_some() {
            if let Some(script) = parsed.script() {
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
            if let Some(script_setup) = parsed.script_setup() {
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
            if let Some(script) = parsed.script() {
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

        let has_script_setup = parsed.script_setup().is_some();
        let script_attrs = if let Some(ss) = parsed.script_setup() {
            extract_attrs(&ss.attributes, input)
        } else if let Some(s) = parsed.script() {
            extract_attrs(&s.attributes, input)
        } else {
            Vec::new()
        };

        script_block = if parsed.script().is_some() || parsed.script_setup().is_some() {
            Some(VerterScriptBlock {
                code: script_code,
                duration_ms: script_duration_ms,
                source_map: script_source_map,
                setup: has_script_setup,
                attrs: script_attrs,
            })
        } else if has_scoped_style || use_vapor || verter_options.ssr {
            // Template-only component with scoped styles, vapor mode, or SSR:
            // Emit a synthetic script block so __scopeId / __vapor / __ssrInlineRender
            // propagates to consumers (playground, bundler, etc.).
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
            if verter_options.ssr {
                code.push_str("__sfc__.__ssrInlineRender = true;\n");
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
    } // end if needs_script

    // ── 5. Template codegen ───────────────────────────────────────
    // Borrow the template AST (it may be needed for both VDOM and TSX codegen).
    let template_ast_opt: Option<&_> = if !has_parse_errors {
        parsed.template_ast()
    } else {
        None
    };

    let needs_tpl_codegen = options.target.needs_template_codegen();
    let needs_tpl_data = options.target.needs_template_data();

    let (template_block, extracted_template_data) =
        if has_parse_errors || (!needs_tpl_codegen && !needs_tpl_data) {
            // Template AST may be invalid after parse errors, or target
            // doesn't need VDOM/template data — skip codegen.
            (None, None)
        } else if let Some(template_ast) = template_ast_opt {
            // Skip codegen for non-HTML template languages (e.g. Pug).
            // The AST positions are from the raw source and don't represent HTML.
            let is_non_html_lang = template_ast.root.lang.as_ref().is_some_and(|span| {
                let lang_val = &input[span.start as usize..span.end as usize];
                !lang_val.is_empty() && lang_val != "html"
            });
            if is_non_html_lang {
                (None, None)
            } else {
                let tpl_start = Instant::now();

                // Reuse early-parsed OxcParsedAst if available, otherwise parse now
                let oxc_ast = match early_oxc_ast {
                    Some(ast) => ast,
                    None => parse_template_expressions(template_ast, input, allocator, source_type),
                };

                // Extract raw template data for cross-file analysis (before bindings are moved)
                let raw_template_data = if needs_tpl_data {
                    Some(template_data::extract_raw_template_data(
                        template_ast,
                        &oxc_ast,
                        input,
                        &script_bindings,
                    ))
                } else {
                    None
                };

                let template_block_inner = if needs_tpl_codegen {
                    let tpl_alloc = Allocator::new();
                    // Use the full SFC input so AST positions (which are absolute) align correctly.
                    // The CT is initialized with the full SFC so AST positions
                    // align. Remove the prefix (before <template>) and suffix
                    // (after </template>) within the CT so build_string() produces
                    // only the template region with correct sourcemap offsets.
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
                    if tpl_tag_start > 0 {
                        tpl_ct.remove(0, tpl_tag_start as u32);
                    }
                    if tpl_tag_end < input.len() {
                        tpl_ct.remove(tpl_tag_end as u32, input.len() as u32);
                    }

                    let tpl_options = TemplateCodeGenOptions {
                        mode: if verter_options.ssr {
                            CodeGenMode::Ssr
                        } else if use_vapor {
                            CodeGenMode::Vapor
                        } else {
                            CodeGenMode::Vdom
                        },
                        is_inline: false,
                        is_production: options.is_production,
                        comments: options.comments.unwrap_or(!options.is_production),
                        force_js: verter_options.force_js,
                        self_name: to_pascal_case(&component_name),
                        const_props: verter_options.prop_constness_overrides.clone(),
                        has_scoped_style,
                        hoist_static: options.resolve_hoist_static(),
                        scope_id: if has_scoped_style {
                            scope_id_full.clone()
                        } else {
                            String::new()
                        },
                    };

                    let tpl_imports = generate_template(
                        template_ast,
                        &oxc_ast,
                        input,
                        &mut tpl_ct,
                        &tpl_alloc,
                        script_bindings,
                        &tpl_options,
                    );

                    // Strip TypeScript syntax from template expressions when force_js is set.
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

                    // Prefix and suffix were removed via CT operations above,
                    // so build_string() produces only the template region.
                    let tpl_code = tpl_ct.build_string();
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
                        imports: tpl_imports.vue,
                        ssr_imports: tpl_imports.ssr,
                        duration_ms: tpl_duration_ms,
                        attrs: tpl_attrs,
                    })
                } else {
                    None
                };

                (template_block_inner, raw_template_data)
            } // close `else` for is_non_html_lang
        } else {
            (None, None)
        };

    // ── 6. TSX codegen (optional) ────────────────────────────────
    // Produces a single combined `.tsx` or `.jsx` file for LSP type checking.
    // Determines output mode from script lang: JS/JSX SFCs get `.jsx` (JSDoc),
    // TS/TSX SFCs get `.tsx` (TypeScript).
    let tsx_block = if options.target.needs_tsx() {
        let tsx_start = Instant::now();
        let filename_str = options.filename.as_deref().unwrap_or("App.vue");
        let js_component_name = ide::sanitize_js_identifier(filename_str);

        // Determine if this is a JS SFC (needs JSX+JSDoc output instead of TSX)
        let is_jsx = {
            use crate::cursor::ScriptLanguage;
            let has_any_script = parsed.script_setup().is_some() || parsed.script().is_some();
            let lang = parsed
                .script_setup()
                .and_then(|s| s.lang)
                .or_else(|| parsed.script().and_then(|s| s.lang));
            match lang {
                // Explicit TS/TSX → TypeScript mode
                Some(ScriptLanguage::TypeScript | ScriptLanguage::TSX) => false,
                // Explicit JS/JSX → JavaScript mode
                Some(ScriptLanguage::JavaScript | ScriptLanguage::JSX) => true,
                // No lang attribute but has script blocks → JavaScript mode
                // (e.g. <script setup> without lang="ts")
                None if has_any_script => true,
                // No script blocks → default to TS (template-only SFCs)
                None => false,
                // Unknown lang → default to TS
                Some(_) => false,
            }
        };

        let tsx_script_opts = ide::IdeScriptOptions {
            component_name: &component_name,
            js_component_name: &js_component_name,
            filename: filename_str,
            scope_id: &scope_id_full,
            has_scoped_style,
            runtime_module_name: options.runtime_module_name.as_deref().unwrap_or("vue"),
            types_module_name: options
                .types_module_name
                .as_deref()
                .unwrap_or("@verter/types"),
            is_vapor: use_vapor,
            embed_ambient_types: options.embed_ambient_types,
            is_jsx,
            conditional_root_narrowing: options.conditional_root_narrowing,
            style_v_bind_vars: verter_options.style_v_bind_vars.clone(),
        };

        // Unified single CodeTransform for both script and template.
        // One CT → one source map → template diagnostics map correctly.
        let tsx_alloc = Allocator::new();
        let mut tsx_ct = CodeTransform::new(input, &tsx_alloc);

        // Compute template end position (byte offset after </template> close tag)
        let template_end: Option<u32> = template_ast_opt.map(|tpl| {
            tpl.root.tag_close.as_ref().map(|tc| tc.end).unwrap_or(
                tpl.root
                    .content
                    .as_ref()
                    .map(|c| c.end)
                    .unwrap_or(tpl.root.tag_open.end),
            )
        });

        // Script codegen — emits function wrapper spanning script to template end
        let tsx_script_result = ide::script::generate_ide_script(
            parsed.script(),
            parsed.script_setup(),
            template_ast_opt,
            input,
            &mut tsx_ct,
            &tsx_alloc,
            &tsx_script_opts,
            template_end,
        );

        // Remove style/custom blocks (NOT template — template codegen transforms it)
        for style in parsed.style_nodes() {
            let s_s = style.tag_open.start;
            let s_e = style
                .tag_close
                .as_ref()
                .map(|tc| tc.end)
                .unwrap_or(style.tag_open.end);
            tsx_ct.remove(s_s, s_e);
        }
        for node in parsed.unknown_nodes() {
            let s_s = node.tag_open.start;
            let s_e = node
                .tag_close
                .as_ref()
                .map(|tc| tc.end)
                .unwrap_or(node.tag_open.end);
            tsx_ct.remove(s_s, s_e);
        }
        remove_inter_block_gaps(&mut tsx_ct, input.len() as u32, &block_ranges);

        // Template codegen — transforms template JSX in place on the same CT
        if !has_parse_errors {
            if let Some(template_ast) = template_ast_opt {
                let is_non_html = template_ast.root.lang.as_ref().is_some_and(|span| {
                    let v = &input[span.start as usize..span.end as usize];
                    !v.is_empty() && v != "html"
                });
                if !is_non_html {
                    let tsx_source_type = if is_jsx {
                        SourceType::jsx()
                    } else {
                        SourceType::tsx()
                    };
                    let tsx_oxc = parse_template_expressions(
                        template_ast,
                        input,
                        &tsx_alloc,
                        tsx_source_type,
                    );
                    let mut tsx_out =
                        crate::template::code_gen::types::CodeGenOutput::new(&tsx_alloc);
                    let tsx_t_opts = ide::IdeTemplateOptions {
                        self_name: &to_pascal_case(&component_name),
                        comments: options.comments.unwrap_or(!options.is_production),
                        is_jsx,
                    };
                    ide::template::generate_ide_template(
                        template_ast,
                        &tsx_oxc,
                        input,
                        &mut tsx_out,
                        &tsx_alloc,
                        &tsx_script_result.bindings,
                        &tsx_t_opts,
                    );
                    tsx_out.apply_to(&mut tsx_ct);
                }
            }
        }

        // Apply deferred return+close AFTER template codegen to avoid interleaving.
        //
        // Template-first SFCs: the template appears before <script setup> in the source.
        // The function wrapper opens at the script tag, so the template JSX (which stays
        // at its original position) would end up outside the function. Fix: move the
        // transformed template content to after </script>, using return_close as the suffix
        // so it appears right after the relocated template.
        let template_start = template_ast_opt.map(|tpl| tpl.root.tag_open.start);
        let script_setup_start = parsed.script_setup().map(|s| s.tag_open.start);
        let template_before_script = template_start
            .is_some_and(|ts| script_setup_start.is_some_and(|ss| ts < ss))
            || template_start
                .is_some_and(|ts| parsed.script().is_some_and(|s| ts < s.tag_open.start));

        if template_before_script {
            if let (Some(ts), Some(te)) = (template_start, template_end) {
                // Compute move target: after the last script closing tag
                let move_target = tsx_script_result.return_close_pos.unwrap_or_else(|| {
                    // Options API: no return_close_pos, use script close tag end
                    let mut pos = te; // fallback to template end
                    if let Some(s) = parsed.script() {
                        if let Some(tc) = &s.tag_close {
                            pos = pos.max(tc.end);
                        }
                    }
                    if let Some(s) = parsed.script_setup() {
                        if let Some(tc) = &s.tag_close {
                            pos = pos.max(tc.end);
                        }
                    }
                    pos
                });

                let suffix = tsx_script_result.return_close.as_deref().unwrap_or("");
                tsx_ct.move_with_suffix(ts, te, move_target, suffix);
            }
        } else if let (Some(return_close), Some(pos)) = (
            &tsx_script_result.return_close,
            tsx_script_result.return_close_pos,
        ) {
            tsx_ct.prepend_left(pos, return_close);
        }

        // Append type constructs via CT outro — they have no sourcemap mapping
        // but must go through the CT so it remains the single source of truth.
        if !tsx_script_result.type_constructs.is_empty() {
            tsx_ct.append(&tsx_script_result.type_constructs);
        }

        // Build output and source map from the single unified CT
        let tsx_code = tsx_ct.build_string();
        let tsx_sm = if verter_options.source_map {
            let sm_opts = SourceMapOptions {
                source: options.filename.as_deref(),
                file: options.filename.as_deref(),
                include_content: true,
            };
            tsx_ct.generate_map(sm_opts).to_json_string()
        } else {
            String::new()
        };
        let tsx_dur = tsx_start.elapsed().as_secs_f64() * 1000.0;

        // Compute block_start/block_end from boundary markers in the final TSX code.
        // The markers are unique constants inserted by script codegen; debug_assert
        // that they are found when destructured_block metadata exists.
        let mut destructured_block = tsx_script_result.destructured_block;
        if let Some(ref mut meta) = destructured_block {
            const START_MARKER: &str = "/* verter-destructured-start */";
            const END_MARKER: &str = "/* verter-destructured-end */";
            if let Some(start) = tsx_code.find(START_MARKER) {
                meta.block_start = start as u32;
                let end = tsx_code.find(END_MARKER);
                debug_assert!(
                    end.is_some(),
                    "Found start marker but not end marker in TSX output"
                );
                if let Some(end) = end {
                    meta.block_end = (end + END_MARKER.len()) as u32;
                }
            } else {
                debug_assert!(
                    false,
                    "destructured_block metadata exists but start marker not found in TSX output"
                );
            }
        }

        Some(VerterTsxBlock {
            code: tsx_code,
            source_map: tsx_sm,
            duration_ms: tsx_dur,
            is_jsx,
            destructured_block,
        })
    } else {
        None
    };

    // ── 7. TSC codegen (optional) ─────────────────────────────────
    // Macro-only extraction — generates a minimal .tsc.tsx declaration file.
    let tsc_block = if options.target.needs_tsc() {
        let tsc_start = Instant::now();
        let tsc_out = tsc::generate_tsc_output_with_options(
            input,
            &component_name,
            &tsc::TscGenOptions {
                conditional_root_narrowing: options.conditional_root_narrowing,
                filename: options.filename.clone(),
                external_types: verter_options.external_types.clone(),
            },
        );
        let tsc_dur = tsc_start.elapsed().as_secs_f64() * 1000.0;
        Some(VerterTsxBlock {
            code: tsc_out.code,
            source_map: tsc_out.source_map,
            duration_ms: tsc_dur,
            is_jsx: false,
            destructured_block: None,
        })
    } else {
        None
    };

    // ── 8. Assemble ───────────────────────────────────────────────
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
        tsx: tsx_block,
        tsc: tsc_block,
        template_data: extracted_template_data,
    }
}

#[cfg(test)]
#[path = "../compile_tests.rs"]
mod tests;
