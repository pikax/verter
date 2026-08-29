<!-- unified-charter-v2
id=FMT2
name=Native JavaScript printer
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT1D,FMT1E,FCFG0
owner=expansion.formatter:JavaScript printer and JavaScript format-view contribution
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
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/expansion-formatter/FMT2.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT2 — Native JavaScript printer

## Independently acceptable outcome and rollback

Land one complete private JavaScript authored-view contribution and native printer for the FMT0-admitted JavaScript cells. It is usable by later TypeScript/JSX and carrier contributions without either successor. Reverting removes only JavaScript view/printer code; the shared substrate and current formatting route remain unchanged.

The sole owner is **JavaScript printer and JavaScript format-view contribution**.

## Surfaces and named boundaries

- Production surfaces: `crates/verter_formatter/src/lib.rs` and `crates/verter_formatter/src/printers/mod.rs` for module registration, `crates/verter_formatter/src/printers/javascript/**`, `crates/verter_language/src/lib.rs` and `crates/verter_language/src/formatter/mod.rs` for module registration, and `crates/verter_language/src/formatter/javascript/**`; dispatch binds exact files/symbols before mutation. All registration files count against the eight-file ceiling.
- Owned boundaries: `JavaScriptFormatView`, `JavaScriptPrinter`, JavaScript statement/expression/module rules, JavaScript comment/trivia/recovery policy, and `Doc<FormatProvenanceId>` production.
- Consumed unchanged: FMT1P provenance/coordinates, FMT1 renderer, FMT1A view contract, FMT1B–FMT1E edit/map/range/cursor geometry, and FCFG0 configuration.
- TypeScript, JSX/TSX, CSS-family, HTML, framework composition, service routing, public DTOs, and boundary conversion are excluded.

## Exact predecessor contracts

- **FMT1D:** supplies safe authored range expansion and edit containment.
- **FMT1E:** supplies cursor affinity/projection over FMT1C map mechanics.
- **FCFG0:** supplies the sole normalized formatter option vocabulary and provenance.

## Binding architecture and subblocks

1. JavaScript view population exposes source-backed syntax/trivia/recovery units with FMT1P IDs and performs zero parse.
2. Statement/expression/module printers emit `Doc` without semantic analysis or post-render string surgery.
3. Comment/trivia/recovery handling preserves every admitted authored byte and returns truthful unsupported outcomes.
4. Differential/idempotence/range/cursor evidence proves only JavaScript cells and the FMT0 numeric envelope.

No subblock may advertise a route or return a public formatter result independently.

Identity/publication law: computation is bound to source revision, JavaScript language identity, FCFG0 config provenance, and FMT1P provenance table. This node owns no warm result cache; cancelled, unsupported, stale, or partial output is never published as complete.

## Acceptance and performance evidence

- **FMT2-AC1 — JavaScript completeness:** every admitted JavaScript grammar/option/recovery cell has deterministic output; an unsupported construct cannot return success.
- **FMT2-AC2 — provenance truth:** every source-backed `Doc` fragment carries the matching FMT1P ID; planted dropped/swapped IDs fail map construction.
- **FMT2-AC3 — composition:** full/range/cursor/edit/map behavior composes through FMT1B–FMT1E without reimplementation and repeated formatting is stable.
- **FMT2-AC4 — bounded work:** zero parser invocations; view, `Doc`, render, allocation, stack, latency, cancellation, and zero-work evidence meet FMT0 for JavaScript fixtures.

## Migration, deletions, and forbidden designs

- This node changes no live formatter route, capability, session dispatcher, public adapter, or legacy consumer.
- The ratified deletion population is empty. A source-confirmed private JavaScript printer prototype under the two owned surfaces requires an FMT0 amendment naming its symbol before same-node replacement/deletion.
- Forbid TypeScript/JSX branches, subprocess formatters, OXC codegen as an unqualified formatting oracle, second parsing, semantic-AST pretty printing, whole-file fallback, and format-after-build surgery.

## Budgets, aborts, verification, and consumers

- Ceiling: 800 production LOC, 8 files, 2 related crates; mandatory split before exceeding any M-node quality ceiling.
- Abort if JavaScript cannot land without TypeScript/JSX semantics, if an admitted cell requires a second parser, or if route/public work enters.
- Verify focused formatter/language tests, pinned JavaScript differential fixtures, `cargo nextest run -p verter_formatter -p verter_language`, and `targeted-domain`.
- Unlocks independent FMT2T and FMT2X extensions. Add only FMT2's ledger row; later carriers consume the converged FMT2TX script contribution by typed registry identity.
