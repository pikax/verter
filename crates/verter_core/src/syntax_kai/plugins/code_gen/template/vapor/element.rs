//! Vapor element handling: open, close, and block body building.

use crate::syntax_kai::{
    plugin::SyntaxPluginContext,
    plugins::code_gen::{
        template::shared::helper::build_prefixed_value, types::VaporImportDependencies,
    },
    types::{ElementKind, ElementScope, OxcCompiledElementClosed, OxcCompiledElementStart},
};

use super::helpers::{apply_var_mappings, build_set_text_call};
use super::types::{VaporElementState, VaporScopeKind, VaporSlotInfo, VaporTextPart};
use super::VaporTemplateGenerator;

impl<'alloc> VaporTemplateGenerator<'alloc> {
    pub(crate) fn handle_element_start(
        &mut self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let open_tag = &ev.event.event_open_tag;
        let open_tag_end = &ev.event.event_open_tag_end;

        let tag_name =
            ctx.input[(open_tag.start + 1) as usize..open_tag.name_end as usize].to_string();

        // Detect element kind.
        let kind = &open_tag.kind;
        let is_component = kind.is_component();
        let is_slot_outlet = *kind == ElementKind::SlotOutlet;
        let is_dynamic_component = *kind == ElementKind::DynamicComponent;
        let is_template_element = *kind == ElementKind::Template;

        // Detect structural directives from scopes.
        let is_vif_continuation = ev
            .scopes
            .iter()
            .any(|s| matches!(s, ElementScope::ElseIf(_) | ElementScope::Else(_)));
        let has_vif = ev.scopes.iter().any(|s| matches!(s, ElementScope::If(_)));
        let has_vfor = ev.scopes.iter().any(|s| matches!(s, ElementScope::For(_)));
        let is_structural = has_vif
            || is_vif_continuation
            || has_vfor
            || is_component
            || is_slot_outlet
            || is_dynamic_component;

        let is_root = self.stack.is_empty();

        // Flush any pending v-if chain if this element is NOT a continuation.
        if !is_vif_continuation && !is_root {
            self.flush_pending_vif_chain();
        }

        // If nested, handle parent state before this element.
        if !is_root {
            // Reset parent's text_child_started (element breaks text sequence).
            if let Some(parent) = self.stack.last_mut() {
                parent.text_child_started = false;
            }

            // Flush parent's pending text_parts to HTML (only for non-structural parents).
            if !is_structural {
                let parent_is_static = self
                    .stack
                    .last()
                    .map(|s| !s.has_dynamic_children)
                    .unwrap_or(false);
                if parent_is_static {
                    self.flush_text_parts_to_html();
                }
            }
        }

        let node_ref = self.next_node_ref();

        let mut state = VaporElementState::new(
            node_ref,
            tag_name.clone(),
            is_root,
            open_tag.is_void_element,
            open_tag_end.is_self_closing,
            open_tag.start,
            open_tag_end.end,
        );

        state.is_component = is_component;
        state.is_slot_outlet = is_slot_outlet;
        state.is_dynamic_component = is_dynamic_component;
        state.is_template_element = is_template_element;

        // Inherit v-for variable mappings from parent.
        if let Some(parent) = self.stack.last() {
            state.for_var_mappings = parent.for_var_mappings.clone();
        }

        // Process structural scopes (may add own v-for/v-slot mappings).
        self.process_scopes(ev, ctx, &mut state);

        // If this element has a v-for scope, add its variable mappings.
        if let Some(VaporScopeKind::For {
            ref callback_params,
            ref original_params,
            ..
        }) = state.scope
        {
            for (i, orig) in original_params.iter().enumerate() {
                if let Some(cb_param) = callback_params.get(i) {
                    state
                        .for_var_mappings
                        .push((orig.clone(), format!("{}.value", cb_param)));
                }
            }
        }

        // Detect built-in components and register component resolution.
        if is_component || is_dynamic_component {
            self.setup_component(&tag_name, &mut state, ev, ctx);
        }

        if is_root {
            // Root elements get var_name immediately.
            state.var_name = Some(format!("n{}", node_ref));
            // Start fresh HTML buffer.
            if !is_component && !is_slot_outlet && !is_dynamic_component {
                self.current_html.clear();
                self.pending_close_tags.clear();
            }
        } else if !is_vif_continuation {
            // Non-root, non-continuation: set child_index and increment parent's child_count.
            if let Some(parent) = self.stack.last_mut() {
                state.child_index = parent.child_count;
                // Structural children don't count as DOM children for navigation
                // (they're virtual nodes, not real DOM children).
                if !is_structural {
                    parent.child_count += 1;
                }
            }
        }

        // For structural elements, save the parent's HTML buffer and start fresh.
        // The structural element's HTML will be built into its own template.
        if is_structural && !is_component && !is_slot_outlet && !is_dynamic_component {
            // Save parent HTML state — will be restored when the structural element closes.
            // For now, just clear the buffer; structural elements get their own template.
            self.current_html.clear();
            self.pending_close_tags.clear();
        }

        // Build HTML for native elements (including those with structural directives).
        // Components, slot outlets, and dynamic components don't produce HTML.
        if !is_component && !is_slot_outlet && !is_dynamic_component && !is_template_element {
            let html_tag = self.build_static_open_tag(&tag_name, ev, ctx);
            self.append_html(&html_tag);
        }

        // Push state onto stack BEFORE processing props.
        self.stack.push(state);

        // Process props: classify static vs dynamic.
        // For slot outlets, we process props to extract name and slot props.
        self.process_props(ev, ctx);

        // For slot outlets, extract static `name` attribute.
        if is_slot_outlet {
            let name = Self::find_static_attr_value("name", ev, ctx);
            if let Some(name) = name {
                self.stack
                    .last_mut()
                    .expect("handle_element_start: stack empty after slot outlet name extraction")
                    .slot_name = Some(name);
            }
        }

        // Void/self-closing elements: complete immediately.
        if open_tag_end.is_self_closing || open_tag.is_void_element {
            self.complete_element_close(None);
        }
    }

    pub(crate) fn handle_element_closed(
        &mut self,
        ev: &OxcCompiledElementClosed,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.complete_element_close(ev.event.event_close_tag.as_ref());
    }

    /// Shared logic for completing an element close (both normal and void/self-closing).
    pub(super) fn complete_element_close(
        &mut self,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) {
        // Flush any pending v-if chain from children before closing this element,
        // BUT only if this element is not itself a v-else-if/v-else continuation
        // (those need to extend the chain, not flush it).
        let is_continuation = self
            .stack
            .last()
            .and_then(|s| s.scope.as_ref())
            .map(|s| matches!(s, VaporScopeKind::ElseIf { .. } | VaporScopeKind::Else))
            .unwrap_or(false);
        if !is_continuation {
            self.flush_pending_vif_chain();
        }

        let mut state = self
            .stack
            .pop()
            .expect("Element close without matching open");

        let is_structural = state.scope.is_some();
        let is_component = state.is_component || state.is_dynamic_component;
        let is_slot_outlet = state.is_slot_outlet;

        // Handle structural elements (v-if, v-for).
        if is_structural {
            self.complete_structural_element_close(&mut state, close_tag);
            return;
        }

        // Handle component elements.
        if is_component || is_slot_outlet {
            self.complete_component_element_close(&mut state, close_tag);
            return;
        }

        // Handle <template #name> children of components → collect as named slot.
        if state.is_template_element && state.slot_name.is_some() {
            self.complete_slot_template_close(&mut state, close_tag);
            return;
        }

        // Handle HTML content and close tag for native elements.
        if state.has_dynamic_children {
            self.append_html(" ");
        } else {
            let texts: Vec<String> = state
                .text_parts
                .drain(..)
                .filter_map(|p| match p {
                    VaporTextPart::Static(s) => Some(s),
                    VaporTextPart::Dynamic(_) => None,
                })
                .collect();
            for text in texts {
                self.append_html(&text);
            }
        }

        if !state.is_void && !state.is_self_closing {
            self.push_close_tag(&state.tag_name);
        }

        if state.is_root {
            self.complete_root_element_close(&mut state, close_tag);
        } else {
            self.complete_non_root_element_close(&mut state, close_tag);
        }
    }

    /// Build a `_setInsertionState(parentVar, null, childIndex, true)` call
    /// if this structural element is nested inside a native element.
    pub(super) fn build_insertion_state(&mut self, state: &VaporElementState) -> String {
        // Only emit for non-root structural elements inside native elements.
        if state.is_root || self.stack.is_empty() {
            return String::new();
        }
        let parent = self
            .stack
            .last()
            .expect("build_insertion_state: stack must not be empty");
        // Don't emit for children of components (they become slots).
        if parent.is_component || parent.is_dynamic_component || parent.is_slot_outlet {
            return String::new();
        }

        // Ensure parent has a var_name for navigation.
        let parent_var = if let Some(ref v) = parent.var_name {
            v.clone()
        } else if parent.is_root {
            format!("n{}", parent.node_ref)
        } else {
            // Non-root parent without var_name — need to ensure ancestors have var_names.
            self.ensure_ancestor_var_names();
            if let Some(ref v) = self
                .stack
                .last()
                .expect("build_insertion_state: stack empty after ensure_ancestor_var_names")
                .var_name
            {
                v.clone()
            } else {
                return String::new();
            }
        };

        self.imports
            .add(VaporImportDependencies::SET_INSERTION_STATE);
        format!(
            "  _setInsertionState({}, null, {}, true)\n",
            parent_var, state.child_index
        )
    }

    /// Complete a component element close: build component call with slots.
    pub(super) fn complete_component_element_close(
        &mut self,
        state: &mut VaporElementState,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) {
        let close_end = close_tag.map(|ct| ct.end).unwrap_or(state.open_tag_end);

        // Collect default slot from children if any structural children exist.
        if !state.structural_children.is_empty() {
            let mut slot_body = String::new();
            for child in state.structural_children.drain(..) {
                slot_body.push_str(&child);
            }
            // Check if there's already a default slot.
            let has_default = state.slot_children.iter().any(|s| s.name == "default");
            if !has_default && !slot_body.is_empty() {
                state.slot_children.push(VaporSlotInfo {
                    name: "default".to_string(),
                    is_dynamic: false,
                    dynamic_name_expr: None,
                    params: None,
                    body: slot_body,
                });
            }
        }

        // Build the component call.
        let comp_code = self.build_component_call(state, "  ");
        let node_ref = state.node_ref;
        let code = format!("  const n{} = {}\n", node_ref, comp_code);

        let is_root = self.stack.is_empty();
        if is_root {
            self.root_nodes.push(node_ref);
            let code_transform = &mut self.code_transform.borrow_mut();
            code_transform.overwrite(state.open_tag_start, close_end, &code);
        } else {
            if let Some(parent) = self.stack.last_mut() {
                parent.structural_children.push(code);
            }
            let code_transform = &mut self.code_transform.borrow_mut();
            code_transform.overwrite(state.open_tag_start, close_end, "");
        }
    }

    /// Complete a native element child of a component: build as slot content block.
    pub(super) fn complete_component_child_close(
        &mut self,
        state: &mut VaporElementState,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) {
        let close_end = close_tag.map(|ct| ct.end).unwrap_or(state.open_tag_end);

        // Build a block body for this element (it becomes slot content).
        let body = self.build_block_body(state, close_tag, "    ");
        let code = body;

        // Add to parent's structural_children (will be collected as default slot).
        if let Some(parent) = self.stack.last_mut() {
            parent.structural_children.push(code);
        }

        // Remove source from code_transform.
        let code_transform = &mut self.code_transform.borrow_mut();
        code_transform.overwrite(state.open_tag_start, close_end, "");
    }

    /// Complete a `<template #name>` close: collect as a named slot on the parent component.
    pub(super) fn complete_slot_template_close(
        &mut self,
        state: &mut VaporElementState,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) {
        let close_end = close_tag.map(|ct| ct.end).unwrap_or(state.open_tag_end);
        let slot_name = state
            .slot_name
            .take()
            .unwrap_or_else(|| "default".to_string());
        let is_dynamic = state.slot_name_is_dynamic;
        let dynamic_name_expr = state.slot_dynamic_name_expr.take();
        let slot_params = state.slot_params.take();

        // Build the slot body from structural children.
        let mut slot_body = String::new();
        for child in state.structural_children.drain(..) {
            slot_body.push_str(&child);
        }

        // If there are no structural children but there are text parts, build a block body.
        if slot_body.is_empty() {
            // Build a block body for the template content.
            let body = self.build_block_body(state, close_tag, "    ");
            slot_body = body;
        }

        // Add this slot to the parent component's slot_children.
        if let Some(parent) = self.stack.last_mut() {
            parent.slot_children.push(VaporSlotInfo {
                name: slot_name,
                is_dynamic,
                dynamic_name_expr,
                params: slot_params,
                body: slot_body,
            });
        }

        // Remove source from code_transform.
        let code_transform = &mut self.code_transform.borrow_mut();
        code_transform.overwrite(state.open_tag_start, close_end, "");
    }

    /// Complete a root element close: finalize HTML, emit template + navigation + effects.
    pub(super) fn complete_root_element_close(
        &mut self,
        state: &mut VaporElementState,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) {
        let html = self.finalize_html();
        let template_idx = self.templates.len() as u32;
        self.templates.push(html);
        self.imports.add(VaporImportDependencies::TEMPLATE);

        // Build creation code: template instantiation + navigation + text creations.
        let mut creation = format!("  const n{} = t{}()\n", state.node_ref, template_idx);

        // Root's own text node creation (if root has dynamic text directly).
        if state.has_dynamic_children && !state.text_parts.is_empty() {
            let text_ref = state
                .text_node_ref
                .expect("root element with dynamic text must have text_node_ref");
            self.imports.add(VaporImportDependencies::TXT);
            creation.push_str(&format!(
                "  const x{} = _txt(n{})\n",
                text_ref, state.node_ref
            ));
        }

        // Append navigation instructions from nested elements.
        for nav in self.pending_nav.drain(..) {
            creation.push_str(&nav);
            creation.push('\n');
        }

        // Append text node creations from nested elements.
        for tc in self.pending_text_creations.drain(..) {
            creation.push_str(&tc);
            creation.push('\n');
        }

        // Build close code: effects + statements.
        let mut close_code = String::new();

        // Collect all effects: root's own + nested.
        let mut all_effects = std::mem::take(&mut state.effects);

        if state.has_dynamic_children && !state.text_parts.is_empty() {
            self.imports.add(VaporImportDependencies::SET_TEXT);
            let text_ref = state
                .text_node_ref
                .expect("root element with dynamic text must have text_node_ref for setText");
            let set_text = build_set_text_call(text_ref, &state.text_parts);
            all_effects.push(set_text);
        }

        all_effects.append(&mut self.pending_nested_effects);

        if !all_effects.is_empty() {
            if state.is_once {
                // v-once: emit effects as direct statements (no _renderEffect wrapping).
                for effect in &all_effects {
                    close_code.push_str(&format!("  {}\n", effect));
                }
            } else {
                self.imports.add(VaporImportDependencies::RENDER_EFFECT);
                if all_effects.len() == 1 {
                    close_code.push_str(&format!("  _renderEffect(() => {})\n", all_effects[0]));
                } else {
                    close_code.push_str("  _renderEffect(() => {\n");
                    for effect in &all_effects {
                        close_code.push_str(&format!("    {}\n", effect));
                    }
                    close_code.push_str("  })\n");
                }
            }
        }

        // Emit structural children (nested v-if/v-for blocks).
        for child in state.structural_children.drain(..) {
            close_code.push_str(&child);
        }

        // Collect all statements: root's own + nested.
        for stmt in &state.statements {
            close_code.push_str(&format!("  {}\n", stmt));
        }
        for stmt in self.pending_nested_statements.drain(..) {
            close_code.push_str(&format!("  {}\n", stmt));
        }

        self.root_nodes.push(state.node_ref);

        // Now borrow code_transform and emit.
        let code_transform = &mut self.code_transform.borrow_mut();

        code_transform.overwrite(state.open_tag_start, state.open_tag_end, &creation);

        let close_start = if let Some(ct) = close_tag {
            ct.start
        } else {
            state.open_tag_end
        };

        if close_start > state.open_tag_end {
            code_transform.overwrite(state.open_tag_end, close_start, "");
        }

        if let Some(ct) = close_tag {
            code_transform.overwrite(ct.start, ct.end, &close_code);
        } else if !close_code.is_empty() {
            code_transform.append_right(state.open_tag_end, &close_code);
        }
    }

    /// Complete a non-root element close: determine if navigation is needed,
    /// and push effects/statements to pending vectors for the root to emit.
    pub(super) fn complete_non_root_element_close(
        &mut self,
        state: &mut VaporElementState,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) {
        // If parent is a component, build this element as a slot content block.
        let parent_is_component = self
            .stack
            .last()
            .map(|p| p.is_component || p.is_dynamic_component)
            .unwrap_or(false);
        if parent_is_component {
            self.complete_component_child_close(state, close_tag);
            return;
        }
        let has_own_dynamic = !state.effects.is_empty()
            || !state.statements.is_empty()
            || (state.has_dynamic_children && !state.text_parts.is_empty());

        let needs_nav = has_own_dynamic || state.needs_node_ref;

        // If navigation is needed and not yet generated, generate it now.
        if needs_nav && state.var_name.is_none() {
            self.ensure_ancestor_var_names();

            let var_name = if has_own_dynamic {
                format!("n{}", state.node_ref)
            } else {
                let p = self.next_path_ref();
                format!("p{}", p)
            };

            let nav = self.build_nav_instruction(&var_name, state.child_index);
            self.pending_nav.push(nav);

            state.var_name = Some(var_name);
        }

        // Push dynamic content to pending vectors.
        if has_own_dynamic {
            let var_name = state
                .var_name
                .as_ref()
                .expect("non-root element with dynamic content must have var_name")
                .clone();

            // Text node creation + setText effect.
            if state.has_dynamic_children && !state.text_parts.is_empty() {
                let text_ref = state
                    .text_node_ref
                    .expect("non-root element with dynamic text must have text_node_ref");
                self.imports.add(VaporImportDependencies::TXT);
                self.pending_text_creations
                    .push(format!("  const x{} = _txt({})", text_ref, var_name));

                self.imports.add(VaporImportDependencies::SET_TEXT);
                let set_text = build_set_text_call(text_ref, &state.text_parts);
                self.pending_nested_effects.push(set_text);
            }

            // Effects and statements.
            for effect in state.effects.drain(..) {
                self.pending_nested_effects.push(effect);
            }
            for stmt in state.statements.drain(..) {
                self.pending_nested_statements.push(stmt);
            }
        }

        // Pass structural children up to the parent.
        if !state.structural_children.is_empty() {
            if let Some(parent) = self.stack.last_mut() {
                for child in state.structural_children.drain(..) {
                    parent.structural_children.push(child);
                }
            }
        }

        // Remove source from code_transform.
        let code_transform = &mut self.code_transform.borrow_mut();
        code_transform.overwrite(state.open_tag_start, state.open_tag_end, "");
        if let Some(ct) = close_tag {
            code_transform.overwrite(ct.start, ct.end, "");
        }
    }

    /// Build a block body for a structural element (v-if branch, v-for iteration, slot).
    /// This generates the template instantiation, navigation, effects, and return statement
    /// as a string that can be used inside a block function.
    ///
    /// The `state.node_ref` is used for the outer structural directive result (e.g., `_createIf`).
    /// A new inner node_ref is allocated for the template instantiation inside the block.
    pub(super) fn build_block_body(
        &mut self,
        state: &mut VaporElementState,
        _close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
        indent: &str,
    ) -> String {
        let mut body = String::new();

        // Allocate a new node_ref for the inner template node.
        // The outer `state.node_ref` is used for the structural directive result.
        let inner_ref = self.next_node_ref();

        if state.is_component || state.is_dynamic_component || state.is_slot_outlet {
            // Component/slot outlet: build the component call.
            let comp_code = self.build_component_call(state, indent);
            body.push_str(&format!("{}const n{} = {}\n", indent, inner_ref, comp_code));
        } else if state.is_template_element {
            // <template v-if/v-for>: children are the block body directly.
            // Structural children from nested v-if/v-for.
            for child in state.structural_children.drain(..) {
                body.push_str(&child);
                body.push('\n');
            }
            // For template wrappers, we don't create a template node.
            return body;
        } else {
            // Native element: finalize HTML and create template.
            // Handle HTML content and close tag.
            if state.has_dynamic_children {
                self.append_html(" ");
            } else {
                let texts: Vec<String> = state
                    .text_parts
                    .drain(..)
                    .filter_map(|p| match p {
                        VaporTextPart::Static(s) => Some(s),
                        VaporTextPart::Dynamic(_) => None,
                    })
                    .collect();
                for text in texts {
                    self.append_html(&text);
                }
            }

            if !state.is_void && !state.is_self_closing {
                self.push_close_tag(&state.tag_name);
            }

            let html = self.finalize_html();
            let template_idx = self.templates.len() as u32;
            self.templates.push(html);
            self.imports.add(VaporImportDependencies::TEMPLATE);

            body.push_str(&format!(
                "{}const n{} = t{}()\n",
                indent, inner_ref, template_idx
            ));

            // Text node creation for dynamic text.
            if state.has_dynamic_children && !state.text_parts.is_empty() {
                let text_ref = state
                    .text_node_ref
                    .expect("block body with dynamic text must have text_node_ref");
                self.imports.add(VaporImportDependencies::TXT);
                body.push_str(&format!(
                    "{}const x{} = _txt(n{})\n",
                    indent, text_ref, inner_ref
                ));
            }

            // Navigation instructions from nested elements.
            for nav in self.pending_nav.drain(..) {
                body.push_str(&nav);
                body.push('\n');
            }

            // Text node creations from nested elements.
            for tc in self.pending_text_creations.drain(..) {
                body.push_str(&tc);
                body.push('\n');
            }

            // Structural children (nested v-if/v-for inside this element).
            for child in state.structural_children.drain(..) {
                body.push_str(&child);
                body.push('\n');
            }
        }

        // Effects — rewrite node_ref references from state.node_ref to inner_ref.
        let mut all_effects = std::mem::take(&mut state.effects);
        let old_ref = format!("n{}", state.node_ref);
        let new_ref = format!("n{}", inner_ref);
        for effect in &mut all_effects {
            *effect = effect.replace(&old_ref, &new_ref);
        }

        if state.has_dynamic_children && !state.text_parts.is_empty() {
            self.imports.add(VaporImportDependencies::SET_TEXT);
            let text_ref = state
                .text_node_ref
                .expect("block body with dynamic text must have text_node_ref for setText");
            let set_text = build_set_text_call(text_ref, &state.text_parts);
            all_effects.push(set_text);
        }
        all_effects.append(&mut self.pending_nested_effects);

        if !all_effects.is_empty() {
            if state.is_once {
                // v-once: emit effects as direct statements (no _renderEffect wrapping).
                for effect in &all_effects {
                    body.push_str(&format!("{}{}\n", indent, effect));
                }
            } else {
                self.imports.add(VaporImportDependencies::RENDER_EFFECT);
                if all_effects.len() == 1 {
                    body.push_str(&format!(
                        "{}_renderEffect(() => {})\n",
                        indent, all_effects[0]
                    ));
                } else {
                    body.push_str(&format!("{}_renderEffect(() => {{\n", indent));
                    for effect in &all_effects {
                        body.push_str(&format!("{}  {}\n", indent, effect));
                    }
                    body.push_str(&format!("{}}})\n", indent));
                }
            }
        }

        // Statements — rewrite node_ref references.
        let mut all_stmts: Vec<String> = state
            .statements
            .drain(..)
            .map(|s| s.replace(&old_ref, &new_ref))
            .collect();
        for stmt in self.pending_nested_statements.drain(..) {
            all_stmts.push(stmt);
        }
        for stmt in &all_stmts {
            body.push_str(&format!("{}{}\n", indent, stmt));
        }

        // Return statement.
        body.push_str(&format!("{}return n{}\n", indent, inner_ref));

        body
    }

    pub(crate) fn handle_comment(
        &mut self,
        ev: &crate::syntax_kai::types::Comment,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let content = &ctx.input[ev.content.start as usize..ev.content.end as usize];
        self.append_html(&format!("<!--{}-->", content));

        let code_transform = &mut self.code_transform.borrow_mut();
        code_transform.overwrite(ev.start, ev.end, "");
    }

    pub(crate) fn handle_text(
        &mut self,
        ev: &crate::syntax_kai::types::Text,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let text = &ctx.input[ev.start as usize..ev.end as usize];

        if let Some(state) = self.stack.last_mut() {
            state
                .text_parts
                .push(VaporTextPart::Static(text.to_string()));

            // Track child_count: consecutive text/interpolation = one DOM child.
            if !state.text_child_started {
                state.child_count += 1;
                state.text_child_started = true;
            }
        }

        let code_transform = &mut self.code_transform.borrow_mut();
        code_transform.overwrite(ev.start, ev.end, "");
    }

    pub(crate) fn handle_interpolation(
        &mut self,
        ev: &crate::syntax_kai::types::OxcInterpolation<'alloc>,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let raw_content = &_ctx.input[ev.content.start as usize..ev.content.end as usize];
        let leading_ws = raw_content.len() - raw_content.trim_start().len();
        let trailing_ws = raw_content.len() - raw_content.trim_end().len();
        let trimmed_start = ev.content.start + leading_ws as u32;
        let trimmed_end = ev.content.end - trailing_ws as u32;
        let expr_text = &_ctx.input[trimmed_start as usize..trimmed_end as usize];
        let mut prefixed = build_prefixed_value(
            expr_text,
            trimmed_start,
            &ev.bindings,
            &self.bindings,
            self.is_production,
        );

        // Apply v-for / v-slot variable mappings (e.g., `item` → `_for_item0.value`).
        if let Some(state) = self.stack.last() {
            prefixed = apply_var_mappings(&prefixed, &state.for_var_mappings);
        }

        self.imports.add(VaporImportDependencies::TO_DISPLAY_STRING);
        let display_expr = format!("_toDisplayString({})", prefixed);

        let needs_text_ref = if let Some(state) = self.stack.last_mut() {
            state.has_dynamic_children = true;
            state.text_parts.push(VaporTextPart::Dynamic(display_expr));

            // Track child_count: consecutive text/interpolation = one DOM child.
            if !state.text_child_started {
                state.child_count += 1;
                state.text_child_started = true;
            }

            state.text_node_ref.is_none()
        } else {
            false
        };

        if needs_text_ref {
            let text_ref = self.next_text_node_ref();
            if let Some(state) = self.stack.last_mut() {
                state.text_node_ref = Some(text_ref);
            }
        }

        let code_transform = &mut self.code_transform.borrow_mut();
        code_transform.overwrite(ev.start, ev.end, "");
    }
}
