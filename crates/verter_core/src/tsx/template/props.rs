//! Prop/attribute → JSX conversion for TSX template codegen.
//!
//! Converts Vue template props and events to JSX equivalents:
//! - Static attributes: pass through
//! - `:prop="expr"` → `prop={expr}`
//! - `@event="handler"` → `onEvent={handler}`
//! - `v-bind="obj"` → `{...obj}` (spread)
//! - `v-on="{ ... }"` → `{...{ ... }}` (spread events, #49)

use oxc_allocator::Allocator;

use crate::ast::types::{ElementNode, TagType};
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::types::CodeGenOutput;
use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;
use crate::template::oxc::types::{OxcParsedElement, OxcParsedProp};
use crate::types::NodeProp;

/// Process all props on an element, converting to JSX syntax.
pub fn process_element_props<'alloc>(
    el: &ElementNode,
    oxc_el: Option<&OxcParsedElement<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
) {
    for (i, prop) in el.props.iter().enumerate() {
        // Find corresponding OXC data for this prop
        let oxc_prop = oxc_el.and_then(|el| el.props.iter().find(|p| p.prop_index == i));

        // Skip structural directives (v-if, v-for, v-slot) — handled separately
        if is_structural_directive(prop, source) {
            // Remove the directive from output
            remove_prop(prop, source, out);
            continue;
        }

        // Skip v-show — handled separately
        if is_builtin_directive(prop, source, "show") {
            continue;
        }

        // Keep raw v-model for now, but still prefix its argument/value bindings
        // so TSX type expressions stay aligned with v5 binding behavior.
        if is_builtin_directive(prop, source, "model") {
            if let Some(oxc_p) = oxc_prop {
                if let Some(ref exp) = oxc_p.exp {
                    if let Some(ref bindings) = exp.bindings {
                        resolver.collect_binding_patches(bindings, out);
                    }
                }
                if let Some(ref arg) = oxc_p.arg {
                    if let Some(ref bindings) = arg.bindings {
                        resolver.collect_binding_patches(bindings, out);
                    }
                }
            }
            continue;
        }

        if !prop.is_directive {
            // Static attribute — pass through
            continue;
        }

        let dir_name = get_directive_name(prop, source);

        // `<component :is="...">` is rewritten at the element level.
        // Skip prop-level bind transform to avoid overlapping rewrites.
        if el.tag_type == TagType::Component
            && &source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize]
                == "component"
            && dir_name == "bind"
            && prop.is_dynamic != Some(true)
        {
            if let (Some(arg_start), Some(arg_end)) = (prop.arg_start, prop.arg_end) {
                if source[arg_start as usize..arg_end as usize].trim() == "is" {
                    continue;
                }
            }
        }

        match dir_name {
            "bind" => process_v_bind(prop, oxc_prop, source, out, alloc, resolver),
            "on" => process_v_on(prop, oxc_prop, source, out, alloc, resolver),
            "html" => process_v_html(prop, source, out),
            "text" => process_v_text(prop, source, out),
            _ => {
                // Unknown directive — remove it (TSX can't represent custom directives)
                remove_prop(prop, source, out);
            }
        }
    }
}

/// Process `v-bind` / `:` directive.
///
/// - `:prop="expr"` → `prop={expr}`
/// - `v-bind:prop="expr"` → `prop={expr}`
/// - `v-bind="obj"` → `{...obj}` (spread)
/// - `:[key]="val"` → `{...{[key]: val}}` (dynamic key)
fn process_v_bind<'alloc>(
    prop: &NodeProp,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
) {
    let has_arg = prop.arg_start.is_some();
    let raw_name = &source[prop.start as usize..prop.name_end as usize];

    if !has_arg {
        // `.foo="bar"` shorthand for v-bind prop modifier
        if raw_name.starts_with('.') {
            let key = raw_name.trim_start_matches('.').trim();
            if key.is_empty() {
                return;
            }
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value_expr = &source[vs as usize..ve as usize];
                let resolved = resolve_prefixed_expr(value_expr, vs, oxc_prop, resolver);
                let prop_end = get_prop_end(prop);
                out.overwrite(prop.start, prop_end, &format!("{}={{{}}}", key, resolved));
            } else {
                let resolved = resolver.resolve_simple_expr(&kebab_to_camel_case(key));
                let prop_end = get_prop_end(prop);
                out.overwrite(prop.start, prop_end, &format!("{}={{{}}}", key, resolved));
            }
            return;
        }

        // v-bind="obj" → spread: `{...obj}`
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value_expr = &source[vs as usize..ve as usize];
            let resolved = resolve_prefixed_expr(value_expr, vs, oxc_prop, resolver);
            // Replace entire prop with spread
            let prop_end = get_prop_end(prop);
            out.overwrite(prop.start, prop_end, &format!("{{...{}}}", resolved));
        }
        return;
    }

    let arg_start = prop.arg_start.unwrap();
    let arg_end = prop.arg_end.unwrap();
    let arg_name = &source[arg_start as usize..arg_end as usize];

    // Dynamic key: :[key]="val" → {...{[key]: val}}
    if prop.is_dynamic == Some(true) {
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value_expr = &source[vs as usize..ve as usize];
            let value_resolved = resolve_prefixed_expr(value_expr, vs, oxc_prop, resolver);
            let arg_expr = arg_name
                .trim()
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or(arg_name)
                .trim();
            let arg_resolved = resolve_prefixed_dynamic_arg(arg_expr, oxc_prop, resolver);
            let prop_end = get_prop_end(prop);
            out.overwrite(
                prop.start,
                prop_end,
                &format!("{{...{{[{}]: {}}}}}", arg_resolved, value_resolved),
            );
        }
        return;
    }

    // Static key: :prop="expr" → prop={expr}
    let prop_end = get_prop_end(prop);
    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
        let value_expr = &source[vs as usize..ve as usize];
        let resolved = resolve_prefixed_expr(value_expr, vs, oxc_prop, resolver);
        out.overwrite(
            prop.start,
            prop_end,
            &format!("{}={{{}}}", arg_name, resolved),
        );
    } else {
        // `:foo` shorthand → `foo={ctx.foo}`; `:foo-bar` uses camelCase lookup.
        let resolved = resolver.resolve_simple_expr(&kebab_to_camel_case(arg_name.trim()));
        out.overwrite(
            prop.start,
            prop_end,
            &format!("{}={{{}}}", arg_name, resolved),
        );
    }
}

/// Process `v-on` / `@` directive.
///
/// - `@click="handler"` → `onClick={handler}`
/// - `@click="handler($event)"` → `onClick={($event) => handler($event)}`
/// - `v-on="{ mousedown: doThis }"` → `{...{ mousedown: doThis }}` (spread, #49)
fn process_v_on<'alloc>(
    prop: &NodeProp,
    oxc_prop: Option<&OxcParsedProp<'alloc>>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    resolver: &BindingResolver<'alloc>,
) {
    let has_arg = prop.arg_start.is_some();

    if !has_arg {
        // v-on="{ mousedown: doThis }" → spread: {...{ mousedown: doThis }}
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value_expr = &source[vs as usize..ve as usize];
            let prop_end = get_prop_end(prop);
            out.overwrite(prop.start, prop_end, &format!("{{...{}}}", value_expr));

            // Apply binding patches
            if let Some(oxc_p) = oxc_prop {
                if let Some(ref exp) = oxc_p.exp {
                    if let Some(ref bindings) = exp.bindings {
                        resolver.collect_binding_patches(bindings, out);
                    }
                }
            }
        }
        return;
    }

    let arg_start = prop.arg_start.unwrap();
    let arg_end = prop.arg_end.unwrap();
    let event_name = &source[arg_start as usize..arg_end as usize];

    // Convert event name to JSX: click → onClick, update:modelValue → onUpdate:modelValue
    let jsx_event_name = event_to_jsx_name(event_name);

    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
        let value_expr = &source[vs as usize..ve as usize];
        let resolved_expr = resolve_prefixed_expr(value_expr, vs, oxc_prop, resolver);
        let resolved_expr = resolved_expr.trim();

        // Determine if the handler needs wrapping
        let is_simple_ident = crate::template::code_gen::binding::is_simple_ident(resolved_expr);
        let is_member_expr = resolved_expr.contains('.') && !resolved_expr.contains('(');
        let is_fn_expr = resolved_expr.starts_with("(")
            || resolved_expr.starts_with("function")
            || resolved_expr.contains("=>");
        let is_object_expr = resolved_expr.starts_with('{') && resolved_expr.ends_with('}');

        // Build prop end position (including modifiers and quotes)
        let prop_end = get_prop_end(prop);

        if is_simple_ident || is_member_expr || is_fn_expr || is_object_expr {
            // Simple handler: @click="handler" → onClick={handler}
            out.overwrite(
                prop.start,
                prop_end,
                &format!("{}={{{}}}", jsx_event_name, resolved_expr),
            );
        } else {
            // Inline expression: @click="count++" → onClick={() => count++}
            // Or expression with $event: @click="handler($event)" → onClick={($event) => handler($event)}
            let has_event_param = resolved_expr.contains("$event");
            if has_event_param {
                out.overwrite(
                    prop.start,
                    prop_end,
                    &format!("{}={{($event) => {{{}}}}}", jsx_event_name, resolved_expr),
                );
            } else {
                out.overwrite(
                    prop.start,
                    prop_end,
                    &format!("{}={{() => {{{}}}}}", jsx_event_name, resolved_expr),
                );
            }
        }
    } else {
        // Event with no value — just remove
        let prop_end = get_prop_end(prop);
        out.overwrite(prop.start, prop_end, "");
    }
}

/// Process `v-html="expr"` → `dangerouslySetInnerHTML={{__html: expr}}`.
fn process_v_html(prop: &NodeProp, source: &str, out: &mut CodeGenOutput<'_>) {
    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
        let expr = &source[vs as usize..ve as usize];
        let prop_end = get_prop_end(prop);
        out.overwrite(prop.start, prop_end, &format!("innerHTML={{{}}}", expr));
    }
}

/// Process `v-text="expr"` → `textContent={{expr}}`.
fn process_v_text(prop: &NodeProp, source: &str, out: &mut CodeGenOutput<'_>) {
    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
        let expr = &source[vs as usize..ve as usize];
        let prop_end = get_prop_end(prop);
        out.overwrite(prop.start, prop_end, &format!("textContent={{{}}}", expr));
    }
}

// ── Helpers ───────────────────────────────────────────────────────

/// Get the directive name from a NodeProp (e.g., "bind", "on", "if", "for").
fn get_directive_name<'a>(prop: &NodeProp, source: &'a str) -> &'a str {
    let name = &source[prop.start as usize..prop.name_end as usize];

    // Handle shorthand: ':' for v-bind, '@' for v-on, '#' for v-slot
    if name.starts_with(':') || name.starts_with('.') {
        return "bind";
    }
    if name.starts_with('@') {
        return "on";
    }
    if name.starts_with('#') {
        return "slot";
    }

    // Full directive name: v-bind, v-on, v-if, etc.
    name.strip_prefix("v-").unwrap_or(name)
}

/// Check if a prop is a structural directive (v-if, v-else-if, v-else, v-for, v-slot).
fn is_structural_directive(prop: &NodeProp, source: &str) -> bool {
    if !prop.is_directive {
        return false;
    }
    let name = get_directive_name(prop, source);
    matches!(name, "if" | "else-if" | "else" | "for" | "slot")
}

/// Check if a prop is a specific built-in directive.
fn is_builtin_directive(prop: &NodeProp, source: &str, dir: &str) -> bool {
    if !prop.is_directive {
        return false;
    }
    get_directive_name(prop, source) == dir
}

/// Remove a prop from output by overwriting with empty string.
/// Handles the full span including value and modifiers.
fn remove_prop(prop: &NodeProp, _source: &str, out: &mut CodeGenOutput<'_>) {
    let prop_end = get_prop_end(prop);
    out.overwrite(prop.start, prop_end, "");
}

/// Get the end position of a prop (including value and closing quote).
pub(crate) fn get_prop_end(prop: &NodeProp) -> u32 {
    // Priority: value_end + 1 (closing quote) > modifiers end > name_end
    if let Some(ve) = prop.value_end {
        ve + 1 // +1 for closing quote
    } else if !prop.modifiers.is_empty() {
        prop.modifiers
            .last()
            .map(|m| m.end)
            .unwrap_or(prop.name_end)
    } else if let Some(ae) = prop.arg_end {
        // Check for dynamic arg closing bracket
        ae
    } else {
        prop.name_end
    }
}

/// Convert a Vue event name to JSX event prop name.
///
/// - `click` → `onClick`
/// - `update:modelValue` → `onUpdate:modelValue`
/// - `custom-event` → `onCustomEvent`  (camelCase)
fn event_to_jsx_name(event_name: &str) -> String {
    // Special case: update: prefix (for v-model)
    if let Some(rest) = event_name.strip_prefix("update:") {
        return format!("onUpdate:{}", rest);
    }

    let mut result = String::with_capacity(event_name.len() + 2);
    result.push_str("on");

    let mut capitalize_next = true;
    for ch in event_name.chars() {
        if ch == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            for upper in ch.to_uppercase() {
                result.push(upper);
            }
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }

    result
}

fn resolve_prefixed_expr(
    raw_expr: &str,
    expr_start: u32,
    oxc_prop: Option<&OxcParsedProp<'_>>,
    resolver: &BindingResolver<'_>,
) -> String {
    if let Some(oxc_p) = oxc_prop {
        if let Some(ref exp) = oxc_p.exp {
            return build_prefixed_expr(raw_expr, expr_start, exp, resolver, &[]);
        }
    }
    resolver.resolve_simple_expr(raw_expr)
}

fn resolve_prefixed_dynamic_arg(
    raw_arg: &str,
    oxc_prop: Option<&OxcParsedProp<'_>>,
    resolver: &BindingResolver<'_>,
) -> String {
    if let Some(oxc_p) = oxc_prop {
        if let Some(ref arg) = oxc_p.arg {
            if let Some(ref bindings) = arg.bindings {
                // Dynamic args are simple in practice for TSX parity cases (`[msg]`),
                // so resolve via simple expression and avoid positional patches.
                if bindings.bindings.len() == 1 {
                    return resolver.resolve_simple_expr(raw_arg);
                }
            }
        }
    }
    resolver.resolve_simple_expr(raw_arg)
}

fn kebab_to_camel_case(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut upper_next = false;
    for ch in input.chars() {
        if ch == '-' {
            upper_next = true;
            continue;
        }
        if upper_next {
            for uc in ch.to_uppercase() {
                out.push(uc);
            }
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_to_jsx_click() {
        assert_eq!(event_to_jsx_name("click"), "onClick");
    }

    #[test]
    fn event_to_jsx_update_model() {
        assert_eq!(
            event_to_jsx_name("update:modelValue"),
            "onUpdate:modelValue"
        );
    }

    #[test]
    fn event_to_jsx_kebab() {
        assert_eq!(event_to_jsx_name("custom-event"), "onCustomEvent");
    }

    #[test]
    fn event_to_jsx_simple() {
        assert_eq!(event_to_jsx_name("input"), "onInput");
        assert_eq!(event_to_jsx_name("change"), "onChange");
        assert_eq!(event_to_jsx_name("mousedown"), "onMousedown");
    }

    #[test]
    fn get_prop_end_with_value() {
        let prop = NodeProp {
            start: 0,
            name_end: 5,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            value_start: Some(7),
            value_end: Some(12),
            modifiers: smallvec::smallvec![],
            is_dynamic: None,
        };
        assert_eq!(get_prop_end(&prop), 13); // value_end + 1
    }

    #[test]
    fn get_prop_end_without_value() {
        let prop = NodeProp {
            start: 0,
            name_end: 5,
            is_directive: true,
            arg_start: None,
            arg_end: None,
            value_start: None,
            value_end: None,
            modifiers: smallvec::smallvec![],
            is_dynamic: None,
        };
        assert_eq!(get_prop_end(&prop), 5); // name_end
    }
}
