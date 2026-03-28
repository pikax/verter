/**
 * @ai-generated - Verifies aggregate meta-ui benchmark report rendering and ratio summaries.
 */

import { describe, expect, it } from "vitest";

import {
  buildMetaUiAggregateReport,
  buildMetaUiMarkdownReport,
  type MetaUiBenchmarkRun,
} from "./meta-ui-report.js";

function makeRun(overrides: Partial<MetaUiBenchmarkRun>): MetaUiBenchmarkRun {
  return {
    kind: "meta-ui-benchmark-run",
    generatedAt: "2026-03-27T00:00:00.000Z",
    version: {
      benchmarkPackageVersion: "0.0.1",
      verterCommitSha: "abc123",
      resolvedTargetSha: "ui123",
      vueComponentMetaVersion: "3.2.6",
      nodeVersion: "v20.0.0",
    },
    target: {
      project: "nuxt-ui",
      repo: "nuxt/ui",
      branch: "v4",
      root: "/tmp/nuxt-ui",
      componentsDir: "/tmp/nuxt-ui/src/runtime/components",
      componentCount: 2,
    },
    config: {
      backend: "verter",
      scenario: "single_cold",
      repeats: 5,
      warmupPasses: 1,
      runtimeMode: "dedicated",
    },
    repeats: [
      {
        index: 1,
        orderStart: 0,
        setupMs: 10,
        warmupMs: 0,
        steadyStateMs: 20,
        endToEndMs: 30,
        componentLatenciesMs: [9, 11],
        outcomeCounts: { success: 2, degraded: 0, query_error: 0, crash: 0 },
        deviationTotals: {
          exactMatches: 2,
          totalMissing: 0,
          totalExtra: 0,
          totalFieldMismatches: 0,
        },
        stats: { min: 20, max: 20, p50: 20, p95: 20, p99: 20, mean: 20, stddev: 0 },
      },
    ],
    summary: {
      setupMs: { min: 10, max: 10, p50: 10, p95: 10, p99: 10, mean: 10, stddev: 0 },
      warmupMs: { min: 0, max: 0, p50: 0, p95: 0, p99: 0, mean: 0, stddev: 0 },
      steadyStateMs: { min: 20, max: 20, p50: 20, p95: 20, p99: 20, mean: 20, stddev: 0 },
      endToEndMs: { min: 30, max: 30, p50: 30, p95: 30, p99: 30, mean: 30, stddev: 0 },
      outcomeCounts: { success: 2, degraded: 0, query_error: 0, crash: 0 },
      deviationTotals: { exactMatches: 2, totalMissing: 0, totalExtra: 0, totalFieldMismatches: 0 },
    },
    ...overrides,
  };
}

describe("buildMetaUiAggregateReport", () => {
  it("groups runs by scenario and computes relative speed ratios", () => {
    const report = buildMetaUiAggregateReport([
      makeRun({
        config: {
          backend: "verter",
          scenario: "single_cold",
          repeats: 5,
          warmupPasses: 1,
          runtimeMode: "dedicated",
        },
      }),
      makeRun({
        config: {
          backend: "vue-component-meta",
          scenario: "single_cold",
          repeats: 5,
          warmupPasses: 1,
          runtimeMode: "dedicated",
        },
        summary: {
          setupMs: { min: 8, max: 8, p50: 8, p95: 8, p99: 8, mean: 8, stddev: 0 },
          warmupMs: { min: 0, max: 0, p50: 0, p95: 0, p99: 0, mean: 0, stddev: 0 },
          steadyStateMs: { min: 40, max: 40, p50: 40, p95: 40, p99: 40, mean: 40, stddev: 0 },
          endToEndMs: { min: 48, max: 48, p50: 48, p95: 48, p99: 48, mean: 48, stddev: 0 },
          outcomeCounts: { success: 2, degraded: 0, query_error: 0, crash: 0 },
          deviationTotals: {
            exactMatches: 2,
            totalMissing: 0,
            totalExtra: 0,
            totalFieldMismatches: 0,
          },
        },
      }),
    ]);

    expect(report.kind).toBe("meta-ui-benchmark-report");
    expect(report.scenarios.single_cold.backends.verter.relativeToBaseline).toBe(0.5);
    expect(report.scenarios.single_cold.backends["vue-component-meta"].relativeToVerter).toBe(2);
  });
});

describe("buildMetaUiMarkdownReport", () => {
  it("renders scenario tables with steady-state and end-to-end metrics", () => {
    const report = buildMetaUiAggregateReport([
      makeRun({
        config: {
          backend: "verter",
          scenario: "single_cold",
          repeats: 5,
          warmupPasses: 1,
          runtimeMode: "dedicated",
        },
      }),
      makeRun({
        config: {
          backend: "vue-component-meta",
          scenario: "single_cold",
          repeats: 5,
          warmupPasses: 1,
          runtimeMode: "dedicated",
        },
        summary: {
          setupMs: { min: 8, max: 8, p50: 8, p95: 8, p99: 8, mean: 8, stddev: 0 },
          warmupMs: { min: 0, max: 0, p50: 0, p95: 0, p99: 0, mean: 0, stddev: 0 },
          steadyStateMs: { min: 40, max: 40, p50: 40, p95: 40, p99: 40, mean: 40, stddev: 0 },
          endToEndMs: { min: 48, max: 48, p50: 48, p95: 48, p99: 48, mean: 48, stddev: 0 },
          outcomeCounts: { success: 2, degraded: 0, query_error: 0, crash: 0 },
          deviationTotals: {
            exactMatches: 2,
            totalMissing: 0,
            totalExtra: 0,
            totalFieldMismatches: 0,
          },
        },
      }),
    ]);

    const markdown = buildMetaUiMarkdownReport(report);

    expect(markdown).toContain("## Meta UI Benchmark Results");
    expect(markdown).toContain("`nuxt/ui@ui123`");
    expect(markdown).toContain("### single_cold");
    expect(markdown).toContain("| Backend |");
    expect(markdown).toContain("| steady p50 |");
    expect(markdown).toContain("vue-component-meta");
    expect(markdown).toContain("2.00x");
  });

  it("includes degraded counts in the markdown summary table", () => {
    const report = buildMetaUiAggregateReport([
      makeRun({
        config: {
          backend: "verter",
          scenario: "single_cold",
          repeats: 5,
          warmupPasses: 1,
          runtimeMode: "dedicated",
        },
        summary: {
          setupMs: { min: 10, max: 10, p50: 10, p95: 10, p99: 10, mean: 10, stddev: 0 },
          warmupMs: { min: 0, max: 0, p50: 0, p95: 0, p99: 0, mean: 0, stddev: 0 },
          steadyStateMs: { min: 20, max: 20, p50: 20, p95: 20, p99: 20, mean: 20, stddev: 0 },
          endToEndMs: { min: 30, max: 30, p50: 30, p95: 30, p99: 30, mean: 30, stddev: 0 },
          outcomeCounts: { success: 1, degraded: 1, query_error: 0, crash: 0 },
          deviationTotals: {
            exactMatches: 1,
            totalMissing: 0,
            totalExtra: 0,
            totalFieldMismatches: 0,
          },
        },
      }),
    ]);

    const markdown = buildMetaUiMarkdownReport(report);

    expect(markdown).toContain("| degraded | crashes | errors |");
    expect(markdown).toContain(
      "| verter | 20.00ms | 30.00ms | 20.00ms | 0.00ms | 1.00x | N/A | 1 | 0 | 0 | 0 | 1 | 0 | 0 |",
    );
  });
});
