<!-- unified-charter-v2
id=PM2
name=Canonical package and module resolution
predecessors=PM1,C1
phase=expansion
train=expansion.project-model
product=project_model
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.project-model:project membership, environment, resolution, and immutable snapshot authority over kernel-owned identities
conflict_domains=project_model,semantic_authority
resource_class=rust-mixed
gate_profile=targeted-domain
review_profile=semantic-3
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
charter=charters/expansion-project-model/PM2.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# PM2 — Canonical package and module resolution

## Independently acceptable outcome and owners

Make one project-bound resolver answer Node/TypeScript modes, paths, project references, package `exports`/`imports`, conditions, extensions, symlinks, casing, and preferred specifiers with a replayable proof. Current authority is split between `ModuleResolverCore` and consumer-local adapters; final authority is the PM resolver service.

## Surfaces, APIs, and predecessor contracts

Expected surfaces are `verter_session::resolver_core`, `verter_workspace`, protocol resolution DTOs, and exact adapters. APIs: `ModuleResolutionRequest`, `ModuleResolutionResult`, `ModuleResolutionProof`, `ResolutionMode`, `ResolutionContributionSet`, `PackageFactReadSet`, and `PreferredSpecifier`. `PM1` supplies project/environment identity; `C1` supplies the historical resolver algorithms and behavior to converge, not a second live owner. Later SM/PPR trains may supply versioned alias/generated-module contributions through the opaque typed contribution set; PM validates and resolves them without owning their host/framework meaning.

## Binding architecture and subblocks

1. Bind every request to source, project environment, resolution mode, conditions, and canonical specifier bytes.
2. Produce ordered probe/package/config read sets and explicit negative facts.
3. Model project references, workspaces, package boundaries, exports/imports, and type/value conditions.
4. Migrate semantic, checker, LSP, compiler, and provider consumers through typed adapters.

Resolution proof identity includes all probes, the ordered contribution-set identity, and package/config generations. Negative results cache only when complete. Source-module queries such as `?raw` remain SM ownership, and framework-generated aliases/modules remain PPR contribution ownership. See the shared expansion contract.

## Migration, deletions, and forbidden designs

Characterize current answers, dual-run only in non-authoritative measurement, switch atomically, then delete consumer-local path/alias/package resolvers. Forbid index-as-resolver, provider-only truth, regex package parsing, fallback to cwd/current file, treating unsupported conditions as not-found, and interpreting SM/PPR contribution payloads inside PM.

## Acceptance, performance, abort, verification, and consumers

- **PM2-AC1:** exact Node/TS mode, exports/imports, workspace, reference, symlink, casing, and preferred-specifier matrix passes.
- **PM2-AC2:** planted omitted condition or package read invalidates proof validation.
- **PM2-AC3:** package/config edit and revert equal a fresh resolver; stale negatives never warm.
- **PM2-AC4:** unchanged warm resolution performs no filesystem work beyond validated negative/read-set reuse.
- Abort if Vite query/asset semantics enter (SM train) or if a checker/private consumer requires its own resolver.
- Verify resolver/session/workspace/provider suites and `semantic-3`.

PM3 and SM0 consume the proof boundary. Ceiling: 800 LOC, 8 files, 2 packages; ledger presence records completion.
