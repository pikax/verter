<!-- unified-charter-v2
id=BND4
name=Seven-bundler Vue-Svelte conformance matrix
predecessors=BND2,BND3,CMP6
phase=expansion
train=expansion.bundler-host
product=bundler_host
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.bundler-host:unplugin lifecycle, virtual-module, build-graph, preprocessing, and HMR authority
conflict_domains=bundler_host,performance_evidence,vue_product,svelte_product
resource_class=ts-heavy
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
charter=charters/expansion-bundler-host/BND4.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# BND4 — Seven-bundler Vue-Svelte conformance matrix

## Independently acceptable outcome and owners

Make Vite, Rollup, webpack, esbuild, Rspack, Rolldown, and Farm adapters truthfully implement a versioned capability matrix for both Vue and Svelte. Current wrapper exports overstate support where hook/HMR/style behavior differs; final owner is the adapter matrix plus thin translations to BND1–BND3.

## Surfaces, APIs, and predecessor contracts

Expected surfaces include all `packages/unplugin/src/{vite,rollup,webpack,esbuild,rspack,rolldown,farm}.ts` entries, package exports, fixtures, and E2E runner. APIs: `BundlerAdapterCapability`, `BundlerAdapterEpoch`, `BundlerHookTranslation`, `BundlerConformanceReceipt`. `BND2` supplies build hooks and processed handoffs; `BND3` supplies HMR planning; `CMP6` supplies accepted Vue/Svelte compiler output foundations.

## Binding architecture and subblocks

1. Freeze exact required/supported/inapplicable hook, dev/build, HMR, SSR/client, virtual/style/custom, maps, diagnostics, watch, and cancellation cells per tool/version. `Unsupported` is a failing state for every applicable required cell.
2. Implement missing adapter translations without forking common semantics.
3. Build hermetic Vue and Svelte fixtures for every applicable cell and explicit negative fixtures for unsupported cells.
4. Measure equivalent work, startup/first/warm/incremental latency, allocation, process handles, and retained memory.

No cell inherits support from unplugin generation or another adapter. Shared laws apply.

## Migration, deletions, forbidden designs, and acceptance

Replace manual support claims with generated matrix/exports, then delete adapter-local semantic branches. Forbid empty wrappers as proof, Vite-only fixtures, Vue-only product claims, network-dependent tests, version ranges without epochs, and sampled HMR checks.

- **BND4-AC1:** all seven adapter/version rows pass every applicable required Vue and Svelte cell. A cell may be inapplicable only with pinned-host negative proof; unsupported applicable cells fail the node.
- **BND4-AC2:** planted no-op transform/HMR/source-map adapter fails the corresponding exact cell.
- **BND4-AC3:** dev/build incremental/fresh, restart, and cancellation matrices are deterministic.
- **BND4-AC4:** no adapter duplicates compiler work; watcher/process/RSS churn plateaus.
- Abort if an adapter requires a separate public option or lifecycle authority not chartered in BND0, or if any applicable required cell remains unsupported.
- Verify the hermetic seven-bundler E2E matrix, canonical gate, and `architecture-3`.

BND5 consumes the receipts. Ceiling: 800 LOC, 8 files, 2 packages; ledger presence records completion.
