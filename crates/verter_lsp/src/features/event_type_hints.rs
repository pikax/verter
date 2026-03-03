// Event handler type hint code actions.
//
// For native DOM event handlers with simple handler bindings (e.g., @click="handler"),
// generates code actions to wrap with a typed parameter:
//   @click="handler" → @click="(e: MouseEvent) => handler(e)"

use tower_lsp_server::ls_types::*;
use verter_analysis::template::TemplateEventHandler;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::features::action_utils;

/// Map native DOM event names to their TypeScript event types.
fn event_type(event_name: &str) -> Option<&'static str> {
    Some(match event_name {
        // Mouse events
        "click" | "dblclick" | "mousedown" | "mouseup" | "mousemove" | "mouseenter"
        | "mouseleave" | "mouseover" | "mouseout" | "contextmenu" => "MouseEvent",

        // Pointer events
        "pointerdown" | "pointerup" | "pointermove" | "pointerenter" | "pointerleave"
        | "pointerover" | "pointerout" | "pointercancel" | "gotpointercapture"
        | "lostpointercapture" => "PointerEvent",

        // Keyboard events
        "keydown" | "keyup" | "keypress" => "KeyboardEvent",

        // Focus events
        "focus" | "blur" | "focusin" | "focusout" => "FocusEvent",

        // Input events
        "input" | "beforeinput" => "InputEvent",

        // Form events
        "change" | "submit" | "reset" | "invalid" => "Event",

        // Touch events
        "touchstart" | "touchend" | "touchmove" | "touchcancel" => "TouchEvent",

        // Drag events
        "drag" | "dragstart" | "dragend" | "dragenter" | "dragleave" | "dragover" | "drop" => {
            "DragEvent"
        }

        // Wheel event
        "wheel" => "WheelEvent",

        // Scroll event
        "scroll" | "scrollend" => "Event",

        // Animation events
        "animationstart" | "animationend" | "animationiteration" => "AnimationEvent",

        // Transition events
        "transitionstart" | "transitionend" | "transitionrun" | "transitioncancel" => {
            "TransitionEvent"
        }

        // Clipboard events
        "copy" | "cut" | "paste" => "ClipboardEvent",

        // Composition events
        "compositionstart" | "compositionupdate" | "compositionend" => "CompositionEvent",

        _ => return None,
    })
}

/// Check if a tag name represents a native HTML element (not a component).
///
/// Vue convention: native elements are lowercase, components are PascalCase.
fn is_native_element(tag: &str) -> bool {
    !tag.is_empty() && tag.chars().next().unwrap().is_ascii_lowercase()
}

/// Generate event handler type hint code actions.
///
/// For each native DOM event handler with a simple binding reference,
/// suggests wrapping with a typed event parameter.
pub fn event_type_hint_actions(
    analysis: &FileAnalysisSnapshot,
    source: &str,
    line_index: &LineIndex,
) -> Vec<CodeActionOrCommand> {
    let template = match &analysis.template {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut actions = Vec::new();

    for handler in &template.event_handlers {
        if let Some(action) = make_event_type_action(handler, source, line_index) {
            actions.push(action);
        }
    }

    actions
}

/// Try to generate a type hint action for a single event handler.
fn make_event_type_action(
    handler: &TemplateEventHandler,
    source: &str,
    line_index: &LineIndex,
) -> Option<CodeActionOrCommand> {
    // Skip inline handlers (@click="count++")
    if handler.is_inline {
        return None;
    }

    // Skip if no handler binding name
    let binding_name = handler.handler_binding.as_deref()?;

    // Skip component events — only handle native elements
    if !is_native_element(&handler.target_tag) {
        return None;
    }

    // Look up the DOM event type
    let dom_type = event_type(&handler.event_name)?;

    // The handler attribute value is the binding name.
    // We want to replace it with a typed wrapper: "(e: MouseEvent) => handler(e)"
    // The spans cover the entire attribute (e.g., `@click="handler"`).
    // We need to find just the value portion to replace.
    // Value is between quotes: find the quote positions in the span.
    let span_text = source.get(handler.span.start as usize..handler.span.end as usize)?;

    // Find the value inside quotes
    let quote_start = span_text.find('"').or_else(|| span_text.find('\''))?;
    let quote_char = span_text.as_bytes()[quote_start];
    let quote_end = span_text[quote_start + 1..].find(quote_char as char)?;
    let value_start = handler.span.start + quote_start as u32 + 1;
    let value_end = handler.span.start + quote_start as u32 + 1 + quote_end as u32;

    let start = line_index.offset_to_position(value_start)?;
    let end = line_index.offset_to_position(value_end)?;

    let new_text = format!("(e: {dom_type}) => {binding_name}(e)");
    let title = format!(
        "Add {dom_type} type annotation to @{event_name}",
        event_name = handler.event_name
    );

    let edit = action_utils::make_replace_edit(
        // Use placeholder URI; server will fix it
        &action_utils::SAME_FILE_URI.parse().unwrap(),
        Range { start, end },
        new_text,
    );

    Some(action_utils::make_code_action(
        title,
        CodeActionKind::QUICKFIX,
        edit,
        false,
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_analysis::template::TemplateAnalysisSnapshot;

    fn make_analysis(handlers: Vec<TemplateEventHandler>) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            template: Some(TemplateAnalysisSnapshot {
                event_handlers: handlers,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn make_handler(
        event_name: &str,
        handler_binding: Option<&str>,
        is_inline: bool,
        target_tag: &str,
        span_start: u32,
        span_end: u32,
    ) -> TemplateEventHandler {
        TemplateEventHandler {
            event_name: event_name.to_string(),
            handler_binding: handler_binding.map(|s| s.to_string()),
            is_inline,
            target_tag: target_tag.to_string(),
            span: verter_span::Span::new(span_start, span_end),
        }
    }

    // -- event_type mapping tests --

    #[test]
    fn click_maps_to_mouse_event() {
        assert_eq!(event_type("click"), Some("MouseEvent"));
        // Negative: not KeyboardEvent
        assert_ne!(event_type("click"), Some("KeyboardEvent"));
    }

    #[test]
    fn keydown_maps_to_keyboard_event() {
        assert_eq!(event_type("keydown"), Some("KeyboardEvent"));
    }

    #[test]
    fn input_maps_to_input_event() {
        assert_eq!(event_type("input"), Some("InputEvent"));
    }

    #[test]
    fn unknown_event_returns_none() {
        assert_eq!(event_type("custom-event"), None);
        assert_eq!(event_type("save"), None);
    }

    // -- is_native_element tests --

    #[test]
    fn lowercase_tag_is_native() {
        assert!(is_native_element("div"));
        assert!(is_native_element("button"));
        assert!(is_native_element("input"));
    }

    #[test]
    fn pascal_case_tag_is_component() {
        assert!(!is_native_element("MyComponent"));
        assert!(!is_native_element("Child"));
        // Negative: components should NOT be native
        assert!(!is_native_element("Button")); // PascalCase Button is a component
    }

    // -- code action generation tests --

    #[test]
    fn click_handler_suggests_mouse_event() {
        // @click="handleClick" on <div>
        let source = "<template><div @click=\"handleClick\"></div></template>";
        // offset 15: @, 21: =, 22: ", 23-33: handleClick, 34: "
        // @click="handleClick" spans offset 15..35
        let analysis = make_analysis(vec![make_handler(
            "click",
            Some("handleClick"),
            false,
            "div",
            15,
            35,
        )]);
        let line_index = LineIndex::new_utf16(source);

        let actions = event_type_hint_actions(&analysis, source, &line_index);

        // Positive: generates action mentioning MouseEvent
        assert_eq!(actions.len(), 1, "should generate 1 action");
        if let CodeActionOrCommand::CodeAction(action) = &actions[0] {
            assert!(
                action.title.contains("MouseEvent"),
                "title should mention MouseEvent"
            );
            assert!(
                action.title.contains("@click"),
                "title should mention event name"
            );
            // Positive: edit replaces handler with typed wrapper
            let edit = action.edit.as_ref().unwrap();
            if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
                if let OneOf::Left(text_edit) = &edits[0].edits[0] {
                    assert!(
                        text_edit.new_text.contains("MouseEvent"),
                        "edit should contain MouseEvent"
                    );
                    assert!(
                        text_edit.new_text.contains("handleClick"),
                        "edit should preserve handler name"
                    );
                    // Negative: should NOT contain KeyboardEvent
                    assert!(!text_edit.new_text.contains("KeyboardEvent"));
                }
            }
        }
    }

    #[test]
    fn keydown_handler_suggests_keyboard_event() {
        let source = "<template><input @keydown=\"onKey\" /></template>";
        // @keydown="onKey" at offset 17..33
        let analysis = make_analysis(vec![make_handler(
            "keydown",
            Some("onKey"),
            false,
            "input",
            17,
            33,
        )]);
        let line_index = LineIndex::new_utf16(source);

        let actions = event_type_hint_actions(&analysis, source, &line_index);

        assert_eq!(actions.len(), 1);
        if let CodeActionOrCommand::CodeAction(action) = &actions[0] {
            assert!(
                action.title.contains("KeyboardEvent"),
                "title should mention KeyboardEvent"
            );
        }
    }

    #[test]
    fn inline_handler_no_suggestion() {
        // @click="count++" (inline) → no code action
        let source = "<template><div @click=\"count++\"></div></template>";
        let analysis = make_analysis(vec![make_handler(
            "click", None, true, // inline
            "div", 15, 30,
        )]);
        let line_index = LineIndex::new_utf16(source);

        let actions = event_type_hint_actions(&analysis, source, &line_index);
        assert!(
            actions.is_empty(),
            "inline handlers should not get type hint actions"
        );
    }

    #[test]
    fn component_event_no_suggestion() {
        // <Child @save="handler" /> → no type hint (not a native element)
        let source = "<template><Child @save=\"handler\" /></template>";
        let analysis = make_analysis(vec![make_handler(
            "save",
            Some("handler"),
            false,
            "Child", // PascalCase = component
            17,
            32,
        )]);
        let line_index = LineIndex::new_utf16(source);

        let actions = event_type_hint_actions(&analysis, source, &line_index);
        assert!(
            actions.is_empty(),
            "component events should not get type hint actions"
        );
    }

    #[test]
    fn unknown_event_no_suggestion() {
        // @custom-event="handler" on native element → no type hint
        let source = "<template><div @custom-event=\"handler\"></div></template>";
        let analysis = make_analysis(vec![make_handler(
            "custom-event",
            Some("handler"),
            false,
            "div",
            15,
            37,
        )]);
        let line_index = LineIndex::new_utf16(source);

        let actions = event_type_hint_actions(&analysis, source, &line_index);
        assert!(
            actions.is_empty(),
            "unknown events should not get type hint actions"
        );
    }

    #[test]
    fn multiple_handlers_independent() {
        let source = "<template><div @click=\"onClick\" @keydown=\"onKey\"></div></template>";
        // @click="onClick" at 15..31, @keydown="onKey" at 32..48
        let analysis = make_analysis(vec![
            make_handler("click", Some("onClick"), false, "div", 15, 31),
            make_handler("keydown", Some("onKey"), false, "div", 32, 48),
        ]);
        let line_index = LineIndex::new_utf16(source);

        let actions = event_type_hint_actions(&analysis, source, &line_index);
        assert_eq!(actions.len(), 2, "should generate 2 actions");

        // Verify different types
        let titles: Vec<_> = actions
            .iter()
            .map(|a| match a {
                CodeActionOrCommand::CodeAction(ca) => ca.title.clone(),
                CodeActionOrCommand::Command(_) => String::new(),
            })
            .collect();
        assert!(titles.iter().any(|t| t.contains("MouseEvent")));
        assert!(titles.iter().any(|t| t.contains("KeyboardEvent")));
        // Negative: no duplicate types
        assert!(!titles.iter().all(|t| t.contains("MouseEvent")));
    }
}
