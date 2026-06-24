# tsgo `--api` capability gate

This is the **reproducible capability gate** for the external-TS-engine architecture
(`docs/arch/external-ts-engine-architecture.md`, "Block 0"). It proves that an
**off-disk generated TSX "carrier"** — served only through the tsgo `--api` FS-overlay
callbacks, **never written to disk** — becomes a real member of a configured TypeScript
project driven by the **user-installed** TS≥7 distribution, so the real `tsconfig.json`
(`paths`/`baseUrl`/`types`/`typeRoots`/`jsx`/project references) applies to it.

**Which package is "tsgo"?** The shipping TS≥7 distribution is the npm **`typescript`** package at
v7 (e.g. `typescript@7.0.1-rc`), whose per-platform native binaries are
`@typescript/typescript-{platform}-{arch}` and which exports the `--api` client at
`typescript/unstable/sync`. The `@typescript/native-preview` dev-preview channel (binaries
`@typescript/native-preview-{platform}-{arch}`) ships the **identical** `--api` surface.

**Engine selection (precise dist-tags).** The installed `typescript` package whose **major is ≥ 7**
wins **always** (regardless of the exact version/dist-tag); `@typescript/native-preview` is accepted
as a fallback **source** when no installed `typescript@>=7` is present. `run-gate.mjs` discovers an
installed/repo `typescript` (major ≥ 7) **first**, then falls back to `@typescript/native-preview`,
and drives whichever is installed (override with `TSGO_PATH`/`TS7_SOURCE`). The production
**no-TypeScript** fallback (not exercised by this gate) **downloads** the npm `typescript` package at
the **`rc`** dist-tag — the current TS7 channel (`npm view typescript@rc` → `7.0.1-rc` today; npm
`latest` is still the `6.x` line), retargeting `latest` once TS7 ships stable — and **fails closed
when offline**. It is download-only, never a bundled/forked binary.

It also serves as the **version-bump gate**: the tsgo `--api` surface is `unstable`/generated,
so re-run this whenever the installed tsgo version changes. A regression here means the tsgo
LSP backend must fail closed for TS≥7 on that build rather than ship a degraded path.

## Run

```bash
# discovers the user-installed tsgo from node_modules, runs all three scripts:
node tools/tsgo-api-gate/run-gate.mjs

# or drive a specific binary:
NM_BASE="$PWD" TSGO_PATH="<abs path to tsgo[.exe]>" node tools/tsgo-api-gate/harness.mjs
```

Exit 0 = GO. The runner gates on `harness.mjs` (all discriminating checks, GATE 1–4),
`control.mjs` part (A) (the negative control), **and** `companion-identity.mjs` (GATE 5 — the
production companion-identity proof). `incremental.mjs` tightens the incrementality proof.
`control.mjs` part (B) is informational (introspects a build-dependent private field).

## What it proves (mechanism — matches §2.3 of the design doc)

- **Membership rides the FS overlay + `open_project`**, NOT an inline `openFiles[].defaultProject`
  (the typescript-go #2824 _proposal_ shape is NOT what the shipped binary takes). The shipped
  `updateSnapshot` param is `{ openProject?, fileChanges?: { changed?[], created?[], deleted?[] } | { invalidateAll } }`.
- The sync client is imported through the **public package export** `<pkg>/unstable/sync`
  (`@typescript/native-preview/unstable/sync`, parameterized so it also drives
  `typescript/unstable/sync`) — resolved via `require.resolve` honouring the package `exports`
  map, NOT a hand-built internal `dist/` path — so the gate exercises the public consumer surface.
- `new API({ tsserverPath, cwd, fs: { readFile, fileExists, getAccessibleEntries, realpath, directoryExists } })`
  — the FS callbacks are the VFS overlay. `readFile` returns carrier content for the carrier
  path, `undefined` to fall through to real disk; `getAccessibleEntries` injects the off-disk
  carrier into its directory's enumeration so the `include` glob discovers it as a root file.
- `snapshot.getDefaultProjectForFile(carrier)` returns the real tsconfig (a queryable association).
- `project.program.getSemanticDiagnostics` / `project.checker.getTypeAtPosition` /
  `typeToString` / `getSymbolAtPosition` answer from the configured Program.
- Incremental edit via `updateSnapshot({ openProject, fileChanges: { changed:[carrier] } })`
  flips diagnostics on the **same stable project handle** with the unchanged dependency retained.

## Companion identity (PROVEN by GATE 5) and carrier discovery (informs Block 5)

**Companion identity.** The shipped `--api` has **no module-resolution-map endpoint**, so the path
Verter serves the carrier at IS the engine identity and tsgo only reaches it by **appending
`.tsx`/`.ts` to the full bare-import basename** (`import "./Comp.vue"` probes `Comp.vue.tsx`). GATE
5 (`companion-identity.mjs`) decided the production identity empirically:

- The component **IDE carrier** is `{name}.vue.tsx` / `{name}.svelte.tsx` — bare-import-probe-compatible
  and **collision-free** (Svelte rune modules are `*.svelte.ts` / `*.svelte.js`, never `.tsx`). GATE
  5 proves a `Widget.svelte.tsx` carrier and a real `state.svelte.ts` rune module coexist with
  correct types both ways.
- A **`.verter.` _component_ identity is REJECTED**: serving `Comp.vue.verter.tsx` does NOT satisfy
  `import "./Comp.vue"` (tsgo never probes a `.verter.` segment) → TS2307.
- The reserved `.verter.` infix is correct ONLY for the **redirect-reached `.ts` API carrier**
  (`{name}.vue.verter.ts` / `{name}.svelte.verter.ts`) — reached via project-reference redirect /
  cross-package `.d.ts`-equivalent, **never bare-probed**, so a reserved infix there avoids
  colliding with a real `*.svelte.ts` rune module (GATE 5 records tsgo probing `.svelte.ts` before
  `.svelte.tsx`).

**Carrier discovery.** The carrier extension here is `.tsx`, matched by an **explicit** `src/**/*.tsx`
glob in the fixture `tsconfig.json`. An extension-specific glob does **not** auto-expand to other
extensions (see the `include` docs), so a real Verter component carrier (`Foo.vue.tsx` /
`Foo.svelte.tsx`) is discoverable for the same reason — its final `.tsx` extension matches a
`.ts`/`.tsx` glob (or the default include). A bare `.vue` would NOT be matched by `src/**/*.ts`. The
bare `./X.vue` module-specifier redirection through the overlay is exercised by **GATE 4** (a bare
`"./Exported.vue"` import resolving to the overlay-served `Exported.vue.tsx` companion) and the
identity is settled by **GATE 5**. The residual for Block 5 is tsconfig-virtualization root-set
injection under a `.vue`-specific `include`/`files` config (see the design doc §2.3).

## Hermeticity

The `fixture/` TS project is fully committed and self-contained: `@spike/shared`, `verterjsx`,
and the ambient `verter-global-types` resolve via `tsconfig` `paths`/`typeRoots` into committed
`packages/` and `vendor/` dirs — **no `node_modules/`** in the fixture (the repo gitignores it).
The referenced package's declaration output lives in `packages/shared/lib/` (NOT `dist/`, which the
repo's root `.gitignore` excludes) so the whole fixture is committable intact — verified with
`git add -n tools/tsgo-api-gate/` listing all fixture files. The off-disk carrier is synthesized
in-memory by the harness and never written. `src/components/` is kept on disk via `.gitkeep` so the
overlay can inject the carrier into its enumeration. The only external dependency is the
user-installed TS≥7 distribution in the repo's own `node_modules` (the engine under test),
consistent with the no-fork/no-bundle policy.
