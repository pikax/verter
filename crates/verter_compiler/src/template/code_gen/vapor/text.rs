//! Vapor text node code generation.
//!
//! In Vapor mode, text nodes are appended directly to the parent element's
//! static HTML buffer. The parent handles text coalescing (multiple text +
//! interpolation nodes form a single DOM text child).

use crate::ast::types::TextNode;
use crate::template::code_gen::shared::helpers;
use crate::template::code_gen::types::{CodeGenOutput, VaporElementState, VaporTextPart};

/// Process a text node in Vapor mode.
///
/// - Appends the text content to `html`, the shared scope buffer for the
///   enclosing template — UNLESS `write_html` is false, which the caller
///   (`VaporCodeGen::visit_text`) passes once this text's coalesced DOM run
///   has already been established as dynamic (contains an interpolation):
///   the run's static content then lives only in the `_setText` expression
///   (via the `text_parts` push below), collapsed to a single space
///   placeholder in `html` by the caller instead of this text's own bytes.
/// - If the text contains characters needing JS escaping (only matters for
///   template string), we use the raw source since HTML templates don't need
///   JS escaping — they're HTML context.
/// - Records a static text part on `parent` for dynamic text assembly if needed.
pub fn process_text<'a>(
    text: &TextNode,
    source: &str,
    html: &mut String,
    parent: &mut VaporElementState<'a>,
    has_interpolation: bool,
    write_html: bool,
    out: &CodeGenOutput<'a>,
) {
    let content = &source[text.start as usize..text.end as usize];

    // Append to the scope HTML buffer, condensing consecutive whitespace to
    // a single space — Vue's condense mode applies to raw template HTML
    // text the same way it applies to `_setText`'s JS string parts below
    // (`vdom::text::process_text`'s identical rule, mirrored here for the
    // HTML-context, non-JS-escaped case).
    if write_html {
        if helpers::has_consecutive_ws(content) {
            helpers::condense_whitespace_into(html, content);
        } else {
            html.push_str(content);
        }
    }

    // Only record text parts when the parent has interpolation children.
    // For purely static elements, text_parts are never consumed, so skip the allocation.
    if has_interpolation && !content.is_empty() {
        // Build quoted+escaped string into local buffer, then bump-allocate
        let mut buf = String::with_capacity(content.len() + 4);
        buf.push('"');
        if helpers::has_consecutive_ws(content) {
            // Condensation implies escaping (single-pass).
            buf.push_str(&helpers::condense_and_escape_js(content));
        } else if helpers::needs_js_escaping(content) {
            helpers::escape_js_string_into(&mut buf, content);
        } else {
            buf.push_str(content);
        }
        buf.push('"');
        parent
            .text_parts
            .push(VaporTextPart::Static(out.alloc_str(&buf)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;

    fn make_parent() -> VaporElementState<'static> {
        VaporElementState::new()
    }

    #[test]
    fn plain_text_appends_to_html() {
        let alloc = Allocator::default();
        let out = CodeGenOutput::new(&alloc);
        let mut html = String::new();
        let mut parent = make_parent();
        let text = TextNode {
            start: 0,
            end: 5,
            is_entity: false,
            is_whitespace_only: false,
        };
        let source = "hello";
        process_text(&text, source, &mut html, &mut parent, true, true, &out);

        assert_eq!(html, "hello");
    }

    #[test]
    fn multiple_text_nodes_append_to_html() {
        let alloc = Allocator::default();
        let out = CodeGenOutput::new(&alloc);
        let mut html = String::new();
        let mut parent = make_parent();
        let source = "hello world";
        let t1 = TextNode {
            start: 0,
            end: 5,
            is_entity: false,
            is_whitespace_only: false,
        };
        let t2 = TextNode {
            start: 5,
            end: 11,
            is_entity: false,
            is_whitespace_only: false,
        };
        process_text(&t1, source, &mut html, &mut parent, true, true, &out);
        process_text(&t2, source, &mut html, &mut parent, true, true, &out);

        assert_eq!(html, "hello world");
        assert_eq!(parent.text_parts.len(), 2);
    }

    #[test]
    fn text_records_static_part() {
        let alloc = Allocator::default();
        let out = CodeGenOutput::new(&alloc);
        let mut html = String::new();
        let mut parent = make_parent();
        let text = TextNode {
            start: 0,
            end: 5,
            is_entity: false,
            is_whitespace_only: false,
        };
        process_text(&text, "hello", &mut html, &mut parent, true, true, &out);

        assert_eq!(parent.text_parts.len(), 1);
        assert_eq!(parent.text_parts[0].to_js(), "\"hello\"");
        assert!(!parent.text_parts[0].is_dynamic());
    }

    #[test]
    fn text_with_quotes_escapes_in_part() {
        let alloc = Allocator::default();
        let out = CodeGenOutput::new(&alloc);
        let mut html = String::new();
        let mut parent = make_parent();
        let source = r#"say "hi""#;
        let text = TextNode {
            start: 0,
            end: source.len() as u32,
            is_entity: false,
            is_whitespace_only: false,
        };
        process_text(&text, source, &mut html, &mut parent, true, true, &out);

        // HTML buffer has raw content
        assert_eq!(html, r#"say "hi""#);
        // Text part has JS-escaped content
        assert_eq!(parent.text_parts[0].to_js(), r#""say \"hi\"""#);
    }

    #[test]
    fn static_only_skips_text_parts() {
        let alloc = Allocator::default();
        let out = CodeGenOutput::new(&alloc);
        let mut html = String::new();
        let mut parent = make_parent();
        let text = TextNode {
            start: 0,
            end: 5,
            is_entity: false,
            is_whitespace_only: false,
        };
        process_text(&text, "hello", &mut html, &mut parent, false, true, &out);

        // HTML buffer still populated
        assert_eq!(html, "hello");
        // But no text_parts recorded (no sibling interpolations)
        assert!(parent.text_parts.is_empty());
    }

    #[test]
    fn write_html_false_skips_html_but_still_records_text_part() {
        // Once a coalesced DOM text run is established as dynamic, static
        // text within it must NOT be written to `html` (the run collapses
        // to one caller-emitted space instead) but still needs its
        // `_setText` text part recorded.
        let alloc = Allocator::default();
        let out = CodeGenOutput::new(&alloc);
        let mut html = String::from("<p> ");
        let mut parent = make_parent();
        let text = TextNode {
            start: 0,
            end: 5,
            is_entity: false,
            is_whitespace_only: false,
        };
        process_text(&text, "hello", &mut html, &mut parent, true, false, &out);

        assert_eq!(html, "<p> ", "write_html=false must not touch html");
        assert_eq!(parent.text_parts.len(), 1);
        assert_eq!(parent.text_parts[0].to_js(), "\"hello\"");
    }

    #[test]
    fn text_with_newline_escapes_in_part() {
        let alloc = Allocator::default();
        let out = CodeGenOutput::new(&alloc);
        let mut html = String::new();
        let mut parent = make_parent();
        let source = "line1\nline2";
        let text = TextNode {
            start: 0,
            end: source.len() as u32,
            is_entity: false,
            is_whitespace_only: false,
        };
        process_text(&text, source, &mut html, &mut parent, true, true, &out);

        assert_eq!(html, "line1\nline2");
        assert_eq!(parent.text_parts[0].to_js(), "\"line1\\nline2\"");
    }
}
