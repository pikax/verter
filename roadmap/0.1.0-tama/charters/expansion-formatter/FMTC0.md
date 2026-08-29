<!-- unified-charter-v2
id=FMTC0
name=Native CSS printer
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT1D,FMT1E,FCFG0
owner=expansion.formatter:CSS printer and CSS format-view contribution
conflict_domains=doc,style_semantics
resource_class=rust-mixed
review_profile=architecture-3
gate_profile=targeted-domain
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/expansion-formatter/FMTC0.md
max_production_loc=700
max_production_files=7
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMTC0 — Native CSS printer

## Independently acceptable outcome and rollback

Land one complete private CSS authored-view contribution and native printer for FMT0-admitted CSS cells. It is usable without SCSS or Less and is the shared base those extensions consume. Reverting removes only CSS view/printer code and changes no route.

The sole owner is **CSS printer and CSS format-view contribution**.

## Surfaces and boundaries

- Production surfaces: `crates/verter_formatter/src/lib.rs` and `crates/verter_formatter/src/printers/mod.rs` for module registration, `crates/verter_formatter/src/printers/css/**`, `crates/verter_language/src/lib.rs` and `crates/verter_language/src/formatter/mod.rs` for module registration, and `crates/verter_language/src/formatter/css/**`. All registration files count against the seven-file ceiling.
- Owns `CssFormatView`, `CssPrinter`, CSS rule/declaration/value/at-rule printing, CSS comments/custom-property trivia, and recovery.
- Consumes FMT1P–FMT1E and FCFG0 unchanged; framework style semantics, SCSS, Less, routing, and public conversion are excluded.

## Exact predecessor contracts

- **FMT1D/FMT1E:** supply range containment and cursor projection over the shared map/edit substrate.
- **FCFG0:** supplies the sole normalized style-format option vocabulary and provenance.

## Binding subblocks and acceptance

1. CSS views reuse existing syntax artifacts and assign FMT1P provenance IDs with zero parse.
2. CSS printer rules emit `Doc` for admitted grammar without post-render surgery.
3. Comments/custom properties/recovery preserve authored bytes and unsupported truth.
4. Differential/idempotence/range/cursor/performance evidence covers CSS only.

Identity/publication law: computation is bound to source revision, CSS language identity, FCFG0 config provenance, and FMT1P provenance table. This node owns no warm result cache; cancelled, unsupported, stale, or partial output is never published as complete.

- **FMTC0-AC1:** every admitted CSS cell has deterministic output and unsupported CSS cannot report success.
- **FMTC0-AC2:** source-backed CSS fragments retain exact FMT1P provenance.
- **FMTC0-AC3:** edits/maps/range/cursor compose only through FMT1B–FMT1E and repeated formatting is stable.
- **FMTC0-AC4:** FMT0 structural/allocation/latency/stack/cancellation/zero-work bounds hold.

## Migration, deletion, budgets, and consumers

- No route, capability, session dispatcher, public API, or carrier consumer changes.
- The deletion population is empty. Any private CSS prototype under the owned surfaces must be named by FMT0 amendment before same-node replacement.
- Forbid SCSS/Less branches, framework scoping, Stylelint fixes, subprocess formatting, second parsing, and format-after-build surgery.
- Ceiling: 700 LOC, 7 files, 2 crates; split before exceeding an M-node quality ceiling.
- Abort if CSS requires SCSS/Less, public/wire, route, parser, or third-package work.
- Verify focused CSS fixtures, `cargo nextest run -p verter_formatter -p verter_language`, and `targeted-domain`.
- Unlocks independent FMTCS0 and FMTCL0. Add only FMTC0's ledger row.
