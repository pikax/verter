# VS Code Extension E2E Testing

## Architecture

The E2E tests launch a real VS Code instance with the Verter extension loaded, open a test workspace, and verify the full stack: activation, LSP health, TypeScript diagnostics, hover, and binding color decorations.

**Framework**: Mocha (TDD interface: `suite`/`test`/`suiteSetup`) + chai v4 inside the VS Code Extension Host via `@vscode/test-electron`. Vitest cannot run inside the Extension Host due to Vite server dependency and ESM-only architecture. chai v4 is required because tests compile to CommonJS and chai v5 is ESM-only.

**Location**: `packages/vue-vscode/e2e/`

## Fixture-Based Design

Each test workspace is a self-contained fixture in `e2e/fixtures/`:

| Fixture                | What it tests                                |
| ---------------------- | -------------------------------------------- |
| `single-project/`      | Standard Vue project with tsconfig           |
| `monorepo/`            | pnpm workspace with cross-package imports    |
| `tsconfig-extends/`    | tsconfig extending a base config             |
| `tsconfig-references/` | Project references with `composite: true`    |
| `path-aliases/`        | tsconfig `paths` + vite `resolve.alias`      |
| `no-config/`           | Bare folder with no tsconfig or package.json |
| `single-file/`         | Single `.vue` file only                      |

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

## Warm Session & Timeout Policy

### Shared Warm Session

The Mocha runner (`suite/index.ts`) warms the fixture **once** in a root `beforeAll` hook before any test suite runs:

1. `ensureFixtureWarm()` — activates the extension and waits for LSP ready
2. `ensureTypeProviderSynced()` — waits for workspace scanner to finish (if type provider is configured)
3. `openReadyCached(getAppVuePath())` — opens App.vue and waits for typed completions

All three are **idempotent** — safe to call again in individual suites (they return immediately on second call). Individual suites should use `openReadyCached()` instead of the manual `waitForExtensionReady() + openVueFile() + waitForFileReady()` chain.

### Warm-Session API (`helpers.ts`)

| Function                          | Purpose                                                                                                           |
| --------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `ensureFixtureWarm()`             | Idempotent. Activates extension + waits for LSP ready.                                                            |
| `ensureTypeProviderSynced()`      | Idempotent. Calls `ensureFixtureWarm()` then waits for type provider sync.                                        |
| `openReadyCached(path, options?)` | Opens a `.vue` file and waits for file readiness. Caches result — second call with same path returns immediately. |
| `invalidateFileCache(path)`       | Clears cached readiness for a file (use after mutation tests modify it on disk).                                  |

### Timeout Policy — the deadline hierarchy

A polling helper is a **child deadline** of the Mocha runnable awaiting it. When the child's
budget meets or exceeds the parent's, Mocha kills the child first and the run reports
`Timeout of Nms exceeded` instead of the assertion the test wrote — a *different* failure, not
a slower one, and one that hides a legitimately empty result behind what looks like a hang.

| Category                                                     | Deadline | How                                                                    |
| ------------------------------------------------------------ | -------- | ---------------------------------------------------------------------- |
| Mocha default (`SUITE_TIMEOUT_MS`)                           | 30s      | `suite/index.ts` builds Mocha with it                                  |
| One polling wait (`DEFAULT_POLL_BUDGET_MS`)                  | 12s      | Stated outright, **not** derived from the suite deadline               |
| Margin (`POLL_BUDGET_MARGIN_MS`)                             | 3s       | Room for the final poll, the assertions, and the report                |
| Root bootstrap                                               | 90s      | `this.timeout(sequenceParent("rootBeforeAll"))` — a **derived** value  |
| Mutation / acceptance suites                                 | 60s      | Suite-level `this.timeout(60_000)`, inherited by everything after it   |
| Benchmark suites                                             | 90s      | Suite-level `this.timeout(90_000)`                                     |

30s is deliberate: **composition is the normal shape**. Almost every test opens a document
(one readiness wait) and then waits for a feature. At 15s only ONE 12s budget could be spent,
so the second wait was inverted by construction. Two default waits plus the margin is 27s.
Deriving the budget from the deadline instead would consume all of it and reproduce the bug.

**Mocha inheritance matters and is easy to get wrong.** `Suite.prototype.addTest` and
`_createHook` copy the suite's *current* timeout into every test and hook created afterwards.
So a suite-level `this.timeout(60_000)` gives all its tests 60s, and a test's own
`this.timeout()` may raise **or lower** that. Never assume a site runs under the 30s default —
read the enclosing suite.

### The budget registry (`e2e/lib/timeouts.ts`)

Every polling default lives in `POLL_BUDGETS` and is consumed via `pollBudget("name")`. Never
spell a literal deadline in a helper or a test: the literal is invisible to the invariant, and
that is exactly how a 20s budget survived under a 15s deadline.

Each entry declares `parentTimeoutMs` — the **smallest** deadline that budget is declared to
run under — plus a `reason` when it exceeds the suite default. It is a lower bound, not an
equality, because a shared budget legitimately runs under many parents.

That claim is **load-bearing and checked at runtime.** The suite runner hands the registry
Mocha's current runnable, and `pollBudget` refuses two things: a runnable whose real deadline
is *below* the claim (the claim is wrong, and everything measured against it — including any
sequence containing that budget — was measured against a deadline that does not exist), and a
budget that cannot fit the real deadline. Both fail at the call site naming every number
involved.

Waits that run **one after another** under a single deadline go in `POLL_SEQUENCES`, which is
checked as a **sum**, not a maximum: the root hook once awaited 45s + 30s + 12s under 60s,
where every member "fit" individually and the series never could. A test that composes waits
takes its deadline from `sequenceParent("name")`.

Every declared sequence is **bound** by its host with `this.timeout(sequenceParent("name"))`,
so the claimed parent IS the deadline in force rather than one matching by coincidence. Do the
same for any new sequence: a sequence that is declared and never bound has its total checked
against a parent no runnable carries.

**Adding a wait:** register its budget, name the smallest parent it truly runs under, and if
the block already awaits something else, declare the sequence and bind the block to it.

## Test Suites

| File                  | Tests                                                   | Notes                                     |
| --------------------- | ------------------------------------------------------- | ----------------------------------------- |
| `activation.test.ts`  | Extension activates, LSP starts, heartbeat, no crashes  | Hard asserts on LSP readiness             |
| `timing.test.ts`      | Startup time measurement, type provider status          | Hard asserts on LSP readiness             |
| `diagnostics.test.ts` | Activation, file open, diagnostics API, valid ranges    | Runs for all fixtures including no-config |
| `hover.test.ts`       | Hover results + latency for ref/computed/prop/function  | Tokens missing from fixture = pass (N/A)  |
| `decorations.test.ts` | Binding colors, Vue API annotations, prop constness     | Polls with `triggerDecorationRefresh()`   |
| `_teardown.test.ts`   | Flushes timing report data (root-level `suiteTeardown`) | Always runs                               |

## Environment Variables

| Variable                 | Purpose                                                                           |
| ------------------------ | --------------------------------------------------------------------------------- |
| `VERTER_E2E_TEST`        | Set to `"1"` to enable test hooks (log file dual-write, decoration state command) |
| `VERTER_E2E_LOG_FILE`    | Path to write log messages for test assertions                                    |
| `VERTER_E2E_FIXTURE`     | Current fixture name                                                              |
| `VERTER_E2E_TIMING_FILE` | Path for JSON timing report output                                                |
| `VERTER_E2E_LSP_PATH`    | Path to a pre-copied LSP binary (prevents file locking on Windows)                |
| `VERTER_LOG`             | Rust LSP log level (set to `"debug"` for verbose output)                          |

## LSP Binary Copy

On Windows, a running `.exe` is locked by the OS, which prevents `cargo build` from overwriting it. Both `runTests.ts` and `.vscode-test.mjs` copy the LSP binary to `$TEMP/verter-e2e-bin/` before launching VS Code and set `VERTER_E2E_LSP_PATH`. The extension's `findLspBinary()` checks this env var first.

Cross-platform: Windows gets `.exe` suffix, Unix gets `chmod 755`.

## Fixture Dependency Installation

Fixtures with `package.json` need `node_modules/vue` for type resolution. Every launcher — `runTests.ts`, `.vscode-test.mjs`, `startupBenchmark.mjs`, `completionBenchmark.mjs` — goes through `e2e/lib/fixtureDeps.ts`, which installs by PROVENANCE rather than by whether `node_modules/` exists. A tree that exists says nothing about which manifest produced it: a four-month-old `@verter/types` in one fixture once shadowed the workspace package and decided eight test outcomes.

An install records a stamp (`node_modules/.verter-e2e-install.json`) covering the manifest bytes AND a fingerprint of the installed tree, so editing `package.json` reinstalls and so does a package added, removed, or re-pointed inside `node_modules` without touching the manifest.

What happens to the tree already there:

| State | Action |
| --- | --- |
| Stamped, manifest and tree both match | Reused, untouched |
| Stamped, only the manifest changed | Renamed aside, installed clean, predecessor deleted **after** success (restored if the install fails) |
| Unstamped, legacy stamp, or tree changed | **Moved to a quarantine**, absolute recovery path printed, then installed clean |
| Undecidable (unreadable current-format stamp, unreadable tree, `node_modules` is a symlink, move fails) | **Refuses**, having changed nothing |

Nothing is ever deleted outright except a predecessor proven to be this module's own output. Quarantines live in `.verter-e2e-quarantine/` at the **repository root**, which is gitignored: the move is a `rename` and a rename cannot cross filesystems, so anchoring it to the temp directory made whether it worked at all a property of the machine's disk layout. `VERTER_E2E_FIXTURE_QUARANTINE_DIR` still overrides it and must name a path on the repository's own filesystem. They are removed only on request:

```bash
pnpm --filter verter-vscode test:e2e:fixtures:clean-quarantine
```

`VERTER_E2E_ADOPT_FIXTURE_DEPS=<fixture>[,<fixture>]` uses an existing tree as-is without installing or stamping, printing a `NON-HERMETIC` banner. It is per-fixture (no wildcards) and is REJECTED under CI. A `node_modules.verter-rollback-*` holding a killed run left inside the fixture is still recovered to the quarantine, in adopt mode too — that is a PREVIOUS tree rather than the one being adopted, and TypeScript's default `exclude` matches the literal name `node_modules`, so leaving it puts the whole holding in the fixture's program.

The decide → displace → install → stamp sequence runs under a cross-process lock (`e2e/lib/fixtureLock.ts`) and re-decides once it owns one, so two concurrent runs cannot both replace one fixture. A wedged lock surfaces as a timeout naming the file to remove. The lock is published with `link`, which is exclusive and atomic in one step; an exclusive create takes over only where hard links are POSITIVELY classified as unavailable (a probe that fails every attempt of a bounded retry, with an errno that says the capability is absent). A link failure that is transient or unexplained is retried and then REFUSES — the fallback is not atomic on NFSv3, so an unexplained failure is never read as licence to use it.

An install failure is FATAL, not a warning — a fixture nobody finished assembling produces results about nothing. A `file:`/`link:` dependency whose target is missing is diagnosed before anything moves, naming the absolute path (`ecosystem-parity` points at a sibling `verter-release-clean` checkout that most machines do not have).

`E2E_FIXTURE` is validated against the canonical route inventory before it is joined onto a path. For monorepo and multi-root fixtures, sub-packages are installed too.

## File Opening Triggers LSP

Verter only starts the LSP when a `.vue` file is first opened. The `ensureFixtureWarm()` helper (via `waitForExtensionReady()`) automatically opens `App.vue` to trigger initialization before polling for the "Verter ready" log message.

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

### `openReadyCached(path)` — Preferred for suite setup (RECOMMENDED)

```typescript
// GOOD: Uses shared warm session, skips redundant polling
suiteSetup(async function () {
  doc = await openReadyCached(getAppVuePath());
});

// Also GOOD: For non-App.vue files in fixture-specific suites
suiteSetup(async function () {
  if (FIXTURE_NAME !== "single-project") return;
  doc = await openReadyCached("src/StyledComp.vue");
});
```

**When to use:** In `suiteSetup` for any suite that opens a `.vue` file. Combines `openVueFile()` + `waitForFileReady()` with caching.

### `waitForFileReady(doc)` — Wait for type provider readiness (inside tests)

```typescript
// Use inside individual tests that open additional files mid-test
const slotDoc = await openVueFile("src/SlotComp.vue");
await waitForFileReady(slotDoc, { probePosition, expectedLabel: "slot" });
```

**When to use:** Inside test bodies when opening files not covered by `openReadyCached`, or when probing specific positions.

### `waitForDiagnostics(uri, options)` — Event-based diagnostic waiting

```typescript
// GOOD: Event-driven — resolves within ms of diagnostic arrival
const diags = await waitForDiagnostics(doc.uri, { source: "ts", minCount: 1 });

// Also GOOD: Checking for absence of diagnostics after file is ready
await waitForFileReady(doc);
const diags = vscode.languages.getDiagnostics(doc.uri);
expect(diags.filter((d) => d.code === "2307")).to.be.empty;
```

**When to use:** When a test needs to wait for specific diagnostics to appear (e.g., TS2304 after inserting an error).

**For absence checks:** Use `waitForFileReady()` first (ensures type provider processed the file), then read diagnostics synchronously.

### `waitForDiagnosticsSettled(uri, options)` — Quiescence-based diagnostic waiting

```typescript
// GOOD: Resolves when diagnostics stop changing for stableMs
const diags = await waitForDiagnosticsSettled(doc.uri, {
  timeoutMs: 5_000,
  stableMs: 500,
});

// BAD: predicate: () => false always burns the full timeout
const diags = await waitForDiagnostics(doc.uri, {
  timeoutMs: 8_000,
  predicate: () => false, // DO NOT DO THIS
});
```

**When to use:** When you want to see what diagnostics look like after all processing is complete, without requiring any specific predicate. Resolves when no `onDidChangeDiagnostics` events fire for `stableMs` milliseconds.

**Defaults:** `timeoutMs: 5_000`, `stableMs: 500`.

### `invalidateFileCache(path)` — After mutation tests

```typescript
// After modifying a file on disk, invalidate the cache so next
// openReadyCached() re-polls for readiness
writeFileOnDisk("src/MyComp.vue", newContent);
invalidateFileCache("src/MyComp.vue");
```

### Never use `sleep()` for LSP readiness

`sleep()` is only acceptable for:

- Short pauses between undo commands: `await sleep(200)`
- Letting VS Code process an edit before reading state: `await sleep(100)`

Never use `sleep()` to wait for:

- Type provider to process a file (use `waitForFileReady`)
- Diagnostics to appear (use `waitForDiagnostics`)
- LSP to be ready (use `ensureFixtureWarm`)

## Adding a New Fixture

1. Create `e2e/fixtures/<name>/` with `.vue` files and config
2. Update `FIXTURES` array in `e2e/runTests.ts`
3. Run `E2E_FIXTURE=<name> pnpm --filter verter-vscode test:e2e` to verify

## Adding a New Test Suite

1. Create `e2e/suite/<name>.test.ts`
2. Import helpers from `../helpers` and timer from `../timer`
3. Use `suiteSetup` to call `openReadyCached(getAppVuePath())` — no need for manual `waitForExtensionReady()` or `waitForTypeProviderSync()`
4. Use hard `expect(isLspReady()).to.be.true` — never `this.skip()` for infrastructure
5. For fixture-specific content, `return` early (pass with N/A) instead of `this.skip()`
6. Never use `sleep()` for LSP readiness — use `openReadyCached()` or `waitForDiagnostics()`
7. The Mocha runner auto-discovers `**/*.test.js` files
8. Do NOT add `this.timeout(60_000)` unless the suite genuinely needs more than 15s (mutation/benchmark tests)

## Key Files

- `packages/vue-vscode/e2e/helpers.ts` — Test utilities, LSP readiness checks, warm-session API
- `packages/vue-vscode/e2e/timer.ts` — Timing measurement + JSON report writer
- `packages/vue-vscode/e2e/suite/index.ts` — Mocha runner entry point + root bootstrap hook
- `packages/vue-vscode/e2e/runTests.ts` — Multi-fixture test launcher
- `packages/vue-vscode/.vscode-test.mjs` — `@vscode/test-cli` configuration
- `packages/vue-vscode/tsconfig.test.json` — TypeScript config for E2E tests
