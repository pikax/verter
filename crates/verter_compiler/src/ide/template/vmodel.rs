//! IDE `v-model` → JSX expansion.
//!
//! `v-model` is the hardest IDE emit site: it emits the bound expression 2-3
//! times (a read occurrence plus an assignment occurrence inside the update
//! handler), and for a dynamic arg the computed prop/event names embed the arg
//! expression. The value/arg expressions are planned ONCE through the unified
//! `plan_user_expr` planner; each occurrence re-emits the plan through the
//! relocated sink (`emit_expr_plan`), so every occurrence's identifier maps back
//! to the same source span while all assignment / call / punctuation scaffolding
//! is unmapped. Extracted from `props.rs` to keep both files within the production
//! line-count budget.

use verter_span::{GeneratedByteLen, SourceByteOffset, SourceByteRange};

use super::props::{get_prop_end, kebab_to_camel_case};
use crate::ast::types::ElementNode;
use crate::ide::get_directive_name;
use crate::ide::template::emit::{
    emit_expr_plan, emit_op, plan_user_expr, EmitOp, EmitText, ExprOptions, ExprPlan, Placement,
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

    // Plan the value expression ONCE through the unified planner. Each generated
    // occurrence (v-model emits the value 2-3×: a read plus an assignment LHS)
    // re-emits this plan through the relocated sink, so every occurrence's
    // identifier maps back to the SAME source span.
    let value_bindings = oxc_prop
        .and_then(|p| p.exp.as_ref())
        .and_then(|e| e.bindings.as_ref())
        .map(|b| b.bindings.as_slice());
    let value_plan = plan_user_expr(
        source,
        SourceByteRange::new(SourceByteOffset(tvs), SourceByteOffset(tve)),
        value_bindings,
        resolver,
        ExprOptions::default(),
    );

    // Determine prop name: default "modelValue" or named v-model:xxx.
    // For a dynamic arg, the computed prop/event/modifier names embed the arg
    // expression, which must itself map back to source; `dyn_arg` captures the
    // arg span + bindings for those mapped emissions.
    let is_dynamic_arg = prop.is_dynamic == Some(true);
    // The dynamic-arg expression is planned ONCE (computed prop/event/modifier
    // names embed it); each `Piece::Arg` re-emits this plan relocated.
    let mut dyn_arg_plan: Option<ExprPlan<'_>> = None;
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
                .and_then(|a| a.bindings.as_ref())
                .map(|b| b.bindings.as_slice());
            dyn_arg_plan = Some(plan_user_expr(
                source,
                SourceByteRange::new(
                    SourceByteOffset(raw_arg_start),
                    SourceByteOffset(raw_arg_end),
                ),
                arg_bindings,
                resolver,
                ExprOptions::default(),
            ));
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
            // The element already declares BOTH the DOM prop and its handler, so
            // v-model's generated value/event pieces would be redundant — push
            // none. Any MODIFIERS are still emitted by the modifier block below.
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
    //
    // The modifier prop is emitted whenever modifiers are present — even when
    // value/event generation is redundant (`empty_replacement`, i.e. the element
    // already declares both the DOM prop and its handler). `empty_replacement`
    // suppresses ONLY the generated value/event pieces, NOT the modifier prop:
    // `<input v-model.trim :value @input>` must still publish `modelModifiers={{
    // trim: true }}`. (The dynamic-arg computed-name modifier path below is never
    // reached under `empty_replacement` — that flag is set only on the native
    // static-arg branch.)
    if !prop.modifiers.is_empty() {
        if is_dynamic_arg {
            // Dynamic arg: the modifiers prop name is COMPUTED. A computed name is
            // NOT a valid bare JSX attribute (`[`…`]={…}` is illegal, and an empty
            // `modifier_prop` would emit an invalid ` ={{`), so emit it as a spread
            // with a computed object key — `{...{[`${ARG}Modifiers`]: { … }}}` —
            // matching how the dynamic-arg model value/event are also spread. The
            // arg expression is a mapped `Piece::Arg`; the surrounding punctuation is
            // unmapped.
            pieces.push(Piece::Syn(" {...{[`${".to_string()));
            pieces.push(Piece::Arg);
            pieces.push(Piece::Syn("}Modifiers`]: { ".to_string()));
        } else {
            pieces.push(Piece::Syn(format!(" {}={{{{ ", modifier_prop)));
        }
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
        if is_dynamic_arg {
            // Close the inner object + the spread object + the JSX expression.
            pieces.push(Piece::Syn(" }}}".to_string()));
        } else {
            pieces.push(Piece::Syn(" }}".to_string()));
        }
    }

    // Delete the original prop span; everything is re-emitted as ordered pieces
    // anchored at `prop.start`. Return early ONLY when the final piece list is
    // empty — `empty_replacement` (redundant value/event) still emits modifier
    // pieces, so a v-model with modifiers is never a bare deletion.
    out.overwrite(prop.start, prop_end, "");
    if pieces.is_empty() {
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
                // Re-emit the value plan relocated for this occurrence — every
                // occurrence's identifier maps back to the same source span.
                emit_expr_plan(out, &value_plan, Placement::Relocated { at }, source);
            }
            Piece::Arg => {
                if let Some(ref plan) = dyn_arg_plan {
                    emit_expr_plan(out, plan, Placement::Relocated { at }, source);
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
