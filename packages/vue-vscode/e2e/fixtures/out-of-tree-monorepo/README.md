# `out-of-tree-monorepo` — the extension-hosted provider's acceptance workspace

Exercised by `e2e/suite/out-of-tree-monorepo.test.ts` on the `extension` route
(`out-of-tree-monorepo@extension` in `e2e/lib/routeInventory.ts`).

## What it proves

That the extension-hosted TypeScript provider serves a carrier from **the
TypeScript the owning package installed**, selected through the project the LSP
declared — not through the workspace folder the file happens to sit under.

The layout is the assertion:

```
<root>/                     package.json, NO tsconfig, NO node_modules
  packages/app/             tsconfig.json ("strict") + its own node_modules/typescript
    src/App.vue             the carrier under test
```

The only configured project is `packages/app`, and the only TypeScript anywhere
is the one `packages/app` installs. A provider that declares the **owning
configured project** resolves that compiler and serves: a typed hover, and the
`TS2322` its own `strict` config produces. A provider that declares the
**workspace folder** resolves nothing and — under the fail-closed contract —
refuses.

## Why it is materialized OUTSIDE the repository

This is load-bearing, not incidental. The extension host resolves a project's
compiler with `createRequire` anchored at the **declared** project root, and
Node's resolution walks ancestors.

Any fixture that stays under `packages/vue-vscode/e2e/fixtures/*` therefore
reaches **this repository's own `node_modules/typescript`** from _any_ root
inside the tree — including the wrongly-declared workspace-folder root. Such a
fixture resolves a compiler either way and passes identically against the correct
producer and the broken one, so it discriminates nothing.

`OUT_OF_TREE_FIXTURES` in `e2e/runTests.ts` copies this directory (minus
`node_modules`) into an OS temp directory and launches the editor there. The
ancestor chain then ends at the filesystem root with no TypeScript above it, so
only a correctly-declared nested package can serve. `installFixtureDeps` runs
inside `packages/*` only — the root staying TypeScript-less is the condition
under test, and the suite asserts it before anything else.

## Current status

The suite skips at `suiteSetup` and reports its tests as pending. Carrier
publication is suppressed for `TypeProviderKind::Tsserver`, and the
extension-hosted service registers under that kind, so no `.vue.tsx` companion is
ever opened to the extension host and every carrier query arrives for a file the
registry has no binding for. The fixture, the route, the out-of-tree
materialization and the assertions are all intact and needed: connect carrier
publication for the extension-hosted topology, delete the skip, and this
workspace proves the fix.

Note what the route-level oracle does with that: `enforceRunSummary`
(`src/runSummaryOracle.ts`) refuses **any** pending test ID in a required run, so
this route still reports a runner failure — now naming the three pending IDs
rather than timing out in shared warmup. That refusal is the harness's
prove-execution contract and is deliberately left alone here; how a
known-blocked acceptance is declared to it is a separate decision.
