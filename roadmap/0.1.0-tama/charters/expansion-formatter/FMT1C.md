<!-- unified-charter-v2
id=FMT1C
name=Authored-to-formatted position-map authority
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT1,FMT1A,FMT1B
owner=expansion.formatter:FormatPositionMap authored-to-formatted authority
conflict_domains=mapping_geometry
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
charter=charters/expansion-formatter/FMT1C.md
max_production_loc=600
max_production_files=5
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT1C — Authored-to-formatted position-map authority

## Independently acceptable outcome and rollback boundary

Install one private `FormatPositionMap` authority that composes authored-view provenance, rendered segments, and minimal edits. Reverting this node removes only map construction/query support; the renderer, authored views, and edit derivation remain independently valid.

The sole owner is **FormatPositionMap authored-to-formatted authority**. FMT4P owns shared `Span` conversion, FMT4L/FMT4F own boundary encoding, and FMT4 is proof-only promotion.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_formatter/src/position_map.rs`, `crates/verter_formatter/src/lib.rs`, and shared span/encoding helpers already owned by `crates/verter_span`.
- Owned boundaries: `FormatPositionMap`, authored-to-formatted and formatted-to-authored queries over FMT1P's coordinate types, retained/inserted/deleted classifications, map-query boundary mechanics, and composition with `FormatEditSet`.
- The four coordinate wrappers are inherited unchanged from FMT1P; this node cannot retrofit or reinterpret their domains.
- FMT1C defines how an explicit low/high boundary bias is executed by a map query. FMT1E alone chooses cursor affinity policy and passes that choice into this mechanism.
- Inputs are FMT1 `Provenanced<RenderedSegment>` values plus render source revision, FMT1A's `FormatProvenanceTable<R>`/recovery spans, and FMT1B edit geometry. Construction joins only on FMT1P's `FormatProvenanceId` after exact source-revision equality and fails on unknown, duplicate, mismatched-revision, or multiply-bound IDs.
- Range/cursor policy and any Rust/NAPI/WASM/LSP/MCP DTO are excluded.

## Exact predecessor contracts

- **FMT1:** supplies rendered segments carrying FMT1P provenance IDs unchanged.
- **FMT1A:** supplies deterministic authored provenance-ID→range bindings and recovery spans using the same FMT1P types.
- **FMT1B:** implemented ledger row for “Minimal non-overlapping edit geometry”.

## Acceptance IDs and discriminating evidence

- **FMT1C-AC1 — exact retained mapping:** every retained authored position maps to the corresponding formatted position through the shared provenance ID and round-trips under the locked bias rules; separately declared or unknown tokens fail construction.
- **FMT1C-AC2 — explicit changed geometry:** inserted and deleted regions are represented explicitly; queries at edit boundaries, Unicode code points, CRLF, and EOF are deterministic and in bounds.
- **FMT1C-AC3 — recovery preservation:** source-backed recovery islands map without byte loss or fabricated semantic correspondence.
- **FMT1C-AC4 — bounded map work:** construction and query meet FMT0 large-file/allocation bounds; the map reuses render/edit provenance and does not rescan or reparse syntax.

Test homes: `crates/verter_formatter/tests` and focused `verter_span` encoding fixtures where needed.

## Deletions and forbidden designs

- The deletion population is explicitly empty. Discovery of a candidate private prototype requires a pre-mutation STOP and FMT0 amendment naming its exact path/symbol and sole owner; this node never conditionally absorbs it.
- Delete no current public/action/source-map authority and no formatter route.
- Forbid action-map reuse, bare/unqualified integer offsets, TSX `Generated*` wrappers, implicit cross-domain conversion, inferred correspondence across deleted/replaced bytes, reparsing, and public wire schema changes.

## Budgets and mandatory rescope

- Target ceiling: 600 production LOC, 5 production files, 2 related crates/packages.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or if range/cursor/service/public work enters the diff.
- Correctness budget: zero offset-encoding ambiguity, out-of-bounds mapping, provenance loss, or false round-trip claim.
- Performance budget: linear/bounded map construction and bounded query behavior under FMT0 thresholds; no duplicate syntax traversal.

## Abort conditions

- Abort if the unchanged FMT1P provenance contract cannot join authored/rendered evidence or distinguish retained, inserted, and deleted geometry exactly; do not retrofit FMT1/FMT1A types here.
- Abort if a carrier requires a second map authority rather than contributing authored-view provenance.
- Abort if exact behavior requires changing public protocol before FMT4P or adapter-specific conversion before FMT4L/FMT4F.

## Verification and review

1. Follow TDD for each mapping/encoding boundary.
2. Run `cargo nextest run -p verter_formatter -p verter_span` with round-trip, recovery, Unicode, and edit-composition cases.
3. Run every final command in `targeted-domain` on the review candidate.

Apply `architecture-3`. Add only FMT1C's trusted implementation-ledger row before review.
