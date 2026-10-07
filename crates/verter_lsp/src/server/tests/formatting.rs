use super::*;

/// On-type formatting is advertised for the proactive tag auto-close handler.
/// Its only meaningful trigger is `>` (the close of an open tag): the handler
/// (`auto_close_tag`) hard-requires a `>` immediately before the cursor, so a
/// `/` more-trigger is a guaranteed no-op. We advertise `>` and NOT `/` — the
/// honest trigger set the handler can actually act on.
#[test]
fn on_type_formatting_triggers_on_gt_only_not_slash() {
    let caps = host_server_capabilities(true);
    let on_type = caps
        .document_on_type_formatting_provider
        .as_ref()
        .expect("on-type formatting must be advertised for tag auto-close");

    assert_eq!(
        on_type.first_trigger_character, ">",
        "the first (and only meaningful) on-type trigger must be `>`"
    );

    // The `/` more-trigger is dead — the handler requires a `>` before the
    // cursor — so it must NOT be advertised. This assertion FAILS against the
    // legacy `more_trigger_character: Some(vec![\"/\"])` and PASSES once dropped.
    let has_slash_trigger = on_type
        .more_trigger_character
        .as_ref()
        .is_some_and(|chars| chars.iter().any(|c| c == "/"));
    assert!(
        !has_slash_trigger,
        "the `/` more-trigger is a guaranteed no-op and must not be advertised"
    );
}
