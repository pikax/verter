<!-- unified-charter-v2
id=FMT2TX
name=Native TSX printer extension
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT2T,FMT2X
owner=expansion.formatter:TSX printer and TSX format-view contribution
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
charter=charters/expansion-formatter/FMT2TX.md
max_production_loc=500
max_production_files=5
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT2TX — Native TSX printer extension

## Independently acceptable outcome and rollback

Join the accepted TypeScript and JSX printer seams into the complete FMT0-admitted TSX view/printer extension. The result is the independently usable terminal script-language contribution consumed by Vue and Svelte. Reverting removes only TSX contributions while JavaScript, TypeScript, and JSX remain accepted.

The sole owner is **TSX printer and TSX format-view contribution**.

## Surfaces and boundaries

- Production surfaces: `crates/verter_formatter/src/printers/mod.rs` for printer registration, `crates/verter_formatter/src/printers/tsx/**`, `crates/verter_language/src/formatter/mod.rs` for view registration, and `crates/verter_language/src/formatter/tsx/**`. Both registration files count against the five-file ceiling.
- Owns `TsxFormatView` and `TsxPrinterExtension` for the interaction of TypeScript syntax with JSX elements/fragments, type arguments, attributes, expressions, comments, and malformed recovery.
- Consumes FMT2T's TypeScript rules and FMT2X's JSX rules without copying or redefining either authority.
- Framework templates, carrier composition, routing, public DTOs, and conversions are excluded.

Internal testable subblocks are TSX view population, TypeScript/JSX seam composition, TSX-only recovery/trivia handling, and differential/idempotence/performance evidence. Identity is source revision + TSX language identity + FCFG0 provenance + FMT1P provenance table; no partial, cancelled, stale, or unsupported result publishes as complete.

## Exact predecessor contracts

- **FMT2T:** supplies accepted JavaScript/TypeScript behavior and the TypeScript extension seam.
- **FMT2X:** supplies accepted JavaScript/JSX behavior and the JSX extension seam.

## Acceptance, migration, and budgets

- **FMT2TX-AC1:** every admitted TSX cell formats deterministically without changing accepted JavaScript, TypeScript, or JSX fixtures.
- **FMT2TX-AC2:** TSX source-backed fragments preserve FMT1P provenance and malformed/unsupported TSX fails truthfully.
- **FMT2TX-AC3:** full/range/cursor/edit/map behavior and idempotence use only shared authorities; TypeScript and JSX rules are reused rather than forked.
- **FMT2TX-AC4:** FMT0 work/allocation/latency/stack/cancellation/zero-work limits hold for the TSX corpus.
- No route, capability, public API, or deletion changes. The deletion population is explicitly empty; discovery of a candidate private prototype requires a pre-mutation STOP and FMT0 amendment naming its exact path/symbol and owner.
- Ceiling: 500 LOC, 5 files, 2 related crates. Abort if framework-template policy, service routing, public/wire work, parser ownership, or a third package enters.
- Verify TSX differential fixtures plus unchanged JavaScript/TypeScript/JSX corpora, `cargo nextest run -p verter_formatter -p verter_language`, and `targeted-domain`.
- Unlocks FMTV0 and FMTS0 together with the completed style extensions. Add only FMT2TX's ledger row.
