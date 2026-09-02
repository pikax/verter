<!-- unified-charter-v2
id=SKT2
name=SvelteKit generated types, aliases, and product integration
predecessors=SKT1,LSO3,LSO7,LSO8,BND5
phase=expansion
train=expansion.project-profiles
product=project_profiles
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.project-profiles:SvelteKit project-profile authority
conflict_domains=sveltekit_project_profile,lsp_publication,bundler_host
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
charter=charters/expansion-project-profiles/SKT2.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SKT2 — SvelteKit generated types, aliases, and product integration

## Independently acceptable outcome and owners

Integrate generated `$types`, `$app/*`/`$env/*` and configured aliases, route-param/data/action/navigation relations, file moves, diagnostics, and build-host profile selection through canonical services. Final owner remains SvelteKit; LSO/BND materialize operations/artifacts only.

## Surfaces, APIs, and predecessor contracts

Expected surfaces: `packages/sveltekit`, session/LSP/protocol profile adapters, TypeInfo/index contributions, unplugin selection, and fixtures. APIs: `SvelteKitGeneratedTypeContribution`, `SvelteKitAliasContribution`, `ProjectProfileResolutionContribution`, `SvelteKitRouteDataFact`, `SvelteKitNavigationFact`, `SvelteKitMoveIntent`. `SKT1` supplies roles/routes/realms; `LSO3` supplies canonical definition/type-definition/implementation navigation; `LSO7` supplies hover/presentation composition; `LSO8` supplies authored edit transactions and carries references/rename/completion ancestry; `BND5` supplies the converged JS bundler host. SvelteKit owns the meaning and provenance of its alias/generated-module contribution; PM remains the sole resolver and SM owns only generic host/source-module facts.

## Binding architecture and subblocks

1. Translate generated route/data/action types, aliases and environment modules into provenance-bearing contributions.
2. Route completion/hover/definition/references/rename/diagnostics through canonical LSO services.
3. Plan route/file moves and let LSO8 validate/materialize atomic authored edits.
4. Select SvelteKit compiler/build profiles in the JS package/unplugin without duplicating compiler/HMR logic.

Resolve keys bind profile/project/route/source/generated/capability epochs. `$env/static/*` and `$env/dynamic/*` remain realm- and provenance-distinct. Shared laws apply.

## Migration, deletions, forbidden designs, and acceptance

Migrate each operation/build consumer, then delete direct `.svelte-kit` scraping, alias/env heuristics, and raw edit builders. Forbid generated text parsing when typed capture exists, secret value publication, direct editor edits, SvelteKit-specific LSO forks, bundler-side TypeInfo truth, and a SvelteKit-local resolver bypassing PM.

- **SKT2-AC1:** generated types/aliases/env/data/action/navigation/move/build cells pass across route/realm variants.
- **SKT2-AC2:** planted stale generated epoch, wrong realm, secret exposure, raw edit overlap, or profile mismatch fails.
- **SKT2-AC3:** generated/config/source edit-revert equals fresh and withdrawal clears stale results.
- **SKT2-AC4:** warm IDE/build avoids duplicate capture/resolve/compile; disabled Kit is zero work.
- Abort if a missing generic LSO/BND semantic must be implemented here.
- Verify SvelteKit package/LSP/move/TypeInfo/unplugin/E2E suites, canonical gate, and `architecture-3`.

SKT3 consumes the integrated product. Ceiling: 800 LOC, 8 files, 2 packages; ledger presence records completion.
