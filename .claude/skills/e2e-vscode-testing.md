# VS Code Extension E2E Testing

## Architecture

The E2E tests launch a real VS Code instance with the Verter extension loaded, open a test workspace, and verify the full stack: activation, LSP health, TypeScript diagnostics, hover, and binding color decorations.

**Framework**: Mocha (TDD interface: `suite`/`test`/`suiteSetup`) + chai v4 inside the VS Code Extension Host via `@vscode/test-electron`. Vitest cannot run inside the Extension Host due to Vite server dependency and ESM-only architecture. chai v4 is required because tests compile to CommonJS and chai v5 is ESM-only.

**Location**: `packages/vue-vscode/e2e/`

## Fixture-Based Design

Each test workspace is a self-contained fixture in `e2e/fixtures/`:

| Fixture | What it tests |
|---------|---------------|
| `single-project/` | Standard Vue project with tsconfig |
| `monorepo/` | pnpm workspace with cross-package imports |
| `tsconfig-extends/` | tsconfig extending a base config |
| `tsconfig-references/` | Project references with `composite: true` |
| `path-aliases/` | tsconfig `paths` + vite `resolve.alias` |
| `no-config/` | Bare folder with no tsconfig or package.json |
| `single-file/` | Single `.vue` file only |

**All 29 tests run for every fixture** — no suite-level skips. The extension works with default tsgo/tsserver config even without tsconfig. Individual tests that check for specific template tokens (e.g. `{{ title }}`) pass with an N/A message when the token isn't in the fixture.

## Running Tests

```bash
# Prerequisites
pnpm run build:lsp                     # Build Rust LSP binary
pnpm --filter verter-vscode build:dev  # Bundle extension

# Run single fixture (fast, targeted)
E2E_FIXTURE=single-project pnpm --filter verter-vscode test:e2e

# Run from monorepo root
pnpm run test:e2e

# Run all fixtures (via runTests.ts)
npx ts-node packages/vue-vscode/e2e/runTests.ts

# Run specific fixture
npx ts-node packages/vue-vscode/e2e/runTests.ts --fixture=monorepo
```

## Test Suites

| File | Tests | Notes |
|------|-------|-------|
| `activation.test.ts` | Extension activates, LSP starts, heartbeat, no crashes | Hard asserts on LSP readiness |
| `timing.test.ts` | Startup time measurement, type provider status | Hard asserts on LSP readiness |
| `diagnostics.test.ts` | Activation, file open, diagnostics API, valid ranges | Runs for all fixtures including no-config |
| `hover.test.ts` | Hover results + latency for ref/computed/prop/function | Tokens missing from fixture = pass (N/A) |
| `decorations.test.ts` | Binding colors, Vue API annotations, prop constness | Polls with `triggerDecorationRefresh()` |
| `_teardown.test.ts` | Flushes timing report data (root-level `suiteTeardown`) | Always runs |

## Environment Variables

| Variable | Purpose |
|----------|---------|
| `VERTER_E2E_TEST` | Set to `"1"` to enable test hooks (log file dual-write, decoration state command) |
| `VERTER_E2E_LOG_FILE` | Path to write log messages for test assertions |
| `VERTER_E2E_FIXTURE` | Current fixture name |
| `VERTER_E2E_TIMING_FILE` | Path for JSON timing report output |
| `VERTER_E2E_LSP_PATH` | Path to a pre-copied LSP binary (prevents file locking on Windows) |
| `VERTER_LOG` | Rust LSP log level (set to `"debug"` for verbose output) |

## LSP Binary Copy

On Windows, a running `.exe` is locked by the OS, which prevents `cargo build` from overwriting it. Both `runTests.ts` and `.vscode-test.mjs` copy the LSP binary to `$TEMP/verter-e2e-bin/` before launching VS Code and set `VERTER_E2E_LSP_PATH`. The extension's `findLspBinary()` checks this env var first.

Cross-platform: Windows gets `.exe` suffix, Unix gets `chmod 755`.

## Fixture Dependency Installation

Fixtures with `package.json` need `node_modules/vue` for type resolution. Both `runTests.ts` and `.vscode-test.mjs` run `npm install --no-package-lock --ignore-scripts` in fixtures that have `package.json` but no `node_modules/`. For monorepo fixtures, workspace sub-packages are also installed.

## File Opening Triggers LSP

Verter only starts the LSP when a `.vue` file is first opened. The `waitForExtensionReady()` helper automatically opens `App.vue` to trigger initialization before polling for the "Verter ready" log message.

## Decoration State Testing

VS Code has no `getDecorations()` API. The framework uses a test-mode command:

1. When `VERTER_E2E_TEST` is set, extension registers `verter._getDecorationState` command
2. Each decoration provider has a `getState()` method storing the last-applied ranges
3. Tests poll with `triggerDecorationRefresh()` (no-op edit + undo) to force decoration re-evaluation
4. Tests call `vscode.commands.executeCommand("verter._getDecorationState")` to get:
   ```json
   {
     "bindingColors": { "ref": [{startLine, startChar, endLine, endChar}], ... },
     "vueApiCalls": { "lifecycle": [...], "watcher": [...], ... },
     "propConstness": { "const": [...], "dynamic": [...] }
   }
   ```

## Timing Report

Each fixture run produces `verter-e2e-timing-<fixture>.json`:

```json
{
  "fixture": "single-project",
  "startup": { "activationToReadyMs": 635, "typeProvider": "tsgo" },
  "hover": { "samples": [{ "target": "count (ref)", "latencyMs": 17 }], "avgMs": 6 },
  "diagnostics": { "timeToFirstDiagnosticMs": 0, "totalDiagnostics": 0 }
}
```

## CI Integration

The E2E tests run in the CI workflow (`.github/workflows/ci.yml` → `vscode-e2e` job) on every push/PR when relevant files change. The CI job builds the LSP binary, bundles the extension, compiles tests, and runs against `single-project` with `xvfb-run` for headless display.

## When to Run E2E Tests (MANDATORY)

After ANY change to:
- `crates/verter_lsp/` (LSP server) — handlers, sync, diagnostics, completions, hover, definition, etc.
- `packages/vue-vscode/src/` (VS Code extension) — activation, client config, decoration providers, commands

Run the E2E suite to verify no regressions:
```bash
pnpm run build:lsp && pnpm --filter verter-vscode build:dev && E2E_FIXTURE=single-project pnpm --filter verter-vscode test:e2e
```

This is non-negotiable. LSP changes directly affect user-facing IDE behavior. Manual testing is never sufficient as the sole verification step.

## Waiting Patterns

### `waitForFileReady(doc)` — Wait for type provider readiness

```typescript
// GOOD: Adaptive — returns in 1-5s when LSP is ready
suiteSetup(async function () {
  await waitForExtensionReady();
  doc = await openVueFile(getAppVuePath());
  await waitForFileReady(doc);  // probes completions until typed results appear
});

// BAD: Blind sleep — always waits 12s even if ready in 2s
suiteSetup(async function () {
  await waitForExtensionReady();
  doc = await openVueFile(getAppVuePath());
  await sleep(12_000);  // DO NOT DO THIS
});
```

**When to use:** After opening any `.vue` file before running LSP feature tests (completions, hover, definition, rename, references, etc.).

**How it works:** Repeatedly requests completions at a mustache expression position. When the type provider has synced the file, completions return with proper kinds (Variable, Function) instead of Text. Auto-detects probe position from `{{ identifier }}` patterns in the document.

**Custom probe:** If the file has no mustache expressions, pass `probePosition` and `expectedLabel` explicitly.

### `waitForDiagnostics(uri, options)` — Event-based diagnostic waiting

```typescript
// GOOD: Event-driven — resolves within ms of diagnostic arrival
const diags = await waitForDiagnostics(doc.uri, { source: "ts", minCount: 1 });

// Also GOOD: Checking for absence of diagnostics after file is ready
await waitForFileReady(doc);
const diags = vscode.languages.getDiagnostics(doc.uri);
expect(diags.filter(d => d.code === "2307")).to.be.empty;
```

**When to use:** When a test needs to wait for specific diagnostics to appear (e.g., TS2304 after inserting an error).

**For absence checks:** Use `waitForFileReady()` first (ensures type provider processed the file), then read diagnostics synchronously.

### Never use `sleep()` for LSP readiness

`sleep()` is only acceptable for:
- Short pauses between undo commands: `await sleep(200)`
- Letting VS Code process an edit before reading state: `await sleep(100)`

Never use `sleep()` to wait for:
- Type provider to process a file (use `waitForFileReady`)
- Diagnostics to appear (use `waitForDiagnostics`)
- LSP to be ready (use `waitForExtensionReady`)

## Adding a New Fixture

1. Create `e2e/fixtures/<name>/` with `.vue` files and config
2. Update `FIXTURES` array in `e2e/runTests.ts`
3. Run `E2E_FIXTURE=<name> pnpm --filter verter-vscode test:e2e` to verify

## Adding a New Test Suite

1. Create `e2e/suite/<name>.test.ts`
2. Import helpers from `../helpers` and timer from `../timer`
3. Use `suiteSetup` to call `waitForExtensionReady()` then `waitForFileReady(doc)` before tests
4. Use hard `expect(isLspReady()).to.be.true` — never `this.skip()` for infrastructure
5. For fixture-specific content, `return` early (pass with N/A) instead of `this.skip()`
6. Never use `sleep()` for LSP readiness — use `waitForFileReady()` or `waitForDiagnostics()`
7. The Mocha runner auto-discovers `**/*.test.js` files

## Key Files

- `packages/vue-vscode/e2e/helpers.ts` — Test utilities, LSP readiness checks
- `packages/vue-vscode/e2e/timer.ts` — Timing measurement + JSON report writer
- `packages/vue-vscode/e2e/suite/index.ts` — Mocha runner entry point
- `packages/vue-vscode/e2e/runTests.ts` — Multi-fixture test launcher
- `packages/vue-vscode/.vscode-test.mjs` — `@vscode/test-cli` configuration
- `packages/vue-vscode/tsconfig.test.json` — TypeScript config for E2E tests
