<!-- unified-charter-v2
id=CCA1T4
name=Mixed compiler option bucket deletion
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=cutover
semantic_role=delivery
class=compiler
predecessors=CCA1T3
owner=compiler.compiler-bridge:legacy cross-framework compiler options and runtime stub deletion
conflict_domains=compiler_execution,capability_catalog
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
charter=charters/compiler-compiler-bridge/CCA1T4.md
max_production_loc=500
max_production_files=5
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1T4 — Mixed compiler option bucket deletion

## Independently acceptable outcome and owners

Delete the unused cross-framework IDE/runtime option bucket, neutral runtime output stubs, and remaining compatibility module shell after the combined trait is gone. Current ownership is legacy `carrier_compiler` data; final ownership is capability-local typed requests/results. Reverting restores only unused data declarations.

## Concrete surfaces and boundary

- Production/documentation surfaces are `crates/verter_compiler/src/framework_common/carrier_compiler.rs`, `crates/verter_compiler/src/framework_common/mod.rs`, bounded final imports in `crates/verter_compiler/src/framework_common/vue_bridge.rs` and `crates/verter_compiler/src/svelte/carrier.rs`, and `docs/plans/framework-plugin-system.md`.
- Own `IdeCompileOptions`, `RuntimeCompileOptions`, combined runtime output/refusal/custom-block/style DTOs, compatibility stubs, final module deletion when empty, and final durable architecture documentation.
- Capability-local request/result types and CCA2 staged artifact work are excluded.

## Exact predecessor contract

- **CCA1T3:** the combined trait and harness are absent, so every remaining legacy option/output declaration must have zero consumer or abort.

## Acceptance, budget, and verification

Repository-wide structural evidence proves no cross-framework option bucket, compatibility runtime output, tooling-only stub, or old module export remains. Typed capability and host suites retain parity and equivalent work. Ceiling: 500 production/documentation LOC, 5 files, 1 crate; abort on a live consumer or staged-artifact requirement. Run compiler/session suites and `targeted-domain`. CCA1 is the zero-production terminal join.
