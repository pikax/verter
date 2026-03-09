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
///
/// When `is_block_root` is true, the element is at a block-tree root position
/// (template root, v-if/v-else-if/v-else branch root, or v-for item root).
/// Block roots use `_createElementBlock` / `_createBlock` instead of
/// `_createElementVNode` / `_createVNode` so the Vue runtime can optimize
/// patch diffing via the block tree.
pub(super) fn vnode_helper(element: &ElementNode, is_block_root: bool) -> VdomHelper {
    if element.tag_type.is_component() {
        if is_block_root {
            VdomHelper::CreateBlock
        } else {
            VdomHelper::CreateVNode
        }
    } else if is_block_root {
        VdomHelper::CreateElementBlock
    } else {
        VdomHelper::CreateElementVNode
    }
}

/// Resolve a component tag name to a setup binding reference or `_resolveComponent()`.
///
/// Checks the resolver for:
/// 1. Exact match (e.g., `Header` -> `$setup["Header"]`)
/// 2. Dot-notation namespace (e.g., `Swiper.Item` → `$setup["Swiper"].Item`)
/// 3. PascalCase conversion (e.g., `my-header` -> `$setup["MyHeader"]`)
/// 4. Fallback: `_resolveComponent("TagName")`, with `maybeSelfReference=true`
///    when the PascalCase tag matches `self_name` (recursive self-reference).
///
/// When `hoisted_resolves` is provided, the `_resolveComponent()` call is hoisted
/// to a `const _component_x` declaration and the variable name is returned.
pub(super) fn resolve_component_tag(
    tag_name: &str,
    resolver: &BindingResolver<'_>,
    out: &mut CodeGenOutput<'_>,
    self_name: &str,
    hoisted_resolves: Option<&mut Vec<(String, String)>>,
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

    // Check dot-notation namespace (e.g., Swiper.Item → resolve "Swiper" + ".Item").
    // Vue splits at the first dot, resolves the prefix from setup bindings,
    // and appends the rest as property access.
    if let Some(dot_pos) = tag_name.find('.') {
        let ns = &tag_name[..dot_pos];
        let member_access = &tag_name[dot_pos..]; // includes the leading dot

        if resolver.get(ns).is_some() {
            let prefix = resolver.resolve_prefix(ns);
            let suffix = resolver.resolve_suffix(ns);
            let mut s =
                String::with_capacity(ns.len() + prefix.len() + suffix.len() + member_access.len());
            s.push_str(prefix);
            s.push_str(ns);
            s.push_str(suffix);
            s.push_str(member_access);
            return s;
        }

        // Also try PascalCase on the namespace prefix
        let pascal_ns = to_pascal_case(ns);
        if resolver.get(&pascal_ns).is_some() {
            let prefix = resolver.resolve_prefix(&pascal_ns);
            let suffix = resolver.resolve_suffix(&pascal_ns);
            let mut s = String::with_capacity(
                pascal_ns.len() + prefix.len() + suffix.len() + member_access.len(),
            );
            s.push_str(prefix);
            s.push_str(&pascal_ns);
            s.push_str(suffix);
            s.push_str(member_access);
            return s;
        }
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

    // Hoist to const variable if hoisted_resolves is provided
    if let Some(hoisted) = hoisted_resolves {
        // Generate variable name: replace hyphens and dots with underscores
        // to produce a valid JS identifier (Vue uses toValidAssetId with char codes,
        // but underscores are simpler and equally valid).
        let var_name = format!("_component_{}", tag_name.replace(['-', '.'], "_"));

        // Check if already hoisted
        if !hoisted.iter().any(|(t, _)| t == tag_name) {
            hoisted.push((tag_name.to_string(), var_name.clone()));
        }
        return var_name;
    }

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
    super::super::shared::helpers::to_pascal_case(s)
}
