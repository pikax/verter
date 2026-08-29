<!-- unified-charter-v2
id=FMT1P
name=Formatter crate and typed byte-domain foundation
phase=expansion
train=expansion.formatter
product=formatter
kind=implementation
semantic_role=delivery
class=successor
predecessors=FMT0
owner=expansion.formatter:formatter crate scaffold, typed byte domains, and shared provenance identity
conflict_domains=doc,mapping_geometry
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
charter=charters/expansion-formatter/FMT1P.md
max_production_loc=350
max_production_files=5
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# FMT1P — Formatter crate and typed byte-domain foundation

## Independently acceptable outcome and rollback boundary

Create and register the otherwise unused `verter_formatter` crate; define its four private UTF-8 byte coordinate domains plus the stable private provenance identity/carrier contract shared by authored views and rendered segments. Reverting removes only the unused crate scaffold, coordinate types, and provenance types; no renderer, parser, edit, map, formatter route, or public API changes.

## Concrete surfaces and APIs

- Surfaces: workspace registration, `crates/verter_formatter/Cargo.toml`, `src/lib.rs`, `src/coordinates.rs`, and `src/provenance.rs`.
- Owns `AuthoredFormatByteOffset`/`AuthoredFormatByteRange` for complete authored carrier source and `FormattedByteOffset`/`FormattedByteRange` for rendered formatter output.
- Owns non-serializable `FormatProvenanceId`, `AuthoredProvenance { id, range }`, generic `Provenanced<T> { value, provenance }`, and `FormatProvenanceTable<R> { source_revision, entries }`. An ID is scoped to one table/source revision, identifies one source-backed authored unit, and is the only join key accepted by FMT1C; `R` is the caller's stable source-revision identity rather than a formatter-global ID.
- `AuthoredProvenance` binds an ID to exactly one authored range. Duplicate IDs with different ranges, unknown rendered IDs, or cross-revision reuse are invalid; reconstruction of the same authored view revision must assign the same IDs deterministically.
- The foundation exposes `Provenanced<RenderedSegment>` and `FormatProvenanceTable<R>` as the sole carrier shapes for later renderer and authored-view nodes. Foundation-local type fixtures instantiate synthetic producer and consumer roles against those same definitions; no successor implementation or call site is required for this node to land.
- The domains have checked construction/range invariants, no cross-domain `From` conversion, no bare integer public parameters, no TSX-specific `Generated*` reuse, and no wire serialization.
- Coordinate and provenance wrappers implement no parser, printer, map, range/cursor, route, or protocol policy. Rendering, authored views, edit derivation, map construction/query, language printers, routing, and public conversion are excluded.

## Exact predecessor contract

- **FMT0:** implemented ledger row for “Full formatter implementation lock”.

## Acceptance and evidence

- Compile-time/type tests prove authored and formatted domains cannot be interchanged or implicitly converted.
- Checked ranges reject inverted/out-of-bounds construction and preserve exact UTF-8 byte identities at Unicode/CRLF/EOF boundaries.
- Compile-time foundation tests prove synthetic producer and consumer roles must share the same `FormatProvenanceId`/`Provenanced<T>`/`FormatProvenanceTable<R>` definitions and cannot substitute a bare integer or separately declared token. FMT1 and FMT1A own the later evidence that their real implementations consume this contract.
- Table-driven identity tests reject duplicate/mismatched authored bindings, unknown IDs, and cross-revision reuse while preserving deterministic same-revision reconstruction.
- Dependency inspection proves the empty foundation cannot parse, render, format, or serialize a request.

## Deletions, budgets, and aborts

- Delete no production formatter route or public API.
- Ceiling: 350 LOC, 5 files, 1 crate, including workspace registration and the crate manifest; rescope if rendering, authored-view assignment policy, map policy, protocol conversion, or another package enters.
- Abort if any coordinate/provenance type needs protocol encoding, parser ownership, syntax semantics, or global/cross-request identity.

## Verification and review

Use TDD for domain separation, range invariants, and provenance identity/carrier discrimination; run `cargo nextest run -p verter_formatter` and `targeted-domain`. Apply `architecture-3`; add only FMT1P's ledger row.
