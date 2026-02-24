//! Component-related VDOM code generation helpers.
//!
//! Extracted from `element.rs`: VNode helper selection, component tag resolution,
//! dynamic component handling, and PascalCase conversion.

use crate::ast::types::ElementNode;
use crate::template::code_gen::binding::BindingResolver;
use crate::template::code_gen::vapor::find_prop_oxc_exp;
use crate::template::oxc::types::OxcParsedElement;

use super::super::shared::helpers::{is_builtin_component, VdomHelper};
use super::super::types::CodeGenOutput;
use super::element::resolve_expr;

/// Determine the helper for creating this element.
pub(super) fn vnode_helper(element: &ElementNode) -> VdomHelper {
    if element.tag_type.is_component() {
        VdomHelper::CreateVNode
    } else {
        VdomHelper::CreateElementVNode
    }
}

/// Resolve a component tag name to a setup binding reference or `_resolveComponent()`.
///
/// Checks the resolver for:
/// 1. Exact match (e.g., `Header` -> `$setup["Header"]`)
/// 2. PascalCase conversion (e.g., `my-header` -> `$setup["MyHeader"]`)
/// 3. Fallback: `_resolveComponent("TagName")`, with `maybeSelfReference=true`
///    when the PascalCase tag matches `self_name` (recursive self-reference).
pub(super) fn resolve_component_tag(
    tag_name: &str,
    resolver: &BindingResolver<'_>,
    out: &mut CodeGenOutput<'_>,
    self_name: &str,
) -> String {
    // Check exact binding
    if resolver.get(tag_name).is_some() {
        let prefix = resolver.resolve_prefix(tag_name);
        let suffix = resolver.resolve_suffix(tag_name);
        let mut s = String::with_capacity(tag_name.len() + prefix.len() + suffix.len());
        s.push_str(prefix);
        s.push_str(tag_name);
        s.push_str(suffix);
        return s;
    }

    // Check PascalCase conversion (for kebab-case tags like <my-header>)
    let pascal = to_pascal_case(tag_name);
    if resolver.get(&pascal).is_some() {
        let prefix = resolver.resolve_prefix(&pascal);
        let suffix = resolver.resolve_suffix(&pascal);
        let mut s = String::with_capacity(pascal.len() + prefix.len() + suffix.len());
        s.push_str(prefix);
        s.push_str(&pascal);
        s.push_str(suffix);
        return s;
    }

    // Check for Vue built-in components (Suspense, Teleport, KeepAlive, etc.).
    // These are imported directly from "vue" instead of using _resolveComponent().
    // Check both original tag name and PascalCase form (for kebab-case like <keep-alive>).
    if let Some((flag, helper_name)) =
        is_builtin_component(tag_name).or_else(|| is_builtin_component(&pascal))
    {
        out.add_builtin_component(flag);
        return helper_name.to_string();
    }

    // Check if this is a recursive self-reference (tag PascalCase matches self_name).
    // Vue emits `_resolveComponent("Name", true)` so the runtime checks
    // `instance.type.__name` as a fallback for unresolved components.
    let is_self_ref = !self_name.is_empty() && pascal == self_name;

    // Fallback: _resolveComponent("Name")
    out.add_vdom_import(VdomHelper::ResolveComponent);
    let mut s = String::with_capacity(tag_name.len() + 32);
    s.push_str("_resolveComponent(\"");
    s.push_str(tag_name);
    if is_self_ref {
        s.push_str("\", true)");
    } else {
        s.push_str("\")");
    }
    s
}

/// Resolve `<component :is="expr">` as `_resolveDynamicComponent(expr)`.
///
/// Returns `Some((resolved_tag, is_prop_index))` if the element is a
/// `<component>` with a `:is` / `v-bind:is` prop, where `is_prop_index`
/// is the prop's index so the caller can skip it in the props object.
pub(super) fn resolve_dynamic_component<'a>(
    el: &ElementNode,
    source: &str,
    oxc_el: Option<&OxcParsedElement<'a>>,
    resolver: &BindingResolver<'a>,
    out: &mut CodeGenOutput<'a>,
    force_js: bool,
) -> Option<(String, usize)> {
    let tag_name = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
    if tag_name != "component" {
        return None;
    }

    // Find the :is directive prop
    for (i, prop) in el.props.iter().enumerate() {
        if !prop.is_directive {
            continue;
        }
        let directive_name = &source[prop.start as usize..prop.name_end as usize];
        let is_bind = directive_name == ":" || directive_name == "v-bind";
        if !is_bind {
            continue;
        }
        if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
            let arg_name = &source[as_ as usize..ae as usize];
            if arg_name != "is" {
                continue;
            }
            // Found :is="expr" -- resolve the expression
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value = &source[vs as usize..ve as usize];
                let oxc_exp = find_prop_oxc_exp(oxc_el, i);
                let resolved_expr = resolve_expr(value, vs, oxc_exp, resolver, force_js);

                out.add_vdom_import(VdomHelper::ResolveDynamicComponent);
                let mut s = String::with_capacity(resolved_expr.len() + 30);
                s.push_str("_resolveDynamicComponent(");
                s.push_str(&resolved_expr);
                s.push(')');
                return Some((s, i));
            }
        }
    }

    // Fallback: check for static is="value" attribute (without colon binding).
    // Vue 3 treats <component is="div"> as _resolveDynamicComponent("div").
    for (i, prop) in el.props.iter().enumerate() {
        if prop.is_directive {
            continue;
        }
        let name = &source[prop.start as usize..prop.name_end as usize];
        if name != "is" {
            continue;
        }
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value = &source[vs as usize..ve as usize];
            out.add_vdom_import(VdomHelper::ResolveDynamicComponent);
            let mut s = String::with_capacity(value.len() + 32);
            s.push_str("_resolveDynamicComponent(\"");
            s.push_str(value);
            s.push_str("\")");
            return Some((s, i));
        }
    }

    None
}

/// Convert a kebab-case or camelCase string to PascalCase.
pub(crate) fn to_pascal_case(s: &str) -> String {
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
