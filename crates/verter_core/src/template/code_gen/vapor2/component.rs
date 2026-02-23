//! Vapor2 component code generation.
//!
//! Handles component detection, resolution, prop/slot generation.

use crate::ast::types::{ElementNode, TagType};
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::shared::helpers::{push_u32, VaporHelper};
use crate::template::code_gen::types::CodeGenOutput;
use crate::types::NodeId;

/// Check if an element is a component.
pub fn is_component(element: &ElementNode) -> bool {
    element.tag_type == TagType::Component
}

/// Check if an element is a slot outlet (<slot>).
pub fn is_slot_outlet(element: &ElementNode) -> bool {
    element.tag_type == TagType::SlotOutlet
}

/// Resolve a component identifier from bindings.
///
/// Checks the resolver for exact match, then PascalCase.
/// Falls back to `_resolveComponent("Name")`.
pub fn resolve_component<'alloc>(
    tag_name: &str,
    resolver: &BindingResolver<'alloc>,
    _out: &mut CodeGenOutput<'alloc>,
) -> String {
    // Check exact binding
    if resolver.get(tag_name).is_some() {
        let prefix = resolver.resolve_prefix(tag_name);
        let suffix = resolver.resolve_suffix(tag_name);
        let mut buf = String::with_capacity(tag_name.len() + prefix.len() + suffix.len());
        buf.push_str(prefix);
        buf.push_str(tag_name);
        buf.push_str(suffix);
        return buf;
    }

    // Check PascalCase conversion
    let pascal = to_pascal_case(tag_name);
    if resolver.get(&pascal).is_some() {
        let prefix = resolver.resolve_prefix(&pascal);
        let suffix = resolver.resolve_suffix(&pascal);
        let mut buf = String::with_capacity(pascal.len() + prefix.len() + suffix.len());
        buf.push_str(prefix);
        buf.push_str(&pascal);
        buf.push_str(suffix);
        return buf;
    }

    // Fallback: _resolveComponent("Name")
    let mut buf = String::with_capacity(tag_name.len() + 24);
    buf.push_str("_resolveComponent(\"");
    buf.push_str(tag_name);
    buf.push_str("\")");
    buf
}

/// Generate component creation code.
///
/// Output: `const n{id} = _createComponent(comp, { props }, { slots })`
pub fn process_component<'alloc>(
    id: NodeId,
    element: &ElementNode,
    source: &'alloc str,
    resolver: &BindingResolver<'alloc>,
    body_lines: &mut Vec<&'alloc str>,
    out: &mut CodeGenOutput<'alloc>,
) {
    let tag_name = &source[element.tag_open.start as usize + 1..element.tag_open.name_end as usize];

    // Dynamic component: <component :is="expr">
    let is_dynamic_component = tag_name == "component";
    let comp_ref = if is_dynamic_component {
        // Find :is directive to get the component expression
        let is_expr = element.props.iter().find_map(|p| {
            if !p.is_directive {
                return None;
            }
            let name = &source[p.start as usize..p.name_end as usize];
            let arg = match (p.arg_start, p.arg_end) {
                (Some(s), Some(e)) => Some(&source[s as usize..e as usize]),
                _ => None,
            };
            if (name.starts_with(':') || name == "v-bind") && arg == Some("is") {
                p.value_start
                    .zip(p.value_end)
                    .map(|(vs, ve)| &source[vs as usize..ve as usize])
            } else {
                None
            }
        });
        match is_expr {
            Some(expr) => {
                let mut buf = String::with_capacity(expr.len() + 30);
                buf.push_str("_resolveDynamicComponent(");
                buf.push_str(expr);
                buf.push(')');
                buf
            }
            None => resolve_component(tag_name, resolver, out),
        }
    } else {
        resolve_component(tag_name, resolver, out)
    };

    let mut line = String::with_capacity(128);
    line.push_str("  const n");
    push_u32(&mut line, id.0 as u32);
    line.push_str(" = _createComponent(");
    line.push_str(&comp_ref);

    // Props object (skip :is for dynamic components)
    let has_props = element.props.iter().any(|p| {
        if !p.is_directive {
            return true;
        }
        let name = &source[p.start as usize..p.name_end as usize];
        let arg = match (p.arg_start, p.arg_end) {
            (Some(s), Some(e)) => Some(&source[s as usize..e as usize]),
            _ => None,
        };
        // Skip :is for dynamic components
        if is_dynamic_component && arg == Some("is") {
            return false;
        }
        // Include bind directives as props
        name.starts_with(':') || name == "v-bind" || name == "v-model"
    });

    if has_props {
        line.push_str(", { ");
        let mut first = true;
        for prop in &element.props {
            if prop.is_directive {
                let name = &source[prop.start as usize..prop.name_end as usize];
                if let Some(arg) = name.strip_prefix(':') {
                    // Skip :is for dynamic components
                    if is_dynamic_component && arg == "is" {
                        continue;
                    }
                    // :prop="expr" → prop: () => (expr)
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        if !first {
                            line.push_str(", ");
                        }
                        line.push_str(arg);
                        line.push_str(": () => (");
                        line.push_str(&source[vs as usize..ve as usize]);
                        line.push(')');
                        first = false;
                    }
                }
                continue;
            }
            // Static prop: name: "value"
            let name = &source[prop.start as usize..prop.name_end as usize];
            if !first {
                line.push_str(", ");
            }
            line.push_str(name);
            line.push_str(": ");
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                line.push('"');
                line.push_str(&source[vs as usize..ve as usize]);
                line.push('"');
            } else {
                line.push_str("true");
            }
            first = false;
        }
        line.push_str(" }");
    } else {
        line.push_str(", null");
    }

    line.push(')');

    body_lines.push(out.alloc_str(&line));
    out.add_vapor_import(VaporHelper::CreateComponent);
}

/// Generate component creation code with slot closures.
///
/// Output: `const n{id} = _createComponent(comp, { props }, { slot: () => { ... } })`
pub fn process_component_with_slots<'alloc>(
    id: NodeId,
    element: &ElementNode,
    source: &'alloc str,
    resolver: &BindingResolver<'alloc>,
    slots_str: Option<&str>,
    body_lines: &mut Vec<&'alloc str>,
    out: &mut CodeGenOutput<'alloc>,
) {
    let tag_name = &source[element.tag_open.start as usize + 1..element.tag_open.name_end as usize];

    let is_dynamic_component = tag_name == "component";
    let comp_ref = if is_dynamic_component {
        let is_expr = element.props.iter().find_map(|p| {
            if !p.is_directive {
                return None;
            }
            let name = &source[p.start as usize..p.name_end as usize];
            let arg = match (p.arg_start, p.arg_end) {
                (Some(s), Some(e)) => Some(&source[s as usize..e as usize]),
                _ => None,
            };
            if (name.starts_with(':') || name == "v-bind") && arg == Some("is") {
                p.value_start
                    .zip(p.value_end)
                    .map(|(vs, ve)| &source[vs as usize..ve as usize])
            } else {
                None
            }
        });
        match is_expr {
            Some(expr) => {
                let mut buf = String::with_capacity(expr.len() + 30);
                buf.push_str("_resolveDynamicComponent(");
                buf.push_str(expr);
                buf.push(')');
                buf
            }
            None => resolve_component(tag_name, resolver, out),
        }
    } else {
        resolve_component(tag_name, resolver, out)
    };

    let mut line = String::with_capacity(256);
    line.push_str("  const n");
    push_u32(&mut line, id.0 as u32);
    line.push_str(" = _createComponent(");
    line.push_str(&comp_ref);

    // Props object
    let has_props = element.props.iter().any(|p| {
        if !p.is_directive {
            return true;
        }
        let name = &source[p.start as usize..p.name_end as usize];
        let arg = match (p.arg_start, p.arg_end) {
            (Some(s), Some(e)) => Some(&source[s as usize..e as usize]),
            _ => None,
        };
        if is_dynamic_component && arg == Some("is") {
            return false;
        }
        name.starts_with(':') || name == "v-bind" || name == "v-model"
    });

    if has_props {
        line.push_str(", { ");
        let mut first = true;
        for prop in &element.props {
            if prop.is_directive {
                let name = &source[prop.start as usize..prop.name_end as usize];
                if let Some(arg) = name.strip_prefix(':') {
                    if is_dynamic_component && arg == "is" {
                        continue;
                    }
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        if !first {
                            line.push_str(", ");
                        }
                        line.push_str(arg);
                        line.push_str(": () => (");
                        line.push_str(&source[vs as usize..ve as usize]);
                        line.push(')');
                        first = false;
                    }
                }
                continue;
            }
            let name = &source[prop.start as usize..prop.name_end as usize];
            if !first {
                line.push_str(", ");
            }
            line.push_str(name);
            line.push_str(": ");
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                line.push('"');
                line.push_str(&source[vs as usize..ve as usize]);
                line.push('"');
            } else {
                line.push_str("true");
            }
            first = false;
        }
        line.push_str(" }");
    } else {
        line.push_str(", null");
    }

    // Slots object
    if let Some(slots) = slots_str {
        line.push_str(", ");
        line.push_str(slots);
    }

    line.push(')');

    body_lines.push(out.alloc_str(&line));
    out.add_vapor_import(VaporHelper::CreateComponent);
}

/// Generate slot outlet code.
///
/// Output: `const n{id} = _createSlot("name", { props })`
pub fn process_slot_outlet<'alloc>(
    id: NodeId,
    element: &ElementNode,
    source: &'alloc str,
    body_lines: &mut Vec<&'alloc str>,
    out: &mut CodeGenOutput<'alloc>,
) {
    // Determine slot name from `name` attribute or default to "default"
    let mut slot_name = "default";
    for prop in &element.props {
        if !prop.is_directive {
            let name = &source[prop.start as usize..prop.name_end as usize];
            if name == "name" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    slot_name = &source[vs as usize..ve as usize];
                }
            }
        }
    }

    let mut line = String::with_capacity(64);
    line.push_str("  const n");
    push_u32(&mut line, id.0 as u32);
    line.push_str(" = _createSlot(\"");
    line.push_str(slot_name);
    line.push_str("\")");

    body_lines.push(out.alloc_str(&line));
    out.add_vapor_import(VaporHelper::CreateSlot);
}

/// Generate slot outlet code with fallback content.
///
/// Output: `const n{id} = _createSlot("name", null, () => { /* fallback */ })`
pub fn process_slot_outlet_with_fallback<'alloc>(
    id: NodeId,
    element: &ElementNode,
    source: &'alloc str,
    fallback_str: Option<&str>,
    body_lines: &mut Vec<&'alloc str>,
    out: &mut CodeGenOutput<'alloc>,
) {
    let mut slot_name = "default";
    for prop in &element.props {
        if !prop.is_directive {
            let name = &source[prop.start as usize..prop.name_end as usize];
            if name == "name" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    slot_name = &source[vs as usize..ve as usize];
                }
            }
        }
    }

    let mut line = String::with_capacity(128);
    line.push_str("  const n");
    push_u32(&mut line, id.0 as u32);
    line.push_str(" = _createSlot(\"");
    line.push_str(slot_name);
    line.push('"');

    if let Some(fallback) = fallback_str {
        // null for props, then the fallback closure
        line.push_str(", null, ");
        line.push_str(fallback);
    }

    line.push(')');

    body_lines.push(out.alloc_str(&line));
    out.add_vapor_import(VaporHelper::CreateSlot);
}

/// Convert a kebab-case or camelCase string to PascalCase.
fn to_pascal_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;
    for ch in s.chars() {
        if ch == '-' || ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_case_simple() {
        assert_eq!(to_pascal_case("my-component"), "MyComponent");
    }

    #[test]
    fn pascal_case_already_pascal() {
        assert_eq!(to_pascal_case("MyComponent"), "MyComponent");
    }

    #[test]
    fn pascal_case_single_word() {
        assert_eq!(to_pascal_case("foo"), "Foo");
    }
}
