import { createRequire } from "node:module";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  applyDefaultBenchmarkTransforms,
  compareNormalizedArtifacts,
  rotateComponentOrder,
  summarizeLatencySeries,
  type ArtifactComparison,
  type MetaUiBackend,
  type MetaUiOutcomeBucket,
  type MetaUiScenario,
  type NormalizedMetaArtifact,
} from "./meta-ui-core.js";
import {
  aggregateRunFromRepeats,
  buildSlaCount,
  type ComponentResultRow,
  type MetaUiBenchmarkRun,
} from "./meta-ui-report.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = resolve(__dirname, "../../..");
const require = createRequire(import.meta.url);
const tsxLoaderPath = pathToFileURL(require.resolve("tsx")).href;

const JSON_MODE = process.argv.includes("--json");

const SUPPORTED_BACKENDS: MetaUiBackend[] = ["vue-component-meta", "verter"];
const SUPPORTED_SCENARIOS: MetaUiScenario[] = [
  "single_cold",
  "single_warm",
  "repo_first_pass",
  "repo_warm_second_pass",
];
export const EXPECTED_ARTIFACTS_MANIFEST = "meta-ui-expected-manifest.json";

interface MetaUiBenchArgs {
  uiRoot: string;
  backends: MetaUiBackend[];
  scenarios: MetaUiScenario[];
  repeats: number;
  warmupPasses: number;
  /// Kill threshold for runaway requests. Aliased from the legacy
  /// `--query-timeout-ms` flag.
  queryTimeoutMs: number;
  /// SLA threshold (metric only). Components that
  /// resolve above `slaMs` but below `queryTimeoutMs` are tallied as
  /// `slaCount.exceededSla` but allowed to complete; under or equal
  /// counts as `slaCount.withinSla`. CI compares
  /// `slaCount.withinSla` regression against the committed baseline
  /// in packages/benchmark/baselines/.
  slaMs: number;
  jsAudit: boolean;
  components: string[];
  limit: number | null;
  expected: "vue-component-meta" | "none";
  expectedDir: string;
  buildExpectedOnly: boolean;
  outputDir: string;
}

interface PreparedComponentSnapshot {
  absolutePath: string;
  relativePath: string;
  transformedSource: string;
}

interface PreparedProject {
  uiRoot: string;
  componentsDir: string;
  resolvedTargetSha: string;
  componentSnapshots: PreparedComponentSnapshot[];
  compilerOptions: {
    baseUrl?: string;
    paths?: Record<string, string[]>;
  };
}

interface ExpectedArtifactsManifest {
  resolvedTargetSha: string;
  componentPaths: string[];
}

interface BackendInstance {
  query(component: PreparedComponentSnapshot): Promise<MeasuredQueryResult>;
  dispose(): Promise<void> | void;
  isAvailable(): boolean;
  /**
   * return the stderr text the worker emitted
   * between the query-send and query-end of the most recent
   * `query()` call (or null when no query has run yet). Used by
   * the runner to write a per-component sidecar log on failure.
   *
   * Returns `null` when the backend does not support per-query
   * stderr capture.
   */
  takeLastQueryStderr?(): string | null;
}

interface MeasuredQueryResult {
  artifact: NormalizedMetaArtifact;
  latencyMs: number;
  outcome: MetaUiOutcomeBucket;
}

interface WorkerInitPayload {
  backend: MetaUiBackend;
  uiRoot: string;
  checkerConfig: Record<string, unknown>;
  components: PreparedComponentSnapshot[];
}

interface QueryWorkerOptions {
  workerEntryPath?: string;
  queryTimeoutMs?: number;
  setupTimeoutMs?: number;
}

interface WorkerReadyMessage {
  type: "ready";
}

interface WorkerResultMessage {
  type: "result";
  requestId: number;
  result: MeasuredQueryResult;
}

interface WorkerErrorMessage {
  type: "error";
  requestId: number;
  message: string;
  stack?: string;
}

interface WorkerFatalMessage {
  type: "fatal";
  message: string;
  stack?: string;
}

type WorkerMessage =
  | WorkerReadyMessage
  | WorkerResultMessage
  | WorkerErrorMessage
  | WorkerFatalMessage;

// SLA-vs-hard-timeout split:
//   - DEFAULT_SLA_MS measures responsiveness (within-SLA / exceeded-SLA
//     buckets). Components above this threshold are reported but not
//     killed; the within-SLA count drives the CI regression gate.
//   - DEFAULT_HARD_TIMEOUT_MS is the actual kill threshold. The
//     pre-Step-10 single 250 ms threshold conflated metric and kill;
//     splitting them lets us tighten the SLA without prematurely
//     terminating slow but still-finishing requests.
const DEFAULT_SLA_MS = 250;
const DEFAULT_HARD_TIMEOUT_MS = 5_000;
/// Backwards-compatible alias retained so existing JSON consumers and
/// older invocations passing --query-timeout-ms continue to work
/// (the legacy flag aliases --hard-timeout-ms with a stderr deprecation
/// warning emitted by `parseMetaUiBenchArgs`).
const DEFAULT_QUERY_TIMEOUT_MS = DEFAULT_HARD_TIMEOUT_MS;
const DEFAULT_SETUP_TIMEOUT_MS = 30_000;

type GlobalWithOptionalGc = typeof globalThis & {
  gc?: () => void;
};

function log(message: string): void {
  if (!JSON_MODE) {
    process.stdout.write(message);
  }
}

function logLine(message: string): void {
  if (!JSON_MODE) {
    console.log(message);
  }
}

export function maybeRunGarbageCollection(): void {
  (globalThis as GlobalWithOptionalGc).gc?.();
}

function formatHeapUsageMb(): string {
  return `${Math.round(process.memoryUsage().heapUsed / 1024 / 1024)}MB`;
}

function logProgress(
  prefix: string,
  component: PreparedComponentSnapshot,
  index: number,
  total: number,
  detail: string,
  jsAudit: boolean,
): void {
  const auditSuffix = jsAudit ? ` heap=${formatHeapUsageMb()}` : "";
  logLine(`${prefix} ${index}/${total} ${component.relativePath} ${detail}${auditSuffix}`);
}

/**
 * detect unquoted CSV in `--scenarios=` / `--backends=` /
 * `--components=`. When the user runs (typically in a shell that
 * splits unquoted commas):
 *
 *   pnpm bench:meta:ui -- --scenarios=single_cold repo_first_pass
 *
 * the `repo_first_pass` token arrives as a positional arg (no flag
 * prefix). The argv parser silently ignores it, leaving the user
 * with a benchmark run that does NOT include the second scenario.
 * The correct quoted form is documented in `packages/benchmark/
 * README.md`:
 *
 *   pnpm bench:meta:ui -- --scenarios="single_cold,repo_first_pass"
 *
 * This helper returns the list of positional tokens that look like
 * known scenario / backend names so the parser can warn the user.
 *
 * Pure helper, exported for the discriminating
 * `bench_meta_ui_per_component_isolation` test.
 */
export function detectUnquotedCsvSpillover(argv: readonly string[]): {
  scenarioSpillover: string[];
  backendSpillover: string[];
  unrecognizedPositional: string[];
} {
  const scenarioSpillover: string[] = [];
  const backendSpillover: string[] = [];
  const unrecognizedPositional: string[] = [];
  for (const arg of argv) {
    if (
      arg.startsWith("--") ||
      arg === "--json" ||
      arg === "--build-expected-only" ||
      arg === "--js-audit"
    ) {
      continue;
    }
    if ((SUPPORTED_SCENARIOS as readonly string[]).includes(arg)) {
      scenarioSpillover.push(arg);
      continue;
    }
    if ((SUPPORTED_BACKENDS as readonly string[]).includes(arg)) {
      backendSpillover.push(arg);
      continue;
    }
    unrecognizedPositional.push(arg);
  }
  return { scenarioSpillover, backendSpillover, unrecognizedPositional };
}

export function parseMetaUiBenchArgs(argv: string[]): MetaUiBenchArgs {
  const defaultOutputDir = resolve(
    repoRoot,
    "packages",
    "benchmark",
    "benchmark-results",
    "meta-ui",
  );
  const args: MetaUiBenchArgs = {
    uiRoot: resolve(repoRoot, ".integration-tests", "repos", "nuxt-ui"),
    backends: ["verter"],
    scenarios: [...SUPPORTED_SCENARIOS],
    repeats: 1,
    warmupPasses: 1,
    queryTimeoutMs: DEFAULT_HARD_TIMEOUT_MS,
    slaMs: DEFAULT_SLA_MS,
    jsAudit: false,
    components: [],
    limit: null,
    expected: "vue-component-meta",
    expectedDir: resolve(defaultOutputDir, ".expected-vue-component-meta"),
    buildExpectedOnly: false,
    outputDir: defaultOutputDir,
  };
  let expectedDirExplicit = false;

  // detect unquoted CSV before the per-flag walk; the warning
  // surfaces on stderr so it does not corrupt --json output, and the
  // walk still proceeds so the parser remains backward-compatible.
  const spillover = detectUnquotedCsvSpillover(argv);
  if (spillover.scenarioSpillover.length > 0) {
    // Reconstruct the most likely intended CSV by combining the
    // existing `--scenarios=...` value with the spillover tokens.
    const existingScenariosValue =
      argv.find((arg) => arg.startsWith("--scenarios="))?.slice("--scenarios=".length) ?? "";
    const reconstructedCsv = [existingScenariosValue, ...spillover.scenarioSpillover]
      .filter(Boolean)
      .join(",");
    process.stderr.write(
      `warning: positional argument(s) [${spillover.scenarioSpillover.join(
        ", ",
      )}] look like scenario names — did you mean to write\n` +
        `         --scenarios="${reconstructedCsv}" (quoted CSV)? Unquoted CSV with whitespace splits in most\n` +
        `         shells; see packages/benchmark/README.md.\n`,
    );
  }
  if (spillover.backendSpillover.length > 0) {
    const existingBackendsValue =
      argv.find((arg) => arg.startsWith("--backends="))?.slice("--backends=".length) ?? "";
    const reconstructedCsv = [existingBackendsValue, ...spillover.backendSpillover]
      .filter(Boolean)
      .join(",");
    process.stderr.write(
      `warning: positional argument(s) [${spillover.backendSpillover.join(
        ", ",
      )}] look like backend names — did you mean to write\n` +
        `         --backends="${reconstructedCsv}" (quoted CSV)? See packages/benchmark/README.md.\n`,
    );
  }

  for (const arg of argv) {
    if (arg === "--json") {
      continue;
    }
    if (arg.startsWith("--ui-root=")) {
      args.uiRoot = resolve(arg.slice("--ui-root=".length));
      continue;
    }
    if (arg.startsWith("--backends=")) {
      args.backends = parseCsv(arg.slice("--backends=".length), SUPPORTED_BACKENDS);
      continue;
    }
    if (arg.startsWith("--scenarios=")) {
      args.scenarios = parseCsv(arg.slice("--scenarios=".length), SUPPORTED_SCENARIOS);
      continue;
    }
    if (arg.startsWith("--repeats=")) {
      args.repeats = parsePositiveInt(arg.slice("--repeats=".length), "repeats");
      continue;
    }
    if (arg.startsWith("--warmup-passes=")) {
      args.warmupPasses = parseNonNegativeInt(
        arg.slice("--warmup-passes=".length),
        "warmup-passes",
      );
      continue;
    }
    if (arg.startsWith("--hard-timeout-ms=")) {
      args.queryTimeoutMs = parseNonNegativeInt(
        arg.slice("--hard-timeout-ms=".length),
        "hard-timeout-ms",
      );
      continue;
    }
    if (arg.startsWith("--sla-ms=")) {
      args.slaMs = parseNonNegativeInt(arg.slice("--sla-ms=".length), "sla-ms");
      continue;
    }
    if (arg.startsWith("--query-timeout-ms=")) {
      // --query-timeout-ms is deprecated. Aliases --hard-timeout-ms
      // with a stderr deprecation warning.
      const value = parseNonNegativeInt(
        arg.slice("--query-timeout-ms=".length),
        "query-timeout-ms",
      );
      args.queryTimeoutMs = value;
      process.stderr.write(
        "warning: --query-timeout-ms is deprecated; use --hard-timeout-ms instead\n",
      );
      continue;
    }
    if (arg === "--js-audit") {
      args.jsAudit = true;
      continue;
    }
    if (arg.startsWith("--components=")) {
      args.components = arg
        .slice("--components=".length)
        .split(",")
        .map((value) => value.trim())
        .filter(Boolean);
      continue;
    }
    if (arg.startsWith("--limit=")) {
      args.limit = parsePositiveInt(arg.slice("--limit=".length), "limit");
      continue;
    }
    if (arg.startsWith("--expected=")) {
      const expected = arg.slice("--expected=".length);
      if (expected !== "vue-component-meta" && expected !== "none") {
        throw new Error(`Unsupported --expected value: ${expected}`);
      }
      args.expected = expected;
      continue;
    }
    if (arg.startsWith("--expected-dir=")) {
      args.expectedDir = resolve(arg.slice("--expected-dir=".length));
      expectedDirExplicit = true;
      continue;
    }
    if (arg === "--build-expected-only") {
      args.buildExpectedOnly = true;
      continue;
    }
    if (arg.startsWith("--output-dir=")) {
      args.outputDir = resolve(arg.slice("--output-dir=".length));
      if (!expectedDirExplicit) {
        args.expectedDir = resolve(args.outputDir, ".expected-vue-component-meta");
      }
      continue;
    }
  }

  return args;
}

function parseCsv<T extends string>(value: string, supported: readonly T[]): T[] {
  const requested = value
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);
  const invalid = requested.filter((entry) => !supported.includes(entry as T));
  if (invalid.length > 0) {
    throw new Error(`Unsupported values: ${invalid.join(", ")}.`);
  }
  return requested as T[];
}

function parsePositiveInt(value: string, label: string): number {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    throw new Error(`--${label} must be a positive integer.`);
  }
  return parsed;
}

function parseNonNegativeInt(value: string, label: string): number {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed) || parsed < 0) {
    throw new Error(`--${label} must be a non-negative integer.`);
  }
  return parsed;
}

export function prepareMetaUiProject(args: MetaUiBenchArgs): PreparedProject {
  const uiRoot = resolve(args.uiRoot);
  const componentsDir = resolve(uiRoot, "src", "runtime", "components");
  if (!existsSync(componentsDir)) {
    throw new Error(`Could not find ${componentsDir}. Run bench:meta:ui:setup first.`);
  }

  const compilerOptions = readNuxtCompilerOptions(uiRoot);
  const resolvedTargetSha = readGitSha(uiRoot);
  const requestedNames = new Set(args.components.map((value) => normalizePath(value)));

  let componentSnapshots = discoverVueComponentFiles(componentsDir)
    .map((absolutePath) => {
      const transformedSource = applyDefaultBenchmarkTransforms(readFileSync(absolutePath, "utf8"));
      return {
        absolutePath: normalizePath(absolutePath),
        relativePath: normalizePath(relative(uiRoot, absolutePath)),
        transformedSource,
      };
    })
    .sort((left, right) =>
      left.relativePath.localeCompare(right.relativePath, undefined, { sensitivity: "base" }),
    );

  if (requestedNames.size > 0) {
    componentSnapshots = componentSnapshots.filter((component) => {
      const fileName = component.relativePath.split("/").pop() ?? component.relativePath;
      return (
        requestedNames.has(fileName) ||
        requestedNames.has(component.relativePath) ||
        requestedNames.has(component.absolutePath)
      );
    });
  }

  if (args.limit !== null) {
    componentSnapshots = componentSnapshots.slice(0, args.limit);
  }

  return {
    uiRoot: normalizePath(uiRoot),
    componentsDir: normalizePath(componentsDir),
    resolvedTargetSha,
    componentSnapshots,
    compilerOptions,
  };
}

function discoverVueComponentFiles(rootDir: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(rootDir, { withFileTypes: true })) {
    const absolutePath = resolve(rootDir, entry.name);
    if (entry.isDirectory()) {
      files.push(...discoverVueComponentFiles(absolutePath));
      continue;
    }
    if (entry.isFile() && entry.name.endsWith(".vue")) {
      files.push(absolutePath);
    }
  }
  return files;
}

function readNuxtCompilerOptions(uiRoot: string) {
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
  prepared: PreparedProject,
  componentPaths: PreparedComponentSnapshot[],
): Record<string, unknown> {
  return {
    extends: `${prepared.uiRoot}/tsconfig.json`,
    skipLibCheck: true,
    include: componentPaths.map((component) => tryResolveTypesDeclaration(component.absolutePath)),
    exclude: [],
    compilerOptions: {
      ...(prepared.compilerOptions.baseUrl ? { baseUrl: prepared.compilerOptions.baseUrl } : {}),
      ...(prepared.compilerOptions.paths ? { paths: prepared.compilerOptions.paths } : {}),
    },
  };
}

function tryResolveTypesDeclaration(fullPath: string): string {
  if (!fullPath.includes("node_modules") || !fullPath.endsWith(".vue")) {
    return fullPath;
  }

  const patterns = [
    fullPath.replace(".vue", ".d.vue.ts"),
    fullPath.replace(".vue", ".vue.d.ts"),
    fullPath.replace(".vue", ".d.ts"),
  ];

  for (const candidate of patterns) {
    if (existsSync(candidate)) {
      return candidate;
    }
  }

  return fullPath;
}

async function createBackendInstance(
  prepared: PreparedProject,
  args: MetaUiBenchArgs,
  backend: MetaUiBackend,
  componentPaths: PreparedComponentSnapshot[],
): Promise<BackendInstance> {
  const checkerConfig = buildCheckerConfig(prepared, componentPaths);
  return createWorkerBackendInstance(
    {
      backend,
      uiRoot: prepared.uiRoot,
      checkerConfig,
      components: componentPaths,
    },
    {
      queryTimeoutMs: args.queryTimeoutMs,
    },
  );
}

export async function createWorkerBackendInstance(
  payload: WorkerInitPayload,
  options: QueryWorkerOptions = {},
): Promise<BackendInstance> {
  const workerEntryPath = options.workerEntryPath ?? resolve(__dirname, "meta-ui-query-worker.ts");
  const queryTimeoutMs = options.queryTimeoutMs ?? DEFAULT_QUERY_TIMEOUT_MS;
  const setupTimeoutMs = options.setupTimeoutMs ?? DEFAULT_SETUP_TIMEOUT_MS;
  const child = spawn(
    process.execPath,
    ["--expose-gc", "--import", tsxLoaderPath, workerEntryPath],
    {
      cwd: repoRoot,
      stdio: ["ignore", "ignore", "pipe", "ipc"],
    },
  );
  const stderr: string[] = [];
  child.stderr?.setEncoding("utf8");
  child.stderr?.on("data", (chunk: string) => {
    stderr.push(chunk);
  });

  const waitForReady = new Promise<void>((resolveReady, rejectReady) => {
    let settled = false;
    const readyTimer =
      setupTimeoutMs > 0
        ? setTimeout(() => {
            finalizeReadyReject(
              new Error(`meta-ui backend setup timed out after ${setupTimeoutMs}ms`),
              true,
            );
          }, setupTimeoutMs)
        : null;

    const cleanup = () => {
      if (readyTimer) {
        clearTimeout(readyTimer);
      }
      child.off("message", onMessage);
      child.off("exit", onExit);
      child.off("error", onError);
    };

    const finalizeReadyResolve = () => {
      if (settled) {
        return;
      }
      settled = true;
      cleanup();
      resolveReady();
    };

    const finalizeReadyReject = (error: Error, terminate: boolean) => {
      if (settled) {
        return;
      }
      settled = true;
      cleanup();
      if (terminate) {
        child.kill("SIGKILL");
      }
      rejectReady(enrichWorkerError(error, stderr));
    };

    const onMessage = (message: WorkerMessage) => {
      if (message?.type === "ready") {
        finalizeReadyResolve();
        return;
      }
      if (message?.type === "fatal") {
        finalizeReadyReject(new Error(message.message), true);
      }
    };

    const onExit = (code: number | null, signal: NodeJS.Signals | null) => {
      finalizeReadyReject(
        new Error(`meta-ui backend worker exited before ready (code=${code}, signal=${signal})`),
        false,
      );
    };

    const onError = (error: Error) => {
      finalizeReadyReject(error, false);
    };

    child.on("message", onMessage);
    child.on("exit", onExit);
    child.on("error", onError);
    child.send({ type: "init", payload });
  });

  await waitForReady;
  return new WorkerBackendInstance(child, stderr, queryTimeoutMs);
}

class WorkerBackendInstance implements BackendInstance {
  private readonly child: ChildProcess;
  private readonly stderr: string[];
  private readonly queryTimeoutMs: number;
  private readonly pending = new Map<
    number,
    {
      resolve: (value: MeasuredQueryResult) => void;
      reject: (reason?: unknown) => void;
      timer: NodeJS.Timeout | null;
      // snapshot of `this.stderr.length` at
      // query-send time. The slice from this index to the array's
      // end at query-completion captures the stderr emitted during
      // this query.
      stderrChunkStart: number;
    }
  >();
  private nextRequestId = 1;
  private unavailableError: Error | null = null;
  // stderr captured during the most recent
  // query, surfaced via `takeLastQueryStderr()`. Cleared on the
  // next query-send to keep the sidecar log per-component-precise.
  private lastQueryStderr: string | null = null;

  constructor(child: ChildProcess, stderr: string[], queryTimeoutMs: number) {
    this.child = child;
    this.stderr = stderr;
    this.queryTimeoutMs = queryTimeoutMs;
    this.child.on("message", this.onMessage);
    this.child.on("exit", this.onExit);
    this.child.on("error", this.onError);
  }

  isAvailable(): boolean {
    return this.unavailableError === null;
  }

  takeLastQueryStderr(): string | null {
    const value = this.lastQueryStderr;
    this.lastQueryStderr = null;
    return value;
  }

  async query(component: PreparedComponentSnapshot): Promise<MeasuredQueryResult> {
    if (this.unavailableError) {
      throw this.unavailableError;
    }
    const requestId = this.nextRequestId++;
    // Snapshot the stderr buffer so the post-query slice captures
    // only what this component triggered.
    const stderrChunkStart = this.stderr.length;
    return new Promise<MeasuredQueryResult>((resolveResult, rejectResult) => {
      const timer =
        this.queryTimeoutMs > 0
          ? setTimeout(() => {
              const timeoutError = new Error(
                `meta-ui query timed out after ${this.queryTimeoutMs}ms while resolving ${component.relativePath}`,
              );
              this.captureQueryStderr(stderrChunkStart);
              this.markUnavailable(timeoutError, true);
            }, this.queryTimeoutMs)
          : null;
      this.pending.set(requestId, {
        resolve: resolveResult,
        reject: rejectResult,
        timer,
        stderrChunkStart,
      });
      this.child.send({ type: "query", requestId, component });
    });
  }

  private captureQueryStderr(chunkStart: number): void {
    if (chunkStart < 0 || chunkStart > this.stderr.length) {
      this.lastQueryStderr = "";
      return;
    }
    this.lastQueryStderr = this.stderr.slice(chunkStart).join("");
  }

  async dispose(): Promise<void> {
    this.child.off("message", this.onMessage);
    this.child.off("exit", this.onExit);
    this.child.off("error", this.onError);
    this.markUnavailable(new Error("meta-ui backend worker disposed"), false);
    if (!this.child.killed) {
      this.child.kill("SIGKILL");
    }
  }

  private readonly onMessage = (message: WorkerMessage) => {
    if (message?.type === "result") {
      const pending = this.pending.get(message.requestId);
      if (!pending) {
        return;
      }
      if (pending.timer) {
        clearTimeout(pending.timer);
      }
      this.pending.delete(message.requestId);
      // capture per-query stderr (success path)
      // so callers that always want the sidecar log can read it.
      this.captureQueryStderr(pending.stderrChunkStart);
      pending.resolve(message.result);
      return;
    }

    if (message?.type === "error") {
      const pending = this.pending.get(message.requestId);
      if (!pending) {
        return;
      }
      if (pending.timer) {
        clearTimeout(pending.timer);
      }
      this.pending.delete(message.requestId);
      // capture per-query stderr on the error
      // path before the rejection so the sidecar log captures the
      // exact window of worker output that produced the failure.
      this.captureQueryStderr(pending.stderrChunkStart);
      const error = new Error(message.message);
      if (message.stack) {
        error.stack = message.stack;
      }
      pending.reject(error);
      return;
    }

    if (message?.type === "fatal") {
      this.markUnavailable(new Error(message.message), true);
    }
  };

  private readonly onExit = (code: number | null, signal: NodeJS.Signals | null) => {
    if (this.unavailableError) {
      return;
    }
    this.markUnavailable(
      new Error(`meta-ui backend worker exited unexpectedly (code=${code}, signal=${signal})`),
      false,
    );
  };

  private readonly onError = (error: Error) => {
    this.markUnavailable(error, false);
  };

  private markUnavailable(error: Error, terminate: boolean): void {
    const finalError = enrichWorkerError(error, this.stderr);
    if (!this.unavailableError) {
      this.unavailableError = finalError;
    }
    for (const [requestId, pending] of this.pending) {
      if (pending.timer) {
        clearTimeout(pending.timer);
      }
      pending.reject(finalError);
      this.pending.delete(requestId);
    }
    if (terminate && !this.child.killed) {
      this.child.kill("SIGKILL");
    }
  }
}

function enrichWorkerError(error: Error, stderr: string[]): Error {
  const stderrText = stderr.join("").trim();
  if (stderrText.length === 0) {
    return error;
  }
  return new Error(`${error.message}\n${stderrText}`);
}

async function buildExpectedArtifacts(
  prepared: PreparedProject,
  args: MetaUiBenchArgs,
  expected: "vue-component-meta" | "none",
  expectedDir: string,
): Promise<Map<string, string>> {
  if (expected === "none") {
    return new Map();
  }
  const artifacts = new Map<string, string>();

  const total = prepared.componentSnapshots.length;
  for (const [index, component] of prepared.componentSnapshots.entries()) {
    const instance = await createBackendInstance(prepared, args, "vue-component-meta", [component]);
    try {
      const { artifact, latencyMs, outcome } = await executeMeasuredQuery(instance, component);
      const filePath = resolve(expectedDir, `${component.relativePath}.json`);
      mkdirSync(dirname(filePath), { recursive: true });
      writeFileSync(filePath, JSON.stringify(artifact));
      artifacts.set(component.relativePath, filePath);
      logProgress(
        "[expected]",
        component,
        index + 1,
        total,
        `baseline-ready outcome=${outcome} latency=${latencyMs.toFixed(2)}ms`,
        args.jsAudit,
      );
    } finally {
      await instance.dispose();
      maybeRunGarbageCollection();
    }
  }

  writeExpectedArtifactsManifest(prepared, expectedDir);

  return artifacts;
}

function writeExpectedArtifactsManifest(prepared: PreparedProject, expectedDir: string): void {
  mkdirSync(expectedDir, { recursive: true });
  const manifest: ExpectedArtifactsManifest = {
    resolvedTargetSha: prepared.resolvedTargetSha,
    componentPaths: prepared.componentSnapshots.map((component) => component.relativePath),
  };
  writeFileSync(
    resolve(expectedDir, EXPECTED_ARTIFACTS_MANIFEST),
    JSON.stringify(manifest, null, 2),
  );
}

export function tryLoadExpectedArtifacts(
  prepared: PreparedProject,
  expectedDir: string,
): Map<string, string> | null {
  const manifestPath = resolve(expectedDir, EXPECTED_ARTIFACTS_MANIFEST);
  if (!existsSync(manifestPath)) {
    return null;
  }

  const manifest = JSON.parse(readFileSync(manifestPath, "utf8")) as ExpectedArtifactsManifest;
  const expectedComponentPaths = prepared.componentSnapshots.map(
    (component) => component.relativePath,
  );
  if (manifest.resolvedTargetSha !== prepared.resolvedTargetSha) {
    return null;
  }
  if (
    manifest.componentPaths.length !== expectedComponentPaths.length ||
    manifest.componentPaths.some(
      (componentPath, index) => componentPath !== expectedComponentPaths[index],
    )
  ) {
    return null;
  }

  const artifacts = new Map<string, string>();
  for (const component of prepared.componentSnapshots) {
    const artifactPath = resolve(expectedDir, `${component.relativePath}.json`);
    if (!existsSync(artifactPath)) {
      return null;
    }
    artifacts.set(component.relativePath, artifactPath);
  }

  return artifacts;
}

async function executeMeasuredQuery(
  instance: BackendInstance,
  component: PreparedComponentSnapshot,
) {
  return instance.query(component);
}

function classifyFailure(error: unknown): MetaUiOutcomeBucket {
  const message = error instanceof Error ? error.message : String(error);
  return /(closed|shutdown|terminated|crash|disconnect|EPIPE|broken pipe)/i.test(message)
    ? "crash"
    : "query_error";
}

/**
 * write a per-component stderr sidecar log.
 *
 * Sidecar path: `<outputDir>/sidecar-logs/<relativePath>.stderr.log`
 * Directory structure mirrors the component's path so two
 * components with the same basename do not collide.
 *
 * Returns the absolute path of the written sidecar, or `null` when
 * the captured stderr was empty (no file is written).
 *
 * Pure helper — exported for the discriminating
 * `bench_meta_ui_per_component_isolation` test.
 */
export function writeComponentStderrSidecar(
  outputDir: string,
  relativePath: string,
  stderrText: string | null,
  reason:
    | { kind: "success"; latencyMs: number; outcome: MetaUiOutcomeBucket }
    | { kind: "failure"; outcome: MetaUiOutcomeBucket; errorMessage: string },
): string | null {
  if (!stderrText || stderrText.length === 0) {
    return null;
  }
  const sidecarDir = resolve(outputDir, "sidecar-logs");
  const sidecarPath = resolve(sidecarDir, `${relativePath}.stderr.log`);
  mkdirSync(dirname(sidecarPath), { recursive: true });
  const header =
    reason.kind === "success"
      ? `# Component: ${relativePath}\n# Outcome: ${reason.outcome} (latency=${reason.latencyMs.toFixed(2)}ms)\n# Generated: ${new Date().toISOString()}\n# per-component stderr sidecar\n\n`
      : `# Component: ${relativePath}\n# Outcome: ${reason.outcome} (FAILED)\n# Error: ${reason.errorMessage}\n# Generated: ${new Date().toISOString()}\n# per-component stderr sidecar\n\n`;
  writeFileSync(sidecarPath, header + stderrText, "utf8");
  return sidecarPath;
}

async function runScenario(
  prepared: PreparedProject,
  args: MetaUiBenchArgs,
  backend: MetaUiBackend,
  scenario: MetaUiScenario,
  repeats: number,
  warmupPasses: number,
  expectedArtifacts: Map<string, string>,
): Promise<MetaUiBenchmarkRun> {
  const repeatResults: MetaUiBenchmarkRun["repeats"] = [];

  for (let index = 0; index < repeats; index++) {
    const rotated = rotateComponentOrder(prepared.componentSnapshots, index);
    if (scenario === "single_cold" || scenario === "single_warm") {
      repeatResults.push(
        await runSingleScenarioRepeat(
          prepared,
          args,
          backend,
          scenario,
          index + 1,
          rotated,
          warmupPasses,
          expectedArtifacts,
        ),
      );
    } else {
      repeatResults.push(
        await runRepoScenarioRepeat(
          prepared,
          args,
          backend,
          scenario,
          index + 1,
          rotated,
          warmupPasses,
          expectedArtifacts,
        ),
      );
    }
  }

  const runBase = {
    kind: "meta-ui-benchmark-run" as const,
    generatedAt: new Date().toISOString(),
    version: {
      benchmarkPackageVersion: readPackageVersion(),
      verterCommitSha: readGitSha(repoRoot),
      resolvedTargetSha: prepared.resolvedTargetSha,
      vueComponentMetaVersion: readDependencyVersion("vue-component-meta"),
      nodeVersion: process.version,
    },
    target: {
      project: "nuxt-ui",
      repo: "nuxt/ui",
      branch: "v4",
      root: prepared.uiRoot,
      componentsDir: prepared.componentsDir,
      componentCount: prepared.componentSnapshots.length,
    },
    config: {
      backend,
      scenario,
      repeats,
      warmupPasses,
      runtimeMode: "dedicated" as const,
    },
    repeats: repeatResults,
  };

  return aggregateRunFromRepeats(runBase);
}

async function runSingleScenarioRepeat(
  prepared: PreparedProject,
  args: MetaUiBenchArgs,
  backend: MetaUiBackend,
  scenario: MetaUiScenario,
  repeatIndex: number,
  components: PreparedComponentSnapshot[],
  warmupPasses: number,
  expectedArtifacts: Map<string, string>,
): Promise<MetaUiBenchmarkRun["repeats"][number]> {
  let setupMs = 0;
  let warmupMs = 0;
  let steadyStateMs = 0;
  const componentResults: ComponentResultRow[] = [];
  const outcomeCounts = { success: 0, degraded: 0, query_error: 0, crash: 0 };
  const deviationTotals = {
    exactMatches: 0,
    totalMissing: 0,
    totalExtra: 0,
    totalFieldMismatches: 0,
  };
  const total = components.length;

  for (const [componentIndex, component] of components.entries()) {
    const setupStartedAt = performance.now();
    const instance = await createBackendInstance(prepared, args, backend, [component]);
    setupMs += performance.now() - setupStartedAt;

    try {
      if (scenario === "single_warm") {
        const warmupStartedAt = performance.now();
        for (let pass = 0; pass < warmupPasses; pass++) {
          await executeMeasuredQuery(instance, component);
        }
        warmupMs += performance.now() - warmupStartedAt;
      }

      const result = await executeMeasuredQuery(instance, component);
      steadyStateMs += result.latencyMs;
      outcomeCounts[result.outcome]++;
      componentResults.push({
        relativePath: component.relativePath,
        componentName: componentNameFromPath(component.relativePath),
        latencyMs: result.latencyMs,
        outcome: result.outcome,
        error: null,
      });
      updateDeviationTotals(
        deviationTotals,
        expectedArtifacts.get(component.relativePath),
        result.artifact,
      );
      logProgress(
        `[repeat ${repeatIndex}]`,
        component,
        componentIndex + 1,
        total,
        `${scenario} outcome=${result.outcome} latency=${result.latencyMs.toFixed(2)}ms`,
        args.jsAudit,
      );
    } catch (error) {
      const outcome = classifyFailure(error);
      outcomeCounts[outcome]++;
      componentResults.push({
        relativePath: component.relativePath,
        componentName: componentNameFromPath(component.relativePath),
        latencyMs: null,
        outcome,
        error: error instanceof Error ? error.message : String(error),
      });
      // write per-component stderr sidecar on
      // failure so the failure context (loader output, plugin
      // resolver noise, native panic line) survives the worker
      // process crash for offline diagnosis.
      writeComponentStderrSidecar(
        args.outputDir,
        component.relativePath,
        instance.takeLastQueryStderr?.() ?? null,
        {
          kind: "failure",
          outcome,
          errorMessage: error instanceof Error ? error.message : String(error),
        },
      );
      logProgress(
        `[repeat ${repeatIndex}]`,
        component,
        componentIndex + 1,
        total,
        `${scenario} outcome=${outcome} error=${error instanceof Error ? error.message : String(error)}`,
        args.jsAudit,
      );
    } finally {
      await instance.dispose();
      maybeRunGarbageCollection();
    }
  }

  const latencies = componentResults.filter((r) => r.latencyMs !== null).map((r) => r.latencyMs!);
  return {
    index: repeatIndex,
    orderStart: repeatIndex - 1,
    setupMs,
    warmupMs,
    steadyStateMs,
    endToEndMs: setupMs + warmupMs + steadyStateMs,
    componentResults,
    outcomeCounts,
    slaCount: buildSlaCount(componentResults, args.slaMs),
    deviationTotals,
    stats: summarizeLatencySeries(latencies.length > 0 ? latencies : [steadyStateMs]),
  };
}

async function runRepoScenarioRepeat(
  prepared: PreparedProject,
  args: MetaUiBenchArgs,
  backend: MetaUiBackend,
  scenario: MetaUiScenario,
  repeatIndex: number,
  components: PreparedComponentSnapshot[],
  warmupPasses: number,
  expectedArtifacts: Map<string, string>,
): Promise<MetaUiBenchmarkRun["repeats"][number]> {
  let setupMs = 0;
  let warmupMs = 0;
  const componentResults: ComponentResultRow[] = [];
  const outcomeCounts = { success: 0, degraded: 0, query_error: 0, crash: 0 };
  const deviationTotals = {
    exactMatches: 0,
    totalMissing: 0,
    totalExtra: 0,
    totalFieldMismatches: 0,
  };
  const total = components.length;
  let instance: BackendInstance | null = null;
  let recoveryMode = false;

  const startInstance = async () => {
    const setupStartedAt = performance.now();
    instance = await createBackendInstance(prepared, args, backend, components);
    setupMs += performance.now() - setupStartedAt;
  };

  await startInstance();

  try {
    if (scenario === "repo_warm_second_pass") {
      const warmupStartedAt = performance.now();
      for (let pass = 0; pass < warmupPasses; pass++) {
        for (const component of components) {
          if (recoveryMode) {
            break;
          }
          if (!instance || !instance.isAvailable()) {
            recoveryMode = true;
            break;
          }
          try {
            await executeMeasuredQuery(instance, component);
          } catch {
            if (instance && !instance.isAvailable()) {
              await instance.dispose();
              instance = null;
              recoveryMode = true;
              break;
            }
          }
        }
        if (recoveryMode) {
          break;
        }
      }
      warmupMs = performance.now() - warmupStartedAt;
    }

    const steadyStartedAt = performance.now();
    for (const [componentIndex, component] of components.entries()) {
      if (recoveryMode) {
        const singleSetupStartedAt = performance.now();
        const singleInstance = await createBackendInstance(prepared, args, backend, [component]);
        setupMs += performance.now() - singleSetupStartedAt;
        try {
          const result = await executeMeasuredQuery(singleInstance, component);
          outcomeCounts[result.outcome]++;
          componentResults.push({
            relativePath: component.relativePath,
            componentName: componentNameFromPath(component.relativePath),
            latencyMs: result.latencyMs,
            outcome: result.outcome,
            error: null,
          });
          updateDeviationTotals(
            deviationTotals,
            expectedArtifacts.get(component.relativePath),
            result.artifact,
          );
          logProgress(
            `[repeat ${repeatIndex}]`,
            component,
            componentIndex + 1,
            total,
            `${scenario} outcome=${result.outcome} latency=${result.latencyMs.toFixed(2)}ms`,
            args.jsAudit,
          );
        } catch (error) {
          const outcome = classifyFailure(error);
          outcomeCounts[outcome]++;
          componentResults.push({
            relativePath: component.relativePath,
            componentName: componentNameFromPath(component.relativePath),
            latencyMs: null,
            outcome,
            error: error instanceof Error ? error.message : String(error),
          });
          // sidecar write for the
          // recovery-mode (post-crash) per-component path.
          writeComponentStderrSidecar(
            args.outputDir,
            component.relativePath,
            singleInstance.takeLastQueryStderr?.() ?? null,
            {
              kind: "failure",
              outcome,
              errorMessage: error instanceof Error ? error.message : String(error),
            },
          );
          logProgress(
            `[repeat ${repeatIndex}]`,
            component,
            componentIndex + 1,
            total,
            `${scenario} outcome=${outcome} error=${error instanceof Error ? error.message : String(error)}`,
            args.jsAudit,
          );
        } finally {
          await singleInstance.dispose();
        }
        continue;
      }
      try {
        const result = await executeMeasuredQuery(instance, component);
        outcomeCounts[result.outcome]++;
        componentResults.push({
          relativePath: component.relativePath,
          componentName: componentNameFromPath(component.relativePath),
          latencyMs: result.latencyMs,
          outcome: result.outcome,
          error: null,
        });
        updateDeviationTotals(
          deviationTotals,
          expectedArtifacts.get(component.relativePath),
          result.artifact,
        );
        logProgress(
          `[repeat ${repeatIndex}]`,
          component,
          componentIndex + 1,
          total,
          `${scenario} outcome=${result.outcome} latency=${result.latencyMs.toFixed(2)}ms`,
          args.jsAudit,
        );
      } catch (error) {
        const outcome = classifyFailure(error);
        outcomeCounts[outcome]++;
        componentResults.push({
          relativePath: component.relativePath,
          componentName: componentNameFromPath(component.relativePath),
          latencyMs: null,
          outcome,
          error: error instanceof Error ? error.message : String(error),
        });
        // sidecar write for the long-lived
        // shared-instance steady-state path. The instance may
        // already be unavailable at this point; takeLastQueryStderr
        // still returns the captured window because stderr was
        // captured at message-receipt time before dispose runs.
        writeComponentStderrSidecar(
          args.outputDir,
          component.relativePath,
          instance?.takeLastQueryStderr?.() ?? null,
          {
            kind: "failure",
            outcome,
            errorMessage: error instanceof Error ? error.message : String(error),
          },
        );
        logProgress(
          `[repeat ${repeatIndex}]`,
          component,
          componentIndex + 1,
          total,
          `${scenario} outcome=${outcome} error=${error instanceof Error ? error.message : String(error)}`,
          args.jsAudit,
        );
        if (instance && !instance.isAvailable()) {
          await instance.dispose();
          instance = null;
          recoveryMode = true;
        }
      }
    }
    const steadyStateMs = performance.now() - steadyStartedAt;

    return {
      index: repeatIndex,
      orderStart: repeatIndex - 1,
      setupMs,
      warmupMs,
      steadyStateMs,
      endToEndMs: setupMs + warmupMs + steadyStateMs,
      componentResults,
      outcomeCounts,
      slaCount: buildSlaCount(componentResults, args.slaMs),
      deviationTotals,
      stats: summarizeLatencySeries(
        (() => {
          const latencies = componentResults
            .filter((r) => r.latencyMs !== null)
            .map((r) => r.latencyMs!);
          return latencies.length > 0 ? latencies : [steadyStateMs];
        })(),
      ),
    };
  } finally {
    await instance?.dispose();
    maybeRunGarbageCollection();
  }
}

function updateDeviationTotals(
  totals: {
    exactMatches: number;
    totalMissing: number;
    totalExtra: number;
    totalFieldMismatches: number;
  },
  expectedArtifactPath: string | undefined,
  actualArtifact: NormalizedMetaArtifact,
): ArtifactComparison | null {
  if (!expectedArtifactPath) {
    return null;
  }

  const expectedArtifact = JSON.parse(
    readFileSync(expectedArtifactPath, "utf8"),
  ) as NormalizedMetaArtifact;
  const comparison = compareNormalizedArtifacts(actualArtifact, expectedArtifact);
  if (comparison.exact) {
    totals.exactMatches++;
  }
  totals.totalMissing += comparison.totalMissing;
  totals.totalExtra += comparison.totalExtra;
  totals.totalFieldMismatches += comparison.totalFieldMismatches;
  return comparison;
}

function readPackageVersion(): string {
  const packageJsonPath = resolve(__dirname, "..", "package.json");
  return JSON.parse(readFileSync(packageJsonPath, "utf8")).version;
}

function readDependencyVersion(specifier: string): string {
  const packageJsonPath = require.resolve(`${specifier}/package.json`);
  return JSON.parse(readFileSync(packageJsonPath, "utf8")).version;
}

function readGitSha(cwd: string): string {
  try {
    return execFileSync("git", ["rev-parse", "HEAD"], {
      cwd,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
  } catch {
    return "unknown";
  }
}

function writeRunArtifact(outputDir: string, run: MetaUiBenchmarkRun): string {
  mkdirSync(outputDir, { recursive: true });
  const filePath = join(outputDir, `meta-ui-${run.config.backend}-${run.config.scenario}.json`);
  writeFileSync(filePath, JSON.stringify(run, null, 2));
  return filePath;
}

async function main() {
  const args = parseMetaUiBenchArgs(process.argv.slice(2));
  const prepared = prepareMetaUiProject(args);
  logLine(
    `Benchmarking ${prepared.componentSnapshots.length} nuxt-ui components from ${prepared.resolvedTargetSha}.`,
  );

  const needsExpectedArtifacts =
    args.expected !== "none" && args.backends.some((backend) => backend !== "vue-component-meta");
  let expectedArtifacts = new Map<string, string>();
  if (args.buildExpectedOnly) {
    if (args.expected === "none") {
      throw new Error("--build-expected-only requires --expected=vue-component-meta.");
    }
    expectedArtifacts =
      tryLoadExpectedArtifacts(prepared, args.expectedDir) ??
      (await buildExpectedArtifacts(prepared, args, args.expected, args.expectedDir));
    logLine(`Prepared ${expectedArtifacts.size} expected artifacts in ${args.expectedDir}.`);
    return;
  }

  if (needsExpectedArtifacts) {
    expectedArtifacts =
      tryLoadExpectedArtifacts(prepared, args.expectedDir) ??
      (await buildExpectedArtifacts(prepared, args, args.expected, args.expectedDir));
  }
  const runs: MetaUiBenchmarkRun[] = [];

  for (const backend of args.backends) {
    for (const scenario of args.scenarios) {
      logLine(`Running ${backend} / ${scenario}...`);
      const run = await runScenario(
        prepared,
        args,
        backend,
        scenario,
        args.repeats,
        args.warmupPasses,
        expectedArtifacts,
      );
      runs.push(run);
      writeRunArtifact(args.outputDir, run);
      logLine(
        `  steady p50=${run.summary.steadyStateMs.p50.toFixed(2)}ms end-to-end p50=${run.summary.endToEndMs.p50.toFixed(2)}ms`,
      );
    }
  }

  if (JSON_MODE) {
    const output = runs.length === 1 ? runs[0] : { kind: "meta-ui-benchmark-runs", runs };
    process.stdout.write(`${JSON.stringify(output, null, 2)}\n`);
    return;
  }

  log("\n");
}

if (process.argv[1] && normalizePath(resolve(process.argv[1])) === normalizePath(__filename)) {
  main().catch((error) => {
    console.error(error instanceof Error ? (error.stack ?? error.message) : error);
    process.exitCode = 1;
  });
}

function normalizePath(value: string): string {
  return value.replace(/\\/g, "/");
}

function componentNameFromPath(filePath: string): string {
  const base = filePath.replace(/\\/g, "/").split("/").pop() ?? filePath;
  return base.replace(/\.vue$/, "");
}
