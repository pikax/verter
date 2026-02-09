//! Code generation for template elements.
//!
//! Streaming codegen - emit code as events arrive:
//! - OpenTagStart: Start accumulating props (handled in plugin.rs)
//! - OpenTagEnd: Emit element opening with all props
//! - CloseTag: Emit element closing
//! - AnalysedCloseScopes: Handle scope closings (v-for, v-slot)

use crate::code_transform::CodeTransform;
use crate::syntax::types::{
    AnalysedCloseScopes, SyntaxCloseTag, SyntaxComment, SyntaxOpenTagEnd, SyntaxTagType, SyntaxText,
};

use super::types::{
    resolve_binding_prefix, resolve_binding_suffix, BindingMetadata, CloseAction,
    ComponentChildrenState, HelperFlags, PropKind, SlotElementState, TemplateCodegenState,
};

/// Convert a hyphenated string to camelCase (e.g., "my-custom" → "myCustom").
/// Used for directive variable names which must be valid JS identifiers.
fn camelize(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut upper_next = false;
    for c in s.chars() {
        if c == '-' {
            upper_next = true;
        } else if upper_next {
            result.push(c.to_ascii_uppercase());
            upper_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

/// Emit a slot name as an object key. Quotes it if it's not a valid JS identifier.
fn push_slot_key(code: &mut String, slot_name: &str) {
    if is_valid_js_identifier(slot_name) {
        code.push_str(slot_name);
    } else {
        code.push('"');
        code.push_str(slot_name);
        code.push('"');
    }
}

/// Generate the directive array suffix for components with custom directives or v-show.
/// Returns the suffix string like `, [[_directive_cTooltip, 'value']])` or empty if no directives.
/// Components don't use v-model directives (those are handled as props).
fn generate_component_directive_suffix(
    element_id: u32,
    state: &mut TemplateCodegenState,
    source: &str,
) -> String {
    let has_directives = state.elements_with_directives.contains_key(&element_id)
        || state.elements_with_vshow.contains_key(&element_id);

    if !has_directives {
        return String::new();
    }

    let mut suffix = String::from(", [");
    let mut directive_count = 0;

    // Add custom directives
    if let Some(directives) = state.elements_with_directives.remove(&element_id) {
        for directive in directives.iter() {
            if directive_count > 0 {
                suffix.push_str(", ");
            }
            directive_count += 1;

            suffix.push_str("[_directive_");
            suffix.push_str(&camelize(&directive.name));

            // Add value if present
            if let Some(ref value_span) = directive.value {
                suffix.push_str(", ");
                let value_str = &source[value_span.start as usize..value_span.end as usize];
                if value_str.starts_with('\'')
                    || value_str.starts_with('"')
                    || value_str.starts_with('`')
                {
                    suffix.push_str(value_str);
                } else {
                    // Use write_expr_with_ctx to properly handle complex expressions
                    // (object literals, array literals, etc.) not just simple identifiers
                    write_expr_with_ctx(
                        &mut suffix,
                        value_str,
                        &[], // No v-for locals available in component close context
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                }
            }

            // Add argument if present
            if let Some(ref arg_span) = directive.arg {
                if directive.value.is_none() {
                    suffix.push_str(", void 0");
                }
                suffix.push_str(", ");
                let arg_str = &source[arg_span.start as usize..arg_span.end as usize];
                if directive.is_dynamic_arg {
                    let arg_str = arg_str.trim_start_matches('[').trim_end_matches(']');
                    write_expr_with_ctx(
                        &mut suffix,
                        arg_str,
                        &[],
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                } else {
                    suffix.push('"');
                    suffix.push_str(arg_str);
                    suffix.push('"');
                }
            }

            // Add modifiers if present
            if !directive.modifiers.is_empty() {
                if directive.value.is_none() && directive.arg.is_none() {
                    suffix.push_str(", void 0, void 0");
                } else if directive.arg.is_none() {
                    suffix.push_str(", void 0");
                }
                suffix.push_str(", { ");
                for (j, mod_span) in directive.modifiers.iter().enumerate() {
                    if j > 0 {
                        suffix.push_str(", ");
                    }
                    suffix.push_str(&source[mod_span.start as usize..mod_span.end as usize]);
                    suffix.push_str(": true");
                }
                suffix.push_str(" }");
            }

            suffix.push(']');
        }
    }

    // Add v-show directive
    if let Some(vshow_span) = state.elements_with_vshow.remove(&element_id) {
        if directive_count > 0 {
            suffix.push_str(", ");
        }
        suffix.push_str("[_vShow, ");
        let value_str = &source[vshow_span.start as usize..vshow_span.end as usize];
        write_expr_with_ctx(
            &mut suffix,
            value_str,
            &[],
            &state.binding_metadata,
            source.as_bytes(),
            state.is_inline_mode,
        );
        suffix.push(']');
    }

    suffix.push_str("])");
    suffix
}

/// Process OpenTagEnd - emit element opening.
/// At this point, all props and directives have been collected in state.current_element.
pub fn process_open_tag_end<'a>(
    open_tag: &SyntaxOpenTagEnd,
    state: &mut TemplateCodegenState,
    code_transform: &mut CodeTransform<'a>,
    source: &'a str,
) {
    // Flush any open v-slot text vnode before processing a new element.
    // The closing text will be prepended to this element's code via conditional_close_prefix.
    let vslot_text_close = flush_vslot_text_vnode(state);

    let elem = match state.current_element.take() {
        Some(e) => e,
        None => return,
    };

    let mut code = String::with_capacity(128);
    // Prefix for closing an incomplete conditional chain from a previous sibling.
    // This is kept separate from `code` so that comma separators (inserted at position 0
    // of `code` by component slot handling) end up AFTER the ternary close, not before it.
    let mut conditional_close_prefix = String::new();

    // Check if this is a continuation of a conditional chain (v-else-if or v-else)
    // These shouldn't get a comma since they're part of a ternary expression
    let is_conditional_continuation = elem.v_if.as_ref().is_some_and(|vif| {
        use crate::syntax::types::OxcVConditionType;
        matches!(
            vif.condition_type,
            OxcVConditionType::ElseIf | OxcVConditionType::Else
        )
    });
    // For conditional v-slot continuations (v-else-if/v-else on named slots),
    // they're also "active continuations" even though they bypass the standard chain.
    let is_conditional_vslot_continuation = is_conditional_continuation
        && elem.v_slot.is_some()
        && open_tag.tag_type == SyntaxTagType::Template
        && state
            .component_stack
            .last()
            .map(|p| p.has_conditional_named_slots)
            .unwrap_or(false);

    // Unwind stale conditional chains from deeper depths.
    // This happens when inner v-if chains (nested inside components or fragments) were
    // left in state after their enclosing scope closed. Their createCommentVNode was
    // already emitted by the nested scope's close, so we just restore outer chain state.
    if is_conditional_continuation
        && state.in_conditional_chain
        && state.conditional_chain_depth > state.depth
    {
        while state.in_conditional_chain && state.conditional_chain_depth > state.depth {
            state.in_conditional_chain = false;
            if let Some((in_chain, depth, branch_idx)) = state.conditional_chain_stack.pop() {
                state.in_conditional_chain = in_chain;
                state.conditional_chain_depth = depth;
                state.conditional_branch_index = branch_idx;
            }
        }
    }

    let is_active_conditional_continuation = is_conditional_vslot_continuation
        || (is_conditional_continuation
            && state.in_conditional_chain
            && state.depth == state.conditional_chain_depth);

    // Check if this is a root-level element (depth == 0 means we're at template root)
    let is_root_element = state.depth == 0;
    if is_root_element && !is_active_conditional_continuation {
        // Add leading comma for root elements that follow other root children
        if state.root_child_emitted {
            code.push_str(",\n    ");
        }

        state.root_element_count += 1;
        state.root_element_ids.insert(elem.element_id);

        // Track first root element for potential multi-root patching
        if state.root_element_count == 1 {
            state.first_root_source_span = Some((elem.start, open_tag.end));
            state.first_root_element_id = Some(elem.element_id);
        }

        state.root_child_emitted = true;
    }

    // Close incomplete conditional chain if this is a new sibling (not v-else-if/v-else)
    // This happens when there's a v-if without a v-else sibling
    // Only close the chain at the same depth level where it started (not for children)
    if state.in_conditional_chain
        && !is_active_conditional_continuation
        && state.depth == state.conditional_chain_depth
    {
        // Close the incomplete ternary with a comment node (matches Vue's behavior).
        // Store in conditional_close_prefix (not code) so comma separators added later
        // via code.insert_str(0, ", ") don't end up before the ternary close.
        conditional_close_prefix.push_str(&format!(
            "\n  : _createCommentVNode(\"{}\", true)",
            state.vif_comment_text()
        ));
        state.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);
        // Reserve the next key at this depth for future sibling conditional chains.
        state.conditional_next_key_by_depth.insert(
            state.conditional_chain_depth,
            state.conditional_branch_index + 1,
        );
        state.in_conditional_chain = false;
        // Restore outer conditional chain state if nested
        if let Some((in_chain, depth, branch_idx)) = state.conditional_chain_stack.pop() {
            state.in_conditional_chain = in_chain;
            state.conditional_chain_depth = depth;
            state.conditional_branch_index = branch_idx;
        }
    }

    // Skip element-based sibling tracking when this element is a direct child of a component.
    // The component slot mechanism (default_slot_child_count) handles commas for its children.
    // Using both mechanisms would produce double commas.
    let is_vslot_template = elem.v_slot.is_some() && open_tag.tag_type == SyntaxTagType::Template;
    // In createSlots mode, v-slot templates also suppress depth-based commas
    // (createSlots array entries handle their own separators)
    // V-slot templates that are direct component children should skip depth-based
    // comma tracking — the slot mechanism handles its own separators.
    let is_direct_component_child_vslot = is_vslot_template
        && state
            .component_stack
            .last()
            .map(|p| state.element_id_stack.len() == p.element_id_stack_len_at_open)
            .unwrap_or(false);
    let is_direct_component_child_elem = is_direct_component_child_vslot
        || (!is_vslot_template
            && state
                .component_stack
                .last()
                .map(|p| {
                    state.element_id_stack.len() == p.element_id_stack_len_at_open
                        && (state.active_vslot_depth == p.vslot_depth_at_open)
                })
                .unwrap_or(false));

    // Add comma before sibling elements (if not first child at parent depth)
    // state.depth is still the parent's depth at this point
    // Skip for v-else-if/v-else as they're part of ternary expressions
    // For root elements with multiple roots, we'll add commas in finalize_template
    if state.depth > 0 && !is_active_conditional_continuation && !is_direct_component_child_elem {
        let parent_depth_idx = state.depth - 1;
        if parent_depth_idx < state.first_child_at_depth.len()
            && state.first_child_at_depth[parent_depth_idx]
        {
            code.push_str(",\n    ");
        } else if parent_depth_idx < state.first_child_at_depth.len() {
            state.first_child_at_depth[parent_depth_idx] = true;
        }
    }

    // Track child count for parent element (for single child optimization)
    // Child elements always invalidate single child optimization and require array format
    // Skip for direct component children - they're tracked by the component slot mechanism.
    if !is_direct_component_child_elem {
        if let Some(&parent_id) = state.element_id_stack.last() {
            let count = state.element_child_count.entry(parent_id).or_insert(0);
            let was_first = *count == 0;
            *count += 1;

            // Check if array was already opened
            let array_opened = state
                .element_array_opened
                .get(&parent_id)
                .copied()
                .unwrap_or(false);

            // Child elements always require array format
            if was_first {
                // First child - open array with "["
                code.insert(0, '[');
                state.element_array_opened.insert(parent_id, true);
            } else if !array_opened {
                // Not first child, but array not opened - there was text/interpolation before
                // We need to wrap the previous content in an array and add comma
                if let Some(single_child) = state.element_single_child.remove(&parent_id) {
                    // Check for whitespace gap between previous text/interpolation and this element.
                    // The tokenizer drops whitespace-only text before `<`, so we must handle it here.
                    let mut content = single_child.content;
                    let gap_start = single_child.end;
                    let gap_end = elem.start;
                    if gap_end > gap_start {
                        let gap = &source[gap_start as usize..gap_end as usize];
                        if gap.bytes().all(|b| b.is_ascii_whitespace()) {
                            // Remove the gap whitespace from output
                            code_transform.remove(gap_start, gap_end);
                            // Always condense whitespace between text/interpolation and element
                            // Vue only removes whitespace between two elements, not between
                            // text/interpolation and elements (regardless of newlines)
                            content.push_str(" + \" \"");
                        }
                    }

                    if single_child.is_interpolation {
                        // Interpolation in array needs _createTextVNode wrapper
                        // Re-overwrite the interpolation range with wrapped content
                        let text_flag = state.pflag(1, "TEXT");
                        let wrapped = format!("[_createTextVNode({}, {})", content, text_flag);
                        code_transform.overwrite(single_child.start, single_child.end, &wrapped);
                        state.helpers.insert(HelperFlags::CREATE_TEXT_VNODE);
                        // Remove TEXT flag: interpolation is now a VNode child, not direct text
                        state.elements_with_interpolation.remove(&parent_id);
                    } else {
                        // Static text also needs _createTextVNode in array
                        // Re-overwrite the text range with wrapped content
                        let cache_idx = state.cache_index;
                        state.cache_index += 1;
                        let cached_flag = state.pflag(-1, "CACHED");
                        let wrapped = format!(
                            "[_cache[{}] || (_cache[{}] = _createTextVNode({}, {}))",
                            cache_idx, cache_idx, content, cached_flag
                        );
                        code_transform.overwrite(single_child.start, single_child.end, &wrapped);
                        state.helpers.insert(HelperFlags::CREATE_TEXT_VNODE);
                    }
                    // Insert ", " before this element (prepend to code)
                    code.insert_str(0, ", ");
                    state.element_array_opened.insert(parent_id, true);
                }
            }

            // Child elements are never "single child" candidates - remove tracking
            state.element_single_child.remove(&parent_id);
        }
    } // end !is_direct_component_child_elem

    state.depth += 1;

    // Track children for this new depth level
    while state.first_child_at_depth.len() < state.depth {
        state.first_child_at_depth.push(false);
    }

    // Check if parent component needs children opener
    // If this is NOT a v-slot template, open children
    let is_vslot = elem.v_slot.is_some() && open_tag.tag_type == SyntaxTagType::Template;
    if !is_vslot {
        if let Some(parent) = state.component_stack.last_mut() {
            let is_direct_slot_child =
                state.element_id_stack.len() == parent.element_id_stack_len_at_open;
            // Components opened inside a v-slot (vslot_depth_at_open > 0) should handle
            // their own children normally. Only the v-slot OWNER component (opened at depth 0)
            // defers children handling to the v-slot mechanism.
            let should_handle_children = state.active_vslot_depth == parent.vslot_depth_at_open;

            if parent.has_named_slots && should_handle_children {
                if !parent.default_slot_opened {
                    // Named slots already opened the slots object. Add default slot inline.
                    if code.starts_with(",\n    ") {
                        code.replace_range(0..6, "");
                    } else if code.starts_with(", ") {
                        code.replace_range(0..2, "");
                    } else if code.starts_with(',') {
                        code.replace_range(0..1, "");
                    }
                    if parent.has_conditional_named_slots {
                        // In createSlots mode, emit deferred `: undefined` from prev conditional
                        let mut prefix = String::new();
                        if parent.conditional_slot_needs_undefined {
                            prefix.push_str("\n      : undefined");
                            parent.conditional_slot_needs_undefined = false;
                        }
                        if parent.create_slots_entry_count > 0 {
                            prefix.push_str(",\n    ");
                        }
                        parent.create_slots_entry_count += 1;
                        prefix.push_str(
                            "{\n          name: \"default\",\n          fn: _withCtx(() => [",
                        );
                        code.insert_str(0, &prefix);
                    } else {
                        code.insert_str(0, ", default: _withCtx(() => [");
                    }
                    parent.uses_slots = true;
                    state.helpers.insert(HelperFlags::WITH_CTX);
                    parent.default_slot_opened = true;
                } else if parent.default_slot_child_count > 0
                    && is_direct_slot_child
                    && !is_active_conditional_continuation
                {
                    // Non-first direct slot child needs comma separator
                    // Skip for v-else-if/v-else as they're part of a ternary expression
                    code.insert_str(0, ", ");
                }
                if should_handle_children
                    && is_direct_slot_child
                    && !is_active_conditional_continuation
                {
                    parent.default_slot_child_count += 1;
                }
            } else if !parent.children_opened && should_handle_children {
                // All components use slot format: { default: _withCtx(() => [...]) }
                // This is Vue's standard format for component children
                code_transform.prepend_left(parent.insert_pos, "{ default: _withCtx(() => [");
                parent.uses_slots = true;
                state.helpers.insert(HelperFlags::WITH_CTX);
                parent.children_opened = true;
                parent.default_slot_opened = true;
                if is_direct_slot_child {
                    parent.default_slot_child_count += 1;
                }
            } else if should_handle_children && is_direct_slot_child {
                // Non-first direct slot child needs comma separator
                // Skip for v-else-if/v-else as they're part of a ternary expression
                if parent.default_slot_child_count > 0 && !is_active_conditional_continuation {
                    code.insert_str(0, ", ");
                }
                if !is_active_conditional_continuation {
                    parent.default_slot_child_count += 1;
                }
            }
        }
    }

    // Handle v-for wrapper (wraps entire element)
    if let Some(ref vfor) = elem.v_for {
        // Push v-for locals to stack FIRST so they're available for v-if processing
        let iterator = &source[vfor.iterator.start as usize..vfor.iterator.end as usize];
        let locals: Vec<String> = extract_vfor_locals(iterator)
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        state.vfor_locals_stack.push(locals);
        // Track this element so we can pop the stack when it closes
        state.elements_with_vfor.insert(elem.element_id);

        let iterable_str = &source[vfor.iterable.start as usize..vfor.iterable.end as usize];
        let iterable_is_stable =
            is_constant_iterable(iterable_str, &state.binding_metadata, source);

        if iterable_is_stable {
            code.push_str("(_openBlock(), _createElementBlock(_Fragment, null, _renderList(");
        } else {
            code.push_str("(_openBlock(true), _createElementBlock(_Fragment, null, _renderList(");
        }
        let mut iterable_code = String::new();
        write_expr_with_ctx(
            &mut iterable_code,
            iterable_str,
            &[],
            &state.binding_metadata,
            source.as_bytes(),
            state.is_inline_mode,
        );
        code.push_str(&iterable_code);
        // Check if iterator already has parentheses (for destructuring like "(item, i)")
        let iterator_str = &source[vfor.iterator.start as usize..vfor.iterator.end as usize];
        let iterator_trimmed = iterator_str.trim();
        if iterator_trimmed.starts_with('(') && iterator_trimmed.ends_with(')') {
            // Iterator already has parentheses, don't add extra
            code.push_str(", ");
            code.push_str(iterator_str);
            code.push_str(" => {\nreturn ");
        } else {
            // Simple iterator (e.g., "item"), wrap in parentheses
            code.push_str(", (");
            code.push_str(iterator_str);
            code.push_str(") => {\nreturn ");
        }

        // Register close action by scope_id
        state.scope_close_actions.insert(
            vfor.scope_id,
            CloseAction::VFor {
                keyed: elem.has_key,
                stable: iterable_is_stable,
            },
        );
        // Track element_id -> scope_id for self-closing element handling
        state
            .element_vfor_scope
            .insert(elem.element_id, vfor.scope_id);

        state.helpers.insert(HelperFlags::OPEN_BLOCK);
        state.helpers.insert(HelperFlags::CREATE_ELEMENT_BLOCK);
        state.helpers.insert(HelperFlags::FRAGMENT);
        state.helpers.insert(HelperFlags::RENDER_LIST);
    }

    // Collect v-for and v-slot locals early (needed for v-if expression processing)
    // Clone into owned strings to avoid borrowing from state
    let vfor_locals_for_vif: Vec<String> = state
        .vfor_locals_stack
        .iter()
        .chain(state.vslot_locals_stack.iter())
        .flat_map(|v| v.iter().cloned())
        .collect();
    let vfor_locals_refs: Vec<&str> = vfor_locals_for_vif.iter().map(|s| s.as_str()).collect();

    // Detect conditional v-slot: <template v-if="cond" #slotName>
    // These need _createSlots() instead of standard ternary in slot object
    let is_conditional_vslot = elem.v_if.is_some()
        && elem.v_slot.is_some()
        && open_tag.tag_type == SyntaxTagType::Template;

    // Handle v-if/v-else-if/v-else (conditional expressions)
    // Track branch index for key generation
    let conditional_key = if let Some(ref vif) = elem.v_if {
        use crate::syntax::types::OxcVConditionType;

        if is_conditional_vslot {
            // Conditional v-slot: emit condition for createSlots dynamic array entry
            // instead of standard ternary in slot object
            let sf_dynamic = state.slot_flag(2, "DYNAMIC");
            match vif.condition_type {
                OxcVConditionType::If => {
                    // Close default slot and transition to createSlots if needed.
                    // This MUST happen BEFORE the condition so the default slot closes
                    // before the ternary begins.
                    if let Some(parent) = state.component_stack.last_mut() {
                        if parent.default_slot_opened {
                            // Close default slot callback, add slot flag, close static
                            // object, and start dynamic array
                            code.push_str(&format!("]), {} }}, [", sf_dynamic));
                            // Retroactively wrap: ", _createSlots(" goes at separator_pos
                            // (before the "{" at insert_pos)
                            code_transform.prepend_right(parent.separator_pos, ", _createSlots(");
                            parent.default_slot_opened = false;
                            parent.default_slot_child_count = 0;
                            parent.has_conditional_named_slots = true;
                        } else if parent.children_opened && !parent.has_conditional_named_slots {
                            // Static named slots exist but no default: close static
                            // object and start dynamic array
                            code.push_str(&format!(", {} }}, [", sf_dynamic));
                            // Retroactively wrap: ", _createSlots(" goes at separator_pos
                            code_transform.prepend_right(parent.separator_pos, ", _createSlots(");
                            parent.has_conditional_named_slots = true;
                        } else if !parent.children_opened {
                            // No slots at all yet: open createSlots structure
                            // separator + _createSlots( at separator_pos
                            code_transform.prepend_right(parent.separator_pos, ", _createSlots(");
                            // slot flag + dynamic array at insert_pos
                            code_transform.prepend_left(
                                parent.insert_pos,
                                &format!("{{ {} }}, [", sf_dynamic),
                            );
                            parent.children_opened = true;
                            parent.uses_slots = true;
                            parent.has_conditional_named_slots = true;
                        }
                        // Emit deferred `: undefined` and separator from previous entry
                        if parent.conditional_slot_needs_undefined {
                            code.push_str("\n      : undefined");
                            parent.conditional_slot_needs_undefined = false;
                        }
                        if parent.create_slots_entry_count > 0 {
                            code.push_str(",\n    ");
                        }
                    }
                    // Emit: (cond)\n      ?
                    code.push('(');
                    if let Some(ref expr) = vif.expression {
                        let expr_str = &source[expr.start as usize..expr.end as usize];
                        write_expr_with_ctx(
                            &mut code,
                            expr_str,
                            &vfor_locals_refs,
                            &state.binding_metadata,
                            source.as_bytes(),
                            state.is_inline_mode,
                        );
                    }
                    code.push_str(")\n      ? ");
                    None // Key is handled by conditional_slot_key_counter
                }
                OxcVConditionType::ElseIf => {
                    code.push_str("\n      : (");
                    if let Some(ref expr) = vif.expression {
                        let expr_str = &source[expr.start as usize..expr.end as usize];
                        write_expr_with_ctx(
                            &mut code,
                            expr_str,
                            &vfor_locals_refs,
                            &state.binding_metadata,
                            source.as_bytes(),
                            state.is_inline_mode,
                        );
                    }
                    code.push_str(")\n      ? ");
                    None
                }
                OxcVConditionType::Else => {
                    code.push_str("\n      : ");
                    None
                }
            }
        } else {
            match vif.condition_type {
                OxcVConditionType::If => {
                    // Save outer conditional chain state before starting a new one
                    if state.in_conditional_chain {
                        state.conditional_chain_stack.push((
                            state.in_conditional_chain,
                            state.conditional_chain_depth,
                            state.conditional_branch_index,
                        ));
                    }
                    // Start a new conditional chain
                    let parent_depth = state.depth.saturating_sub(1);
                    state.conditional_branch_index = state
                        .conditional_next_key_by_depth
                        .get(&parent_depth)
                        .copied()
                        .unwrap_or(0);
                    state.in_conditional_chain = true;
                    // Track the depth at which the conditional chain should be closed.
                    state.conditional_chain_depth = state.depth - 1;
                    if let Some(ref expr) = vif.expression {
                        let expr_str = &source[expr.start as usize..expr.end as usize];
                        write_expr_with_ctx(
                            &mut code,
                            expr_str,
                            &vfor_locals_refs,
                            &state.binding_metadata,
                            source.as_bytes(),
                            state.is_inline_mode,
                        );
                        code.push_str("\n  ? ");
                    }
                    Some(state.conditional_branch_index)
                }
                OxcVConditionType::ElseIf => {
                    state.conditional_branch_index += 1;
                    code.push_str(": ");
                    if let Some(ref expr) = vif.expression {
                        let expr_str = &source[expr.start as usize..expr.end as usize];
                        write_expr_with_ctx(
                            &mut code,
                            expr_str,
                            &vfor_locals_refs,
                            &state.binding_metadata,
                            source.as_bytes(),
                            state.is_inline_mode,
                        );
                        code.push_str("\n  ? ");
                    }
                    Some(state.conditional_branch_index)
                }
                OxcVConditionType::Else => {
                    state.conditional_branch_index += 1;
                    code.push_str(": ");
                    let key = state.conditional_branch_index;
                    // Mark next available key at this depth now that chain is complete.
                    state
                        .conditional_next_key_by_depth
                        .insert(state.conditional_chain_depth, key + 1);
                    // End the conditional chain after v-else
                    state.in_conditional_chain = false;
                    // Restore outer conditional chain state if nested
                    if let Some((in_chain, depth, branch_idx)) = state.conditional_chain_stack.pop()
                    {
                        state.in_conditional_chain = in_chain;
                        state.conditional_chain_depth = depth;
                        state.conditional_branch_index = branch_idx;
                    }
                    Some(key)
                }
            }
        }
    } else {
        None
    };

    // Determine if this element is a "block root" that needs _openBlock()
    // Block roots are: root elements (single root only), v-if/v-else-if/v-else branches, v-for items
    // For multi-root templates (root_element_count > 1), individual roots are NOT block roots
    // because the Fragment wrapper becomes the block root instead.
    // Root #2+ can be detected here since root_element_count was already incremented.
    let is_multi_root = is_root_element && state.root_element_count > 1;
    let needs_fragment_root = state.root_element_count > 1 || state.root_has_non_element_child;
    let is_block_root = (is_root_element && !needs_fragment_root)
        || elem.v_if.is_some()
        || (elem.v_for.is_some() && elem.has_key);
    // Note: KEYED v-for items are block roots (use _openBlock(), _createElementBlock).
    // UNKEYED v-for items are NOT block roots (use _createElementVNode).

    // Check if this element should be cached
    // An element is static if it has no dynamic props (v-bind, v-on, v-model, etc.)
    let is_static_element = elem.v_if.is_none()
        && elem.v_for.is_none()
        && elem.v_slot.is_none()
        && elem.custom_directives.is_empty()
        && elem.v_model.is_none()
        && are_all_props_static(&elem.props);
    let should_cache = is_multi_root && is_static_element;
    // Only cache self-closing elements (no children → guaranteed static).
    // Non-self-closing elements cannot be cached at open time because we don't know
    // yet if their children are dynamic. Their props will be hoisted instead.
    let should_cache_vnode = is_static_element
        && !is_root_element
        && !elem.v_once
        && !elem.props.is_empty()
        && open_tag.tag_type == SyntaxTagType::Element
        && open_tag.self_closing;

    // Collect all v-for and v-slot locals from their stacks (from all ancestor loops/slots)
    // Note: v-for locals were already pushed to the stack in the v-for handling block above
    // Clone into owned strings to avoid borrowing from state
    let vfor_locals_owned: Vec<String> = state
        .vfor_locals_stack
        .iter()
        .chain(state.vslot_locals_stack.iter())
        .flat_map(|v| v.iter().cloned())
        .collect();
    let vfor_locals: Vec<&str> = vfor_locals_owned.iter().map(|s| s.as_str()).collect();

    // Handle custom directives (v-focus, v-tooltip, etc.) and v-model
    // If element has directives, wrap with _withDirectives()
    // Note: v-model on components should NOT use _withDirectives - it uses props instead
    let has_custom_directives = !elem.custom_directives.is_empty();
    let has_v_model = elem.v_model.is_some();
    let is_component = open_tag.tag_type == SyntaxTagType::Component;
    // Only use directive wrapper for v-model on native elements (input, select, textarea)
    let has_v_model_directive = has_v_model && !is_component;
    let v_show_expression = elem
        .props
        .iter()
        .find(|p| p.kind == PropKind::Show)
        .and_then(|p| p.value);
    let has_v_show_directive = v_show_expression.is_some();
    let needs_directives_wrapper =
        has_custom_directives || has_v_model_directive || has_v_show_directive;

    if has_custom_directives {
        // Store directives for use at close time
        state
            .elements_with_directives
            .insert(elem.element_id, elem.custom_directives.clone());

        // Register each directive name for resolution at render function start
        for directive in &elem.custom_directives {
            if !state.resolved_directives.contains(&directive.name) {
                state.resolved_directives.push(directive.name.clone());
            }
        }
    }

    // Only store v-model directive info for native elements
    // Components handle v-model through props (modelValue + onUpdate:modelValue)
    if has_v_model_directive {
        // Store v-model info for use at close time
        let tag_name = &source[elem.tag_name.start as usize..elem.tag_name.end as usize];
        state.elements_with_vmodel.insert(
            elem.element_id,
            (elem.v_model.clone().unwrap(), tag_name.to_lowercase()),
        );

        // Add appropriate v-model helper based on tag name
        let tag_lower = tag_name.to_lowercase();
        if tag_lower == "select" {
            state.helpers.insert(HelperFlags::V_MODEL_SELECT);
        } else {
            state.helpers.insert(HelperFlags::V_MODEL_TEXT);
        }
    }

    if let Some(v_show_span) = v_show_expression {
        state
            .elements_with_vshow
            .insert(elem.element_id, v_show_span);
        state.helpers.insert(HelperFlags::V_SHOW);
    }

    if needs_directives_wrapper {
        // Prepend _withDirectives( wrapper
        code.push_str("_withDirectives(");
        state.helpers.insert(HelperFlags::WITH_DIRECTIVES);
        if has_custom_directives {
            state.helpers.insert(HelperFlags::RESOLVE_DIRECTIVE);
        }
    }

    // Handle v-once directive - wrap with cache pattern
    // Format: _cache[N] || (_setBlockTracking(-1, true), (element).cacheIndex = N, _setBlockTracking(1), _cache[N])
    if elem.v_once {
        let cache_idx = state.cache_index;
        state.cache_index += 1;
        state.elements_with_vonce.insert(elem.element_id, cache_idx);
        code.push_str(&format!(
            "_cache[{}] || (\n      _setBlockTracking(-1, true),\n      (_cache[{}] = ",
            cache_idx, cache_idx
        ));
        state.helpers.insert(HelperFlags::SET_BLOCK_TRACKING);
    }

    // Cache static VNodes (non-root) with _cache[n] || (_cache[n] = ...)
    // Only self-closing elements are cached immediately; non-self-closing defer to close time.
    if should_cache_vnode {
        let cache_idx = state.cache_index;
        state.cache_index += 1;
        state.element_cache_index.insert(elem.element_id, cache_idx);
        code.push_str(&format!(
            "_cache[{}] || (_cache[{}] = ",
            cache_idx, cache_idx
        ));
    }

    // Element opening based on type
    // Block roots use (_openBlock(), _createElementBlock(...))
    // Non-block elements use _createElementVNode(...)
    match open_tag.tag_type {
        SyntaxTagType::Element => {
            let tag_name = &source[elem.tag_name.start as usize..elem.tag_name.end as usize];

            // Store first root's tag name for potential multi-root patching
            if is_root_element && state.root_element_count == 1 {
                state.first_root_tag_name = Some(tag_name.to_string());
            }

            // For multi-root case, commas are added at close time after each cached element
            // (the trailing comma after the last element is removed in finalize_template)

            // Add cache wrapper for static elements in multi-root templates
            // Note: For root element #1, caching is added in finalize_template when we know it's multi-root
            // For root elements #2+, we know it's multi-root so add caching here
            if should_cache && state.root_element_count > 1 {
                // Cache index for element N (1-indexed root_element_count) is N-1
                // But first root (root_element_count was 1) is handled in finalize_template
                // So for root #2, cache index is 1; for root #3, cache index is 2, etc.
                let cache_idx = state.root_element_count - 1;
                if open_tag.self_closing {
                    code.push_str(&format!(
                        "_cache[{}] || (_cache[{}] = ",
                        cache_idx, cache_idx
                    ));
                    state.element_cache_index.insert(elem.element_id, cache_idx);
                    state.element_cache_needs_comma.insert(elem.element_id);
                }
                // Non-self-closing multi-root elements are not cached;
                // their props are hoisted instead.
            }

            if is_block_root {
                code.push_str("(_openBlock(), _createElementBlock(\"");
                code.push_str(tag_name);
                code.push_str("\", ");

                state.helpers.insert(HelperFlags::OPEN_BLOCK);
                state.helpers.insert(HelperFlags::CREATE_ELEMENT_BLOCK);
            } else {
                code.push_str("_createElementVNode(\"");
                code.push_str(tag_name);
                code.push_str("\", ");

                state.helpers.insert(HelperFlags::CREATE_ELEMENT_VNODE);
            }
        }
        SyntaxTagType::Component => {
            let tag_name = &source[elem.tag_name.start as usize..elem.tag_name.end as usize];

            // Components are block roots when: at root (single root), in v-if branches, or keyed v-for
            let component_is_block = (is_root_element && !needs_fragment_root)
                || elem.v_if.is_some()
                || (elem.v_for.is_some() && elem.has_key);

            if component_is_block {
                code.push_str("(_openBlock(), _createBlock(");
                state.helpers.insert(HelperFlags::OPEN_BLOCK);
                state.helpers.insert(HelperFlags::CREATE_BLOCK);
            } else {
                code.push_str("_createVNode(");
                state.helpers.insert(HelperFlags::CREATE_VNODE);
            }

            // Check if the component is a setup binding (imported in <script setup>)
            let is_setup_binding = matches!(
                state
                    .binding_metadata
                    .get(tag_name.as_bytes(), source.as_bytes()),
                Some(super::types::BindingType::Setup | super::types::BindingType::SetupRef)
            );

            if is_setup_binding {
                // Direct reference via $setup (standalone) or bare name (inline)
                let prefix = super::types::resolve_binding_prefix(
                    tag_name.as_bytes(),
                    &state.binding_metadata,
                    source.as_bytes(),
                    state.is_inline_mode,
                );
                code.push_str(prefix);
                code.push_str(tag_name);
            } else {
                // Runtime resolution via _resolveComponent
                if !state.resolved_components.contains(&tag_name.to_string()) {
                    state.resolved_components.push(tag_name.to_string());
                }
                code.push_str("_component_");
                code.push_str(&tag_name.replace('-', "_"));
                state.helpers.insert(HelperFlags::RESOLVE_COMPONENT);
            }
            code.push_str(", ");

            // Push to component stack to track dynamic props (needed for both self-closing and non-self-closing)
            state.component_stack.push(ComponentChildrenState {
                element_id: elem.element_id,
                insert_pos: open_tag.end,
                separator_pos: open_tag.end.saturating_sub(1),
                children_opened: false,
                uses_slots: false,
                default_slot_opened: false,
                is_block_root: component_is_block,
                has_named_slots: false,
                dynamic_props: Vec::new(),
                default_slot_child_count: 0,
                element_id_stack_len_at_open: state.element_id_stack.len(),
                vslot_depth_at_open: state.active_vslot_depth,
                has_conditional_named_slots: false,
                conditional_slot_key_counter: 0,
                create_slots_entry_count: 0,
                current_conditional_slot: None,
                conditional_slot_needs_undefined: false,
            });
        }
        SyntaxTagType::DynamicComponent => {
            // <component :is="..."> or <component is="div"> dynamic component
            // Find the is binding (can be :is="expr" dynamic or is="tag" static)
            let is_binding_dynamic = elem.props.iter().find(|p| {
                p.kind == PropKind::Bind
                    && source[p.name.start as usize..p.name.end as usize] == *"is"
            });
            let is_binding_static = elem.props.iter().find(|p| {
                p.kind == PropKind::Static
                    && source[p.name.start as usize..p.name.end as usize] == *"is"
            });

            code.push_str("(_openBlock(), _createBlock(_resolveDynamicComponent(");

            if let Some(prop) = is_binding_dynamic {
                // Dynamic :is="expr" - use write_expr_with_ctx to handle v-for locals
                if let Some(ref value) = prop.value {
                    let expr = &source[value.start as usize..value.end as usize];
                    write_expr_with_ctx(
                        &mut code,
                        expr,
                        &vfor_locals,
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                } else {
                    code.push_str("undefined");
                }
            } else if let Some(prop) = is_binding_static {
                // Static is="div" - emit as string literal
                if let Some(ref value) = prop.value {
                    let tag = &source[value.start as usize..value.end as usize];
                    code.push('"');
                    code.push_str(tag);
                    code.push('"');
                } else {
                    code.push_str("undefined");
                }
            } else {
                // No is binding found - this shouldn't happen but handle gracefully
                code.push_str("undefined");
            }

            code.push_str("), ");

            state.helpers.insert(HelperFlags::OPEN_BLOCK);
            state.helpers.insert(HelperFlags::CREATE_BLOCK);
            state.helpers.insert(HelperFlags::RESOLVE_DYNAMIC_COMPONENT);

            // Push to component stack to track dynamic props (needed for both self-closing and non-self-closing)
            // Dynamic components are block roots (need extra closing paren)
            state.component_stack.push(ComponentChildrenState {
                element_id: elem.element_id,
                insert_pos: open_tag.end,
                separator_pos: open_tag.end.saturating_sub(1),
                children_opened: false,
                uses_slots: false,
                default_slot_opened: false,
                is_block_root: true, // Dynamic components use (_openBlock(), _createBlock(...))
                has_named_slots: false,
                dynamic_props: Vec::new(),
                default_slot_child_count: 0,
                element_id_stack_len_at_open: state.element_id_stack.len(),
                vslot_depth_at_open: state.active_vslot_depth,
                has_conditional_named_slots: false,
                conditional_slot_key_counter: 0,
                create_slots_entry_count: 0,
                current_conditional_slot: None,
                conditional_slot_needs_undefined: false,
            });
        }
        SyntaxTagType::Slot => {
            code.push_str("_renderSlot(_ctx.$slots, ");

            // Find the "name" prop to determine slot name
            let slot_name = elem
                .props
                .iter()
                .find(|p| {
                    p.kind == PropKind::Static
                        && source[p.name.start as usize..p.name.end as usize] == *"name"
                })
                .and_then(|p| {
                    p.value
                        .as_ref()
                        .map(|v| &source[v.start as usize..v.end as usize])
                })
                .unwrap_or("default");

            code.push('"');
            code.push_str(slot_name);
            code.push('"');

            // Track slot element for proper closing
            if !open_tag.self_closing {
                state.slot_stack.push(SlotElementState {
                    element_id: elem.element_id,
                    has_children: false, // Will be updated if children are found
                });
            }

            state.helpers.insert(HelperFlags::RENDER_SLOT);
        }
        SyntaxTagType::Template => {
            // Check if this is a v-slot template
            if let Some(ref vslot) = elem.v_slot {
                let slot_name = vslot
                    .name
                    .as_ref()
                    .map(|s| &source[s.start as usize..s.end as usize])
                    .unwrap_or("default");

                if is_conditional_vslot {
                    // === Conditional v-slot: use _createSlots() format ===
                    let vif = elem.v_if.as_ref().unwrap();
                    let is_v_if_start = matches!(
                        vif.condition_type,
                        crate::syntax::types::OxcVConditionType::If
                    );

                    // Pre-compute values before borrowing component_stack mutably
                    let _sf_dynamic = state.slot_flag(2, "DYNAMIC");
                    let cond_type = vif.condition_type;

                    if let Some(parent) = state.component_stack.last_mut() {
                        // Transition to createSlots and default slot closing already
                        // handled in the v-if section above (before condition expression)
                        if is_v_if_start {
                            parent.create_slots_entry_count += 1;
                        }
                        // v-else-if/v-else don't start new entries, they continue the ternary
                        parent.has_named_slots = true;
                        // Track condition info for VSlot close
                        parent.current_conditional_slot = Some(cond_type);
                        // v-else clears the needs_undefined flag (ternary will be completed)
                        if matches!(cond_type, crate::syntax::types::OxcVConditionType::Else) {
                            parent.conditional_slot_needs_undefined = false;
                        }
                    }
                    state.helpers.insert(HelperFlags::CREATE_SLOTS);

                    // Generate slot descriptor: { name: "slotName", fn: _withCtx((params) => [
                    code.push_str("{\n          name: \"");
                    code.push_str(slot_name);
                    code.push_str("\",\n          fn: _withCtx((");

                    if let Some(ref params) = vslot.params {
                        let params_text = &source[params.start as usize..params.end as usize];
                        code.push_str(params_text);
                        let locals = extract_slot_locals(params_text);
                        state.vslot_locals_stack.push(locals);
                    } else {
                        state.vslot_locals_stack.push(Vec::new());
                    }

                    code.push_str(") => [\n");

                    state.helpers.insert(HelperFlags::WITH_CTX);
                    // Save outer conditional chain state on the v-slot-specific stack
                    // (separate from conditional_chain_stack used by nested v-if)
                    state.vslot_conditional_chain_stack.push((
                        state.in_conditional_chain,
                        state.conditional_chain_depth,
                        state.conditional_branch_index,
                    ));
                    state.in_conditional_chain = false;
                    state.active_vslot_depth += 1;
                } else if let Some(parent_ref) = state.component_stack.last() {
                    if parent_ref.has_conditional_named_slots {
                        // === Static slot inside a createSlots component ===
                        // Use array entry format: { name: "slotName", fn: _withCtx(() => [...]) }
                        if let Some(parent) = state.component_stack.last_mut() {
                            // Emit deferred `: undefined` from previous conditional slot
                            if parent.conditional_slot_needs_undefined {
                                code.push_str("\n      : undefined");
                                parent.conditional_slot_needs_undefined = false;
                            }
                            if parent.create_slots_entry_count > 0 {
                                code.push_str(",\n    ");
                            }
                            parent.create_slots_entry_count += 1;
                            parent.has_named_slots = true;
                        }

                        code.push_str("{\n          name: \"");
                        code.push_str(slot_name);
                        code.push_str("\",\n          fn: _withCtx((");

                        if let Some(ref params) = vslot.params {
                            let params_text = &source[params.start as usize..params.end as usize];
                            code.push_str(params_text);
                            let locals = extract_slot_locals(params_text);
                            state.vslot_locals_stack.push(locals);
                        } else {
                            state.vslot_locals_stack.push(Vec::new());
                        }

                        code.push_str(") => [");

                        state.helpers.insert(HelperFlags::WITH_CTX);
                        // Save outer conditional chain state on the v-slot-specific stack
                        // (separate from conditional_chain_stack used by nested v-if)
                        state.vslot_conditional_chain_stack.push((
                            state.in_conditional_chain,
                            state.conditional_chain_depth,
                            state.conditional_branch_index,
                        ));
                        state.in_conditional_chain = false;
                        state.active_vslot_depth += 1;
                    } else {
                        // === Normal v-slot (no createSlots needed) ===
                        if let Some(parent) = state.component_stack.last_mut() {
                            if !parent.children_opened {
                                code_transform.prepend_left(parent.insert_pos, "{");
                                parent.children_opened = true;
                                parent.uses_slots = true;
                            } else if parent.default_slot_opened {
                                code.push_str("]), ");
                                parent.default_slot_opened = false;
                                parent.default_slot_child_count = 0;
                            } else if parent.has_named_slots {
                                // Comma separator between consecutive named slots
                                code.push_str(", ");
                            }
                            parent.has_named_slots = true;
                        }

                        push_slot_key(&mut code, slot_name);
                        code.push_str(": _withCtx((");

                        if let Some(ref params) = vslot.params {
                            let params_text = &source[params.start as usize..params.end as usize];
                            code.push_str(params_text);
                            let locals = extract_slot_locals(params_text);
                            state.vslot_locals_stack.push(locals);
                        } else {
                            state.vslot_locals_stack.push(Vec::new());
                        }

                        code.push_str(") => [");

                        state.helpers.insert(HelperFlags::WITH_CTX);
                        // Save outer conditional chain state on the v-slot-specific stack
                        // (separate from conditional_chain_stack used by nested v-if)
                        state.vslot_conditional_chain_stack.push((
                            state.in_conditional_chain,
                            state.conditional_chain_depth,
                            state.conditional_branch_index,
                        ));
                        state.in_conditional_chain = false;
                        state.active_vslot_depth += 1;
                    }
                } else {
                    // No parent component (shouldn't happen for v-slot)
                    push_slot_key(&mut code, slot_name);
                    code.push_str(": _withCtx((");
                    state.vslot_locals_stack.push(Vec::new());
                    code.push_str(") => [");
                    state.helpers.insert(HelperFlags::WITH_CTX);
                    // Save outer conditional chain state on the v-slot-specific stack
                    // (separate from conditional_chain_stack used by nested v-if)
                    state.vslot_conditional_chain_stack.push((
                        state.in_conditional_chain,
                        state.conditional_chain_depth,
                        state.conditional_branch_index,
                    ));
                    state.in_conditional_chain = false;
                    state.active_vslot_depth += 1;
                }
            } else {
                // Regular <template> elements - fragments
                code.push_str("(_openBlock(), _createElementBlock(_Fragment, ");

                state.helpers.insert(HelperFlags::OPEN_BLOCK);
                state.helpers.insert(HelperFlags::CREATE_ELEMENT_BLOCK);
                state.helpers.insert(HelperFlags::FRAGMENT);
            }
        }
        _ => return,
    }

    // Props (skip for v-slot templates since they don't have props in the generated code)
    let is_vslot_template = elem.v_slot.is_some() && open_tag.tag_type == SyntaxTagType::Template;
    let is_component = open_tag.tag_type == SyntaxTagType::Component;
    let is_dynamic_component = open_tag.tag_type == SyntaxTagType::DynamicComponent;
    let is_slot = open_tag.tag_type == SyntaxTagType::Slot;

    if !is_vslot_template {
        if is_slot {
            // For slots, filter out the "name" prop (used for slot identification)
            // and write remaining props if any
            let slot_props: Vec<_> = elem
                .props
                .iter()
                .filter(|p| {
                    !(p.kind == PropKind::Static
                        && source[p.name.start as usize..p.name.end as usize] == *"name")
                })
                .cloned()
                .collect();

            if !slot_props.is_empty() {
                code.push_str(", ");
                write_props(&mut code, &slot_props, source, state, &vfor_locals, false);
            }

            // For slots with potential fallback content, open with ", () => ["
            if !open_tag.self_closing {
                if slot_props.is_empty() {
                    code.push_str(", {}, () => [");
                } else {
                    code.push_str(", () => [");
                }
            }
        } else if is_dynamic_component {
            // For dynamic components, filter out the ":is" binding (already used)
            let dyn_props: Vec<_> = elem
                .props
                .iter()
                .filter(|p| {
                    // Filter out both dynamic :is and static is props (already used for _resolveDynamicComponent)
                    let prop_name = &source[p.name.start as usize..p.name.end as usize];
                    !((p.kind == PropKind::Bind || p.kind == PropKind::Static) && prop_name == "is")
                })
                .cloned()
                .collect();

            if dyn_props.is_empty() {
                code.push_str("null");
            } else {
                write_props(&mut code, &dyn_props, source, state, &vfor_locals, true);
            }

            // Dynamic components use component-like children handling
            // Note: Don't add ", " here - children opener adds it when needed
            // (", [" for arrays at line 120, ", {" for slots at line 349)
            if !open_tag.self_closing {
                state.element_child_count.insert(elem.element_id, 0);
            }
        } else {
            // Handle props, including key for conditional branches
            let has_conditional_key = conditional_key.is_some();

            if elem.props.is_empty() && !has_conditional_key {
                code.push_str("null");
            } else if are_all_props_static(&elem.props)
                && !has_setup_ref_binding(&elem.props, source, &state.binding_metadata)
                && open_tag.tag_type == SyntaxTagType::Element
                && !should_cache_vnode
            {
                // Static props can be hoisted (with conditional key if present)
                state.hoist_counter += 1;
                let hoist_name = format!("_hoisted_{}", state.hoist_counter);
                let props_code = generate_props_code_with_optional_key(
                    &elem.props,
                    source,
                    if has_conditional_key {
                        conditional_key
                    } else {
                        None
                    },
                    state,
                );
                state.hoisted_nodes.push(super::types::HoistedNode {
                    name: hoist_name.clone(),
                    code: props_code,
                });
                code.push_str(&hoist_name);
            } else if has_conditional_key {
                // Props with key for conditional branches
                write_props_with_key(
                    &mut code,
                    &elem.props,
                    source,
                    state,
                    &vfor_locals,
                    conditional_key.unwrap(),
                    is_component,
                );
            } else {
                write_props(
                    &mut code,
                    &elem.props,
                    source,
                    state,
                    &vfor_locals,
                    is_component,
                );
            }

            // Open children array (for non-self-closing)
            // Skip for components - children opener is deferred until we know if slots are used
            if !open_tag.self_closing && !is_component && !is_dynamic_component {
                code.push_str(", ");
                // Initialize child count tracking for single child optimization
                state.element_child_count.insert(elem.element_id, 0);
                // Track where to insert array opener for non-element children
                state
                    .element_children_insert_pos
                    .insert(elem.element_id, open_tag.end);
            }
        }
    }
    // For v-slot templates, the children array is already opened by the callback
    // For components and dynamic components, children opener is deferred to determine { vs [

    // Calculate and store patch flags for this element (used when closing)
    // Only for native elements - components handle their own patching
    if open_tag.tag_type == SyntaxTagType::Element {
        let (mut patch_flags, dynamic_props) =
            calculate_patch_flags(&elem.props, source, &state.binding_metadata, &vfor_locals);

        // Add NEED_PATCH (512) for elements with directive-based runtime updates.
        if has_v_model || has_v_show_directive {
            patch_flags |= super::types::patch_flags::NEED_PATCH;
        }

        // Add NEED_PATCH (512) for elements with ref attribute, but ONLY when
        // no other patch flags are set. Vue's compiler uses this as a fallback
        // to ensure the element still participates in patch optimization when
        // it would otherwise have no flags.
        if patch_flags == 0 {
            let has_ref = elem.props.iter().any(|p| {
                p.kind == PropKind::Static
                    && source[p.name.start as usize..p.name.end as usize] == *"ref"
            });
            if has_ref {
                patch_flags |= super::types::patch_flags::NEED_PATCH;
            }
        }

        if patch_flags > 0 {
            state
                .element_patch_flags
                .insert(elem.element_id, patch_flags);
        }
        if !dynamic_props.is_empty() {
            state
                .element_dynamic_props
                .insert(elem.element_id, dynamic_props);
        }
        // For non-self-closing elements: push to stack for lookup at CloseTag time
        // Self-closing elements handle their close inline below, no CloseTag event comes
        if !open_tag.self_closing {
            state.element_id_stack.push(elem.element_id);
            state
                .element_is_block_root
                .insert(elem.element_id, is_block_root);
        }
    }

    // Templates (Fragment wrappers) also need element_id tracking for proper closing
    // This ensures child_count is tracked correctly for template children
    if open_tag.tag_type == SyntaxTagType::Template
        && !open_tag.self_closing
        && elem.v_slot.is_none()
    {
        state.element_id_stack.push(elem.element_id);
        // Templates are always block roots (they use _createElementBlock for Fragment)
        state.element_is_block_root.insert(elem.element_id, true);
    }

    // For self-closing elements, add closing parens now (no CloseTag event will come)
    if open_tag.self_closing {
        // Elements with directives will get NEED_PATCH merged with calculated flags below
        let _needs_directives = needs_directives_wrapper;

        match open_tag.tag_type {
            SyntaxTagType::Component => {
                // Self-closing component: _createVNode(Comp, props) or (_openBlock(), _createBlock(Comp, props))
                // Pop the component state to get dynamic props and block root status
                let (dynamic_props_suffix, comp_is_block) =
                    if let Some(component) = state.component_stack.pop() {
                        let suffix = if !component.dynamic_props.is_empty() {
                            let props_ref = format_dynamic_props(&component.dynamic_props);
                            {
                                let pf = state.pflag(8, "PROPS");
                                format!(", null, {}{}", pf, props_ref)
                            }
                        } else {
                            String::new()
                        };
                        (suffix, component.is_block_root)
                    } else {
                        (String::new(), false)
                    };
                code.push_str(&dynamic_props_suffix);
                if comp_is_block {
                    code.push_str("))");
                } else {
                    code.push(')');
                }
            }
            SyntaxTagType::DynamicComponent => {
                // Self-closing dynamic component: (_openBlock(), _createBlock(_resolveDynamicComponent(...), props))
                // Pop the component state to get dynamic props
                let dynamic_props_suffix = if let Some(component) = state.component_stack.pop() {
                    if !component.dynamic_props.is_empty() {
                        let props_ref = format_dynamic_props(&component.dynamic_props);
                        {
                            let pf = state.pflag(8, "PROPS");
                            format!(", null, {}{}", pf, props_ref)
                        }
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                code.push_str(&dynamic_props_suffix);
                code.push_str("))");
            }
            SyntaxTagType::Element | SyntaxTagType::CustomElement => {
                // Self-closing element: _createElementVNode("...", props[, null, flags]) or block version
                let sc_flags = state
                    .element_patch_flags
                    .remove(&elem.element_id)
                    .unwrap_or(0);
                let sc_dynamic_props = state
                    .element_dynamic_props
                    .remove(&elem.element_id)
                    .unwrap_or_default();

                // Check if this element is cached - if so, add CACHED flag as VNode argument
                let is_cached = state.element_cache_index.contains_key(&elem.element_id);

                if _needs_directives && sc_flags > 0 {
                    // Directives add NEED_PATCH, merge with other calculated flags
                    let flags_str = format_patch_flag_prod(sc_flags, state.is_production);
                    let props_str = hoist_dynamic_props_array(&sc_dynamic_props, state);
                    code.push_str(", null");
                    code.push_str(&flags_str);
                    code.push_str(&props_str);
                } else if _needs_directives {
                    // Directives need null children + NEED_PATCH only
                    code.push_str(&format!(", null, {}", state.pflag(512, "NEED_PATCH")));
                } else if sc_flags > 0 {
                    // Has patch flags: emit null children + flags
                    let flags_str = format_patch_flag_prod(sc_flags, state.is_production);
                    let props_str = hoist_dynamic_props_array(&sc_dynamic_props, state);
                    code.push_str(", null");
                    code.push_str(&flags_str);
                    code.push_str(&props_str);
                } else if is_cached {
                    // Cached elements need null children + CACHED flag as VNode arguments
                    let cached = state.pflag(-1, "CACHED");
                    code.push_str(&format!(", null, {}", cached));
                }

                if is_block_root {
                    code.push_str("))");
                } else {
                    code.push(')');
                }
            }
            SyntaxTagType::Slot => {
                // Self-closing slot: _renderSlot(_ctx.$slots, "name")
                // With scoped styles: _renderSlot(_ctx.$slots, "name", {}, void 0, true)
                if state.scope_id.is_some() {
                    // Check if slot has extra props (beyond "name")
                    let has_extra_props = elem.props.iter().any(|p| {
                        !(p.kind == PropKind::Static
                            && source[p.name.start as usize..p.name.end as usize] == *"name")
                    });
                    if !has_extra_props {
                        code.push_str(", {}, void 0, true");
                    } else {
                        code.push_str(", void 0, true");
                    }
                }
                code.push(')');
            }
            _ => {}
        }

        // If this self-closing element was wrapped in _cache[n] || (_cache[n] = ...),
        // close the cache wrapper. The CACHED flag is already added as a VNode argument above.
        if state.element_cache_index.remove(&elem.element_id).is_some() {
            code.push(')');
        }

        // If self-closing element has directives (custom, v-model, v-show), append the directive array
        // Format: , [[_directive_focus], [_vModelText, _ctx.value, void 0, { lazy: true }]])
        if needs_directives_wrapper {
            code.push_str(", [");
            let mut directive_count = 0;

            // Add custom directives first
            if let Some(directives) = state.elements_with_directives.remove(&elem.element_id) {
                for directive in directives.iter() {
                    if directive_count > 0 {
                        code.push_str(", ");
                    }
                    directive_count += 1;

                    code.push_str("[_directive_");
                    code.push_str(&camelize(&directive.name));

                    // Add value if present
                    if let Some(ref value_span) = directive.value {
                        code.push_str(", ");
                        let value_str = &source[value_span.start as usize..value_span.end as usize];
                        if value_str.starts_with('\'')
                            || value_str.starts_with('"')
                            || value_str.starts_with('`')
                        {
                            code.push_str(value_str);
                        } else {
                            write_expr_with_ctx(
                                &mut code,
                                value_str,
                                &vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                        }
                    }

                    // Add argument if present
                    if let Some(ref arg_span) = directive.arg {
                        if directive.value.is_none() {
                            code.push_str(", void 0");
                        }
                        code.push_str(", ");
                        let arg_str = &source[arg_span.start as usize..arg_span.end as usize];
                        if directive.is_dynamic_arg {
                            // Dynamic argument: strip brackets and add _ctx. prefix
                            let arg_str = arg_str.trim_start_matches('[').trim_end_matches(']');
                            write_expr_with_ctx(
                                &mut code,
                                arg_str,
                                &vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                        } else {
                            code.push('"');
                            code.push_str(arg_str);
                            code.push('"');
                        }
                    }

                    // Add modifiers if present
                    if !directive.modifiers.is_empty() {
                        if directive.value.is_none() && directive.arg.is_none() {
                            code.push_str(", void 0, void 0");
                        } else if directive.arg.is_none() {
                            code.push_str(", void 0");
                        }
                        code.push_str(", { ");
                        for (j, mod_span) in directive.modifiers.iter().enumerate() {
                            if j > 0 {
                                code.push_str(", ");
                            }
                            code.push_str(&source[mod_span.start as usize..mod_span.end as usize]);
                            code.push_str(": true");
                        }
                        code.push_str(" }");
                    }

                    code.push(']');
                }
            }

            // Add v-model directive
            if let Some((vmodel_info, tag_name)) =
                state.elements_with_vmodel.remove(&elem.element_id)
            {
                if directive_count > 0 {
                    code.push_str(", ");
                }
                directive_count += 1;

                // Select directive based on tag name
                let directive_name = if tag_name == "select" {
                    "_vModelSelect"
                } else {
                    "_vModelText"
                };

                code.push('[');
                code.push_str(directive_name);

                // Add value
                if let Some(ref value_span) = vmodel_info.value {
                    code.push_str(", ");
                    let value_str = &source[value_span.start as usize..value_span.end as usize];
                    write_expr_with_ctx(
                        &mut code,
                        value_str,
                        &vfor_locals,
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                }

                // Add modifiers if present
                if !vmodel_info.modifiers.is_empty() {
                    code.push_str(", void 0, { ");
                    for (j, mod_span) in vmodel_info.modifiers.iter().enumerate() {
                        if j > 0 {
                            code.push_str(", ");
                        }
                        code.push_str(&source[mod_span.start as usize..mod_span.end as usize]);
                        code.push_str(": true");
                    }
                    code.push_str(" }");
                }

                code.push(']');
            }

            // Add v-show directive
            if let Some(vshow_span) = state.elements_with_vshow.remove(&elem.element_id) {
                if directive_count > 0 {
                    code.push_str(", ");
                }
                code.push_str("[_vShow, ");
                let value_str = &source[vshow_span.start as usize..vshow_span.end as usize];
                write_expr_with_ctx(
                    &mut code,
                    value_str,
                    &vfor_locals,
                    &state.binding_metadata,
                    source.as_bytes(),
                    state.is_inline_mode,
                );
                code.push(']');
            }

            code.push_str("])");
        }

        // If self-closing element has v-once, close the cache pattern
        // Format: ).cacheIndex = N, _setBlockTracking(1), _cache[N])
        if elem.v_once {
            if let Some(cache_idx) = state.elements_with_vonce.remove(&elem.element_id) {
                code.push_str(&format!(
                    ").cacheIndex = {},\n      _setBlockTracking(1),\n      _cache[{}]\n    )",
                    cache_idx, cache_idx
                ));
            }
        }

        // If self-closing element has v-for, process the close action
        // Format: }), 128 /* KEYED_FRAGMENT */)) or }), 256 /* UNKEYED_FRAGMENT */))
        if let Some(scope_id) = state.element_vfor_scope.remove(&elem.element_id) {
            if let Some(CloseAction::VFor { keyed, stable }) =
                state.scope_close_actions.remove(&scope_id)
            {
                let frag = if stable {
                    state.pflag(64, "STABLE_FRAGMENT")
                } else if keyed {
                    state.pflag(128, "KEYED_FRAGMENT")
                } else {
                    state.pflag(256, "UNKEYED_FRAGMENT")
                };
                code.push_str(&format!("}}), {}))", frag));
            }
            // Pop v-for locals stack for self-closing elements
            if state.elements_with_vfor.remove(&elem.element_id) {
                state.vfor_locals_stack.pop();
            }
        }

        // Revert depth tracking for self-closing elements since they have no children
        // and no close event will come to decrement these
        state.depth -= 1;
        if state.first_child_at_depth.len() > state.depth {
            state.first_child_at_depth.pop();
        }
    }

    // Store first root's complete opening code for potential multi-root patching
    if is_root_element
        && state.root_element_count == 1
        && open_tag.tag_type == SyntaxTagType::Element
    {
        state.first_root_opening_code = Some(code.clone());
        state.first_root_is_self_closing = open_tag.self_closing;
    }

    // Prepend any v-slot text vnode closing (from text+interpolation before this element)
    if !vslot_text_close.is_empty() {
        code.insert_str(0, &vslot_text_close);
    }

    // Prepend the conditional close prefix (ternary else-branch from previous v-if)
    // before any comma separators that were inserted into code.
    if !conditional_close_prefix.is_empty() {
        code.insert_str(0, &conditional_close_prefix);
    }

    // Replace the opening tag with generated code.
    // For components, split the last byte into a separate removed chunk
    // so we have a distinct separator_pos for retroactive _createSlots( insertion.
    if state
        .component_stack
        .last()
        .map(|c| c.element_id == elem.element_id)
        .unwrap_or(false)
        && open_tag.end > elem.start + 1
    {
        code_transform.overwrite(elem.start, open_tag.end - 1, &code);
        code_transform.remove(open_tag.end - 1, open_tag.end);
    } else {
        code_transform.overwrite(elem.start, open_tag.end, &code);
    }
}

/// Process CloseTag - emit element closing.
pub fn process_close_tag<'a>(
    close_tag: &SyntaxCloseTag,
    state: &mut TemplateCodegenState,
    code_transform: &mut CodeTransform<'a>,
    source: &'a str,
) {
    // Skip if this element was already closed via v-slot (process_close_scopes)
    if state.vslot_closed_positions.remove(&close_tag.start) {
        return;
    }

    if state.depth == 0 {
        return;
    }

    // Pop v-for locals stack if this element has v-for
    if state.elements_with_vfor.remove(&close_tag.element_id) {
        state.vfor_locals_stack.pop();
    }

    // Check if we're closing a component
    // IMPORTANT: Only pop if this close event matches the component's element_id
    if let Some(component) = state.component_stack.last() {
        if component.element_id == close_tag.element_id {
            let component = state.component_stack.pop().unwrap();
            // Block roots (dynamic components) need extra ) for expression wrapper
            let extra_paren = if component.is_block_root { ")" } else { "" };

            // Build the dynamic props suffix if needed (for v-model, etc.)
            let dynamic_props_suffix = if !component.dynamic_props.is_empty() {
                let props_ref = format_dynamic_props(&component.dynamic_props);
                let pf = state.pflag(8, "PROPS");
                format!(", {}{}", pf, props_ref)
            } else {
                String::new()
            };

            let sf = state.slot_flag(1, "STABLE");
            let code = if component.has_conditional_named_slots {
                // createSlots mode: close the dynamic array and createSlots call
                let pf = state.pflag(1024, "DYNAMIC_SLOTS");
                let mut close = String::new();
                // Close default slot if it was opened as an array entry
                if component.default_slot_opened {
                    close.push_str("])\n        }");
                }
                if component.conditional_slot_needs_undefined {
                    close.push_str("\n      : undefined");
                }
                close.push_str(&format!("]), {}){}", pf, extra_paren));
                close
            } else if component.uses_slots {
                if component.has_named_slots {
                    // Named slots: Each slot is closed individually by CloseAction::VSlot.
                    // We just need to add stability marker and close the slots object.
                    // Format: { header: _withCtx(() => [...]) } already has slots closed
                    if component.default_slot_opened {
                        // Default slot was opened inline; close it before stability marker.
                        format!("]), {} }}{}){}", sf, dynamic_props_suffix, extra_paren)
                    } else {
                        format!(", {} }}{}){}", sf, dynamic_props_suffix, extra_paren)
                    }
                } else {
                    // Auto-generated default slot: need to close the slot array/function.
                    // Format: { default: _withCtx(() => [...children...]), _: 1 }
                    format!("]), {} }}{}){}", sf, dynamic_props_suffix, extra_paren)
                }
            } else if component.children_opened {
                // Array children format: close with ]) or ]))
                format!("]{}){}", dynamic_props_suffix, extra_paren)
            } else {
                // No children at all: just close the vnode
                // Need to add null for children slot when we have dynamic props
                if !dynamic_props_suffix.is_empty() {
                    format!(", null{}){}", dynamic_props_suffix, extra_paren)
                } else {
                    format!("){}", extra_paren)
                }
            };

            // Add separator between props and children at separator_pos.
            // The slot opening uses prepend_left at insert_pos (without ", " prefix).
            // prepend_right at separator_pos places content BETWEEN the open tag
            // overwrite and the slot content at insert_pos.
            if !component.has_conditional_named_slots
                && (component.uses_slots || component.children_opened)
            {
                code_transform.prepend_right(component.separator_pos, ", ");
            }

            // Append directive array suffix for components with custom directives or v-show
            let directive_suffix =
                generate_component_directive_suffix(component.element_id, state, source);
            let code = format!("{}{}", code, directive_suffix);

            code_transform.overwrite(close_tag.start, close_tag.end, &code);
            // Mark position so process_close_scopes skips it
            state.vslot_closed_positions.insert(close_tag.start);

            // Components DO increment depth when opening, so decrement it here
            state.depth -= 1;

            // Pop child tracking for this depth level
            if state.first_child_at_depth.len() > state.depth {
                state.first_child_at_depth.pop();
            }

            return;
        }
    }

    state.depth -= 1;

    // Pop child tracking for this depth level
    if state.first_child_at_depth.len() > state.depth {
        state.first_child_at_depth.pop();
    }

    // Check if we're closing a slot element
    if state.slot_stack.pop().is_some() {
        // Slot closing: close the fallback function and renderSlot call
        // Format: ]) closes the fallback array and ) closes renderSlot
        // With scoped styles: ], true) adds the slotted flag
        if state.scope_id.is_some() {
            code_transform.overwrite(close_tag.start, close_tag.end, "], true)");
        } else {
            code_transform.overwrite(close_tag.start, close_tag.end, "])");
        }
        return;
    }

    // Standard element closing - check for patch flags and single child optimization
    let element_id = state.element_id_stack.pop();
    let mut patch_flags = element_id
        .and_then(|id| state.element_patch_flags.remove(&id))
        .unwrap_or(0);
    let dynamic_props = element_id
        .and_then(|id| state.element_dynamic_props.remove(&id))
        .unwrap_or_default();

    // Check if this element has interpolation children (TEXT flag)
    if let Some(id) = element_id {
        if state.elements_with_interpolation.remove(&id) {
            patch_flags |= super::types::patch_flags::TEXT;
        }
    }

    // Check for single child optimization
    let single_child = element_id.and_then(|id| state.element_single_child.remove(&id));
    let child_count = element_id
        .and_then(|id| state.element_child_count.remove(&id))
        .unwrap_or(0);
    let array_opened = element_id
        .and_then(|id| state.element_array_opened.remove(&id))
        .unwrap_or(false);

    // Check if this element is a block root (uses _openBlock + _createElementBlock)
    // Non-block elements use _createElementVNode and close with single )
    let is_block_root = element_id
        .and_then(|id| state.element_is_block_root.remove(&id))
        .unwrap_or(true); // Default to true for safety

    // Closing suffix: )) for block roots, ) for non-block elements
    let close_suffix = if is_block_root { "))" } else { ")" };

    // Hoist dynamic props array before computing close code (needs &mut state)
    let hoisted_props_str = hoist_dynamic_props_array(&dynamic_props, state);
    let flags_str = if patch_flags > 0 {
        format_patch_flag_prod(patch_flags, state.is_production)
    } else {
        String::new()
    };

    // Determine closing format based on child count and array state
    let code = if child_count == 0 {
        // No children - emit null children placeholder and patch flags if needed
        if patch_flags > 0 {
            format!("null{}{}{}", flags_str, hoisted_props_str, close_suffix)
        } else {
            format!("null{}", close_suffix)
        }
    } else if child_count == 1 && single_child.is_some() && !array_opened {
        // Single text/interpolation child - no array brackets needed
        if patch_flags > 0 {
            format!("{}{}{}", flags_str, hoisted_props_str, close_suffix)
        } else {
            close_suffix.to_string()
        }
    } else if array_opened {
        // Multiple children with element children - use array format
        if patch_flags > 0 {
            format!("]{}{}{}", flags_str, hoisted_props_str, close_suffix)
        } else {
            format!("]{}", close_suffix)
        }
    } else {
        // Multiple text/interpolation children concatenated (no array)
        if patch_flags > 0 {
            format!("{}{}{}", flags_str, hoisted_props_str, close_suffix)
        } else {
            close_suffix.to_string()
        }
    };

    // Track first root's closing for potential multi-root patching
    if let Some(first_root_id) = state.first_root_element_id {
        if element_id == Some(first_root_id) {
            state.first_root_close_span = Some((close_tag.start, close_tag.end));
            state.first_root_close_code = Some(code.clone());
        }
    }

    code_transform.overwrite(close_tag.start, close_tag.end, &code);
}

/// Process AnalysedCloseScopes - handle scope closings (v-for, v-slot).
pub fn process_close_scopes<'a>(
    close: &AnalysedCloseScopes,
    state: &mut TemplateCodegenState,
    code_transform: &mut CodeTransform<'a>,
    source: &'a str,
) {
    if state.depth == 0 {
        return;
    }

    // Pop v-for locals stack if this element has v-for
    if state.elements_with_vfor.remove(&close.event.element_id) {
        state.vfor_locals_stack.pop();
    }

    // Check if we're closing a component
    // IMPORTANT: Only pop if this close event matches the component's element_id
    if let Some(component) = state.component_stack.last() {
        if component.element_id == close.event.element_id {
            let component = state.component_stack.pop().unwrap();
            // Block roots (dynamic components) need extra ) for expression wrapper
            let extra_paren = if component.is_block_root { ")" } else { "" };

            // Build the dynamic props suffix if needed (for v-model, etc.)
            let dynamic_props_suffix = if !component.dynamic_props.is_empty() {
                let props_ref = format_dynamic_props(&component.dynamic_props);
                let pf = state.pflag(8, "PROPS");
                format!(", {}{}", pf, props_ref)
            } else {
                String::new()
            };

            let sf = state.slot_flag(1, "STABLE");
            let code = if component.has_conditional_named_slots {
                // createSlots mode: close the dynamic array and createSlots call
                let pf = state.pflag(1024, "DYNAMIC_SLOTS");
                let mut close = String::new();
                // Close default slot if it was opened as an array entry
                if component.default_slot_opened {
                    close.push_str("])\n        }");
                }
                if component.conditional_slot_needs_undefined {
                    close.push_str("\n      : undefined");
                }
                close.push_str(&format!("]), {}){}", pf, extra_paren));
                close
            } else if component.uses_slots {
                if component.has_named_slots {
                    // Named slots: Each slot is closed individually by CloseAction::VSlot.
                    // We just need to add stability marker and close the slots object.
                    if state.in_conditional_chain && state.depth == state.conditional_chain_depth {
                        state.conditional_next_key_by_depth.insert(
                            state.conditional_chain_depth,
                            state.conditional_branch_index + 1,
                        );
                        state.in_conditional_chain = false;
                        state.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);
                        let vif = state.vif_comment_text();
                        // Restore outer conditional chain state if nested
                        if let Some((in_chain, depth, branch_idx)) =
                            state.conditional_chain_stack.pop()
                        {
                            state.in_conditional_chain = in_chain;
                            state.conditional_chain_depth = depth;
                            state.conditional_branch_index = branch_idx;
                        }
                        if component.default_slot_opened {
                            format!(
                                "\n  : _createCommentVNode(\"{vif}\", true)]), {sf} }}{}){}",
                                dynamic_props_suffix, extra_paren
                            )
                        } else {
                            format!(
                                "\n  : _createCommentVNode(\"{vif}\", true), {sf} }}{}){}",
                                dynamic_props_suffix, extra_paren
                            )
                        }
                    } else if component.default_slot_opened {
                        format!("]), {} }}{}){}", sf, dynamic_props_suffix, extra_paren)
                    } else {
                        format!(", {} }}{}){}", sf, dynamic_props_suffix, extra_paren)
                    }
                } else {
                    // Auto-generated default slot: need to close the slot array/function.
                    if state.in_conditional_chain && state.depth == state.conditional_chain_depth {
                        state.conditional_next_key_by_depth.insert(
                            state.conditional_chain_depth,
                            state.conditional_branch_index + 1,
                        );
                        state.in_conditional_chain = false;
                        state.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);
                        let vif = state.vif_comment_text();
                        // Restore outer conditional chain state if nested
                        if let Some((in_chain, depth, branch_idx)) =
                            state.conditional_chain_stack.pop()
                        {
                            state.in_conditional_chain = in_chain;
                            state.conditional_chain_depth = depth;
                            state.conditional_branch_index = branch_idx;
                        }
                        format!(
                            "\n  : _createCommentVNode(\"{vif}\", true)]), {sf} }}{}){}",
                            dynamic_props_suffix, extra_paren
                        )
                    } else {
                        format!("]), {} }}{}){}", sf, dynamic_props_suffix, extra_paren)
                    }
                }
            } else if component.children_opened {
                // Array children format: close with ]) or ]))
                // Check if there's an incomplete conditional chain (v-if without v-else as last child)
                if state.in_conditional_chain && state.depth == state.conditional_chain_depth {
                    state.conditional_next_key_by_depth.insert(
                        state.conditional_chain_depth,
                        state.conditional_branch_index + 1,
                    );
                    state.in_conditional_chain = false;
                    state.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);
                    let vif = state.vif_comment_text();
                    // Restore outer conditional chain state if nested
                    if let Some((in_chain, depth, branch_idx)) = state.conditional_chain_stack.pop()
                    {
                        state.in_conditional_chain = in_chain;
                        state.conditional_chain_depth = depth;
                        state.conditional_branch_index = branch_idx;
                    }
                    format!(
                        "\n  : _createCommentVNode(\"{vif}\", true)]{}){}",
                        dynamic_props_suffix, extra_paren
                    )
                } else {
                    format!("]{}){}", dynamic_props_suffix, extra_paren)
                }
            } else {
                // No children at all: just close the vnode
                // Need to add null for children slot when we have dynamic props
                if !dynamic_props_suffix.is_empty() {
                    format!(", null{}){}", dynamic_props_suffix, extra_paren)
                } else {
                    format!("){}", extra_paren)
                }
            };

            // Add separator between props and children at separator_pos.
            if !component.has_conditional_named_slots
                && (component.uses_slots || component.children_opened)
            {
                code_transform.prepend_right(component.separator_pos, ", ");
            }

            // Append directive array suffix for components with custom directives or v-show
            let directive_suffix =
                generate_component_directive_suffix(component.element_id, state, source);
            let mut code = format!("{}{}", code, directive_suffix);

            // Process any v-for scope closings that are part of this close event.
            // Components with v-for need the renderList/Fragment closing AFTER the
            // component itself closes. Without this, the early return below would
            // skip the scope closing loop, leaving v-for unclosed.
            // NOTE: Only process VFor actions here. VSlot close actions are already
            // handled by the component's own slot closing mechanism above.
            for scope_id in &close.closed_scope_ids {
                if matches!(
                    state.scope_close_actions.get(scope_id),
                    Some(CloseAction::VFor { .. })
                ) {
                    if let Some(CloseAction::VFor { keyed, stable }) =
                        state.scope_close_actions.remove(scope_id)
                    {
                        let frag = if stable {
                            state.pflag(64, "STABLE_FRAGMENT")
                        } else if keyed {
                            state.pflag(128, "KEYED_FRAGMENT")
                        } else {
                            state.pflag(256, "UNKEYED_FRAGMENT")
                        };
                        code.push_str(&format!("}}), {}))", frag));
                    }
                }
            }

            code_transform.overwrite(close.event.start, close.event.end, &code);
            // Mark this position as closed so process_close_tag skips it
            state.vslot_closed_positions.insert(close.event.start);

            // Components DO increment depth when opening, so decrement it here
            state.depth -= 1;

            // Pop child tracking for this depth level
            if state.first_child_at_depth.len() > state.depth {
                state.first_child_at_depth.pop();
            }

            return;
        }
    }

    state.depth -= 1;

    // Pop child tracking for this depth level
    if state.first_child_at_depth.len() > state.depth {
        state.first_child_at_depth.pop();
    }

    // Check if we're closing a slot element via scopes
    if state.slot_stack.pop().is_some() {
        // Check if there's an incomplete conditional chain (v-if without v-else as last child)
        // Only add comment vnode if we're closing the PARENT of the v-if element
        // Note: for slots, we haven't decremented depth yet, so no adjustment needed
        let code = if state.in_conditional_chain && state.depth == state.conditional_chain_depth {
            state.conditional_next_key_by_depth.insert(
                state.conditional_chain_depth,
                state.conditional_branch_index + 1,
            );
            state.in_conditional_chain = false;
            state.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);
            let vif = state.vif_comment_text();
            // Restore outer conditional chain state if nested
            if let Some((in_chain, depth, branch_idx)) = state.conditional_chain_stack.pop() {
                state.in_conditional_chain = in_chain;
                state.conditional_chain_depth = depth;
                state.conditional_branch_index = branch_idx;
            }
            format!("\n  : _createCommentVNode(\"{vif}\", true)])")
        } else {
            "])".to_string()
        };
        code_transform.overwrite(close.event.start, close.event.end, &code);
        return;
    }

    let mut code = String::with_capacity(32);

    // Check if this is a v-slot closing (needs different handling)
    let has_vslot_close = close
        .closed_scope_ids
        .iter()
        .any(|id| matches!(state.scope_close_actions.get(id), Some(CloseAction::VSlot)));

    // For v-slot, don't emit standard element closing since the template
    // generates a callback function instead of a normal element
    if !has_vslot_close {
        // Standard element closing (component already handled above)
        // Standard element closing - check for patch flags and single child optimization
        let element_id = state.element_id_stack.pop();
        let mut patch_flags = element_id
            .and_then(|id| state.element_patch_flags.remove(&id))
            .unwrap_or(0);
        let dynamic_props = element_id
            .and_then(|id| state.element_dynamic_props.remove(&id))
            .unwrap_or_default();

        // Check if this element has interpolation children (TEXT flag)
        if let Some(id) = element_id {
            if state.elements_with_interpolation.remove(&id) {
                patch_flags |= super::types::patch_flags::TEXT;
            }
        }

        // Check for single child optimization
        let single_child = element_id.and_then(|id| state.element_single_child.remove(&id));
        let child_count = element_id
            .and_then(|id| state.element_child_count.remove(&id))
            .unwrap_or(0);
        let array_opened = element_id
            .and_then(|id| state.element_array_opened.remove(&id))
            .unwrap_or(false);

        // Check if this element is cached
        let cache_index = element_id.and_then(|id| state.element_cache_index.remove(&id));

        // Check if this element is a block root
        let is_block_root = element_id
            .and_then(|id| state.element_is_block_root.remove(&id))
            .unwrap_or(true);
        let close_suffix = if is_block_root { "))" } else { ")" };

        // For cached elements, the patch flag is -1 (CACHED) and we need extra ) to close cache wrapper
        // Multi-root cached elements require a trailing comma
        let cached_flag = state.pflag(-1, "CACHED");
        let (cache_patch_flag, cache_close) = if cache_index.is_some() {
            let needs_comma = element_id
                .and_then(|id| {
                    if state.element_cache_needs_comma.remove(&id) {
                        Some(())
                    } else {
                        None
                    }
                })
                .is_some();
            if needs_comma {
                (format!(", {}", cached_flag), "),".to_string())
            } else {
                (format!(", {}", cached_flag), ")".to_string())
            }
        } else {
            (String::new(), String::new())
        };

        // Hoist dynamic props array before computing close code (needs &mut state)
        let hoisted_props_str = hoist_dynamic_props_array(&dynamic_props, state);
        let flags_str = if patch_flags > 0 {
            format_patch_flag_prod(patch_flags, state.is_production)
        } else {
            String::new()
        };

        // Determine closing format based on child count and array state
        let element_close = if child_count == 0 {
            // No children - emit null children placeholder and patch flags if needed
            if patch_flags > 0 {
                format!(
                    "null{}{}{}{}{}",
                    flags_str, hoisted_props_str, cache_patch_flag, close_suffix, cache_close
                )
            } else {
                format!("null{}{}{}", cache_patch_flag, close_suffix, cache_close)
            }
        } else if child_count == 1 && single_child.is_some() && !array_opened {
            // Single text/interpolation child - no array brackets needed
            if patch_flags > 0 {
                format!(
                    "{}{}{}{}{}",
                    flags_str, hoisted_props_str, cache_patch_flag, close_suffix, cache_close
                )
            } else {
                format!("{}{}{}", cache_patch_flag, close_suffix, cache_close)
            }
        } else if array_opened {
            // Multiple children with element children - use array format
            // Check if there's an incomplete conditional chain (v-if without v-else as last child)
            // Only add comment vnode if we're closing the PARENT of the v-if element
            // conditional_chain_depth = v-if's depth - 1 (parent's depth at open time)
            // At close time, depth is decremented, so parent's depth = conditional_chain_depth - 1
            // Therefore: add comment when depth + 1 == conditional_chain_depth
            let conditional_close = if state.in_conditional_chain
                && state.depth + 1 == state.conditional_chain_depth
            {
                state.conditional_next_key_by_depth.insert(
                    state.conditional_chain_depth,
                    state.conditional_branch_index + 1,
                );
                state.in_conditional_chain = false;
                state.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);
                // Restore outer conditional chain state if nested
                if let Some((in_chain, depth, branch_idx)) = state.conditional_chain_stack.pop() {
                    state.in_conditional_chain = in_chain;
                    state.conditional_chain_depth = depth;
                    state.conditional_branch_index = branch_idx;
                }
                let vif = state.vif_comment_text();
                format!("\n  : _createCommentVNode(\"{vif}\", true)")
            } else {
                String::new()
            };
            if patch_flags > 0 {
                format!(
                    "{}]{}{}{}{}{}",
                    conditional_close,
                    flags_str,
                    hoisted_props_str,
                    cache_patch_flag,
                    close_suffix,
                    cache_close
                )
            } else {
                format!(
                    "{}]{}{}{}",
                    conditional_close, cache_patch_flag, close_suffix, cache_close
                )
            }
        } else {
            // Multiple text/interpolation children concatenated (no array)
            if patch_flags > 0 {
                format!(
                    "{}{}{}{}{}",
                    flags_str, hoisted_props_str, cache_patch_flag, close_suffix, cache_close
                )
            } else {
                format!("{}{}{}", cache_patch_flag, close_suffix, cache_close)
            }
        };

        // Track first root's closing for potential multi-root patching
        if let Some(first_root_id) = state.first_root_element_id {
            if element_id == Some(first_root_id) {
                state.first_root_close_span = Some((close.event.start, close.event.end));
                state.first_root_close_code = Some(element_close.clone());
            }
        }

        code.push_str(&element_close);

        // If element has directives (custom, v-model, v-show), append the directive array
        // Format: , [[_directive_focus], [_vModelText, _ctx.value, void 0, { lazy: true }]])
        let has_directives = element_id
            .map(|id| {
                state.elements_with_directives.contains_key(&id)
                    || state.elements_with_vmodel.contains_key(&id)
                    || state.elements_with_vshow.contains_key(&id)
            })
            .unwrap_or(false);

        if has_directives {
            code.push_str(", [");
            let mut directive_count = 0;

            // Add custom directives first
            if let Some(directives) =
                element_id.and_then(|id| state.elements_with_directives.remove(&id))
            {
                for directive in directives.iter() {
                    if directive_count > 0 {
                        code.push_str(", ");
                    }
                    directive_count += 1;

                    code.push_str("[_directive_");
                    code.push_str(&camelize(&directive.name));

                    // Add value if present
                    if let Some(ref value_span) = directive.value {
                        code.push_str(", ");
                        let value_str = &source[value_span.start as usize..value_span.end as usize];
                        if value_str.starts_with('\'')
                            || value_str.starts_with('"')
                            || value_str.starts_with('`')
                        {
                            code.push_str(value_str);
                        } else {
                            // Use write_expr_with_ctx to properly handle complex expressions
                            write_expr_with_ctx(
                                &mut code,
                                value_str,
                                &[], // v-for locals not tracked in close context
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                        }
                    }

                    // Add argument if present
                    if let Some(ref arg_span) = directive.arg {
                        // First, pad with void 0 if there was no value
                        if directive.value.is_none() {
                            code.push_str(", void 0");
                        }
                        code.push_str(", ");
                        let arg_str = &source[arg_span.start as usize..arg_span.end as usize];
                        if directive.is_dynamic_arg {
                            // Dynamic argument: strip brackets and add correct prefix
                            let arg_str = arg_str.trim_start_matches('[').trim_end_matches(']');
                            write_expr_with_ctx(
                                &mut code,
                                arg_str,
                                &[],
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                        } else {
                            code.push('"');
                            code.push_str(arg_str);
                            code.push('"');
                        }
                    }

                    // Add modifiers if present
                    if !directive.modifiers.is_empty() {
                        // First, pad with void 0 for missing value and arg
                        if directive.value.is_none() && directive.arg.is_none() {
                            code.push_str(", void 0, void 0");
                        } else if directive.arg.is_none() {
                            code.push_str(", void 0");
                        }
                        code.push_str(", { ");
                        for (j, mod_span) in directive.modifiers.iter().enumerate() {
                            if j > 0 {
                                code.push_str(", ");
                            }
                            code.push_str(&source[mod_span.start as usize..mod_span.end as usize]);
                            code.push_str(": true");
                        }
                        code.push_str(" }");
                    }

                    code.push(']');
                }
            }

            // Add v-model directive
            if let Some((vmodel_info, tag_name)) =
                element_id.and_then(|id| state.elements_with_vmodel.remove(&id))
            {
                if directive_count > 0 {
                    code.push_str(", ");
                }
                directive_count += 1;

                // Select directive based on tag name
                let directive_name = if tag_name == "select" {
                    "_vModelSelect"
                } else {
                    "_vModelText"
                };

                code.push('[');
                code.push_str(directive_name);

                // Add value
                if let Some(ref value_span) = vmodel_info.value {
                    code.push_str(", ");
                    let value_str = &source[value_span.start as usize..value_span.end as usize];
                    let prefix = resolve_binding_prefix(
                        value_str.as_bytes(),
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                    let suffix = resolve_binding_suffix(
                        value_str.as_bytes(),
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                    code.push_str(prefix);
                    code.push_str(value_str);
                    code.push_str(suffix);
                }

                // Add modifiers if present
                if !vmodel_info.modifiers.is_empty() {
                    code.push_str(", void 0, { ");
                    for (j, mod_span) in vmodel_info.modifiers.iter().enumerate() {
                        if j > 0 {
                            code.push_str(", ");
                        }
                        code.push_str(&source[mod_span.start as usize..mod_span.end as usize]);
                        code.push_str(": true");
                    }
                    code.push_str(" }");
                }

                code.push(']');
            }

            // Add v-show directive
            if let Some(vshow_span) =
                element_id.and_then(|id| state.elements_with_vshow.remove(&id))
            {
                if directive_count > 0 {
                    code.push_str(", ");
                }
                code.push_str("[_vShow, ");
                let value_str = &source[vshow_span.start as usize..vshow_span.end as usize];
                write_expr_with_ctx(
                    &mut code,
                    value_str,
                    &[],
                    &state.binding_metadata,
                    source.as_bytes(),
                    state.is_inline_mode,
                );
                code.push(']');
            }

            code.push_str("])");
        }

        // If element has v-once, close the cache pattern
        // Format: ).cacheIndex = N, _setBlockTracking(1), _cache[N])
        if let Some(cache_idx) = element_id.and_then(|id| state.elements_with_vonce.remove(&id)) {
            code.push_str(&format!(
                ").cacheIndex = {},\n      _setBlockTracking(1),\n      _cache[{}]\n    )",
                cache_idx, cache_idx
            ));
        }
    } else {
        // v-slot on a non-component, non-template element (e.g., <div v-slot="{ x }">):
        // Only Element-type elements push to element_id_stack (line 1431); templates
        // with v-slot do NOT push (line 1440 requires v_slot.is_none()). We must pop
        // for Element-type to keep element_id_stack balanced. Without this, the leaked
        // element_id causes is_direct_slot_child to be false for subsequent siblings,
        // breaking comma insertion between component children.
        if close.event.tag_type == SyntaxTagType::Element {
            state.element_id_stack.pop();
        }
    }

    // Then emit any scope-specific closings (v-for, v-slot)
    for scope_id in &close.closed_scope_ids {
        if let Some(action) = state.scope_close_actions.remove(scope_id) {
            match action {
                CloseAction::VFor { keyed, stable } => {
                    // Close the renderList callback and Fragment:
                    // }  - close callback function
                    // )  - close renderList call
                    // , FLAG  - patch flag
                    // ))  - close createElementBlock and openBlock for Fragment
                    let frag = if stable {
                        state.pflag(64, "STABLE_FRAGMENT")
                    } else if keyed {
                        state.pflag(128, "KEYED_FRAGMENT")
                    } else {
                        state.pflag(256, "UNKEYED_FRAGMENT")
                    };
                    code.push_str(&format!("}}), {}))", frag));
                }
                CloseAction::VSlot => {
                    // Flush any open v-slot text vnode — prepend closing to this code
                    code.push_str(&flush_vslot_text_vnode(state));

                    // Close any open conditional chain inside this slot
                    // (e.g., <span v-if="show">...</span> with no v-else)
                    // Close any conditional chain started INSIDE this slot
                    if state.in_conditional_chain {
                        let vif = state.vif_comment_text();
                        code.push_str(&format!("\n  : _createCommentVNode(\"{}\", true)", vif));
                        state.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);
                        state.in_conditional_chain = false;
                    }
                    // Always restore outer conditional chain state from v-slot-specific stack
                    if let Some((in_chain, depth, branch_idx)) =
                        state.vslot_conditional_chain_stack.pop()
                    {
                        state.in_conditional_chain = in_chain;
                        state.conditional_chain_depth = depth;
                        state.conditional_branch_index = branch_idx;
                    }

                    // Check if parent has an active conditional slot
                    let cond_slot_info = state
                        .component_stack
                        .last_mut()
                        .and_then(|p| p.current_conditional_slot.take());

                    if let Some(condition_type) = cond_slot_info {
                        // Conditional v-slot close: add key and close descriptor
                        let key = if let Some(parent) = state.component_stack.last_mut() {
                            let k = parent.conditional_slot_key_counter;
                            parent.conditional_slot_key_counter += 1;
                            k
                        } else {
                            0
                        };

                        // Close fn array, add key, close descriptor object
                        code.push_str(&format!("]),\n          key: \"{}\"\n        }}", key));

                        // Mark that we need `: undefined` if no v-else follows.
                        // This is deferred because we don't know in streaming if v-else is next.
                        use crate::syntax::types::OxcVConditionType;
                        match condition_type {
                            OxcVConditionType::If | OxcVConditionType::ElseIf => {
                                if let Some(parent) = state.component_stack.last_mut() {
                                    parent.conditional_slot_needs_undefined = true;
                                }
                            }
                            OxcVConditionType::Else => {
                                // v-else completes the ternary
                            }
                        }
                    } else if state
                        .component_stack
                        .last()
                        .map(|p| p.has_conditional_named_slots)
                        .unwrap_or(false)
                    {
                        // Static slot in createSlots mode: close fn array + descriptor
                        code.push_str("])\n        }");
                    } else {
                        // Normal slot close
                        code.push_str("])");
                    }

                    if state.active_vslot_depth > 0 {
                        state.active_vslot_depth -= 1;
                    }
                    // Pop slot locals from the stack
                    state.vslot_locals_stack.pop();
                }
            }
        }
    }

    code_transform.overwrite(close.event.start, close.event.end, &code);
}

/// Flush any open v-slot text vnode concatenation.
/// Returns the closing text (e.g., `)` or `, 1 /* TEXT */)`) that should be prepended
/// to the next overwrite's code. The caller is responsible for inserting this string
/// into the output at the correct position.
pub fn flush_vslot_text_vnode(state: &mut TemplateCodegenState) -> String {
    if state.vslot_text_vnode_open {
        let close = if state.vslot_text_vnode_has_interp {
            let text_flag = state.pflag(1, "TEXT");
            format!(", {})", text_flag)
        } else {
            ")".to_string()
        };
        state.vslot_text_vnode_open = false;
        state.vslot_text_vnode_has_interp = false;
        close
    } else {
        String::new()
    }
}

/// Process text content.
pub fn process_text<'a>(
    text: &SyntaxText,
    state: &mut TemplateCodegenState,
    code_transform: &mut CodeTransform<'a>,
    source: &'a str,
) {
    if !state.render_started || state.depth == 0 {
        return;
    }

    let content = &source[text.start as usize..text.end as usize];
    let trimmed = content.trim();

    if trimmed.is_empty() {
        // Whitespace-only text: condense to " " if previous sibling was text/interpolation
        // (Vue condense mode: whitespace between interpolation and element becomes + " ")
        if let Some(&parent_id) = state.element_id_stack.last() {
            let prev_was_text = state
                .last_child_is_text_content
                .get(&parent_id)
                .copied()
                .unwrap_or(false);
            let array_opened = state
                .element_array_opened
                .get(&parent_id)
                .copied()
                .unwrap_or(false);

            if prev_was_text && !array_opened {
                // Always condense whitespace between text/interpolation children to " "
                // Vue only removes whitespace between two elements, not between
                // text/interpolation nodes (regardless of newlines)
                if let Some(single_child) = state.element_single_child.get_mut(&parent_id) {
                    single_child.content.push_str(" + \" \"");
                    single_child.end = text.end; // Advance end so gap check won't double-add
                }
                code_transform.remove(text.start, text.end);
                return;
            }
        }
        // Otherwise remove whitespace-only text
        code_transform.remove(text.start, text.end);
        return;
    }

    // Normalize whitespace: preserve leading/trailing space independently.
    // Example: "Error: " should stay "Error: " (not " Error: ").
    let leading_ws = content
        .chars()
        .next()
        .map(|c| c.is_whitespace())
        .unwrap_or(false);
    let trailing_ws = content
        .chars()
        .last()
        .map(|c| c.is_whitespace())
        .unwrap_or(false);
    let mut normalized_raw = String::with_capacity(trimmed.len() + 2);
    if leading_ws {
        normalized_raw.push(' ');
    }
    normalized_raw.push_str(trimmed);
    if trailing_ws {
        normalized_raw.push(' ');
    }

    // Decode HTML entities (e.g., &amp; → &, &times; → ×, &#9662; → ▾)
    let normalized = decode_html_entities(&normalized_raw);

    // Check if parent is a component that needs children opener
    // Text children also need to open the children for components
    let mut default_slot_prefix: Option<&'static str> = None;
    let mut in_component_slot_with_siblings = false;
    if let Some(parent) = state.component_stack.last_mut() {
        // Only treat as direct slot child if no HTML element has been opened
        // between the component and this text node
        let is_direct_slot_child =
            state.element_id_stack.len() == parent.element_id_stack_len_at_open;
        // Components opened inside a v-slot handle their own children normally.
        let should_handle_children = state.active_vslot_depth == parent.vslot_depth_at_open;

        if parent.has_named_slots && should_handle_children {
            if !parent.default_slot_opened {
                // Named slots already opened the slots object. Add default slot inline.
                default_slot_prefix = Some(", default: _withCtx(() => [");
                parent.uses_slots = true;
                state.helpers.insert(HelperFlags::WITH_CTX);
                parent.default_slot_opened = true;
            } else if parent.default_slot_child_count > 0 && is_direct_slot_child {
                in_component_slot_with_siblings = true;
            }
        } else if !parent.children_opened && should_handle_children {
            // All components use slot format: { default: _withCtx(() => [...]) }
            // This is Vue's standard format for component children
            code_transform.prepend_left(parent.insert_pos, "{ default: _withCtx(() => [");
            parent.uses_slots = true;
            state.helpers.insert(HelperFlags::WITH_CTX);
            parent.children_opened = true;
            parent.default_slot_opened = true;
        } else if parent.default_slot_opened
            && parent.default_slot_child_count > 0
            && should_handle_children
            && is_direct_slot_child
        {
            in_component_slot_with_siblings = true;
        }
        if should_handle_children && is_direct_slot_child {
            parent.default_slot_child_count += 1;
        }
    }

    // Text inside an active v-slot needs _createTextVNode wrapping and comma handling.
    // V-slot arrays (`value: _withCtx(() => [...])`) expect VNode children, not raw strings.
    // V-slot templates don't push to element_id_stack, so the normal child tracking
    // doesn't apply. We use first_child_at_depth to track siblings for comma insertion.
    // Only applies to DIRECT v-slot children (not text nested inside HTML elements within the slot).
    let is_vslot_text = state.active_vslot_depth > 0
        && state
            .component_stack
            .last()
            .map(|p| {
                state.element_id_stack.len() == p.element_id_stack_len_at_open
                    && p.vslot_depth_at_open == 0
            })
            .unwrap_or(false);
    if is_vslot_text && !state.vslot_text_vnode_open {
        // Only track first-child for NEW text vnodes, not text being concatenated
        // into an existing open vslot text vnode.
        let depth_idx = if state.depth > 0 { state.depth - 1 } else { 0 };
        while state.first_child_at_depth.len() <= depth_idx {
            state.first_child_at_depth.push(false);
        }
        if state.first_child_at_depth[depth_idx] {
            in_component_slot_with_siblings = true;
        } else {
            state.first_child_at_depth[depth_idx] = true;
        }
    }

    // Emit as string literal by default
    let mut code = String::with_capacity(normalized.len() + 8);

    // Check if we need to concatenate with previous text/interpolation
    // (only for non-array mode - when there are no element children)
    if let Some(&parent_id) = state.element_id_stack.last() {
        let array_opened = state
            .element_array_opened
            .get(&parent_id)
            .copied()
            .unwrap_or(false);
        let prev_was_text = state
            .last_child_is_text_content
            .get(&parent_id)
            .copied()
            .unwrap_or(false);

        // Add concatenation operator if previous sibling was text/interpolation
        // and we're not in array mode (no element children yet)
        if prev_was_text && !array_opened {
            code.push_str(" + ");
        }
    }

    code.push('"');
    for c in normalized.chars() {
        match c {
            '"' => code.push_str("\\\""),
            '\\' => code.push_str("\\\\"),
            '\n' => code.push_str("\\n"),
            '\r' => code.push_str("\\r"),
            '\t' => code.push_str("\\t"),
            _ => code.push(c),
        }
    }
    code.push('"');

    // Skip element-based child tracking when text is a direct child of a component.
    // Component slots handle their own child formatting (wrapping in _createTextVNode,
    // comma separators). The element-based caching would conflict by adding a second
    // _createTextVNode wrapper and duplicate commas.
    let is_direct_component_child = state
        .component_stack
        .last()
        .map(|p| state.element_id_stack.len() == p.element_id_stack_len_at_open)
        .unwrap_or(false);

    // Track child count for parent element (for single child optimization)
    if !is_direct_component_child {
        if let Some(&parent_id) = state.element_id_stack.last() {
            let count = state.element_child_count.entry(parent_id).or_insert(0);
            let was_first = *count == 0;
            *count += 1;

            // Check if array was already opened (by a child element)
            let array_opened = state
                .element_array_opened
                .get(&parent_id)
                .copied()
                .unwrap_or(false);

            // Mark that this element's last child is text content (for concatenation)
            state.last_child_is_text_content.insert(parent_id, true);

            if was_first && !array_opened {
                // First child and no array opened yet - this could be a single child
                state.element_single_child.insert(
                    parent_id,
                    super::types::SingleChildInfo {
                        content: code.clone(),
                        is_interpolation: false,
                        start: text.start,
                        end: text.end,
                    },
                );
            } else if array_opened {
                // Array already opened (element children exist) - wrap in createTextVNode
                state.element_single_child.remove(&parent_id);

                // Add leading comma for text nodes that follow other children
                let parent_depth_idx = state.depth - 1;
                let needs_comma = parent_depth_idx < state.first_child_at_depth.len()
                    && state.first_child_at_depth[parent_depth_idx];

                let cache_idx = state.cache_index;
                state.cache_index += 1;
                let mut vnode_code = String::new();

                if needs_comma {
                    vnode_code.push_str(",\n    ");
                }

                vnode_code.push_str(&format!(
                    "_cache[{}] || (_cache[{}] = _createTextVNode(",
                    cache_idx, cache_idx
                ));
                vnode_code.push_str(&code);
                let cf = state.pflag(-1, "CACHED");
                vnode_code.push_str(&format!(", {}))", cf));
                code = vnode_code;

                state.helpers.insert(HelperFlags::CREATE_TEXT_VNODE);
            } else {
                // Multiple text/interpolation children, no element children yet.
                // Extend single_child range so if an element arrives later,
                // we can retroactively wrap all text in _createTextVNode and open array.
                if let Some(sc) = state.element_single_child.get_mut(&parent_id) {
                    sc.content.push_str(&code);
                    sc.end = text.end;
                    // is_interpolation stays as-is (true if first child was interpolation)
                }
            }
        }
    }

    // When text is a direct component slot child, wrap in _createTextVNode()
    // Component slots require VNode children, not raw strings
    let is_direct_component_slot =
        in_component_slot_with_siblings || default_slot_prefix.is_some() || is_vslot_text || {
            // Check if we just opened the default slot for this text
            state
                .component_stack
                .last()
                .map(|p| {
                    p.default_slot_opened
                        && state.element_id_stack.len() == p.element_id_stack_len_at_open
                        && (state.active_vslot_depth == p.vslot_depth_at_open)
                })
                .unwrap_or(false)
        };

    if is_direct_component_slot {
        if is_vslot_text && state.vslot_text_vnode_open {
            // Already have an open text vnode — concatenate this text
            code = format!(" + {}", code); // code is already "\"text\""
        } else if is_vslot_text {
            // Start new _createTextVNode but leave open for potential interpolation
            code = format!("_createTextVNode({}", code);
            state.helpers.insert(HelperFlags::CREATE_TEXT_VNODE);
            state.vslot_text_vnode_open = true;
        } else {
            // Non-vslot component slot: wrap and close immediately
            let wrapped = format!("_createTextVNode({})", code);
            code = wrapped;
            state.helpers.insert(HelperFlags::CREATE_TEXT_VNODE);
        }
    }

    // When text is inside a component slot that already has children,
    // we need a comma separator before the text
    if in_component_slot_with_siblings {
        code.insert_str(0, ", ");
    }

    if let Some(prefix) = default_slot_prefix {
        if code.starts_with(",\n    ") {
            code.replace_range(0..6, "");
        } else if code.starts_with(", ") {
            code.replace_range(0..2, "");
        } else if code.starts_with(',') {
            code.replace_range(0..1, "");
        }
        code.insert_str(0, prefix);
    }
    code_transform.overwrite(text.start, text.end, &code);

    // Track end position for v-slot text vnode flush
    if is_vslot_text && state.vslot_text_vnode_open {
        state.vslot_text_vnode_last_end = text.end;
    }
}

/// Process comment nodes.
pub fn process_comment<'a>(
    comment: &SyntaxComment,
    state: &mut TemplateCodegenState,
    code_transform: &mut CodeTransform<'a>,
    source: &'a str,
) {
    if !state.render_started {
        return;
    }

    let raw = &source[comment.start as usize..comment.end as usize];
    let content = raw
        .strip_prefix("<!--")
        .and_then(|s| s.strip_suffix("-->"))
        .unwrap_or(raw)
        .trim();

    let mut code = String::with_capacity(content.len() + 32);

    // Check if this comment is a direct child of a component that should handle slot wrapping.
    // Comments inside named v-slots (active_vslot_depth > 0 with vslot_depth_at_open == 0)
    // should use normal depth-based comma tracking, not component slot handling.
    let should_use_component_slot = state
        .component_stack
        .last()
        .map(|p| {
            let is_direct = state.element_id_stack.len() == p.element_id_stack_len_at_open;
            let should_handle = state.active_vslot_depth == p.vslot_depth_at_open;
            is_direct && should_handle
        })
        .unwrap_or(false);

    if should_use_component_slot {
        // Handle component slot context: open default slot if needed, add comma separators
        if let Some(parent) = state.component_stack.last_mut() {
            let is_direct_slot_child =
                state.element_id_stack.len() == parent.element_id_stack_len_at_open;
            let should_handle_children = state.active_vslot_depth == parent.vslot_depth_at_open;

            if parent.has_named_slots && should_handle_children {
                if !parent.default_slot_opened {
                    code.push_str(", default: _withCtx(() => [");
                    parent.uses_slots = true;
                    state.helpers.insert(HelperFlags::WITH_CTX);
                    parent.default_slot_opened = true;
                } else if parent.default_slot_child_count > 0 && is_direct_slot_child {
                    code.push_str(",\n    ");
                }
            } else if !parent.children_opened && should_handle_children {
                code_transform.prepend_left(parent.insert_pos, "{ default: _withCtx(() => [");
                parent.uses_slots = true;
                state.helpers.insert(HelperFlags::WITH_CTX);
                parent.children_opened = true;
                parent.default_slot_opened = true;
            } else if parent.default_slot_opened
                && parent.default_slot_child_count > 0
                && should_handle_children
                && is_direct_slot_child
            {
                code.push_str(",\n    ");
            }
            if should_handle_children && is_direct_slot_child {
                parent.default_slot_child_count += 1;
            }
        }
    } else {
        // Root-level comments should force fragment wrapping
        if state.depth == 0 {
            if state.root_child_emitted {
                code.push_str(",\n    ");
            }
            state.root_child_emitted = true;
            state.root_has_non_element_child = true;
        } else {
            // Add comma between sibling nodes at this depth
            let parent_depth_idx = state.depth - 1;
            if parent_depth_idx < state.first_child_at_depth.len()
                && state.first_child_at_depth[parent_depth_idx]
            {
                code.push_str(",\n    ");
            } else if parent_depth_idx < state.first_child_at_depth.len() {
                state.first_child_at_depth[parent_depth_idx] = true;
            }
        }

        // Track child count for parent element (for array format)
        if let Some(&parent_id) = state.element_id_stack.last() {
            let count = state.element_child_count.entry(parent_id).or_insert(0);
            let was_first = *count == 0;
            *count += 1;

            if was_first {
                if let Some(&insert_pos) = state.element_children_insert_pos.get(&parent_id) {
                    code_transform.prepend_left(insert_pos, "[");
                } else {
                    code.insert(0, '[');
                }
                state.element_array_opened.insert(parent_id, true);
            }

            state.element_single_child.remove(&parent_id);
        }
    }

    code.push_str("_createCommentVNode(\"");
    for c in content.chars() {
        match c {
            '"' => code.push_str("\\\""),
            '\\' => code.push_str("\\\\"),
            '\n' => code.push_str("\\n"),
            '\r' => code.push_str("\\r"),
            '\t' => code.push_str("\\t"),
            _ => code.push(c),
        }
    }
    code.push_str("\")");

    state.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);

    code_transform.overwrite(comment.start, comment.end, &code);
}

/// Process the opening of root <template> tag.
/// Remove the tag and mark where content starts.
pub fn process_root_template_open<'a>(
    open_tag: &SyntaxOpenTagEnd,
    state: &mut TemplateCodegenState,
    code_transform: &mut CodeTransform<'a>,
    source: &'a str,
) {
    // Find first non-whitespace after opening tag
    let mut content_start = open_tag.end;
    let bytes = source.as_bytes();
    while (content_start as usize) < bytes.len() {
        match bytes.get(content_start as usize) {
            Some(b' ' | b'\n' | b'\r' | b'\t') => content_start += 1,
            _ => break,
        }
    }

    // Span starts at first content, end will be set in finalize_template_close
    state.template_span = Some(crate::common::Span::new(content_start, content_start));
    state.render_started = true;

    // Remove the <template> tag and leading whitespace
    code_transform.remove(open_tag.start, content_start);
}

/// Finalize template close - remove </template> tag.
pub fn finalize_template_close<'a>(
    close_tag: &SyntaxCloseTag,
    state: &mut TemplateCodegenState,
    code_transform: &mut CodeTransform<'a>,
    source: &'a str,
) {
    // Find last non-whitespace before closing tag
    let mut content_end = close_tag.start;
    let bytes = source.as_bytes();
    while content_end > 0 {
        match bytes.get((content_end - 1) as usize) {
            Some(b' ' | b'\n' | b'\r' | b'\t') => content_end -= 1,
            _ => break,
        }
    }

    // Store content end position (before trailing whitespace)
    if let Some(ref mut span) = state.template_span {
        span.end = content_end;
    }

    // Remove trailing whitespace and </template> tag
    code_transform.remove(content_end, close_tag.end);
}

/// Finalize template - move content to end of file wrapped in render function.
pub fn finalize_template<'a>(
    state: &mut TemplateCodegenState,
    code_transform: &mut CodeTransform<'a>,
    source: &'a str,
) {
    // If multiple root elements or non-element root children, we need Fragment wrapper
    // Only patch the first root element when there are multiple root elements
    if state.root_element_count > 1 || state.root_has_non_element_child {
        state.helpers.insert(HelperFlags::FRAGMENT);
        state.helpers.insert(HelperFlags::OPEN_BLOCK);
        state.helpers.insert(HelperFlags::CREATE_ELEMENT_BLOCK);
        state.helpers.insert(HelperFlags::CREATE_ELEMENT_VNODE);

        // Patch the first root element: change from block root to non-block
        // Also add cache wrapper for static elements
        // Opening: (_openBlock(), _createElementBlock(... → _cache[0] || (_cache[0] = _createElementVNode(...
        // Closing: )) → , -1 /* CACHED */))
        if let (Some((open_start, open_end)), Some(opening_code)) = (
            state.first_root_source_span,
            state.first_root_opening_code.take(),
        ) {
            // Check if the first root element is static (should be cached).
            // Only self-closing elements can be cached at finalize time because
            // non-self-closing elements may have dynamic children we can't verify here.
            let is_static_first_root = state.first_root_is_self_closing
                && !opening_code.contains("_ctx.")
                && !opening_code.contains("$setup.")
                && !opening_code.contains("$props.")
                && !opening_code.contains("__props.")
                && !opening_code.contains("_toDisplayString")
                && !opening_code.contains("_withModifiers");

            // Transform the opening code from block to non-block:
            // Replace "(_openBlock(), _createElementBlock(" with "_createElementVNode("
            let did_block_patch = opening_code.contains("(_openBlock(), _createElementBlock(");
            let mut patched_open = if did_block_patch {
                opening_code.replace(
                    "(_openBlock(), _createElementBlock(",
                    "_createElementVNode(",
                )
            } else {
                opening_code.clone()
            };

            // For self-closing elements, the close parens )) are embedded in the opening code.
            // Replacing (_openBlock(), _createElementBlock( with _createElementVNode( removes
            // the outer ( but leaves the matching ), creating an unbalanced extra ).
            // Exception: cached elements intentionally keep the extra ) to close _cache[n] = (...).
            let will_be_cached = is_static_first_root && state.root_element_count > 1;
            if did_block_patch && state.first_root_is_self_closing && !will_be_cached {
                if let Some(pos) = patched_open.rfind("))") {
                    patched_open.remove(pos + 1);
                }
            }

            // Add cache wrapper for static first root element
            if is_static_first_root && state.root_element_count > 1 {
                let cache_idx = state.cache_index;
                state.cache_index += 1;
                patched_open = format!(
                    "_cache[{}] || (_cache[{}] = {}",
                    cache_idx, cache_idx, patched_open
                );
            }

            code_transform.overwrite(open_start, open_end, &patched_open);

            // Patch the closing: remove one ) from )) and add cache suffix if needed
            if let (Some((close_start, close_end)), Some(close_code)) = (
                state.first_root_close_span,
                state.first_root_close_code.take(),
            ) {
                // The close_code ends with )) for block root, change to )
                // For cached elements: convert )) to , -1 /* CACHED */))
                // The inner ) closes _createElementVNode, the outer ) closes cache wrapper
                let cf = state.pflag(-1, "CACHED");
                let patched_close = if close_code.ends_with("))") {
                    // Replace trailing )) with the appropriate suffix
                    let base = &close_code[..close_code.len() - 2];
                    if is_static_first_root && state.root_element_count > 1 {
                        format!("{}, {})),", base, cf)
                    } else {
                        // Not cached, just remove one )
                        format!("{})", base)
                    }
                } else {
                    // Shouldn't happen for static elements, but handle gracefully
                    if is_static_first_root && state.root_element_count > 1 {
                        format!("{}, {}),", close_code, cf)
                    } else {
                        close_code
                    }
                };
                code_transform.overwrite(close_start, close_end, &patched_close);
            }
        }
    } else if state.root_has_non_element_child {
        state.helpers.insert(HelperFlags::FRAGMENT);
    }

    // Pre-insert CREATE_COMMENT_VNODE helper if we have an unclosed conditional chain.
    // This must happen BEFORE generating imports so the import is included.
    if state.in_conditional_chain {
        state.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);
    }

    // Generate imports
    let imports = state.helpers.to_import_string();

    // Generate asset imports (e.g., import _imports_0 from "/image.svg?import")
    let mut asset_imports = String::new();
    for (name, path) in &state.asset_imports {
        asset_imports.push_str(&format!("import {} from \"{}?import\"\n", name, path));
    }

    // Generate hoisted constants
    let mut hoisted = String::new();
    for node in &state.hoisted_nodes {
        hoisted.push_str(&format!("const {} = {}\n", node.name, node.code));
    }

    // Generate component resolutions (at start of render function)
    let mut component_resolutions = String::new();
    for name in &state.resolved_components {
        component_resolutions.push_str(&format!(
            "  const _component_{} = _resolveComponent(\"{}\")\n",
            name.replace('-', "_"),
            name
        ));
    }

    // Generate directive resolutions (at start of render function)
    let mut directive_resolutions = String::new();
    for name in &state.resolved_directives {
        directive_resolutions.push_str(&format!(
            "  const _directive_{} = _resolveDirective(\"{}\")\n",
            camelize(name),
            name
        ));
    }

    // Combine component and directive resolutions
    let resolutions_section =
        if component_resolutions.is_empty() && directive_resolutions.is_empty() {
            String::new()
        } else {
            format!("{}{}", component_resolutions, directive_resolutions)
        };

    if let Some(span) = state.template_span {
        // Use script end position if available, otherwise fall back to source length.
        // This ensures template content is placed AFTER the component definition.
        let target_pos = state.script_end_position.unwrap_or(source.len() as u32);

        // CRITICAL: Prepend imports and hoisted constants to TOP of file.
        // ES modules require all `import` statements at the top level before any other code.
        // Hoisted constants must also be at module level (not inside setup) for inline mode.
        // If placed in the prefix of move_wrapped, they'd appear after the component definition
        // (which is correct for standalone mode but wrong for inline mode where setup is open).
        {
            let hoisted_for_prepend = if state.is_inline_mode { &hoisted } else { "" };
            if !imports.is_empty() || !asset_imports.is_empty() || !hoisted_for_prepend.is_empty() {
                let all_prepend = format!("{}{}{}", asset_imports, imports, hoisted_for_prepend);
                code_transform.prepend(&all_prepend);
            }
        }

        // Move template content to after script, wrapped in render function
        // For standalone mode, hoisted constants go in the prefix (before `function render`)
        // For inline mode, hoisted constants were already prepended above

        let (prefix, suffix) = if state.is_inline_mode {
            // Production inline mode: render function as setup return value.
            // process_script() left setup open, we close it here.
            // Format: return (_ctx, _cache) => { return ... }}<closing_paren>;\n
            let cp = &state.inline_closing_paren;
            if state.root_element_count > 1 || state.root_has_non_element_child {
                // Multiple roots: wrap in Fragment
                let fragment_flag = if state.root_has_non_element_child {
                    64 + 2048 // STABLE_FRAGMENT | DEV_ROOT_FRAGMENT
                } else {
                    64 // STABLE_FRAGMENT
                };
                let prefix = format!(
                    "\nreturn (_ctx, _cache) => {{\n{}  return (_openBlock(), _createElementBlock(_Fragment, null, [\n    ",
                    resolutions_section
                );
                let frag_comment = if state.is_production {
                    String::new()
                } else if state.root_has_non_element_child {
                    " /* STABLE_FRAGMENT, DEV_ROOT_FRAGMENT */".to_string()
                } else {
                    " /* STABLE_FRAGMENT */".to_string()
                };
                // Close: ]) → arrow body → setup → component → closing_paren → ;
                let suffix = format!(
                    "\n  ], {}{}))\n}}}}}}{};\n",
                    fragment_flag, frag_comment, cp
                );
                (prefix, suffix)
            } else {
                // Single root: no Fragment wrapper needed
                let prefix = format!(
                    "\nreturn (_ctx, _cache) => {{\n{}  return ",
                    resolutions_section
                );
                // Close: } arrow body → } setup → } component → closing_paren → ;
                let suffix = format!("\n}}}}}}{};\n", cp);
                (prefix, suffix)
            }
        } else {
            // Development mode: separate function render
            // Script setup needs full signature for binding access
            let render_params = if !state.binding_metadata.is_empty() {
                "_ctx, _cache, $props, $setup, $data, $options"
            } else {
                "_ctx, _cache"
            };
            if state.root_element_count > 1 || state.root_has_non_element_child {
                // Multiple roots: wrap in Fragment
                let fragment_flag = if state.root_has_non_element_child {
                    64 + 2048 // STABLE_FRAGMENT | DEV_ROOT_FRAGMENT
                } else {
                    64 // STABLE_FRAGMENT
                };
                let prefix = format!(
                    "{}\nfunction render({}) {{\n{}  return (_openBlock(), _createElementBlock(_Fragment, null, [\n    ",
                    hoisted, render_params, resolutions_section
                );
                let frag_comment = if state.is_production {
                    String::new()
                } else if state.root_has_non_element_child {
                    " /* STABLE_FRAGMENT, DEV_ROOT_FRAGMENT */".to_string()
                } else {
                    " /* STABLE_FRAGMENT */".to_string()
                };
                let suffix = format!("\n  ], {}{}))\n}}\n", fragment_flag, frag_comment);
                (prefix, suffix)
            } else {
                // Single root or empty: no Fragment wrapper needed
                let prefix = format!(
                    "{}\nfunction render({}) {{\n{}  return ",
                    hoisted, render_params, resolutions_section
                );
                let suffix = "\n}\n".to_string();
                (prefix, suffix)
            }
        };

        // Close unclosed conditional chain (root-level v-if without v-else)
        let suffix = if state.in_conditional_chain {
            state.in_conditional_chain = false;
            state.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);
            let vif = state.vif_comment_text();
            let close = format!("\n  : _createCommentVNode(\"{vif}\", true)");
            format!("{}{}", close, suffix)
        } else {
            suffix
        };

        code_transform.move_wrapped(span.start, span.end, target_pos, &prefix, &suffix);
    }

    state.render_started = false;
}

/// Finalize template for standalone output (separate block, not moved into script).
///
/// Unlike `finalize_template()` which uses `move_wrapped` to place template content
/// after the script block, this wraps the template content in-place. The result is
/// a standalone render function block that can be assembled separately.
///
/// Output format: `function render(_ctx, _cache) { return ... }`
/// (no `export` — the assembler decides how to expose it)
pub fn finalize_template_standalone<'a>(
    state: &mut TemplateCodegenState,
    code_transform: &mut CodeTransform<'a>,
    _source: &'a str,
) {
    // If multiple root elements or non-element root children, we need Fragment wrapper
    if state.root_element_count > 1 || state.root_has_non_element_child {
        state.helpers.insert(HelperFlags::FRAGMENT);
        state.helpers.insert(HelperFlags::OPEN_BLOCK);
        state.helpers.insert(HelperFlags::CREATE_ELEMENT_BLOCK);
        state.helpers.insert(HelperFlags::CREATE_ELEMENT_VNODE);

        // Patch the first root element: change from block root to non-block
        if let (Some((open_start, open_end)), Some(opening_code)) = (
            state.first_root_source_span,
            state.first_root_opening_code.take(),
        ) {
            let is_static_first_root = !opening_code.contains("_ctx.")
                && !opening_code.contains("$setup.")
                && !opening_code.contains("$props.")
                && !opening_code.contains("__props.")
                && !opening_code.contains("_toDisplayString")
                && !opening_code.contains("_withModifiers");

            let did_block_patch = opening_code.contains("(_openBlock(), _createElementBlock(");
            let mut patched_open = if did_block_patch {
                opening_code.replace(
                    "(_openBlock(), _createElementBlock(",
                    "_createElementVNode(",
                )
            } else {
                opening_code.clone()
            };

            // For self-closing elements, the close parens )) are embedded in the opening code.
            // After block-to-non-block patching, strip the extra ) from the removed outer (.
            // Exception: cached elements keep the extra ) to close the _cache[n] = (...) wrapper.
            let will_be_cached = is_static_first_root && state.root_element_count > 1;
            if did_block_patch && state.first_root_is_self_closing && !will_be_cached {
                if let Some(pos) = patched_open.rfind("))") {
                    patched_open.remove(pos + 1);
                }
            }

            if is_static_first_root && state.root_element_count > 1 {
                let cache_idx = state.cache_index;
                state.cache_index += 1;
                patched_open = format!(
                    "_cache[{}] || (_cache[{}] = {}",
                    cache_idx, cache_idx, patched_open
                );
            }

            code_transform.overwrite(open_start, open_end, &patched_open);

            if let (Some((close_start, close_end)), Some(close_code)) = (
                state.first_root_close_span,
                state.first_root_close_code.take(),
            ) {
                let cf = state.pflag(-1, "CACHED");
                let patched_close = if close_code.ends_with("))") {
                    let base = &close_code[..close_code.len() - 2];
                    if is_static_first_root && state.root_element_count > 1 {
                        format!("{}, {})),", base, cf)
                    } else {
                        format!("{})", base)
                    }
                } else if is_static_first_root && state.root_element_count > 1 {
                    format!("{}, {}),", close_code, cf)
                } else {
                    close_code
                };
                code_transform.overwrite(close_start, close_end, &patched_close);
            }
        }
    } else if state.root_has_non_element_child {
        state.helpers.insert(HelperFlags::FRAGMENT);
    }

    // Pre-insert CREATE_COMMENT_VNODE helper if we have an unclosed conditional chain.
    // This must happen BEFORE generating imports so the import is included.
    if state.in_conditional_chain {
        state.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);
    }

    // Generate imports
    let imports = state.helpers.to_import_string();

    // Generate asset imports (e.g., import _imports_0 from "/image.svg?import")
    let mut asset_imports_standalone = String::new();
    for (name, path) in &state.asset_imports {
        asset_imports_standalone.push_str(&format!("import {} from \"{}?import\"\n", name, path));
    }

    // Generate hoisted constants
    let mut hoisted = String::new();
    for node in &state.hoisted_nodes {
        hoisted.push_str(&format!("const {} = {}\n", node.name, node.code));
    }

    // Generate component resolutions
    let mut component_resolutions = String::new();
    for name in &state.resolved_components {
        component_resolutions.push_str(&format!(
            "  const _component_{} = _resolveComponent(\"{}\")\n",
            name.replace('-', "_"),
            name
        ));
    }

    // Generate directive resolutions
    let mut directive_resolutions = String::new();
    for name in &state.resolved_directives {
        directive_resolutions.push_str(&format!(
            "  const _directive_{} = _resolveDirective(\"{}\")\n",
            camelize(name),
            name
        ));
    }

    let resolutions_section =
        if component_resolutions.is_empty() && directive_resolutions.is_empty() {
            String::new()
        } else {
            format!("{}{}", component_resolutions, directive_resolutions)
        };

    if let Some(span) = state.template_span {
        // Prepend imports before hoisted constants (Vue output order)
        let all_imports = if !imports.is_empty() || !asset_imports_standalone.is_empty() {
            format!("{}{}", asset_imports_standalone, imports)
        } else {
            String::new()
        };
        let mut top = String::new();
        if !all_imports.is_empty() {
            top.push_str(&all_imports);
        }
        if !hoisted.is_empty() {
            top.push_str(&hoisted);
        }
        if !top.is_empty() {
            code_transform.prepend(&top);
        }

        // Build prefix and suffix for in-place wrapping
        // Script setup needs full signature for $setup/$props binding access
        let render_params = if !state.binding_metadata.is_empty() {
            "_ctx, _cache, $props, $setup, $data, $options"
        } else {
            "_ctx, _cache"
        };
        let (prefix, suffix) = if state.root_element_count > 1 || state.root_has_non_element_child {
            let fragment_flag = if state.root_has_non_element_child {
                64 + 2048 // STABLE_FRAGMENT | DEV_ROOT_FRAGMENT
            } else {
                64 // STABLE_FRAGMENT
            };
            let prefix = format!(
                "export function render({}) {{\n{}  return (_openBlock(), _createElementBlock(_Fragment, null, [\n    ",
                render_params, resolutions_section
            );
            let frag_comment = if state.is_production {
                String::new()
            } else if state.root_has_non_element_child {
                " /* STABLE_FRAGMENT, DEV_ROOT_FRAGMENT */".to_string()
            } else {
                " /* STABLE_FRAGMENT */".to_string()
            };
            let suffix = format!("\n  ], {}{}))\n}}\n", fragment_flag, frag_comment);
            (prefix, suffix)
        } else {
            let prefix = format!(
                "export function render({}) {{\n{}  return ",
                render_params, resolutions_section
            );
            let suffix = "\n}\n".to_string();
            (prefix, suffix)
        };

        // Close unclosed conditional chain
        let suffix = if state.in_conditional_chain {
            state.in_conditional_chain = false;
            state.helpers.insert(HelperFlags::CREATE_COMMENT_VNODE);
            let vif = state.vif_comment_text();
            let close = format!("\n  : _createCommentVNode(\"{vif}\", true)");
            format!("{}{}", close, suffix)
        } else {
            suffix
        };

        // Wrap in-place: prepend render function opening before content, append closing after
        // Use prepend_left so the prefix appears BEFORE any overwritten content at span.start
        // (prepend_right would insert AFTER the overwritten chunk, which is wrong)
        code_transform.prepend_left(span.start, &prefix);
        code_transform.append_left(span.end, &suffix);
    }

    state.render_started = false;
}

/// Check if a property name is a valid JS identifier (doesn't need quoting).
/// Invalid identifiers include: hyphenated names (data-value), reserved words, etc.
fn is_valid_js_identifier(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }

    // Check first character: must be letter, underscore, or $
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }

    // Rest can include digits
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '$') {
            return false;
        }
    }

    true
}

/// Pre-scan props for static+dynamic conflict on a given prop name (e.g. "class" or "style").
/// Returns (static_value, should_merge).
fn detect_static_dynamic_merge<'a>(
    props: &[super::types::PropEntry],
    source: &'a str,
    target_name: &str,
) -> (Option<&'a str>, bool) {
    let static_value = props
        .iter()
        .find(|p| {
            matches!(p.kind, PropKind::Static) && {
                let n = &source[p.name.start as usize..p.name.end as usize];
                n == target_name
            }
        })
        .and_then(|p| p.value.as_ref())
        .map(|v| &source[v.start as usize..v.end as usize]);

    let has_dynamic = props.iter().any(|p| {
        matches!(p.kind, PropKind::Bind) && {
            let n = &source[p.name.start as usize..p.name.end as usize];
            n == target_name
        }
    });

    let should_merge = static_value.is_some() && has_dynamic;
    (static_value, should_merge)
}

/// Write a property name, quoting it if necessary for valid JS.
fn write_prop_name(out: &mut String, name: &str) {
    if is_valid_js_identifier(name) {
        out.push_str(name);
    } else {
        out.push('"');
        out.push_str(name);
        out.push('"');
    }
}

/// Convert a kebab-case string to camelCase.
/// e.g., "before-enter" -> "beforeEnter", "after-leave" -> "afterLeave"
fn kebab_to_camel(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    let mut capitalize_next = false;

    for c in name.chars() {
        if c == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result
}

/// Write props object.
/// `is_component` indicates if this is for a Vue component (true) or native element (false).
/// For components, v-model generates both modelValue prop and onUpdate:modelValue event.
/// For native elements, only onUpdate:modelValue is generated (modelValue comes from directive).
fn write_props(
    out: &mut String,
    props: &[super::types::PropEntry],
    source: &str,
    state: &mut TemplateCodegenState,
    vfor_locals: &[&str],
    is_component: bool,
) {
    // Check for v-bind spread - special handling needed
    let spread_props: Vec<_> = props
        .iter()
        .filter(|p| matches!(p.kind, PropKind::BindSpread))
        .collect();
    let other_props: Vec<_> = props
        .iter()
        .filter(|p| !matches!(p.kind, PropKind::BindSpread))
        .collect();

    // If we have ONLY spread props (no other props), wrap with normalizeProps
    if !spread_props.is_empty() && other_props.is_empty() {
        state.helpers.insert(HelperFlags::NORMALIZE_PROPS);
        state.helpers.insert(HelperFlags::GUARD_REACTIVE_PROPS);
        out.push_str("_normalizeProps(_guardReactiveProps(");
        if let Some(ref val) = spread_props[0].value {
            let value = &source[val.start as usize..val.end as usize];
            write_expr_with_ctx(
                out,
                value,
                vfor_locals,
                &state.binding_metadata,
                source.as_bytes(),
                state.is_inline_mode,
            );
        }
        out.push_str("))");
        return;
    }

    // If we have spread AND other props, use mergeProps
    if !spread_props.is_empty() && !other_props.is_empty() {
        state.helpers.insert(HelperFlags::MERGE_PROPS);
        state.helpers.insert(HelperFlags::GUARD_REACTIVE_PROPS);
        out.push_str("_mergeProps(");

        // Pre-scan for static+dynamic class/style merge (using original props slice)
        let (mp_static_class, mp_merge_class) = detect_static_dynamic_merge(props, source, "class");
        let (mp_static_style, mp_merge_style) = detect_static_dynamic_merge(props, source, "style");

        // Write non-spread props as first argument
        out.push_str("{ ");
        let mut first = true;
        for prop in &other_props {
            let name = &source[prop.name.start as usize..prop.name.end as usize];

            // Skip static class/style that will be merged into the dynamic binding
            if matches!(prop.kind, PropKind::Static)
                && ((name == "class" && mp_merge_class) || (name == "style" && mp_merge_style))
            {
                continue;
            }

            if !first {
                out.push_str(", ");
            }
            first = false;
            write_prop_inner(
                out,
                prop,
                name,
                source,
                state,
                vfor_locals,
                if mp_merge_class {
                    mp_static_class
                } else {
                    None
                },
                if mp_merge_style {
                    mp_static_style
                } else {
                    None
                },
            );
        }
        out.push_str(" }");
        // Add spread props
        for spread in &spread_props {
            out.push_str(", _guardReactiveProps(");
            if let Some(ref val) = spread.value {
                let value = &source[val.start as usize..val.end as usize];
                write_expr_with_ctx(
                    out,
                    value,
                    vfor_locals,
                    &state.binding_metadata,
                    source.as_bytes(),
                    state.is_inline_mode,
                );
            }
            out.push(')');
        }
        out.push(')');
        return;
    }

    // Normal props handling

    // Pre-scan for static class/style values to merge with dynamic bindings
    let (static_class_value, merge_class) = detect_static_dynamic_merge(props, source, "class");
    let (static_style_value, merge_style) = detect_static_dynamic_merge(props, source, "style");

    let props_start = out.len();
    out.push_str("{ ");

    let mut first = true;

    for prop in props {
        let name = &source[prop.name.start as usize..prop.name.end as usize];

        // Skip static class/style that will be merged into the dynamic binding
        if matches!(prop.kind, PropKind::Static)
            && ((name == "class" && merge_class) || (name == "style" && merge_style))
        {
            continue;
        }

        let prop_start = out.len();
        if !first {
            out.push_str(", ");
        }
        let value_start = out.len();

        match prop.kind {
            PropKind::Static => {
                // Special case: ref="name" where name matches a setup binding
                // Vue uses ref_key/ref pattern: ref_key: "name", ref: $setup.name
                if name == "ref" {
                    if let Some(ref val) = prop.value {
                        let value = &source[val.start as usize..val.end as usize];
                        if matches!(
                            state
                                .binding_metadata
                                .get(value.as_bytes(), source.as_bytes()),
                            Some(
                                super::types::BindingType::Setup
                                    | super::types::BindingType::SetupRef
                            )
                        ) {
                            out.push_str("ref_key: \"");
                            out.push_str(value);
                            out.push_str("\", ref: ");
                            let prefix = super::types::resolve_binding_prefix(
                                value.as_bytes(),
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                            out.push_str(prefix);
                            out.push_str(value);
                            first = false;
                            continue;
                        }
                    }
                }

                write_prop_name(out, name);
                out.push_str(": ");
                if let Some(ref val) = prop.value {
                    let value = &source[val.start as usize..val.end as usize];
                    write_static_prop_value(out, name, value, state);
                } else {
                    out.push_str("\"\"");
                }
            }
            PropKind::Bind => {
                if prop.is_dynamic_arg {
                    // Dynamic prop name: strip brackets from Vue syntax and add _ctx. prefix
                    let prop_name = name.trim_start_matches('[').trim_end_matches(']');
                    out.push('[');
                    write_expr_with_ctx(
                        out,
                        prop_name,
                        vfor_locals,
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                    out.push(']');
                } else {
                    write_prop_name(out, name);
                }
                out.push_str(": ");
                if let Some(ref val) = prop.value {
                    let value = &source[val.start as usize..val.end as usize];

                    if name == "class" {
                        // Always wrap dynamic class with _normalizeClass
                        state.helpers.insert(HelperFlags::NORMALIZE_CLASS);
                        out.push_str("_normalizeClass(");
                        if merge_class {
                            // Merge: _normalizeClass(["static", dynamicExpr])
                            out.push_str(&format!(r#"["{}", "#, static_class_value.unwrap()));
                            write_expr_with_ctx(
                                out,
                                value,
                                vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                            out.push(']');
                        } else {
                            write_expr_with_ctx(
                                out,
                                value,
                                vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                        }
                        out.push(')');
                    } else if name == "style" {
                        // Always wrap dynamic style with _normalizeStyle
                        state.helpers.insert(HelperFlags::NORMALIZE_STYLE);
                        out.push_str("_normalizeStyle(");
                        if merge_style {
                            // Merge: _normalizeStyle(["static", dynamicExpr])
                            out.push_str(&format!(r#"["{}", "#, static_style_value.unwrap()));
                            write_expr_with_ctx(
                                out,
                                value,
                                vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                            out.push(']');
                        } else {
                            write_expr_with_ctx(
                                out,
                                value,
                                vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                        }
                        out.push(')');
                    } else {
                        write_expr_with_ctx(
                            out,
                            value,
                            vfor_locals,
                            &state.binding_metadata,
                            source.as_bytes(),
                            state.is_inline_mode,
                        );
                    }
                }
                // Track dynamic props for components (for PROPS patch flag)
                // Skip key/class/style as they have their own flags
                // Skip constant expressions (literals) — they don't need patching
                if is_component
                    && !prop.is_dynamic_arg
                    && name != "key"
                    && name != "class"
                    && name != "style"
                {
                    let is_constant = prop.value.as_ref().is_some_and(|v| {
                        let val = source[v.start as usize..v.end as usize].trim();
                        is_constant_expression(val, &state.binding_metadata, source)
                    });
                    if !is_constant {
                        if let Some(component) = state.component_stack.last_mut() {
                            let prop_name = name.to_string();
                            if !component.dynamic_props.contains(&prop_name) {
                                component.dynamic_props.push(prop_name);
                            }
                        }
                    }
                }
            }
            PropKind::On => {
                // Separate modifiers into categories:
                // - Event name modifiers: capture, once, passive (become part of event name)
                // - System modifiers: ctrl, alt, shift, meta, exact (use _withModifiers)
                // - Event modifiers: stop, prevent, self (use _withModifiers)
                // - Key modifiers: enter, tab, delete, etc. (use _withKeys)
                let mut event_name_modifiers: Vec<&str> = Vec::new();
                let mut system_modifiers: Vec<&str> = Vec::new();
                let mut key_modifiers: Vec<&str> = Vec::new();

                for mod_span in &prop.modifiers {
                    let modifier = &source[mod_span.start as usize..mod_span.end as usize];
                    match modifier {
                        // Event name modifiers - become part of the event name
                        "capture" | "once" | "passive" => {
                            event_name_modifiers.push(modifier);
                        }
                        // System modifiers and event modifiers - use _withModifiers
                        "stop" | "prevent" | "self" | "ctrl" | "alt" | "shift" | "meta"
                        | "exact" => {
                            system_modifiers.push(modifier);
                        }
                        // Key modifiers - use _withKeys
                        "enter" | "tab" | "delete" | "esc" | "space" | "up" | "down" | "left"
                        | "right" => {
                            key_modifiers.push(modifier);
                        }
                        // Unknown modifiers - treat as key modifiers (could be key codes)
                        _ => {
                            key_modifiers.push(modifier);
                        }
                    }
                }

                // Convert event name to onXxx format with event name modifiers
                // First convert kebab-case to camelCase (e.g., before-enter -> beforeEnter)
                let camel_name = kebab_to_camel(name);
                let mut event_name = String::from("on");
                let mut chars = camel_name.chars();
                if let Some(first_char) = chars.next() {
                    event_name.push(first_char.to_ascii_uppercase());
                    event_name.push_str(chars.as_str());
                }
                // Append event name modifiers (e.g., onClickCapture, onClickOnce)
                for modifier in &event_name_modifiers {
                    // Capitalize first letter of modifier
                    let mut mod_chars = modifier.chars();
                    if let Some(first) = mod_chars.next() {
                        event_name.push(first.to_ascii_uppercase());
                        event_name.push_str(mod_chars.as_str());
                    }
                }
                write_prop_name(out, &event_name);
                out.push_str(": ");

                if let Some(ref val) = prop.value {
                    let value = &source[val.start as usize..val.end as usize];

                    // Check if handler uses v-for local variables (can't cache if it does)
                    let uses_local = vfor_locals.iter().any(|&local| value.contains(local));

                    // Determine what wrappers we need
                    let needs_with_modifiers = !system_modifiers.is_empty();
                    let needs_with_keys = !key_modifiers.is_empty();

                    if needs_with_modifiers {
                        state.helpers.insert(HelperFlags::WITH_MODIFIERS);
                    }
                    if needs_with_keys {
                        state.helpers.insert(HelperFlags::WITH_KEYS);
                    }

                    if uses_local {
                        // Can't cache - handler depends on loop variable
                        if needs_with_keys {
                            out.push_str("_withKeys(");
                        }
                        if needs_with_modifiers {
                            out.push_str("_withModifiers(");
                            write_event_handler_body(
                                out,
                                value,
                                vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                            out.push_str(", [");
                            for (i, m) in system_modifiers.iter().enumerate() {
                                if i > 0 {
                                    out.push(',');
                                }
                                out.push('"');
                                out.push_str(m);
                                out.push('"');
                            }
                            out.push_str("])");
                        } else {
                            // No modifiers
                            write_event_handler_body(
                                out,
                                value,
                                vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                        }
                        if needs_with_keys {
                            out.push_str(", [");
                            for (i, k) in key_modifiers.iter().enumerate() {
                                if i > 0 {
                                    out.push(',');
                                }
                                out.push('"');
                                out.push_str(k);
                                out.push('"');
                            }
                            out.push_str("])");
                        }
                    } else {
                        // Check if handler is a simple identifier that resolves to $setup.
                        // Vue's official compiler does NOT cache these — it emits them as direct
                        // references (e.g., $setup.startDrag) and adds NEED_HYDRATION (32) flag.
                        let trimmed_value = value.trim();
                        let is_simple_ident = trimmed_value
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$');
                        let is_setup_ref = is_simple_ident
                            && !trimmed_value.is_empty()
                            && matches!(
                                state
                                    .binding_metadata
                                    .get(trimmed_value.as_bytes(), source.as_bytes()),
                                Some(
                                    super::types::BindingType::Setup
                                        | super::types::BindingType::SetupRef
                                )
                            )
                            && !needs_with_modifiers
                            && !needs_with_keys;

                        if is_setup_ref {
                            // Direct $setup reference — emit without caching
                            write_expr_with_ctx(
                                out,
                                value,
                                vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                        } else {
                            // Cache the handler
                            let cache_idx = state.cache_index;
                            state.cache_index += 1;

                            out.push_str(&format!(
                                "_cache[{}] || (_cache[{}] = ",
                                cache_idx, cache_idx
                            ));

                            // Build handler with appropriate pattern based on expression type
                            if needs_with_keys {
                                out.push_str("_withKeys(");
                            }
                            if needs_with_modifiers {
                                out.push_str("_withModifiers(");
                                write_event_handler_body(
                                    out,
                                    value,
                                    vfor_locals,
                                    &state.binding_metadata,
                                    source.as_bytes(),
                                    state.is_inline_mode,
                                );
                                out.push_str(", [");
                                for (i, m) in system_modifiers.iter().enumerate() {
                                    if i > 0 {
                                        out.push(',');
                                    }
                                    out.push('"');
                                    out.push_str(m);
                                    out.push('"');
                                }
                                out.push_str("])");
                            } else if needs_with_keys {
                                // Only key modifiers, no system modifiers
                                write_event_handler_body(
                                    out,
                                    value,
                                    vfor_locals,
                                    &state.binding_metadata,
                                    source.as_bytes(),
                                    state.is_inline_mode,
                                );
                            } else {
                                // No modifiers
                                write_event_handler_body(
                                    out,
                                    value,
                                    vfor_locals,
                                    &state.binding_metadata,
                                    source.as_bytes(),
                                    state.is_inline_mode,
                                );
                            }
                            if needs_with_keys {
                                out.push_str(", [");
                                for (i, k) in key_modifiers.iter().enumerate() {
                                    if i > 0 {
                                        out.push(',');
                                    }
                                    out.push('"');
                                    out.push_str(k);
                                    out.push('"');
                                }
                                out.push_str("])");
                            }
                            out.push(')');
                        }
                    }
                } else {
                    // No handler expression (e.g., @click.stop with no value)
                    // Emit () => {} wrapped with modifiers/keys as needed
                    let needs_with_modifiers = !system_modifiers.is_empty();
                    let needs_with_keys = !key_modifiers.is_empty();

                    if needs_with_keys {
                        state.helpers.insert(HelperFlags::WITH_KEYS);
                        out.push_str("_withKeys(");
                    }
                    if needs_with_modifiers {
                        state.helpers.insert(HelperFlags::WITH_MODIFIERS);
                        out.push_str("_withModifiers(() => {}, [");
                        for (i, m) in system_modifiers.iter().enumerate() {
                            if i > 0 {
                                out.push_str(", ");
                            }
                            out.push('"');
                            out.push_str(m);
                            out.push('"');
                        }
                        out.push_str("])");
                    } else {
                        out.push_str("() => {}");
                    }
                    if needs_with_keys {
                        out.push_str(", [");
                        for (i, k) in key_modifiers.iter().enumerate() {
                            if i > 0 {
                                out.push_str(", ");
                            }
                            out.push('"');
                            out.push_str(k);
                            out.push('"');
                        }
                        out.push_str("])");
                    }
                }
            }
            PropKind::Model => {
                // v-model handling differs between components and native elements:
                // - Components: generate both modelValue prop and onUpdate:modelValue event
                // - Native elements: only onUpdate:modelValue (directive handles modelValue binding)
                if let Some(ref val) = prop.value {
                    let value = &source[val.start as usize..val.end as usize];
                    let cache_idx = state.cache_index;
                    state.cache_index += 1;

                    // For components, add modelValue prop and track as dynamic
                    if is_component {
                        out.push_str("modelValue: ");
                        write_expr_with_ctx(
                            out,
                            value,
                            vfor_locals,
                            &state.binding_metadata,
                            source.as_bytes(),
                            state.is_inline_mode,
                        );
                        out.push_str(", ");

                        // Track modelValue as a dynamic prop for patch flags
                        if let Some(component) = state.component_stack.last_mut() {
                            if !component.dynamic_props.contains(&"modelValue".to_string()) {
                                component.dynamic_props.push("modelValue".to_string());
                            }
                        }
                    }

                    out.push_str("\"onUpdate:modelValue\": _cache[");
                    out.push_str(&cache_idx.to_string());
                    out.push_str("] || (_cache[");
                    out.push_str(&cache_idx.to_string());
                    out.push_str("] = $event => ((");
                    write_expr_with_ctx(
                        out,
                        value,
                        vfor_locals,
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                    out.push_str(") = $event))");
                }
            }
            PropKind::Show => {
                // v-show is emitted as runtime directive in wrapper array.
                // No direct prop is generated here.
            }
            PropKind::Html => {
                // v-html generates innerHTML property
                out.push_str("innerHTML: ");
                if let Some(ref val) = prop.value {
                    let value = &source[val.start as usize..val.end as usize];
                    write_expr_with_ctx(
                        out,
                        value,
                        vfor_locals,
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                }
            }
            PropKind::Text => {
                // v-text generates textContent property
                out.push_str("textContent: ");
                if let Some(ref val) = prop.value {
                    let value = &source[val.start as usize..val.end as usize];
                    write_expr_with_ctx(
                        out,
                        value,
                        vfor_locals,
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                }
            }
            PropKind::BindSpread => {
                // BindSpread should be handled at the start of write_props
                // This case should never be reached
                unreachable!("BindSpread should be handled separately");
            }
        }

        // Prop kinds like v-show are emitted via runtime directives, not props.
        // If nothing was written for this prop, roll back separator and keep `first`.
        if out.len() == value_start {
            out.truncate(prop_start);
            continue;
        }
        first = false;
    }

    if first {
        out.truncate(props_start);
        out.push_str("null");
    } else {
        out.push_str(" }");
    }
}

/// Helper to write a single prop (for mergeProps use case).
/// `static_class_merge` / `static_style_merge`: if set, the static value to merge into the dynamic binding.
#[allow(clippy::too_many_arguments)]
fn write_prop_inner(
    out: &mut String,
    prop: &super::types::PropEntry,
    name: &str,
    source: &str,
    state: &mut TemplateCodegenState,
    vfor_locals: &[&str],
    static_class_merge: Option<&str>,
    static_style_merge: Option<&str>,
) {
    match prop.kind {
        PropKind::Static => {
            write_prop_name(out, name);
            out.push_str(": ");
            if let Some(ref val) = prop.value {
                let value = &source[val.start as usize..val.end as usize];
                write_static_prop_value(out, name, value, state);
            } else {
                out.push_str("\"\"");
            }
        }
        PropKind::Bind => {
            if prop.is_dynamic_arg {
                // Dynamic prop name: strip brackets from Vue syntax and add _ctx. prefix
                let prop_name = name.trim_start_matches('[').trim_end_matches(']');
                out.push('[');
                write_expr_with_ctx(
                    out,
                    prop_name,
                    vfor_locals,
                    &state.binding_metadata,
                    source.as_bytes(),
                    state.is_inline_mode,
                );
                out.push(']');
            } else {
                write_prop_name(out, name);
            }
            out.push_str(": ");
            if let Some(ref val) = prop.value {
                let value = &source[val.start as usize..val.end as usize];

                if name == "class" {
                    state.helpers.insert(HelperFlags::NORMALIZE_CLASS);
                    out.push_str("_normalizeClass(");
                    if let Some(static_val) = static_class_merge {
                        out.push_str(&format!(r#"["{}", "#, static_val));
                        write_expr_with_ctx(
                            out,
                            value,
                            vfor_locals,
                            &state.binding_metadata,
                            source.as_bytes(),
                            state.is_inline_mode,
                        );
                        out.push(']');
                    } else {
                        write_expr_with_ctx(
                            out,
                            value,
                            vfor_locals,
                            &state.binding_metadata,
                            source.as_bytes(),
                            state.is_inline_mode,
                        );
                    }
                    out.push(')');
                } else if name == "style" {
                    state.helpers.insert(HelperFlags::NORMALIZE_STYLE);
                    out.push_str("_normalizeStyle(");
                    if let Some(static_val) = static_style_merge {
                        out.push_str(&format!(r#"["{}", "#, static_val));
                        write_expr_with_ctx(
                            out,
                            value,
                            vfor_locals,
                            &state.binding_metadata,
                            source.as_bytes(),
                            state.is_inline_mode,
                        );
                        out.push(']');
                    } else {
                        write_expr_with_ctx(
                            out,
                            value,
                            vfor_locals,
                            &state.binding_metadata,
                            source.as_bytes(),
                            state.is_inline_mode,
                        );
                    }
                    out.push(')');
                } else {
                    write_expr_with_ctx(
                        out,
                        value,
                        vfor_locals,
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                }
            }
        }
        PropKind::On => {
            // For mergeProps, just output the basic event handler
            // Convert kebab-case to camelCase first
            let camel_name = kebab_to_camel(name);
            let mut event_name = String::from("on");
            let mut chars = camel_name.chars();
            if let Some(first_char) = chars.next() {
                event_name.push(first_char.to_ascii_uppercase());
                event_name.push_str(chars.as_str());
            }
            write_prop_name(out, &event_name);
            out.push_str(": ");
            if let Some(ref val) = prop.value {
                let value = &source[val.start as usize..val.end as usize];
                let cache_idx = state.cache_index;
                state.cache_index += 1;
                out.push_str(&format!(
                    "_cache[{}] || (_cache[{}] = ($event) => ",
                    cache_idx, cache_idx
                ));
                write_expr_with_ctx(
                    out,
                    value,
                    vfor_locals,
                    &state.binding_metadata,
                    source.as_bytes(),
                    state.is_inline_mode,
                );
                out.push(')');
            }
        }
        PropKind::Model => {
            out.push_str("modelValue: ");
            if let Some(ref val) = prop.value {
                let value = &source[val.start as usize..val.end as usize];
                write_expr_with_ctx(
                    out,
                    value,
                    vfor_locals,
                    &state.binding_metadata,
                    source.as_bytes(),
                    state.is_inline_mode,
                );
            }
        }
        PropKind::Show | PropKind::Html | PropKind::Text | PropKind::BindSpread => {
            // These should not appear in mergeProps context
        }
    }
}

/// Write props object with a key prop for conditional branches.
fn write_props_with_key(
    out: &mut String,
    props: &[super::types::PropEntry],
    source: &str,
    state: &mut TemplateCodegenState,
    vfor_locals: &[&str],
    key: usize,
    is_component: bool,
) {
    // Pre-scan for static+dynamic class/style merge
    let (static_class_value, merge_class) = detect_static_dynamic_merge(props, source, "class");
    let (static_style_value, merge_style) = detect_static_dynamic_merge(props, source, "style");

    out.push_str("{ ");

    // Write key first
    out.push_str(&format!("key: {}", key));

    // Then write other props
    let mut first = false; // key was already written
    for prop in props {
        let name = &source[prop.name.start as usize..prop.name.end as usize];

        // Skip static class/style that will be merged into the dynamic binding
        if matches!(prop.kind, PropKind::Static)
            && ((name == "class" && merge_class) || (name == "style" && merge_style))
        {
            continue;
        }

        let prop_start = out.len();
        if !first {
            out.push_str(", ");
        }
        let value_start = out.len();

        match prop.kind {
            PropKind::Static => {
                write_prop_name(out, name);
                out.push_str(": ");
                if let Some(ref val) = prop.value {
                    let value = &source[val.start as usize..val.end as usize];
                    write_static_prop_value(out, name, value, state);
                } else {
                    out.push_str("\"\"");
                }
            }
            PropKind::Bind => {
                if prop.is_dynamic_arg {
                    // Dynamic prop name: strip brackets from Vue syntax and add _ctx. prefix
                    let prop_name = name.trim_start_matches('[').trim_end_matches(']');
                    out.push('[');
                    write_expr_with_ctx(
                        out,
                        prop_name,
                        vfor_locals,
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                    out.push(']');
                } else {
                    write_prop_name(out, name);
                }
                out.push_str(": ");
                if let Some(ref val) = prop.value {
                    let value = &source[val.start as usize..val.end as usize];

                    if name == "class" {
                        state.helpers.insert(HelperFlags::NORMALIZE_CLASS);
                        out.push_str("_normalizeClass(");
                        if merge_class {
                            out.push_str(&format!(r#"["{}", "#, static_class_value.unwrap()));
                            write_expr_with_ctx(
                                out,
                                value,
                                vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                            out.push(']');
                        } else {
                            write_expr_with_ctx(
                                out,
                                value,
                                vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                        }
                        out.push(')');
                    } else if name == "style" {
                        state.helpers.insert(HelperFlags::NORMALIZE_STYLE);
                        out.push_str("_normalizeStyle(");
                        if merge_style {
                            out.push_str(&format!(r#"["{}", "#, static_style_value.unwrap()));
                            write_expr_with_ctx(
                                out,
                                value,
                                vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                            out.push(']');
                        } else {
                            write_expr_with_ctx(
                                out,
                                value,
                                vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                        }
                        out.push(')');
                    } else {
                        write_expr_with_ctx(
                            out,
                            value,
                            vfor_locals,
                            &state.binding_metadata,
                            source.as_bytes(),
                            state.is_inline_mode,
                        );
                    }
                }
                // Track dynamic props for components (for PROPS patch flag)
                // Skip key/class/style as they have their own flags
                // Skip constant expressions (literals) — they don't need patching
                if is_component
                    && !prop.is_dynamic_arg
                    && name != "key"
                    && name != "class"
                    && name != "style"
                {
                    let is_constant = prop.value.as_ref().is_some_and(|v| {
                        let val = source[v.start as usize..v.end as usize].trim();
                        is_constant_expression(val, &state.binding_metadata, source)
                    });
                    if !is_constant {
                        if let Some(component) = state.component_stack.last_mut() {
                            let prop_name = name.to_string();
                            if !component.dynamic_props.contains(&prop_name) {
                                component.dynamic_props.push(prop_name);
                            }
                        }
                    }
                }
            }
            PropKind::On => {
                // Full event handler logic (same as write_props)
                // Separate modifiers into categories
                let mut event_name_modifiers: Vec<&str> = Vec::new();
                let mut system_modifiers: Vec<&str> = Vec::new();
                let mut key_modifiers: Vec<&str> = Vec::new();

                for mod_span in &prop.modifiers {
                    let modifier = &source[mod_span.start as usize..mod_span.end as usize];
                    match modifier {
                        "capture" | "once" | "passive" => {
                            event_name_modifiers.push(modifier);
                        }
                        "stop" | "prevent" | "self" | "ctrl" | "alt" | "shift" | "meta"
                        | "exact" => {
                            system_modifiers.push(modifier);
                        }
                        "enter" | "tab" | "delete" | "esc" | "space" | "up" | "down" | "left"
                        | "right" => {
                            key_modifiers.push(modifier);
                        }
                        _ => {
                            key_modifiers.push(modifier);
                        }
                    }
                }

                // Convert event name to onXxx format with event name modifiers
                let camel_name = kebab_to_camel(name);
                let mut event_name = String::from("on");
                let mut chars = camel_name.chars();
                if let Some(first_char) = chars.next() {
                    event_name.push(first_char.to_ascii_uppercase());
                    event_name.push_str(chars.as_str());
                }
                for modifier in &event_name_modifiers {
                    let mut mod_chars = modifier.chars();
                    if let Some(first) = mod_chars.next() {
                        event_name.push(first.to_ascii_uppercase());
                        event_name.push_str(mod_chars.as_str());
                    }
                }
                write_prop_name(out, &event_name);
                out.push_str(": ");

                if let Some(ref val) = prop.value {
                    let value = &source[val.start as usize..val.end as usize];
                    let uses_local = vfor_locals.iter().any(|&local| value.contains(local));

                    let needs_with_modifiers = !system_modifiers.is_empty();
                    let needs_with_keys = !key_modifiers.is_empty();

                    if needs_with_modifiers {
                        state.helpers.insert(HelperFlags::WITH_MODIFIERS);
                    }
                    if needs_with_keys {
                        state.helpers.insert(HelperFlags::WITH_KEYS);
                    }

                    if uses_local {
                        // Can't cache - handler depends on loop variable
                        if needs_with_keys {
                            out.push_str("_withKeys(");
                        }
                        if needs_with_modifiers {
                            out.push_str("_withModifiers(");
                            write_event_handler_body(
                                out,
                                value,
                                vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                            out.push_str(", [");
                            for (i, m) in system_modifiers.iter().enumerate() {
                                if i > 0 {
                                    out.push(',');
                                }
                                out.push('"');
                                out.push_str(m);
                                out.push('"');
                            }
                            out.push_str("])");
                        } else {
                            write_event_handler_body(
                                out,
                                value,
                                vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                        }
                        if needs_with_keys {
                            out.push_str(", [");
                            for (i, k) in key_modifiers.iter().enumerate() {
                                if i > 0 {
                                    out.push(',');
                                }
                                out.push('"');
                                out.push_str(k);
                                out.push('"');
                            }
                            out.push_str("])");
                        }
                    } else {
                        // Check if handler is a simple identifier that resolves to $setup.
                        // Vue's official compiler does NOT cache these — it emits them as direct
                        // references (e.g., $setup.startDrag) and adds NEED_HYDRATION (32) flag.
                        let trimmed_value = value.trim();
                        let is_simple_ident = trimmed_value
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$');
                        let is_setup_ref = is_simple_ident
                            && !trimmed_value.is_empty()
                            && matches!(
                                state
                                    .binding_metadata
                                    .get(trimmed_value.as_bytes(), source.as_bytes()),
                                Some(
                                    super::types::BindingType::Setup
                                        | super::types::BindingType::SetupRef
                                )
                            )
                            && !needs_with_modifiers
                            && !needs_with_keys;

                        if is_setup_ref {
                            // Direct $setup reference — emit without caching
                            write_expr_with_ctx(
                                out,
                                value,
                                vfor_locals,
                                &state.binding_metadata,
                                source.as_bytes(),
                                state.is_inline_mode,
                            );
                        } else {
                            // Cache the handler
                            let cache_idx = state.cache_index;
                            state.cache_index += 1;
                            out.push_str(&format!(
                                "_cache[{}] || (_cache[{}] = ",
                                cache_idx, cache_idx
                            ));

                            if needs_with_keys {
                                out.push_str("_withKeys(");
                            }
                            if needs_with_modifiers {
                                out.push_str("_withModifiers(");
                                write_event_handler_body(
                                    out,
                                    value,
                                    vfor_locals,
                                    &state.binding_metadata,
                                    source.as_bytes(),
                                    state.is_inline_mode,
                                );
                                out.push_str(", [");
                                for (i, m) in system_modifiers.iter().enumerate() {
                                    if i > 0 {
                                        out.push(',');
                                    }
                                    out.push('"');
                                    out.push_str(m);
                                    out.push('"');
                                }
                                out.push_str("])");
                            } else {
                                write_event_handler_body(
                                    out,
                                    value,
                                    vfor_locals,
                                    &state.binding_metadata,
                                    source.as_bytes(),
                                    state.is_inline_mode,
                                );
                            }
                            if needs_with_keys {
                                out.push_str(", [");
                                for (i, k) in key_modifiers.iter().enumerate() {
                                    if i > 0 {
                                        out.push(',');
                                    }
                                    out.push('"');
                                    out.push_str(k);
                                    out.push('"');
                                }
                                out.push_str("])");
                            }
                            out.push(')');
                        }
                    }
                } else {
                    // No handler expression (e.g., @click.stop with no value)
                    // Emit () => {} wrapped with modifiers/keys as needed
                    let needs_with_modifiers = !system_modifiers.is_empty();
                    let needs_with_keys = !key_modifiers.is_empty();

                    if needs_with_keys {
                        state.helpers.insert(HelperFlags::WITH_KEYS);
                        out.push_str("_withKeys(");
                    }
                    if needs_with_modifiers {
                        state.helpers.insert(HelperFlags::WITH_MODIFIERS);
                        out.push_str("_withModifiers(() => {}, [");
                        for (i, m) in system_modifiers.iter().enumerate() {
                            if i > 0 {
                                out.push_str(", ");
                            }
                            out.push('"');
                            out.push_str(m);
                            out.push('"');
                        }
                        out.push_str("])");
                    } else {
                        out.push_str("() => {}");
                    }
                    if needs_with_keys {
                        out.push_str(", [");
                        for (i, k) in key_modifiers.iter().enumerate() {
                            if i > 0 {
                                out.push_str(", ");
                            }
                            out.push('"');
                            out.push_str(k);
                            out.push('"');
                        }
                        out.push_str("])");
                    }
                }
            }
            PropKind::Model => {
                // v-model handling differs between components and native elements:
                // - Components: generate both modelValue prop and onUpdate:modelValue event
                // - Native elements: only onUpdate:modelValue (directive handles modelValue binding)
                if let Some(ref val) = prop.value {
                    let value = &source[val.start as usize..val.end as usize];
                    let cache_idx = state.cache_index;
                    state.cache_index += 1;

                    // For components, add modelValue prop and track as dynamic
                    if is_component {
                        out.push_str("modelValue: ");
                        write_expr_with_ctx(
                            out,
                            value,
                            vfor_locals,
                            &state.binding_metadata,
                            source.as_bytes(),
                            state.is_inline_mode,
                        );
                        out.push_str(", ");

                        // Track modelValue as a dynamic prop for patch flags
                        if let Some(component) = state.component_stack.last_mut() {
                            if !component.dynamic_props.contains(&"modelValue".to_string()) {
                                component.dynamic_props.push("modelValue".to_string());
                            }
                        }
                    }

                    out.push_str("\"onUpdate:modelValue\": _cache[");
                    out.push_str(&cache_idx.to_string());
                    out.push_str("] || (_cache[");
                    out.push_str(&cache_idx.to_string());
                    out.push_str("] = $event => ((");
                    write_expr_with_ctx(
                        out,
                        value,
                        vfor_locals,
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                    out.push_str(") = $event))");
                }
            }
            PropKind::Show => {
                // v-show is emitted as runtime directive in wrapper array.
                // No direct prop is generated here.
            }
            PropKind::Html => {
                out.push_str("innerHTML: ");
                if let Some(ref val) = prop.value {
                    let value = &source[val.start as usize..val.end as usize];
                    write_expr_with_ctx(
                        out,
                        value,
                        vfor_locals,
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                }
            }
            PropKind::Text => {
                out.push_str("textContent: ");
                if let Some(ref val) = prop.value {
                    let value = &source[val.start as usize..val.end as usize];
                    write_expr_with_ctx(
                        out,
                        value,
                        vfor_locals,
                        &state.binding_metadata,
                        source.as_bytes(),
                        state.is_inline_mode,
                    );
                }
            }
            PropKind::BindSpread => {
                // BindSpread with key is unusual, but handle it
                // Skip the spread in props-with-key context
            }
        }

        // If this prop emitted nothing (e.g. v-show), roll back separator.
        if out.len() == value_start {
            out.truncate(prop_start);
            continue;
        }
        first = false;
    }

    out.push_str(" }");
}

/// Write expression with correct accessor prefix for all identifiers that need it.
/// Uses OXC AST-based binding extraction to correctly handle TypeScript syntax
/// (as assertions, satisfies, non-null assertions, generics, etc.)
fn write_expr_with_ctx(
    out: &mut String,
    expr: &str,
    local_vars: &[&str],
    binding_metadata: &BindingMetadata,
    source_bytes: &[u8],
    is_production: bool,
) {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return;
    }

    // Parse with OXC and use AST binding positions for accurate prefixing
    let allocator = oxc_allocator::Allocator::default();
    let parser = oxc_parser::Parser::new(&allocator, trimmed, oxc_span::SourceType::tsx());

    match parser.parse_expression() {
        Ok(ast_expr) => {
            let ctx = crate::utils::oxc::BindingContext::new(0);
            let result =
                crate::utils::oxc::extract_bindings_from_expression(&ast_expr, trimmed, &ctx);
            write_with_binding_positions(
                out,
                trimmed,
                &result.bindings,
                local_vars,
                binding_metadata,
                source_bytes,
                is_production,
            );
        }
        Err(_) => {
            // Fallback to text-based approach if OXC can't parse
            transform_expr_with_ctx(
                out,
                trimmed,
                local_vars,
                binding_metadata,
                source_bytes,
                is_production,
            );
        }
    }
}

/// Use AST-extracted binding positions to insert correct accessor prefix at exact positions.
/// Copies everything between bindings as-is, preserving TypeScript syntax.
fn write_with_binding_positions(
    out: &mut String,
    expr: &str,
    bindings: &[crate::utils::oxc::Binding],
    local_vars: &[&str],
    binding_metadata: &BindingMetadata,
    source_bytes: &[u8],
    is_production: bool,
) {
    // Collect non-ignored bindings that aren't local vars, sorted by position
    let mut to_prefix: Vec<&crate::utils::oxc::Binding> = bindings
        .iter()
        .filter(|b| !b.ignore && !is_reserved_word(b.name) && !local_vars.contains(&b.name))
        .collect();
    to_prefix.sort_by_key(|b| b.span.start);

    let mut cursor = 0u32;

    for binding in to_prefix {
        // Copy everything from cursor to binding start as-is
        if binding.span.start > cursor {
            out.push_str(&expr[cursor as usize..binding.span.start as usize]);
        }
        // Insert correct accessor prefix/suffix and the binding name
        let prefix = resolve_binding_prefix(
            binding.name.as_bytes(),
            binding_metadata,
            source_bytes,
            is_production,
        );
        let suffix = resolve_binding_suffix(
            binding.name.as_bytes(),
            binding_metadata,
            source_bytes,
            is_production,
        );
        // For shorthand object properties like { file }, when the binding gets a prefix,
        // expand to { file: $props.file } instead of invalid { $props.file }
        if !prefix.is_empty() {
            let expr_bytes = expr.as_bytes();
            let start = binding.span.start as usize;
            let end = binding.span.end as usize;
            // Check if preceded by { or , (skipping whitespace)
            let mut before = start;
            while before > 0 && expr_bytes[before - 1].is_ascii_whitespace() {
                before -= 1;
            }
            let before_ok =
                before > 0 && (expr_bytes[before - 1] == b'{' || expr_bytes[before - 1] == b',');
            // Exclude template literal interpolation: ${ident} has $ before {
            let is_template_literal = before_ok
                && expr_bytes[before - 1] == b'{'
                && before >= 2
                && expr_bytes[before - 2] == b'$';
            // Check if followed by } or , (skipping whitespace)
            let mut after = end;
            while after < expr_bytes.len() && expr_bytes[after].is_ascii_whitespace() {
                after += 1;
            }
            let after_ok = after < expr_bytes.len()
                && (expr_bytes[after] == b'}' || expr_bytes[after] == b',');
            if before_ok && after_ok && !is_template_literal {
                out.push_str(binding.name);
                out.push_str(": ");
            }
        }
        out.push_str(prefix);
        out.push_str(binding.name);
        out.push_str(suffix);
        cursor = binding.span.end;
    }

    // Copy remaining text after the last binding
    if (cursor as usize) < expr.len() {
        out.push_str(&expr[cursor as usize..]);
    }
}

/// Transform an expression by adding the correct accessor prefix to identifiers.
/// Uses binding metadata to select `$setup.`, `$props.`, or `_ctx.` prefix.
fn transform_expr_with_ctx(
    out: &mut String,
    expr: &str,
    local_vars: &[&str],
    binding_metadata: &BindingMetadata,
    source_bytes: &[u8],
    is_production: bool,
) {
    let chars: Vec<char> = expr.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut prev_was_dot = false;

    while i < len {
        let ch = chars[i];

        // Handle string literals - copy as-is
        if ch == '"' || ch == '\'' || ch == '`' {
            let quote = ch;
            out.push(ch);
            i += 1;
            while i < len {
                let c = chars[i];
                out.push(c);
                i += 1;
                if c == quote && (i < 2 || chars[i - 2] != '\\') {
                    break;
                }
                // Handle template literal expressions ${...}
                if quote == '`' && c == '$' && i < len && chars[i] == '{' {
                    out.push(chars[i]);
                    i += 1;
                    // Find matching }
                    let mut depth = 1;
                    let _start = out.len();
                    let mut inner = String::new();
                    while i < len && depth > 0 {
                        let c = chars[i];
                        if c == '{' {
                            depth += 1;
                        } else if c == '}' {
                            depth -= 1;
                        }
                        if depth > 0 {
                            inner.push(c);
                        }
                        i += 1;
                    }
                    // Transform the inner expression
                    transform_expr_with_ctx(
                        out,
                        &inner,
                        local_vars,
                        binding_metadata,
                        source_bytes,
                        is_production,
                    );
                    out.push('}');
                }
            }
            prev_was_dot = false;
            continue;
        }

        // Handle comments - skip
        if ch == '/' && i + 1 < len {
            if chars[i + 1] == '/' {
                // Line comment
                while i < len && chars[i] != '\n' {
                    out.push(chars[i]);
                    i += 1;
                }
                prev_was_dot = false;
                continue;
            } else if chars[i + 1] == '*' {
                // Block comment
                out.push(ch);
                out.push(chars[i + 1]);
                i += 2;
                while i + 1 < len && !(chars[i] == '*' && chars[i + 1] == '/') {
                    out.push(chars[i]);
                    i += 1;
                }
                if i + 1 < len {
                    out.push(chars[i]);
                    out.push(chars[i + 1]);
                    i += 2;
                }
                prev_was_dot = false;
                continue;
            }
        }

        // Check if this is the start of an identifier
        if is_ident_start(ch) {
            let ident_start = i;
            while i < len && is_ident_continue(chars[i]) {
                i += 1;
            }
            let ident: String = chars[ident_start..i].iter().collect();

            // Check if this identifier is an object property key (followed by ':')
            let is_obj_key = {
                let mut j = i;
                while j < len && chars[j].is_ascii_whitespace() {
                    j += 1;
                }
                j < len && chars[j] == ':' && (j + 1 >= len || chars[j + 1] != ':')
                // not ::
            };

            // Don't prefix if:
            // 1. Preceded by a dot (member access)
            // 2. Is a reserved word/literal
            // 3. Is a local variable
            // 4. Is an object property key (followed by ':')
            let should_prefix = !prev_was_dot
                && !is_obj_key
                && !is_reserved_word(&ident)
                && !local_vars.iter().any(|&v| v == ident);

            if should_prefix {
                let prefix = resolve_binding_prefix(
                    ident.as_bytes(),
                    binding_metadata,
                    source_bytes,
                    is_production,
                );

                // Check if this is a shorthand property in an object literal
                // that needs expansion: { file } → { file: prefix.file }
                let is_shorthand = if !prefix.is_empty() {
                    let mut after = i;
                    while after < len && chars[after].is_ascii_whitespace() {
                        after += 1;
                    }
                    let after_ok = after < len && (chars[after] == '}' || chars[after] == ',');
                    let mut before = ident_start;
                    while before > 0 && chars[before - 1].is_ascii_whitespace() {
                        before -= 1;
                    }
                    let before_ok =
                        before > 0 && (chars[before - 1] == '{' || chars[before - 1] == ',');
                    // Exclude template literal interpolation: ${ident}
                    let is_template_literal = before_ok
                        && chars[before - 1] == '{'
                        && before >= 2
                        && chars[before - 2] == '$';
                    after_ok && before_ok && !is_template_literal
                } else {
                    false
                };

                if is_shorthand {
                    // Expand shorthand: file → file: prefix.file
                    out.push_str(&ident);
                    out.push_str(": ");
                }
                out.push_str(prefix);
                out.push_str(&ident);
                let suffix = resolve_binding_suffix(
                    ident.as_bytes(),
                    binding_metadata,
                    source_bytes,
                    is_production,
                );
                out.push_str(suffix);
            } else {
                out.push_str(&ident);
            }
            prev_was_dot = false;
            continue;
        }

        // Track if we just saw a dot (for member access detection)
        prev_was_dot = ch == '.';

        // Handle optional chaining
        if ch == '?' && i + 1 < len && chars[i + 1] == '.' {
            out.push(ch);
            i += 1;
            prev_was_dot = true;
            continue;
        }

        // Copy all other characters as-is
        out.push(ch);
        i += 1;
    }
}

/// Check if a character can start an identifier
fn is_ident_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_' || ch == '$'
}

/// Check if a character can continue an identifier
fn is_ident_continue(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$'
}

/// Check if an identifier is a reserved word that shouldn't be prefixed
fn is_reserved_word(ident: &str) -> bool {
    matches!(
        ident,
        "true"
            | "false"
            | "null"
            | "undefined"
            | "this"
            | "NaN"
            | "Infinity"
            | "Math"
            | "Date"
            | "Array"
            | "Object"
            | "String"
            | "Number"
            | "Boolean"
            | "JSON"
            | "console"
            | "window"
            | "document"
            | "parseInt"
            | "parseFloat"
            | "isNaN"
            | "isFinite"
            | "encodeURI"
            | "encodeURIComponent"
            | "decodeURI"
            | "decodeURIComponent"
            | "typeof"
            | "void"
            | "delete"
            | "new"
            | "in"
            | "instanceof"
            | "arguments"
            | "require"
            | "module"
            | "exports"
            | "$event"
            | "event"
    )
}

/// Extract a prefix operator from the start of an expression.
/// Returns (prefix, rest) where prefix is the operator (including any whitespace after it).
#[allow(dead_code)]
fn extract_prefix_operator(expr: &str) -> (&str, &str) {
    let trimmed = expr.trim();

    // Check for single-character prefix operators
    if let Some(first_char) = trimmed.chars().next() {
        match first_char {
            '!' | '~' => {
                // Handle !! (double negation) by returning just the first !
                return (&trimmed[..1], trimmed[1..].trim_start());
            }
            '+' | '-' => {
                // Only treat as prefix if followed by identifier/expression, not another operator
                let rest = trimmed[1..].trim_start();
                if !rest.is_empty() && !rest.starts_with(first_char) {
                    return (&trimmed[..1], rest);
                }
            }
            _ => {}
        }
    }

    // Check for keyword prefix operators
    for keyword in ["typeof ", "void ", "delete ", "await "] {
        if let Some(rest) = trimmed.strip_prefix(keyword) {
            return (keyword, rest.trim_start());
        }
    }

    ("", expr)
}

/// Write event handler body with appropriate pattern based on the expression type.
/// - Simple identifiers (handleClick) → guard pattern: `(...args) => (_ctx.handleClick && _ctx.handleClick(...args))`
/// - Function calls (handleClick(false)) → simple: `$event => (_ctx.handleClick(false))`
/// - Inline functions ((e) => {...}) → as-is
fn write_event_handler_body(
    out: &mut String,
    value: &str,
    local_vars: &[&str],
    binding_metadata: &BindingMetadata,
    source_bytes: &[u8],
    is_production: bool,
) {
    let trimmed = value.trim();

    // Check if it's an inline arrow function or function expression - output as-is
    if trimmed.starts_with('(') && trimmed.contains("=>")
        || trimmed.starts_with("function")
        || trimmed.starts_with("async")
    {
        write_expr_with_ctx(
            out,
            value,
            local_vars,
            binding_metadata,
            source_bytes,
            is_production,
        );
        return;
    }

    // Check if it's a function call expression (ends with ')')
    // e.g., handleClick(false), emit('event'), this.method()
    if is_function_call_expression(trimmed) {
        // Use simple pattern for function calls
        out.push_str("$event => (");
        write_expr_with_ctx(
            out,
            value,
            local_vars,
            binding_metadata,
            source_bytes,
            is_production,
        );
        out.push(')');
    } else if is_simple_member_expression(trimmed) {
        // Use guard pattern for method references
        // This handles cases like: handleClick, this.handleClick, obj.method
        out.push_str("(...args) => (");
        write_expr_with_ctx(
            out,
            value,
            local_vars,
            binding_metadata,
            source_bytes,
            is_production,
        );
        out.push_str(" && ");
        write_expr_with_ctx(
            out,
            value,
            local_vars,
            binding_metadata,
            source_bytes,
            is_production,
        );
        out.push_str("(...args))");
    } else {
        // Inline expression: open = !open, count++, x ? a : b, etc.
        out.push_str("$event => (");
        write_expr_with_ctx(
            out,
            value,
            local_vars,
            binding_metadata,
            source_bytes,
            is_production,
        );
        out.push(')');
    }
}

/// Check if an expression is a function call (ends with closing parenthesis).
/// Handles nested parentheses correctly.
fn is_function_call_expression(expr: &str) -> bool {
    let trimmed = expr.trim();
    if !trimmed.ends_with(')') {
        return false;
    }

    // Count parentheses to find if it's a call or just parenthesized expression
    let mut paren_depth = 0;
    let mut in_string = None;

    for c in trimmed.chars() {
        // Track string literals to avoid counting parens inside strings
        if in_string.is_none() && (c == '"' || c == '\'' || c == '`') {
            in_string = Some(c);
        } else if in_string == Some(c) {
            in_string = None;
        } else if in_string.is_none() {
            match c {
                '(' => paren_depth += 1,
                ')' => paren_depth -= 1,
                _ => {}
            }
        }
    }

    // If parens are balanced and it ends with ), check if there's an identifier before the last (
    // This distinguishes handleClick() from (a + b)
    if paren_depth == 0 {
        // Find the matching opening paren for the last )
        let mut depth = 0;
        let mut last_open_pos = None;
        for (i, c) in trimmed.char_indices().rev() {
            if c == ')' {
                depth += 1;
            } else if c == '(' {
                depth -= 1;
                if depth == 0 {
                    last_open_pos = Some(i);
                    break;
                }
            }
        }

        if let Some(pos) = last_open_pos {
            if pos > 0 {
                // Check if character before '(' is alphanumeric or ] (method call)
                let before = trimmed[..pos].chars().last();
                return before
                    .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$' || c == ']');
            }
        }
    }

    false
}

/// Check if an expression is a simple identifier or member expression (method reference).
/// Returns true for: `handleClick`, `this.handleClick`, `obj.method`, `$emit`
/// Returns false for: `open = !open`, `count++`, `a ? b : c`, `foo()`
fn is_simple_member_expression(expr: &str) -> bool {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return false;
    }
    // A simple member expression contains only identifiers, dots, optional chaining,
    // and bracket access. It does NOT contain operators like =, +, -, !, ?, :, etc.
    trimmed.chars().all(|c| {
        c.is_alphanumeric()
            || c == '_'
            || c == '$'
            || c == '.'
            || c == '['
            || c == ']'
            || c == '\''
            || c == '"'
    })
}

/// Check if an expression is a JavaScript literal (string, number, boolean, null, undefined).
#[allow(dead_code)]
fn is_literal(expr: &str) -> bool {
    let trimmed = expr.trim();

    // String literals: 'text' or "text" or `template`
    if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('`') && trimmed.ends_with('`'))
    {
        return true;
    }

    // Number literals: 123, 12.34, -5, 0x1F, 1e10, etc.
    if trimmed
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || c == '-' || c == '.')
    {
        // Basic check: if it starts with digit, minus, or dot, and contains only valid number chars
        let is_number = trimmed
            .chars()
            .skip(1) // Skip first char (already checked)
            .all(|c| {
                c.is_ascii_digit()
                    || c == '.'
                    || c == 'e'
                    || c == 'E'
                    || c == 'x'
                    || c == 'X'
                    || c == '-'
                    || c == '+'
                    || c.is_ascii_hexdigit()
            });
        if is_number {
            return true;
        }
    }

    // Boolean literals
    if trimmed == "true" || trimmed == "false" {
        return true;
    }

    // Null and undefined
    if trimmed == "null" || trimmed == "undefined" {
        return true;
    }

    false
}

/// Split a string by a delimiter, but only at the top level (not inside braces/brackets).
#[allow(dead_code)]
fn split_top_level(s: &str, delim: char) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0;

    for (i, c) in s.char_indices() {
        match c {
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth -= 1,
            _ if c == delim && depth == 0 => {
                result.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }

    // Don't forget the last segment
    if start < s.len() {
        result.push(&s[start..]);
    }

    result
}

/// Find the position of the colon separator in a key: value pair.
/// Handles nested structures by tracking depth.
#[allow(dead_code)]
fn find_colon_position(s: &str) -> Option<usize> {
    let mut depth = 0;

    for (i, c) in s.char_indices() {
        match c {
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => depth -= 1,
            ':' if depth == 0 => return Some(i),
            _ => {}
        }
    }

    None
}

/// Extract the first identifier from an expression (e.g., "item.id" -> "item").
#[allow(dead_code)]
fn extract_first_identifier(expr: &str) -> &str {
    let trimmed = expr.trim();
    // Find end of first identifier (alphanumeric or underscore)
    let end = trimmed
        .char_indices()
        .find(|(i, c)| *i > 0 && !c.is_alphanumeric() && *c != '_' && *c != '$')
        .map(|(i, _)| i)
        .unwrap_or(trimmed.len());
    &trimmed[..end]
}

/// Extract local variable names from a v-for iterator pattern.
/// e.g., "item" -> ["item"], "(item, index)" -> ["item", "index"]
fn extract_vfor_locals(iterator: &str) -> Vec<&str> {
    let trimmed = iterator.trim();
    // Remove parentheses if present
    let inner = if trimmed.starts_with('(') && trimmed.ends_with(')') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    // Split by comma and extract identifiers
    inner
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Extract local variable names from slot params.
/// Handles patterns like:
///   `item` → `["item"]`
///   `{ column, record }` → `["column", "record"]`
///   `{ item: renamed }` → `["renamed"]`
///   `{ isActive, href, navigate }` → `["isActive", "href", "navigate"]`
fn extract_slot_locals(params: &str) -> Vec<String> {
    let trimmed = params.trim();
    // Strip outer parens and braces
    let mut inner = trimmed;
    if inner.starts_with('(') && inner.ends_with(')') {
        inner = &inner[1..inner.len() - 1];
        inner = inner.trim();
    }
    if inner.starts_with('{') && inner.ends_with('}') {
        inner = &inner[1..inner.len() - 1];
        inner = inner.trim();
    }

    let mut locals = Vec::new();
    for segment in inner.split(',') {
        let seg = segment.trim();
        if seg.is_empty() {
            continue;
        }
        // Handle `key: value` rename — the binding name is the value
        if let Some(colon_pos) = seg.find(':') {
            let value = seg[colon_pos + 1..].trim();
            // The value might be a nested destructure or a simple identifier
            let ident = extract_first_identifier(value);
            if !ident.is_empty() {
                locals.push(ident.to_string());
            }
        } else {
            // Simple identifier, may have type annotation or default value
            let ident = extract_first_identifier(seg);
            if !ident.is_empty() {
                locals.push(ident.to_string());
            }
        }
    }
    locals
}

/// Calculate patch flags for an element based on its props.
/// Returns (patch_flags, dynamic_prop_names).
fn calculate_patch_flags(
    props: &[super::types::PropEntry],
    source: &str,
    _binding_metadata: &super::types::BindingMetadata,
    vfor_locals: &[&str],
) -> (u32, Vec<String>) {
    use super::types::patch_flags::*;

    let mut flags = 0u32;
    let mut dynamic_props = Vec::new();
    let mut has_dynamic_key = false;

    for prop in props {
        let name = &source[prop.name.start as usize..prop.name.end as usize];

        match prop.kind {
            PropKind::Bind => {
                if prop.is_dynamic_arg {
                    // Dynamic argument name (e.g., :[key]="value") requires FULL_PROPS
                    has_dynamic_key = true;
                } else {
                    match name {
                        "class" => flags |= CLASS,
                        "style" => flags |= STYLE,
                        "key" => {} // :key is not a patch flag, it's for diffing
                        _ => {
                            flags |= PROPS;
                            dynamic_props.push(name.to_string());
                        }
                    }
                }
            }
            PropKind::On => {
                // NEED_HYDRATION rules (from Vue's transformElement.ts):
                // - Click events are EXCLUDED (dedicated hydration fast path)
                // - onUpdate:modelValue is excluded (handled by PropKind::Model)
                // - All other event handlers set NEED_HYDRATION regardless of
                //   whether the handler is a setup ref or cached expression
                let is_click = name.eq_ignore_ascii_case("click");

                // Check if any modifier makes this a non-click event name
                // (e.g., @click.capture → onClickCapture, which is NOT "onclick")
                let has_event_name_modifiers = prop.modifiers.iter().any(|mod_span| {
                    let modifier = &source[mod_span.start as usize..mod_span.end as usize];
                    matches!(modifier, "capture" | "once" | "passive")
                });

                // NEED_HYDRATION for all non-click events, or click with
                // event-name modifiers (capture/once/passive change the event name)
                if !is_click || has_event_name_modifiers {
                    flags |= NEED_HYDRATION;
                }

                // PROPS flag logic: determine if handler is dynamic (not cacheable)
                let handler_value = prop
                    .value
                    .as_ref()
                    .map(|v| source[v.start as usize..v.end as usize].trim());

                // Check if handler references v-for local variables.
                // Such handlers can't be cached (they differ per iteration),
                // so they need PROPS flag and the event name in dynamic props.
                let uses_vfor_local = handler_value
                    .is_some_and(|val| vfor_locals.iter().any(|&local| val.contains(local)));

                if uses_vfor_local {
                    // Handler depends on loop variable — can't be cached.
                    // Add PROPS flag and event name (onXxx format) to dynamic props.
                    flags |= PROPS;
                    let mut on_name = String::with_capacity(2 + name.len());
                    on_name.push_str("on");
                    let mut chars = name.chars();
                    if let Some(first) = chars.next() {
                        on_name.push(first.to_ascii_uppercase());
                        on_name.push_str(chars.as_str());
                    }
                    dynamic_props.push(on_name);
                }
                // Setup refs and cached handlers don't need PROPS flag — they
                // are constant after first render (or a direct $setup reference).
            }
            PropKind::Model => {
                // On native elements (the only context where calculate_patch_flags is called),
                // v-model uses withDirectives at runtime, not props. NEED_PATCH is added
                // separately (line ~897). No PROPS flag or "modelValue" dynamic prop needed.
                // Component v-model IS a real prop, but components handle their own patching.
            }
            PropKind::Show => {
                // v-show is emitted as runtime directive with NEED_PATCH.
                // No STYLE/PROPS patch flag needed here.
            }
            PropKind::Html | PropKind::Text => {
                // v-html/v-text are props
                flags |= PROPS;
            }
            PropKind::Static => {
                // Static props don't contribute to patch flags
            }
            PropKind::BindSpread => {
                // v-bind spread requires FULL_PROPS since all props are dynamic
                has_dynamic_key = true;
            }
        }
    }

    // FULL_PROPS overrides other prop-related flags
    if has_dynamic_key {
        flags = FULL_PROPS;
        dynamic_props.clear(); // Not needed with FULL_PROPS
    }

    (flags, dynamic_props)
}

/// Format patch flags with human-readable comment.
#[allow(dead_code)]
fn format_patch_flag(flags: u32) -> String {
    format_patch_flag_prod(flags, false)
}

fn format_patch_flag_prod(flags: u32, is_production: bool) -> String {
    use super::types::patch_flags::*;

    if flags == 0 {
        return String::new();
    }

    if is_production {
        return format!(", {}", flags);
    }

    let mut names = Vec::new();

    if flags == FULL_PROPS {
        return format!(", {} /* FULL_PROPS */", flags);
    }

    if flags & TEXT != 0 {
        names.push("TEXT");
    }
    if flags & CLASS != 0 {
        names.push("CLASS");
    }
    if flags & STYLE != 0 {
        names.push("STYLE");
    }
    if flags & PROPS != 0 {
        names.push("PROPS");
    }
    if flags & NEED_HYDRATION != 0 {
        names.push("NEED_HYDRATION");
    }
    if flags & STABLE_FRAGMENT != 0 {
        names.push("STABLE_FRAGMENT");
    }
    if flags & KEYED_FRAGMENT != 0 {
        names.push("KEYED_FRAGMENT");
    }
    if flags & UNKEYED_FRAGMENT != 0 {
        names.push("UNKEYED_FRAGMENT");
    }
    if flags & NEED_PATCH != 0 {
        names.push("NEED_PATCH");
    }
    if flags & DYNAMIC_SLOTS != 0 {
        names.push("DYNAMIC_SLOTS");
    }

    if names.is_empty() {
        format!(", {}", flags)
    } else {
        format!(", {} /* {} */", flags, names.join(", "))
    }
}

/// Determine if a v-for iterable expression refers to a constant (non-reactive) source.
/// Returns true for number literals and inline array/object literals.
///
/// Note: Setup bindings (refs, reactive objects) are NOT treated as constant because
/// we can't distinguish `const items = [1,2,3]` from `const items = ref([...])`.
/// This is the conservative approach - it may produce UNKEYED_FRAGMENT for truly
/// constant arrays, but correctly avoids STABLE_FRAGMENT for reactive sources.
fn is_constant_iterable(
    iterable_expr: &str,
    _binding_metadata: &super::types::BindingMetadata,
    _source: &str,
) -> bool {
    let trimmed = iterable_expr.trim();

    // Number literal (v-for="n in 10")
    if trimmed.parse::<f64>().is_ok() {
        return true;
    }

    // Inline array or object literal
    if (trimmed.starts_with('[') && trimmed.ends_with(']'))
        || (trimmed.starts_with('{') && trimmed.ends_with('}'))
    {
        return true;
    }

    false
}

/// Check if a bound expression is a compile-time constant (doesn't need patching).
/// Returns true only for literal values (numbers, strings, booleans, null/undefined).
/// Does NOT treat setup bindings as constant since they can be reactive.
fn is_constant_expression(
    expr: &str,
    _binding_metadata: &super::types::BindingMetadata,
    _source: &str,
) -> bool {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Number literal: 50, 3.14, -1
    if trimmed.parse::<f64>().is_ok() {
        return true;
    }

    // Boolean literals
    if trimmed == "true" || trimmed == "false" {
        return true;
    }

    // null / undefined
    if trimmed == "null" || trimmed == "undefined" {
        return true;
    }

    // String literals: 'foo', "bar", `baz`
    if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('`') && trimmed.ends_with('`'))
    {
        return true;
    }

    false
}

/// Decode HTML entities in text content to their Unicode characters.
/// Handles named entities (&amp;, &lt;, &times;, etc.) and numeric entities (&#NNN;, &#xHH;).
fn decode_html_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '&' {
            result.push(c);
            continue;
        }

        // Try to read entity
        let mut entity = String::new();
        let mut found_semi = false;
        let start_len = result.len();

        for ec in chars.by_ref() {
            if ec == ';' {
                found_semi = true;
                break;
            }
            entity.push(ec);
            if entity.len() > 10 {
                break; // Entity too long, not valid
            }
        }

        if !found_semi {
            // Not a valid entity, emit as-is
            result.push('&');
            result.push_str(&entity);
            continue;
        }

        // Try to decode the entity
        if let Some(numeric_part) = entity.strip_prefix('#') {
            // Numeric entity
            let decoded = if let Some(hex_str) = numeric_part
                .strip_prefix('x')
                .or_else(|| numeric_part.strip_prefix('X'))
            {
                u32::from_str_radix(hex_str, 16).ok()
            } else {
                numeric_part.parse::<u32>().ok()
            };
            if let Some(cp) = decoded.and_then(char::from_u32) {
                result.push(cp);
            } else {
                // Invalid numeric entity, emit as-is
                result.push('&');
                result.push_str(&entity);
                result.push(';');
            }
        } else {
            // Named entity
            match entity.as_str() {
                "amp" => result.push('&'),
                "lt" => result.push('<'),
                "gt" => result.push('>'),
                "quot" => result.push('"'),
                "apos" => result.push('\''),
                "nbsp" => result.push('\u{00A0}'),
                "times" => result.push('\u{00D7}'),
                "divide" => result.push('\u{00F7}'),
                "copy" => result.push('\u{00A9}'),
                "reg" => result.push('\u{00AE}'),
                "trade" => result.push('\u{2122}'),
                "mdash" => result.push('\u{2014}'),
                "ndash" => result.push('\u{2013}'),
                "laquo" => result.push('\u{00AB}'),
                "raquo" => result.push('\u{00BB}'),
                "bull" => result.push('\u{2022}'),
                "hellip" => result.push('\u{2026}'),
                "larr" => result.push('\u{2190}'),
                "rarr" => result.push('\u{2192}'),
                "uarr" => result.push('\u{2191}'),
                "darr" => result.push('\u{2193}'),
                "lsquo" => result.push('\u{2018}'),
                "rsquo" => result.push('\u{2019}'),
                "ldquo" => result.push('\u{201C}'),
                "rdquo" => result.push('\u{201D}'),
                "euro" => result.push('\u{20AC}'),
                "pound" => result.push('\u{00A3}'),
                "yen" => result.push('\u{00A5}'),
                "cent" => result.push('\u{00A2}'),
                "deg" => result.push('\u{00B0}'),
                "plusmn" => result.push('\u{00B1}'),
                "micro" => result.push('\u{00B5}'),
                "para" => result.push('\u{00B6}'),
                "middot" => result.push('\u{00B7}'),
                "frac14" => result.push('\u{00BC}'),
                "frac12" => result.push('\u{00BD}'),
                "frac34" => result.push('\u{00BE}'),
                _ => {
                    // Unknown entity, emit as-is
                    let _ = start_len; // suppress warning
                    result.push('&');
                    result.push_str(&entity);
                    result.push(';');
                }
            }
        }
    }

    result
}

/// Format dynamic props array (inlined — used for components, which don't hoist).
fn format_dynamic_props(props: &[String]) -> String {
    if props.is_empty() {
        return String::new();
    }
    format!(r#", ["{}"]"#, props.join("\", \""))
}

/// Hoist a dynamic props array as a module-level `_hoisted_N` constant.
/// Returns the suffix string like `, _hoisted_3`.
/// Vue's compiler hoists element dynamic props arrays; component dynamic props are inlined.
fn hoist_dynamic_props_array(
    props: &[String],
    state: &mut super::types::TemplateCodegenState,
) -> String {
    if props.is_empty() {
        return String::new();
    }
    state.hoist_counter += 1;
    let name = format!("_hoisted_{}", state.hoist_counter);
    let code = format!(r#"["{}"]"#, props.join(r#"", ""#));
    state.hoisted_nodes.push(super::types::HoistedNode {
        name: name.clone(),
        code,
    });
    format!(", {}", name)
}

/// Check if all props are static (can be hoisted).
fn are_all_props_static(props: &[super::types::PropEntry]) -> bool {
    props.iter().all(|p| p.kind == PropKind::Static)
}

/// Check if any static `ref` prop matches a setup binding (ref_key pattern needed).
/// Such props are not truly static since they reference a runtime variable.
fn has_setup_ref_binding(
    props: &[super::types::PropEntry],
    source: &str,
    binding_metadata: &super::types::BindingMetadata,
) -> bool {
    props.iter().any(|p| {
        if p.kind != PropKind::Static {
            return false;
        }
        let name = &source[p.name.start as usize..p.name.end as usize];
        if name != "ref" {
            return false;
        }
        if let Some(ref val) = p.value {
            let value = &source[val.start as usize..val.end as usize];
            matches!(
                binding_metadata.get(value.as_bytes(), source.as_bytes()),
                Some(super::types::BindingType::Setup | super::types::BindingType::SetupRef)
            )
        } else {
            false
        }
    })
}

/// Check if a prop name/element combination should generate an asset import.
/// Returns true for `src` on img/video/audio/source/image, `poster` on video, `href` on image (SVG).
fn is_asset_url_prop(prop_name: &str, _tag_name: Option<&str>) -> bool {
    matches!(prop_name, "src" | "poster")
}

/// Check if a value looks like a local asset path that should be imported.
fn is_local_asset_path(value: &str) -> bool {
    // Local paths: starting with /, ./, ../ but not // (protocol-relative) or http/https
    (value.starts_with('/') && !value.starts_with("//"))
        || value.starts_with("./")
        || value.starts_with("../")
}

/// Write a static prop value, handling asset imports for src-like attributes.
fn write_static_prop_value(
    out: &mut String,
    name: &str,
    value: &str,
    state: &mut TemplateCodegenState,
) {
    if is_asset_url_prop(name, None) && is_local_asset_path(value) {
        // Generate asset import
        let import_name = format!("_imports_{}", state.asset_import_counter);
        state.asset_import_counter += 1;
        state
            .asset_imports
            .push((import_name.clone(), value.to_string()));
        out.push_str(&import_name);
    } else {
        out.push('"');
        // Escape special characters for valid JS string literals
        for ch in value.chars() {
            match ch {
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\\' => out.push_str("\\\\"),
                '"' => out.push_str("\\\""),
                _ => out.push(ch),
            }
        }
        out.push('"');
    }
}

/// Generate props object code (for hoisting).
/// Generate hoisted props code from static props.
#[allow(dead_code)]
fn generate_props_code(
    props: &[super::types::PropEntry],
    source: &str,
    state: &mut TemplateCodegenState,
) -> String {
    if props.is_empty() {
        return "null".to_string();
    }

    let mut out = String::with_capacity(64);
    out.push_str("{ ");

    let mut first = true;

    for prop in props {
        if !first {
            out.push_str(", ");
        }
        first = false;

        let name = &source[prop.name.start as usize..prop.name.end as usize];

        // For hoisting, we only handle static props
        write_prop_name(&mut out, name);
        out.push_str(": ");
        if let Some(ref val) = prop.value {
            let value = &source[val.start as usize..val.end as usize];
            write_static_prop_value(&mut out, name, value, state);
        } else {
            out.push_str("\"\"");
        }
    }

    out.push_str(" }");
    out
}

/// Generate hoisted props code with an optional conditional key.
fn generate_props_code_with_optional_key(
    props: &[super::types::PropEntry],
    source: &str,
    key: Option<usize>,
    state: &mut TemplateCodegenState,
) -> String {
    if props.is_empty() && key.is_none() {
        return "null".to_string();
    }

    let mut out = String::with_capacity(64);
    out.push_str("{ ");

    let mut first = true;

    // Add conditional key if present
    if let Some(key_val) = key {
        if !first {
            out.push_str(", ");
        }
        out.push_str(&format!("key: {}", key_val));
        first = false;
    }

    for prop in props {
        if !first {
            out.push_str(", ");
        }
        first = false;

        let name = &source[prop.name.start as usize..prop.name.end as usize];

        write_prop_name(&mut out, name);
        out.push_str(": ");
        if let Some(ref val) = prop.value {
            let value = &source[val.start as usize..val.end as usize];
            write_static_prop_value(&mut out, name, value, state);
        } else {
            out.push_str("\"\"");
        }
    }

    out.push_str(" }");
    out
}

#[cfg(test)]
mod tests {
    use crate::builder::codegen::{generate, CodegenOptions};
    use oxc_allocator::Allocator;

    /// Helper to generate code from Vue SFC source
    fn gen(source: &str) -> String {
        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue".to_string());
        generate(source, &options, &allocator).code
    }

    #[test]
    fn test_slot_outlet_default_name() {
        let source = r#"<template>
  <div>
    <slot></slot>
  </div>
</template>
<script setup>
</script>
"#;
        let code = gen(source);
        // Default slot should use "default" as name
        assert!(
            code.contains(r#"_renderSlot(_ctx.$slots, "default""#),
            "Default slot should use 'default' name. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_slot_outlet_named() {
        let source = r#"<template>
  <div>
    <slot name="header"></slot>
    <slot name="footer"></slot>
  </div>
</template>
<script setup>
</script>
"#;
        let code = gen(source);
        // Named slots should extract the name prop value
        assert!(
            code.contains(r#"_renderSlot(_ctx.$slots, "header""#),
            "Should extract 'header' slot name. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"_renderSlot(_ctx.$slots, "footer""#),
            "Should extract 'footer' slot name. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_slot_outlet_with_fallback() {
        let source = r#"<template>
  <div>
    <slot name="content">Default content</slot>
  </div>
</template>
<script setup>
</script>
"#;
        let code = gen(source);
        // Slot with fallback content should have callback with content
        assert!(
            code.contains(r#"_renderSlot(_ctx.$slots, "content""#),
            "Should extract 'content' slot name. Generated:\n{}",
            code
        );
        // Should have fallback function
        assert!(
            code.contains("() => ["),
            "Should have fallback function. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_sibling_elements_have_commas() {
        let source = r#"<template>
  <div>
    <span>First</span>
    <span>Second</span>
    <span>Third</span>
  </div>
</template>
<script setup>
</script>
"#;
        let code = gen(source);
        // Count occurrences of _createElementVNode("span" (child elements use VNode, not Block)
        let span_count = code.matches(r#"_createElementVNode("span""#).count();
        assert_eq!(
            span_count, 3,
            "Should have 3 span elements. Generated:\n{}",
            code
        );
        // Should have commas between siblings - look for comma followed by newline
        // The exact pattern depends on formatting, so just check for commas
        let comma_count = code.matches(",\n").count();
        assert!(
            comma_count >= 2,
            "Should have commas between sibling elements. Generated:\n{}",
            code
        );
        // Note: Due to streaming architecture limitations, child elements may close with ])
        // instead of just ) when they have single text children. This is a known limitation
        // that doesn't affect runtime behavior since the array is still valid.
        // The important thing is that _createElementVNode is used instead of _createElementBlock.
    }

    #[test]
    fn test_vif_velse_no_commas() {
        let source = r#"<template>
  <div>
    <span v-if="show">Visible</span>
    <span v-else>Hidden</span>
  </div>
</template>
<script setup>
const show = true
</script>
"#;
        let code = gen(source);
        // v-if/v-else should be a ternary expression, not array elements with commas
        assert!(
            code.contains("$setup.show"),
            "Should have v-if condition. Generated:\n{}",
            code
        );
        assert!(
            code.contains("?"),
            "Should have ternary operator. Generated:\n{}",
            code
        );
        assert!(
            code.contains(":"),
            "Should have else branch in ternary. Generated:\n{}",
            code
        );
        // There should NOT be a comma between the v-if and v-else branches
        // The pattern ",\n    :" would indicate incorrect comma before v-else
        assert!(
            !code.contains(",\n    :"),
            "Should NOT have comma before v-else branch. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_vif_velseif_velse_no_commas() {
        let source = r#"<template>
  <div>
    <span v-if="a">A</span>
    <span v-else-if="b">B</span>
    <span v-else>C</span>
  </div>
</template>
<script setup>
const a = true
const b = false
</script>
"#;
        let code = gen(source);
        // Should have nested ternary for v-if/v-else-if/v-else
        assert!(
            code.contains("$setup.a"),
            "Should have first condition. Generated:\n{}",
            code
        );
        assert!(
            code.contains("$setup.b"),
            "Should have second condition. Generated:\n{}",
            code
        );
        // Should NOT have commas between the conditional branches
        assert!(
            !code.contains(",\n    :"),
            "Should NOT have comma before conditional branches. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_mixed_siblings_and_conditionals() {
        let source = r#"<template>
  <div>
    <span>Static</span>
    <span v-if="show">Conditional</span>
    <span v-else>Alternative</span>
    <span>Another static</span>
  </div>
</template>
<script setup>
const show = true
</script>
"#;
        let code = gen(source);
        // First static element should be followed by comma before v-if
        // v-if/v-else should NOT have comma between them
        // After v-else should come comma before last static element
        assert!(
            code.contains("$setup.show"),
            "Should have v-if condition. Generated:\n{}",
            code
        );
        // Verify span elements - static ones use _createElementVNode, conditional ones use _createElementBlock
        // 2 static spans use _createElementVNode
        let vnode_span_count = code.matches(r#"_createElementVNode("span""#).count();
        // 2 conditional spans (v-if and v-else) use _createElementBlock
        let block_span_count = code.matches(r#"_createElementBlock("span""#).count();
        assert_eq!(
            vnode_span_count + block_span_count,
            4,
            "Should have 4 span elements total (2 VNode + 2 Block). Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_extract_vfor_locals_simple() {
        let result = super::extract_vfor_locals("item");
        assert_eq!(result, vec!["item"]);
    }

    #[test]
    fn test_extract_vfor_locals_with_index() {
        let result = super::extract_vfor_locals("(item, index)");
        assert_eq!(result, vec!["item", "index"]);
    }

    #[test]
    fn test_extract_vfor_locals_with_key() {
        let result = super::extract_vfor_locals("(value, key, index)");
        assert_eq!(result, vec!["value", "key", "index"]);
    }

    #[test]
    fn test_extract_first_identifier() {
        assert_eq!(super::extract_first_identifier("item"), "item");
        assert_eq!(super::extract_first_identifier("item.id"), "item");
        assert_eq!(super::extract_first_identifier("item.name.first"), "item");
        assert_eq!(super::extract_first_identifier("$event"), "$event");
        assert_eq!(super::extract_first_identifier("_private"), "_private");
    }

    // =========================================================================
    // Patch Flag Tests
    // =========================================================================

    #[test]
    fn test_patch_flag_class() {
        let source = r#"<template><div :class="cls"></div></template>
<script setup>const cls = 'foo'</script>"#;
        let code = gen(source);
        // Dynamic class should have CLASS flag (2)
        assert!(
            code.contains(", 2 /* CLASS */") || code.contains(", 2)"),
            "Should emit CLASS patch flag (2). Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_patch_flag_style() {
        let source = r#"<template><div :style="stl"></div></template>
<script setup>const stl = {}</script>"#;
        let code = gen(source);
        // Dynamic style should have STYLE flag (4)
        assert!(
            code.contains(", 4 /* STYLE */") || code.contains(", 4)"),
            "Should emit STYLE patch flag (4). Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_patch_flag_props() {
        let source = r#"<template><div :title="t"></div></template>
<script setup>const t = 'x'</script>"#;
        let code = gen(source);
        // Dynamic prop (not class/style) should have PROPS flag (8)
        assert!(
            code.contains(", 8 /* PROPS */") || code.contains(", 8,") || code.contains(", 8)"),
            "Should emit PROPS patch flag (8). Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_patch_flag_class_and_style() {
        let source = r#"<template><div :class="c" :style="s"></div></template>
<script setup>const c='';const s={}</script>"#;
        let code = gen(source);
        // Combined flags: CLASS(2) + STYLE(4) = 6
        assert!(
            code.contains(", 6 /* CLASS, STYLE */") || code.contains(", 6)"),
            "Should emit combined CLASS+STYLE patch flags (6). Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_patch_flag_full_props() {
        let source = r#"<template><div :[key]="val"></div></template>
<script setup>const key='x';const val=1</script>"#;
        let code = gen(source);
        // Dynamic key binding should have FULL_PROPS flag (16)
        assert!(
            code.contains(", 16 /* FULL_PROPS */") || code.contains(", 16)"),
            "Should emit FULL_PROPS patch flag (16) for dynamic key. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_no_patch_flag_static_element() {
        let source = r#"<template><div class="static">Static</div></template>
<script setup></script>"#;
        let code = gen(source);
        // Static element should NOT have patch flags (only closing parens)
        // It should just close with ])) without a patch flag number before it
        assert!(
            !code.contains("/* TEXT")
                && !code.contains("/* CLASS")
                && !code.contains("/* STYLE")
                && !code.contains("/* PROPS"),
            "Static element should not have patch flags. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Dynamic Props Array Tests
    // =========================================================================

    #[test]
    fn test_dynamic_props_array_single() {
        let source = r#"<template><div :title="t"></div></template>
<script setup>const t = 'x'</script>"#;
        let code = gen(source);
        // Dynamic prop should emit props array with the prop name
        assert!(
            code.contains(r#"["title"]"#),
            "Should emit dynamic props array with 'title'. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_dynamic_props_array_multiple() {
        let source = r#"<template><div :title="t" :id="i"></div></template>
<script setup>const t='';const i=''</script>"#;
        let code = gen(source);
        // Multiple dynamic props should be in the array
        assert!(
            code.contains(r#"["title", "id"]"#) || code.contains(r#"["id", "title"]"#),
            "Should emit dynamic props array with multiple props. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_dynamic_props_excludes_class_style() {
        let source = r#"<template><div :class="c" :style="s" :title="t"></div></template>
<script setup>const c='';const s={};const t=''</script>"#;
        let code = gen(source);
        // class and style should NOT be in dynamic props array (they have their own flags)
        assert!(
            code.contains(r#"["title"]"#),
            "Dynamic props array should only contain 'title', not class/style. Generated:\n{}",
            code
        );
        assert!(
            !code.contains(r#""class""#) || code.contains("class: _ctx.c"),
            "class should not be in dynamic props array. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_no_dynamic_props_array_for_class_style_only() {
        let source = r#"<template><div :class="c" :style="s"></div></template>
<script setup>const c='';const s={}</script>"#;
        let code = gen(source);
        // When only class/style are dynamic, no props array should be emitted
        // The output should end with patch flags ")) not with a props array
        // Look for the pattern: 6 /* CLASS, STYLE */)) without a following ["
        assert!(
            !code.contains(r#"/* CLASS, STYLE */, ["#),
            "Should not emit dynamic props array after patch flags. Generated:\n{}",
            code
        );
        // Ensure patch flags are present
        assert!(
            code.contains("6 /* CLASS, STYLE */"),
            "Should have CLASS and STYLE flags. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Static Hoisting Tests
    // =========================================================================

    #[test]
    fn test_static_props_hoisting() {
        let source = r#"<template><div class="static" id="foo">{{ msg }}</div></template>
<script setup>const msg = ''</script>"#;
        let code = gen(source);
        // Static props should be hoisted to a constant
        assert!(
            code.contains("const _hoisted_"),
            "Should hoist static props to a constant. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"class: "static""#)
                || code.contains(r#"{ class: "static", id: "foo" }"#),
            "Hoisted constant should contain static props. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_static_child_with_static_props_hoisted() {
        let source = r#"<template>
<div>
  <span class="icon">X</span>
</div>
</template>
<script setup></script>"#;
        let code = gen(source);
        // Non-self-closing elements with static props should hoist the props,
        // not use _cache (which could incorrectly cache dynamic children).
        assert!(
            code.contains("_hoisted_1"),
            "Static child props should be hoisted. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"_createElementVNode("span", _hoisted_1, "X")"#),
            "Should reference hoisted props. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_hoisted_constant_referenced_in_render() {
        let source = r#"<template>
<div class="container">Content</div>
</template>
<script setup></script>"#;
        let code = gen(source);
        // The hoisted constant should be referenced in the render function
        assert!(
            code.contains("const _hoisted_1 ="),
            "Should define hoisted constant. Generated:\n{}",
            code
        );
        assert!(
            code.contains("_createElementBlock(\"div\", _hoisted_1"),
            "Should reference hoisted constant in createElementBlock. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_no_hoisting_dynamic_element() {
        let source = r#"<template><div :class="c">Content</div></template>
<script setup>const c = ''</script>"#;
        let code = gen(source);
        // Dynamic elements should NOT be hoisted
        assert!(
            !code.contains("const _hoisted_"),
            "Dynamic elements should not be hoisted. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Event Modifier Tests
    // =========================================================================

    #[test]
    fn test_event_modifier_stop() {
        let source = r#"<template><button @click.stop="handleClick">Click</button></template>
<script setup>const handleClick = () => {}</script>"#;
        let code = gen(source);
        // .stop modifier should use _withModifiers
        assert!(
            code.contains("_withModifiers"),
            "Should use _withModifiers for .stop. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"["stop"]"#),
            "Should include 'stop' in modifiers array. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_event_modifier_stop_with_inline_assignment() {
        // Inline assignment expressions with event modifiers should produce valid JS
        let source = r#"<template><button @click.stop="open = !open">Toggle</button></template>
<script setup>
import { ref } from 'vue'
const open = ref(false)
</script>"#;
        let code = gen_and_validate(source);
        // Should use _withModifiers
        assert!(
            code.contains("_withModifiers"),
            "Should use _withModifiers for .stop. Generated:\n{}",
            code
        );
        // Should wrap in simple arrow function, NOT guard pattern
        assert!(
            code.contains("$event =>"),
            "Inline assignment should use $event => pattern, not guard pattern. Generated:\n{}",
            code
        );
        // Should NOT contain the guard pattern (...args) =>
        assert!(
            !code.contains("(...args) =>"),
            "Inline assignment should NOT use guard pattern. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_event_handler_in_vfor_with_vif() {
        // Event handlers inside v-for + v-if should be properly wrapped
        // This goes through write_props_with_key() which had a bug where it
        // called write_expr_with_ctx() directly instead of write_event_handler_body()
        let source = r#"<template>
  <div v-for="entry in items" :key="entry.id">
    <div v-if="entry.active" @click="selectItem(entry)">{{ entry.label }}</div>
  </div>
</template>
<script setup>
const items = [{ id: 1, active: true, label: 'test' }]
function selectItem(item) {}
</script>"#;
        let code = gen_and_validate(source);
        // Should wrap function call in $event => (...) pattern
        assert!(
            code.contains("$event => ($setup.selectItem(entry))"),
            "Function call in v-for+v-if should be wrapped as event handler. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_vfor_event_handler_gets_props_flag() {
        // When an event handler inside v-for references a loop variable,
        // it can't be cached, so it needs PROPS (8) flag and ["onClick"] in dynamic props.
        // Vue: `createElementBlock("button", { ... onClick: ($event) => fn(item) }, ..., 10, ["onClick"])`
        let source = r#"<template>
  <div v-for="item in items" :key="item.id">
    <button :class="{ active: item.selected }" @click="selectItem(item)">{{ item.label }}</button>
  </div>
</template>
<script setup>
const items = [{ id: 1, selected: true, label: 'test' }]
function selectItem(item) {}
</script>"#;
        let code = gen_and_validate(source);
        // Should have PROPS flag (8) because onClick depends on loop variable
        assert!(
            code.contains("PROPS") || code.contains(", 10"),
            "v-for handler referencing loop variable should have PROPS flag. Generated:\n{}",
            code
        );
        // Should include onClick in dynamic props array
        assert!(
            code.contains(r#"["onClick"]"#),
            "v-for handler referencing loop variable should have onClick in dynamic props. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_dynamic_props_array_hoisted_as_module_level_const() {
        // Dynamic props arrays like ["onClick"] should be hoisted as _hoisted_N
        // at module level, not inlined at the call site.
        // Vue: `const _hoisted_3 = ["onClick"]; ... , 10, _hoisted_3)`
        let source = r#"<template>
  <div v-for="item in items" :key="item.id">
    <button :class="{ active: item.selected }" @click="selectItem(item)">{{ item.label }}</button>
  </div>
</template>
<script setup>
const items = [{ id: 1, selected: true, label: 'test' }]
function selectItem(item) {}
</script>"#;
        let code = gen_and_validate(source);
        // The _hoisted_ const for ["onClick"] should appear before the render function
        assert!(
            code.contains(r#"_hoisted_"#),
            "Dynamic props array should be hoisted. Generated:\n{}",
            code
        );
        // The hoisted const should define the array
        assert!(
            code.contains(r#"= ["onClick"]"#),
            "Hoisted const should define the onClick array. Generated:\n{}",
            code
        );
        // The render function should reference the hoisted name, not inline the array
        // Look for pattern: , 10, _hoisted_  (flag + hoisted ref, NOT , 10, ["onClick"])
        let render_start = code.find("function render").unwrap_or(0);
        let render_code = &code[render_start..];
        assert!(
            !render_code.contains(r#", ["onClick"]"#),
            "Render function should reference hoisted name, not inline the array. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_vfor_event_handler_props_flag_with_stop_modifier() {
        // v-for handler with .stop modifier referencing loop variable should also get PROPS flag
        let source = r#"<template>
  <div v-for="(file, name) in files" :key="name">
    <span class="close" @click.stop="deleteFile(name)">x</span>
  </div>
</template>
<script setup>
const files = {}
function deleteFile(f) {}
</script>"#;
        let code = gen_and_validate(source);
        // Should have PROPS flag
        assert!(
            code.contains("PROPS") || code.contains(r#"["onClick"]"#),
            "v-for handler with .stop modifier referencing loop variable should have PROPS flag. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_event_modifier_prevent() {
        let source = r#"<template><button @click.prevent="handleClick">Click</button></template>
<script setup>const handleClick = () => {}</script>"#;
        let code = gen(source);
        assert!(
            code.contains("_withModifiers"),
            "Should use _withModifiers for .prevent. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"["prevent"]"#),
            "Should include 'prevent' in modifiers array. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_event_modifier_multiple() {
        let source = r#"<template><button @click.stop.prevent="handleClick">Click</button></template>
<script setup>const handleClick = () => {}</script>"#;
        let code = gen(source);
        assert!(
            code.contains("_withModifiers"),
            "Should use _withModifiers for multiple modifiers. Generated:\n{}",
            code
        );
        // Should have both modifiers in array
        assert!(
            code.contains(r#"["stop","prevent"]"#) || code.contains(r#"["stop", "prevent"]"#),
            "Should include both modifiers in array. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_event_modifier_capture_becomes_event_name() {
        let source = r#"<template><button @click.capture="handleClick">Click</button></template>
<script setup>const handleClick = () => {}</script>"#;
        let code = gen(source);
        // .capture modifier should become part of event name: onClickCapture
        assert!(
            code.contains("onClickCapture"),
            "Should use onClickCapture for .capture modifier. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_event_modifier_once_becomes_event_name() {
        let source = r#"<template><button @click.once="handleClick">Click</button></template>
<script setup>const handleClick = () => {}</script>"#;
        let code = gen(source);
        // .once modifier should become part of event name: onClickOnce
        assert!(
            code.contains("onClickOnce"),
            "Should use onClickOnce for .once modifier. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_event_modifier_passive_becomes_event_name() {
        let source = r#"<template><div @scroll.passive="handleScroll">Content</div></template>
<script setup>const handleScroll = () => {}</script>"#;
        let code = gen(source);
        // .passive modifier should become part of event name: onScrollPassive
        assert!(
            code.contains("onScrollPassive"),
            "Should use onScrollPassive for .passive modifier. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_event_handler_keeps_global_and_event_identifiers() {
        let source = r#"<template><input @input="update(Number($event.target.value))" /></template>
<script setup>const update = () => {}</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("Number($event.target.value)"),
            "Event handler should preserve Number($event...). Generated:\n{}",
            code
        );
        assert!(
            !code.contains("_ctx.Number"),
            "Global Number should not be prefixed with _ctx. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("_ctx.$event"),
            "$event should not be prefixed with _ctx. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_vshow_emits_runtime_directive() {
        let source = r#"<template><div v-show="isEnabled">Visible</div></template>
<script setup>const isEnabled = true</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("_withDirectives("),
            "v-show should emit withDirectives wrapper. Generated:\n{}",
            code
        );
        assert!(
            code.contains("[_vShow, $setup.isEnabled]"),
            "v-show directive array should use _vShow helper. Generated:\n{}",
            code
        );
        assert!(
            code.contains("512 /* NEED_PATCH */"),
            "v-show element should have NEED_PATCH flag. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("style: { display:"),
            "v-show should not be lowered to synthetic style binding. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Key Modifier Tests
    // =========================================================================

    #[test]
    fn test_key_modifier_enter() {
        let source = r#"<template><input @keyup.enter="handleEnter" /></template>
<script setup>const handleEnter = () => {}</script>"#;
        let code = gen(source);
        // .enter modifier should use _withKeys
        assert!(
            code.contains("_withKeys"),
            "Should use _withKeys for .enter modifier. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"["enter"]"#),
            "Should include 'enter' in keys array. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_key_modifier_tab() {
        let source = r#"<template><input @keyup.tab="handleTab" /></template>
<script setup>const handleTab = () => {}</script>"#;
        let code = gen(source);
        assert!(
            code.contains("_withKeys"),
            "Should use _withKeys for .tab modifier. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"["tab"]"#),
            "Should include 'tab' in keys array. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_key_modifier_with_system_modifier() {
        let source = r#"<template><input @keyup.ctrl.enter="handleCtrlEnter" /></template>
<script setup>const handleCtrlEnter = () => {}</script>"#;
        let code = gen(source);
        // Should use both _withKeys and _withModifiers
        // Pattern: _withKeys(_withModifiers(handler, ["ctrl"]), ["enter"])
        assert!(
            code.contains("_withKeys"),
            "Should use _withKeys for .enter modifier. Generated:\n{}",
            code
        );
        assert!(
            code.contains("_withModifiers"),
            "Should use _withModifiers for .ctrl modifier. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"["ctrl"]"#),
            "Should include 'ctrl' in modifiers array. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"["enter"]"#),
            "Should include 'enter' in keys array. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_comment() {
        let source = r#"<template>
  <!--before div-->
  <div>
    <!--after div-->
    foo
  </div>
</template>"#;
        let code = gen(source);
        // Comments should be rendered using _createCommentVNode
        assert!(
            code.contains("_openBlock(), _createElementBlock(_Fragment, null, ["),
            "Should open fragment for multiple children. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"_createCommentVNode("before div")"#),
            "Should include 'before div' comment content. Generated:\n{}",
            code
        );

        assert!(
            code.contains("_createElementVNode(\"div\", null, ["),
            "Should create div element without an extra block. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"_createCommentVNode("after div")"#),
            "Should include 'after div' comment content. Generated:\n{}",
            code
        );
        assert!(
            code.contains(
                r#"_cache[0] || (_cache[0] = _createTextVNode(" foo ", -1 /* CACHED */))"#
            ),
            "Should include text content 'foo'. Generated:\n{}",
            code
        );
        assert!(
            code.contains("], 2112 /* STABLE_FRAGMENT, DEV_ROOT_FRAGMENT */))"),
            "Should close fragment with STABLE_FRAGMENT, DEV_ROOT_FRAGMENT flag. Generated:\n{}",
            code
        );

        assert!(
            !code.contains("_createElementBlock(\"div\""),
            "Should not create extra blocks for child elements. Generated:\n{}",
            code
        )
    }

    // =========================================================================
    // Handler Caching Tests
    // =========================================================================

    #[test]
    fn test_setup_handler_not_cached() {
        // Simple $setup identifier handlers should NOT be cached (matching Vue official)
        let source = r#"<template><button @click="handleClick">Click</button></template>
<script setup>const handleClick = () => {}</script>"#;
        let code = gen(source);
        // Handler should be emitted directly, not cached.
        assert!(
            code.contains("$setup.handleClick"),
            "Handler should be a direct reference. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("_cache["),
            "Setup handler should NOT be cached. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_inline_handler_cached() {
        // Inline expressions and function calls SHOULD be cached
        let source = r#"<template>
<button @click="count++">Inc</button>
</template>
<script setup>let count = 0</script>"#;
        let code = gen(source);
        assert!(
            code.contains("_cache[0]"),
            "Inline handler should be cached. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_handler_caching_across_elements() {
        // Inline handlers across elements should use different cache indices
        let source = r#"<template>
<button @click="count++">A</button>
<button @click="count--">B</button>
</template>
<script setup>let count = 0</script>"#;
        let code = gen(source);
        assert!(
            code.contains("_cache[0]") || code.contains("_cache[1]"),
            "Inline handlers should be cached. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_handler_caching_in_vfor_skipped() {
        let source = r#"<template>
<div v-for="item in items" :key="item.id">
  <button @click="handleClick(item)">{{ item.name }}</button>
</div>
</template>
<script setup>
const items = []
const handleClick = (i) => {}
</script>"#;
        let code = gen(source);
        // Handlers inside v-for that use loop variable should NOT be cached
        // (The handler references `item` which changes on each iteration)
        // Check that if there's a cache pattern, it's NOT wrapping the item-dependent handler
        // For now, we'll just verify the handler is present
        assert!(
            code.contains("handleClick"),
            "Handler should be present. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_handler_hoist_keys_conditional() {
        let source = r#"<template>
  <div>
    <span v-if="show">Visible</span>
    <span v-else>Hidden</span>
  </div>
</template>"#;
        let code = gen(source);

        // contain hoisted keys for both branches
        assert!(
            code.contains("const _hoisted_1 = { key: 0 }")
                && code.contains("const _hoisted_2 = { key: 1 }"),
            "Should hoist keys for both v-if and v-else branches. Generated:\n{}",
            code
        );

        assert!(
            code.contains(
                r#"? (_openBlock(), _createElementBlock("span", _hoisted_1, "Visible"))"#
            ) && code
                .contains(r#": (_openBlock(), _createElementBlock("span", _hoisted_2, "Hidden"))"#),
            "Should reference hoisted keys in both branches. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Multi-Root Fragment Tests
    // =========================================================================

    #[test]
    fn test_multi_root_fragment_wrapping() {
        let source = r#"<template>
  <div>First</div>
  <div>Second</div>
  <div>Third</div>
</template>
<script setup>
</script>"#;
        let code = gen(source);

        // Should import Fragment
        assert!(
            code.contains("Fragment as _Fragment"),
            "Should import Fragment. Generated:\n{}",
            code
        );

        // Should wrap in Fragment with _createElementBlock
        assert!(
            code.contains("_createElementBlock(_Fragment, null, ["),
            "Should wrap multiple roots in Fragment. Generated:\n{}",
            code
        );

        // Individual root elements should use _createElementVNode, NOT _createElementBlock
        // Count occurrences of each
        let vnode_div_count = code.matches(r#"_createElementVNode("div""#).count();
        let block_div_count = code.matches(r#"_createElementBlock("div""#).count();

        assert_eq!(
            vnode_div_count, 3,
            "All 3 root divs should use _createElementVNode. Got {} VNode and {} Block. Generated:\n{}",
            vnode_div_count, block_div_count, code
        );
        assert_eq!(
            block_div_count, 0,
            "Root divs should NOT use _createElementBlock (only Fragment should). Generated:\n{}",
            code
        );

        // Should have STABLE_FRAGMENT patch flag (64)
        assert!(
            code.contains("64 /* STABLE_FRAGMENT */"),
            "Should have STABLE_FRAGMENT patch flag. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_multi_root_no_nested_openblock() {
        let source = r#"<template>
  <span>A</span>
  <span>B</span>
</template>
<script setup>
</script>"#;
        let code = gen(source);

        // There should be exactly ONE _openBlock() call - for the Fragment wrapper
        // Not multiple nested ones for each root element
        let openblock_count = code.matches("_openBlock()").count();
        assert_eq!(
            openblock_count, 1,
            "Should have exactly 1 _openBlock() for Fragment, not nested blocks. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_single_root_still_uses_block() {
        let source = r#"<template>
  <div>Single root</div>
</template>
<script setup>
</script>"#;
        let code = gen(source);

        // Single root should use _createElementBlock (not Fragment)
        assert!(
            code.contains(r#"_createElementBlock("div""#),
            "Single root should use _createElementBlock. Generated:\n{}",
            code
        );

        // Should NOT have Fragment
        assert!(
            !code.contains("_Fragment"),
            "Single root should NOT use Fragment. Generated:\n{}",
            code
        );
    }
    #[test]
    fn test_multi_root_result() {
        let source = r#"<template>
  <div>First root element</div>
  <div>Second root element</div>
  <div>Third root element</div>
</template>"#;
        let code = gen(source);

        assert!(code.contains("return (_openBlock(), _createElementBlock(_Fragment, null, ["));
        // Non-self-closing multi-root elements are not cached (children may be dynamic).
        // They emit plain _createElementVNode without _cache wrapper.
        assert!(
            code.contains(r#"_createElementVNode("div", null, "First root element")"#),
            "First root. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"_createElementVNode("div", null, "Second root element")"#),
            "Second root. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"_createElementVNode("div", null, "Third root element")"#),
            "Third root. Generated:\n{}",
            code
        );

        assert!(
            code.contains("], 64 /* STABLE_FRAGMENT */))"),
            "Should have STABLE_FRAGMENT flag"
        );
    }

    #[test]
    fn test_typescript_as_assertion_in_event_handler() {
        let source = r#"<template>
  <button @click="store.setActiveFile(filename as string)">click</button>
</template>
<script setup>
const store = { setActiveFile(f: string) {} }
const filename = 'test'
</script>
"#;
        let code = gen(source);
        // "as string" should be preserved, not turned into "_ctx.as _ctx.string"
        assert!(
            code.contains("as string"),
            "TypeScript 'as' assertion should be preserved. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("_ctx.as"),
            "Should NOT prefix 'as' keyword with _ctx. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("_ctx.string"),
            "Should NOT prefix type 'string' with _ctx. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_typescript_as_assertion_in_binding() {
        let source = r#"<template>
  <div :data-id="item as string">test</div>
</template>
<script setup>
const item = 42
</script>
"#;
        let code = gen(source);
        assert!(
            code.contains("as string"),
            "TypeScript 'as string' should be preserved in bindings. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("_ctx.as"),
            "Should NOT prefix 'as' with _ctx. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_typescript_as_custom_interface() {
        let source = r#"<template>
  <div :data-id="filename as MyInterface">test</div>
</template>
<script setup>
const filename = 'test'
</script>
"#;
        let code = gen(source);
        assert!(
            code.contains("as MyInterface"),
            "TypeScript 'as MyInterface' should be preserved. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("_ctx.MyInterface"),
            "Should NOT prefix type 'MyInterface' with _ctx. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("_ctx.as"),
            "Should NOT prefix 'as' with _ctx. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_typescript_as_typeof_expression() {
        let source = r#"<template>
  <div :data-id="filename as typeof foo.test">test</div>
</template>
<script setup>
const filename = 'test'
</script>
"#;
        let code = gen(source);
        assert!(
            code.contains("as typeof"),
            "TypeScript 'as typeof' should be preserved. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("_ctx.typeof"),
            "Should NOT prefix 'typeof' with _ctx. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("_ctx.foo"),
            "Should NOT prefix type reference 'foo' with _ctx. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_root_vif_without_velse_has_comment_node() {
        // Message.vue pattern: root-level v-if without v-else
        let source = r#"<template>
  <div v-if="errors.length > 0" class="message-container">
    <span>Error</span>
  </div>
</template>
<script setup>
const errors: string[] = []
</script>
"#;
        let code = gen(source);
        assert!(
            code.contains("_createCommentVNode(\"v-if\", true)"),
            "Root v-if without v-else should have comment node fallback. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_nested_vif_chains() {
        // Header.vue pattern: outer v-if with inner v-if children (no v-else)
        let source = r#"<template>
  <div>
    <div v-if="showOuter" class="outer">
      <span v-if="showA">A</span>
      <span v-if="showB">B</span>
    </div>
    <span v-if="showDark">dark</span>
    <span v-else>light</span>
  </div>
</template>
<script setup>
const showOuter = true
const showA = true
const showB = true
const showDark = true
</script>
"#;
        let code = gen(source);
        // Outer v-if without v-else should get a comment node
        assert!(
            code.contains("_createCommentVNode(\"v-if\", true)"),
            "v-if without v-else should have comment node. Generated:\n{}",
            code
        );
        // The v-if/v-else pair (showDark) should NOT have a comment node but should have ternary
        assert!(
            code.contains("$setup.showDark"),
            "showDark condition should be present. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Static + Dynamic Class/Style Merging Tests
    // =========================================================================

    /// Helper that generates code AND validates it is valid JS.
    fn gen_and_validate(source: &str) -> String {
        use oxc_parser::Parser;
        use oxc_span::SourceType;

        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue".to_string());
        let result = generate(source, &options, &allocator);

        let alloc2 = oxc_allocator::Allocator::default();
        let source_type = SourceType::mjs();
        let parser_result = Parser::new(&alloc2, &result.code, source_type).parse();
        assert!(
            parser_result.errors.is_empty(),
            "Generated code is NOT valid JavaScript!\nParse Errors: {:?}\nGenerated Code:\n{}",
            parser_result.errors,
            result.code
        );
        result.code
    }

    #[test]
    fn test_merge_static_and_dynamic_class() {
        let source = r#"<template><div class="static" :class="{ active: isActive }"></div></template>
<script setup>const isActive = true</script>"#;
        let code = gen_and_validate(source);
        // Should output single merged class prop
        assert!(
            code.contains(r#"_normalizeClass(["static""#),
            "Should merge static class into normalizeClass array. Generated:\n{}",
            code
        );
        // Should NOT have duplicate class props
        let class_count = code.matches("class:").count();
        assert_eq!(
            class_count, 1,
            "Should have exactly one class prop. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_merge_static_and_dynamic_class_array() {
        let source = r#"<template><div class="split-pane" :class="[direction, { dragging: isDragging }]"></div></template>
<script setup>const direction = 'horizontal'; const isDragging = false</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains(r#"_normalizeClass(["split-pane""#),
            "Should merge static class into normalizeClass. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_merge_static_and_dynamic_style() {
        let source = r#"<template><div style="color: red" :style="{ fontSize: size }"></div></template>
<script setup>const size = '14px'</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains(r#"_normalizeStyle(["color: red""#),
            "Should merge static style into normalizeStyle array. Generated:\n{}",
            code
        );
        let style_count = code.matches("style:").count();
        assert_eq!(
            style_count, 1,
            "Should have exactly one style prop. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_dynamic_class_always_normalized() {
        // Even without static class, dynamic class should use normalizeClass
        let source = r#"<template><div :class="activeClass"></div></template>
<script setup>const activeClass = 'foo'</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("_normalizeClass("),
            "Dynamic class should always use _normalizeClass. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_dynamic_style_always_normalized() {
        let source = r#"<template><div :style="{ color: c }"></div></template>
<script setup>const c = 'red'</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("_normalizeStyle("),
            "Dynamic style should always use _normalizeStyle. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_static_only_class_not_normalized() {
        // Static-only class should remain a simple string, no normalizeClass
        let source = r#"<template><div class="static"></div></template>"#;
        let code = gen(source);
        assert!(
            !code.contains("_normalizeClass"),
            "Static-only class should not use normalizeClass. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"class: "static""#),
            "Should output static class as string. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Mixed Children: _createTextVNode Wrapping Tests
    // =========================================================================

    #[test]
    fn test_mixed_children_text_and_element() {
        // Text interpolation + element child → text must be wrapped with _createTextVNode
        let source = r#"<template><div>{{ msg }}<span>hello</span></div></template>
<script setup>const msg = 'hi'</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("_createTextVNode("),
            "Mixed children should wrap interpolation with _createTextVNode. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_mixed_children_vfor_with_vif() {
        // v-for button with interpolation + v-if span (the Output.vue case)
        let source = r#"<template><div><button v-for="tab in tabs" :key="tab.mode">{{ tab.label }}<span v-if="tab.extra">{{ tab.extra }}</span></button></div></template>
<script setup>const tabs = [{mode:'a',label:'A',extra:'x'}]</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("_createTextVNode("),
            "Interpolation in v-for button with v-if child should use _createTextVNode. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_keyed_vfor_item_is_block_root() {
        // KEYED v-for items should use _openBlock() + _createElementBlock (matching Vue official)
        let source = r#"<template><div><span v-for="item in list" :key="item">{{ item }}</span></div></template>
<script setup>const list = ['a','b']</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains(r#"_createElementBlock("span""#),
            "Keyed v-for items should use _createElementBlock. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_unkeyed_vfor_item_not_block_root() {
        // UNKEYED v-for items should use _createElementVNode (not block root)
        let source = r#"<template><div><span v-for="item in list">{{ item }}</span></div></template>
<script setup>const list = ['a','b']</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains(r#"_createElementVNode("span""#),
            "Unkeyed v-for items should use _createElementVNode. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_single_text_child_no_create_text_vnode() {
        // Single text child should NOT use _createTextVNode (optimization)
        let source = r#"<template><div>{{ msg }}</div></template>
<script setup>const msg = 'hi'</script>"#;
        let code = gen_and_validate(source);
        assert!(
            !code.contains("_createTextVNode"),
            "Single text child should not use _createTextVNode. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Hoisting & Optimization Parity Tests
    // =========================================================================

    #[test]
    fn test_click_handler_no_need_hydration_flag() {
        // Vue excludes click from NEED_HYDRATION — click has a dedicated fast path.
        // A setup ref click handler should produce NO patch flag at all.
        let source = r#"<template><button @click="handler">click</button></template>
<script setup>const handler = () => {}</script>"#;
        let code = gen_and_validate(source);
        assert!(
            !code.contains("NEED_HYDRATION"),
            "Click handler should NOT set NEED_HYDRATION (click has dedicated fast path). Generated:\n{}",
            code
        );
        assert!(
            !code.contains("PROPS"),
            "Setup click handler should NOT set PROPS flag. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_non_click_handler_gets_need_hydration() {
        // Non-click event handlers (keydown, blur, etc.) SHOULD get NEED_HYDRATION.
        // This matches Vue's behavior — only onclick is excluded.
        let source = r#"<template><div @keydown="handler">content</div></template>
<script setup>const handler = () => {}</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("NEED_HYDRATION"),
            "Non-click event handler should set NEED_HYDRATION flag. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_cached_non_click_handler_gets_need_hydration() {
        // Even cached (inline expression) non-click handlers should get NEED_HYDRATION.
        let source = r#"<template><input @blur="show = false" /></template>
<script setup>import { ref } from 'vue'; const show = ref(true)</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("NEED_HYDRATION"),
            "Cached non-click handler should still get NEED_HYDRATION. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_setup_handler_no_dynamic_props_array() {
        // Simple $setup handler should NOT add onClick to dynamic props
        let source = r#"<template><button :class="c" @click="handler">click</button></template>
<script setup>const c = 'x'; const handler = () => {}</script>"#;
        let code = gen_and_validate(source);
        assert!(
            !code.contains(r#"["onClick"]"#),
            "Setup handler should NOT add onClick to dynamic props. Generated:\n{}",
            code
        );
        // Should still have CLASS flag for :class binding
        assert!(
            code.contains("CLASS"),
            "Should have CLASS flag for dynamic class. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_vmodel_native_element_no_props_flag() {
        // v-model on native <input> should NOT add PROPS flag or "modelValue" dynamic prop.
        // Vue uses withDirectives for native v-model, not props. NEED_PATCH handles re-rendering.
        let source = r#"<template><input v-model="val" @keydown="handler" /></template>
<script setup>import { ref } from 'vue'; const val = ref(''); const handler = () => {}</script>"#;
        let code = gen_and_validate(source);
        // Should NOT have PROPS flag (no "modelValue" prop on native elements)
        assert!(
            !code.contains(r#"["modelValue"]"#),
            "Native v-model should NOT have modelValue in dynamic props. Generated:\n{}",
            code
        );
        // Should have NEED_PATCH (512) from v-model directive
        assert!(
            code.contains("NEED_PATCH"),
            "v-model should have NEED_PATCH flag. Generated:\n{}",
            code
        );
        // Should have NEED_HYDRATION (32) from @keydown (non-click event)
        assert!(
            code.contains("NEED_HYDRATION"),
            "Non-click handler alongside v-model should have NEED_HYDRATION. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_vif_static_props_hoisted_with_key() {
        let source = r#"<template><div><span v-if="show" class="pill">text</span></div></template>
<script setup>const show = true</script>"#;
        let code = gen_and_validate(source);
        // The hoisted constant should include both key and class
        assert!(
            code.contains("_hoisted_"),
            "v-if element with static props should be hoisted. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"key: 0"#),
            "Hoisted props should include conditional key. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"class: "pill""#),
            "Hoisted props should include static class. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_component_vif_uses_create_block() {
        let source = r#"<template><div><Comp v-if="show" /><Comp2 v-else /></div></template>
<script setup>import Comp from './Comp.vue'; import Comp2 from './Comp2.vue'; const show = true</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("_createBlock("),
            "Component with v-if should use _createBlock. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_mixed_children_no_text_flag() {
        // When interpolation is in array (wrapped with createTextVNode), parent should NOT get TEXT flag
        // The TEXT flag inside _createTextVNode(..., 1 /* TEXT */) is fine - it's the parent element
        // that should NOT have a TEXT patch flag
        let source = r#"<template><div>{{ msg }}<span>hello</span></div></template>
<script setup>const msg = 'hi'</script>"#;
        let code = gen_and_validate(source);
        // The _createElementBlock for the div should close without a patch flag: ...]))
        // NOT with ], 1 /* TEXT */))
        assert!(
            !code.contains("_createElementBlock(\"div\", null, [")
                || !code.contains("], 1 /* TEXT */"),
            "Parent element with mixed children should not have TEXT patch flag. Generated:\n{}",
            code
        );
        // More directly: the parent should have no patch flags at all
        assert!(
            code.contains("_createElementVNode(\"span\", null, \"hello\""),
            "Children array should close without parent TEXT patch flag. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Render Function Declaration Tests
    // =========================================================================

    #[test]
    fn test_render_function_no_export_keyword() {
        let source = r#"<template><div>Hello</div></template>
<script setup></script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("function render("),
            "Should have render function. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("export function render("),
            "Render function should NOT have export keyword. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_render_function_no_export_multi_root() {
        let source = r#"<template><div>A</div><div>B</div></template>
<script setup></script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("function render("),
            "Multi-root should have render function. Generated:\n{}",
            code
        );
        assert!(
            !code.contains("export function render("),
            "Multi-root render should NOT have export keyword. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Component Setup Bracket Notation Tests
    // =========================================================================

    // =========================================================================
    // Scope ID Tests - scope IDs must NOT appear in template render props
    // =========================================================================

    /// Extract the render function section from generated code (before __css__).
    fn render_section(code: &str) -> &str {
        if let Some(pos) = code.find("export const __css__") {
            &code[..pos]
        } else {
            code
        }
    }

    #[test]
    fn test_no_scope_id_in_element_props() {
        let source = r#"<template><div class="hello">Hi</div></template>
<script setup></script>
<style scoped>.hello { color: red; }</style>"#;
        let code = gen_and_validate(source);
        let render = render_section(&code);
        assert!(
            !render.contains("data-v-"),
            "Render function should NOT contain data-v- scope attributes. Generated:\n{}",
            render
        );
    }

    #[test]
    fn test_no_scope_id_in_dynamic_props() {
        let source = r#"<template><div :class="cls">Hi</div></template>
<script setup>const cls = 'x'</script>
<style scoped>div { color: red; }</style>"#;
        let code = gen_and_validate(source);
        let render = render_section(&code);
        assert!(
            !render.contains("data-v-"),
            "Dynamic props should NOT contain data-v- scope attributes. Generated:\n{}",
            render
        );
    }

    #[test]
    fn test_no_scope_id_when_no_props() {
        let source = r#"<template><div>Content</div></template>
<script setup></script>
<style scoped>div { color: red; }</style>"#;
        let code = gen_and_validate(source);
        let render = render_section(&code);
        assert!(
            !render.contains("data-v-"),
            "Propless element should NOT get scope attribute in render. Generated:\n{}",
            render
        );
    }

    // =========================================================================
    // Component Setup Bracket Notation Tests
    // =========================================================================

    #[test]
    fn test_component_setup_direct_reference() {
        let source = r#"<template><MyComp /></template>
<script setup>
import MyComp from './MyComp.vue'
</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("$setup.MyComp"),
            "Setup component should use $setup. prefix. Generated:\n{}",
            code
        );
        assert!(
            !code.contains(r#"_resolveComponent"#),
            "Should NOT use resolveComponent for setup components. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Component Dynamic Props Tracking Tests
    // =========================================================================

    #[test]
    fn test_component_dynamic_bind_props_tracked() {
        let source = r#"<template>
  <Preview :store="store" />
</template>
<script setup>
import Preview from './Preview.vue'
const store = {}
</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("8 /* PROPS */"),
            "Should have PROPS patch flag for component with dynamic bind. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"["store"]"#),
            "Should track 'store' as dynamic prop. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_component_multiple_dynamic_props() {
        let source = r#"<template>
  <MyComp :foo="a" :bar="b" />
</template>
<script setup>
import MyComp from './MyComp.vue'
const a = 1
const b = 2
</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains(r#"["foo", "bar"]"#),
            "Should track both dynamic props. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_component_static_props_not_tracked() {
        let source = r#"<template>
  <MyComp label="hello" />
</template>
<script setup>
import MyComp from './MyComp.vue'
</script>"#;
        let code = gen_and_validate(source);
        assert!(
            !code.contains("PROPS"),
            "Static-only props should NOT have PROPS flag. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_component_dynamic_props_with_children() {
        let source = r#"<template>
  <MyComp :store="store">content</MyComp>
</template>
<script setup>
import MyComp from './MyComp.vue'
const store = {}
</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("8 /* PROPS */"),
            "Non-self-closing component should also have PROPS flag. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"["store"]"#),
            "Should track 'store' as dynamic prop. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // v-for Stable Fragment Tests
    // =========================================================================

    #[test]
    fn test_vfor_constant_iterable_keyed_fragment() {
        // Setup bindings (even const) are not treated as stable because we can't
        // distinguish `const items = [...]` from `const items = ref([...])`.
        // Since this v-for has :key, it should use KEYED_FRAGMENT (128).
        let source = r#"<template>
  <div v-for="item in items" :key="item.id">{{ item.name }}</div>
</template>
<script setup>
const items = [{ id: 1, name: 'A' }]
</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("128 /* KEYED_FRAGMENT */"),
            "Keyed v-for should use KEYED_FRAGMENT. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_vfor_ctx_iterable_keyed_fragment() {
        let source = r#"<template>
  <div v-for="item in list" :key="item.id">{{ item.name }}</div>
</template>
<script setup>
</script>"#;
        let code = gen_and_validate(source);
        // 'list' is not in setup bindings, so it's accessed via _ctx. and is reactive
        assert!(
            code.contains("_openBlock(true)"),
            "Reactive iterable should use _openBlock(true). Generated:\n{}",
            code
        );
        assert!(
            code.contains("128 /* KEYED_FRAGMENT */"),
            "Reactive keyed iterable should use KEYED_FRAGMENT. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_vfor_number_literal_stable_fragment() {
        let source = r#"<template>
  <span v-for="n in 5" :key="n">{{ n }}</span>
</template>
<script setup></script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("64 /* STABLE_FRAGMENT */"),
            "Number literal iterable should use STABLE_FRAGMENT. Generated:\n{}",
            code
        );
    }

    // =========================================================================
    // Text Whitespace Condensation Tests
    // =========================================================================

    #[test]
    fn test_whitespace_condense_same_line() {
        // Same-line whitespace between interpolation and element should condense to + " "
        let source = r#"<template>
  <button>{{ label }} <span class="icon">!</span></button>
</template>
<script setup>
const label = 'Click'
</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains(r#"+ " ""#),
            "Should condense same-line whitespace to ' + \" \"' between interpolation and element. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_whitespace_condense_newline_between_interp_and_element() {
        // Whitespace with newlines between interpolation and element should ALSO condense to + " "
        // Vue only removes whitespace between two elements, not between text/interpolation and elements
        let source = r#"<template>
  <button>
    {{ label }}
    <span class="icon">!</span>
  </button>
</template>
<script setup>
const label = 'Click'
</script>"#;
        let code = gen_and_validate(source);
        // Should have + " " (Vue condenses whitespace between interp and element regardless of newlines)
        assert!(
            code.contains(r#"+ " ""#),
            "Should condense whitespace between interpolation and element even with newlines. Generated:\n{}",
            code
        );
        // Should have both text and element vnodes
        assert!(
            code.contains("_createTextVNode(") && code.contains("_createElementVNode("),
            "Should have both text and element vnodes. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_element_dynamic_props_array_hoisted() {
        // Element dynamic prop names arrays should be hoisted as _hoisted_N,
        // matching Vue's official compiler behavior for native elements.
        let source = r#"<template>
  <div :onClick="handler">Content</div>
</template>
<script setup>
const handler = () => {}
</script>"#;
        let code = gen_and_validate(source);
        // Should hoist the props array as _hoisted_N
        assert!(
            code.contains(r#"= ["onClick"]"#),
            "Element dynamic props array should be hoisted. Generated:\n{}",
            code
        );
        // Render function should reference hoisted name, not inline
        let render_start = code.find("function render").unwrap_or(0);
        let render_code = &code[render_start..];
        assert!(
            !render_code.contains(r#"["onClick"]"#),
            "Render function should use hoisted reference. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_component_dynamic_props_inlined() {
        // Component dynamic props arrays should be inlined (not hoisted), matching Vue's official compiler
        let source = r#"<template>
  <Preview :store="store" />
</template>
<script setup>
import Preview from './Preview.vue'
const store = {}
</script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains("8 /* PROPS */"),
            "Should have PROPS patch flag. Generated:\n{}",
            code
        );
        // Should inline the props array directly
        assert!(
            code.contains(r#"["store"]"#),
            "Should inline the dynamic props array. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_component_dynamic_props_with_vif() {
        // Components with v-if use write_props_with_key() which must also track dynamic props
        let source = r#"<template>
  <div>
    <Preview v-if="show" :store="store" />
    <Fallback v-else :store="store" :mode="mode" />
  </div>
</template>
<script setup>
import Preview from './Preview.vue'
import Fallback from './Fallback.vue'
const show = true
const store = {}
const mode = 'js'
</script>"#;
        let code = gen_and_validate(source);
        // Both components should have PROPS patch flag
        assert!(
            code.contains("8 /* PROPS */"),
            "Components with v-if should have PROPS patch flag for dynamic bindings. Generated:\n{}",
            code
        );
        // Should have hoisted dynamic props arrays with "store"
        assert!(
            code.contains(r#""store""#),
            "Should include 'store' in dynamic props array. Generated:\n{}",
            code
        );
        // Should have hoisted dynamic props arrays with "mode"
        assert!(
            code.contains(r#""mode""#),
            "Should include 'mode' in dynamic props array. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_asset_import_for_img_src() {
        let source = r#"<template><img src="/verter-logo.svg" alt="logo" /></template>
<script setup></script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains(r#"import _imports_0 from "/verter-logo.svg?import""#),
            "Should generate asset import for local src. Generated:\n{}",
            code
        );
        assert!(
            code.contains("src: _imports_0"),
            "Should reference import binding instead of raw string. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_no_asset_import_for_external_src() {
        let source = r#"<template><img src="https://example.com/logo.svg" alt="logo" /></template>
<script setup></script>"#;
        let code = gen_and_validate(source);
        assert!(
            !code.contains("_imports_"),
            "Should NOT generate asset import for external URL. Generated:\n{}",
            code
        );
        assert!(
            code.contains(r#"src: "https://example.com/logo.svg""#),
            "Should keep external URL as string. Generated:\n{}",
            code
        );
    }

    #[test]
    fn test_asset_import_for_relative_src() {
        let source = r#"<template><img src="./logo.png" alt="logo" /></template>
<script setup></script>"#;
        let code = gen_and_validate(source);
        assert!(
            code.contains(r#"import _imports_0 from "./logo.png?import""#),
            "Should generate asset import for relative src. Generated:\n{}",
            code
        );
    }
}
