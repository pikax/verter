<!-- unified-charter-v2
id=FMT2X
name=Native JSX printer extension
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT2
owner=expansion.formatter:JSX printer and JSX format-view contribution
conflict_domains=doc
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
charter=charters/expansion-formatter/FMT2X.md
max_production_loc=500
max_production_files=5
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT2X — Native JSX printer extension

## Independently acceptable outcome and rollback

Add the complete FMT0-admitted JSX view/printer extension over the accepted JavaScript population. The result formats JavaScript and JSX without TypeScript or TSX and is independently usable by JavaScript-facing consumers. Reverting removes only JSX contributions.

The sole owner is **JSX printer and JSX format-view contribution**.

## Surfaces and boundaries

- Production surfaces: `crates/verter_formatter/src/printers/mod.rs` for printer registration, `crates/verter_formatter/src/printers/jsx/**`, `crates/verter_language/src/formatter/mod.rs` for view registration, and `crates/verter_language/src/formatter/jsx/**`. Both registration files count against the five-file ceiling.
- Owns `JsxFormatView` and `JsxPrinterExtension` for JSX elements/fragments, attributes, expression containers, comments, and malformed recovery.
- Consumes FMT2's JavaScript printer and all shared provenance/edit/map/range/cursor/config contracts unchanged.
- TypeScript, TSX, framework templates, carrier composition, routing, public DTOs, and conversions are excluded.

Internal testable subblocks are JSX view population, JSX printer rules, recovery/trivia handling, and differential/idempotence/performance evidence. Identity is source revision + JSX language identity + FCFG0 provenance + FMT1P provenance table; no partial, cancelled, stale, or unsupported result publishes as complete.

## Exact predecessor contract

- **FMT2:** supplies the accepted JavaScript printer/view population and JSX extension seam; its ancestry supplies FMT0/FMT1P–FMT1E/FCFG0 contracts.

## Acceptance, migration, and budgets

- **FMT2X-AC1:** every admitted JSX cell formats deterministically without changing accepted JavaScript fixtures.
- **FMT2X-AC2:** JSX source-backed fragments preserve FMT1P provenance and malformed/unsupported JSX fails truthfully.
- **FMT2X-AC3:** full/range/cursor/edit/map behavior and idempotence use only shared authorities.
- **FMT2X-AC4:** FMT0 work/allocation/latency/stack/cancellation/zero-work limits hold for the JSX corpus.
- No route, capability, public API, or deletion changes. The deletion population is explicitly empty; discovery of a candidate private prototype requires a pre-mutation STOP and FMT0 amendment naming its exact path/symbol and owner.
- Ceiling: 500 LOC, 5 files, 2 related crates. Abort if TypeScript/TSX, framework-template policy, service routing, public/wire work, parser ownership, or a third package enters.
- Verify JSX differential fixtures plus the unchanged JavaScript corpus, `cargo nextest run -p verter_formatter -p verter_language`, and `targeted-domain`.
- Unlocks FMT2TX. Add only FMT2X's ledger row.
