<!-- unified-charter-v2
id=CCA1M3
name=Runtime-render bundle runtime delegation
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1K
owner=compiler.compiler-bridge:compile_entry_runtime_render temporary bundle runtime delegation
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
charter=charters/compiler-compiler-bridge/CCA1M3.md
max_production_loc=450
max_production_files=4
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1M3 — Runtime-render bundle runtime delegation

## Independently acceptable outcome and owners

The fixed-Vue `compile_entry_runtime_render` transaction keeps its outer `CarrierCompiler::compile_bundle` call while its compatibility implementation delegates runtime construction to the Vue `RuntimeCompilerBackend`. CCA1N4 alone owns later selector deletion. Reverting restores only this render adapter's internal delegation.

## Concrete surfaces and boundary

- Surfaces are `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs::compile_entry_runtime_render`, runtime-render request construction in `crates/verter_session/src/host_resolve/compile_request_build.rs`, the Vue bundle compatibility implementation in `crates/verter_compiler/src/framework_common/vue_bridge.rs`, and focused runtime-render tests.
- Preserve fixed Vue selection, render-only requested products, refusal classification, diagnostics, cancellation, and one outer bundle call.
- Host-backed multi-product and direct/batch routes are excluded.

## Exact predecessor contract

- **CCA1K:** the Vue runtime backend accepts the typed runtime-render request with equivalent output.

## Acceptance, budget, and verification

Prove render bytes/maps/diagnostics and work counters remain equivalent and exactly the runtime-render outer bundle call remains. Delete only displaced internal Vue runtime construction. Ceiling: 450 LOC, 4 files, 2 crates; abort on selector or lifecycle mutation. Run runtime-render suites and `targeted-domain`.
