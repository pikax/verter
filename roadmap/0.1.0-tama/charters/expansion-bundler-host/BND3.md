<!-- unified-charter-v2
id=BND3
name=Client-SSR build graphs and HMR invalidation semantics
predecessors=BND1,BND2
phase=expansion
train=expansion.bundler-host
product=bundler_host
kind=implementation
semantic_role=delivery
class=successor
owner=expansion.bundler-host:unplugin lifecycle, virtual-module, build-graph, preprocessing, and HMR authority
conflict_domains=bundler_host,source_lineage,semantic_cache_store
resource_class=ts-heavy
gate_profile=canonical
review_profile=concurrency-3
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
charter=charters/expansion-bundler-host/BND3.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# BND3 — Client-SSR build graphs and HMR invalidation semantics

## Independently acceptable outcome and owners

Own graph-qualified invalidation and hot-update planning for main, script, template, style, custom block, dependency, client, and SSR artifacts. Current behavior is tool/framework-hook-local; final owner is `HotUpdatePlanner`, while the bundler owns module transport and runtime application.

## Surfaces, APIs, and predecessor contracts

Expected surfaces: unplugin cache/dependency/HMR modules and adapter hooks. APIs: `BundlerGraphKind`, `ArtifactDependencyGraph`, `HotUpdateCause`, `HotUpdatePlan`, `HotAcceptDisposition`, `ReloadDisposition`, `StatePreservationClaim`. `BND1` supplies graph/module/session identity; `BND2` supplies qualified artifacts, maps, and processed dependencies.

## Binding architecture and subblocks

1. Build explicit artifact/dependency edges from compiler and processed-content facts, not regex import scans.
2. Classify script/template/style/custom/external/dependency edits separately for Vue and Svelte.
3. Produce invalidate/accept/reload plans per adapter capability and client/SSR graph, with no runtime transport emulation in core.
4. Serialize update generations, cancel superseded work, and clear stale modules/diagnostics on removal or framework-capability, source-module-environment, or compile-intent change.

State-preservation is an evidenced claim, not a default; unsupported preservation escalates to reload. Cache/publication bases include graph kind and HMR generation. Shared laws apply.

## Migration, deletions, forbidden designs, and acceptance

Characterize current update behavior, shadow plan only, switch each adapter atomically, then delete heuristic invalidators. Forbid universal “invalidate everything,” cross-client/SSR reuse, framework-neutral state assumptions, timestamp identity, and acceptance after incomplete compile.

- **BND3-AC1:** Vue/Svelte style-only/template-only/script/custom/dependency/remove/rename matrix yields exact invalidate/accept/reload plans.
- **BND3-AC2:** planted missing graph/framework/dependency edge produces a failing negative control.
- **BND3-AC3:** burst edit/cancel/revert/restart sequences equal a fresh build graph and publish no stale hot update.
- **BND3-AC4:** affected-subgraph work is bounded; warm no-change is zero compile/invalidation work and churn plateaus.
- Abort if runtime patch code or framework compiler semantics would enter this node.
- Verify HMR graph/unit/E2E state tests, canonical gate, and `concurrency-3`.

BND4 consumes the planner. Ceiling: 800 LOC, 8 files, 2 packages; ledger presence records completion.
