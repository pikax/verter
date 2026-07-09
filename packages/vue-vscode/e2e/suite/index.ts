import * as path from "path";
import Mocha from "mocha";
import * as fs from "fs";
import { getTimer } from "../timer";
import {
  ensureFixtureWarm,
  ensureTypeProviderSynced,
  openReadyCached,
  getAppVuePath,
  TYPE_PROVIDER,
} from "../helpers";

/** Recursively find all *.test.js files under a directory. */
function findTestFiles(dir: string): string[] {
  const results: string[] = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...findTestFiles(fullPath));
    } else if (entry.name.endsWith(".test.js")) {
      results.push(fullPath);
    }
  }
  return results;
}

/**
 * Persist a run summary beside the E2E log so the runner (and CI) can verify a
 * narrowed run actually EXECUTED tests. The `@vscode/test-electron` host does not
 * relay mocha's reporter output to the parent process on every platform, so without
 * this a run that loaded ZERO tests — a flaky launch, a bad `VERTER_E2E_ONLY`
 * pattern, or an aborted root hook — would exit 0 and masquerade as a pass.
 */
function writeRunSummary(summary: Record<string, unknown>): void {
  const base = process.env.VERTER_E2E_LOG_FILE;
  if (!base) return;
  try {
    fs.writeFileSync(`${base}.runsummary`, JSON.stringify(summary, null, 2));
  } catch {
    /* best-effort */
  }
}

export async function run(): Promise<void> {
  const mocha = new Mocha({
    ui: "tdd",
    timeout: 15_000,
    color: true,
  });

  const testsRoot = path.resolve(__dirname);
  const onlyPattern = process.env.VERTER_E2E_ONLY || process.env.E2E_ONLY;
  const files = findTestFiles(testsRoot).filter((file) =>
    onlyPattern ? file.includes(onlyPattern) : true,
  );

  for (const f of files) {
    mocha.addFile(f);
  }

  let rootHookError: string | undefined;

  // Root hooks run once before/after all test suites.
  // Warm the fixture so individual suites skip redundant polling.
  mocha.rootHooks({
    async beforeAll(this: Mocha.Context) {
      this.timeout(60_000);
      try {
        await ensureFixtureWarm();
        if (TYPE_PROVIDER) {
          await ensureTypeProviderSynced();
        }
        await openReadyCached(getAppVuePath());
      } catch (err) {
        // Record the root-hook failure so the summary shows WHY 0 tests ran, then
        // rethrow so mocha reports the failure rather than silently skipping suites.
        rootHookError = err instanceof Error ? `${err.message}\n${err.stack ?? ""}` : String(err);
        throw err;
      }
    },
  });

  return new Promise((resolve, reject) => {
    const runner = mocha.run((failures: number) => {
      // Write timing report regardless of test outcome
      getTimer().flush();

      const stats = runner.stats ?? { passes: 0, failures: 0, pending: 0, tests: 0 };
      const executed = (stats.passes ?? 0) + (stats.failures ?? 0) + (stats.pending ?? 0);
      writeRunSummary({
        onlyPattern: onlyPattern ?? null,
        loadedFiles: files.map((f) => path.basename(f)),
        passes: stats.passes ?? 0,
        failures: stats.failures ?? 0,
        pending: stats.pending ?? 0,
        executed,
        rootHookError: rootHookError ?? null,
      });

      // Reject on the runner's OWN failure count too — on some hosts the callback's
      // `failures` arg under-counts a failed hook while `runner.stats.failures` is
      // authoritative, so a hook-throwing gate would otherwise resolve as a pass.
      const failed = failures > 0 || (stats.failures ?? 0) > 0;
      if (failed) {
        reject(new Error(`${Math.max(failures, stats.failures ?? 0)} tests failed`));
        return;
      }
      // Zero-test guard: a NARROWED run (VERTER_E2E_ONLY set) that executed NO tests
      // is a vacuous pass — the suite failed to load, the pattern matched nothing, or
      // the root hook aborted. Fail closed so a gate can never silently not-run.
      if (onlyPattern && executed === 0) {
        reject(
          new Error(
            `narrowed run (VERTER_E2E_ONLY=${onlyPattern}) executed 0 tests — vacuous pass refused` +
              (rootHookError ? `; root hook error: ${rootHookError}` : ""),
          ),
        );
        return;
      }
      resolve();
    });
  });
}
