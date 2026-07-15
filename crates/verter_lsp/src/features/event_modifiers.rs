//! Canonical Vue event-modifier descriptions.
//!
//! This is the single source of truth for the human-readable description of each
//! `v-on` / `@event` modifier. Two consumers share it:
//! - completion (offering `.stop`/`.prevent`/… after `@event.`), and
//! - hover (describing a `.stop` modifier token sitting on an event directive).
//!
//! Keeping one table avoids the descriptions drifting between the two surfaces.

/// Runtime modifiers available for all events.
pub const RUNTIME_MODIFIERS: &[(&str, &str)] = &[
    ("stop", "Call event.stopPropagation()"),
    ("prevent", "Call event.preventDefault()"),
    ("self", "Only trigger if event.target is the element itself"),
    ("once", "Trigger at most once"),
    ("capture", "Use capture mode for addEventListener"),
    ("passive", "Mark addEventListener as passive"),
];

/// System modifier keys (available for all events).
pub const SYSTEM_MODIFIERS: &[(&str, &str)] = &[
    ("ctrl", "Require Ctrl key"),
    ("shift", "Require Shift key"),
    ("alt", "Require Alt key"),
    ("meta", "Require Meta/Command key"),
    ("exact", "Require exact modifier combination"),
];

/// Key modifiers for keyboard events (keydown, keyup, keypress).
pub const KEY_MODIFIERS: &[(&str, &str)] = &[
    ("enter", "Enter key"),
    ("tab", "Tab key"),
    ("delete", "Delete or Backspace key"),
    ("esc", "Escape key"),
    ("space", "Space key"),
    ("up", "Arrow Up"),
    ("down", "Arrow Down"),
    ("left", "Arrow Left (key)"),
    ("right", "Arrow Right (key)"),
    ("page-down", "Page Down"),
    ("page-up", "Page Up"),
    ("home", "Home key"),
    ("end", "End key"),
];

/// Mouse button modifiers (for click, mousedown, mouseup).
pub const MOUSE_BUTTON_MODIFIERS: &[(&str, &str)] = &[
    ("left", "Left mouse button"),
    ("right", "Right mouse button"),
    ("middle", "Middle mouse button"),
];

/// Whether `event_name` is a keyboard event, so [`KEY_MODIFIERS`] (`.enter`,
/// `.esc`, the arrow keys) are the applicable family.
///
/// Shared by completion (which family to offer) and hover (how to describe an
/// ambiguous modifier) so the two surfaces never disagree.
pub fn is_keyboard_event(event_name: &str) -> bool {
    event_name.starts_with("key")
}

/// Whether `event_name` is a mouse-button event, so [`MOUSE_BUTTON_MODIFIERS`]
/// (`.left`/`.right`/`.middle` as mouse buttons) are the applicable family.
///
/// Shared by completion and hover (see [`is_keyboard_event`]).
pub fn is_mouse_button_event(event_name: &str) -> bool {
    matches!(
        event_name,
        "click" | "dblclick" | "mousedown" | "mouseup" | "contextmenu"
    )
}

/// Look up the human-readable description for an event modifier by name,
/// searching every modifier family in order. Returns `None` for an unknown
/// modifier (e.g. a custom key alias the user invented).
///
/// Note: `left`/`right` appear in both [`KEY_MODIFIERS`] and
/// [`MOUSE_BUTTON_MODIFIERS`]; the key-family description wins in this
/// context-free lookup because the arrow-key reading is the more common one.
/// Callers that know the event name should prefer
/// [`modifier_description_for_event`], which disambiguates mouse-button events.
pub fn modifier_description(name: &str) -> Option<&'static str> {
    RUNTIME_MODIFIERS
        .iter()
        .chain(SYSTEM_MODIFIERS)
        .chain(KEY_MODIFIERS)
        .chain(MOUSE_BUTTON_MODIFIERS)
        .find(|(modifier, _)| *modifier == name)
        .map(|(_, desc)| *desc)
}

/// Look up a modifier description WITH event-name context.
///
/// The ambiguous `left`/`right` modifiers mean different things by event family:
/// on a mouse-button event (`@click.left`) they are mouse buttons; on a keyboard
/// event (`@keydown.left`) they are arrow keys. When the event is a mouse-button
/// event we consult [`MOUSE_BUTTON_MODIFIERS`] first; otherwise we fall back to the
/// context-free [`modifier_description`] (whose key-family ordering yields the
/// arrow-key reading). Runtime/system modifiers (`.stop`, `.ctrl`, …) resolve
/// identically through the fallback regardless of event family.
pub fn modifier_description_for_event(event_name: &str, modifier: &str) -> Option<&'static str> {
    if is_mouse_button_event(event_name) {
        if let Some(desc) = MOUSE_BUTTON_MODIFIERS
            .iter()
            .find(|(name, _)| *name == modifier)
            .map(|(_, desc)| *desc)
        {
            return Some(desc);
        }
    }
    modifier_description(modifier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_event_picks_mouse_button_reading_for_left_right() {
        // `@click.left` must describe the LEFT MOUSE BUTTON, not Arrow Left.
        assert_eq!(
            modifier_description_for_event("click", "left"),
            Some("Left mouse button")
        );
        assert_eq!(
            modifier_description_for_event("mousedown", "right"),
            Some("Right mouse button")
        );
        // Context-free lookup (no event) keeps the arrow-key reading.
        assert_eq!(modifier_description("left"), Some("Arrow Left (key)"));
    }

    #[test]
    fn keyboard_event_keeps_arrow_key_reading_for_left_right() {
        assert_eq!(
            modifier_description_for_event("keydown", "left"),
            Some("Arrow Left (key)")
        );
        assert_eq!(
            modifier_description_for_event("keyup", "right"),
            Some("Arrow Right (key)")
        );
    }

    #[test]
    fn runtime_modifier_resolves_regardless_of_event_family() {
        // `.stop` is a runtime modifier — same description on any event.
        assert_eq!(
            modifier_description_for_event("click", "stop"),
            Some("Call event.stopPropagation()")
        );
        assert_eq!(
            modifier_description_for_event("keydown", "stop"),
            Some("Call event.stopPropagation()")
        );
    }

    #[test]
    fn event_family_classification() {
        assert!(is_keyboard_event("keydown"));
        assert!(is_keyboard_event("keyup"));
        assert!(!is_keyboard_event("click"));
        assert!(is_mouse_button_event("click"));
        assert!(is_mouse_button_event("contextmenu"));
        assert!(!is_mouse_button_event("keydown"));
    }
}
