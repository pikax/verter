<!-- unified-charter-v2
id=CCA1T2V
name=Vue combined-compiler compatibility deletion
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=cutover
semantic_role=delivery
class=compiler
predecessors=CCA1T1
owner=compiler.compiler-bridge:Vue CarrierCompiler implementation and compatibility-helper deletion
conflict_domains=compiler_execution,capability_catalog,vue_product
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
charter=charters/compiler-compiler-bridge/CCA1T2V.md
max_production_loc=500
max_production_files=4
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1T2V — Vue combined-compiler compatibility deletion

## Independently acceptable outcome, role, and owners

Delete the unused Vue `impl CarrierCompiler` and Vue-only compatibility helpers after every Vue production route uses typed capabilities. Current compatibility ownership is `VueCarrierCompiler`; final execution ownership is the five Vue typed backends. Svelte's implementation and the shared trait remain valid for separate deletion.

## Exact surfaces and API boundary

- `crates/verter_compiler/src/framework_common/vue_bridge.rs` — delete the combined trait implementation and compatibility-only wrappers, imports, and tests.
- `crates/verter_compiler/src/framework_common/sourcemap_e2e_helpers.rs` — delete or retarget only helpers whose sole consumer is the Vue combined implementation; durable generic map helpers remain.
- Focused compiler direct-result and Vue adapter tests may be retargeted to typed backends; they are evidence, not extra production surfaces.

Do not touch `svelte/carrier.rs`, `svelte/mod.rs`, the `CarrierCompiler` declaration/harness, mixed option/output DTOs, registry authority already deleted by CCA1T1, or staged artifacts.

## Exact predecessor contract and binding laws

- **CCA1T1:** `CarrierCompilerRegistry` and registry-only lookups are absent, so no production trait object can select the Vue implementation.
- Structural route evidence must also prove all Vue frontend, semantic, projection, runtime, and host calls already select typed backends from immutable catalog identity.
- Tests, comments, assertions, and diagnostics use durable backend/capability language only; no roadmap, node, phase, sequence, or deletion-history vocabulary may enter code or tests.

## Migration, deletions, acceptance, and performance

Characterize typed-versus-compatibility bytes/maps/diagnostics and work counts, remove the implementation/helper population atomically, then prove the typed route alone. No shadow adapter or fallback may remain.

- **CCA1T2V-AC1:** no Vue `CarrierCompiler` implementation, compatibility wrapper, production call, or Vue-only trait helper remains.
- **CCA1T2V-AC2:** Vue frontend/semantic/projection/runtime/host outputs, maps, diagnostics, refusals, and ordering remain equivalent.
- **CCA1T2V-AC3:** fresh/incremental/cancelled behavior and complete-only admission remain unchanged.
- **CCA1T2V-AC4:** no duplicate parse, semantic, projection, compile, assembly, map, or copy work appears; inapplicable products stay zero-work.

Ceiling: 500 production LOC, 4 production files, 1 crate. Abort on a live Vue trait consumer, Svelte mutation, shared-trait deletion, cross-crate migration, or staged-artifact need. Run focused Vue compiler/capability/map suites and `targeted-domain`; CCA1T2 joins this with CCA1T2S.
