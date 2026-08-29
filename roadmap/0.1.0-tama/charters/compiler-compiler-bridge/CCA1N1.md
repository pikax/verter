<!-- unified-charter-v2
id=CCA1N1
name=Vue native host-integration backend
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=implementation
semantic_role=delivery
class=compiler
predecessors=CCA1J,CCA1M
owner=compiler.compiler-bridge:Vue FrameworkHostIntegrationBackend implementation
conflict_domains=compiler_execution,host_service_graph,vue_product
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
charter=charters/compiler-compiler-bridge/CCA1N1.md
max_production_loc=700
max_production_files=7
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1N1 — Vue native host-integration backend

## Independently acceptable outcome and rollback boundary

Implement the Vue `FrameworkHostIntegrationBackend<VueEpoch, NativeHostEpoch>` behind the existing session route. It coordinates one canonical multi-product `CompileRequest` without becoming the production selector. Reverting removes only this unused Vue host backend and catalog row.

## Concrete surfaces and APIs

- Surfaces: Vue-specific helpers reached from `crates/verter_session/src/host_resolve/virtual_file_pipeline.rs`, `host_executor.rs`, `host_compile.rs`, and compiler-side Vue host adapters, including backend entry methods for the future `compile_entry` and `compile_entry_runtime_render` selectors.
- Owns Vue request construction, prerequisite sharing, ordered calls to the frontend/semantic/projection/runtime capabilities, diagnostic aggregation, refusal atomicity, lifecycle/cancellation handoff, the current Vue host-assembly handoff, the runtime-render lane's render-only handoff, and per-product publication payloads. CCA2BV alone may relocate Vue framework assembly, with CCA2B joining both framework migrations and CCA2C owning the later staged host handoff.
- A runtime refusal preserves the current all-or-none transaction outcome; no sibling projection/template product may publish or warm after refusal.
- Does not move the generic session selector, TypeScript/NAPI DTOs, Svelte orchestration, or CCA2 staged artifact schema.

## Exact predecessor contracts

- **CCA1J:** implemented ledger row for “IDE projection route convergence”.
- **CCA1M:** implemented ledger row for “Runtime compile route convergence join”; CCA1M1–CCA1M3 prove direct, host-backed, and runtime-render runtime delegation while retaining both outer calls.

## Acceptance and evidence

- Vue runtime, IDE, and template-fact demands share one admitted request and one prerequisite population with no duplicate parse, semantic, projection, plan, emit, assembly, or copy pass.
- Produced/refused diagnostics, maps, virtual modules, source identity, cancellation, and publication eligibility match the current transaction exactly.
- Structural evidence proves both generic production callers still use their old bundle adapters in this node.

## Deletions, budgets, and aborts

- Delete no generic host route; forbid Svelte, NAPI/TypeScript public DTO, unplugin, and staged-artifact work.
- Ceiling: 700 LOC, 7 files, 2 crates; rescope if generic selection or another framework enters.
- Abort on partial publication after refusal, duplicate prerequisites, or parity/performance divergence.

## Verification and review

Use TDD around Vue multi-product/refusal/publication boundaries, run compiler/session Vue host suites and `targeted-domain`. Apply `public-3`; add only CCA1N1's ledger row.
