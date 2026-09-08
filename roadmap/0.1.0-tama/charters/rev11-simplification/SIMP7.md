<!-- unified-charter-v2
id=SIMP7
name=Reuse provider coordinate conversion state
predecessors=A6
phase=rev11
train=rev11.simplification
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.simplification:existing provider document snapshot and codec
conflict_domains=provider_lifecycle,source_lineage
resource_class=rust-mixed
gate_profile=targeted-domain
review_profile=semantic-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=medium
review_effort_default=medium
verification_effort_min=medium
verification_effort_default=high
confirmation_effort_min=medium
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-simplification/SIMP7.md
size=M
max_production_loc=450
max_production_files=4
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP7 — Reuse provider coordinate conversion state

## Independently acceptable outcome and owner

Coordinate conversion reuses state for the exact source snapshot or batch with fewer full-source scans and copies and unchanged encoding/range behavior.

Current problem: Diagnostic conversion builds and copies a full LineIndex per endpoint, potentially rebuilding the same source index 2D times for D diagnostics.

Final owner: **existing provider document snapshot and codec**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **A6:** ratified current architecture and proportionate evidence baseline, already implemented.

## Concrete surfaces and implementation

- `crates/verter_type_runtime/src/codec.rs`
- `crates/verter_type_runtime/src/tsgo/ipc.rs`
- `crates/verter_type_runtime/src/tsserver/ipc.rs`

Bind LineIndex::new, position_to_offset_with_encoding, strict endpoint conversion and parse_lsp_diagnostic. Reuse an index per immutable source version or response batch through the existing document owner; borrow/share existing bytes as appropriate. Share only genuinely identical strict conversion logic, preserving different LSP/tsserver conventions.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No wire change, global cache, lifecycle migration, compiler mapping, native/unplugin or lockfile mutation.

## Acceptance and proportionate proof

- **SIMP7-AC1:** Identical source indexing is reused across response endpoints.
- **SIMP7-AC2:** Negotiated encodings, strict rejection and source-version/target identity remain correct.
- **SIMP7-AC3:** Source scans/copies decrease without another cache or unbounded retention.

Existing evidence to retain or extend: Codec/provider cases for UTF-8/UTF-16, surrogate boundaries, invalid edits, changed sources and target files; equivalent diagnostic batches measuring index builds and copied bytes.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

Focused codec/provider tests, representative equivalent-work batches, node scripts/gate.mjs.

Apply semantic-3 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
