<!-- unified-charter-v2
id=NUX0
name=Nuxt profile epoch and captured project facts
predecessors=PPR0,VCP7
phase=expansion
train=expansion.project-profiles
product=project_profiles
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.project-profiles:Nuxt project-profile authority
conflict_domains=nuxt_project_profile,vue_product
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
charter=charters/expansion-project-profiles/NUX0.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# NUX0 — Nuxt profile epoch and captured project facts

## Independently acceptable outcome and owners

Detect one exact admitted Nuxt 4 profile and capture its source/config/generated project facts with provenance and read sets. Current knowledge is scattered across `packages/nuxt`, aliases, generated `.nuxt` state, and LSP/project heuristics; final owner is `NuxtProjectProfileSnapshot`.

## Surfaces, APIs, and predecessor contracts

Expected surfaces: `packages/nuxt`, Nuxt capture adapters, project-profile DTOs/services, and fixtures. APIs: `NuxtProfileEpoch`, `NuxtProjectProfileSnapshot`, `NuxtCapturedConfig`, `NuxtGeneratedFacts`, `NuxtLayerId`, `NuxtFactReadSet`. `PPR0` supplies generic identity/trust/coexistence; `VCP7` supplies the accepted Vue compiler product without owning Nuxt roles.

Nuxt implementation lands in Nuxt-specific adapter/registration modules. A required mutation to shared PPR vocabulary aborts this lane and reopens PPR0 rather than racing or silently extending the generic owner.

## Binding architecture and subblocks

1. Select exact Nuxt/Nitro/module compatibility epochs from package/project evidence.
2. Capture declarative config, layers, source dirs, aliases, modules, generated type/import/component/route metadata through the JS package host where execution is required.
3. Translate captured results into versioned typed facts for Rust/WASM/LSP/MCP/CLI consumers.
4. Publish complete/NeedInputs/unsupported/disabled outcomes and precise config/package/generated read sets.

Generated files are observations tied to source config/package/profile epochs, never sole truth. Shared laws apply.

## Migration, deletions, forbidden designs, and acceptance

Migrate Nuxt detection/config/generated readers to the snapshot, then delete duplicated detection and alias parsing. Forbid core execution of `nuxt.config`, importing arbitrary Nuxt modules during semantic demand, treating `.nuxt` presence as sufficient identity, and silent version fallback.

- **NUX0-AC1:** layers/source dirs/modules/generated facts and exact profile applicability are deterministic.
- **NUX0-AC2:** planted stale generated dir, wrong Nuxt/Nitro epoch, and missing read set fail closed.
- **NUX0-AC3:** config/layer/package/generated edit-revert equals fresh; disabled/plain Vue is zero work.
- **NUX0-AC4:** warm unchanged queries invoke no JS capture and do no filesystem scan.
- Abort if route semantics enter (NUX1) or bundler execution enters (BND).
- Verify Nuxt package/capture/session fixtures, canonical gate, and `public-3`.

NUX1 consumes the snapshot. Ceiling: 800 LOC, 8 files, 2 packages; ledger presence records completion.
