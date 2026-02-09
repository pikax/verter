//! Code generation for Vue directives.
//!
//! Streaming approach - directives are processed before OpenTagEnd and stored on current_element.
//! The actual code generation happens in element.rs when OpenTagEnd fires.

use crate::code_transform::CodeTransform;
use crate::common::Span;
use crate::syntax::types::{
    AnalysedOxcProp, AnalysedStartScopeVConditional, AnalysedVFor, AnalysedVSlot,
};

use super::types::{
    CloseAction, HelperFlags, PropEntry, PropKind, TemplateCodegenState, VForInfo, VIfInfo,
    VSlotInfo,
};

/// Process an analysed v-if with scope information.
/// Stores v-if info on current element for processing at OpenTagEnd.
pub fn process_analysed_v_if<'a>(
    analysed: &AnalysedStartScopeVConditional<'a>,
    state: &mut TemplateCodegenState,
    code_transform: &mut CodeTransform<'a>,
    _source: &'a str,
) {
    // Store v-if info on current element
    if let Some(ref mut elem) = state.current_element {
        // Get the expression value span from the SyntaxProp
        let expression = analysed
            .event
            .event
            .value
            .as_ref()
            .map(|v| Span::new(v.start, v.end));

        // Check if there are sibling conditions
        let has_siblings = analysed
            .scope
            .condition
            .as_ref()
            .map(|c| !c.siblings.is_empty())
            .unwrap_or(false);

        elem.v_if = Some(VIfInfo {
            condition_type: analysed.event.condition_type,
            expression,
            has_siblings,
            scope_id: analysed.scope.id,
        });
    }

    // Remove the v-if directive from source
    code_transform.remove(analysed.event.event.start, analysed.event.event.end);

    state.helpers.insert(HelperFlags::OPEN_BLOCK);
    state.helpers.insert(HelperFlags::CREATE_ELEMENT_BLOCK);
}

/// Process an analysed v-for with scope information.
/// Stores v-for info on current element for processing at OpenTagEnd.
pub fn process_analysed_v_for<'a>(
    analysed: &AnalysedVFor<'a>,
    state: &mut TemplateCodegenState,
    code_transform: &mut CodeTransform<'a>,
    source: &'a str,
) {
    // Extract iterable and iterator spans from the parsed v-for
    let (iterable, iterator) = extract_vfor_spans(&analysed.event, source);

    if let Some(ref mut elem) = state.current_element {
        elem.v_for = Some(VForInfo {
            iterable,
            iterator,
            scope_id: analysed.scope.id,
        });
        elem.scope_id = Some(analysed.scope.id);
    }

    // Remove the v-for directive from source
    code_transform.remove(analysed.event.event.start, analysed.event.event.end);

    state.helpers.insert(HelperFlags::OPEN_BLOCK);
    state.helpers.insert(HelperFlags::CREATE_ELEMENT_BLOCK);
    state.helpers.insert(HelperFlags::FRAGMENT);
    state.helpers.insert(HelperFlags::RENDER_LIST);
}

/// Extract iterable and iterator spans from v-for directive.
fn extract_vfor_spans(v_for: &crate::syntax::types::OxcVForProp, source: &str) -> (Span, Span) {
    // The v-for expression is like "item in items" or "(item, index) in items"
    // We need to find the spans for both sides

    // Get the value span from the event
    let value_span = v_for
        .event
        .value
        .as_ref()
        .map(|v| Span::new(v.start, v.end))
        .unwrap_or(Span::new(0, 0));

    let value_str = &source[value_span.start as usize..value_span.end as usize];

    // Find " in " or " of " to split
    let separator_pos = value_str
        .find(" in ")
        .map(|p| (p, 4))
        .or_else(|| value_str.find(" of ").map(|p| (p, 4)));

    if let Some((pos, sep_len)) = separator_pos {
        let iterator_end = value_span.start + pos as u32;
        let iterable_start = value_span.start + (pos + sep_len) as u32;

        (
            Span::new(iterable_start, value_span.end),
            Span::new(value_span.start, iterator_end),
        )
    } else {
        // Fallback - use locals and references from the parsed v-for
        let locals = v_for.locals();
        let refs = v_for.references();

        let iterator = if !locals.is_empty() {
            locals[0]
        } else {
            value_span
        };

        let iterable = if !refs.is_empty() {
            refs[0]
        } else {
            value_span
        };

        (iterable, iterator)
    }
}

/// Process an analysed v-slot with scope information.
pub fn process_analysed_v_slot<'a>(
    analysed: &AnalysedVSlot<'a>,
    state: &mut TemplateCodegenState,
    code_transform: &mut CodeTransform<'a>,
    _source: &'a str,
) {
    // Store v-slot info on current element
    if let Some(ref mut elem) = state.current_element {
        // Extract slot name from directive arg (e.g., #header → "header")
        let slot_name = analysed
            .event
            .event
            .arg
            .as_ref()
            .map(|a| Span::new(a.start, a.end));

        // Extract params from value (e.g., "{ item }" → params span)
        let params = analysed
            .event
            .event
            .value
            .as_ref()
            .map(|v| Span::new(v.start, v.end));

        elem.v_slot = Some(VSlotInfo {
            name: slot_name,
            params,
            scope_id: analysed.scope.id,
        });

        elem.scope_id = Some(analysed.scope.id);

        // Register close action
        state
            .scope_close_actions
            .insert(analysed.scope.id, CloseAction::VSlot);
    }

    // Remove the v-slot directive from source
    code_transform.remove(analysed.event.event.start, analysed.event.event.end);

    state.helpers.insert(HelperFlags::WITH_CTX);
}

/// Process an analysed prop (v-bind, v-on, static, v-model).
/// Accumulates props on current element for processing at OpenTagEnd.
pub fn process_analysed_prop<'a>(
    analysed: &AnalysedOxcProp<'a>,
    state: &mut TemplateCodegenState,
    code_transform: &mut CodeTransform<'a>,
    source: &'a str,
) {
    let prop = &analysed.event;
    let event = &prop.event;

    // Determine prop kind based on the directive name
    let prop_name_start = event.start as usize;
    let prop_name_end = event.name_end as usize;
    let prop_name = &source[prop_name_start..prop_name_end];

    // Handle v-show directive - converts to style binding
    if prop_name == "v-show" {
        let value = prop.exp.as_ref().map(|e| Span::new(e.start, e.end));
        if let Some(ref mut elem) = state.current_element {
            elem.props.push(PropEntry {
                kind: PropKind::Show,
                name: Span::new(0, 0), // Will use "style" in codegen
                value,
                modifiers: vec![],
                is_dynamic_arg: false,
            });
        }
        // Remove the v-show directive from source
        code_transform.remove(event.start, event.end);
        return;
    }

    // Handle v-html directive - converts to innerHTML binding
    if prop_name == "v-html" {
        let value = prop.exp.as_ref().map(|e| Span::new(e.start, e.end));
        if let Some(ref mut elem) = state.current_element {
            elem.props.push(PropEntry {
                kind: PropKind::Html,
                name: Span::new(0, 0), // Will use "innerHTML" in codegen
                value,
                modifiers: vec![],
                is_dynamic_arg: false,
            });
        }
        // Remove the v-html directive from source
        code_transform.remove(event.start, event.end);
        return;
    }

    // Handle v-text directive - converts to textContent binding
    if prop_name == "v-text" {
        let value = prop.exp.as_ref().map(|e| Span::new(e.start, e.end));
        if let Some(ref mut elem) = state.current_element {
            elem.props.push(PropEntry {
                kind: PropKind::Text,
                name: Span::new(0, 0), // Will use "textContent" in codegen
                value,
                modifiers: vec![],
                is_dynamic_arg: false,
            });
        }
        // Remove the v-text directive from source
        code_transform.remove(event.start, event.end);
        return;
    }

    let (kind, name_span, value_span, is_dynamic_arg) =
        if prop_name.starts_with(':') || prop_name.starts_with("v-bind") {
            // v-bind - use event.arg for static arg names, prop.arg for dynamic expressions
            let value = prop.exp.as_ref().map(|e| Span::new(e.start, e.end));

            // Check if this is a v-bind spread (no argument name)
            if event.arg.is_none() {
                // v-bind="obj" spread syntax - spreads all properties
                (PropKind::BindSpread, Span::new(0, 0), value, false)
            } else {
                let name = event
                    .arg
                    .as_ref()
                    .map(|a| Span::new(a.start, a.end))
                    .unwrap_or(Span::new(0, 0));
                let is_dynamic = event.arg.as_ref().map(|a| a.is_dynamic).unwrap_or(false);

                // Check for :key specifically
                if let Some(ref arg) = event.arg {
                    let arg_str = &source[arg.start as usize..arg.end as usize];
                    if arg_str == "key" {
                        if let Some(ref mut elem) = state.current_element {
                            elem.has_key = true;
                        }
                    }
                }

                (PropKind::Bind, name, value, is_dynamic)
            }
        } else if prop_name == "v-once" {
            // v-once directive - mark element for caching
            if let Some(ref mut elem) = state.current_element {
                elem.v_once = true;
            }
            code_transform.remove(event.start, event.end);
            return;
        } else if prop_name.starts_with('@') || prop_name.starts_with("v-on") {
            // v-on - use event.arg for static arg names
            let name = event
                .arg
                .as_ref()
                .map(|a| Span::new(a.start, a.end))
                .unwrap_or(Span::new(0, 0));
            let value = prop.exp.as_ref().map(|e| Span::new(e.start, e.end));
            let is_dynamic = event.arg.as_ref().map(|a| a.is_dynamic).unwrap_or(false);
            (PropKind::On, name, value, is_dynamic)
        } else if prop_name.starts_with("v-model") {
            // v-model - store info for directive-based handling
            let value = prop.exp.as_ref().map(|e| Span::new(e.start, e.end));
            let modifiers: Vec<Span> = event
                .modifiers
                .as_ref()
                .map(|m| m.to_vec())
                .unwrap_or_default();

            // Store v-model info on element for _withDirectives wrapping
            if let Some(ref mut elem) = state.current_element {
                elem.v_model = Some(super::types::VModelInfo { value, modifiers });
            }

            // Also add as prop for the onUpdate:modelValue handler generation
            let name = event
                .arg
                .as_ref()
                .map(|a| Span::new(a.start, a.end))
                .unwrap_or(Span::new(event.start, event.name_end)); // default to "modelValue"
            (PropKind::Model, name, value, false)
        } else if prop_name.starts_with("v-") {
            // Custom directive (v-focus, v-tooltip, etc.)
            // Extract directive name without "v-" prefix
            let directive_name = prop_name
                .strip_prefix("v-")
                .unwrap_or(prop_name)
                .to_string();

            // Get argument, value, and modifiers
            let arg = event.arg.as_ref().map(|a| Span::new(a.start, a.end));
            let is_dynamic_arg = event.arg.as_ref().map(|a| a.is_dynamic).unwrap_or(false);
            let value = prop.exp.as_ref().map(|e| Span::new(e.start, e.end));
            let modifiers: Vec<Span> = event
                .modifiers
                .as_ref()
                .map(|m| m.to_vec())
                .unwrap_or_default();

            // Store in current element's custom_directives
            if let Some(ref mut elem) = state.current_element {
                elem.custom_directives
                    .push(super::types::CustomDirectiveEntry {
                        name: directive_name,
                        value,
                        arg,
                        is_dynamic_arg,
                        modifiers,
                    });
            }

            // Remove the directive from source
            code_transform.remove(event.start, event.end);
            return;
        } else {
            // Static prop
            let name = Span::new(event.start, event.name_end);
            let value = event.value.as_ref().map(|v| Span::new(v.start, v.end));
            (PropKind::Static, name, value, false)
        };

    // Get modifiers
    let modifiers = event
        .modifiers
        .as_ref()
        .map(|m| m.to_vec())
        .unwrap_or_default();

    // Add to current element's props
    if let Some(ref mut elem) = state.current_element {
        elem.props.push(PropEntry {
            kind,
            name: name_span,
            value: value_span,
            modifiers,
            is_dynamic_arg,
        });
    }
}

/// Check if a handler expression is an inline statement vs a method reference.
#[allow(dead_code)]
fn is_inline_handler(handler: &str) -> bool {
    let trimmed = handler.trim();
    // Inline if it contains operators, function calls, or assignments
    trimmed.contains('=')
        || trimmed.contains('(')
        || trimmed.contains('+')
        || trimmed.contains('-')
        || trimmed.contains('!')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_inline_handler() {
        assert!(!is_inline_handler("handleClick"));
        assert!(!is_inline_handler("foo.bar"));
        assert!(is_inline_handler("count++"));
        assert!(is_inline_handler("handleClick()"));
        assert!(is_inline_handler("x = 1"));
        assert!(is_inline_handler("!isActive"));
    }
}
