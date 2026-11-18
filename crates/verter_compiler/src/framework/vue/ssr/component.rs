//! Components: dynamic, built-in and resolved; custom directives on them.

use std::fmt::Write;

use crate::ast::types::ElementNode;
use crate::framework::vue::semantic::VueComponentClass;
use crate::framework::vue::structure::VueWalkControl;
use crate::template::code_gen::shared::helpers::{SsrHelper, VdomHelper};
use crate::template::oxc::types::{OxcParsedElement, OxcParsedExpression};
use crate::template::vue_syntax::BuiltinComponent;
use crate::template::vue_syntax::{camelize, format_event_handler_key_into, needs_quoted_key};
use crate::template::vue_syntax::{
    is_member_expression, is_multi_statement_handler, trim_handler_body,
};
use crate::types::NodeProp;
use oxc_ast::ast::Expression;

use super::escape::{css_to_js_object, directive_to_camel, escape_js_string, html_attr_to_js_key};
use super::props_object::PropsObject;
use super::state::{ElemCtx, SlotArray, SsrEmitter, SsrOut, Visit};

impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    pub(super) fn register_component_resolve(
        &mut self,
        tag: &str,
        variable: &str,
        self_reference: bool,
        out: &mut SsrOut<'c>,
    ) {
        out.add_vdom_import(VdomHelper::ResolveComponent);
        let tail = if self_reference { ", true" } else { "" };
        let declaration = format!("const {variable} = _resolveComponent(\"{tag}\"{tail})");
        if !self.component_resolves.contains(&declaration) {
            self.component_resolves.push(declaration);
        }
    }

    /// Resolve a global (non-setup) directive: emit `_resolveDirective("name")` declaration
    /// and return the variable name `_directive_name`.
    pub(super) fn resolve_directive_global(
        &mut self,
        directive_name: &str,
        out: &mut SsrOut<'c>,
    ) -> String {
        let var_name = format!("_directive_{}", directive_name.replace('-', "_"));
        let decl = format!(
            "const {} = _resolveDirective(\"{}\")",
            var_name, directive_name
        );
        if !self.directive_resolves.contains(&decl) {
            self.directive_resolves.push(decl);
            out.add_vdom_import(VdomHelper::ResolveDirective);
        }
        var_name
    }

    /// Build a `_ssrGetDirectiveProps(_ctx, ref, value?, arg?, modifiers?)` call
    /// for a custom directive prop on a component. Returns the call string and
    /// registers the SSR helper import + directive resolution.
    pub(super) fn build_directive_props_call(
        &mut self,
        prop: &NodeProp,
        prop_name: &str,
        source: &str,
        oxc: Option<&OxcParsedElement<'c>>,
        i: usize,
        out: &mut SsrOut<'c>,
    ) -> Option<String> {
        let directive_name = prop_name.strip_prefix("v-")?;
        let binding_name = directive_to_camel(directive_name);

        // Resolve directive reference: setup binding or _resolveDirective().
        // Non-inline SSR reaches setup directives through the instance proxy
        // (`_ctx.vFocus`); free `$setup[...]` would ReferenceError in ssrRender.
        let directive_ref = if let Some(bt) = self.bindings.get(&binding_name) {
            if bt.is_setup() {
                let prefix = self.bindings.resolve_prefix(&binding_name);
                if prefix == "$setup." {
                    format!("$setup[\"{}\"]", binding_name)
                } else if prefix.is_empty() {
                    binding_name
                } else {
                    // `_ctx.` / other proxy prefixes: keep valid identifier form
                    format!("{}{}", prefix, binding_name)
                }
            } else {
                self.resolve_directive_global(directive_name, out)
            }
        } else {
            self.resolve_directive_global(directive_name, out)
        };

        let mut dir_call = format!("_ssrGetDirectiveProps(_ctx, {}", directive_ref);

        // Value (optional)
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let expr = &source[vs as usize..ve as usize];
            let oxc_prop = oxc.and_then(|o| o.prop(i));
            let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
            let resolved = self.resolve_expr(expr, vs, oxc_expr);
            dir_call.push_str(", ");
            dir_call.push_str(&resolved);
        }

        // Argument (optional)
        if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
            if prop.value_start.is_none() {
                dir_call.push_str(", void 0");
            }
            if prop.is_dynamic == Some(true) {
                let raw_arg = &source[as_ as usize..ae as usize];
                let arg = raw_arg
                    .strip_prefix('[')
                    .and_then(|s| s.strip_suffix(']'))
                    .unwrap_or(raw_arg);
                let arg_offset = if raw_arg.starts_with('[') {
                    as_ + 1
                } else {
                    as_
                };
                let oxc_prop = oxc.and_then(|o| o.prop(i));
                let oxc_expr = oxc_prop.and_then(|p| p.arg.as_ref());
                let resolved = self.resolve_expr(arg, arg_offset, oxc_expr);
                dir_call.push_str(", ");
                dir_call.push_str(&resolved);
            } else {
                let arg = &source[as_ as usize..ae as usize];
                dir_call.push_str(", \"");
                dir_call.push_str(arg);
                dir_call.push('"');
            }
        }

        // Modifiers (optional)
        if !prop.modifiers.is_empty() {
            if prop.arg_start.is_none() {
                if prop.value_start.is_none() {
                    dir_call.push_str(", void 0");
                }
                dir_call.push_str(", void 0");
            }
            dir_call.push_str(", { ");
            for (j, modifier) in prop.modifiers.iter().enumerate() {
                let mod_name = &source[modifier.start as usize..modifier.end as usize];
                if j > 0 {
                    dir_call.push_str(", ");
                }
                dir_call.push_str(mod_name);
                dir_call.push_str(": true");
            }
            dir_call.push_str(" }");
        }

        dir_call.push(')');
        out.add_ssr_import(SsrHelper::GetDirectiveProps);
        Some(dir_call)
    }

    /// Detect `<component :is="expr">` and return the resolved expression + prop index.
    pub(super) fn resolve_dynamic_is(
        &self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'c>>,
        source: &str,
    ) -> Option<(String, usize)> {
        // Find :is or v-bind:is directive
        for (i, prop) in el.props.iter().enumerate() {
            if !prop.is_directive {
                // Check for static is="value"
                let name = &source[prop.start as usize..prop.name_end as usize];
                if name == "is" {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let value = &source[vs as usize..ve as usize];
                        return Some((format!("_resolveDynamicComponent(\"{}\")", value), i));
                    }
                }
                continue;
            }
            let directive_name = &source[prop.start as usize..prop.name_end as usize];
            if !(directive_name.starts_with(':') || directive_name.starts_with("v-bind")) {
                continue;
            }
            if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                let arg_name = &source[as_ as usize..ae as usize];
                if arg_name != "is" {
                    continue;
                }
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let expr = &source[vs as usize..ve as usize];
                    let oxc_prop = oxc.and_then(|o| o.prop(i));
                    let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                    let resolved = self.resolve_expr(expr, vs, oxc_expr);
                    return Some((format!("_resolveDynamicComponent({})", resolved), i));
                }
            }
        }
        None
    }

    pub(super) fn enter_component(
        &mut self,
        v: &Visit<'c>,
        out: &mut SsrOut<'c>,
    ) -> VueWalkControl {
        let (tid, el, oxc, is_root) = (v.tid, v.el, v.oxc, v.is_root);
        let tag_name = v.tag_name;
        let source = self.source;
        let component_tag = self.inputs.semantic.component_tag(tid);
        // ── 2a. Dynamic component: <component :is="expr"> ───────
        if component_tag.class == VueComponentClass::Dynamic {
            if let Some((dynamic_expr, is_prop_idx)) = self.resolve_dynamic_is(el, oxc, source) {
                self.close_push(el.tag_open.start, out);

                out.add_ssr_import(SsrHelper::RenderVNode);
                out.add_vdom_import(VdomHelper::CreateVNode);
                out.add_vdom_import(VdomHelper::ResolveDynamicComponent);

                // Build props excluding the :is prop
                // Pre-scan for class merge: static `class` + `:class` → merged array
                let mut dyn_static_class: Option<String> = None;
                let mut dyn_static_class_idx: Option<usize> = None;
                let mut dyn_dynamic_class: Option<String> = None;
                let mut dyn_dynamic_class_idx: Option<usize> = None;
                for (i, prop) in el.props.iter().enumerate() {
                    if i == is_prop_idx {
                        continue;
                    }
                    let pn = &source[prop.start as usize..prop.name_end as usize];
                    if !prop.is_directive && pn == "class" {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            dyn_static_class = Some(source[vs as usize..ve as usize].to_string());
                            dyn_static_class_idx = Some(i);
                        }
                    } else if prop.is_directive && (pn.starts_with(':') || pn.starts_with("v-bind"))
                    {
                        if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                            if &source[as_ as usize..ae as usize] == "class" {
                                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                    let expr = &source[vs as usize..ve as usize];
                                    let oxc_expr =
                                        oxc.and_then(|o| o.prop(i)).and_then(|p| p.exp.as_ref());
                                    dyn_dynamic_class = Some(self.resolve_expr(expr, vs, oxc_expr));
                                    dyn_dynamic_class_idx = Some(i);
                                }
                            }
                        }
                    }
                }
                let dyn_merge_class = dyn_static_class.is_some() && dyn_dynamic_class.is_some();

                let mut props_parts: Vec<String> = Vec::new();
                let mut dyn_directive_calls: Vec<String> = Vec::new();
                let mut has_v_bind_spread = false;
                let mut v_bind_spread_expr = String::new();
                for (i, prop) in el.props.iter().enumerate() {
                    if i == is_prop_idx {
                        continue;
                    }
                    // Handle class merge
                    if dyn_merge_class
                        && (Some(i) == dyn_static_class_idx || Some(i) == dyn_dynamic_class_idx)
                    {
                        if Some(i) == dyn_static_class_idx {
                            props_parts.push(format!(
                                "class: [\"{}\", {}]",
                                escape_js_string(dyn_static_class.as_ref().unwrap()),
                                dyn_dynamic_class.as_ref().unwrap()
                            ));
                        }
                        continue;
                    }
                    let prop_name = &source[prop.start as usize..prop.name_end as usize];
                    if prop.is_directive {
                        // Event handlers on dynamic component
                        if prop_name.starts_with('@') || prop_name == "v-on" {
                            let event_name = if let Some(after_at) = prop_name.strip_prefix('@') {
                                if after_at.is_empty() {
                                    match (prop.arg_start, prop.arg_end) {
                                        (Some(s), Some(e)) => &source[s as usize..e as usize],
                                        _ => {
                                            continue;
                                        }
                                    }
                                } else {
                                    after_at
                                }
                            } else {
                                match (prop.arg_start, prop.arg_end) {
                                    (Some(s), Some(e)) => &source[s as usize..e as usize],
                                    _ => {
                                        continue;
                                    }
                                }
                            };
                            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                let expr = &source[vs as usize..ve as usize];
                                let oxc_prop = oxc.and_then(|o| o.prop(i));
                                let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                                let resolved = self.handler_value(expr, vs, oxc_expr);
                                let mut js_key = String::with_capacity(event_name.len() + 2);
                                format_event_handler_key_into(&mut js_key, event_name);
                                if needs_quoted_key(&js_key) {
                                    props_parts.push(format!("\"{}\": {}", js_key, resolved));
                                } else {
                                    props_parts.push(format!("{}: {}", js_key, resolved));
                                }
                            }
                            continue;
                        }
                        // v-bind spread (no argument): v-bind="obj"
                        if (prop_name == "v-bind" || prop_name == ":") && prop.arg_start.is_none() {
                            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                let expr = &source[vs as usize..ve as usize];
                                let oxc_prop = oxc.and_then(|o| o.prop(i));
                                let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                                let resolved = self.resolve_expr(expr, vs, oxc_expr);
                                has_v_bind_spread = true;
                                v_bind_spread_expr = resolved;
                            }
                            continue;
                        }
                        // v-model on dynamic component
                        if prop_name.starts_with("v-model") {
                            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                let expr = &source[vs as usize..ve as usize];
                                let oxc_prop = oxc.and_then(|o| o.prop(i));
                                let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                                let resolved = self.resolve_expr(expr, vs, oxc_expr);
                                // Determine model prop name (v-model:title -> "title")
                                let model_prop =
                                    if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                                        source[as_ as usize..ae as usize].to_string()
                                    } else {
                                        "modelValue".to_string()
                                    };
                                props_parts.push(format!("{}: {}", model_prop, resolved));
                                props_parts.push(format!(
                                    "\"onUpdate:{}\": $event => (({}) = $event)",
                                    model_prop, resolved
                                ));
                            }
                            continue;
                        }
                        if prop_name.starts_with(':') || prop_name == "v-bind" {
                            if let (Some(as_), Some(ae), Some(vs), Some(ve)) = (
                                prop.arg_start,
                                prop.arg_end,
                                prop.value_start,
                                prop.value_end,
                            ) {
                                let attr = &source[as_ as usize..ae as usize];
                                // :key is passed through on components (unlike HTML elements)
                                let expr = &source[vs as usize..ve as usize];
                                let oxc_prop = oxc.and_then(|o| o.prop(i));
                                let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                                let resolved = self.resolve_expr(expr, vs, oxc_expr);
                                props_parts.push(format!(
                                    "{}: {}",
                                    html_attr_to_js_key(attr),
                                    resolved
                                ));
                            }
                            continue;
                        }
                        // Custom directives on dynamic components.
                        // Exclude built-in directives handled elsewhere or
                        // that are structural (removed from el.props by the
                        // parser).
                        if prop_name.starts_with("v-")
                            && !matches!(
                                prop_name,
                                "v-bind"
                                    | "v-on"
                                    | "v-model"
                                    | "v-show"
                                    | "v-html"
                                    | "v-text"
                                    | "v-memo"
                                    | "v-cloak"
                            )
                        {
                            if let Some(call) = self
                                .build_directive_props_call(prop, prop_name, source, oxc, i, out)
                            {
                                dyn_directive_calls.push(call);
                            }
                        }
                    } else if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let value = &source[vs as usize..ve as usize];
                        props_parts.push(format!(
                            "{}: \"{}\"",
                            html_attr_to_js_key(prop_name),
                            escape_js_string(value)
                        ));
                    }
                }

                // For root-level dynamic components, merge _attrs into props
                let dir_suffix = if dyn_directive_calls.is_empty() {
                    String::new()
                } else {
                    format!(", {}", dyn_directive_calls.join(", "))
                };
                let props_str = if has_v_bind_spread {
                    // v-bind spread: merge spread expr with any other props + _attrs
                    out.add_vdom_import(VdomHelper::MergeProps);
                    if is_root {
                        if props_parts.is_empty() {
                            format!("_mergeProps({}, _attrs{})", v_bind_spread_expr, dir_suffix)
                        } else {
                            format!(
                                "_mergeProps({{ {} }}, {}, _attrs{})",
                                props_parts.join(", "),
                                v_bind_spread_expr,
                                dir_suffix
                            )
                        }
                    } else if props_parts.is_empty() && dyn_directive_calls.is_empty() {
                        v_bind_spread_expr
                    } else {
                        out.add_vdom_import(VdomHelper::MergeProps);
                        if props_parts.is_empty() {
                            format!("_mergeProps({}{})", v_bind_spread_expr, dir_suffix)
                        } else {
                            format!(
                                "_mergeProps({{ {} }}, {}{})",
                                props_parts.join(", "),
                                v_bind_spread_expr,
                                dir_suffix
                            )
                        }
                    }
                } else if !dyn_directive_calls.is_empty() {
                    out.add_vdom_import(VdomHelper::MergeProps);
                    let dir_args = dyn_directive_calls.join(", ");
                    if is_root {
                        if props_parts.is_empty() {
                            format!("_mergeProps(_attrs, {})", dir_args)
                        } else {
                            format!(
                                "_mergeProps({{ {} }}, _attrs, {})",
                                props_parts.join(", "),
                                dir_args
                            )
                        }
                    } else if props_parts.is_empty() {
                        dir_args
                    } else {
                        format!(
                            "_mergeProps({{ {} }}, {})",
                            props_parts.join(", "),
                            dir_args
                        )
                    }
                } else if is_root {
                    if props_parts.is_empty() {
                        "_attrs".to_string()
                    } else {
                        out.add_vdom_import(VdomHelper::MergeProps);
                        format!("_mergeProps({{ {} }}, _attrs)", props_parts.join(", "))
                    }
                } else if props_parts.is_empty() {
                    "null".to_string()
                } else {
                    format!("{{ {} }}", props_parts.join(", "))
                };

                let has_children = self.has_effective_children(tid);

                if !has_children {
                    let sid_arg = self.scope_id_arg();
                    self.buf.clear();
                    let _ = writeln!(
                        self.buf,
                        "_ssrRenderVNode(_push, _createVNode({}, {}, null), _parent{})",
                        dynamic_expr, props_str, sid_arg
                    );
                    let el_end = self.el_end(el);
                    out.overwrite_segmented(el.tag_open.start, el_end, &self.buf, &[]);
                    self.elem_ctx.push(ElemCtx::Complete);
                } else {
                    // Has children — render as slot content like regular components
                    out.add_vdom_import(VdomHelper::WithCtx);
                    let has_named_slots = self.has_template_slot_children(tid);
                    let slot_array = has_named_slots && self.has_conditional_slot_templates(tid);
                    if slot_array {
                        out.add_vdom_import(VdomHelper::CreateSlots);
                    }

                    self.saved_default_slot_open.push(self.default_slot_open);
                    self.saved_default_slot_move.push((
                        self.default_slot_move_start.take(),
                        self.default_slot_move_end.take(),
                        self.default_slot_close.take(),
                    ));

                    self.buf.clear();
                    let _ = write!(
                        self.buf,
                        "_ssrRenderVNode(_push, _createVNode({}, {}, {}{{",
                        dynamic_expr,
                        props_str,
                        if slot_array { "_createSlots(" } else { "" }
                    );

                    if !has_named_slots {
                        let params = if let Some(ref v_slot) = el.v_slot {
                            if let (Some(vs), Some(ve)) = (v_slot.value_start, v_slot.value_end) {
                                let p = source[vs as usize..ve as usize].trim();
                                if p.is_empty() {
                                    "_"
                                } else {
                                    p
                                }
                            } else {
                                "_"
                            }
                        } else {
                            "_"
                        };
                        let _ = write!(
                                self.buf,
                                "\ndefault: _withCtx(({}, _push, _parent, _scopeId) => {{\nif (_push) {{\n",
                                params
                            );
                        self.default_slot_open = true;
                        // Track scoped slot depth for child component DYNAMIC flag
                        let is_scoped = params != "_";
                        if is_scoped {
                            self.v_slot_scope_depth += 1;
                        }
                        self.scoped_slot_entered.push(is_scoped);
                    } else {
                        self.default_slot_open = false;
                        self.comp_children_start = Some(el.tag_open.end);
                        self.scoped_slot_entered.push(false);
                    }

                    out.overwrite_segmented(el.tag_open.start, el.tag_open.end, &self.buf, &[]);
                    self.in_push = false;
                    self.in_component_slots += 1;
                    self.slot_arrays.push(slot_array.then(SlotArray::default));
                    self.elem_ctx.push(ElemCtx::DynamicComponentWithSlots);
                }
                return VueWalkControl::Continue;
            }
        }

        // ── 2b. Built-in component: <Suspense> ───────────────────
        if component_tag.class == VueComponentClass::BuiltIn {
            if component_tag.built_in == Some(BuiltinComponent::Suspense) {
                self.close_push(el.tag_open.start, out);
                out.add_ssr_import(SsrHelper::RenderSuspense);

                let has_children = self.has_effective_children(tid);
                if !has_children {
                    self.buf.clear();
                    let _ = writeln!(self.buf, "_ssrRenderSuspense(_push, {{}})");
                    let el_end = self.el_end(el);
                    out.overwrite_segmented(el.tag_open.start, el_end, &self.buf, &[]);
                    self.elem_ctx.push(ElemCtx::Complete);
                } else {
                    let has_named_slots = self.has_template_slot_children(tid);
                    let has_bare = self.has_bare_children(tid);
                    self.buf.clear();
                    if has_named_slots && has_bare {
                        // Mixed content: bare children become implicit default slot,
                        // named <template v-slot> children are separate entries.
                        // `default: () => {` opened here; closed when first
                        // <template v-slot> is encountered.
                        self.buf
                            .push_str("_ssrRenderSuspense(_push, {\ndefault: () => {\n");
                    } else if has_named_slots {
                        // Pure named slots: just open the slots object; each <template v-slot>
                        // will emit its own `slotName: () => { ... }` entry.
                        self.buf.push_str("_ssrRenderSuspense(_push, {\n");
                    } else {
                        // Implicit default slot
                        self.buf
                            .push_str("_ssrRenderSuspense(_push, {\ndefault: () => {\n");
                    }
                    out.overwrite_segmented(el.tag_open.start, el.tag_open.end, &self.buf, &[]);
                    self.in_push = false;
                    self.in_component_slots += 1;
                    self.suspense_implicit_default_open = has_named_slots && has_bare;
                    self.elem_ctx.push(ElemCtx::SuspenseSlots);
                }
                return VueWalkControl::Continue;
            }

            // TransitionGroup — renders its `tag` prop as a real HTML element
            if component_tag.built_in == Some(BuiltinComponent::TransitionGroup) {
                // Extract tag prop (default: "span")
                let mut tg_tag = "span".to_string();
                let mut tg_attrs: Vec<String> = Vec::new();
                for prop in el.props.iter() {
                    let prop_name = &source[prop.start as usize..prop.name_end as usize];
                    if !prop.is_directive {
                        if prop_name == "tag" {
                            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                tg_tag = source[vs as usize..ve as usize].to_string();
                            }
                        } else if prop_name == "appear"
                            || prop_name == "css"
                            || prop_name == "duration"
                            || prop_name == "mode"
                            || prop_name == "moveClass"
                            || prop_name == "move-class"
                            || prop_name == "enterFromClass"
                            || prop_name == "enter-from-class"
                            || prop_name == "enterActiveClass"
                            || prop_name == "enter-active-class"
                            || prop_name == "enterToClass"
                            || prop_name == "enter-to-class"
                            || prop_name == "leaveFromClass"
                            || prop_name == "leave-from-class"
                            || prop_name == "leaveActiveClass"
                            || prop_name == "leave-active-class"
                            || prop_name == "leaveToClass"
                            || prop_name == "leave-to-class"
                        {
                            // TransitionGroup transition-only props — skip in HTML
                        } else if prop_name == "name" {
                            // Official @vue/compiler-ssr forwards `name` onto the
                            // host tag (e.g. `<ul name="list">`).
                            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                let value = &source[vs as usize..ve as usize];
                                tg_attrs.push(format!(" name=\"{}\"", escape_js_string(value)));
                            }
                        } else if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let value = &source[vs as usize..ve as usize];
                            tg_attrs.push(format!(
                                " {}=\"{}\"",
                                prop_name,
                                escape_js_string(value)
                            ));
                        }
                    }
                    // Dynamic props are skipped for now (rarely used on TransitionGroup)
                }

                // Emit opening tag: `<ul class="list">`
                self.buf.clear();
                let _ = write!(self.buf, "<{}", tg_tag);
                for attr in &tg_attrs {
                    self.buf.push_str(attr);
                }
                self.buf.push('>');

                // Insert into push stream
                if self.in_push {
                    out.overwrite_segmented(el.tag_open.start, el.tag_open.end, &self.buf, &[]);
                } else {
                    out.overwrite_fmt(
                        el.tag_open.start,
                        el.tag_open.end,
                        format_args!("_push(`{}`)\n", self.buf),
                    );
                }
                self.elem_ctx.push(ElemCtx::TransitionGroupTag(tg_tag));
                return VueWalkControl::Continue;
            }

            // Transition / KeepAlive / BaseTransition — transparent in SSR
            if matches!(
                component_tag.built_in,
                Some(
                    BuiltinComponent::Transition
                        | BuiltinComponent::KeepAlive
                        | BuiltinComponent::BaseTransition
                )
            ) {
                // Remove the opening tag — children render directly.
                out.overwrite_segmented(el.tag_open.start, el.tag_open.end, "", &[]);
                self.elem_ctx.push(ElemCtx::TransparentBuiltin);
                return VueWalkControl::Continue;
            }

            // Teleport — uses _ssrRenderTeleport helper
            if component_tag.built_in == Some(BuiltinComponent::Teleport) {
                self.close_push(el.tag_open.start, out);
                out.add_ssr_import(SsrHelper::RenderTeleport);

                // Extract `to` and `disabled` props
                let mut target = "\"body\"".to_string();
                let mut disabled = "false".to_string();
                let oxc = self.oxc_element(tid);
                for (i, prop) in el.props.iter().enumerate() {
                    let prop_name = &source[prop.start as usize..prop.name_end as usize];

                    if prop.is_directive && (prop_name == ":" || prop_name == "v-bind") {
                        // Dynamic binding: :to or v-bind:to
                        if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                            let arg = &source[as_ as usize..ae as usize];
                            match arg {
                                "to" => {
                                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end)
                                    {
                                        let expr = &source[vs as usize..ve as usize];
                                        let oxc_expr = oxc
                                            .and_then(|o| o.prop(i))
                                            .and_then(|p| p.exp.as_ref());
                                        target = self.resolve_expr(expr, vs, oxc_expr);
                                    }
                                }
                                "disabled" => {
                                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end)
                                    {
                                        let expr = &source[vs as usize..ve as usize];
                                        let oxc_expr = oxc
                                            .and_then(|o| o.prop(i))
                                            .and_then(|p| p.exp.as_ref());
                                        disabled = self.resolve_expr(expr, vs, oxc_expr);
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else if !prop.is_directive {
                        // Static attribute: to="body" or disabled
                        match prop_name {
                            "to" => {
                                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                    let value = &source[vs as usize..ve as usize];
                                    target = format!("\"{}\"", value);
                                }
                            }
                            "disabled" => {
                                disabled = "true".to_string();
                            }
                            _ => {}
                        }
                    }
                }

                // Store closing args for leave_element
                self.teleport_closing_args
                    .push(format!(", {}, {}, _parent)", target, disabled));

                self.buf.clear();
                self.buf
                    .push_str("_ssrRenderTeleport(_push, (_push) => {\n");
                out.overwrite_segmented(el.tag_open.start, el.tag_open.end, &self.buf, &[]);
                self.in_push = false;
                self.in_component_slots += 1;
                self.elem_ctx.push(ElemCtx::TeleportBody);
                return VueWalkControl::Continue;
            }
        }

        // ── 2c. Normal component ─────────────────────────────────
        self.close_push(el.tag_open.start, out);

        out.add_ssr_import(SsrHelper::RenderComponent);

        let component_ref = match component_tag.class {
            VueComponentClass::Binding => {
                let binding = component_tag
                    .binding
                    .as_ref()
                    .expect("snapshot binding match");
                let mut reference = self.bindings.resolve_asset_ref(&binding.name);
                reference.push_str(&binding.member_tail);
                reference
            }
            VueComponentClass::SelfReference
            | VueComponentClass::Resolved
            | VueComponentClass::Dynamic
            | VueComponentClass::None => {
                let variable = format!("_component_{}", tag_name.replace(['-', '.'], "_"));
                self.register_component_resolve(
                    tag_name,
                    &variable,
                    component_tag.class == VueComponentClass::SelfReference,
                    out,
                );
                variable
            }
            VueComponentClass::BuiltIn => unreachable!("built-in emission handled above"),
        };

        // Build component props
        // Pre-scan for class merge: static `class` + `:class` → merged array
        let mut comp_static_class: Option<String> = None;
        let mut comp_static_class_idx: Option<usize> = None;
        let mut comp_dynamic_class: Option<String> = None;
        let mut comp_dynamic_class_idx: Option<usize> = None;
        for (i, prop) in el.props.iter().enumerate() {
            let prop_name = &source[prop.start as usize..prop.name_end as usize];
            if !prop.is_directive && prop_name == "class" {
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    comp_static_class = Some(source[vs as usize..ve as usize].to_string());
                    comp_static_class_idx = Some(i);
                }
            } else if prop.is_directive
                && (prop_name.starts_with(':') || prop_name.starts_with("v-bind"))
            {
                if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                    let arg_name = &source[as_ as usize..ae as usize];
                    if arg_name == "class" {
                        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                            let expr = &source[vs as usize..ve as usize];
                            let oxc_expr = oxc.and_then(|o| o.prop(i)).and_then(|p| p.exp.as_ref());
                            comp_dynamic_class = Some(self.resolve_expr(expr, vs, oxc_expr));
                            comp_dynamic_class_idx = Some(i);
                        }
                    }
                }
            }
        }
        let comp_merge_class = comp_static_class.is_some() && comp_dynamic_class.is_some();

        let mut props_parts = PropsObject::new();
        let mut comp_directive_calls: Vec<String> = Vec::new();
        let mut comp_v_bind_spread: Option<String> = None;
        // Track props collected before the v-bind spread for position-aware merging.
        // Vue splits props into groups around v-bind spreads:
        //   <Comp :a="1" v-bind="obj" :b="2" /> → _mergeProps({a: 1}, obj, {b: 2})
        let mut props_before_spread: Option<PropsObject> = None;
        for (i, prop) in el.props.iter().enumerate() {
            // Handle class merge: skip individual class entries, emit merged on first
            if comp_merge_class
                && (Some(i) == comp_static_class_idx || Some(i) == comp_dynamic_class_idx)
            {
                if Some(i) == comp_static_class_idx {
                    props_parts.push_at(
                        "class",
                        format!(
                            "[\"{}\", {}]",
                            escape_js_string(comp_static_class.as_ref().unwrap()),
                            comp_dynamic_class.as_ref().unwrap()
                        ),
                        prop.start,
                    );
                }
                continue;
            }

            let prop_name = &source[prop.start as usize..prop.name_end as usize];
            if prop.is_directive {
                // Event handlers: @click → onClick, v-on:input → onInput
                if prop_name.starts_with('@') || prop_name == "v-on" {
                    let event_name = if let Some(after_at) = prop_name.strip_prefix('@') {
                        if after_at.is_empty() {
                            // @ shorthand with arg in arg_start/arg_end
                            match (prop.arg_start, prop.arg_end) {
                                (Some(s), Some(e)) => &source[s as usize..e as usize],
                                _ => {
                                    continue;
                                }
                            }
                        } else {
                            after_at
                        }
                    } else {
                        // v-on with arg
                        match (prop.arg_start, prop.arg_end) {
                            (Some(s), Some(e)) => &source[s as usize..e as usize],
                            _ => {
                                continue;
                            }
                        }
                    };
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let expr = &source[vs as usize..ve as usize];
                        let oxc_prop = oxc.and_then(|o| o.prop(i));
                        let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                        let value = self.handler_value(expr, vs, oxc_expr);

                        let mut js_key = String::with_capacity(event_name.len() + 2);
                        format_event_handler_key_into(&mut js_key, event_name);
                        if needs_quoted_key(&js_key) {
                            js_key = format!("\"{}\"", js_key);
                        }
                        props_parts.push_at(js_key, value, prop.start);
                    }
                    continue;
                }
                // v-model on component: decompose into modelValue + onUpdate:modelValue
                if prop_name.starts_with("v-model") {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let expr = &source[vs as usize..ve as usize];
                        let oxc_prop = oxc.and_then(|o| o.prop(i));
                        let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                        let resolved = self.resolve_expr(expr, vs, oxc_expr);

                        // Determine model prop name: v-model:title → "title", v-model → "modelValue"
                        let model_prop =
                            if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                                source[as_ as usize..ae as usize].to_string()
                            } else {
                                "modelValue".to_string()
                            };

                        // Emit value prop: modelValue: <expr>
                        // Quote kebab-case model prop names (e.g., "page-size")
                        let model_key = if model_prop.contains('-') {
                            format!("\"{}\"", model_prop)
                        } else {
                            model_prop.clone()
                        };
                        props_parts.push_at(model_key, resolved.clone(), prop.start);

                        // Emit update handler: "onUpdate:modelValue": $event => ((<expr>) = $event)
                        // Vue camelCases kebab-case model names in the handler key
                        let update_name = if model_prop.contains('-') {
                            camelize(&model_prop).into_owned()
                        } else {
                            model_prop.clone()
                        };
                        props_parts.push_at(
                            format!("\"onUpdate:{}\"", update_name),
                            format!("$event => (({}) = $event)", resolved),
                            prop.start,
                        );

                        // Emit modifiers if any: modelModifiers: { trim: true, ... }
                        if !prop.modifiers.is_empty() {
                            let mod_key = if model_prop == "modelValue" {
                                "modelModifiers".to_string()
                            } else {
                                format!("{}Modifiers", model_prop)
                            };
                            let mods: Vec<String> = prop
                                .modifiers
                                .iter()
                                .map(|m| {
                                    let name = &source[m.start as usize..m.end as usize];
                                    format!("{}: true", name)
                                })
                                .collect();
                            props_parts.push_at(
                                mod_key,
                                format!("{{ {} }}", mods.join(", ")),
                                prop.start,
                            );
                        }
                    }
                    continue;
                }
                if prop_name.starts_with(':') || (prop_name == "v-bind" && prop.arg_start.is_some())
                {
                    if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                        let attr = &source[as_ as usize..ae as usize];
                        // :key is passed through on components (unlike HTML elements).
                        // Vue 3.4+ same-name shorthand: `:cards` (no value) ≡
                        // `:cards="cards"`. Camelize for binding lookup so
                        // `:heading-value` resolves to `headingValue`.
                        let resolved =
                            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                let expr = &source[vs as usize..ve as usize];
                                let oxc_prop = oxc.and_then(|o| o.prop(i));
                                let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                                self.resolve_expr(expr, vs, oxc_expr)
                            } else {
                                let camelized = camelize(attr);
                                self.bindings.resolve_simple_expr(&camelized)
                            };
                        props_parts.push_at(html_attr_to_js_key(attr), resolved, prop.start);
                    }
                    continue;
                }
                // v-bind spread (no argument): v-bind="obj" — merge into props
                if (prop_name == "v-bind" || prop_name == ":") && prop.arg_start.is_none() {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        let expr = &source[vs as usize..ve as usize];
                        let oxc_prop = oxc.and_then(|o| o.prop(i));
                        let oxc_expr = oxc_prop.and_then(|p| p.exp.as_ref());
                        let resolved = self.resolve_expr(expr, vs, oxc_expr);
                        comp_v_bind_spread = Some(resolved);
                        // Snapshot props collected so far as "before spread" group.
                        // Props added after this point go into the "after spread" group.
                        props_before_spread = Some(props_parts.take());
                    }
                    continue;
                }
                // Custom directives on components: v-foo, v-tooltip, etc.
                // Exclude built-in directives that are handled elsewhere or
                // are structural (removed from el.props by the parser).
                if prop_name.starts_with("v-")
                    && !matches!(
                        prop_name,
                        "v-bind" | "v-on" | "v-model" | "v-show" | "v-html" | "v-text"
                    )
                {
                    if let Some(call) =
                        self.build_directive_props_call(prop, prop_name, source, oxc, i, out)
                    {
                        comp_directive_calls.push(call);
                    }
                }
            } else if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value = &source[vs as usize..ve as usize];
                if prop_name == "style" {
                    // Vue SSR converts static style to JS object for component props
                    props_parts.push_at("style", css_to_js_object(value), prop.start);
                } else {
                    props_parts.push_at(
                        html_attr_to_js_key(prop_name),
                        format!("\"{}\"", escape_js_string(value)),
                        prop.start,
                    );
                }
            } else {
                // Boolean attribute with no value: <Comp rounded /> → rounded: ""
                props_parts.push_at(html_attr_to_js_key(prop_name), "\"\"", prop.start);
            }
        }

        // Add ref to component props in source order (cached in el.v_ref, removed from el.props)
        // Note: unlike HTML elements, components DO get ref in SSR props
        if let Some(ref v_ref) = el.v_ref {
            if let (Some(vs), Some(ve)) = (v_ref.value_start, v_ref.value_end) {
                let ref_val = &source[vs as usize..ve as usize];
                let ref_value = if v_ref.is_directive {
                    self.resolve_expr(ref_val, vs, None)
                } else {
                    format!("\"{}\"", escape_js_string(ref_val))
                };
                props_parts.insert_by_source_order("ref", ref_value, v_ref.start);
            }
        }

        // Merge duplicate keys into arrays. When v-model and an explicit
        // @update:model-value handler coexist, Vue passes both handlers as
        // "onUpdate:modelValue": [handler1, handler2].
        props_parts.merge_duplicate_keys();

        // Also merge in the before-spread group if applicable
        if let Some(ref mut before) = props_before_spread {
            before.merge_duplicate_keys();
        }

        // Build the props expression, handling combinations of:
        // - static props (props_parts)
        // - v-bind spread (comp_v_bind_spread)
        // - custom directives (comp_directive_calls)
        // - root element _attrs merge
        let has_spread = comp_v_bind_spread.is_some();
        let has_directives = !comp_directive_calls.is_empty();

        let props_expr = if has_spread || has_directives {
            // Multiple sources to merge — use _mergeProps.
            // Vue preserves the position of v-bind spreads relative to other props:
            //   <Comp :a="1" v-bind="obj" :b="2" /> → _mergeProps({a:1}, obj, {b:2})
            // The `props_before_spread` snapshot contains props before the spread,
            // `props_parts` contains props after the spread.
            let mut merge_args: Vec<String> = Vec::new();
            if let Some(ref before) = props_before_spread {
                if !before.is_empty() {
                    merge_args.push(format!("{{ {} }}", before.render_body()));
                }
            }
            if let Some(spread) = &comp_v_bind_spread {
                merge_args.push(spread.clone());
            }
            // Props after the spread (or all props if no spread was present)
            if !props_parts.is_empty() {
                merge_args.push(format!("{{ {} }}", props_parts.render_body()));
            }
            if is_root {
                merge_args.push("_attrs".to_string());
            }
            for call in &comp_directive_calls {
                merge_args.push(call.clone());
            }
            // If only a single source (spread-only, no root, no directives),
            // use it directly without _mergeProps wrapping
            if merge_args.len() == 1 && !is_root && !has_directives {
                merge_args.into_iter().next().unwrap()
            } else {
                out.add_vdom_import(VdomHelper::MergeProps);
                format!("_mergeProps({})", merge_args.join(", "))
            }
        } else if is_root {
            // Root component: merge static props with _attrs
            if props_parts.is_empty() {
                "_attrs".to_string()
            } else {
                out.add_vdom_import(VdomHelper::MergeProps);
                format!("_mergeProps({{ {} }}, _attrs)", props_parts.render_body())
            }
        } else if props_parts.is_empty() {
            "null".to_string()
        } else {
            format!("{{ {} }}", props_parts.render_body())
        };

        // CSS v-bind custom properties reach ROOT-LEVEL components as a
        // merged prop (official `ssrInjectCssVars` injects on every
        // root-level node, components included — without this the
        // emitted `const _cssVars` is dead and the custom properties
        // never reach the HTML).
        let props_expr = if self.is_css_vars_root() && self.has_css_vars() {
            if props_expr == "null" {
                "_cssVars".to_string()
            } else {
                out.add_vdom_import(VdomHelper::MergeProps);
                format!("_mergeProps({}, _cssVars)", props_expr)
            }
        } else {
            props_expr
        };

        let has_children = self.has_effective_children(tid);

        if !has_children {
            self.buf.clear();
            let sid_arg = self.scope_id_arg();
            let _ = writeln!(
                self.buf,
                "_push(_ssrRenderComponent({}, {}, null, _parent{}))",
                component_ref, props_expr, sid_arg
            );
            let el_end = self.el_end(el);
            out.overwrite_segmented(el.tag_open.start, el_end, &self.buf, &[]);
            self.elem_ctx.push(ElemCtx::Complete);
        } else {
            // Has children — emit slot wrappers via _withCtx.
            out.add_vdom_import(VdomHelper::WithCtx);
            let has_named_slots = self.has_template_slot_children(tid);
            let slot_array = has_named_slots && self.has_conditional_slot_templates(tid);
            if slot_array {
                out.add_vdom_import(VdomHelper::CreateSlots);
            }

            // Save outer default_slot_open state before entering this component's
            // slot scope. Nested components must start with a fresh state.
            self.saved_default_slot_open.push(self.default_slot_open);
            self.saved_default_slot_move.push((
                self.default_slot_move_start.take(),
                self.default_slot_move_end.take(),
                self.default_slot_close.take(),
            ));

            self.buf.clear();
            let _ = write!(
                self.buf,
                "_push(_ssrRenderComponent({}, {}, {}{{",
                component_ref,
                props_expr,
                if slot_array { "_createSlots(" } else { "" }
            );

            if !has_named_slots {
                // Extract v-slot params from the component itself (e.g., <Comp v-slot="{ item }">)
                let params = if let Some(ref v_slot) = el.v_slot {
                    if let (Some(vs), Some(ve)) = (v_slot.value_start, v_slot.value_end) {
                        let p = source[vs as usize..ve as usize].trim();
                        if p.is_empty() {
                            "_"
                        } else {
                            p
                        }
                    } else {
                        "_"
                    }
                } else {
                    "_"
                };
                let _ = write!(
                    self.buf,
                    "\ndefault: _withCtx(({}, _push, _parent, _scopeId) => {{\nif (_push) {{\n",
                    params
                );
                // Mark default slot as already opened so the implicit
                // default-slot check doesn't open a second one.
                self.default_slot_open = true;
                // Track scoped slot depth for child component DYNAMIC flag
                let is_scoped = params != "_";
                if is_scoped {
                    self.v_slot_scope_depth += 1;
                }
                self.scoped_slot_entered.push(is_scoped);
            } else {
                // Named slots present — reset for nested scope; children will
                // open the default slot implicitly when needed.
                self.default_slot_open = false;
                self.comp_children_start = Some(el.tag_open.end);
                self.scoped_slot_entered.push(false);
            }

            out.overwrite_segmented(el.tag_open.start, el.tag_open.end, &self.buf, &[]);
            self.in_push = false;
            self.in_component_slots += 1;
            self.slot_arrays.push(slot_array.then(SlotArray::default));
            self.elem_ctx.push(ElemCtx::ComponentWithSlots);
        }
        VueWalkControl::Continue
    }

    pub(super) fn leave_transparent_builtin(&mut self, v: &Visit<'c>, out: &mut SsrOut<'c>) {
        let el = v.el;
        // Remove closing tag — children already rendered directly.
        if let Some(ref tc) = el.tag_close {
            out.overwrite_segmented(tc.start, tc.end, "", &[]);
        }
    }

    pub(super) fn leave_transition_group_tag(
        &mut self,
        v: &Visit<'c>,
        tg_tag: &str,
        out: &mut SsrOut<'c>,
    ) {
        let el = v.el;
        // Emit closing tag for TransitionGroup: `</ul>`
        if let Some(ref tc) = el.tag_close {
            if self.in_push {
                out.overwrite_fmt(tc.start, tc.end, format_args!("</{}>", tg_tag));
            } else {
                out.overwrite_fmt(tc.start, tc.end, format_args!("_push(`</{}>`)\n", tg_tag));
            }
        }
    }

    pub(super) fn leave_teleport_body(&mut self, v: &Visit<'c>, out: &mut SsrOut<'c>) {
        let el = v.el;
        // Close the _ssrRenderTeleport callback and append target/disabled args.
        self.in_component_slots -= 1;
        let close_pos = el
            .tag_close
            .as_ref()
            .map(|tc| tc.start)
            .unwrap_or(self.el_end(el));
        self.close_push(close_pos, out);

        let closing_args = self
            .teleport_closing_args
            .pop()
            .unwrap_or_else(|| ", \"body\", false, _parent)".to_string());

        self.buf.clear();
        let _ = writeln!(self.buf, "}}{}", closing_args);

        if let Some(ref tc) = el.tag_close {
            out.overwrite_segmented(tc.start, tc.end, &self.buf, &[]);
        } else {
            let el_end = self.el_end(el);
            out.prepend_alloc(el_end, &self.buf);
        }
    }
}

impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    /// An event handler's value, shaped by its parse facts: a function
    /// expression as written, a statement list in a block, a member path as
    /// a reference, anything else (a call, an assignment) deferred as
    /// `$event => (…)`.
    fn handler_value(
        &self,
        expr: &str,
        offset: u32,
        oxc_expr: Option<&OxcParsedExpression<'c>>,
    ) -> String {
        let resolved = self.resolve_expr(expr, offset, oxc_expr);
        let is_function = oxc_expr
            .and_then(|oxc| oxc.expression.as_ref())
            .is_some_and(|expression| {
                matches!(
                    expression,
                    Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_)
                )
            });
        if is_multi_statement_handler(oxc_expr) {
            format!("$event => {{{resolved}}}")
        } else if is_function || is_member_expression(expr) {
            resolved
        } else {
            format!("$event => ({})", trim_handler_body(&resolved))
        }
    }
}
