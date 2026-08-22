# TCM1 — Compact mapping products inside `CodeTransform`

**Status:** DRAFT, pending DAG amendment + authorization record.
**Class:** foundational. **Predecessor:** TCM0's ratified mapping contract.
**Downstream:** TCM2, TCM3, TCM4.
**This is the block the rest of the program is currently building on top of.**

## Why this one is urgent

`source_projection_map()` returns a JSON **string** at the assembly boundary
today — consumed as `PositionMapper::from_json(… .unwrap_or(""))`
(`crates/verter_tsc/src/checker.rs:411`,
`crates/verter_lsp/tests/cases/kebab_tag_mapping_full_columns.rs:65`). Semantic
projection is string-encoded. No correct TypeScript span-map adapter can be built
on that, and every consumer written against it meanwhile is written against a
shape that must change.

## Scope

Replace string-encoded semantic projection with a typed compact
`SourceProjectionMap`, recorded inside `CodeTransform`.

**Keep the four products distinct.** `PlacementMap` (source-unit placement and
composition), `SourceProjectionMap` (authored→generated semantic/IDE projection),
`RuntimeSourceMapData` (runtime/build geometry), `EncodedSourceMap` (terminal
external serialisation). They may share packed primitives, boundary validation,
sorting and coordinate conversion. There is no `UniversalProjectionMap`,
`OneMapForAllConsumers` or `MapperOwnedSourceMap`.

**`CodeTransform` stays the byte and geometry authority.** Projection facts
originate from the same write/edit/chunk operations that produce bytes. Statically
dispatched no-projection mode — `CodeTransform<NoProjection>` /
`CodeTransform<ProjectionRecorder>` or an equivalent sealed design.

Forbidden: rescanning generated output; decoding V3 mappings to recover semantics;
identifying aliases from generated spelling; inferring classes from equal lengths;
mapper-specific duplicate codegen; any second writer that can disagree with
`CodeTransform`.

**Geometry.** Generic relations `ExactCopy`, `Atom`, `IdentityAlias` — the
TypeScript terminal adapter maps these to `Verbatim` / `Atom` / `Alias`. **There
is no fourth `Anchor` relation**: a synthesised definition target is
`relation = Atom`, `original_len = 0`, `projection_class = DefinitionAnchor`.
Generated scaffolding is an unmapped generated gap, never fake authored ownership.

**Invariants:** generated spans ordered and non-overlapping; original overlap
permitted; one original range may project to many generated ranges; exact-copy
ranges textually and length identical; edit-producing operations require
length-preserving `ExactCopy`; zero-length original ranges require a named
semantic class; all range additions checked before narrowing.

**Compactness.** Packed contiguous records with sparse side tables. No `String`,
`Arc`, `Vec`, `Box`, `HashMap`, repeated bitset, repeated provenance struct or
allocated semantic object on any per-segment record. Local ownership during
one-shot compilation; `Arc` only at a proven shared publication boundary. Final
shape, tag layout, size budget and indexing chosen by TCM1 measurement.

**Zero work when unrequested.** The no-projection route must have no per-write
`maps_enabled` branch, no projection allocation or side table, no UTF-16
conversion, no feature-policy work, no V3 serialisation, no mapper JSON. Prove it
with allocation counters, invocation counters, generated/IR inspection, structure
sizes, equivalent-work comparison, and a test that FAILS if projection recording
is invoked under `NoProjection`.

**Offsets.** Canonical UTF-8 internally unless TCM1 evidence shows better.
Validate `usize`→`u32`, `u32` limits, UTF-8 boundaries, ordering, additions before
narrowing, non-overlap, UTF-16 conversion, and TypeScript's signed `int32` wire
range `0..=i32::MAX`. Overflow is a typed error before serialisation — never
wrap, saturate, truncate or reinterpret.

Position encoding and terminal feature policy are terminal state and never enter
the compiler-artifact cache key.

## Non-scope

No mapper process. No TypeScript JSON-RPC types in compiler core. No semantic API
client. No TypeScript feature-mask constants in framework codegen. No activation.

## Acceptance

Property, differential, mutation, composition and concurrency tests; proof that no
second semantic geometry authority remains; proof of zero projection-product
allocation when unrequested; atomic publication of bytes and mapping products
preserved.
