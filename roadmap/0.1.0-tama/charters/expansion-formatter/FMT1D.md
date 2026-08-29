<!-- unified-charter-v2
id=FMT1D
name=Authored range expansion and edit containment
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT1C
owner=expansion.formatter:FormatSelection and range-relative edit containment
conflict_domains=mapping_geometry,doc
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
charter=charters/expansion-formatter/FMT1D.md
max_production_loc=500
max_production_files=4
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT1D — Authored range expansion and edit containment

## Independently acceptable outcome and rollback boundary

Define safe format-range expansion over authored views and exact range-relative edit containment. Reverting this node removes only private range-selection policy; full-document rendering, views, edits, maps, and cursor projection remain independently valid.

The sole owner is **FormatSelection and range-relative edit containment**. Cursor projection is FMT1E; FMT4P owns public `Span` DTO geometry, FMT4F owns NAPI/WASM range conversion, FMT4L owns full-document LSP edit conversion only, and FMT4 is proof-only promotion.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_formatter/src/range.rs`, `crates/verter_formatter/src/lib.rs`.
- Owned boundaries: `FormatSelection`, safe authored-unit expansion, and range-relative edit filtering/containment.
- Inputs are the authored boundaries, edits, and position maps converged by FMT1C.
- No printer, carrier composition, service router, public DTO, or LSP capability belongs here.

## Exact predecessor contracts

- **FMT1C:** implemented ledger row for “Authored-to-formatted position-map authority”.

## Acceptance IDs and discriminating evidence

- **FMT1D-AC1 — safe range expansion:** partial selections expand only to the smallest locked authored formatting units and never split recovery islands, raw-text regions, tokens, or required delimiter pairs.
- **FMT1D-AC2 — edit containment:** range formatting returns sorted non-overlapping edits that apply exactly and do not mutate bytes outside the expanded selection except locked boundary whitespace.
- **FMT1D-AC3 — recovery containment:** a selected malformed/recovery island is either retained whole or rejected according to the locked policy; no range fabricates syntax or correspondence.
- **FMT1D-AC4 — full/range consistency:** formatting an already safely expanded range agrees with the corresponding slice of full formatting and remains bounded under FMT0 thresholds.

Test home: `crates/verter_formatter/tests`.

## Deletions and forbidden designs

- The deletion population is explicitly empty. Discovery of a candidate private prototype requires a pre-mutation STOP and FMT0 amendment naming its exact path/symbol and sole owner; this node never conditionally absorbs it.
- Delete no current formatter route, public DTO, or LSP capability.
- Forbid byte/UTF offset confusion, range expansion through malformed islands, whole-file fallback for a supported bounded range, cursor policy, action-map reuse, and public capability advertisement.

## Budgets and mandatory rescope

- Target ceiling: 500 production LOC, 4 production files, 1 related crate/package.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or if printer/service/public work enters the diff.
- Correctness budget: zero out-of-range edit, unsafe syntax split, or full/range inconsistency.
- Performance budget: selection queries are bounded by touched authored units/edits and perform zero parse or semantic work.

## Abort conditions

- Abort if FMT0 does not specify one safe expansion answer for an admitted cell.
- Abort if exact range behavior requires a carrier-specific branch in the shared geometry owner rather than an authored-view contribution.
- Abort if implementation requires public protocol changes before FMT4P or FFI range conversion before FMT4F. LSP range capability/handler work requires a separately ratified node and cannot enter FMT4L or FMT3.

## Verification and review

1. Follow TDD for each range-selection and edit-containment boundary.
2. Run `cargo nextest run -p verter_formatter` with full/range, recovery, Unicode, and adversarial cases.
3. Run every final command in `targeted-domain` on the review candidate.

Apply `architecture-3`. Add only FMT1D's trusted implementation-ledger row before review.
