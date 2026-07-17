# Editor Integration E2E Testing

This guide covers the editor-neutral LSP contract and the real VS Code extension E2E suites. Keep
the two layers distinct: standard LSP behavior belongs in the reusable contract, while extension
activation and VS Code command wiring belong in the VS Code suites.

## Prerequisites

1. **Install test dependencies** (handled by `pnpm install`):
   - `@vscode/test-electron` — Downloads and launches VS Code
   - `@vscode/test-cli` — CLI configuration wrapper
   - `mocha` + `chai` (v4, CommonJS) — Test framework and assertions
   - `@types/mocha` + `@types/chai` (v4) + `@types/node` — TypeScript types

> **Note**: chai v4 is required because tests compile to CommonJS (`tsconfig.test.json`). chai v5 is ESM-only and cannot be `require()`'d.

## Editor-neutral LSP contract

Run the complete raw-stdio contract from the repository root:

```bash
pnpm test:lsp:neutral
```

The command builds the real `verter-lsp` and `verter-relay-shim` binaries, the TypeScript provider
plugin, and then executes one immutable Vue/Svelte, JavaScript/TypeScript fixture against
`tsserver`, managed `tsgo`, and real relay-backed `shared-tsgo`.

The inventory is typed and fail-closed:

- 71 standard-LSP cases cover diagnostics (including the absence of TS7026), hover, definition,
  completion, rename across script and markup, direct SFC imports, two-hop barrel imports, and a
  plain-TypeScript navigation control that distinguishes SFC projection defects from provider-route
  defects. Definition rows validate the exact declaration selection range rather than accepting a
  matching document URI alone. Local script↔markup definitions run both a first and repeated request;
  the receipt records both durations under the existing bounded request/case timeouts, without a
  machine-speed micro-threshold. Rename rows require every edit to carry the requested `newText` and
  to select exactly the original authored script or markup token. A dedicated Svelte 5 case requires
  `let title: string = $state("x")` to remain clean when
  interpolated as `<p>{title}</p>` on every provider route. Unannotated TypeScript Vue `<script setup>`
  handlers and Svelte 5 runes-mode instance handlers referenced by DOM `pointerdown` bindings must infer
  `PointerEvent`: a valid `pointerId` access is clean, hover cannot degrade to `any` or generic
  `Event`, and an authored `@ts-expect-error` on a nonexistent member must consume TS2339 (degrading
  to `any` instead produces TS2578). Automatic named-handler inference is not applied to JavaScript:
  strict unannotated JavaScript rows consume the intended TS7006 at the declaration, while typed
  JavaScript rows carry authored `@param {PointerEvent}` JSDoc and require PointerEvent language
  features plus a consumed invalid-member error. Strict Svelte legacy-mode controls likewise consume
  TS7006 at the declaration; accidental back-propagated inference makes that expectation unused and
  fails with TS2578. `.jsx` and `.tsx`
  `HTMLElement.onpointerdown` controls record native TypeScript behavior without generated JSDoc, parameter
  annotations, or JSX-runtime shims. Both controls are compiled directly by TypeScript 7 or newer,
  and their versions plus exact TS2339 diagnostics are recorded as authority evidence separately from
  the standard-LSP executions. Nested authored-config rows separate strict checking from lax
  JavaScript policy: their authored JSDoc keeps hover, completion, and definition concretely
  `PointerEvent`-aware, while `checkJs: false`/`strict: false` keeps the invalid member diagnostic-free.
  The lax invalid member is deliberately not hidden by `@ts-expect-error`, so the zero-diagnostic
  result proves the authored policy is actually honored.
  The strict and lax rows are run together so a generated or hidden override of `allowJs`, `checkJs`,
  `strict`, or `noImplicitAny` cannot satisfy both policies. TypeScript 7 also compiles the nested lax
  project directly and records its zero-diagnostic result in the receipt, independently of Verter's
  provider routing.
- One Verter custom-protocol case attests the selected provider route separately from standard LSP.
- One provider-topology case applies only to `shared-tsgo`; it requires a live editor-owned relay
  and forbids activation of a managed fallback provider.
- The three routes produce exactly 217 required executions. A missing route, startup failure, empty
  response, skipped/N/A case, or incomplete execution count fails the run.

The fixture intentionally has no configured `jsxImportSource`. The raw server process sets the same
provider-only E2E completion flags as the real-editor parity suites, so completion evidence cannot be
satisfied by Verter-authored fallback suggestions. Public Svelte hovers must expose the
Svelte 5 `Component` contract and must not leak generated carrier types such as
`__VerterPublicInstance`. The runner writes a JSON receipt to `VERTER_EDITOR_NEUTRAL_RECEIPT`, or
to the operating-system temporary directory when that variable is absent. The receipt records each
route/case outcome, duration, and failure message, plus stable inventory and execution groupings by
route, surface, feature, and framework/language in addition to exact attempted/passed/failed totals.

The shared contract and fixture live under `packages/lsp-test-client`; the real-process driver and
gate live under `packages/dx-harness`. Editor clients can implement the same narrow driver interface
for their own smoke suites without reclassifying Verter notifications as standard LSP.

## Running Tests

### Single fixture (recommended for development)

```bash
# Rebuild the LSP + extension + E2E bundle, then run a single fixture/provider
E2E_FIXTURE=single-project E2E_TYPE_PROVIDER=tsserver pnpm --filter verter-vscode test:e2e
```

`pnpm --filter verter-vscode test:e2e` now prepares the Rust binary, extension bundle, and compiled E2E tests before launching VS Code. Use `pnpm --filter verter-vscode test:e2e:run` only when artifacts are already prepared, such as in CI.

The `E2E_FIXTURE` environment variable selects which test workspace to open. `E2E_TYPE_PROVIDER`
selects the provider under test (`tsserver`, `tsgo`, or, for relay-enabled fixtures,
`shared-tsgo`). Available fixtures include:

- `single-project` — Standard Vue project with tsconfig
- `monorepo` — pnpm workspace with cross-package imports
- `tsconfig-extends` — tsconfig extending a base config
- `tsconfig-references` — Project references (composite)
- `path-aliases` — tsconfig paths + vite resolve.alias
- `no-config` — Bare folder, no configuration
- `single-file` — Just one .vue file

Every CI E2E run is fixture x applicable provider, so provider-specific regressions are caught
explicitly instead of relying on `auto` mode.

### All fixtures and applicable providers

```bash
pnpm run test:e2e
```

This prepares the artifacts once, then iterates over every fixture/provider combination with a fresh VS Code instance for each run.

### Specific fixture via runTests.ts

```bash
pnpm --filter verter-vscode test:e2e:matrix -- --fixture=monorepo@tsgo
```

### From the monorepo root

```bash
pnpm run test:e2e                                                # Full fixture/provider matrix
E2E_FIXTURE=no-config E2E_TYPE_PROVIDER=tsgo pnpm run test:e2e:single
```

## CI Integration

The editor-neutral contract and VS Code E2E tests run as separate CI jobs on relevant changes. The
neutral job builds the shared contract packages, downloads the same native/provider artifacts used
by editor integration E2E, executes all three provider routes without a display server, and uploads
the non-vacuity receipt.

The VS Code job:

1. Prepares the LSP binary, extension bundle, and compiled E2E tests once
2. Downloads those artifacts into the E2E job
3. Runs the fixture matrix against each applicable provider with `xvfb-run` (headless display)

See `.github/workflows/ci.yml` jobs `editor-neutral-lsp` and `vscode-e2e`.

## Writing New Tests

### Test file location

Place test files in `packages/vue-vscode/e2e/suite/`. The Mocha runner auto-discovers all `*.test.ts` files.

### Template

```typescript
import { expect } from "chai";
import * as vscode from "vscode";
import {
  waitForExtensionReady,
  openVueFile,
  getAppVuePath,
  isLspReady,
  FIXTURE_NAME,
} from "../helpers";

suite(`My Test Suite [${FIXTURE_NAME}]`, function () {
  this.timeout(60_000);

  suiteSetup(async function () {
    await waitForExtensionReady(60_000);
  });

  test("my test case", async function () {
    expect(isLspReady(), "LSP should reach ready state").to.be.true;
    const doc = await openVueFile(getAppVuePath());
    // ... assertions
  });
});
```

### Design principles

- **No `this.skip()` for infrastructure** — every test must either pass or fail. If the LSP doesn't start, that's a failure, not a skip.
- **Graceful N/A for fixture-specific content** — when a template token (e.g. `{{ title }}`) isn't in a fixture, `return` early with a log message. The test passes but reports N/A.
- **Soft assertions for analysis-dependent data** — decoration categories like ref/computed that depend on deep analysis use `at.least(0)` with warning logs, since analysis depth varies.

### Available helpers

- `waitForExtensionReady(timeout)` — Wait for extension + LSP ready (opens a `.vue` file to trigger LSP init)
- `isLspReady()` — Check if LSP reached ready state (use as hard `expect`, not as skip guard)
- `openVueFile(path)` — Open a file from the workspace
- `getAppVuePath()` — Get the main App.vue path (varies by fixture)
- `waitForDiagnostics(uri, options)` — Poll for diagnostics
- `measureHover(uri, position)` — Measure hover latency
- `getDecorationState()` — Get decoration ranges from all providers
- `triggerDecorationRefresh()` — Force decoration providers to re-request analysis via no-op edit
- `assertLogContains(needle)` / `assertLogNotContains(needle)` — Log assertions

## Creating a New Fixture

1. Create a directory under `packages/vue-vscode/e2e/fixtures/<name>/`
2. Add at minimum an `App.vue` file
3. Add `tsconfig.json`, `package.json`, etc. as needed
4. Update `e2e/runTests.ts` → add to `FIXTURES` array

### Standard App.vue template

Use this for consistent testing across fixtures:

```vue
<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";

const count = ref(0);
const doubled = computed(() => count.value * 2);
const props = defineProps<{ title: string }>();

onMounted(() => {
  console.log("mounted");
});
watch(count, (val) => {
  console.log(val);
});

function increment() {
  count.value++;
}
</script>
<template>
  <div>
    <h1>{{ title }}</h1>
    <p>{{ count }} x 2 = {{ doubled }}</p>
    <button @click="increment">+</button>
  </div>
</template>
```

## Timing Reports

Each test run produces a JSON timing report at `$TEMP/verter-e2e-timing-<fixture>.json`:

```bash
cat /tmp/verter-e2e-timing-single-project.json | jq .
```

Fields:

- `startup.activationToReadyMs` — Time from extension activation to LSP ready
- `hover.samples[].latencyMs` — Individual hover latencies
- `hover.avgMs` / `hover.p95Ms` — Aggregate hover stats
- `diagnostics.timeToFirstDiagnosticMs` — Time to first diagnostic

## Troubleshooting

### Tests time out waiting for "Verter ready"

- Check that `pnpm run build:lsp` produced a binary at `target/debug/verter-lsp`
- Check the log file (path shown in test output) for error messages
- Increase the timeout in `waitForExtensionReady()` if needed
- Verter only starts the LSP when a `.vue` file is opened — `waitForExtensionReady()` handles this automatically

### LSP binary locked on Windows

On Windows, a running `.exe` is locked by the OS. The test runner copies the binary to `$TEMP/verter-e2e-bin/` and sets `VERTER_E2E_LSP_PATH`. If you're rebuilding with `cargo build` while tests run, this prevents file locking conflicts.

### Fixture dependencies missing

Fixtures with `package.json` need `node_modules/vue` installed for proper type resolution. The test runner automatically runs `npm install --no-package-lock --ignore-scripts` in fixtures before launching VS Code. If you still see type resolution issues, manually run `npm install` in the fixture directory.

### Decoration state command returns undefined

- Ensure `VERTER_E2E_TEST=1` is set in the environment
- The command is only registered when this env var is present

### Type provider not starting

- The fixture needs a `package.json` with `vue` dependency and TypeScript installed
- Check `VERTER_LOG=debug` output for type provider initialization messages
- Even without tsconfig, Verter uses tsgo/tsserver with default config

### VS Code fails to download

- `@vscode/test-electron` downloads VS Code to `.vscode-test/`
- Check network connectivity
- On CI, ensure the cache directory is writable
