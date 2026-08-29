<!-- unified-charter-v2
id=FMTCL0
name=Native Less printer extension
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMTC0
owner=expansion.formatter:Less printer and Less format-view contribution
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
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/expansion-formatter/FMTCL0.md
max_production_loc=600
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMTCL0 — Native Less printer extension

## Independently acceptable outcome and rollback

Add a complete private Less view/printer extension over accepted CSS while preserving every CSS fixture. Less becomes independently usable by later carrier composition without waiting for SCSS. Reverting removes only Less contributions.

The sole owner is **Less printer and Less format-view contribution**.

## Surfaces, predecessor, and architecture

- Surfaces: `crates/verter_formatter/src/printers/mod.rs` for printer registration, `crates/verter_formatter/src/printers/less/**`, `crates/verter_language/src/formatter/mod.rs` for view registration, and `crates/verter_language/src/formatter/less/**`. Both registration files count against the six-file ceiling.
- Owns `LessFormatView`/`LessPrinterExtension` for variables, nesting, mixins, guards, interpolation, detached rulesets, comments, and recovery.
- **FMTC0** supplies complete CSS behavior, extension seams, configuration, provenance, and shared geometry through ancestry.
- CSS behavior is reused, not copied; SCSS, framework style policy, routes, and public adapters are excluded.

Internal testable subblocks are Less view population, printer extension, recovery/trivia handling, and differential/idempotence/performance evidence. Identity is source revision + Less language identity + FCFG0 provenance + FMT1P provenance table; no partial/cancelled/stale result publishes as complete.

## Acceptance, deletion, and budgets

- **FMTCL0-AC1:** admitted Less cells format deterministically and all CSS fixtures remain byte-identical.
- **FMTCL0-AC2:** Less view/render evidence preserves FMT1P IDs and truthful malformed/unsupported outcomes.
- **FMTCL0-AC3:** edits/maps/range/cursor/idempotence use only shared authorities.
- **FMTCL0-AC4:** FMT0 work/allocation/latency/stack/cancellation/zero-work limits hold for Less.
- No route, public API, carrier consumer, or deletion changes. The default private-prototype deletion population is empty and requires amendment before use.
- Ceiling: 600 LOC, 6 files, 2 crates. Abort if SCSS/framework/service/public/parser/third-package work enters.
- Verify Less differential fixtures plus unchanged CSS corpus, `cargo nextest run -p verter_formatter -p verter_language`, and `targeted-domain`.
- Unlocks Vue/Svelte style composition together with FMTCS0. Add only FMTCL0's ledger row.
