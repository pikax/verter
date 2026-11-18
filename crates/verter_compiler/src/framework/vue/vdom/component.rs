//! Component-related VDOM code generation helpers.
//!
//! Extracted from `element.rs`: VNode helper selection, component tag resolution,
//! dynamic component handling, and PascalCase conversion.

use crate::ast::types::ElementNode;
use crate::framework::vue::semantic::{
    VueBindingView, VueComponentClass, VueComponentTagFacts, NO_COMPONENT_TAG,
};
use crate::template::binding::prefixed_expr::find_prop_oxc_exp;
use crate::template::oxc::types::OxcParsedElement;

use super::state::{VdomOut, VdomState};
use crate::template::binding::prefixed_expr::resolve_expr;
use crate::template::code_gen::shared::helpers::{is_builtin_component, VdomHelper};
use crate::types::NodeId;

/// A hoisted runtime resolution and the snapshot fact its declaration needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedComponent {
    pub tag: String,
    pub variable: String,
    pub self_reference: bool,
}

impl<'c> VdomState<'c> {
    /// What the snapshot says the component tag of parser node `id` refers to.
    pub(super) fn component_tag(&self, id: NodeId) -> &'c VueComponentTagFacts {
        let inputs = self.inputs;
        match inputs.topology.node_for_syntax(id) {
            Some(node) => inputs.semantic.component_tag(node),
            None => &NO_COMPONENT_TAG,
        }
    }
}

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

/// Resolve a component tag to a setup binding reference, a built-in import
/// or `_resolveComponent()`, as the snapshot classified it:
///
/// - `Binding`: the matched binding's asset reference plus the member tail
///   (`Swiper.Item` -> `$setup["Swiper"].Item`).
/// - `BuiltIn`: the built-in's runtime import (flag and name only).
/// - `SelfReference`: `_resolveComponent("Name", true)`, so the runtime
///   falls back to `instance.type.__name`.
/// - `Resolved` (and a `<component>` whose `is` resolved to nothing):
///   `_resolveComponent("Name")`.
///
/// When `hoisted_resolves` is provided, the `_resolveComponent()` call is hoisted
/// to a `const _component_x` declaration and the variable name is returned.
pub(super) fn resolve_component_tag(
    tag_name: &str,
    facts: &VueComponentTagFacts,
    resolver: &VueBindingView<'_>,
    out: &mut VdomOut<'_>,
    hoisted_resolves: Option<&mut Vec<ResolvedComponent>>,
) -> String {
    match facts.class {
        VueComponentClass::Binding => {
            if let Some(binding) = &facts.binding {
                let mut s = resolver.resolve_asset_ref(&binding.name);
                s.push_str(&binding.member_tail);
                return s;
            }
        }
        // Built-ins import from the runtime module instead of resolving by name.
        VueComponentClass::BuiltIn => {
            if let Some((flag, helper_name)) = facts
                .built_in
                .and_then(|built_in| is_builtin_component(built_in.export_name()))
            {
                out.add_builtin_component(flag);
                return helper_name.to_string();
            }
        }
        VueComponentClass::SelfReference
        | VueComponentClass::Resolved
        | VueComponentClass::Dynamic
        | VueComponentClass::None => {}
    }

    // A recursive self-reference (the tag names the component's own file).
    // Vue emits `_resolveComponent("Name", true)` so the runtime checks
    // `instance.type.__name` as a fallback for unresolved components.
    let is_self_ref = facts.class == VueComponentClass::SelfReference;

    // Fallback: _resolveComponent("Name")
    out.add_vdom_import(VdomHelper::ResolveComponent);

    // Hoist to const variable if hoisted_resolves is provided
    if let Some(hoisted) = hoisted_resolves {
        // Generate variable name: replace hyphens and dots with underscores
        // to produce a valid JS identifier (Vue uses toValidAssetId with char codes,
        // but underscores are simpler and equally valid).
        let var_name = format!("_component_{}", tag_name.replace(['-', '.'], "_"));

        // Check if already hoisted
        if !hoisted.iter().any(|reference| reference.tag == tag_name) {
            hoisted.push(ResolvedComponent {
                tag: tag_name.to_string(),
                variable: var_name.clone(),
                self_reference: is_self_ref,
            });
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

/// True when `el` is a `<component>` whose `is` binding resolves through
/// `_resolveDynamicComponent` — a `v-bind`-family `:is` (with or without a
/// value; the value-less form is the Vue 3.4 same-name shorthand) or a static
/// `is="value"` attribute. Mirrors [`resolve_dynamic_component`]'s detection
/// exactly so blockification and resolution never disagree.
///
/// Official Vue ALWAYS treats a dynamic component as a block root
/// (`(_openBlock(), _createBlock(_resolveDynamicComponent(...)))`) at any
/// position — including multi-root fragment children that are not themselves
/// v-if/v-for branches (the reka-ui CheckboxRoot pattern): the resolved
/// target can be any shape (element/component/Fragment), so the runtime
/// needs a block boundary for dynamicChildren tracking.
pub(super) fn is_dynamic_component_tag(el: &ElementNode, source: &str) -> bool {
    let tag_name = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
    if tag_name != "component" {
        return false;
    }
    el.props.iter().any(|prop| {
        if prop.is_directive {
            let directive_name = &source[prop.start as usize..prop.name_end as usize];
            super::is_v_bind(directive_name)
                && matches!(
                    (prop.arg_start, prop.arg_end),
                    (Some(arg_start), Some(arg_end))
                        if &source[arg_start as usize..arg_end as usize] == "is"
                )
        } else {
            &source[prop.start as usize..prop.name_end as usize] == "is"
                && prop.value_start.is_some()
                && prop.value_end.is_some()
        }
    })
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
    resolver: &VueBindingView<'a>,
    out: &mut VdomOut<'a>,
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
        let is_bind = super::is_v_bind(directive_name);
        if !is_bind {
            continue;
        }
        if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
            let arg_name = &source[as_ as usize..ae as usize];
            if arg_name != "is" {
                continue;
            }
            // Found `:is` — resolve its bound expression.
            let resolved_expr = if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                // `:is="expr"` — resolve the explicit value expression.
                let value = &source[vs as usize..ve as usize];
                let oxc_exp = find_prop_oxc_exp(oxc_el, i);
                resolve_expr(value, vs, oxc_exp, resolver, force_js)
            } else {
                // Value-less `:is` — Vue 3.4 same-name shorthand: `:is` == `:is="is"`.
                // The bound expression is the arg name itself (`is`), resolved as a
                // bare identifier through the binding resolver ($setup./ $props. / _ctx.).
                resolve_expr(arg_name, as_, None, resolver, force_js)
            };

            out.add_vdom_import(VdomHelper::ResolveDynamicComponent);
            let mut s = String::with_capacity(resolved_expr.len() + 30);
            s.push_str("_resolveDynamicComponent(");
            s.push_str(&resolved_expr);
            s.push(')');
            return Some((s, i));
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
