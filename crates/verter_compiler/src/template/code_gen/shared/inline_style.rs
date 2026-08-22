//! Thin wrapper over the sole CSS declaration-list parse entry point
//! (`verter_css_syntax::parse_inline_style_declarations`) for VDOM/SSR
//! inline `style="..."` codegen readers (VDOM's `emit_static_style_object`,
//! SSR's `css_to_js_object`).
//!
//! This module does no CSS-declaration scanning of its own — it calls the
//! shared parser exactly once and slices the caller's own string using the
//! returned local spans.

use verter_css_syntax::parse_inline_style_declarations;
use verter_span::Span;

/// Parse an inline `style="..."` attribute value into its property/value
/// text pairs, in source order (duplicate property names preserved —
/// callers decide "last wins" cascade semantics themselves).
pub(crate) fn parse_style_declarations(style: &str) -> Vec<(&str, &str)> {
    parse_inline_style_declarations(style)
        .into_iter()
        .map(|decl| {
            (
                slice(style, decl.name_span()),
                slice(style, decl.value_span()),
            )
        })
        .collect()
}

fn slice(source: &str, span: Span) -> &str {
    &source[span.start as usize..span.end as usize]
}
