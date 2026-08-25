# J1/A16 — architecture deviation memo: the landed `svelte_compat.rs` relocation does not close A16

**Status: CLOSED.** The independent `CssParser` is deleted. `style_body_reject_code`
parses the parser-minted body span through `parse_style_ir` and projects the first
Svelte reject code from `StyleSyntaxIr` facts. The official-reject gate admits a
clean IR so `analyze_style_body` reuses it (one grammar, one parse). See
`compile_client_parses_a_style_body_once` and `crates/verter_css_syntax/tests/cases/svelte_compat_profile.rs`.

The remainder of this memo is the historical F1 finding that this close implements.

## Summary

Block `css3` (branch `block/svelte-css-grammar`, worktree `verter-css3`) landed two commits
(`28f7ae77b`, `cae15c69e`) that delete `crates/verter_compiler/src/svelte/runtime/css_reject.rs` and
relocate its 837-line independent `CssParser` to `crates/verter_css_syntax/src/svelte_compat.rs`. A
pre-landing review (F1) found this relocation does not satisfy the ratified invariant A16 was written
against. A falsification consult (codex, `gpt-5.6-sol`, read-only, `model_reasoning_effort=xhigh`,
transcript `/tmp/css3-f1-consult.out`, session `01a02d44-24b4-71c2-9a0a-0ab7f2730801`) confirmed the
review is correct and that closing it properly is block-sized new work, not a fix-cycle patch.

## The deviation, in `governance.md` §10 form

```text
Failed assumption:
  The css3 block assumed that hosting css_reject.rs's reader inside verter_css_syntax (the one crate
  that owns all CSS-family parsing/scanning production code) and sharing its lexer token stream plus
  two "sole authority" shape-matcher functions (percentage_len_at, nth_of_len_at) with the general
  grammar's own selector projections was sufficient to satisfy A16 and the governing
  ONE-CSS-PARSER-PARSE-ONCE invariant. It is not: the maintainer's ruling text is "we ONLY need 1 CSS
  parser and we only parse once, no extra scanners are to be done if the item has been parsed already"
  (docs/arch/refactor/rev11/rulings/MAINTAINER-RULING-ONE-CSS-PARSER-PARSE-ONCE.md:19), stating
  explicitly "No second parser, no private grammar, no fallback parser" and "a second parse... is a
  violation regardless of which surface performs it" (same file, lines 22, 38). Being hosted in the
  authority crate does not make a second, independently-invoked parse into the first one.

Measured/source evidence:
  - `style_body_reject_code` (crates/verter_css_syntax/src/svelte_compat.rs:121) constructs its OWN
    `Lexer` and its own `CssParser` and runs `read_body()` independently of `parse_style_ir`. It is
    called once per `<style>` block from `official_reject.rs:669`, and the normal Svelte style
    pipeline separately calls `parse_style_ir` from `svelte/runtime/css/mod.rs:130` on the SAME bytes.
    Two independent parses of the same CSS body, on two different code paths, is exactly what the
    ruling forbids "regardless of which surface performs it."
  - `CssParser::body_finished` (svelte_compat.rs:298-301) rediscovers the `</style>` boundary by
    matching raw source (`self.matches(b"</style") || self.at_eof()`) rather than consuming a
    parser-minted content-span fact, even though the carrier tokenizer already has the complete
    content span (tokenizer.rs:1143) — a narrower instance of the same "carrier geometry from
    registered facts" violation the review's F2 finding was checking for.
  - `parse_style_ir`/`StyleSyntaxIr` (crates/verter_css_syntax/src/style_ir.rs) is a general,
    dialect-parameterized (CSS/SCSS/Sass/Less/Stylus) grammar. Its only production mode
    (`CssParseMode::Recover`, used at all 4 production call sites) collects diagnostics and never
    fails on an ordinary defect; its `CssDiagnosticKind` taxonomy (~14 variants) has no equivalent to
    `css_empty_declaration` and no dedicated "invalid selector" kind; and byte-level behavior
    genuinely diverges from what the Svelte race needs (e.g. the general lexer's string-token
    consumer bails on an embedded raw newline; upstream's raw-run value reader does not, by design —
    svelte_compat.rs:22-30). So `style_body_reject_code` cannot simply call `parse_style_ir` and
    translate its output today; the general grammar's own state machine does not yet carry Svelte's
    compatibility control flow at all.
  - J1's own generating ruling (docs/arch/refactor/rev11/evidence/J1/css-family-authority-inventory-gap.md:36-53)
    already specified the required shape: "Extend verter_css_syntax with a Svelte-5.56.3 compatibility
    validation projection/profile that produces the first typed failure (or exact code) using the
    canonical parser authority... Ensure this is part of the one canonical parse/result carried
    forward for the style, not another [admission/rejection] pass." The landed relocation satisfies
    the literal charter test list (docs/arch/refactor/rev11/charters/J1.md:317 —
    svelte_compat_profile.rs exists and passes; official_reject.rs itself performs no lexing;
    css_reject.rs is path-absent) but not this generating intent: the reader is still an
    independently-invoked private grammar, merely relocated one file over.

Affected architecture/verification invariants:
  - MAINTAINER-RULING-ONE-CSS-PARSER-PARSE-ONCE — NOT satisfied by the current tree.
  - J1-A16 (charter text: "css_reject.rs (row 6) deleted; its ... diagnostic-race behavior reproduced
    by a verter_css_syntax compatibility validation profile") — the deletion half is done; the
    "canonical parser authority" half (per the generating consult, not just the charter's literal test
    list) is not.
  - J1-A24 (the union invariant across A1-A23) — NOT satisfied while A16 is open.
  - Carrier Geometry From Registered Facts (CLAUDE.md, MANDATORY) — the `</style>` boundary rescan is
    a narrower, independent instance of the same class of violation.
  - Compiled-Output Conformance (CLAUDE.md, CRITICAL) — NOT at risk from this deviation: the current
    reader still reproduces the exact upstream first-error race and exact error codes; the objection
    is architectural placement/authority, not observable conformance. Conformance must not regress
    while this is corrected.

Compatibility or consumer consequences:
  None externally observable today: `official_reject_gate`'s output is unchanged, and no wire format,
  cache identity, or public API is touched by this deviation. The consequence is internal-only: a
  second parse of the same bytes runs on every `<style>` block, and the crate's own "one canonical
  parse" invariant is not yet load-bearing for this reader the way it is for every other CSS-family
  consumer in the inventory.

Alternatives:
  1. FORCE-LAND the current relocation against F1 by reinterpreting A16's literal charter test list
     (which the current tree does pass) as the sole authority, ignoring the generating consult's
     "canonical parser authority" requirement and the maintainer's explicit ONE-CSS-PARSER-PARSE-ONCE
     text. REJECTED: the ruling text is unambiguous ("no second parser, no private grammar... a second
     parse... is a violation regardless of which surface performs it"), and reinterpreting a charter's
     test list to narrow a maintainer ruling is exactly the "narrow a fix to make it land" pattern this
     block's fix-cycle brief and CLAUDE.md's Stub Prevention rule forbid.
  2. WRAPPER: introduce a shared entry point (e.g. `parse_style_ir(..., CssParseMode::SvelteRejectProbe)`)
     that dispatches internally to the unchanged private `CssParser`. REJECTED per the consult: this
     changes API topology, not authority, parse count, or dataflow — "indistinguishable only to a weak
     name/path scanner," and CLAUDE.md's clean-cutover rule forbids a wrapper that preserves the old
     implementation beside the new one.
  3. FULL CORRECTIVE IMPLEMENTATION NOW, in this fix cycle: build a first-class Svelte-5.56.10
     compatibility-profile axis inside `verter_css_syntax`'s own parser state machine (orthogonal to
     `CssDialect` and `CssRecoveryPolicy`), consume a parser-minted content-span/close-boundary fact
     instead of rescanning for `</style>`, emit a typed `SvelteStyleRejectKind` fact from the profile's
     control flow (not a translation of the generic ~14-variant taxonomy, which has no 1:1 mapping),
     store that fact on the parser-minted style record, and make `official_reject` and the normal
     Svelte CSS pipeline both consume the SAME one parse. This is what actually closes A16/A24.
     REJECTED FOR THIS FIX CYCLE — not because it is wrong, but because it touches parser
     request/profile configuration, lexer behavior, parser control flow, the `StyleSyntaxIr` artifact
     shape, carrier-minted style-envelope facts, `ParsedSvelte` ownership, official-reject arbitration,
     the normal Svelte CSS artifact-reuse path, and every existing compat/exact-code test — a
     block-sized surface, not a bounded fix. Attempting it unchartered risks exactly the kind of
     under-scoped, self-reviewed change the maintainer's stop-and-escalate rules exist to prevent.
  4. RESCOPE: disposition F1 as `ADOPT-NOW`, but as a named corrective J1 slice (e.g.
     `block/svelte-css-reject-profile`) chartered, implemented, and reviewed on its own before J1
     acceptance — reopening A16/A24 as explicitly not-yet-satisfied rather than accepted-by-relocation.
     This is a superset of alternative 3 that gives it the scoping and review weight the ratified
     ONE-CSS-PARSER-PARSE-ONCE + Compiled-Output-Conformance intersection deserves. RECOMMENDED.

Recommended disposition:
  ADOPT-NOW / RESCOPE, at alternative 4. F1 is accepted as a real, ratified-rule violation — not an
  over-application of doctrine (the consult's Claim C verdict: FALSE) and not satisfied by relocation
  alone (Claim A verdict: FALSE). It is NOT eligible for `DEFER` past J1 acceptance: J1's own ledger
  states partial slices do not constitute acceptance until every acceptance ID is covered
  (docs/arch/architecture-lock/ledger/program-state.toml:1457), and J1 permits landing as slices
  (docs/arch/refactor/rev11/rulings/MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md:50), so a `DEFER` here
  would leave A16/A24 unsatisfied at J1 acceptance with no accepted mechanism to catch that. Owner:
  a maintainer/architecture-ratified corrective slice, scoped per alternative 3's outline, landing
  before J1 is marked accepted. This candidate (css3 / block/svelte-css-grammar) should NOT be marked
  ACCEPTED while A16/A24 stand open under this deviation.

Work that remains valid:
  Everything else this fix cycle lands (F2 confirmed no further raw-source scanners exist in the live
  Svelte CSS compile path beyond this one; F3's oracle-parity strengthening; F4's analyzer/guard fixes;
  F5's fail-closed conversions) is independent of how A16's corrective slice is eventually implemented
  and would not be invalidated by it landing later.
```

## What a ruling on this memo must decide

1. **Disposition** — `ADOPT-NOW` (as the rescoped corrective slice above), `DEFER` (against the ledger
   rule cited above, which appears to forbid it for an acceptance-blocking ID), or `REJECT` (a reading
   under which A16/A24 are already satisfied by relocation — the consult found no support for this).
2. **Ownership and naming** of the corrective slice, and whether it lands as a new named block or an
   explicit scope expansion of an existing one.
3. **Whether this candidate (`css3`) may land ahead of the corrective slice** with A16/A24 explicitly
   recorded as open, or must hold until the corrective slice lands first.

## Supporting evidence

- Falsification consult transcript: `/tmp/css3-f1-consult.out` (session `01a02d44-24b4-71c2-9a0a-0ab7f2730801`,
  `gpt-5.6-sol`, `model_reasoning_effort=xhigh`, read-only sandbox). Verdicts: Claim A FALSE, Claim B
  PARTLY TRUE, Claim C FALSE, Claim D TRUE-with-correction, wrapper option FALSE-as-compliance.
- `docs/arch/refactor/rev11/rulings/MAINTAINER-RULING-ONE-CSS-PARSER-PARSE-ONCE.md` — the governing
  ratified text.
- `docs/arch/refactor/rev11/evidence/J1/css-family-authority-inventory-gap.md` — A16's generating
  consult, "Consult 1 — css_reject.rs disposition."
- `docs/arch/refactor/rev11/charters/J1.md:158,295,317,325` — the inventory row, A4, A16, A24 text.
