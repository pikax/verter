import * as path from "path";
import Mocha from "mocha";
import * as fs from "fs";
import { createHash } from "node:crypto";
import { getTimer } from "../timer";
import {
  ensureFixtureWarm,
  ensureTypeProviderSynced,
  openReadyCached,
  getAppVuePath,
  TYPE_PROVIDER,
  FIXTURE_NAME,
  readTestLog,
} from "../helpers";
import { suiteAllowedForFixture } from "../lib/fixtureSuiteMap";
import { assertSharedTsgoServedWithoutFallback } from "../../src/e2eProviderAttestation";
import { assertNotVacuousPassLog } from "../lib/vacuousPass";
import { pollBudget, sequenceParent, setRunnableAccessor, SUITE_TIMEOUT_MS } from "../lib/timeouts";
import { launchServerProfile, routeBaseServerProfile } from "../helpers";
import { serverProfileForSuite } from "../lib/serverProfiles";

/** Recursively find the authored test sources under a directory. */
function findTestSources(dir: string): string[] {
  const results: string[] = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      results.push(...findTestSources(fullPath));
    } else if (entry.name.endsWith(".test.ts")) {
      results.push(fullPath);
    }
  }
  return results;
}

function discoverCompiledTests(compiledRoot: string): string[] {
  const sourceRoot = path.resolve(compiledRoot, "../../../e2e/suite");
  const sources = findTestSources(sourceRoot).sort();
  if (sources.length === 0) {
    throw new Error(`E2E discovery found no authored *.test.ts files under ${sourceRoot}`);
  }

  const manifestPath = path.resolve(compiledRoot, "../../e2e-suite-build-manifest.json");
  if (!fs.existsSync(manifestPath)) {
    throw new Error(`E2E build manifest is missing: ${manifestPath}`);
  }
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8")) as {
    version?: number;
    entries?: Array<{
      source?: string;
      sourceSha256?: string;
      compiled?: string;
      compiledSha256?: string;
    }>;
  };
  if (manifest.version !== 4 || !Array.isArray(manifest.entries)) {
    throw new Error(`E2E build manifest has an unsupported shape: ${manifestPath}`);
  }
  const packageRoot = path.resolve(compiledRoot, "../../..");
  const bySource = new Map(manifest.entries.map((entry) => [entry.source, entry]));
  const sourceRelatives = sources.map((source) =>
    path.relative(packageRoot, source).replace(/\\/g, "/"),
  );
  const unexpected = [...bySource.keys()].filter(
    (source): source is string => typeof source === "string" && !sourceRelatives.includes(source),
  );
  if (bySource.size !== sources.length || unexpected.length > 0) {
    throw new Error(
      `E2E build manifest/source inventory mismatch; sources=${sources.length} ` +
        `manifest=${bySource.size} unexpected=${unexpected.join(",") || "none"}`,
    );
  }

  const hash = (file: string): string =>
    createHash("sha256").update(fs.readFileSync(file)).digest("hex");
  return sources.map((source) => {
    const relative = path.relative(sourceRoot, source).replace(/\.ts$/, ".js");
    const compiled = path.join(compiledRoot, relative);
    if (!fs.existsSync(compiled)) {
      throw new Error(`E2E test source was not compiled: ${source} -> ${compiled}`);
    }
    const sourceRelative = path.relative(packageRoot, source).replace(/\\/g, "/");
    const compiledRelative = path.relative(packageRoot, compiled).replace(/\\/g, "/");
    const attested = bySource.get(sourceRelative);
    if (
      !attested ||
      attested.compiled !== compiledRelative ||
      attested.sourceSha256 !== hash(source) ||
      attested.compiledSha256 !== hash(compiled)
    ) {
      throw new Error(`stale or mismatched E2E build artifact: ${sourceRelative}`);
    }
    return compiled;
  });
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
    timeout: SUITE_TIMEOUT_MS,
    color: true,
  });

  const testsRoot = path.resolve(__dirname);
  const onlyPattern = process.env.VERTER_E2E_ONLY || process.env.E2E_ONLY;
  const sourceRoot = path.resolve(testsRoot, "../../../e2e/suite");

  // A LAUNCH configures ONE server, and Verter's native lane is an initialization
  // option, so a suite that declares a different server profile belongs to a
  // different launch — never this one. The runner launches once per profile in
  // use; this filter is the other half of that split, so a suite can never run
  // against a server configured for someone else's profile.
  const runProfile = launchServerProfile();
  const baseProfile = routeBaseServerProfile();
  const files = discoverCompiledTests(testsRoot).filter((file) => {
    const rel = path.relative(path.join(testsRoot), file).replace(/\\/g, "/");
    // Fixture-scoped discovery: specialty fixtures only load matching suite globs.
    if (!suiteAllowedForFixture(FIXTURE_NAME, rel)) {
      return false;
    }
    if (serverProfileForSuite(rel, baseProfile) !== runProfile) {
      return false;
    }
    if (!onlyPattern) return true;
    return file.replace(/\\/g, "/").includes(onlyPattern.replace(/\\/g, "/"));
  });

  if (files.length === 0) {
    throw new Error(
      `E2E discovery selected 0 suite files for fixture=${FIXTURE_NAME} profile=${runProfile}` +
        (onlyPattern ? ` only=${onlyPattern}` : "") +
        ` (sourceRoot=${sourceRoot})`,
    );
  }

  for (const f of files) {
    mocha.addFile(f);
  }

  // Hand the budget registry the runnable Mocha is currently executing, so a
  // registered parent claim is CHECKED against the deadline in force rather than
  // taken on trust. `runner.currentRunnable` is Mocha's own view of that.
  let activeRunner: Mocha.Runner | undefined;
  setRunnableAccessor(() => activeRunner?.currentRunnable);

  let rootHookError: string | undefined;

  mocha.rootHooks({
    async beforeAll(this: Mocha.Context) {
      // Activation, then provider sync, then file readiness — three waits IN
      // SERIES under this one deadline. Each was individually under the old 60s
      // and their sum was 87s, so a late first wait could kill the second before
      // it reached its own budget and the run would blame the hook, naming none
      // of them. The deadline is derived from the registry's `rootBeforeAll`
      // sequence, which is checked as a SUM.
      this.timeout(sequenceParent("rootBeforeAll"));
      try {
        // The root hook is the ONE place these two may take the large budgets, and
        // it passes them explicitly rather than letting a shared default carry a
        // deadline only this hook has.
        await ensureFixtureWarm(pollBudget("rootExtensionReady"));
        if (TYPE_PROVIDER) {
          await ensureTypeProviderSynced({
            readyBudgetMs: pollBudget("rootExtensionReady"),
            syncBudgetMs: pollBudget("rootTypeProviderSync"),
          });
        }
        // The shared warmup waits for a carrier to answer a typed completion.
        // The extension-hosted provider registers as `TypeProviderKind::Tsserver`
        // and carrier publication is suppressed for that kind, so no `.vue.tsx`
        // companion ever reaches the extension host and a carrier never gets a
        // typed answer — the warmup cannot settle on that route, and its suite
        // skips at `suiteSetup` naming the same defect. Guarding the warmup here
        // lets that suite reach its own fixture-premise checks and report the
        // defect, instead of dying in shared infrastructure 20s earlier. It
        // asserts nothing, so no other route loses anything: every provider that
        // does serve carriers keeps the warmup as its readiness gate. Delete this
        // guard when carrier publication is connected for the extension-hosted
        // topology.
        if (TYPE_PROVIDER !== "extension") {
          await openReadyCached(getAppVuePath());
        }
      } catch (err) {
        rootHookError = err instanceof Error ? `${err.message}\n${err.stack ?? ""}` : String(err);
        throw err;
      }
    },
    afterAll() {
      if (TYPE_PROVIDER === "shared-tsgo") {
        assertSharedTsgoServedWithoutFallback(readTestLog());
      }
    },
  });

  return new Promise((resolve, reject) => {
    const passedTestIds: string[] = [];
    const pendingTestIds: string[] = [];
    const failedTests: Array<{ id: string; err: string; stack?: string }> = [];

    const originalConsoleLog = console.log;
    console.log = (...args: unknown[]) => {
      assertNotVacuousPassLog(args.map(String).join(" "));
      originalConsoleLog(...args);
    };

    const runner: Mocha.Runner = mocha.run((failures: number) => {
      console.log = originalConsoleLog;
      getTimer().flush();

      const stats = runner.stats ?? { passes: 0, failures: 0, pending: 0, tests: 0 };
      const executed = (stats.passes ?? 0) + (stats.failures ?? 0) + (stats.pending ?? 0);
      writeRunSummary({
        fixture: FIXTURE_NAME,
        typeProvider: TYPE_PROVIDER ?? null,
        onlyPattern: onlyPattern ?? null,
        loadedFiles: files.map((f) => path.relative(testsRoot, f).replace(/\\/g, "/")),
        passes: stats.passes ?? 0,
        failures: stats.failures ?? 0,
        pending: stats.pending ?? 0,
        executed,
        passedTestIds,
        pendingTestIds,
        failedTests,
        rootHookError: rootHookError ?? null,
      });

      const failed = failures > 0 || (stats.failures ?? 0) > 0;
      if (failed) {
        const detail =
          failedTests.length > 0
            ? failedTests
                .slice(0, 5)
                .map((f) => `${f.id}: ${f.err}`)
                .join(" | ")
            : "see mocha output";
        reject(new Error(`${Math.max(failures, stats.failures ?? 0)} tests failed — ${detail}`));
        return;
      }
      if (executed === 0) {
        reject(
          new Error(
            `E2E run${onlyPattern ? ` (VERTER_E2E_ONLY=${onlyPattern})` : ""} executed 0 tests — vacuous pass refused` +
              (rootHookError ? `; root hook error: ${rootHookError}` : ""),
          ),
        );
        return;
      }
      resolve();
    });
    // Published as soon as the runner exists, BEFORE any test executes: the budget
    // registry consults it to check each claimed parent against the real deadline.
    activeRunner = runner;
    runner.on("pass", (test) => passedTestIds.push(test.title));
    runner.on("pending", (test) => pendingTestIds.push(test.title));
    runner.on("fail", (test, err) => {
      failedTests.push({
        id: test.title,
        err: err instanceof Error ? err.message : String(err),
        stack: err instanceof Error ? err.stack : undefined,
      });
    });
  });
}
