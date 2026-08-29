<!-- unified-charter-v2
id=CCA1M2
name=Host-backed bundle runtime delegation
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1K,CCA1L
owner=compiler.compiler-bridge:compile_entry temporary bundle runtime delegation
conflict_domains=compiler_execution,host_service_graph
resource_class=rust-mixed
review_profile=public-3
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
charter=charters/compiler-compiler-bridge/CCA1M2.md
max_production_loc=500
max_production_files=4
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1M2 — Host-backed bundle runtime delegation

## Independently acceptable outcome and owners

The generic host-backed `compile_entry` transaction keeps its outer `CarrierCompiler::compile_bundle` call but that compatibility implementation delegates runtime construction to `RuntimeCompilerBackend`. Current outer selector ownership remains session-owned; CCA1N3 alone will replace it. Reverting restores only this temporary adapter's internal delegation.

## Concrete surfaces and boundary

- Surfaces are `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs::compile_entry`, `crates/verter_session/src/host_resolve/compile_request_build.rs`, and the Vue/Svelte bundle compatibility implementations in `crates/verter_compiler/src/framework_common/vue_bridge.rs` and `crates/verter_compiler/src/svelte/carrier.rs`.
- Preserve the outer registry lookup, one `compile_bundle` call, atomic multi-product refusal, diagnostics, assembly handoff, and publication transaction exactly.
- Direct/batch routes and runtime-render are excluded.

## Exact predecessor contracts

- **CCA1K/CCA1L:** both framework runtime backends accept the typed runtime requests used by this adapter.

## Acceptance, budget, and verification

Prove runtime bytes/maps/diagnostics and equivalent-work counters are unchanged and exactly the host-backed `compile_entry` outer bundle call remains. Delete only the displaced internal runtime construction branch. Ceiling: 500 LOC, 4 files, 2 crates; abort on selector, lifecycle, or publication mutation. Run host-backed virtual/batch suites and `targeted-domain`.
