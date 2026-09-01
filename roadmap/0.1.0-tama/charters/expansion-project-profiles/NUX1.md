<!-- unified-charter-v2
id=NUX1
name=Nuxt routes, roles, and client-server realms
predecessors=NUX0
phase=expansion
train=expansion.project-profiles
product=project_profiles
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.project-profiles:Nuxt project-profile authority
conflict_domains=nuxt_project_profile
resource_class=rust-mixed
gate_profile=canonical
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
charter=charters/expansion-project-profiles/NUX1.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# NUX1 — Nuxt routes, roles, and client-server realms

## Independently acceptable outcome and owners

Own Nuxt pages, layouts, middleware, plugins, modules, server/API routes, app/server components, and client/server/shared file-role and route relations. Vue compiler semantics remain VCP-owned; PM owns file/project resolution.

## Surfaces, APIs, and predecessor contract

Expected surfaces: project-profile semantic service, indexes/contributions, diagnostics, and Nuxt fixtures. APIs: `NuxtFileRole`, `NuxtRouteId`, `NuxtRouteSegment`, `NuxtExecutionRealm`, `NuxtAssociation`, `NuxtRoleDiagnostic`. `NUX0` supplies exact profile/config/layer/generated facts and applicability.

## Binding architecture and subblocks

1. Classify source files by layer/root/convention with explicit ambiguity and precedence.
2. Construct route/layout/middleware/plugin/server associations and stable route IDs.
3. Compute client/server/universal realms and forbidden/ambiguous crossings without reimplementing TypeScript.
4. Publish bounded index contributions, diagnostics, and navigation facts with authored provenance.

Identity includes project/profile/layer/root/route/role and source generation. Rename/add/remove invalidates only affected associations. Shared laws apply.

## Migration, deletions, forbidden designs, and acceptance

Migrate Nuxt-aware route/role heuristics, then delete exact duplicates. Forbid path-only classification outside an admitted profile, assuming runtime routing not evidenced by source/captured facts, making routes TypeScript modules, and using Vue semantics to infer Nuxt realm.

- **NUX1-AC1:** nested/dynamic/catch-all routes, layouts, middleware, plugins, server/API and layer precedence matrix passes.
- **NUX1-AC2:** planted wrong realm/route precedence/stale layer fails.
- **NUX1-AC3:** add/remove/rename/move/edit/revert equals fresh and clears stale relations.
- **NUX1-AC4:** lookups are bounded by affected project/profile contributions; plain Vue is zero work.
- Abort if auto-import/generated-type integration enters (NUX2).
- Verify Nuxt semantic/index/diagnostic fixtures, canonical gate, and `semantic-3`.

NUX2 consumes roles/relations. Ceiling: 800 LOC, 8 files, 2 packages; ledger presence records completion.
