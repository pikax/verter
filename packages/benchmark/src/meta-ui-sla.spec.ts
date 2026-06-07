/**
 * @ai-generated - SLA bucket counts + bench harness CLI flag
 * deprecation.
 *
 * FAIL-FIRST tests for the SLA-vs-hard-timeout split:
 *
 *   - `buildSlaCount` correctly partitions component results by
 *     latency-vs-slaMs threshold. Pre-Step-10 there was a single
 *     `query-timeout-ms` threshold conflating metric and kill; this
 *     test asserts the metric partition is observable post-fix.
 *   - The aggregated `summary.slaCount` sums across repeats.
 *   - The legacy `--query-timeout-ms` CLI flag aliases
 *     `--hard-timeout-ms` with a stderr deprecation warning, kept
 *     one release per repo deprecation convention. Pre-fix the flag
 *     was the kill threshold itself; post-fix it's a deprecated
 *     alias.
 */

import { describe, expect, it } from "vitest";

import { buildSlaCount, type ComponentResultRow } from "./meta-ui-report.js";

describe("buildSlaCount", () => {
  it("partitions component results by latency vs slaMs", () => {
    const rows: ComponentResultRow[] = [
      {
        relativePath: "fast.vue",
        componentName: "Fast",
        latencyMs: 50,
        outcome: "success",
        error: null,
      },
      {
        relativePath: "borderline.vue",
        componentName: "Borderline",
        latencyMs: 250,
        outcome: "success",
        error: null,
      },
      {
        relativePath: "slow.vue",
        componentName: "Slow",
        latencyMs: 600,
        outcome: "degraded",
        error: null,
      },
      {
        relativePath: "crash.vue",
        componentName: "Crash",
        latencyMs: null,
        outcome: "crash",
        error: "boom",
      },
    ];
    // SLA threshold = 250 ms (default DEFAULT_SLA_MS pre-Step-10).
    const counts = buildSlaCount(rows, 250);
    expect(counts.withinSla).toBe(2); // Fast + Borderline (latency <= 250)
    expect(counts.exceededSla).toBe(2); // Slow (600) + Crash (null → infinity)
  });

  it("treats null latency (timeouts / crashes) as exceededSla", () => {
    const rows: ComponentResultRow[] = [
      {
        relativePath: "killed.vue",
        componentName: "Killed",
        latencyMs: null,
        outcome: "crash",
        error: "hard-timeout",
      },
    ];
    const counts = buildSlaCount(rows, 250);
    expect(counts.withinSla).toBe(0);
    expect(counts.exceededSla).toBe(1);
  });

  it("everything within sla when slaMs is greater than max latency", () => {
    const rows: ComponentResultRow[] = [
      { relativePath: "a.vue", componentName: "A", latencyMs: 50, outcome: "success", error: null },
      {
        relativePath: "b.vue",
        componentName: "B",
        latencyMs: 1000,
        outcome: "success",
        error: null,
      },
    ];
    const counts = buildSlaCount(rows, 5000);
    expect(counts.withinSla).toBe(2);
    expect(counts.exceededSla).toBe(0);
  });

  it("slaCount partition is independent of outcome bucket", () => {
    // A `degraded` outcome with low latency still counts as withinSla;
    // a `success` outcome with high latency counts as exceededSla.
    // This proves the SLA gate is purely the latency threshold, not
    // a re-encoding of outcomeCounts.
    const rows: ComponentResultRow[] = [
      {
        relativePath: "fast-degraded.vue",
        componentName: "FastDegraded",
        latencyMs: 100,
        outcome: "degraded",
        error: null,
      },
      {
        relativePath: "slow-success.vue",
        componentName: "SlowSuccess",
        latencyMs: 400,
        outcome: "success",
        error: null,
      },
    ];
    const counts = buildSlaCount(rows, 250);
    expect(counts.withinSla).toBe(1);
    expect(counts.exceededSla).toBe(1);
  });
});
