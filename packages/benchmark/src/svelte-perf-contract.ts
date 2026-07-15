import type { SvelteCompilerFixture } from "./svelte-perf-fixtures";

export const PINNED_OFFICIAL_SVELTE_VERSION = "5.56.3";
export const SVELTE_COMPILER_WALL_THRESHOLD = 1.1;
export const SVELTE_COMPILER_RSS_THRESHOLD = 1.1;
export const MIN_SVELTE_PERF_ITERATIONS = 50;
export const MIN_SVELTE_RSS_ITERATIONS = 10;

export interface SourceProvenance {
  sourceRevision: string;
  worktreeClean: boolean;
}

export interface NativeArtifactAttestation {
  name: string;
  sha256: string;
}

export function validateStableBenchmarkProvenance(
  initialSource: SourceProvenance,
  finalSource: SourceProvenance,
  initialNative: NativeArtifactAttestation,
  finalNative: NativeArtifactAttestation,
): void {
  if (
    initialSource.sourceRevision !== finalSource.sourceRevision ||
    initialSource.worktreeClean !== finalSource.worktreeClean
  ) {
    throw new Error("Verter source provenance changed while the benchmark was running");
  }
  if (initialNative.name !== finalNative.name || initialNative.sha256 !== finalNative.sha256) {
    throw new Error("Verter native artifact changed while the benchmark was running");
  }
}

export interface BackendMeasurement {
  medianCompileMs: number;
  opsPerSec: number;
  medianPeakRssMB: number;
  wallSamplesMs: number[];
  peakRssSamplesMB: number[];
  rounds: number;
  iterationsPerRound: number;
  rssIterationsPerSample: number;
}

export interface FixtureFenceResult {
  fixture: SvelteCompilerFixture["name"];
  verter: BackendMeasurement;
  officialSvelte: BackendMeasurement;
  wallRatio: number;
  peakRssRatio: number;
  pass: boolean;
}

export function validateBenchmarkShape(iterations: number, rounds: number): void {
  if (!Number.isSafeInteger(iterations) || iterations < MIN_SVELTE_PERF_ITERATIONS) {
    throw new Error(
      `Svelte performance iterations must be an integer >= ${MIN_SVELTE_PERF_ITERATIONS}, got ${iterations}`,
    );
  }
  if (!Number.isSafeInteger(rounds) || rounds < 3 || rounds % 2 === 0) {
    throw new Error(`Svelte performance rounds must be an odd integer >= 3, got ${rounds}`);
  }
}

export function validateWarmupIterations(iterations: number): void {
  if (!Number.isSafeInteger(iterations) || iterations < 0) {
    throw new Error(`Svelte performance warmup must be a non-negative integer, got ${iterations}`);
  }
}

export function validateRssIterations(iterations: number): void {
  if (!Number.isSafeInteger(iterations) || iterations < MIN_SVELTE_RSS_ITERATIONS) {
    throw new Error(
      `Svelte peak RSS iterations must be an integer >= ${MIN_SVELTE_RSS_ITERATIONS}, got ${iterations}`,
    );
  }
}

export function median(values: readonly number[]): number {
  if (values.length === 0) throw new Error("cannot calculate a median from zero samples");
  const sorted = [...values].sort((left, right) => left - right);
  return sorted[Math.floor(sorted.length / 2)]!;
}

function validatePositiveMetric(value: number, label: string): void {
  if (!Number.isFinite(value) || value <= 0) {
    throw new Error(`Svelte performance ${label} must be finite and > 0, got ${value}`);
  }
}

export function validateMeasurement(measurement: BackendMeasurement): void {
  validateBenchmarkShape(measurement.iterationsPerRound, measurement.rounds);
  validatePositiveMetric(measurement.medianCompileMs, "median compile time");
  validatePositiveMetric(measurement.opsPerSec, "operations per second");
  validatePositiveMetric(measurement.medianPeakRssMB, "median peak RSS");
  validateRssIterations(measurement.rssIterationsPerSample);
  if (measurement.wallSamplesMs.length !== measurement.rounds) {
    throw new Error(
      `Svelte performance wall sample count must equal rounds (${measurement.rounds}), got ${measurement.wallSamplesMs.length}`,
    );
  }
  if (measurement.peakRssSamplesMB.length !== measurement.rounds) {
    throw new Error(
      `Svelte performance peak RSS sample count must equal rounds (${measurement.rounds}), got ${measurement.peakRssSamplesMB.length}`,
    );
  }
  for (const sample of measurement.wallSamplesMs) {
    validatePositiveMetric(sample, "wall sample");
  }
  for (const sample of measurement.peakRssSamplesMB) {
    validatePositiveMetric(sample, "peak RSS sample");
  }
  if (measurement.medianCompileMs !== median(measurement.wallSamplesMs)) {
    throw new Error("Svelte performance median compile time does not match its raw samples");
  }
  if (measurement.medianPeakRssMB !== median(measurement.peakRssSamplesMB)) {
    throw new Error("Svelte performance median peak RSS does not match its raw samples");
  }
}

/** A zero baseline passes only when the measured side is also exactly zero. */
export function safeRatio(measured: number, baseline: number): number {
  if (baseline === 0) return measured === 0 ? 1 : Number.POSITIVE_INFINITY;
  return measured / baseline;
}

export function evaluateFixtureFence(
  fixture: SvelteCompilerFixture["name"],
  verter: BackendMeasurement,
  officialSvelte: BackendMeasurement,
  wallThreshold = SVELTE_COMPILER_WALL_THRESHOLD,
  rssThreshold = SVELTE_COMPILER_RSS_THRESHOLD,
): FixtureFenceResult {
  validateMeasurement(verter);
  validateMeasurement(officialSvelte);
  validatePositiveMetric(wallThreshold, "wall threshold");
  validatePositiveMetric(rssThreshold, "RSS threshold");
  const wallRatio = safeRatio(verter.medianCompileMs, officialSvelte.medianCompileMs);
  const peakRssRatio = safeRatio(verter.medianPeakRssMB, officialSvelte.medianPeakRssMB);
  return {
    fixture,
    verter,
    officialSvelte,
    wallRatio,
    peakRssRatio,
    pass: wallRatio <= wallThreshold && peakRssRatio <= rssThreshold,
  };
}
