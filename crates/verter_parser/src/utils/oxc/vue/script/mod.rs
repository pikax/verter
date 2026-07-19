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
pub mod setup;
pub mod shared;
pub mod types;
pub mod usage;

use oxc_ast::ast::Program;

pub use macros::{
    detect_macro_kind, is_define_component, MacroArrayArg, MacroArrayElement, MacroDeclarator,
    MacroObjectArg, MacroProperty, MacroTypeParams, RuntimeConstructorSyntax, ScriptMacro,
    VueMacroKind,
};
pub use setup::{extract_options_component_macro_args, OptionsComponentMacroArgs};
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

    // Second pass: mode-specific syntax processing and binding extraction.
    let bindings = match mode {
        ScriptMode::Setup => {
            let mut setup_ctx = SetupContext::new();
            process_setup_statements(&program.body, &ctx, &mut setup_ctx, &mut items, &mut errors);
            is_async = setup_ctx.is_async;

            bindings::extract_bindings(program, &ctx)
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

    ScriptParseResult {
        is_async,
        items,
        errors,
        bindings,
    }
}

#[cfg(test)]
mod tests;
