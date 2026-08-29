<!-- unified-charter-v2
id=FMT1E
name=Cursor projection and bias geometry
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT1C
owner=expansion.formatter:CursorMap projection and bias geometry
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
charter=charters/expansion-formatter/FMT1E.md
max_production_loc=350
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT1E — Cursor projection and bias geometry

## Independently acceptable outcome and rollback boundary

Project one authored cursor through `FormatPositionMap` and `FormatEditSet` under a deterministic bias/affinity contract. Reverting this node removes only private cursor projection; rendering, authored views, edits, maps, and range selection remain independently valid.

The sole owner is **CursorMap projection and bias geometry**. FMT4P owns cursor-free public DTOs plus the non-Serde session-private cursor request/result carriers; FMT4F alone constructs a cursor-bearing private request after strict UTF-16 conversion and owns NAPI/WASM boundary cursor conversion. FMT4 is proof-only promotion. LSP, Rust, and MCP publish no formatter cursor request or result.

## Concrete surfaces and APIs

- Production surfaces: `crates/verter_formatter/src/cursor.rs`, `crates/verter_formatter/src/lib.rs`.
- Owned boundaries: private `CursorMap`, the policy that chooses cursor affinity/bias, and projection through retained, inserted, deleted, recovery, Unicode, CRLF, and EOF geometry.
- FMT1E never redefines map boundary mechanics: it selects an affinity and invokes FMT1C's explicit biased query over typed authored/formatted byte domains.
- Inputs are the edit and position-map authorities converged by FMT1C; this node owns no authored range-selection policy.

## Exact predecessor contract

- **FMT1C:** implemented ledger row for “Authored-to-formatted position-map authority”.

## Acceptance IDs and discriminating evidence

- **FMT1E-AC1 — retained projection:** cursors before, inside, and after retained regions round-trip under one documented affinity rule.
- **FMT1E-AC2 — changed geometry:** inserted/deleted/replaced regions, edit boundaries, Unicode, CRLF, and EOF produce deterministic in-bounds positions.
- **FMT1E-AC3 — recovery truth:** recovery islands preserve source-backed cursor correspondence without fabricated semantic anchors.
- **FMT1E-AC4 — bounded query:** projection uses existing map/edit indexes, performs no parse or semantic work, and meets FMT0 query bounds.

Test home: `crates/verter_formatter/tests`.

## Deletions and forbidden designs

- The deletion population is explicitly empty. Discovery of a candidate private prototype requires a pre-mutation STOP and FMT0 amendment naming its exact path/symbol and sole owner; this node never conditionally absorbs it.
- Delete no range policy, formatter route, public DTO, or LSP capability.
- Forbid byte/UTF offset confusion, action-map reuse, whole-document recomputation, inferred semantic anchors across deleted bytes, and public capability advertisement.

## Budgets and mandatory rescope

- Target ceiling: 350 production LOC, 3 production files, 1 related crate/package.
- Mandatory rescope above 1,500 production LOC, 12 files, 3 unrelated crates/packages, or if range/service/public work enters the diff.
- Correctness budget: zero cursor loss, out-of-bounds projection, or bias ambiguity.
- Performance budget: bounded indexed queries with zero parse, semantic, or whole-source scan work.

## Abort conditions

- Abort if FMT0 does not define one cursor-bias answer for an admitted changed-geometry cell.
- Abort if exact projection requires a second map authority, public protocol work before FMT4P, or boundary cursor conversion before FMT4F. Any LSP/Rust/MCP cursor result requires a separately ratified public-capability node.
- Abort on unexplained offset, recovery, or affinity divergence.

## Verification and review

1. Follow TDD for each cursor-geometry boundary.
2. Run `cargo nextest run -p verter_formatter` with retained/changed/recovery/Unicode/CRLF/EOF cases.
3. Run every final command in `targeted-domain` on the review candidate.

Apply `architecture-3`. Add only FMT1E's trusted implementation-ledger row before review.
