//! VDOM comment node code generation.
//!
//! Transforms `<!-- content -->` into `_createCommentVNode("content")`.
//! When comments are disabled, the comment is removed entirely.
//!
//! Two processing modes:
//! - **Simple**: content has no special chars → two overwrites for prefix and suffix.
//! - **Complex**: content has `"`, `\`, newlines → same two overwrites + `prepend_alloc`
//!   for the escaped content and `overwrite_alloc` to delete the original.

use crate::new_impl::ast::types::CommentNode;

use super::super::shared::helpers::{escape_js_string_into, needs_js_escaping, VdomHelper};
use super::super::types::{ChildKind, ChildRecord, CodeGenOutput};

/// Process a comment node for VDOM codegen.
///
/// If `comments_enabled` is `true`, transforms `<!-- content -->` into
/// `_createCommentVNode("content")`. If `false`, returns `None` (comment is dropped).
///
/// Returns a [`ChildRecord`] with `ChildKind::Comment` when the comment is kept.
pub fn process_comment<'alloc>(
    comment: &CommentNode,
    source: &str,
    comments_enabled: bool,
    out: &mut CodeGenOutput<'alloc>,
) -> Option<ChildRecord> {
    if !comments_enabled {
        // Drop the comment: overwrite entire span with empty string
        out.overwrite(comment.start, comment.end, "");
        return None;
    }

    let content = &source[comment.content_start as usize..comment.content_end as usize];

    // Simple: no escaping needed — two overwrites for prefix and suffix
    out.overwrite(
        comment.start,
        comment.content_start,
        "_createCommentVNode(\"",
    );
    out.overwrite(comment.content_end, comment.end, "\")");

    // Register the runtime helper import
    out.add_vdom_import(VdomHelper::CreateCommentVNode);

    // Complex: content needs escaping — prepend escaped content, delete original
    if needs_js_escaping(content) {
        let mut buf = String::with_capacity(content.len() + content.len() / 4);
        escape_js_string_into(&mut buf, content);
        out.prepend_alloc(comment.content_start, &buf);
        out.overwrite_alloc(comment.content_start, comment.content_end, "");
    }

    Some(ChildRecord {
        start: comment.start,
        end: comment.end,
        kind: ChildKind::Comment,
        condition: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_transform::CodeTransform;
    use oxc_allocator::Allocator;

    fn make_comment(start: u32, content_start: u32, content_end: u32, end: u32) -> CommentNode {
        CommentNode {
            start,
            end,
            content_start,
            content_end,
        }
    }

    /// Apply accumulated output to the source and return the final string.
    fn apply_to_string<'a>(source: &str, out: CodeGenOutput<'a>, alloc: &'a Allocator) -> String {
        let mut ct = CodeTransform::new(source, alloc);
        out.apply_to(&mut ct);
        ct.build_string()
    }

    // ==================== Comments enabled: simple path ====================

    #[test]
    fn simple_comment_two_overwrites() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // <!-- hello -->
        //  0123456789...
        let source = "<!-- hello -->";
        let comment = make_comment(0, 5, 10, 14);

        let record = process_comment(&comment, source, true, &mut out).unwrap();

        assert_eq!(record.kind, ChildKind::Comment);
        assert_eq!(record.start, 0);
        assert_eq!(record.end, 14);

        // Two overwrites: prefix and suffix; no prepends
        assert_eq!(out.overwrites.len(), 2);
        assert_eq!(out.prepends.len(), 0);
        assert_eq!(out.overwrites[0], (0, 5, "_createCommentVNode(\""));
        assert_eq!(out.overwrites[1], (10, 14, "\")"));
    }

    #[test]
    fn simple_comment_e2e() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "<!-- hello -->";
        let comment = make_comment(0, 5, 10, 14);

        process_comment(&comment, source, true, &mut out).unwrap();

        // content is "hello" (content_start=5 excludes leading space, content_end=10 excludes trailing space)
        let result = apply_to_string(source, out, &alloc);
        assert_eq!(result, "_createCommentVNode(\"hello\")");
    }

    #[test]
    fn empty_comment_two_overwrites() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "<!---->";
        let comment = make_comment(0, 4, 4, 7);

        let record = process_comment(&comment, source, true, &mut out).unwrap();

        assert_eq!(record.kind, ChildKind::Comment);
        assert_eq!(out.overwrites.len(), 2);
        assert_eq!(out.prepends.len(), 0);
        assert_eq!(out.overwrites[0], (0, 4, "_createCommentVNode(\""));
        assert_eq!(out.overwrites[1], (4, 7, "\")"));
    }

    #[test]
    fn simple_comment_registers_import() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "<!-- hello -->";
        let comment = make_comment(0, 5, 10, 14);

        process_comment(&comment, source, true, &mut out);

        assert!(
            out.vdom_imports().has(VdomHelper::CreateCommentVNode),
            "Expected CreateCommentVNode import"
        );
    }

    // ==================== Comments enabled: complex path ====================

    #[test]
    fn comment_with_quotes_prepend_path() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // <!-- say "hi" -->  content "say "hi"" is at positions 5..13
        let source = "<!-- say \"hi\" -->";
        let comment = make_comment(0, 5, 13, 17);

        let record = process_comment(&comment, source, true, &mut out).unwrap();

        assert_eq!(record.kind, ChildKind::Comment);
        // Three overwrites: prefix, suffix, delete-original; one prepend with escaped content
        assert_eq!(out.overwrites.len(), 3);
        assert_eq!(out.prepends.len(), 1);
        assert_eq!(out.overwrites[0], (0, 5, "_createCommentVNode(\""));
        assert_eq!(out.overwrites[1], (13, 17, "\")"));
        assert_eq!(out.overwrites[2], (5, 13, ""));
        assert_eq!(out.prepends[0], (5, "say \\\"hi\\\""));
    }

    #[test]
    fn comment_with_quotes_e2e() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "<!-- say \"hi\" -->";
        let comment = make_comment(0, 5, 13, 17);

        process_comment(&comment, source, true, &mut out).unwrap();

        let result = apply_to_string(source, out, &alloc);
        assert_eq!(result, "_createCommentVNode(\"say \\\"hi\\\"\")");
    }

    #[test]
    fn comment_with_newline_prepend_path() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "<!--line1\nline2-->";
        let comment = make_comment(0, 4, 15, 18);

        process_comment(&comment, source, true, &mut out);

        assert_eq!(out.overwrites.len(), 3);
        assert_eq!(out.prepends.len(), 1);
        assert_eq!(out.prepends[0], (4, "line1\\nline2"));
        assert_eq!(out.overwrites[2], (4, 15, ""));
    }

    #[test]
    fn comment_with_newline_e2e() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "<!--line1\nline2-->";
        let comment = make_comment(0, 4, 15, 18);

        process_comment(&comment, source, true, &mut out);

        let result = apply_to_string(source, out, &alloc);
        assert_eq!(result, "_createCommentVNode(\"line1\\nline2\")");
    }

    #[test]
    fn comment_with_backslash_prepend_path() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = r"<!--a\b-->";
        let comment = make_comment(0, 4, 7, 10);

        process_comment(&comment, source, true, &mut out);

        assert_eq!(out.overwrites.len(), 3);
        assert_eq!(out.prepends.len(), 1);
        assert_eq!(out.prepends[0], (4, "a\\\\b"));
        assert_eq!(out.overwrites[2], (4, 7, ""));
    }

    #[test]
    fn comment_with_backslash_e2e() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = r"<!--a\b-->";
        let comment = make_comment(0, 4, 7, 10);

        process_comment(&comment, source, true, &mut out);

        let result = apply_to_string(source, out, &alloc);
        assert_eq!(result, "_createCommentVNode(\"a\\\\b\")");
    }

    // ==================== Comments disabled ====================

    #[test]
    fn comments_disabled_returns_none() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "<!-- hello -->";
        let comment = make_comment(0, 5, 10, 14);

        let result = process_comment(&comment, source, false, &mut out);

        assert!(result.is_none());
        // Overwrite entire span with empty string
        assert_eq!(out.overwrites.len(), 1);
        assert_eq!(out.overwrites[0], (0, 14, ""));
    }

    // ==================== Offset handling ====================

    #[test]
    fn comment_at_offset() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // Comment starts at offset 5 in the source
        let source = "<div><!-- hi --></div>";
        let comment = make_comment(5, 10, 13, 16);

        let record = process_comment(&comment, source, true, &mut out).unwrap();

        assert_eq!(record.start, 5);
        assert_eq!(record.end, 16);
        assert_eq!(out.overwrites[0], (5, 10, "_createCommentVNode(\""));
        assert_eq!(out.overwrites[1], (13, 16, "\")"));
        assert_eq!(out.prepends.len(), 0);
    }

    #[test]
    fn comment_at_offset_e2e() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "<div><!-- hi --></div>";
        let comment = make_comment(5, 10, 13, 16);

        process_comment(&comment, source, true, &mut out).unwrap();

        let result = apply_to_string(source, out, &alloc);
        assert_eq!(result, "<div>_createCommentVNode(\"hi \")</div>");
    }
}
