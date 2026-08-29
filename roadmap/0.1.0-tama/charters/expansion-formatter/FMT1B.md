<!-- unified-charter-v2
id=FMT1B
name=Minimal non-overlapping edit geometry
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT1
owner=expansion.formatter:FormatEditSet and minimal edit derivation
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
charter=charters/expansion-formatter/FMT1B.md
max_production_loc=500
max_production_files=4
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT1B — Minimal non-overlapping edit geometry

## Independently acceptable outcome and rollback boundary

Derive deterministic, minimal non-overlapping text edits from authored bytes and a rendered result. Reverting this node removes only private edit derivation; `Doc` rendering remains usable and no formatter service/public route has changed.

The sole owner is **FormatEditSet and minimal edit derivation**.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_formatter/src/edit.rs`, `crates/verter_formatter/src/lib.rs`.
- Owned boundaries: `FormatEdit`/`FormatEditSet` using FMT1P's authored ranges, edit normalization/coalescing, unchanged prefix/suffix handling, and deterministic application order.
- Inputs are authored bytes plus rendered bytes/segments. Authored syntax selection, position maps, range expansion, cursor projection, and public DTOs are excluded.

## Exact predecessor contract

- **FMT1:** implemented ledger row for “Document algebra and bounded renderer”; its FMT1P ancestry supplies the typed authored/formatted coordinate domains.

## Acceptance IDs and discriminating evidence

- **FMT1B-AC1 — exact application:** applying the emitted edit set to authored bytes yields the rendered bytes exactly for table-driven insert/delete/replace/Unicode/newline cases.
- **FMT1B-AC2 — non-overlap and order:** edits are sorted, non-overlapping, in bounds, deterministic, and contain no no-op replacements.
- **FMT1B-AC3 — minimal boundary:** unchanged prefix/suffix and internal stable regions are excluded; whole-file replacement is rejected whenever a smaller edit set is proven by the locked algorithm.
- **FMT1B-AC4 — bounded derivation:** adversarial source/result pairs meet FMT0 work/allocation bounds and do not use unbounded quadratic diff fallback.

Test home: `crates/verter_formatter/tests`.

## Deletions and forbidden designs

- The deletion population is explicitly empty. Discovery of a candidate private prototype requires a pre-mutation STOP and FMT0 amendment naming its exact path/symbol and sole owner; this node never conditionally absorbs it.
- Delete no current formatter dispatcher, carrier route, or public formatter API.
- Forbid whole-file replacement as a convenience fallback, overlapping edits, byte/UTF offset confusion, action-map reuse, and unbounded general-purpose diff search.

## Budgets and mandatory rescope

- Target ceiling: 500 production LOC, 4 production files, 1 related crate/package.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or if syntax-view/map/range/service/public work enters the diff.
- Correctness budget: zero malformed edit sets, offset aliasing, or rendered-byte mismatch.
- Performance budget: edit derivation obeys the FMT0 locked large-file and allocation bounds; no quadratic fallback.

## Abort conditions

- Abort if the FMT0 lock does not define enough minimality to choose one deterministic edit set.
- Abort if exact Unicode/line-ending geometry requires public protocol changes in this node.
- Abort on an adversarial case that forces unbounded search or whole-file fallback contrary to the lock.

## Verification and review

1. Follow TDD for each edit-geometry regression boundary.
2. Run `cargo nextest run -p verter_formatter` including adversarial geometry cases.
3. Run every final command in `targeted-domain` on the review candidate.

Apply `architecture-3`. Add only FMT1B's trusted implementation-ledger row before review.
