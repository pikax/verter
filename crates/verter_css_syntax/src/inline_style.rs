//! The sole declaration-list parse entry point for a standalone inline
//! `style="..."` attribute value (or any equivalent bare
//! `prop: value; prop2: value2` text with no enclosing selector).
//!
//! Every reader that needs the property/value pairs of an inline `style`
//! attribute — VDOM/SSR static-style codegen, the analysis-side static
//! `--*` variable extractor — calls [`parse_inline_style_declarations`]
//! instead of hand-scanning for `;`/`:` bytes. A hand-rolled scan cannot
//! tell a statement-separating `;` from one inside a quoted string
//! (`content: "a;b"; color: red;`); this entry point routes through the
//! same grammar every other CSS-family reader uses, where a string is a
//! single opaque token regardless of what punctuation it contains.

use std::sync::Arc;

use verter_span::Span;

use crate::dialect::CssDialect;
use crate::parser::{CssParseMode, CssSource};
use crate::style_ir::{parse_style_ir, ComponentValue, StyleCompleteness, StyleStatement};
use crate::token::TokenKind;

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Per-thread count of [`parse_inline_style_declarations`] executions —
    /// the routing half of the "exactly one shared parse" proof (A17/A20).
    /// Compiled only under `test-support` (a consumer dev-dependency edge —
    /// `#[cfg(test)]` alone cannot serve a cross-crate integration test), so
    /// production builds carry neither the TLS nor the increment.
    static PARSE_INVOCATIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The number of [`parse_inline_style_declarations`] executions performed on
/// the CALLING thread. Test/guard observability only — see the thread-local's
/// doc.
#[cfg(any(test, feature = "test-support"))]
#[must_use]
pub fn parse_inline_style_declarations_thread_invocations() -> u64 {
    PARSE_INVOCATIONS.with(std::cell::Cell::get)
}

/// A single declaration parsed from a standalone inline CSS declaration
/// list, with spans LOCAL to the exact `&str` passed to
/// [`parse_inline_style_declarations`] — offset `0` is that input's first
/// byte, not an SFC-absolute position.
#[derive(Debug, Clone)]
pub struct InlineStyleDeclaration {
    name_span: Span,
    value_span: Span,
}

impl InlineStyleDeclaration {
    /// Trimmed span of the property (or `--custom-property`) name.
    pub const fn name_span(&self) -> Span {
        self.name_span
    }

    /// Trimmed span of the value text.
    pub const fn value_span(&self) -> Span {
        self.value_span
    }
}

/// Parse a standalone inline `style="..."` attribute value into its
/// complete declarations, in source order, keeping duplicate property names
/// (callers that need "last wins" cascade semantics apply that themselves —
/// this entry point only parses, it does not adjudicate CSS cascade rules).
///
/// Malformed/incomplete declarations are simply absent from the result, the
/// same tolerance the hand-rolled `split(';')`/`find(':')` scanners this
/// supersedes already had.
pub fn parse_inline_style_declarations(style_value: &str) -> Vec<InlineStyleDeclaration> {
    #[cfg(any(test, feature = "test-support"))]
    PARSE_INVOCATIONS.with(|count| count.set(count.get().saturating_add(1)));

    // Wrap the bare declaration list in a synthetic rule block so the shared
    // grammar parses its statements as `StyleStatement::Declaration` (a bare
    // top-level declaration outside any block is otherwise attempted as a
    // qualified-rule selector). The same wrapping technique already backs
    // `parse_selector_authority`'s selector-list-only parse.
    const PREFIX: &str = "x{";
    let wrapped = format!("{PREFIX}{style_value}}}");
    let Ok(prefix_len) = u32::try_from(PREFIX.len()) else {
        return Vec::new();
    };

    let Ok(source) = CssSource::new(Arc::from(wrapped), 0) else {
        return Vec::new();
    };
    let Ok(ir) = parse_style_ir(source.clone(), CssDialect::Css, CssParseMode::Recover) else {
        return Vec::new();
    };

    let Some(rule) = ir
        .statements()
        .iter()
        .find_map(|statement| match statement {
            StyleStatement::Rule(rule) => Some(rule),
            _ => None,
        })
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for statement in rule.body().statements() {
        let StyleStatement::Declaration(decl) = statement else {
            continue;
        };
        if decl.completeness() != StyleCompleteness::Complete {
            continue;
        }
        let Some(value_span) = trim_value_span(&source, decl.value().values()) else {
            continue;
        };
        out.push(InlineStyleDeclaration {
            name_span: local_span(decl.name_span(), prefix_len),
            value_span: local_span(value_span, prefix_len),
        });
    }
    out
}

fn local_span(span: Span, prefix_len: u32) -> Span {
    Span::new(
        span.start.saturating_sub(prefix_len),
        span.end.saturating_sub(prefix_len),
    )
}

fn trim_value_span(source: &CssSource, values: &[ComponentValue]) -> Option<Span> {
    let first = values.iter().find(|value| !is_trivia(value))?;
    let last = values.iter().rfind(|value| !is_trivia(value))?;
    Some(trim_span(
        source,
        Span::new(first.span().start, last.span().end),
    ))
}

fn is_trivia(value: &ComponentValue) -> bool {
    matches!(value, ComponentValue::Token(token) if token.kind() == TokenKind::Whitespace)
}

fn trim_span(source: &CssSource, span: Span) -> Span {
    let text = source.slice(span);
    let start = text.len() - text.trim_start().len();
    let end = text.trim_end().len();
    Span::new(
        span.start
            .saturating_add(u32::try_from(start).unwrap_or(u32::MAX)),
        span.start
            .saturating_add(u32::try_from(end).unwrap_or(u32::MAX)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl_text<'a>(source: &'a str, decl: &InlineStyleDeclaration) -> (&'a str, &'a str) {
        (
            &source[decl.name_span().start as usize..decl.name_span().end as usize],
            &source[decl.value_span().start as usize..decl.value_span().end as usize],
        )
    }

    #[test]
    fn parses_plain_declarations_in_source_order() {
        let source = "color: red; font-size: 14px";
        let decls = parse_inline_style_declarations(source);
        let texts: Vec<_> = decls.iter().map(|d| decl_text(source, d)).collect();
        assert_eq!(texts, vec![("color", "red"), ("font-size", "14px")]);
    }

    #[test]
    fn quoted_semicolon_inside_value_does_not_split_the_declaration() {
        let source = r#"content: "a;b"; color: red;"#;
        let decls = parse_inline_style_declarations(source);
        let texts: Vec<_> = decls.iter().map(|d| decl_text(source, d)).collect();
        assert_eq!(texts, vec![("content", "\"a;b\""), ("color", "red")]);
    }

    #[test]
    fn empty_input_yields_no_declarations() {
        assert!(parse_inline_style_declarations("").is_empty());
    }

    #[test]
    fn duplicate_property_names_are_both_returned_in_order() {
        let source = "color: red; color: blue";
        let decls = parse_inline_style_declarations(source);
        let texts: Vec<_> = decls.iter().map(|d| decl_text(source, d)).collect();
        assert_eq!(texts, vec![("color", "red"), ("color", "blue")]);
    }
}
