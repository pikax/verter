<!-- unified-charter-v2
id=SKT1
name=SvelteKit routes, load/actions/hooks, and execution realms
predecessors=SKT0
phase=expansion
train=expansion.project-profiles
product=project_profiles
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.project-profiles:SvelteKit project-profile authority
conflict_domains=sveltekit_project_profile
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
charter=charters/expansion-project-profiles/SKT1.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SKT1 — SvelteKit routes, load/actions/hooks, and execution realms

## Independently acceptable outcome and owners

Own SvelteKit route groups/params/layouts/pages/endpoints, universal/server load, actions, hooks, params matchers, service-worker and client/server/universal file roles and associations. Svelte compiler semantics remain SCP-owned; PM owns file/project resolution.

## Surfaces, APIs, and predecessor contract

Expected surfaces: project-profile semantic/index/diagnostic services and SvelteKit fixtures. APIs: `SvelteKitFileRole`, `SvelteKitRouteId`, `SvelteKitRouteSegment`, `SvelteKitExecutionRealm`, `SvelteKitAssociation`, `SvelteKitRoleDiagnostic`. `SKT0` supplies exact profile/config/generated facts and applicability.

## Binding architecture and subblocks

1. Classify `+page`, `+layout`, `+server`, error, hooks, params, service-worker and related modules with exact precedence/ambiguity.
2. Construct route/layout/parent/endpoint/action/load associations and stable route IDs, including groups and optional/rest params.
3. Compute server/client/universal realms and illegal/ambiguous crossings from source-observable facts.
4. Publish bounded index contributions, diagnostics, and navigation facts with authored provenance.

Identity binds profile/project/routes-root/route/role/realm and source generation. Add/remove/rename invalidates only affected route branches. Shared laws apply.

## Migration, deletions, forbidden designs, and acceptance

Migrate any route/role heuristics and delete exact duplicates. Forbid classification outside an admitted profile, runtime behavior guesses, conflating `+page.svelte` with `+page.ts`, and using Svelte semantics to infer SvelteKit realm.

- **SKT1-AC1:** groups/params/rest/layout/page/error/endpoint/load/action/hooks/service-worker matrix passes.
- **SKT1-AC2:** planted wrong parent, realm, precedence, or stale routes-root fails.
- **SKT1-AC3:** add/remove/rename/move/edit/revert equals fresh and clears stale relations.
- **SKT1-AC4:** affected-route work is bounded; plain Svelte is zero work.
- Abort if generated-type/IDE/build integration enters (SKT2).
- Verify SvelteKit semantic/index/diagnostic fixtures, canonical gate, and `semantic-3`.

SKT2 consumes roles/relations. Ceiling: 800 LOC, 8 files, 2 packages; ledger presence records completion.
