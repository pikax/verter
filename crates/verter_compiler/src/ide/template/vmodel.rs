//! IDE `v-model` → JSX expansion.
//!
//! `v-model` is the hardest IDE emit site: it emits the bound expression 2-3
//! times (a read occurrence plus an assignment occurrence inside the update
//! handler), and for a dynamic arg the computed prop/event names embed the arg
//! expression. Each occurrence becomes its own mapped emission through the typed
//! `EmitOp` substrate (`emit_jsx_binding_value`); all assignment / call /
//! punctuation scaffolding is unmapped. Extracted from `props.rs` to keep both
//! files within the production line-count budget.

use verter_span::{GeneratedByteLen, SourceByteOffset, SourceByteRange};

use super::props::{get_prop_end, kebab_to_camel_case};
use crate::ast::types::ElementNode;
use crate::ide::get_directive_name;
use crate::ide::template::emit::{
    binding_slice, emit_jsx_binding_value, emit_op, EmitOp, EmitText, JsxBindingValue,
};
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::types::CodeGenOutput;
use crate::template::oxc::types::OxcParsedProp;
use crate::types::NodeProp;

/// Process `v-model` directive → expand to prop + update event pair.
///
/// - `v-model="count"` → `modelValue={count} onUpdate:modelValue={($event) => (count = $event)}`
/// - `v-model:title="val"` → `title={val} onUpdate:title={($event) => (val = $event)}`
/// - Modifiers are emitted as a modifiers prop (e.g., `modelModifiers={{ trim: true }}`)
pub(super) fn process_v_model<'alloc>(
    prop: &NodeProp,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
    el: &ElementNode,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    resolver: &BindingResolver<'alloc>,
    is_jsx: bool,
) {
    let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) else {
        // v-model with no value — remove
        let prop_end = get_prop_end(prop);
        out.overwrite(prop.start, prop_end, "");
        return;
    };

    let raw_expr = &source[vs as usize..ve as usize];
    let prop_end = get_prop_end(prop);

    // Trimmed value-expression span — the mapped occurrences anchor here.
    let value_leading_ws = (raw_expr.len() - raw_expr.trim_start().len()) as u32;
    let value_trailing_ws = (raw_expr.len() - raw_expr.trim_end().len()) as u32;
    let tvs = vs + value_leading_ws;
    let tve = ve - value_trailing_ws;
    let trimmed_expr = raw_expr.trim();

    // Pre-resolve the value-expression accessor decision for the no-bindings
    // fallback (single bare identifier / parse failure). When OXC produced
    // bindings, `emit_jsx_binding_value` maps each identifier directly.
    let value_bindings = oxc_prop
        .and_then(|p| p.exp.as_ref())
        .and_then(|e| e.bindings.as_ref());
    let (value_prefix, value_suffix): (Option<String>, Option<String>) = if value_bindings
        .map(|b| !b.bindings.is_empty())
        .unwrap_or(false)
    {
        (None, None)
    } else {
        // No extracted bindings: split the resolved simple expression into a
        // mapped identifier core plus unmapped prefix/suffix (mirrors the legacy
        // prefix-only split). If the resolved form does not contain the trimmed
        // expression verbatim, fall back to an unmapped prefix only.
        let resolved = resolver.resolve_simple_expr(trimmed_expr);
        if let Some(idx) = resolved.find(trimmed_expr) {
            let pre = resolved[..idx].to_string();
            let suf = resolved[idx + trimmed_expr.len()..].to_string();
            (
                (!pre.is_empty()).then_some(pre),
                (!suf.is_empty()).then_some(suf),
            )
        } else {
            // Resolved form rewrote the expression entirely — emit it all as the
            // (still mapped-at-start) core with no extra prefix/suffix.
            (None, None)
        }
    };
    let value_jsx = JsxBindingValue {
        source_expr: SourceByteRange::new(SourceByteOffset(tvs), SourceByteOffset(tve)),
        prefix: value_prefix.as_deref(),
        suffix: value_suffix.as_deref(),
        occurrences: 1,
        bindings: binding_slice(value_bindings),
    };

    // Determine prop name: default "modelValue" or named v-model:xxx.
    // For a dynamic arg, the computed prop/event/modifier names embed the arg
    // expression, which must itself map back to source; `dyn_arg` captures the
    // arg span + bindings for those mapped emissions.
    let is_dynamic_arg = prop.is_dynamic == Some(true);
    struct DynArg<'a> {
        span: SourceByteRange,
        bindings: &'a [crate::utils::oxc::Binding<'a>],
        prefix: Option<String>,
        suffix: Option<String>,
    }
    let mut dyn_arg: Option<DynArg<'_>> = None;
    let (value_prop, update_event, modifier_prop) = if let (Some(arg_s), Some(arg_e)) =
        (prop.arg_start, prop.arg_end)
    {
        let arg = &source[arg_s as usize..arg_e as usize];
        if is_dynamic_arg {
            // Dynamic arg: v-model:[expr]="val" → spread syntax with a computed
            // prop name. The arg expression is mapped at its source position.
            let raw_arg = arg
                .trim()
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or(arg)
                .trim();
            let raw_arg_start = arg_s + (raw_arg.as_ptr() as usize - arg.as_ptr() as usize) as u32;
            let raw_arg_end = raw_arg_start + raw_arg.len() as u32;
            let arg_bindings = oxc_prop
                .and_then(|p| p.arg.as_ref())
                .and_then(|a| a.bindings.as_ref());
            let (arg_prefix, arg_suffix): (Option<String>, Option<String>) = if arg_bindings
                .map(|b| !b.bindings.is_empty())
                .unwrap_or(false)
            {
                (None, None)
            } else {
                let resolved = resolver.resolve_simple_expr(raw_arg);
                if let Some(idx) = resolved.find(raw_arg) {
                    let pre = resolved[..idx].to_string();
                    let suf = resolved[idx + raw_arg.len()..].to_string();
                    (
                        (!pre.is_empty()).then_some(pre),
                        (!suf.is_empty()).then_some(suf),
                    )
                } else {
                    (None, None)
                }
            };
            dyn_arg = Some(DynArg {
                span: SourceByteRange::new(
                    SourceByteOffset(raw_arg_start),
                    SourceByteOffset(raw_arg_end),
                ),
                bindings: binding_slice(arg_bindings),
                prefix: arg_prefix,
                suffix: arg_suffix,
            });
            // The textual placeholders are unused once we emit the arg as a
            // mapped piece, but the surrounding scaffolding still needs the
            // literal punctuation, captured directly in the piece list below.
            (String::new(), String::new(), String::new())
        } else {
            let camel_arg = kebab_to_camel_case(arg);
            (
                camel_arg.clone(),
                format!("onUpdate:{}", camel_arg),
                format!("{}Modifiers", camel_arg),
            )
        }
    } else {
        (
            "modelValue".to_string(),
            "onUpdate:modelValue".to_string(),
            "modelModifiers".to_string(),
        )
    };

    // For native HTML elements, use the actual DOM property (value/checked) and a
    // valid JSX event handler (onInput/onChange). For components, use modelValue
    // prop + spread for the event handler.
    let is_native = el.tag_type.is_element();
    let event_param = if is_jsx { "$event" } else { "$event: any" };

    // A generated piece: unmapped synthetic text, one mapped emission of the
    // value expression / the dynamic-arg expression, or a mapped modifier name.
    enum Piece {
        Syn(String),
        Value,
        Arg,
        Modifier(SourceByteRange),
    }

    // Build the ordered piece list for the active branch (preserving every branch
    // decision from the original `format!` shapes). Each `{}` that previously
    // embedded `resolved` becomes a `Piece::Value`; each embedded `resolved_arg`
    // becomes a `Piece::Arg`.
    let mut pieces: Vec<Piece> = Vec::new();
    let mut empty_replacement = false;

    if is_dynamic_arg {
        // Byte-identical to the prior single-`format!` emission
        //   {...{[ARG]:VALUE, "[`onUpdate:${ARG}`]":(PARAM) => ((VALUE) = $event)}}
        // (value_prop = `[ARG]`, update_event = `[`onUpdate:${ARG}`]`, quoted as a
        // string key exactly as before). Each ARG / VALUE occurrence is now its own
        // mapped emission; all punctuation is unmapped.
        pieces.push(Piece::Syn("{...{[".to_string()));
        pieces.push(Piece::Arg);
        pieces.push(Piece::Syn("]:".to_string()));
        pieces.push(Piece::Value);
        pieces.push(Piece::Syn(", \"[`onUpdate:${".to_string()));
        pieces.push(Piece::Arg);
        pieces.push(Piece::Syn(format!("}}`]\":({}) => ((", event_param)));
        pieces.push(Piece::Value);
        pieces.push(Piece::Syn(") = $event)}}".to_string()));
    } else if is_native {
        let tag = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
        let (dom_prop, event_name) = native_vmodel_prop_and_event(el, source, tag);

        let vue_event = event_name
            .strip_prefix("on")
            .map(|s| {
                let mut c = s.chars();
                match c.next() {
                    Some(ch) => {
                        let lower = ch.to_lowercase().to_string();
                        format!("{}{}", lower, c.as_str())
                    }
                    None => String::new(),
                }
            })
            .unwrap_or_default();
        let has_explicit_handler = el.props.iter().any(|p| {
            p.is_directive && {
                let dn = get_directive_name(p, source);
                (dn == "on" || dn == "@")
                    && p.arg_start
                        .zip(p.arg_end)
                        .map(|(a, b)| source[a as usize..b as usize] == vue_event)
                        .unwrap_or(false)
            }
        });
        let has_explicit_prop = el.props.iter().any(|p| {
            if p.is_directive {
                let dn = get_directive_name(p, source);
                (dn == "bind" || dn == ":")
                    && p.arg_start
                        .zip(p.arg_end)
                        .map(|(a, b)| &source[a as usize..b as usize] == dom_prop)
                        .unwrap_or(false)
            } else {
                let name = &source[p.start as usize..p.name_end as usize];
                name == dom_prop
            }
        });

        if has_explicit_prop && has_explicit_handler {
            empty_replacement = true;
        } else if has_explicit_prop {
            // <event_name>={(<param>) => ((<value>) = $event)}
            pieces.push(Piece::Syn(format!(
                "{}={{({}) => ((",
                event_name, event_param
            )));
            pieces.push(Piece::Value);
            pieces.push(Piece::Syn(") = $event)}".to_string()));
        } else if has_explicit_handler {
            // <dom_prop>={<value>}
            pieces.push(Piece::Syn(format!("{}={{", dom_prop)));
            pieces.push(Piece::Value);
            pieces.push(Piece::Syn("}".to_string()));
        } else {
            // <dom_prop>={<value>} <event_name>={(<param>) => ((<value>) = $event)}
            pieces.push(Piece::Syn(format!("{}={{", dom_prop)));
            pieces.push(Piece::Value);
            pieces.push(Piece::Syn(format!(
                "}} {}={{({}) => ((",
                event_name, event_param
            )));
            pieces.push(Piece::Value);
            pieces.push(Piece::Syn(") = $event)}".to_string()));
        }
    } else {
        // Component: <value_prop>={<value>} {...{"<update_event>":(<param>) => ((<value>) = $event)}}
        pieces.push(Piece::Syn(format!("{}={{", value_prop)));
        pieces.push(Piece::Value);
        pieces.push(Piece::Syn(format!(
            "}} {{...{{\"{}\":({}) => ((",
            update_event, event_param
        )));
        pieces.push(Piece::Value);
        pieces.push(Piece::Syn(") = $event)}}".to_string()));
    }

    // Append the modifiers prop (each modifier name maps back to source).
    if !prop.modifiers.is_empty() && !empty_replacement {
        pieces.push(Piece::Syn(format!(" {}={{{{ ", modifier_prop)));
        for (i, m) in prop.modifiers.iter().enumerate() {
            if i > 0 {
                pieces.push(Piece::Syn(", ".to_string()));
            }
            // The prop span is overwritten, so the modifier name cannot be
            // preserved in place — emit it as a mapped insertion at its source
            // offset, followed by `: true`.
            pieces.push(Piece::Modifier(SourceByteRange::new(
                SourceByteOffset(m.start),
                SourceByteOffset(m.end),
            )));
            pieces.push(Piece::Syn(": true".to_string()));
        }
        pieces.push(Piece::Syn(" }}".to_string()));
    }

    // Delete the original prop span; everything is re-emitted as ordered pieces
    // anchored at `prop.start`. (Empty-replacement branches just delete.)
    out.overwrite(prop.start, prop_end, "");
    if empty_replacement {
        return;
    }

    let at = SourceByteOffset(prop.start);
    for piece in &pieces {
        match piece {
            Piece::Syn(text) => {
                // RELOCATED unmapped scaffolding — must interleave with the mapped
                // expression pieces at `prop.start` in insertion order.
                emit_op(
                    out,
                    &EmitOp::InsertUnmapped {
                        at,
                        text: EmitText::Borrowed(text),
                    },
                );
            }
            Piece::Value => {
                emit_jsx_binding_value(out, at, source, &value_jsx, resolver);
            }
            Piece::Arg => {
                if let Some(ref da) = dyn_arg {
                    let arg_jsx = JsxBindingValue {
                        source_expr: da.span,
                        prefix: da.prefix.as_deref(),
                        suffix: da.suffix.as_deref(),
                        occurrences: 1,
                        bindings: da.bindings,
                    };
                    emit_jsx_binding_value(out, at, source, &arg_jsx, resolver);
                }
            }
            Piece::Modifier(span) => {
                // Modifier name mapped at its source offset; no accessor prefix.
                emit_op(
                    out,
                    &EmitOp::InsertMapped {
                        at,
                        text: EmitText::Borrowed(
                            &source[span.start.0 as usize..span.end.0 as usize],
                        ),
                        source_start: span.start,
                        content_offset: GeneratedByteLen(0),
                    },
                );
            }
        }
    }
}

/// Determine the DOM property and event handler for v-model on native elements.
/// Returns (prop_name, event_name) — both are valid JSX attribute identifiers.
fn native_vmodel_prop_and_event(
    el: &ElementNode,
    source: &str,
    tag: &str,
) -> (&'static str, &'static str) {
    match tag {
        "input" => {
            // Check for type="checkbox" or type="radio"
            for prop in &el.props {
                if !prop.is_directive {
                    let name = &source[prop.start as usize..prop.name_end as usize];
                    if name == "type" {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let type_value = &source[vs as usize..ve as usize];
                            if type_value == "checkbox" || type_value == "radio" {
                                return ("checked", "onChange");
                            }
                        }
                    }
                }
            }
            ("value", "onInput")
        }
        "select" => ("value", "onChange"),
        "textarea" => ("value", "onInput"),
        _ => ("value", "onInput"),
    }
}
