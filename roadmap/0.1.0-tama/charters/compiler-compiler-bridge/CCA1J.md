<!-- unified-charter-v2
id=CCA1J
name=IDE projection route convergence
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1H,CCA1I
owner=compiler.compiler-bridge:registered IDE projection delegation behind the temporary bundle adapter
conflict_domains=compiler_execution,mapping_geometry,host_service_graph
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
charter=charters/compiler-compiler-bridge/CCA1J.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1J — IDE projection route convergence

## Independently acceptable outcome and rollback boundary

Route registered Vue/Svelte IDE/checkable projection calls through `ProjectionBackend`, including delegation from the temporary combined bundle adapter, while leaving the generic host multi-product transaction intact for CCA1N3. Reverting restores that projection delegation while both backend implementations remain intact.

## Concrete surfaces and APIs

- Surfaces: `framework_common/registered_carrier_projection.rs`, the combined carrier adapter's IDE delegation, and IDE/checkable consumers outside the generic `compile_entry` transaction.
- Owns catalog lookup, typed projection delegation, and deletion of direct combined `compile_ide` calls in that population.
- `host_resolve/virtual_file_pipeline.rs::compile_entry` remains on its temporary atomic bundle adapter until CCA1N3; this node does not split its request, refusal, diagnostics, or publication transaction.
- Does not own parse publication, template facts, runtime compilation, or host/bundler lifecycle.

## Exact predecessor contracts

- **CCA1H:** implemented ledger row for “Vue ProjectionBackend implementation”.
- **CCA1I:** implemented ledger row for “Svelte ProjectionBackend implementation”.

## Acceptance and evidence

- All IDE/checkable consumers outside the explicitly retained host bundle adapter use one catalog-selected backend; the adapter delegates its IDE product to the same backend.
- Fresh/persisted/incremental outputs preserve bytes, maps, diagnostics, language, provenance, and ordering.
- Projection adds no parse, semantic, source-copy, or duplicate projection pass; rejected work cannot warm.

## Deletions, budgets, and aborts

- Delete only displaced projection route methods/adapters; retain the named host bundle adapter for CCA1N3.
- Ceiling: 800 LOC, 8 files, 2 crates; rescope if runtime or public output schema enters.
- Abort on unlisted consumer or unexplained output/map/cache divergence.

## Verification and review

Use TDD at route boundaries, run compiler/session IDE suites and `targeted-domain`. Apply `architecture-3`; add only CCA1J's ledger row.
