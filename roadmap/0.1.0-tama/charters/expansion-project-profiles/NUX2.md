<!-- unified-charter-v2
id=NUX2
name=Nuxt imports, aliases, generated types, and product integration
predecessors=NUX1,LSO3,LSO7,LSO8,BND5
phase=expansion
train=expansion.project-profiles
product=project_profiles
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.project-profiles:Nuxt project-profile authority
conflict_domains=nuxt_project_profile,lsp_publication,bundler_host
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
charter=charters/expansion-project-profiles/NUX2.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# NUX2 — Nuxt imports, aliases, generated types, and product integration

## Independently acceptable outcome and owners

Integrate Nuxt auto-imported composables/components, `#imports`/`#components` and Nuxt aliases, generated types, route navigation/references, file moves, diagnostics, and build-host profile selection through canonical services. Final owner remains the Nuxt profile; LSO/BND only materialize operations/artifacts.

## Surfaces, APIs, and predecessor contracts

Expected surfaces: `packages/nuxt`, project-profile adapters in session/LSP/protocol, TypeInfo/index contributions, unplugin selection, and fixtures. APIs: `NuxtAutoImportContribution`, `NuxtAliasContribution`, `ProjectProfileResolutionContribution`, `NuxtGeneratedTypeContribution`, `NuxtNavigationFact`, `NuxtMoveIntent`. `NUX1` supplies roles/routes/realms; `LSO3` supplies canonical definition/type-definition/implementation navigation; `LSO7` supplies hover/presentation composition; `LSO8` supplies authored edit transactions and carries references/rename/completion ancestry; `BND5` supplies the converged JS bundler host. Nuxt owns the meaning and provenance of its alias/generated-module contribution; PM remains the sole resolver and SM owns only generic host/source-module facts.

## Binding architecture and subblocks

1. Translate captured/generated imports/components/types into provenance-bearing contributions; generated output is never semantic authority by itself.
2. Route completion/hover/definition/references/rename/diagnostics through canonical LSO target/occurrence/edit services.
3. Plan route/file moves as intents and let LSO8 validate/materialize atomic authored edits.
4. Select Nuxt build/compiler profiles in `packages/nuxt`/unplugin without duplicating compiler or HMR logic.

All resolve keys bind profile/project/source/generated/capability epochs. Disabled/inapplicable profiles publish capability withdrawal and clear stale results. Shared laws apply.

## Migration, deletions, forbidden designs, and acceptance

Migrate each operation/build consumer by matrix row, then delete direct `.nuxt` scraping, alias heuristics, and raw edit builders. Forbid generated text parsing when typed capture exists, direct editor edits, Nuxt-specific LSO forks, bundler-side TypeInfo truth, and a Nuxt-local resolver bypassing PM.

- **NUX2-AC1:** imports/components/aliases/generated types/navigation/moves/build cells work across layers and client/server realms.
- **NUX2-AC2:** planted stale generated epoch, wrong route target, raw edit overlap, or compiler-profile mismatch fails.
- **NUX2-AC3:** generated/config/source edit-revert equals fresh; withdrawal clears stale capabilities/results.
- **NUX2-AC4:** warm IDE/build requests avoid duplicate capture/resolve/compile; disabled Nuxt is zero work.
- Abort if a missing generic LSO/BND semantic must be implemented here.
- Verify Nuxt package, LSP, move, TypeInfo, unplugin and E2E suites, canonical gate, and `architecture-3`.

NUX3 consumes the integrated product. Ceiling: 800 LOC, 8 files, 2 packages; ledger presence records completion.
