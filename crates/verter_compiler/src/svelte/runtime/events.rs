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
}
