<!-- unified-charter-v2
id=CCA1N4
name=Vue runtime-render route cutover
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1N1
owner=compiler.compiler-bridge:exclusive deletion of the compile_entry_runtime_render bundle route
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
size=M
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA1N4.md
max_production_loc=650
max_production_files=6
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1N4 — Vue runtime-render route cutover

## Independently acceptable outcome and owners

Replace only `compile_entry_runtime_render`'s fixed-Vue outer `CarrierCompiler::compile_bundle` call with direct immutable-catalog selection of the Vue `FrameworkHostIntegrationBackend`. Current ownership is the render-specific bundle selector; final ownership is the Vue host backend. Reverting restores only this route.

## Exact production population and boundary

- The six production surfaces are `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`, `crates/verter_session/src/host_resolve/compile_request_build.rs`, runtime-render entry wiring in `crates/verter_session/src/host_executor.rs` and `crates/verter_session/src/host_compile.rs`, `crates/verter_session/src/host_compile_audit.rs`, and `crates/verter_session/src/runtime_render_lane_tests.rs`.
- Owns fixed Vue selection, render-only request construction, the render audit label, and exclusive deletion of the runtime-render outer bundle call.
- Host-backed multi-product selection, Svelte route records, public DTOs, and staged artifacts are excluded.

## Focused direct-result route evidence

`crates/verter_compiler/src/direct_result_tests/vdom_ssr_root_prefix_comment_absorption.rs` is additional test evidence, not a seventh production surface. Retarget its `native_route_compile_bundle` helper and the affected comments/assertions from `CarrierCompilerRegistry`/`CarrierCompiler::compile_bundle` to direct typed Vue runtime-backend construction used by the host/runtime contract. Use durable names such as `compile_through_vue_runtime_backend`; diagnostics state the Vue runtime/SSR behavior and typed-backend invariant, never a roadmap, node, block, phase, cutover sequence, registry history, or deletion history. The discriminator remains the root-prefix comment absorption behavior across backend/build-mode cells, not the historical adapter path.

## Exact predecessor contract

- **CCA1N1:** the Vue host backend owns the runtime-render handoff and equivalent render/refusal outcomes.

## Invariants and acceptance

- Preserve render bytes/maps/diagnostics, requested mode, missing-versus-refused behavior, lifecycle, cancellation, and deterministic ordering.
- One render request makes one backend call and adds no parse/semantic/projection/runtime/assembly/copy pass.
- Structural evidence proves only the runtime-render outer bundle call is absent and CCA1N3's host-backed call is untouched when this node lands independently; the direct-result evidence proves the typed Vue runtime backend still exercises the native SSR behavior.

## Deletions, budget, and verification

Delete only the render selector branch and its one outer bundle call. Ceiling: 650 production LOC, 6 production files, 2 crates; focused direct-result retargeting is evidence outside that production budget. Abort on a seventh production surface or host-backed mutation. Run direct-result typed-backend, runtime-render/audit suites, and `targeted-domain`. CCA1N joins this with CCA1N3.
