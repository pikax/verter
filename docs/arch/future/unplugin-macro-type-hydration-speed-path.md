# Unplugin macro-type-hydration drop-in speed path (deferred plan)

> **Status:** DEFERRED — future plan (recorded 2026-07-20). Intent reference: commit `82ea84ce` (`perf(unplugin): drop-in fixture speed path and Vue runtime parity`, on `codex/release-clean-tsc-performance`) plus the newer uncommitted working-tree WIP on that same branch. Deferred (not landed on release/clean-review) because it carries a **perf-regression risk**, a **VueMacros plugin-shape conflict**, and is **WIP-dependent** — it must land as ONE coherent TDD package, not piecemeal.

## Goal

Cut unplugin transform-time cost by (1) narrowing macro-type hydration to **type-relevant edges** and (2) **stopping speculative value-import bulk upserts** on every transform.

## Already on release/clean-review (do NOT re-do)

The landed tip already has: the demand gate (`if (!analysis.macroTypeDeps?.length) return`, `packages/unplugin/src/core/macro-type-hydration.ts`), the per-host hydrated memo + `evictHydratedPath`, the async `ws.readDir`/`readFile` scanner, and the `forceJs` auto rule (`index.ts`, `!viteConfig || meta.framework !== "vite"`).

## The work (all five as ONE package)

1. **Type-relevant EDGE filtering** in the dependency closure. `hydrateDependencyClosure` (`macro-type-hydration.ts`) currently walks ALL imports + reexports of every dep transitively; the monorepo fan-out that `82ea` cut is still live. Filter to type-relevant edges only.
2. **Stop speculative value-import bulk upserts.** `resolveUpsertDependencies` (`index.ts`) still bulk-reads + upserts every module reference per transform AND calls `setImportDependencies` itself while hydration writes routes separately. Change it to RETURN routes + do a single merged `setImportDependencies`.
3. **vite `createTypeOnlyResolveGuard`** pre-plugin (type-only resolve guard).
4. **User-facing `forceJs` option** + tsdown `fromVite` detection.
5. **Shared resolve/analysis/inflight caches** + `clearMacroTypeHydrationCache`.

## Perf-regression risk (the flagged stream risk)

Skipping the speculative value upserts (item 2) can surface `HOST_MISSING_*` errors that the bulk upserts were masking. The type-relevant closure (item 1) is the compensation. **Land (1)+(2) together — never (2) alone.** Measure before/after: unplugin vitest suite, `scripts/integration-test` Tier-1 reka-ui wall-time, and the component-meta compare.

## Known conflict to resolve first

`82ea` returns `[guard, mainPlugin]` (an array) from `vite.ts`; the landed tip's docstring explicitly pins a **single** `Plugin` return because returning an array "broke VueMacros." Before adopting the array shape, verify VueMacros' plugin flattening — otherwise attach the `resolveId` guard by a different mechanism.

## TDD oracle + source-of-truth

`82ea`'s `packages/unplugin/src/index.spec.ts` (+445) is the test oracle — its cases port nearly as-is ("type-only resolve guard", "shouldForceJs/tsdown", "nested path-alias heritage", "empty interface extends imported heritage"). Per the maintainer's "check who has the better implementation" directive, when this is picked up, diff the newer **uncommitted WIP** on `codex/release-clean-tsc-performance` against `82ea` per file and take the better.

## Measurement prerequisite (NOT landed here)

The vize↔verter benchmark/compare tooling (`scripts/vize-fixture-compare/*`, `scripts/integration-test/component-meta-compare.mjs`, the `verter_bench` `vize_fixture_corpus` example) is the harness needed to prove the perf delta. Per maintainer direction it is **not** being landed on release/clean-review; obtain/run it from the `codex/release-clean-tsc-performance` branch when doing this work.
