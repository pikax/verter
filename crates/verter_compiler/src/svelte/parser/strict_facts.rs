//! The STRICT-PARSE fact recovery helpers for the Svelte tokenizer.
//!
//! Verter's tokenizer is intentionally infallible / recovery-based: it never panics and
//! always emits a faithful tree, even on malformed markup. Each recovery point the
//! official `svelte@5.56.3` STRICT parser rejects MUST push a typed
//! [`SvelteStrictParseError`] so the official-reject gate fails closed (no `Main`)
//! instead of accepting a divergent module. These named helpers are the SINGLE family
//! every such recovery routes through — the static guard
//! `svelte_parser_recovery_routes_through_strict_facts` asserts no new recovery
//! primitive bypasses a `record_` call. Housed in a sibling module (a second `impl`
//! block on the parser) so the tokenizer file stays within the source-size budget.

use verter_span::Span;

use super::template_ast::{SvelteStrictParseError, SvelteStrictParseErrorKind};
use super::tokenizer::SvelteParser;

impl SvelteParser<'_> {
    /// Record a STRICT-PARSE error at a recovery point official rejects — the SINGLE
    /// sink every named strict-fact helper routes through. The `official_code` is
    /// derived from the kind so the refusal pins the exact official diagnostic code.
    /// `pub(super)` so a recovery site that selects its kind dynamically (e.g. the
    /// comment-EOF reader, whose empty-vs-started distinction picks `unexpected_eof` vs
    /// `expected_token`) can record UNCONDITIONALLY through one sink call.
    pub(super) fn record_strict_parse_error(
        &mut self,
        kind: SvelteStrictParseErrorKind,
        span: Span,
    ) {
        // Drawn from the SAME monotonic defect counter as the close-tag rail so the two
        // streams share one global discovery order — the official-reject gate arbitrates
        // by minimum `encounter_order`, matching official (first parse error wins).
        let encounter_order = self.next_defect_seq();
        self.strict_parse_errors.push(SvelteStrictParseError {
            kind,
            span,
            official_code: kind.official_code(),
            encounter_order,
        });
    }

    /// Strict fact: a `<` that does not begin a valid tag name (`<` in text, `<.`, `<{`)
    /// — official `tag_invalid_name`. The recovery emits the `<` as literal text.
    pub(super) fn record_tag_invalid_name(&mut self, span: Span) {
        self.record_strict_parse_error(SvelteStrictParseErrorKind::TagInvalidName, span);
    }

    /// Strict fact: a close tag carrying a trailing token before `>` (`</div x>`), an
    /// unterminated open tag, or an unterminated comment — official `expected_token`.
    pub(super) fn record_expected_token(&mut self, span: Span) {
        self.record_strict_parse_error(SvelteStrictParseErrorKind::ExpectedToken, span);
    }

    /// Strict fact: an attribute `=` with no following value (`id=`) — official
    /// `expected_attribute_value`.
    pub(super) fn record_empty_attribute_value(&mut self, span: Span) {
        self.record_strict_parse_error(SvelteStrictParseErrorKind::ExpectedAttributeValue, span);
    }

    /// Strict fact: a nameless close tag (`</>`) — official `element_invalid_closing_tag`.
    pub(super) fn record_nameless_close(&mut self, span: Span) {
        self.record_strict_parse_error(SvelteStrictParseErrorKind::ElementInvalidClosingTag, span);
    }

    /// Strict fact: an element / raw block left open at EOF, or a raw-block close carrying
    /// a trailing token (the close is not recognised) — official `element_unclosed`.
    pub(super) fn record_element_unclosed(&mut self, span: Span) {
        self.record_strict_parse_error(SvelteStrictParseErrorKind::ElementUnclosed, span);
    }

    /// Strict fact: an end of input reached mid-construct (an unterminated quoted
    /// attribute value, a `<` at EOF, a `</` at EOF) — official `unexpected_eof`.
    pub(super) fn record_unexpected_eof(&mut self, span: Span) {
        self.record_strict_parse_error(SvelteStrictParseErrorKind::UnexpectedEof, span);
    }

    /// Strict fact: a top-level `<style>` left unterminated (the CSS reader reaches EOF
    /// inside a rule) — official `css_expected_identifier`.
    pub(super) fn record_css_expected_identifier(&mut self, span: Span) {
        self.record_strict_parse_error(SvelteStrictParseErrorKind::CssExpectedIdentifier, span);
    }
}
