<!-- unified-charter-v2
id=FMT1A
name=Authored format views and recovery islands
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT1P
owner=expansion.formatter:AuthoredFormatView and RecoveryIsland provenance
conflict_domains=doc,carrier_parser
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
charter=charters/expansion-formatter/FMT1A.md
max_production_loc=700
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT1A — Authored format views and recovery islands

## Independently acceptable outcome and rollback boundary

Define the neutral borrowed `AuthoredFormatView`/`RecoveryIsland` contract and one `CarrierBlockInventory` adapter without implementing any language printer or parser-specific formatting population. Reverting this node removes only that neutral contract and adapter; the FMT1 crate/renderer, parser artifacts, and all current formatting behavior remain unchanged.

The sole owner is **AuthoredFormatView and RecoveryIsland provenance**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_formatter/src/view.rs`, `crates/verter_formatter/src/recovery.rs`, and the bounded `CarrierBlockInventory` adapter under `crates/verter_language/src`.
- Owned boundaries: the framework-neutral `AuthoredFormatView` contract using FMT1P's `AuthoredFormatByteRange`, `AuthoredProvenance`, and `FormatProvenanceTable<R>`, deterministic same-revision provenance-ID assignment, authored trivia/order vocabulary, `RecoveryIsland`, and truthful unsupported-view outcomes.
- This node emits one `AuthoredProvenance { id, range }` entry per source-backed formatting unit. It consumes FMT1P's identity/carrier types unchanged and cannot declare a view-local token type.
- Language-specific JS/TS/JSX/TSX, CSS-family, HTML, Vue, and Svelte view populations remain with their printer nodes; this node owns only the common contract and neutral carrier-block adapter.
- Views borrow or reference existing parser/source artifacts and may not become a second semantic parser or own printer policy.
- Rendering is FMT1; edits are FMT1B; maps are FMT1C; range behavior is FMT1D; cursor behavior is FMT1E.

## Exact predecessor contract

- **FMT1P:** supplies the formatter crate, authored coordinate domain, and sole `FormatProvenanceId`/`AuthoredProvenance`/`FormatProvenanceTable<R>` carrier contract used by FMT1 and FMT1C.

## Acceptance IDs and discriminating evidence

- **FMT1A-AC1 — neutral contract completeness:** locked `CarrierBlockInventory` fixtures expose every admitted carrier block, authored span/trivia boundary, raw-text region, and sibling/order identity exactly once in authored order.
- **FMT1A-AC2 — recovery truthfulness:** malformed/unsupported syntax becomes source-backed recovery islands whose bytes and spans are preserved exactly; no regex/string reconstruction occurs.
- **FMT1A-AC3 — parser reuse:** structural/counter evidence proves views reuse the existing parser artifact and perform zero second parse or semantic analysis.
- **FMT1A-AC4 — stable identity:** repeated view construction over the same parse/source revision yields identical `FormatProvenanceId`→range bindings and traversal; duplicate, missing, or cross-revision bindings fail.

Test homes: `crates/verter_formatter/tests` and the owning parser/language fixtures.

## Deletions and forbidden designs

- The deletion population is explicitly empty. Discovery of a candidate private prototype requires a pre-mutation STOP and FMT0 amendment naming its exact path/symbol and sole owner; this node never conditionally absorbs it.
- Delete no parser, printer, formatter route, or public API.
- Forbid a second semantic parser, regex/string recovery, owned AST duplication, printer decisions inside views, fabricated spans for malformed source, or any provenance identity/carrier parallel to FMT1P.

## Budgets and mandatory rescope

- Target ceiling: 700 production LOC, 6 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or if rendering/edit/map/range/service/public work enters the diff.
- Correctness budget: zero authored-byte, trivia, order, span, or recovery loss.
- Performance budget: zero additional parse passes; view construction remains linear in visited authored syntax with bounded retained memory.

## Abort conditions

- Abort if `CarrierBlockInventory` cannot expose enough authored span/order/recovery information for the neutral contract without importing a language-printer policy.
- Abort rather than fabricate parity if a syntax view is too lossy.
- Abort if the design requires a formatter-owned semantic AST or reparsing source.

## Verification and review

1. Follow TDD for each missing trivia/recovery boundary.
2. Run focused language/parser fixtures and `cargo nextest run -p verter_formatter -p verter_language`.
3. Run every final command in `targeted-domain` on the review candidate.

Apply `architecture-3`. Add only FMT1A's trusted implementation-ledger row before review.
