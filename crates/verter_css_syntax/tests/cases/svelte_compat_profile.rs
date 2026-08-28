//! Unit tests for the Svelte reject projection over [`StyleSyntaxIr`].
//!
//! Each expectation is grounded against the pinned `svelte@5.56.10` compiler (a `None` here ⇔ the
//! pinned compiler ACCEPTS the style body / reports `style_duplicate` for a duplicate race; a
//! `Some(code)` ⇔ the pinned compiler throws that exact CSS parse code at `read_style`). The
//! projection is fed the parser-minted body span (open `>` through `</style` or EOF) — the same
//! span Verter's Svelte tokenizer records on `StyleBodyProbe`.
//!
//! One case (`bare_unterminated_style_is_the_raw_block_strict_error_not_unexpected_eof`)
//! exercised the FULL Svelte official-reject gate rather than this isolated reader and stayed in
//! `verter_compiler`'s own test suite (`svelte_parse_defect_exact_codes.rs`) — it needs the
//! compiler's parser + gate, which this crate does not and must not depend on.

use verter_span::Span;

use crate::svelte_compat::style_body_reject_code;

/// The parser-minted CSS body span of the FIRST `<style>` in `source`.
fn first_style_content_span(source: &str) -> Span {
    let start = first_style_content_start(source);
    let rest = &source[start..];
    let end = start + rest.find("</style").unwrap_or(rest.len());
    Span::new(start as u32, end as u32)
}

fn first_style_content_start(source: &str) -> usize {
    let open = source.find("<style").expect("a <style> tag");
    let gt = source[open..].find('>').expect("the <style> open tag's >");
    open + gt + 1
}

/// The CSS parse code the projection reports for the FIRST `<style>` body in `source`.
fn code(source: &str) -> Option<&'static str> {
    style_body_reject_code(source, first_style_content_span(source))
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
    // `:nth-child(2n+1 of .x)` — upstream `REGEX_NTH_OF` includes the `\s+of(\s+|(?=[.#[*:&]))`
    // arm, so the `<An+B> of <selector>` form PARSES (the `.x` selector reads through the
    // normal loop). Grounded: pinned svelte@5.56.10 ACCEPTS this CSS.
    assert_eq!(code("<style>p:nth-child(2n+1 of .x) {}</style>"), None);
}

#[test]
fn nth_child_of_without_whitespace_before_simple_selector_is_clean() {
    // Pinned svelte@5.56.10 `REGEX_NTH_OF` allows `of` followed immediately by a simple-selector
    // start (`.`, `#`, `[`, `*`, `:`, `&`) with no whitespace. A `\s+of\s+` matcher wrongly
    // rejects `2n of.x` as `css_expected_identifier`.
    assert_eq!(code("<style>p:nth-child(2n of.x) {}</style>"), None);
    assert_eq!(code("<style>p:nth-child(2n of#x) {}</style>"), None);
    assert_eq!(code("<style>p:nth-child(2n of[x]) {}</style>"), None);
    assert_eq!(code("<style>p:nth-child(2n of*) {}</style>"), None);
    assert_eq!(code("<style>p:nth-child(2n of:x) {}</style>"), None);
    assert_eq!(code("<style>p:nth-child(2n of&) {}</style>"), None);
}

#[test]
fn nth_child_of_without_whitespace_before_of_is_not_the_of_arm() {
    // `2nof.x` is one dimension token; official REGEX_NTH_OF requires `\s+` before `of`.
    assert_eq!(
        code("<style>p:nth-child(2nof.x) {}</style>"),
        Some("css_expected_identifier"),
    );
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

// ── JS-`\s` (Unicode) whitespace parity in the reject reader ──
// The reject reader mirrors `read/style.js`, whose `\s`-bearing scans (`REGEX_NTH_OF`, the
// declaration-property `/[\s:]/`, the unquoted-attribute close `/[\s\]]/`, the `value.trim()`, and
// the unicode-escape trailing `(\r\n|\s)?`) all use JS `\s` (Unicode: NBSP + the other Unicode
// spaces). A byte-ASCII scan in THIS reject reader can wrongly REJECT a body the pinned compiler
// ACCEPTS. The `REGEX_NTH_OF` site is the OBSERVABLE divergence (the nth grammar is strictly
// validated, so a missed Unicode space falls through to the digit-leading identifier reject); the
// other swept sites parse clean both ways in the reject-only contract, so they carry no separate
// (non-discriminating) case here. This whitespace set is DELIBERATELY distinct from the crate's
// own general [`verter_css_syntax::lexer`], which uses the ASCII-only CSS Syntax Module set — the
// two must not be unified, or this parity breaks.
#[test]
fn nth_child_an_b_offset_with_nbsp_is_clean_like_svelte() {
    // NBSP (U+00A0) around the `+` offset — svelte's `\s*[+-]\s*` matches it; a byte-ASCII scan
    // misses the offset, the selector loop reads `2n␠…` as a digit-leading identifier and throws
    // `css_expected_identifier`. Oracle-confirmed: svelte@5.56.10 compiles `p:nth-child(2n␠+␠1)` to
    // `p.svelte-…:nth-child(2n + 1){…}` (no throw). Clean here iff the reject reader is
    // codepoint-aware — RED (`Some("css_expected_identifier")`) against a byte-ASCII scan.
    assert_eq!(
        code("<style>p:nth-child(2n\u{a0}+\u{a0}1) {}</style>"),
        None
    );
    // The `\s+of\s+` arm with NBSP separators is likewise clean.
    assert_eq!(
        code("<style>p:nth-child(2n\u{a0}of\u{a0}.x) {}</style>"),
        None
    );
}

// ── UTF-8 boundary safety in the reject reader (the readers step whole chars, not bytes) ──
// The reject reader mirrors `read/style.js`, which iterates JS string CHARACTERS. Verter's readers
// must step whole UTF-8 scalars and build the value with whole chars — a byte-step (`index += 1`)
// or a byte→char cast (`push(byte as char)`) either PANICS on a multibyte char (`codepoint_at` on
// a continuation byte / a mid-char `value[len-3..]` slice) or corrupts the value. Oracle-grounded
// against pinned svelte@5.56.10 (which ACCEPTS the first three and REJECTS the NBSP-only value).

#[test]
fn unquoted_attribute_value_with_non_ascii_char_is_clean_not_a_panic() {
    // svelte@5.56.10 accepts + scopes `[data-x=café]` and `[lang=中文]`. A byte-advancing
    // `read_attribute_value` lands `codepoint_at` on a UTF-8 continuation byte → char-boundary
    // panic. Clean iff the reader steps whole chars.
    assert_eq!(code("<style>[data-x=café]{color:red}</style>"), None);
    assert_eq!(code("<style>[lang=中文]{color:red}</style>"), None);
}

#[test]
fn declaration_value_with_non_ascii_before_paren_is_clean_not_a_panic() {
    // svelte@5.56.10 accepts `a{color:é(foo)}` (marks it unused). The `url(` lookbehind must use
    // `ends_with("url")` (not a byte-index slice `value[len-3..]`, which panics mid-char) and the
    // value must accumulate whole chars (not `byte as char`).
    assert_eq!(code("<style>a{color:é(foo)}</style>"), None);
}

// ── An+B negative-form discrimination (REGEX_NTH_OF two-branch structure) ──
// `REGEX_NTH_OF` (`read/style.js:10`) is
// `(even|odd|\+?(\d+|\d*n(\s*[+-]\s*\d+)?)|-\d*n(\s*\+\s*\d+))((?=\s*[,)])|\s+of\s+)`. The leading
// `-` form is matched ONLY by the negative branch `-\d*n(\s*\+\s*\d+)`, which REQUIRES an `n` AND
// a `+`-only offset: a bare `-2`, `-2n`, or `-2n-1` does NOT match the nth grammar, so the
// selector loop falls through to `read_identifier`, whose leading-`-?\d` guard rejects the
// digit-leading negatives → `css_expected_identifier`. A digit-leading `nth_of_len` over-accept
// would consume `-2` as an nth match and emit NO defect — these rows discriminate that bug
// (they parse clean against the pre-fix `nth_of_len`). Grounded against pinned svelte@5.56.10:
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
// entry (BEFORE `style_duplicate`). Grounded against pinned svelte@5.56.10.

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
// `<style>` is the unterminated-raw-block strict error (`css_expected_identifier`), NOT this — see
// `verter_compiler`'s `svelte_parse_defect_exact_codes.rs` for that FULL-GATE negative control,
// which needs the Svelte parser + official-reject gate this crate does not depend on. So each
// fixture here CLOSES the `<style>` and opens a quote/value that swallows the close. Grounded
// against pinned svelte@5.56.10 (each ⇒ `unexpected_eof`); the parse-parity corpus's `read_style`
// `single_unexpected_eof_*` rows assert the SAME shapes through the full gate.

/// The CSS code the reader reports for `frag` (a CLOSED-`<style>` fragment) appended after the
/// §1.2-core scaffold prefix, feeding [`style_body_reject_code`] the whole source so the nested
/// reader runs past `</style>` exactly as upstream's `read_style` does.
fn swallow_code(frag: &str) -> Option<&'static str> {
    let src = format!("<script>let c = $state(0);</script>\n{frag}");
    style_body_reject_code(&src, first_style_content_span(&src))
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
fn at_rule_with_empty_name_and_block_reports_css_expected_identifier() {
    assert_eq!(
        code("<style>@ {}</style>\n<button>x</button>\n"),
        Some("css_expected_identifier"),
    );
}

#[test]
fn empty_global_parens_report_css_expected_identifier() {
    assert_eq!(
        code("<style>:global() {}</style>\n<button>x</button>\n"),
        Some("css_expected_identifier"),
    );
}

#[test]
fn empty_global_parens_with_whitespace_report_css_expected_identifier() {
    assert_eq!(
        code("<style>:global( ) {}</style>\n<button>x</button>\n"),
        Some("css_expected_identifier"),
    );
}

#[test]
fn empty_global_parens_with_comment_report_css_expected_identifier() {
    assert_eq!(
        code("<style>:global(/**/) {}</style>\n<button>x</button>\n"),
        Some("css_expected_identifier"),
    );
}

#[test]
fn unknown_functional_pseudos_with_arguments_stay_clean() {
    // `:lang(en)` / `:dir(rtl)` / `:foo(.a)` are functional but NOT selector-list
    // pseudos: the shared parser leaves `selector_list` absent. That must not be
    // treated as trivia-only-empty the way `:global( )` is.
    assert_eq!(code("<style>:lang(en) {}</style>"), None);
    assert_eq!(code("<style>:dir(rtl) {}</style>"), None);
    assert_eq!(code("<style>:foo(.a) {}</style>"), None);
}

#[test]
fn trivia_only_unknown_functional_pseudo_reports_css_expected_identifier() {
    // Official `read_selector_list` still requires a selector after skipping
    // trivia, for ANY parenthesized pseudo — including unknown `:lang` / `:foo`.
    assert_eq!(
        code("<style>:lang( ) {}</style>\n<button>x</button>\n"),
        Some("css_expected_identifier"),
    );
    assert_eq!(
        code("<style>:lang(/**/) {}</style>\n<button>x</button>\n"),
        Some("css_expected_identifier"),
    );
}

#[test]
fn dangling_combinator_reports_css_selector_invalid() {
    assert_eq!(
        code("<style>.a > {}</style>\n<button>x</button>\n"),
        Some("css_selector_invalid"),
    );
}

#[test]
fn dangling_plus_combinator_reports_css_selector_invalid() {
    assert_eq!(
        code("<style>.a + {}</style>\n<button>x</button>\n"),
        Some("css_selector_invalid"),
    );
}

#[test]
fn empty_global_then_dangling_combinator_reports_css_expected_identifier() {
    // Official svelte fails inside `:global()` first and never reaches the
    // outer dangling-combinator check. The combinator candidate must not win
    // just because it was recorded with the whole selector span.
    assert_eq!(
        code("<style>:global() > {}</style>\n<button>x</button>\n"),
        Some("css_expected_identifier"),
    );
}

#[test]
fn nth_dangling_plus_then_combinator_reports_css_expected_identifier() {
    assert_eq!(
        code("<style>p:nth-child(2n+) > {}</style>\n<button>x</button>\n"),
        Some("css_expected_identifier"),
    );
}

#[test]
fn empty_id_selector_reports_css_expected_identifier() {
    assert_eq!(
        code("<style># {}</style>\n<button>x</button>\n"),
        Some("css_expected_identifier"),
    );
}

#[test]
fn nth_child_dangling_plus_after_an_reports_css_expected_identifier() {
    assert_eq!(
        code("<style>p:nth-child(2n+){}</style>\n<button>x</button>\n"),
        Some("css_expected_identifier"),
    );
}

#[test]
fn nth_child_dangling_plus_after_n_reports_css_selector_invalid() {
    assert_eq!(
        code("<style>p:nth-child(n+){}</style>\n<button>x</button>\n"),
        Some("css_selector_invalid"),
    );
}

#[test]
fn nth_last_child_dangling_plus_after_an_reports_css_expected_identifier() {
    assert_eq!(
        code("<style>p:nth-last-child(2n+){}</style>\n<button>x</button>\n"),
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
fn reader_uses_content_span_not_an_isolated_guess() {
    // Two styles in one source: the SECOND style's malformed body is projected from its own
    // parser-minted content span. The 1st (clean) reports None; the 2nd reports the CSS code.
    let src =
        "<style>.a {}</style>\n<style>.b {</style>\n<button onclick={() => c++}>{c}</button>\n";
    assert_eq!(
        style_body_reject_code(src, first_style_content_span(src)),
        None,
        "1st style clean"
    );
    let first_end = first_style_content_span(src).end as usize;
    let second_open = src[first_end..].find("<style").unwrap() + first_end;
    let second_gt = src[second_open..].find('>').unwrap();
    let second_start = second_open + second_gt + 1;
    let second_rest = &src[second_start..];
    let second_end = second_start + second_rest.find("</style").unwrap_or(second_rest.len());
    assert_eq!(
        style_body_reject_code(src, Span::new(second_start as u32, second_end as u32)),
        Some("css_expected_identifier"),
        "2nd style malformed body reports the CSS code"
    );
}
