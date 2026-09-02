<!-- unified-charter-v2
id=PM3
name=Atomic project snapshot publication and invalidation
predecessors=PM1,PM2,F1
phase=expansion
train=expansion.project-model
product=project_model
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.project-model:project membership, environment, resolution, and immutable snapshot authority over kernel-owned identities
conflict_domains=project_model,host_service_graph,semantic_cache_store
resource_class=rust-mixed
gate_profile=targeted-domain
review_profile=concurrency-3
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
charter=charters/expansion-project-model/PM3.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# PM3 — Atomic project snapshot publication and invalidation

## Independently acceptable outcome and owners

Publish membership, environment, references, package facts, and resolver state as one immutable project snapshot. Current snapshots and registries can advance independently; final authority is `ProjectSnapshot` plus one publication generation and validation proof.

## Surfaces, APIs, and predecessor contracts

Expected surfaces: `verter_workspace`, `verter_session` host/project state, and LSP project publication. APIs: `ProjectSnapshot`, `ProjectSnapshotId`, `ProjectGeneration`, `ProjectReadSet`, `ProjectSnapshotReceipt`, `ProjectDelta`. `PM1` supplies identities; `PM2` supplies resolution proofs/read sets; `F1` supplies coherent input bases and snapshot fences.

## Binding architecture and subblocks

1. Build candidate snapshots privately from one `InputBasis` and bounded discovery/load wave.
2. Validate every config/package/source/reference generation before compare-and-publish.
3. Publish one atomic snapshot or typed stale/cancelled/NeedInputs outcome.
4. Derive minimal affected-project deltas and retire generations after readers release them.

No cache may combine membership from one generation with resolver state from another. Cancellation publishes neither partial snapshots nor reusable negative facts. See the shared expansion contract.

## Migration, deletions, and forbidden designs

Add the snapshot beside current registries for measurement, migrate consumers by exact manifest, then remove independent generation counters and torn-update routes. Forbid in-place mutation, publish-then-fill, polling readiness, open-document exceptions, and unbounded retained generations.

## Acceptance, performance, abort, verification, and consumers

- **PM3-AC1:** simultaneous tsconfig/package/reference/source changes publish all-or-nothing coherent snapshots.
- **PM3-AC2:** planted mixed-generation and post-validation mutations are rejected.
- **PM3-AC3:** arbitrary edit/revert/project-open-close sequences equal fresh reconstruction.
- **PM3-AC4:** unchanged warm publication is zero work; churn plateaus and released projects free retained generations.
- Abort if atomicity requires changing semantic algorithms or public protocol in the same node.
- Verify state-machine, concurrency, cancellation, workspace/session/LSP tests and `concurrency-3`.

PM4 owns consumer cutover. Ceiling: 800 LOC, 8 files, 2 packages; ledger presence records completion.
