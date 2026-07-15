/**
 * Profile-audit mode for bench:meta:ui (--profile-audit, with
 * --memory-audit kept as an alias) — flag parsing, the loud-failure
 * setup gate against binaries that predate the runtime memory-audit
 * surface, the runtime enable handshake, per-query delta math, phase-
 * timing extraction from the native audit record, sampled allocation
 * sites, and the separate .profile.json artifact. Hermetic: native
 * access is mocked; no built .node or corpus checkout is required.
 */

import { existsSync, readFileSync } from "node:fs";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import {
  buildProfileAuditArtifact,
  computeMemoryAuditMeasure,
  ensureMemoryAuditCapable,
  extractAuditTimings,
  parseMemoryAuditSites,
  type MemoryAuditSiteRow,
  type MemoryAuditSnapshot,
} from "./meta-ui-core.js";
import {
  parseMetaUiBenchArgs,
  shouldCollectMemorySites,
  writeProfileAuditArtifact,
} from "./meta-ui-bench.js";

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

describe("parseMetaUiBenchArgs --profile-audit", () => {
  it("keeps profile audit off by default", () => {
    expect(parseMetaUiBenchArgs([]).profileAudit).toBe(false);
  });

  it("enables profile audit with --profile-audit", () => {
    expect(parseMetaUiBenchArgs(["--profile-audit"]).profileAudit).toBe(true);
  });

  it("keeps --memory-audit as an alias for --profile-audit", () => {
    expect(parseMetaUiBenchArgs(["--memory-audit"]).profileAudit).toBe(true);
  });
});

describe("ensureMemoryAuditCapable (loud-failure setup gate + runtime enable)", () => {
  // A binding whose exports are missing entirely — an older prebuilt
  // binary that predates the runtime memory-audit surface. This is the
  // ONLY loud-failure class now: the single binary always carries the
  // surface and enables at runtime.
  it("throws loudly when the binding lacks the memoryAuditSnapshot export", () => {
    expect(() => ensureMemoryAuditCapable({})).toThrow(/@verter\/native/);
  });

  it("throws loudly when the binding lacks the memoryAuditEnable export (old binary)", () => {
    const binding = {
      memoryAuditSnapshot: () => snapshot(),
      memoryAuditResetHighWater: () => true,
    };
    expect(() => ensureMemoryAuditCapable(binding)).toThrow(/memoryAuditEnable/);
  });

  it("enables the runtime audit before probing the snapshot", () => {
    const calls: string[] = [];
    let enabled = false;
    const binding = {
      memoryAuditEnable: () => {
        calls.push("enable");
        enabled = true;
        return true;
      },
      memoryAuditSnapshot: () => {
        calls.push("snapshot");
        return enabled ? snapshot({ allocCount: 7 }) : null;
      },
      memoryAuditResetHighWater: () => enabled,
    };
    const capable = ensureMemoryAuditCapable(binding);
    expect(calls[0]).toBe("enable");
    expect(capable.snapshot().allocCount).toBe(7);
  });

  it("throws loudly when the snapshot stays null even after enabling", () => {
    const binding = {
      memoryAuditEnable: () => true,
      memoryAuditSnapshot: () => null,
      memoryAuditResetHighWater: () => false,
    };
    expect(() => ensureMemoryAuditCapable(binding)).toThrow(/memoryAuditEnable/);
  });

  it("returns a capable handle for an enabled binding", () => {
    let resets = 0;
    const binding = {
      memoryAuditEnable: () => true,
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

describe("shouldCollectMemorySites (env gate)", () => {
  it("collects only when profile audit is on AND VERTER_MEMORY_AUDIT_SAMPLE is set", () => {
    expect(shouldCollectMemorySites(true, { VERTER_MEMORY_AUDIT_SAMPLE: "97" })).toBe(true);
    expect(shouldCollectMemorySites(false, { VERTER_MEMORY_AUDIT_SAMPLE: "97" })).toBe(false);
    expect(shouldCollectMemorySites(true, {})).toBe(false);
    expect(shouldCollectMemorySites(true, { VERTER_MEMORY_AUDIT_SAMPLE: "  " })).toBe(false);
  });
});

function siteRow(overrides: Partial<MemoryAuditSiteRow> = {}): MemoryAuditSiteRow {
  return {
    count: 12,
    bytes: 48_000,
    estimatedTotalBytes: 4_656_000,
    frames: ["verter_session::foo::bar", "verter_session::baz"],
    ...overrides,
  };
}

describe("parseMemoryAuditSites", () => {
  it("passes null through (sampling not armed / audit disabled)", () => {
    expect(parseMemoryAuditSites(null)).toBeNull();
  });

  it("parses a valid site report", () => {
    const rows = [
      siteRow(),
      siteRow({ count: 1, bytes: 8, estimatedTotalBytes: 776, frames: ["a"] }),
    ];
    expect(parseMemoryAuditSites(JSON.stringify(rows))).toEqual(rows);
  });

  it("parses an empty report (armed but nothing sampled)", () => {
    expect(parseMemoryAuditSites("[]")).toEqual([]);
  });

  it("rejects malformed JSON as null", () => {
    expect(parseMemoryAuditSites("{not json")).toBeNull();
  });

  it("rejects a non-array payload as null", () => {
    expect(parseMemoryAuditSites(JSON.stringify({ sites: [] }))).toBeNull();
  });

  it("rejects rows missing the wire fields as null", () => {
    expect(parseMemoryAuditSites(JSON.stringify([{ count: 1, bytes: 2 }]))).toBeNull();
    expect(
      parseMemoryAuditSites(
        JSON.stringify([{ count: 1, bytes: 2, estimatedTotalBytes: "3", frames: [] }]),
      ),
    ).toBeNull();
  });
});

describe("ensureMemoryAuditCapable sites surface (additive)", () => {
  const enabledBinding = {
    memoryAuditEnable: () => true,
    memoryAuditSnapshot: () => snapshot(),
    memoryAuditResetHighWater: () => true,
  };

  it("returns null sites when the binding lacks the memoryAuditSites export", () => {
    const capable = ensureMemoryAuditCapable({ ...enabledBinding });
    expect(capable.sites(50)).toBeNull();
  });

  it("returns null sites when sampling was not armed (native returns null)", () => {
    const capable = ensureMemoryAuditCapable({
      ...enabledBinding,
      memoryAuditSites: () => null,
    });
    expect(capable.sites(50)).toBeNull();
  });

  it("parses the native JSON report and forwards topK", () => {
    const seen: number[] = [];
    const rows = [siteRow()];
    const capable = ensureMemoryAuditCapable({
      ...enabledBinding,
      memoryAuditSites: (topK: number) => {
        seen.push(topK);
        return JSON.stringify(rows);
      },
    });
    expect(capable.sites(50)).toEqual(rows);
    expect(seen).toEqual([50]);
  });
});

describe("extractAuditTimings", () => {
  const record = {
    timings: {
      total_ms: 123.5,
      capture_inputs_ms: 0.1,
      store_read_ms: 4.25,
      store_merge_ms: 1.5,
      solver_ms: 88.0,
      materialize_ms: 20.75,
      serialize_ms: 2.0,
    },
  };

  it("maps the native snake_case phase timings onto the camelCase row shape", () => {
    expect(extractAuditTimings(record)).toEqual({
      totalMs: 123.5,
      materializeMs: 20.75,
      solverMs: 88.0,
      storeReadMs: 4.25,
      storeMergeMs: 1.5,
    });
  });

  it("returns null for a record without timings", () => {
    expect(extractAuditTimings({})).toBeNull();
    expect(extractAuditTimings(null)).toBeNull();
    expect(extractAuditTimings("nope")).toBeNull();
  });

  it("returns null when a phase field is missing or non-numeric", () => {
    expect(extractAuditTimings({ timings: { ...record.timings, solver_ms: "88" } })).toBeNull();
    const { solver_ms: _dropped, ...withoutSolver } = record.timings;
    expect(extractAuditTimings({ timings: withoutSolver })).toBeNull();
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

describe("buildProfileAuditArtifact", () => {
  it("aggregates per-component rows (including timings) into sum/max totals", () => {
    const artifact = buildProfileAuditArtifact({
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
          timings: {
            totalMs: 12,
            materializeMs: 4,
            solverMs: 6,
            storeReadMs: 1,
            storeMergeMs: 0.5,
          },
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

    expect(artifact.kind).toBe("meta-ui-profile-audit");
    expect(artifact.backend).toBe("verter");
    expect(artifact.scenario).toBe("repo_first_pass");
    expect(artifact.components).toHaveLength(2);
    expect(artifact.components[0]?.timings?.solverMs).toBe(6);
    expect(artifact.components[1]?.timings).toBeUndefined();
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
    const artifact = buildProfileAuditArtifact({
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

  it("attaches sampled allocation sites under `sites` when collected", () => {
    const sites = [siteRow(), siteRow({ bytes: 8, estimatedTotalBytes: 776 })];
    const artifact = buildProfileAuditArtifact({
      backend: "verter",
      scenario: "repo_first_pass",
      rows: [],
      sites,
    });
    expect(artifact.sites).toEqual(sites);
  });

  it("keeps `sites` collected-but-empty distinct from not-collected", () => {
    const collected = buildProfileAuditArtifact({
      backend: "verter",
      scenario: "repo_first_pass",
      rows: [],
      sites: [],
    });
    expect(collected.sites).toEqual([]);
    expect("sites" in collected).toBe(true);
  });

  it("omits the `sites` key entirely when site sampling was not armed", () => {
    const absent = buildProfileAuditArtifact({
      backend: "verter",
      scenario: "repo_first_pass",
      rows: [],
    });
    expect("sites" in absent).toBe(false);
    const explicitNull = buildProfileAuditArtifact({
      backend: "verter",
      scenario: "repo_first_pass",
      rows: [],
      sites: null,
    });
    expect("sites" in explicitNull).toBe(false);
  });
});

describe("writeProfileAuditArtifact", () => {
  it("writes meta-ui-<backend>-<scenario>.profile.json separate from the timing artifact", () => {
    const outputDir = mkdtempSync(resolve(tmpdir(), "verter-profile-audit-artifact-"));
    const artifact = buildProfileAuditArtifact({
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

    const filePath = writeProfileAuditArtifact(outputDir, artifact);

    expect(filePath).toBe(resolve(outputDir, "meta-ui-verter-repo_first_pass.profile.json"));
    const parsed = JSON.parse(readFileSync(filePath, "utf8"));
    expect(parsed.kind).toBe("meta-ui-profile-audit");
    expect(parsed.components).toHaveLength(1);
    expect(existsSync(resolve(outputDir, "meta-ui-verter-repo_first_pass.json"))).toBe(false);
  });
});
