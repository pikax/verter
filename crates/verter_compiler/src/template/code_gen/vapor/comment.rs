//! Vapor comment node code generation.
//!
//! In Vapor mode, comments are appended directly to the parent element's
//! static HTML buffer when comments are enabled, or skipped entirely
//! when comments are disabled (production mode).

use crate::ast::types::CommentNode;

/// Process a comment node in Vapor mode.
///
/// When `comments` is true, the full `<!-- ... -->` is appended to `html` —
/// the shared scope buffer for the enclosing template — and the caller counts
/// it as a DOM child. When false, the comment is completely skipped.
pub fn process_comment(comment: &CommentNode, source: &str, comments: bool, html: &mut String) {
    if !comments {
        return;
    }

    // Append the raw comment HTML
    let raw = &source[comment.start as usize..comment.end as usize];
    html.push_str(raw);
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let c1 = CommentNode {
            start: 0,
            end: 10,
            content_start: 5,
            content_end: 7,
        };
        let c2 = CommentNode {
            start: 10,
            end: 20,
            content_start: 15,
            content_end: 17,
        };
        process_comment(&c1, source, true, &mut html);
        process_comment(&c2, source, true, &mut html);

        assert_eq!(html, "<!-- a --><!-- b -->");
    }
}
