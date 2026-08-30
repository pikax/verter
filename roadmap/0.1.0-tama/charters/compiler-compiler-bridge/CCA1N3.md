<!-- unified-charter-v2
id=CCA1N3
name=Host-backed multi-product route cutover
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=migration
semantic_role=delivery
class=compiler
predecessors=CCA1N4
owner=compiler.compiler-bridge:exclusive deletion of the compile_entry host-backed bundle route
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
charter=charters/compiler-compiler-bridge/CCA1N3.md
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1N3 — Host-backed multi-product route cutover

## Independently acceptable outcome and owners

The generic host-backed transaction consumes the request-scoped `BoundNativeHostRequest` with the host-backed multi-product demand: the selected `FrameworkHostIntegrationBackend` issues the demand-specific `CompileAdmission`, and this node exclusively deletes the host-backed `compile_entry` outer `CarrierCompiler::compile_bundle` call. Runtime-render behavior is not modified. Current ownership is the generic bundle selector; final ownership is the framework host backend. Reverting restores this one selector without affecting runtime-render.

## Exact eight-file production/fixture population

1. `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs` — `compile_entry` bound-demand consumption and exclusive outer-call deletion.
2. `crates/verter_session/src/host_executor.rs` — generic host-backed entrypoint.
3. `crates/verter_session/src/host_compile.rs` — managed/batch host-backed entrypoint.
4. `crates/verter_session/src/framework/framework_product_surface_inventory.json` — the host-backed route rows this node owns; the runtime-render cutover already performed the physical split of the combined batch record and owns the runtime-render rows, so this node updates only the residual host-backed rows.
5. `crates/verter_session/src/host_compile_audit.rs` — audit route attribution.
6. `crates/verter_compiler/src/compile/mod.rs` — external-bundle route documentation.
7. `crates/verter_session/src/svelte_conformance_cell_record.json` — committed per-cell Svelte route fact; it remains host-backed-owned while its cells exercise the Main virtual-file host route, and is not relabeled runtime-render without changing the route actually exercised.
8. `crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs` — observed-record route construction paired with that fixture.

No other production, fixture, or route-record surface is owned. The observed route and committed record must change together so the equality gate cannot remain green on a stale registry string; route evidence prefers structured execution observations (requested lane, selected host catalog identity, selected runtime identity, adapter and carrier language, framework and host epochs, source generation, requested products, outcome) with any retained string derived from or equality-checked against them.

## Focused route-guard evidence

`crates/verter_session/tests/cases/svelte_compiler_block1_guards.rs` is additional test evidence, not a ninth production/fixture surface. The implementation must retarget `compile_entry_routes_through_carrier_registry_not_hardcoded_vue` to durable behavior: the host-backed route consumes the request-scoped bound host request and the registered `FrameworkHostIntegrationBackend`, while the existing AST discriminator still rejects direct/hardcoded Vue producers and aliased or glob-imported producer calls. Rename the test and its comments/assertion diagnostics to registered-host-backend/no-hardcoded-framework wording; because the current filename contains transient implementation vocabulary, rename the evidence file to a durable backend-selection name in the same candidate and update its test registration. No assertion may mention a roadmap, node, block, phase, cutover sequence, registry history, or deletion history.

## Exact predecessor contract

- **CCA1N4:** the runtime-render lane already executes through bound framework host backends, so deleting the host-backed outer call and its shared request-construction substrate cannot leave any lane dependent on the compatibility bundle path. CCA1N4 transitively provides CCA1N2B's production binding, CCA1N2A's substrate, and both host backends.

## Invariants and acceptance

- Preserve atomic multi-product refusal, lifecycle, cancellation, supersession, diagnostic ordering, maps, publication, and fresh/first-warm/repeated-warm/incremental equivalence; existing cache and fact behavior remains, and last-good behavior remains only where already owned by this lane.
- Framework binding occurred once upstream; this lane consumes the bound demand, reconstructs no Vue/Svelte topology, contains no framework selector, and adds no parse/semantic/projection/runtime/assembly/copy pass.
- No sibling product publishes after refusal.
- Structural evidence proves the host-backed outer bundle call and every stale host-backed registry route record are absent, runtime-render behavior is untouched, and the retargeted guard proves selection is registered and framework-neutral rather than merely proving an old call disappeared.

## Deletions, budget, and verification

Delete only the host-backed selector branch and its one outer bundle call. Ceiling: 800 production LOC, exactly 8 production/fixture files, 2 crates; focused guard retarget/rename is evidence outside that production budget, while any ninth production/fixture surface requires rescope. Run the registered-backend/no-hardcoded-framework guard, host/virtual/batch, Svelte conformance-record, audit, cancellation, and `targeted-domain` evidence. CCA1N joins this with CCA1N4.
