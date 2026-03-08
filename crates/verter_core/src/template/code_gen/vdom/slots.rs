//! Slot-related code generation for the VDOM backend.
//!
//! This module handles all slot processing: `<slot>` outlets (`_renderSlot`),
//! `<template v-slot:name>` bodies (`_withCtx`), component slot wrappers
//! (static `{ name: fn }` and dynamic `_createSlots()`), and implicit
//! default slots.

use rustc_hash::FxHashMap;

use crate::ast::types::{AstNodeKind, ElementNode, TagType};
use crate::template::oxc::types::OxcParsedElement;
use crate::types::NodeId;

use super::super::shared::helpers::{self, VdomHelper};
use super::super::types::{ChildKind, ChildRecord, CodeGenOutput, ConditionChainRole};
use super::{children, component, directives, element, VdomCodeGen};

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
    /// Returns "default" if no static `name` prop is found.
    pub(super) fn extract_slot_name<'s>(&self, element: &ElementNode, source: &'s str) -> &'s str {
        for prop in &element.props {
            if !prop.is_directive {
                let name = &source[prop.start as usize..prop.name_end as usize];
                if name == "name" {
                    if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                        return &source[vs as usize..ve as usize];
                    }
                }
            }
        }
        "default"
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
    pub(super) fn process_slot_outlet(
        &mut self,
        el: &ElementNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) -> ChildRecord {
        let slot_name = self.extract_slot_name(el, source);
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

        let mut buf = std::mem::take(&mut self.buf);
        buf.clear();

        if children.is_empty() {
            // No fallback: _renderSlot(_ctx.$slots, "name")
            buf.push_str("_renderSlot(_ctx.$slots, \"");
            buf.push_str(slot_name);
            buf.push_str("\")");
            out.overwrite(el.tag_open.start, tag_end, &buf);
        } else {
            // Has fallback: split into open/close overwrites so children
            // remain in place with their own overwrites.
            // Open: _renderSlot(_ctx.$slots, "name", {}, () => [
            buf.push_str("_renderSlot(_ctx.$slots, \"");
            buf.push_str(slot_name);
            buf.push_str("\", {}, () => [");
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
            condition_expr_start: None,
            condition_binding_prefix_len: 0,
        }
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

            // Add child separators
            children::add_children_separators_array(
                &children,
                out,
                &self.options,
                source,
                self.ast,
                el_children,
            );

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
            condition_expr_start: None,
            condition_binding_prefix_len: 0,
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
    pub(super) fn leave_component_with_slots(
        &mut self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        el_children: &[NodeId],
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
        is_block_root: bool,
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
            component::resolve_component_tag(tag_name, &self.resolver, out, &self.options.self_name)
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

        // Check if any slot children have v-if/v-else-if/v-else conditions
        let any_dynamic = children.iter().any(|c| c.condition.is_some());

        // Build slot name map: child start position -> slot name
        let mut slot_names: FxHashMap<u32, &str> = FxHashMap::default();
        for &child_id in el_children {
            let node = &self.ast.nodes[child_id.0];
            if let AstNodeKind::Element(ref child_el) = node.kind {
                if child_el.tag_type == TagType::Template && child_el.v_slot.is_some() {
                    let name = self.extract_v_slot_name(child_el, source);
                    slot_names.insert(child_el.tag_open.start, name);
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
        let needs_block_wrapper = is_block_root && (el.v_for.is_some() || el.v_condition.is_some());
        if needs_block_wrapper {
            buf.push_str("(_openBlock(), ");
            out.add_vdom_import(VdomHelper::OpenBlock);
        }

        buf.push_str(comp_helper.name());
        buf.push('(');
        buf.push_str(&resolved);

        // Props
        let dynamic_props = if has_props {
            buf.push_str(", ");
            let props_result = element::build_props_object_into(
                &mut buf,
                el,
                source,
                &self.resolver,
                oxc,
                skip_prop,
                self.options.force_js,
            );
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
            props_result.dynamic_props
        } else {
            if has_children {
                buf.push_str(", null");
            }
            Vec::new()
        };

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

        buf.clear();
        if has_children && any_dynamic {
            // Dynamic: close the _createSlots array and component call
            buf.push_str("]))");
        } else if has_children {
            // Static: close the slot object
            if self.options.is_production {
                buf.push_str(", _: 1}");
            } else {
                buf.push_str(", _: 1 /* STABLE */}");
            }
            // Add PatchFlags for components with dynamic props
            if !dynamic_props.is_empty() {
                buf.push_str(", ");
                let flag_str = helpers::format_patch_flag(
                    helpers::PATCH_PROPS,
                    self.options.is_production,
                    |s| out.alloc_str(s),
                );
                buf.push_str(flag_str);
                buf.push_str(", ");
                let props_ref = element::format_dynamic_props_ref(
                    &dynamic_props,
                    Some(&mut self.hoisted_constants),
                );
                buf.push_str(&props_ref);
            }
            buf.push(')');
        } else {
            buf.push(')');
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
        &self,
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
                    // 2. Inner text wrapping (adds _createTextVNode etc.)
                    children::add_children_separators_array(
                        group,
                        out,
                        &self.options,
                        source,
                        self.ast,
                        el_children,
                    );
                    // 3. Outer wrapper close LAST (appears after inner closings)
                    out.prepend_static(group.last().unwrap().end, "])");
                }
            }
        }
    }

    /// Emit dynamic slot format: each slot is `{ name: "x", fn: _withCtx(...) }`
    /// in an array, with ternary wrapping for conditional slots.
    fn emit_dynamic_slot_wrappers(
        &self,
        entries: &[SlotEntry],
        children: &[ChildRecord],
        slot_names: &FxHashMap<u32, &str>,
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

                    // Separator before this entry
                    if ei > 0 {
                        if is_continuation {
                            if let Some(ref prefix) = child.condition_prefix {
                                let sep = format!(" : {prefix}");
                                out.prepend_alloc(child.start, &sep);
                            } else {
                                out.prepend_static(child.start, " : ");
                            }
                        } else {
                            let prev_end = Self::slot_entry_end(&entries[ei - 1], children);
                            out.prepend_static(prev_end, ", ");
                        }
                    }

                    // Condition prefix for v-if start
                    if is_start {
                        if let Some(ref prefix) = child.condition_prefix {
                            out.prepend_alloc(child.start, prefix);
                        }
                    }

                    // Object wrapper: { name: "slot_name", fn:
                    let wrapper = format!("{{ name: \"{slot_name}\", fn: ");
                    out.prepend_alloc(child.start, &wrapper);

                    // Close object
                    out.prepend_static(child.end, " }");

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
                    // 2. Inner text wrapping
                    children::add_children_separators_array(
                        group,
                        out,
                        &self.options,
                        source,
                        self.ast,
                        el_children,
                    );
                    // 3. Outer wrapper close LAST
                    out.prepend_static(group.last().unwrap().end, "]) }");
                }
            }
        }
    }

    /// Process a component element with implicit default slot (non-slot children).
    /// Wraps children in `{ default: _withCtx(() => [...]), _: 1 }`.
    pub(super) fn leave_component_with_default_slot(
        &mut self,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        el_children: &[NodeId],
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
        is_block_root: bool,
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
            component::resolve_component_tag(tag_name, &self.resolver, out, &self.options.self_name)
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
        let needs_block_wrapper = is_block_root && (el.v_for.is_some() || el.v_condition.is_some());
        if needs_block_wrapper {
            buf.push_str("(_openBlock(), ");
            out.add_vdom_import(VdomHelper::OpenBlock);
        }

        buf.push_str(comp_helper.name());
        buf.push('(');
        buf.push_str(&resolved);

        let dynamic_props = if has_props {
            buf.push_str(", ");
            let props_result = element::build_props_object_into(
                &mut buf,
                el,
                source,
                &self.resolver,
                oxc,
                skip_prop,
                self.options.force_js,
            );
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
            props_result.dynamic_props
        } else {
            if has_children {
                buf.push_str(", null");
            }
            Vec::new()
        };

        let tag_end = el
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(el.tag_open.end);

        if !has_children {
            // All children were whitespace -- no meaningful content.
            // Overwrite the entire element [open_start, close_end) with the
            // component call (avoids leaving raw `</Component>` in output).
            buf.push(')');
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

        children::add_children_separators_array(
            &children,
            out,
            &self.options,
            source,
            self.ast,
            el_children,
        );

        buf.clear();
        buf.push_str("]), _: 1 /* STABLE */}");
        // Add PatchFlags for components with dynamic props
        if !dynamic_props.is_empty() {
            buf.push_str(", ");
            let flag_str =
                helpers::format_patch_flag(helpers::PATCH_PROPS, self.options.is_production, |s| {
                    out.alloc_str(s)
                });
            buf.push_str(flag_str);
            buf.push_str(", ");
            let props_ref = element::format_dynamic_props_ref(
                &dynamic_props,
                Some(&mut self.hoisted_constants),
            );
            buf.push_str(&props_ref);
        }
        buf.push(')');
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

    /// Check whether an element's AST children contain any `<template v-slot>` elements.
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
}
