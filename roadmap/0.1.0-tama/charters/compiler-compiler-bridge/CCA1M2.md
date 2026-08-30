<!-- unified-charter-v2
id=CCA1M2
name=Carrier bundle runtime-backend delegation
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1K,CCA1L
owner=compiler.compiler-bridge:sole backend-internal compatibility runtime delegation for both carrier bundle implementations
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

# CCA1M2 — Carrier bundle runtime-backend delegation

## Independently acceptable outcome and owners

Both compatibility `CarrierCompiler::compile_bundle` implementations — Vue in `framework_common/vue_bridge.rs` and Svelte in `svelte/carrier.rs` — delegate their internal runtime construction to their framework's typed `RuntimeCompilerBackend`. This node is the SOLE owner of deleting the legacy runtime construction inside both compatibility implementations; no other node may delete, re-own, or reimplement those internal runtime branches. Current ownership is the two combined-trait internal runtime branches; final ownership is the framework runtime backends. Reverting restores only the two internal delegation branches.

## Concrete surfaces and boundary

- Surfaces are the `want_runtime` runtime-construction branch inside `VueCarrierCompiler::compile_bundle` in `crates/verter_compiler/src/framework_common/vue_bridge.rs`, the runtime-construction branch inside `SvelteCarrierCompiler::compile_bundle` in `crates/verter_compiler/src/svelte/carrier.rs`, and bounded imports/exports in their module files.
- Each compatibility implementation delegates to the typed runtime backend of its own framework (`VueRuntimeBackend`, `SvelteRuntimeBackend`); this is compatibility-internal composition, not a production selector, lifecycle cutover, or catalog route change.
- The generic host-backed outer `compile_entry` call and the fixed-Vue `compile_entry_runtime_render` outer call in `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs` are excluded and remain byte-for-byte present.
- RuntimeRender routing, request construction, and session selection are excluded entirely.

## Exact predecessor contracts

- **CCA1K:** the Vue runtime backend accepts the typed runtime request with equivalent output for every bundle-runtime case.
- **CCA1L:** the Svelte runtime backend accepts the typed runtime request with equivalent output, refusal, and diagnostics for every bundle-runtime case, including `RuntimeSurfaceRefused`.

## Invariants and acceptance

- Both compatibility implementations call their typed `RuntimeCompilerBackend`; no legacy internal runtime construction remains in either.
- Runtime bytes, maps, diagnostics, refusal classification, and equivalent-work counters are unchanged for both frameworks.
- Both outer session `compile_bundle` calls remain, and no third production outer call appears.
- One request performs one parse/semantic/plan/emit/assembly/copy sequence; no dual execution or shadow branch remains.

## Deletions, budget, and verification

Delete only the two displaced internal runtime-construction branches. Ceiling: 500 production LOC, 4 files, 2 crates; abort on selector, lifecycle, publication, or session-route mutation. Run Vue and Svelte bundle/runtime/conformance suites, host-backed virtual/batch suites, and `targeted-domain`. CCA1M3 builds directly on this delegated state; CCA1M consumes this route fact.
