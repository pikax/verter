//! Vapor2 dynamic property processing.
//!
//! Classifies directive props and emits setter calls.

use crate::ast::types::{ElementNode, PropFlags};
use crate::template::code_gen::shared::helpers::{push_u32, VaporHelper};
use crate::template::code_gen::types::CodeGenOutput;
use crate::types::NodeId;

/// Directive classification for codegen.
enum DirectiveKind<'a> {
    Class,
    Style,
    Prop(&'a str),
    Unknown,
}

/// Classify a directive based on its arg name.
fn classify_directive<'a>(
    arg: Option<&'a str>,
    prop_flag: &crate::ast::types::PropFlag,
) -> DirectiveKind<'a> {
    match arg {
        Some("class") if prop_flag.has(PropFlags::HasDynamicClass) => DirectiveKind::Class,
        Some("style") if prop_flag.has(PropFlags::HasDynamicStyle) => DirectiveKind::Style,
        Some(name) => DirectiveKind::Prop(name),
        None => DirectiveKind::Unknown,
    }
}

/// Process dynamic props on an element, emitting setter lines for the render effect.
///
/// Returns `true` if any dynamic props were emitted.
pub fn process_dynamic_props<'alloc>(
    id: NodeId,
    element: &ElementNode,
    source: &'alloc str,
    effect_lines: &mut Vec<&'alloc str>,
    out: &mut CodeGenOutput<'alloc>,
) -> bool {
    if element.prop_flag.is_empty() || !element.prop_flag.needs_oxc_parsing() {
        return false;
    }

    // Check for v-text
    if element.prop_flag.has(PropFlags::HasVText) {
        for prop in &element.props {
            if !prop.is_directive {
                continue;
            }
            let name = &source[prop.start as usize..prop.name_end as usize];
            if name == "v-text" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let expr = &source[vs as usize..ve as usize];
                    let mut line = String::with_capacity(40);
                    line.push_str("    _setText(n");
                    push_u32(&mut line, id.0 as u32);
                    line.push_str(", ");
                    line.push_str(expr);
                    line.push(')');
                    effect_lines.push(out.alloc_str(&line));
                    out.add_vapor_import(VaporHelper::SetText);
                    return true;
                }
            }
        }
    }

    // Check for v-html
    if element.prop_flag.has(PropFlags::HasVHtml) {
        for prop in &element.props {
            if !prop.is_directive {
                continue;
            }
            let name = &source[prop.start as usize..prop.name_end as usize];
            if name == "v-html" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let expr = &source[vs as usize..ve as usize];
                    let mut line = String::with_capacity(40);
                    line.push_str("    _setHtml(n");
                    push_u32(&mut line, id.0 as u32);
                    line.push_str(", ");
                    line.push_str(expr);
                    line.push(')');
                    effect_lines.push(out.alloc_str(&line));
                    out.add_vapor_import(VaporHelper::SetHtml);
                }
            }
        }
    }

    let mut emitted = false;

    for prop in &element.props {
        if !prop.is_directive {
            continue;
        }

        let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) else {
            continue;
        };
        let value = &source[vs as usize..ve as usize];

        // Get directive arg
        let arg = match (prop.arg_start, prop.arg_end) {
            (Some(s), Some(e)) => Some(&source[s as usize..e as usize]),
            _ => None,
        };

        // Skip non-bind directives (v-if, v-for, v-slot, events, etc.)
        let name = &source[prop.start as usize..prop.name_end as usize];
        if name.starts_with("v-") && name != "v-bind" && name != "v-html" {
            continue;
        }
        // Skip event listeners (@click, v-on:click)
        if name.starts_with('@') || name == "v-on" {
            continue;
        }

        // For v-bind="obj" (no arg) → spread
        if (name == "v-bind" || name == ":") && arg.is_none() {
            let mut line = String::with_capacity(48);
            line.push_str("    _setDynamicProps(n");
            push_u32(&mut line, id.0 as u32);
            line.push_str(", [");
            line.push_str(value);
            line.push_str("])");
            effect_lines.push(out.alloc_str(&line));
            out.add_vapor_import(VaporHelper::SetDynamicProps);
            emitted = true;
            continue;
        }

        match classify_directive(arg, &element.prop_flag) {
            DirectiveKind::Class => {
                let mut line = String::with_capacity(40);
                line.push_str("    _setClass(n");
                push_u32(&mut line, id.0 as u32);
                line.push_str(", ");
                line.push_str(value);
                line.push(')');
                effect_lines.push(out.alloc_str(&line));
                out.add_vapor_import(VaporHelper::SetClass);
                emitted = true;
            }
            DirectiveKind::Style => {
                let mut line = String::with_capacity(40);
                line.push_str("    _setStyle(n");
                push_u32(&mut line, id.0 as u32);
                line.push_str(", ");
                line.push_str(value);
                line.push(')');
                effect_lines.push(out.alloc_str(&line));
                out.add_vapor_import(VaporHelper::SetStyle);
                emitted = true;
            }
            DirectiveKind::Prop(attr) => {
                let mut line = String::with_capacity(48);
                line.push_str("    _setProp(n");
                push_u32(&mut line, id.0 as u32);
                line.push_str(", \"");
                line.push_str(attr);
                line.push_str("\", ");
                line.push_str(value);
                line.push(')');
                effect_lines.push(out.alloc_str(&line));
                out.add_vapor_import(VaporHelper::SetProp);
                emitted = true;
            }
            DirectiveKind::Unknown => {}
        }
    }

    emitted
}
