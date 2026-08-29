<!-- unified-charter-v2
id=CCA1B
name=Vue CarrierFrontend backend
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1A
owner=compiler.compiler-bridge:Vue CarrierFrontend implementation
conflict_domains=carrier_parser,vue_product
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
charter=charters/compiler-compiler-bridge/CCA1B.md
max_production_loc=500
max_production_files=5
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1B — Vue CarrierFrontend backend

## Independently acceptable outcome and rollback boundary

Implement the Vue `CarrierFrontend` registration against the existing Vue parser and registered parse-artifact constructor without changing any production consumer route. Reverting this node removes only the unused Vue catalog row/adapter; Svelte and all current parse/publication behavior remain unchanged.

## Concrete surfaces and APIs

- Surfaces: `crates/verter_compiler/src/framework_common/vue_bridge.rs`, the Vue-owned adapter beside `registered_carrier_projection.rs`, and catalog registration.
- Owns only Vue `CarrierFrontend::parse`, typed rejection, adapter identity, and conversion to the existing `FrameworkParseArtifact`.
- Does not own publication, session routing, `eval_source`, template facts, `compile_ide`, or runtime compilation.

## Exact predecessor contract

- **CCA1A:** implemented ledger row for “Typed compiler capability catalog”.

## Acceptance and evidence

- Vue parser/recovery/artifact fixtures are byte-, span-, diagnostic-, identity-, and provenance-equivalent through the new backend.
- The backend reuses the current registered parse constructor and performs no second parse or publication.
- Structural evidence proves no production caller routes through the new row yet.

## Deletions, budgets, and aborts

- Delete no current consumer route or combined-registry method.
- Forbid a second Vue parser, session imports, fallback, or projection/runtime work.
- Ceiling: 500 production LOC, 5 files, 1 crate; rescope if publication or another capability enters.
- Abort on parser/artifact divergence or if Vue cannot register without framework-private types leaking into the catalog.

## Verification and review

Run focused Vue parse/artifact tests, `cargo nextest run -p verter_compiler`, and `targeted-domain`. Apply `architecture-3`; add only CCA1B's ledger row.
