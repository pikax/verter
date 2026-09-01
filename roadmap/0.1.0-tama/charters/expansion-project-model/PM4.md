<!-- unified-charter-v2
id=PM4
name=Project-model consumer cutover and convergence
predecessors=PM3,PER0,VIM1
phase=expansion
train=expansion.project-model
product=project_model
kind=terminal
semantic_role=convergence
class=successor
owner=expansion.project-model:project membership, environment, resolution, and immutable snapshot authority over kernel-owned identities
conflict_domains=project_model,program_authority,performance_evidence
resource_class=rust-mixed
gate_profile=targeted-domain
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
release_gating=product
external_requirements=
charter=charters/expansion-project-model/PM4.md
size=S
max_production_loc=300
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# PM4 — Project-model consumer cutover and convergence

## Independently acceptable outcome and owners

Certify one project-model product after every named consumer and displaced project/resolution authority has been migrated or deleted by PM1–PM3. PM4 adds no project or resolution semantics and performs only bounded residual manifest/guard cleanup within its S-node budget. Final ownership is the PM1–PM3 API set plus a capability/deletion receipt.

## Surfaces, APIs, and predecessor contracts

Expected surfaces are the exact consumer/deletion manifests, generated guards, and terminal evidence spanning session, workspace, LSP, TypeScript provider/plugin, compiler/build, diagnostics, and indexes. APIs: `ProjectModelCapabilitySnapshot`, `ProjectModelConsumerManifest`, `ProjectModelDeletionReceipt`. `PM3` supplies the complete snapshot and closed migration population; `PER0` supplies cache/cancellation/zero-work law; `VIM1` supplies cross-vertical validation infrastructure.

## Binding architecture and subblocks

1. Freeze a required consumer matrix and prove every row uses `ProjectSnapshotId`/`ProjectEnvironmentId`.
2. Prove PM1–PM3 deleted LSP `ProjectRegistry` semantics, consumer-local resolver/option hashes, and alternate snapshot generations named by the manifest; perform only bounded residual deletion/guard wiring that fits this charter.
3. Run multi-root/monorepo/provider/build/IDE differential, state-machine, performance, and memory gates.
4. Publish the product receipt and exact residual external-owner ledger.

Terminal work may fix manifests/tests/deletions only; semantic findings reopen PM1–PM3. Common laws are in the shared expansion contract.

Forbidden terminal designs include implementing a new resolver or project-selection rule, weakening a required consumer row, certifying a sampled subset, retaining an unversioned dual route, or expanding cleanup past the declared budget instead of reopening its predecessor.

## Acceptance, performance, abort, verification, and consumers

- **PM4-AC1:** all required consumers use the canonical snapshot and proof routes.
- **PM4-AC2:** planted consumer-local project selection/resolution is structurally rejected.
- **PM4-AC3:** incremental/fresh and transition-sequence suites pass with no stale publication.
- **PM4-AC4:** warm, disabled, open/close, churn, cancellation, allocation, and RSS gates pass.
- Exact residual deletions include only manifest-listed alternate project identities, registries, resolvers, option hashes, and migration flags.
- Abort and reopen predecessors for missing semantics, an incomplete inventory, any required retained dual authority, or cleanup exceeding 300 LOC/3 files/1 related package.
- Verify the complete project-model matrix, targeted gate, strict DAG validation, and `architecture-3`.

SM3, BND0, and PPR0 consume this convergence. Ceiling: 300 LOC, 3 files, 1 package; the ledger row is completion.
