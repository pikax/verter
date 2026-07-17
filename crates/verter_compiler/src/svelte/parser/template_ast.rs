//! The Svelte template AST.
//!
//! The parser produces a [`ParsedSvelte`] carrying the instance and module
//! `<script>` spans, the component `<style>` span, and a tree of
//! [`SvelteNode`]s covering the FULL current Svelte syntax (Svelte 5.56.x):
//! elements, components, attributes (including Svelte-5 lowercase event
//! attributes and spreads), interpolation, the block constructs
//! (`{#if}`/`{#each}`/`{#await}`/`{#key}`/`{#snippet}`), the rendered-content
//! tags (`{@render}`/`{@html}`/`{@attach}`/`{@const}`/declaration tags
//! `{const}`/`{let}`/`{@debug}`), directive attributes (`bind:`/`class:`/
//! `style:`/`use:`/`transition:`/`in:`/`out:`/`animate:`/legacy `on:`), and
//! the special elements (`<svelte:head>` / `<svelte:element>` / `<svelte:window>`
//! / `<svelte:boundary>` / `<svelte:options>` / `<svelte:component>` /
//! `<svelte:self>` / `<svelte:fragment>`).
//!
//! Every node records spans into the ORIGINAL source so a later projector maps
//! positions precisely. The AST is intentionally LOSSLESS over the matrix: a
//! row's SUPPORTED/OUT-OF-SCOPE disposition is a projector concern, never a
//! parser one — the parser accepts every current-docs construct without crash.

use verter_span::Span;

use super::options_custom_element::{AcceptedCustomElementValue, CustomElementDescriptor};

/// A parsed Svelte component.
///
/// Carries the script region spans (instance + module), the component-level
/// `<style>` span, the template node tree, and any parse diagnostics collected
/// inline. Spans index into the original source.
#[derive(Debug, Clone, Default)]
pub struct ParsedSvelte {
    /// The instance `<script>` block (the default `<script>`), if present.
    pub instance_script: Option<SvelteScript>,
    /// The module `<script module>` (or legacy `<script context="module">`)
    /// block, if present.
    pub module_script: Option<SvelteScript>,
    /// The component-level `<style>` blocks (opaque content; CSS domain).
    pub styles: Vec<SvelteStyle>,
    /// The template node tree (everything outside the script/style blocks).
    pub template: Vec<SvelteNode>,
    /// Parse diagnostics collected inline (never a hard failure).
    pub diagnostics: Vec<SvelteParseDiagnostic>,
    /// The CLOSE-TAG well-formedness violations the parser observed and silently
    /// recovered from — an element open at EOF, a stray / mismatched close tag, or a
    /// void element carrying content / an explicit close. The official compiler
    /// COMPILE-ERRORS each of these in its parse phase (`element_unclosed` /
    /// `element_invalid_closing_tag` / `void_element_invalid_content`,
    /// `phases/1-parse/state/element.js` + `index.js`); the parser is the close-tag
    /// authority, so it records the faithful violation here for the official-reject
    /// gate to fail closed on (instead of tolerating a malformed tree). In SOURCE
    /// ORDER of the offending tag; empty for a well-formed component.
    pub close_tag_violations: Vec<CloseTagViolation>,
    /// The STRICT-PARSE errors observed and silently recovered from — markup Verter's
    /// forgiving parser accepts but the official `svelte@5.56.3` STRICT parser rejects
    /// (a raw `<` in text, a close tag with a trailing token, an empty attribute value,
    /// a nameless close, an unterminated tag / raw block / quoted value). The
    /// official-reject gate fails closed on a non-empty list (the single
    /// `ParserStrictness` rule), so a malformed input never becomes a divergent `Main`.
    /// In SOURCE ORDER; empty for a well-formed component.
    pub strict_parse_errors: Vec<SvelteStrictParseError>,
    /// The PARSE-DOMAIN official-reject facts the parser recorded during the forward pass
    /// — the `<script>` attribute / duplicate-script rejects, the template duplicate
    /// attribute / duplicate-`<svelte:options>`, and the explicit-`</p>` autoclose. These
    /// are parse-phase compile errors official throws WHILE parsing, so the official-reject
    /// gate arbitrates them by `encounter_order` against the close-tag and strict-parse
    /// rails (the FIRST-discovered parse defect wins). In discovery order; empty for a
    /// well-formed component.
    pub parse_reject_facts: Vec<SvelteParseRejectFact>,
    /// The RESERVED script-body-parse slots the parser allocated, one per `<script>` block
    /// with a body — each carrying the `encounter_order` minted at the upstream-faithful
    /// body-parse position (AFTER the open-tag attribute-duplicate, BEFORE the source-order
    /// reserved/context/module attribute validation, matching official `read_script`, which
    /// runs Acorn on the body before validating the attributes), the body `Span`, and the
    /// body grammar (plain `<script>` = JS, `lang="ts"` = TS). The parser does NOT run the
    /// body parse — it reserves the slot; the official-reject gate (which holds OXC) fills
    /// it by parsing the body once at the reserved order and minting `js_parse_error` on a
    /// parse FAILURE (a body that parses clean contributes NO defect). In discovery order;
    /// empty for a component with no script body.
    pub script_body_probes: Vec<ScriptBodyProbe>,
    /// The RESERVED style-body-parse slots the parser allocated, one per top-level `<style>`
    /// block — each carrying the `encounter_order` minted at the upstream-faithful `read_style`
    /// body-parse position (BEFORE the `style_duplicate` check; upstream's `element.js` runs
    /// `read_style`, which PARSES the CSS body via a full CSS reader and can throw, BEFORE
    /// `if (current.css) e.style_duplicate(start)`), plus the CSS body's content-start offset.
    /// The parser does NOT run the CSS parse — it reserves the slot; the official-reject gate
    /// fills it with a faithful port of upstream's `read/style.js` parse control flow, minting
    /// the first exact CSS parse code (`css_expected_identifier` / `css_empty_declaration` /
    /// `css_selector_invalid` / `expected_token` / `unexpected_eof`) on a body-parse FAILURE at
    /// the reserved order — so a MALFORMED 2nd-style body wins the first-error race over the
    /// later `style_duplicate`. A body that parses CLEAN contributes NO defect (the duplicate /
    /// unsupported-style rail then wins). In discovery order; empty for a component with no
    /// top-level `<style>`.
    pub style_body_probes: Vec<StyleBodyProbe>,
    /// The RESOLVED `<svelte:options customElement={EXPR}>` validation slots — one per
    /// expression-valued `customElement` attribute on the FIRST root `<svelte:options>` element,
    /// each carrying its source-order `encounter_order` (among the options attributes), the
    /// expression span, and the RETAINED typed resolution (the parser runs the one
    /// validate+extract engine at the options-finalization position — where upstream's
    /// `read_options` runs — and retains the exact official reject code OR the accepted typed
    /// value). The official-reject gate consumes the reject side at the reserved orders; the
    /// runtime lowering consumes the accepted side. In discovery order; empty for a component
    /// with no expression-valued `customElement` axis.
    pub options_custom_element_probes: Vec<OptionsCustomElementProbe>,
    /// The RETAINED string-tag `<svelte:options customElement="my-el">` descriptors — the
    /// Text-value counterpart of [`options_custom_element_probes`]: the parser VALIDATES the
    /// text tag at the options-finalization position (`validate_custom_element_tag`) and, on
    /// accept, retains the RESOLVED descriptor keyed by the attribute's text-value span. The
    /// runtime lowering consumes ONLY this retained descriptor (the raw source is never
    /// re-sliced at lowering); a rejected text tag mints its `OptionsInvalid` fact directly at
    /// finalization and retains nothing. In attribute source order; empty for a component with
    /// no accepted Text-valued `customElement` axis.
    ///
    /// [`options_custom_element_probes`]: Self::options_custom_element_probes
    pub options_custom_element_text_tags: Vec<OptionsCustomElementTextTag>,
}

/// One RESERVED script-body-parse slot — the "reserved hole in the parse-reject stream" the
/// parser allocates at the upstream-faithful body-parse position so a body `js_parse_error`
/// arbitrates by the order official discovers it (BEFORE the reserved/context/module
/// attribute validation), not by the body span (which starts after the attributes) or gate
/// execution time. The parser mints only the slot; the official-reject gate fills it by
/// parsing the body once at [`encounter_order`](Self::encounter_order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptBodyProbe {
    /// The parser's monotonic discovery sequence reserved for this script's body parse —
    /// drawn from the shared defect counter at the body-parse position so a body parse
    /// failure arbitrates against the other parse-defect rails by minimum `encounter_order`.
    pub encounter_order: u32,
    /// The script body content `Span` the gate parses (the inner text between the open and
    /// close tags). The REPORT anchor for a body parse failure; never the arbitration key.
    pub body_span: Span,
    /// The grammar the body is parsed under — plain `<script>` is JS (Acorn-equivalent, so
    /// TS-only syntax in a plain script is `js_parse_error`); `lang="ts"` is TS.
    pub grammar: ScriptBodyGrammar,
}

/// One RESERVED style-body-parse slot — the CSS analogue of [`ScriptBodyProbe`]. The parser
/// allocates it at the upstream `read_style` body-parse position (BEFORE the `style_duplicate`
/// check) so a malformed CSS body's exact parse code arbitrates by the order official discovers
/// it (which, for the 2nd `<style>`, is BEFORE `style_duplicate`). The parser mints only the
/// slot; the official-reject gate fills it by running a faithful port of upstream's
/// `read/style.js` parse control flow from [`content_start`](Self::content_start).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleBodyProbe {
    /// The parser's monotonic discovery sequence reserved for this style's CSS body parse —
    /// drawn from the shared defect counter at the `read_style` position (before the duplicate
    /// check) so a body parse failure arbitrates against the other parse-defect rails by minimum
    /// `encounter_order`.
    pub encounter_order: u32,
    /// The byte offset where the CSS body BEGINS (just past the `<style …>` open tag's `>`). The
    /// gate's CSS reader parses from HERE into the rest of the source (honouring upstream's
    /// `</style`-or-EOF body-loop finish predicate), so a nested CSS reader that runs into the
    /// literal `</style>` reproduces the exact upstream code. The REPORT anchor for a CSS body
    /// failure; never the arbitration key.
    pub content_start: u32,
}

/// One RESOLVED `<svelte:options customElement={EXPR}>` validation slot. The
/// EXPRESSION-VALUED `customElement` axis is the ONE options attribute whose disposition depends
/// on the JS expression. The parser RESOLVES it at options finalization — the position upstream's
/// `read_options` runs — through the one validate+extract engine
/// ([`resolve_custom_element_expr`]) and RETAINS the typed outcome on [`resolution`], exactly as
/// upstream retains `AST.SvelteOptions['customElement']`. The official-reject gate consumes the
/// `Err` side; the runtime lowering consumes the `Ok` side — the expression is never re-parsed
/// from raw source. A reject falls into TWO upstream-distinct positions on the encounter
/// timeline:
///
/// - the expression FAILS as a parse-position fault (`js_parse_error` when the prefix expression
///   does not parse; `expected_token` when it parses but trailing non-trivia content leaves the
///   required `}` missing): upstream's acorn attribute-expression read runs WHILE parsing the
///   `customElement` attribute in the open-tag attribute loop, so this rides
///   [`parse_encounter_order`] — the ATTRIBUTE's source position in that loop (the point upstream's
///   `read_expression` reaches the value). It beats a LATER template defect, loses to an EARLIER one.
/// - the expression parses but is INVALID (`svelte_options_invalid_customelement` /
///   `svelte_options_invalid_tagname` / `svelte_options_invalid_customelement_props` /
///   `svelte_options_invalid_customelement_shadow` / `svelte_options_reserved_tagname`): upstream's
///   `read_options` runs at FINALIZATION (after the whole template parse), so this rides
///   [`encounter_order`] — the options-finalization position, after every walk fact, losing to ANY
///   template parse defect (matching upstream). A `null` / valid object is ACCEPT (no defect).
///
/// The two reserved orders are the ARBITRATION positions the gate mints the retained code at —
/// the parser resolves the value once; the gate only routes the retained code to the correct
/// position.
///
/// [`parse_encounter_order`]: Self::parse_encounter_order
/// [`encounter_order`]: Self::encounter_order
/// [`resolution`]: Self::resolution
/// [`resolve_custom_element_expr`]: super::options_custom_element::resolve_custom_element_expr
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionsCustomElementProbe {
    /// The monotonic discovery sequence for the `read_options` VALIDATION fault, drawn at the
    /// options-finalization position in attribute SOURCE ORDER (after every walk fact), so a
    /// VALIDATION fault arbitrates against the other options-attribute faults by minimum
    /// `encounter_order` and loses to any template parse defect — the upstream
    /// first-faulting-attribute-in-source-order rule at finalization.
    pub encounter_order: u32,
    /// The monotonic discovery sequence for the attribute-expression PARSE-POSITION fault — drawn
    /// while parsing the `customElement` attribute IN THE OPEN-TAG ATTRIBUTE LOOP (the point
    /// upstream's `read_expression` reaches the value), at that attribute's source position. A
    /// retained `js_parse_error` (the prefix expression does not parse) or `expected_token` (it
    /// parses but trailing non-trivia content leaves the `}` missing) mints HERE, so a
    /// malformed-expression parse fault beats a LATER template defect and loses to an EARLIER one —
    /// exactly as upstream's during-parse acorn error does.
    pub parse_encounter_order: u32,
    /// The `{EXPR}` inner-expression `Span` (the bytes between the `customElement=` braces) the
    /// parser resolved. The REPORT anchor, and the lowering's key back to this probe (the
    /// attribute value carries the same span).
    pub expr_span: Span,
    /// The RETAINED typed resolution — the one validate+extract walk's outcome, resolved ONCE at
    /// options finalization: `Err` carries the EXACT official reject code (the official-reject
    /// gate's side); `Ok` carries the ACCEPTED value (the runtime lowering's side — the `null`
    /// backwards-compat skip or the typed descriptor).
    pub resolution: Result<AcceptedCustomElementValue, &'static str>,
}

/// One RETAINED string-tag `<svelte:options customElement="my-el">` descriptor — the Text-value
/// counterpart of [`OptionsCustomElementProbe`]. The parser validates the text tag at the
/// options-finalization position (`validate_custom_element_tag`, where upstream's `read_options`
/// runs) and, on ACCEPT, resolves the descriptor ONCE and retains it here (exactly as upstream
/// retains `AST.SvelteOptions['customElement']`); the runtime lowering consumes ONLY the retained
/// descriptor — the tag text is never re-sliced from raw source at lowering. A REJECTED text tag
/// (`svelte_options_invalid_tagname` / `svelte_options_reserved_tagname`) mints its fact directly
/// at finalization and retains nothing — no reserved arbitration orders exist here because a Text
/// value never takes the attribute-expression parse-fault rail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionsCustomElementTextTag {
    /// The attribute's Text value `Span` (the bytes between the quotes) — the lowering's key
    /// back to this retained descriptor (the attribute value carries the same span).
    pub text_span: Span,
    /// The RESOLVED descriptor: the validated tag plus the string-tag form's fixed axes (open
    /// shadow, no explicit props, no extend, injected styles).
    pub descriptor: CustomElementDescriptor,
}

/// The grammar a [`ScriptBodyProbe`] body is parsed under, mirroring upstream's
/// `acorn.parse(source, comments, parser.ts, …)`: when the parser-wide TS flag is OFF every
/// body parses as JS (no TS), when it is ON every body parses as TS.
///
/// The flag is a SINGLE parser-wide value (not a per-script choice) — see
/// [`script_body_grammar_for_source`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptBodyGrammar {
    /// JS grammar (no TS annotations; TS-only syntax is `js_parse_error`). The parser-wide TS
    /// flag is OFF (no first lowercase `<script lang="ts">`).
    Js,
    /// TS grammar (type annotations parse cleanly). The parser-wide TS flag is ON (the first
    /// lowercase `<script ... lang="ts">` carried an exact `ts` value).
    Ts,
}

/// Compute the SINGLE parser-wide script-body grammar for a whole component `source`,
/// mirroring upstream `svelte@5.56.3`'s `Parser` constructor EXACTLY
/// (`phases/1-parse/index.js`):
///
/// ```js
/// const regex_lang_attribute =
///   /<!--[^]*?-->|<script\s+(?:[^>]*|(?:[^=>'"/]+=(?:"[^"]*"|'[^']*'|[^>\s]+)\s+)*)lang=(["'])?([^"' >]+)\1[^>]*>/g;
/// let match_lang;
/// do match_lang = regex_lang_attribute.exec(template);
/// while (match_lang && match_lang[0][1] !== 's');   // ensure it starts with '<s'
/// this.ts = match_lang?.[2] === 'ts';
/// ```
///
/// i.e. the FIRST `<script …>` open tag (case-SENSITIVE `<script`, lowercased; a TERMINATED
/// `<!-- … -->` comment is consumed so a `lang=` inside it never counts) that carries a `lang=`
/// substring decides the WHOLE parse: the body grammar is [`ScriptBodyGrammar::Ts`] iff that
/// script's effective lang value is EXACTLY `ts`. A `<script …>` whose open tag contains NO
/// matchable `lang=` is skipped (the regex's script-tag alternative fails there), so the scan
/// continues to the next `<script … lang=…>`.
///
/// `lang=` matches as a RAW SUBSTRING anywhere inside the open tag — INCLUDING inside another
/// attribute's quoted value (`data-lang="ts"` / `foo="lang=ts"` both select TS) — and, because the
/// regex prefix `(?:[^>]*|…)` is GREEDY, the RIGHTMOST viable `lang=` within a single open tag
/// wins (`lang="js" data-lang="ts"` → TS; `lang="ts" data-lang="js"` → JS). The match is NOT an
/// attribute-name-boundary / first-occurrence scan.
///
/// The lang VALUE is read faithfully to the regex's `(["'])?([^"' >]+)\1`: an optional opening
/// quote (`"` or `'`), then a NON-EMPTY run of bytes excluding quote / space / `>`, then — when an
/// opening quote was present — the SAME closing quote IMMEDIATELY after the value (so `lang="ts "`
/// does NOT match `ts`, and an empty `lang=""` does not match either). The whole tag must also
/// reach a closing `>` (the regex's trailing `[^>]*>`). A byte scan, not a regex dependency — the
/// parser is a hand-written byte tokenizer and must not pull in a regex engine.
///
/// Coverage of this byte realization is the DETERMINISTIC finite set the parse-parity corpus
/// generates (the `script_lang` axis: quoted / unquoted / empty / `ts` / `tsx` / `typescript` /
/// `TS` / no-lang / unrelated-quoted-substring / rightmost-overriding forms) plus the
/// `lang_scan_*` unit tests — NOT a randomized / proptest fuzz harness, and the corpus does NOT
/// claim grammar-scan equivalence to the regex on inputs outside that finite set.
///
/// The "rightmost wins" statement holds for those finite forms, where no `>` byte sits inside a
/// quoted value before a later `lang=`. It does NOT hold byte-for-byte against the regex on the
/// EXOTIC case of a quoted `>` BETWEEN two `lang=` attributes
/// (`<script lang=js data-x=">" lang="ts">`): the regex's `[^>]*` prefix cannot cross the quoted
/// `>` (so the regex selects the EARLIER `lang=js`), whereas this attribute-aware byte scan skips
/// the quoted value and reaches the later `lang="ts"`. That internal grammar-scan divergence is
/// UNOBSERVABLE end-to-end — the source carries TWO `lang=` attributes, so BOTH the official
/// compiler and Verter reject it with `attribute_duplicate` regardless of which grammar the scan
/// would pick (behavioral parity is met; the divergent grammar choice never reaches a body parse).
/// The quoted-`>` lang corner is OUTSIDE the finite lower-case raw-block / lang contract and is
/// ledgered (owner `svelte-native-parser-parity`, `docs/arch/svelte-native-compiler-plan.md`).
#[must_use]
pub fn script_body_grammar_for_source(source: &str) -> ScriptBodyGrammar {
    if first_script_lang_is_ts(source.as_bytes()) {
        ScriptBodyGrammar::Ts
    } else {
        ScriptBodyGrammar::Js
    }
}

/// Whether the FIRST lowercase `<script …>` open tag in `src` that carries a matchable `lang=`
/// substring resolves to an EXACT `ts` value (skipping TERMINATED `<!-- … -->` comments). The
/// upstream-faithful byte realization of [`script_body_grammar_for_source`]'s `regex_lang_attribute`.
fn first_script_lang_is_ts(src: &[u8]) -> bool {
    let mut i = 0usize;
    let n = src.len();
    while i < n {
        // The regex's comment alternative `<!--[^]*?-->` matches ONLY a TERMINATED comment. A
        // `<!--` with a later `-->` is consumed whole (so a `lang=` / `<script` inside it is
        // invisible); a `<!--` with NO `-->` is NOT a comment match — the engine advances ONE
        // byte and retries, so a later `<script lang=…>` is still reachable.
        if src[i..].starts_with(b"<!--") {
            match find_subslice(&src[i + 4..], b"-->") {
                Some(off) => {
                    i = i + 4 + off + 3;
                    continue;
                }
                None => {
                    i += 1;
                    continue;
                }
            }
        }
        // A lowercase `<script` (case-SENSITIVE, matching upstream's flag-less regex) FOLLOWED
        // by ASCII whitespace (the regex's `<script\s+`). `<scriptfoo`/`<script>`/`<script/>`
        // never carry a matchable `lang=` here.
        if src[i..].starts_with(b"<script")
            && matches!(src.get(i + 7), Some(b) if b.is_ascii_whitespace())
        {
            if let Some(is_ts) = match_script_tag_lang_ts(src, i) {
                return is_ts;
            }
            // No matchable `lang=` formed a complete match for this `<script>` — the regex's
            // script-tag alternative failed here, so the engine advances past the `<` and retries
            // (a later `<script … lang=…>` may still match).
            i += 7;
            continue;
        }
        i += 1;
    }
    false
}

/// Match the regex's `<script\s+ … lang=… >` alternative starting at the `<` byte `lt`
/// (`src[lt..]` begins with `<script` + whitespace). Returns `Some(is_ts)` for the RIGHTMOST
/// viable `lang=` whose value reads complete and whose tag reaches a closing `>`, or `None` when
/// no `lang=` forms a complete match for this open tag.
fn match_script_tag_lang_ts(src: &[u8], lt: usize) -> Option<bool> {
    // The regex's `\s+` after `<script` consumes ≥1 whitespace; `after_ws` is the first byte
    // past that run (the earliest position a `lang=` prefix may start from).
    let mut after_ws = lt + 7;
    while matches!(src.get(after_ws), Some(b) if b.is_ascii_whitespace()) {
        after_ws += 1;
    }
    // Collect every `lang=` occurrence at or after `<script` + the whitespace, then try them
    // RIGHTMOST-first (the greedy `(?:[^>]*|…)` prefix prefers the last viable `lang=`).
    let mut lang_positions = Vec::new();
    let mut k = lt + 7;
    while k + 5 <= src.len() {
        if src[k..].starts_with(b"lang=") {
            lang_positions.push(k);
        }
        k += 1;
    }
    for &p in lang_positions.iter().rev() {
        if p < after_ws {
            continue; // before the `\s+` — not reachable by the prefix
        }
        // The prefix from `after_ws` to `p` must match the regex's `(?:[^>]*|<attr-aware>)`
        // alternation (either branch).
        if !(lang_prefix_branch_lenient(src, after_ws, p)
            || lang_prefix_branch_attr(src, after_ws, p))
        {
            continue;
        }
        // Read the value `(["'])?([^"' >]+)\1`. `None` ⇒ this `lang=` did not match (empty value,
        // or a quoted value with no immediate closing quote) — try the next-rightmost.
        let Some((is_ts, value_end)) = lang_value_is_ts(src, p + 5) else {
            continue;
        };
        // The trailing `[^>]*>` requires SOME `>` at or after the value end.
        if !src[value_end..].contains(&b'>') {
            continue;
        }
        return Some(is_ts);
    }
    None
}

/// The regex prefix's LENIENT branch `[^>]*`: every byte in `[from, lang_pos)` is a non-`>` byte
/// (the branch stops the open tag at the first bare `>`, NOT honouring quotes).
fn lang_prefix_branch_lenient(src: &[u8], from: usize, lang_pos: usize) -> bool {
    !src[from..lang_pos].contains(&b'>')
}

/// The regex prefix's ATTRIBUTE-AWARE branch `(?:[^=>'"/]+=(?:"[^"]*"|'[^']*'|[^>\s]+)\s+)*`:
/// zero or more `name = value <ws>` attribute groups, where a quoted value may contain `>`. Whether
/// `[from, lang_pos)` matches this branch exactly.
fn lang_prefix_branch_attr(src: &[u8], from: usize, lang_pos: usize) -> bool {
    let n = src.len();
    let mut i = from;
    while i < lang_pos {
        // `[^=>'"/]+` — a non-empty attribute name run.
        let name_start = i;
        while i < lang_pos && !matches!(src[i], b'=' | b'>' | b'\'' | b'"' | b'/') {
            i += 1;
        }
        if i == name_start {
            return false;
        }
        // `=`
        if src.get(i) != Some(&b'=') {
            return false;
        }
        i += 1;
        // value: `"[^"]*"` | `'[^']*'` | `[^>\s]+`
        match src.get(i) {
            Some(&q @ (b'"' | b'\'')) => {
                i += 1;
                while i < n && src[i] != q {
                    i += 1;
                }
                if src.get(i) != Some(&q) {
                    return false;
                }
                i += 1;
            }
            _ => {
                let value_start = i;
                while i < n && !(src[i] == b'>' || src[i].is_ascii_whitespace()) {
                    i += 1;
                }
                if i == value_start {
                    return false;
                }
            }
        }
        // `\s+` — at least one whitespace after the group.
        let ws_start = i;
        while i < n && src[i].is_ascii_whitespace() {
            i += 1;
        }
        if i == ws_start {
            return false;
        }
    }
    i == lang_pos
}

/// Read the `lang` attribute VALUE starting at `at` (the byte just past `lang=`), faithful to the
/// regex value capture `(["'])?([^"' >]+)\1`: an optional opening quote, a NON-EMPTY run of
/// non-quote/space/`>` bytes, and — when an opening quote was present — the matching closing quote
/// IMMEDIATELY after that run. Returns `Some((is_ts, end))` where `is_ts` is whether the value is
/// exactly `ts` and `end` is the byte index after the value (and its closing quote, if any); or
/// `None` when this `lang=` does NOT match (an empty value, or a quoted value with no immediate
/// closing quote — `lang=""` / `lang="ts "` both fail).
fn lang_value_is_ts(src: &[u8], at: usize) -> Option<(bool, usize)> {
    let n = src.len();
    let quote = match src.get(at) {
        Some(&q @ (b'"' | b'\'')) => Some(q),
        _ => None,
    };
    let value_start = if quote.is_some() { at + 1 } else { at };
    let mut j = value_start;
    while j < n && !matches!(src[j], b'"' | b'\'' | b' ' | b'>') {
        j += 1;
    }
    let value = &src[value_start..j];
    if value.is_empty() {
        return None; // `[^"' >]+` requires ≥1 byte
    }
    match quote {
        // Quoted: the closing quote must IMMEDIATELY follow the value run (the `\1`
        // backreference); otherwise the alternative fails and `lang` did not match.
        Some(q) => {
            if src.get(j) == Some(&q) {
                Some((value == b"ts", j + 1))
            } else {
                None
            }
        }
        // Unquoted: the value run is the value; `ts` iff it equals `ts`.
        None => Some((value == b"ts", j)),
    }
}

/// Find the first occurrence of `needle` in `haystack`, returning its start offset.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// The reserved custom-element tag names upstream's `validate_tag` rejects with
/// `svelte_options_reserved_tagname` (`read/options.js`).
pub const RESERVED_CUSTOM_ELEMENT_TAG_NAMES: &[&str] = &[
    "annotation-xml",
    "color-profile",
    "font-face",
    "font-face-src",
    "font-face-uri",
    "font-face-format",
    "font-face-name",
    "missing-glyph",
];

/// Validate a `<svelte:options customElement>` tag NAME exactly as upstream's `validate_tag`
/// (`read/options.js`): a `None` tag (the value was not a string) is
/// `svelte_options_invalid_tagname`; a non-empty tag that fails the valid-custom-element-name
/// pattern is `svelte_options_invalid_tagname`; a reserved tag name is
/// `svelte_options_reserved_tagname`; otherwise `None` (valid). Shared by the parser's
/// Text-valued `customElement` path and the gate's object-`tag` path so both produce the exact
/// same codes from one validator.
///
/// `tag` is `None` when the value is not a string literal (upstream's `typeof tag !== 'string'`
/// branch → `svelte_options_invalid_tagname`).
#[must_use]
pub fn validate_custom_element_tag(tag: Option<&str>) -> Option<&'static str> {
    let Some(tag) = tag else {
        return Some("svelte_options_invalid_tagname");
    };
    // Upstream: `if (tag)` — an empty string skips the pattern/reserved checks (valid).
    if tag.is_empty() {
        return None;
    }
    if !is_valid_custom_element_tag_name(tag) {
        return Some("svelte_options_invalid_tagname");
    }
    if RESERVED_CUSTOM_ELEMENT_TAG_NAMES.contains(&tag) {
        return Some("svelte_options_reserved_tagname");
    }
    None
}

/// Whether `tag` matches upstream's `regex_valid_tag_name` (`read/options.js`):
/// `^[a-z]<char>*-<char>*$` where `<char>` is the custom-element tag-name character class
/// (`[a-z0-9_.·À-ÖØ-öø-ͽͿ-...-]` plus the non-ASCII ranges) — i.e. starts with a lowercase
/// ASCII letter, every char is a tag-name char, and there is at least one `-`.
fn is_valid_custom_element_tag_name(tag: &str) -> bool {
    let mut chars = tag.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let mut has_hyphen = false;
    for c in std::iter::once(first).chain(chars) {
        if c == '-' {
            has_hyphen = true;
        }
        if !is_custom_element_tag_name_char(c) {
            return false;
        }
    }
    has_hyphen
}

/// One character of upstream's `tag_name_char` class (`read/options.js`):
/// `[a-z0-9_.\xB7\xC0-\xD6\xD8-\xF6\xF8-ͽͿ-῿‌-‍‿-⁀`
/// `⁰-↏Ⰰ-⿯、-퟿豈-﷏ﷰ-�\u{10000}-\u{EFFFF}-]`.
fn is_custom_element_tag_name_char(c: char) -> bool {
    matches!(c, 'a'..='z' | '0'..='9' | '_' | '.' | '-' | '\u{B7}')
        || matches!(c as u32,
            0xC0..=0xD6
            | 0xD8..=0xF6
            | 0xF8..=0x37D
            | 0x37F..=0x1FFF
            | 0x200C..=0x200D
            | 0x203F..=0x2040
            | 0x2070..=0x218F
            | 0x2C00..=0x2FEF
            | 0x3001..=0xD7FF
            | 0xF900..=0xFDCF
            | 0xFDF0..=0xFFFD
            | 0x10000..=0xEFFFF)
}

/// One CLOSE-TAG well-formedness violation observed by the parser, mirroring the
/// official `svelte@5.56.3` parse-phase close-tag errors restricted to the close-tag
/// universe (an HTML element open / stray-or-mismatched close / void-content close).
/// A `<svelte:*>` special element / component does NOT participate (its closing is a
/// distinct concern) — only HTML/intrinsic-element close-tag balance is modeled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseTagViolation {
    /// The kind of close-tag violation.
    pub kind: CloseTagViolationKind,
    /// The lowercased tag name the violation concerns (the unclosed element's name,
    /// or the stray / void close tag's name).
    pub tag: String,
    /// The source span of the offending tag (the open tag for an unclosed element, or
    /// the close tag for a stray / void-content close). This is the REPORT / IDE anchor;
    /// it is NOT the official-reject gate's arbitration key (see `encounter_order`).
    pub span: Span,
    /// The parser's monotonic discovery sequence for this defect (assigned when the
    /// defect was PROVEN/recorded during the single forward pass) — the official-reject
    /// gate arbitrates competing parse defects by minimum `encounter_order` so the
    /// FIRST-discovered defect wins (matching official, which stops at the first parse
    /// error). The `span` is the report anchor, which for an `Unclosed` defect is the
    /// open tag even though the defect is only proven at EOF; `encounter_order`, not
    /// `span`, is the arbitration key.
    pub encounter_order: u32,
}

/// The kind of a [`CloseTagViolation`], each mirroring exactly one official
/// parse-phase close-tag error class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseTagViolationKind {
    /// A non-void element left open at EOF — official `element_unclosed`
    /// (`index.js`: a `RegularElement` still on the stack at end of input).
    Unclosed,
    /// A close tag that matched no open element (a stray `</div>` with nothing open,
    /// or a mismatched close that no ancestor closes) — official
    /// `element_invalid_closing_tag` (`element.js`).
    InvalidClosingTag,
    /// A VOID element carrying an explicit close tag / content — official
    /// `void_element_invalid_content` (`element.js`: a `</name>` where `is_void(name)`).
    VoidElementInvalidContent,
}

/// One PARSE-DOMAIN official-reject fact the parser recorded during its single forward
/// pass — the script-attribute / duplicate-script rejects and the explicit-`</p>`
/// autoclose. These are parse-phase compile errors official throws WHILE parsing (a
/// `<script>`'s `read_script` attribute validation, the second-`<script>` duplicate
/// check, the surviving-`</p>` autoclose close), so — like [`CloseTagViolation`] and
/// [`SvelteStrictParseError`] — they carry an [`encounter_order`](Self::encounter_order)
/// minted at the DISCOVERY point and arbitrate by minimum encounter order against the
/// other parse-defect rails (never by source span). They are the SOLE parse-error
/// source for these classes — the official-reject gate no longer re-derives them from
/// a span-keyed AST scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvelteParseRejectFact {
    /// The kind of parse-domain reject (which official class it mirrors).
    pub kind: SvelteParseRejectKind,
    /// The exact official `svelte@5.56.3` diagnostic code this reject mirrors (e.g.
    /// `script_reserved_attribute`, `script_invalid_attribute_value`,
    /// `element_invalid_closing_tag_autoclosed`). A multi-code class (the script
    /// `context` vs valued-`module` site) carries its EXACT site code here.
    pub official_code: &'static str,
    /// The REPORT / IDE anchor span — the offending `<script>` open tag's attribute, the
    /// duplicate `<script>` open tag, or the surviving `</p>` close tag. This is NOT the
    /// arbitration key (see `encounter_order`).
    pub span: Span,
    /// The parser's monotonic discovery sequence for this defect (drawn from the shared
    /// defect counter at the moment it was recorded during the single forward pass) — the
    /// official-reject gate arbitrates competing parse defects by minimum `encounter_order`
    /// so the FIRST-discovered defect wins (matching official, which stops at the first
    /// parse error). `span`, the report anchor, never arbitrates.
    pub encounter_order: u32,
}

/// The kind of a [`SvelteParseRejectFact`], each mirroring one official parse-phase
/// reject class the parser is the authority for (the `<script>` attribute / duplicate
/// rejects and the explicit-`</p>` autoclose). Distinct from the strict-parse and
/// close-tag rails (a different parser-recorded fact stream); the gate maps each kind to
/// its `CoreOfficialValidationRule`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteParseRejectKind {
    /// A `<script>` carrying a RESERVED attribute (`server` / `client` / `worker` /
    /// `test` / `default`) — official `script_reserved_attribute`.
    ScriptReservedAttribute,
    /// A `<script>` with an invalid `context` value, or a valued `module="x"` — official
    /// `script_invalid_context` / `script_invalid_attribute_value` (the exact site code is
    /// on `official_code`).
    ScriptInvalidContext,
    /// A DUPLICATE top-level `<script>` (a second instance or module script) — official
    /// `script_duplicate`.
    ScriptDuplicate,
    /// A DUPLICATE top-level `<style>` (a second component-level `<style>`) — official
    /// `style_duplicate`.
    StyleDuplicate,
    /// A NESTED / non-root root-only `<svelte:*>` meta element (a `<svelte:options>` /
    /// `<svelte:head>` / … NOT at the component root) — official
    /// `svelte_meta_invalid_placement`.
    SvelteMetaInvalidPlacement,
    /// A `<script>` body that fails to parse — official `js_parse_error`. Minted from the
    /// RESERVED body-probe slot by the official-reject gate (which holds OXC), at the
    /// reserved `encounter_order` (the upstream-faithful body-parse position), NOT from the
    /// body span or gate execution time.
    ScriptBodyParse,
    /// A DUPLICATE attribute / directive on a TEMPLATE element open tag — official
    /// `attribute_duplicate`. Minted during the open-tag attribute loop at the second
    /// occurrence (the parser point analogous to upstream throwing during the open-tag
    /// parse), so it competes in encounter-order arbitration against later close/strict
    /// defects.
    AttributeDuplicate,
    /// A DUPLICATE root-only `<svelte:*>` meta element (a second `<svelte:options>` /
    /// `<svelte:head>` / …) — official `svelte_meta_duplicate`. Minted when the second
    /// such root-only meta tag is encountered.
    SvelteMetaDuplicate,
    /// An explicit `</p>` closing a `<p>` the browser already auto-closed (a direct
    /// disallowed block child) — official `element_invalid_closing_tag_autoclosed`.
    ParagraphAutoclose,
    /// An invalid `<svelte:options>` attribute or child content — a FAMILY of official
    /// `read_options` / `disallow_children` codes (`svelte_options_unknown_attribute`,
    /// `svelte_options_invalid_attribute_value`, `svelte_options_deprecated_tag`,
    /// `svelte_options_invalid_tagname`, `svelte_options_invalid_customelement`,
    /// `svelte_options_invalid_attribute`, `svelte_meta_invalid_content`). The exact site code is
    /// carried on [`SvelteParseRejectFact::official_code`]. Minted by the parser's `read_options`
    /// FINALIZATION (after the root walk), so it arbitrates later than every template/script/style
    /// defect — exactly like upstream, which runs `read_options` in the constructor after the walk.
    OptionsInvalid,
}

impl ParsedSvelte {
    /// The instance-script content span, if an instance `<script>` is present.
    #[must_use]
    pub fn instance_content(&self) -> Option<Span> {
        self.instance_script.as_ref().and_then(|s| s.content)
    }

    /// The module-script content span, if a `<script module>` is present.
    #[must_use]
    pub fn module_content(&self) -> Option<Span> {
        self.module_script.as_ref().and_then(|s| s.content)
    }
}

/// The explicit reactivity-mode forced by a top-level `<svelte:options runes={…}>`
/// element (Svelte's own forced-mode switch). Returns `Some(true)` for `runes` /
/// `runes={true}`, `Some(false)` for `runes={false}`, and `None` when no
/// `<svelte:options runes>` is present (the caller then falls back to rune-USAGE
/// detection).
///
/// Only a TOP-LEVEL options element counts (Svelte requires `<svelte:options>` at
/// the component root). This is the SINGLE shared syntactic query both the IDE TSX
/// projector (legacy-mode classification) and the runtime-IR mode inference
/// consume — the parse-tree types are owned here, so the query lives here rather
/// than being forked per surface.
#[must_use]
pub fn forced_runes_option(source: &str, nodes: &[SvelteNode]) -> Option<bool> {
    for node in nodes {
        if let SvelteNode::Element(el) = node {
            if matches!(
                el.kind,
                SvelteElementKind::Special(SvelteSpecialKind::Options)
            ) {
                if let Some(v) = runes_option_value(source, el) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// The `runes` option value on a `<svelte:options>` element: a valueless `runes`
/// boolean-shorthand is `true`; `runes={true}` / `runes={false}` read the literal;
/// any other form is treated as absent (`None`).
fn runes_option_value(source: &str, el: &SvelteElement) -> Option<bool> {
    el.attributes.iter().find_map(|a| match &a.kind {
        SvelteAttributeKind::Plain { name, value, .. } if name == "runes" => match value {
            // `runes` (no value) — boolean shorthand ⇒ true.
            None => Some(true),
            // `runes={true}` / `runes={false}` — read the expression literal.
            Some(SvelteAttributeValue::Expression(span)) => {
                let text = source[span.start as usize..span.end as usize].trim();
                match text {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => None,
                }
            }
            _ => None,
        },
        _ => None,
    })
}

/// One `<script>` block (instance or module).
#[derive(Debug, Clone)]
pub struct SvelteScript {
    /// Whether this is the module script (`<script module>` /
    /// `<script context="module">`).
    pub is_module: bool,
    /// The full open-tag span (`<script ...>`).
    pub tag_open: Span,
    /// The script content span (between the open and close tags), if any.
    pub content: Option<Span>,
    /// The recognised close-tag span, including `</script` through `>`.
    /// `None` for self-closing or unterminated blocks.
    pub tag_close: Option<Span>,
    /// The raw attribute spans on the open tag.
    pub attributes: Vec<SvelteAttribute>,
    /// The `lang` attribute value, if present (`ts` / `tsx` / …).
    pub lang: Option<String>,
}

impl SvelteScript {
    /// The FIRST `<script>` open-tag attribute (in SOURCE ORDER) that fails official's
    /// `read_script` post-body semantic-attribute validation
    /// (`phases/1-parse/read/script.js`), with its EXACT official code + span — or `None`
    /// when every attribute is valid. `source` is the original component source the
    /// attribute-value spans index into.
    ///
    /// Mirrors upstream's attribute loop EXACTLY: it iterates attributes in source order
    /// and, per attribute, applies the per-attribute checks in upstream order — a RESERVED
    /// name (`server` / `client` / `worker` / `test` / `default`) is `script_reserved_attribute`;
    /// a VALUED `module="x"` (module is boolean-only) is `script_invalid_attribute_value`;
    /// an invalid `context` value (anything but the text `"module"` — a valueless / expression
    /// / non-`module` text value) is `script_invalid_context`. The FIRST faulting attribute in
    /// source order wins (upstream throws on it before reaching any later attribute), so the
    /// order is NOT a duplicate→reserved→context bucket.
    ///
    /// The attribute NAME is matched CASE-SENSITIVELY, mirroring official's
    /// `RESERVED_ATTRIBUTES.includes(attribute.name)` / `attribute.name === 'context'` /
    /// `=== 'module'` checks: `Context` / `Module` / `Server` (any non-lowercase form) is an
    /// UNKNOWN attribute (a warning, accepted), never a reject — so it is not over-rejected.
    #[must_use]
    pub fn first_semantic_attribute_fault(&self, source: &str) -> Option<(&'static str, Span)> {
        const RESERVED: &[&str] = &["server", "client", "worker", "test", "default"];
        for attr in &self.attributes {
            let SvelteAttributeKind::Plain { name, value, .. } = &attr.kind else {
                continue;
            };
            // Per-attribute checks in upstream order: reserved first, then module, then
            // context. Each attribute name is distinct, so at most one fires per attribute;
            // the loop's source order decides which attribute faults first.
            if RESERVED.contains(&name.as_str()) {
                return Some(("script_reserved_attribute", attr.span));
            }
            if name == "module" {
                // A `module` attribute is valid ONLY valueless (`<script module>`); a valued
                // `module="x"` is `script_invalid_attribute_value`.
                if value.is_some() {
                    return Some(("script_invalid_attribute_value", attr.span));
                }
            } else if name == "context" {
                // A `context` attribute is valid ONLY as the text value `"module"`; a
                // valueless `context`, an expression value, or any other text value is
                // `script_invalid_context`.
                let valid_module = matches!(
                    value,
                    Some(SvelteAttributeValue::Text(span))
                        if source.get(span.start as usize..span.end as usize) == Some("module")
                );
                if !valid_module {
                    return Some(("script_invalid_context", attr.span));
                }
            }
        }
        None
    }
}

/// One component-level `<style>` block — opaque content (CSS domain).
#[derive(Debug, Clone)]
pub struct SvelteStyle {
    /// The full open-tag span.
    pub tag_open: Span,
    /// The style content span, if any.
    pub content: Option<Span>,
    /// The recognised close-tag span, including `</style` through `>`.
    /// `None` for self-closing or unterminated blocks.
    pub tag_close: Option<Span>,
    /// The raw attribute spans on the open tag.
    pub attributes: Vec<SvelteAttribute>,
}

/// One template node.
#[derive(Debug, Clone)]
pub enum SvelteNode {
    /// Literal text run.
    Text(Span),
    /// `<!-- ... -->` comment.
    Comment(Span),
    /// A `{expr}` interpolation. The span covers the inner expression (excludes
    /// the braces).
    Interpolation(Span),
    /// A regular element, component, or special element.
    Element(SvelteElement),
    /// A block construct (`{#if}` / `{#each}` / `{#await}` / `{#key}` /
    /// `{#snippet}`).
    Block(SvelteBlock),
    /// A standalone tag (`{@render}` / `{@html}` / `{@const}` / `{@debug}` /
    /// `{@attach}` / declaration tags `{const}` / `{let}`).
    Tag(SvelteTag),
}

/// A template element / component / special element.
#[derive(Debug, Clone)]
pub struct SvelteElement {
    /// The tag name (`div`, `MyComponent`, `svelte:head`, …).
    pub name: String,
    /// The span of the tag name in the source.
    pub name_span: Span,
    /// The element's structural kind.
    pub kind: SvelteElementKind,
    /// The attributes / directives on the open tag.
    pub attributes: Vec<SvelteAttribute>,
    /// The child nodes (empty for self-closing / void elements).
    pub children: Vec<SvelteNode>,
    /// Whether the element was self-closing (`<x />`).
    pub self_closing: bool,
    /// The full open-tag span.
    pub open_span: Span,
    /// The full span of the MATCHING `</name>` close tag — `start` at the `<` of
    /// `</name`, `end` just past the closing `>`. `None` for a self-closing or
    /// unterminated element. Recorded by the string/brace-aware child walk (the
    /// parser is the close-tag authority); consumers read this instead of
    /// re-deriving the close tag with a literal-unaware source scan.
    pub close_span: Option<Span>,
}

/// The structural kind of an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SvelteElementKind {
    /// A lowercase intrinsic HTML element (`div`, `span`, …).
    Intrinsic,
    /// An uppercase or dotted component reference (`Foo`, `Foo.Bar`).
    Component,
    /// A `<svelte:*>` special element, carrying its closed kind.
    Special(SvelteSpecialKind),
    /// A nested `<style>` element inside template markup (opaque, CSS domain).
    NestedStyle,
}

/// The closed family of `<svelte:*>` special elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteSpecialKind {
    /// `<svelte:head>`.
    Head,
    /// `<svelte:window>`.
    Window,
    /// `<svelte:document>`.
    Document,
    /// `<svelte:body>`.
    Body,
    /// `<svelte:element this={...}>`.
    Element,
    /// `<svelte:boundary>`.
    Boundary,
    /// `<svelte:options>`.
    Options,
    /// `<svelte:component this={C}>` (dynamic component, F8).
    Component,
    /// `<svelte:self>` (recursive self reference, F8).
    SelfRef,
    /// `<svelte:fragment slot="x">` (transparent slot-grouping fragment, F9).
    Fragment,
    /// An unrecognised `<svelte:*>` name — parsed without crash.
    Unknown,
}

impl SvelteSpecialKind {
    /// Classify a `svelte:<local>` special-element local name.
    #[must_use]
    pub fn from_local(local: &str) -> Self {
        match local {
            "head" => Self::Head,
            "window" => Self::Window,
            "document" => Self::Document,
            "body" => Self::Body,
            "element" => Self::Element,
            "boundary" => Self::Boundary,
            "options" => Self::Options,
            "component" => Self::Component,
            "self" => Self::SelfRef,
            "fragment" => Self::Fragment,
            _ => Self::Unknown,
        }
    }
}

/// One attribute or directive on an element open tag.
#[derive(Debug, Clone)]
pub struct SvelteAttribute {
    /// The attribute kind (plain / directive / spread / …).
    pub kind: SvelteAttributeKind,
    /// The full attribute span (name + value).
    pub span: Span,
}

/// The closed family of attribute / directive kinds.
///
/// Every current-docs attribute and directive form is represented; a row's
/// SUPPORTED/OUT-OF-SCOPE status is a projector concern, not a parser one.
#[derive(Debug, Clone)]
pub enum SvelteAttributeKind {
    /// A plain attribute (`class="x"`, `id={expr}`, shorthand `{value}`,
    /// CSS custom property `--name={expr}`). Carries the name and the optional
    /// value span (an interpolation value span excludes braces).
    Plain {
        /// The attribute name (e.g. `class`, `onclick`, `--accent`).
        name: String,
        /// The name span.
        name_span: Span,
        /// The value span (string body or `{expr}` inner), if present.
        value: Option<SvelteAttributeValue>,
    },
    /// A spread attribute (`{...rest}`). Carries the inner-expression span.
    Spread(Span),
    /// A directive attribute (`bind:`, `class:`, `style:`, `use:`,
    /// `transition:`/`in:`/`out:`, `animate:`, legacy `on:`).
    Directive(SvelteDirective),
    /// An attribute-position `{@attach expr}` attachment (the official `AttachTag`,
    /// valid ONLY inside an open tag — the child-position form is the official
    /// `expected_tag` parse reject). Carries the inner EXPRESSION span (after the
    /// `@attach` keyword + separating whitespace), captured cleanly at tokenization —
    /// downstream lowering never re-slices the body text.
    Attach {
        /// The attachment expression span.
        expr_span: Span,
    },
}

/// An attribute value (a quoted string body or an interpolation expression).
#[derive(Debug, Clone)]
pub enum SvelteAttributeValue {
    /// A quoted string value body (span excludes the quotes).
    Text(Span),
    /// A single `{expr}` value (span excludes the braces).
    Expression(Span),
    /// A mixed/concatenated value (string + interpolation runs) — the whole
    /// value span is recorded; the parser does not split the runs.
    Mixed(Span),
}

/// A directive attribute.
#[derive(Debug, Clone)]
pub struct SvelteDirective {
    /// The directive kind.
    pub kind: SvelteDirectiveKind,
    /// The directive's local name (the part after the `:`, before any
    /// modifiers) — e.g. `value` in `bind:value`, `click` in `on:click`.
    pub local: String,
    /// The `|modifier` list (e.g. `|important`, `|local`, `|stop`).
    pub modifiers: Vec<String>,
    /// The value expression span (`{expr}` inner or quoted body), if present.
    /// A two-expression function binding `bind:x={get, set}` records the whole
    /// inner span (both expressions); the projector splits it.
    pub value: Option<SvelteAttributeValue>,
}

/// The closed family of directive kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteDirectiveKind {
    /// `bind:` two-way binding (incl. function-binding `bind:x={get, set}`).
    Bind,
    /// `class:` conditional class.
    Class,
    /// `style:` inline style (+ `|important`).
    Style,
    /// `use:` action.
    Use,
    /// `transition:`.
    Transition,
    /// `in:` (one-way transition in).
    In,
    /// `out:` (one-way transition out).
    Out,
    /// `animate:`.
    Animate,
    /// Legacy `on:` event listener.
    On,
    /// `let:` slot-prop binding (`<C let:item={alias}>` / shorthand `let:item`).
    Let,
    /// An unrecognised `name:` directive — parsed without crash.
    Unknown,
}

impl SvelteDirectiveKind {
    /// Classify a directive prefix (the part before the first `:`).
    #[must_use]
    pub fn from_prefix(prefix: &str) -> Self {
        match prefix {
            "bind" => Self::Bind,
            "class" => Self::Class,
            "style" => Self::Style,
            "use" => Self::Use,
            "transition" => Self::Transition,
            "in" => Self::In,
            "out" => Self::Out,
            "animate" => Self::Animate,
            "on" => Self::On,
            "let" => Self::Let,
            _ => Self::Unknown,
        }
    }
}

/// A block construct.
#[derive(Debug, Clone)]
pub struct SvelteBlock {
    /// The block kind.
    pub kind: SvelteBlockKind,
    /// The full block span (open tag through the closing `{/...}`).
    pub span: Span,
    /// The grammar-balanced opening-tag span, including the `{#` prefix and
    /// closing `}`. The parser records this while it owns the balanced brace
    /// boundary so downstream projectors never rescan ambiguous source bytes.
    pub head_span: Span,
    /// The block's primary head expression span (the `{#if expr}` condition,
    /// the `{#each list as item}` list, the `{#await expr}` promise, the
    /// `{#key expr}` key). `None` for `{#snippet}` (its head is the name +
    /// params, recorded on the snippet kind).
    pub head_expr: Option<Span>,
    /// The block's children (the immediate body run).
    pub children: Vec<SvelteNode>,
    /// Branch clauses (`{:else if}` / `{:else}` / `{:then}` / `{:catch}`).
    pub clauses: Vec<SvelteBlockClause>,
}

/// The closed family of block kinds.
#[derive(Debug, Clone)]
pub enum SvelteBlockKind {
    /// `{#if expr}` … `{/if}`.
    If,
    /// `{#each list as item, index (key)}` … `{/each}`. Records the `as`
    /// binding span (absent for the `{#each {length: n}}` no-item form), the
    /// optional index binding span, and the optional `(key)` span.
    Each {
        /// The `as <pattern>` binding span, if present.
        item: Option<Span>,
        /// The `, <index>` binding span, if present.
        index: Option<Span>,
        /// The `(<key>)` expression span, if present.
        key: Option<Span>,
    },
    /// `{#await expr}` … `{:then v}` … `{:catch e}` … `{/await}`.
    Await {
        /// The `{:then <pattern>}` binding span, if present.
        then_binding: Option<Span>,
        /// The `{:catch <pattern>}` binding span, if present.
        catch_binding: Option<Span>,
    },
    /// `{#key expr}` … `{/key}`.
    Key,
    /// `{#snippet name(params)}` … `{/snippet}`. Records the name span and the
    /// parameter list span.
    Snippet {
        /// The snippet name span.
        name: Span,
        /// The snippet name as text.
        name_text: String,
        /// The `(params)` span (excludes the parens), if present.
        params: Option<Span>,
    },
}

/// One branch clause of a block.
#[derive(Debug, Clone)]
pub struct SvelteBlockClause {
    /// The clause kind.
    pub kind: SvelteClauseKind,
    /// The clause's expression/binding span, if any (the `{:else if expr}`
    /// condition, the `{:then v}` binding).
    pub expr: Option<Span>,
    /// The clause-tag head span — the whole `{:else}` / `{:else if d}` /
    /// `{:then v}` / `{:catch e}` head INCLUDING the braces. The projector
    /// OVERWRITES this span directly (no source reverse-scan), so an empty
    /// clause (`{:else}` / `{:then}` / `{:catch}` with no expr and no children)
    /// is still rewritten and never leaks raw `{:…}` into the projected TSX.
    pub tag_span: Span,
    /// The clause's children.
    pub children: Vec<SvelteNode>,
}

/// The closed family of clause kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteClauseKind {
    /// `{:else if expr}`.
    ElseIf,
    /// `{:else}`.
    Else,
    /// `{:then v}`.
    Then,
    /// `{:catch e}`.
    Catch,
}

/// A standalone tag.
#[derive(Debug, Clone)]
pub struct SvelteTag {
    /// The tag kind.
    pub kind: SvelteTagKind,
    /// The full tag span.
    pub span: Span,
    /// The tag's inner expression / declaration span (excludes the braces and
    /// the leading keyword).
    pub inner: Span,
}

/// The closed family of standalone-tag kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteTagKind {
    /// `{@render snippet(args)}`.
    Render,
    /// `{@html expr}`.
    Html,
    /// `{@const x = expr}` (documented legacy since 5.56).
    LegacyConst,
    /// `{const x = expr}` (5.56 declaration tag).
    Const,
    /// `{let x = expr}` (5.56 declaration tag).
    Let,
    /// `{@debug var1, var2}`.
    Debug,
    /// `{@attach expr}` (5.29).
    Attach,
    /// An unrecognised `{@name ...}` tag — parsed without crash.
    Unknown,
}

/// One inline parse diagnostic.
#[derive(Debug, Clone)]
pub struct SvelteParseDiagnostic {
    /// A short machine-stable code (e.g. `unterminated-block`).
    pub code: &'static str,
    /// A human-readable message.
    pub message: String,
    /// The diagnostic span.
    pub span: Span,
}

/// One STRICT-PARSE-error fact: a markup form Verter's forgiving/recovery-based parser
/// silently ACCEPTS but the official `svelte@5.56.3` STRICT parser REJECTS as a
/// compile error.
///
/// The parser is intentionally infallible (it always produces a faithful tree, never a
/// `SyntaxError`), which is correct for the IDE projection (it owns its own error
/// recovery). For the CLIENT runtime, though, the contract is "Verter emits a `Main` ⇔
/// official ACCEPTS the same source"; a recovery point that accepts markup official
/// rejects would emit a DIVERGENT `Main` (official: "compile error, no module";
/// Verter: "module exists"). So every recovery point that official rejects pushes one
/// of these facts onto [`ParsedSvelte::strict_parse_errors`], and the official-reject
/// gate refuses on a non-empty fact list (the single
/// `CoreOfficialValidationRule::ParserStrictness` rule) BEFORE lowering — so no `Main`
/// is emitted. The `official_code` is the exact pinned-compiler diagnostic code the
/// recovery point mirrors (test metadata only; the runtime contract is "no `Main`").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvelteStrictParseError {
    /// The kind of strict-parse error (which recovery point it came from).
    pub kind: SvelteStrictParseErrorKind,
    /// The source span of the offending markup. Used to report the FIRST strict-parse
    /// error in document order (mirroring official, which stops at the first parse
    /// error).
    pub span: Span,
    /// The exact official `svelte@5.56.3` diagnostic code this recovery point mirrors
    /// (e.g. `tag_invalid_name`, `expected_token`, `element_unclosed`). Carried so the
    /// refusal — and the parse-parity freshness corpus — pin the precise official code.
    pub official_code: &'static str,
    /// The parser's monotonic discovery sequence for this defect (assigned when the
    /// defect was PROVEN/recorded during the single forward pass) — the official-reject
    /// gate arbitrates competing parse defects by minimum `encounter_order` so the
    /// FIRST-discovered defect wins (matching official, which stops at the first parse
    /// error). The `span` is the report anchor, which for an `Unclosed` defect is the
    /// open tag even though the defect is only proven at EOF; `encounter_order`, not
    /// `span`, is the arbitration key.
    pub encounter_order: u32,
}

/// The kind of a [`SvelteStrictParseError`], each mirroring exactly one official
/// parse-phase error class. Every variant is emitted by a NAMED strict-fact helper at a
/// real recovery point (no dead variants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteStrictParseErrorKind {
    /// A `<` followed by a byte that cannot begin a tag name (`<` in text, `< `, `<.`,
    /// `<{`, …) — official `tag_invalid_name`.
    TagInvalidName,
    /// A close tag carrying a trailing token before `>` (`</div x>`) — official
    /// `expected_token`.
    ExpectedToken,
    /// An attribute `=` with no following value (`id=`, `lang=`) — official
    /// `expected_attribute_value`.
    ExpectedAttributeValue,
    /// A nameless close tag (`</>`) — official `element_invalid_closing_tag`.
    ElementInvalidClosingTag,
    /// An element open tag (intrinsic, script, or style) that never reaches its `>` /
    /// matching close before EOF, OR a raw-block close carrying a trailing token (the
    /// close is not recognised, so the block is left open) — official `element_unclosed`.
    ElementUnclosed,
    /// An end of input reached mid-construct where the official parser expects more
    /// input (an unterminated quoted attribute value, an unterminated comment, a `</` at
    /// EOF) — official `unexpected_eof`.
    UnexpectedEof,
    /// A top-level `<style>` whose CSS reader reaches EOF inside an unterminated rule (no
    /// `</style>` close) — official's CSS parser errors `css_expected_identifier` (NOT
    /// `element_unclosed`, which is the RAW-block close-recognition failure used for
    /// `<script>`).
    CssExpectedIdentifier,
}

impl SvelteStrictParseErrorKind {
    /// The exact official `svelte@5.56.3` diagnostic code this kind mirrors.
    #[must_use]
    pub fn official_code(self) -> &'static str {
        match self {
            Self::TagInvalidName => "tag_invalid_name",
            Self::ExpectedToken => "expected_token",
            Self::ExpectedAttributeValue => "expected_attribute_value",
            Self::ElementInvalidClosingTag => "element_invalid_closing_tag",
            Self::ElementUnclosed => "element_unclosed",
            Self::UnexpectedEof => "unexpected_eof",
            Self::CssExpectedIdentifier => "css_expected_identifier",
        }
    }
}
