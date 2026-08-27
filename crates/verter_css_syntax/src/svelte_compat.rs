//! Svelte compatibility validation profile — projects the first
//! `svelte@5.56.10` CSS body-parse reject code from a single
//! [`StyleSyntaxIr`] parse.
//!
//! The official `element.js` calls `read_style` — which PARSES the `<style>`
//! CSS body and can THROW — BEFORE `if (current.css) e.style_duplicate(start)`.
//! Verter's official-reject gate reserves a probe at that position;
//! [`style_body_reject_code`] fills it from the SAME shared grammar the Svelte
//! CSS pipeline uses (`parse_style_ir`), never a second private CSS parser.
//! A body that projects CLEAN returns `None`, so the caller's later
//! `style_duplicate` (or unsupported-`<style>` rail) wins.
//!
//! The projection consumes parser-minted facts only: diagnostics, recovered
//! statement shape, [`CompoundTail`], [`SvelteNthArg`], functional-pseudo
//! emptiness, and an unpaired `<!--` CDO token recorded on the IR. It does
//! not re-lex argument or tail bytes, and it does not scan the carrier for
//! `</style>` — the caller supplies the parser-minted content [`Span`]. The
//! Svelte-lenient matchers (`nth_of_len_at`, the trailing-identifier
//! [`Lexer`]) run at parse time when those facts are minted, never from
//! reject projection.

use std::sync::Arc;

use verter_span::Span;

use crate::diagnostic::CssDiagnosticKind;
use crate::dialect::CssDialect;
use crate::lexer::{
    ascii_digits_len, codepoint_at, is_js_whitespace_codepoint, IdentifierProfile, Lexer,
    WhitespaceProfile,
};
use crate::parser::{CssParseMode, CssSource};
use crate::selector::{
    ComplexSelectorPart, CompoundTail, PseudoFunctionKind, SelectorComponentKind, SelectorCompound,
    SelectorList, SvelteNthArg,
};
use crate::style_ir::{
    parse_style_ir, StyleCompleteness, StyleStatement, StyleSyntaxIr, UnknownStatementKind,
};
/// Parse the CSS body at `content` through the shared grammar and return the
/// FIRST exact upstream CSS parse code on a body-parse FAILURE, or `None`
/// when the body parses cleanly for Svelte's `read/style.js` race.
#[must_use]
#[allow(dead_code)]
pub(crate) fn style_body_reject_code(source: &str, content: Span) -> Option<&'static str> {
    match parse_style_body(source, content) {
        Ok(ir) => svelte_reject_from_ir(&ir),
        Err(_) => Some("css_expected_identifier"),
    }
}

/// Parse the CSS body at the parser-minted `content` span through
/// [`parse_style_ir`]. `CssSource` construction failure or a structure
/// overflow fails closed as `Err` — never as a silent "no defect".
pub fn parse_style_body(source: &str, content: Span) -> Result<StyleSyntaxIr, CssBodyParseError> {
    let start = usize::try_from(content.start).unwrap_or(source.len());
    let end = usize::try_from(content.end).unwrap_or(source.len());
    let body = source
        .get(start..end)
        .ok_or(CssBodyParseError::Unconstructable)?;
    let css_source = CssSource::new(Arc::from(body), content.start)
        .map_err(|_| CssBodyParseError::Unconstructable)?;
    parse_style_ir(css_source, CssDialect::Css, CssParseMode::Recover)
        .map_err(|_| CssBodyParseError::Unconstructable)
}

/// Why [`parse_style_body`] could not produce an IR. Both variants fail
/// closed at the reject gate (mapped to `css_expected_identifier`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssBodyParseError {
    /// The content span is not a valid `CssSource` (out of range, or the
    /// u32 span domain overflowed).
    Unconstructable,
}

/// Project the first Svelte `read/style.js` reject code from an already
/// parsed [`StyleSyntaxIr`]. Source-order: the earliest spanning defect wins.
#[must_use]
pub fn svelte_reject_from_ir(ir: &StyleSyntaxIr) -> Option<&'static str> {
    let mut first: Option<(u32, &'static str)> = None;
    if let Some(span) = ir.unpaired_cdo_span() {
        consider(&mut first, span, "expected_token");
    }
    for diagnostic in ir.diagnostics() {
        if let Some(code) = map_diagnostic(diagnostic.kind, ir) {
            consider(&mut first, diagnostic.span, code);
        }
    }
    walk_statements(ir.statements(), &mut first);
    first.map(|(_, code)| code)
}

fn consider(first: &mut Option<(u32, &'static str)>, span: Span, code: &'static str) {
    if first.is_none_or(|(at, _)| span.start < at) {
        *first = Some((span.start, code));
    }
}

fn map_diagnostic(kind: CssDiagnosticKind, ir: &StyleSyntaxIr) -> Option<&'static str> {
    match kind {
        CssDiagnosticKind::UnterminatedComment => Some("expected_token"),
        CssDiagnosticKind::UnterminatedString
        | CssDiagnosticKind::BadString
        | CssDiagnosticKind::UnterminatedUrl => Some("unexpected_eof"),
        CssDiagnosticKind::UnterminatedBlock
        | CssDiagnosticKind::ExpectedAtRuleTerminator
        | CssDiagnosticKind::UnexpectedClosingDelimiter
        | CssDiagnosticKind::MismatchedDelimiter => Some("css_expected_identifier"),
        CssDiagnosticKind::ExpectedDeclarationColon => Some("css_empty_declaration"),
        CssDiagnosticKind::ExpectedRuleBlock => {
            if recovered_rule_has_unknown_body(ir.statements()) {
                Some("css_empty_declaration")
            } else {
                Some("css_expected_identifier")
            }
        }
        CssDiagnosticKind::AmbiguousStatement
        | CssDiagnosticKind::InconsistentIndentation
        | CssDiagnosticKind::UnexpectedIndentation
        | CssDiagnosticKind::UnterminatedInterpolation
        | CssDiagnosticKind::BadUrl => None,
    }
}

fn recovered_rule_has_unknown_body(statements: &[StyleStatement]) -> bool {
    statements.iter().any(|statement| match statement {
        StyleStatement::Rule(rule) => {
            rule.completeness() == StyleCompleteness::Recovered
                && rule.body().statements().iter().any(|inner| {
                    matches!(
                        inner,
                        StyleStatement::Unknown(unknown)
                            if unknown.kind() == UnknownStatementKind::Recovery
                    )
                })
                || recovered_rule_has_unknown_body(rule.body().statements())
        }
        StyleStatement::AtRule(atrule) => atrule
            .body()
            .is_some_and(|body| recovered_rule_has_unknown_body(body.statements())),
        StyleStatement::Declaration(declaration) => declaration
            .body()
            .is_some_and(|body| recovered_rule_has_unknown_body(body.statements())),
        StyleStatement::MixinOrFunction(rule) => rule
            .body()
            .is_some_and(|body| recovered_rule_has_unknown_body(body.statements())),
        StyleStatement::Unknown(_) => false,
    })
}

fn walk_statements(statements: &[StyleStatement], first: &mut Option<(u32, &'static str)>) {
    for statement in statements {
        match statement {
            StyleStatement::Rule(rule) => {
                walk_selector_list(rule.selector_list(), first);
                walk_statements(rule.body().statements(), first);
            }
            StyleStatement::AtRule(atrule) => {
                if let Some(body) = atrule.body() {
                    walk_statements(body.statements(), first);
                }
            }
            StyleStatement::Declaration(declaration) => {
                if let Some(body) = declaration.body() {
                    walk_statements(body.statements(), first);
                }
            }
            StyleStatement::MixinOrFunction(rule) => {
                if let Some(body) = rule.body() {
                    walk_statements(body.statements(), first);
                }
            }
            StyleStatement::Unknown(_) => {}
        }
    }
}

fn walk_selector_list(list: &SelectorList, first: &mut Option<(u32, &'static str)>) {
    for selector in list.selectors() {
        if let Some(ComplexSelectorPart::Combinator(combinator)) = selector.parts().last() {
            // Use the combinator span, not the whole selector: an earlier
            // nested reject (`:global()`, `2n+`) must still win.
            consider(first, combinator.span(), "css_selector_invalid");
        }
        for part in selector.parts() {
            if let ComplexSelectorPart::Compound(compound) = part {
                inspect_compound(compound, first);
            }
        }
    }
}

fn inspect_compound(compound: &SelectorCompound, first: &mut Option<(u32, &'static str)>) {
    if let CompoundTail::Unclassified {
        span,
        expected_identifier: true,
        ..
    } = compound.tail()
    {
        consider(first, span, "css_expected_identifier");
    }
    for component in compound.components() {
        if let Some(pseudo) = component.pseudo() {
            if matches!(
                pseudo.kind(),
                PseudoFunctionKind::NthChild | PseudoFunctionKind::NthLastChild
            ) {
                match pseudo.svelte_nth_arg() {
                    Some(SvelteNthArg::Empty | SvelteNthArg::LeadingHyphenOrDigit) => {
                        consider(first, pseudo.argument_span(), "css_expected_identifier");
                    }
                    Some(SvelteNthArg::Other) => {
                        consider(first, pseudo.argument_span(), "css_selector_invalid");
                    }
                    Some(SvelteNthArg::Formula | SvelteNthArg::TrailingIdentifier) | None => {}
                }
            } else if component.kind() == SelectorComponentKind::FunctionalPseudo
                && pseudo.argument_is_empty()
            {
                // Official `read_selector_list` skips trivia then still
                // requires a selector, for ANY parenthesized pseudo.
                // `:lang(en)` / `:foo(.a)` have argument content; `:lang( )`
                // / `:global(/**/)` do not. Paren-less `:hover` is PseudoClass.
                consider(first, pseudo.argument_span(), "css_expected_identifier");
            }
            if let Some(nested) = pseudo.selector_list() {
                walk_selector_list(nested, first);
            }
        }
    }
}

/// Parse-time classification of a `:nth-child` / `:nth-last-child` argument.
/// Called from the selector sink when the pseudo node is built; reject
/// projection reads the stored [`SvelteNthArg`] and never calls this.
pub(crate) fn classify_svelte_nth_arg(source: &CssSource, arg: Span) -> SvelteNthArg {
    if arg.start >= arg.end {
        return SvelteNthArg::Empty;
    }
    if nth_consumes_arg_or_of(source, arg) {
        return SvelteNthArg::Formula;
    }
    if svelte_trailing_type_selector_span(source, arg).is_some() {
        return SvelteNthArg::TrailingIdentifier;
    }
    if leading_hyphen_or_digit(source, arg) {
        SvelteNthArg::LeadingHyphenOrDigit
    } else {
        SvelteNthArg::Other
    }
}

/// Parse-time emptiness of a functional-pseudo argument. Called from the
/// selector sink; reject projection reads [`SelectorPseudo::argument_is_empty`].
pub(crate) fn classify_argument_is_empty(
    source: &CssSource,
    arg: Span,
    selector_list: Option<&SelectorList>,
) -> bool {
    if arg.start >= arg.end {
        return true;
    }
    match selector_list {
        Some(list) => list
            .selectors()
            .iter()
            .all(|selector| selector.compounds().is_empty()),
        None => argument_is_trivia_only(source, arg),
    }
}

/// Parse-time `css_expected_identifier` shape for an unclassified compound
/// tail. Called from [`crate::selector`] when the compound node is built;
/// reject projection reads the stored `expected_identifier` flag.
pub(crate) fn svelte_unclassified_expected_identifier(
    source: &CssSource,
    span: Span,
    starts_with_dot: bool,
) -> bool {
    starts_with_dot
        || leading_hyphen_or_digit(source, span)
        || unclassified_delim_requires_identifier(source, span)
}

fn argument_is_trivia_only(source: &CssSource, arg: Span) -> bool {
    let mut text = source.slice(arg).to_string();
    while let Some(start) = text.find("/*") {
        match text[start + 2..].find("*/") {
            Some(rel) => text.replace_range(start..start + 2 + rel + 2, ""),
            None => {
                text.replace_range(start.., "");
                break;
            }
        }
    }
    svelte_trim_js_whitespace(&text).is_empty()
}

fn nth_consumes_arg_or_of(source: &CssSource, arg: Span) -> bool {
    let Some(matched) = svelte_nth_of_selector_span(source, arg) else {
        return false;
    };
    if matched.end == arg.end {
        return true;
    }
    svelte_trim_js_whitespace(source.slice(matched)).ends_with("of")
}

fn unclassified_delim_requires_identifier(source: &CssSource, span: Span) -> bool {
    matches!(
        svelte_trim_js_whitespace(source.slice(span))
            .as_bytes()
            .first(),
        Some(b'@' | b'#')
    )
}

fn leading_hyphen_or_digit(source: &CssSource, span: Span) -> bool {
    let bytes = source.slice(span).as_bytes();
    let mut j = 0usize;
    if bytes.first() == Some(&b'-') {
        j = 1;
    }
    matches!(bytes.get(j), Some(b) if b.is_ascii_digit())
}

/// The byte length of a `\d+(\.\d+)?%` match at `start` in `src`, or `None`. The digit-run scans
/// are the same shared primitive ([`crate::lexer::ascii_digits_len`]) the general lexer's
/// number/dimension tokenizing uses. See the module doc for why this stays a dedicated grammar
/// routine rather than a general `Lexer::consume_number` call (that grammar additionally allows a
/// sign and an exponent, which this narrower upstream regex does not). The SOLE percentage-shape
/// authority: parse-time [`CompoundTail`] minting and
/// [`svelte_percentage_selector_span`] both call this, never a
/// second independently-derived matcher. Reject projection reads the stored
/// tail fact and never calls this.
fn percentage_len_at(src: &[u8], start: usize) -> Option<usize> {
    let mut j = start + ascii_digits_len(src, start);
    if j == start {
        return None; // need ≥1 digit
    }
    if src.get(j) == Some(&b'.') {
        let frac_start = j + 1;
        let frac_len = ascii_digits_len(src, frac_start);
        if frac_len > 0 {
            j = frac_start + frac_len; // a fractional part requires ≥1 digit
        }
    }
    if src.get(j) == Some(&b'%') {
        Some(j + 1 - start)
    } else {
        None
    }
}

/// The byte length of a `REGEX_NTH_OF` match at `start` in `src`, or `None`. Models
/// `(even|odd|\+?(\d+|\d*n(\s*[+-]\s*\d+)?)|-\d*n(\s*\+\s*\d+))((?=\s*[,)])|\s+of(\s+|(?=[.#[*:&])))`:
/// the leading An+B alternation, then upstream's trailing alternation — the `\s*[,)]` end
/// LOOKAHEAD (zero-width, the selector-list / pseudo-args terminator) OR the CONSUMING
/// `\s+of(\s+|(?=[.#[*:&]))` arm (so `2n+1 of .x` / `2n of.x` match through `of` and the
/// following selector reads through the normal loop). The `of` arm is tried only when the
/// lookahead fails — matching the regex alternation's left-to-right order.
///
/// The An+B alternation has THREE faithfully-distinct arms (upstream's `even` / `odd` / positive
/// `\+?(...)` / negative `-\d*n(...)`):
/// - `even` / `odd` keywords.
/// - POSITIVE `\+?(\d+|\d*n(\s*[+-]\s*\d+)?)` — an OPTIONAL leading `+`, then EITHER a plain `\d+`
///   (no `n`) OR `\d*n` (optional digits, `n`) with an OPTIONAL `±` offset.
/// - NEGATIVE `-\d*n(\s*\+\s*\d+)` — a MANDATORY leading `-`, then `\d*n` (optional digits, `n`),
///   then a MANDATORY `\s*\+\s*\d+` offset (a `+` offset ONLY — `-` is not in this arm).
///
/// So a bare leading-`-` form `-2` (no `n`), `-2n` (no offset), or `-2n-1` (a `-` offset) is NOT
/// an nth match (it falls through the selector loop to `read_identifier`, which rejects a
/// digit-leading `-?\d` as `css_expected_identifier`), while `-2n+1` / `-n+2` ARE the negative
/// arm. A generic-optional-sign reader would over-accept `-2` / `-2n` / `-2n-1` and emit no
/// reject — diverging from pinned `svelte@5.56.10`. The SOLE nth-of-shape authority: see
/// [`percentage_len_at`]'s sibling note.
fn nth_of_len_at(src: &[u8], start: usize) -> Option<usize> {
    let rest = &src[start..];
    let j = if rest.starts_with(b"even") {
        4
    } else if rest.starts_with(b"odd") {
        3
    } else if rest.first() == Some(&b'-') {
        // NEGATIVE arm `-\d*n(\s*\+\s*\d+)`: `-`, optional digits, MANDATORY `n`, MANDATORY
        // `\s*\+\s*\d+` (`+` offset only).
        nth_negative_arm_len(rest)?
    } else {
        // POSITIVE arm `\+?(\d+|\d*n(\s*[+-]\s*\d+)?)`.
        nth_positive_arm_len(rest)?
    };
    // trailing alternation `((?=\s*[,)])|\s+of\s+)`, tried left-to-right (JS `\s` — Unicode):
    // (1) the zero-width end lookahead `\s*[,)]` — return `j` WITHOUT consuming the ws.
    // End-of-span is the same lookahead: `argument_span` / a compound span does
    // not include the closing `)` of `:nth-*(…)`, and a delimited slice is
    // already bounded.
    let k = skip_js_whitespace(rest, j);
    if matches!(rest.get(k), Some(b',' | b')')) || k == rest.len() {
        return Some(j);
    }
    // (2) the CONSUMING `\s+of(\s+|(?=[.#[*:&]))` arm — ≥1 JS whitespace, the
    // literal `of`, then EITHER ≥1 whitespace OR a zero-width lookahead at a
    // simple-selector start (`.`, `#`, `[`, `*`, `:`, `&`). Minifiers omit the
    // space after `of`. On match, return the byte length through the trailing
    // whitespace (or through `of` when the lookahead arm fires) so the
    // `<selector>` after `of` is read by the normal selector loop.
    let m = skip_js_whitespace(rest, j);
    if m == j {
        return None; // `\s+` needs ≥1 whitespace before `of`
    }
    if !rest[m..].starts_with(b"of") {
        return None;
    }
    let after_of = skip_js_whitespace(rest, m + 2);
    if after_of != m + 2 {
        return Some(after_of);
    }
    if matches!(
        rest.get(m + 2),
        Some(b'.' | b'#' | b'[' | b'*' | b':' | b'&')
    ) {
        return Some(m + 2);
    }
    None
}

/// The byte length of the POSITIVE An+B arm `\+?(\d+|\d*n(\s*[+-]\s*\d+)?)` at the start of
/// `rest`, or `None`. An OPTIONAL leading `+`; then EITHER a plain `\d+` (one-or-more digits, NO
/// `n`) OR `\d*n` (zero-or-more digits then `n`) with an OPTIONAL `\s*[+-]\s*\d+` offset (a `+`
/// OR `-` offset). Returns `None` when neither alternative matches (e.g. a bare `+`, or `+`
/// followed by a non-digit non-`n`).
fn nth_positive_arm_len(rest: &[u8]) -> Option<usize> {
    let mut j = 0usize;
    if rest.first() == Some(&b'+') {
        j += 1;
    }
    let dstart = j;
    j += ascii_digits_len(rest, j);
    if rest.get(j) == Some(&b'n') {
        // `\d*n` (the leading digits are OPTIONAL here) + an OPTIONAL `\s*[+-]\s*\d+` offset.
        j += 1;
        if let Some(off) = nth_offset_len(rest, j, true) {
            j = off;
        }
        Some(j)
    } else if j > dstart {
        // a plain `\d+` (one-or-more digits, no `n`).
        Some(j)
    } else {
        None // neither `\d+` nor `\d*n` — not a positive nth match
    }
}

/// The byte length of the NEGATIVE An+B arm `-\d*n(\s*\+\s*\d+)` at the start of `rest` (which
/// must begin with `-`), or `None`. A MANDATORY `-`; `\d*` (optional digits); a MANDATORY `n`;
/// then a MANDATORY `\s*\+\s*\d+` offset — a `+` offset ONLY (a `-` offset, a missing `n`, or a
/// missing offset is NOT this arm, so `-2`, `-2n`, and `-2n-1` all return `None`).
fn nth_negative_arm_len(rest: &[u8]) -> Option<usize> {
    verter_debug_assert_eq!(rest.first(), Some(&b'-'));
    let mut j = 1usize; // the leading `-`
    j += ascii_digits_len(rest, j);
    if rest.get(j) != Some(&b'n') {
        return None; // the `n` is mandatory
    }
    j += 1;
    // a MANDATORY `+`-only offset (`plus_or_minus = false`).
    nth_offset_len(rest, j, false)
}

/// The byte length up to and including a `\s*<sign>\s*\d+` An+B offset starting at byte index
/// `from` in `rest`, or `None` when no offset is present. `plus_or_minus` selects the offset sign
/// set: `true` accepts `+` OR `-` (the positive arm's `[+-]`), `false` accepts `+` ONLY (the
/// negative arm's `\+`). An offset requires the sign AND ≥1 trailing digit; a sign with no digit
/// (e.g. `2n+`) is not a match.
fn nth_offset_len(rest: &[u8], from: usize, plus_or_minus: bool) -> Option<usize> {
    // `\s*<sign>\s*\d+` — the `\s*` runs are JS `\s` (Unicode).
    let mut k = skip_js_whitespace(rest, from);
    let sign_ok = match rest.get(k) {
        Some(b'+') => true,
        Some(b'-') => plus_or_minus,
        _ => false,
    };
    if !sign_ok {
        return None;
    }
    k = skip_js_whitespace(rest, k + 1);
    let ds = k;
    k += ascii_digits_len(rest, k);
    if k > ds {
        Some(k)
    } else {
        None // a sign with no trailing digit is not an offset
    }
}

/// Svelte's keyframe-step PERCENTAGE selector shape (`REGEX_PERCENTAGE = /\d+(\.\d+)?%/y`) at the
/// START of `span`'s raw source text — the typed classification a Svelte-side selector-compound
/// PROJECTION reads when the shared selector grammar recognized no typed
/// [`SelectorComponent`](crate::selector::SelectorComponent) inside a
/// [`SelectorCompound`](crate::selector::SelectorCompound) (an unrecognized token run is exactly
/// what a keyframe step selector like `50%` produces today: the general CSS Syntax Module
/// selector grammar this crate implements has no percentage-selector production, so the compound
/// closes with zero components and its raw span is all that is available). `Some` carries the
/// exact matched span (a PREFIX of `span` when trailing bytes remain unmatched — the caller
/// compares it against `span` to decide whether the WHOLE compound was consumed); `None` when
/// `span`'s text does not start with a percentage token. Reuses [`percentage_len_at`] — the SAME
/// authority the Svelte reject projection uses; this crate never maintains two
/// independently-derived percentage matchers.
#[must_use]
pub fn svelte_percentage_selector_span(source: &CssSource, span: Span) -> Option<Span> {
    let text = source.slice(span);
    let len = percentage_len_at(text.as_bytes(), 0)?;
    Some(Span::new(span.start, span.start + u32::try_from(len).ok()?))
}

/// Svelte's lenient nth-formula pseudo-class-ARGUMENT shape (`REGEX_NTH_OF`) at the START of
/// `span`'s raw source text (typically a pseudo's
/// [`SelectorPseudo::argument_span`](crate::selector::SelectorPseudo::argument_span)) — upstream
/// accepts this SHAPE inside ANY pseudo-class's arguments (not only `:nth-child`/
/// `:nth-last-child`, which this crate's own [`selector`](crate::selector) module already gives a
/// structured `NthExpression` for), producing an opaque nth-formula simple-selector rather than
/// attempting to parse the argument as a selector list. `Some` carries the exact matched span
/// (INCLUDING a consumed ` of ` arm, exactly as upstream — a Svelte-side projection compares it
/// against `span` to decide whether a `<selector>` remains after the ` of `); `None` when
/// `span`'s text does not start with an nth-formula token (a normal selector-list argument, e.g.
/// `:not(.a, .b)`). Reuses [`nth_of_len_at`] — the SAME authority parse-time
/// [`classify_svelte_nth_arg`] / [`CompoundTail::NthOf`] minting uses. Reject
/// projection reads those stored facts and never calls this.
#[must_use]
pub fn svelte_nth_of_selector_span(source: &CssSource, span: Span) -> Option<Span> {
    let text = source.slice(span);
    let len = nth_of_len_at(text.as_bytes(), 0)?;
    Some(Span::new(span.start, span.start + u32::try_from(len).ok()?))
}

/// Svelte's lenient "a bare identifier immediately following another simple
/// selector, with no combinator" TYPE-SELECTOR shape (`read_selector`'s
/// unconditional `read_identifier` fallback production, e.g. the `div` in
/// `:global(.x)div`) — classifies whether an ALREADY-DELIMITED span (the
/// compound's own trailing byte run left UNCLAIMED once the shared
/// selector grammar's compound-parsing loop stops recognizing further
/// components) is a single, complete CSS identifier. General CSS3 requires
/// a type/universal selector to be FIRST in a compound, so this shared
/// crate's own `parse_selector_list` never builds a typed
/// [`SelectorComponent`](crate::selector::SelectorComponent) out of a
/// trailing identifier in that position — the compound simply closes with
/// the trailing bytes unclassified in its raw span, exactly like the
/// percentage/nth-of gaps above. Reuses the SAME identifier-recognition
/// authority this module's own `read_identifier` reader step already
/// applies (`consume_name_profiled` under [`IdentifierProfile::SvelteCompat`]
/// / [`WhitespaceProfile::JsUnicode`], plus the same leading-hyphen-or-digit
/// rejection) — never a second, independently-derived identifier matcher.
///
/// `Some(span)` only when `span`'s ENTIRE text is consumed as one
/// identifier (no leftover, no partial match) — unlike the percentage/
/// nth-of prefix classifiers, a partial match here is NOT the lenient
/// implicit-type-selector shape (an unclaimed trailing run that is only
/// PART identifier, part something else, is not what upstream's read loop
/// would have produced as a bare type-selector token) and returns `None`.
#[must_use]
pub fn svelte_trailing_type_selector_span(source: &CssSource, span: Span) -> Option<Span> {
    if span.start >= span.end {
        return None;
    }
    let text = source.slice(span);
    let bytes = text.as_bytes();
    // `REGEX_LEADING_HYPHEN_OR_DIGIT = /-?\d/y` — a leading `-?\d` is never
    // an identifier (the same rejection `read_identifier` applies).
    let mut j = 0usize;
    if bytes.first() == Some(&b'-') {
        j += 1;
    }
    if matches!(bytes.get(j), Some(b) if b.is_ascii_digit()) {
        return None;
    }
    let local_start = span.start.checked_sub(source.origin())?;
    let mut lexer = Lexer::new(source, CssDialect::Css);
    lexer.seek(usize::try_from(local_start).ok()?);
    let start = lexer.position();
    lexer.consume_name_profiled(
        IdentifierProfile::SvelteCompat,
        WhitespaceProfile::JsUnicode,
    );
    let end = lexer.position();
    (end > start && end == span.end).then_some(Span::new(start, end))
}

/// The JS `String.prototype.trim()` set — identical to the JS `\s` set (WhiteSpace ∪
/// LineTerminator): INCLUDES U+FEFF, EXCLUDES U+0085. Rust `str::trim` (Unicode `White_Space`)
/// diverges on exactly those two, so the official `value.trim()` routes here.
fn trim_js_whitespace(s: &str) -> &str {
    s.trim_matches(|c: char| is_js_whitespace_codepoint(c as u32))
}

/// Public re-export of [`trim_js_whitespace`] for Svelte-side text reconstruction (e.g. an
/// at-rule's trimmed prelude text, reconstructed from the already-delimited
/// [`crate::style_ir::ComponentValueTree`]/raw-span text `verter_css_syntax` itself parsed) that
/// needs the SAME JS-whitespace trim rule upstream's `read_value`/`read_at_rule` use, rather than
/// Rust's `str::trim` (which diverges on U+FEFF / U+0085).
#[must_use]
pub fn svelte_trim_js_whitespace(s: &str) -> &str {
    trim_js_whitespace(s)
}

/// Advance `k` past the JS-`\s` whitespace run in `rest` (a valid-UTF-8 suffix of the source; `k`
/// on a char boundary), decoding codepoints — the `\s*` / `\s+` scans of `REGEX_NTH_OF` are
/// Unicode-aware.
fn skip_js_whitespace(rest: &[u8], mut k: usize) -> usize {
    while let Some((cp, len)) = codepoint_at(rest, k) {
        if !is_js_whitespace_codepoint(cp) {
            break;
        }
        k += len;
    }
    k
}

/// Svelte's `read_value` TEXT RECONSTRUCTION — the trimmed, comment-stripped
/// (except inside an unquoted, case-SENSITIVE `url(...)`), quote-respecting
/// raw text of a declaration value OR at-rule prelude, with backslash
/// escapes RE-ENCODED as `\` + the following character (not decoded) — the
/// official `Declaration.value` / `Atrule.prelude` text.
///
/// This runs over an ALREADY-DELIMITED span the shared grammar produced
/// (a [`StyleDeclaration::value`](crate::style_ir::StyleDeclaration::value)
/// or [`StyleDirective::opaque_args`](crate::style_ir::StyleDirective::opaque_args)
/// tree's [`ComponentValueTree::span`](crate::style_ir::ComponentValueTree::span))
/// — it reproduces official `read_value`'s OUTPUT from a boundary the ONE
/// shared parse already found, never a second boundary search: the `;` /
/// `{` / `}` stop check below is retained only as the defensive no-op it is
/// for a well-formed span (those bytes occur, if at all, only inside a
/// quote or an unquoted `url(...)`, where the state machine already treats
/// them as ordinary content) — a well-formed input never actually breaks
/// out of the loop through that arm.
///
/// The general CSS tokenizer recognizes `url(` CASE-INSENSITIVELY and folds
/// an unquoted url into one opaque token; upstream's own `in_url` state
/// switches on a literal, case-SENSITIVE `value.ends_with("url")` byte
/// check instead (so `URL(...)` does NOT enter upstream's url mode and its
/// embedded comments ARE stripped, while `url(...)` does and its embedded
/// comments are NOT). This reconstruction re-derives that exact distinction
/// from the raw span text — a bounded TEXT transform of an already-known
/// region, not new CSS structural classification (the same class of work as
/// [`svelte_percentage_selector_span`] / [`svelte_nth_of_selector_span`]
/// above).
#[must_use]
pub fn svelte_read_value_text(source: &CssSource, span: Span) -> String {
    let text = source.slice(span);
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0usize;
    let mut escaped = false;
    let mut in_url = false;
    let mut quote_mark: Option<u8> = None;
    while index < bytes.len() {
        let ch = bytes[index];
        if escaped {
            out.push('\\');
            push_whole_char(text, &mut index, &mut out);
            escaped = false;
            continue;
        } else if ch == b'\\' {
            escaped = true;
            index += 1;
            continue;
        } else if Some(ch) == quote_mark {
            quote_mark = None;
        } else if ch == b')' {
            in_url = false;
        } else if quote_mark.is_none() && (ch == b'"' || ch == b'\'') {
            quote_mark = Some(ch);
        } else if ch == b'(' && out.ends_with("url") {
            in_url = true;
        } else if (ch == b';' || ch == b'{' || ch == b'}') && !in_url && quote_mark.is_none() {
            // A well-formed already-delimited span never reaches this arm —
            // see the doc comment above.
            break;
        } else if ch == b'/'
            && !in_url
            && quote_mark.is_none()
            && bytes.get(index + 1) == Some(&b'*')
        {
            // Skip the `/* … */` comment span entirely (upstream's inline
            // comment-skip: the comment contributes nothing to `value`).
            index = match text[index + 2..].find("*/") {
                Some(rel) => index + 2 + rel + 2,
                None => bytes.len(),
            };
            continue;
        }
        push_whole_char(text, &mut index, &mut out);
    }
    svelte_trim_js_whitespace(&out).to_string()
}

/// Append the whole char at byte offset `*index` in `text` to `out` and
/// advance `*index` past it (by its UTF-8 byte length) — steps whole scalars;
/// character is never split.
fn push_whole_char(text: &str, index: &mut usize, out: &mut String) {
    if let Some(c) = text[*index..].chars().next() {
        out.push(c);
        *index += c.len_utf8();
    } else {
        *index += 1;
    }
}

/// The at-rule prelude's or a declaration value's FIRST SIGNIFICANT value
/// span (skipping comments and trivia tokens) — e.g. Svelte's keyframe-name
/// token span (`@keyframes <name> { … }` / `@keyframes /* c */ <name> {
/// … }`). Read as a TYPED fact off the already-parsed
/// [`ComponentValueTree`](crate::style_ir::ComponentValueTree) rather than a
/// positional byte scan over raw source text: the shared parser already
/// tokenized the prelude/value into a typed value list, so "the first
/// non-trivia value" is a lookup over that list, not new classification.
/// `None` for an empty (or entirely trivia/comment) tree.
#[must_use]
pub fn svelte_first_significant_value_span(
    tree: &crate::style_ir::ComponentValueTree,
) -> Option<Span> {
    use crate::style_ir::ComponentValue;
    tree.values().iter().find_map(|value| match value {
        ComponentValue::Comment(_) => None,
        ComponentValue::Token(token) if token.kind().is_trivia() => None,
        other => Some(other.span()),
    })
}
