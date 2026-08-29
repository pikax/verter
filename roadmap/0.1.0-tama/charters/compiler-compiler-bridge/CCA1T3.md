<!-- unified-charter-v2
id=CCA1T3
name=Combined CarrierCompiler trait deletion
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=cutover
semantic_role=delivery
class=compiler
predecessors=CCA1T2
owner=compiler.compiler-bridge:CarrierCompiler trait and trait-test harness deletion
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
charter=charters/compiler-compiler-bridge/CCA1T3.md
max_production_loc=350
max_production_files=2
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1T3 — Combined CarrierCompiler trait deletion

## Independently acceptable outcome and owners

Delete the now-unimplemented `CarrierCompiler` trait and its trait-contract harness while retaining the separately owned legacy option/data bucket for CCA1T4. Current authority is the unused combined trait declaration; final authority is the five typed capability interfaces. Reverting restores only the unused trait/harness.

## Concrete surfaces and boundary

- Surfaces are the trait and trait-only test module in `crates/verter_compiler/src/framework_common/carrier_compiler.rs` plus trait exports in `crates/verter_compiler/src/framework_common/mod.rs`.
- Do not delete or rename remaining option/output structs in this node; CCA1T4 owns their consumer audit and terminal deletion.

## Exact predecessor contract

- **CCA1T2:** its CCA1T2V/CCA1T2S ancestors separately removed the Vue and Svelte implementations/helpers/exports, and the zero-production join proves no framework implementation, compatibility wrapper, or production call remains for the combined trait.

## Acceptance, budget, and verification

Structural/type evidence proves no `CarrierCompiler` definition, trait object, implementation, bound, import, or call remains while typed capability APIs compile and their suites stay green. Ceiling: 350 production LOC, 2 files, 1 crate; abort on a live trait consumer. Run compiler checks/suites and `targeted-domain`. CCA1T4 consumes the trait-free option bucket.
