<!-- unified-charter-v2
id=FMTCS0
name=Native SCSS printer extension
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMTC0
owner=expansion.formatter:SCSS printer and SCSS format-view contribution
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
charter=charters/expansion-formatter/FMTCS0.md
max_production_loc=600
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMTCS0 — Native SCSS printer extension

## Independently acceptable outcome and rollback

Add a complete private SCSS view/printer extension over accepted CSS while preserving every CSS fixture. SCSS becomes independently usable by later carrier composition without waiting for Less. Reverting removes only SCSS contributions.

The sole owner is **SCSS printer and SCSS format-view contribution**.

## Surfaces, predecessor, and architecture

- Surfaces: `crates/verter_formatter/src/printers/mod.rs` for printer registration, `crates/verter_formatter/src/printers/scss/**`, `crates/verter_language/src/formatter/mod.rs` for view registration, and `crates/verter_language/src/formatter/scss/**`. Both registration files count against the six-file ceiling.
- Owns `ScssFormatView`/`ScssPrinterExtension` for SCSS variables, nesting, interpolation, mixins/functions, directives, comments, and recovery.
- **FMTC0** supplies complete CSS behavior, extension seams, configuration, provenance, and shared geometry through ancestry.
- CSS behavior is reused, not copied; Less, framework style policy, routes, and public adapters are excluded.

Internal testable subblocks are SCSS view population, printer extension, recovery/trivia handling, and differential/idempotence/performance evidence. Identity is source revision + SCSS language identity + FCFG0 provenance + FMT1P provenance table; no partial/cancelled/stale result publishes as complete.

## Acceptance, deletion, and budgets

- **FMTCS0-AC1:** admitted SCSS cells format deterministically and all CSS fixtures remain byte-identical.
- **FMTCS0-AC2:** SCSS view/render evidence preserves FMT1P IDs and truthful malformed/unsupported outcomes.
- **FMTCS0-AC3:** edits/maps/range/cursor/idempotence use only shared authorities.
- **FMTCS0-AC4:** FMT0 work/allocation/latency/stack/cancellation/zero-work limits hold for SCSS.
- No route, public API, carrier consumer, or deletion changes. The default private-prototype deletion population is empty and requires amendment before use.
- Ceiling: 600 LOC, 6 files, 2 crates. Abort if Less/framework/service/public/parser/third-package work enters.
- Verify SCSS differential fixtures plus unchanged CSS corpus, `cargo nextest run -p verter_formatter -p verter_language`, and `targeted-domain`.
- Unlocks Vue/Svelte style composition together with FMTCL0. Add only FMTCS0's ledger row.
