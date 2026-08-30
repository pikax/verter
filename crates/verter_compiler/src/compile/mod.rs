//! AST compilation orchestrator (SFC → JS).
//!
//! Tokenize → style (if `STYLE`) → script (if `needs_script`) →
//! template (if `TEMPLATE`) → TSX (if `TSX`) → TSC (if `TSC`) → assemble.
//!
//! Presets: [`CompileTarget::BUNDLER`], [`CompileTarget::IDE`],
//! [`CompileTarget::ANALYSIS`].

mod helpers;
mod macro_scope_check;
mod macro_semantic_diagnostics;
pub mod style_usage;
pub mod template_data;
pub(crate) mod template_expr_overlay;
pub mod types;

pub use helpers::*;
pub use template_data::*;
pub use types::*;
// The macro-codegen DTO vocabulary is owned by the dependency-neutral
// `verter_macro_dto` leaf (shared with the resolution-side producer);
// re-exported here so compiler-internal consumers keep the `compile::…`
// path. New consumers outside this crate import `verter_macro_dto`
// directly.
pub use verter_macro_dto::*;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use oxc_allocator::Allocator;
use oxc_span::SourceType;
use rustc_hash::FxHashSet;

use crate::code_transform::{CodeTransform, SourceMapOptions};
use crate::diagnostics::{
    CompilerErrorCode, Diagnostic, DiagnosticSeverity, SyntaxPluginContext, SyntaxPluginOptions,
};
use crate::ide;
use crate::parser::types::{ParsedSfc, RootNodeScript, StyleLang};
use crate::parser::Syntax;
use crate::script::prepared::PreparedScript;
use crate::script::{generate_script, ScriptCodeGenOptions};
use crate::style_planner::{
    analyze_css_module_classes, complete_static_class_names, generate_var_name,
    prepared_style_for_sealed_slot, run_vue_style_cascade, AuthoredStyleInput, StyleRewriteFailure,
    VBindVar,
};
use crate::template::code_gen::vdom::element::to_pascal_case;
use crate::template::code_gen::{generate_template, CodeGenMode, TemplateCodeGenOptions};
use crate::template::oxc::types::OxcParsedAst;
use crate::tokenizer::byte::{
    tokenize, tokenize_sfc, tokenize_sfc_with_delimiters, tokenize_with_delimiters,
};
use crate::tsc;
use verter_css_syntax::CssDialect;

use helpers::{empty_sfc_script_block, extract_attrs, extract_block_ranges};
use macro_scope_check::collect_invalid_options_scope_diagnostics;
use macro_semantic_diagnostics::{collect_macro_semantic_diagnostics, tsc_generation_diagnostic};

pub(crate) fn style_dialect(lang: Option<StyleLang>) -> Option<CssDialect> {
    match lang {
        None | Some(StyleLang::Css) => Some(CssDialect::Css),
        Some(StyleLang::Scss) => Some(CssDialect::Scss),
        Some(StyleLang::Sass) => Some(CssDialect::Sass),
        Some(StyleLang::Less) => Some(CssDialect::Less),
        Some(StyleLang::Stylus) => Some(CssDialect::Stylus),
        Some(StyleLang::Unknown) => None,
    }
}

/// Dialect used to read class names for editor completions.
///
/// Deliberately more tolerant than [`style_dialect`]: an unrecognised `lang`
/// still yields class-name completions, read under the base CSS grammar,
/// because the completion surface is advisory and a class selector is spelled
/// the same way in every dialect this compiler accepts. `style_dialect`
/// returns `None` for an unrecognised `lang` so that the rewriting pipeline
/// refuses content whose grammar it cannot claim to understand; extraction
/// carries no such risk, and returning `None` here would silently drop every
/// completion for a block the editor can still usefully complete.
fn class_extraction_dialect(lang: Option<StyleLang>) -> CssDialect {
    style_dialect(lang).unwrap_or(CssDialect::Css)
}

fn push_style_rewrite_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    content_span: crate::common::Span,
    error: &StyleRewriteFailure,
) {
    diagnostics.push(Diagnostic {
        severity: DiagnosticSeverity::Error,
        code: CompilerErrorCode::XCssParseError,
        plugin: "style-planner",
        message: error.to_string(),
        arguments: Vec::new(),
        span: error.span.map_or(content_span, |span| {
            crate::common::Span::new(
                content_span.start + span.start,
                content_span.start + span.end,
            )
        }),
    });
}

// ── Orchestrator ───────────────────────────────────────────────────

/// Parse an SFC source string into a [`ParsedSfc`].
///
/// Tokenizes in SFC mode with optional custom delimiters and custom element
/// prefixes. The result can be cached and passed to [`compile_from_parsed`]
/// to avoid re-parsing the same source.
pub(crate) fn parse_sfc(
    input: &str,
    delimiters: Option<(&str, &str)>,
    custom_elements: Option<&[String]>,
) -> ParsedSfc {
    verter_audit::attribute_n!(CarrierParse, input.len());
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

    let mut parsed = syntax.into_parsed_sfc();
    // Official Vue's SFC parser (`packages/compiler-sfc/src/parse.ts`)
    // unconditionally rejects a carrier with no `<template>`, `<script>`, or
    // `<script setup>` block — see the `# 6676` regression test in
    // `parse.spec.ts` ("should throw error if no <template> or <script> is
    // present"). Verter deliberately narrows that one rule: a block-less,
    // trivia-only carrier (empty, or whitespace/comments only) is still
    // admitted as an empty component shell — pre-existing base behavior
    // (`empty_sfc_compiles_to_empty_component_shell`) for a freshly created
    // blank file. Any other block-less content (styles, custom blocks,
    // malformed comments, arbitrary top-level text) is still diagnosed.
    //
    // A `<script>`/`<script setup>` block only counts as a real entry block
    // when official Vue would keep it: `parse.ts`'s `ignoreEmpty` branch
    // (lines 159-169) drops a script/scriptSetup node from the descriptor
    // when `isEmpty()` (lines 421-429 — every child is whitespace-only text,
    // or there are no children at all) holds AND the block has no `src`
    // attribute. `<script/>`, `<script></script>`, and whitespace-only
    // script content are therefore NOT entry blocks, matching
    // `parse.spec.ts`'s "should ignore other nodes with no content" case.
    if parsed.template_ast.is_none()
        && !script_node_counts_as_entry_block(parsed.script_node.as_ref(), input)
        && !script_node_counts_as_entry_block(parsed.script_setup_node.as_ref(), input)
        && (!parsed.style_nodes.is_empty()
            || !parsed.unknown_nodes.is_empty()
            || !is_empty_sfc_trivia(input))
    {
        parsed.diagnostics.push(Diagnostic::error(
            "syntax",
            CompilerErrorCode::MissingSfcEntryBlock,
            crate::common::Span::new(0, input.len() as u32),
        ));
    }
    if let Some(template) = parsed.template_ast.as_ref() {
        if let Some(attribute) = template.root.attributes.iter().find(|attribute| {
            input[attribute.start as usize..attribute.name_end as usize]
                .eq_ignore_ascii_case("functional")
        }) {
            parsed.diagnostics.push(Diagnostic::error(
                "syntax",
                CompilerErrorCode::TemplateFunctionalUnsupported,
                crate::common::Span::new(attribute.start, attribute.name_end),
            ));
        }
    }
    verter_parser::diagnostics::sort_diagnostics(&mut parsed.diagnostics);
    parsed.has_errors = parsed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error);
    parsed
}

/// Whether a parsed `<script>`/`<script setup>` node counts as a real entry
/// block for `MissingSfcEntryBlock`, matching official Vue's `isEmpty()` +
/// `ignoreEmpty` combination (`compiler-sfc/src/parse.ts` lines 159-169 and
/// 421-429): a `src`-attributed block always counts (line 165's
/// `!hasAttr(node, 'src')` guard — `hasAttr` tests ATTRIBUTE PRESENCE, line
/// 413-415, never the attribute's value), otherwise it counts only when its
/// content has at least one non-whitespace byte — a self-closing tag (no
/// content span at all) or whitespace-only content does not.
///
/// `RootNodeScript::src` only carries the `src` attribute's VALUE span (`None`
/// for a valueless `src`, e.g. `<script src/>` / `<script src></script>`), so
/// testing it directly would wrongly treat a present-but-valueless `src` as
/// absent. Test presence on the full attribute list instead.
///
/// The name match is CASE-SENSITIVE, matching `hasAttr`'s `p.name === name`
/// (`parse.ts:413-415`): Vue's own attribute-name parsing preserves the
/// authored spelling verbatim (`onattribname`'s `name: getSlice(start, end)`
/// in `compiler-core/src/parser.ts` — no `toLowerCase()`), so `<script SRC>`
/// / `<script Src>` do NOT count as `src`-attributed there, and Verter's own
/// parser likewise never case-folds attribute name spans.
fn script_node_counts_as_entry_block(node: Option<&RootNodeScript>, input: &str) -> bool {
    let Some(node) = node else {
        return false;
    };
    let has_src_attr = node
        .attributes
        .iter()
        .any(|attribute| &input[attribute.start as usize..attribute.name_end as usize] == "src");
    if has_src_attr {
        return true;
    }
    match node.content {
        Some(span) => !input[span.start as usize..span.end as usize]
            .trim()
            .is_empty(),
        None => false,
    }
}

/// Verter deliberately admits a block-less, trivia-only carrier as an empty
/// component shell. Vue's parser rejects every carrier without a template or
/// script, so keep this exception narrower than the structural no-entry test:
/// styles, custom blocks, malformed comments, and arbitrary top-level text are
/// still diagnosed as [`CompilerErrorCode::MissingSfcEntryBlock`].
fn is_empty_sfc_trivia(mut source: &str) -> bool {
    loop {
        source = source.trim_start();
        if source.is_empty() {
            return true;
        }
        let Some(comment_body) = source.strip_prefix("<!--") else {
            return false;
        };
        let Some(comment_end) = comment_body.find("-->") else {
            return false;
        };
        source = &comment_body[comment_end + 3..];
    }
}

/// Parse one selected HTML template source space without fabricating an SFC.
pub(crate) fn parse_template_block(
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
    let mut syntax = Syntax::new(true);
    if let Some((open, close)) = delimiters {
        tokenize_with_delimiters(
            bytes,
            |event| syntax.handle(&event, &ctx),
            open.as_bytes(),
            close.as_bytes(),
        );
    } else {
        tokenize(bytes, |event| syntax.handle(&event, &ctx));
    }
    syntax.into_parsed_sfc()
}

/// Describe one admitted raw script unit without fabricating carrier markup.
pub(crate) fn parse_script_block(input: &str, lang: &str, is_setup: bool) -> ParsedSfc {
    let length = input.len() as u32;
    let lang_kind = crate::cursor::ScriptLanguage::from_bytes(lang.as_bytes());
    let script = RootNodeScript {
        tag_open: crate::types::NodeTag {
            start: 0,
            end: 0,
            name_end: 0,
        },
        tag_close: Some(crate::types::NodeTag {
            start: length,
            end: length,
            name_end: length,
        }),
        is_setup,
        lang: Some(lang_kind),
        lang_value: Some(lang.to_string().into_boxed_str()),
        src: None,
        generic: None,
        attrs: None,
        attributes: Vec::new(),
        content: Some(crate::common::Span::new(0, length)),
    };
    ParsedSfc {
        template_ast: None,
        script_node: (!is_setup).then_some(script.clone()),
        script_setup_node: is_setup.then_some(script),
        style_nodes: Vec::new(),
        unknown_nodes: Vec::new(),
        has_style_scope: false,
        has_style_module: false,
        is_vapor: false,
        diagnostics: Vec::new(),
        has_errors: false,
    }
}

/// Byte span `[start, end)` of the `<template>` region in the SFC source.
///
/// Used as part of the shared template-expression overlay key so the parsed
/// facts are identified by the region they cover.
fn template_region_span(template_ast: &crate::ast::types::TemplateAst) -> (u32, u32) {
    let root = &template_ast.root;
    let end = root.tag_close.as_ref().map(|tc| tc.end).unwrap_or_else(|| {
        root.content
            .as_ref()
            .map(|c| c.end)
            .unwrap_or(root.tag_open.end)
    });
    (root.tag_open.start, end)
}

pub(crate) fn template_unit_used_vars(
    input: &str,
    parsed: &ParsedSfc,
    delimiters: Option<(String, String)>,
    custom_elements: Option<Vec<String>>,
    allocator: &Allocator,
) -> FxHashSet<String> {
    let Some(template_ast) = parsed.template_ast() else {
        return FxHashSet::default();
    };
    let parse_options = template_expr_overlay::ParseOptionsKey::new(delimiters, custom_elements);
    let mut store = template_expr_overlay::TemplateExprStore::new();
    let oxc = store.get_or_build(
        template_ast,
        input,
        allocator,
        template_region_span(template_ast),
        &parse_options,
        SourceType::tsx(),
        false,
    );
    template_expr_overlay::collect_template_used_vars(oxc, template_ast, input).0
}

/// Collect OXC expression parse errors from template expressions and emit them
/// as `XInvalidExpression` diagnostics.
///
/// Only checks interpolation expressions and structural directive expressions
/// (v-if/v-else-if conditions, v-for). Regular directive prop values (`:attr="..."`)
/// are excluded because they may contain HTML entities or template-specific syntax
/// that fails OXC parsing but is handled by codegen.
fn collect_expression_errors(
    oxc_ast: &OxcParsedAst<'_>,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
    compile_failures: &mut Vec<CompileDiagnostic>,
) {
    use crate::template::oxc::types::OxcNodeData;

    for node in &oxc_ast.data {
        match node {
            OxcNodeData::Interpolation(expr) => {
                push_expression_errors(expr, source, diagnostics, compile_failures);
            }
            OxcNodeData::Element(el) => {
                if let Some(ref cond) = el.condition {
                    push_expression_errors(cond, source, diagnostics, compile_failures);
                }
                if let Some(ref v_for) = el.v_for {
                    for err in &v_for.parsed.result.left_errors {
                        push_oxc_error(err, source, diagnostics, compile_failures);
                    }
                    for err in &v_for.parsed.result.right_errors {
                        push_oxc_error(err, source, diagnostics, compile_failures);
                    }
                }
                if let Some(ref v_slot) = el.v_slot {
                    if let Some(ref errors) = v_slot.parsed.result.errors {
                        for err in errors {
                            push_oxc_error(err, source, diagnostics, compile_failures);
                        }
                    }
                }
                // Note: regular directive prop expressions (`:class="..."`, `@click="..."`)
                // are intentionally NOT checked here. They may contain HTML entities
                // (e.g., `&quot;`) that haven't been decoded yet, causing false OXC errors.
            }
            OxcNodeData::None => {}
        }
    }
}

/// Push parse errors from an OXC expression as XInvalidExpression diagnostics.
///
/// These are warnings (not errors) because:
/// - The IDE codegen handles broken expressions gracefully (best-effort JSX output)
/// - The type checker (TSGO/tsserver) will report the actual TS error for the broken syntax
/// - Warning severity prevents the host from discarding usable IDE output
fn push_expression_errors(
    expr: &crate::template::oxc::types::OxcParsedExpression<'_>,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
    compile_failures: &mut Vec<CompileDiagnostic>,
) {
    if let Some(ref errors) = expr.errors {
        for err in errors {
            push_oxc_error(err, source, diagnostics, compile_failures);
        }
    }
}

fn push_oxc_error(
    error: &oxc_diagnostics::OxcDiagnostic,
    source: &str,
    diagnostics: &mut Vec<Diagnostic>,
    compile_failures: &mut Vec<CompileDiagnostic>,
) {
    let span = error.labels.as_ref().and_then(|labels| {
        labels
            .iter()
            .find(|label| label.primary())
            .or_else(|| labels.first())
            .and_then(|label| {
                let start = u32::try_from(label.offset()).ok()?;
                let end = u32::try_from(label.offset().checked_add(label.len())?).ok()?;
                ((end as usize) <= source.len()
                    && source.is_char_boundary(start as usize)
                    && source.is_char_boundary(end as usize))
                .then_some(crate::common::Span::new(start, end))
            })
    });
    if let Some(span) = span {
        diagnostics.push(
            Diagnostic::warning("template", CompilerErrorCode::XInvalidExpression, span)
                .with_message(error.message.to_string()),
        );
    } else {
        compile_failures.push(CompileDiagnostic {
            severity: CompileDiagnosticSeverity::Error,
            code: format!("{:?}", CompilerErrorCode::XInvalidExpression),
            message: "Template expression parser returned an unmapped diagnostic.".to_string(),
            span: None,
        });
    }
}

/// Validate template semantics that belong to compilation rather than the SFC
/// parse seam. Vue's SFC parser accepts `v-slot` on a native element; the
/// template compiler (`transformElement`, `X_V_SLOT_MISPLACED`) is the layer
/// that rejects it — see `packages/compiler-core/src/transforms/transformElement.ts`.
fn collect_template_compile_diagnostics(parsed: &ParsedSfc, diagnostics: &mut Vec<Diagnostic>) {
    use crate::ast::types::{AstNodeKind, TagType};

    let Some(template) = parsed.template_ast() else {
        return;
    };
    for node in &template.nodes {
        let AstNodeKind::Element(element) = &node.kind else {
            continue;
        };
        let Some(v_slot) = element.v_slot.as_ref() else {
            continue;
        };
        if !matches!(element.tag_type, TagType::Component | TagType::Template) {
            diagnostics.push(Diagnostic::error(
                "template",
                CompilerErrorCode::XVSlotMisplaced,
                crate::common::Span::new(v_slot.start, v_slot.name_end),
            ));
        }
    }
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
///
/// Every production route reaches [`compile_from_parsed`] (via
/// [`parse_sfc`] + [`compile_from_parsed`] as two explicit steps, or the
/// standalone one-shot direct core,
/// [`crate::standalone::StandaloneCompiler::compile`], which publishes an
/// atomic [`crate::assembly::ArtifactSet`]) directly — this plain
/// parse-and-discard convenience wrapper has NO production caller at all,
/// so unlike [`compile_from_parsed`] it needs no `pub(crate)` production
/// arm: it is `pub` ONLY under `cfg(test)`/`feature = "test-support"`,
/// genuinely absent from a shipped build, not merely hidden from docs — the
/// SAME opt-in seam `verter_css_syntax`'s own cross-crate test-support edge
/// uses (see this crate's `Cargo.toml`), for callers that genuinely need
/// the pre-assembly, per-block [`VerterCompileResult`] shape directly
/// rather than an atomic [`crate::assembly::ArtifactSet`]: this crate's own
/// `direct_result_tests`/`compile_tests`, and `verter_bench`'s profiling
/// examples/benches (both enable `test-support` on their own
/// `verter_compiler` dev-dependency edge, never on their regular one).
#[cfg(any(test, feature = "test-support"))]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn compile(
    input: &str,
    request: &crate::compile_request::CompileRequest,
    execution_inputs: &VueExecutionInputs,
    macro_semantics: &VueMacroSemanticInput,
    allocator: &Allocator,
) -> Result<VerterCompileResult, crate::compile_request::CompileRequestError> {
    Ok(compile_with_parsed(input, request, execution_inputs, macro_semantics, allocator)?.1)
}

/// Compile a Vue SFC and retain the exact parse used by code generation.
///
/// This is the standalone bridge for consumers that must qualify emitted
/// block maps without reparsing the carrier.
///
/// Same visibility seam as [`compile`] and for the same reason: never
/// publishes an [`crate::assembly::ArtifactSet`], so it is not a second
/// alternate core, but a SHIPPED build must not be able to reach it at all
/// — `pub` only under `cfg(test)`/`feature = "test-support"`, genuinely
/// absent otherwise. `crate::standalone`'s direct one-shot core no longer
/// calls this combined parse-and-compile convenience in production — it
/// now calls [`parse_sfc`] and [`compile_from_parsed`] as two explicit
/// steps (the seam prepared/batch compiling shares), so this function's
/// only remaining callers are cross-crate conformance/characterization
/// harnesses like `verter_vue_conformance`'s seed comparator and
/// `verter_session`'s dispatch-byte-identity pin that build a
/// [`crate::framework_common::RuntimeCompileOutput`] from the SAME parse
/// `compile_with_parsed` produced, rather than reparsing.
#[cfg(any(test, feature = "test-support"))]
pub fn compile_with_parsed(
    input: &str,
    request: &crate::compile_request::CompileRequest,
    execution_inputs: &VueExecutionInputs,
    macro_semantics: &VueMacroSemanticInput,
    allocator: &Allocator,
) -> Result<(ParsedSfc, VerterCompileResult), crate::compile_request::CompileRequestError> {
    compile_with_parsed_impl(input, request, execution_inputs, macro_semantics, allocator)
}

#[cfg(any(test, feature = "test-support"))]
fn compile_with_parsed_impl(
    input: &str,
    request: &crate::compile_request::CompileRequest,
    execution_inputs: &VueExecutionInputs,
    macro_semantics: &VueMacroSemanticInput,
    allocator: &Allocator,
) -> Result<(ParsedSfc, VerterCompileResult), crate::compile_request::CompileRequestError> {
    let Some(vue) = request.vue() else {
        return Err(
            crate::compile_request::CompileRequestError::FrameworkMismatch {
                expected: "Vue",
                actual: "Svelte",
            },
        );
    };
    let parse_start = Instant::now();
    let parsed = parse_sfc(
        input,
        vue.delimiters
            .as_ref()
            .map(|(o, c)| (o.as_str(), c.as_str())),
        Some(vue.is_custom_element.as_slice()),
    );
    let parse_duration_ms = parse_start.elapsed().as_secs_f64() * 1000.0;
    if let Some(observer) = verter_audit::current_observer() {
        observer.record_phase_timing("compile.parse", parse_duration_ms);
    }
    // Resolve the backend NOW — the earliest point the implicit `<template
    // vapor>` marker is knowable — before any codegen decision reads it.
    // Fails closed on `SSR x Vapor` / `inline x Vapor` here, the exact two
    // cases `CompileRequest::new` could not see at construction time.
    let resolved_backend = request.resolve_vue_backend(parsed.is_vapor())?;
    let (options, verter_options) =
        derive_legacy_vue_options(request, resolved_backend, execution_inputs);
    let result = compile_inner(
        input,
        &parsed,
        &options,
        &verter_options,
        macro_semantics,
        allocator,
        parse_duration_ms,
    )?;
    Ok((parsed, result))
}

/// Compile a pre-parsed SFC. Skips tokenization and parsing.
///
/// The `parsed` must have been produced from the same `input` string.
/// Parse-affecting options (delimiters, custom_elements) must match those
/// used to create the [`ParsedSfc`] — the caller is responsible for cache
/// key correctness.
pub(crate) fn compile_from_parsed(
    input: &str,
    parsed: &ParsedSfc,
    request: &crate::compile_request::CompileRequest,
    execution_inputs: &VueExecutionInputs,
    macro_semantics: &VueMacroSemanticInput,
    allocator: &Allocator,
) -> Result<VerterCompileResult, crate::compile_request::CompileRequestError> {
    let resolved_backend = request.resolve_vue_backend(parsed.is_vapor())?;
    let (options, verter_options) =
        derive_legacy_vue_options(request, resolved_backend, execution_inputs);
    compile_inner(
        input,
        parsed,
        &options,
        &verter_options,
        macro_semantics,
        allocator,
        0.0,
    )
}

/// Crate-internal escape hatch: drives [`compile_inner`] directly from
/// already-legacy-shaped options, bypassing `CompileRequest` derivation.
///
/// Used by `framework_common::vue_bridge`'s block-content composition
/// sub-calls and by [`crate::standalone::StandaloneCompiler`]'s selected-
/// template IDE prerequisite. Those decompose one external request into
/// fine-grained sub-compiles over selected SFC fragments (a projected
/// script unit, a selected template block) that do not themselves
/// correspond to an independent top-level product request — internal
/// plumbing, not a second production request-construction point. NOT
/// reachable outside this crate.
///
/// Infallible in practice: it bypasses `CompileRequest::resolve_vue_backend`
/// entirely (there is no top-level `CompileRequest` at this decomposition
/// layer to resolve), so `verter_options.ssr`/`.force_vapor` are values the
/// CALLER's own top-level request already validated before ever building
/// this sub-compile's `RuntimeCompileOptions` — `compile_inner` itself never
/// returns `Err` (both typed refusals are raised only by
/// `resolve_vue_backend`), so unwrapping here cannot mask a real refusal.
pub(crate) fn compile_from_parsed_legacy(
    input: &str,
    parsed: &ParsedSfc,
    options: &CodegenOptions,
    verter_options: &ResolvedVueCompileOptions,
    macro_semantics: &VueMacroSemanticInput,
    allocator: &Allocator,
) -> Result<VerterCompileResult, crate::compile_request::CompileRequestError> {
    compile_inner(
        input,
        parsed,
        options,
        verter_options,
        macro_semantics,
        allocator,
        0.0,
    )
}

/// Derives the internal, pre-validated `(CodegenOptions,
/// ResolvedVueCompileOptions)` pair from a canonical `CompileRequest` — the
/// SOLE production path that constructs either type. Every construction-time
/// fail-closed rule already ran in `CompileRequest::new`; `resolved_backend`
/// already ran the post-parse half (`resolve_vue_backend`) — this function
/// only translates, never re-decides, semantics.
pub(crate) fn derive_legacy_vue_options(
    request: &crate::compile_request::CompileRequest,
    resolved_backend: crate::compile_request::ResolvedVueBackend,
    execution_inputs: &VueExecutionInputs,
) -> (CodegenOptions, ResolvedVueCompileOptions) {
    use crate::compile_request::{CompileProduct, ResolvedVueBackend};

    let vue = request
        .vue()
        .expect("derive_legacy_vue_options is Vue-only");
    let use_vapor = matches!(resolved_backend, ResolvedVueBackend::Vapor);
    let ssr = request
        .products()
        .iter()
        .any(|p| matches!(p, CompileProduct::RuntimeServer(_)));

    // Reconstruct the exact legacy bit membership — see
    // `CompileRequest`'s zero-work predicate doc comments for the verified
    // 1:1 mapping from product membership to each raw bit.
    let mut target = CompileTarget::empty();
    if request.wants_style_codegen() {
        target |= CompileTarget::STYLE;
    }
    if request.has_runtime_product() || request.analysis_wants_script_bindings() {
        target |= CompileTarget::SCRIPT;
    }
    if request.wants_template_codegen() {
        target |= CompileTarget::TEMPLATE;
    }
    if request.wants_tsx() {
        target |= CompileTarget::TSX;
    }
    if request.wants_tsc() {
        target |= CompileTarget::TSC;
    }
    if request.analysis_wants_template_data() {
        target |= CompileTarget::TEMPLATE_DATA;
    }

    let ide = request.products().iter().find_map(|p| match p {
        CompileProduct::IdeCompanion(i) => Some(i),
        _ => None,
    });
    let runtime = request.products().iter().find_map(|p| match p {
        CompileProduct::RuntimeClient(r) | CompileProduct::RuntimeServer(r) => Some(r),
        _ => None,
    });

    let codegen_options = CodegenOptions {
        filename: request.filename().map(str::to_string),
        is_production: request.is_production(),
        custom_element: vue.script_custom_element.unwrap_or(false),
        component_id: request.component_id().map(str::to_string),
        target,
        ide_chunk_boundaries: ide.is_some_and(|i| i.ide_chunk_boundaries),
        delimiters: vue.delimiters.clone(),
        custom_elements: if vue.is_custom_element.is_empty() {
            None
        } else {
            Some(vue.is_custom_element.clone())
        },
        comments: vue.comments,
        runtime_module_name: vue.runtime_module_name.clone(),
        types_module_name: ide.and_then(|i| i.types_module_name.clone()),
        hoist_static: vue.hoist_static,
        inline: runtime.and_then(|r| r.inline),
        embed_ambient_types: ide.is_some_and(|i| i.embed_ambient_types),
        conditional_root_narrowing: ide.is_some_and(|i| i.conditional_root_narrowing),
        strict_slots: ide.is_some_and(|i| i.strict_slots),
    };

    let resolved_flags = ResolvedVueCompileOptions {
        force_vapor: use_vapor,
        force_js: request.force_js(),
        source_map: if ssr {
            request.products().iter().find_map(|p| match p {
                CompileProduct::RuntimeServer(r) => Some(r.runtime_source_map),
                _ => None,
            })
        } else {
            request.products().iter().find_map(|p| match p {
                CompileProduct::RuntimeClient(r) => Some(r.runtime_source_map),
                _ => None,
            })
        }
        .unwrap_or(false),
        ide_source_map: ide.is_some_and(|i| i.want_source_map),
        ssr,
        prop_constness_overrides: execution_inputs.prop_constness_overrides.clone(),
        style_v_bind_vars: execution_inputs.style_v_bind_vars.clone(),
        style_v_bind_usage_complete: execution_inputs.style_v_bind_usage_complete,
        template_binding_metadata: execution_inputs.template_binding_metadata.clone(),
        template_used_vars: execution_inputs.template_used_vars.clone(),
        runtime_template_hole: execution_inputs.runtime_template_hole,
        runtime_inline_template_chunk: execution_inputs.runtime_inline_template_chunk,
        prepared_styles: execution_inputs.prepared_styles.clone(),
    };

    (codegen_options, resolved_flags)
}

/// Internal compilation driver. Borrows a pre-parsed [`ParsedSfc`] — no cloning
/// of template AST, script nodes, or style nodes.
///
/// Returns a typed refusal exactly for the two `SSR x Vapor` / `inline x
/// Vapor` cases `CompileRequest::new` could not see at construction time
/// (backend resolution needs the parsed source) — every other fail-closed
/// rule (unsupported options, `SSR x Vapor` explicit backend, `inline x
/// SSR`) is already enforced upstream by the canonical request, so
/// `verter_options`/`options` here are already-validated derived values,
/// never a second place those rules are re-decided.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn compile_inner(
    input: &str,
    parsed: &ParsedSfc,
    options: &CodegenOptions,
    verter_options: &ResolvedVueCompileOptions,
    macro_semantics: &VueMacroSemanticInput,
    allocator: &Allocator,
    parse_duration_ms: f64,
) -> Result<VerterCompileResult, crate::compile_request::CompileRequestError> {
    let total_start = Instant::now();

    // Prepare the setup + companion script blocks once. This single parse backs
    // script codegen syntax ownership, bindings, and force-js type stripping.
    let prepared_script =
        PreparedScript::build(input, parsed.script(), parsed.script_setup(), allocator);

    // Clone diagnostics — this is the only clone needed from ParsedSfc.
    let mut all_diagnostics = parsed.clone_diagnostics();
    let has_parse_errors = parsed.has_errors();
    if options.target.needs_template_codegen()
        || options.target.needs_tsx()
        || options.target.needs_template_data()
    {
        collect_template_compile_diagnostics(parsed, &mut all_diagnostics);
    }
    let macro_validation =
        collect_macro_semantic_diagnostics(&prepared_script, options.target, macro_semantics);
    let mut compile_failures = macro_validation.compile_failures;
    all_diagnostics.extend(macro_validation.diagnostics);
    let validated_runtime = macro_validation
        .runtime_valid
        .then(|| macro_semantics.runtime())
        .flatten();
    // Official `checkInvalidScopeReference`: runtime macro arguments are
    // hoisted outside `setup()` — reject setup-scope references.
    all_diagnostics.extend(collect_invalid_options_scope_diagnostics(&prepared_script));

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

    // `verter_options.force_vapor` is already the RESOLVED backend
    // (`CompileRequest::resolve_vue_backend`, called by the derivation
    // function before this pipeline ever runs) — `verter_options.ssr &&
    // use_vapor` is therefore unreachable here: a request that would
    // produce that combination is refused with a typed
    // `SsrVaporBackendUnsupported` before `compile()` is invoked at all,
    // both for an explicit `force_vapor` and for an implicit `<template
    // vapor>` marker. Recomputing `parsed.is_vapor()` here is redundant
    // with that resolution but not wrong (same source, same answer).
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
    let mut total_style_duration_ms: f64 = 0.0;

    if options.target.needs_style() {
        for (style_index, style) in parsed.style_nodes().iter().enumerate() {
            let style_start = Instant::now();
            let mut style_module_classes = Vec::new();
            let style_code = if let Some(content) = &style.content {
                let style_source = &input[content.start as usize..content.end as usize];
                match style_dialect(style.lang) {
                    None => {
                        // Rewrite must refuse an unrecognized lang rather
                        // than parse the bytes as CSS.
                        all_diagnostics.push(Diagnostic::error_with_message(
                            "style-planner",
                            CompilerErrorCode::XCssParseError,
                            "style rewrite refused unknown dialect; expected css, scss, sass, less, or stylus",
                            *content,
                        ));
                        style_source.to_string()
                    }
                    Some(authored_dialect) => {
                        let source_name = options.filename.as_deref().unwrap_or("<style>");

                        // The CSS-Modules byte-level class-name rewrite stays
                        // CSS-only; only that one stage is conditioned on the
                        // resolved dialect.
                        let cascade_module = style.module && authored_dialect == CssDialect::Css;
                        let mut cascade_input = AuthoredStyleInput::new(
                            style_source,
                            authored_dialect,
                            source_name,
                            "standalone:carrier",
                            "standalone:carrier-bytes",
                        );
                        if let Some(prepared) = prepared_style_for_sealed_slot(
                            None,
                            &verter_options.prepared_styles,
                            style_index,
                            style_source,
                        ) {
                            cascade_input = cascade_input.with_prepared(prepared.ir());
                        }

                        let outcome = run_vue_style_cascade(
                            cascade_input,
                            scope_id_str,
                            cascade_module,
                            style.scoped,
                            verter_options.source_map,
                        );
                        all_v_bind_vars.extend(outcome.facts.v_bind_vars);
                        for refusal in outcome
                            .facts
                            .refusals
                            .iter()
                            .chain(outcome.stage_failures.iter())
                        {
                            push_style_rewrite_diagnostic(&mut all_diagnostics, *content, refusal);
                        }
                        style_module_classes = outcome.facts.module_classes;

                        // CSS-Modules class *analysis* is dialect-unconditional
                        // (A10a): for the 4 RECOGNIZED non-CSS dialects the
                        // byte-level rewrite above never runs against (SCSS/Sass/
                        // Less/Stylus), still analyze the authored `$style` class
                        // surface so IDE/consumer metadata is populated even though
                        // the emitted CSS text itself is left for external
                        // preprocessing to rewrite.
                        if style.module && authored_dialect != CssDialect::Css {
                            match analyze_css_module_classes(cascade_input, scope_id_str) {
                                Ok(classes) => style_module_classes = classes,
                                Err(error) => {
                                    push_style_rewrite_diagnostic(
                                        &mut all_diagnostics,
                                        *content,
                                        &error,
                                    );
                                }
                            }
                        }

                        outcome.code
                    }
                }
            } else {
                String::new()
            };

            let style_duration_ms = style_start.elapsed().as_secs_f64() * 1000.0;
            total_style_duration_ms += style_duration_ms;
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
                module_classes: style_module_classes,
            });
        }
        // Emit one CSS-analysis phase-boundary timing per compile —
        // not per <style> block. The audit denylist (see
        // `audit_no_hot_loop_instrumentation`) keeps instrumentation
        // out of the per-block loop.
        if !parsed.style_nodes().is_empty() {
            if let Some(observer) = verter_audit::current_observer() {
                observer.record_phase_timing("compile.css_analysis", total_style_duration_ms);
            }
        }
    } // end if needs_style

    if all_v_bind_vars.is_empty() && !options.target.needs_style() {
        all_v_bind_vars.extend(verter_options.style_v_bind_vars.iter().map(|expression| {
            VBindVar {
                expression: expression.clone(),
                var_name: generate_var_name(scope_id_str, expression),
                expr_start: 0,
                expr_end: 0,
            }
        }));
    }

    // ── 4. Script codegen ─────────────────────────────────────────
    // Script codegen is skipped when the target only needs TSX or TSC,
    // since those paths have their own independent codegen.
    //
    // The template expressions parse into a per-compile overlay store, shared
    // read-only by every lane that requests identical parse facts. The runtime,
    // template-data, and script-import-elision consumers all reuse the single
    // `ide_completion = false` `tsx()` entry; the IDE/TSX lane keys
    // `ide_completion = true` (its scoped-completion binding facts differ), so
    // it never reuses the runtime overlay and stays a separate output owner. The
    // key holds the full `SourceType` and `ide_completion` flag, so a `jsx()`
    // lane or a `true` completion lane can never reuse a `tsx()`/`false` overlay.
    // Built in the top compile allocator so the facts outlive every lane that
    // borrows them.
    let source_type = SourceType::tsx();
    // Exact parse-affecting options shared across every lane in this compile.
    // Held by value (not hashed) so overlay reuse is an exact-content match.
    let parse_options = template_expr_overlay::ParseOptionsKey::new(
        options.delimiters.clone(),
        options.custom_elements.clone(),
    );
    let mut expr_store = template_expr_overlay::TemplateExprStore::new();
    let transferred_bindings = verter_options.template_binding_metadata.as_ref();
    let mut script_bindings: rustc_hash::FxHashMap<
        &str,
        crate::template::code_gen::binding::BindingType,
    > = transferred_bindings
        .into_iter()
        .flat_map(|metadata| metadata.bindings.iter())
        .map(|(name, kind)| (allocator.alloc_str(name) as &str, *kind))
        .collect();
    let mut script_block: Option<VerterScriptBlock> = None;
    // Official `setup-maybe-ref` user imports — inline template refs to
    // these names bind `ref_key`/`ref` (populated by the script lane and
    // threaded to the VDOM resolver).
    let mut ref_bindable_imports: rustc_hash::FxHashSet<String> = transferred_bindings
        .map(|metadata| metadata.ref_bindable_imports.clone())
        .unwrap_or_default();

    // Inline-template (official production topology): the render function is
    // emitted INSIDE `setup()` as a returned closure that references setup
    // bindings directly. Official defaults to inline in production builds
    // (`resolve_inline`); it only applies to client VDOM with both a
    // `<script setup>` and a template. `inline x ssr` and `inline x vapor`
    // are NOT silently demoted to non-inline here: `CompileRequest::new`
    // already refuses `inline x ssr` at construction, and the derivation
    // function already refused `inline x vapor` (explicit or implicit) via
    // `resolve_vue_backend` before this pipeline ever ran — so
    // `resolve_inline() && (use_vapor || verter_options.ssr)` is
    // unreachable here; the two conjuncts below are defense-in-depth, not
    // the enforcement point. Template-only / script-only SFCs still stay
    // non-inline (no `<script setup>` or no template to merge).
    let inline_active = options.resolve_inline()
        && !use_vapor
        && !verter_options.ssr
        && !has_parse_errors
        && parsed.script_setup().is_some()
        && (parsed.template_ast().is_some() || verter_options.runtime_template_hole);

    let use_transferred_script_semantics = transferred_bindings.is_some()
        && parsed.script().is_none()
        && parsed.script_setup().is_none();
    if options.target.needs_script() && !use_transferred_script_semantics {
        let script_start = Instant::now();

        let mut ct = CodeTransform::new(input, allocator);

        // Parse template expressions early so we can collect the set of identifiers
        // actually used in the template (for import elision in script codegen).
        // This avoids the text-based heuristic and correctly handles TS type positions.
        let template_used_vars: Option<FxHashSet<String>> = if let Some(used) =
            verter_options.template_used_vars.as_ref()
        {
            Some(used.clone())
        } else if let (false, Some(template_ast_ref)) = (has_parse_errors, parsed.template_ast()) {
            // Runtime / script-import-elision lane — completion-prefix
            // matching off so partial identifiers stay real references.
            let oxc_ast = expr_store.get_or_build(
                template_ast_ref,
                input,
                allocator,
                template_region_span(template_ast_ref),
                &parse_options,
                source_type,
                false,
            );
            // Import elision only needs the used-name set (it is already
            // best-effort and conservative); completeness is irrelevant here,
            // so the per-expression completeness flag is dropped on this lane.
            let (used, _complete) =
                template_expr_overlay::collect_template_used_vars(oxc_ast, template_ast_ref, input);
            Some(used)
        } else {
            None
        };

        let script_options = ScriptCodeGenOptions {
            macro_runtime: validated_runtime,
            is_production: options.is_production,
            custom_element: options.custom_element,
            component_name: &component_name,
            scope_id: &scope_id_full,
            keep_ts_types: !verter_options.force_js,
            inline_template: inline_active,
            is_vapor: use_vapor,
            ssr: verter_options.ssr,
            has_scoped_style,
            css_v_binds: &all_v_bind_vars,
            template_used_vars,
        };

        let script_result = generate_script(
            parsed.script(),
            parsed.script_setup(),
            &prepared_script,
            input,
            &mut ct,
            allocator,
            &script_options,
        );

        // Save bindings for template codegen (borrow/move happens later)
        script_bindings = script_result.bindings;
        // Inline mode: where the render closure is spliced into `setup()`
        // (the setup close-tag position, before the wrapper end).
        let inline_inject_pos = script_result.inline_inject_pos;
        // Official `setup-maybe-ref` user imports — inline template refs to
        // these names bind `ref_key`/`ref` (threaded to the VDOM resolver).
        ref_bindable_imports = script_result
            .ref_bindable_imports
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        // Remove template and style blocks from script output. Inline mode
        // keeps the template region: the render codegen runs on this same CT
        // below and the emitted chunk is moved into `setup()`.
        if !inline_active {
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

        // Strip remaining TypeScript syntax if requested. Both blocks were
        // already parsed once into the compile allocator; strip from those
        // programs instead of re-parsing here.
        if verter_options.force_js {
            if let Some(setup) = prepared_script.setup() {
                // `generate_script` owns setup imports (type-only removal,
                // value/mixed reconstruction + hoist), so the body strip skips
                // import declarations — editing an import twice corrupts the
                // transform and no-ops later body strips.
                crate::strip_types::typescript::strip_typescript_body_types(
                    setup.program(),
                    &mut ct,
                    setup.content_start(),
                    setup.content_str(),
                );
            }
            if let Some(companion) = prepared_script.companion() {
                // The Options-API companion is emitted at module scope with its
                // imports in place, so the full strip owns its imports too.
                crate::strip_types::typescript::strip_typescript_types(
                    companion.program(),
                    &mut ct,
                    companion.content_start(),
                    companion.content_str(),
                );
            }
        }

        // Inline merge: run the template render codegen on the SAME CT (the
        // template region was kept above), then move the emitted
        // `return (_ctx,_cache) => { ... }` chunk into `setup()` at the inject
        // position. Hoisted statics were emitted as a file-top prepend by the
        // codegen itself. All template-expression source mappings stay on the
        // single shared CT, so the merged module keeps full map fidelity.
        let mut inline_tpl_imports: Vec<&'static str> = Vec::new();
        if inline_active && parsed.template_ast().is_some() {
            let template_ast = parsed
                .template_ast()
                .expect("inline_active requires a template");
            // Reuse the runtime overlay entry (same parse facts as the
            // used-vars lane above; completion-prefix matching off).
            let oxc_ast = expr_store.get_or_build(
                template_ast,
                input,
                allocator,
                template_region_span(template_ast),
                &parse_options,
                source_type,
                false,
            );
            collect_expression_errors(oxc_ast, input, &mut all_diagnostics, &mut compile_failures);

            let tpl_options = TemplateCodeGenOptions {
                mode: CodeGenMode::Vdom,
                is_inline: true,
                is_production: options.is_production,
                comments: options.comments.unwrap_or(!options.is_production),
                force_js: verter_options.force_js,
                self_name: to_pascal_case(&component_name),
                const_props: verter_options.prop_constness_overrides.clone(),
                has_script: true,
                ref_bindable_imports: ref_bindable_imports.clone(),
                has_scoped_style,
                hoist_static: options.resolve_hoist_static(),
                scope_id: if has_scoped_style {
                    scope_id_full.clone()
                } else {
                    String::new()
                },
                ssr_css_vars: Vec::new(),
            };
            let tpl_imports = generate_template(
                template_ast,
                oxc_ast,
                input,
                &mut ct,
                allocator,
                std::mem::take(&mut script_bindings),
                &tpl_options,
            );
            inline_tpl_imports = tpl_imports.vue;
            // Inline-mode hoisted constants (`_hoisted_N`) go through
            // `ct.prepend` — a position-anchored prepend here would lose the
            // ordering race against the script codegen's own position-0
            // user-import hoist (already applied by this point). Applying it
            // BEFORE the helper-import-line `ct.prepend` below means the
            // final order is: helper import, hoisted consts, user code —
            // each `ct.prepend` call lands its content immediately in front
            // of whatever the transform's intro already holds.
            if let Some(preamble) = tpl_imports.module_preamble {
                ct.prepend(preamble);
            }

            // Strip TypeScript syntax from template expressions when force_js
            // is set (same pass the standalone template lane runs).
            if verter_options.force_js {
                for expr in oxc_ast.iter_expressions() {
                    if let Some(ref expression) = expr.expression {
                        crate::strip_types::typescript::strip_typescript_from_expression(
                            expression,
                            &mut ct,
                            expr.offset,
                            &input[expr.offset as usize..],
                        );
                    }
                }
            }

            // Splice the render chunk into setup: the whole template region
            // (now the `return (...) => { ... }` statement) moves to the
            // setup close-tag position, before the wrapper end. An unmapped
            // `\n` prefix separates it from whatever the authored setup body
            // ends with — every existing fixture happens to have `</script>`
            // on its own line already, but a tightly-packed body with no
            // trailing whitespace/semicolon (`const n = 1</script>`) would
            // otherwise abut the closure directly (`1return`), which is a
            // genuine ECMAScript syntax error (`NumericLiteral` may not be
            // immediately followed by `IdentifierStart`).
            let root = &template_ast.root;
            let tpl_start = root.tag_open.start;
            let tpl_end = root
                .tag_close
                .as_ref()
                .map(|tc| tc.end)
                .unwrap_or(root.tag_open.end);
            let inject_pos = inline_inject_pos.expect("inline mode sets inline_inject_pos");
            ct.move_with_prefix(tpl_start, tpl_end, inject_pos, "\n");
        }
        const RUNTIME_TEMPLATE_HOLE: &str = "/* verter-runtime-template-hole */";
        let runtime_template_marker = verter_options
            .runtime_template_hole
            .then(|| {
                inline_inject_pos.and_then(|inject_pos| {
                    ct.prepend_left_with_generated_marker(
                        inject_pos,
                        RUNTIME_TEMPLATE_HOLE,
                        0,
                        RUNTIME_TEMPLATE_HOLE.len() as u32,
                    )
                })
            })
            .flatten();

        // Emit imports. `runtime_imports` (below) stays the union of both
        // sets for output metadata regardless of topology — only the
        // CODE-TEXT emission differs by topology.
        let all_imports: Vec<&'static str> = if inline_active {
            let mut v = script_result.imports.clone();
            v.extend(inline_tpl_imports.iter().copied());
            v
        } else {
            script_result.imports.clone()
        };
        let emit_import_line = |ct: &mut CodeTransform<'_>, names: &[&'static str]| {
            if names.is_empty() {
                return;
            }
            let runtime = options.runtime_module_name.as_deref().unwrap_or("vue");
            let mut sorted = names.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            let specifiers: Vec<String> = sorted
                .iter()
                .map(|name| format_import_specifier(name))
                .collect();
            let import_line = format!(
                "import {{ {} }} from \"{}\"\n",
                specifiers.join(", "),
                runtime,
            );
            ct.prepend(&import_line);
        };
        if inline_active && !script_result.imports.is_empty() && !inline_tpl_imports.is_empty() {
            // Official Vue's script-setup wrap codegen and its template
            // compiler independently prepend their OWN import line — they
            // are never merged into one deduplicated statement, even when
            // both import from the same module source (verified against
            // `@vue/compiler-sfc`: `compileScript`'s own `defineComponent`
            // helper import and `compileTemplate`'s returned preamble are
            // two separate `MagicString.prepend()` calls). `ct.prepend` is
            // LIFO (matching `MagicString.prepend`), so prepending the
            // template line first and the script line second puts the
            // script's import line frontmost, then the template's, then
            // the already-prepended hoisted-const preamble, then user code
            // — official's exact order.
            emit_import_line(&mut ct, &inline_tpl_imports);
            emit_import_line(&mut ct, &script_result.imports);
        } else {
            emit_import_line(&mut ct, &all_imports);
        }

        let script_code = ct.build_string();
        let generated_template_hole =
            runtime_template_marker.and_then(|marker| ct.generated_content_range(marker));
        // Resolve the declared `__sfc__` markers into built-output bytes —
        // only now, after every later edit (import hoisting, the inline
        // `move_slice`) has been applied and `build_string()` has run.
        // Positions can still shift after the rename target is written, so
        // this must run last, never at the point of writing.
        let sfc_binding_ranges: Vec<std::ops::Range<u32>> = script_result
            .sfc_binding_markers
            .iter()
            .filter_map(|marker| ct.generated_content_range(*marker))
            .collect();
        let sfc_export_statement_range = script_result
            .sfc_export_statement_marker
            .and_then(|marker| ct.generated_content_range(marker));
        let sfc_export_placement =
            if sfc_binding_ranges.is_empty() && sfc_export_statement_range.is_none() {
                None
            } else {
                Some(crate::assembly::fragment::SfcExportPlacement {
                    binding_ranges: sfc_binding_ranges,
                    export_statement_range: sfc_export_statement_range,
                })
            };
        let sourcemap_start = Instant::now();
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
        let sourcemap_duration_ms = sourcemap_start.elapsed().as_secs_f64() * 1000.0;
        let script_duration_ms = script_start.elapsed().as_secs_f64() * 1000.0;
        // Emit phase boundaries: transform (script) excludes the
        // sourcemap chunk so timings stay disjoint and the audit
        // payload's `transform_ms` / `sourcemap_ms` add up to roughly
        // the wall-clock cost of this block.
        if let Some(observer) = verter_audit::current_observer() {
            let transform_only_ms = (script_duration_ms - sourcemap_duration_ms).max(0.0);
            observer.record_phase_timing("compile.transform", transform_only_ms);
            if verter_options.source_map {
                observer.record_phase_timing("compile.sourcemap", sourcemap_duration_ms);
            }
        }

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
                generated_template_hole,
                runtime_imports: all_imports.clone(),
                sfc_export_placement,
            })
        } else if has_scoped_style || use_vapor || verter_options.ssr {
            // Template-only component with scoped styles, vapor mode, or SSR:
            // Emit a synthetic script block so __scopeId / __vapor propagates
            // to consumers (playground, bundler, etc.).
            use crate::assembly::fragment::SfcExportPlacement;
            use crate::script::{push_default_export_statement, push_sfc_binding};
            let mut code = String::with_capacity(128);
            let mut binding_ranges = Vec::with_capacity(3);
            // Official `@vue/compiler-sfc` (`compileScript`'s non-TS
            // `runtimeOptions` string) builds `__vapor: true` as an INLINE
            // object-literal property, never a separate trailing
            // `__sfc__.__vapor = true` assignment — confirmed directly
            // against the vendored rc.5 compiler source and against the
            // pinned rc.5 golden for `slots.vue`'s template-only vapor
            // cell (`const _sfc_main = { __vapor: true }`). `__scopeId`
            // is a DIFFERENT, bundler-level mechanism in real
            // `@vitejs/plugin-vue` (`attachedProps` + the `_export_sfc`
            // helper, not `compileScript`'s `runtimeOptions` at all) — its
            // existing separate-statement emission is left untouched;
            // neither in-scope seed fixture exercises scoped styles.
            code.push_str("const ");
            binding_ranges.push(push_sfc_binding(&mut code));
            if use_vapor {
                code.push_str(" = { __vapor: true };\n");
            } else {
                code.push_str(" = {};\n");
            }
            if has_scoped_style {
                binding_ranges.push(push_sfc_binding(&mut code));
                code.push_str(".__scopeId = \"");
                code.push_str(&scope_id_full);
                code.push_str("\";\n");
            }
            // Non-inline SSR attaches `ssrRender` on the component after
            // template codegen; do not claim `__ssrInlineRender`.
            let (export_binding_range, export_statement_range) =
                push_default_export_statement(&mut code);
            binding_ranges.push(export_binding_range);
            Some(VerterScriptBlock {
                code,
                duration_ms: script_duration_ms,
                source_map: String::new(),
                setup: false,
                attrs: Vec::new(),
                generated_template_hole: None,
                runtime_imports: Vec::new(),
                sfc_export_placement: Some(SfcExportPlacement {
                    binding_ranges,
                    export_statement_range: Some(export_statement_range),
                }),
            })
        } else {
            // A completely empty SFC is a valid EMPTY component (see
            // `empty_sfc_script_block`); anything else has no script block.
            empty_sfc_script_block(
                parsed,
                &custom_blocks,
                &component_name,
                options.runtime_module_name.as_deref().unwrap_or("vue"),
                script_duration_ms,
            )
        };
    } // end if needs_script

    let template_binding_metadata = TemplateBindingMetadata {
        bindings: script_bindings
            .iter()
            .map(|(name, kind)| ((*name).to_string(), *kind))
            .collect(),
        has_script: parsed.script().is_some()
            || parsed.script_setup().is_some()
            || transferred_bindings.is_some_and(|metadata| metadata.has_script),
        const_props: verter_options
            .prop_constness_overrides
            .clone()
            .or_else(|| transferred_bindings.and_then(|m| m.const_props.clone())),
        ref_bindable_imports: ref_bindable_imports.clone(),
    };

    // ── 5. Template codegen ───────────────────────────────────────
    // Borrow the template AST (it may be needed for both VDOM and TSX codegen).
    let template_ast_opt: Option<&_> = if !has_parse_errors {
        parsed.template_ast()
    } else {
        None
    };

    let needs_tpl_codegen = options.target.needs_template_codegen();
    let needs_tpl_data = options.target.needs_template_data();
    // Diagnostics attributable to the template-data extraction pass itself
    // (expression parse errors). A subset of `errors`, carried separately on
    // the result so the template-facts consumer can publish them.
    let mut template_data_diagnostics: Vec<CompileDiagnostic> = Vec::new();

    let (template_block, extracted_template_data) = if has_parse_errors
        || (!needs_tpl_codegen && !needs_tpl_data)
    {
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

            // Reuse the runtime overlay — the single `ide_completion = false`
            // `tsx()` entry the early script-elision lane built (or build it
            // cold here when this is the first runtime consumer). Runtime
            // (VDOM/Vapor) codegen keeps completion-prefix matching off so
            // partial identifiers stay real references.
            let oxc_ast = expr_store.get_or_build(
                template_ast,
                input,
                allocator,
                template_region_span(template_ast),
                &parse_options,
                source_type,
                false,
            );

            // Collect OXC expression parse errors as XInvalidExpression diagnostics.
            // The appended delta is ALSO recorded as the template-data
            // extraction's own diagnostic slice: this pass is the only one
            // that parses these expressions when no template codegen target
            // is requested, so the template-facts consumer republishes the
            // slice rather than re-running the pass (or inheriting unrelated
            // channels such as macro-semantic validation).
            let expr_diag_start = all_diagnostics.len();
            let expr_failure_start = compile_failures.len();
            collect_expression_errors(oxc_ast, input, &mut all_diagnostics, &mut compile_failures);
            if needs_tpl_data {
                template_data_diagnostics =
                    convert_diagnostics(&all_diagnostics[expr_diag_start..]);
                template_data_diagnostics
                    .extend_from_slice(&compile_failures[expr_failure_start..]);
            }

            // Extract raw template data for cross-file analysis (before bindings are moved)
            let raw_template_data = if needs_tpl_data {
                Some(template_data::extract_raw_template_data(
                    template_ast,
                    oxc_ast,
                    input,
                    &script_bindings,
                ))
            } else {
                None
            };

            let template_block_inner = if needs_tpl_codegen && !inline_active {
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

                let ssr_css_vars = if verter_options.ssr {
                    // Dedup by var_name (same v-bind may appear in multiple style blocks)
                    let mut seen = rustc_hash::FxHashSet::default();
                    all_v_bind_vars
                        .iter()
                        .filter(|v| seen.insert(v.var_name.clone()))
                        .map(|v| (v.var_name.clone(), v.expression.clone()))
                        .collect()
                } else {
                    Vec::new()
                };
                let tpl_options = TemplateCodeGenOptions {
                    mode: if verter_options.ssr {
                        CodeGenMode::Ssr
                    } else if use_vapor {
                        CodeGenMode::Vapor
                    } else {
                        CodeGenMode::Vdom
                    },
                    is_inline: verter_options.runtime_inline_template_chunk,
                    is_production: options.is_production,
                    comments: options.comments.unwrap_or(!options.is_production),
                    force_js: verter_options.force_js,
                    self_name: to_pascal_case(&component_name),
                    const_props: verter_options
                        .prop_constness_overrides
                        .clone()
                        .or_else(|| transferred_bindings.and_then(|m| m.const_props.clone())),
                    // Full 6-param render signature only when the SFC has a
                    // script block (official: `bindingMetadata && !inline`).
                    has_script: parsed.script().is_some()
                        || parsed.script_setup().is_some()
                        || transferred_bindings.is_some_and(|metadata| metadata.has_script),
                    ref_bindable_imports: ref_bindable_imports.clone(),
                    has_scoped_style,
                    hoist_static: options.resolve_hoist_static(),
                    scope_id: if has_scoped_style {
                        scope_id_full.clone()
                    } else {
                        String::new()
                    },
                    ssr_css_vars,
                };

                let tpl_imports = generate_template(
                    template_ast,
                    oxc_ast,
                    input,
                    &mut tpl_ct,
                    &tpl_alloc,
                    script_bindings,
                    &tpl_options,
                );
                // See the merged/inline_active lane's identical call for why
                // this goes through `ct.prepend` rather than a
                // position-anchored prepend. This standalone lane's `tpl_ct`
                // has no script content of its own to race against, but the
                // consumption stays symmetric with the merged lane.
                if let Some(preamble) = tpl_imports.module_preamble {
                    tpl_ct.prepend(preamble);
                }

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
                let tpl_sourcemap_start = Instant::now();
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
                let tpl_sourcemap_ms = tpl_sourcemap_start.elapsed().as_secs_f64() * 1000.0;
                let tpl_duration_ms = tpl_start.elapsed().as_secs_f64() * 1000.0;
                if let Some(observer) = verter_audit::current_observer() {
                    let codegen_only_ms = (tpl_duration_ms - tpl_sourcemap_ms).max(0.0);
                    observer.record_phase_timing("compile.codegen", codegen_only_ms);
                    if verter_options.source_map {
                        observer.record_phase_timing("compile.sourcemap", tpl_sourcemap_ms);
                    }
                }

                let tpl_attrs = extract_attrs(&template_ast.root.attributes, input);

                Some(VerterTemplateBlock {
                    code: tpl_code,
                    source_map: tpl_source_map,
                    imports: tpl_imports.vue,
                    ssr_imports: tpl_imports.ssr,
                    render_export: if verter_options.ssr {
                        crate::framework_common::TemplateRenderExport::SsrRender
                    } else {
                        crate::framework_common::TemplateRenderExport::Render
                    },
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

        // Determine if this is a JS SFC (needs JSX+JSDoc output instead of TSX),
        // through the SHARED SFC script-dialect classification — the same one
        // the public-API surface labels itself with, so the validation carrier
        // and its API companion can never disagree about a file's ScriptKind.
        // The carrier is JSX-bearing by construction (the template lowers into
        // it), so only the JS-vs-TS axis is open here: `.jsx` or `.tsx`.
        let is_jsx =
            crate::parser::types::sfc_script_dialect(parsed.script_setup(), parsed.script())
                .is_javascript();

        // Extract CSS module class names for IDE completions
        let css_modules: Vec<ide::CssModuleInfo> = parsed
            .style_nodes()
            .iter()
            .filter(|s| s.module)
            .filter_map(|s| {
                let content_span = s.content.as_ref()?;
                let dialect = class_extraction_dialect(s.lang);
                let css_content = &input[content_span.start as usize..content_span.end as usize];
                let class_names = complete_static_class_names(css_content, dialect);
                if class_names.is_empty() {
                    return None;
                }
                // TODO: parse module="customName" from attributes; default is "$style"
                Some(ide::CssModuleInfo {
                    binding_name: "$style".to_string(),
                    class_names,
                })
            })
            .collect();

        // AST-driven inventory of identifiers the template references. Drives
        // unused-binding liveness in the script-setup lowering: a setup binding
        // used NOWHERE (not template, not script body, not style v-bind) must
        // emit a type-only unwrap entry so TS6133 fires at its source decl.
        //
        // Liveness MUST use the `ide_completion = false` overlay, NOT the IDE
        // template-codegen lane's `ide_completion = true` overlay: completion
        // mode intentionally suppresses real references (`BindingContext::
        // completion_prefixes`), so a genuinely-used binding would be reported
        // as unused and false-positive a TS6133. The zero-extra-parse
        // optimisation is therefore abandoned — `TemplateExprStore` reuses the
        // `false` overlay if the runtime/template-data lane already built it,
        // otherwise an extra parse is the accepted correctness cost.
        //
        // `None` is the conservative case (parse errors / template-less SFC):
        // every binding is treated as template-used so no false unused
        // diagnostic fires.
        let tsx_template_used_vars: Option<FxHashSet<String>> =
            if let (false, Some(template_ast_ref)) = (has_parse_errors, parsed.template_ast()) {
                let tsx_source_type = if is_jsx {
                    SourceType::jsx()
                } else {
                    SourceType::tsx()
                };
                let usage_oxc = expr_store.get_or_build(
                    template_ast_ref,
                    input,
                    allocator,
                    template_region_span(template_ast_ref),
                    &parse_options,
                    tsx_source_type,
                    false,
                );
                // Liveness REQUIRES completeness: a template-expression parse
                // error makes its references unknowable, so an incomplete result
                // collapses to `None` (fail open — no TS6133 demotion).
                let (used, complete) = template_expr_overlay::collect_template_used_vars(
                    usage_oxc,
                    template_ast_ref,
                    input,
                );
                if complete {
                    Some(used)
                } else {
                    None
                }
            } else {
                None
            };

        // SOUND style `v-bind()` usage, parsed from the raw `<style>` bodies (the
        // typed-AST path, not the host's `.split('.')` heuristic). Computed here
        // regardless of `target.needs_style()` because the IDE/TSX target does
        // not run full style codegen yet still needs sound style liveness. A
        // single unparseable `v-bind()` marks the set incomplete → fail open.
        let style_usage = if let Some(complete) = verter_options.style_v_bind_usage_complete {
            style_usage::StyleVBindUsage {
                used: verter_options.style_v_bind_vars.iter().cloned().collect(),
                complete,
            }
        } else {
            let mut usage = style_usage::extract_style_v_bind_usage_for_dialects(
                parsed.style_nodes().iter().filter_map(|style| {
                    let content = style.content.as_ref()?;
                    Some((
                        &input[content.start as usize..content.end as usize],
                        style_dialect(style.lang)?,
                    ))
                }),
            );
            usage.complete &= parsed
                .style_nodes()
                .iter()
                .all(|style| style_dialect(style.lang).is_some());
            usage
        };

        let tsx_script_opts = ide::IdeScriptOptions {
            component_name: &component_name,
            js_component_name: &js_component_name,
            filename: filename_str,
            scope_id: &scope_id_full,
            has_scoped_style,
            runtime_module_name: options.runtime_module_name.as_deref().unwrap_or("vue"),
            macro_runtime: validated_runtime,
            types_module_name: options
                .types_module_name
                .as_deref()
                .unwrap_or("@verter/types"),
            is_vapor: use_vapor,
            embed_ambient_types: options.embed_ambient_types,
            is_jsx,
            conditional_root_narrowing: options.conditional_root_narrowing,
            style_v_bind_vars: style_usage.used.iter().cloned().collect(),
            style_usage_complete: style_usage.complete,
            css_modules,
            template_used_vars: tsx_template_used_vars,
            custom_elements: options.custom_elements.as_deref(),
        };

        // Unified single CodeTransform for both script and template.
        // One CT → one source map → template diagnostics map correctly.
        let tsx_alloc = Allocator::new();
        let mut tsx_ct = CodeTransform::new(input, &tsx_alloc);
        // The per-file official Vue JSX authority must lead every TSX/JSX
        // carrier, including carriers whose first authored script declaration
        // maps to generated line/column zero. Put it in CodeTransform's
        // unmapped intro so source-map generation shifts authored mappings
        // structurally instead of a provider mutating mapped bytes later.
        tsx_ct.prepend(ide::VUE_JSX_PRAGMA);

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
        let generated_template_end = if options.ide_chunk_boundaries {
            Some(template_end.unwrap_or(0))
        } else {
            template_end
        };
        let mut tsx_script_result = ide::script::generate_ide_script(
            parsed.script(),
            parsed.script_setup(),
            template_ast_opt,
            input,
            &mut tsx_ct,
            &tsx_alloc,
            &tsx_script_opts,
            generated_template_end,
        );
        if let Some(metadata) = verter_options.template_binding_metadata.as_ref() {
            for (name, binding) in &metadata.bindings {
                tsx_script_result
                    .bindings
                    .insert(tsx_alloc.alloc_str(name), *binding);
            }
        }

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
        let mut generated_template_chunk = None;
        if !has_parse_errors {
            if let Some(template_ast) = template_ast_opt {
                let is_non_html = template_ast.root.lang.as_ref().is_some_and(|span| {
                    let v = &input[span.start as usize..span.end as usize];
                    !v.is_empty() && v != "html"
                });
                if !is_non_html {
                    // The IDE/TSX lane keys `ide_completion = true`, so even a TS
                    // SFC — whose runtime and TSX lanes both use `tsx()` — parses
                    // a distinct overlay entry from the runtime lane's `false`
                    // entry, because their stored binding facts differ for scoped
                    // completion. A JS SFC additionally parses with `jsx()`,
                    // another distinct key. Built in the top compile allocator so
                    // the facts outlive the lane that borrows them.
                    let tsx_source_type = if is_jsx {
                        SourceType::jsx()
                    } else {
                        SourceType::tsx()
                    };
                    // IDE/TSX codegen — enable completion-prefix matching so partial
                    // identifiers inside v-for / v-slot scopes stay bare for completion.
                    let tsx_oxc = expr_store.get_or_build(
                        template_ast,
                        input,
                        allocator,
                        template_region_span(template_ast),
                        &parse_options,
                        tsx_source_type,
                        true,
                    );
                    let mut tsx_out =
                        crate::template::code_gen::types::CodeGenOutput::new(&tsx_alloc);
                    let tsx_t_opts = ide::IdeTemplateOptions {
                        self_name: &to_pascal_case(&component_name),
                        comments: options.comments.unwrap_or(!options.is_production),
                        is_jsx,
                        strict_slots: options.strict_slots,
                        custom_elements: options.custom_elements.as_deref(),
                    };
                    ide::template::generate_ide_template(
                        template_ast,
                        tsx_oxc,
                        input,
                        &mut tsx_out,
                        &tsx_alloc,
                        &tsx_script_result.bindings,
                        &tsx_t_opts,
                        &tsx_script_result.template_component_bindings,
                    );
                    if options.ide_chunk_boundaries {
                        let mut chunk_ct = CodeTransform::new(input, &tsx_alloc);
                        tsx_out.clone().apply_to(&mut chunk_ct);
                        let code = chunk_ct.build_string();
                        let source_map = chunk_ct.generate_map_json(SourceMapOptions {
                            source: options.filename.as_deref(),
                            file: options.filename.as_deref(),
                            include_content: true,
                        });
                        generated_template_chunk =
                            Some(crate::compile::types::GeneratedCodeChunk { code, source_map });
                    }
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

        const GENERATED_TEMPLATE_HOLE: &str = "/* verter-generated-template-hole */";
        let mut generated_template_marker = None;
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
                if options.ide_chunk_boundaries {
                    let suffix = format!("{GENERATED_TEMPLATE_HOLE}{suffix}");
                    generated_template_marker = tsx_ct.move_with_suffix_and_generated_marker(
                        ts,
                        te,
                        move_target,
                        &suffix,
                        0,
                        GENERATED_TEMPLATE_HOLE.len() as u32,
                    );
                } else {
                    tsx_ct.move_with_suffix(ts, te, move_target, suffix);
                }
            }
        } else if let (Some(return_close), Some(pos)) = (
            &tsx_script_result.return_close,
            tsx_script_result.return_close_pos,
        ) {
            if options.ide_chunk_boundaries {
                let content = format!("{GENERATED_TEMPLATE_HOLE}{return_close}");
                generated_template_marker = tsx_ct.prepend_left_with_generated_marker(
                    pos,
                    &content,
                    0,
                    GENERATED_TEMPLATE_HOLE.len() as u32,
                );
            } else {
                tsx_ct.prepend_left(pos, return_close);
            }
        }
        if options.ide_chunk_boundaries && tsx_script_result.return_close.is_none() {
            let script_end = parsed
                .script_setup()
                .or_else(|| parsed.script())
                .and_then(|script| script.content.as_ref().map(|content| content.end));
            if let Some(script_end) = script_end {
                generated_template_marker = tsx_ct.prepend_left_with_generated_marker(
                    script_end,
                    GENERATED_TEMPLATE_HOLE,
                    0,
                    GENERATED_TEMPLATE_HOLE.len() as u32,
                );
            }
        }

        // Append type constructs via CT outro — they have no sourcemap mapping
        // but must go through the CT so it remains the single source of truth.
        if !tsx_script_result.type_constructs.is_empty() {
            tsx_ct.append(&tsx_script_result.type_constructs);
        }

        // Build output and source map from the single unified CT
        let tsx_code = tsx_ct.build_string();
        let tsx_sourcemap_start = Instant::now();
        let tsx_sm = if verter_options.ide_source_map {
            let sm_opts = SourceMapOptions {
                source: options.filename.as_deref(),
                file: options.filename.as_deref(),
                include_content: true,
            };
            // Carry the typed helper-import-preamble end boundary on the IDE source map so the LSP
            // auto-import classifier can re-anchor preamble insertions and reject trailing-synthetic
            // edits even when the file has no mapped runs (an empty `<script setup>`).
            tsx_ct.generate_map_json_with_preamble(sm_opts)
        } else {
            String::new()
        };
        let tsx_sourcemap_ms = tsx_sourcemap_start.elapsed().as_secs_f64() * 1000.0;
        let tsx_dur = tsx_start.elapsed().as_secs_f64() * 1000.0;
        if let Some(observer) = verter_audit::current_observer() {
            let codegen_only_ms = (tsx_dur - tsx_sourcemap_ms).max(0.0);
            observer.record_phase_timing("compile.codegen", codegen_only_ms);
            if verter_options.ide_source_map {
                observer.record_phase_timing("compile.sourcemap", tsx_sourcemap_ms);
            }
        }

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
                verter_debug_assert!(
                    end.is_some(),
                    "Found start marker but not end marker in TSX output"
                );
                if let Some(end) = end {
                    meta.block_end = (end + END_MARKER.len()) as u32;
                }
            } else {
                verter_debug_assert!(
                    false,
                    "destructured_block metadata exists but start marker not found in TSX output"
                );
            }
        }

        let generated_template_hole =
            generated_template_marker.and_then(|marker| tsx_ct.generated_content_range(marker));

        Some(VerterTsxBlock {
            code: tsx_code,
            source_map: tsx_sm,
            duration_ms: tsx_dur,
            is_jsx,
            destructured_block,
            generated_template_hole,
            generated_template_chunk,
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
                mode: tsc::TscMode::Public,
            },
            macro_semantics.tsc().map_or(
                tsc::MacroTscInput::NotRequired,
                tsc::MacroTscInput::Authoritative,
            ),
            // The in-crate compile pipeline has no inheritance resolver — that
            // lives in `verter_session`, above this crate. The type-checked
            // `.tsc.tsx` a consumer actually sees is rendered by the session's
            // public-API projector (`get_public_api` / `get_public_api_batch`),
            // which supplies the resolved surface; this block is the
            // bundler-facing artifact and widens nothing.
            &tsc::FallthroughPropsProjection::none(),
        );
        let tsc_dur = tsc_start.elapsed().as_secs_f64() * 1000.0;
        if let Some(observer) = verter_audit::current_observer() {
            observer.record_phase_timing("compile.codegen", tsc_dur);
        }
        match tsc_out {
            Ok(tsc_out) => Some(VerterTsxBlock {
                code: tsc_out.code,
                source_map: tsc_out.source_map,
                duration_ms: tsc_dur,
                is_jsx: false,
                destructured_block: None,
                generated_template_hole: None,
                generated_template_chunk: None,
            }),
            Err(error) => {
                compile_failures.push(tsc_generation_diagnostic(error));
                None
            }
        }
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

    Ok(VerterCompileResult {
        script: script_block,
        template: template_block,
        styles: style_blocks,
        custom_blocks,
        scope_id: scope_id_result,
        errors: {
            crate::diagnostics::sort_diagnostics(&mut all_diagnostics);
            let mut errors = convert_diagnostics(&all_diagnostics);
            errors.append(&mut compile_failures);
            errors
        },
        parse_duration_ms,
        total_duration_ms,
        tsx: tsx_block,
        tsc: tsc_block,
        template_data: extracted_template_data,
        template_data_diagnostics,
        // True when the render function was inlined into `setup()` (official
        // production topology) — the script block already contains the full
        // component, and no separate template block was emitted. Gated on the
        // runtime script lane actually running: the IDE/TSX-only target emits
        // no runtime inline body, so the flag must not be set merely because
        // the option is on.
        inline: inline_active && options.target.needs_script(),
        // The compiler is cache-mode agnostic — it produces output for a
        // single direct invocation. The host's cache routing wraps this
        // call; a bare `compile()` reports the default Session mode with
        // no downgrade. Host-side cache-routed paths set the public
        // result fields from their own classification.
        requested_mode: verter_audit::payloads::tags::CompileCacheModeTag::Session,
        actual_mode: verter_audit::payloads::tags::CompileCacheModeTag::Session,
        downgrade_reason: None,
        template_binding_metadata,
    })
}

/// Test-only compatibility shim reproducing the legacy
/// `CodegenOptions`/`VerterCompileOptions`/`compile()` shape, so the
/// crate's large pre-existing codegen test suite (tens of thousands of
/// lines across ~11 files, each exercising specific option COMBINATIONS
/// via hand-built `CompileTarget` bitsets) does not need a line-by-line
/// rewrite into `CompileRequest`-shaped construction. NEVER compiled into
/// production (`#[cfg(test)]`) — it is not a second production option
/// authority; it is scaffolding over the SAME canonical `CompileRequest`
/// constructor every production route now uses. Test files shadow the
/// production `CodegenOptions`/`VerterCompileOptions`/`compile` names
/// with these via an explicit `use ... as` import (Rust's ordinary
/// glob-shadowing rule), so individual test bodies need no changes.
#[cfg(test)]
#[allow(dead_code)] // compatibility-shape fields/methods not every test file exercises
pub(crate) mod legacy_test_support {
    use super::*;
    use crate::compile_request::{
        AnalysisProductRequest, CompileProduct, CompileRequest, DeclarationProductRequest,
        FrameworkCompileRequest, IdeProductRequest, RuntimeProductRequest, VueBackendRequest,
        VueCompileRequest,
    };

    /// Exact field-for-field mirror of the deleted public `CodegenOptions`.
    #[derive(Debug, Clone, Default)]
    pub(crate) struct CodegenOptions {
        pub filename: Option<String>,
        pub is_production: bool,
        pub custom_element: bool,
        pub component_id: Option<String>,
        pub target: CompileTarget,
        pub skip_source_map: bool,
        pub ide_chunk_boundaries: bool,
        pub delimiters: Option<(String, String)>,
        pub custom_elements: Option<Vec<String>>,
        pub comments: Option<bool>,
        pub runtime_module_name: Option<String>,
        pub types_module_name: Option<String>,
        pub hoist_static: Option<bool>,
        pub whitespace: Option<WhitespaceStrategy>,
        pub cache_handlers: Option<bool>,
        pub inline: Option<bool>,
        pub slotted: Option<bool>,
        pub prefix_identifiers: Option<bool>,
        pub embed_ambient_types: bool,
        pub conditional_root_narrowing: bool,
        pub strict_slots: bool,
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
        pub fn with_custom_element(mut self, custom_element: bool) -> Self {
            self.custom_element = custom_element;
            self
        }
    }

    /// Exact field-for-field mirror of the deleted public `VerterCompileOptions`.
    #[derive(Debug, Clone, Default)]
    pub(crate) struct VerterCompileOptions {
        pub force_vapor: bool,
        pub force_js: bool,
        pub source_map: bool,
        pub ssr: bool,
        pub extract_template_data: bool,
        pub prop_constness_overrides: Option<rustc_hash::FxHashSet<String>>,
        pub style_v_bind_vars: Vec<String>,
        pub style_v_bind_usage_complete: Option<bool>,
        pub template_binding_metadata: Option<TemplateBindingMetadata>,
        pub template_used_vars: Option<rustc_hash::FxHashSet<String>>,
        pub runtime_template_hole: bool,
        pub runtime_inline_template_chunk: bool,
    }

    /// Rebuilds a canonical `CompileRequest` from the legacy bit/flag shape
    /// — the reverse of `derive_legacy_vue_options`. Not bit-perfect for
    /// every theoretical `CompileTarget` value (no production caller ever
    /// constructs one directly anymore); faithful for every combination the
    /// test suite actually exercises: `BUNDLER` (`TEMPLATE` bit present),
    /// `IDE`/`TSX`, `TSC`, `ANALYSIS`/`META`-shaped (`SCRIPT` without
    /// `TEMPLATE`), `TEMPLATE_DATA` alone, and any OR-combination of these.
    fn request_from_legacy(
        options: &CodegenOptions,
        verter_options: &VerterCompileOptions,
    ) -> CompileRequest {
        let target = options.target;
        let mut products = Vec::new();
        if target.contains(CompileTarget::TEMPLATE) {
            let rp = RuntimeProductRequest {
                inline: options.inline,
                runtime_source_map: verter_options.source_map,
                ..Default::default()
            };
            products.push(if verter_options.ssr {
                CompileProduct::RuntimeServer(rp)
            } else {
                CompileProduct::RuntimeClient(rp)
            });
        }
        if target.contains(CompileTarget::TSX) {
            products.push(CompileProduct::IdeCompanion(IdeProductRequest {
                want_source_map: verter_options.source_map,
                embed_ambient_types: options.embed_ambient_types,
                conditional_root_narrowing: options.conditional_root_narrowing,
                strict_slots: options.strict_slots,
                types_module_name: options.types_module_name.clone(),
                ide_chunk_boundaries: options.ide_chunk_boundaries,
                ..Default::default()
            }));
        }
        if target.contains(CompileTarget::TSC) {
            products.push(CompileProduct::Declarations(
                DeclarationProductRequest::default(),
            ));
        }
        let want_script_bindings_only =
            target.contains(CompileTarget::SCRIPT) && !target.contains(CompileTarget::TEMPLATE);
        let want_template_data =
            target.contains(CompileTarget::TEMPLATE_DATA) || verter_options.extract_template_data;
        if want_script_bindings_only || want_template_data {
            products.push(CompileProduct::Analysis(AnalysisProductRequest {
                want_script_bindings: want_script_bindings_only,
                want_template_data,
            }));
        }
        if products.is_empty() {
            // A target with no recognized bit still needs SOME product for
            // `CompileRequest::new` to accept it; fall back to a runtime
            // client so an all-default `CodegenOptions` (target: BUNDLER
            // via `Default`) behaves as the tests expect.
            products.push(CompileProduct::RuntimeClient(RuntimeProductRequest {
                inline: options.inline,
                runtime_source_map: verter_options.source_map,
                ..Default::default()
            }));
        }

        let vue = VueCompileRequest {
            backend: if verter_options.force_vapor {
                VueBackendRequest::Vapor
            } else {
                VueBackendRequest::Inferred
            },
            ssr: verter_options.ssr,
            is_custom_element: options.custom_elements.clone().unwrap_or_default(),
            delimiters: options.delimiters.clone(),
            comments: options.comments,
            hoist_static: options.hoist_static,
            runtime_module_name: options.runtime_module_name.clone(),
            script_custom_element: Some(options.custom_element),
            ..Default::default()
        };

        CompileRequest::new(
            products,
            FrameworkCompileRequest::Vue(vue),
            None,
            options.filename.clone(),
            options.component_id.clone(),
            options.is_production,
            verter_options.force_js,
        )
        .expect("legacy-shaped test options always translate to a constructible request")
    }

    fn execution_inputs_from_legacy(verter_options: &VerterCompileOptions) -> VueExecutionInputs {
        VueExecutionInputs {
            // The legacy shape threads macro semantics through the separate
            // `macro_semantics: &VueMacroSemanticInput` parameter, unchanged
            // from before this carrier existed — not through this field.
            macro_runtime: None,
            prop_constness_overrides: verter_options.prop_constness_overrides.clone(),
            style_v_bind_vars: verter_options.style_v_bind_vars.clone(),
            style_v_bind_usage_complete: verter_options.style_v_bind_usage_complete,
            template_binding_metadata: verter_options.template_binding_metadata.clone(),
            template_used_vars: verter_options.template_used_vars.clone(),
            runtime_template_hole: verter_options.runtime_template_hole,
            runtime_inline_template_chunk: verter_options.runtime_inline_template_chunk,
            prepared_styles: Vec::new(),
        }
    }

    pub(crate) fn compile(
        input: &str,
        options: &CodegenOptions,
        verter_options: &VerterCompileOptions,
        macro_semantics: &VueMacroSemanticInput,
        allocator: &Allocator,
    ) -> VerterCompileResult {
        let request = request_from_legacy(options, verter_options);
        let execution_inputs = execution_inputs_from_legacy(verter_options);
        super::compile(
            input,
            &request,
            &execution_inputs,
            macro_semantics,
            allocator,
        )
        .unwrap_or_else(|err| {
            panic!(
                "legacy test compile() call refused by the canonical request layer: {err:?} \
                 (a test exercising the NEW fail-closed behavior should call \
                 crate::compile_request::CompileRequest::new / compile directly, not this shim)"
            )
        })
    }

    pub(crate) fn compile_with_parsed(
        input: &str,
        options: &CodegenOptions,
        verter_options: &VerterCompileOptions,
        macro_semantics: &VueMacroSemanticInput,
        allocator: &Allocator,
    ) -> (ParsedSfc, VerterCompileResult) {
        let request = request_from_legacy(options, verter_options);
        let execution_inputs = execution_inputs_from_legacy(verter_options);
        super::compile_with_parsed(
            input,
            &request,
            &execution_inputs,
            macro_semantics,
            allocator,
        )
        .unwrap_or_else(|err| panic!("legacy test compile_with_parsed() call refused: {err:?}"))
    }

    pub(crate) fn compile_from_parsed(
        input: &str,
        parsed: &ParsedSfc,
        options: &CodegenOptions,
        verter_options: &VerterCompileOptions,
        macro_semantics: &VueMacroSemanticInput,
        allocator: &Allocator,
    ) -> VerterCompileResult {
        let request = request_from_legacy(options, verter_options);
        let execution_inputs = execution_inputs_from_legacy(verter_options);
        super::compile_from_parsed(
            input,
            parsed,
            &request,
            &execution_inputs,
            macro_semantics,
            allocator,
        )
        .unwrap_or_else(|err| panic!("legacy test compile_from_parsed() call refused: {err:?}"))
    }
}

#[cfg(test)]
#[path = "../compile_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../compile_template_error_tests.rs"]
mod compile_template_error_tests;
