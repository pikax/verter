<!-- unified-charter-v2
id=CCA1T2S
name=Svelte combined-compiler compatibility deletion
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=cutover
semantic_role=delivery
class=compiler
predecessors=CCA1T1
owner=compiler.compiler-bridge:Svelte CarrierCompiler implementation and compatibility-export deletion
conflict_domains=compiler_execution,capability_catalog,svelte_product
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
charter=charters/compiler-compiler-bridge/CCA1T2S.md
max_production_loc=500
max_production_files=4
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1T2S — Svelte combined-compiler compatibility deletion

## Independently acceptable outcome, role, and owners

Delete the unused Svelte `impl CarrierCompiler`, compatibility wrappers, and Svelte-only exports after every Svelte production route uses typed capabilities. Current compatibility ownership is `SvelteCarrierCompiler`; final execution ownership is the five Svelte typed backends. Vue's implementation and the shared trait remain separate concerns.

## Exact surfaces and API boundary

- `crates/verter_compiler/src/svelte/carrier.rs` — delete the combined trait implementation, compatibility-only wrappers/imports, and implementation-only tests.
- `crates/verter_compiler/src/svelte/mod.rs` — delete only Svelte compatibility exports made unreferenced by that implementation.
- Focused Svelte runtime, IDE, conformance, and map tests may be retargeted to typed backends; they are evidence, not extra production surfaces.

Do not touch `framework_common/vue_bridge.rs`, the `CarrierCompiler` declaration/harness, generic sourcemap helpers still used by typed Svelte tests, mixed option/output DTOs, or staged artifacts.

## Exact predecessor contract and binding laws

- **CCA1T1:** `CarrierCompilerRegistry` and registry-only lookups are absent, so no production trait object can select the Svelte implementation.
- Structural route evidence must also prove all Svelte frontend, semantic, projection, runtime, and host calls already select typed backends from immutable catalog identity, and that both the host-backed and runtime-render production lanes consume typed `CompileAdmission` issued by the Svelte `FrameworkHostIntegrationBackend` before this compatibility implementation is deleted.
- Tests, comments, assertions, and diagnostics use durable backend/capability language only; no roadmap, node, phase, sequence, or deletion-history vocabulary may enter code or tests.

## Migration, deletions, acceptance, and performance

Characterize typed-versus-compatibility bytes/maps/diagnostics and work counts, remove the implementation/export population atomically, then prove the typed route alone. No shadow adapter or fallback may remain.

- **CCA1T2S-AC1:** no Svelte `CarrierCompiler` implementation, compatibility wrapper, production call, or Svelte-only compatibility export remains.
- **CCA1T2S-AC2:** Svelte frontend/semantic/projection/runtime/host outputs, maps, diagnostics, refusals, and ordering remain equivalent.
- **CCA1T2S-AC3:** fresh/incremental/cancelled behavior and complete-only admission remain unchanged.
- **CCA1T2S-AC4:** no duplicate parse, semantic, projection, compile, assembly, map, or copy work appears; inapplicable products stay zero-work.

Ceiling: 500 production LOC, 4 production files, 1 crate. Abort on a live Svelte trait consumer, Vue mutation, shared-trait deletion, cross-crate migration, or staged-artifact need. Run focused Svelte compiler/capability/conformance/map suites and `targeted-domain`; CCA1T2 joins this with CCA1T2V.
