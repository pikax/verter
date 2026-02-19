//! Vapor2 directive code generation (v-model, v-show, custom directives).

use crate::new_impl::ast::types::{ElementNode, PropFlags};
use crate::new_impl::template::code_gen::shared::helpers::push_u32;
use crate::new_impl::template::code_gen::types::CodeGenOutput;
use crate::new_impl::types::NodeId;

/// Process v-show directive on an element.
///
/// Emits: `_applyVShow(n{id}, () => (expr))`
/// Returns `true` if v-show was processed.
pub fn process_v_show<'alloc>(
    id: NodeId,
    element: &ElementNode,
    source: &'alloc str,
    body_lines: &mut Vec<&'alloc str>,
    out: &mut CodeGenOutput<'alloc>,
) -> bool {
    if !element.prop_flag.has(PropFlags::HasShow) {
        return false;
    }

    for prop in &element.props {
        if !prop.is_directive {
            continue;
        }
        let name = &source[prop.start as usize..prop.name_end as usize];
        if name == "v-show" {
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let expr = &source[vs as usize..ve as usize];
                let mut line = String::with_capacity(48);
                line.push_str("  _applyVShow(n");
                push_u32(&mut line, id.0 as u32);
                line.push_str(", () => (");
                line.push_str(expr);
                line.push_str("))");
                body_lines.push(out.alloc_str(&line));
                // v-show uses the _vShow directive internally but the vapor
                // API exposes _applyVShow — for now just track as a statement
            }
            return true;
        }
    }

    false
}

/// Process v-model directive on an element.
///
/// Classifies by element type and emits the appropriate model applier.
/// Returns `true` if v-model was processed.
pub fn process_v_model<'alloc>(
    id: NodeId,
    element: &ElementNode,
    source: &'alloc str,
    body_lines: &mut Vec<&'alloc str>,
    out: &mut CodeGenOutput<'alloc>,
) -> bool {
    if !element.prop_flag.has(PropFlags::HasModel) {
        return false;
    }

    let tag_name = &source[element.tag_open.start as usize + 1..element.tag_open.name_end as usize];

    for prop in &element.props {
        if !prop.is_directive {
            continue;
        }
        let name = &source[prop.start as usize..prop.name_end as usize];
        if name != "v-model" {
            continue;
        }
        let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) else {
            continue;
        };
        let expr = &source[vs as usize..ve as usize];

        // Determine input type
        let input_type = get_input_type(element, source);
        let applier = match tag_name {
            "select" => "_applySelectModel",
            "textarea" => "_applyTextModel",
            "input" => match input_type.as_deref() {
                Some("checkbox") => "_applyCheckboxModel",
                Some("radio") => "_applyRadioModel",
                _ => "_applyTextModel",
            },
            _ => "_applyTextModel", // Default
        };

        let mut line = String::with_capacity(80);
        line.push_str("  ");
        line.push_str(applier);
        line.push_str("(n");
        push_u32(&mut line, id.0 as u32);
        line.push_str(", () => ");
        line.push_str(expr);
        line.push_str(", v => ");
        line.push_str(expr);
        line.push_str(" = v");

        // Collect modifiers
        let modifiers: Vec<&str> = prop
            .modifiers
            .iter()
            .map(|m| &source[m.start as usize..m.end as usize])
            .collect();
        if !modifiers.is_empty() {
            line.push_str(", { ");
            for (i, m) in modifiers.iter().enumerate() {
                if i > 0 {
                    line.push_str(", ");
                }
                line.push_str(m);
                line.push_str(": true");
            }
            line.push_str(" }");
        }

        line.push(')');
        body_lines.push(out.alloc_str(&line));
        return true;
    }

    false
}

/// Process custom directives on an element.
///
/// Emits: `_withVaporDirectives(n{id}, [[_resolveDirective("name"), expr]])`
/// Returns `true` if any custom directives were processed.
pub fn process_custom_directives<'alloc>(
    id: NodeId,
    element: &ElementNode,
    source: &'alloc str,
    body_lines: &mut Vec<&'alloc str>,
    out: &mut CodeGenOutput<'alloc>,
) -> bool {
    if !element.prop_flag.has(PropFlags::HasCustomDirective) {
        return false;
    }

    let mut emitted = false;
    for prop in &element.props {
        if !prop.is_directive {
            continue;
        }
        let full_name = &source[prop.start as usize..prop.name_end as usize];
        // Custom directives start with v- but are not built-in
        if !full_name.starts_with("v-") {
            continue;
        }
        let directive_name = &full_name[2..];
        // Skip built-in directives
        if matches!(
            directive_name,
            "if" | "else-if"
                | "else"
                | "for"
                | "slot"
                | "once"
                | "show"
                | "model"
                | "html"
                | "text"
                | "bind"
                | "on"
                | "memo"
                | "pre"
                | "cloak"
        ) {
            continue;
        }

        let mut line = String::with_capacity(80);
        line.push_str("  _withVaporDirectives(n");
        push_u32(&mut line, id.0 as u32);
        line.push_str(", [[_resolveDirective(\"");
        line.push_str(directive_name);
        line.push_str("\")");

        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let expr = &source[vs as usize..ve as usize];
            line.push_str(", ");
            line.push_str(expr);
        }

        line.push_str("]])");
        body_lines.push(out.alloc_str(&line));
        emitted = true;
    }

    emitted
}

/// Process template ref on an element.
///
/// Emits: `_setTemplateRef(n{id}, "refName")`
/// Returns `true` if a ref was processed.
pub fn process_template_ref<'alloc>(
    id: NodeId,
    element: &ElementNode,
    source: &'alloc str,
    body_lines: &mut Vec<&'alloc str>,
    out: &mut CodeGenOutput<'alloc>,
) -> bool {
    if !element.prop_flag.has(PropFlags::HasRef) {
        return false;
    }

    for prop in &element.props {
        if prop.is_directive {
            continue;
        }
        let name = &source[prop.start as usize..prop.name_end as usize];
        if name == "ref" {
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let ref_name = &source[vs as usize..ve as usize];
                let mut line = String::with_capacity(40);
                line.push_str("  _setTemplateRef(n");
                push_u32(&mut line, id.0 as u32);
                line.push_str(", \"");
                line.push_str(ref_name);
                line.push_str("\")");
                body_lines.push(out.alloc_str(&line));
                return true;
            }
        }
    }

    false
}

// Note: v-pre is handled by the tokenizer — it suppresses expression/directive
// parsing for the entire subtree, so v-pre elements appear as plain static
// elements to codegen. No codegen-side handling is needed.
//
// Note: v-cloak is a no-op at compile time — it's parsed as a directive
// (is_directive: true) and automatically stripped from HTML by build_open_tag_html.
//
// Note: v-memo is handled in close_render_effect() in mod.rs — it wraps the
// render effect body with _withMemo([deps], () => { ... }, _cache, idx).

/// Get the type attribute value for an input element.
fn get_input_type(element: &ElementNode, source: &str) -> Option<String> {
    for prop in &element.props {
        if prop.is_directive {
            continue;
        }
        let name = &source[prop.start as usize..prop.name_end as usize];
        if name == "type" {
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                return Some(source[vs as usize..ve as usize].to_string());
            }
        }
    }
    None
}
