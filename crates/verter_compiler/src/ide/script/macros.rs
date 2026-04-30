//! Vue macro projection (D3 of Phase 11d ownership-domain analysis).
//!
//! Hosts the macro state types (`TsxMacroState`, `MacroBindingEntry`,
//! `ModelBindingEntry`, `MacroSourceCtx`), the `macro_span` extractor, and
//! the `process_*` family that emits the IDE-side macro projection (type
//! aliases, variable rebinds, inherit-attrs detection) from the parsed
//! `ScriptMacro` items.

use crate::common::Span;
use crate::template::code_gen::types::CodeGenOutput;
use crate::utils::oxc::vue::{MacroDeclarator, MacroTypeParams, ScriptItem, ScriptMacro};

use super::PREFIX;

// ── Macro State Types ────────────────────────────────────────────

/// Accumulated macro processing info for type constructs.
#[derive(Debug, Default)]
pub(super) struct TsxMacroState {
    /// Per-macro binding info.
    pub(super) macro_bindings: Vec<MacroBindingEntry>,
    /// DefineModel entries.
    pub(super) model_bindings: Vec<ModelBindingEntry>,
    /// Whether `defineOptions({ inheritAttrs: false })` was detected.
    pub(super) has_inherit_attrs_false: bool,
}

/// Info about a macro binding (defineProps, defineEmits, defineSlots, withDefaults).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) struct MacroBindingEntry {
    /// Original macro name: "defineProps", "defineEmits", etc.
    pub(super) macro_name: String,
    /// Variable name holding the macro result (e.g., `props` or `___VERTER___props`).
    pub(super) var_name: Option<String>,
    /// Type alias name if type params were used (e.g., `___VERTER___defineProps_Type`).
    pub(super) type_name: Option<String>,
    /// Whether this macro used type params.
    pub(super) is_type: bool,
}

/// Info about a defineModel binding.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) struct ModelBindingEntry {
    /// Model name (e.g., "modelValue" or "title").
    pub(super) model_name: String,
    /// Variable name holding the model ref.
    pub(super) var_name: String,
    /// Type alias name if type params were used.
    pub(super) type_name: Option<String>,
    /// Whether this model used type params.
    pub(super) is_type: bool,
}

/// Shared context for macro processing functions.
///
/// Groups the 4 parameters threaded through `process_single_macro`,
/// `process_standard_macro`, `process_define_model`, and `process_with_defaults`.
pub(super) struct MacroSourceCtx<'a, 'alloc> {
    pub(super) source: &'a str,
    pub(super) content_str: &'a str,
    pub(super) content_start: u32,
    pub(super) out: &'a mut CodeGenOutput<'alloc>,
    pub(super) is_jsx: bool,
}

/// Extract the span from a ScriptMacro variant.
pub(super) fn macro_span(m: &ScriptMacro<'_>) -> Span {
    match m {
        ScriptMacro::DefineProps { span, .. }
        | ScriptMacro::DefineEmits { span, .. }
        | ScriptMacro::DefineExpose { span, .. }
        | ScriptMacro::DefineOptions { span, .. }
        | ScriptMacro::DefineModel { span, .. }
        | ScriptMacro::DefineSlots { span, .. }
        | ScriptMacro::WithDefaults { span, .. } => *span,
    }
}

// ── Macro Boxing ──────────────────────────────────────────────────

/// Process all macros in the parsed items: emit type aliases (no boxing).
pub(super) fn process_macros(
    items: &[ScriptItem<'_>],
    ctx: &mut MacroSourceCtx<'_, '_>,
    skip_damaged: &[verter_span::Span],
) -> TsxMacroState {
    let mut state = TsxMacroState::default();

    for item in items {
        if let ScriptItem::Macro(mac) = item {
            // Skip macros whose spans overlap with parse errors
            if !skip_damaged.is_empty() {
                let span = macro_span(mac);
                if skip_damaged
                    .iter()
                    .any(|d| span.start < d.end && span.end > d.start)
                {
                    continue;
                }
            }
            process_single_macro(mac, ctx, &mut state);
        }
    }

    state
}

/// Process a single macro call: emit type alias if type params present.
fn process_single_macro(
    mac: &ScriptMacro<'_>,
    ctx: &mut MacroSourceCtx<'_, '_>,
    state: &mut TsxMacroState,
) {
    match mac {
        ScriptMacro::DefineProps {
            span,
            declarator,
            type_params,
            object_arg: _,
            array_arg: _,
        } => {
            let entry = process_standard_macro(
                "defineProps",
                "props",
                *span,
                declarator.as_ref(),
                type_params.as_ref(),
                false,
                ctx,
            );
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineEmits {
            span,
            declarator,
            type_params,
            object_arg: _,
            array_arg: _,
        } => {
            let entry = process_standard_macro(
                "defineEmits",
                "emits",
                *span,
                declarator.as_ref(),
                type_params.as_ref(),
                false,
                ctx,
            );
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineSlots {
            span,
            declarator,
            type_params,
        } => {
            let entry = process_standard_macro(
                "defineSlots",
                "slots",
                *span,
                declarator.as_ref(),
                type_params.as_ref(),
                false,
                ctx,
            );
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineExpose {
            span, declarator, ..
        } => {
            let entry = process_standard_macro(
                "defineExpose",
                "expose",
                *span,
                declarator.as_ref(),
                None,
                true,
                ctx,
            );
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineOptions {
            span,
            declarator,
            object_arg: _,
            has_inherit_attrs_false,
        } => {
            if *has_inherit_attrs_false {
                state.has_inherit_attrs_false = true;
            }
            let entry = process_standard_macro(
                "defineOptions",
                "options",
                *span,
                declarator.as_ref(),
                None,
                true,
                ctx,
            );
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineModel {
            span,
            declarator,
            type_params,
            name_span,
            options_span: _,
        } => {
            process_define_model(
                *span,
                declarator.as_ref(),
                type_params.as_ref(),
                *name_span,
                ctx,
                state,
            );
        }
        ScriptMacro::WithDefaults {
            span,
            declarator,
            define_props_span: _,
            define_props_type_params,
            defaults: _,
            defaults_arg_span: _,
        } => {
            process_with_defaults(
                *span,
                declarator.as_ref(),
                define_props_type_params.as_ref(),
                ctx,
                state,
            );
        }
    }
}

/// Process a standard macro (defineProps, defineEmits, defineSlots, defineExpose, defineOptions).
///
/// For type params: emit type alias, replace type param in call with alias name.
/// For no-declarator non-no-return macros: prepend `const ___VERTER___xxx=`.
#[allow(clippy::too_many_arguments)]
fn process_standard_macro(
    macro_name: &str,
    var_suffix: &str,
    call_span: Span,
    declarator: Option<&MacroDeclarator<'_>>,
    type_params: Option<&MacroTypeParams>,
    is_no_return: bool,
    ctx: &mut MacroSourceCtx<'_, '_>,
) -> MacroBindingEntry {
    let type_name_str = format!("{}{}_Type", PREFIX, macro_name);
    let auto_var_name = format!("{}{}", PREFIX, var_suffix);

    let has_type_params = type_params.is_some();

    // Determine the statement start position for prepending
    let stmt_start = declarator
        .map(|d| ctx.content_start + d.statement_span.start)
        .unwrap_or(ctx.content_start + call_span.start);

    // Emit type alias for type params
    if let Some(tp) = type_params {
        if ctx.is_jsx {
            // JSX mode: remove the generic type brackets entirely (JS has no generics)
            ctx.out.overwrite(tp.lt_span.start, tp.gt_span.end, "");
        } else {
            let type_text = &ctx.source[tp.type_span.start as usize..tp.type_span.end as usize];
            let needs_prettify = !is_simple_type_reference(type_text);

            let prefix = if needs_prettify {
                format!(";type {}={}Prettify<", type_name_str, PREFIX)
            } else {
                format!(";type {}=", type_name_str)
            };
            let suffix = if needs_prettify { ">;" } else { ";" };

            // Move original type content to stmt_start, wrapped with type declaration.
            // This preserves fine-grained sourcemap for hover on individual properties.
            ctx.out.move_wrapped(
                tp.type_span.start,
                tp.type_span.end,
                stmt_start,
                &prefix,
                suffix,
            );

            // Fill the gap left by the move with the type alias name
            ctx.out.prepend_alloc(tp.type_span.start, &type_name_str);
        }
    }

    // Add variable assignment if no declarator and not a no-return macro
    if declarator.is_none() && !is_no_return {
        let call_abs_start = ctx.content_start + call_span.start;
        ctx.out
            .prepend_alloc(call_abs_start, &format!("const {}=", auto_var_name));
    }

    // Handle destructured declarators: `const { foo } = defineProps(...)` →
    // `const ___VERTER___props = defineProps(...)`. Overwrite the destructuring
    // pattern with the auto var name so `__props = ___VERTER___props` resolves.
    if let Some(d) = declarator {
        if d.name.is_none() && !is_no_return {
            let binding_start = ctx.content_start + d.binding_span.start;
            let binding_end = ctx.content_start + d.binding_span.end;
            ctx.out
                .overwrite(binding_start, binding_end, &auto_var_name);
        }
    }

    // Determine the effective variable name
    let effective_var_name = if is_no_return && declarator.is_none() {
        None
    } else {
        Some(
            declarator
                .and_then(|d| d.name.map(|n| n.to_string()))
                .unwrap_or_else(|| auto_var_name.clone()),
        )
    };

    MacroBindingEntry {
        macro_name: macro_name.to_string(),
        var_name: effective_var_name,
        type_name: if has_type_params {
            Some(type_name_str)
        } else {
            None
        },
        is_type: has_type_params,
    }
}

/// Process defineModel macro.
fn process_define_model(
    call_span: Span,
    declarator: Option<&MacroDeclarator<'_>>,
    type_params: Option<&MacroTypeParams>,
    name_span: Option<Span>,
    ctx: &mut MacroSourceCtx<'_, '_>,
    state: &mut TsxMacroState,
) {
    // Determine model name
    let model_name = if let Some(ns) = name_span {
        let name_text = &ctx.content_str[ns.start as usize..ns.end as usize];
        name_text.trim_matches('\'').trim_matches('"').to_string()
    } else {
        "modelValue".to_string()
    };

    let prepend = format!("{}_", model_name);
    let type_name_str = format!("{}{}defineModel_Type", PREFIX, prepend);
    let auto_var_name = format!("{}models_{}", PREFIX, model_name);

    let has_type_params = type_params.is_some();

    let stmt_start = declarator
        .map(|d| ctx.content_start + d.statement_span.start)
        .unwrap_or(ctx.content_start + call_span.start);

    // Emit type alias for type params
    if let Some(tp) = type_params {
        if ctx.is_jsx {
            // JSX mode: remove the generic type brackets entirely (JS has no generics)
            ctx.out.overwrite(tp.lt_span.start, tp.gt_span.end, "");
        } else {
            let type_text = &ctx.source[tp.type_span.start as usize..tp.type_span.end as usize];
            let needs_prettify = !is_simple_type_reference(type_text);

            let prefix = if needs_prettify {
                format!(";type {}={}Prettify<", type_name_str, PREFIX)
            } else {
                format!(";type {}=", type_name_str)
            };
            let suffix = if needs_prettify { ">;" } else { ";" };

            // Move original type content to stmt_start, wrapped with type declaration.
            // This preserves fine-grained sourcemap for hover on individual properties.
            ctx.out.move_wrapped(
                tp.type_span.start,
                tp.type_span.end,
                stmt_start,
                &prefix,
                suffix,
            );

            // Fill the gap left by the move with the type alias name
            ctx.out.prepend_alloc(tp.type_span.start, &type_name_str);
        }
    }

    // Add variable assignment if no declarator
    if declarator.is_none() {
        let call_abs_start = ctx.content_start + call_span.start;
        ctx.out
            .prepend_alloc(call_abs_start, &format!("const {}=", auto_var_name));
    }

    let effective_var_name = declarator
        .and_then(|d| d.name.map(|n| n.to_string()))
        .unwrap_or_else(|| auto_var_name.clone());

    state.model_bindings.push(ModelBindingEntry {
        model_name,
        var_name: effective_var_name,
        type_name: if has_type_params {
            Some(type_name_str)
        } else {
            None
        },
        is_type: has_type_params,
    });
}

/// Process withDefaults(defineProps<T>(), { defaults }).
fn process_with_defaults(
    call_span: Span,
    declarator: Option<&MacroDeclarator<'_>>,
    define_props_type_params: Option<&MacroTypeParams>,
    ctx: &mut MacroSourceCtx<'_, '_>,
    state: &mut TsxMacroState,
) {
    let type_name_str = format!("{}defineProps_Type", PREFIX);
    let auto_var_name = format!("{}props", PREFIX);

    let has_type_params = define_props_type_params.is_some();

    let stmt_start = declarator
        .map(|d| ctx.content_start + d.statement_span.start)
        .unwrap_or(ctx.content_start + call_span.start);

    // Emit type alias for inner defineProps type params
    if let Some(tp) = define_props_type_params {
        if ctx.is_jsx {
            // JSX mode: remove the generic type brackets entirely (JS has no generics)
            ctx.out.overwrite(tp.lt_span.start, tp.gt_span.end, "");
        } else {
            let type_text = &ctx.source[tp.type_span.start as usize..tp.type_span.end as usize];
            let needs_prettify = !is_simple_type_reference(type_text);

            let prefix = if needs_prettify {
                format!(";type {}={}Prettify<", type_name_str, PREFIX)
            } else {
                format!(";type {}=", type_name_str)
            };
            let suffix = if needs_prettify { ">;" } else { ";" };

            // Move original type content to stmt_start, wrapped with type declaration.
            // This preserves fine-grained sourcemap for hover on individual properties.
            ctx.out.move_wrapped(
                tp.type_span.start,
                tp.type_span.end,
                stmt_start,
                &prefix,
                suffix,
            );

            // Fill the gap left by the move with the type alias name
            ctx.out.prepend_alloc(tp.type_span.start, &type_name_str);
        }
    }

    // Add variable assignment if no declarator
    if declarator.is_none() {
        let call_abs_start = ctx.content_start + call_span.start;
        ctx.out
            .prepend_alloc(call_abs_start, &format!("const {}=", auto_var_name));
    }

    let effective_var_name = declarator
        .and_then(|d| d.name.map(|n| n.to_string()))
        .unwrap_or_else(|| auto_var_name.clone());

    // Register defineProps binding (withDefaults wraps it)
    state.macro_bindings.push(MacroBindingEntry {
        macro_name: "defineProps".to_string(),
        var_name: Some(effective_var_name),
        type_name: if has_type_params {
            Some(type_name_str)
        } else {
            None
        },
        is_type: has_type_params,
    });
}

/// Check if a type string is a simple reference (identifier) that doesn't need Prettify wrapping.
pub(super) fn is_simple_type_reference(type_text: &str) -> bool {
    let trimmed = type_text.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Simple identifier: starts with letter/underscore, only alphanumeric/underscore/dots
    // Also handle qualified references like `Foo.Bar`
    trimmed
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && trimmed
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
}
