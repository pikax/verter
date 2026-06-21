//! PARSER-STRICTNESS PARITY — Verter's forgiving parser must FAIL CLOSED on the
//! malformed markup the official `svelte@5.56.3` STRICT parser rejects.
//!
//! Verter's tokenizer is intentionally infallible / recovery-based: it never panics,
//! emitting a faithful tree even on malformed input. That recovery is correct for the
//! IDE projection (which owns its own error recovery), but for the CLIENT runtime the
//! contract is "Verter emits a Main ⇔ official ACCEPTS the same source". A recovery
//! point that ACCEPTS markup official rejects would emit a divergent `Main` (official:
//! "compile error, no module"; Verter: "module exists") — a behavioral divergence.
//!
//! Every parser recovery point that official rejects pushes a typed
//! [`SvelteStrictParseError`] onto [`ParsedSvelte::strict_parse_errors`]; the
//! official-reject gate refuses (the single [`CoreOfficialValidationRule::ParserStrictness`]
//! rule) BEFORE lowering, so NO `Main` is emitted. This test pins each known malformed
//! input to a fail-closed refusal carrying the exact official error code, and pins the
//! well-formed §1.2 controls to a STILL-emitted `Main` (so the strict gate does not
//! over-reject valid input).

use oxc_allocator::Allocator;
use verter_compiler::svelte::parser::parse_svelte;
use verter_compiler::svelte::runtime::{compile_client, ClientCompileError, SvelteRuntimeOptions};

/// Compile a source through the client backend, returning the typed result.
fn compile(source: &str) -> Result<String, ClientCompileError> {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    compile_client(source, &parsed, &opts, &alloc, false).map(|m| m.code)
}

/// The official error code a `compile_client` refusal carries, or an error string when
/// the input was NOT refused (an emitted `Main` — the accept-invalid leak) or refused
/// through a non-parser-strictness channel.
fn parser_strictness_code(result: &Result<String, ClientCompileError>) -> Result<String, String> {
    match result {
        Ok(js) => Err(format!("emitted a Main (accept-invalid leak):\n{js}")),
        Err(ClientCompileError::OfficialReject(rejection)) => {
            use verter_compiler::svelte::runtime::CoreOfficialValidationRule;
            if rejection.rule == CoreOfficialValidationRule::ParserStrictness {
                Ok(rejection.official_code.to_string())
            } else {
                Err(format!(
                    "refused, but as {:?} (not ParserStrictness)",
                    rejection.rule
                ))
            }
        }
        Err(other) => Err(format!(
            "refused through a non-official-reject channel: {other:?}"
        )),
    }
}

/// Assert a malformed source fails closed (NO `Main`) as a `ParserStrictness` refusal
/// carrying the exact `expected_code`.
fn assert_fails_closed(label: &str, source: &str, expected_code: &str) {
    match parser_strictness_code(&compile(source)) {
        Ok(code) if code == expected_code => {}
        Ok(code) => panic!(
            "{label}: fails closed as ParserStrictness but with code `{code}`, expected `{expected_code}`"
        ),
        Err(why) => panic!("{label}: {why}"),
    }
}

/// Assert a source fails closed (NO `Main`) as ANY `OfficialReject` whose
/// `official_code` equals `expected_code`, REGARDLESS of which
/// [`CoreOfficialValidationRule`] variant carries it. Use this — not
/// [`assert_fails_closed`] — for a defect that is a STRUCTURAL RAIL-A reject (an unclosed
/// element ⇒ the dedicated `ElementUnclosed` rule carrying `element_unclosed`): the
/// official CODE is the parity contract, not the internal rule taxonomy. Genuine
/// RAIL-B strict-parse facts (`expected_token`, `tag_invalid_name`, …) keep
/// [`assert_fails_closed`], which additionally pins the `ParserStrictness` rule.
fn assert_fails_closed_with_code(label: &str, source: &str, expected_code: &str) {
    match compile(source) {
        Ok(js) => panic!("{label}: emitted a Main (accept-invalid leak):\n{js}"),
        Err(ClientCompileError::OfficialReject(rejection)) => {
            assert_eq!(
                rejection.official_code, expected_code,
                "{label}: fails closed as {:?} carrying `{}`, expected official code `{expected_code}`",
                rejection.rule, rejection.official_code
            );
        }
        Err(other) => panic!("{label}: refused through a non-official-reject channel: {other:?}"),
    }
}

// ── The parser-leniency leak class: each malformed form MUST fail closed ────────

#[test]
fn close_tag_with_trailing_token_fails_closed_like_official() {
    // `</div x>` — a trailing token in an element close tag: official `expected_token`.
    assert_fails_closed(
        "close_trailing_token",
        "<script>let c = $state(0);</script>\n<div><button onclick={() => c++}>{c}</button></div x>\n",
        "expected_token",
    );
}

#[test]
fn raw_block_close_with_trailing_token_fails_closed_like_official() {
    // `</script x>` — a trailing token in the raw-block close tag: official
    // `element_unclosed` (the close is not recognised, so the script is left open).
    assert_fails_closed(
        "script_close_trailing_token",
        "<script>let c = $state(0);</script x>\n<button onclick={() => c++}>{c}</button>\n",
        "element_unclosed",
    );
}

#[test]
fn empty_attribute_value_fails_closed_like_official() {
    // `<div id=>` — an `=` with no value: official `expected_attribute_value`.
    assert_fails_closed(
        "empty_attr_value",
        "<script>let c = $state(0);</script>\n<div id=></div>\n<button onclick={() => c++}>{c}</button>\n",
        "expected_attribute_value",
    );
}

#[test]
fn empty_script_attribute_value_fails_closed_like_official() {
    // `<script lang=>` — an `=` with no value on a script attribute: official
    // `expected_attribute_value` (the script-domain attribute is parsed the same way).
    assert_fails_closed(
        "empty_script_attr_value",
        "<script lang=>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        "expected_attribute_value",
    );
}

#[test]
fn raw_lt_in_text_fails_closed_like_official() {
    // `a < b` — a raw `<` followed by a non-tag-name byte (a space): official
    // `tag_invalid_name` (the `<` begins a tag whose name is malformed).
    assert_fails_closed(
        "raw_lt_in_text",
        "<script>let c = $state(0);</script>\n<button onclick={() => c++}>a < b {c}</button>\n",
        "tag_invalid_name",
    );
}

#[test]
fn nameless_close_tag_fails_closed_like_official() {
    // `</>` — a close tag with no name: official `element_invalid_closing_tag`.
    assert_fails_closed(
        "nameless_close",
        "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</></button>\n",
        "element_invalid_closing_tag",
    );
}

#[test]
fn lt_bang_text_fails_closed_with_expected_token() {
    // `<!x` in text — official `expected_token` (NOT `tag_invalid_name`): the `<!`
    // markup-declaration lead expects a recognised declaration / comment token.
    assert_fails_closed(
        "lt_bang_text",
        "<script>let c = $state(0);</script>\n<p>a <!x b</p>\n<button onclick={() => c++}>{c}</button>\n",
        "expected_token",
    );
}

#[test]
fn close_with_space_then_name_fails_closed_with_expected_token() {
    // `</ div>` — a close whose `</` is followed by whitespace then a name: official
    // `expected_token` (NOT the nameless `element_invalid_closing_tag`). The `</` opens
    // a close tag and the parser expects the name immediately.
    assert_fails_closed(
        "close_space_then_name",
        "<script>let c = $state(0);</script>\n</ div>\n<button onclick={() => c++}>{c}</button>\n",
        "expected_token",
    );
}

#[test]
fn lt_slash_at_eof_fails_closed_with_unexpected_eof() {
    // `</` at EOF — official `unexpected_eof` (NOT the nameless
    // `element_invalid_closing_tag`): the close tag is cut off mid-construct.
    assert_fails_closed(
        "lt_slash_eof",
        "<script>let c = $state(0);</script>\n<p>ok</p>\n<button onclick={() => c++}>{c}</button>\n</",
        "unexpected_eof",
    );
}

#[test]
fn close_with_trailing_slash_fails_closed_like_official() {
    // `</div/>` — the byte after the close name is `/` (not whitespace, not `>`): a
    // trailing token. Official `expected_token`. The close-boundary classifier records the
    // strict fact AHEAD of any ancestor absorption, so a malformed-boundary close cannot be
    // silently absorbed as an ancestor close.
    assert_fails_closed(
        "close_trailing_slash",
        "<script>let c = $state(0);</script>\n<div>x</div/>\n<button onclick={() => c++}>{c}</button>\n",
        "expected_token",
    );
}

#[test]
fn nested_style_trailing_token_fails_closed_like_official() {
    // `<div><style>.a {}</style x></div>` — a nested `<style>` whose close carries a
    // trailing token. Official `expected_token`. A nested `<style>` close is a regular
    // element close (a trailing token is rejected), distinct from the LENIENT top-level
    // `<style>` CSS-reader close.
    assert_fails_closed(
        "nested_style_trailing_token",
        "<script>let c = $state(0);</script>\n<div><style>.a {}</style x></div>\n<button onclick={() => c++}>{c}</button>\n",
        "expected_token",
    );
}

#[test]
fn unterminated_style_fails_closed_with_css_expected_identifier() {
    // `<style>.a { color: red;` with no close — official's CSS reader reaches EOF inside
    // the rule and errors `css_expected_identifier` (NOT `element_unclosed`).
    assert_fails_closed(
        "unterminated_style",
        "<script>let c = $state(0);</script>\n<style>.a { color: red;\n<button onclick={() => c++}>{c}</button>\n",
        "css_expected_identifier",
    );
}

// ── A malformed special open tag must TERMINATE (no infinite parse loop) ──────────
//
// `<script`, `<script lang=`, `<style` at EOF must not loop forever: a malformed special
// open tag whose attribute parse returns `None` (no `>` before EOF) MUST make forward
// progress to EOF (so the root loop cannot re-enter the same `<`) and the compile MUST
// fail closed (no `Main`). Bounded on a worker thread so a regression cannot wedge the
// gate — a non-terminating parse is reported as a hard failure, never an actual hang.

/// Parse `source` on a worker thread, returning `true` if the parse TERMINATED within
/// the bound. A regression that re-introduces the infinite loop never finishes, so the
/// join times out and this returns `false` (a hard test failure) instead of hanging the
/// whole test binary.
fn parse_terminates_within(source: &'static str, bound: std::time::Duration) -> bool {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let parsed = parse_svelte(source);
        // Touch the result so the parse is not optimised away.
        let _ = parsed.strict_parse_errors.len();
        let _ = tx.send(());
    });
    rx.recv_timeout(bound).is_ok()
}

#[test]
fn malformed_script_open_tag_terminates() {
    // `<script` at EOF (no `>`, no close) — the parse must terminate, and the compile
    // must fail closed.
    let src = "<script";
    assert!(
        parse_terminates_within(src, std::time::Duration::from_secs(5)),
        "`<script` (malformed open tag at EOF) must terminate the parse, not loop forever"
    );
    assert!(
        compile(src).is_err(),
        "`<script` must fail closed (no Main)"
    );
}

#[test]
fn malformed_script_open_tag_with_partial_attribute_terminates() {
    // `<script lang=` at EOF — the open-tag attribute parse hits EOF mid-attribute; the
    // parse must terminate and the compile must fail closed.
    let src = "<script lang=";
    assert!(
        parse_terminates_within(src, std::time::Duration::from_secs(5)),
        "`<script lang=` must terminate the parse, not loop forever"
    );
    assert!(
        compile(src).is_err(),
        "`<script lang=` must fail closed (no Main)"
    );
}

#[test]
fn malformed_style_open_tag_terminates() {
    // `<style` at EOF — the same no-forward-progress class for the `<style>` special
    // block; the parse must terminate and the compile must fail closed.
    let src = "<style";
    assert!(
        parse_terminates_within(src, std::time::Duration::from_secs(5)),
        "`<style` (malformed open tag at EOF) must terminate the parse, not loop forever"
    );
    assert!(compile(src).is_err(), "`<style` must fail closed (no Main)");
}

#[test]
fn self_closing_script_block_terminates_in_bounded_work() {
    // `<script />` followed by more markup — a self-closing special-block open tag. The
    // self-close recovery records its `expected_token` strict fact but MUST advance past
    // the consumed `/>` so the root scan cannot re-enter the same `<script` forever. A
    // no-advance return is an UNBOUNDED re-parse loop that pushes a strict fact each pass
    // (the 8GB runaway). Bounded on a worker thread so the regression is a hard test
    // failure, never an actual hang.
    let src = "<svelte:options runes={true} /><script /><button onclick={() => {}}>x</button>";
    assert!(
        parse_terminates_within(src, std::time::Duration::from_secs(5)),
        "`<script />` (self-closing special block) must terminate the parse in bounded work, \
         not loop forever re-parsing the same `<script`"
    );
    assert!(
        compile(src).is_err(),
        "`<script />` must fail closed (no Main)"
    );
}

#[test]
fn self_closing_style_block_terminates_in_bounded_work() {
    // `<style />` followed by more markup — the same no-forward-progress class on the
    // `<style>` special block. The self-close recovery must advance past `/>` so the root
    // scan cannot spin on the same `<style` forever.
    let src = "<svelte:options runes={true} /><style /><button onclick={() => {}}>x</button>";
    assert!(
        parse_terminates_within(src, std::time::Duration::from_secs(5)),
        "`<style />` (self-closing special block) must terminate the parse in bounded work, \
         not loop forever re-parsing the same `<style`"
    );
    assert!(
        compile(src).is_err(),
        "`<style />` must fail closed (no Main)"
    );
}

// ── Positive controls: well-formed §1.2 input STILL emits a Main ─────────────────

#[test]
fn section_1_2_headline_still_emits_a_main() {
    // The §1.2 headline shape — well-formed, must NOT be over-rejected by the strict
    // gate (it remains an emitted Main).
    let src = "<script>let name = $state('world'); let count = $state(0);</script>\n<h1>Hello {name}!</h1>\n<input bind:value={name} />\n<button onclick={() => count += 1}>clicks: {count}</button>\n";
    assert!(
        compile(src).is_ok(),
        "the §1.2 headline must still emit a Main (the strict gate must not over-reject it)"
    );
}

#[test]
fn empty_quoted_attribute_value_still_emits_a_main() {
    // `id=""` — an EMPTY QUOTED attribute value is official-ACCEPTED (distinct from the
    // value-less `id=`): it must NOT be rejected by the strict gate.
    let src = "<script>let c = $state(0);</script>\n<div id=\"\"><button onclick={() => c++}>{c}</button></div>\n";
    assert!(
        compile(src).is_ok(),
        "`id=\"\"` (empty quoted value) is official-accepted and must still emit a Main"
    );
}

#[test]
fn whitespace_in_close_tag_still_emits_a_main() {
    // `</button >` — trailing WHITESPACE (no token) before `>` in a close tag is
    // official-ACCEPTED: it must NOT be rejected by the strict gate (distinct from a
    // trailing TOKEN `</button x>`).
    let src = "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button >\n";
    assert!(
        compile(src).is_ok(),
        "trailing whitespace in a close tag is official-accepted and must still emit a Main"
    );
}

// ── Unquoted-value / self-close boundary: a LEADING `/` is a value byte ───────────
//
// Official's unquoted-value reader treats `/` as an ordinary value byte; the `/>`
// self-close marker terminates the value only AFTER at least one value byte. So `id=/>`
// parses as `id="/"` + a NORMAL `>` close (the element stays open ⇒ `element_unclosed`),
// NOT a self-close — whereas `id=x/>` self-closes with value `x`.

#[test]
fn unquoted_value_leading_slash_is_not_self_close_and_fails_closed_like_official() {
    // `<div id=/>` followed by a supported sibling: the `/` is the value, the `>` closes
    // the open tag, the `<div>` is never closed — official `element_unclosed`. The bare
    // forgiving parser would otherwise read an EMPTY value + a self-close and emit a Main
    // (the accept-invalid leak this pins shut).
    //
    // This is a STRUCTURAL reject: an unclosed element is RAIL-A `ElementUnclosed`
    // carrying `element_unclosed`, NOT a `ParserStrictness` strict-parse fact. So it
    // asserts the official CODE (via `assert_fails_closed_with_code`), not the internal
    // rule — only genuine recovery-point strict facts route through `ParserStrictness`.
    assert_fails_closed_with_code(
        "unquoted_value_leading_slash",
        "<script>let c = $state(0);</script>\n<div id=/><button onclick={() => c++}>{c}</button>\n",
        "element_unclosed",
    );
}

#[test]
fn unquoted_value_leading_slash_on_script_attr_fails_closed_like_official() {
    // `<div lang=/>` — the same leading-`/` boundary on a generic attribute: value `/`,
    // normal `>` close, the `<div>` stays open ⇒ `element_unclosed`.
    //
    // Same structural class as above: the unclosed `<div>` is RAIL-A `ElementUnclosed`
    // carrying `element_unclosed`, so this asserts the official CODE (not the
    // `ParserStrictness` rule, which only the genuine strict-parse recovery points use).
    assert_fails_closed_with_code(
        "unquoted_value_leading_slash_lang",
        "<script>let c = $state(0);</script>\n<div lang=/><button onclick={() => c++}>{c}</button>\n",
        "element_unclosed",
    );
}

#[test]
fn unquoted_value_then_self_close_still_emits_a_main() {
    // `<div id=x/>` — value `x`, THEN `/>` self-close: official ACCEPTS it (a self-closed
    // `<div>`), so it must STILL emit a Main (the strict gate must not over-reject the
    // genuine self-close, the control that proves the leading-`/` fix is position-precise).
    let src = "<script>let c = $state(0);</script>\n<div id=x/>\n<button onclick={() => c++}>{c}</button>\n";
    assert!(
        compile(src).is_ok(),
        "`id=x/>` (value `x` then a genuine self-close) is official-accepted and must still emit a Main"
    );
}

// ── Self-closing special blocks `<script />` / `<style />` fail closed ────────────
//
// A self-closing `<script>` / `<style>` open tag (`<script />`, `<style />`, `<script/>`)
// is official `expected_token` (a bare `/>` where a `>` is expected). The forgiving parser
// would otherwise treat it as a content-less block and emit a Main when forced runes +
// a static supported template reach client emission (the accept-invalid leak this pins).

#[test]
fn self_closing_script_block_fails_closed_like_official() {
    // `<script />` — a self-closed instance script: official `expected_token`.
    assert_fails_closed(
        "self_closing_script",
        "<svelte:options runes={true} /><script /><button onclick={() => {}}>x</button>\n",
        "expected_token",
    );
}

#[test]
fn self_closing_style_block_fails_closed_like_official() {
    // `<style />` — a self-closed style block: official `expected_token`.
    assert_fails_closed(
        "self_closing_style",
        "<svelte:options runes={true} /><style /><button onclick={() => {}}>x</button>\n",
        "expected_token",
    );
}

#[test]
fn self_closing_script_block_no_space_fails_closed_like_official() {
    // `<script/>` (no space before `/>`) — the same self-close form: official
    // `expected_token`.
    assert_fails_closed(
        "self_closing_script_no_space",
        "<svelte:options runes={true} /><script/><button onclick={() => {}}>x</button>\n",
        "expected_token",
    );
}

// ── Nested `<style>` left unterminated at EOF records its strict fact ──────────────

#[test]
fn nested_style_unterminated_eof_fails_closed_like_official() {
    // `<div><style>.a {}</style` (EOF, the nested-style close never reaches `>`) — the
    // nested raw-close scan hits EOF with no recognised close. Official `expected_token`
    // (NOT `element_unclosed`): the close-tag scan reached EOF expecting `>`.
    assert_fails_closed(
        "nested_style_unterminated_eof",
        "<div><style>.a {}</style",
        "expected_token",
    );
}

// ── Top-level `<style>` close: longer-name continuation is ACCEPTED (no over-reject) ─
//
// Official's CSS reader matches the `</style` close as a CASE-SENSITIVE prefix and then
// consumes to `>` / EOF — so `</stylefoo>`, `</style-x>`, `</style x>`, `</style >`
// all CLOSE the style cleanly (official ACCEPTS them). A `<style>` block is a NON-core
// FEATURE in Verter's §1.2 surface, so an accepted close fails closed as an unsupported
// feature — but it must NEVER be over-rejected as a parser-strictness (malformed) error.

/// Assert a source does NOT fail closed as a `ParserStrictness` (malformed) refusal — it
/// is either an emitted Main OR a non-strictness refusal (an unsupported FEATURE). This is
/// the "no over-rejection of an official-accepted input" assertion.
fn assert_not_over_rejected(label: &str, source: &str) {
    if let Err(ClientCompileError::OfficialReject(rejection)) = compile(source) {
        use verter_compiler::svelte::runtime::CoreOfficialValidationRule;
        assert_ne!(
            rejection.rule,
            CoreOfficialValidationRule::ParserStrictness,
            "{label}: official ACCEPTS this, but Verter over-rejected it as a parser-strictness \
             (malformed) error carrying `{}`",
            rejection.official_code
        );
    }
}

#[test]
fn top_level_style_longer_name_close_is_not_over_rejected() {
    // `</stylefoo>` — a longer-name continuation of the `</style` prefix: official ACCEPTS
    // it as the style close. Verter must not over-reject it (it fails closed as the
    // unsupported top-level `<style>` FEATURE, never a malformed-parse reject).
    assert_not_over_rejected(
        "top_level_style_longer_name_close",
        "<script>let c = $state(0);</script>\n<style>.a {}</stylefoo>\n<button onclick={() => c++}>{c}</button>\n",
    );
}

#[test]
fn top_level_style_hyphen_name_close_is_not_over_rejected() {
    // `</style-x>` — a hyphenated longer-name continuation: official ACCEPTS the close.
    assert_not_over_rejected(
        "top_level_style_hyphen_name_close",
        "<script>let c = $state(0);</script>\n<style>.a {}</style-x>\n<button onclick={() => c++}>{c}</button>\n",
    );
}

#[test]
fn top_level_style_whitespace_before_gt_close_is_not_over_rejected() {
    // `</style >` — whitespace before `>` in the top-level style close: official ACCEPTS it
    // (the CSS reader consumes to `>`). Verter must not over-reject it.
    assert_not_over_rejected(
        "top_level_style_ws_close",
        "<script>let c = $state(0);</script>\n<style>.a {}</style >\n<button onclick={() => c++}>{c}</button>\n",
    );
}

#[test]
fn nested_style_longer_name_then_real_close_is_not_over_rejected() {
    // `<div><style>.a {}</stylefoo></style></div>` — the `</stylefoo>` is body text (a
    // longer-name continuation the CSS reader does NOT match as the close), the later
    // `</style>` closes. Official ACCEPTS it; Verter must not over-reject the `</stylefoo>`
    // as a malformed close.
    assert_not_over_rejected(
        "nested_style_longer_name_then_close",
        "<script>let c = $state(0);</script>\n<div><style>.a {}</stylefoo></style></div>\n<button onclick={() => c++}>{c}</button>\n",
    );
}

// ── Truncated open-tag EOF + comment EOF codes match official BY CONSTRUCT ─────────

#[test]
fn truncated_intrinsic_open_tag_eof_is_unexpected_eof() {
    // `<div` at EOF — a truncated intrinsic open tag: official `unexpected_eof` (the tag is
    // cut off mid-construct), NOT `expected_token`.
    assert_fails_closed("truncated_div_open_eof", "<div", "unexpected_eof");
}

#[test]
fn truncated_intrinsic_open_tag_with_name_eof_is_unexpected_eof() {
    // `<div id` at EOF — a truncated open tag mid-attribute-name: official `unexpected_eof`.
    assert_fails_closed("truncated_div_id_eof", "<div id", "unexpected_eof");
}

#[test]
fn truncated_script_open_tag_eof_is_unexpected_eof() {
    // `<script` at EOF — official `unexpected_eof` (NOT `expected_token`).
    assert_fails_closed("truncated_script_open_eof", "<script", "unexpected_eof");
}

#[test]
fn truncated_style_open_tag_eof_is_unexpected_eof() {
    // `<style` at EOF — official `unexpected_eof`.
    assert_fails_closed("truncated_style_open_eof", "<style", "unexpected_eof");
}

#[test]
fn script_attr_eq_at_eof_is_expected_attribute_value() {
    // `<script lang=` at EOF — an `=` with EOF before the value: official
    // `expected_attribute_value` (NOT `unexpected_eof`): the value is expected.
    assert_fails_closed(
        "script_lang_eq_eof",
        "<script lang=",
        "expected_attribute_value",
    );
}

#[test]
fn style_attr_eq_at_eof_is_expected_attribute_value() {
    // `<style lang=` at EOF — the same construct on the style block: official
    // `expected_attribute_value`.
    assert_fails_closed(
        "style_lang_eq_eof",
        "<style lang=",
        "expected_attribute_value",
    );
}

#[test]
fn empty_comment_at_eof_is_unexpected_eof() {
    // `<!--` at EOF (nothing after the lead) — official `unexpected_eof`: the comment is
    // cut off immediately.
    assert_fails_closed("empty_comment_eof", "<!--", "unexpected_eof");
}

#[test]
fn started_comment_at_eof_is_expected_token() {
    // `<!-- oops` at EOF (a started-but-unterminated comment) — official `expected_token`
    // (distinct from the EMPTY `<!--` which is `unexpected_eof`).
    assert_fails_closed("started_comment_eof", "<!-- oops", "expected_token");
}

// ── DEFECT-ENCOUNTER-ORDER ARBITRATION ──────────────────────────────────────────
//
// When more than one official-reject defect is present, the official compiler stops at
// the FIRST parse error in its single forward pass, then (only on a CLEAN parse) runs its
// analyze-phase validations. The gate must therefore arbitrate every PARSE defect — close-tag,
// strict, script-domain, and the explicit-`</p>`-autoclose — by the parser's DISCOVERY order
// (`encounter_order`), never by source span; and it must gate the analyze-phase placement /
// declaration / global-reference checks behind an EMPTY parse-defect stream. Each expected code
// below was verified against the pinned `svelte@5.56.3` compiler (see the reject oracle); the
// `// upstream:` annotation is the pinned verdict, NOT a guess.

#[test]
fn inner_stray_close_beats_outer_unclosed_by_encounter_order() {
    // `<div></span>` — the `<div>` opens, the inner stray `</span>` is DISCOVERED first (a
    // mismatched close), and only at EOF is `<div>` PROVEN unclosed. Span order would pick the
    // `<div>` open (earlier byte); encounter order picks the inner stray close.
    // upstream: element_invalid_closing_tag (NOT element_unclosed).
    assert_fails_closed_with_code(
        "inner_stray_beats_outer_unclosed",
        "<script>let c = $state(0);</script>\n<div></span>\n<button onclick={() => c++}>{c}</button>\n",
        "element_invalid_closing_tag",
    );
}

#[test]
fn inner_void_content_close_beats_outer_unclosed_by_encounter_order() {
    // `<div><input></input></div>` — the inner `</input>` void-content close is the first defect.
    // upstream: void_element_invalid_content.
    assert_fails_closed_with_code(
        "inner_void_beats_outer_unclosed",
        "<script>let c = $state(0);</script>\n<div><input></input></div>\n<button onclick={() => c++}>{c}</button>\n",
        "void_element_invalid_content",
    );
}

#[test]
fn earlier_parse_strict_defect_beats_later_placement() {
    // `<a href="/"><a href="/x">x</a></a>` (a nested-`<a>` PLACEMENT defect) followed by
    // `<div bar=></div>` (an empty-attr-value PARSE defect). Placement is an ANALYZE-phase
    // validation that runs ONLY on a clean parse; the `<div bar=>` empty value is a PARSE
    // defect, so the parse defect wins even though the nested-`<a>` is at an earlier byte.
    // upstream: expected_attribute_value (NOT node_invalid_placement).
    assert_fails_closed(
        "parse_strict_beats_placement",
        "<script>let c = $state(0);</script>\n<a href=\"/\"><a href=\"/x\">x</a></a>\n<div bar=></div>\n<button onclick={() => c++}>{c}</button>\n",
        "expected_attribute_value",
    );
}

#[test]
fn parse_close_defect_beats_placement_on_unclean_parse() {
    // `<button><button>x</button></button></div>` — the nested `<button>` is a placement defect,
    // but the trailing stray `</div>` is a PARSE close defect that makes the parse unclean, so the
    // analyze-phase placement check never runs.
    // upstream: element_invalid_closing_tag (NOT node_invalid_placement).
    assert_fails_closed_with_code(
        "parse_close_beats_placement",
        "<script>let c = $state(0);</script>\n<button><button>x</button></button></div>\n<button onclick={() => c++}>{c}</button>\n",
        "element_invalid_closing_tag",
    );
}

#[test]
fn earlier_template_close_defect_beats_later_script_reject() {
    // `</span>` (a stray close, DISCOVERED first in the forward pass) precedes a
    // `<script context="bad">` (a script-domain reject). The script reject is a PARSE defect too,
    // but it is discovered LATER, so the earlier template close wins.
    // upstream: element_invalid_closing_tag (NOT script_invalid_context).
    assert_fails_closed_with_code(
        "template_close_beats_script",
        "</span>\n<script context=\"bad\">let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        "element_invalid_closing_tag",
    );
}

#[test]
fn module_script_reject_beats_later_instance_script_reject_by_discovery_order() {
    // `<script module server>` (a reserved attr on the MODULE script, discovered first in source
    // order) precedes `<script context="bad">` (the instance script's invalid context). A fixed
    // instance-before-module pre-pass would wrongly pick the instance's context error; discovery
    // order picks the module's reserved attr.
    // upstream: script_reserved_attribute.
    assert_fails_closed_with_code(
        "module_reject_beats_instance_reject",
        "<script module server>const K = 1;</script>\n<script context=\"bad\">let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        "script_reserved_attribute",
    );
}

#[test]
fn inner_parse_defect_beats_paragraph_autoclose() {
    // `<p><div id=></div></p>` — the `<div id=>` empty-attr-value PARSE defect is discovered
    // inside the `<p>` BEFORE the surviving `</p>` autoclose is consumed, so it wins. (Anchoring
    // the autoclose at the `<p>` OPEN span — the old behavior — would wrongly out-rank the inner
    // defect.)
    // upstream: expected_attribute_value (NOT element_invalid_closing_tag_autoclosed).
    assert_fails_closed(
        "inner_parse_beats_autoclose",
        "<script>let c = $state(0);</script>\n<p><div id=></div></p>\n<button onclick={() => c++}>{c}</button>\n",
        "expected_attribute_value",
    );
}

#[test]
fn paragraph_autoclose_beats_a_later_stray_close_by_discovery_order() {
    // `<p><div>x</div></p>` (the surviving `</p>` autoclose) precedes a trailing stray `</span>`.
    // The autoclose is minted at the `</p>` close site, discovered before the later stray.
    // upstream: element_invalid_closing_tag_autoclosed.
    assert_fails_closed_with_code(
        "autoclose_beats_later_stray",
        "<script>let c = $state(0);</script>\n<p><div>x</div></p>\n</span>\n<button onclick={() => c++}>{c}</button>\n",
        "element_invalid_closing_tag_autoclosed",
    );
}

#[test]
fn earlier_stray_close_beats_paragraph_autoclose() {
    // A stray `</span>` BEFORE the `<p><div>x</div></p>` autoclose is discovered first.
    // upstream: element_invalid_closing_tag.
    assert_fails_closed_with_code(
        "earlier_stray_beats_autoclose",
        "<script>let c = $state(0);</script>\n</span>\n<p><div>x</div></p>\n<button onclick={() => c++}>{c}</button>\n",
        "element_invalid_closing_tag",
    );
}

#[test]
fn script_domain_reject_is_minted_after_open_tag_attribute_parse() {
    // `<script server lang=>` — a reserved attr `server` AND an empty-attr-value `lang=`. The
    // empty-value strict fact is recorded DURING the open-tag attribute parse, so it is discovered
    // BEFORE the reserved-attr reject (which is minted only after the open tag is parsed),
    // matching upstream's parse-time-before-`read_script`-validation order.
    // upstream: expected_attribute_value (NOT script_reserved_attribute).
    assert_fails_closed(
        "script_emptyattr_before_reserved",
        "<script server lang=>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        "expected_attribute_value",
    );
}

// ── ANALYZE-PHASE ORDER (clean parse only) ──────────────────────────────────────
//
// On a CLEAN parse, the analyze phase runs in upstream pass order: the script-scope / global
// `$`-reference checks (`scope.js` / the store-subscription guard) precede the template-walk
// `node_invalid_placement`. So a script declaration / global-reference defect wins over a
// concurrent template placement defect.

#[test]
fn dollar_prefix_declaration_beats_template_placement() {
    // `<script>let $x = 1;</script>` (a `$`-prefixed declaration, a script-scope analyze defect)
    // with `<button><button>x</button></button>` (a placement defect). Both are analyze-phase, but
    // the script-scope check runs first.
    // upstream: dollar_prefix_invalid (NOT node_invalid_placement).
    assert_fails_closed_with_code(
        "dollar_prefix_beats_placement",
        "<script>let $x = 1;</script>\n<button><button>x</button></button>\n",
        "dollar_prefix_invalid",
    );
}

#[test]
fn global_reference_beats_template_placement() {
    // `<script>let c = $state(0); let x = $foo;</script>` (an undeclared `$foo` global reference)
    // with a nested-`<button>` placement defect. The global-reference analyze check runs before the
    // template-walk placement check.
    // upstream: global_reference_invalid (NOT node_invalid_placement).
    assert_fails_closed_with_code(
        "global_reference_beats_placement",
        "<script>let c = $state(0); let x = $foo;</script>\n<button><button>x</button></button>\n",
        "global_reference_invalid",
    );
}
