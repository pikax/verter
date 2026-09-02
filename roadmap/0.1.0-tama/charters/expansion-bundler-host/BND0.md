<!-- unified-charter-v2
id=BND0
name=Bundler-host and HMR product constitution
predecessors=CMP4,PM4,SM3
phase=expansion
train=expansion.bundler-host
product=bundler_host
kind=contract
semantic_role=delivery
class=successor
owner=expansion.bundler-host:unplugin lifecycle, virtual-module, build-graph, preprocessing, and HMR authority
conflict_domains=bundler_host
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
charter=charters/expansion-bundler-host/BND0.md
size=M
max_production_loc=0
max_production_files=0
max_related_packages=0
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# BND0 — Bundler-host and HMR product constitution

## Independently acceptable outcome and owners

Establish `@verter/unplugin` and its JavaScript adapters as Verter's sole bundler-host product authority. Current ownership is split between generic unplugin hooks, compiler request glue, framework-specific virtual modules, caches, preprocessors, and ad hoc HMR behavior. Final ownership is BND1–BND5. Verter does not become a bundler/dev server; it owns the semantics its plugin presents to them.

## Surfaces, APIs, and predecessor contracts

Expected surfaces are `packages/unplugin/src`, its seven adapter exports and E2E matrix, plus typed compiler/session boundaries. APIs consume kernel-registered opaque bundler session/module identities and define `BundlerGraphKind`, `BundlerCompileIntent`, `ProcessedContentEnvelope`, `HotUpdatePlan`, and `BundlerArtifactPublication`. `CMP4` supplies segmented qualified artifacts and explicitly delegates host integration; `PM4` supplies canonical project snapshots; `SM3` supplies static source-module facts and does not execute bundler work. Missing identity registration aborts for a VID0/CAT0 amendment rather than expanding BND into an identity owner.

## Binding architecture and subblocks

1. Freeze lifecycle/hook ordering, virtual-ID grammar, client/SSR/dev/prod graph identity, and typed outcomes.
2. Freeze processed-content and source-map handoffs for Vite/Rollup/Webpack/Rspack/esbuild/Rolldown/Farm.
3. Define HMR invalidation, acceptance, reload, and state-preservation responsibilities per framework and adapter capability.
4. Inventory all current plugin routes and assign BND1–BND5 owners/deletions.

The required all-adapter baseline is Vue and Svelte resolve/load/transform, virtual modules, styles/custom blocks, diagnostics, composed source maps, client and SSR builds, cancellation, and deterministic close. Watch/HMR/dev-server cells are additionally required wherever the pinned host exposes the corresponding lifecycle. Host-inapplicability requires a versioned negative proof; an unsupported applicable cell cannot satisfy BND4 or BND5.

JavaScript/TypeScript is an intended implementation surface here and in other `packages/*`. Rust/WASM remains the canonical compiler/semantic substrate and never executes arbitrary bundler configuration. BND may accept an opaque typed compile-intent identity from a later project profile, but it may only key and transport that identity; it may not interpret profile semantics or require project-profile convergence. Common laws are in `contracts/product-surface-expansion.md`.

The capability schema records cold, first-warm, repeated-warm, edit/revert, restart, client/SSR switch, cancellation, disabled/inapplicable, allocation, watcher/process-handle, and retained-memory evidence. A zero-production contract node defines those performance cells but does not manufacture measurements.

## Migration, deletions, forbidden designs, and acceptance

This contract changes no production code and deletes nothing. It forbids framework-blind HMR, filename-only virtual IDs, bundler objects as cache identity, unqualified processed CSS, plugin-side semantic compilation, bundler-specific compiler forks, and claiming support from an exported wrapper alone.

- **BND0-AC1:** every live resolve/load/transform/build/HMR/cache/preprocess route maps to exactly one BND node.
- **BND0-AC2:** planted graph-kind loss, stale project snapshot, and virtual-ID alias are rejected.
- **BND0-AC3:** Vue and Svelte requirements are co-equal in every applicable capability row.
- Abort if an adapter needs a distinct semantic product rather than a host translation.
- Verify strict DAG validation, docs build, and `architecture-3`; production LOC is zero.

BND1–BND5 consume this constitution. Ledger presence is completion.
