<!-- unified-charter-v2
id=CCA2A
name=Compile artifact schema and map qualification
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1,B4
owner=compiler.compiler-bridge:CompileArtifactSet schema identities relations provenance and qualified maps
conflict_domains=compiler_execution,source_lineage
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
charter=charters/compiler-compiler-bridge/CCA2A.md
max_production_loc=700
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA2A — Compile artifact schema and map qualification

## Independently acceptable outcome and owners

Define `CompileArtifactSet` as a stable compiler-owned schema with artifact identity, role, language, typed relations, provenance, qualified source-map families, and deterministic terminal serialization. Current durable output is split among `VerterCompileResult` and assembly `ArtifactSet`; final schema ownership is this node. Reverting removes the unused schema without moving any route.

## Concrete surfaces and boundary

- Production surfaces are `crates/verter_compiler/src/assembly/mod.rs`, `fragment.rs`, `source_space.rs`, `source_unit.rs`, `publish.rs`, and bounded public exports/types in `crates/verter_compiler/src/compile/types.rs` or `lib.rs`.
- Every artifact has canonical identity, product role, language, source-unit relation, provenance/input basis, content availability, and map family qualified by source/generated spaces.
- Rust stores SFC-absolute byte `Span`; serialization adapters remain responsible for UTF-16. No relative/generated-only span serializes as source geometry.
- Assembly migration, host publication, style continuation, custom blocks, and C2 facade integration are excluded.

## Exact predecessor contracts

- **CCA1:** typed compiler capabilities and all legacy combined authority are terminally converged.
- **B4:** logical source-unit mapping composition and atomic publication identity are available for qualified artifact relations.

## Acceptance, budget, and verification

Type/schema fixtures prove stable identity, relation completeness, deterministic order, map qualification, provenance, and fail-closed rejection of aliasing/unqualified maps; creating the schema performs zero compile work. Delete nothing. Ceiling: 700 LOC, 6 files, 2 crates; abort on route mutation or a seventh production surface. Run compiler assembly/map/type suites and `targeted-domain`. CCA2B, CCA2D, and CCA2E consume the schema.
