//! Slot-related code generation for the VDOM backend.
//!
//! This module handles all slot processing: `<slot>` outlets (`_renderSlot`),
//! `<template v-slot:name>` bodies (`_withCtx`), component slot wrappers
//! (static `{ name: fn }` and dynamic `_createSlots()`), and implicit
//! default slots.

use rustc_hash::FxHashMap;

use crate::ast::types::{AstNodeKind, ElementNode, TagType};
use crate::template::oxc::types::{ExpressionFlag, OxcParsedElement};
use crate::types::NodeId;

use super::super::shared::helpers::{self, VdomHelper};
use super::super::types::{ChildKind, ChildRecord, CodeGenOutput, ConditionChainRole};
use super::{children, component, directives, element, props, VdomCodeGen};

/// Check if a string is a valid JS identifier (can be used as a bare property name).
fn is_valid_js_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

/// Format a slot name as a JS property key. Quotes names with hyphens etc.
fn format_slot_key(buf: &mut String, name: &str) {
    if is_valid_js_ident(name) {
        buf.push_str(name);
    } else {
        buf.push('"');
        helpers::escape_js_string_into(buf, name);
        buf.push('"');
    }
}

/// The resolved slot name for a `<slot>` outlet.
pub(super) enum SlotName {
    /// Static name: `name="header"` → `"header"`
    Static(String),
    /// Dynamic name: `:name="expr"` → resolved expression
    Dynamic(String),
}

/// A slot entry is either a named template slot (single child) or a default
/// slot (group of consecutive non-template children).
pub(super) enum SlotEntry {
    /// A named template slot at child index `i`.
    Named(usize),
    /// Default slot: consecutive non-template children at indices `[start, end)`.
    Default { start: usize, end: usize },
}

impl<'ast, 'alloc> VdomCodeGen<'ast, 'alloc> {
    /// Extract the slot name from a `<slot>` element's `name` attribute.
    /// Returns `SlotName::Static("default")` if no `name` prop is found.
    /// Handles both static `name="xxx"` and dynamic `:name="expr"`.
    pub(super) fn extract_slot_name_ex(
        &self,
        element: &ElementNode,
        oxc_el: Option<&OxcParsedElement<'alloc>>,
        source: &str,
    ) -> SlotName {
        for (prop_idx, prop) in element.props.iter().enumerate() {
            if !prop.is_directive {
                let name = &source[prop.start as usize..prop.name_end as usize];
                if name == "name" {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        return SlotName::Static(source[vs as usize..ve as usize].to_string());
                    }
                }
            } else {
                let dname = &source[prop.start as usize..prop.name_end as usize];
                if super::is_v_bind(dname) {
                    if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                        let arg = &source[as_ as usize..ae as usize];
                        if arg == "name" {
                            // Dynamic :name="expr"
                            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                                let raw = &source[vs as usize..ve as usize];
                                let oxc_exp =
                                    super::super::vapor::find_prop_oxc_exp(oxc_el, prop_idx);
                                let resolved = element::resolve_expr(
                                    raw,
                                    vs,
                                    oxc_exp,
                                    &self.resolver,
                                    self.options.force_js,
                                );
                                return SlotName::Dynamic(resolved);
                            } else {
                                // Same-name shorthand: `:name` → use `name` as expression
                                let resolved = self.resolver.resolve_simple_expr("name");
                                return SlotName::Dynamic(resolved);
                            }
                        }
                    }
                }
            }
        }
        SlotName::Static("default".to_string())
    }

    /// Extract the slot name from a `v-slot` directive on a `<template>` element.
    /// Handles `v-slot:name`, `#name`, and bare `v-slot` / `#default`.
    pub(super) fn extract_v_slot_name<'s>(
        &self,
        element: &ElementNode,
        source: &'s str,
    ) -> &'s str {
        if let Some(ref v_slot) = element.v_slot {
            if let (Some(as_), Some(ae)) = (v_slot.arg_start, v_slot.arg_end) {
                return &source[as_ as usize..ae as usize];
            }
        }
        "default"
    }

    /// Process a `<slot>` outlet element, generating `_renderSlot(_ctx.$slots, "name")`.
    /// When the slot has fallback children, generates
    /// `_renderSlot(_ctx.$slots, "name", {}, () => [children])`.
    ///
    /// Supports:
    /// - Static `name="xxx"` and dynamic `:name="expr"` slot names
    /// - Slot outlet props (`:prop="expr"`, shorthand `:prop`)
    pub(super) fn process_slot_outlet(
        &mut self,
        el: &ElementNode,
        oxc_el: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> ChildRecord {
        let slot_name = self.extract_slot_name_ex(el, oxc_el, source);
        out.add_vdom_import(VdomHelper::RenderSlot);

        let tag_end = el
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(el.tag_open.end);

        let el_children = el
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[]);
        let mut children = self.build_child_records(el_children, source);
        // Pass false: tag extension + gap-filling below cover all removed regions,
        // so emitting removal overwrites here would create overlapping ranges.
        element::resolve_whitespace(&mut children, out, false);
        element::strip_interstitial_condition_nodes(&mut children, out, false);

        // Build slot outlet props (named + bare `v-bind` spreads).
        let slot_props = self.build_slot_outlet_props(el, oxc_el, source);
        if slot_props
            .as_deref()
            .is_some_and(|s| s.contains("_mergeProps("))
        {
            out.add_vdom_import(VdomHelper::MergeProps);
        }

        let mut buf = std::mem::take(&mut self.buf);
        buf.clear();

        // Build the _renderSlot call prefix
        buf.push_str("_renderSlot(_ctx.$slots, ");
        match &slot_name {
            SlotName::Static(name) => {
                buf.push('"');
                helpers::escape_js_string_into(&mut buf, name);
                buf.push('"');
            }
            SlotName::Dynamic(expr) => {
                buf.push_str(expr);
            }
        }

        if children.is_empty() && slot_props.is_none() {
            // No fallback, no props: _renderSlot(_ctx.$slots, "name")
            buf.push(')');
            out.overwrite(el.tag_open.start, tag_end, &buf);
        } else if children.is_empty() {
            // Props but no fallback: _renderSlot(_ctx.$slots, "name", propsExpr)
            buf.push_str(", ");
            buf.push_str(&slot_props.unwrap());
            buf.push(')');
            out.overwrite(el.tag_open.start, tag_end, &buf);
        } else {
            // Has fallback: split into open/close overwrites so children
            // remain in place with their own overwrites.
            // Open: _renderSlot(_ctx.$slots, "name", propsExpr, () => [
            buf.push_str(", ");
            buf.push_str(slot_props.as_deref().unwrap_or("{}"));
            buf.push_str(", () => [");
            let open_end = children[0].start;
            out.overwrite(el.tag_open.start, open_end, &buf);

            // Remove gaps between children
            for i in 1..children.len() {
                let prev_end = children[i - 1].end;
                let next_start = children[i].start;
                if next_start > prev_end {
                    out.overwrite(prev_end, next_start, "");
                }
            }

            // Add child separators
            children::add_children_separators_array(
                &children,
                out,
                &self.options,
                source,
                self.ast,
                el_children,
            );

            // Close: ])
            buf.clear();
            buf.push_str("])");
            let close_start = children.last().unwrap().end;
            out.overwrite(close_start, tag_end, &buf);
        }

        buf.clear();
        self.buf = buf;

        ChildRecord {
            start: el.tag_open.start,
            end: tag_end,
            kind: ChildKind::Element,
            condition: None,
            condition_prefix: None,
        }
    }

    /// Build the slot outlet props expression for `_renderSlot(..., props)`.
    ///
    /// Collects:
    /// - `:prop="expr"` / shorthand `:prop` → object literal members
    /// - bare `v-bind="expr"` spreads (reka-ui AlertDialogRoot:
    ///   `<slot v-bind="slotProps" />` → third arg is the resolved expr)
    ///
    /// When both spreads and named props exist, wraps with `_mergeProps` so
    /// named keys win over the spread (official Vue order).
    /// Returns `None` if no slot props are present.
    fn build_slot_outlet_props(
        &self,
        el: &ElementNode,
        oxc_el: Option<&OxcParsedElement<'alloc>>,
        source: &str,
    ) -> Option<String> {
        let mut named_buf = String::new();
        let mut named_count = 0;
        let mut spreads: Vec<String> = Vec::new();

        for (prop_idx, prop) in el.props.iter().enumerate() {
            if !prop.is_directive {
                // Skip static `name` attribute
                let name = &source[prop.start as usize..prop.name_end as usize];
                if name == "name" {
                    continue;
                }
                // Other static attributes become slot props
                if named_count > 0 {
                    named_buf.push_str(", ");
                }
                if super::props::needs_quoted_key(name) {
                    named_buf.push('"');
                    helpers::escape_js_string_into(&mut named_buf, name);
                    named_buf.push('"');
                } else {
                    named_buf.push_str(name);
                }
                named_buf.push_str(": ");
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    named_buf.push('"');
                    helpers::escape_js_string_into(
                        &mut named_buf,
                        &source[vs as usize..ve as usize],
                    );
                    named_buf.push('"');
                } else {
                    named_buf.push_str("\"\"");
                }
                named_count += 1;
                continue;
            }

            let dname = &source[prop.start as usize..prop.name_end as usize];
            if !super::is_v_bind(dname) {
                continue;
            }

            if let (Some(as_), Some(ae)) = (prop.arg_start, prop.arg_end) {
                let arg = &source[as_ as usize..ae as usize];
                // Skip :name (used for dynamic slot name, not props)
                if arg == "name" {
                    continue;
                }

                if named_count > 0 {
                    named_buf.push_str(", ");
                }

                // Emit key
                let key = super::props::camelize(arg);
                if super::props::needs_quoted_key(&key) {
                    named_buf.push('"');
                    helpers::escape_js_string_into(&mut named_buf, &key);
                    named_buf.push('"');
                } else {
                    named_buf.push_str(&key);
                }
                named_buf.push_str(": ");

                // Emit value
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let raw = &source[vs as usize..ve as usize];
                    let oxc_exp = super::super::vapor::find_prop_oxc_exp(oxc_el, prop_idx);
                    let resolved = element::resolve_expr(
                        raw,
                        vs,
                        oxc_exp,
                        &self.resolver,
                        self.options.force_js,
                    );
                    named_buf.push_str(&resolved);
                } else {
                    // Same-name shorthand: `:item` → `item: resolvedBinding`
                    let resolved = self.resolver.resolve_simple_expr(&key);
                    named_buf.push_str(&resolved);
                }
                named_count += 1;
            } else if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                // Bare `v-bind="expr"` spread — pass the object through as
                // the slot props (or merge with named props below).
                let raw = &source[vs as usize..ve as usize];
                let oxc_exp = super::super::vapor::find_prop_oxc_exp(oxc_el, prop_idx);
                let resolved =
                    element::resolve_expr(raw, vs, oxc_exp, &self.resolver, self.options.force_js);
                spreads.push(resolved);
            }
        }

        if spreads.is_empty() && named_count == 0 {
            return None;
        }

        if spreads.is_empty() {
            return Some(format!("{{ {} }}", named_buf));
        }

        if named_count == 0 && spreads.len() == 1 {
            // Sole spread: `_renderSlot(..., slotProps)`
            return Some(spreads.into_iter().next().unwrap());
        }

        // Multiple spreads and/or named props → `_mergeProps(...)`.
        // Named object last so explicit keys override the spread.
        let mut out = String::from("_mergeProps(");
        for (i, s) in spreads.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(s);
        }
        if named_count > 0 {
            if !spreads.is_empty() {
                out.push_str(", ");
            }
            out.push_str("{ ");
            out.push_str(&named_buf);
            out.push_str(" }");
        }
        out.push(')');
        Some(out)
    }

    /// Process a `<template v-slot:name>` element within a component.
    /// Generates the slot function body: `_withCtx(() => [children])`.
    ///
    /// The slot name prefix (e.g. `header: ` or `{ name: "header", fn: `)
    /// is NOT emitted here --- it is added by `leave_component_with_slots`
    /// which decides between static and dynamic (`_createSlots`) format
    /// based on whether any sibling slots have `v-if`.
    pub(super) fn process_template_slot(
        &mut self,
        el: &ElementNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> ChildRecord {
        out.add_vdom_import(VdomHelper::WithCtx);

        let el_children = el
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[]);
        let mut children = self.build_child_records(el_children, source);
        // Pass false: tag extension + gap-filling below cover all removed regions,
        // so emitting removal overwrites here would create overlapping ranges.
        element::resolve_whitespace(&mut children, out, false);
        element::strip_interstitial_condition_nodes(&mut children, out, false);

        let has_children = !children.is_empty();
        let mut buf = std::mem::take(&mut self.buf);
        buf.clear();

        // Build the _withCtx open: `_withCtx((...params) => [`
        // Slot name prefix is added by parent (`leave_component_with_slots`).
        // If v_slot has a value (scoped slot params), inject them as arrow function params.
        buf.push_str("_withCtx(");
        if let Some(v_slot) = &el.v_slot {
            if let (Some(vs), Some(ve)) = (v_slot.value_start, v_slot.value_end) {
                let params = &source[vs as usize..ve as usize];
                if !params.trim().is_empty() {
                    buf.push('(');
                    buf.push_str(params);
                    buf.push(')');
                } else {
                    buf.push_str("()");
                }
            } else {
                buf.push_str("()");
            }
        } else {
            buf.push_str("()");
        }
        buf.push_str(" => [");

        let tag_end = el
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(el.tag_open.end);

        if has_children {
            let open_end = children[0].start;
            out.overwrite(el.tag_open.start, open_end, &buf);

            // Remove gaps between children
            for i in 1..children.len() {
                let prev_end = children[i - 1].end;
                let next_start = children[i].start;
                if next_start > prev_end {
                    out.overwrite(prev_end, next_start, "");
                }
            }

            // Add child separators + wrap static children in slot cache groups.
            // These must be a single combined pass because text-run wrapping
            // and cache wrapping both prepend at child boundary positions —
            // two separate passes cause position collisions where cache
            // prefixes/suffixes appear inside _createTextVNode() content.
            self.emit_slot_children_with_cache(&children, out, source, el_children);

            // Close: `])`
            buf.clear();
            buf.push_str("])");
            let close_start = children.last().unwrap().end;
            out.overwrite(close_start, tag_end, &buf);
        } else {
            // No children: single overwrite covers the entire element
            // (open tag through close tag or self-closing tag end).
            // This avoids leaving `</template>` unconsumed in the output
            // and avoids zero-length overwrite conflicts with parent gap-filling.
            buf.push_str("])");
            out.overwrite(el.tag_open.start, tag_end, &buf);
        }

        buf.clear();
        self.buf = buf;

        ChildRecord {
            start: el.tag_open.start,
            end: tag_end,
            kind: ChildKind::Element,
            condition: None,
            condition_prefix: None,
        }
    }

    /// Process a component element that has named slot children (`<template v-slot>`).
    /// The children were already processed as slot functions by `process_template_slot`.
    /// This method generates the component call with a slot object wrapper.
    ///
    /// When any slot child has `v-if`, uses `_createSlots()` dynamic format:
    /// ```js
    /// _createVNode(Comp, null, _createSlots({ _: 2 }, [
    ///   { name: "header", fn: _withCtx(() => [...]) },
    ///   (cond) ? { name: "footer", fn: _withCtx(() => [...]) } : undefined
    /// ]))
    /// ```
    ///
    /// Otherwise uses the static slot object format:
    /// ```js
    /// _createVNode(Comp, null, { header: _withCtx(() => [...]), _: 1 })
    /// ```
    #[allow(clippy::too_many_arguments)] // walker-context threading (id for hasScopeRef)
    pub(super) fn leave_component_with_slots(
        &mut self,
        id: NodeId,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        el_children: &[NodeId],
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
        is_block_root: bool,
        force_open_block: bool,
        injected_key: Option<u32>,
    ) {
        // Check for <component :is="expr"> -> _resolveDynamicComponent
        let dynamic_is = component::resolve_dynamic_component(
            el,
            source,
            oxc,
            &self.resolver,
            out,
            self.options.force_js,
        );
        let skip_prop = dynamic_is.as_ref().map(|(_, idx)| *idx);

        let resolved = if let Some((ref resolved_tag, _)) = dynamic_is {
            resolved_tag.clone()
        } else {
            let tag_name = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
            component::resolve_component_tag(
                tag_name,
                &self.resolver,
                out,
                &self.options.self_name,
                Some(&mut self.resolved_components),
            )
        };
        let comp_helper = if is_block_root {
            VdomHelper::CreateBlock
        } else {
            VdomHelper::CreateVNode
        };
        out.add_vdom_import(comp_helper);

        // Build child records for separator logic
        let mut children = self.build_child_records(el_children, source);
        // Pass false: tag extension + gap-filling below cover all removed regions,
        // so emitting removal overwrites here would create overlapping ranges.
        element::resolve_whitespace(&mut children, out, false);
        element::strip_interstitial_condition_nodes(&mut children, out, false);
        let has_children = !children.is_empty();

        // Check if any slot children have v-if/v-else-if/v-else conditions,
        // v-for, or dynamic slot names — all require _createSlots() dynamic format.
        let any_dynamic = children.iter().any(|c| c.condition.is_some())
            || el_children.iter().any(|&child_id| {
                let node = &self.ast.nodes[child_id.0];
                if let AstNodeKind::Element(ref child_el) = node.kind {
                    if child_el.tag_type == TagType::Template {
                        // v-for on slot template
                        if child_el.v_for.is_some() {
                            return true;
                        }
                        // Dynamic slot name: #[expr]
                        if let Some(ref v_slot) = child_el.v_slot {
                            if v_slot.is_dynamic == Some(true) {
                                return true;
                            }
                        }
                    }
                }
                false
            });

        // Build slot name map: child start position -> slot name
        // Also track dynamic slot info (dynamic names, v-for)
        let mut slot_names: FxHashMap<u32, &str> = FxHashMap::default();
        let mut slot_is_dynamic_name: FxHashMap<u32, bool> = FxHashMap::default();
        // v-for info: (params, resolved_iterable)
        let mut slot_vfor_info: FxHashMap<u32, (String, String)> = FxHashMap::default();
        for &child_id in el_children {
            let node = &self.ast.nodes[child_id.0];
            if let AstNodeKind::Element(ref child_el) = node.kind {
                if child_el.tag_type == TagType::Template && child_el.v_slot.is_some() {
                    let name = self.extract_v_slot_name(child_el, source);
                    let start = child_el.tag_open.start;
                    slot_names.insert(start, name);
                    if let Some(ref v_slot) = child_el.v_slot {
                        slot_is_dynamic_name.insert(start, v_slot.is_dynamic == Some(true));
                    }
                    if let Some(ref v_for) = child_el.v_for {
                        let full_expr = helpers::extract_directive_value(v_for, source);
                        let (params, iterable) = helpers::parse_v_for_expression(full_expr);
                        let resolved_iterable = self.resolver.resolve_simple_expr(iterable);
                        slot_vfor_info.insert(start, (params.to_string(), resolved_iterable));
                    }
                }
            }
        }

        // Count effective props (excluding the :is prop consumed by dynamic component)
        let has_props = if skip_prop.is_some() {
            el.props.len() > 1
        } else {
            !el.props.is_empty()
        };
        let mut buf = std::mem::take(&mut self.buf);
        buf.clear();

        // v-for prefix
        let v_for_prefix = self.v_for_prefixes.pop().flatten();
        if let Some((prefix, _)) = v_for_prefix.as_ref() {
            buf.push_str(prefix);
        }

        // Block root wrapping for v-for/v-if
        let needs_block_wrapper =
            force_open_block || (is_block_root && (el.v_for.is_some() || el.v_condition.is_some()));
        if needs_block_wrapper {
            buf.push_str("(_openBlock(), ");
            out.add_vdom_import(VdomHelper::OpenBlock);
        }

        // Props
        // `uses_full_props_spread` tracks the single v-bind object-spread path
        // (`_normalizeProps(_guardReactiveProps(expr))`) which official Vue
        // always tags with FULL_PROPS (16) so fallthrough attrs re-diff.
        // Also collect v-show / custom directives for `_withDirectives` wrapping
        // (components with slots used to drop them — AvatarImage regression).
        let mut props_buf = String::new();
        let (dynamic_props, uses_full_props_spread, native_vmodel, directive_entries) = if has_props
        {
            props_buf.push_str(", ");
            let props_start = props_buf.len();
            let props_result = element::build_props_object_into(
                &mut props_buf,
                el,
                source,
                &self.resolver,
                oxc,
                skip_prop,
                self.options.force_js,
            );
            // v-if branch root (component) with user props: inject `key: N` as
            // the first property (unless the user authored an explicit :key).
            if let Some(k) = injected_key {
                element::inject_branch_key(&mut props_buf, props_start, k);
            }
            if props_result.uses_merge {
                out.add_vdom_import(VdomHelper::MergeProps);
            }
            if props_result.uses_normalize_class {
                out.add_vdom_import(VdomHelper::NormalizeClass);
            }
            if props_result.uses_normalize_style {
                out.add_vdom_import(VdomHelper::NormalizeStyle);
            }
            if props_result.uses_with_modifiers {
                out.add_vdom_import(VdomHelper::WithModifiers);
            }
            if props_result.uses_with_keys {
                out.add_vdom_import(VdomHelper::WithKeys);
            }
            if props_result.uses_normalize_props {
                out.add_vdom_import(VdomHelper::NormalizeProps);
            }
            if props_result.uses_guard_reactive_props {
                out.add_vdom_import(VdomHelper::GuardReactiveProps);
            }
            if props_result.uses_to_handlers {
                out.add_vdom_import(VdomHelper::ToHandlers);
            }
            let full_props_spread =
                props_result.uses_normalize_props && props_result.uses_guard_reactive_props;
            (
                props_result.dynamic_props,
                full_props_spread,
                props_result.native_vmodel,
                props_result.directive_entries,
            )
        } else if let Some(k) = injected_key {
            // v-if branch root (component) with no user props: the branch key is
            // the props object.
            props_buf.push_str(", { key: ");
            props_buf.push_str(&k.to_string());
            props_buf.push_str(" }");
            (Vec::new(), false, None, Vec::new())
        } else {
            if has_children {
                props_buf.push_str(", null");
            }
            (Vec::new(), false, None, Vec::new())
        };

        let has_directives_wrap = native_vmodel.is_some() || !directive_entries.is_empty();
        if has_directives_wrap {
            buf.push_str("_withDirectives(");
            out.add_vdom_import(VdomHelper::WithDirectives);
        }

        buf.push_str(comp_helper.name());
        buf.push('(');
        buf.push_str(&resolved);
        buf.push_str(&props_buf);

        if has_children {
            if any_dynamic {
                out.add_vdom_import(VdomHelper::CreateSlots);
                if self.options.is_production {
                    buf.push_str(", _createSlots({ _: 2 }, [");
                } else {
                    buf.push_str(", _createSlots({ _: 2 /* DYNAMIC */ }, [");
                }
            } else {
                buf.push_str(", {");
            }
        }

        let open_end = if has_children {
            children[0].start
        } else {
            el.tag_open.end
        };
        out.overwrite(el.tag_open.start, open_end, &buf);

        // Remove gaps between children
        for i in 1..children.len() {
            let prev_end = children[i - 1].end;
            let next_start = children[i].start;
            if next_start > prev_end {
                out.overwrite(prev_end, next_start, "");
            }
        }

        if has_children {
            // Build slot entries: named template slots are single entries,
            // consecutive non-template children form the default slot group.
            let entries = self.build_slot_entries(&children, &slot_names);

            if any_dynamic {
                // Dynamic slot format: each slot is `{ name: "x", fn: _withCtx(...) }`
                // with ternary wrapping for conditional slots.
                self.emit_dynamic_slot_wrappers(
                    &entries,
                    &children,
                    &slot_names,
                    &slot_is_dynamic_name,
                    &slot_vfor_info,
                    out,
                    source,
                    el_children,
                );
            } else {
                // Static slot format: each slot is `name: _withCtx(...)`
                self.emit_static_slot_wrappers(
                    &entries,
                    &children,
                    &slot_names,
                    out,
                    source,
                    el_children,
                );
            }
        }

        // Close
        let tag_end = el
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(el.tag_open.end);

        // Patch flags for the component call itself (props/attrs).
        // MUST include FULL_PROPS for v-bind spreads even when `dynamic_props`
        // is empty — the spread path uses `_normalizeProps(_guardReactiveProps(...))`
        // with no per-key dynamicProps list. Omitting FULL_PROPS freezes initial
        // fallthrough attrs (e.g. class) across setProps / $attrs updates
        // (oku Label dynamic class regression).
        let expr_flag = oxc
            .map(|o| o.expression_flag)
            .unwrap_or(ExpressionFlag::empty());
        let mut props_patch_flag =
            props::compute_patch_flags(el.prop_flag, expr_flag, el.children_mode);
        if !dynamic_props.is_empty() {
            props_patch_flag |= helpers::PATCH_PROPS;
        } else {
            // Same as element.rs: empty dynamic_props clears PATCH_PROPS only.
            // FULL_PROPS from spreads is preserved.
            props_patch_flag &= !helpers::PATCH_PROPS;
        }
        // Bare object-spread emission (`_normalizeProps(_guardReactiveProps(...))`)
        // always needs FULL_PROPS, matching official Vue — even when `dynamic_props`
        // is empty and regardless of whether `prop_flag` retained the spread bit.
        if uses_full_props_spread || el.prop_flag.has_spread() || el.has_spread() {
            props_patch_flag |= helpers::PATCH_FULL_PROPS;
        }

        buf.clear();
        if has_children && any_dynamic {
            // Dynamic: close the _createSlots array and component call
            buf.push_str("])");
            // Emit DYNAMIC_SLOTS plus any props patch flags (incl. FULL_PROPS).
            // Strip TEXT — component children are slots, not element text.
            buf.push_str(", ");
            let flag = helpers::PATCH_DYNAMIC_SLOTS | (props_patch_flag & !helpers::PATCH_TEXT);
            let flag_str =
                helpers::format_patch_flag(flag, self.options.is_production, |s| out.alloc_str(s));
            buf.push_str(flag_str);
            if (props_patch_flag & helpers::PATCH_PROPS) != 0 && !dynamic_props.is_empty() {
                buf.push_str(", ");
                let props_ref = element::format_dynamic_props_ref(
                    &dynamic_props,
                    Some(&mut self.hoisted_constants),
                );
                buf.push_str(&props_ref);
            }
            buf.push(')');
        } else if has_children {
            // Static named/default slots object: DYNAMIC iff the slot
            // subtree references an OUTER template-scope variable
            // (official-parity `hasScopeRef`); forwarded `<slot>` → see
            // helper.
            let slots_dynamic = self.component_slots_reference_outer_scope(id, oxc, source);
            self.emit_named_slots_object_close(
                &mut buf,
                out,
                el_children,
                props_patch_flag,
                &dynamic_props,
                slots_dynamic,
            );
            buf.push(')');
        } else {
            buf.push(')');
        }
        // Close `_withDirectives(...)` for v-show / custom dirs on components
        // with children (named/default slots).
        if has_directives_wrap {
            element::emit_with_directives_close(&mut buf, &native_vmodel, &directive_entries, out);
        }
        // Close the outer (_openBlock(), ...) wrapper for block root components
        if needs_block_wrapper {
            buf.push(')');
        }

        let close_start = if has_children {
            children.last().unwrap().end
        } else {
            tag_end
        };
        out.overwrite(close_start, tag_end, &buf);

        buf.clear();
        self.buf = buf;

        // Emit scope close suffix for structural directives
        if let Some(scope_close) = self.scope_closes.pop().flatten() {
            let suffix = directives::format_scope_close(&scope_close, self.options.is_production);
            if !suffix.is_empty() {
                out.prepend_static(tag_end, suffix);
            }
        }
    }

    /// Group children into slot entries: named template slots (single records)
    /// and default slot groups (consecutive non-template children).
    fn build_slot_entries(
        &self,
        children: &[ChildRecord],
        slot_names: &FxHashMap<u32, &str>,
    ) -> Vec<SlotEntry> {
        let mut entries = Vec::new();
        let mut default_start: Option<usize> = None;

        for (i, child) in children.iter().enumerate() {
            if slot_names.contains_key(&child.start) {
                // Flush any pending default group
                if let Some(ds) = default_start.take() {
                    entries.push(SlotEntry::Default { start: ds, end: i });
                }
                entries.push(SlotEntry::Named(i));
            } else if default_start.is_none() {
                default_start = Some(i);
            }
        }
        if let Some(ds) = default_start.take() {
            entries.push(SlotEntry::Default {
                start: ds,
                end: children.len(),
            });
        }
        entries
    }

    /// Get the end position of a slot entry (end of last child in the entry).
    fn slot_entry_end(entry: &SlotEntry, children: &[ChildRecord]) -> u32 {
        match entry {
            SlotEntry::Named(i) => children[*i].end,
            SlotEntry::Default { end, .. } => children[*end - 1].end,
        }
    }

    /// Emit static slot format: `name: _withCtx(...)` for named slots,
    /// `default: _withCtx(() => [...])` for default slot groups.
    fn emit_static_slot_wrappers(
        &mut self,
        entries: &[SlotEntry],
        children: &[ChildRecord],
        slot_names: &FxHashMap<u32, &str>,
        out: &mut CodeGenOutput<'alloc>,
        source: &'alloc str,
        el_children: &[NodeId],
    ) {
        for (ei, entry) in entries.iter().enumerate() {
            // Comma separator between top-level slot entries
            if ei > 0 {
                let prev_end = Self::slot_entry_end(&entries[ei - 1], children);
                out.prepend_static(prev_end, ", ");
            }

            match entry {
                SlotEntry::Named(i) => {
                    let slot_name = slot_names
                        .get(&children[*i].start)
                        .copied()
                        .unwrap_or("default");
                    let mut prefix = String::new();
                    format_slot_key(&mut prefix, slot_name);
                    prefix.push_str(": ");
                    out.prepend_alloc(children[*i].start, &prefix);
                }
                SlotEntry::Default { start, end } => {
                    let group = &children[*start..*end];
                    out.add_vdom_import(VdomHelper::WithCtx);
                    // Order matters for prepend stacking at the same position:
                    // 1. Outer wrapper open FIRST (appears before inner wrappers)
                    out.prepend_static(group[0].start, "default: _withCtx(() => [");
                    // 2. Combined text wrapping + slot cache wrapping
                    self.emit_slot_children_with_cache(group, out, source, el_children);
                    // 3. Outer wrapper close LAST (appears after inner closings)
                    out.prepend_static(group.last().unwrap().end, "])");
                }
            }
        }
    }

    /// Emit dynamic slot format: each slot is `{ name: "x", fn: _withCtx(...) }`
    /// in an array, with ternary wrapping for conditional slots.
    /// Dynamic slot names use `name: resolvedExpr` instead of `name: "staticName"`.
    /// v-for slots use `_renderList(iterable, (params) => ({ name: expr, fn: ... }))`.
    #[allow(clippy::too_many_arguments)]
    fn emit_dynamic_slot_wrappers(
        &mut self,
        entries: &[SlotEntry],
        children: &[ChildRecord],
        slot_names: &FxHashMap<u32, &str>,
        slot_is_dynamic_name: &FxHashMap<u32, bool>,
        slot_vfor_info: &FxHashMap<u32, (String, String)>,
        out: &mut CodeGenOutput<'alloc>,
        source: &'alloc str,
        el_children: &[NodeId],
    ) {
        for (ei, entry) in entries.iter().enumerate() {
            match entry {
                SlotEntry::Named(i) => {
                    let child = &children[*i];
                    let slot_name = slot_names.get(&child.start).copied().unwrap_or("default");
                    let is_start = child.condition == Some(ConditionChainRole::Start);
                    let is_continuation = child.condition == Some(ConditionChainRole::Continuation);

                    // Separator before this entry.
                    //
                    // The dynamic-slot object wrapper (`{ name: …, fn: `) is an
                    // unmapped prepend at this same anchor that must follow the
                    // condition head, so the condition is emitted UNMAPPED through
                    // the same channel to preserve insertion order (the channel
                    // merge places all mapped prepends after unmapped ones at a
                    // shared position). This path maps nothing to source, so the
                    // no-synthetic-bleed invariant holds trivially; the array-mode
                    // sites (`emit_slot_children_with_cache`, `children.rs`) carry
                    // the per-segment mapping.
                    if ei > 0 {
                        if is_continuation {
                            if let Some(ref prefix) = child.condition_prefix {
                                out.prepend_fmt(child.start, format_args!(" : {}", prefix.text));
                            } else {
                                out.prepend_static(child.start, " : ");
                            }
                        } else {
                            let prev_end = Self::slot_entry_end(&entries[ei - 1], children);
                            out.prepend_static(prev_end, ", ");
                        }
                    }

                    // Condition prefix for v-if start (unmapped, see above).
                    if is_start {
                        if let Some(ref prefix) = child.condition_prefix {
                            out.prepend_alloc(child.start, &prefix.text);
                        }
                    }

                    // v-for wrapping: _renderList(iterable, (params) => {return ...})
                    let has_vfor = slot_vfor_info.contains_key(&child.start);
                    if has_vfor {
                        let (params, iterable) = slot_vfor_info.get(&child.start).unwrap();
                        out.prepend_fmt(
                            child.start,
                            format_args!("_renderList({iterable}, ({params}) => {{return "),
                        );
                        out.add_vdom_import(VdomHelper::RenderList);
                    }

                    // Object wrapper: { name: "slot_name", fn:  (or { name: expr, fn: for dynamic)
                    let is_dynamic = slot_is_dynamic_name
                        .get(&child.start)
                        .copied()
                        .unwrap_or(false);
                    let wrapper = if is_dynamic {
                        // Dynamic slot name: strip brackets and resolve
                        let raw_name = slot_name.trim();
                        let inner = if raw_name.starts_with('[') && raw_name.ends_with(']') {
                            &raw_name[1..raw_name.len() - 1]
                        } else {
                            raw_name
                        };
                        let resolved = self.resolver.resolve_simple_expr(inner);
                        format!("{{ name: {resolved}, fn: ")
                    } else {
                        format!("{{ name: \"{slot_name}\", fn: ")
                    };
                    out.prepend_alloc(child.start, &wrapper);

                    // Close object
                    out.prepend_static(child.end, " }");

                    // Close v-for wrapping
                    if has_vfor {
                        out.prepend_static(child.end, "})")
                    }

                    // Ternary fallback for end of v-if chain
                    if is_start || (is_continuation && child.condition_prefix.is_some()) {
                        // Check if NEXT entry is a continuation of this chain
                        let next_is_continuation =
                            if let Some(SlotEntry::Named(ni)) = entries.get(ei + 1) {
                                children[*ni].condition == Some(ConditionChainRole::Continuation)
                            } else {
                                false
                            };
                        if !next_is_continuation {
                            out.prepend_static(child.end, " : undefined");
                        }
                    }
                }
                SlotEntry::Default { start, end } => {
                    let group = &children[*start..*end];
                    out.add_vdom_import(VdomHelper::WithCtx);

                    // Comma separator before default entry
                    if ei > 0 {
                        let prev_end = Self::slot_entry_end(&entries[ei - 1], children);
                        out.prepend_static(prev_end, ", ");
                    }

                    // Order matters for prepend stacking:
                    // 1. Outer wrapper open FIRST
                    out.prepend_static(group[0].start, "{ name: \"default\", fn: _withCtx(() => [");
                    // 2. Combined text wrapping + slot cache wrapping
                    self.emit_slot_children_with_cache(group, out, source, el_children);
                    // 3. Outer wrapper close LAST
                    out.prepend_static(group.last().unwrap().end, "]) }");
                }
            }
        }
    }

    /// Process a component element with implicit default slot (non-slot children).
    /// Wraps children in `{ default: _withCtx(() => [...]), _: 1 }`.
    #[allow(clippy::too_many_arguments)] // walker-context threading (id for hasScopeRef)
    pub(super) fn leave_component_with_default_slot(
        &mut self,
        id: NodeId,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        el_children: &[NodeId],
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
        is_block_root: bool,
        force_open_block: bool,
        injected_key: Option<u32>,
    ) {
        // Check for <component :is="expr"> -> _resolveDynamicComponent
        let dynamic_is = component::resolve_dynamic_component(
            el,
            source,
            oxc,
            &self.resolver,
            out,
            self.options.force_js,
        );
        let skip_prop = dynamic_is.as_ref().map(|(_, idx)| *idx);

        let resolved = if let Some((ref resolved_tag, _)) = dynamic_is {
            resolved_tag.clone()
        } else {
            let tag_name = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];
            component::resolve_component_tag(
                tag_name,
                &self.resolver,
                out,
                &self.options.self_name,
                Some(&mut self.resolved_components),
            )
        };
        let comp_helper = if is_block_root {
            VdomHelper::CreateBlock
        } else {
            VdomHelper::CreateVNode
        };
        out.add_vdom_import(comp_helper);
        out.add_vdom_import(VdomHelper::WithCtx);

        let mut children = self.build_child_records(el_children, source);
        // Pass false: tag extension + gap-filling below cover all removed regions,
        // so emitting removal overwrites here would create overlapping ranges.
        element::resolve_whitespace(&mut children, out, false);
        element::strip_interstitial_condition_nodes(&mut children, out, false);
        let has_children = !children.is_empty();

        // Count effective props (excluding the :is prop consumed by dynamic component)
        let effective_prop_count = if skip_prop.is_some() {
            el.props.len().saturating_sub(1)
        } else {
            el.props.len()
        };
        let has_props = effective_prop_count > 0;
        let mut buf = std::mem::take(&mut self.buf);
        buf.clear();

        let v_for_prefix = self.v_for_prefixes.pop().flatten();
        if let Some((prefix, _)) = v_for_prefix.as_ref() {
            buf.push_str(prefix);
        }

        // Block root wrapping for v-for/v-if
        let needs_block_wrapper =
            force_open_block || (is_block_root && (el.v_for.is_some() || el.v_condition.is_some()));
        if needs_block_wrapper {
            buf.push_str("(_openBlock(), ");
            out.add_vdom_import(VdomHelper::OpenBlock);
        }

        let mut props_buf = String::new();
        let (dynamic_props, uses_full_props_spread, native_vmodel, directive_entries) = if has_props
        {
            props_buf.push_str(", ");
            let props_start = props_buf.len();
            let props_result = element::build_props_object_into(
                &mut props_buf,
                el,
                source,
                &self.resolver,
                oxc,
                skip_prop,
                self.options.force_js,
            );
            // v-if branch root (component) with user props: inject `key: N` as
            // the first property (unless the user authored an explicit :key).
            if let Some(k) = injected_key {
                element::inject_branch_key(&mut props_buf, props_start, k);
            }
            if props_result.uses_merge {
                out.add_vdom_import(VdomHelper::MergeProps);
            }
            if props_result.uses_normalize_class {
                out.add_vdom_import(VdomHelper::NormalizeClass);
            }
            if props_result.uses_normalize_style {
                out.add_vdom_import(VdomHelper::NormalizeStyle);
            }
            if props_result.uses_with_modifiers {
                out.add_vdom_import(VdomHelper::WithModifiers);
            }
            if props_result.uses_with_keys {
                out.add_vdom_import(VdomHelper::WithKeys);
            }
            if props_result.uses_normalize_props {
                out.add_vdom_import(VdomHelper::NormalizeProps);
            }
            if props_result.uses_guard_reactive_props {
                out.add_vdom_import(VdomHelper::GuardReactiveProps);
            }
            if props_result.uses_to_handlers {
                out.add_vdom_import(VdomHelper::ToHandlers);
            }
            let full_props_spread =
                props_result.uses_normalize_props && props_result.uses_guard_reactive_props;
            (
                props_result.dynamic_props,
                full_props_spread,
                props_result.native_vmodel,
                props_result.directive_entries,
            )
        } else if let Some(k) = injected_key {
            // v-if branch root (component) with no user props: the branch key is
            // the props object.
            props_buf.push_str(", { key: ");
            props_buf.push_str(&k.to_string());
            props_buf.push_str(" }");
            (Vec::new(), false, None, Vec::new())
        } else {
            if has_children {
                props_buf.push_str(", null");
            }
            (Vec::new(), false, None, Vec::new())
        };

        let has_directives_wrap = native_vmodel.is_some() || !directive_entries.is_empty();
        if has_directives_wrap {
            buf.push_str("_withDirectives(");
            out.add_vdom_import(VdomHelper::WithDirectives);
        }

        buf.push_str(comp_helper.name());
        buf.push('(');
        buf.push_str(&resolved);
        buf.push_str(&props_buf);

        // Same FULL_PROPS rule as leave_component_with_slots — implicit default
        // slot path is what Label.vue and most component-with-children use.
        let expr_flag = oxc
            .map(|o| o.expression_flag)
            .unwrap_or(ExpressionFlag::empty());
        let mut props_patch_flag =
            props::compute_patch_flags(el.prop_flag, expr_flag, el.children_mode);
        if !dynamic_props.is_empty() {
            props_patch_flag |= helpers::PATCH_PROPS;
        } else {
            props_patch_flag &= !helpers::PATCH_PROPS;
        }
        if uses_full_props_spread || el.prop_flag.has_spread() || el.has_spread() {
            props_patch_flag |= helpers::PATCH_FULL_PROPS;
        }

        let tag_end = el
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(el.tag_open.end);

        if !has_children {
            // All children were whitespace -- no meaningful content.
            // Overwrite the entire element [open_start, close_end) with the
            // component call (avoids leaving raw `</Component>` in output).
            if props_patch_flag != 0 {
                buf.push_str(", null, ");
                let flag_str =
                    helpers::format_patch_flag(props_patch_flag, self.options.is_production, |s| {
                        out.alloc_str(s)
                    });
                buf.push_str(flag_str);
                if (props_patch_flag & helpers::PATCH_PROPS) != 0 && !dynamic_props.is_empty() {
                    buf.push_str(", ");
                    let props_ref = element::format_dynamic_props_ref(
                        &dynamic_props,
                        Some(&mut self.hoisted_constants),
                    );
                    buf.push_str(&props_ref);
                }
            }
            buf.push(')');
            if has_directives_wrap {
                element::emit_with_directives_close(
                    &mut buf,
                    &native_vmodel,
                    &directive_entries,
                    out,
                );
            }
            if needs_block_wrapper {
                buf.push(')');
            }
            out.overwrite(el.tag_open.start, tag_end, &buf);
            buf.clear();
            self.buf = buf;

            if let Some(scope_close) = self.scope_closes.pop().flatten() {
                let suffix =
                    directives::format_scope_close(&scope_close, self.options.is_production);
                if !suffix.is_empty() {
                    out.prepend_static(tag_end, suffix);
                }
            }
            return;
        }

        // Inject component-level v-slot params into the default slot function.
        // <Comp v-slot="{ item }">...</Comp> -> {default: _withCtx(({ item }) => [...])}
        buf.push_str(", {default: _withCtx(");
        let has_slot_params = if let Some(ref v_slot) = el.v_slot {
            if let (Some(vs), Some(ve)) = (v_slot.value_start, v_slot.value_end) {
                let params = &source[vs as usize..ve as usize];
                if !params.trim().is_empty() {
                    buf.push('(');
                    buf.push_str(params);
                    buf.push(')');
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };
        if !has_slot_params {
            buf.push_str("()");
        }
        buf.push_str(" => [");

        let open_end = children[0].start;
        out.overwrite(el.tag_open.start, open_end, &buf);

        // Remove gaps between children
        for i in 1..children.len() {
            let prev_end = children[i - 1].end;
            let next_start = children[i].start;
            if next_start > prev_end {
                out.overwrite(prev_end, next_start, "");
            }
        }

        // Combined text wrapping + slot cache wrapping
        self.emit_slot_children_with_cache(&children, out, source, el_children);

        buf.clear();
        // DYNAMIC iff the slot subtree references an OUTER template-scope
        // variable (official-parity `hasScopeRef`); forwarded → FORWARDED;
        // else STABLE. TEXT is stripped (slots are not element text children).
        let slots_dynamic = self.component_slots_reference_outer_scope(id, oxc, source);
        self.emit_component_slot_close(
            &mut buf,
            out,
            el_children,
            props_patch_flag,
            &dynamic_props,
            slots_dynamic,
        );
        buf.push(')');
        // Close `_withDirectives(...)` for v-show / custom dirs on components
        // with an implicit default slot (AvatarImage, etc.).
        if has_directives_wrap {
            element::emit_with_directives_close(&mut buf, &native_vmodel, &directive_entries, out);
        }
        // Close the outer (_openBlock(), ...) wrapper for block root components
        if needs_block_wrapper {
            buf.push(')');
        }

        let close_start = children.last().unwrap().end;
        out.overwrite(close_start, tag_end, &buf);

        buf.clear();
        self.buf = buf;

        if let Some(scope_close) = self.scope_closes.pop().flatten() {
            let suffix = directives::format_scope_close(&scope_close, self.options.is_production);
            if !suffix.is_empty() {
                out.prepend_static(tag_end, suffix);
            }
        }
    }

    /// Official-parity `hasScopeRef`: a component's static-format slots are
    /// `_: 2 /* DYNAMIC */` iff the slot subtree references a template-scope
    /// variable bound OUTSIDE the slot boundary — an enclosing `v-for` alias
    /// (including the component's OWN `v-for`) or an enclosing component's
    /// slot parameters. The component's OWN `v-slot` params do NOT count:
    /// a STABLE slot re-renders through the child's own effect, which
    /// re-invokes the slot function with fresh args (`v-slot="{ grid }"` at
    /// top level is STABLE in official output).
    ///
    /// Matches official build-mode (`prefixIdentifiers`) semantics, which
    /// REPLACE the coarse in-v-for/in-v-slot scope counters: a component
    /// inside `v-for` whose slot content is scope-independent stays STABLE.
    /// Like official, the check is name-based (shadowing below the boundary
    /// still counts as a reference — the safe, over-marking direction).
    fn component_slots_reference_outer_scope(
        &self,
        id: NodeId,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &str,
    ) -> bool {
        let outer = self.outer_scope_names(id, oxc, source);
        if outer.is_empty() {
            return false;
        }
        let el_children = match &self.ast.nodes[id.0].kind {
            AstNodeKind::Element(el) => el
                .content
                .as_ref()
                .map(|c| c.children.as_slice())
                .unwrap_or(&[]),
            _ => return false,
        };
        self.subtree_references_scope_names(el_children, &outer)
    }

    /// Template-scope variable names active at `id` from OUTER scopes:
    /// every enclosing `v-for` alias / `v-slot` param plus the element's
    /// own `v-for` aliases — but NOT its own `v-slot` params.
    ///
    /// `provided_locals` on the element's OXC parse already carries
    /// inherited + own locals (own pushed LAST, v-for before v-slot), so
    /// own slot params are removed by last-occurrence; an element without
    /// scoping directives inherits the nearest ancestor's set.
    fn outer_scope_names(
        &self,
        id: NodeId,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &str,
    ) -> Vec<String> {
        // The element's own locals row, if it has scoping directives.
        if let Some(oxc_el) = oxc {
            if let Some(locals) = &oxc_el.provided_locals {
                let mut names: Vec<String> = locals.iter().map(|s| s.to_string()).collect();
                if let Some(v_slot) = &oxc_el.v_slot {
                    for span in &v_slot.parsed.locals {
                        let name = span.slice(source);
                        if let Some(pos) = names.iter().rposition(|n| n == name) {
                            names.remove(pos);
                        }
                    }
                }
                return names;
            }
        }
        // No own scoping directives: nearest ancestor's provided locals.
        let mut current = self.ast.nodes[id.0].parent;
        while let Some(pid) = current {
            if let Some(crate::template::oxc::types::OxcNodeData::Element(ancestor)) =
                self.oxc_ast.data.get(pid.0)
            {
                if let Some(locals) = &ancestor.provided_locals {
                    return locals.iter().map(|s| s.to_string()).collect();
                }
            }
            current = self.ast.nodes[pid.0].parent;
        }
        Vec::new()
    }

    /// True when any expression under `children` references one of `names`.
    /// Scans interpolations, prop values/args, v-if conditions, v-for
    /// iterables, and v-slot default-value expressions, recursing through
    /// the subtree. Name-based like official `hasScopeRef` — descendant
    /// shadowing is deliberately not subtracted.
    fn subtree_references_scope_names(&self, children: &[NodeId], names: &[String]) -> bool {
        let name_hit = |n: &str| names.iter().any(|outer| outer == n);
        for &child_id in children {
            match self.oxc_ast.data.get(child_id.0) {
                Some(crate::template::oxc::types::OxcNodeData::Interpolation(expr)) => {
                    if let Some(bindings) = &expr.bindings {
                        if bindings
                            .bindings
                            .iter()
                            .any(|b| b.ignore && name_hit(b.name))
                        {
                            return true;
                        }
                    }
                }
                Some(crate::template::oxc::types::OxcNodeData::Element(oxc_el)) => {
                    let expr_hits =
                        |expr: &crate::template::oxc::types::OxcParsedExpression<'alloc>| {
                            expr.bindings.as_ref().is_some_and(|bindings| {
                                bindings
                                    .bindings
                                    .iter()
                                    .any(|b| b.ignore && name_hit(b.name))
                            })
                        };
                    if oxc_el.condition.as_ref().is_some_and(expr_hits) {
                        return true;
                    }
                    for prop in &oxc_el.props {
                        if prop.exp.as_ref().is_some_and(expr_hits)
                            || prop.arg.as_ref().is_some_and(expr_hits)
                        {
                            return true;
                        }
                    }
                    // v-for iterables / v-slot defaults record their
                    // template-scope references in the dedicated
                    // scope-local name set (their `references` and
                    // liveness sets both EXCLUDE scope locals).
                    if let Some(v_for) = &oxc_el.v_for {
                        if v_for
                            .parsed
                            .scope_local_reference_names
                            .iter()
                            .any(|n| name_hit(n))
                        {
                            return true;
                        }
                    }
                    if let Some(v_slot) = &oxc_el.v_slot {
                        if v_slot
                            .parsed
                            .scope_local_reference_names
                            .iter()
                            .any(|n| name_hit(n))
                        {
                            return true;
                        }
                    }
                }
                _ => {}
            }
            // Recurse into element children.
            if let AstNodeKind::Element(child_el) = &self.ast.nodes[child_id.0].kind {
                if let Some(content) = &child_el.content {
                    if self.subtree_references_scope_names(&content.children, names) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Emit the slot-flag / patch-flag tail for a component with children.
    ///
    /// - Slot subtree references an OUTER template-scope variable
    ///   (official-parity `hasScopeRef`) → `_: 2 /* DYNAMIC */` +
    ///   `DYNAMIC_SLOTS` (and props flags)
    /// - Forwarded `<slot>` → `_: 3 /* FORWARDED */`
    /// - Otherwise → `_: 1 /* STABLE */`
    ///
    /// Component patch flags never include `TEXT` (children are slots, not
    /// element text nodes).
    fn emit_component_slot_close(
        &mut self,
        buf: &mut String,
        out: &mut CodeGenOutput<'_>,
        el_children: &[NodeId],
        mut props_patch_flag: u32,
        dynamic_props: &[String],
        slots_dynamic: bool,
    ) {
        // TEXT applies to element children only; strip for components.
        props_patch_flag &= !helpers::PATCH_TEXT;

        let forwarded = !slots_dynamic && self.has_forwarded_slots(el_children);

        if slots_dynamic {
            if self.options.is_production {
                buf.push_str("]), _: 2}");
            } else {
                buf.push_str("]), _: 2 /* DYNAMIC */}");
            }
            props_patch_flag |= helpers::PATCH_DYNAMIC_SLOTS;
        } else if forwarded {
            if self.options.is_production {
                buf.push_str("]), _: 3}");
            } else {
                buf.push_str("]), _: 3 /* FORWARDED */}");
            }
        } else if self.options.is_production {
            buf.push_str("]), _: 1}");
        } else {
            buf.push_str("]), _: 1 /* STABLE */}");
        }

        if props_patch_flag != 0 {
            buf.push_str(", ");
            let flag_str =
                helpers::format_patch_flag(props_patch_flag, self.options.is_production, |s| {
                    out.alloc_str(s)
                });
            buf.push_str(flag_str);
            if (props_patch_flag & helpers::PATCH_PROPS) != 0 && !dynamic_props.is_empty() {
                buf.push_str(", ");
                let props_ref = element::format_dynamic_props_ref(
                    dynamic_props,
                    Some(&mut self.hoisted_constants),
                );
                buf.push_str(&props_ref);
            }
        }
    }

    /// Variant of [`emit_component_slot_close`] for the named-slots object path
    /// where the slots object is closed with `, _: N}` rather than `]), _: N}`.
    fn emit_named_slots_object_close(
        &mut self,
        buf: &mut String,
        out: &mut CodeGenOutput<'_>,
        el_children: &[NodeId],
        mut props_patch_flag: u32,
        dynamic_props: &[String],
        slots_dynamic: bool,
    ) {
        props_patch_flag &= !helpers::PATCH_TEXT;

        let forwarded = !slots_dynamic && self.has_forwarded_slots(el_children);

        if slots_dynamic {
            if self.options.is_production {
                buf.push_str(", _: 2}");
            } else {
                buf.push_str(", _: 2 /* DYNAMIC */}");
            }
            props_patch_flag |= helpers::PATCH_DYNAMIC_SLOTS;
        } else if forwarded {
            if self.options.is_production {
                buf.push_str(", _: 3}");
            } else {
                buf.push_str(", _: 3 /* FORWARDED */}");
            }
        } else if self.options.is_production {
            buf.push_str(", _: 1}");
        } else {
            buf.push_str(", _: 1 /* STABLE */}");
        }

        if props_patch_flag != 0 {
            buf.push_str(", ");
            let flag_str =
                helpers::format_patch_flag(props_patch_flag, self.options.is_production, |s| {
                    out.alloc_str(s)
                });
            buf.push_str(flag_str);
            if (props_patch_flag & helpers::PATCH_PROPS) != 0 && !dynamic_props.is_empty() {
                buf.push_str(", ");
                let props_ref = element::format_dynamic_props_ref(
                    dynamic_props,
                    Some(&mut self.hoisted_constants),
                );
                buf.push_str(&props_ref);
            }
        }
    }

    /// Check whether an element's AST children contain any `<template v-slot>` elements.
    /// Check if any descendant of the given children contains a `<slot>` outlet.
    /// When true, the parent component should use `_: 3 /* FORWARDED */` instead
    /// of `_: 1 /* STABLE */` to ensure proper reactivity tracking.
    pub(super) fn has_forwarded_slots(&self, el_children: &[NodeId]) -> bool {
        for &child_id in el_children {
            let node = &self.ast.nodes[child_id.0];
            if let AstNodeKind::Element(ref child_el) = node.kind {
                if child_el.tag_type.is_slot_outlet() {
                    return true;
                }
                // Recurse into child's children
                if let Some(ref content) = child_el.content {
                    if self.has_forwarded_slots(&content.children) {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub(super) fn has_slot_children(&self, el_children: &[NodeId]) -> bool {
        for &child_id in el_children {
            let node = &self.ast.nodes[child_id.0];
            if let AstNodeKind::Element(ref child_el) = node.kind {
                if child_el.tag_type == TagType::Template && child_el.v_slot.is_some() {
                    return true;
                }
            }
        }
        false
    }

    /// Check if a child record represents a static item for slot caching purposes.
    /// Combined pass that emits array-mode separators, text-run wrapping,
    /// and slot cache wrapping for `<template v-slot>` children.
    ///
    /// This MUST be a single pass because text-run wrapping (`_createTextVNode`)
    /// and cache wrapping (`_cache[N] || ...`) both prepend at child boundary
    /// positions. Two separate passes cause position collisions where cache
    /// wrappers appear inside `_createTextVNode("...")` content or vice versa.
    ///
    /// The pass walks children left-to-right, identifying:
    /// - **Cache groups**: consecutive static children, wrapped in `_cache[N]`
    /// - **Dynamic text runs**: consecutive Text/Interpolation, wrapped in
    ///   `_createTextVNode(...)`
    /// - **Dynamic elements**: standalone elements with condition chain support
    ///
    /// Within a cache group, items get inner separators and text wrapping.
    fn emit_slot_children_with_cache(
        &mut self,
        children: &[ChildRecord],
        out: &mut CodeGenOutput<'alloc>,
        source: &'alloc str,
        el_children: &[NodeId],
    ) {
        if children.is_empty() {
            return;
        }

        // Pre-compute static flags for cache grouping
        let static_flags: Vec<bool> = if self.options.hoist_static {
            children
                .iter()
                .map(|c| self.is_slot_child_static(c, el_children))
                .collect()
        } else {
            vec![false; children.len()]
        };

        let mut i = 0;
        let mut is_first_item = true;
        let mut prev_item_end: u32 = 0;

        while i < children.len() {
            if static_flags[i] {
                // === Cache group: consecutive static children ===
                let run_start = i;
                while i < children.len() && static_flags[i] {
                    i += 1;
                }
                let run_end = i;
                let run_len = run_end - run_start;

                let cache_idx = self.cache_index;
                self.cache_index += 1;

                // Comma separator before the cache group
                if !is_first_item {
                    out.prepend_static(prev_item_end, ", ");
                }

                if run_len == 1 {
                    // Single static child: _cache[N] || (_cache[N] = <child>)
                    let child = &children[run_start];
                    if child.kind == ChildKind::Text {
                        // Single static text: wrap in _createTextVNode inside cache
                        out.add_vdom_import(VdomHelper::CreateTextVNode);
                        out.prepend_fmt(
                            child.start,
                            format_args!(
                                "_cache[{cache_idx}] || (_cache[{cache_idx}] = _createTextVNode(\""
                            ),
                        );
                        out.prepend_static(child.end, "\"))");
                    } else {
                        out.prepend_fmt(
                            child.start,
                            format_args!("_cache[{cache_idx}] || (_cache[{cache_idx}] = "),
                        );
                        out.prepend_static(child.end, ")");
                    }
                } else {
                    // Multiple static children: ...(_cache[N] || (_cache[N] = [...]))
                    let first = &children[run_start];
                    let last = &children[run_end - 1];

                    out.prepend_fmt(
                        first.start,
                        format_args!("...(_cache[{cache_idx}] || (_cache[{cache_idx}] = ["),
                    );

                    // Emit inner items: separators + text wrapping within the cache group.
                    // All items are static, so no v-if/interpolation to handle.
                    self.emit_cache_group_inner(&children[run_start..run_end], out, source);

                    out.prepend_static(last.end, "]))");
                }

                prev_item_end = children[run_end - 1].end;
                is_first_item = false;
            } else if children[i].kind == ChildKind::Text
                || children[i].kind == ChildKind::Interpolation
            {
                // === Dynamic text run ===
                let run_start = i;
                let mut has_dynamic = children[i].kind == ChildKind::Interpolation;
                i += 1;
                while i < children.len()
                    && !static_flags[i]
                    && matches!(children[i].kind, ChildKind::Text | ChildKind::Interpolation)
                {
                    if children[i].kind == ChildKind::Interpolation {
                        has_dynamic = true;
                    }
                    i += 1;
                }
                let run_end = i;

                // Comma separator
                if !is_first_item {
                    out.prepend_static(prev_item_end, ", ");
                }

                // _createTextVNode( prefix
                out.add_vdom_import(VdomHelper::CreateTextVNode);
                let mut prefix = String::new();
                prefix.push_str("_createTextVNode(");
                if children[run_start].kind == ChildKind::Text {
                    prefix.push('"');
                } else {
                    prefix.push_str("_toDisplayString");
                    out.add_vdom_import(VdomHelper::ToDisplayString);
                }
                out.prepend_alloc(children[run_start].start, &prefix);

                // Inner separators within the text run
                for j in (run_start + 1)..run_end {
                    let sep = children::text_separator(children[j - 1].kind, children[j].kind);
                    if !sep.is_empty() {
                        out.prepend_static(children[j].start, sep);
                    }
                    if children[j].kind == ChildKind::Interpolation {
                        out.add_vdom_import(VdomHelper::ToDisplayString);
                    }
                }

                // Close: closing quote + patch flag + )
                let last = &children[run_end - 1];
                let mut suffix = String::new();
                if last.kind == ChildKind::Text {
                    suffix.push('"');
                }
                if has_dynamic {
                    if self.options.is_production {
                        suffix.push_str(", 1");
                    } else {
                        suffix.push_str(", 1 /* TEXT */");
                    }
                }
                suffix.push(')');
                out.prepend_alloc(last.end, &suffix);

                prev_item_end = last.end;
                is_first_item = false;
            } else if matches!(children[i].kind, ChildKind::StaticVNode { .. }) {
                // === Static VNode ===
                if !is_first_item {
                    out.prepend_static(prev_item_end, ", ");
                }
                children::emit_static_vnode(
                    &children[i],
                    source,
                    out,
                    &self.options,
                    self.ast,
                    el_children,
                );
                prev_item_end = children[i].end;
                i += 1;
                is_first_item = false;
            } else {
                // === Dynamic element ===
                let is_continuation =
                    children[i].condition == Some(ConditionChainRole::Continuation);
                let needs_comma = !is_first_item && !is_continuation;
                let has_prefix = children[i].condition_prefix.is_some();

                if needs_comma && has_prefix {
                    let cond = children[i].condition_prefix.as_ref().unwrap();
                    out.prepend_static(children[i].start, ", ");
                    children::emit_condition_prefix_mapped(out, children[i].start, cond);
                } else if needs_comma {
                    out.prepend_static(prev_item_end, ", ");
                } else if has_prefix {
                    let cond = children[i].condition_prefix.as_ref().unwrap();
                    children::emit_condition_prefix_mapped(out, children[i].start, cond);
                }

                prev_item_end = children[i].end;
                i += 1;
                if !is_continuation {
                    is_first_item = false;
                }
            }
        }
    }

    /// Emit separators and text wrapping for items WITHIN a cache group.
    ///
    /// All items are static (pure text or fully-static elements).
    /// No interpolation, v-if, or StaticVNode to handle.
    fn emit_cache_group_inner(
        &mut self,
        items: &[ChildRecord],
        out: &mut CodeGenOutput<'alloc>,
        _source: &'alloc str,
    ) {
        let mut is_first = true;
        let mut prev_end: u32 = 0;
        let mut i = 0;

        while i < items.len() {
            if items[i].kind == ChildKind::Text {
                // Text run within cache group (all static, no interpolation)
                let run_start = i;
                while i < items.len() && items[i].kind == ChildKind::Text {
                    i += 1;
                }
                let run_end = i;

                // Comma separator
                if !is_first {
                    out.prepend_static(prev_end, ", ");
                }

                // Wrap in _createTextVNode("...")
                out.add_vdom_import(VdomHelper::CreateTextVNode);
                out.prepend_static(items[run_start].start, "_createTextVNode(\"");

                // Inner separators for multi-text runs
                for j in (run_start + 1)..run_end {
                    let sep = children::text_separator(items[j - 1].kind, items[j].kind);
                    if !sep.is_empty() {
                        out.prepend_static(items[j].start, sep);
                    }
                }

                // Close: ")
                out.prepend_static(items[run_end - 1].end, "\")");

                prev_end = items[run_end - 1].end;
                is_first = false;
            } else {
                // Static element — already codegen'd, just add separator
                if !is_first {
                    out.prepend_static(prev_end, ", ");
                }
                prev_end = items[i].end;
                i += 1;
                is_first = false;
            }
        }
    }

    fn is_slot_child_static(&self, record: &ChildRecord, el_children: &[NodeId]) -> bool {
        match record.kind {
            ChildKind::Text => true,
            ChildKind::Interpolation => false,
            ChildKind::Element => {
                // Find the AST element node by position
                el_children.iter().any(|&nid| {
                    if let AstNodeKind::Element(ref el) = self.ast.nodes[nid.0].kind {
                        el.tag_open.start == record.start && el.is_fully_static
                    } else {
                        false
                    }
                })
            }
            _ => false,
        }
    }
}
