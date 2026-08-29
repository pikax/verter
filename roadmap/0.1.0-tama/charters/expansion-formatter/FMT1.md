<!-- unified-charter-v2
id=FMT1
name=Document algebra and bounded renderer
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT1P
owner=expansion.formatter:Doc algebra and deterministic bounded renderer
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
charter=charters/expansion-formatter/FMT1.md
max_production_loc=700
max_production_files=7
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT1 — Document algebra and bounded renderer

## Independently acceptable outcome and rollback boundary

Land a compact framework-neutral `Doc` algebra and deterministic renderer with group/break/indent/line-suffix semantics and bounded adversarial behavior in the already-created `verter_formatter` crate. Reverting this node removes only the unused document renderer; the crate/coordinate foundation, authored syntax views, edits, maps, range/cursor behavior, printers, service routing, and public APIs remain untouched.

The sole owner is **Doc algebra and deterministic bounded renderer**. This node is a foundation, not a formatter-service or public cutover.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_formatter/src/doc.rs`, `crates/verter_formatter/src/render.rs`, and bounded exports in `crates/verter_formatter/src/lib.rs`.
- Owned boundaries: `Doc<FormatProvenanceId>`, group and break identifiers, indentation/alignment, conditional content, line suffixes, `RenderOptions`, and `RenderedDoc` segment behavior.
- Every source-backed rendered segment carries FMT1P's unchanged `FormatProvenanceId` through `Provenanced<RenderedSegment>`. This node cannot declare another token, assign authored ranges, reinterpret identity, or expose `FormatPositionMap`.
- No parser, carrier printer, edit derivation, range selection, cursor projection, session route, or public protocol belongs here.

## Exact predecessor contract

- **FMT1P:** supplies the formatter crate, typed authored/formatted byte domains, and the sole `FormatProvenanceId`/`Provenanced<T>` carrier contract.

## Acceptance IDs and discriminating evidence

- **FMT1-AC1 — algebra laws:** table-driven fixtures discriminate group flatten/break choice, nesting, indentation, conditional docs, hard/soft lines, and line-suffix ordering.
- **FMT1-AC2 — deterministic rendering:** identical `Doc` plus options yields byte-identical output and stable segment/provenance order; a planted token replacement or dropped source-backed ID fails.
- **FMT1-AC3 — bounded behavior:** adversarial deeply nested and wide docs complete within the locked work/allocation bounds; no quadratic group search or recursive stack overflow.
- **FMT1-AC4 — zero semantic work:** dependency and call-site evidence proves rendering cannot parse source, invoke semantic analysis, or route a formatting request.

Test home: `crates/verter_formatter/tests`.

## Deletions and forbidden designs

- The deletion population is explicitly empty. Discovery of a candidate private prototype requires a pre-mutation STOP and FMT0 amendment naming its exact path/symbol and sole owner; this node never conditionally absorbs it.
- Delete no current formatter route or public API; FMT3 owns the sole shared/LSP route cutover and deletion, adapter nodes own their exact boundary contributions, and FMT4 is proof-only promotion.
- Forbid semantic-AST pretty printing, unbounded backtracking, quadratic group search, parser callbacks, action-map reuse, and format-after-build string surgery.

## Budgets and mandatory rescope

- Target ceiling: 700 production LOC, 7 production files, 1 related crate/package.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or if authored-view/edit/map/range/service/public work enters the diff.
- Correctness budget: zero nondeterminism or group/line-suffix semantic ambiguity.
- Performance budget: meet the FMT0 locked renderer work/allocation/stack bounds; no unbounded fallback is permitted.

## Abort conditions

- Abort if the locked compatibility contract requires parser-specific branches in `Doc` or the renderer.
- Abort if bounded rendering cannot be proven without changing the FMT0 performance contract.
- Abort if provenance retention requires this node to own authored position-map semantics.

## Verification and review

1. Follow TDD for each algebra or adversarial boundary.
2. Run `cargo nextest run -p verter_formatter`.
3. Run every final command in `targeted-domain` on the review candidate.

Apply `architecture-3`. Add only FMT1's trusted implementation-ledger row before review.
