//! VDOM text node code generation.
//!
//! Handles whitespace condensation (Vue's condense mode) and JS string escaping.
//! All mutations go through `CodeGenOutput` overwrites.
//!
//! The text handler:
//! - For all-whitespace content: returns `WhitespaceNewline`/`WhitespaceSpace` for
//!   the parent's leave phase to resolve based on sibling context.
//! - For content text: condenses consecutive whitespace to single space,
//!   escapes characters for JS string literals, and returns `ChildKind::Text`.
//!
//! The parent is responsible for adding the wrapping quotes (`".."`).

use crate::new_impl::ast::types::TextNode;

use super::super::shared::helpers::{escape_js_string_into, needs_js_escaping};
use super::super::types::{ChildKind, ChildRecord, CodeGenOutput};

/// Check if a byte is whitespace (space, tab, newline, carriage return).
#[inline]
fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// Check if text has consecutive whitespace that needs condensing.
#[inline]
fn has_consecutive_ws(text: &str) -> bool {
    let bytes = text.as_bytes();
    for i in 0..bytes.len().saturating_sub(1) {
        if is_ws(bytes[i]) && is_ws(bytes[i + 1]) {
            return true;
        }
    }
    false
}

/// Condense consecutive whitespace to single space AND escape for JS string literal.
/// Combined in a single pass for efficiency.
fn condense_and_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_ws = false;

    for ch in text.chars() {
        if ch.is_ascii() && is_ws(ch as u8) {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
            continue;
        }
        in_ws = false;
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\0' => out.push_str("\\0"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if c.is_ascii_control() => {
                use std::fmt::Write;
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Classify text content into a [`ChildKind`] for parent-level processing.
///
/// This is a pure classification function with no side effects — it does not
/// modify `CodeGenOutput`. Used by `build_child_records` to construct child
/// records from the AST without re-running overwrite logic.
///
/// Returns:
/// - `None` if the content is empty
/// - `Some(WhitespaceNewline)` if all whitespace containing a newline
/// - `Some(WhitespaceSpace)` if all whitespace without a newline
/// - `Some(Text)` if the content has non-whitespace characters
pub fn classify_text_kind(content: &str) -> Option<ChildKind> {
    if content.is_empty() {
        return None;
    }

    let is_all_ws = content.bytes().all(is_ws);
    if is_all_ws {
        let has_newline = content.bytes().any(|b| b == b'\n');
        return Some(if has_newline {
            ChildKind::WhitespaceNewline
        } else {
            ChildKind::WhitespaceSpace
        });
    }

    Some(ChildKind::Text)
}

/// Process a text node for VDOM codegen.
///
/// Returns a [`ChildRecord`] describing this text node's classification.
/// Returns `None` only if the text is empty.
///
/// For **all-whitespace** text, returns `WhitespaceNewline` or `WhitespaceSpace`.
/// The parent's `leave_element` resolves these based on sibling context (Vue's condense rules).
///
/// For **content** text, condenses consecutive whitespace and escapes for JS string
/// literal context. If any changes were needed, pushes an overwrite to replace the
/// original source text with the escaped version.
pub fn process_text<'alloc>(
    text: &TextNode,
    source: &str,
    out: &mut CodeGenOutput<'alloc>,
) -> Option<ChildRecord> {
    let content = &source[text.start as usize..text.end as usize];
    let kind = classify_text_kind(content)?;

    // Only apply overwrites for content text (not whitespace-only)
    if kind == ChildKind::Text {
        let need_condense = has_consecutive_ws(content);
        let need_escape = needs_js_escaping(content);

        if need_condense {
            // Condensation implies escaping (single-pass)
            let escaped = condense_and_escape(content);
            out.overwrite(text.start, text.end, &escaped);
        } else if need_escape {
            // Escape only (no condensation needed)
            let mut buf = String::with_capacity(content.len() + 8);
            escape_js_string_into(&mut buf, content);
            out.overwrite(text.start, text.end, &buf);
        }
    }
    // If neither needed, the original source text stays in place

    Some(ChildRecord {
        start: text.start,
        end: text.end,
        kind,
        condition: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;

    fn make_text(start: u32, end: u32) -> TextNode {
        TextNode {
            start,
            end,
            is_entity: false,
        }
    }

    // ==================== Whitespace detection ====================

    #[test]
    fn all_spaces_returns_whitespace_space() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "   ";
        let text = make_text(0, 3);

        let record = process_text(&text, source, &mut out).unwrap();
        assert_eq!(record.kind, ChildKind::WhitespaceSpace);
        assert!(out.overwrites.is_empty());
    }

    #[test]
    fn newline_returns_whitespace_newline() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = " \n ";
        let text = make_text(0, 3);

        let record = process_text(&text, source, &mut out).unwrap();
        assert_eq!(record.kind, ChildKind::WhitespaceNewline);
        assert!(out.overwrites.is_empty());
    }

    #[test]
    fn tab_only_returns_whitespace_space() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "\t\t";
        let text = make_text(0, 2);

        let record = process_text(&text, source, &mut out).unwrap();
        assert_eq!(record.kind, ChildKind::WhitespaceSpace);
    }

    #[test]
    fn cr_lf_returns_whitespace_newline() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "\r\n";
        let text = make_text(0, 2);

        let record = process_text(&text, source, &mut out).unwrap();
        assert_eq!(record.kind, ChildKind::WhitespaceNewline);
    }

    // ==================== Empty text ====================

    #[test]
    fn empty_text_returns_none() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "hello";
        let text = make_text(2, 2); // zero-length

        assert!(process_text(&text, source, &mut out).is_none());
    }

    // ==================== Plain text (no escaping) ====================

    #[test]
    fn plain_text_no_escaping_no_overwrites() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "hello world";
        let text = make_text(0, 11);

        let record = process_text(&text, source, &mut out).unwrap();
        assert_eq!(record.kind, ChildKind::Text);
        assert_eq!(record.start, 0);
        assert_eq!(record.end, 11);
        // No escaping needed → no overwrites
        assert!(out.overwrites.is_empty());
    }

    // ==================== Text with escaping ====================

    #[test]
    fn text_with_double_quote_produces_overwrite() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = r#"say "hi""#;
        let text = make_text(0, 8);

        let record = process_text(&text, source, &mut out).unwrap();
        assert_eq!(record.kind, ChildKind::Text);
        assert_eq!(out.overwrites.len(), 1);
        assert_eq!(out.overwrites[0].0, 0);
        assert_eq!(out.overwrites[0].1, 8);
        assert_eq!(out.overwrites[0].2, r#"say \"hi\""#);
    }

    #[test]
    fn text_with_newline_produces_overwrite() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "line1\nline2";
        let text = make_text(0, 11);

        let record = process_text(&text, source, &mut out).unwrap();
        assert_eq!(record.kind, ChildKind::Text);
        assert_eq!(out.overwrites.len(), 1);
        assert_eq!(out.overwrites[0].2, "line1\\nline2");
    }

    #[test]
    fn text_with_backslash_produces_overwrite() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = r"a\b";
        let text = make_text(0, 3);

        let record = process_text(&text, source, &mut out).unwrap();
        assert_eq!(record.kind, ChildKind::Text);
        assert_eq!(out.overwrites[0].2, "a\\\\b");
    }

    // ==================== Whitespace condensation ====================

    #[test]
    fn consecutive_spaces_condensed_to_one() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "a   b";
        let text = make_text(0, 5);

        let record = process_text(&text, source, &mut out).unwrap();
        assert_eq!(record.kind, ChildKind::Text);
        assert_eq!(out.overwrites.len(), 1);
        assert_eq!(out.overwrites[0].2, "a b");
    }

    #[test]
    fn mixed_whitespace_condensed() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let source = "a \n\t b";
        let text = make_text(0, 6);

        let record = process_text(&text, source, &mut out).unwrap();
        assert_eq!(record.kind, ChildKind::Text);
        assert_eq!(out.overwrites.len(), 1);
        assert_eq!(out.overwrites[0].2, "a b");
    }

    #[test]
    fn condense_plus_escape_combined() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // Content has consecutive whitespace AND a quote that needs escaping
        let source = "a  \"b";
        let text = make_text(0, 5);

        let record = process_text(&text, source, &mut out).unwrap();
        assert_eq!(record.kind, ChildKind::Text);
        assert_eq!(out.overwrites[0].2, "a \\\"b");
    }

    // ==================== Source offset ====================

    #[test]
    fn text_with_offset_uses_correct_range() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // Text "hi" starts at offset 5 in the source
        let source = "<div>hi</div>";
        let text = make_text(5, 7);

        let record = process_text(&text, source, &mut out).unwrap();
        assert_eq!(record.kind, ChildKind::Text);
        assert_eq!(record.start, 5);
        assert_eq!(record.end, 7);
        assert!(out.overwrites.is_empty()); // "hi" needs no escaping
    }

    // ==================== classify_text_kind ====================

    #[test]
    fn classify_empty_returns_none() {
        assert!(classify_text_kind("").is_none());
    }

    #[test]
    fn classify_whitespace_space() {
        assert_eq!(classify_text_kind("   "), Some(ChildKind::WhitespaceSpace));
    }

    #[test]
    fn classify_whitespace_newline() {
        assert_eq!(
            classify_text_kind(" \n "),
            Some(ChildKind::WhitespaceNewline)
        );
    }

    #[test]
    fn classify_content_text() {
        assert_eq!(classify_text_kind("hello"), Some(ChildKind::Text));
    }

    #[test]
    fn classify_tabs_only() {
        assert_eq!(classify_text_kind("\t\t"), Some(ChildKind::WhitespaceSpace));
    }

    #[test]
    fn classify_cr_lf() {
        assert_eq!(
            classify_text_kind("\r\n"),
            Some(ChildKind::WhitespaceNewline)
        );
    }

    #[test]
    fn classify_mixed_ws_and_content() {
        assert_eq!(classify_text_kind("  hello  "), Some(ChildKind::Text));
    }

    // ==================== Internal helpers ====================

    #[test]
    fn has_consecutive_ws_detects_double_space() {
        assert!(has_consecutive_ws("a  b"));
        assert!(has_consecutive_ws("a \n b"));
    }

    #[test]
    fn has_consecutive_ws_false_for_single_spaces() {
        assert!(!has_consecutive_ws("a b c"));
        assert!(!has_consecutive_ws("hello"));
    }

    #[test]
    fn condense_and_escape_handles_all() {
        assert_eq!(condense_and_escape("a  b"), "a b");
        assert_eq!(condense_and_escape("a \n b"), "a b");
        assert_eq!(condense_and_escape("a  \"b"), "a \\\"b");
        assert_eq!(condense_and_escape(" a "), " a ");
    }
}
