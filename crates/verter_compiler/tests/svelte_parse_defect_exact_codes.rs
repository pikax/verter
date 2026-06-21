//! INDEPENDENT EXACT-CODE RAIL: the Svelte official-reject gate must report the EXACT
//! official `svelte@5.56.3` diagnostic code for every multi-defect / ordering-sensitive
//! parse fixture — asserted against a HARDCODED expected-code table independently confirmed
//! by running the pinned compiler, NOT derived from the parser's own public facts.
//!
//! The sibling arbitration guard
//! (`svelte_parse_defect_arbitration_by_encounter_order.rs`) proves the gate picks the
//! minimum-`encounter_order` defect — but it derives the EXPECTED code from the SAME public
//! `ParsedSvelte` facts the gate consumes, so it cannot catch a WRONG fact code or a MISSING
//! fact rail (a defect that should be a parser fact but is routed code-less / dropped). This
//! file closes that gap: each expected code below was grounded by executing
//! `svelte@5.56.3` over the exact source, so the assertion fails if the gate reports the
//! wrong code OR fails to mint the fact at all.
//!
//! Covers the per-`<script>` minting order (attribute-duplicate → body-parse → source-order
//! reserved/context/module → script-duplicate), the body-parse reserved slot (a body syntax
//! error / redeclaration wins over a later semantic-attr / duplicate-script defect), the
//! template `attribute_duplicate` + duplicate-`<svelte:options>` parser facts, and the
//! module→instance analyze order.

use verter_compiler::svelte::parser::parse_svelte;
use verter_compiler::svelte::runtime::official_reject_gate;

const BUTTON: &str = "<button onclick={() => c++}>{c}</button>";

/// The EXACT official code the real gate reports for `source` (panics if the gate ACCEPTS —
/// every fixture here is a genuine reject confirmed against pinned svelte@5.56.3).
fn gate_code(source: &str) -> &'static str {
    let parsed = parse_svelte(source);
    official_reject_gate(source, &parsed)
        .unwrap_or_else(|| panic!("gate ACCEPTED a fixture official rejects:\n{source}"))
        .official_code
}

/// Assert the gate reports `expected` for `source`, with a discriminating failure message.
fn assert_code(name: &str, source: &str, expected: &str) {
    let got = gate_code(source);
    assert_eq!(
        got, expected,
        "{name}: gate reports `{got}`, but svelte@5.56.3 rejects this source with `{expected}`:\n{source}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (1) Per-`<script>` source-order semantic-attribute validation (item 1).
//     Upstream `read_script` validates reserved/context/module attributes in SOURCE
//     ORDER (the FIRST faulting attribute wins), NOT a duplicate→reserved→context bucket.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn context_before_reserved_reports_invalid_context() {
    // `context="bad"` precedes `server` in source order → `script_invalid_context`
    // (the bucketed path WRONGLY reported `script_reserved_attribute`).
    assert_code(
        "context_before_reserved",
        &format!("<script context=\"bad\" server>let c = $state(0);</script>\n{BUTTON}\n"),
        "script_invalid_context",
    );
}

#[test]
fn reserved_before_context_reports_reserved_attribute() {
    // `server` precedes `context="bad"` in source order → `script_reserved_attribute`.
    assert_code(
        "reserved_before_context",
        &format!("<script server context=\"bad\">let c = $state(0);</script>\n{BUTTON}\n"),
        "script_reserved_attribute",
    );
}

#[test]
fn valued_module_before_reserved_reports_invalid_attribute_value() {
    // `module="x"` (valued module — boolean-only) precedes `server` → the per-attribute
    // module check fires first: `script_invalid_attribute_value`.
    assert_code(
        "valued_module_before_reserved",
        "<script module=\"x\" server>const K = 1;</script>\n<button>x</button>\n",
        "script_invalid_attribute_value",
    );
}

#[test]
fn duplicate_attribute_beats_context_in_a_script() {
    // The open-tag attribute-duplicate (`lang lang`) is minted in the element attribute
    // loop, BEFORE `read_script`'s context validation → `attribute_duplicate` wins over the
    // earlier-positioned `context="bad"`.
    assert_code(
        "dup_attr_beats_context",
        &format!(
            "<script context=\"bad\" lang=\"js\" lang=\"js\">let c = $state(0);</script>\n{BUTTON}\n"
        ),
        "attribute_duplicate",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (2)+(5) Body-parse reserved slot: upstream parses the script body with Acorn BEFORE
//     validating reserved/context/module, so a body syntax error / redeclaration wins over a
//     later semantic-attr defect AND over the duplicate-script check.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn body_parse_error_beats_reserved_attribute() {
    // `<script server>let = ;</script>` — the body fails to parse (`js_parse_error`), which
    // upstream discovers BEFORE the reserved-attribute validation → `js_parse_error`
    // (NOT `script_reserved_attribute`).
    assert_code(
        "body_parse_beats_reserved",
        &format!("<script server>let = ;</script>\n{BUTTON}\n"),
        "js_parse_error",
    );
}

#[test]
fn second_script_body_parse_error_beats_script_duplicate() {
    // `<script>let x=1;</script><script>let = ;</script>` — the SECOND script's body parse
    // fails; upstream parses the body before throwing the duplicate-script error →
    // `js_parse_error` (NOT `script_duplicate`).
    assert_code(
        "second_body_parse_beats_duplicate",
        &format!("<script>let x = 1;</script>\n<script>let = ;</script>\n{BUTTON}\n"),
        "js_parse_error",
    );
}

#[test]
fn same_scope_redeclaration_is_a_body_parse_error() {
    // `let a=1; let a=2;` in one script — a same-lexical-scope `let` redeclaration Acorn
    // rejects in the parse phase → `js_parse_error` (the body slot, NOT a later analyze
    // `declaration_duplicate`).
    assert_code(
        "same_scope_redeclaration",
        &format!("<script>let a = 1; let a = 2;</script>\n{BUTTON}\n"),
        "js_parse_error",
    );
}

#[test]
fn typescript_syntax_in_plain_script_is_a_body_parse_error() {
    // A plain `<script>` body using TS-only syntax (a type annotation) — upstream parses the
    // plain-script body as JS (Acorn, no TS), so the annotation is `js_parse_error`. (A
    // `lang="ts"` body would parse it cleanly; this is the JS-grammar discrimination.)
    assert_code(
        "ts_in_plain_script",
        &format!("<script>let a: number = 1;</script>\n{BUTTON}\n"),
        "js_parse_error",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (3) Template `attribute_duplicate` + duplicate-`<svelte:options>` are PARSER facts that
//     compete in encounter-order arbitration (minted during the open-tag parse), so they win
//     over a LATER close/strict defect — they are no longer routed code-less behind the gate.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn template_duplicate_attribute_beats_later_stray_close() {
    // `<div id id></span>` — the open-tag duplicate `id` is discovered BEFORE the stray
    // `</span>` close → `attribute_duplicate` (the close-tag defect must NOT win).
    assert_code(
        "template_dup_beats_stray_close",
        &format!("<script>let c = $state(0);</script>\n<div id id></span>\n{BUTTON}\n"),
        "attribute_duplicate",
    );
}

#[test]
fn template_duplicate_attribute_beats_paragraph_autoclose() {
    // `<p id id><div>x</div></p>` — the open-tag duplicate `id` is discovered before the
    // surviving-`</p>` autoclose → `attribute_duplicate`.
    assert_code(
        "template_dup_beats_autoclose",
        &format!("<script>let c = $state(0);</script>\n<p id id><div>x</div></p>\n{BUTTON}\n"),
        "attribute_duplicate",
    );
}

#[test]
fn template_duplicate_attribute_beats_later_script_reject() {
    // `<div id id></div>…<script context="bad">…` — the template duplicate attribute
    // (earlier) wins over the later script-domain reject → `attribute_duplicate`.
    assert_code(
        "template_dup_beats_later_script",
        &format!(
            "<script>let c = $state(0);</script>\n<div id id></div>\n<script context=\"bad\">let d = 1;</script>\n{BUTTON}\n"
        ),
        "attribute_duplicate",
    );
}

#[test]
fn duplicate_svelte_options_is_svelte_meta_duplicate() {
    // A SECOND `<svelte:options>` — official `svelte_meta_duplicate`, minted by the parser
    // when the second root-only meta tag is encountered, so it competes in arbitration
    // (rather than being silently accepted by the official-reject gate and only refused
    // later as a code-less Unsupported surface).
    assert_code(
        "duplicate_svelte_options",
        &format!(
            "<script>let c = $state(0);</script>\n<svelte:options runes={{true}} />\n<svelte:options runes={{true}} />\n{BUTTON}\n"
        ),
        "svelte_meta_duplicate",
    );
}

#[test]
fn duplicate_svelte_options_beats_later_stray_close() {
    // The duplicate `<svelte:options>` (discovered at the second options tag) beats a LATER
    // stray `</span>` close → `svelte_meta_duplicate`.
    assert_code(
        "duplicate_options_beats_stray",
        &format!(
            "<script>let c = $state(0);</script>\n<svelte:options runes={{true}} />\n<svelte:options runes={{true}} />\n</span>\n{BUTTON}\n"
        ),
        "svelte_meta_duplicate",
    );
}

#[test]
fn nested_svelte_options_beats_later_stray_close() {
    // A NESTED `<svelte:options>` (not at the component root) is `svelte_meta_invalid_placement`,
    // discovered when the nested options tag is parsed — it beats a LATER stray `</span>` close.
    assert_code(
        "nested_options_beats_stray",
        &format!(
            "<script>let c = $state(0);</script>\n<div><svelte:options runes={{true}} /></div>\n</span>\n{BUTTON}\n"
        ),
        "svelte_meta_invalid_placement",
    );
}

#[test]
fn duplicate_style_beats_later_stray_close() {
    // A SECOND top-level `<style>` is `style_duplicate`, discovered at the second `<style>`
    // open tag — it beats a LATER stray `</span>` close (which a code-less routing would have
    // wrongly let win).
    assert_code(
        "duplicate_style_beats_stray",
        &format!(
            "<script>let c = $state(0);</script>\n<style>.a {{}}</style>\n<style>.b {{}}</style>\n</span>\n{BUTTON}\n"
        ),
        "style_duplicate",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (F2) Upstream `element.js` runs `read_style` (which PARSES the CSS body and can throw)
//      BEFORE `style_duplicate`, so a MALFORMED CSS body in the 2nd `<style>` wins the
//      first-error race over the duplicate. Verter reserves a `StyleBodyProbe` at the
//      `read_style` position and fills it with a faithful CSS-body reader; a clean / empty
//      body lets the later `style_duplicate` fact win. (All codes grounded against pinned
//      svelte@5.56.3.)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn malformed_second_style_body_beats_style_duplicate() {
    // `<style>.b {</style>` — the 2nd style's body opens a block that never closes; upstream's
    // CSS reader (run from the 2nd style's content start, BEFORE `style_duplicate`) reaches a
    // non-identifier where it requires one ⇒ `css_expected_identifier`, which wins over the
    // duplicate-style error. (Verter previously minted `style_duplicate` without parsing the
    // body.)
    assert_code(
        "malformed_2nd_style_beats_dup",
        &format!(
            "<script>let c = $state(0);</script>\n<style>.a {{}}</style>\n<style>.b {{</style>\n{BUTTON}"
        ),
        "css_expected_identifier",
    );
}

#[test]
fn empty_declaration_in_second_style_body_beats_style_duplicate() {
    // `<style>.b { color }</style>` — a declaration with no `:`/value is `css_empty_declaration`,
    // which the `read_style` body parse discovers before the duplicate-style error.
    assert_code(
        "empty_decl_2nd_style_beats_dup",
        &format!(
            "<script>let c = $state(0);</script>\n<style>.a {{}}</style>\n<style>.b {{ color }}</style>\n{BUTTON}"
        ),
        "css_empty_declaration",
    );
}

#[test]
fn malformed_first_style_body_wins_over_second_style_duplicate() {
    // The FIRST style's malformed body is parsed (at its own `read_style` position) BEFORE the
    // 2nd style is even reached, so the 1st body's `css_expected_identifier` wins over the
    // duplicate. (Each style body is parsed at its own position, left to right — exactly like
    // the per-script body probes.)
    assert_code(
        "malformed_1st_style_wins",
        &format!(
            "<script>let c = $state(0);</script>\n<style>.a {{</style>\n<style>.b {{}}</style>\n{BUTTON}"
        ),
        "css_expected_identifier",
    );
}

#[test]
fn clean_second_style_body_still_reports_style_duplicate() {
    // POSITIVE CONTROL: a 2nd style with a CLEAN non-empty body parses without error, so the
    // later `style_duplicate` fact wins — the StyleBodyProbe must not over-reject valid CSS.
    assert_code(
        "clean_2nd_style_is_duplicate",
        &format!(
            "<script>let c = $state(0);</script>\n<style>.a {{}}</style>\n<style>.c {{ color: red; }}</style>\n{BUTTON}"
        ),
        "style_duplicate",
    );
}

#[test]
fn empty_second_style_body_still_reports_style_duplicate() {
    // POSITIVE CONTROL: a 2nd style with an EMPTY body parses without error ⇒ `style_duplicate`.
    assert_code(
        "empty_2nd_style_is_duplicate",
        &format!(
            "<script>let c = $state(0);</script>\n<style>.a {{}}</style>\n<style></style>\n{BUTTON}"
        ),
        "style_duplicate",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (F3) `<svelte:options>` attribute / child-content EXACT codes. Upstream `read_options`
//      (the Parser constructor's parse FINALIZATION, AFTER the root walk) validates the
//      options attributes in source order (the FIRST faulting attribute wins) and then
//      `disallow_children`. These are PARSE/finalization errors with exact upstream codes —
//      minted as `OptionsInvalid` parse facts at the finalization position (after every
//      template/script/style fact). (All codes grounded against pinned svelte@5.56.3.)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn options_name_attribute_is_unknown_attribute() {
    // `name="x"` is not a `read_options` switch key → `svelte_options_unknown_attribute`.
    assert_code(
        "options_name",
        &format!("<svelte:options name=\"x\" />\n{BUTTON}\n"),
        "svelte_options_unknown_attribute",
    );
}

#[test]
fn options_unknown_attribute_is_unknown_attribute() {
    assert_code(
        "options_unknown",
        &format!("<svelte:options frobnicate=\"x\" />\n{BUTTON}\n"),
        "svelte_options_unknown_attribute",
    );
}

#[test]
fn options_bad_namespace_is_invalid_attribute_value() {
    assert_code(
        "options_namespace_bad",
        &format!("<svelte:options namespace=\"bad\" />\n{BUTTON}\n"),
        "svelte_options_invalid_attribute_value",
    );
}

#[test]
fn options_bad_css_is_invalid_attribute_value() {
    // `css` accepts only `"injected"`.
    assert_code(
        "options_css_external",
        &format!("<svelte:options css=\"external\" />\n{BUTTON}\n"),
        "svelte_options_invalid_attribute_value",
    );
}

#[test]
fn options_string_runes_is_invalid_attribute_value() {
    // A `runes` value is boolean-only (`get_boolean_value`); a quoted `runes="true"` is a
    // non-boolean → `svelte_options_invalid_attribute_value`.
    assert_code(
        "options_runes_string",
        &format!("<svelte:options runes=\"true\" />\n{BUTTON}\n"),
        "svelte_options_invalid_attribute_value",
    );
}

#[test]
fn options_nonliteral_runes_is_invalid_attribute_value() {
    // `runes={foo}` — a non-literal expression resolves to a non-boolean `get_static_value` →
    // `svelte_options_invalid_attribute_value`.
    assert_code(
        "options_runes_foo",
        &format!("<svelte:options runes={{foo}} />\n{BUTTON}\n"),
        "svelte_options_invalid_attribute_value",
    );
}

#[test]
fn options_nonbool_immutable_is_invalid_attribute_value() {
    assert_code(
        "options_immutable_str",
        &format!("<svelte:options immutable=\"yes\" />\n{BUTTON}\n"),
        "svelte_options_invalid_attribute_value",
    );
}

#[test]
fn options_tag_is_deprecated_tag() {
    // The `tag` attribute is ALWAYS the deprecated-tag hard error (regardless of value).
    assert_code(
        "options_tag",
        &format!("<svelte:options tag=\"my-el\" />\n{BUTTON}\n"),
        "svelte_options_deprecated_tag",
    );
}

#[test]
fn options_customelement_bad_tagname_is_invalid_tagname() {
    // `customElement="nodash"` — a Text value that is not a valid custom-element tag name →
    // `svelte_options_invalid_tagname`.
    assert_code(
        "options_ce_bad_tagname",
        &format!("<svelte:options customElement=\"nodash\" />\n{BUTTON}\n"),
        "svelte_options_invalid_tagname",
    );
}

#[test]
fn options_customelement_boolean_shorthand_is_invalid_customelement() {
    // `customElement` (boolean shorthand, value === true) → `svelte_options_invalid_customelement`.
    assert_code(
        "options_ce_shorthand",
        &format!("<svelte:options customElement />\n{BUTTON}\n"),
        "svelte_options_invalid_customelement",
    );
}

#[test]
fn options_spread_is_invalid_attribute() {
    // A spread `{...x}` is not an `Attribute` → `svelte_options_invalid_attribute`.
    assert_code(
        "options_spread",
        &format!("<svelte:options {{...x}} />\n{BUTTON}\n"),
        "svelte_options_invalid_attribute",
    );
}

#[test]
fn options_directive_is_invalid_attribute() {
    // A directive `bind:x={y}` is not an `Attribute` → `svelte_options_invalid_attribute`.
    assert_code(
        "options_directive",
        &format!("<svelte:options bind:x={{y}} />\n{BUTTON}\n"),
        "svelte_options_invalid_attribute",
    );
}

#[test]
fn options_child_content_is_meta_invalid_content() {
    // `<svelte:options>hi</svelte:options>` — child content on a valid-attribute options element
    // is `svelte_meta_invalid_content` (the `disallow_children` finalization, AFTER attributes).
    assert_code(
        "options_child_content",
        &format!("<svelte:options runes={{true}}>hi</svelte:options>\n{BUTTON}\n"),
        "svelte_meta_invalid_content",
    );
}

#[test]
fn options_first_faulting_attribute_wins_unknown_before_invalid_value() {
    // `name="x" namespace="bad"` — the FIRST faulting attribute in source order wins:
    // `name` (`svelte_options_unknown_attribute`) precedes `namespace="bad"`.
    assert_code(
        "options_name_before_ns",
        &format!("<svelte:options name=\"x\" namespace=\"bad\" />\n{BUTTON}\n"),
        "svelte_options_unknown_attribute",
    );
}

#[test]
fn options_first_faulting_attribute_wins_invalid_value_before_unknown() {
    // The reverse order: `namespace="bad"` precedes `name="x"` → `svelte_options_invalid_attribute_value`.
    assert_code(
        "options_ns_before_name",
        &format!("<svelte:options namespace=\"bad\" name=\"x\" />\n{BUTTON}\n"),
        "svelte_options_invalid_attribute_value",
    );
}

#[test]
fn options_duplicate_attribute_beats_read_options_finalization() {
    // `<svelte:options runes={true} runes={true} />` — the OPEN-TAG duplicate-attribute fact is
    // minted DURING the walk (an earlier `encounter_order`), so it wins over the `read_options`
    // finalization (which runs after the walk) → `attribute_duplicate`. (Upstream throws
    // `attribute_duplicate` in the open-tag loop before `read_options`.)
    assert_code(
        "options_dup_attr_beats_finalization",
        &format!("<svelte:options runes={{true}} runes={{true}} />\n{BUTTON}\n"),
        "attribute_duplicate",
    );
}

#[test]
fn options_duplicate_attribute_beats_unknown_attribute_fault() {
    // `<svelte:options name="x" name="y" />` — the open-tag duplicate `name` (walk) beats the
    // `read_options` unknown-attribute fault on `name` (finalization) → `attribute_duplicate`.
    assert_code(
        "options_dup_name_beats_unknown",
        &format!("<svelte:options name=\"x\" name=\"y\" />\n{BUTTON}\n"),
        "attribute_duplicate",
    );
}

#[test]
fn options_attribute_fault_wins_over_child_content() {
    // `<svelte:options name="x">hi</svelte:options>` — the attribute validation runs BEFORE
    // `disallow_children`, so the attribute fault wins over the child content.
    assert_code(
        "options_attr_before_child",
        &format!("<svelte:options name=\"x\">hi</svelte:options>\n{BUTTON}\n"),
        "svelte_options_unknown_attribute",
    );
}

#[test]
fn options_fault_is_discovered_after_an_earlier_template_defect() {
    // ORDER: the options finalization runs AFTER the root walk, so an EARLIER template defect
    // (a stray `</span>` before the options) wins over the options-attribute fault.
    assert_code(
        "stray_close_before_options_fault",
        &format!(
            "<script>let c = $state(0);</script>\n</span>\n<svelte:options name=\"x\" />\n{BUTTON}\n"
        ),
        "element_invalid_closing_tag",
    );
}

#[test]
fn options_fault_beats_a_later_template_defect() {
    // ...but the options finalization is discovered (in upstream encounter order) BEFORE the
    // analyze-phase placement / after a clean parse — here a LATER unsupported feature is not a
    // parse defect, so the options-attribute exact code is reported. A standalone bad options is
    // its own exact code.
    assert_code(
        "options_fault_standalone",
        &format!(
            "<script>let c = $state(0);</script>\n<svelte:options namespace=\"bad\" />\n{BUTTON}\n"
        ),
        "svelte_options_invalid_attribute_value",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// POSITIVE CONTROLS for F3 — officially-ACCEPTED unsupported options axes must NOT mint an
// `OptionsInvalid` exact-code reject (they fail closed later as an unsupported FEATURE). The
// gate must report NONE of the `svelte_options_*` / `svelte_meta_invalid_content` codes.
// ─────────────────────────────────────────────────────────────────────────────

/// Assert the gate does NOT report any `svelte_options_*` / `svelte_meta_invalid_content` code
/// for `source` (an officially-accepted options axis Verter refuses as an unsupported feature).
fn assert_not_options_reject(name: &str, source: &str) {
    let parsed = parse_svelte(source);
    let code = official_reject_gate(source, &parsed).map(|r| r.official_code);
    let is_options_code = matches!(
        code,
        Some(
            "svelte_options_unknown_attribute"
                | "svelte_options_invalid_attribute_value"
                | "svelte_options_deprecated_tag"
                | "svelte_options_invalid_tagname"
                | "svelte_options_invalid_customelement"
                | "svelte_options_invalid_attribute"
                | "svelte_meta_invalid_content"
        )
    );
    assert!(
        !is_options_code,
        "{name}: an officially-ACCEPTED options axis must NOT be an `OptionsInvalid` reject, but \
         the gate reported `{code:?}`:\n{source}"
    );
}

#[test]
fn options_valid_svg_namespace_is_not_an_options_reject() {
    assert_not_options_reject(
        "options_namespace_svg",
        &format!("<svelte:options namespace=\"svg\" />\n{BUTTON}\n"),
    );
}

#[test]
fn options_injected_css_is_not_an_options_reject() {
    assert_not_options_reject(
        "options_css_injected",
        &format!("<svelte:options css=\"injected\" />\n{BUTTON}\n"),
    );
}

#[test]
fn options_runes_false_is_not_an_options_reject() {
    // `runes={false}` is a VALID boolean (selects legacy mode); Verter refuses it as the
    // legacy-mode feature, NOT an options reject.
    assert_not_options_reject(
        "options_runes_false",
        &format!("<svelte:options runes={{false}} />\n{BUTTON}\n"),
    );
}

#[test]
fn options_valid_boolean_immutable_is_not_an_options_reject() {
    assert_not_options_reject(
        "options_immutable_ok",
        &format!("<svelte:options immutable />\n{BUTTON}\n"),
    );
}

#[test]
fn options_valid_customelement_string_is_not_an_options_reject() {
    // `customElement="my-el"` is a VALID custom-element tag — Verter refuses it as the 5h
    // host/custom-element feature, NOT an options reject.
    assert_not_options_reject(
        "options_ce_valid",
        &format!("<svelte:options customElement=\"my-el\" />\n{BUTTON}\n"),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (F3 customElement={EXPR}) The expression-valued `customElement` axis is reserved by the
//     parser and FILLED by the gate via OXC — the expression's AST decides the exact code.
//     (Grounded against pinned svelte@5.56.3.)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn options_customelement_number_expr_is_invalid_customelement() {
    // `customElement={42}` — a non-string, non-object, non-null literal →
    // `svelte_options_invalid_customelement`.
    assert_code(
        "options_ce_number",
        &format!("<svelte:options customElement={{42}} />\n{BUTTON}\n"),
        "svelte_options_invalid_customelement",
    );
}

#[test]
fn options_customelement_identifier_expr_is_invalid_customelement() {
    // `customElement={foo}` — a non-literal identifier → `svelte_options_invalid_customelement`.
    assert_code(
        "options_ce_ident",
        &format!("<svelte:options customElement={{foo}} />\n{BUTTON}\n"),
        "svelte_options_invalid_customelement",
    );
}

#[test]
fn options_customelement_object_bad_tag_is_invalid_tagname() {
    // `customElement={{ tag: "nodash" }}` — a valid object with an invalid tag name →
    // `svelte_options_invalid_tagname`.
    assert_code(
        "options_ce_obj_bad_tag",
        &format!("<svelte:options customElement={{{{ tag: \"nodash\" }}}} />\n{BUTTON}\n"),
        "svelte_options_invalid_tagname",
    );
}

#[test]
fn options_customelement_object_bad_props_is_invalid_customelement_props() {
    // `customElement={{ props: 1 }}` — `props` not an object → `svelte_options_invalid_customelement_props`.
    assert_code(
        "options_ce_obj_bad_props",
        &format!("<svelte:options customElement={{{{ props: 1 }}}} />\n{BUTTON}\n"),
        "svelte_options_invalid_customelement_props",
    );
}

#[test]
fn options_customelement_object_bad_shadow_is_invalid_customelement_shadow() {
    // `customElement={{ shadow: 1 }}` — `shadow` not `"open"`/`"none"`/object →
    // `svelte_options_invalid_customelement_shadow`.
    assert_code(
        "options_ce_obj_bad_shadow",
        &format!("<svelte:options customElement={{{{ shadow: 1 }}}} />\n{BUTTON}\n"),
        "svelte_options_invalid_customelement_shadow",
    );
}

#[test]
fn options_customelement_object_reserved_tag_is_reserved_tagname() {
    // `customElement={{ tag: "annotation-xml" }}` — a reserved custom-element tag name →
    // `svelte_options_reserved_tagname`.
    assert_code(
        "options_ce_obj_reserved",
        &format!("<svelte:options customElement={{{{ tag: \"annotation-xml\" }}}} />\n{BUTTON}\n"),
        "svelte_options_reserved_tagname",
    );
}

#[test]
fn options_customelement_mixed_text_first_is_invalid_tagname() {
    // `customElement="a{c}b"` — a MIXED value (text + interpolation) whose first chunk is TEXT
    // is a multi-chunk value (`get_static_value` is null) on the Text branch →
    // `validate_tag(null)` → `svelte_options_invalid_tagname`.
    assert_code(
        "options_ce_mixed_text_first",
        &format!("<svelte:options customElement=\"a{{c}}b\" />\n{BUTTON}\n"),
        "svelte_options_invalid_tagname",
    );
}

#[test]
fn options_customelement_mixed_expr_first_is_invalid_customelement() {
    // `customElement="{c}b"` — a MIXED value whose first chunk is an EXPRESSION takes the
    // expression branch → `svelte_options_invalid_customelement`.
    assert_code(
        "options_ce_mixed_expr_first",
        &format!("<svelte:options customElement=\"{{c}}b\" />\n{BUTTON}\n"),
        "svelte_options_invalid_customelement",
    );
}

#[test]
fn options_customelement_null_expr_is_not_an_options_reject() {
    // `customElement={null}` — upstream accepts (backwards-compat skip); Verter refuses it later
    // as the 5h feature, NOT an options reject.
    assert_not_options_reject(
        "options_ce_null",
        &format!("<svelte:options customElement={{null}} />\n{BUTTON}\n"),
    );
}

#[test]
fn options_customelement_valid_object_expr_is_not_an_options_reject() {
    // `customElement={{ tag: "my-el" }}` — a valid object → upstream accepts; Verter refuses it
    // later as the 5h feature, NOT an options reject.
    assert_not_options_reject(
        "options_ce_valid_obj",
        &format!("<svelte:options customElement={{{{ tag: \"my-el\" }}}} />\n{BUTTON}\n"),
    );
}

#[test]
fn options_customelement_expr_fault_competes_in_source_order() {
    // `customElement={42} name="x"` — the EXPRESSION fault (customElement, earlier in source
    // order) wins over the later `name` unknown-attribute → `svelte_options_invalid_customelement`.
    assert_code(
        "options_ce_before_name",
        &format!("<svelte:options customElement={{42}} name=\"x\" />\n{BUTTON}\n"),
        "svelte_options_invalid_customelement",
    );
}

#[test]
fn options_clean_customelement_expr_lets_later_attribute_fault_win() {
    // `customElement={{ tag: "my-el" }} name="x"` — the customElement expression is CLEAN
    // (mints nothing), so the LATER `name` unknown-attribute wins → `svelte_options_unknown_attribute`.
    assert_code(
        "options_clean_ce_then_name",
        &format!(
            "<svelte:options customElement={{{{ tag: \"my-el\" }}}} name=\"x\" />\n{BUTTON}\n"
        ),
        "svelte_options_unknown_attribute",
    );
}

#[test]
fn options_malformed_customelement_expr_js_parse_error_beats_a_later_template_defect() {
    // A SYNTACTICALLY-malformed `customElement={<}` raises `js_parse_error` during the
    // `<svelte:options>` ELEMENT parse (upstream's `read_expression`), which is BEFORE a LATER
    // template defect (a stray `</div>`). So the `js_parse_error` wins — NOT the stray close.
    // (Verified pinned svelte@5.56.3: this source → `js_parse_error`.)
    assert_code(
        "options_ce_malformed_then_stray",
        &format!("<svelte:options customElement={{<}} />\n</div>\n{BUTTON}\n"),
        "js_parse_error",
    );
    // A LATER malformed `<style>` body likewise loses to the earlier customElement `js_parse_error`.
    assert_code(
        "options_ce_malformed_then_bad_style",
        "<svelte:options customElement={<} />\n<style>/*</style>\n<button>x</button>\n",
        "js_parse_error",
    );
}

#[test]
fn options_malformed_customelement_expr_js_parse_error_loses_to_an_earlier_template_defect() {
    // An EARLIER stray `</div>` (a template parse defect discovered first in the forward pass)
    // beats the customElement `js_parse_error` that follows it → `element_invalid_closing_tag`.
    // (Verified pinned svelte@5.56.3.)
    assert_code(
        "options_stray_then_ce_malformed",
        &format!("</div>\n<svelte:options customElement={{<}} />\n{BUTTON}\n"),
        "element_invalid_closing_tag",
    );
}

#[test]
fn options_customelement_validation_fault_loses_to_a_later_template_defect() {
    // A VALID-but-invalid `customElement={42}` (→ `svelte_options_invalid_customelement`) is a
    // `read_options` VALIDATION fault raised at FINALIZATION (after the whole template parse), so
    // it loses to a LATER stray `</div>` (which the validation fault could only beat if it rode the
    // element source position — it does NOT). (Verified pinned svelte@5.56.3:
    // `element_invalid_closing_tag`.)
    assert_code(
        "options_ce_validation_then_stray",
        &format!("<svelte:options customElement={{42}} />\n</div>\n{BUTTON}\n"),
        "element_invalid_closing_tag",
    );
    // And an EARLIER stray beats it too (it is not a syntactic parse fault at the element position).
    assert_code(
        "options_stray_then_ce_validation",
        &format!("</div>\n<svelte:options customElement={{42}} />\n{BUTTON}\n"),
        "element_invalid_closing_tag",
    );
}

#[test]
fn options_customelement_parse_fault_at_attribute_position_beats_a_later_duplicate_attr() {
    // `customElement={} foo foo` — the EMPTY-expression `js_parse_error` is a SYNTACTIC parse fault
    // raised at the `customElement` attribute's source position (upstream parses the value via
    // `read_expression` DURING the attribute loop), which precedes the LATER duplicate `foo foo`.
    // So the `js_parse_error` wins over `attribute_duplicate`. A finalization-positioned parse fault
    // (drawn AFTER the whole open tag) would WRONGLY lose to the duplicate minted in the loop.
    // (Verified pinned svelte@5.56.3: `js_parse_error`.)
    assert_code(
        "options_ce_empty_then_dup",
        &format!("<svelte:options customElement={{}} foo foo />\n{BUTTON}\n"),
        "js_parse_error",
    );
    // The `{1 2}` trailing-junk `expected_token` is ALSO a syntactic parse fault at the attribute
    // position, so it likewise beats the later duplicate. (Verified pinned: `expected_token`.)
    assert_code(
        "options_ce_one_two_then_dup",
        &format!("<svelte:options customElement={{1 2}} foo foo />\n{BUTTON}\n"),
        "expected_token",
    );
}

#[test]
fn options_customelement_parse_fault_loses_to_an_earlier_duplicate_attr() {
    // `foo foo customElement={}` — the duplicate `foo foo` is encountered BEFORE the `customElement`
    // attribute, so `attribute_duplicate` (minted first in the loop) beats the customElement
    // `js_parse_error` that follows it. This confirms the parse fault rides the ATTRIBUTE'S source
    // position (not a fixed pre-loop / post-loop position). (Verified pinned: `attribute_duplicate`.)
    assert_code(
        "options_dup_then_ce_empty",
        &format!("<svelte:options foo foo customElement={{}} />\n{BUTTON}\n"),
        "attribute_duplicate",
    );
}

#[test]
fn options_clean_customelement_expr_lets_a_later_duplicate_attr_win() {
    // `customElement={42} foo foo` — the customElement expression PARSES clean (its validation fault
    // rides finalization), so the duplicate `foo foo` minted in the loop wins → `attribute_duplicate`
    // (NOT the finalization `svelte_options_invalid_customelement`). (Verified pinned:
    // `attribute_duplicate`.)
    assert_code(
        "options_clean_ce_then_dup",
        &format!("<svelte:options customElement={{42}} foo foo />\n{BUTTON}\n"),
        "attribute_duplicate",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (4) Analyze scan order = module → instance → template. A module-script global reference
//     is reported BEFORE an instance-script defect.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn module_global_reference_beats_instance_legacy_props() {
    // `<script module>let x=$foo;</script><script>let c=$state(0); let y=$$props;</script>`
    // — upstream constructs the MODULE scope before the INSTANCE scope, so the module's
    // `$foo` (`global_reference_invalid`) is reported before the instance's `$$props`
    // (`legacy_props_invalid`). An instance-first scan order would WRONGLY report
    // `legacy_props_invalid`.
    assert_code(
        "module_global_ref_first",
        &format!(
            "<script module>let x = $foo;</script>\n<script>let c = $state(0); let y = $$props;</script>\n{BUTTON}\n"
        ),
        "global_reference_invalid",
    );
}

#[test]
fn quoted_gt_between_two_langs_rejects_with_attribute_duplicate_both_directions() {
    // The EXOTIC quoted-`>` lang corner ledgered as out-of-finite-scope: a quoted `>` between two
    // `lang=` attributes makes the internal grammar scan diverge from the official regex (Verter's
    // attribute-aware scan crosses the quoted `>`, the regex stops at it). That divergence is
    // UNOBSERVABLE end-to-end — the source carries TWO `lang=` attributes, so the gate rejects with
    // `attribute_duplicate` in BOTH directions, byte-identically to pinned svelte@5.56.3. This is
    // the end-to-end behavioral-parity lock for the ledgered corner.
    assert_code(
        "quoted_gt_then_lang_ts_dup",
        &format!("<script lang=js data-x=\">\" lang=\"ts\">let a = 1;</script>\n{BUTTON}\n"),
        "attribute_duplicate",
    );
    assert_code(
        "quoted_gt_then_lang_js_dup",
        &format!("<script lang=ts data-x=\">\" lang=\"js\">let a = 1;</script>\n{BUTTON}\n"),
        "attribute_duplicate",
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// POSITIVE CONTROLS — clean §1.2-core inputs MUST still be accepted by the gate (no
// over-rejection from the reordered validation / body-probe / template-dup mint).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn clean_section_1_2_core_is_accepted() {
    let parsed = parse_svelte(
        "<script>let name = $state('world'); let count = $state(0);</script>\n<h1>Hello {name}!</h1>\n<input bind:value={name} />\n<button onclick={() => count += 1}>clicks: {count}</button>\n",
    );
    assert!(
        official_reject_gate(
            "<script>let name = $state('world'); let count = $state(0);</script>\n<h1>Hello {name}!</h1>\n<input bind:value={name} />\n<button onclick={() => count += 1}>clicks: {count}</button>\n",
            &parsed,
        )
        .is_none(),
        "a clean §1.2-core component must NOT be rejected by the gate"
    );
}

#[test]
fn valid_module_context_script_is_accepted() {
    // A valid `<script context="module">` + an instance script — neither the source-order
    // attr validation nor the body-probe must over-reject the clean module/instance pair.
    let src = "<script context=\"module\">const K = 1;</script>\n<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
    let parsed = parse_svelte(src);
    assert!(
        official_reject_gate(src, &parsed).is_none(),
        "a valid `context=\"module\"` + instance script must be accepted"
    );
}

#[test]
fn empty_quoted_attribute_value_is_accepted() {
    // `id=""` is a VALID attribute (not a duplicate, not malformed) — the template-dup mint
    // must not flag a single valid attribute.
    let src = "<script>let c = $state(0);</script>\n<div id=\"\">x</div>\n<button onclick={() => c++}>{c}</button>\n";
    let parsed = parse_svelte(src);
    assert!(
        official_reject_gate(src, &parsed).is_none(),
        "a single valid `id=\"\"` attribute must be accepted"
    );
}

#[test]
fn lang_ts_lowercase_body_with_type_annotation_is_not_a_body_parse_error() {
    // NEGATIVE / grammar discrimination: a `lang="ts"` (EXACT lowercase) body with an ordinary
    // type annotation is VALID TypeScript — the parser-level script-body grammar is TS (set by
    // the first lowercase `<script lang="ts">`), so the body-probe parses it in TS grammar and
    // it is NOT a `js_parse_error`. (Verter may still refuse `lang="ts"` downstream as an
    // unsupported FEATURE, but the official-reject gate's body-probe must not mint a parse error
    // for valid TS.) Asserts the gate does not report `js_parse_error` for this source.
    let src = "<script lang=\"ts\">let a: number = 1;</script>\n<button>x</button>\n";
    let parsed = parse_svelte(src);
    let code = official_reject_gate(src, &parsed).map(|r| r.official_code);
    assert_ne!(
        code,
        Some("js_parse_error"),
        "a valid lowercase `lang=\"ts\"` type annotation must NOT be a body `js_parse_error`"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (F1) The script-body grammar is a SINGLE parser-wide flag, set ONLY by the FIRST
//      lowercase `<script ... lang="ts">` with an EXACT `ts` value (upstream's
//      `regex_lang_attribute` scan + `match?.[2] === 'ts'` in `Parser` constructor).
//      `lang="TS"` / `lang="tsx"` / `lang="typescript"` are NOT TS — a TS-only body under
//      any of those parses as JS ⇒ `js_parse_error`. A plain first script using TS syntax
//      PLUS a later `lang="ts"` script makes the WHOLE parse TS ⇒ that body parses clean.
//      (Confirmed against pinned svelte@5.56.3.)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn uppercase_lang_ts_with_type_annotation_is_a_body_parse_error() {
    // `lang="TS"` (uppercase) does NOT set the parser-wide TS flag — upstream's exact `=== 'ts'`
    // compare is case-sensitive — so the body parses as JS and the type annotation is
    // `js_parse_error`. (Verter previously accepted uppercase `TS` as TS and WRONGLY parsed it
    // clean; this is the case-sensitivity discrimination F1 fixes.)
    assert_code(
        "uppercase_lang_ts",
        "<script lang=\"TS\">let a: number = 1;</script>\n<button>x</button>\n",
        "js_parse_error",
    );
}

#[test]
fn lang_tsx_with_type_annotation_is_a_body_parse_error() {
    // `lang="tsx"` is NOT the exact `ts` value, so the parser-wide grammar stays JS and a
    // TS-only annotation is `js_parse_error` (upstream parses a `lang="tsx"` body with Acorn in
    // NON-TS mode — only the exact `ts` value flips `parser.ts`).
    assert_code(
        "lang_tsx",
        "<script lang=\"tsx\">let a: number = 1;</script>\n<button>x</button>\n",
        "js_parse_error",
    );
}

#[test]
fn lang_typescript_with_type_annotation_is_a_body_parse_error() {
    // `lang="typescript"` is NOT the exact `ts` value either → JS grammar → `js_parse_error`.
    assert_code(
        "lang_typescript",
        "<script lang=\"typescript\">let a: number = 1;</script>\n<button>x</button>\n",
        "js_parse_error",
    );
}

#[test]
fn plain_first_script_with_ts_syntax_then_later_lang_ts_parses_clean() {
    // A PLAIN first script using a TS-only type annotation PLUS a LATER `lang="ts"` module
    // script: the parser-wide grammar is set TS by the first-lowercase-`lang` scan (which finds
    // the module's `lang="ts"`, the first script WITH a `lang` attribute), so the EARLIER plain
    // script's body parses under TS too and is NOT a `js_parse_error`. Upstream ACCEPTS this
    // source — the gate must not mint a body parse error. (A per-script grammar would WRONGLY
    // parse the plain script as JS and report `js_parse_error`.)
    let src = "<script>let a: number = 1;</script>\n<script module lang=\"ts\">const b = 1;</script>\n<button>x</button>\n";
    let parsed = parse_svelte(src);
    let code = official_reject_gate(src, &parsed).map(|r| r.official_code);
    assert_ne!(
        code,
        Some("js_parse_error"),
        "a plain first script + a later lowercase `lang=\"ts\"` makes the whole parse TS — the \
         earlier plain body must NOT be a `js_parse_error`"
    );
}
