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
//! object-property values) is emitted through the typed `EmitOp` substrate
//! (`emit_relocated_value`), so each identifier maps 1:1 back to its source span;
//! the event-key transformation, object braces, computed-key template literals, and
//! handler-wrapper scaffolding are unmapped synthetic text. Extracted from `props.rs`
//! to keep both files within the production line-count budget.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, ObjectPropertyKind};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};

use verter_span::{SourceByteOffset, SourceByteRange};

use super::props::get_prop_end;
use crate::ide::event_to_jsx_name;
use crate::ide::template::emit::{emit_op, emit_relocated_value, trim_span, EmitOp, EmitText};
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::types::CodeGenOutput;
use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;
use crate::template::oxc::types::{OxcParsedExpression, OxcParsedProp};
use crate::types::NodeProp;

/// Emit a `v-on="{ … }"` object literal as a mapped spread `{...{ … }}`.
///
/// Walks the SOURCE object AST (not a reparsed flat string) so each property
/// value is emitted relocated through the [`emit_jsx_binding_value`] substrate and
/// stays navigable; the event-key transformation (`click` → `onClick`), braces,
/// and separators are unmapped synthetic text. Returns `false` when the object
/// cannot be walked structurally (parse failure / unsupported shape) so the caller
/// can fall back.
pub(super) fn emit_v_on_object_spread<'alloc>(
    out: &mut CodeGenOutput<'alloc>,
    at: SourceByteOffset,
    source: &'alloc str,
    exp: &OxcParsedExpression<'alloc>,
    resolver: &BindingResolver<'alloc>,
) -> bool {
    let Some(Expression::ObjectExpression(obj)) = exp.expression.as_ref() else {
        return false;
    };
    let base = exp.offset;
    let bindings = exp.bindings.as_ref().map(|b| b.bindings.as_slice());

    let unmapped = |out: &mut CodeGenOutput<'alloc>, text: String| {
        emit_op(
            out,
            &EmitOp::InsertUnmapped {
                at,
                text: EmitText::Owned(text),
            },
        );
    };

    unmapped(out, "{...{".to_string());
    let mut first = true;
    for prop in &obj.properties {
        match prop {
            ObjectPropertyKind::SpreadProperty(spread) => {
                let span = spread.argument.span();
                if span.end <= span.start {
                    continue;
                }
                if !first {
                    unmapped(out, ", ".to_string());
                }
                first = false;
                unmapped(out, "...".to_string());
                let (s, e) = trim_span(source, base + span.start, base + span.end);
                emit_relocated_value(
                    out,
                    at,
                    source,
                    SourceByteRange::new(SourceByteOffset(s), SourceByteOffset(e)),
                    bindings,
                    resolver,
                );
            }
            ObjectPropertyKind::ObjectProperty(p) => {
                let key_span = p.key.span();
                let value_span = p.value.span();
                if key_span.end <= key_span.start || value_span.end <= value_span.start {
                    continue;
                }
                let (vs, ve) = trim_span(source, base + value_span.start, base + value_span.end);
                let value_range = SourceByteRange::new(SourceByteOffset(vs), SourceByteOffset(ve));

                if p.computed {
                    // Computed key `[expr]: value` — both are navigable. Keep them
                    // mapped; the brackets/colon are unmapped.
                    if !first {
                        unmapped(out, ", ".to_string());
                    }
                    first = false;
                    let (ks, ke) = trim_span(source, base + key_span.start, base + key_span.end);
                    unmapped(out, "[".to_string());
                    emit_relocated_value(
                        out,
                        at,
                        source,
                        SourceByteRange::new(SourceByteOffset(ks), SourceByteOffset(ke)),
                        bindings,
                        resolver,
                    );
                    unmapped(out, "]: ".to_string());
                    emit_relocated_value(out, at, source, value_range, bindings, resolver);
                } else {
                    // Static event key (`click`, `"my-event"`) → JSX event name.
                    let raw_key =
                        &source[(base + key_span.start) as usize..(base + key_span.end) as usize];
                    let Some(event_key) = parse_static_event_key(raw_key.trim()) else {
                        return false;
                    };
                    let mapped_name = event_to_jsx_name(event_key);
                    let key = if crate::template::code_gen::binding::is_simple_ident(&mapped_name) {
                        mapped_name
                    } else {
                        format!("\"{}\"", mapped_name)
                    };
                    if !first {
                        unmapped(out, ", ".to_string());
                    }
                    first = false;
                    // The event-key text is synthetic (remapped) → unmapped.
                    unmapped(out, format!("{}: ", key));
                    emit_relocated_value(out, at, source, value_range, bindings, resolver);
                }
            }
        }
    }
    unmapped(out, "}}".to_string());
    true
}

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
        // v-on="{ mousedown: doThis }" → spread: {...{ mousedown: doThis }}
        // Each handler value is a navigable user expression, so the object is
        // emitted through the typed `EmitOp` substrate (each value mapped at its
        // source span; event keys / braces unmapped). The prop span is deleted and
        // the spread re-emitted at `prop.start`.
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let prop_end = get_prop_end(prop);
            out.overwrite(prop.start, prop_end, "");
            let at = SourceByteOffset(prop.start);
            let emitted = oxc_prop
                .and_then(|p| p.exp.as_ref())
                .map(|exp| emit_v_on_object_spread(out, at, source, exp, resolver))
                .unwrap_or(false);
            if !emitted {
                // Structural walk unavailable (parse failure / unsupported shape):
                // the value has no navigable bindings to preserve, so a flat
                // resolution is emitted as UNMAPPED synthetic text (never a mapped
                // overwrite — nothing maps back to prop.start).
                let value_expr = &source[vs as usize..ve as usize];
                let resolved = resolver.resolve_simple_expr(value_expr);
                let rewritten = rewrite_v_on_object_literal_expr(&resolved);
                emit_op(
                    out,
                    &EmitOp::InsertUnmapped {
                        at,
                        text: EmitText::Owned(format!("{{...{}}}", rewritten)),
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
                emit_relocated_value(out, at, source, value_range, value_bindings, resolver);
            };

            if is_simple_ident || is_member_expr || is_fn_expr {
                // Simple ident, member expression, or fn/arrow expression → pass raw.
                unmapped(out, format!("{{...{{\"{}\": ", jsx_event_name));
                value(out);
                unmapped(out, "}}".to_string());
            } else if has_event_param {
                // Inline expression with $event → wrap with eventCallbacks for type inference.
                unmapped(
                    out,
                    format!(
                        "{{...{{\"{}\": (...___VERTER___eventArgs) => ___VERTER___eventCallbacks(___VERTER___eventArgs, ($event) => {{",
                        jsx_event_name
                    ),
                );
                value(out);
                unmapped(out, "})}}".to_string());
            } else {
                // Inline expression without $event → wrap with () => { ... }.
                unmapped(out, format!("{{...{{\"{}\": () => {{", jsx_event_name));
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

        if is_fn_expr || is_object_expr {
            // Explicit function/object expressions are already valid handlers.
            if expr_unchanged {
                out.overwrite(prop.start, trimmed_vs, &format!("{}={{", jsx_event_name));
                out.overwrite(trimmed_ve, prop_end, "}");
            } else {
                // Patch-based: preserve source map tokens for sub-expressions.
                out.overwrite(prop.start, trimmed_vs, &format!("{}={{", jsx_event_name));
                out.overwrite(trimmed_ve, prop_end, "}");
                if let Some(oxc_p) = oxc_prop {
                    if let Some(ref exp) = oxc_p.exp {
                        if let Some(ref bindings) = exp.bindings {
                            resolver.collect_binding_patches(bindings, out);
                        }
                    }
                }
            }
        } else if has_event_param {
            // $event can only exist inside a callback parameter scope.
            // Use eventCallbacks wrapper for proper type inference:
            //   onClick={(...___VERTER___eventArgs) => ___VERTER___eventCallbacks(___VERTER___eventArgs, ($event) => {EXPR})}
            let guard_prefix = v_if_guard
                .map(|guard| format!("if (!({})) {{ return undefined; }} ", guard))
                .unwrap_or_default();
            let prefix = format!(
                "{}={{(...___VERTER___eventArgs) => ___VERTER___eventCallbacks(___VERTER___eventArgs, ($event) => {{{}",
                jsx_event_name, guard_prefix
            );
            let suffix = "})}";
            if expr_unchanged {
                out.overwrite(prop.start, trimmed_vs, &prefix);
                out.overwrite(trimmed_ve, prop_end, suffix);
            } else {
                // Patch-based: preserve source map tokens inside callback body.
                out.overwrite(prop.start, trimmed_vs, &prefix);
                out.overwrite(trimmed_ve, prop_end, suffix);
                if let Some(oxc_p) = oxc_prop {
                    if let Some(ref exp) = oxc_p.exp {
                        if let Some(ref bindings) = exp.bindings {
                            resolver.collect_binding_patches(bindings, out);
                        }
                    }
                }
            }
        } else if is_simple_ident || is_member_expr {
            // Simple handler: @click="handler" → onClick={handler}
            if expr_unchanged {
                out.overwrite(prop.start, trimmed_vs, &format!("{}={{", jsx_event_name));
                out.overwrite(trimmed_ve, prop_end, "}");
            } else {
                // Patch-based: preserve source map tokens.
                out.overwrite(prop.start, trimmed_vs, &format!("{}={{", jsx_event_name));
                out.overwrite(trimmed_ve, prop_end, "}");
                if let Some(oxc_p) = oxc_prop {
                    if let Some(ref exp) = oxc_p.exp {
                        if let Some(ref bindings) = exp.bindings {
                            resolver.collect_binding_patches(bindings, out);
                        }
                    }
                }
            }
        } else {
            // Inline expression: @click="count++" → onClick={() => count++}
            let guard_prefix = v_if_guard
                .map(|guard| format!("if (!({})) {{ return undefined; }} ", guard))
                .unwrap_or_default();
            if expr_unchanged {
                out.overwrite(
                    prop.start,
                    trimmed_vs,
                    &format!("{}={{() => {{{}", jsx_event_name, guard_prefix),
                );
                out.overwrite(trimmed_ve, prop_end, "}}");
            } else {
                // Patch-based: preserve source map tokens inside callback body.
                out.overwrite(
                    prop.start,
                    trimmed_vs,
                    &format!("{}={{() => {{{}", jsx_event_name, guard_prefix),
                );
                out.overwrite(trimmed_ve, prop_end, "}}");
                if let Some(oxc_p) = oxc_prop {
                    if let Some(ref exp) = oxc_p.exp {
                        if let Some(ref bindings) = exp.bindings {
                            resolver.collect_binding_patches(bindings, out);
                        }
                    }
                }
            }
        }
    } else {
        // Event with no value — just remove
        let prop_end = get_prop_end(prop);
        out.overwrite(prop.start, prop_end, "");
    }
}

fn rewrite_v_on_object_literal_expr(expr: &str) -> String {
    let trimmed = expr.trim();
    if !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
        return expr.to_string();
    }

    let alloc = Allocator::new();
    let Ok(parsed) = Parser::new(&alloc, trimmed, SourceType::mjs()).parse_expression() else {
        return expr.to_string();
    };
    let Expression::ObjectExpression(obj) = parsed else {
        return expr.to_string();
    };

    let mut rebuilt = String::from("{");
    let mut first = true;

    for prop in &obj.properties {
        let piece = match prop {
            ObjectPropertyKind::SpreadProperty(spread) => {
                let span = spread.argument.span();
                if span.end <= span.start {
                    continue;
                }
                format!(
                    "...{}",
                    trimmed[span.start as usize..span.end as usize].trim()
                )
            }
            ObjectPropertyKind::ObjectProperty(p) => {
                if p.computed {
                    let key_span = p.key.span();
                    let value_span = p.value.span();
                    if key_span.end <= key_span.start || value_span.end <= value_span.start {
                        continue;
                    }
                    let key_src = trimmed[key_span.start as usize..key_span.end as usize].trim();
                    let value_src =
                        trimmed[value_span.start as usize..value_span.end as usize].trim();
                    format!("[{}]: {}", key_src, value_src)
                } else {
                    let key_span = p.key.span();
                    let value_span = p.value.span();
                    if key_span.end <= key_span.start || value_span.end <= value_span.start {
                        continue;
                    }

                    let raw_key = trimmed[key_span.start as usize..key_span.end as usize].trim();
                    let Some(event_key) = parse_static_event_key(raw_key) else {
                        return expr.to_string();
                    };
                    let mapped = event_to_jsx_name(event_key);
                    let key = if crate::template::code_gen::binding::is_simple_ident(&mapped) {
                        mapped
                    } else {
                        format!("\"{}\"", mapped)
                    };
                    let value_src =
                        trimmed[value_span.start as usize..value_span.end as usize].trim();
                    format!("{}: {}", key, value_src)
                }
            }
        };

        if !first {
            rebuilt.push_str(", ");
        }
        first = false;
        rebuilt.push_str(&piece);
    }

    rebuilt.push('}');
    rebuilt
}

fn parse_static_event_key(raw_key: &str) -> Option<&str> {
    let trimmed = raw_key.trim();
    if let Some(stripped) = trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
        })
    {
        return Some(stripped.trim());
    }
    if crate::template::code_gen::binding::is_simple_ident(trimmed) {
        return Some(trimmed);
    }
    None
}
