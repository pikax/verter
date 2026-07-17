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
    timeout: 15_000,
    color: true,
  });

  const testsRoot = path.resolve(__dirname);
  const onlyPattern = process.env.VERTER_E2E_ONLY || process.env.E2E_ONLY;
  const sourceRoot = path.resolve(testsRoot, "../../../e2e/suite");

  const files = discoverCompiledTests(testsRoot).filter((file) => {
    const rel = path.relative(path.join(testsRoot), file).replace(/\\/g, "/");
    // Fixture-scoped discovery: specialty fixtures only load matching suite globs.
    if (!suiteAllowedForFixture(FIXTURE_NAME, rel)) {
      return false;
    }
    if (!onlyPattern) return true;
    return file.replace(/\\/g, "/").includes(onlyPattern.replace(/\\/g, "/"));
  });

  if (files.length === 0) {
    throw new Error(
      `E2E discovery selected 0 suite files for fixture=${FIXTURE_NAME}` +
        (onlyPattern ? ` only=${onlyPattern}` : "") +
        ` (sourceRoot=${sourceRoot})`,
    );
  }

  for (const f of files) {
    mocha.addFile(f);
  }

  let rootHookError: string | undefined;

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

    const runner = mocha.run((failures: number) => {
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
