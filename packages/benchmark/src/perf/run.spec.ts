import { describe, it, expect, vi } from "vitest";
import {
  discoverTsgoVersion,
  summarizeRssSamples,
  medianPresentOrNull,
  summarizeMetricSamples,
  printReport,
  type RunnerReport,
} from "./run.js";
import { resolveEngineVersion } from "./gate.js";
import { summarize } from "./stats.js";
import type { WorkloadSample } from "./workloads.js";

describe("run.ts engine label is canonical (resolved from the workspace ROOT typescript)", () => {
  it("discoverTsgoVersion equals gate.resolveEngineVersion (NOT the bench-local typescript)", () => {
    // The standalone runner must record the SAME engine the gate pins: the
    // workspace-root `typescript` (tsgo = TS7), not the benchmark package's own
    // older `typescript` devDep. Resolving from `import.meta.url` (the bench
    // package) recorded the wrong version; this keeps the two in lockstep.
    expect(discoverTsgoVersion()).toBe(resolveEngineVersion());
    expect(discoverTsgoVersion()).toMatch(/^typescript@/);
    expect(discoverTsgoVersion()).not.toMatch(/native-preview/);
  });
});

describe("run.ts RSS reporting is honest (median of PEAKs; null when not sampled)", () => {
  const s = (rssBytes: number | null): WorkloadSample => ({
    totalMs: 0,
    attribution: null,
    rssBytes,
    metrics: {},
  });

  it("reports null peak + null median-peak when NO sample carried RSS (e.g. LSP workloads)", () => {
    const r = summarizeRssSamples([s(null), s(null)]);
    expect(r.peak).toBeNull();
    expect(r.medianPeak).toBeNull();
  });

  it("reports the max peak and the MEDIAN of per-run peaks (never a fabricated steady-state)", () => {
    const r = summarizeRssSamples([s(100), s(300), s(200)]);
    expect(r.peak).toBe(300);
    expect(r.medianPeak).toBe(200); // p50 of [100, 200, 300]
  });

  it("ignores null samples when SOME runs carried RSS", () => {
    const r = summarizeRssSamples([s(null), s(100), s(300)]);
    expect(r.peak).toBe(300);
    expect(r.medianPeak).toBe(200); // p50 of [100, 300]
  });
});

describe("run.ts attribution: a missing measurement is null, never a fabricated 0", () => {
  it("medianPresentOrNull returns NULL for an entirely-missing / empty vector (never 0)", () => {
    // `med` must return null (not 0) when no value is present — a 0 would fabricate
    // a fully-unavailable attribution field (codegenMs, sourcemapMs, …) as a present
    // value in the standalone harness output, a false lower-is-better measurement.
    expect(medianPresentOrNull([null, null])).toBeNull();
    expect(medianPresentOrNull([])).toBeNull();
    expect(medianPresentOrNull([undefined as unknown as number | null])).toBeNull();
  });

  it("medianPresentOrNull returns the median of the PRESENT values, ignoring nulls", () => {
    expect(medianPresentOrNull([10, null, 20, null, 30])).toBe(20);
    expect(medianPresentOrNull([null, 42])).toBe(42);
  });

  it("printReport renders (not sampled) for a null attribution field — never 0", () => {
    const report: RunnerReport = {
      timestamp: new Date().toISOString(),
      corpusHash: "sha256:test",
      corpusFiles: 1,
      isGateCorpus: false,
      threads: 1,
      runsPerWorkload: 1,
      tsgoVersion: "typescript@7.0.1-rc",
      results: [
        {
          id: "axis-a-codegen",
          axis: "A",
          title: "axis A",
          interactive: false,
          skipped: false,
          runs: 1,
          totalMs: summarize([100]),
          // nonCheckerMs + parse/transform/transport are DEFERRED (null on a real
          // run); only codegen + source-map are present.
          attribution: {
            codegenMs: 10,
            sourcemapMs: 5,
            parseTransformTransportMs: null,
            nonCheckerMs: null,
            outputBytes: 100_000,
            sourceMapBytes: null,
            codeTransformOps: null,
          },
          peakRssBytes: null,
          medianPeakRssBytes: null,
          metrics: {},
          latencyDistributions: {},
        },
      ],
    };
    const lines: string[] = [];
    const spy = vi.spyOn(console, "log").mockImplementation((...args: unknown[]) => {
      lines.push(args.map((a) => String(a)).join(" "));
    });
    try {
      printReport(report);
    } finally {
      spy.mockRestore();
    }
    const out = lines.join("\n");
    const attribLine = out.split("\n").find((l) => l.includes("non-checker")) ?? "";
    expect(attribLine).toMatch(/non-checker \(not sampled\)/);
    // The null field must NOT render as a fabricated 0ms / 0KB.
    expect(attribLine).not.toMatch(/non-checker 0\.00ms/);
    const bytesLine = out.split("\n").find((l) => l.includes("transform-ops")) ?? "";
    expect(bytesLine).toMatch(/sourcemap \(not sampled\)/);
    expect(bytesLine).toMatch(/transform-ops \(not sampled\)/);
    expect(bytesLine).not.toMatch(/sourcemap 0KB/);
  });
});

describe("run.ts metric summary omits a missing sample, never fabricates a present 0", () => {
  it("summarizeMetricSamples summarizes ONLY present finite values (a missing sample is omitted, not 0)", () => {
    // The metrics summary must NOT map `s.metrics[k] ?? 0` — a sample MISSING the
    // metric would fabricate a present 0 that skews the standalone summary. The
    // present-only summary ignores the missing samples entirely.
    const s = summarizeMetricSamples([10, undefined, 20, undefined, 30]);
    expect(s.p50).toBe(20); // p50 of [10, 20, 30], NOT [0, 0, 10, 20, 30]
    expect(s.n).toBe(3);
    expect(summarizeMetricSamples([100]).p50).toBe(100);
    // Non-finite readings are broken instrumentation — omitted too, never 0.
    const f = summarizeMetricSamples([NaN, 50, Infinity]);
    expect(f.n).toBe(1);
    expect(f.p50).toBe(50);
  });
});

describe("run.ts memory print is honest (median-peak null renders (not sampled), never a fabricated 0MB)", () => {
  it("printReport renders median-peak (not sampled) for a null medianPeakRssBytes — never 0MB", () => {
    const report: RunnerReport = {
      timestamp: new Date().toISOString(),
      corpusHash: "sha256:test",
      corpusFiles: 1,
      isGateCorpus: false,
      threads: 1,
      runsPerWorkload: 1,
      tsgoVersion: "typescript@7.0.1-rc",
      results: [
        {
          id: "cold-typecheck",
          axis: "B",
          title: "cold",
          interactive: false,
          skipped: false,
          runs: 1,
          totalMs: summarize([100]),
          attribution: null,
          // peak sampled, but the MEDIAN of per-run peaks is unavailable — it must
          // render (not sampled), never a fabricated 0MB from a `?? 0` coercion.
          peakRssBytes: 1_000_000,
          medianPeakRssBytes: null,
          metrics: {},
          latencyDistributions: {},
        },
      ],
    };
    const lines: string[] = [];
    const spy = vi.spyOn(console, "log").mockImplementation((...args: unknown[]) => {
      lines.push(args.map((a) => String(a)).join(" "));
    });
    try {
      printReport(report);
    } finally {
      spy.mockRestore();
    }
    const memLine = lines.find((l) => l.includes("median-peak")) ?? "";
    expect(memLine).toMatch(/median-peak \(not sampled\)/);
    expect(memLine).not.toMatch(/median-peak 0\.0MB/);
  });
});
