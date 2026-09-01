<!-- unified-charter-v2
id=PM1
name=Project identity derivation, membership, and compiler environment
predecessors=PM0
phase=expansion
train=expansion.project-model
product=project_model
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.project-model:project membership, environment, resolution, and immutable snapshot authority over kernel-owned identities
conflict_domains=project_model,source_lineage
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
release_gating=none
external_requirements=
charter=charters/expansion-project-model/PM1.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# PM1 — Project identity derivation, membership, and compiler environment

## Independently acceptable outcome and owners

Produce stable set-valued project membership and exact compiler-environment identities for configured, inferred, referenced, and out-of-tree projects. Current owners are workspace/LSP registries and ad hoc option hashes; final owner is the PM project catalog.

## Surfaces, APIs, and predecessor contract

Expected surfaces: `verter_workspace` and project setup in `verter_session`/`verter_lsp`. APIs: kernel-owned configured-project/environment IDs plus PM-owned `ProjectMembershipSet`, `ProjectRoot`, `CompilerEnvironment`, and `ProjectDiscoveryReadSet`. `PM0` supplies the accepted derivation and owner map; identity type law and registration remain VID0/CAT0 ownership.

## Binding architecture and subblocks

1. Canonicalize project roots/config lineage without resolving symlinks or casing inconsistently with source identity.
2. Represent multiple legitimate memberships and deterministic preferred ownership separately.
3. Hash only semantic compiler-environment inputs: options, libs, types/typeRoots, references, conditions, and already-converged language/capability epochs.
4. Publish explicit inferred/no-config projects and ambiguity rather than selecting by open order.

Membership and environment caches bind source identity, config read set, project-discovery generation, and the accepted host-language capability epoch. They do not depend on the later `ProjectProfileId`/`ProjectProfileEpoch`; framework project profiles are overlays keyed by the published project snapshot, so the DAG and cache identities do not form a semantic cycle. Config/package changes invalidate only affected projects; cancelled discovery publishes nothing. Common laws are in `contracts/product-surface-expansion.md`.

## Migration, deletions, and forbidden designs

Characterize current project selection, install the typed catalog, compare under mixed monorepo/multi-root fixtures, then switch identity consumers. Delete only displaced project-id/option-hash constructors. Forbid one-project-per-file assumptions, editor-open-order preference, cwd identity, ambient home config, and stringly framework options.

## Acceptance, performance, abort, verification, and consumers

- **PM1-AC1:** configured/inferred/referenced/multi-root/out-of-tree fixtures produce deterministic membership and preferred-owner results.
- **PM1-AC2:** a planted compiler-option omission aliases no environment IDs.
- **PM1-AC3:** incremental config edits equal fresh discovery; revert restores the original IDs without stale publication.
- **PM1-AC4:** warm unchanged requests perform zero config reads and bounded membership work.
- Abort if identity requires module-resolution answers owned by PM2 or atomic snapshot publication owned by PM3.
- Verify targeted workspace/session/LSP project tests plus the bound gate and `architecture-3`.

PM2 and PM3 consume the identities. Ceiling: 800 LOC, 8 files, 2 related packages; the ledger row is the completion fact.
