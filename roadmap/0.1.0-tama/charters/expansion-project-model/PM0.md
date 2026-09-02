<!-- unified-charter-v2
id=PM0
name=Project-model derivation and authority constitution
predecessors=BR0,C1,F1,CFG0,IDX0,TE5,VID0
phase=expansion
train=expansion.project-model
product=project_model
kind=contract
semantic_role=delivery
class=successor
owner=expansion.project-model:project membership, environment, resolution, and immutable snapshot authority over kernel-owned identities
conflict_domains=project_model
resource_class=docs-light
gate_profile=docs-domain
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
charter=charters/expansion-project-model/PM0.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# PM0 — Project-model derivation and authority constitution

## Independently acceptable outcome and owners

Ratify one live successor to historical `C1` for deriving configured-project membership, compiler-environment identity, package/module resolution, and immutable project-snapshot publication. Current ownership is split across `ProjectGraph`, `WorkspaceSnapshot`, LSP `ProjectRegistry`, compiler-host setup, and consumer-local resolution. `VID0`/`CAT0` remain the sole owners of orthogonal identity types and catalog registration; `expansion.project-model` owns how project facts are derived and published over those identities. IDX0 remains candidate/index authority only.

## Surfaces, APIs, and predecessor contracts

Expected surfaces are `verter_workspace`, `verter_session`, `verter_protocol`, `verter_lsp`, and `packages/typescript-plugin`; dispatch binds exact symbols. The contract consumes kernel-owned configured-project/environment/snapshot identity types and defines `ProjectMembership`, `CompilerEnvironment`, `ProjectSnapshot`, `ModuleResolutionRequest`, `ModuleResolutionProof`, and typed degraded outcomes. It may not mutate the kernel identity owner or introduce a second identity family; a missing exact identity aborts for a VID0/CAT0 amendment.

- `BR0` permits successor work after the Rev11 terminal and therefore carries the final accepted Flow convergence. This train does not duplicate intermediate Flow audit or typed-outcome authority.
- `C1` supplies the historical module-resolver boundary, not proof of the larger live project model.
- `F1` supplies coherent input bases and snapshot fences.
- `CFG0` supplies captured configuration provenance/read sets without core-side arbitrary JS execution.
- `IDX0` supplies bounded project/member candidates and may not resolve them authoritatively.
- `TE5` supplies selective-forcing convergence and independently gates product-surface genesis. The final Rev11 convergence must also consume TE5; this direct edge is deliberate defense in depth, not permission to remove `TE5 → D8`.
- `VID0` supplies orthogonal configured-project/profile identities and exact-release law; PM derives facts over those IDs and never redefines them.

## Binding architecture and internal subblocks

1. Freeze identity, membership, environment, resolution, and publication vocabularies.
2. Inventory every current producer/consumer and assign PM1–PM4 as the sole final owners.
3. Define `Complete | NeedInputs | Ambiguous | Unsupported | Cancelled | Stale` outcomes and negative guards against path-shaped identity and consumer-local resolution.

Project identity is independent of open-editor state and symlink spelling. Environment identity includes relevant compiler options, lib/types/typeRoots selections, resolution mode, conditions, package facts, and project-reference visibility. See `contracts/product-surface-expansion.md` for common migration, cache, proof, and performance law.

## Migration, deletions, and forbidden designs

This node changes authority bytes only and deletes nothing. It forbids making `WorkspaceIndex`, `ProjectRegistry`, a TypeScript provider, or a bundler the project authority; global ambient configuration; executing user JS in Rust/WASM; and collapsing missing/ambiguous resolution into not-found.

## Acceptance, abort, verification, and consumers

- **PM0-AC1:** every live project/resolution producer and consumer maps to exactly one PM node or an explicit external owner.
- **PM0-AC2:** planted duplicate authority, path-only identity, and unqualified environment-cache entries are rejected.
- **PM0-AC3:** the constitution demonstrates Vue/Svelte and Nuxt/SvelteKit counterexamples without framework-specific fields in the generic identity.
- Abort if the inventory disproves one cohesive project authority or requires semantic indexing inside the resolver.
- Verify with strict DAG validation, docs build, and `architecture-3`; production LOC is zero.

PM1–PM4 consume this contract. The implementation ledger remains the only completion fact.
