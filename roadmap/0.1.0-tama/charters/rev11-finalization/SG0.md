<!-- unified-charter-v2
id=SG0
name=Shipped-configuration behavioral verification restoration
predecessors=A6
phase=rev11
train=rev11.finalization
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.finalization:canonical shipped-configuration execution and required-job evidence
conflict_domains=release_orchestration
resource_class=rust-mixed
gate_profile=canonical
review_profile=architecture-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-finalization/SG0.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SG0 — Shipped-configuration behavioral verification restoration

## Independently acceptable outcome

Restore behavioral execution under the shipped debug-assertions-off configuration in the canonical gate and its CI/release owners. A6 supplies the accepted verification baseline. Current state explicitly skips the lane; this node owns removal of that exception before L4.

## Concrete surfaces and acceptance

Expected surfaces: scripts/gate-internals.mjs, scripts/gate.mjs, the shipped configuration contract crate, applicable workflow jobs and owning gate/testing documentation. Reuse the existing lane and gate evidence protocol; do not create a parallel test runner or duplicate test universe.

- **SG0-AC1 — sole-owner outcome:** the canonical gate executes both cargo check --workspace --all-targets --profile no-debug-assertions and the verter_shipped_cfg_contract nextest lane under that profile. Nonzero selection, actual execution and complete terminal summaries are required.
- **SG0-AC2 — positive contract:** a demonstrably applied state mutation inside debug_assert! is caught through the exact canonical entry point; a missing, skipped, zero-work or aborted shipped lane cannot pass. The clean candidate passes with the control removed.
- **SG0-AC3 — incremental equivalence:** required CI and release invocation ownership covers the restored lane; bounded child deadlines/memory limits and serialized resources hold. Update the temporary-skip disclosures only after the lane actually runs. Release cargo check alone never satisfies the behavioral outcome.

- **SG0-AC4 — bounded work:** the restored lane preserves bounded supervisor deadlines and serialized build resources, reports actual selected/executed work, and cannot hide an aborted child behind core-test success.

## Verification and abort

Run focused gate supervisor tests and the clean/negative canonical gate experiments, then the canonical final profile. Abort on inability to prove the plant applied, incomplete child summaries or runtime-only side effects left untested. L4 requires this node; no release-close waiver silently replaces it.

## Review and completion

Apply the node's fresh review profile and the bound final gate; affected findings and evidence are rerun after material changes. Transition only this node's predeclared implementation row inside its own implementation patch before review. Commit message, approximate date and optional PR are locator hints only. This charter amendment leaves the node pending.
