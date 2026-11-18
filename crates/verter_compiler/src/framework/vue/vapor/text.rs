//! Static text contributes HTML and, beside interpolation, quoted effect parts.
use oxc_allocator::Allocator;

use super::effect::TextPart;
use super::scope::VaporScope;
use crate::template::code_gen::shared::helpers;
use crate::template::vue_syntax::{escape_js_string_into, needs_js_escaping};

pub(super) fn process_text<'c>(
    content: &str,
    html: &mut String,
    parent: &mut VaporScope<'c>,
    has_interpolation: bool,
    write_html: bool,
    allocator: &'c Allocator,
    preserve_whitespace: bool,
) {
    if write_html {
        if !preserve_whitespace {
            helpers::condense_whitespace_into(html, content);
        } else {
            html.push_str(content);
        }
    }
    let normalized =
        crate::template::vue_syntax::normalize_template_text(content, preserve_whitespace);
    let content = normalized.as_ref();
    if has_interpolation && !content.is_empty() {
        let mut literal = String::with_capacity(content.len() + 4);
        literal.push('"');
        if needs_js_escaping(content) {
            escape_js_string_into(&mut literal, content);
        } else {
            literal.push_str(content);
        }
        literal.push('"');
        parent
            .text_parts
            .push(TextPart::Static(allocator.alloc_str(&literal)));
    }
}
