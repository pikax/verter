/**
 * Full-graph stress test for nuxt-ui component metadata extraction.
 *
 * Discovers ALL Vue components from the nuxt-ui checkout, creates a single
 * @verter/component-meta session, and resolves every component in Expanded
 * mode. Proves the architecture can fully load, cache, and expand the nuxt-ui
 * type graph without duplicate work.
 *
 * Usage:
 *   node --expose-gc --import tsx src/meta-ui-stress.ts
 *   node --expose-gc --import tsx src/meta-ui-stress.ts --per-component-timeout=60000
 */

import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { applyDefaultBenchmarkTransforms } from "./meta-ui-core.js";
import { loadVerterCompatModule } from "./verter-compat.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = resolve(__dirname, "../../..");

// ─── Configuration ─────────────────────────────────────────────────────────

const OUTER_HARD_STOP_MS = 10 * 60 * 1000; // 10 minutes
const DEFAULT_PER_COMPONENT_TIMEOUT_MS = 120_000; // 120 seconds

interface StressConfig {
  uiRoot: string;
  componentsDir: string;
  outputDir: string;
  perComponentTimeoutMs: number;
}

interface ComponentEntry {
  absolutePath: string;
  relativePath: string;
  name: string;
  transformedSource: string;
}

interface ComponentResult {
  name: string;
  file: string;
  status: "success" | "error";
  time_ms: number;
  error?: string;
}

interface StressReport {
  total_discovered: number;
  total_success: number;
  total_failed: number;
  wall_clock_ms: number;
  artifact_path: string;
  components: ComponentResult[];
}

// ─── Helpers ───────────────────────────────────────────────────────────────

function normalizePath(value: string): string {
  return value.replace(/\\/g, "/");
}

function discoverVueFiles(rootDir: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(rootDir, { withFileTypes: true })) {
    const absolutePath = resolve(rootDir, entry.name);
    if (entry.isDirectory()) {
      files.push(...discoverVueFiles(absolutePath));
      continue;
    }
    if (entry.isFile() && entry.name.endsWith(".vue")) {
      files.push(absolutePath);
    }
  }
  return files;
}

function componentNameFromPath(filePath: string): string {
  return basename(filePath, ".vue");
}

function readNuxtCompilerOptions(uiRoot: string): {
  baseUrl: string;
  paths: Record<string, string[]>;
} {
  const baseUrl = resolve(uiRoot, ".nuxt");
  const appTsconfigPath = resolve(baseUrl, "tsconfig.app.json");
  const sharedTsconfigPath = resolve(baseUrl, "tsconfig.shared.json");
  if (!existsSync(appTsconfigPath) || !existsSync(sharedTsconfigPath)) {
    throw new Error(
      `Missing generated Nuxt tsconfig files under ${baseUrl}. Run bench:meta:ui:setup first.`,
    );
  }

  const appTsconfig = JSON.parse(readFileSync(appTsconfigPath, "utf8"));
  const sharedTsconfig = JSON.parse(readFileSync(sharedTsconfigPath, "utf8"));
  return {
    baseUrl: normalizePath(baseUrl),
    paths: {
      ...(appTsconfig.compilerOptions?.paths ?? {}),
      ...(sharedTsconfig.compilerOptions?.paths ?? {}),
    },
  };
}

function buildCheckerConfig(
  uiRoot: string,
  compilerOptions: { baseUrl?: string; paths?: Record<string, string[]> },
  componentPaths: string[],
): Record<string, unknown> {
  return {
    extends: `${normalizePath(uiRoot)}/tsconfig.json`,
    skipLibCheck: true,
    include: componentPaths,
    exclude: [],
    compilerOptions: {
      ...(compilerOptions.baseUrl ? { baseUrl: compilerOptions.baseUrl } : {}),
      ...(compilerOptions.paths ? { paths: compilerOptions.paths } : {}),
    },
  };
}

function parseArgs(argv: string[]): StressConfig {
  const uiRoot = resolve(repoRoot, ".integration-tests", "repos", "nuxt-ui");
  const componentsDir = resolve(uiRoot, "src", "runtime", "components");
  let outputDir = resolve(repoRoot, "packages", "benchmark", "benchmark-results", "meta-ui");
  let perComponentTimeoutMs = DEFAULT_PER_COMPONENT_TIMEOUT_MS;

  for (const arg of argv) {
    if (arg.startsWith("--output-dir=")) {
      outputDir = resolve(arg.slice("--output-dir=".length));
      continue;
    }
    if (arg.startsWith("--per-component-timeout=")) {
      const value = Number.parseInt(arg.slice("--per-component-timeout=".length), 10);
      if (!Number.isFinite(value) || value <= 0) {
        throw new Error("--per-component-timeout must be a positive integer (ms).");
      }
      perComponentTimeoutMs = value;
    }
  }

  return { uiRoot, componentsDir, outputDir, perComponentTimeoutMs };
}

function stressArtifactPath(outputDir: string): string {
  return normalizePath(join(outputDir, "meta-ui-stress.json"));
}

function writeStressArtifact(outputDir: string, report: StressReport): string {
  const artifactPath = stressArtifactPath(outputDir);
  mkdirSync(dirname(artifactPath), { recursive: true });
  writeFileSync(artifactPath, JSON.stringify(report, null, 2));
  return artifactPath;
}

// ─── Setup ─────────────────────────────────────────────────────────────────

function setupAndDiscover(config: StressConfig): ComponentEntry[] {
  if (!existsSync(config.uiRoot)) {
    console.error(`FATAL: nuxt-ui checkout not found at ${config.uiRoot}`);
    console.error("Run: pnpm bench:meta:ui:setup");
    process.exit(1);
  }

  if (!existsSync(config.componentsDir)) {
    console.error(`FATAL: components directory not found at ${config.componentsDir}`);
    console.error("Run: pnpm bench:meta:ui:setup");
    process.exit(1);
  }

  const rawFiles = discoverVueFiles(config.componentsDir);
  if (rawFiles.length === 0) {
    console.error(`FATAL: 0 Vue components found in ${config.componentsDir}`);
    process.exit(1);
  }

  const entries: ComponentEntry[] = rawFiles
    .map((absolutePath) => ({
      absolutePath: normalizePath(absolutePath),
      relativePath: normalizePath(relative(config.uiRoot, absolutePath)),
      name: componentNameFromPath(absolutePath),
      transformedSource: applyDefaultBenchmarkTransforms(readFileSync(absolutePath, "utf8")),
    }))
    .sort((a, b) => a.relativePath.localeCompare(b.relativePath, undefined, { sensitivity: "base" }));

  return entries;
}

// ─── Query with timeout ────────────────────────────────────────────────────

function queryWithTimeout(
  checker: any,
  absolutePath: string,
  timeoutMs: number,
): Promise<{ ok: true; meta: any } | { ok: false; error: string }> {
  return new Promise((resolveResult) => {
    const timer = setTimeout(() => {
      resolveResult({ ok: false, error: "timeout" });
    }, timeoutMs);

    const queryPromise: Promise<any> = checker.getComponentMeta(absolutePath);
    queryPromise
      .then((meta: any) => {
        clearTimeout(timer);
        resolveResult({ ok: true, meta });
      })
      .catch((err: any) => {
        clearTimeout(timer);
        resolveResult({
          ok: false,
          error: err instanceof Error ? err.message : String(err),
        });
      });
  });
}

// ─── Main ──────────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  // Hard-stop outer timeout
  const hardStopTimer = setTimeout(() => {
    console.error("FATAL: 10-minute outer hard-stop timeout exceeded. Exiting.");
    process.exit(2);
  }, OUTER_HARD_STOP_MS);
  hardStopTimer.unref();

  const config = parseArgs(process.argv.slice(2));
  const components = setupAndDiscover(config);

  console.error(`Discovered ${components.length} Vue components`);
  console.error(`Per-component timeout: ${config.perComponentTimeoutMs}ms`);

  // Read Nuxt compiler options for path aliases
  const compilerOptions = readNuxtCompilerOptions(config.uiRoot);
  const checkerConfig = buildCheckerConfig(
    config.uiRoot,
    compilerOptions,
    components.map((c) => c.absolutePath),
  );

  // Initialize backend via the same compat layer the benchmark uses
  console.error("Initializing Verter checker (Expanded mode)...");
  const initStart = performance.now();
  const { createCheckerByJson } = await loadVerterCompatModule();
  const checker = await createCheckerByJson(normalizePath(config.uiRoot), checkerConfig, {
    forceUseTs: true,
    schema: { literalBooleanSchema: true },
    runtimeMode: "dedicated",
    typeExpansionBackend: "verter",
  });
  const initMs = performance.now() - initStart;
  console.error(`Checker initialized in ${Math.round(initMs)}ms`);

  // Feed all component sources into the checker
  console.error("Upserting component sources...");
  for (const entry of components) {
    checker.updateFile(entry.absolutePath, entry.transformedSource);
  }

  // Resolve every component
  const wallStart = performance.now();
  const results: ComponentResult[] = [];
  let successCount = 0;
  let failCount = 0;

  for (let i = 0; i < components.length; i++) {
    const entry = components[i];
    const componentStart = performance.now();
    const outcome = await queryWithTimeout(
      checker,
      entry.absolutePath,
      config.perComponentTimeoutMs,
    );
    const elapsed = performance.now() - componentStart;

    if (outcome.ok) {
      successCount++;
      results.push({
        name: entry.name,
        file: entry.relativePath,
        status: "success",
        time_ms: Math.round(elapsed),
      });
      console.error(
        `  [${i + 1}/${components.length}] ${entry.name} OK (${Math.round(elapsed)}ms)`,
      );
    } else {
      failCount++;
      results.push({
        name: entry.name,
        file: entry.relativePath,
        status: "error",
        time_ms: Math.round(elapsed),
        error: outcome.error,
      });
      console.error(
        `  [${i + 1}/${components.length}] ${entry.name} FAILED: ${outcome.error} (${Math.round(elapsed)}ms)`,
      );
    }
  }

  const wallClockMs = performance.now() - wallStart;

  // Dispose checker
  if (typeof checker.dispose === "function") {
    await checker.dispose();
  } else if (typeof checker.close === "function") {
    checker.close();
  }

  // Output machine-readable JSON report to stdout
  const artifactPath = stressArtifactPath(config.outputDir);
  const report: StressReport = {
    total_discovered: components.length,
    total_success: successCount,
    total_failed: failCount,
    wall_clock_ms: Math.round(wallClockMs),
    artifact_path: artifactPath,
    components: results,
  };

  writeStressArtifact(config.outputDir, report);

  console.log(JSON.stringify(report, null, 2));

  console.error(
    `\nDone: ${successCount}/${components.length} success, ${failCount} failed, wall=${Math.round(wallClockMs)}ms`,
  );
  console.error(`Artifact: ${artifactPath}`);

  clearTimeout(hardStopTimer);
  process.exit(failCount > 0 ? 1 : 0);
}

main().catch((err) => {
  console.error("FATAL:", err);
  process.exit(2);
});
