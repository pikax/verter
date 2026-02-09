//! Code generation for template interpolations.
//!
//! Transforms `{{ expression }}` into `_toDisplayString(_ctx.expression)`.
//! Uses provided_bindings from scope events to determine if _ctx. prefix is needed.

use crate::code_transform::CodeTransform;
use crate::syntax::types::AnalysedOxcInterpolation;

use super::types::{
    resolve_binding_prefix, resolve_binding_suffix, BindingMetadata, HelperFlags,
    TemplateCodegenState,
};

/// Process an analysed interpolation with scope information.
/// Uses provided_bindings to check if identifiers are local (from v-for/v-slot).
pub fn process_analysed_interpolation<'a>(
    analysed: &AnalysedOxcInterpolation<'a>,
    state: &mut TemplateCodegenState,
    code_transform: &mut CodeTransform<'a>,
    source: &'a str,
) {
    let content_start = analysed.event.event.content_start as usize;
    let content_end = analysed.event.event.content_end as usize;

    if content_start >= content_end || content_end > source.len() {
        return;
    }

    let expression = source[content_start..content_end].trim();

    if expression.is_empty() {
        // Empty interpolation, remove it entirely
        code_transform.remove(analysed.event.start, analysed.event.end);
        return;
    }

    // Collect local bindings for context prefixing
    // Include both: 1) bindings from scope events, 2) v-for locals from state stack
    let mut local_vars = collect_local_bindings(analysed, source);

    // Add v-for locals from the accumulated stack (from nested v-for loops)
    for locals in &state.vfor_locals_stack {
        for local in locals {
            if !local_vars.contains(&local.as_str()) {
                local_vars.push(local.as_str());
            }
        }
    }

    // Add v-slot locals from the accumulated stack (from nested scoped slots)
    for locals in &state.vslot_locals_stack {
        for local in locals {
            if !local_vars.contains(&local.as_str()) {
                local_vars.push(local.as_str());
            }
        }
    }

    // Build the replacement code
    let mut code = String::with_capacity(expression.len() + 40);

    // Check if we need to concatenate with previous text/interpolation
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

    code.push_str("_toDisplayString(");

    // Transform the expression by adding the correct accessor prefix
    transform_expr_with_ctx(
        &mut code,
        expression,
        &local_vars,
        &state.binding_metadata,
        source.as_bytes(),
        state.is_inline_mode,
    );

    code.push(')');

    // Handle v-slot direct child interpolation (text+interpolation concatenation).
    // When interpolation is a direct child of a v-slot template, it needs to be combined
    // with adjacent text into a single _createTextVNode() call.
    let is_vslot_interp = state.active_vslot_depth > 0
        && state
            .component_stack
            .last()
            .map(|p| {
                state.element_id_stack.len() == p.element_id_stack_len_at_open
                    && p.vslot_depth_at_open == 0
            })
            .unwrap_or(false);

    if is_vslot_interp {
        if state.vslot_text_vnode_open {
            // Concatenate with existing text vnode: prepend " + "
            code.insert_str(0, " + ");
        } else {
            // Start new text vnode for interpolation-only
            // Handle comma for sibling slot children
            let depth_idx = if state.depth > 0 { state.depth - 1 } else { 0 };
            while state.first_child_at_depth.len() <= depth_idx {
                state.first_child_at_depth.push(false);
            }
            let mut prefix = String::new();
            if state.first_child_at_depth[depth_idx] {
                prefix.push_str(", ");
            } else {
                state.first_child_at_depth[depth_idx] = true;
            }
            prefix.push_str("_createTextVNode(");
            code.insert_str(0, &prefix);
            state.helpers.insert(HelperFlags::CREATE_TEXT_VNODE);
            state.vslot_text_vnode_open = true;
        }

        state.vslot_text_vnode_has_interp = true;
        state.vslot_text_vnode_last_end = analysed.event.end;

        code_transform.overwrite(analysed.event.start, analysed.event.end, &code);
        state.helpers.insert(HelperFlags::TO_DISPLAY_STRING);
        return; // Early return - skip normal component/element handling
    }

    // Check if parent is a component that needs slot wrapper for interpolation children.
    // Component slots require: { default: _withCtx(() => [_createTextVNode(...), _: 1 }) }
    // Pre-compute text flag before mutable borrow of component_stack
    let text_flag = state.pflag(1, "TEXT");
    let mut component_slot_prefix = String::new();
    let is_direct_component_slot_child;
    if let Some(parent) = state.component_stack.last_mut() {
        let is_direct = state.element_id_stack.len() == parent.element_id_stack_len_at_open;
        is_direct_component_slot_child = is_direct;

        // Components opened inside a v-slot (vslot_depth_at_open > 0) should handle
        // their own children normally. Only skip when we're in a v-slot but the component
        // was opened outside it (which shouldn't normally happen).
        let should_handle_children = state.active_vslot_depth == parent.vslot_depth_at_open;
        if is_direct && should_handle_children {
            if parent.has_named_slots && !parent.default_slot_opened {
                // Named slots already opened the slots object. Add default slot inline.
                component_slot_prefix = ", default: _withCtx(() => [".to_string();
                parent.uses_slots = true;
                state.helpers.insert(HelperFlags::WITH_CTX);
                parent.default_slot_opened = true;
            } else if !parent.children_opened {
                // First child of component: open slot wrapper
                code_transform.prepend_left(parent.insert_pos, "{ default: _withCtx(() => [");
                parent.uses_slots = true;
                state.helpers.insert(HelperFlags::WITH_CTX);
                parent.children_opened = true;
                parent.default_slot_opened = true;
            }

            // Wrap interpolation in _createTextVNode for component slots
            if parent.default_slot_child_count > 0 {
                code = format!(", _createTextVNode({}, {})", code, text_flag);
            } else {
                code = format!("_createTextVNode({}, {})", code, text_flag);
            }
            state.helpers.insert(HelperFlags::CREATE_TEXT_VNODE);
            parent.default_slot_child_count += 1;
        }
    } else {
        is_direct_component_slot_child = false;
    }

    // Replace the entire interpolation
    if !component_slot_prefix.is_empty() {
        // For named slot default: prepend the slot opener before the interpolation
        let full_code = format!("{}{}", component_slot_prefix, code);
        code_transform.overwrite(analysed.event.start, analysed.event.end, &full_code);
    } else {
        code_transform.overwrite(analysed.event.start, analysed.event.end, &code);
    }

    state.helpers.insert(HelperFlags::TO_DISPLAY_STRING);

    // Mark the parent element as having interpolation (for TEXT patch flag)
    // Also track child count for single child optimization
    // Skip element-level tracking for direct component slot children (handled above)
    if is_direct_component_slot_child {
        // Component slot interpolation is already handled above
    } else if let Some(&parent_id) = state.element_id_stack.last() {
        // Only mark for TEXT flag if NOT in array mode.
        // In array mode, the interpolation becomes a _createTextVNode VNode child,
        // not direct text content of the element, so TEXT flag is inappropriate.
        let array_opened_for_text = state
            .element_array_opened
            .get(&parent_id)
            .copied()
            .unwrap_or(false);
        if !array_opened_for_text {
            state.elements_with_interpolation.insert(parent_id);
        }

        // Track child count for single child optimization
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
                    is_interpolation: true,
                    start: analysed.event.start,
                    end: analysed.event.end,
                },
            );
        } else if array_opened {
            // Array already opened (element children exist) - wrap in createTextVNode
            state.element_single_child.remove(&parent_id);
            let text_flag = state.pflag(1, "TEXT");
            let wrapped = format!(", _createTextVNode({}, {})", code, text_flag);
            code_transform.overwrite(analysed.event.start, analysed.event.end, &wrapped);
            state.helpers.insert(HelperFlags::CREATE_TEXT_VNODE);
        } else {
            // Multiple text/interpolation children, no element children yet.
            // Extend single_child range so if an element arrives later,
            // we can retroactively wrap all text in _createTextVNode and open array.
            if let Some(sc) = state.element_single_child.get_mut(&parent_id) {
                sc.content.push_str(&code);
                sc.end = analysed.event.end;
                sc.is_interpolation = true; // Contains interpolation
            }
        }
    }
}

/// Collect local bindings from scope events into a Vec<&str>
fn collect_local_bindings<'a>(
    analysed: &AnalysedOxcInterpolation<'a>,
    source: &'a str,
) -> Vec<&'a str> {
    let mut locals = Vec::new();
    if let Some(ref scope_data) = analysed.interpolation {
        if let Some(ref provided_bindings) = scope_data.provided_bindings {
            for binding in provided_bindings {
                for span in &binding.spans {
                    let binding_name = &source[span.start as usize..span.end as usize];
                    if !locals.contains(&binding_name) {
                        locals.push(binding_name);
                    }
                }
            }
        }
    }
    locals
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

/// Check if an identifier is a local binding using provided_bindings from scope events.
#[allow(dead_code)]
fn check_is_local_binding(
    identifier: &str,
    analysed: &AnalysedOxcInterpolation,
    source: &str,
) -> bool {
    // Get provided_bindings from the scope event data
    if let Some(ref scope_data) = analysed.interpolation {
        if let Some(ref provided_bindings) = scope_data.provided_bindings {
            for binding in provided_bindings {
                for span in &binding.spans {
                    let binding_name = &source[span.start as usize..span.end as usize];
                    if binding_name == identifier {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Check if an interpolation is a simple identifier (for optimization).
pub fn is_simple_identifier(expression: &str) -> bool {
    let trimmed = expression.trim();
    !trimmed.is_empty()
        && trimmed
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        && !trimmed.chars().next().unwrap_or('0').is_numeric()
}

/// Check if an interpolation is a member expression (for optimization).
pub fn is_member_expression(expression: &str) -> bool {
    let trimmed = expression.trim();
    // Simple check for member access patterns like `foo.bar` or `foo?.bar`
    trimmed.contains('.') && !trimmed.contains('(') && !trimmed.contains('[')
}

/// Extract the first identifier from an expression.
/// e.g., "item" -> "item", "item.name" -> "item", "foo.bar.baz" -> "foo"
#[allow(dead_code)]
fn extract_first_identifier(expr: &str) -> &str {
    let trimmed = expr.trim();
    // Find end of first identifier (alphanumeric, underscore, or dollar sign)
    let end = trimmed
        .char_indices()
        .find(|(i, c)| *i > 0 && !c.is_alphanumeric() && *c != '_' && *c != '$')
        .map(|(i, _)| i)
        .unwrap_or(trimmed.len());
    &trimmed[..end]
}

/// Extract a prefix operator from the start of an expression.
/// Returns (prefix, rest) where prefix includes all prefix operators and rest is the remaining expression.
/// Handles: !, !!, ~, +, -, typeof, void, delete, await
/// e.g., "!isLoading" -> ("!", "isLoading"), "!!value" -> ("!!", "value")
#[allow(dead_code)]
fn extract_prefix_operator(expr: &str) -> (&str, &str) {
    let trimmed = expr.trim();
    let bytes = trimmed.as_bytes();
    let mut pos = 0;

    // Collect all consecutive prefix operators
    while pos < bytes.len() {
        let remaining = &trimmed[pos..];

        // Check for single-character prefix operators
        if let Some(first_char) = remaining.chars().next() {
            match first_char {
                '!' | '~' => {
                    pos += 1;
                    continue;
                }
                '+' | '-' => {
                    // Only treat as prefix if followed by identifier/expression, not another same operator
                    // (to avoid confusing with ++ or --)
                    let next = remaining.chars().nth(1);
                    if next.is_some_and(|c| c != first_char) {
                        pos += 1;
                        continue;
                    }
                }
                _ => {}
            }
        }

        // Check for keyword prefix operators
        for keyword in ["typeof ", "void ", "delete ", "await "] {
            if remaining.starts_with(keyword) {
                pos += keyword.len();
                continue;
            }
        }

        // No more prefix operators found
        break;
    }

    if pos > 0 {
        (&trimmed[..pos], trimmed[pos..].trim_start())
    } else {
        ("", trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_simple_identifier() {
        assert!(is_simple_identifier("foo"));
        assert!(is_simple_identifier("_bar"));
        assert!(is_simple_identifier("$baz"));
        assert!(is_simple_identifier("foo123"));
        assert!(!is_simple_identifier("123foo"));
        assert!(!is_simple_identifier("foo.bar"));
        assert!(!is_simple_identifier("foo()"));
        assert!(!is_simple_identifier(""));
    }

    #[test]
    fn test_is_member_expression() {
        assert!(is_member_expression("foo.bar"));
        assert!(is_member_expression("foo.bar.baz"));
        assert!(is_member_expression("foo?.bar"));
        assert!(!is_member_expression("foo"));
        assert!(!is_member_expression("foo()"));
        assert!(!is_member_expression("foo[0]"));
    }
}
