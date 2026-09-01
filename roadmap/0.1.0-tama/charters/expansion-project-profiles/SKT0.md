<!-- unified-charter-v2
id=SKT0
name=SvelteKit profile epoch and captured project facts
predecessors=PPR0,SCP7
phase=expansion
train=expansion.project-profiles
product=project_profiles
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.project-profiles:SvelteKit project-profile authority
conflict_domains=sveltekit_project_profile,svelte_product
resource_class=rust-mixed
gate_profile=canonical
review_profile=public-3
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
charter=charters/expansion-project-profiles/SKT0.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SKT0 — SvelteKit profile epoch and captured project facts

## Independently acceptable outcome and owners

Detect one exact admitted SvelteKit profile and capture its source/config/generated project facts with provenance/read sets. There is no current first-class SvelteKit owner; final owner is `SvelteKitProjectProfileSnapshot`, implemented through shared Rust services and an allowed JS package capture surface.

## Surfaces, APIs, and predecessor contracts

Expected surfaces: a new `packages/sveltekit` adapter if required, project-profile DTOs/services, SvelteKit fixtures, and bounded session integration. APIs: `SvelteKitProfileEpoch`, `SvelteKitProjectProfileSnapshot`, `SvelteKitCapturedConfig`, `SvelteKitGeneratedFacts`, `SvelteKitFactReadSet`. `PPR0` supplies generic identity/trust/coexistence; `SCP7` supplies the accepted Svelte compiler product without owning SvelteKit roles.

SvelteKit implementation lands in SvelteKit-specific adapter/registration modules. A required mutation to shared PPR vocabulary aborts this lane and reopens PPR0 rather than racing or silently extending the generic owner.

## Binding architecture and subblocks

1. Select exact SvelteKit/Svelte/Vite/adapter compatibility epochs from project/package evidence.
2. Capture kit config, routes root, aliases, generated `$types`, ambient/project types, adapter-visible static facts, and relevant read sets through the JS host where execution is required.
3. Translate captured results into versioned typed facts for Rust/WASM/LSP/MCP/CLI.
4. Publish complete/NeedInputs/unsupported/disabled outcomes with no plain-Svelte work.

Generated `.svelte-kit` artifacts are observations tied to source/config/package/profile epochs. Shared laws apply.

## Migration, deletions, forbidden designs, and acceptance

Introduce the package/profile without changing base Svelte behavior; migrate any SvelteKit heuristics and delete exact duplicates. Forbid core execution of `svelte.config`, importing adapters during semantic demand, `.svelte-kit` presence as sole identity, and silent version fallback.

- **SKT0-AC1:** routes root, aliases, generated types and exact applicability are deterministic.
- **SKT0-AC2:** planted stale generated facts, wrong kit/Svelte epoch, and missing read set fail closed.
- **SKT0-AC3:** config/package/generated edit-revert equals fresh; disabled/plain Svelte is zero work.
- **SKT0-AC4:** warm unchanged requests invoke no JS capture and perform no scan.
- Abort if route/load/action semantics enter (SKT1) or bundler execution enters BND.
- Verify SvelteKit capture/session fixtures, canonical gate, and `public-3`.

SKT1 consumes the snapshot. Ceiling: 800 LOC, 8 files, 2 packages; ledger presence records completion.
