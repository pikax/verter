<!-- unified-charter-v2
id=CCA1H
name=Vue ProjectionBackend implementation
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1G
owner=compiler.compiler-bridge:Vue compile_ide ProjectionBackend
conflict_domains=compiler_execution,mapping_geometry,vue_product
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
charter=charters/compiler-compiler-bridge/CCA1H.md
max_production_loc=600
max_production_files=5
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1H — Vue ProjectionBackend implementation

## Independently acceptable outcome and rollback boundary

Implement the Vue `ProjectionBackend` over the canonical `standalone.rs` parsed compiler core with an IDE-only `CompileRequest`, without moving session consumers. Reverting removes only the unused Vue projection adapter/catalog row.

## Concrete surfaces and APIs

- Surfaces: `crates/verter_compiler/src/standalone.rs`, `framework_common/vue_bridge.rs`, and Vue IDE projection helpers.
- Owns only Vue IDE/checkable bytes, maps, diagnostics, language/provenance, and typed projection request identity.
- For supplied block content, the IDE-only request may consume the canonical prerequisite planner's runtime-template chunk; it must not call `RuntimeCompilerBackend`, plan a runtime product, or publish runtime output.
- Does not own `registered_carrier_projection.rs`, parse artifacts, template facts, runtime products, or session routing.

## Exact predecessor contract

- **CCA1G:** implemented ledger row for “Framework semantic consumer convergence”.

## Acceptance and evidence

- Vue `compile_ide` corpora preserve bytes, maps, diagnostics, provenance, and deterministic output.
- Projection consumes existing parse/semantic inputs without reparsing; any Vue template-chunk prerequisite is computed once inside the canonical parsed core and is not a runtime product.
- Structural evidence proves production callers remain on the old projection route in this node.

## Deletions, budgets, and aborts

- Delete no consumer route; forbid template-fact, runtime, host, or publication work.
- Ceiling: 600 LOC, 5 files, 1 crate; abort/rescope on route migration or output-schema work.

## Verification and review

Run Vue IDE/map tests, `cargo nextest run -p verter_compiler`, and `targeted-domain`. Apply `architecture-3`; add only CCA1H's ledger row.
