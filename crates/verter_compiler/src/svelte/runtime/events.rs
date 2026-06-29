//! The Svelte-5 event-attribute policy (a faithful port of the official
//! `svelte@5.56.3` event helpers).
//!
//! Ports:
//! - `is_capture_event` (`src/utils.js`): a name is a capture-phase handler iff
//!   it ENDS in `capture` AND is not the excluded `gotpointercapture` /
//!   `lostpointercapture`.
//! - `can_delegate_event` + `DELEGATED_EVENTS` (`src/utils.js`): the closed set
//!   of element events Svelte 5 delegates (a document-level listener via
//!   `$.delegate([...])` + per-node `$.delegated(...)`).
//! - the event-name normalization from `visit_event_attribute`
//!   (`phases/3-transform/client/visitors/shared/events.js`): the event name is
//!   `name.slice(2)` (the attribute name with the leading `on` stripped — NO
//!   lowercase filter), then the trailing `capture` (the last 7 chars) is
//!   stripped when `is_capture_event`.
//! - the modern-vs-legacy DELEGATION rule (`phases/2-analyze/visitors/Attribute.js`
//!   `node.metadata.delegated = parent.type === 'RegularElement' &&
//!   can_delegate_event(node.name.slice(2))` + `OnDirective.js`'s `build_event(…,
//!   false)`): only a MODERN `onclick={…}` attribute on a regular element may be
//!   delegated; a legacy `on:click` directive is ALWAYS a direct `$.event`.
//!
//! These drive the `AttrIr::Event` / `EventOp` `event_type` / `capture` /
//! `delegated` fields and the topology's delegated set.

/// The Svelte-5 delegated event types (`DELEGATED_EVENTS`) — `onclick` etc.
/// register their event type in the delegated set; `onfocus` / `onblur` (and the
/// other non-bubbling events) are NON-delegated direct listeners. A capture
/// handler's RAW name (`clickcapture`) is never in this set, so a capture event
/// is never delegated.
const DELEGATED_EVENTS: &[&str] = &[
    "beforeinput",
    "click",
    "change",
    "dblclick",
    "contextmenu",
    "focusin",
    "focusout",
    "input",
    "keydown",
    "keyup",
    "mousedown",
    "mousemove",
    "mouseout",
    "mouseover",
    "mouseup",
    "pointerdown",
    "pointermove",
    "pointerout",
    "pointerover",
    "pointerup",
    "touchend",
    "touchmove",
    "touchstart",
];

/// Whether `event_name` is a delegated event (the official `can_delegate_event`:
/// membership in `DELEGATED_EVENTS`). `focus` / `blur` and other non-bubbling
/// events are NOT delegated; a capture-suffixed RAW name (`clickcapture`) is not
/// in the set either.
#[must_use]
pub fn can_delegate_event(event_name: &str) -> bool {
    DELEGATED_EVENTS.contains(&event_name)
}

/// Whether `name` is a capture-phase handler name (the official
/// `is_capture_event`): it ends in `capture`, EXCEPT the two pointer events whose
/// own names end in `capture` but are not capture-phase handlers
/// (`gotpointercapture` / `lostpointercapture`). Operates on the RAW name (the
/// attribute name with the leading `on` already stripped).
#[must_use]
pub fn is_capture_event(name: &str) -> bool {
    name.ends_with("capture") && name != "gotpointercapture" && name != "lostpointercapture"
}

/// The Svelte-5 passive-by-default event types (`PASSIVE_EVENTS`) — `touchstart` /
/// `touchmove` register with a passive listener by default. The official
/// `is_passive_event` (`src/utils.js`) drives the MODERN attribute form's default
/// `passive: true` (`visit_event_attribute` passes `is_passive_event(name) ? true :
/// undefined`); the legacy `on:` directive form derives passive from its
/// `|passive` / `|nonpassive` modifiers ONLY (it does NOT consult this set — verified
/// against svelte@5.56.3: `on:touchstart={h}` emits no passive arg).
const PASSIVE_EVENTS: &[&str] = &["touchstart", "touchmove"];

/// Whether `event_name` is a passive-by-default event (the official
/// `is_passive_event`: membership in `PASSIVE_EVENTS`). Drives the MODERN attribute
/// form's default `passive: true` — `touchstart` / `touchmove` ⇒ `true`, every other
/// type ⇒ no default passive.
#[must_use]
pub fn is_passive_event(event_name: &str) -> bool {
    PASSIVE_EVENTS.contains(&event_name)
}

/// The official `EVENT_MODIFIERS` set (the analyze-phase `validate_element`
/// allowlist). A legacy `on:` directive modifier not in this set is the official
/// `event_handler_invalid_modifier` compile error. (`capture` selects the capture
/// phase; `passive` / `nonpassive` select the passive option; the remaining six are
/// the handler WRAPPERS.)
const EVENT_MODIFIERS: &[&str] = &[
    "preventDefault",
    "stopPropagation",
    "stopImmediatePropagation",
    "capture",
    "once",
    "passive",
    "nonpassive",
    "self",
    "trusted",
];

/// An official-invalid legacy `on:` modifier set — the analyze-phase
/// `validate_element` rejections, surfaced as a typed policy error the caller maps to
/// its fail-closed refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventModifierError {
    /// A modifier not in [`EVENT_MODIFIERS`] (official `event_handler_invalid_modifier`).
    Unknown(String),
    /// `passive` co-occurring with `nonpassive` / `preventDefault` (official
    /// `event_handler_invalid_modifier_combination`).
    InvalidPassiveCombination {
        /// The conflicting modifier (`nonpassive` or `preventDefault`).
        conflicting: String,
    },
}

/// Validate a legacy `on:` directive's modifier set against the official
/// `validate_element` rules: every modifier must be recognized, and `passive` must
/// not co-occur with `nonpassive` / `preventDefault`. Pure policy — the caller maps
/// the error to its fail-closed refusal surface. A modern attribute form (no
/// modifiers) validates trivially.
pub fn validate_event_modifiers(modifiers: &[String]) -> Result<(), EventModifierError> {
    let mut has_passive = false;
    let mut conflicting = None;
    for m in modifiers {
        if !EVENT_MODIFIERS.contains(&m.as_str()) {
            return Err(EventModifierError::Unknown(m.clone()));
        }
        match m.as_str() {
            "passive" => has_passive = true,
            "nonpassive" | "preventDefault" => conflicting = Some(m.clone()),
            _ => {}
        }
    }
    if has_passive {
        if let Some(conflicting) = conflicting {
            return Err(EventModifierError::InvalidPassiveCombination { conflicting });
        }
    }
    Ok(())
}

/// Normalize a RAW event name (the attribute name without the leading `on`, or a
/// legacy `on:` directive's local name) into `(event_type, is_capture)`, mirroring
/// `visit_event_attribute`'s `is_capture_event` + `slice(0, -7)` strip: when the
/// name is a capture event, drop the trailing 7-char `capture` suffix and set the
/// capture flag. The two excluded pointer-capture events keep their whole name and
/// are not capture handlers.
#[must_use]
pub fn normalize_event_name(raw: &str) -> (String, bool) {
    if is_capture_event(raw) {
        // Strip the trailing `capture` (7 chars), matching `slice(0, -7)`.
        (raw[..raw.len() - "capture".len()].to_string(), true)
    } else {
        (raw.to_string(), false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_capture_event_excludes_pointer_capture_events() {
        assert!(is_capture_event("clickcapture"));
        assert!(is_capture_event("pointerdowncapture"));
        // The two excluded names end in `capture` but are NOT capture events.
        assert!(!is_capture_event("gotpointercapture"));
        assert!(!is_capture_event("lostpointercapture"));
        // The doubled form IS a capture event (the OUTER capture).
        assert!(is_capture_event("gotpointercapturecapture"));
        assert!(!is_capture_event("click"));
    }

    #[test]
    fn is_passive_event_matches_official_passive_set() {
        // The passive-by-default set (PASSIVE_EVENTS) ground-truthed against
        // svelte@5.56.3 `is_passive_event` — exactly `touchstart` / `touchmove`.
        assert!(is_passive_event("touchstart"));
        assert!(is_passive_event("touchmove"));
        // Every other event type (including the other touch / delegated events) is
        // NOT passive-by-default.
        assert!(!is_passive_event("touchend"));
        assert!(!is_passive_event("click"));
        assert!(!is_passive_event("wheel"));
        assert!(!is_passive_event("scroll"));
        assert!(!is_passive_event("focus"));
    }

    fn mods(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn validate_event_modifiers_accepts_the_official_set() {
        // Every recognized modifier (the official `EVENT_MODIFIERS`) validates, alone
        // and in a non-conflicting stack.
        for m in [
            "preventDefault",
            "stopPropagation",
            "stopImmediatePropagation",
            "capture",
            "once",
            "passive",
            "nonpassive",
            "self",
            "trusted",
        ] {
            assert!(
                validate_event_modifiers(&mods(&[m])).is_ok(),
                "{m} is valid"
            );
        }
        // A non-conflicting stack (wrappers + capture).
        assert!(validate_event_modifiers(&mods(&[
            "preventDefault",
            "stopPropagation",
            "self",
            "capture"
        ]))
        .is_ok());
        // `passive` with a non-conflicting wrapper (`stopPropagation`) is allowed.
        assert!(validate_event_modifiers(&mods(&["passive", "stopPropagation"])).is_ok());
        // The empty (modern attribute) set validates trivially.
        assert!(validate_event_modifiers(&[]).is_ok());
    }

    #[test]
    fn validate_event_modifiers_rejects_unknown_and_invalid_combos() {
        // An unrecognized modifier (`stop`) is the official `event_handler_invalid_modifier`.
        assert_eq!(
            validate_event_modifiers(&mods(&["stop"])),
            Err(EventModifierError::Unknown("stop".to_string()))
        );
        // `passive` + `preventDefault` and `passive` + `nonpassive` are the official
        // `event_handler_invalid_modifier_combination` (both source orders).
        assert!(matches!(
            validate_event_modifiers(&mods(&["passive", "preventDefault"])),
            Err(EventModifierError::InvalidPassiveCombination { .. })
        ));
        assert!(matches!(
            validate_event_modifiers(&mods(&["preventDefault", "passive"])),
            Err(EventModifierError::InvalidPassiveCombination { .. })
        ));
        assert!(matches!(
            validate_event_modifiers(&mods(&["passive", "nonpassive"])),
            Err(EventModifierError::InvalidPassiveCombination { .. })
        ));
    }
}
