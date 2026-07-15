import { describe, expect, it } from "vitest";

import { classifyLatency, percentile, summarizeLatency } from "../src/collectors/index.js";
import type { CollectorEventKey } from "../src/collectors/index.js";

const key: CollectorEventKey = {
  scenario: "minimal-member-access",
  editStepIndex: 0,
  driver: "rawLsp",
  provider: "tsgo",
  probe: "hover-latency",
  version: 1,
  anchor: "ref",
};

describe("percentile — R-7 linear interpolation", () => {
  it("computes exact and interpolated percentiles on a small sample", () => {
    const sorted = [10, 20, 30, 40, 50];
    expect(percentile(sorted, 50)).toBe(30);
    expect(percentile(sorted, 0)).toBe(10);
    expect(percentile(sorted, 100)).toBe(50);
    expect(percentile(sorted, 95)).toBeCloseTo(48, 6);
    expect(percentile(sorted, 99)).toBeCloseTo(49.6, 6);
  });

  it("matches the canonical p95 of 1..100", () => {
    const sorted = Array.from({ length: 100 }, (_, i) => i + 1);
    expect(percentile(sorted, 50)).toBeCloseTo(50.5, 6);
    expect(percentile(sorted, 95)).toBeCloseTo(95.05, 6);
    expect(percentile(sorted, 99)).toBeCloseTo(99.01, 6);
  });

  it("is total over a single sample and an empty sample", () => {
    expect(percentile([42], 95)).toBe(42);
    expect(percentile([], 95)).toBe(0);
  });
});

describe("summarizeLatency", () => {
  it("summarizes count/min/max/mean and the percentiles (input order-insensitive)", () => {
    const summary = summarizeLatency([50, 10, 30, 40, 20]);
    expect(summary.count).toBe(5);
    expect(summary.min).toBe(10);
    expect(summary.max).toBe(50);
    expect(summary.mean).toBe(30);
    expect(summary.p50).toBe(30);
  });

  it("is total over an empty sample", () => {
    const summary = summarizeLatency([]);
    expect(summary).toEqual({ count: 0, min: 0, max: 0, mean: 0, p50: 0, p95: 0, p99: 0 });
  });
});

describe("classifyLatency", () => {
  it("flags a p95 over its threshold as user-visible", () => {
    const samples = Array.from({ length: 100 }, (_, i) => i + 1); // p95 ≈ 95.05
    const event = classifyLatency({ key, method: "hover", samples, thresholds: { p95Ms: 50 } });
    expect(event.ok).toBe(false);
    expect(event.severity).toBe("userVisible");
    expect(event.signal).toBe("latency_breach");
    expect((event.data as { p95?: number }).p95).toBeCloseTo(95.05, 2);
    expect((event.data as { breaches?: string[] }).breaches).toContain("p95");
  });

  it("passes when every measured percentile is within its threshold", () => {
    const samples = [10, 12, 14, 16, 18];
    const event = classifyLatency({
      key,
      method: "hover",
      samples,
      thresholds: { p50Ms: 50, p95Ms: 50, p99Ms: 50 },
    });
    expect(event.ok).toBe(true);
    expect(event.signal).toBe("latency_summary");
  });

  it("reports the summary even with no thresholds (report-only)", () => {
    const event = classifyLatency({ key, method: "completion", samples: [5, 6, 7] });
    expect(event.ok).toBe(true);
    expect((event.data as { count?: number }).count).toBe(3);
  });
});
