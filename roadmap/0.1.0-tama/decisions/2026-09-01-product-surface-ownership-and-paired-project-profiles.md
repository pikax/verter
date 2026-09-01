# Product-surface ownership and paired project profiles

## Decision

Promote four previously implicit or portfolio-only areas into the active DAG:

- `expansion.project-model` (`PM0`–`PM4`);
- `expansion.source-modules` (`SM0`–`SM3`);
- `expansion.bundler-host` (`BND0`–`BND5`);
- `expansion.project-profiles` (`PPR0`, `NUX0`–`NUX3`, `SKT0`–`SKT3`, `PPR1`).

Nuxt and SvelteKit are co-equal first project-profile implementations. The earlier portfolio choice of Next as first candidate is superseded; Next remains deferred behind full React/MDX prerequisites and a future production charter.

## JavaScript boundary ruling

Verter may and does contain JavaScript/TypeScript product code in `packages/*`. `@verter/unplugin`, Nuxt/SvelteKit host adapters, editor packages, and other package integrations may execute their host hooks and capture typed ecosystem configuration. The restriction is narrower: Rust/WASM semantic demand does not execute arbitrary user ecosystem configuration, and JavaScript integrations may not become duplicate compiler, project, source-module, or framework semantic authorities.

## Flow and compiler-train reconciliation

This amendment was designed against the current `dag_mods` authority plus `origin/train/rev11-flow`. The Flow train adds `D2C` (flow-return audit partiality projection) and `D2D` (typed resolution outcome for every surface producer), changes `D2B`/`D3R` edges, and keeps those concerns inside `rev11.flow`. The new product trains depend on stable successor/compiler convergence boundaries; after the Flow train lands, `D2C`/`D2D` are transitive ancestors. They are intentionally not copied into this branch.

The incoming branch and `dag_mods` are not a choose-one merge. Required post-merge authority is the union: `D2D → D2B → D2C → D3R` must be retained, and `D8` must retain `TE5` alongside `D4`–`D7`. The root module list must retain both `rev11-type-algebra.toml` and `rev11-type-evaluation.toml` plus the four new expansion modules. The implementation ledger remains schema 2; incoming implemented/pending facts must be translated into its `[implementation]` table rather than replacing it with the older schema-1 `[[implemented]]` representation. Strict DAG and ledger validation after that reconciliation is a merge gate.

The compiler bridge already migrates unplugin request construction (`CCA1O4`) and delegates bundler/HMR/virtual-module policy downstream (`CMP4`). `BND0` starts only after that compiler boundary plus project/source-module convergence, preventing duplicate compiler authority while finally giving the installed build product an explicit terminal.

## Current-product language-service closure

Project-profile completion depends on `LSO10`, not merely the language-service transport cutover. `LSO0`, `LSO9`, and `LSO10` now require a complete inventory of shipped language-service operations and distinguish required current-product obligations from optional or explicitly retired surfaces. A required operation cannot be discharged as externally owned, unsupported, or residual: it must have a canonical proof-backed owner, or an earlier charter must remove and unadvertise it. This prevents Nuxt or SvelteKit completion from masking regressions in the existing Vue/Svelte editor product.

## Existing kernel identity ownership

`VID0` and `CAT0` remain the sole owners of orthogonal configured-project/project-profile identity types, exact-release law, and immutable catalog registration. PM derives membership, environments, resolution, and snapshots over those identities. PPR supplies Nuxt/SvelteKit project-role facts and profile snapshots using kernel catalog rows. Neither train mints a parallel `ProjectId`/`ProjectProfileId` family.

## Rejected collapses

- PM package/module resolution is not SM Vite query/asset semantics.
- SM static facts are not BND build/HMR execution.
- BND is not a compiler, bundler, or dev server.
- Project profiles do not own TypeScript projects or framework compiler semantics.
- Nuxt and SvelteKit do not share a framework-shaped implementation; only the PPR vocabulary and canonical lower services are shared.
