use super::{effect::TextPart, scope::VaporScope, text::process_text};
use oxc_allocator::Allocator;
#[test]
fn plain_text_appends_to_html() {
    let alloc = Allocator::default();
    let mut html = String::new();
    let mut scope = VaporScope::default();
    process_text("hello", &mut html, &mut scope, true, true, &alloc, false);
    assert_eq!(html, "hello");
}
#[test]
fn multiple_text_nodes_append_to_html() {
    let alloc = Allocator::default();
    let mut html = String::new();
    let mut scope = VaporScope::default();
    process_text("hello", &mut html, &mut scope, true, true, &alloc, false);
    process_text(" world", &mut html, &mut scope, true, true, &alloc, false);
    assert_eq!(html, "hello world");
    assert_eq!(scope.text_parts.len(), 2);
}
#[test]
fn text_records_static_part() {
    let alloc = Allocator::default();
    let mut html = String::new();
    let mut scope = VaporScope::default();
    process_text("hello", &mut html, &mut scope, true, true, &alloc, false);
    assert_eq!(scope.text_parts, [TextPart::Static("\"hello\"")]);
}
#[test]
fn text_with_quotes_escapes_in_part() {
    let alloc = Allocator::default();
    let mut html = String::new();
    let mut scope = VaporScope::default();
    process_text(
        "say \"hi\"",
        &mut html,
        &mut scope,
        true,
        true,
        &alloc,
        false,
    );
    assert_eq!(html, "say \"hi\"");
    assert_eq!(scope.text_parts, [TextPart::Static("\"say \\\"hi\\\"\"")]);
}
#[test]
fn static_only_skips_text_parts() {
    let alloc = Allocator::default();
    let mut html = String::new();
    let mut scope = VaporScope::default();
    process_text("hello", &mut html, &mut scope, false, true, &alloc, false);
    assert_eq!(html, "hello");
    assert!(scope.text_parts.is_empty());
}
#[test]
fn write_html_false_skips_html_but_still_records_text_part() {
    let alloc = Allocator::default();
    let mut html = "<p> ".to_owned();
    let mut scope = VaporScope::default();
    process_text("hello", &mut html, &mut scope, true, false, &alloc, false);
    assert_eq!(html, "<p> ");
    assert_eq!(scope.text_parts, [TextPart::Static("\"hello\"")]);
}
#[test]
fn text_with_newline_escapes_in_part() {
    let alloc = Allocator::default();
    let mut html = String::new();
    let mut scope = VaporScope::default();
    process_text(
        "line1\nline2",
        &mut html,
        &mut scope,
        true,
        true,
        &alloc,
        true,
    );
    assert_eq!(html, "line1\nline2");
    assert_eq!(scope.text_parts, [TextPart::Static("\"line1\\nline2\"")]);
}
