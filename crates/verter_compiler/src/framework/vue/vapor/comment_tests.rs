use super::comment::process_comment;
use crate::ast::types::CommentNode;

#[test]
fn comment_enabled_appends_to_html() {
    let mut html = String::new();
    let source = "<!-- hello -->";
    let comment = CommentNode {
        start: 0,
        end: 14,
        content_start: 5,
        content_end: 11,
    };
    process_comment(&comment, source, true, &mut html);
    assert_eq!(html, "<!-- hello -->");
}
#[test]
fn comment_disabled_skips() {
    let mut html = String::new();
    let source = "<!-- hello -->";
    let comment = CommentNode {
        start: 0,
        end: 14,
        content_start: 5,
        content_end: 11,
    };
    process_comment(&comment, source, false, &mut html);
    assert_eq!(html, "");
}
#[test]
fn comment_multiple_appends_to_html() {
    let mut html = String::new();
    let source = "<!-- a --><!-- b -->";
    let first = CommentNode {
        start: 0,
        end: 10,
        content_start: 5,
        content_end: 7,
    };
    let second = CommentNode {
        start: 10,
        end: 20,
        content_start: 15,
        content_end: 17,
    };
    process_comment(&first, source, true, &mut html);
    process_comment(&second, source, true, &mut html);
    assert_eq!(html, "<!-- a --><!-- b -->");
}
