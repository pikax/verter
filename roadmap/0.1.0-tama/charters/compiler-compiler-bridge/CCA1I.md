<!-- unified-charter-v2
id=CCA1I
name=Svelte ProjectionBackend implementation
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1G
owner=compiler.compiler-bridge:Svelte compile_ide ProjectionBackend
conflict_domains=compiler_execution,mapping_geometry,svelte_product
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
charter=charters/compiler-compiler-bridge/CCA1I.md
max_production_loc=600
max_production_files=5
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1I — Svelte ProjectionBackend implementation

## Independently acceptable outcome and rollback boundary

Implement the Svelte `ProjectionBackend` over the canonical `standalone.rs` parsed compiler core with an IDE-only `CompileRequest`, without moving session consumers. Reverting removes only the unused Svelte projection adapter/catalog row.

## Concrete surfaces and APIs

- Surfaces: `crates/verter_compiler/src/standalone.rs`, `crates/verter_compiler/src/svelte/carrier.rs`, and Svelte IDE projection helpers.
- Owns only Svelte IDE/checkable bytes, maps, diagnostics, language/provenance, and typed projection request identity.
- Does not own registered parse artifacts, template facts, runtime bundles, or session routing.

## Exact predecessor contract

- **CCA1G:** implemented ledger row for “Framework semantic consumer convergence”.

## Acceptance and evidence

- Svelte `compile_ide` corpora preserve bytes, maps, diagnostics, provenance, and deterministic output.
- Projection consumes existing parse/semantic inputs without reparsing or runtime compilation.
- Structural evidence proves production callers remain on the old projection route in this node.

## Deletions, budgets, and aborts

- Delete no consumer route; forbid template-fact, runtime, host, or publication work.
- Ceiling: 600 LOC, 5 files, 1 crate; abort/rescope on route migration or output-schema work.

## Verification and review

Run Svelte IDE/map tests, `cargo nextest run -p verter_compiler`, and `targeted-domain`. Apply `architecture-3`; add only CCA1I's ledger row.
