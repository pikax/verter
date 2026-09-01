<!-- unified-charter-v2
id=CCA1M3
name=Fixed-Vue runtime-render compatibility route
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1M2
owner=compiler.compiler-bridge:fixed-Vue compile_entry_runtime_render compatibility route and Svelte-degradation characterization
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
max_production_loc=400
max_production_files=4
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1M3 — Fixed-Vue runtime-render compatibility route

## Independently acceptable outcome and owners

The fixed-Vue `compile_entry_runtime_render` transaction is proven correct and fully characterized on top of CCA1M2's delegated compatibility implementations, while keeping its outer `CarrierCompiler::compile_bundle` call. This node owns the render route's request construction, render-only requested products, refusal/diagnostic parity evidence, the retained fixed-Vue outer call, and characterization of the Svelte degradation exactly as it exists today. It does NOT delete, re-own, or modify any compatibility-backend internal runtime construction — CCA1M2 is the sole owner of that deletion. CCA1N4 alone owns later route cutover. Reverting restores only the render route's local construction and evidence.

## Concrete surfaces and boundary

- Surfaces are `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs::compile_entry_runtime_render`, runtime-render request construction in `crates/verter_session/src/host_resolve/compile_request_build.rs`, and focused runtime-render tests including `crates/verter_session/src/runtime_render_lane_tests.rs`.
- Preserve fixed Vue request construction, render-only requested products, refusal classification, diagnostics, cancellation, and exactly one outer bundle call.
- The current Svelte behavior on this lane is transitional and must be characterized as it actually exists: a Svelte carrier admitted to the render lane receives a Vue-shaped compile request, the registry still selects the Svelte compatibility implementation by artifact identity, and the Svelte-specific request options collapse to defaults. Characterization pins that observable outcome without presenting it as final RuntimeRender ownership; the bound cutover later replaces it.
- Introducing a shared Vue/Svelte render selector, a new framework branch, or any compatibility-backend mutation is forbidden.

## Exact predecessor contract

- **CCA1M2:** both compatibility bundle implementations already delegate runtime construction to their typed runtime backends, so the render route's parity evidence measures the delegated state; the internal-runtime deletion population is complete and owned there.

## Invariants and acceptance

- Render bytes, maps, diagnostics, refusal classification, and equivalent-work counters remain equivalent through the delegated compatibility implementation.
- Exactly the runtime-render outer bundle call remains on this route; the host-backed outer call is untouched.
- Characterization evidence discriminates the current Svelte degradation (Vue-shaped request, collapsed Svelte options) so the later bound cutover can prove its replacement.
- No new selector, no compatibility-backend runtime edits, no host lifecycle mutation.

## Deletions, budget, and verification

This node deletes no production construction: the fixed-Vue constructor, the outer bundle call, and every compatibility branch are preserved exactly, with their deletion owned by their named later owners. Ceiling: 400 production LOC, 4 files, 2 crates; abort on any production deletion, selector, compatibility-backend, or lifecycle mutation. Run runtime-render suites and `targeted-domain`. CCA1M consumes this route fact.
