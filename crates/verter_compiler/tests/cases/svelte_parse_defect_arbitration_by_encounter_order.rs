//! ARCHITECTURE GUARD: the Svelte official-reject gate arbitrates PARSE defects PURELY by
//! the parser's `encounter_order` (the single forward-pass discovery sequence) — never by
//! source span, and never via a pre-stream script-domain priority pass.
//!
//! The gate consumes one parser-owned, encounter-ordered defect stream (close-tag +
//! strict-parse + parse-domain reject facts) and selects the minimum-`encounter_order`
//! unsuppressed defect — matching the official `svelte@5.56.3` compiler, which stops at the
//! FIRST parse error. Two regressions this gate has historically carried are FORBIDDEN:
//!   (a) MIXED-UNIT / SPAN arbitration — picking the defect with the smallest `span.start`
//!       (a byte offset), so a late-PROVEN outer defect anchored at an earlier span (an
//!       `Unclosed` proven at EOF) wrongly out-ranks an earlier-DISCOVERED inner defect; and
//!   (b) a PRE-STREAM SCRIPT-DOMAIN priority pass — returning a `<script>` attribute /
//!       duplicate reject BEFORE consulting the encounter-ordered stream, so a later script
//!       defect pre-empts an EARLIER template parse defect.
//!
//! This guard is DISCRIMINATING and STRUCTURAL without editing production `src/`: it
//! computes, from the gate's PUBLIC parser facts (`ParsedSvelte::{close_tag_violations,
//! strict_parse_errors, parse_reject_facts}`, each carrying both a `span` and an
//! `encounter_order`), THREE reference picks — the architecturally-correct
//! `encounter_order` pick, the FORBIDDEN span-min pick, and the FORBIDDEN script-first pick
//! — and asserts three properties (each wrapped continuation kept on one logical line):
//!
//! - (1) the REAL gate's official code EQUALS the encounter-order pick on every curated case (the ordering invariant the architecture promises);
//! - (2) on the span-sensitive case the span-min pick DIFFERS from the encounter-order pick (so the case discriminates), AND the real gate does NOT match the span-min pick (so the gate is not span-arbitrated);
//! - (3) on the script-sensitive case the script-first pick DIFFERS from the encounter-order pick (so the case discriminates), AND the real gate does NOT match the script-first pick (so the gate has no pre-stream script-priority pass).
//!
//! If production reverts to span arbitration, (2) fails (the gate would match the span-min
//! pick); if it reverts to a script-first pre-pass, (3) fails (the gate would match the
//! script-first pick). The discrimination is exercised on real parsed fixtures, never by
//! mutating production source.

use verter_compiler::svelte::parser::{
    parse_svelte, CloseTagViolationKind, ParsedSvelte, SvelteParseRejectKind,
};
use verter_compiler::svelte::runtime::official_reject_gate;

/// The §1.2-core scaffold pieces (a reactive `<script>` + `<button>`), matching the reject
/// corpus convention so a case is otherwise a §1.2-core `Main`.
const SCRIPT: &str = "<script>let c = $state(0);</script>";
const BUTTON: &str = "<button onclick={() => c++}>{c}</button>";

/// The official code the REAL gate produces for `source` (panics if the gate accepts — every
/// case here is a multi-defect reject).
fn gate_code(source: &str) -> &'static str {
    let parsed = parse_svelte(source);
    official_reject_gate(source, &parsed)
        .unwrap_or_else(|| panic!("gate accepted a multi-defect reject fixture:\n{source}"))
        .official_code
}

/// One PUBLIC parse-defect fact reduced to the three fields arbitration could key on: its
/// `encounter_order`, its `span.start`, the official code it would report, and whether it is a
/// SCRIPT-DOMAIN reject (a `<script>` attribute / duplicate fact — the family the forbidden
/// pre-stream pass would prioritize).
struct DefectFact {
    encounter_order: u32,
    span_start: u32,
    official_code: &'static str,
    is_script_domain: bool,
}

/// Flatten the gate's three PUBLIC parser fact rails into one `DefectFact` list (the same
/// facts the gate arbitrates). `Unclosed` close-tag facts are kept as-is here (the curated
/// cases avoid the implicit-`<p>` autoclose suppression so this flattening matches the gate's
/// selection exactly). This is the data a span-based / encounter-based / script-first
/// arbitrator each reduces differently.
fn defect_facts(parsed: &ParsedSvelte) -> Vec<DefectFact> {
    let mut facts = Vec::new();
    for v in &parsed.close_tag_violations {
        let code = match v.kind {
            CloseTagViolationKind::Unclosed => "element_unclosed",
            CloseTagViolationKind::InvalidClosingTag => "element_invalid_closing_tag",
            CloseTagViolationKind::VoidElementInvalidContent => "void_element_invalid_content",
        };
        facts.push(DefectFact {
            encounter_order: v.encounter_order,
            span_start: v.span.start,
            official_code: code,
            is_script_domain: false,
        });
    }
    for e in &parsed.strict_parse_errors {
        facts.push(DefectFact {
            encounter_order: e.encounter_order,
            span_start: e.span.start,
            official_code: e.official_code,
            is_script_domain: false,
        });
    }
    for f in &parsed.parse_reject_facts {
        // The SCRIPT-DOMAIN family the forbidden pre-stream pass would prioritize: the
        // `<script>`-specific rejects (reserved / context-or-module / duplicate-script). A
        // template `attribute_duplicate` / duplicate `<svelte:options>` / `<p>` autoclose are
        // template-positioned parse defects, NOT the script pre-pass family.
        let is_script_domain = matches!(
            f.kind,
            SvelteParseRejectKind::ScriptReservedAttribute
                | SvelteParseRejectKind::ScriptInvalidContext
                | SvelteParseRejectKind::ScriptDuplicate
        );
        facts.push(DefectFact {
            encounter_order: f.encounter_order,
            span_start: f.span.start,
            official_code: f.official_code,
            is_script_domain,
        });
    }
    // NOTE: the RESERVED script-body-parse slots (`parsed.script_body_probes`) are NOT
    // flattened here — this reference reduces only the parser's already-decided defect facts,
    // and a body-probe's disposition needs an OXC parse the gate owns (a CLEAN body contributes
    // no defect). The curated cases below avoid body-parse defects; the body-probe arbitration
    // (a body `js_parse_error` beating a later reserved-attr / duplicate-script reject) is
    // covered by the independent exact-code rail (`svelte_parse_defect_exact_codes`).
    facts
}

/// The CORRECT architecture's pick: the official code of the minimum-`encounter_order` defect.
fn encounter_order_pick(source: &str) -> Option<&'static str> {
    let parsed = parse_svelte(source);
    defect_facts(&parsed)
        .into_iter()
        .min_by_key(|f| f.encounter_order)
        .map(|f| f.official_code)
}

/// The FORBIDDEN span-min pick: the official code of the minimum-`span.start` defect (mixed-
/// unit / span arbitration — the regression that lets a late-proven outer `Unclosed` anchored
/// at an earlier byte out-rank an earlier-discovered inner defect).
fn span_min_pick(source: &str) -> Option<&'static str> {
    let parsed = parse_svelte(source);
    defect_facts(&parsed)
        .into_iter()
        .min_by_key(|f| f.span_start)
        .map(|f| f.official_code)
}

/// The FORBIDDEN script-first pick: if ANY script-domain reject fact exists, its code (the
/// earliest such by encounter order, mirroring a per-script pre-pass) — BEFORE consulting the
/// rest of the stream. Otherwise the encounter-order pick. This models the old pre-stream
/// script-priority pass.
fn script_first_pick(source: &str) -> Option<&'static str> {
    let parsed = parse_svelte(source);
    let facts = defect_facts(&parsed);
    facts
        .iter()
        .filter(|f| f.is_script_domain)
        .min_by_key(|f| f.encounter_order)
        .map(|f| f.official_code)
        .or_else(|| {
            facts
                .iter()
                .min_by_key(|f| f.encounter_order)
                .map(|f| f.official_code)
        })
}

#[test]
fn gate_official_code_equals_the_encounter_order_pick_across_the_curated_set() {
    // INVARIANT (1): the REAL gate's chosen code is ALWAYS the minimum-`encounter_order` defect
    // across the parser's three fact rails — for every curated multi-defect case. This is the
    // architecture's promise; it fails if any rail is arbitrated by something other than
    // encounter order. Each case is a genuine multi-defect reject (verified against the pinned
    // svelte@5.56.3).
    let cases: &[(&str, &str)] = &[
        // inner stray close (discovered first) vs outer EOF-unclosed.
        (
            "inner_stray_vs_outer_unclosed",
            &concat_case("<div></span>"),
        ),
        // inner void-content close (discovered at `</input>`) vs outer EOF-unclosed (the
        // `<div>` is left OPEN — no `</div>` — so the two genuinely compete).
        (
            "inner_void_vs_outer_unclosed",
            &concat_case("<div><input></input>"),
        ),
        // nested-<a> placement (analyze) vs a later <div bar=> empty-attr (parse strict).
        (
            "placement_vs_strict",
            &concat_case("<a href=\"/\"><a href=\"/x\">x</a></a>\n<div bar=></div>"),
        ),
        // a trailing stray </div> (parse) vs nested-<button> placement (analyze).
        (
            "parse_close_vs_placement",
            &concat_case("<button><button>x</button></button></div>"),
        ),
        // a stray </span> (template, earlier) vs a later script-domain reject.
        (
            "template_close_vs_script",
            "</span>\n<script context=\"bad\">let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        // the module script's reserved attr (earlier) vs the instance script's bad context.
        (
            "module_reject_vs_instance_reject",
            "<script module server>const K = 1;</script>\n<script context=\"bad\">let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n",
        ),
        // an inner <div id=> empty-attr (parse strict) vs the surviving </p> autoclose.
        (
            "inner_parse_vs_autoclose",
            &concat_case("<p><div id=></div></p>"),
        ),
        // the </p> autoclose (earlier) vs a later trailing stray </span>.
        (
            "autoclose_vs_later_stray",
            &concat_case_two("<p><div>x</div></p>", "</span>"),
        ),
    ];
    let mut mismatches = Vec::new();
    for (name, source) in cases {
        let gate = gate_code(source);
        let expected = encounter_order_pick(source)
            .unwrap_or_else(|| panic!("{name}: the fixture produced no parse defect"));
        if gate != expected {
            mismatches.push(format!(
                "{name}: gate reports `{gate}`, but the minimum-encounter-order defect is \
                 `{expected}` — the gate is NOT arbitrating by encounter order"
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "the official-reject gate diverged from the encounter-order arbitration invariant:\n{}",
        mismatches.join("\n")
    );
}

#[test]
fn gate_is_not_span_min_arbitrated_on_a_span_sensitive_case() {
    // INVARIANT (2): on a case where the span-min pick DIFFERS from the encounter-order pick,
    // the real gate must match the ENCOUNTER-ORDER pick, NOT the span-min pick. `<div></span>`
    // is exactly such a case: the `<div>` open tag (the EOF-unclosed defect's anchor) is at an
    // EARLIER byte than the inner `</span>` stray close, but the `</span>` is DISCOVERED first.
    let source = concat_case("<div></span>");
    let span = span_min_pick(&source).expect("span-min pick");
    let enc = encounter_order_pick(&source).expect("encounter pick");
    // The case must discriminate: the two arbitrators DISAGREE (else the test proves nothing).
    assert_ne!(
        span, enc,
        "the span-sensitive fixture is not discriminating: span-min and encounter-order picks \
         agree (`{span}`) — pick a case where the outer unclosed anchors before the inner defect"
    );
    let gate = gate_code(&source);
    assert_eq!(
        gate, enc,
        "the gate must match the encounter-order pick (`{enc}`) on the span-sensitive case"
    );
    assert_ne!(
        gate, span,
        "MIXED-UNIT/SPAN ARBITRATION REGRESSION: the gate matched the span-min pick (`{span}`) — \
         it is arbitrating parse defects by source span, not encounter order"
    );
}

#[test]
fn gate_has_no_pre_stream_script_priority_pass_on_a_script_sensitive_case() {
    // INVARIANT (3): on a case where the script-first pick DIFFERS from the encounter-order
    // pick, the real gate must match the ENCOUNTER-ORDER pick, NOT the script-first pick.
    // `</span><script context="bad">…` is such a case: the stray `</span>` (a template parse
    // defect) is discovered BEFORE the later `<script context="bad">` script-domain reject, but
    // a pre-stream script pass would return the script reject first.
    let source =
        "</span>\n<script context=\"bad\">let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
    let script = script_first_pick(source).expect("script-first pick");
    let enc = encounter_order_pick(source).expect("encounter pick");
    assert_ne!(
        script, enc,
        "the script-sensitive fixture is not discriminating: script-first and encounter-order \
         picks agree (`{script}`) — pick a case with an EARLIER template defect than the script \
         reject"
    );
    let gate = gate_code(source);
    assert_eq!(
        gate, enc,
        "the gate must match the encounter-order pick (`{enc}`) on the script-sensitive case"
    );
    assert_ne!(
        gate, script,
        "PRE-STREAM SCRIPT-PRIORITY REGRESSION: the gate matched the script-first pick \
         (`{script}`) — a script-domain reject is being returned before the encounter-ordered \
         parse stream is consulted"
    );
}

#[test]
fn placement_is_gated_behind_an_empty_parse_stream_not_keyed_against_it() {
    // The analyze-phase placement check (`node_invalid_placement`) must run ONLY on a clean
    // parse — any parse defect beats it. Proven by a discriminating pair:
    //  - a CLEAN parse with only a nested-<button> placement defect ⇒ node_invalid_placement;
    //  - the SAME placement defect PLUS a trailing stray </div> parse defect ⇒ the parse defect
    //    wins (NOT node_invalid_placement), because the parse stream is non-empty so the
    //    analyze-phase placement check never runs.
    let clean_placement = concat_case("<button><button>x</button></button>");
    assert_eq!(
        gate_code(&clean_placement),
        "node_invalid_placement",
        "a clean-parse nested-<button> must reach the analyze-phase placement check"
    );
    let placement_plus_parse_defect = concat_case("<button><button>x</button></button></div>");
    assert_ne!(
        gate_code(&placement_plus_parse_defect),
        "node_invalid_placement",
        "a parse defect (the trailing stray </div>) must beat the analyze-phase placement check \
         — placement is gated behind an EMPTY parse-defect stream, not keyed against it"
    );
}

/// Wrap a single template fragment in the §1.2-core scaffold (script + the fragment + button).
fn concat_case(fragment: &str) -> String {
    format!("{SCRIPT}\n{fragment}\n{BUTTON}\n")
}

/// Wrap two template fragments (one before, one after) around the button in the §1.2-core
/// scaffold: `<script>…</script>\n<frag_a>\n<frag_b>\n<button>…`.
fn concat_case_two(frag_a: &str, frag_b: &str) -> String {
    format!("{SCRIPT}\n{frag_a}\n{frag_b}\n{BUTTON}\n")
}
