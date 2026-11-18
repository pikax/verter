//! Text and attribute encodings of server output.
//!
//! Server output is HTML inside JavaScript template literals, so authored
//! text is decoded once, HTML-escaped, then escaped for the literal. Entity
//! decoding, whitespace condensing and literal escaping are the shared ones;
//! only what is specific to server output lives here.

use crate::template::code_gen::shared::helpers::escape_template_literal_into;
use crate::template::vue_syntax::escape_js_string_into;

/// Escape text for HTML: `&`, `<`, `>`, `"` and `'`, as `escapeHtml` of
/// `@vue/shared` does.
pub(crate) fn escape_html(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    push_escaped_html(&mut result, s);
    result
}

fn push_escaped_html(buf: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '&' => buf.push_str("&amp;"),
            '<' => buf.push_str("&lt;"),
            '>' => buf.push_str("&gt;"),
            '"' => buf.push_str("&quot;"),
            '\'' => buf.push_str("&#39;"),
            _ => buf.push(ch),
        }
    }
}

/// Authored text as it appears inside a push literal: entities decoded,
/// whitespace condensed unless the text is preformatted, HTML special
/// characters re-encoded, literal delimiters escaped.
pub(crate) fn escape_ssr_text(s: &str, preserve_whitespace: bool) -> String {
    let text = crate::template::vue_syntax::normalize_template_text(s, preserve_whitespace);
    escape_template_literal(&escape_html(&text))
}

/// Escape text for a JavaScript template literal.
pub(crate) fn escape_template_literal(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    escape_template_literal_into(&mut result, s);
    result
}

/// Escape text for a double-quoted JavaScript string literal.
pub(crate) fn escape_js_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    escape_js_string_into(&mut result, s);
    result
}

/// An HTML attribute name as a JavaScript object key: hyphenated names are
/// quoted.
pub(crate) fn html_attr_to_js_key(attr: &str) -> String {
    if attr.contains('-') {
        format!("\"{}\"", attr)
    } else {
        attr.to_string()
    }
}

/// A CSS inline style as a JavaScript object literal with kebab-case keys,
/// as server rendering expects: `color: red; margin-left: 20px` →
/// `{"color":"red","margin-left":"20px"}`. Declarations come from the
/// shared declaration-list parser, never from a split on `;`.
pub(crate) fn css_to_js_object(css: &str) -> String {
    let mut result = String::from("{");
    let mut first = true;
    for (prop, value) in
        crate::template::code_gen::shared::inline_style::parse_style_declarations(css)
    {
        if prop.is_empty() || value.is_empty() {
            continue;
        }
        if !first {
            result.push(',');
        }
        first = false;
        result.push('"');
        result.push_str(prop);
        result.push_str("\":\"");
        escape_js_string_into(&mut result, value);
        result.push('"');
    }
    result.push('}');
    result
}

/// Attributes whose presence alone is their value.
pub(crate) fn is_boolean_html_attr(attr: &str) -> bool {
    matches!(
        attr,
        "disabled"
            | "checked"
            | "selected"
            | "readonly"
            | "required"
            | "multiple"
            | "autofocus"
            | "autoplay"
            | "controls"
            | "default"
            | "defer"
            | "formnovalidate"
            | "hidden"
            | "ismap"
            | "loop"
            | "muted"
            | "nomodule"
            | "novalidate"
            | "open"
            | "reversed"
            | "scoped"
            | "seamless"
            | "allowfullscreen"
            | "async"
            | "itemscope"
            | "inert"
    )
}

/// A directive name after `v-` as its binding name: `click-outside` →
/// `vClickOutside`.
pub(crate) fn directive_to_camel(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 1);
    result.push('v');
    let mut capitalize_next = true;
    for ch in name.chars() {
        if ch == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
#[path = "escape_tests.rs"]
mod escape_tests;
