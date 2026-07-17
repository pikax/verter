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
//! `plan_object_literal` planner — relocated values via the relocated sink, in-place
//! handler values via the in-place sink — so each identifier maps 1:1 back to its
//! source span. The JSX event NAME (`onClick`, `onMy-event`) is a navigable semantic
//! anchor MAPPED to the source event token. Object braces, computed-key template
//! literals, the `($event) => { … }` handler-wrapper scaffolding (with an explicit
//! event-payload annotation on the spread path, where JSX contextual typing cannot
//! flow), and the v-if narrowing guard are unmapped synthetic text. This module
//! lives apart from `props.rs` so each stays within the production line-count budget.

use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;

use verter_span::{GeneratedByteLen, SourceByteOffset, SourceByteRange};

use super::props::get_prop_end;
use crate::ast::types::{ElementNode, TagType};
use crate::ide::template::emit::{
    emit_expr_plan, emit_op, emit_relocated_value, plan_object_literal, plan_user_expr, trim_span,
    EmitOp, EmitText, ExprOptions, KeyRewritePolicy, Placement,
};
use crate::ide::{
    event_handler_params_type, event_to_jsx_name, native_dom_event_payload_type,
    TemplateComponentBindings,
};
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::types::CodeGenOutput;
use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;
use crate::template::oxc::types::{OxcParsedExpression, OxcParsedProp};
use crate::types::NodeProp;

/// Process `v-on` / `@` directive.
///
/// - `@click="handler"` → `onClick={handler}`
/// - `@click="handler($event)"` → `onClick={($event) => handler($event)}`
/// - `v-on="{ mousedown: doThis }"` → `{...{ mousedown: doThis }}` (spread, #49)
#[allow(clippy::too_many_arguments)]
pub(super) fn process_v_on<'alloc>(
    prop: &NodeProp,
    el: &ElementNode,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
    components: &TemplateComponentBindings,
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
            // Arrow / function-expression handlers are classified from the OXC expression
            // KIND, never a `starts_with("(")` / `contains("=>")` text probe — a string
            // literal or member expression that happens to contain `=>` can never be
            // misread as a function handler.
            let is_fn_or_arrow = is_fn_or_arrow_expr(oxc_prop.and_then(|p| p.exp.as_ref()));
            let has_event_param =
                references_event_param(value_expr, oxc_prop.and_then(|p| p.exp.as_ref()));

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

            if is_fn_or_arrow {
                // Arrow / function-expression handler on a spread key (duplicate or
                // hyphenated event). JSX contextual typing does NOT flow through a spread
                // attribute, so the function's parameters would be implicit-`any`. Wrap the
                // (still source-mapped) user function in a `satisfies` clause whose target
                // is the element's event-handler signature — TypeScript then contextually
                // types the parameters against the real payload tuple while the user code
                // stays navigable. The `satisfies` wrapper and rest tuple are synthetic,
                // unmapped scaffolding.
                let types = spread_event_types(
                    el,
                    source,
                    event_name,
                    &jsx_event_name,
                    components,
                    resolver,
                );
                event_key(out);
                unmapped(out, ": (".to_string());
                value(out);
                unmapped(
                    out,
                    format!(
                        ") satisfies (...___VERTER___eventArgs: {}) => unknown}}}}",
                        types.params_tuple
                    ),
                );
            } else if is_simple_ident || is_member_expr {
                // Simple handler reference (`handler` / `obj.method`) → pass raw. Its own
                // declaration carries the parameter types (script-side event-handler param
                // inference annotates a local function declaration); the spread does not
                // re-annotate it.
                event_key(out);
                unmapped(out, ": ".to_string());
                value(out);
                unmapped(out, "}}".to_string());
            } else if has_event_param {
                // Inline expression with `$event` on a spread key. Name the sole callback
                // parameter `$event` and annotate it explicitly with the payload type (JSX
                // contextual typing cannot reach it through the spread, so the explicit
                // annotation is what keeps `$event` from collapsing to `any`).
                let types = spread_event_types(
                    el,
                    source,
                    event_name,
                    &jsx_event_name,
                    components,
                    resolver,
                );
                event_key(out);
                unmapped(out, format!(": ($event: {}) => {{", types.payload));
                value(out);
                unmapped(out, "}}}".to_string());
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
        let has_event_param =
            references_event_param(value_expr, oxc_prop.and_then(|p| p.exp.as_ref()));

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

        // Every in-place branch shares the SAME emission shape: the JSX boundary is
        // decomposed through the typed `EmitOp` substrate (NOT one flat mapped
        // overwrite). The event NAME is a navigable semantic anchor (mapped to the
        // source event token), the synthetic wrapper scaffolding is UNMAPPED, the
        // optional v-if narrowing guard is UNMAPPED synthetic text injected at the
        // body start, and the handler value is PRESERVED IN PLACE — planned + emitted
        // through the unified in-place sink so each identifier stays an `Original`
        // (1:1-mapped) chunk while accessor prefixes/suffixes are applied as in-place
        // prepends. The `expr_unchanged` fast path is subsumed: with no rewrite the
        // plan's prefixes are empty, so the in-place sink is a pure no-op over the
        // preserved bytes. The branches differ ONLY in the scaffolding text and
        // whether a narrowing guard applies.
        //
        // `scaffold_after_event` is the synthetic text emitted AFTER the mapped event
        // name and BEFORE the (optional) guard + body: `={`, `={() => {`, or the
        // `={($event) => {` wrapper. `guard_text` (when present) is the narrowing guard
        // injected at the body start — a COMPOSED, span-erased compiler-synthesized
        // scaffold (see `emit_in_place_handler` docs) → UNMAPPED. Guards apply only to
        // the two wrapping branches ($event / inline expression); a function/object/
        // simple-ident handler is already a valid handler value and takes no guard.
        let _ = expr_unchanged;
        let (scaffold_after_event, boundary_suffix, guard_text): (String, &str, Option<String>) =
            if is_fn_expr || is_object_expr {
                // Explicit function/object expressions are already valid handlers.
                ("={".to_string(), "}", None)
            } else if has_event_param {
                // `$event` can only exist inside a callback parameter scope. Name the
                // handler's sole parameter `$event` so it is contextually typed by the
                // JSX event prop (`onClick`, `onCustom`, …) — the exact mechanism that
                // types an inline arrow's parameter. This needs no synthetic event-type
                // formula and no generic `eventCallbacks` indirection (which left
                // `$event` as `any` because contextual typing does not flow through a
                // synthetic rest parameter into a generic helper call).
                (
                    "={($event) => {".to_string(),
                    "}}",
                    v_if_guard.map(|guard| format!("if (!({})) {{ return undefined; }} ", guard)),
                )
            } else if is_simple_ident || is_member_expr {
                // Simple handler: @click="handler" → onClick={handler}
                ("={".to_string(), "}", None)
            } else {
                // Inline expression: @click="count++" → onClick={() => count++}
                (
                    "={() => {".to_string(),
                    "}}",
                    v_if_guard.map(|guard| format!("if (!({})) {{ return undefined; }} ", guard)),
                )
            };

        emit_in_place_handler(
            out,
            resolver,
            source,
            prop.start,
            arg_start,
            &jsx_event_name,
            &scaffold_after_event,
            trimmed_vs,
            trimmed_ve,
            prop_end,
            boundary_suffix,
            guard_text.as_deref(),
            oxc_prop,
        );
    } else {
        // Event with no value — just remove
        let prop_end = get_prop_end(prop);
        out.overwrite(prop.start, prop_end, "");
    }
}

/// Emit an in-place `v-on` handler. The JSX boundary is DECOMPOSED through the typed
/// `EmitOp` substrate — never one flat mapped `out.overwrite` carrying both synthetic
/// scaffolding and the navigable event name:
///
/// - The leading `@` / `v-on:` prefix `[prop_start, arg_start)` is deleted (unmapped).
/// - The JSX event NAME (`onClick`, `onMy-event`) is emitted MAPPED to the source
///   event token (`arg_start`). It is a NAVIGABLE semantic anchor: hover /
///   go-to-definition on a component's `@custom` resolves the child's `onCustom`
///   payload (and the LSP rewrites `onCustom` back to `@custom`). This matches the
///   v-on spread branch, which maps the event-name key via `InsertMapped@arg_start`.
/// - `scaffold_after_event` (`={`, `={() => {`, the `={($event) => {` wrapper) is
///   synthetic JSX scaffolding → UNMAPPED.
/// - The optional `guard_text` is a v-if narrowing guard injected at the body start.
///   It is a COMPOSED, span-erased compiler-synthesized scaffold (own positive
///   condition + sibling negations from OTHER elements + ancestor scopes, already
///   flattened to a string and joined with synthetic `!(…) && (…)`), so it has no
///   single source span → emitted UNMAPPED (None), exactly like the sibling
///   `process_v_bind` guarded-value path's `out.prepend_alloc(injection.offset, …)`.
/// - The handler VALUE is PRESERVED IN PLACE through the unified in-place sink (each
///   surviving identifier stays an `Original`, 1:1-mapped chunk; accessor
///   prefixes/suffixes applied as in-place prepends).
/// - The closing `boundary_suffix` is synthetic → UNMAPPED.
///
/// The guard is emitted BEFORE the in-place value plan so that at a shared anchor (an
/// inline handler whose first body identifier sits exactly at `trimmed_vs`) the
/// stable-sorted same-position prepend order is `<guard><accessor-prefix><identifier>`.
#[allow(clippy::too_many_arguments)]
fn emit_in_place_handler<'alloc>(
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
    source: &'alloc str,
    prop_start: u32,
    arg_start: u32,
    jsx_event_name: &str,
    scaffold_after_event: &str,
    trimmed_vs: u32,
    trimmed_ve: u32,
    prop_end: u32,
    boundary_suffix: &str,
    guard_text: Option<&str>,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
) {
    // Delete the leading `@` / `v-on:` prefix (unmapped — `overwrite(.., .., "")`).
    out.overwrite(prop_start, arg_start, "");
    let at = SourceByteOffset(arg_start);
    // The event NAME is the navigable anchor → MAPPED to the source event token.
    emit_op(
        out,
        &EmitOp::InsertMapped {
            at,
            text: EmitText::Owned(jsx_event_name.to_string()),
            source_start: at,
            content_offset: GeneratedByteLen(0),
        },
    );
    // The synthetic scaffolding after the event name → UNMAPPED.
    emit_op(
        out,
        &EmitOp::InsertUnmapped {
            at,
            text: EmitText::Owned(scaffold_after_event.to_string()),
        },
    );
    // The arg-side span between the event token and the value start (`="`, modifiers,
    // whitespace) is deleted; the synthetic scaffolding already supplied the `={`.
    out.overwrite(arg_start, trimmed_vs, "");
    // The v-if narrowing guard (synthetic) → UNMAPPED prepend at the body start,
    // emitted before the in-place value so the same-position order keeps the guard
    // ahead of any body identifier / accessor prefix.
    //
    // Guard-injection offset is per-wrapper-shape but the mapping discipline is
    // shared: this handler path injects at `trimmed_vs` (the guard scaffolds a
    // statement-body `{ if (!(…)) return undefined; … }`, so it lands at the body
    // start), while the sibling v-bind function-value path
    // (`process_v_bind` → `compute_function_guard_injection`) computes a
    // wrapper-shape-specific offset (arrow-expression body vs arrow-block / fn-expr
    // body `{`). Both emit the guard as an UNMAPPED prepend (synthetic narrowing
    // text → None) ordered ahead of the in-place body identifiers — the offsets
    // differ, the unmapped-guard contract is identical.
    if let Some(guard) = guard_text {
        out.prepend_alloc(trimmed_vs, guard);
    }
    // The handler value is planned + emitted IN PLACE through the unified planner.
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
    // The closing `boundary_suffix` is synthetic JSX scaffolding (the `}` /
    // `}}` / `})}` wrapper + container close) → UNMAPPED, exactly like the
    // leading-prefix delete and the `scaffold_after_event` insert. Lowered
    // through `OverwriteSyntheticBoundary`: the source tail `[trimmed_ve,
    // prop_end)` (the close quote + trailing whitespace) is DELETED, then the
    // synthetic suffix is inserted as an unmapped `Inserted` chunk. A mapped
    // `out.overwrite(trimmed_ve, prop_end, suffix)` would instead map the
    // synthetic braces back to the body end (the close quote) — the boundary
    // desync this decomposition exists to prevent. The generated TSX text is
    // unchanged: the suffix lands at the same position (right after the in-place
    // body, where the deleted tail was), only its source mapping becomes None.
    emit_op(
        out,
        &EmitOp::OverwriteSyntheticBoundary {
            source: SourceByteRange::new(SourceByteOffset(trimmed_ve), SourceByteOffset(prop_end)),
            text: EmitText::Borrowed(boundary_suffix),
            anchor: None,
        },
    );
}

/// Whether the handler value is an arrow-function or function expression, classified
/// from the OXC expression KIND — never a `starts_with("(")` / `contains("=>")` text
/// probe. Drives the spread-path `satisfies` wrapping, where JSX contextual typing
/// cannot reach the function's parameters.
fn is_fn_or_arrow_expr(exp: Option<&OxcParsedExpression<'_>>) -> bool {
    matches!(
        exp.and_then(|e| e.expression.as_ref()),
        Some(Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_))
    )
}

/// The payload + parameter-tuple types for a spread-path event handler.
///
/// A spread object literal (`{...{"onClick": …}}`) gives the handler NO JSX contextual
/// typing, so `$event` and arrow/function parameters must be annotated explicitly. Both
/// annotations derive from one shared lookup so they never disagree.
struct SpreadEventTypes {
    /// Type of the first handler argument (`$event`): the ambient DOM payload for a
    /// native element, or `params_tuple[0]` for a component.
    payload: String,
    /// The full handler parameter tuple, used as the
    /// `satisfies (...___VERTER___eventArgs: TUPLE) => unknown` target for arrow/function
    /// handlers.
    params_tuple: String,
}

impl SpreadEventTypes {
    /// Untyped fallback for a surface with no derivable type (a component with neither a
    /// local binding nor a GlobalComponents fallback const — the unresolved-component case).
    /// Explicit `any`, never an implicit-`any` bare parameter (which errors under
    /// `noImplicitAny`) and never the retired `Array<any>` helper.
    fn any() -> Self {
        SpreadEventTypes {
            payload: "any".to_string(),
            params_tuple: "[any]".to_string(),
        }
    }
}

/// Resolve the spread-path event types for an element, by tag kind. The two surfaces
/// have different TypeProvider-resolvability:
///
/// - NATIVE element → the ambient DOM event map via [`native_dom_event_payload_type`].
///   The `import('vue').IntrinsicElementAttributes` formula resolves only under a full
///   `tsserver`; the native preview (`tsgo`) cannot resolve `import('vue')` type queries
///   from the generated virtual `.vue.tsx`. The ambient-DOM type resolves everywhere.
/// - COMPONENT → the emit/prop surface via the shared [`event_handler_params_type`]
///   formula (`Parameters<NonNullable<Required<InstanceType<typeof Binding>["$props"]>["onX"]>>`).
///   The binding is resolved through the shared [`TemplateComponentBindings`] inventory —
///   a local script binding OR a GlobalComponents fallback const — so a globally-registered
///   component types identically to an imported one, NEVER through
///   `import('vue').GlobalComponents[...]` (which `tsgo` cannot resolve).
fn spread_event_types<'alloc>(
    el: &ElementNode,
    source: &'alloc str,
    event_name: &str,
    jsx_event_name: &str,
    components: &TemplateComponentBindings,
    resolver: &BindingResolver<'alloc>,
) -> SpreadEventTypes {
    match el.tag_type {
        TagType::Element => {
            let payload = native_dom_event_payload_type(event_name);
            let params_tuple = format!("[{payload}]");
            SpreadEventTypes {
                payload,
                params_tuple,
            }
        }
        TagType::Component => {
            let tag_name = &source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];
            let binding = components.resolve(tag_name, el.tag_type, |n| resolver.get(n).is_some());
            match binding
                .as_deref()
                .and_then(|b| event_handler_params_type(el.tag_type, Some(b), jsx_event_name, None))
            {
                Some(params_tuple) => {
                    let payload = format!("{params_tuple}[0]");
                    SpreadEventTypes {
                        payload,
                        params_tuple,
                    }
                }
                None => SpreadEventTypes::any(),
            }
        }
        _ => SpreadEventTypes::any(),
    }
}

/// Whether a `v-on` handler value references the Vue `$event` built-in.
///
/// Typed-IR-first: when OXC parsed the handler into binding facts, `$event` use is
/// an EXACT identifier match in `exp.bindings` (the binding visitor records every
/// identifier, including the ignored `$event` built-in). The match is identifier-
/// exact: it never fires for `$event` inside a string literal, a comment, or a
/// longer identifier like `my$eventBus`.
///
/// For an INCOMPLETE handler that OXC could not lower into facts (e.g. `$event.`
/// mid-completion), fall back to a token-level identifier scan that skips string and
/// comment contents and matches only a whole `$event` identifier.
fn references_event_param(value_expr: &str, exp: Option<&OxcParsedExpression<'_>>) -> bool {
    if let Some(exp) = exp {
        if let Some(bindings) = exp.bindings.as_ref() {
            if bindings.bindings.iter().any(|b| b.name == "$event") {
                return true;
            }
            // A complete, error-free parse that surfaced no `$event` binding is
            // authoritative: `$event` is genuinely absent.
            if exp.expression.is_some() && !bindings.has_errors {
                return false;
            }
        }
    }
    // No usable facts (unparsed / incomplete / errored) — token-level scan.
    scan_for_event_identifier(value_expr)
}

/// Token-level scan for a whole `$event` identifier, ignoring string-literal and
/// comment contents. The incomplete-expression fallback for
/// [`references_event_param`] — never the primary classifier.
fn scan_for_event_identifier(src: &str) -> bool {
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            // String literals — skip to the matching close quote.
            quote @ (b'"' | b'\'' | b'`') => {
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b if b == quote => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
            }
            // Line comment.
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            // Block comment.
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < bytes.len() && !(bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i += 2;
            }
            // Identifier (`$event` starts with `$`).
            b if is_ident_start(b) => {
                let start = i;
                i += 1;
                while i < bytes.len() && is_ident_continue(bytes[i]) {
                    i += 1;
                }
                if &src[start..i] == "$event" {
                    return true;
                }
            }
            _ => i += 1,
        }
    }
    false
}

#[inline]
fn is_ident_start(b: u8) -> bool {
    b == b'$' || b == b'_' || b.is_ascii_alphabetic() || b >= 0x80
}

#[inline]
fn is_ident_continue(b: u8) -> bool {
    is_ident_start(b) || b.is_ascii_digit()
}

#[cfg(test)]
mod event_param_detection_tests {
    use super::scan_for_event_identifier;

    #[test]
    fn matches_real_event_identifier() {
        assert!(scan_for_event_identifier("handle($event)"));
        assert!(scan_for_event_identifier("$event"));
        assert!(scan_for_event_identifier("$event.clientX"));
        // Incomplete member access (the mid-completion case the scanner exists for).
        assert!(scan_for_event_identifier("$event."));
        assert!(scan_for_event_identifier("emit('x', $event)"));
    }

    #[test]
    fn ignores_event_in_string_literal() {
        // The old `.contains("$event")` wrongly matched all of these.
        assert!(!scan_for_event_identifier("log('use $event here')"));
        assert!(!scan_for_event_identifier("log(\"$event\")"));
        assert!(!scan_for_event_identifier("t(`a $event b`)"));
    }

    #[test]
    fn ignores_event_in_comment() {
        assert!(!scan_for_event_identifier("doThing() // $event"));
        assert!(!scan_for_event_identifier("doThing() /* $event */"));
    }

    #[test]
    fn ignores_event_as_substring_of_longer_identifier() {
        assert!(!scan_for_event_identifier("my$eventBus.emit()"));
        assert!(!scan_for_event_identifier("handle$event"));
        assert!(!scan_for_event_identifier("$eventually"));
    }

    #[test]
    fn no_match_when_absent() {
        assert!(!scan_for_event_identifier("count++"));
        assert!(!scan_for_event_identifier("handleClick()"));
        assert!(!scan_for_event_identifier("a.b.c"));
    }
}
