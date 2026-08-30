<!-- unified-charter-v2
id=CCA1N2
name=Svelte native host-integration backend
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1J,CCA1M,CCA1N1
owner=compiler.compiler-bridge:Svelte FrameworkHostIntegrationBackend with demand-specific admission issuance
conflict_domains=compiler_execution,host_service_graph,svelte_product
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
charter=charters/compiler-compiler-bridge/CCA1N2.md
max_production_loc=700
max_production_files=7
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1N2 — Svelte native host-integration backend

## Independently acceptable outcome and rollback boundary

Implement the Svelte `FrameworkHostIntegrationBackend<SvelteEpoch, NativeHostEpoch>` behind the existing session route, with demand-specific `CompileAdmission` issuance for both lanes. It coordinates canonical typed compile requests without becoming the production selector, and remains unused by the production Svelte runtime-render route until the bound runtime-render cutover. Reverting removes only this unused Svelte host backend and catalog row.

## Concrete surfaces and APIs

- Surfaces: Svelte-specific helpers reached from `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`, `host_executor.rs`, `host_compile.rs`, and compiler-side Svelte host adapters.
- Consumes the generic demand-specific issuance surface introduced by CCA1N1; must not modify the generic capability trait.
- Supports both demands symmetrically: host-backed multi-product and runtime-render, including the Svelte self-contained Main module and requested style side-products from one admitted population.
- Capability validation is demand-specific: a runtime-render demand must not require `ProjectionBackend` capability; a missing required runtime capability yields a typed unavailability outcome, never a fallback to host-backed, another framework, or a compatibility compiler.
- Owns Svelte request construction for both demands, prerequisite sharing, ordered calls to the frontend/semantic/projection/runtime capabilities, diagnostic aggregation, refusal classification and atomicity, lifecycle/cancellation handoff, the current self-contained-module handoff, and per-product publication payloads. CCA2BS alone may relocate Svelte framework assembly, with CCA2B joining both framework migrations and CCA2C owning the later staged host handoff.
- One admitted parse/semantic/runtime/assembly population per request; producing Main plus requested style side-products must not trigger a second compile.
- A runtime refusal preserves the current all-or-none transaction outcome; no sibling projection/template product may publish or warm after refusal.
- Does not move the generic session selector, does not add a generic framework switch, and does not touch TypeScript/NAPI DTOs, Vue orchestration, or CCA2 staged artifact schema.

## Exact predecessor contracts

- **CCA1J:** implemented ledger row for “IDE projection route convergence”.
- **CCA1M:** implemented ledger row for “Runtime compile route convergence join”; CCA1M1/CCA1M2 prove direct and compatibility-internal runtime delegation while retaining the host-backed outer call.
- **CCA1N1:** the generic demand-specific issuance surface exists on the capability trait, so this node implements against it without modifying the generic trait.

## Acceptance and evidence

- Svelte runtime, IDE, and template-fact demands share one admitted request and one prerequisite population with no duplicate parse, semantic, projection, plan, emit, assembly, or copy pass.
- Host-backed multi-product and runtime-render demands each receive a demand-specific `CompileAdmission` issued only by this backend; product backends consume, never mint, admission.
- Produced/refused diagnostics, maps, self-contained modules, source identity, cancellation, and publication eligibility match the current transaction exactly.
- Structural evidence proves generic production callers still use the old selector in this node, and the production Svelte runtime-render route remains on its characterized transitional behavior.

## Deletions, budgets, and aborts

- Delete no generic host route; forbid Vue, generic-trait, NAPI/TypeScript public DTO, unplugin, and staged-artifact work.
- Ceiling: 700 LOC, 7 files, 2 crates; rescope if generic selection or another framework enters.
- Abort on partial publication after refusal, duplicate prerequisites, an admission token exposing a general capability/service bag, or parity/performance divergence.

## Verification and review

Use TDD around Svelte multi-product/render/refusal/publication boundaries, run compiler/session Svelte host suites and `targeted-domain`. Apply `public-3`; add only CCA1N2's ledger row.
