<!-- unified-charter-v2
id=FMT2T
name=Native TypeScript printer extension
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT2
owner=expansion.formatter:TypeScript printer and TypeScript format-view contribution
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
charter=charters/expansion-formatter/FMT2T.md
max_production_loc=600
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT2T — Native TypeScript printer extension

## Independently acceptable outcome and rollback

Extend the private JavaScript printer with complete FMT0-admitted TypeScript syntax/view rules while preserving JavaScript behavior byte-for-byte. The result formats JavaScript and TypeScript without JSX/TSX and can be accepted independently. Reverting removes only TypeScript contributions.

The sole owner is **TypeScript printer and TypeScript format-view contribution**.

## Surfaces and boundaries

- Surfaces: `crates/verter_formatter/src/printers/mod.rs` for printer registration, `crates/verter_formatter/src/printers/typescript/**`, `crates/verter_language/src/formatter/mod.rs` for view registration, and `crates/verter_language/src/formatter/typescript/**`. Both registration files count against the six-file ceiling.
- Owns `TypeScriptFormatView` and `TypeScriptPrinterExtension` for type syntax, declarations, modifiers, assertions, and TypeScript-specific trivia/recovery.
- Consumes FMT2's JavaScript printer and all shared provenance/edit/map/range/cursor/config contracts unchanged.
- JSX/TSX, carrier composition, routing, public adapters, and conversions are excluded.

Internal testable subblocks are TypeScript view population, printer extension, recovery/trivia handling, and differential/idempotence/performance evidence. Identity is source revision + TypeScript language identity + FCFG0 provenance + FMT1P provenance table; no partial/cancelled/stale result publishes as complete.

## Exact predecessor contract

- **FMT2:** supplies a complete private JavaScript printer/view population and the extension seam; its ancestry supplies all shared geometry and configuration contracts.

## Acceptance, migration, and budgets

- **FMT2T-AC1:** admitted TypeScript-only syntax formats deterministically while every FMT2 JavaScript fixture remains byte-identical.
- **FMT2T-AC2:** TypeScript view entries and rendered segments preserve FMT1P IDs; malformed/unsupported TypeScript remains truthful.
- **FMT2T-AC3:** full/range/cursor/edit/map and idempotence evidence uses the shared substrate with no TypeScript-local geometry.
- **FMT2T-AC4:** FMT0 structural/allocation/latency/stack/cancellation/zero-work limits hold for the TypeScript corpus.
- No live route, capability, public API, or deletion changes. The default deletion population is empty; an owned private prototype must be named by amendment before replacement.
- Ceiling: 600 LOC, 6 files, 2 crates. Abort if JSX/TSX, service, public/wire, parser, or a third package enters.
- Verify focused TypeScript differential fixtures, unchanged JavaScript corpus, `cargo nextest run -p verter_formatter -p verter_language`, and `targeted-domain`.
- Unlocks FMT2TX together with FMT2X. Add only FMT2T's ledger row.
