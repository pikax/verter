import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, renameSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import {
  evaluateFixtureFence,
  median,
  type BackendMeasurement,
  type FixtureFenceResult,
  type NativeArtifactAttestation,
  type SourceProvenance,
  PINNED_OFFICIAL_SVELTE_VERSION,
  SVELTE_COMPILER_RSS_THRESHOLD,
  SVELTE_COMPILER_WALL_THRESHOLD,
  validateBenchmarkShape,
  validateMeasurement,
  validateRssIterations,
  validateStableBenchmarkProvenance,
  validateWarmupIterations,
} from "./svelte-perf-contract";
import {
  sourceForBenchmarkSequence,
  SVELTE_COMPILER_FIXTURES,
  type SvelteCompilerFixture,
} from "./svelte-perf-fixtures";

type Backend = "verter" | "official-svelte";
type WorkerMetric = "wall" | "peak-rss";

interface WallWorkerResult {
  backend: Backend;
  metric: "wall";
  backendVersion: string;
  wallSamplesMs: number[];
}

interface RssWorkerResult {
  backend: Backend;
  metric: "peak-rss";
  backendVersion: string;
  peakRssMB: number;
}

type WorkerResult = WallWorkerResult | RssWorkerResult;

interface FenceReport {
  schemaVersion: 3;
  generatedAt: string;
  wallThreshold: number;
  peakRssThreshold: number;
  node: string;
  platform: NodeJS.Platform;
  arch: string;
  sourceRevision: string;
  worktreeClean: boolean;
  nativeArtifactSha256: string;
  nativeArtifactName: string;
  officialSvelteVersion: string;
  officialSvelteExpectedVersion: string;
  analysisLevel: "none";
  sourceMapMode: "enabled-both-backends";
  cacheMode: "verter-stateless-attested-per-sample";
  memoryMetric: "isolated-process-peak-rss";
  provenanceMode: "parent-initial-final";
  warmupIterations: number;
  rounds: number;
  iterationsPerRound: number;
  rssWarmupIterations: number;
  rssIterationsPerSample: number;
  corpus: Array<{
    name: SvelteCompilerFixture["name"];
    slug: string;
    sourceBytes: number;
    coverage: string[];
  }>;
  results: FixtureFenceResult[];
  pass: boolean;
}

interface FailureReport {
  schemaVersion: 3;
  generatedAt: string;
  pass: false;
  error: string;
  node: string;
  platform: NodeJS.Platform;
  arch: string;
  officialSvelteExpectedVersion: string;
}

interface CompiledObservation {
  codeLength: number;
  mapWeight: number;
}

interface CompilerDriver {
  backendVersion: string;
  compileOnce: (validateMapContent: boolean) => CompiledObservation;
  close: () => void;
}

const RESULT_PREFIX = "VERTER_SVELTE_BENCH_RESULT=";
const WORKER_TIMEOUT_MS = 5 * 60 * 1_000;
const MEBIBYTE = 1024 * 1024;
const require = createRequire(import.meta.url);

function valueArg(name: string): string | undefined {
  const prefix = `--${name}=`;
  return process.argv.find((arg) => arg.startsWith(prefix))?.slice(prefix.length);
}

function integerArg(name: string, fallback: number): number {
  const raw = valueArg(name);
  if (raw === undefined) return fallback;
  const value = Number(raw);
  if (!Number.isSafeInteger(value)) throw new Error(`--${name} must be an integer, got ${raw}`);
  return value;
}

function backendArg(): Backend | undefined {
  const value = valueArg("worker");
  if (value === undefined) return undefined;
  if (value !== "verter" && value !== "official-svelte") {
    throw new Error(`unknown Svelte performance worker backend: ${value}`);
  }
  return value;
}

function workerMetricArg(): WorkerMetric {
  const value = valueArg("metric");
  if (value === "wall" || value === "peak-rss") return value;
  throw new Error(`worker requires --metric=wall|peak-rss, got ${value ?? "none"}`);
}

function fixtureByName(name: string): SvelteCompilerFixture {
  const fixture = SVELTE_COMPILER_FIXTURES.find((candidate) => candidate.name === name);
  if (!fixture) throw new Error(`unknown Svelte performance fixture: ${name}`);
  return fixture;
}

function gitOutput(args: string[]): string {
  const result = spawnSync("git", args, {
    cwd: process.cwd(),
    encoding: "utf8",
    windowsHide: true,
  });
  if (result.error || result.status !== 0) {
    throw new Error(`git ${args.join(" ")} failed: ${result.error?.message ?? result.stderr}`);
  }
  return result.stdout.trim();
}

function sourceProvenance(requireImmutable: boolean): SourceProvenance {
  const head = gitOutput(["rev-parse", "HEAD"]);
  const githubSha = process.env.GITHUB_SHA?.trim();
  if (githubSha && githubSha !== head) {
    throw new Error(`GITHUB_SHA ${githubSha} does not match checked-out HEAD ${head}`);
  }
  const sourceRevision = githubSha ?? head;
  const worktreeClean = gitOutput(["status", "--porcelain", "--untracked-files=all"]) === "";
  if (!/^[0-9a-f]{40}$/u.test(sourceRevision)) {
    throw new Error(`Svelte benchmark source revision is not a full Git SHA: ${sourceRevision}`);
  }
  if (requireImmutable && !worktreeClean) {
    throw new Error("immutable Svelte benchmark requires a clean tracked worktree");
  }
  return { sourceRevision, worktreeClean };
}

function nativeArtifactAttestation(): NativeArtifactAttestation {
  const nativeEntry = require.resolve("@verter/native");
  const distDirectory = join(dirname(nativeEntry), "dist");
  const binaries = readdirSync(distDirectory).filter((name) => name.endsWith(".node"));
  if (binaries.length !== 1) {
    throw new Error(
      `expected exactly one built @verter/native artifact in ${distDirectory}, found ${binaries.length}`,
    );
  }
  const name = binaries[0]!;
  const sha256 = createHash("sha256")
    .update(readFileSync(join(distDirectory, name)))
    .digest("hex");
  if (!/^[0-9a-f]{64}$/u.test(sha256)) {
    throw new Error("failed to attest the native compiler artifact with SHA-256");
  }
  return { name, sha256 };
}

function validateSourceMapJson(mapJson: string, source: string, label: string): number {
  let parsed: unknown;
  try {
    parsed = JSON.parse(mapJson);
  } catch (error: unknown) {
    throw new Error(`${label} emitted invalid source-map JSON: ${String(error)}`);
  }
  if (!parsed || typeof parsed !== "object")
    throw new Error(`${label} emitted no source-map object`);
  const map = parsed as { mappings?: unknown; sourcesContent?: unknown };
  if (typeof map.mappings !== "string" || map.mappings.length === 0) {
    throw new Error(`${label} emitted a source map without mappings`);
  }
  if (!Array.isArray(map.sourcesContent) || !map.sourcesContent.includes(source)) {
    throw new Error(`${label} source map does not embed the exact compiled source`);
  }
  return map.mappings.length;
}

async function createCompiler(
  fixture: SvelteCompilerFixture,
  backend: Backend,
): Promise<CompilerDriver> {
  let sequence = 0;
  if (backend === "official-svelte") {
    const { compile, VERSION } = await import("svelte/compiler");
    if (VERSION !== PINNED_OFFICIAL_SVELTE_VERSION) {
      throw new Error(
        `official Svelte compiler version drift: expected ${PINNED_OFFICIAL_SVELTE_VERSION}, installed ${VERSION}`,
      );
    }
    return {
      backendVersion: VERSION,
      compileOnce: (validateMapContent) => {
        const source = sourceForBenchmarkSequence(fixture, sequence++);
        const output = compile(source, {
          filename: fixture.filename,
          generate: "client",
          dev: false,
          css: "external",
        });
        const map = output.js.map;
        if (output.warnings.length > 0 || output.js.code.length === 0 || !map?.mappings) {
          throw new Error(
            `${fixture.name}/official-svelte did not produce clean mapped client output: ${JSON.stringify(output.warnings)}`,
          );
        }
        const mapWeight = validateMapContent
          ? validateSourceMapJson(JSON.stringify(map), source, `${fixture.name}/official-svelte`)
          : map.mappings.length;
        return { codeLength: output.js.code.length, mapWeight };
      },
      close: () => undefined,
    };
  }

  const { VerterHost } = await import("@verter/native");
  // Artifact hashing is performed by the unmeasured parent before and after the
  // complete run. Reading the binary here would inflate this worker's lifetime
  // peak RSS with the hash buffer and contaminate the memory comparison.
  const host = new VerterHost({ devMode: false, analysisLevel: "none", hostCpuThreads: 1 });
  return {
    backendVersion: "native-artifact",
    compileOnce: (validateMapContent) => {
      const source = sourceForBenchmarkSequence(fixture, sequence++);
      const upsert = host.upsert({
        inputId: fixture.filename,
        source,
        fileKind: "svelte",
      });
      if (!upsert.changed || !upsert.changedVirtualNodes.some((node) => node.kind === "main")) {
        throw new Error(`${fixture.name}/verter benchmark revision did not invalidate Main`);
      }
      const output = host.getVirtualFile({
        canonicalId: upsert.canonicalId,
        nodeKind: { kind: "main" },
        compileProfile: {
          target: "bundler",
          sourceMap: true,
          requestedMode: "stateless",
        },
      });
      if (
        !output ||
        output.code.length === 0 ||
        !output.sourceMap ||
        output.diagnostics.diagnostics.length > 0
      ) {
        throw new Error(
          `${fixture.name}/verter did not produce clean mapped client output: ${JSON.stringify(output?.diagnostics ?? null)}`,
        );
      }
      if (
        output.cacheHit ||
        output.requestedMode !== "stateless" ||
        output.actualMode !== "stateless" ||
        output.downgradeReason !== undefined
      ) {
        throw new Error(
          `${fixture.name}/verter cache attestation failed: ${JSON.stringify({
            cacheHit: output.cacheHit,
            requestedMode: output.requestedMode,
            actualMode: output.actualMode,
            downgradeReason: output.downgradeReason,
          })}`,
        );
      }
      const mapWeight = validateMapContent
        ? validateSourceMapJson(output.sourceMap, source, `${fixture.name}/verter`)
        : output.sourceMap.length;
      return { codeLength: output.code.length, mapWeight };
    },
    close: () => host.close(),
  };
}

function observe(observation: CompiledObservation, checksum: number): number {
  return checksum + observation.codeLength + observation.mapWeight;
}

function peakRssMB(): number {
  // Node documents resourceUsage().maxRSS in KiB on every supported platform.
  return process.resourceUsage().maxRSS / 1024;
}

async function runWorker(backend: Backend): Promise<never> {
  if (typeof global.gc !== "function") {
    throw new Error("Svelte performance workers require node --expose-gc");
  }
  const fixtureName = valueArg("fixture");
  if (!fixtureName) throw new Error("worker requires --fixture=<name>");
  const metric = workerMetricArg();
  const iterations = integerArg("iterations", 500);
  const rounds = integerArg("rounds", 5);
  const warmupIterations = integerArg("warmup", 50);
  const rssIterations = integerArg("rss-iterations", 100);
  const rssWarmupIterations = integerArg("rss-warmup", 20);
  validateBenchmarkShape(iterations, rounds);
  validateWarmupIterations(warmupIterations);
  validateRssIterations(rssIterations);
  validateWarmupIterations(rssWarmupIterations);

  const compiler = await createCompiler(fixtureByName(fixtureName), backend);
  let checksum = 0;
  try {
    checksum = observe(compiler.compileOnce(true), checksum);
    if (metric === "wall") {
      for (let index = 0; index < warmupIterations; index++) {
        checksum = observe(compiler.compileOnce(false), checksum);
      }
      const wallSamplesMs: number[] = [];
      for (let round = 0; round < rounds; round++) {
        global.gc();
        const started = performance.now();
        for (let index = 0; index < iterations; index++) {
          checksum = observe(compiler.compileOnce(false), checksum);
        }
        wallSamplesMs.push((performance.now() - started) / iterations);
      }
      if (checksum <= 0) throw new Error("Svelte wall worker produced no observable output");
      const result: WallWorkerResult = {
        backend,
        metric,
        backendVersion: compiler.backendVersion,
        wallSamplesMs,
      };
      process.stdout.write(`${RESULT_PREFIX}${JSON.stringify(result)}\n`);
    } else {
      for (let index = 0; index < rssWarmupIterations; index++) {
        checksum = observe(compiler.compileOnce(false), checksum);
      }
      global.gc();
      for (let index = 0; index < rssIterations; index++) {
        checksum = observe(compiler.compileOnce(false), checksum);
      }
      if (checksum <= 0) throw new Error("Svelte RSS worker produced no observable output");
      const result: RssWorkerResult = {
        backend,
        metric,
        backendVersion: compiler.backendVersion,
        peakRssMB: Math.max(peakRssMB(), process.memoryUsage().rss / MEBIBYTE),
      };
      process.stdout.write(`${RESULT_PREFIX}${JSON.stringify(result)}\n`);
    }
  } finally {
    compiler.close();
  }
  process.exit(0);
}

interface WorkerOptions {
  fixture: SvelteCompilerFixture;
  backend: Backend;
  metric: WorkerMetric;
  iterations: number;
  rounds: number;
  warmupIterations: number;
  rssIterations: number;
  rssWarmupIterations: number;
}

function runWorkerProcess(options: WorkerOptions): WorkerResult {
  const script = fileURLToPath(import.meta.url);
  const args = [
    "--expose-gc",
    "--import",
    "tsx",
    script,
    `--worker=${options.backend}`,
    `--metric=${options.metric}`,
    `--fixture=${options.fixture.name}`,
    `--iterations=${options.iterations}`,
    `--rounds=${options.rounds}`,
    `--warmup=${options.warmupIterations}`,
    `--rss-iterations=${options.rssIterations}`,
    `--rss-warmup=${options.rssWarmupIterations}`,
  ];
  const child = spawnSync(process.execPath, args, {
    cwd: process.cwd(),
    encoding: "utf8",
    maxBuffer: 4 * 1024 * 1024,
    timeout: WORKER_TIMEOUT_MS,
    windowsHide: true,
  });
  if (child.error || child.status !== 0) {
    throw new Error(
      `${options.fixture.name}/${options.backend}/${options.metric} worker failed (${child.status ?? child.signal ?? "spawn"}): ${child.error?.message ?? ""}\n${child.stderr}\n${child.stdout}`,
    );
  }
  const line = child.stdout
    .split(/\r?\n/u)
    .find((candidate) => candidate.startsWith(RESULT_PREFIX));
  if (!line) {
    throw new Error(
      `${options.fixture.name}/${options.backend}/${options.metric} worker emitted no result`,
    );
  }
  const result = JSON.parse(line.slice(RESULT_PREFIX.length)) as WorkerResult;
  if (result.backend !== options.backend || result.metric !== options.metric) {
    throw new Error(
      `${options.fixture.name}/${options.backend}/${options.metric} worker mislabeled its result`,
    );
  }
  return result;
}

function formatRatio(value: number): string {
  return Number.isFinite(value) ? `${value.toFixed(3)}x` : "infinite";
}

function writeReport(report: FenceReport | FailureReport): void {
  const outputPath = valueArg("out");
  if (!outputPath) return;
  const absolutePath = resolve(outputPath);
  mkdirSync(dirname(absolutePath), { recursive: true });
  const temporaryPath = `${absolutePath}.tmp-${process.pid}`;
  writeFileSync(temporaryPath, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  renameSync(temporaryPath, absolutePath);
}

function runParent(): void {
  const iterations = integerArg("iterations", 500);
  const rounds = integerArg("rounds", 5);
  const warmupIterations = integerArg("warmup", 50);
  const rssIterations = integerArg("rss-iterations", 100);
  const rssWarmupIterations = integerArg("rss-warmup", 20);
  const requireImmutable = process.argv.includes("--require-immutable");
  validateBenchmarkShape(iterations, rounds);
  validateWarmupIterations(warmupIterations);
  validateRssIterations(rssIterations);
  validateWarmupIterations(rssWarmupIterations);
  const initialProvenance = sourceProvenance(requireImmutable);
  const nativeArtifact = nativeArtifactAttestation();

  let officialSvelteVersion: string | undefined;
  const results = SVELTE_COMPILER_FIXTURES.map((fixture, fixtureIndex) => {
    const backendOrder: readonly Backend[] =
      fixtureIndex % 2 === 0 ? ["official-svelte", "verter"] : ["verter", "official-svelte"];
    const wallResults = new Map<Backend, WallWorkerResult>();
    for (const backend of backendOrder) {
      const result = runWorkerProcess({
        fixture,
        backend,
        metric: "wall",
        iterations,
        rounds,
        warmupIterations,
        rssIterations,
        rssWarmupIterations,
      });
      if (result.metric !== "wall") throw new Error("wall worker returned an RSS result");
      wallResults.set(backend, result);
    }

    const peakRssSamples = new Map<Backend, number[]>([
      ["verter", []],
      ["official-svelte", []],
    ]);
    for (let round = 0; round < rounds; round++) {
      const rssOrder: readonly Backend[] =
        (fixtureIndex + round) % 2 === 0
          ? ["official-svelte", "verter"]
          : ["verter", "official-svelte"];
      for (const backend of rssOrder) {
        const result = runWorkerProcess({
          fixture,
          backend,
          metric: "peak-rss",
          iterations,
          rounds,
          warmupIterations,
          rssIterations,
          rssWarmupIterations,
        });
        if (result.metric !== "peak-rss") throw new Error("RSS worker returned a wall result");
        peakRssSamples.get(backend)!.push(result.peakRssMB);
      }
    }

    const officialWall = wallResults.get("official-svelte")!;
    officialSvelteVersion ??= officialWall.backendVersion;
    if (officialWall.backendVersion !== officialSvelteVersion) {
      throw new Error("official Svelte worker versions were inconsistent across fixtures");
    }

    const toMeasurement = (backend: Backend): BackendMeasurement => {
      const wall = wallResults.get(backend)!;
      const rss = peakRssSamples.get(backend)!;
      const medianCompileMs = median(wall.wallSamplesMs);
      const measurement: BackendMeasurement = {
        medianCompileMs,
        opsPerSec: 1_000 / medianCompileMs,
        medianPeakRssMB: median(rss),
        wallSamplesMs: wall.wallSamplesMs,
        peakRssSamplesMB: rss,
        rounds,
        iterationsPerRound: iterations,
        rssIterationsPerSample: rssIterations,
      };
      validateMeasurement(measurement);
      return measurement;
    };

    return evaluateFixtureFence(
      fixture.name,
      toMeasurement("verter"),
      toMeasurement("official-svelte"),
    );
  });

  if (officialSvelteVersion !== PINNED_OFFICIAL_SVELTE_VERSION) {
    throw new Error(
      `official Svelte version mismatch: expected ${PINNED_OFFICIAL_SVELTE_VERSION}, measured ${officialSvelteVersion ?? "none"}`,
    );
  }
  const finalProvenance = sourceProvenance(requireImmutable);
  const finalNativeArtifact = nativeArtifactAttestation();
  validateStableBenchmarkProvenance(
    initialProvenance,
    finalProvenance,
    nativeArtifact,
    finalNativeArtifact,
  );

  const report: FenceReport = {
    schemaVersion: 3,
    generatedAt: new Date().toISOString(),
    wallThreshold: SVELTE_COMPILER_WALL_THRESHOLD,
    peakRssThreshold: SVELTE_COMPILER_RSS_THRESHOLD,
    node: process.version,
    platform: process.platform,
    arch: process.arch,
    sourceRevision: initialProvenance.sourceRevision,
    worktreeClean: initialProvenance.worktreeClean,
    nativeArtifactSha256: nativeArtifact.sha256,
    nativeArtifactName: nativeArtifact.name,
    officialSvelteVersion,
    officialSvelteExpectedVersion: PINNED_OFFICIAL_SVELTE_VERSION,
    analysisLevel: "none",
    sourceMapMode: "enabled-both-backends",
    cacheMode: "verter-stateless-attested-per-sample",
    memoryMetric: "isolated-process-peak-rss",
    provenanceMode: "parent-initial-final",
    warmupIterations,
    rounds,
    iterationsPerRound: iterations,
    rssWarmupIterations,
    rssIterationsPerSample: rssIterations,
    corpus: SVELTE_COMPILER_FIXTURES.map(({ name, slug, sourceBytes, coverage }) => ({
      name,
      slug,
      sourceBytes,
      coverage: [...coverage],
    })),
    results,
    pass: results.every((result) => result.pass),
  };
  writeReport(report);

  if (process.argv.includes("--json")) {
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  } else {
    process.stdout.write(
      `Verter/official-Svelte ${officialSvelteVersion} equal-work compiler fence ` +
        `(wall <= ${SVELTE_COMPILER_WALL_THRESHOLD.toFixed(2)}x, peak RSS <= ${SVELTE_COMPILER_RSS_THRESHOLD.toFixed(2)}x, ` +
        `${rounds}x${iterations}, maps enabled, stateless attested)\n`,
    );
    for (const result of results) {
      process.stdout.write(
        `${result.fixture}: wall ${result.verter.medianCompileMs.toFixed(3)}ms / ` +
          `${result.officialSvelte.medianCompileMs.toFixed(3)}ms = ${formatRatio(result.wallRatio)}; ` +
          `peak RSS ${result.verter.medianPeakRssMB.toFixed(1)}MiB / ` +
          `${result.officialSvelte.medianPeakRssMB.toFixed(1)}MiB = ${formatRatio(result.peakRssRatio)}; ` +
          `${result.pass ? "PASS" : "FAIL"}\n`,
      );
    }
  }
  if (!report.pass) process.exitCode = 1;
}

async function main(): Promise<void> {
  const worker = backendArg();
  if (worker) await runWorker(worker);
  runParent();
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? (error.stack ?? error.message) : String(error);
  const report: FailureReport = {
    schemaVersion: 3,
    generatedAt: new Date().toISOString(),
    pass: false,
    error: message,
    node: process.version,
    platform: process.platform,
    arch: process.arch,
    officialSvelteExpectedVersion: PINNED_OFFICIAL_SVELTE_VERSION,
  };
  try {
    writeReport(report);
  } catch (writeError: unknown) {
    process.stderr.write(
      `failed to write Svelte benchmark failure artifact: ${String(writeError)}\n`,
    );
  }
  process.stderr.write(`${message}\n`);
  process.exitCode = 1;
});
