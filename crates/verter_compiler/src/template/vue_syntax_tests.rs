use super::*;

fn format_event_handler_key(event_name: &str) -> String {
    let mut buf = String::new();
    format_event_handler_key_into(&mut buf, event_name);
    buf
}

// ==================== camelize ====================

#[test]
fn camelize_no_hyphens() {
    assert_eq!(camelize("click"), "click");
}

#[test]
fn camelize_single_hyphen() {
    assert_eq!(camelize("my-event"), "myEvent");
}

#[test]
fn camelize_multiple_hyphens() {
    assert_eq!(camelize("foo-bar-baz"), "fooBarBaz");
}

#[test]
fn camelize_leading_hyphen() {
    assert_eq!(camelize("-leading"), "Leading");
}

#[test]
fn camelize_trailing_hyphen() {
    assert_eq!(camelize("trailing-"), "trailing");
}

#[test]
fn camelize_empty() {
    assert_eq!(camelize(""), "");
}

#[test]
fn camelize_preserves_colons() {
    assert_eq!(camelize("update:modelValue"), "update:modelValue");
}

// ==================== format_event_handler_key ====================

#[test]
fn event_key_click() {
    assert_eq!(format_event_handler_key("click"), "onClick");
}

#[test]
fn event_key_hyphenated() {
    assert_eq!(format_event_handler_key("my-event"), "onMyEvent");
}

#[test]
fn event_key_with_colon() {
    assert_eq!(
        format_event_handler_key("update:modelValue"),
        "onUpdate:modelValue"
    );
}

#[test]
fn event_key_keyup() {
    assert_eq!(format_event_handler_key("keyup"), "onKeyup");
}

#[test]
fn event_key_empty() {
    assert_eq!(format_event_handler_key(""), "on");
}

#[test]
fn event_key_multi_hyphen() {
    assert_eq!(
        format_event_handler_key("my-custom-event"),
        "onMyCustomEvent"
    );
}

#[test]
fn event_key_into_appends() {
    let mut buf = String::from("prefix:");
    format_event_handler_key_into(&mut buf, "click");
    assert_eq!(buf, "prefix:onClick");
}

// ==================== needs_quoted_key ====================

#[test]
fn quoted_key_simple_identifiers() {
    assert!(!needs_quoted_key("class"));
    assert!(!needs_quoted_key("onClick"));
    assert!(!needs_quoted_key("_private"));
    assert!(!needs_quoted_key("$data"));
}

#[test]
fn quoted_key_with_hyphen() {
    assert!(needs_quoted_key("data-id"));
    assert!(needs_quoted_key("my-prop"));
}

#[test]
fn quoted_key_with_colon() {
    assert!(needs_quoted_key("onUpdate:modelValue"));
}

#[test]
fn quoted_key_starts_with_digit() {
    assert!(needs_quoted_key("0abc"));
}

#[test]
fn quoted_key_empty() {
    assert!(needs_quoted_key(""));
}

#[test]
fn quoted_key_alphanumeric() {
    assert!(!needs_quoted_key("prop1"));
    assert!(!needs_quoted_key("a123"));
}
