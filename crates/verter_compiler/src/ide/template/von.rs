//! IDE `v-on` / `@` directive → JSX expansion.
//!
//! `v-on` has several shapes that all relocate the bound handler into synthetic
//! JSX/spread scaffolding:
//! - `@click="handler"` → `onClick={handler}` (in-place value via `process_v_bind`-style split)
//! - `v-on="{ mousedown: doThis }"` → `{...{ onMousedown: doThis }}` (object spread)
//! - `@[event]="handler"` → `{...{[`on${event}` as any]: handler}}` (dynamic event name)
//! - duplicate / hyphenated events → `{...{ "onFoo": handler }}` (spread to avoid TS17001)
//!
//! Every navigable user expression (handler value, dynamic event-name expression,
//! object-property values) is planned through the unified `plan_user_expr` /
//! `plan_object_literal` planner and emitted through the relocated sink, so each
//! identifier maps 1:1 back to its source span; the event-key transformation,
//! object braces, computed-key template literals, and handler-wrapper scaffolding
//! are unmapped synthetic text. Extracted from `props.rs` to keep both files within
//! the production line-count budget.

use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;

use verter_span::{GeneratedByteLen, SourceByteOffset, SourceByteRange};

use super::props::get_prop_end;
use crate::ide::event_to_jsx_name;
use crate::ide::template::emit::{
    emit_expr_plan, emit_op, emit_relocated_value, plan_object_literal, plan_user_expr, trim_span,
    EmitOp, EmitText, ExprOptions, KeyRewritePolicy, Placement,
};
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::types::CodeGenOutput;
use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;
use crate::template::oxc::types::OxcParsedProp;
use crate::types::NodeProp;

/// Process `v-on` / `@` directive.
///
/// - `@click="handler"` → `onClick={handler}`
/// - `@click="handler($event)"` → `onClick={($event) => handler($event)}`
/// - `v-on="{ mousedown: doThis }"` → `{...{ mousedown: doThis }}` (spread, #49)
#[allow(clippy::too_many_arguments)]
pub(super) fn process_v_on<'alloc>(
    prop: &NodeProp,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
    v_if_guard: Option<&str>,
    use_spread: bool,
) {
    let has_arg = prop.arg_start.is_some();

    if !has_arg {
        // v-on="{ mousedown: doThis }" → spread `{...{ onMousedown: doThis }}`, OR
        // v-on="handlers" (non-object) → spread `{...handlers}`. The prop span is
        // deleted and the value re-emitted relocated at `prop.start`. Both forms
        // route through the unified planner so every handler identifier maps back
        // to its source span (never the foreign prop start).
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let prop_end = get_prop_end(prop);
            let at = SourceByteOffset(prop.start);

            // Object literal → property-aware rewrite (event-key remap + mapped
            // values via `plan_object_literal`). Returns `None` for an unsupported
            // static-key shape, in which case fall through to the bare spread.
            let object_plan = oxc_prop
                .and_then(|p| p.exp.as_ref())
                .filter(|exp| {
                    matches!(
                        exp.expression.as_ref(),
                        Some(Expression::ObjectExpression(_))
                    )
                })
                .and_then(|exp| {
                    let Some(Expression::ObjectExpression(obj)) = exp.expression.as_ref() else {
                        return None;
                    };
                    let bindings = exp.bindings.as_ref().map(|b| b.bindings.as_slice());
                    plan_object_literal(
                        source,
                        obj,
                        exp.offset,
                        bindings,
                        resolver,
                        KeyRewritePolicy::VOnEventObject,
                    )
                });

            out.overwrite(prop.start, prop_end, "");
            if let Some(plan) = object_plan {
                emit_expr_plan(out, &plan, Placement::Relocated { at }, source);
            } else {
                // NOT an object literal (or an unsupported static-key object): spread
                // the whole user expression as `{...<mapped expr>}`. The expression
                // is planned + relocated so its identifiers map to source — never a
                // flat unmapped insert.
                let (tvs, tve) = trim_span(source, vs, ve);
                let value_bindings = oxc_prop
                    .and_then(|p| p.exp.as_ref())
                    .and_then(|e| e.bindings.as_ref())
                    .map(|b| b.bindings.as_slice());
                emit_op(
                    out,
                    &EmitOp::InsertUnmapped {
                        at,
                        text: EmitText::Static("{..."),
                    },
                );
                emit_relocated_value(
                    out,
                    at,
                    source,
                    SourceByteRange::new(SourceByteOffset(tvs), SourceByteOffset(tve)),
                    value_bindings,
                    resolver,
                    ExprOptions::default(),
                );
                emit_op(
                    out,
                    &EmitOp::InsertUnmapped {
                        at,
                        text: EmitText::Static("}"),
                    },
                );
            }
        }
        return;
    }

    let arg_start = prop.arg_start.unwrap();
    let arg_end = prop.arg_end.unwrap();
    let event_name = &source[arg_start as usize..arg_end as usize];

    // Dynamic event name: @[eventName]="handler" → {...{[`on${eventName}`]: handler}}
    // Both the dynamic event-name expression and the handler are navigable user
    // expressions, so each is emitted relocated through the typed `EmitOp`
    // substrate; the computed-key template literal / object punctuation is unmapped.
    if prop.is_dynamic == Some(true) {
        let raw_arg = event_name
            .trim()
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
            .unwrap_or(event_name)
            .trim();
        let raw_arg_start =
            arg_start + (raw_arg.as_ptr() as usize - event_name.as_ptr() as usize) as u32;
        let raw_arg_end = raw_arg_start + raw_arg.len() as u32;
        let prop_end = get_prop_end(prop);

        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let (tvs, tve) = trim_span(source, vs, ve);
            out.overwrite(prop.start, prop_end, "");
            let at = SourceByteOffset(prop.start);
            let arg_bindings = oxc_prop
                .and_then(|p| p.arg.as_ref())
                .and_then(|a| a.bindings.as_ref())
                .map(|b| b.bindings.as_slice());
            let value_bindings = oxc_prop
                .and_then(|p| p.exp.as_ref())
                .and_then(|e| e.bindings.as_ref())
                .map(|b| b.bindings.as_slice());

            emit_op(
                out,
                &EmitOp::InsertUnmapped {
                    at,
                    text: EmitText::Static("{...{[`on${"),
                },
            );
            emit_relocated_value(
                out,
                at,
                source,
                SourceByteRange::new(
                    SourceByteOffset(raw_arg_start),
                    SourceByteOffset(raw_arg_end),
                ),
                arg_bindings,
                resolver,
                ExprOptions::default(),
            );
            emit_op(
                out,
                &EmitOp::InsertUnmapped {
                    at,
                    text: EmitText::Static("}` as any]: "),
                },
            );
            emit_relocated_value(
                out,
                at,
                source,
                SourceByteRange::new(SourceByteOffset(tvs), SourceByteOffset(tve)),
                value_bindings,
                resolver,
                ExprOptions::default(),
            );
            emit_op(
                out,
                &EmitOp::InsertUnmapped {
                    at,
                    text: EmitText::Static("}}"),
                },
            );
        } else {
            out.overwrite(prop.start, prop_end, "");
        }
        return;
    }

    // Convert event name to JSX: click → onClick, update:modelValue → onUpdate:modelValue
    let jsx_event_name = event_to_jsx_name(event_name);

    // Use spread syntax when:
    // 1. Duplicate event name on same element (TS17001: cannot have multiple same-name attrs)
    // 2. JSX name contains a hyphen (not a valid JSX identifier, e.g. "onCustom-event")
    let needs_spread = use_spread || jsx_event_name.contains('-');
    if needs_spread {
        let prop_end = get_prop_end(prop);
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value_expr = &source[vs as usize..ve as usize];
            // Flat resolution drives handler-shape classification ONLY; the handler
            // value is emitted relocated through the typed `EmitOp` substrate below
            // so each identifier maps back to source.
            let resolved_expr = match oxc_prop.and_then(|p| p.exp.as_ref()) {
                Some(exp) => build_prefixed_expr(value_expr, vs, exp, resolver, &[]),
                None => resolver.resolve_simple_expr(value_expr),
            };
            let resolved_expr = resolved_expr.trim();
            let is_simple_ident =
                crate::template::code_gen::binding::is_simple_ident(resolved_expr);
            let is_member_expr = resolved_expr.contains('.') && !resolved_expr.contains('(');
            let is_fn_expr = resolved_expr.starts_with("(")
                || resolved_expr.starts_with("function")
                || resolved_expr.contains("=>");
            let has_event_param = resolved_expr.contains("$event");

            let (tvs, tve) = trim_span(source, vs, ve);
            let value_range = SourceByteRange::new(SourceByteOffset(tvs), SourceByteOffset(tve));
            let value_bindings = oxc_prop
                .and_then(|p| p.exp.as_ref())
                .and_then(|e| e.bindings.as_ref())
                .map(|b| b.bindings.as_slice());

            out.overwrite(prop.start, prop_end, "");
            let at = SourceByteOffset(prop.start);
            let unmapped = |out: &mut CodeGenOutput<'alloc>, text: String| {
                emit_op(
                    out,
                    &EmitOp::InsertUnmapped {
                        at,
                        text: EmitText::Owned(text),
                    },
                );
            };
            let value = |out: &mut CodeGenOutput<'alloc>| {
                emit_relocated_value(
                    out,
                    at,
                    source,
                    value_range,
                    value_bindings,
                    resolver,
                    ExprOptions::default(),
                );
            };
            // The spread object's string key is the JSX event name (`"onKeyDown"`,
            // `"onMy-custom-event"`). The event NAME is a navigable semantic anchor
            // (hover / go-to-definition on a component `@event` resolves the child's
            // `onEvent` payload), so emit `{...{"` unmapped, then the `onEvent` key
            // text MAPPED to the source event-name token (`arg_start`), then the
            // closing `"` unmapped. This mirrors the in-place handler's mapped
            // event-name boundary.
            let event_key = |out: &mut CodeGenOutput<'alloc>| {
                unmapped(out, "{...{\"".to_string());
                emit_op(
                    out,
                    &EmitOp::InsertMapped {
                        at,
                        text: EmitText::Owned(jsx_event_name.clone()),
                        source_start: SourceByteOffset(arg_start),
                        content_offset: GeneratedByteLen(0),
                    },
                );
                unmapped(out, "\"".to_string());
            };

            if is_simple_ident || is_member_expr || is_fn_expr {
                // Simple ident, member expression, or fn/arrow expression → pass raw.
                event_key(out);
                unmapped(out, ": ".to_string());
                value(out);
                unmapped(out, "}}".to_string());
            } else if has_event_param {
                // Inline expression with $event → wrap with eventCallbacks for type inference.
                event_key(out);
                unmapped(
                    out,
                    ": (...___VERTER___eventArgs) => ___VERTER___eventCallbacks(___VERTER___eventArgs, ($event) => {".to_string(),
                );
                value(out);
                unmapped(out, "})}}".to_string());
            } else {
                // Inline expression without $event → wrap with () => { ... }.
                event_key(out);
                unmapped(out, ": () => {".to_string());
                value(out);
                unmapped(out, "}}}".to_string());
            }
        } else {
            out.overwrite(prop.start, prop_end, "");
        }
        return;
    }

    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
        let value_expr = &source[vs as usize..ve as usize];
        // Flat resolution drives handler-shape classification only; the handler
        // identifier is preserved in place by the split emission below.
        let resolved_expr = match oxc_prop.and_then(|p| p.exp.as_ref()) {
            Some(exp) => build_prefixed_expr(value_expr, vs, exp, resolver, &[]),
            None => resolver.resolve_simple_expr(value_expr),
        };
        let resolved_expr = resolved_expr.trim();

        // Determine if the handler needs wrapping
        let is_simple_ident = crate::template::code_gen::binding::is_simple_ident(resolved_expr);
        let is_member_expr = resolved_expr.contains('.') && !resolved_expr.contains('(');
        let is_fn_expr = resolved_expr.starts_with("(")
            || resolved_expr.starts_with("function")
            || resolved_expr.contains("=>");
        let is_object_expr = resolved_expr.starts_with('{') && resolved_expr.ends_with('}');
        let has_event_param = resolved_expr.contains("$event");

        // Build prop end position (including modifiers and quotes)
        let prop_end = get_prop_end(prop);

        // Check if binding resolution changed the expression text.
        // When unchanged (common for TSX inline mode), we split the overwrite
        // to preserve the original expression span, keeping source map tokens
        // for TSGO hover.
        let trimmed_expr = value_expr.trim();
        let expr_unchanged = resolved_expr == trimmed_expr;

        // Calculate trimmed expression boundaries within the source.
        // Leading/trailing whitespace must be included in the overwrite prefix/suffix
        // to avoid emitting raw whitespace between the JSX prop name and expression.
        let leading_ws = value_expr.len() - value_expr.trim_start().len();
        let trailing_ws = value_expr.len() - value_expr.trim_end().len();
        let trimmed_vs = vs + leading_ws as u32;
        let trimmed_ve = ve - trailing_ws as u32;

        // Every in-place branch shares the SAME emission shape: overwrite the two
        // synthetic boundaries (the JSX `onEvent={`-style prefix and the closing
        // suffix) with UNMAPPED literal text, and keep the handler value PRESERVED
        // IN PLACE — planned + emitted through the unified in-place sink so each
        // identifier stays an `Original` (1:1-mapped) chunk while accessor
        // prefixes/suffixes are applied as in-place prepends. The `expr_unchanged`
        // fast path is subsumed: with no rewrite the plan's prefixes are empty, so
        // the in-place sink is a pure no-op over the preserved bytes. The branches
        // differ ONLY in the boundary prefix/suffix scaffolding text.
        let _ = expr_unchanged;
        let (boundary_prefix, boundary_suffix): (String, &str) = if is_fn_expr || is_object_expr {
            // Explicit function/object expressions are already valid handlers.
            (format!("{}={{", jsx_event_name), "}")
        } else if has_event_param {
            // $event can only exist inside a callback parameter scope. Wrap with
            // eventCallbacks for proper type inference.
            let guard_prefix = v_if_guard
                .map(|guard| format!("if (!({})) {{ return undefined; }} ", guard))
                .unwrap_or_default();
            (
                format!(
                    "{}={{(...___VERTER___eventArgs) => ___VERTER___eventCallbacks(___VERTER___eventArgs, ($event) => {{{}",
                    jsx_event_name, guard_prefix
                ),
                "})}",
            )
        } else if is_simple_ident || is_member_expr {
            // Simple handler: @click="handler" → onClick={handler}
            (format!("{}={{", jsx_event_name), "}")
        } else {
            // Inline expression: @click="count++" → onClick={() => count++}
            let guard_prefix = v_if_guard
                .map(|guard| format!("if (!({})) {{ return undefined; }} ", guard))
                .unwrap_or_default();
            (
                format!("{}={{() => {{{}", jsx_event_name, guard_prefix),
                "}}",
            )
        };

        emit_in_place_handler(
            out,
            resolver,
            source,
            prop.start,
            trimmed_vs,
            trimmed_ve,
            prop_end,
            &boundary_prefix,
            boundary_suffix,
            oxc_prop,
        );
    } else {
        // Event with no value — just remove
        let prop_end = get_prop_end(prop);
        out.overwrite(prop.start, prop_end, "");
    }
}

/// Emit an in-place `v-on` handler: overwrite the two JSX boundaries (the
/// `onEvent={`-style prefix and the closing suffix) and keep the handler VALUE
/// PRESERVED IN PLACE through the unified in-place sink (each surviving identifier
/// stays an `Original`, 1:1-mapped chunk; accessor prefixes/suffixes applied as
/// in-place prepends).
///
/// The boundaries are plain `out.overwrite` (mapped) — NOT unmapped
/// `OverwriteSyntheticBoundary`. The JSX event NAME (`onCustom`) in the prefix is a
/// NAVIGABLE semantic anchor: hover / go-to-definition on a component's `@custom`
/// must map the source event name into the generated `onCustom` so the type
/// provider resolves the event payload (and the LSP rewrites `onCustom` back to
/// `@custom`). Mapping the boundary to the prop start preserves that navigation.
/// This is NOT the desync class — the boundary text is synthetic event-name
/// scaffolding, not a baked user EXPRESSION; the handler value (the navigable user
/// expression) is the planned in-place emission below.
#[allow(clippy::too_many_arguments)]
fn emit_in_place_handler<'alloc>(
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
    source: &'alloc str,
    prop_start: u32,
    trimmed_vs: u32,
    trimmed_ve: u32,
    prop_end: u32,
    boundary_prefix: &str,
    boundary_suffix: &str,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
) {
    out.overwrite(prop_start, trimmed_vs, boundary_prefix);
    let bindings = oxc_prop
        .and_then(|p| p.exp.as_ref())
        .and_then(|e| e.bindings.as_ref())
        .map(|b| b.bindings.as_slice());
    let plan = plan_user_expr(
        source,
        SourceByteRange::new(SourceByteOffset(trimmed_vs), SourceByteOffset(trimmed_ve)),
        bindings,
        resolver,
        ExprOptions::in_place(),
    );
    emit_expr_plan(out, &plan, Placement::InPlace, source);
    out.overwrite(trimmed_ve, prop_end, boundary_suffix);
}
