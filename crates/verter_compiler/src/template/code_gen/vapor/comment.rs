//! Vapor comment node code generation.
//!
//! In Vapor mode, comments are appended directly to the parent element's
//! static HTML buffer when comments are enabled, or skipped entirely
//! when comments are disabled (production mode).

use crate::ast::types::CommentNode;
use crate::template::code_gen::types::VaporElementState;

/// Process a comment node in Vapor mode.
///
/// When `comments` is true, the full `<!-- ... -->` is appended to the
/// parent's HTML buffer and counted as a DOM child.
/// When false, the comment is completely skipped.
pub fn process_comment(
    comment: &CommentNode,
    source: &str,
    comments: bool,
    parent: &mut VaporElementState<'_>,
) {
    if !comments {
        return;
    }

    // Append the raw comment HTML
    let raw = &source[comment.start as usize..comment.end as usize];
    parent.html.push_str(raw);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_parent() -> VaporElementState<'static> {
        VaporElementState::new()
    }

    #[test]
    fn comment_enabled_appends_to_html() {
        let mut parent = make_parent();
        let source = "<!-- hello -->";
        let comment = CommentNode {
            start: 0,
            end: 14,
            content_start: 5,
            content_end: 11,
        };
        process_comment(&comment, source, true, &mut parent);

        assert_eq!(parent.html, "<!-- hello -->");
    }

    #[test]
    fn comment_disabled_skips() {
        let mut parent = make_parent();
        let source = "<!-- hello -->";
        let comment = CommentNode {
            start: 0,
            end: 14,
            content_start: 5,
            content_end: 11,
        };
        process_comment(&comment, source, false, &mut parent);

        assert_eq!(parent.html, "");
    }

    #[test]
    fn comment_multiple_appends_to_html() {
        let mut parent = make_parent();
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
        process_comment(&c1, source, true, &mut parent);
        process_comment(&c2, source, true, &mut parent);

        assert_eq!(parent.html, "<!-- a --><!-- b -->");
    }
}
