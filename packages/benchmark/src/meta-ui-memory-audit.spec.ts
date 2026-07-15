/**
 * Memory-audit mode for bench:meta:ui — flag parsing, the loud-failure
 * setup gate against non-instrumented binaries, per-query delta math,
 * and the separate .memory.json artifact. Hermetic: native access is
 * mocked; no built .node or corpus checkout is required.
 */

import { existsSync, readFileSync } from "node:fs";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import {
  buildMemoryAuditArtifact,
  computeMemoryAuditMeasure,
  ensureMemoryAuditCapable,
  type MemoryAuditSnapshot,
} from "./meta-ui-core.js";
import { parseMetaUiBenchArgs, writeMemoryAuditArtifact } from "./meta-ui-bench.js";

function snapshot(overrides: Partial<MemoryAuditSnapshot> = {}): MemoryAuditSnapshot {
  return {
    allocCount: 0,
    deallocCount: 0,
    allocatedBytesTotal: 0,
    liveBytes: 0,
    peakLiveBytes: 0,
    ...overrides,
  };
}

describe("parseMetaUiBenchArgs --memory-audit", () => {
  it("keeps memory audit off by default", () => {
    expect(parseMetaUiBenchArgs([]).memoryAudit).toBe(false);
  });

  it("enables memory audit with --memory-audit", () => {
    expect(parseMetaUiBenchArgs(["--memory-audit"]).memoryAudit).toBe(true);
  });
});

describe("ensureMemoryAuditCapable (loud-failure setup gate)", () => {
  // A binding whose export is missing entirely — an older prebuilt
  // binary that predates the memory-audit surface.
  it("throws loudly when the binding lacks the memoryAuditSnapshot export", () => {
    expect(() => ensureMemoryAuditCapable({})).toThrow(/--features memory_audit/);
    expect(() => ensureMemoryAuditCapable({})).toThrow(/build:memory-audit/);
  });

  // The always-exported fns exist but the binary was built WITHOUT the
  // cargo feature: snapshot() returns null by contract.
  it("throws loudly when memoryAuditSnapshot() returns null (non-instrumented binary)", () => {
    const binding = {
      memoryAuditSnapshot: () => null,
      memoryAuditResetHighWater: () => false,
    };
    expect(() => ensureMemoryAuditCapable(binding)).toThrow(/--features memory_audit/);
    expect(() => ensureMemoryAuditCapable(binding)).toThrow(/build:memory-audit/);
  });

  it("throws loudly when the reset export is missing even if snapshot works", () => {
    const binding = {
      memoryAuditSnapshot: () => snapshot(),
    };
    expect(() => ensureMemoryAuditCapable(binding)).toThrow(/--features memory_audit/);
  });

  it("returns a capable handle for an instrumented binding", () => {
    let resets = 0;
    const binding = {
      memoryAuditSnapshot: () => snapshot({ allocCount: 7 }),
      memoryAuditResetHighWater: () => {
        resets++;
        return true;
      },
    };
    const capable = ensureMemoryAuditCapable(binding);
    expect(capable.snapshot().allocCount).toBe(7);
    capable.resetHighWater();
    expect(resets).toBe(1);
  });
});

describe("computeMemoryAuditMeasure", () => {
  it("reports allocator deltas plus window peak and process memory", () => {
    const before = snapshot({
      allocCount: 100,
      deallocCount: 90,
      allocatedBytesTotal: 10_000,
      liveBytes: 1_000,
      peakLiveBytes: 1_000,
    });
    const after = snapshot({
      allocCount: 175,
      deallocCount: 160,
      allocatedBytesTotal: 46_000,
      liveBytes: 2_048,
      peakLiveBytes: 30_720,
    });

    const measure = computeMemoryAuditMeasure(before, after, {
      rss: 123_456_789,
      heapUsed: 9_876_543,
    });

    expect(measure).toEqual({
      allocCount: 75,
      allocatedBytes: 36_000,
      peakLiveBytes: 30_720,
      rssBytes: 123_456_789,
      jsHeapUsedBytes: 9_876_543,
    });
  });
});

describe("buildMemoryAuditArtifact", () => {
  it("aggregates per-component rows into sum/max totals", () => {
    const artifact = buildMemoryAuditArtifact({
      backend: "verter",
      scenario: "repo_first_pass",
      rows: [
        {
          relativePath: "src/runtime/components/A.vue",
          repeatIndex: 1,
          allocCount: 10,
          allocatedBytes: 1_000,
          peakLiveBytes: 500,
          rssBytes: 100,
          jsHeapUsedBytes: 50,
        },
        {
          relativePath: "src/runtime/components/B.vue",
          repeatIndex: 1,
          allocCount: 30,
          allocatedBytes: 4_000,
          peakLiveBytes: 900,
          rssBytes: 700,
          jsHeapUsedBytes: 20,
        },
      ],
    });

    expect(artifact.kind).toBe("meta-ui-memory-audit");
    expect(artifact.backend).toBe("verter");
    expect(artifact.scenario).toBe("repo_first_pass");
    expect(artifact.components).toHaveLength(2);
    expect(artifact.totals).toEqual({
      components: 2,
      allocCount: 40,
      allocatedBytes: 5_000,
      maxPeakLiveBytes: 900,
      maxRssBytes: 700,
      maxJsHeapUsedBytes: 50,
    });
    expect(typeof artifact.generatedAt).toBe("string");
  });

  it("produces zeroed totals for an empty run", () => {
    const artifact = buildMemoryAuditArtifact({
      backend: "verter",
      scenario: "single_cold",
      rows: [],
    });
    expect(artifact.totals).toEqual({
      components: 0,
      allocCount: 0,
      allocatedBytes: 0,
      maxPeakLiveBytes: 0,
      maxRssBytes: 0,
      maxJsHeapUsedBytes: 0,
    });
  });
});

describe("writeMemoryAuditArtifact", () => {
  it("writes meta-ui-<backend>-<scenario>.memory.json separate from the timing artifact", () => {
    const outputDir = mkdtempSync(resolve(tmpdir(), "verter-memory-audit-artifact-"));
    const artifact = buildMemoryAuditArtifact({
      backend: "verter",
      scenario: "repo_first_pass",
      rows: [
        {
          relativePath: "src/runtime/components/A.vue",
          repeatIndex: 1,
          allocCount: 1,
          allocatedBytes: 2,
          peakLiveBytes: 3,
          rssBytes: 4,
          jsHeapUsedBytes: 5,
        },
      ],
    });

    const filePath = writeMemoryAuditArtifact(outputDir, artifact);

    expect(filePath).toBe(resolve(outputDir, "meta-ui-verter-repo_first_pass.memory.json"));
    const parsed = JSON.parse(readFileSync(filePath, "utf8"));
    expect(parsed.kind).toBe("meta-ui-memory-audit");
    expect(parsed.components[0].relativePath).toBe("src/runtime/components/A.vue");
    // The timing artifact for the same run must NOT be touched by the
    // memory artifact writer.
    expect(existsSync(resolve(outputDir, "meta-ui-verter-repo_first_pass.json"))).toBe(false);
  });
});
