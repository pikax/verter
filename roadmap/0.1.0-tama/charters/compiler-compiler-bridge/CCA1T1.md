<!-- unified-charter-v2
id=CCA1T1
name=Dynamic compiler registry deletion
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=cutover
semantic_role=delivery
class=compiler
predecessors=CCA1O
owner=compiler.compiler-bridge:CarrierCompilerRegistry deletion
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
charter=charters/compiler-compiler-bridge/CCA1T1.md
max_production_loc=350
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1T1 — Dynamic compiler registry deletion

## Independently acceptable outcome and owners

Delete `CarrierCompilerRegistry` after every production selector uses the immutable capability catalog, while retaining the combined `CarrierCompiler` trait and framework compatibility implementations for later deletion nodes. Current lookup ownership is dynamic registry construction; final lookup ownership is the immutable catalog. Reverting restores only the unused registry.

## Concrete surfaces and boundary

- Production surfaces are `crates/verter_compiler/src/framework_common/registry.rs`, registry exports in `crates/verter_compiler/src/framework_common/mod.rs`, and residual test-helper lookup in `crates/verter_compiler/src/framework_common/registered_carrier_projection.rs`.
- Own registry constructors, dynamic lookup tables, exports, and registry-only tests. Trait definitions, option types, Vue/Svelte trait implementations, and public DTOs are excluded.

## Exact predecessor contract

- **CCA1O:** the complete capability/backend/route/public-request chain is implemented and shared legacy FFI profile deletion is complete; its ancestors prove no production selector requires the dynamic registry.

## Acceptance, budget, and verification

Structural/dependency evidence proves no production `CarrierCompilerRegistry` definition, export, lookup, or call remains and immutable catalog construction is unique. Delete only registry-owned code. Ceiling: 350 production LOC, 3 files, 1 crate; abort on a remaining selector. Run compiler/session capability suites and `targeted-domain`. CCA1T2 consumes the registry-free state.
