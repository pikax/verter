//! VDOM comment node code generation.
//!
//! Transforms `<!-- content -->` into `_createCommentVNode("content")`.
//! When comments are disabled, the comment is removed entirely.
//!
//! Two processing modes:
//! - **Simple**: content has no special chars → two overwrites for prefix and suffix.
//! - **Complex**: content has `"`, `\`, newlines → same two overwrites + `prepend_alloc`
//!   for the escaped content and `overwrite_alloc` to delete the original.

use crate::ast::types::CommentNode;

use super::super::shared::helpers::{escape_js_string_into, needs_js_escaping, VdomHelper};
use super::super::types::CodeGenOutput;

/// Outcome of [`process_comment`].
///
/// A disabled comment's removal is NOT queued as an overwrite here — the
/// span is only a fact returned to the caller. `leave_template`'s
/// root-prefix/suffix owner is the sole authority deciding whether the
/// removal is absorbed (wholly contained by a claimed header range whose
/// synthetic content already elides those bytes) or must be emitted as an
/// ordinary deletion (a comment left unclaimed, interior or trailing). This
/// is what keeps the `overwrites` and `segmented_overwrites` channels
/// disjoint at flush time.
#[derive(Debug)]
pub enum CommentOutcome {
    /// Comments enabled: the comment was transformed into
    /// `_createCommentVNode(...)`; the overwrites/imports were already
    /// queued into `out`. Child-record bookkeeping for a kept comment is
    /// computed separately by `build_child_records` (which walks the AST
    /// directly) — this outcome carries no payload.
    Kept,
    /// Comments disabled: the comment is dropped from output. No overwrite
    /// was queued for its span — the caller records `(start, end)` as a
    /// pending removal.
    Dropped { start: u32, end: u32 },
}

/// Process a comment node for VDOM codegen.
///
/// If `comments_enabled` is `true`, transforms `<!-- content -->` into
/// `_createCommentVNode("content")` and returns [`CommentOutcome::Kept`]. If
/// `false`, returns [`CommentOutcome::Dropped`] carrying the comment's span —
/// see [`CommentOutcome`] for why no overwrite is queued directly here.
pub fn process_comment<'alloc>(
    comment: &CommentNode,
    source: &str,
    comments_enabled: bool,
    out: &mut CodeGenOutput<'alloc>,
) -> CommentOutcome {
    if !comments_enabled {
        return CommentOutcome::Dropped {
            start: comment.start,
            end: comment.end,
        };
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

    CommentOutcome::Kept
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

        let outcome = process_comment(&comment, source, true, &mut out);

        assert!(matches!(outcome, CommentOutcome::Kept));

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

        process_comment(&comment, source, true, &mut out);

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

        let outcome = process_comment(&comment, source, true, &mut out);

        assert!(matches!(outcome, CommentOutcome::Kept));
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

        let outcome = process_comment(&comment, source, true, &mut out);

        assert!(matches!(outcome, CommentOutcome::Kept));
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

        process_comment(&comment, source, true, &mut out);

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
    fn comments_disabled_returns_dropped_span_with_no_overwrite() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "<!-- hello -->";
        let comment = make_comment(0, 5, 10, 14);

        let result = process_comment(&comment, source, false, &mut out);

        // The disabled-comment mutation branch is gone: process_comment
        // returns the span as a FACT, it never queues an overwrite itself —
        // the caller (leave_template's root-prefix/suffix owner, or the
        // leftover-removal drain) decides how the removal is realized.
        match result {
            CommentOutcome::Dropped { start, end } => {
                assert_eq!(start, 0);
                assert_eq!(end, 14);
            }
            CommentOutcome::Kept => panic!("expected CommentOutcome::Dropped"),
        }
        assert_eq!(out.overwrites.len(), 0);
    }

    // ==================== Offset handling ====================

    #[test]
    fn comment_at_offset() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // Comment starts at offset 5 in the source
        let source = "<div><!-- hi --></div>";
        let comment = make_comment(5, 10, 13, 16);

        let outcome = process_comment(&comment, source, true, &mut out);

        assert!(matches!(outcome, CommentOutcome::Kept));
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

        process_comment(&comment, source, true, &mut out);

        let result = apply_to_string(source, out, &alloc);
        assert_eq!(result, "<div>_createCommentVNode(\"hi \")</div>");
    }
}
