/**
 * The PARSER-STRICTNESS PARITY corpus generator.
 *
 * Verter's Svelte tokenizer is intentionally infallible / recovery-based: it never
 * panics and always emits a faithful tree, even on malformed markup. That recovery is
 * correct for the IDE projection (it owns its own error recovery), but for the CLIENT
 * runtime the contract is "Verter emits a `Main` ⇔ official `svelte@5.56.3` ACCEPTS the
 * same source". A recovery point that ACCEPTS markup official REJECTS would emit a
 * divergent module — a behavioral divergence. Hand-auditing the recovery points has
 * repeatedly missed leaks, so this generator mechanically ENUMERATES malformed (and
 * accepted-control) markup across the parser's perturbation axes, runs the PINNED
 * official compiler over each, and records its disposition — producing a committed
 * corpus the Rust gate uses to assert, per fixture:
 *   - official REJECTED  ⇒ Verter returns NO `Main` (the strict-parse gate refused).
 *   - official ACCEPTED  ⇒ Verter emits a `Main` (the strict gate must not over-reject).
 *
 * Beyond the original per-axis perturbation controls, the generator SYSTEMATICALLY enumerates
 * the finite Block-4 parser-strictness axes (loops over fixed token sets, NOT hand-found
 * examples): the full `REGEX_NTH_OF` An+B matrix, the `read/style.js` parse-entry code set, the
 * `<svelte:options customElement>` value + source-order arbitration space, the open-tag
 * duplicate-timing forms, and the script-`lang` finite forms. Each row carries an `axis` tag, and
 * the writer/`--check` asserts every `REQUIRED_AXES` entry contributes ≥1 row (a dropped
 * enumerator fails HARD), so the corpus cannot silently lose a finite axis to a hand-audit gap. A
 * Rust COVERAGE gate enforces the same `required_axes` contract on the committed corpus.
 *
 * Sibling of `gen-svelte-diff-corpus.mjs`; reuses the SHARED `loadPinnedCompiler` from
 * `svelte-golden-lib.mjs` (the single oracle pin). Writes a NEW `parse_parity/` subtree
 * (NOT the reject corpus, NOT the differential corpus). Each fixture is wrapped in a
 * minimal §1.2-core scaffold so an ACCEPTED control is also a §1.2-core `Main`.
 *
 * USAGE
 *   node scripts/gen-svelte-parse-parity-corpus.mjs           # rewrite the corpus
 *   node scripts/gen-svelte-parse-parity-corpus.mjs --check   # assert in sync (CI gate)
 */

import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

import { loadPinnedCompiler, SVELTE_ORACLE_VERSION } from "./svelte-golden-lib.mjs";

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(__dirname, "..");
const CORPUS_ROOT = join(REPO_ROOT, "crates/verter_compiler/tests/svelte_oracle_corpus");
// The parse-parity corpus is its OWN subtree (segregated from the reject corpus and the
// generated differential corpus): a `<name>.svelte` fixture + a flat `<name>.json`
// metadata `{ "disposition": "accepted" | "rejected", "official_code": "<code>" }`.
const PARSE_PARITY_DIR = join(CORPUS_ROOT, "parse_parity");

// ---------------------------------------------------------------------------
// §1.2-core scaffold
// ---------------------------------------------------------------------------

// Wrap a TEMPLATE fragment in the §1.2-core scaffold (an instance `<script>` with a
// single `$state` + a supported reactive `<button>`), so an ACCEPTED fragment yields a
// genuine §1.2-core `Main`. The `extra` lines are appended after the button.
function scaffoldTemplate(fragment) {
  return `<script>let c = $state(0);</script>\n${fragment}\n<button onclick={() => c++}>{c}</button>\n`;
}

// Wrap a SCRIPT-tag perturbation (the whole `<script …>…</script>` is the fixture) plus
// a supported `<button>` template, so an accepted script form is a §1.2-core `Main`.
function scaffoldScript(scriptTag) {
  return `${scriptTag}\n<button onclick={() => c++}>{c}</button>\n`;
}

// ---------------------------------------------------------------------------
// Perturbation axes — each a family of `{ axis, slug, source }` fixtures.
//
// The set is DETERMINISTIC and order-stable. Each axis exercises one parser recovery
// surface; both REJECTED perturbations (the leak class) and ACCEPTED controls (so the
// strict gate is proven NOT to over-reject) are included. The pinned compiler decides
// each one's disposition at generation time — the lists below are the INPUT space, not a
// pre-judged verdict.
// ---------------------------------------------------------------------------

function fixtureCases() {
  const cases = [];
  // `core` classifies the fixture's SURFACE (ignoring any malformation) as within
  // Verter's §1.2-core supported surface. An ACCEPTED `core` fixture therefore emits a
  // §1.2-core `Main`; an ACCEPTED NON-core fixture is officially accepted but uses a
  // surface Verter does not yet support (a top-level / nested `<style>` FEATURE, a
  // non-allowlisted `hidden` attribute), so it fails closed as an unsupported feature
  // (never a parser-strictness reject). It defaults to `true` (the §1.2-core scaffold);
  // the rare NON-core fixtures pass `false` explicitly. (For a REJECTED fixture the flag
  // is informational — the disposition forces fail-closed regardless.)
  const add = (axis, slug, source, core = true) => cases.push({ axis, slug, source, core });

  // ── text / raw-token axis ──────────────────────────────────────────────────
  add("text", "raw_lt_space", scaffoldTemplate("<p>a < b</p>"));
  add("text", "raw_lt_dot", scaffoldTemplate("<p>a <.b</p>"));
  add("text", "raw_lt_brace", scaffoldTemplate("<p>a <} b</p>"));
  add("text", "raw_lt_digit", scaffoldTemplate("<p>a <1 b</p>"));
  add("text", "raw_lt_eof", scaffoldTemplate("<p>ok</p>") + "<");
  add("text", "lt_slash_eof", scaffoldTemplate("<p>ok</p>") + "</");
  add("text", "lt_bang_text", scaffoldTemplate("<p>a <!x b</p>"));
  // `a &amp; b` is officially ACCEPTED but the `&amp;` ENTITY makes it a complex text
  // chunk Verter does not support in §1.2-core (NON-core): it fails closed as an
  // unsupported feature.
  add("text", "valid_amp_entity", scaffoldTemplate("<p>a &amp; b</p>"), false); // ACCEPT control, NON-core
  add("text", "valid_gt_in_text", scaffoldTemplate("<p>a > b</p>")); // ACCEPT control (bare `>`)

  // ── open-tag axis ──────────────────────────────────────────────────────────
  add(
    "open_tag",
    "open_no_gt_eof",
    "<script>let c = $state(0);</script>\n<button onclick={() => c++",
  );
  add("open_tag", "empty_name_slash", scaffoldTemplate("</ div>"));
  // `<svelte:nope>` is a SPECIAL element outside the §1.2-core surface (NON-core) — and
  // official rejects it (`svelte_meta_invalid_tag`), so Verter fails it closed via the
  // unsupported-feature path, NOT a parser-strictness reject.
  add("open_tag", "invalid_svelte_special", scaffoldTemplate("<svelte:nope></svelte:nope>"), false);
  add("open_tag", "valid_self_close", scaffoldTemplate("<input />")); // ACCEPT control
  add("open_tag", "valid_div", scaffoldTemplate("<div>x</div>")); // ACCEPT control

  // ── close-tag axis ─────────────────────────────────────────────────────────
  add("close_tag", "nameless", scaffoldTemplate("<div>x</></div>"));
  add("close_tag", "trailing_token", scaffoldTemplate("<div>x</div y>"));
  add("close_tag", "trailing_slash", scaffoldTemplate("<div>x</div/>"));
  add("close_tag", "stray_root", scaffoldTemplate("</section>"));
  add("close_tag", "mismatch", scaffoldTemplate("<div>x</span></div>"));
  add("close_tag", "void_close", scaffoldTemplate("<input></input>"));
  add("close_tag", "close_eof_no_gt", "<script>let c = $state(0);</script>\n<div>x</div");
  add("close_tag", "valid_ws_in_close", scaffoldTemplate("<div>x</div >")); // ACCEPT control

  // ── attribute axis ─────────────────────────────────────────────────────────
  add("attribute", "empty_value", scaffoldTemplate("<div id=>x</div>"));
  add("attribute", "empty_value_ws", scaffoldTemplate("<div id= >x</div>"));
  add("attribute", "empty_value_eof", "<script>let c = $state(0);</script>\n<div id=");
  add("attribute", "unterminated_quote", scaffoldTemplate('<div id="oops>x</div>'));
  add("attribute", "valid_empty_quoted", scaffoldTemplate('<div id="">x</div>')); // ACCEPT control
  // `<div hidden>` is officially ACCEPTED but `hidden` is NOT in Verter's §1.2-core
  // static-attr allowlist (NON-core) — it fails closed as an unsupported feature.
  add("attribute", "valid_boolean", scaffoldTemplate("<div hidden>x</div>"), false); // ACCEPT control, NON-core
  add("attribute", "valid_unquoted", scaffoldTemplate("<div id=x>y</div>")); // ACCEPT control

  // ── script / style open-tag axis ───────────────────────────────────────────
  add("script", "empty_lang_value", scaffoldScript("<script lang=>let c = $state(0);</script>"));
  add("script", "close_trailing_token", scaffoldScript("<script>let c = $state(0);</script x>"));
  add(
    "script",
    "unterminated",
    "<script>let c = $state(0);\n<button onclick={() => c++}>{c}</button>\n",
  );
  // The `<style>` axis: every `<style>`-bearing fixture is NON-core (a top-level or
  // nested `<style>` is an unsupported FEATURE in Verter's §1.2-core surface). An
  // ACCEPTED style fixture therefore fails closed as an unsupported feature (NOT a
  // parser-strictness reject); a malformed one rejects on its own parse-phase code.
  add("style", "unterminated", scaffoldTemplate("<style>.a { color: red;"), false);
  add("style", "close_trailing_token", scaffoldTemplate("<style>.a {}</style x>"), false);
  add(
    "style",
    "nested_trailing_token",
    scaffoldTemplate("<div><style>.a {}</style x></div>"),
    false,
  );
  add("style", "nested_clean", scaffoldTemplate("<div><style>.a {}</style></div>"), false);

  // ── comment axis ───────────────────────────────────────────────────────────
  add("comment", "unterminated", scaffoldTemplate("<!-- oops"));
  add("comment", "valid", scaffoldTemplate("<!-- ok -->")); // ACCEPT control
  // The EMPTY `<!--` lead at EOF is `unexpected_eof` (cut off immediately), distinct from a
  // STARTED-but-unterminated `<!-- oops` which is `expected_token`. Bare (un-scaffolded) so
  // the comment is the LAST construct at EOF.
  add("comment", "empty_lead_eof", "<script>let c = $state(0);</script>\n<!--");

  // ── unquoted-value / self-close boundary axis ──────────────────────────────
  // A LEADING `/` in an unquoted value is a value byte (per official's unquoted-value
  // reader), so `id=/>` is `id="/"` + a NORMAL `>` close (the element stays open ⇒
  // `element_unclosed`), NOT a self-close — whereas `id=x/>` reads value `x` then
  // self-closes (ACCEPT). The pinned compiler decides each disposition.
  add("attribute", "unquoted_leading_slash", scaffoldTemplate("<div id=/><span>x</span>"));
  add("attribute", "unquoted_leading_slash_lang", scaffoldTemplate("<div lang=/><span>x</span>"));
  add("attribute", "unquoted_value_then_self_close", scaffoldTemplate("<div id=x/>")); // ACCEPT control

  // ── truncated open-tag / attribute EOF axis (exact construct → exact code) ──
  // A truncated INTRINSIC open tag at EOF reaches end of input ⇒ `unexpected_eof`; a
  // SPECIAL-block `attr=` truncated at EOF is `expected_attribute_value`. Bare so the
  // truncation is the last construct.
  add("open_tag", "intrinsic_truncated_eof", "<script>let c = $state(0);</script>\n<div");
  add("open_tag", "intrinsic_truncated_name_eof", "<script>let c = $state(0);</script>\n<div id");
  add("open_tag", "script_truncated_eof", "<div>x</div>\n<script");
  add("open_tag", "style_truncated_eof", "<div>x</div>\n<style", false);
  add("script", "lang_eq_eof", "<div>x</div>\n<script lang=");
  add("style", "lang_eq_eof", "<div>x</div>\n<style lang=", false);

  // ── self-closing special block axis ────────────────────────────────────────
  // A self-closing `<script />` / `<style />` is official `expected_token` (a bare `/>`
  // where the raw body's `>` is expected). NON-core (`<script>`/`<style>` blocks are not in
  // the §1.2-core surface), but the strict gate must still reject the malformed self-close.
  add(
    "script",
    "self_closing",
    "<svelte:options runes={true} /><script /><button onclick={() => {}}>x</button>\n",
    false,
  );
  add(
    "style",
    "self_closing",
    "<svelte:options runes={true} /><style /><button onclick={() => {}}>x</button>\n",
    false,
  );

  // ── nested / top-level <style> raw-close axis (longer name / whitespace) ────
  // A NESTED `<style>` close is the LITERAL `</style>` only — a longer-name continuation
  // (`</stylefoo>`) is body text (the later `</style>` closes ⇒ ACCEPT), while reaching EOF
  // with no clean close is `expected_token`. A TOP-LEVEL `<style>` CSS reader matches the
  // `</style` PREFIX and consumes to `>`, so a longer-name continuation / whitespace-before-
  // `>` close is ACCEPTED. All `<style>`-bearing fixtures are NON-core.
  add("style", "top_level_longer_name_close", scaffoldTemplate("<style>.a {}</stylefoo>"), false);
  add("style", "top_level_hyphen_name_close", scaffoldTemplate("<style>.a {}</style-x>"), false);
  add("style", "top_level_ws_before_gt_close", scaffoldTemplate("<style>.a {}</style >"), false);
  add(
    "style",
    "nested_longer_name_then_clean_close",
    scaffoldTemplate("<div><style>.a {}</stylefoo></style></div>"),
    false,
  );
  add("style", "nested_unterminated_eof", "<div><style>.a {}</style", false);

  // ── defect-encounter-order arbitration axis ────────────────────────────────
  // Multi-defect fixtures: the official compiler stops at the FIRST parse error in its
  // single forward pass, so the pinned-compiler disposition pins exactly which defect wins.
  // These exercise the gate's encounter-order arbitration (the FIRST-discovered parse defect
  // wins; the analyze-phase placement check runs only on a clean parse). The pinned compiler
  // decides each disposition at generation time — these are INPUT, not a pre-judged verdict.
  // An inner stray / void-content close DISCOVERED before the outer element is proven
  // unclosed at EOF wins over the outer `element_unclosed`. The void fixture leaves the
  // outer `<div>` UNCLOSED (no `</div>`) so the inner void-content close (`</input>`,
  // discovered first) genuinely competes against the outer EOF-unclosed — the case
  // discriminates encounter-order arbitration (a span-min / closed-`<div>` variant would not).
  add("arbitration", "inner_stray_vs_outer_unclosed", scaffoldTemplate("<div></span>"));
  add("arbitration", "inner_void_vs_outer_unclosed", scaffoldTemplate("<div><input></input>"));
  // A PARSE defect (an empty-attr-value `<div bar=>`) beats a nested-`<a>` PLACEMENT defect
  // (placement is an analyze-phase check, gated on a clean parse).
  add(
    "arbitration",
    "placement_then_parse_strict",
    scaffoldTemplate('<a href="/"><a href="/x">x</a></a>\n<div bar=></div>'),
  );
  // A trailing stray `</div>` parse defect makes the parse unclean ⇒ the nested-`<button>`
  // placement check never runs.
  add(
    "arbitration",
    "placement_then_parse_close",
    scaffoldTemplate("<button><button>x</button></button></div>"),
  );
  // An inner empty-attr-value parse defect inside `<p>` beats the surviving-`</p>` autoclose.
  add("arbitration", "inner_parse_vs_p_autoclose", scaffoldTemplate("<p><div id=></div></p>"));
  // The surviving-`</p>` autoclose (discovered at the `</p>` close) beats a LATER trailing stray.
  add(
    "arbitration",
    "p_autoclose_vs_later_stray",
    scaffoldTemplate("<p><div>x</div></p>\n</span>"),
  );
  // A template close defect (a stray `</span>`) DISCOVERED before a later script-domain reject
  // wins. Custom source (a leading stray close before the bad script + the supported button).
  add(
    "arbitration",
    "template_close_vs_later_script",
    '</span>\n<script context="bad">let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n',
  );
  // The MODULE script's reserved attr (discovered first in source order) beats the later
  // instance script's invalid context (NOT an instance-before-module pre-pass).
  add(
    "arbitration",
    "module_reserved_vs_instance_context",
    '<script module server>const K = 1;</script>\n<script context="bad">let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n',
  );

  // ── script-body grammar axis (F1) ──────────────────────────────────────────
  // Upstream sets ONE parser-wide `ts` flag from the FIRST lowercase `<script lang="ts">` (an
  // exact `ts` value). A TS-only body under `lang="TS"` / `lang="tsx"` / `lang="typescript"`
  // (NOT the exact value) parses as JS → `js_parse_error`; a plain first script with TS syntax
  // PLUS a later `lang="ts"` is the whole-parse-is-TS ACCEPT. All NON-core (a `<script lang>` /
  // TS body is outside the §1.2-core plain-JS surface). The pinned compiler decides disposition.
  add(
    "script_grammar",
    "lang_uppercase_ts_body",
    '<script lang="TS">let a: number = 1;</script>\n<button>x</button>\n',
    false,
  );
  add(
    "script_grammar",
    "lang_tsx_body",
    '<script lang="tsx">let a: number = 1;</script>\n<button>x</button>\n',
    false,
  );
  add(
    "script_grammar",
    "lang_typescript_body",
    '<script lang="typescript">let a: number = 1;</script>\n<button>x</button>\n',
    false,
  );
  add(
    "script_grammar",
    "plain_ts_then_module_lang_ts",
    '<script>let a: number = 1;</script>\n<script module lang="ts">const b = 1;</script>\n<button>x</button>\n',
    false,
  );

  // ── script-lang RAW-SUBSTRING / RIGHTMOST selection axis (F-lang) ───────────
  // Upstream selects the parser-wide `ts` flag with a constructor regex
  // (`phases/1-parse/index.js`) that matches `lang=` as a RAW SUBSTRING anywhere in a
  // `<script …>` open tag — RIGHTMOST effective match, INCLUDING inside quoted values — NOT
  // an attribute-name-boundary first-occurrence scan. Each fixture carries a TS-ONLY body
  // (`let a: number = 1;`) so the disposition pivots EXACTLY on whether the lang scan selects
  // TS: TS-selected ⇒ the body parses (ACCEPT); JS-selected ⇒ `let a: number` is `js_parse_error`.
  // All NON-core (a `<script lang>` / TS body is outside the §1.2-core plain-JS surface). The
  // pinned compiler decides each disposition at generation time.
  // A `lang=` substring inside an UNRELATED quoted attribute value selects TS (`data-lang="ts"`,
  // `foo="lang=ts"`).
  add(
    "script_lang",
    "data_lang_quoted_ts_selects_ts",
    '<script data-lang="ts">let a: number = 1;</script>\n<button>x</button>\n',
    false,
  );
  add(
    "script_lang",
    "quoted_value_lang_ts_selects_ts",
    '<script foo="lang=ts">let a: number = 1;</script>\n<button>x</button>\n',
    false,
  );
  // RIGHTMOST wins: a later `data-lang="ts"` substring overrides an earlier `lang="js"` (→ TS,
  // ACCEPT); a later `data-lang="js"` overrides an earlier `lang="ts"` (→ JS, `js_parse_error`).
  add(
    "script_lang",
    "lang_js_then_data_lang_ts_rightmost_ts",
    '<script lang="js" data-lang="ts">let a: number = 1;</script>\n<button>x</button>\n',
    false,
  );
  add(
    "script_lang",
    "lang_ts_then_data_lang_js_rightmost_js",
    '<script lang="ts" data-lang="js">let a: number = 1;</script>\n<button>x</button>\n',
    false,
  );
  // An UNQUOTED `lang=ts` value selects TS.
  add(
    "script_lang",
    "unquoted_lang_ts_selects_ts",
    "<script lang=ts>let a: number = 1;</script>\n<button>x</button>\n",
    false,
  );
  // A trailing space INSIDE the quotes (`lang="ts "`) fails the regex `\1` close, so the lang
  // attribute does NOT match `ts` → JS → `js_parse_error`.
  add(
    "script_lang",
    "lang_ts_trailing_space_selects_js",
    '<script lang="ts ">let a: number = 1;</script>\n<button>x</button>\n',
    false,
  );
  // The scan is CASE-SENSITIVE (the regex is flag-less): `LANG="ts"` does not match → JS →
  // `js_parse_error`.
  add(
    "script_lang",
    "uppercase_lang_name_selects_js",
    '<script LANG="ts">let a: number = 1;</script>\n<button>x</button>\n',
    false,
  );
  // Control: a plain `lang="ts"` selects TS (the body parses).
  add(
    "script_lang",
    "plain_lang_ts_selects_ts",
    '<script lang="ts">let a: number = 1;</script>\n<button>x</button>\n',
    false,
  );

  // ── style-body-before-duplicate axis (F2) ──────────────────────────────────
  // Upstream `read_style` parses the CSS body (and can throw) BEFORE `style_duplicate`, so a
  // malformed 2nd `<style>` body wins the first-error race; a clean/empty 2nd style → the later
  // `style_duplicate`. A single malformed style is its own CSS parse code. All NON-core (any
  // top-level `<style>` is an unsupported FEATURE). The pinned compiler decides each disposition.
  add(
    "style_body",
    "single_malformed_unterminated_rule",
    scaffoldTemplate("<style>.b {</style>"),
    false,
  );
  add(
    "style_body",
    "second_malformed_beats_duplicate",
    scaffoldTemplate("<style>.a {}</style>\n<style>.b {</style>"),
    false,
  );
  add(
    "style_body",
    "second_empty_declaration_beats_duplicate",
    scaffoldTemplate("<style>.a {}</style>\n<style>.b { color }</style>"),
    false,
  );
  add(
    "style_body",
    "first_malformed_wins_over_second_duplicate",
    scaffoldTemplate("<style>.a {</style>\n<style>.b {}</style>"),
    false,
  );
  add(
    "style_body",
    "second_clean_is_duplicate",
    scaffoldTemplate("<style>.a {}</style>\n<style>.c { color: red; }</style>"),
    false,
  );

  // ── read_style COMMENT-CLOSE parse-entry axis (F-css-comment) ───────────────
  // Upstream `read_style`'s `allow_comment_or_whitespace` uses REQUIRED close tokens
  // (`eat('*/', true)` / `eat('-->', true)`): an unterminated CSS comment raises `expected_token`
  // at the read_style parse entry (BEFORE `style_duplicate`). All NON-core (any `<style>` is an
  // unsupported FEATURE). The pinned compiler decides each disposition.
  add("style_body", "comment_unterminated_single", scaffoldTemplate("<style>/*</style>"), false);
  add(
    "style_body",
    "comment_unterminated_second_beats_duplicate",
    scaffoldTemplate("<style>p{}</style>\n<style>/*</style>"),
    false,
  );
  add(
    "style_body",
    "html_comment_unterminated_single",
    scaffoldTemplate("<style><!--</style>"),
    false,
  );

  // ── read_style NTH-OF parse-entry axis (F-css-nthof) ───────────────────────
  // Upstream `REGEX_NTH_OF` (`read/style.js`) includes the `\s+of\s+` arm, so
  // `:nth-child(<An+B> of <selector>)` PARSES clean. A single such style is officially ACCEPTED
  // (Verter refuses it as the unsupported `<style>` FEATURE); a second one is `style_duplicate`
  // (the clean parse reaches the duplicate check). All NON-core. The pinned compiler decides.
  add(
    "style_body",
    "nth_of_selector_single_accepts",
    scaffoldTemplate("<style>p:nth-child(2n+1 of .x){}</style>"),
    false,
  );
  add(
    "style_body",
    "nth_of_selector_second_is_duplicate",
    scaffoldTemplate("<style>p{}</style>\n<style>p:nth-child(2n+1 of .x){}</style>"),
    false,
  );
  add(
    "style_body",
    "nth_of_even_keyword_second_is_duplicate",
    scaffoldTemplate("<style>p{}</style>\n<style>p:nth-child(even of .x){}</style>"),
    false,
  );

  // ── <svelte:options> read_options axis (F3) ────────────────────────────────
  // Upstream's `read_options` (parse finalization) validates options attributes in source order
  // then disallows children; each fault is an exact `svelte_options_*` / `svelte_meta_invalid_content`
  // code, while an officially-ACCEPTED axis (a valid `namespace`/`css`, a boolean `runes`, a valid
  // `customElement`) is accepted (Verter refuses it as an unsupported FEATURE). All NON-core. The
  // pinned compiler decides each disposition.
  const optScaffold = (frag) =>
    `${frag}<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n`;
  // REJECTED options forms (exact codes).
  add("options", "name_unknown_attribute", optScaffold('<svelte:options name="x" />\n'), false);
  add("options", "bad_namespace_value", optScaffold('<svelte:options namespace="bad" />\n'), false);
  add("options", "bad_css_value", optScaffold('<svelte:options css="external" />\n'), false);
  add("options", "string_runes_value", optScaffold('<svelte:options runes="true" />\n'), false);
  add("options", "nonliteral_runes_value", optScaffold("<svelte:options runes={foo} />\n"), false);
  add("options", "deprecated_tag", optScaffold('<svelte:options tag="my-el" />\n'), false);
  add(
    "options",
    "customelement_bad_tagname",
    optScaffold('<svelte:options customElement="nodash" />\n'),
    false,
  );
  add(
    "options",
    "customelement_shorthand",
    optScaffold("<svelte:options customElement />\n"),
    false,
  );
  add(
    "options",
    "customelement_number_expr",
    optScaffold("<svelte:options customElement={42} />\n"),
    false,
  );
  // A SYNTACTICALLY-MALFORMED `customElement={EXPR}` fails during attribute-expression PARSING
  // (`read_expression` → acorn) with `js_parse_error`, BEFORE `read_options` ever inspects the
  // value. Verified pinned: `{{ tag: }}`, `{<}`, and `{}` (empty) all raise `js_parse_error`.
  add(
    "options",
    "customelement_malformed_expr_tag_colon",
    optScaffold("<svelte:options customElement={{ tag: }} />\n"),
    false,
  );
  add(
    "options",
    "customelement_malformed_expr_bare_lt",
    optScaffold("<svelte:options customElement={<} />\n"),
    false,
  );
  add(
    "options",
    "customelement_malformed_expr_empty",
    optScaffold("<svelte:options customElement={} />\n"),
    false,
  );
  // The customElement EXPRESSION-PARSE `js_parse_error` rides the `<svelte:options>` element's
  // source position (upstream's `read_expression`), so it BEATS a LATER template defect (a stray
  // `</div>`) and LOSES to an EARLIER one — while a `read_options` VALIDATION fault rides
  // finalization (after the whole parse) and loses to ANY template defect. The pinned compiler
  // pins each disposition.
  add(
    "options",
    "customelement_malformed_expr_beats_later_stray_close",
    `<svelte:options customElement={<} />\n</div>\n${optScaffold("")}`,
    false,
  );
  add(
    "options",
    "customelement_malformed_expr_loses_to_earlier_stray_close",
    `</div>\n<svelte:options customElement={<} />\n${optScaffold("")}`,
    false,
  );
  add(
    "options",
    "customelement_validation_fault_loses_to_later_stray_close",
    `<svelte:options customElement={42} />\n</div>\n${optScaffold("")}`,
    false,
  );
  add(
    "options",
    "customelement_object_bad_tag",
    optScaffold('<svelte:options customElement={{ tag: "nodash" }} />\n'),
    false,
  );
  add(
    "options",
    "customelement_object_bad_props",
    optScaffold("<svelte:options customElement={{ props: 1 }} />\n"),
    false,
  );
  add(
    "options",
    "customelement_object_bad_shadow",
    optScaffold("<svelte:options customElement={{ shadow: 1 }} />\n"),
    false,
  );
  add(
    "options",
    "customelement_mixed_text_first",
    optScaffold('<svelte:options customElement="a{x}b" />\n'),
    false,
  );
  add(
    "options",
    "customelement_mixed_expr_first",
    optScaffold('<svelte:options customElement="{x}b" />\n'),
    false,
  );
  add("options", "spread_attribute", optScaffold("<svelte:options {...opts} />\n"), false);
  add("options", "directive_attribute", optScaffold("<svelte:options bind:x={y} />\n"), false);
  add(
    "options",
    "child_content",
    optScaffold("<svelte:options runes={true}>hi</svelte:options>\n"),
    false,
  );
  add(
    "options",
    "first_fault_in_source_order",
    optScaffold('<svelte:options name="x" namespace="bad" />\n'),
    false,
  );
  // ACCEPTED-but-unsupported options axes (Verter fails closed as a FEATURE, not a parser-strictness
  // reject — the strict gate must not over-reject these).
  add("options", "valid_svg_namespace", optScaffold('<svelte:options namespace="svg" />\n'), false);
  add("options", "injected_css", optScaffold('<svelte:options css="injected" />\n'), false);
  add("options", "runes_false_legacy", optScaffold("<svelte:options runes={false} />\n"), false);
  add(
    "options",
    "valid_customelement_string",
    optScaffold('<svelte:options customElement="my-el" />\n'),
    false,
  );
  add(
    "options",
    "valid_customelement_object",
    optScaffold('<svelte:options customElement={{ tag: "my-el" }} />\n'),
    false,
  );
  add(
    "options",
    "customelement_null_expr",
    optScaffold("<svelte:options customElement={null} />\n"),
    false,
  );

  // ── MECHANICAL FINITE-AXIS ENUMERATION ──────────────────────────────────────
  // The hand-listed fixtures above are the original controls; the calls below
  // SYSTEMATICALLY enumerate the finite Block-4 parser-strictness axes (the An+B grammar, the
  // read_style parse-entry code set, the <svelte:options> customElement value + source-order
  // arbitration space, the open-tag duplicate-timing forms, and the script-lang finite forms) so
  // the corpus is a REAL gate — no in-contract grammar row can be lost to a hand-audit gap. Each
  // enumerator emits `add(axis, slug, source, core)` rows over a fixed token set; the pinned
  // compiler decides each disposition at generation time. `REQUIRED_AXES` (below) asserts every
  // enumerated axis contributes ≥1 row, so a dropped enumerator fails `--check`.
  enumerateNthOfMatrix(add);
  enumerateReadStyleParseEntry(add);
  enumerateOptionsCustomElement(add);
  enumerateOpenTagDuplicateTiming(add);
  enumerateScriptLangForms(add);

  return cases;
}

// ---------------------------------------------------------------------------
// Mechanical finite-axis enumerators (loops over fixed token sets — NOT hand-found
// examples). Each row is wrapped in the §1.2-core scaffold (or a `<style>` / options /
// script scaffold) and NON-core where the surface itself is unsupported (a `<style>` /
// `<svelte:options>` / `<script lang>` feature), so an ACCEPTED enumerated row fails closed
// as an unsupported feature rather than over-rejecting. The pinned compiler decides
// disposition at generation time — these lists are the finite INPUT space.
// ---------------------------------------------------------------------------

// The shared `<button>` template used by the options / script-lang scaffolds (a supported
// §1.2-core reactive button).
const BUTTON = "<button onclick={() => c++}>{c}</button>";

// The REQUIRED finite axes: `--check` (and the writer) asserts every one contributes at least
// one generated row, so a dropped enumerator is a HARD failure (the corpus would silently lose a
// finite axis). The `nth_of` / `read_style` / `options_ce` / `dup_timing` / `script_lang_enum`
// axes are the enumerated ones; the original hand-listed axes are NOT required here (they are
// supplementary controls, not the mechanical coverage contract).
const REQUIRED_AXES = ["nth_of", "read_style", "options_ce", "dup_timing", "script_lang_enum"];

// (A) The full `REGEX_NTH_OF` An+B matrix inside `:nth-child(<token>)`, plus the `of <selector>`
// arm. Enumerated over keyword / integer / `n` / `An±B` / negative-form / whitespace / malformed
// tokens, so every accepted form AND every rejected bare-negative / dangling-sign form is a row.
function enumerateNthOfMatrix(add) {
  // Each token sits inside `:nth-child(<token>)`; the pinned compiler decides accept/reject.
  const tokens = [
    // keywords
    "even",
    "odd",
    // plain integers (B only)
    "0",
    "1",
    "2",
    "10",
    "+0",
    "+1",
    "+2",
    // rejected bare-negative integers (no `n`)
    "-1",
    "-2",
    "-10",
    // `n` forms (A=±1)
    "n",
    "+n",
    "-n",
    // `An` forms
    "2n",
    "+2n",
    "-2n", // rejected: negative `An` with no `+b`
    "0n",
    // `An+B` / `An-B`
    "n+0",
    "n-0",
    "n+1",
    "n-1",
    "2n+1",
    "2n-1",
    "2n+0",
    "+2n+1",
    "+2n-1",
    // negative-form: only `-…n+b` is valid; `-…n-b` falls through to identifier
    "-2n+1",
    "-2n-1", // rejected: negative `An` with a `-b` offset
    "-n+1",
    "-n-1",
    "-n+2",
    // whitespace inside the offset
    "2n + 1",
    "2n  +  1",
    // malformed: dangling sign
    "2n+",
    "n+",
  ];
  for (const tok of tokens) {
    add(
      "nth_of",
      `bare_${nthSlug(tok)}`,
      scaffoldTemplate(`<style>p:nth-child(${tok}){}</style>`),
      false,
    );
  }
  // The `\s+of\s+<selector>` arm over a representative accepted-token subset.
  for (const tok of ["2n", "even", "odd", "-2n+1", "n", "2n+1"]) {
    add(
      "nth_of",
      `of_selector_${nthSlug(tok)}`,
      scaffoldTemplate(`<style>p:nth-child(${tok} of .x){}</style>`),
      false,
    );
  }
}

// A filesystem-safe slug for an An+B token, distinct per token (incl. distinct whitespace-run
// widths so `2n + 1` and `2n  +  1` do not collide): each run of `k` spaces becomes `ws<k>`.
function nthSlug(tok) {
  return tok
    .replace(/ +/g, (m) => `_ws${m.length}_`)
    .replace(/\+/g, "_plus")
    .replace(/^-/, "neg")
    .replace(/-/g, "_minus")
    .replace(/_+/g, "_")
    .replace(/^_|_$/g, "");
}

// (B) The `read/style.js` PARSE-ENTRY code set: each fixture is a `<style>` body chosen to hit a
// specific parse-entry code (`css_expected_identifier` / `css_empty_declaration` /
// `css_selector_invalid` / `expected_token` / `unexpected_eof`), in BOTH single and
// second-style-duplicate-competition positions (a malformed 2nd body's parse-entry code beats the
// `style_duplicate`; a clean 2nd body IS the duplicate).
function enumerateReadStyleParseEntry(add) {
  // body → the parse-entry code the body is chosen to surface (informational; the oracle decides).
  const bodies = [
    ["expected_ident_lone_dot", ".{}"],
    ["expected_ident_bare_at", "@ {}"],
    ["expected_ident_leading_digit", "1px {}"],
    ["expected_ident_global_paren", ":global() {}"],
    ["expected_ident_nth_bare_negative", "p:nth-child(-2) {}"],
    ["empty_declaration", ".a { color }"],
    ["selector_invalid_dangling_combinator", ".a > {}"],
    ["selector_invalid_nth_dangling_sign", "p:nth-child(n+) {}"],
    ["expected_token_unterminated_block_comment", "/*"],
    ["expected_token_unterminated_html_comment", "<!--"],
    ["expected_token_unterminated_rule", ".b {"],
  ];
  for (const [slug, body] of bodies) {
    // single occurrence
    add("read_style", `single_${slug}`, scaffoldTemplate(`<style>${body}</style>`), false);
    // second-style duplicate competition: a clean first body + this body second.
    add(
      "read_style",
      `second_${slug}_vs_duplicate`,
      scaffoldTemplate(`<style>p{}</style>\n<style>${body}</style>`),
      false,
    );
  }
  // A clean second body IS the duplicate (the parse-entry race is won by `style_duplicate`).
  add(
    "read_style",
    "second_clean_is_duplicate",
    scaffoldTemplate("<style>.a {}</style>\n<style>.b { color: red; }</style>"),
    false,
  );
  // An unterminated FIRST body beats a clean second's duplicate (first-error wins).
  add(
    "read_style",
    "first_unterminated_beats_second_duplicate",
    scaffoldTemplate("<style>.a {</style>\n<style>.b {}</style>"),
    false,
  );

  // The `unexpected_eof` parse-entry code: upstream's nested CSS readers (`read_value`,
  // `read_attribute_value`) loop on `parser.index < parser.template.length` and raise
  // `e.unexpected_eof(template.length)` when they run off the END of the source MID-construct. For
  // a TOP-LEVEL `<style>` this is the WINNING reject code ONLY when the `<style>` is properly
  // CLOSED (a BARE unterminated `<style>` is an EARLIER unterminated-raw-block strict error that
  // pre-empts it) yet its CSS body opens an UNTERMINATED QUOTE that SWALLOWS the literal `</style>`
  // text — so the reader runs PAST the close to true EOF (a quote closes only on a matching quote,
  // never on markup). So each row CLOSES the `<style>` and opens a quote/value swallowing the close.
  // Grounded against pinned svelte@5.56.3 (each ⇒ `unexpected_eof`), one per distinct raise-site.
  const eofBodies = [
    // `read_value` opens a `"` that swallows `</style>`; the value reader runs to EOF.
    ["value_open_quote_swallows_close", '<style>.a { content: "x</style>'],
    // `read_attribute_value` opens a `"` that swallows `</style>`; the attribute value runs to EOF.
    ["attribute_value_open_quote_swallows_close", '<style>a[x="y</style>'],
  ];
  for (const [slug, frag] of eofBodies) {
    add("read_style", `single_unexpected_eof_${slug}`, scaffoldTemplate(frag), false);
  }
  // Duplicate competition: a CLEAN first `<style>…</style>` then a second CLOSED `<style>` whose
  // open quote swallows its `</style>` and runs to EOF — the second body's `unexpected_eof` is the
  // only defect (the never-reached `style_duplicate` loses the first-error race), proving the EOF
  // parse-entry code arbitrates against the duplicate timing exactly like the other parse-entry
  // codes above. Grounded: pinned svelte@5.56.3 ⇒ `unexpected_eof`.
  add(
    "read_style",
    "second_unexpected_eof_open_quote_beats_duplicate",
    scaffoldTemplate('<style>p {}</style>\n<style>.a { content: "x</style>'),
    false,
  );
}

// (C) The `<svelte:options customElement={EXPR}>` value space + source-order arbitration. Every
// customElement VALUE branch (string / object / null / number / identifier / expression / mixed /
// shadow / props / tag), the SYNTACTIC parse faults (malformed prefix → js_parse_error; trailing
// junk → expected_token; empty), and the encounter-order arbitration of a parse fault vs a later
// duplicate attribute / a later/earlier template defect / a finalization validation fault.
function enumerateOptionsCustomElement(add) {
  const opt = (frag) => `${frag}<script>let c = $state(0);</script>\n${BUTTON}\n`;
  // Value branches (string Text / object / null / number / identifier / valid object / shadow /
  // props / tag forms). The oracle decides accept (Verter refuses as the feature) vs the exact
  // svelte_options_* / js_parse_error / expected_token code.
  const valueBranches = [
    ["string_text", 'customElement="my-el"'],
    ["string_text_bad_tag", 'customElement="nodash"'],
    ["object_valid", 'customElement={{ tag: "my-el" }}'],
    ["object_empty", "customElement={{}}"],
    ["object_bad_tag", 'customElement={{ tag: "nodash" }}'],
    ["object_reserved_tag", 'customElement={{ tag: "annotation-xml" }}'],
    ["object_non_string_tag", "customElement={{ tag: 1 }}"],
    ["object_bad_props", "customElement={{ props: 1 }}"],
    ["object_bad_shadow", "customElement={{ shadow: 1 }}"],
    ["object_string_shadow", 'customElement={{ shadow: "open" }}'],
    ["object_spread", "customElement={{ ...x }}"],
    ["object_computed_key", "customElement={{ [k]: 1 }}"],
    ["null_expr", "customElement={null}"],
    ["number_expr", "customElement={42}"],
    ["identifier_expr", "customElement={foo}"],
    ["string_expr", 'customElement={"my-el"}'],
    ["boolean_shorthand", "customElement"],
    ["mixed_text_first", 'customElement="a{x}b"'],
    ["mixed_expr_first", 'customElement="{x}b"'],
    // syntactic parse faults (the C cursor-parse axis)
    ["malformed_empty", "customElement={}"],
    ["malformed_bare_lt", "customElement={<}"],
    ["malformed_object_colon", "customElement={{ tag: }}"],
    ["trailing_junk_two_ints", "customElement={1 2}"],
    ["trailing_junk_idents", "customElement={foo bar}"],
    ["trailing_junk_semicolon", "customElement={1;2}"],
    ["sequence_expr", "customElement={1,2}"],
    ["incomplete_binary", "customElement={1 + }"],
  ];
  for (const [slug, attr] of valueBranches) {
    add("options_ce", `value_${slug}`, opt(`<svelte:options ${attr} />\n`), false);
  }
  // Source-order arbitration: a syntactic customElement parse fault vs a later duplicate attribute,
  // an earlier duplicate, a later/earlier template defect, and a finalization validation fault.
  const arb = [
    ["parse_empty_then_dup", "<svelte:options customElement={} foo foo />\n"],
    ["parse_one_two_then_dup", "<svelte:options customElement={1 2} foo foo />\n"],
    ["dup_then_parse_empty", "<svelte:options foo foo customElement={} />\n"],
    ["clean_value_then_dup", "<svelte:options customElement={42} foo foo />\n"],
    ["parse_then_later_stray", `<svelte:options customElement={<} />\n</div>\n`],
    ["stray_then_parse", `</div>\n<svelte:options customElement={<} />\n`],
    ["validation_then_later_stray", `<svelte:options customElement={42} />\n</div>\n`],
    [
      "clean_value_then_later_name",
      '<svelte:options customElement={{ tag: "my-el" }} name="x" />\n',
    ],
    ["value_fault_then_later_name", '<svelte:options customElement={42} name="x" />\n'],
  ];
  for (const [slug, frag] of arb) {
    add("options_ce", `arb_${slug}`, opt(frag), false);
  }
}

// (D) Open-tag attribute / duplicate timing across the attribute FORMS (normal, expression-valued,
// shorthand, spread, directive) and same-tag duplicate competitions — so the duplicate-mint
// encounter point is exercised against each attribute kind.
function enumerateOpenTagDuplicateTiming(add) {
  // Every expression / binding references the scaffold's declared `c` so the ONLY defect a
  // duplicate fixture surfaces is `attribute_duplicate` (NOT an undeclared-binding semantic reject,
  // which would not exercise the open-tag duplicate-timing axis). The clean singles reference `c`
  // too (or a harmless spread), so they are genuine ACCEPT controls.
  const forms = [
    // normal-attr duplicate
    ["normal_dup", "<div id=a id=b>x</div>"],
    // expression-valued attribute duplicate
    ["expr_dup", "<div title={c} title={c}>x</div>"],
    // class/style namespace duplicate handling
    ["class_dup", '<div class="a" class="b">x</div>'],
    // a clean single attribute of each form (ACCEPT controls)
    ["normal_single_clean", "<div id=a>x</div>"],
    ["expr_single_clean", "<div title={c}>x</div>"],
    ["shorthand_single_clean", "<div {c}>x</div>"],
    ["spread_single_clean", "<div {...c}>x</div>"],
    ["directive_single_clean", "<input bind:value={c} />"],
    // duplicate where a directive + a plain attr collide on the same resolved name
    ["bind_then_plain_dup", "<input bind:value={c} value={c} />"],
  ];
  for (const [slug, frag] of forms) {
    // These use real intrinsic elements / attributes; an ACCEPTED row only asserts "not
    // over-rejected" (NON-core), while a duplicate row asserts the exact `attribute_duplicate` code.
    add("dup_timing", slug, scaffoldTemplate(frag), false);
  }
}

// (E) Script-`lang` finite forms (the deterministic in-contract lang selection space). A TS-only
// body (`let a: number = 1;`) makes the disposition pivot on the lang scan: TS-selected ⇒ the body
// parses (ACCEPT, refused later as the feature); JS-selected ⇒ `js_parse_error`. Quoted / unquoted
// / empty / `ts` / `tsx` / `typescript` / `TS` / no-lang / unrelated-quoted-substring /
// rightmost-overriding forms. (The exotic quoted-`>` corner is LEDGERED, not generated.)
function enumerateScriptLangForms(add) {
  const tsBody = "let a: number = 1;";
  const opens = [
    ["plain_lang_ts", '<script lang="ts">'],
    ["plain_lang_js", '<script lang="js">'],
    ["unquoted_lang_ts", "<script lang=ts>"],
    ["unquoted_lang_js", "<script lang=js>"],
    ["empty_quoted_lang", '<script lang="">'],
    ["lang_tsx", '<script lang="tsx">'],
    ["lang_typescript", '<script lang="typescript">'],
    ["lang_uppercase_ts_value", '<script lang="TS">'],
    ["uppercase_lang_name", '<script LANG="ts">'],
    ["no_lang", "<script>"],
    ["lang_ts_trailing_space", '<script lang="ts ">'],
    ["data_lang_quoted_ts", '<script data-lang="ts">'],
    ["quoted_value_lang_ts", '<script foo="lang=ts">'],
    ["rightmost_lang_js_then_data_ts", '<script lang="js" data-lang="ts">'],
    ["rightmost_lang_ts_then_data_js", '<script lang="ts" data-lang="js">'],
    ["gt_in_earlier_quoted_then_lang_ts", '<script data-x="1>2" lang="ts">'],
  ];
  for (const [slug, open] of opens) {
    add("script_lang_enum", slug, `${open}${tsBody}</script>\n${BUTTON}\n`, false);
  }
}

// ---------------------------------------------------------------------------
// Disposition via the pinned compiler
// ---------------------------------------------------------------------------

// Compile one source through the pinned CLIENT compiler, returning the official
// disposition `{ disposition, official_code }`:
//   - accepted  → { disposition: "accepted", official_code: "" }
//   - rejected  → { disposition: "rejected", official_code: "<svelte-error-code>" }
function officialDisposition(compiler, source, filename) {
  try {
    compiler.compile(source, { generate: "client", dev: false, filename });
    return { disposition: "accepted", official_code: "" };
  } catch (err) {
    const code = err && err.code ? err.code : `error:${err && err.message}`;
    return { disposition: "rejected", official_code: code };
  }
}

// ---------------------------------------------------------------------------
// Corpus build (path -> content map) + manifest
// ---------------------------------------------------------------------------

function metadataJson(meta) {
  // A flat object the Rust gate reads without a JSON crate, 2-space indented + trailing
  // newline (matching the reject-corpus + generated-golden conventions). `core` (the
  // §1.2-core surface classification) is a boolean the gate uses to assert an
  // ACCEPTED-core fixture emits a `Main` while an ACCEPTED non-core one fails closed as
  // an unsupported feature. `axis` is the finite-axis tag the coverage gate keys on.
  return `${JSON.stringify(
    {
      disposition: meta.disposition,
      official_code: meta.official_code,
      core: meta.core,
      axis: meta.axis,
    },
    null,
    2,
  )}\n`;
}

function buildCorpus(compiler) {
  const cases = fixtureCases();
  // Stable basenames: `<NNN>_<axis>_<slug>` (zero-padded ordinal keeps file order
  // deterministic regardless of the per-axis grouping).
  const files = new Map();
  const manifest = [];
  let ordinal = 0;
  // Guard against a duplicate (axis, slug) collision producing a non-unique basename.
  const seen = new Set();
  // Track which REQUIRED finite axes contributed at least one row.
  const axisRowCounts = new Map();
  for (const c of cases) {
    const key = `${c.axis}/${c.slug}`;
    if (seen.has(key)) {
      throw new Error(`duplicate parse-parity fixture (axis, slug): ${key}`);
    }
    seen.add(key);
    const name = `${String(ordinal).padStart(3, "0")}_${c.axis}_${c.slug}`;
    ordinal += 1;
    const disposition = officialDisposition(compiler, c.source, `${name}.svelte`);
    // `core` is a fixture-author classification (Verter's §1.2-core surface), NOT an
    // oracle-derived field — it rides alongside the pinned-compiler disposition. `axis` is
    // carried into the metadata so the Rust coverage gate can assert per-axis presence.
    const meta = { ...disposition, core: c.core, axis: c.axis };
    files.set(join(PARSE_PARITY_DIR, `${name}.svelte`), c.source);
    files.set(join(PARSE_PARITY_DIR, `${name}.json`), metadataJson(meta));
    manifest.push({ name, ...meta });
    axisRowCounts.set(c.axis, (axisRowCounts.get(c.axis) ?? 0) + 1);
  }
  // COVERAGE GATE: every REQUIRED finite axis must contribute ≥1 row, so a dropped enumerator
  // (the corpus silently losing a finite axis) fails generation HARD — the corpus is only a real
  // gate if it cannot lose an axis to a hand-audit gap.
  const missingAxes = REQUIRED_AXES.filter((axis) => (axisRowCounts.get(axis) ?? 0) === 0);
  if (missingAxes.length > 0) {
    throw new Error(
      `parse-parity corpus is missing required finite axes (no rows generated): ${missingAxes.join(", ")}`,
    );
  }
  // A top-level manifest summarizing the corpus (counts + per-axis row counts) — useful for a
  // quick human read and the coverage contract; the per-fixture `.json` files are the authority
  // the Rust gate consumes, and the `required_axes` list is what the Rust coverage gate enforces.
  const accepted = manifest.filter((m) => m.disposition === "accepted").length;
  const rejected = manifest.length - accepted;
  const axisCounts = Object.fromEntries(
    [...axisRowCounts.entries()].sort((a, b) => (a[0] < b[0] ? -1 : 1)),
  );
  const manifestJson = `${JSON.stringify(
    {
      svelte_oracle_version: SVELTE_ORACLE_VERSION,
      total: manifest.length,
      accepted,
      rejected,
      required_axes: REQUIRED_AXES,
      axis_counts: axisCounts,
    },
    null,
    2,
  )}\n`;
  files.set(join(PARSE_PARITY_DIR, "manifest.json"), manifestJson);
  return { files, total: manifest.length, accepted, rejected };
}

function walkFiles(dir) {
  const out = [];
  if (!existsSync(dir)) return out;
  for (const e of readdirSync(dir, { withFileTypes: true }).sort((a, b) =>
    a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
  )) {
    const p = join(dir, e.name);
    if (e.isDirectory()) out.push(...walkFiles(p));
    else out.push(p);
  }
  out.sort();
  return out;
}

function writeMode(compiler) {
  const { files, total, accepted, rejected } = buildCorpus(compiler);
  rmSync(PARSE_PARITY_DIR, { recursive: true, force: true });
  const all = [...files].sort((a, b) => (a[0] < b[0] ? -1 : 1));
  for (const [path, content] of all) {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, content);
  }
  console.log(
    `gen-svelte-parse-parity-corpus: wrote ${total} fixture(s) (${accepted} accepted, ` +
      `${rejected} rejected) from svelte@${SVELTE_ORACLE_VERSION} into ` +
      `${relative(REPO_ROOT, PARSE_PARITY_DIR)}`,
  );
}

function checkMode(compiler) {
  const { files, total } = buildCorpus(compiler);
  const drift = [];
  // 1. Every fresh artifact exists on-disk and is byte-equal.
  for (const [path, content] of files) {
    const rel = relative(REPO_ROOT, path);
    if (!existsSync(path)) {
      drift.push(`MISSING parse-parity artifact: ${rel}`);
      continue;
    }
    if (readFileSync(path, "utf8") !== content) {
      drift.push(`DRIFTED parse-parity artifact (on-disk != regenerated): ${rel}`);
    }
  }
  // 2. No stale orphan files under the subtree.
  for (const path of walkFiles(PARSE_PARITY_DIR)) {
    if (!files.has(path)) {
      drift.push(`STALE parse-parity artifact (no fresh source): ${relative(REPO_ROOT, path)}`);
    }
  }
  if (drift.length > 0) {
    console.error(
      `gen-svelte-parse-parity-corpus --check: ${drift.length} drift(s) detected:\n` +
        drift.map((d) => `  - ${d}`).join("\n") +
        `\n\nRun \`node scripts/gen-svelte-parse-parity-corpus.mjs\` to regenerate.`,
    );
    process.exit(1);
  }
  console.log(
    `gen-svelte-parse-parity-corpus --check: ${total} fixture(s) in sync with ` +
      `svelte@${SVELTE_ORACLE_VERSION}.`,
  );
}

function main() {
  const check = process.argv.includes("--check");
  const compiler = loadPinnedCompiler(REPO_ROOT);
  if (check) checkMode(compiler);
  else writeMode(compiler);
}

main();
