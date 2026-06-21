//! Unit tests for the faithful `read/style.js` CSS body reader port.
//!
//! Each expectation is grounded against the pinned `svelte@5.56.3` compiler (a `None` here ⇔ the
//! pinned compiler ACCEPTS the style body / reports `style_duplicate` for a duplicate race; a
//! `Some(code)` ⇔ the pinned compiler throws that exact CSS parse code at `read_style`). The
//! reader is fed the WHOLE component source + the CSS body's content-start so a nested reader can
//! run past `</style>` exactly as upstream does.

use super::css_body_parse_error;

/// The byte offset just past the FIRST `<style>`/`<style …>` open tag's `>` in `source` — the CSS
/// content start the parser would reserve. Panics if there is no `<style …>` open tag.
fn first_style_content_start(source: &str) -> usize {
    let open = source.find("<style").expect("a <style> tag");
    let gt = source[open..].find('>').expect("the <style> open tag's >");
    open + gt + 1
}

/// The CSS parse code the reader reports for the FIRST `<style>` body in `source`.
fn code(source: &str) -> Option<&'static str> {
    css_body_parse_error(source, first_style_content_start(source))
}

// ── clean bodies parse cleanly (no defect ⇒ duplicate / unsupported-style rail wins) ──

#[test]
fn empty_body_is_clean() {
    assert_eq!(code("<style></style>"), None);
}

#[test]
fn whitespace_only_body_is_clean() {
    assert_eq!(code("<style>   \n  </style>"), None);
}

#[test]
fn simple_rule_is_clean() {
    assert_eq!(code("<style>.a { color: red; }</style>"), None);
}

#[test]
fn rule_with_empty_block_is_clean() {
    assert_eq!(code("<style>.a {}</style>"), None);
}

#[test]
fn multiple_rules_and_at_rule_are_clean() {
    assert_eq!(
        code(
            "<style>.a, .b { color: red }\n@media (min-width: 1px) { .c { color: blue } }</style>"
        ),
        None
    );
}

#[test]
fn nth_child_an_b_of_selector_is_clean() {
    // `:nth-child(2n+1 of .x)` — upstream `REGEX_NTH_OF` includes the `\s+of\s+` arm, so the
    // `<An+B> of <selector>` form PARSES (the `.x` selector reads through the normal loop).
    // Grounded: pinned svelte@5.56.3 ACCEPTS this CSS.
    assert_eq!(code("<style>p:nth-child(2n+1 of .x) {}</style>"), None);
}

#[test]
fn nth_child_even_of_selector_is_clean() {
    // The `even` keyword form before the `of` arm also parses clean.
    assert_eq!(code("<style>p:nth-child(even of .x) {}</style>"), None);
}

#[test]
fn nth_child_an_b_terminator_lookahead_is_clean() {
    // The OTHER trailing alternative — the `\s*[,)]` end lookahead — still parses a plain
    // `:nth-child(2n+1)` clean (no `of`). NEGATIVE control that the `of` arm did not regress the
    // lookahead branch.
    assert_eq!(code("<style>p:nth-child(2n+1) {}</style>"), None);
}

// ── An+B negative-form discrimination (REGEX_NTH_OF two-branch structure) ──
// `REGEX_NTH_OF` (`read/style.js:10`) is
// `(even|odd|\+?(\d+|\d*n(\s*[+-]\s*\d+)?)|-\d*n(\s*\+\s*\d+))((?=\s*[,)])|\s+of\s+)`. The leading
// `-` form is matched ONLY by the negative branch `-\d*n(\s*\+\s*\d+)`, which REQUIRES an `n` AND
// a `+`-only offset: a bare `-2`, `-2n`, or `-2n-1` does NOT match the nth grammar, so the
// selector loop falls through to `read_identifier`, whose leading-`-?\d` guard rejects the
// digit-leading negatives → `css_expected_identifier`. A digit-leading `nth_of_len` over-accept
// would consume `-2` as an nth match and emit NO defect — these rows discriminate that bug
// (they parse clean against the pre-fix `nth_of_len`). Grounded against pinned svelte@5.56.3:
// `(-2)` / `(-2n)` / `(-2n-1)` all throw `css_expected_identifier`.
#[test]
fn nth_child_bare_negative_integer_reports_css_expected_identifier() {
    assert_eq!(
        code("<style>p:nth-child(-2) {}</style>"),
        Some("css_expected_identifier")
    );
}

#[test]
fn nth_child_bare_negative_an_reports_css_expected_identifier() {
    assert_eq!(
        code("<style>p:nth-child(-2n) {}</style>"),
        Some("css_expected_identifier")
    );
}

#[test]
fn nth_child_negative_an_minus_b_reports_css_expected_identifier() {
    // `-2n-1` — the negative branch allows ONLY a `+` offset, so the `-1` tail is not part of an
    // nth match; the leading `-2` reaches `read_identifier` and rejects.
    assert_eq!(
        code("<style>p:nth-child(-2n-1) {}</style>"),
        Some("css_expected_identifier")
    );
}

#[test]
fn nth_child_negative_an_plus_b_is_clean() {
    // `-2n+1` IS the negative branch `-\d*n\s*\+\s*\d+` — upstream ACCEPTS it. Positive control
    // pairing the rejected `-2n-1` above (only the offset sign differs).
    assert_eq!(code("<style>p:nth-child(-2n+1) {}</style>"), None);
}

#[test]
fn nth_child_negative_n_is_clean_identifier() {
    // `-n` is NOT an nth match (the negative branch requires `\s*\+\s*\d+`), but `-n` IS a valid
    // CSS identifier (leading `-` then a LETTER, not a digit), so `read_identifier` accepts it —
    // upstream ACCEPTS `:nth-child(-n)`. This is the boundary the digit-leading reject must NOT
    // over-reach: `-n` clean, `-2n` rejected.
    assert_eq!(code("<style>p:nth-child(-n) {}</style>"), None);
}

#[test]
fn nth_child_negative_n_plus_b_is_clean() {
    // `-n+2` IS the negative branch (zero digits before `n`, a `+` offset) — upstream ACCEPTS it.
    assert_eq!(code("<style>p:nth-child(-n+2) {}</style>"), None);
}

#[test]
fn custom_property_empty_value_is_clean() {
    // A `--custom:` declaration with an empty value is NOT `css_empty_declaration` (the `--`
    // prefix exemption in upstream `read_declaration`).
    assert_eq!(code("<style>.a { --x: ; }</style>"), None);
}

#[test]
fn pseudo_class_and_attribute_selectors_are_clean() {
    assert_eq!(
        code("<style>a:hover[href^=\"http\"] { color: red }</style>"),
        None
    );
}

#[test]
fn comment_in_body_is_clean() {
    assert_eq!(code("<style>/* c */ .a { color: red }</style>"), None);
}

#[test]
fn html_comment_in_body_is_clean() {
    // A terminated `<!-- … -->` comment in the CSS body is skipped by
    // `allow_comment_or_whitespace` (the required `eat('-->', true)` succeeds).
    assert_eq!(code("<style><!-- c --> .a { color: red }</style>"), None);
}

// ── unterminated comments require the close token (expected_token) ──
// Upstream `allow_comment_or_whitespace` uses REQUIRED close tokens (`eat('*/', true)` /
// `eat('-->', true)`): an unterminated CSS comment is `expected_token` AT the read_style parse
// entry (BEFORE `style_duplicate`). Grounded against pinned svelte@5.56.3.

#[test]
fn unterminated_block_comment_reports_expected_token() {
    // `<style>/*</style>…`: the `/*` runs (past `</style>`) to end-of-input with no `*/`, so the
    // required `eat('*/', true)` fails → `expected_token`.
    assert_eq!(
        code("<style>/*</style>\n<button onclick={() => c++}>{c}</button>\n"),
        Some("expected_token"),
    );
}

#[test]
fn unterminated_html_comment_reports_expected_token() {
    // `<style><!--</style>…`: the `<!--` runs to end-of-input with no `-->`, so the required
    // `eat('-->', true)` fails → `expected_token`.
    assert_eq!(
        code("<style><!--</style>\n<button onclick={() => c++}>{c}</button>\n"),
        Some("expected_token"),
    );
}

// ── a CSS reader running off the end of the source reports unexpected_eof ──
// Upstream's nested CSS readers (`read_value`, `read_attribute_value`) loop on
// `parser.index < parser.template.length` and call `e.unexpected_eof(template.length)` when they
// reach the END of the source mid-construct. For a TOP-LEVEL `<style>` this is reachable as the
// WINNING reject code ONLY when the `<style>` is properly CLOSED (so the parser records NO
// unterminated-raw-block strict error — which would otherwise be `css_expected_identifier` at an
// EARLIER encounter order and pre-empt this) yet its CSS body opens an UNTERMINATED QUOTE that
// SWALLOWS the literal `</style>` text, so the reader runs PAST the close to true EOF (a quote
// closes only on a matching quote, never on markup). A SCAFFOLDED body whose reader does NOT
// swallow `</style>` stops at the close and surfaces a different code; a BARE unterminated
// `<style>` is the unterminated-raw-block strict error (`css_expected_identifier`), NOT this. So
// each fixture here CLOSES the `<style>` and opens a quote/value that swallows the close.
// Grounded against pinned svelte@5.56.3 (each ⇒ `unexpected_eof`); the parse-parity corpus's
// `read_style` `single_unexpected_eof_*` rows assert the SAME shapes through the full gate.

/// The CSS code the reader reports for `frag` (a CLOSED-`<style>` fragment) appended after the
/// §1.2-core scaffold prefix, feeding `css_body_parse_error` the whole source so the nested reader
/// runs past `</style>` exactly as upstream's `read_style` does.
fn swallow_code(frag: &str) -> Option<&'static str> {
    let src = format!("<script>let c = $state(0);</script>\n{frag}");
    css_body_parse_error(&src, first_style_content_start(&src))
}

#[test]
fn declaration_value_open_quote_swallows_close_reports_unexpected_eof() {
    // `read_value` opens a `"` that swallows `</style>`; the value reader then runs to EOF.
    assert_eq!(
        swallow_code("<style>.a { content: \"x</style>"),
        Some("unexpected_eof"),
    );
}

#[test]
fn attribute_selector_value_open_quote_swallows_close_reports_unexpected_eof() {
    // `read_attribute_value` opens a `"` that swallows `</style>`; the attribute value reader runs
    // to EOF.
    assert_eq!(
        swallow_code("<style>a[x=\"y</style>"),
        Some("unexpected_eof"),
    );
}

#[test]
fn bare_unterminated_style_is_the_raw_block_strict_error_not_unexpected_eof() {
    // NEGATIVE control — the EOF rows above are NOT a blanket "any `<style>` EOF ⇒ unexpected_eof".
    // A BARE unterminated `<style>.a ` (no `</style>`) is flagged by the parser as an unterminated
    // RAW BLOCK (`css_expected_identifier`) at an earlier encounter order, which is what the full
    // gate reports — NOT `unexpected_eof`. (The isolated `css_body_parse_error` port can report
    // `unexpected_eof` for the same bytes, but the WINNING gate code is the raw-block strict
    // error; the corpus fixtures therefore use the CLOSED-`<style>` swallow shapes above.)
    let bare = "<script>let c = $state(0);</script>\n<style>.a ";
    let parsed = crate::svelte::parser::parse_svelte(bare);
    let gate = crate::svelte::runtime::official_reject_gate(bare, &parsed).map(|r| r.official_code);
    assert_eq!(gate, Some("css_expected_identifier"));
}

// ── malformed bodies report the exact upstream code ──

#[test]
fn unterminated_block_reports_css_expected_identifier() {
    // `.b {` with content after `</style>` — the unterminated block runs past `</style>` and the
    // reader eventually requires an identifier where there is none. (Grounded: the pinned
    // compiler reports `css_expected_identifier` for this scaffolded form.)
    assert_eq!(
        code("<style>.b {</style>\n<button onclick={() => c++}>{c}</button>\n"),
        Some("css_expected_identifier"),
    );
}

#[test]
fn lone_dot_selector_reports_css_expected_identifier() {
    assert_eq!(
        code("<style>.</style>\n<button onclick={() => c++}>{c}</button>\n"),
        Some("css_expected_identifier"),
    );
}

#[test]
fn leading_digit_selector_reports_css_expected_identifier() {
    // `1px {}` — a TypeSelector identifier starting with a digit is `css_expected_identifier`.
    assert_eq!(
        code("<style>1px {}</style>\n<button>x</button>\n"),
        Some("css_expected_identifier"),
    );
}

#[test]
fn global_open_paren_reports_css_expected_identifier() {
    assert_eq!(
        code("<style>:global(</style>\n<button onclick={() => c++}>{c}</button>\n"),
        Some("css_expected_identifier"),
    );
}

#[test]
fn bare_at_rule_reports_css_expected_identifier() {
    // `@` with no name — `read_at_rule` reads an identifier and finds none.
    assert_eq!(
        code("<style>@</style>\n<button onclick={() => c++}>{c}</button>\n"),
        Some("css_expected_identifier"),
    );
}

#[test]
fn declaration_without_value_reports_css_empty_declaration() {
    // `.b { color }` — a declaration with no `:`/value (and not a `--` custom prop) is
    // `css_empty_declaration`.
    assert_eq!(
        code("<style>.b { color }</style>\n<button>x</button>\n"),
        Some("css_empty_declaration"),
    );
}

// ── the body is read relative to content_start within the FULL source ──

#[test]
fn reader_uses_content_start_not_an_isolated_slice() {
    // Two styles in one source: the SECOND style's malformed body is read from its own
    // content-start and runs past its `</style>` into the trailing template. The reader for the
    // 2nd style reports the CSS code; the 1st (clean) reports None.
    let src =
        "<style>.a {}</style>\n<style>.b {</style>\n<button onclick={() => c++}>{c}</button>\n";
    let first_start = first_style_content_start(src);
    assert_eq!(
        css_body_parse_error(src, first_start),
        None,
        "1st style clean"
    );
    // content-start of the 2nd style:
    let second_open = src[first_start..].find("<style").unwrap() + first_start;
    let second_gt = src[second_open..].find('>').unwrap();
    let second_start = second_open + second_gt + 1;
    assert_eq!(
        css_body_parse_error(src, second_start),
        Some("css_expected_identifier"),
        "2nd style malformed body reports the CSS code"
    );
}
