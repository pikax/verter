//! Vapor element handling: open, close, and block body building.

use crate::syntax_kai::{
    plugin::SyntaxPluginContext,
    plugins::code_gen::types::{
        TemplateCodeGenError, TemplateCodeGenResult, VaporImportDependencies,
    },
    types::{ElementKind, ElementScope, OxcCompiledElementClosed, OxcCompiledElementStart},
};

use super::helpers::{build_set_text_call, replace_node_ref};
use super::types::{
    VaporEffect, VaporElementKind, VaporElementState, VaporScopeKind, VaporSlotInfo, VaporTextPart,
};
use super::VaporTemplateGenerator;

impl<'alloc> VaporTemplateGenerator<'alloc> {
    pub(crate) fn handle_element_start(
        &mut self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
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

        // Check is_dynamic_component before is_component because
        // ElementKind::is_component() returns true for DynamicComponent too.
        state.kind = if is_dynamic_component {
            VaporElementKind::DynamicComponent {
                dynamic_is_expr: None,
                slot_children: Vec::new(),
            }
        } else if is_component {
            VaporElementKind::Component {
                component_var: String::new(),
                slot_children: Vec::new(),
                needs_vapor_ctx: false,
                slot_name: None,
                slot_params: None,
            }
        } else if is_slot_outlet {
            VaporElementKind::SlotOutlet {
                slot_name: None,
                slot_children: Vec::new(),
            }
        } else if is_template_element {
            VaporElementKind::TemplateWrapper {
                slot_name: None,
                slot_name_is_dynamic: false,
                slot_dynamic_name_expr: None,
                slot_params: None,
            }
        } else {
            VaporElementKind::Native
        };

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
        self.process_props(ev, ctx)?;

        // For slot outlets, extract static `name` attribute.
        if is_slot_outlet {
            let name = Self::find_static_attr_value("name", ev, ctx);
            if let Some(name) = name {
                let top = self
                    .stack
                    .last_mut()
                    .ok_or(TemplateCodeGenError::StackUnderflow(
                        "handle_element_start: stack empty after slot outlet name extraction",
                    ))?;
                if let VaporElementKind::SlotOutlet {
                    ref mut slot_name, ..
                } = top.kind
                {
                    *slot_name = Some(name);
                }
            }
        }

        // Void/self-closing elements: complete immediately.
        if open_tag_end.is_self_closing || open_tag.is_void_element {
            self.complete_element_close(None)?;
        }

        Ok(())
    }

    pub(crate) fn handle_element_closed(
        &mut self,
        ev: &OxcCompiledElementClosed,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
        self.complete_element_close(ev.event.event_close_tag.as_ref())
    }

    /// Shared logic for completing an element close (both normal and void/self-closing).
    pub(super) fn complete_element_close(
        &mut self,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) -> TemplateCodeGenResult {
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
            .ok_or(TemplateCodeGenError::StackUnderflow(
                "element close without matching open",
            ))?;

        let is_structural = state.scope.is_some();
        let is_component = state.is_component() || state.is_dynamic_component();
        let is_slot_outlet = state.is_slot_outlet();

        // Handle structural elements (v-if, v-for).
        if is_structural {
            self.complete_structural_element_close(&mut state, close_tag)?;
            return Ok(());
        }

        // Handle component elements.
        if is_component || is_slot_outlet {
            self.complete_component_element_close(&mut state, close_tag)?;
            return Ok(());
        }

        // Handle <template #name> children of components → collect as named slot.
        if state.is_template_element()
            && matches!(
                state.kind,
                VaporElementKind::TemplateWrapper {
                    slot_name: Some(_),
                    ..
                }
            )
        {
            self.complete_slot_template_close(&mut state, close_tag)?;
            return Ok(());
        }

        // Handle HTML content and close tag for native elements.
        self.flush_element_text_to_html(&mut state);

        if state.is_root {
            self.complete_root_element_close(&mut state, close_tag)?;
        } else {
            self.complete_non_root_element_close(&mut state, close_tag)?;
        }

        Ok(())
    }

    /// Build a `_setInsertionState(parentVar, null, childIndex, true)` call
    /// if this structural element is nested inside a native element.
    pub(super) fn build_insertion_state(
        &mut self,
        state: &VaporElementState,
    ) -> TemplateCodeGenResult<String> {
        // Only emit for non-root structural elements inside native elements.
        if state.is_root || self.stack.is_empty() {
            return Ok(String::new());
        }
        let parent = self
            .stack
            .last()
            .ok_or(TemplateCodeGenError::StackUnderflow(
                "build_insertion_state: stack must not be empty",
            ))?;
        // Don't emit for children of components (they become slots).
        if parent.is_component() || parent.is_dynamic_component() || parent.is_slot_outlet() {
            return Ok(String::new());
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
                .ok_or(TemplateCodeGenError::StackUnderflow(
                    "build_insertion_state: stack empty after ensure_ancestor_var_names",
                ))?
                .var_name
            {
                v.clone()
            } else {
                return Ok(String::new());
            }
        };

        self.imports
            .add(VaporImportDependencies::SET_INSERTION_STATE);
        Ok(format!(
            "  _setInsertionState({}, null, {}, true)\n",
            parent_var, state.child_index
        ))
    }

    /// Complete a component element close: build component call with slots.
    pub(super) fn complete_component_element_close(
        &mut self,
        state: &mut VaporElementState,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) -> TemplateCodeGenResult {
        let close_end = close_tag.map(|ct| ct.end).unwrap_or(state.open_tag_end);

        // Extract slot_params from Component kind (set by v-slot on the component itself).
        let component_slot_params = if let VaporElementKind::Component {
            ref mut slot_params,
            ..
        } = state.kind
        {
            slot_params.take()
        } else {
            None
        };

        // Collect default slot from children if any structural children exist.
        if !state.structural_children.is_empty() {
            let mut slot_body = String::new();
            for child in state.structural_children.drain(..) {
                slot_body.push_str(&child);
            }
            // Check if there's already a default slot.
            let has_default = state
                .kind
                .slot_children()
                .is_some_and(|sc| sc.iter().any(|s| s.name == "default"));
            if !has_default && !slot_body.is_empty() {
                state
                    .kind
                    .slot_children_mut()
                    .ok_or(TemplateCodeGenError::MissingScope(
                        "complete_component_element_close: kind must have slot_children",
                    ))?
                    .push(VaporSlotInfo {
                        name: "default".to_string(),
                        is_dynamic: false,
                        dynamic_name_expr: None,
                        params: component_slot_params,
                        body: slot_body,
                    });
            }
        }

        // Build the component call.
        let comp_code = self.build_component_call(state, "  ")?;
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
        Ok(())
    }

    /// Complete a native element child of a component: build as slot content block.
    pub(super) fn complete_component_child_close(
        &mut self,
        state: &mut VaporElementState,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) {
        let close_end = close_tag.map(|ct| ct.end).unwrap_or(state.open_tag_end);

        // Build a block body for this element (it becomes slot content).
        let body = self
            .build_block_body(state, close_tag, "    ")
            .unwrap_or_default();
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
    ) -> TemplateCodeGenResult {
        let close_end = close_tag.map(|ct| ct.end).unwrap_or(state.open_tag_end);
        let slot_name = if let VaporElementKind::TemplateWrapper {
            ref mut slot_name, ..
        } = state.kind
        {
            slot_name.take().unwrap_or_else(|| "default".to_string())
        } else {
            "default".to_string()
        };
        let (is_dynamic, dynamic_name_expr, slot_params) =
            if let VaporElementKind::TemplateWrapper {
                slot_name_is_dynamic,
                ref mut slot_dynamic_name_expr,
                ref mut slot_params,
                ..
            } = state.kind
            {
                (
                    slot_name_is_dynamic,
                    slot_dynamic_name_expr.take(),
                    slot_params.take(),
                )
            } else {
                (false, None, None)
            };

        // Build the slot body from structural children.
        let mut slot_body = String::new();
        for child in state.structural_children.drain(..) {
            slot_body.push_str(&child);
        }

        // If there are no structural children but there are text parts, build a block body.
        if slot_body.is_empty() {
            // Build a block body for the template content.
            let body = self
                .build_block_body(state, close_tag, "    ")
                .unwrap_or_default();
            slot_body = body;
        }

        // Add this slot to the parent component's slot_children.
        if let Some(parent) = self.stack.last_mut() {
            parent
                .kind
                .slot_children_mut()
                .ok_or(TemplateCodeGenError::MissingScope(
                    "complete_slot_template_close: parent kind must have slot_children",
                ))?
                .push(VaporSlotInfo {
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
        Ok(())
    }

    /// Complete a root element close: finalize HTML, emit template + navigation + effects.
    pub(super) fn complete_root_element_close(
        &mut self,
        state: &mut VaporElementState,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) -> TemplateCodeGenResult {
        let template_idx = self.register_template();

        // Build creation code: template instantiation + navigation + text creations.
        let mut creation = format!("  const n{} = t{}()\n", state.node_ref, template_idx);

        // Root's own text node creation (if root has dynamic text directly).
        if state.has_dynamic_children && !state.text_parts.is_empty() {
            let text_ref = state.text_node_ref.ok_or(TemplateCodeGenError::MissingArg(
                "root element with dynamic text must have text_node_ref",
            ))?;
            self.imports.add(VaporImportDependencies::TXT);
            creation.push_str(&format!(
                "  const x{} = _txt(n{})\n",
                text_ref, state.node_ref
            ));
        }

        // Append navigation instructions and text node creations from nested elements.
        self.drain_pending_instructions(&mut creation);

        // Build close code: effects + statements.
        let mut close_code = String::new();

        // Collect all effects: root's own + nested.
        let mut all_effects = std::mem::take(&mut state.effects);

        if state.has_dynamic_children && !state.text_parts.is_empty() {
            self.imports.add(VaporImportDependencies::SET_TEXT);
            let text_ref = state.text_node_ref.ok_or(TemplateCodeGenError::MissingArg(
                "root element with dynamic text must have text_node_ref for setText",
            ))?;
            let set_text = build_set_text_call(text_ref, &state.text_parts);
            all_effects.push(VaporEffect::Raw(set_text));
        }

        all_effects.append(&mut self.pending.nested_effects);

        if !all_effects.is_empty() {
            // Render all effects to code strings (no node_ref override for root).
            let rendered: Vec<String> =
                all_effects.iter().map(|e| e.to_code_string(None)).collect();
            self.emit_render_effect(&rendered, state.is_once, "  ", &mut close_code);
        }

        // Emit structural children (nested v-if/v-for blocks).
        for child in state.structural_children.drain(..) {
            close_code.push_str(&child);
        }

        // Collect all statements: root's own + nested.
        for stmt in &state.statements {
            close_code.push_str(&format!("  {}\n", stmt));
        }
        for stmt in self.pending.nested_statements.drain(..) {
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
        Ok(())
    }

    /// Complete a non-root element close: determine if navigation is needed,
    /// and push effects/statements to pending vectors for the root to emit.
    pub(super) fn complete_non_root_element_close(
        &mut self,
        state: &mut VaporElementState,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) -> TemplateCodeGenResult {
        // If parent is a component, build this element as a slot content block.
        let parent_is_component = self
            .stack
            .last()
            .map(|p| p.is_component() || p.is_dynamic_component())
            .unwrap_or(false);
        if parent_is_component {
            self.complete_component_child_close(state, close_tag);
            return Ok(());
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

            let nav = self.build_nav_instruction(&var_name, state.child_index)?;
            self.pending.nav.push(nav);

            state.var_name = Some(var_name);
        }

        // Push dynamic content to pending vectors.
        if has_own_dynamic {
            let var_name = state
                .var_name
                .as_ref()
                .ok_or(TemplateCodeGenError::StackUnderflow(
                    "non-root element with dynamic content must have var_name",
                ))?
                .clone();

            // Text node creation + setText effect.
            if state.has_dynamic_children && !state.text_parts.is_empty() {
                let text_ref = state.text_node_ref.ok_or(TemplateCodeGenError::MissingArg(
                    "non-root element with dynamic text must have text_node_ref",
                ))?;
                self.imports.add(VaporImportDependencies::TXT);
                self.pending
                    .text_creations
                    .push(format!("  const x{} = _txt({})", text_ref, var_name));

                self.imports.add(VaporImportDependencies::SET_TEXT);
                let set_text = build_set_text_call(text_ref, &state.text_parts);
                self.pending.nested_effects.push(VaporEffect::Raw(set_text));
            }

            // Effects and statements.
            for effect in state.effects.drain(..) {
                self.pending.nested_effects.push(effect);
            }
            for stmt in state.statements.drain(..) {
                self.pending.nested_statements.push(stmt);
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

        Ok(())
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
    ) -> TemplateCodeGenResult<String> {
        let mut body = String::new();

        // Allocate a new node_ref for the inner template node.
        // The outer `state.node_ref` is used for the structural directive result.
        let inner_ref = self.next_node_ref();

        if state.is_component() || state.is_dynamic_component() || state.is_slot_outlet() {
            // Component/slot outlet: build the component call.
            let comp_code = self.build_component_call(state, indent)?;
            body.push_str(&format!("{}const n{} = {}\n", indent, inner_ref, comp_code));
        } else if state.is_template_element() {
            // <template v-if/v-for>: children are the block body directly.
            // Structural children from nested v-if/v-for.
            for child in state.structural_children.drain(..) {
                body.push_str(&child);
                body.push('\n');
            }
            // For template wrappers, we don't create a template node.
            return Ok(body);
        } else {
            // Native element: finalize HTML and create template.
            self.flush_element_text_to_html(state);
            let template_idx = self.register_template();

            body.push_str(&format!(
                "{}const n{} = t{}()\n",
                indent, inner_ref, template_idx
            ));

            // Text node creation for dynamic text.
            if state.has_dynamic_children && !state.text_parts.is_empty() {
                let text_ref = state.text_node_ref.ok_or(TemplateCodeGenError::MissingArg(
                    "block body with dynamic text must have text_node_ref",
                ))?;
                self.imports.add(VaporImportDependencies::TXT);
                body.push_str(&format!(
                    "{}const x{} = _txt(n{})\n",
                    indent, text_ref, inner_ref
                ));
            }

            // Navigation instructions and text node creations from nested elements.
            self.drain_pending_instructions(&mut body);

            // Structural children (nested v-if/v-for inside this element).
            for child in state.structural_children.drain(..) {
                body.push_str(&child);
                body.push('\n');
            }
        }

        // Effects — render this element's own effects with inner_ref override, then
        // append nested child effects (from pending_nested_effects) WITHOUT override.
        // Own effects need the override because the structural directive's node_ref
        // differs from the inner template node ref. Nested child effects already have
        // their correct node_refs (from navigation) and must not be overridden.
        let own_effects = std::mem::take(&mut state.effects);
        let nested_effects = std::mem::take(&mut self.pending.nested_effects);

        // Render own effects with inner_ref override.
        let mut rendered: Vec<String> = own_effects
            .iter()
            .map(|e| e.to_code_string(Some(inner_ref)))
            .collect();

        // _setText for this element's text parts (Raw, no override needed).
        if state.has_dynamic_children && !state.text_parts.is_empty() {
            self.imports.add(VaporImportDependencies::SET_TEXT);
            let text_ref = state.text_node_ref.ok_or(TemplateCodeGenError::MissingArg(
                "block body with dynamic text must have text_node_ref for setText",
            ))?;
            let set_text = build_set_text_call(text_ref, &state.text_parts);
            rendered.push(set_text);
        }

        // Nested child effects — no override, they have their own node_refs.
        for e in &nested_effects {
            rendered.push(e.to_code_string(None));
        }

        self.emit_render_effect(&rendered, state.is_once, indent, &mut body);

        // Statements — rewrite node_ref references.
        // Uses whole-word boundary matching to avoid corrupting n1 inside n10, n11, etc.
        let mut all_stmts: Vec<String> = state
            .statements
            .drain(..)
            .map(|s| replace_node_ref(&s, state.node_ref, inner_ref))
            .collect();
        for stmt in self.pending.nested_statements.drain(..) {
            all_stmts.push(stmt);
        }
        for stmt in &all_stmts {
            body.push_str(&format!("{}{}\n", indent, stmt));
        }

        // Return statement.
        body.push_str(&format!("{}return n{}\n", indent, inner_ref));

        Ok(body)
    }

    pub(crate) fn handle_comment(
        &mut self,
        ev: &crate::syntax_kai::types::Comment,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
        let content = &ctx.input[ev.content.start as usize..ev.content.end as usize];
        self.append_html(&format!("<!--{}-->", content));

        let code_transform = &mut self.code_transform.borrow_mut();
        code_transform.overwrite(ev.start, ev.end, "");

        Ok(())
    }

    pub(crate) fn handle_text(
        &mut self,
        ev: &crate::syntax_kai::types::Text,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
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

        Ok(())
    }

    pub(crate) fn handle_interpolation(
        &mut self,
        ev: &crate::syntax_kai::types::OxcInterpolation<'alloc>,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
        let raw_content = &_ctx.input[ev.content.start as usize..ev.content.end as usize];
        let leading_ws = raw_content.len() - raw_content.trim_start().len();
        let trailing_ws = raw_content.len() - raw_content.trim_end().len();
        let trimmed_start = ev.content.start + leading_ws as u32;
        let trimmed_end = ev.content.end - trailing_ws as u32;
        let expr_text = &_ctx.input[trimmed_start as usize..trimmed_end as usize];
        let prefixed = self.prefix_expr(expr_text, trimmed_start, ev.bindings.as_ref());

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

        Ok(())
    }
}
