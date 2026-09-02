<!-- unified-charter-v2
id=PPR0
name=Framework-neutral project-profile constitution
predecessors=PM4,SM3,CFG0,IDX0,CAT0,VID0
phase=expansion
train=expansion.project-profiles
product=project_profiles
kind=contract
semantic_role=delivery
class=successor
owner=expansion.project-profiles:generic project-role vocabulary plus equal Nuxt and SvelteKit profile authorities
conflict_domains=project_profiles
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
charter=charters/expansion-project-profiles/PPR0.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# PPR0 — Framework-neutral project-profile constitution

## Independently acceptable outcome and owners

Define a project-profile overlay that classifies source-observable framework project roles without becoming a TypeScript project, compiler, bundler, or framework semantic authority. Nuxt and SvelteKit are co-selected first-class profiles and separate final owners; neither is a counterfixture or follow-on to the other.

## Surfaces, APIs, and predecessor contracts

Expected surfaces: `verter_protocol`, `verter_session`, `verter_lsp`, `packages/nuxt`, and future `packages/sveltekit`. PPR consumes kernel-owned `ProjectProfileId`, exact-release/epoch law, and catalog rows without mutating the identity owner; it owns `ProjectFileRole`, `ProjectRouteId`, `ExecutionRealm`, `GeneratedProjectFact`, `ProjectProfileSnapshot`, and the Nuxt/SvelteKit profile fact payloads. `PM4` supplies projects/resolution; `SM3` supplies static source-module facts; `CFG0` supplies captured config/read sets; `IDX0` supplies bounded candidates only; `CAT0`/`VID0` supply identity and catalog authority that PPR must not duplicate.

## Binding architecture and subblocks

1. Freeze generic concepts: file/route role, association, realm, generated fact, auto-import contribution, navigation edge, and applicability—not Next/Vue/Svelte-shaped fields.
2. Require exact framework release/profile epochs and typed JS-host capture for executable ecosystem config.
3. Define coexistence when Nuxt/SvelteKit are absent, nested, mixed, or explicitly disabled.
4. Freeze equal required product cells and the paired PPR1 promotion rule.

Generic caches bind project snapshot, profile epoch, captured-config/source-module generations, and relevant package/generated facts. PPR owns framework-defined `ProjectProfileResolutionContribution` payloads; PM remains the only resolver and SM remains the generic source-module fact owner. Rust/WASM never executes arbitrary project config; `packages/*` may capture it. Shared laws apply.

## Migration, deletions, forbidden designs, and acceptance

This contract changes no production code. Forbid using filename convention without project/profile proof, assuming every Vue project is Nuxt or every Svelte project is SvelteKit, making generated directories semantic identity, or promoting one profile while the other's required lane is absent.

- **PPR0-AC1:** Nuxt and SvelteKit adversarial schemas fit the generic vocabulary without framework-specific generic fields.
- **PPR0-AC2:** planted cross-profile role/realm reuse and stale generated-fact epoch are rejected.
- **PPR0-AC3:** base Vue/Svelte and mixed monorepo fixtures remain zero-work when profiles are inapplicable.
- Abort if the generic schema starts owning framework semantics or requires runtime config execution in core.
- Verify strict DAG validation, docs build, and `architecture-3`; production LOC is zero.

NUX0 and SKT0 may start in parallel. Ledger presence is completion.
