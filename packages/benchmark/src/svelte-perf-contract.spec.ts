import { compile, VERSION as installedSvelteVersion } from "svelte/compiler";
import { describe, expect, it } from "vitest";
import {
  evaluateFixtureFence,
  MIN_SVELTE_PERF_ITERATIONS,
  MIN_SVELTE_RSS_ITERATIONS,
  PINNED_OFFICIAL_SVELTE_VERSION,
  safeRatio,
  validateBenchmarkShape,
  validateMeasurement,
  validateRssIterations,
  validateStableBenchmarkProvenance,
  validateWarmupIterations,
} from "./svelte-perf-contract";
import {
  sourceForBenchmarkSequence,
  SVELTE_BENCHMARK_REVISION_COUNT,
  SVELTE_COMPILER_FIXTURES,
} from "./svelte-perf-fixtures";

const measurement = (compileMs: number, peakRssMB: number) => ({
  medianCompileMs: compileMs,
  opsPerSec: 1_000 / compileMs,
  medianPeakRssMB: peakRssMB,
  wallSamplesMs: [compileMs, compileMs, compileMs, compileMs, compileMs],
  peakRssSamplesMB: [peakRssMB, peakRssMB, peakRssMB, peakRssMB, peakRssMB],
  rounds: 5,
  iterationsPerRound: 500,
  rssIterationsPerSample: 50,
});

describe("Verter/official-Svelte compiler performance fence", () => {
  it("pins a representative oracle-backed denominator with source maps on", () => {
    expect(installedSvelteVersion).toBe(PINNED_OFFICIAL_SVELTE_VERSION);
    expect(SVELTE_COMPILER_FIXTURES.map(({ name }) => name)).toEqual([
      "basic_runes",
      "keyed_each",
      "typescript_instance",
      "typescript_module",
      "scoped_css",
      "component_snippet",
      "await_block",
      "legacy_store",
      "special_window",
      "large_dashboard",
    ]);
    expect(new Set(SVELTE_COMPILER_FIXTURES.map(({ name }) => name)).size).toBe(10);
    expect(
      Math.max(...SVELTE_COMPILER_FIXTURES.map(({ sourceBytes }) => sourceBytes)),
    ).toBeGreaterThan(4_096);
    for (const fixture of SVELTE_COMPILER_FIXTURES) {
      expect(fixture.source.trim().length).toBeGreaterThan(0);
      expect(fixture.coverage.length).toBeGreaterThan(0);
      const output = compile(fixture.source, {
        filename: fixture.filename,
        generate: "client",
        dev: false,
        css: "external",
      });
      expect(output.warnings).toEqual([]);
      expect(output.js.code.length).toBeGreaterThan(0);
      expect(output.js.map).toBeTruthy();
      expect(output.js.map!.sourcesContent).toContain(fixture.source);
      expect(output.js.map!.mappings.length).toBeGreaterThan(0);
    }
  });

  it("cycles distinct same-behavior sources without deleting host state", () => {
    for (const fixture of SVELTE_COMPILER_FIXTURES) {
      const first = sourceForBenchmarkSequence(fixture, 0);
      const second = sourceForBenchmarkSequence(fixture, 1);
      expect(first).not.toBe(second);
      expect(sourceForBenchmarkSequence(fixture, SVELTE_BENCHMARK_REVISION_COUNT)).toBe(first);
      const compileRevision = (source: string) =>
        compile(source, {
          filename: fixture.filename,
          generate: "client",
          dev: false,
          css: "external",
        });
      const firstOutput = compileRevision(first);
      const secondOutput = compileRevision(second);
      expect(firstOutput.warnings).toEqual([]);
      expect(secondOutput.warnings).toEqual([]);
      expect(secondOutput.js.code).toBe(firstOutput.js.code);
      expect(firstOutput.js.map?.mappings.length).toBeGreaterThan(0);
      expect(secondOutput.js.map?.mappings.length).toBeGreaterThan(0);
    }
  });

  it("rejects vacuous sample counts", () => {
    expect(() => validateBenchmarkShape(MIN_SVELTE_PERF_ITERATIONS - 1, 5)).toThrow(/>= 50/);
    expect(() => validateBenchmarkShape(500, 0)).toThrow(/odd integer >= 3/);
    expect(() => validateBenchmarkShape(500, 4)).toThrow(/odd integer >= 3/);
    expect(() => validateBenchmarkShape(500, 5)).not.toThrow();
    expect(() => validateRssIterations(MIN_SVELTE_RSS_ITERATIONS - 1)).toThrow(/>= 10/);
    expect(() => validateRssIterations(50)).not.toThrow();
    expect(() => validateWarmupIterations(-1)).toThrow(/non-negative integer/);
    expect(() => validateWarmupIterations(10)).not.toThrow();
  });

  it("rejects degenerate or incomplete measurements", () => {
    expect(() => validateMeasurement(measurement(1, 1))).not.toThrow();
    expect(() =>
      validateMeasurement({ ...measurement(1, 1), medianCompileMs: Number.NaN }),
    ).toThrow(/median compile time/);
    expect(() =>
      validateMeasurement({ ...measurement(1, 100), peakRssSamplesMB: [100, 100] }),
    ).toThrow(/peak RSS sample count/);
    expect(() => validateMeasurement({ ...measurement(1, 100), medianPeakRssMB: 0 })).toThrow(
      /peak RSS/,
    );
  });

  it("requires both wall and total-process peak RSS ratios to stay within threshold", () => {
    expect(
      evaluateFixtureFence("basic_runes", measurement(1.1, 110), measurement(1, 100)).pass,
    ).toBe(true);
    expect(
      evaluateFixtureFence("basic_runes", measurement(1.11, 100), measurement(1, 100)).pass,
    ).toBe(false);
    expect(evaluateFixtureFence("basic_runes", measurement(1, 111), measurement(1, 100)).pass).toBe(
      false,
    );
  });

  it("does not hide a nonzero RSS measurement behind a zero baseline", () => {
    expect(safeRatio(0, 0)).toBe(1);
    expect(safeRatio(0.01, 0)).toBe(Number.POSITIVE_INFINITY);
  });

  it("rejects source or native drift across the parent-owned measurement window", () => {
    const source = { sourceRevision: "a".repeat(40), worktreeClean: true };
    const native = { name: "verter-native.node", sha256: "b".repeat(64) };
    expect(() => validateStableBenchmarkProvenance(source, source, native, native)).not.toThrow();
    expect(() =>
      validateStableBenchmarkProvenance(
        source,
        { ...source, sourceRevision: "c".repeat(40) },
        native,
        native,
      ),
    ).toThrow(/source provenance changed/);
    expect(() =>
      validateStableBenchmarkProvenance(
        source,
        { ...source, worktreeClean: false },
        native,
        native,
      ),
    ).toThrow(/source provenance changed/);
    expect(() =>
      validateStableBenchmarkProvenance(source, source, native, {
        ...native,
        sha256: "d".repeat(64),
      }),
    ).toThrow(/native artifact changed/);
    expect(() =>
      validateStableBenchmarkProvenance(source, source, native, {
        ...native,
        name: "different-native.node",
      }),
    ).toThrow(/native artifact changed/);
  });
});
