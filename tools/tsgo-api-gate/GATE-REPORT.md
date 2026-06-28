# Block 0 Gate Report — tsgo `--api` off-disk carrier capability

**VERDICT: GO.** Reproduced from this repo via `node tools/tsgo-api-gate/run-gate.mjs`
(exit 0) against the user-installed tsgo. The correctness hinge of the tsgo backend holds:
an off-disk generated TSX carrier, served only through the `--api` FS overlay, is a real
member of the configured TS project and answers diagnostics/hover/definition from the
configured Program, with incremental Program reuse across edits.

**PRODUCTION CARRIER IDENTITY — PROVEN (`GATE 5`, the load-bearing P0).**
Because the shipped `--api` has **no module-resolution-map endpoint**, the path Verter serves
the carrier at IS the engine file identity, and tsgo only reaches it by **appending `.ts`/`.tsx`
to the FULL bare-import basename** (`import "./Comp.vue"` probes `Comp.d.vue.ts` then `Comp.vue.ts`
then `Comp.vue.tsx`). GATE 5 decided the identity empirically against real tsgo (7/7 green):

- The production **bare-import resolution target is the DECLARATION carrier
  `{name}.d.vue.ts` / `{name}.d.svelte.ts`** — the **extension-MIDDLE** `.d.<ext>.ts` form (the
  `.d.` sits between the stem and the carrier extension). tsgo's basename-append probe reaches it
  **FIRST** (probe order `.d.<ext>.ts` → `.<ext>.ts` → `.<ext>.tsx`, so the declaration **wins**
  over the IDE carrier). GATE 5(B) proves `CompB.d.vue.ts` satisfies `import "./CompB.vue"` with
  zero diagnostics and the type flows; GATE 5(B2) proves the **precedence** — with BOTH a
  `CompB2.d.vue.ts` (`label:string`) and a `CompB2.vue.tsx` (`label:number`) present, the bare
  `import "./CompB2.vue"` resolves to the **declaration carrier** (`label` → `string`), so
  `.d.vue.ts` **wins** over `.vue.tsx`.
- The **component IDE carrier `{name}.vue.tsx` / `{name}.svelte.tsx`** is the **self-diagnostics
  surface** (the file `B.vue` is type-checked AS, source-mapped back), **NOT** the bare-import
  target. It is bare-import-probe-compatible in the absence of the declaration carrier and
  **collision-free** against Svelte rune modules (`*.svelte.ts` / `*.svelte.js`, never `.tsx`):
  GATE 5(C) proves a `Widget.svelte.tsx` IDE carrier and a REAL `state.svelte.ts` rune module
  coexist in one directory with correct types **both** ways, no clash.
- A **`.verter.` infix is REJECTED** for any bare-probed carrier — serving `Comp.vue.verter.tsx`
  does NOT satisfy `import "./Comp.vue"` (tsgo never probes a `.verter.` segment) → **TS2307**.
  (The earlier doc's `.verter.` _component_ identity is refuted.)
- The redirect-reached **`.ts` public-API carrier** (`CarrierApi`, reached via project-reference
  redirect / cross-package `.d.ts`-equivalent — **never bare-probed**) is the ONLY place a
  reserved infix is needed/safe: it uses `{name}.vue.verter.ts` / `{name}.svelte.verter.ts` so it
  never aliases a real `*.svelte.ts` rune module. GATE 5(D) records that tsgo probes `.svelte.ts`
  **before** `.svelte.tsx`, so a bare `.svelte.ts` API-carrier identity would collide with a rune
  module — hence the reserved infix on the (non-bare-probed) `.ts` carrier only.
- Edge (GATE 5 D2): a component carrier and a rune module that share the **exact same stem**
  (`Widget.svelte.tsx` + `Widget.svelte.ts`) DO collide on `import "./Widget.svelte"` (the `.ts`
  rune is probed first → TS1192). This is inherent to Svelte's rune-module naming and the
  `.verter.` infix would NOT fix it; the component carrier identity stays `.svelte.tsx` (it wins
  whenever no same-stem `.svelte.ts` exists — the normal case).

- **TS≥7 distribution = npm `typescript@7.x`** (e.g. `typescript@7.0.1-rc`): the shipping Go-port
  ("Corsa") `typescript` package, with per-platform native binaries `@typescript/typescript-{plat}-{arch}`
  and the `--api` client exported at `typescript/unstable/sync`. The `@typescript/native-preview`
  dev-preview channel ships the **identical** `--api` surface; `run-gate.mjs` selects the installed
  `typescript` whose **major is ≥ 7** and falls back to `@typescript/native-preview` as a source.
- **Binary under test in THIS recorded run:** `@typescript/native-preview 7.0.0-dev.20260526.1`
  (win32-x64), the channel installed in the repo at the time (`typescript@7.0.1-rc` not installed here).
  The `typescript@7.0.1-rc` `--api` **surface** is npm-confirmed equivalent, so the gate is expected to
  hold there — but that is a **gate obligation, proven by running the gate against `typescript@7.0.1-rc`**
  (design doc Block 5 / B4), not assumed from surface parity.
  JS API client: `…/dist/api/sync/api.js` (class `API`), spawning `tsgo --api --cwd … --callbacks=…`
  over MessagePack. An `unstable/async` (JSON-RPC) twin exists for a concurrent/multi-threaded driver.
- **Harness:** `tools/tsgo-api-gate/{harness,control,incremental,run-gate}.mjs`; hermetic fixture
  `tools/tsgo-api-gate/fixture/`.
- **Public-export import:** all three gate scripts import the sync client through the **public
  package export** `<pkg>/unstable/sync` (resolved via `require.resolve`, honouring the package
  `exports` map — `@typescript/native-preview/unstable/sync`, and parameterized so it equally drives
  `typescript/unstable/sync`), NOT a hand-built internal `dist/api/sync/api.js` path. The gate thus
  exercises exactly the surface a real consumer uses.

## The shipped `--api` shape (the mechanism the design must target)

```
import { API } from "@typescript/native-preview/unstable/sync";

const api = new API({
  tsserverPath: "<path to tsgo[.exe]>",   // explicit user-installed tsgo
  cwd: workspaceRoot,
  fs: {                                    // sparse VFS overlay (FS callbacks)
    readFile(fileName)            -> string (off-disk carrier) | undefined (fall through) | null (force not-found),
    fileExists(fileName)          -> true (carrier) | undefined (fall through),
    getAccessibleEntries(dirName) -> { files:[], directories:[] } (inject carrier into its dir listing) | undefined,
    // realpath / directoryExists also available
  },
});

const snapshot = api.updateSnapshot({ openProject: "<abs tsconfig path>" });   // holds the CONFIGURED Program
const project  = snapshot.getProject(tsconfigPath);              // .compilerOptions / .rootFiles
const defProj  = snapshot.getDefaultProjectForFile(carrierPath); // file -> configured project (queryable)
const diags    = project.program.getSemanticDiagnostics(carrierPath);
const type     = project.checker.getTypeAtPosition(carrierPath, offset);
const hover    = project.checker.typeToString(type);
// incremental edit — push only a per-file delta; the SAME project Program is reused:
const snap2 = api.updateSnapshot({ openProject: tsconfigPath, fileChanges: { changed: [carrierPath] } });
```

**Key correction vs the typescript-go #2824 proposal:** this shipped binary's public
`updateSnapshot` does **NOT** take inline `openFiles:[{ uri, content, scriptKind,
defaultProject, version }]`. Its param is `{ openProject?, fileChanges?: { changed?[],
created?[], deleted?[] } | { invalidateAll: true } }`. Membership is conferred by the FS
overlay + `openProject` (the overlay makes the off-disk carrier visible to the tsconfig's
normal resolution + `include` enumeration), then queried via `getDefaultProjectForFile`. This
is a better fit for Verter than the proposal shape: it maps 1:1 onto the VFS and needs no
special per-file "open" call.

## Gate results (reproduced; 17/17 harness checks green — GATE 1–4 — plus GATE 5, 7/7)

Fixture: a real `tsconfig.json` with `baseUrl` + `paths` (`@/* → ./src/*`) + `jsx:react-jsx` +
`jsxImportSource:verterjsx` + `types:["verter-global-types"]` + `typeRoots` + a **project
reference** to `packages/shared`. The carrier `src/components/Widget.carrier.tsx` is **never
written to disk** (asserted) and is served only through the overlay.

- **GATE 1 (resolves identically to on-disk TSX):** off-disk carrier is a Program root file
  (scriptKind TSX); `getDefaultProjectForFile` → the real tsconfig (not inferred); **no false
  TS2307** (`@/*` alias AND project reference both resolve on the off-disk file); **no TS2304**
  (tsconfig `types`/`typeRoots` global in scope); clean carrier = zero diagnostics; off-disk ≡
  on-disk twin (identical diagnostic codes). PASS.
- **GATE 2 (diagnostics + hover + definition from the configured Program):** deliberate error
  fires TS2345 with exact span; hover on a binding → `"string"`; definition of `formatLabel`
  reaches the path-aliased `src/utils/format.ts`; definition of `makeUser` reaches across the
  project reference. PASS.
- **GATE 3 (carrier-only edit updates the SAME Program incrementally):** an off-disk edit +
  `fileChanges:{changed:[carrier]}` flips the diagnostic set (TS2345 → TS2322 → clean) on the
  **same stable project handle** (`p.<canonical-tsconfig-path>`), with the unchanged dependency
  retained (same source-file identity). PASS.
- **GATE 4 (plain `.ts` imports a BARE `./X.vue` + enhanced types flow — §2.9 DX):** a second
  off-disk `Consumer.ts` (served via the overlay, a member of the same configured Program) does
  `import { widget, type Widget, type ExportedProps } from "./Exported.vue"` — the **bare `.vue`
  specifier**. The overlay serves the **companion** at `Exported.vue.tsx` and serves **nothing** at
  the bare `Exported.vue` path (asserted absent on disk). It resolves with **zero diagnostics** (no
  TS2307) and the companion's exported `widget.label` type **flows into the plain `.ts`**
  (`getTypeAtPosition` → `"string"`). This is the tsgo-side proof of "import a `.vue` from a plain
  `.ts`/`.js` and get enhanced types" **without an in-process plugin** (tsgo cannot load one). The
  verified mechanism: tsgo appends `.tsx`/`.ts` to the `Exported.vue` basename and resolves the
  overlay-served companion — serving TSX at the bare `.vue` path does NOT work (a separate probe
  showed `TS2307`), so the redirection MUST target the companion extension. PASS.
- **GATE 5 (production carrier identity — bare-import target = `.d.<ext>.ts` declaration carrier,
  proven to resolve + WIN over `.vue.tsx`; `.verter.` rejected for a bare-probed carrier; rune
  coexistence — `companion-identity.mjs`, 7/7):**
  - `verter_infix_rejected_for_bare_import` — serving `CompA.vue.verter.tsx` does NOT satisfy
    `import "./CompA.vue"` → **TS2307**. The `.verter.` _component_ identity is rejected.
  - `vue_declaration_carrier_resolves_and_types_flow` — `CompB.d.vue.ts` (a hand-written
    declaration: `declare const widget: { label: string }`) satisfies `import "./CompB.vue"` with
    zero diagnostics; `widget.label` → `"string"` flows into the plain `.ts`. Production
    **declaration** carrier identity is `.d.vue.ts`.
  - `vue_declaration_carrier_wins_over_ide_carrier` — with BOTH `CompB2.d.vue.ts` (`label:string`)
    and `CompB2.vue.tsx` (`label:number`) served, bare `import "./CompB2.vue"` resolves to the
    **declaration** carrier (`widget.label` → `"string"`): `.d.vue.ts` **wins** the probe order over
    `.vue.tsx`. The `.vue.tsx` IDE carrier is the self-diagnostics surface, NOT the bare-import
    target.
  - `svelte_component_carrier_and_rune_module_coexist` + `svelte_bare_import_targets_tsx_component_carrier`
    — `import "./Widget.svelte"` resolves to the `Widget.svelte.tsx` IDE carrier
    (`WidgetProps.label` → `"string"`) while a REAL `state.svelte.ts` rune module in the same dir
    stays resolvable (`count.value` → `"number"`); no clash, correct types both ways. (This section
    serves only the `.svelte.tsx` carrier — no `.d.svelte.ts` — so it records the IDE carrier's
    bare-probe compatibility + rune collision-freedom; the production bare-import TARGET is the
    `.d.svelte.ts` declaration carrier the same probe reaches first, proven for Vue in B/B2.)
  - `svelte_probe_order_recorded` — tsgo probes `.svelte.ts` **before** `.svelte.tsx` (so the
    redirect-reached `.ts` API carrier must avoid a bare `.svelte.ts` identity).
  - `svelte_same_stem_ts_rune_shadows_tsx_carrier` — a same-stem `Same.svelte.ts` rune shadows the
    `Same.svelte.tsx` carrier on `import "./Same.svelte"` (TS1192); documented edge, not fixed by a
    `.verter.` infix. PASS (all 7).

### The bare-`.vue` redirection probe (settles the mechanism)

A 3-strategy probe against the same tsgo established exactly how `import "./Comp.vue"` resolves:

- **A — overlay serves TSX at the bare `Comp.vue` path:** `TS2307` (does NOT resolve).
- **B — overlay serves the companion `Comp.vue.tsx` only:** RESOLVES, types flow.
- **C — overlay serves both:** RESOLVES.
  Conclusion: tsgo's resolver appends `.tsx`/`.ts` to the `.vue` basename; the `.vue`→companion
  redirection is served by answering `<specifier>.tsx`/`.ts` in the FS overlay — no module-resolution-map
  endpoint, and the companion-extension identity (§2.2/§2.3) is load-bearing.
- **Negative control:** with **no `openProject`**, `getProjects()` is `[]` and the carrier
  errors "no project found for file" — today's inferred/config-less path provably cannot pass.
  CONFIRMED.

## Notes / caveats (carried into Block 5 of the design doc)

- The `--api` surface is `unstable`/generated and regenerates per tsgo build → **pin the tsgo
  version and re-run this gate on bumps** (the §2.8 capability handshake). A regression ⇒ fail
  closed for TS≥7 on that build, never ship a degraded path.
- No dedicated static module-resolution-map endpoint yet; `.vue`-specifier redirection rides the
  FS overlay (answer the companion path). Proven here for the `@/*` alias, the project-reference
  redirect, AND the **bare `./X.vue` specifier** redirection — **GATE 4** imports a bare
  `"./Exported.vue"` and resolves it to the overlay-served `Exported.vue.tsx` companion (the bare
  path asserted absent on disk; tsgo appends `.tsx`/`.ts` to the basename). The bare-`.vue` case is
  PROVEN here, not deferred. (Residual for Block 5: tsconfig-virtualization root-set injection under
  a `.vue`-specific `include`/`files` config — see the design doc §2.3.)
- The **production carrier identities** are settled by GATE 5 (above): the **bare-import resolution
  target** is the **declaration carrier** `{name}.d.vue.ts` / `{name}.d.svelte.ts` (extension-MIDDLE,
  the path the basename-append probe reaches FIRST — proven to resolve AND win over `.vue.tsx`); the
  `{name}.vue.tsx` / `{name}.svelte.tsx` IDE carrier is the **self-diagnostics** surface (bare-probe-
  compatible + rune-collision-free, but NOT the bare-import target); the redirect-reached `.ts` API
  carrier uses the reserved `{name}.vue.verter.ts` / `{name}.svelte.verter.ts` infix (never
  bare-probed). A `.verter.` _component_ identity is refuted.
- **Engine acquisition (precise):** the installed `typescript` package whose **major is ≥ 7** wins
  always; `@typescript/native-preview` is accepted as a fallback SOURCE. The production NO-TS
  fallback (not exercised here) downloads `typescript@rc` (the TS7 channel — `rc` = `7.0.1-rc`
  today; npm `latest` is still `6.x`), retargeting `latest` once TS7 is stable, and fails closed
  offline; download-only, never bundled/forked.
- `control.mjs` part (B) introspects a build-dependent private `changes` field and is
  informational only; the authoritative incrementality proof is the observable GATE 3 +
  `incremental.mjs` (stable handle + retained dependency + flipped diagnostics).
- The sync client serializes one request at a time; use `unstable/async` for concurrency if the
  interactive latency needs it.
