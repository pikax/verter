<!-- unified-charter-v2
id=CCA1N4
name=Bound runtime-render execution cutover
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1N2B
owner=compiler.compiler-bridge:bound Vue and Svelte runtime-render execution and exclusive deletion of the compile_entry_runtime_render bundle route
conflict_domains=compiler_execution,host_service_graph,vue_product,svelte_product
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
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA1N4.md
max_production_loc=750
max_production_files=7
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1N4 — Bound runtime-render execution cutover

## Independently acceptable outcome and owners

The runtime-render lane consumes the request-scoped `BoundNativeHostRequest` with the runtime-render demand and routes both Vue and Svelte through their selected `FrameworkHostIntegrationBackend`, which issues the demand-specific `CompileAdmission` consumed by the framework's `RuntimeCompilerBackend`. This node exclusively deletes the runtime-render outer `CarrierCompiler::compile_bundle` call and removes every fixed-framework selection from the generic runtime-render path, including the transitional Svelte degradation. Final ownership: the bound framework host backend owns framework request topology and `CompileAdmission`; generic session code owns only lane/lifecycle orchestration. Reverting restores only this route.

## Exact production population and boundary

- Production surfaces: `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs` (runtime-render entry and exclusive outer-call deletion), `crates/verter_session/src/host_resolve/compile_request_build.rs` (render-only bound request construction), runtime-render entry wiring in `crates/verter_session/src/host_executor.rs` and `crates/verter_session/src/host_compile.rs`, `crates/verter_session/src/host_compile_audit.rs` (runtime-render structured audit attribution), `crates/verter_session/src/framework/framework_product_surface_inventory.json` (the runtime-render route rows this node owns), and `crates/verter_session/src/runtime_render_lane_tests.rs`.
- Owns the runtime-render route record: each batch entry is its own host compile request with its own bound host request, the selected framework host backend, its demand-specific admission (one admission token type; demand carried in the issued value), and the matching runtime backend. Because this node lands before the host-backed cutover, it also performs the physical split of any combined batch route record: it extracts only the runtime-render rows it owns and amends the inventory note, leaving the residual combined record host-backed-owned for that cutover. Route evidence prefers structured facts (requested lane, selected host catalog identity, selected runtime identity, adapter and carrier language, framework and host epochs, source generation, requested products, produced/refused/error outcome) over free-form route strings; any retained string is derived from or equality-checked against those observations.
- Removes the Svelte degradation on this lane: a Svelte carrier receives a Svelte-bound request with the exact requested style policy over the option axes the current render profile can express — extending the public request/option surface remains excluded and stays with the typed host compile-request schema work; missing runtime or host capability produces a typed unavailability, never a fallback to the host-backed lane, another framework, or a compatibility compiler.
- Excluded: host-backed multi-product selection and its outer call, the host-backed route inventory rows and Svelte official-conformance cell record (those remain with the host-backed cutover), public DTOs, and staged artifacts.

## Focused direct-result route evidence

`crates/verter_compiler/src/direct_result_tests/vdom_ssr_root_prefix_comment_absorption.rs` is additional test evidence, not an eighth production surface. Retarget its `native_route_compile_bundle` helper and affected comments/assertions from `CarrierCompilerRegistry`/`CarrierCompiler::compile_bundle` to direct typed Vue runtime-backend construction used by the host/runtime contract, using durable names such as `compile_through_vue_runtime_backend`; diagnostics state the runtime/SSR behavior and typed-backend invariant only. The discriminator remains the root-prefix comment absorption behavior across backend/build-mode cells.

## Exact predecessor contract

- **CCA1N2B:** every host compile request already creates and threads exactly one bound host request; no lane-specific framework selection remains, so this node performs a pure execution cutover on an already-bound lane. CCA1N2B transitively provides CCA1N2A's substrate and both host backends.

## Invariants and acceptance

- Preserve render bytes/maps/diagnostics for Vue, requested mode, missing-versus-refused behavior, lifecycle, cancellation, and deterministic ordering; Svelte render behavior becomes the bound Svelte backend's characterized output with exact requested style policy and no silent substitution through host-backed defaults.
- One render request makes one backend call with one admitted parse/semantic/runtime/assembly population; Svelte Main plus requested style side-products come from that one population with no second compile and no host-backed wrapper source clone, cache-mode classification, store-view construction, resolver-context creation, or semantic-transitive synchronization unless a requested product contractually requires it.
- No cache admission and no last-known-good fallback on this lane.
- Structural evidence proves only the runtime-render outer bundle call is absent, the host-backed call is untouched, no framework predicate remains on the lane, and the route/audit records describe the executed bound topology.

## Deletions, budget, and verification

Delete only the runtime-render selector branch, its one outer bundle call, and the lane's fixed-framework construction. Ceiling: 750 production LOC, 7 production files, 2 crates; focused direct-result retargeting is evidence outside that budget. Abort on host-backed mutation or an eighth production surface. Run runtime-render (both frameworks), audit, direct-result typed-backend suites, and `targeted-domain`. CCA1N3 consumes the bound-lane fact; CCA1N joins this with CCA1N3.
