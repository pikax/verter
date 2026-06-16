//! Vue SFC script parsing module.
//!
//! This module provides parsing for Vue Single File Component script blocks,
//! supporting both Options API (`<script>`) and Script Setup (`<script setup>`).
//!
//! # Example
//!
//! ```ignore
//! use verter_parser::syntax::plugins::analysis::plugins::script::{
//!     parse_script, ScriptMode, ScriptParseResult,
//! };
//!
//! let result = parse_script(program, ScriptMode::Setup, base_offset, source);
//! ```

pub mod bindings;
pub mod macros;
pub mod options;
pub mod raw_surface;
pub mod resolve_type;
pub mod setup;
pub mod shared;
pub mod types;
pub mod usage;

use oxc_ast::ast::Program;

pub use macros::{
    detect_macro_kind, is_define_component, MacroArrayArg, MacroArrayElement, MacroDeclarator,
    MacroObjectArg, MacroProperty, MacroTypeParams, ScriptMacro, VueMacroKind,
};
pub use resolve_type::{
    build_type_context, extract_companion_types, format_runtime_types, resolve_type_elements,
    resolve_type_elements_with_ctx, resolve_type_elements_with_ctx_ref,
    take_last_resolution_budget_exceeded, DiagnosticLocation, ResolutionBudgetExceeded,
    ResolutionDiagnostic, ResolutionDiagnosticKind, ResolvedElements, ResolvedEmit,
    ResolvedEmitSignature, RuntimeType, TypeResolutionContext, PARSER_SYNTACTIC_DEPTH_LIMIT,
};
pub use shared::ScriptParseContext;
pub use types::*;
pub use usage::{
    detect_vue_api_call,
    // Template usage types
    BindingRefContext,
    BindingRefInfo,
    // Sync context tracking (getCurrentInstance safety)
    CallSiteContext,
    ComponentUsageInfo,
    // Loop & render performance tracking
    ConditionLikelihood,
    EmitCallUsage,
    EmitEventName,
    FileUsageFlags,
    InjectUsage,
    IterableType,
    LifecycleHook,
    LifecycleUsage,
    LoopChildren,
    LoopInfo,
    ProvideKey,
    ProvideKeyKind,
    ProvideUsage,
    ReactiveKind,
    ReactiveStateUsage,
    RenderPatternWarning,
    SlotDefinitionInfo,
    SlotName,
    SlotUsageInfo,
    StaticConditionValue,
    SyncContextUsage,
    TemplateMetrics,
    TemplateRefAttrUsage,
    TemplateUsageCollector,
    TemplateUtilUsage,
    UsageCollector,
    VueApiCategory,
    VueApiKind,
    WarningSeverity,
    WatcherUsage,
};

use options::{process_options_statements, OptionsContext};
use setup::{process_setup_statements, SetupContext};
use shared::{try_process_export, try_process_import};

/// Parse a Vue SFC script block.
///
/// This is the main entry point for script parsing. It walks the already-parsed
/// OXC Program AST and extracts relevant information based on the script mode.
///
/// # Arguments
///
/// * `program` - The OXC-parsed program AST
/// * `mode` - The script mode (Options or Setup)
/// * `content_offset` - The byte offset for unadjusted TypeScript type annotation spans.
///   In the `syntax` pipeline this is `content_start` (where script content begins in the SFC).
///   In direct parsing (tests), this is 0 since spans are already local.
/// * `source` - The source text of the script content
///
/// # Returns
///
/// A `ScriptParseResult` containing all extracted items, async status, and errors.
pub fn parse_script<'a>(
    program: &'a Program<'a>,
    mode: ScriptMode,
    content_offset: u32,
    source: &'a str,
) -> ScriptParseResult<'a> {
    parse_script_with_companion(program, mode, content_offset, source, None)
}

/// Parse a script program with optional companion type information.
///
/// When `companion_types` is `Some`, type references in `defineProps<T>()`
/// that can't be resolved from the setup script's own declarations will
/// fall back to these pre-resolved types from the companion `<script>` block.
pub fn parse_script_with_companion<'a>(
    program: &'a Program<'a>,
    mode: ScriptMode,
    content_offset: u32,
    source: &'a str,
    companion_types: Option<rustc_hash::FxHashMap<String, resolve_type::ResolvedElements>>,
) -> ScriptParseResult<'a> {
    let ctx = ScriptParseContext::new(content_offset, source.as_bytes());
    let mut items = Vec::new();
    let mut errors = Vec::new();
    let mut is_async = false;

    // First pass: collect imports and exports (shared across modes)
    for stmt in &program.body {
        if let Some(import_item) = try_process_import(stmt, &ctx) {
            items.push(import_item);
        }
        if let Some(export_item) = try_process_export(stmt, &ctx) {
            items.push(export_item);
        }
    }

    // Second pass: mode-specific processing + binding extraction.
    //
    // The setup type-resolution context is built once and shared by the
    // statement pass and the binding pass — both need the same companion-aware
    // context, so building it twice (and re-resolving `defineProps<T>`) is pure
    // duplication. The binding pass reuses the macro pass's resolved prop-key
    // spans instead of resolving the macro type a second time.
    let bindings = match mode {
        ScriptMode::Setup => {
            let mut type_ctx = build_type_context(program, source.as_bytes(), content_offset);
            if let Some(companion) = companion_types {
                type_ctx.companion_types = companion;
            }
            let mut setup_ctx = SetupContext::new();
            process_setup_statements(
                &program.body,
                &ctx,
                &type_ctx,
                &mut setup_ctx,
                &mut items,
                &mut errors,
            );
            is_async = setup_ctx.is_async;

            let macro_prop_keys = collect_macro_prop_keys(&items);
            bindings::extract_bindings(program, &ctx, &type_ctx, Some(&macro_prop_keys))
        }
        ScriptMode::Options => {
            let mut options_ctx = OptionsContext::new();
            process_options_statements(
                &program.body,
                &ctx,
                &mut options_ctx,
                &mut items,
                &mut errors,
                &mut is_async,
            );
            options::extract_options_bindings(program)
        }
    };

    // Collect dynamic import() calls with .vue specifiers for IDE rewriting
    let vue_dynamic_import_spans = collect_vue_dynamic_imports(program);

    ScriptParseResult {
        is_async,
        items,
        errors,
        bindings,
        vue_dynamic_import_spans,
    }
}

/// Collect the local prop-key spans the macro pass already resolved for
/// `defineProps<T>()` (and `withDefaults(defineProps<T>(), …)`), so the binding
/// pass can reuse them instead of resolving the macro type a second time.
///
/// Only members whose `key` span addresses the local SFC are kept (`map_local`):
/// a cross-file member's `key` span points into another source file and must not
/// become a setup binding. This matches the binding pass's pre-existing
/// local-only span output, which resolved the type without the companion
/// fallback and so only ever produced local members.
fn collect_macro_prop_keys(items: &[ScriptItem]) -> Vec<crate::common::Span> {
    let mut keys = Vec::new();
    for item in items {
        let type_params = match item {
            ScriptItem::Macro(ScriptMacro::DefineProps {
                type_params: Some(tp),
                ..
            }) => tp,
            ScriptItem::Macro(ScriptMacro::WithDefaults {
                define_props_type_params: Some(tp),
                ..
            }) => tp,
            _ => continue,
        };
        for prop in &type_params.resolved.props {
            if prop.map_local {
                keys.push(prop.key);
            }
        }
    }
    keys
}

/// Collect source literal spans of `import('./Foo.vue')` expressions.
///
/// Walks the full AST to find `ImportExpression` nodes whose source is a string
/// literal ending in `.vue`. Returns the span of each source literal (including quotes).
fn collect_vue_dynamic_imports(program: &Program<'_>) -> Vec<crate::common::Span> {
    use crate::common::Span;
    use oxc_ast::ast::{Expression, Statement};

    let mut spans = Vec::new();

    fn scan_expr(expr: &Expression<'_>, spans: &mut Vec<Span>) {
        if let Expression::ImportExpression(import) = expr {
            if let Expression::StringLiteral(lit) = &import.source {
                if lit.value.ends_with(".vue") {
                    spans.push(Span::from(lit.span));
                }
            }
        }
        // Recurse into common expression forms
        match expr {
            Expression::CallExpression(call) => {
                scan_expr(&call.callee, spans);
                for arg in &call.arguments {
                    if let Some(expr) = arg.as_expression() {
                        scan_expr(expr, spans);
                    }
                }
            }
            Expression::ArrowFunctionExpression(arrow) => {
                for stmt in &arrow.body.statements {
                    scan_stmt(stmt, spans);
                }
            }
            Expression::ParenthesizedExpression(paren) => {
                scan_expr(&paren.expression, spans);
            }
            Expression::ConditionalExpression(cond) => {
                scan_expr(&cond.test, spans);
                scan_expr(&cond.consequent, spans);
                scan_expr(&cond.alternate, spans);
            }
            Expression::LogicalExpression(log) => {
                scan_expr(&log.left, spans);
                scan_expr(&log.right, spans);
            }
            _ => {}
        }
    }

    fn scan_stmt(stmt: &Statement<'_>, spans: &mut Vec<Span>) {
        match stmt {
            Statement::ExpressionStatement(es) => scan_expr(&es.expression, spans),
            Statement::VariableDeclaration(vd) => {
                for decl in &vd.declarations {
                    if let Some(init) = &decl.init {
                        scan_expr(init, spans);
                    }
                }
            }
            Statement::ReturnStatement(ret) => {
                if let Some(arg) = &ret.argument {
                    scan_expr(arg, spans);
                }
            }
            Statement::BlockStatement(block) => {
                for s in &block.body {
                    scan_stmt(s, spans);
                }
            }
            _ => {}
        }
    }

    for stmt in &program.body {
        scan_stmt(stmt, &mut spans);
    }

    spans
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod resolve_type_typed_form_tests;

#[cfg(test)]
mod single_parse_dedup_tests;
