/**
 * Provider-sample attribution: proving the RSS ceiling watches the real process.
 *
 * The defect this suite exists for: a `node` child was directly observed at
 * ~3.9 GB RSS while the receipt recorded that route's provider at 357 MB. The
 * sampler was bounding one advertised pid, so the process that actually held
 * the memory was outside the ceiling entirely — a genuine tsserver blow-up
 * would have passed unseen.
 *
 * Two guarantees are asserted here, in both directions:
 *  1. TREE COVERAGE — every process in the spawned tree is sampled, so a fat
 *     descendant breaches the per-process ceiling instead of hiding behind a
 *     thin provider.
 *  2. LOUD ATTRIBUTION — a missing, mis-attributed, or never-sampled provider
 *     FAILS the gate. It is never a silent pass; an unobservable platform is
 *     an explicit, recorded advisory.
 *
 * The last test drives REAL processes (a node child that spawns a grandchild),
 * because a pure-fixture proof cannot show the platform readers work.
 */
import { spawn, type ChildProcess } from "node:child_process";

import { afterAll, describe, expect, it } from "vitest";

import { evaluateRoute, evaluateRouteAdvisories } from "../src/corpus-gate/assertions.js";
import {
  ProcessTreeSampler,
  classifyProviderAttribution,
  descendantPids,
  processImage,
  snapshotProcessTable,
  unsampledTreeMembers,
  type ProcessRow,
} from "../src/corpus-gate/processTree.js";
import { sampleManifestHash } from "../src/corpus-gate/sample.js";
import type {
  CorpusGateThresholds,
  CorpusKindSummary,
  CorpusProviderAttribution,
  CorpusRouteReport,
} from "../src/corpus-gate/types.js";

const THRESHOLDS: CorpusGateThresholds = {
  hoverP95Ms: 300,
  definitionP95Ms: 500,
  completionP95Ms: 500,
  referencesP95Ms: 800,
  rssMaxBytes: 4 * 1024 * 1024 * 1024,
  allowedEmptyCategories: ["classToken"],
};

/**
 * A tree shaped like the real tsserver route:
 *   1 init → 100 verter-lsp → 200 node (provider) → 300 node (the fat child)
 * plus 900, an unrelated process that is NOT in the tree.
 */
const ROWS: readonly ProcessRow[] = [
  { pid: 1, ppid: 0, image: "init" },
  { pid: 100, ppid: 1, image: "verter-lsp" },
  { pid: 200, ppid: 100, image: "node" },
  { pid: 300, ppid: 200, image: "node" },
  { pid: 400, ppid: 100, image: "tsgo" },
  { pid: 900, ppid: 1, image: "node" },
  { pid: 500, ppid: 600, image: "relay-child" },
  { pid: 600, ppid: 1, image: "verter-relay-shim" },
  // The harness itself is a live `node` process in the real table.
  { pid: 999, ppid: 1, image: "node" },
];

function kindSummary(overrides: Partial<CorpusKindSummary> = {}): CorpusKindSummary {
  return {
    count: 4,
    p50Ms: 20,
    p90Ms: 40,
    p95Ms: 50,
    maxMs: 60,
    over2500Count: 0,
    timeoutCount: 0,
    emptyCount: 0,
    unexpectedEmptyCount: 0,
    errorCount: 0,
    ...overrides,
  };
}

function reportWith(
  attribution: CorpusProviderAttribution | undefined,
  overrides: Partial<CorpusRouteReport> = {},
): CorpusRouteReport {
  const sample = ["src/components/BaseButton.vue"];
  return {
    route: "tsserver",
    completed: true,
    wedged: false,
    wedgeDetail: null,
    fatalError: null,
    startup: {
      initializeMs: 10,
      readyObserved: true,
      syncObserved: true,
      quiesced: true,
      settleMs: 10,
    },
    accounting: {
      requestsSent: 16,
      requestsAnswered: 16,
      requestsEmpty: 0,
      requestsTimedOut: 0,
      requestsErrored: 0,
      requestsAbandoned: 0,
      filesOpened: 1,
      filesSkipped: 0,
      probesMined: 10,
    },
    kinds: {
      hover: kindSummary(),
      definition: kindSummary(),
      completion: kindSummary(),
      references: kindSummary(),
    },
    memory: [
      {
        label: "verter-lsp",
        pid: 100,
        supported: true,
        sampleCount: 5,
        firstRssBytes: 100_000_000,
        lastRssBytes: 120_000_000,
        maxRssBytes: 130_000_000,
        samples: [],
        role: "server",
        parentPid: 1,
        image: "verter-lsp",
      },
    ],
    ...(attribution === undefined ? {} : { providerAttribution: attribution }),
    earlyStop: { enabled: false, stopped: false, reason: null },
    isolation: {
      topology: "serial",
      executor: "dedicated",
      mode: "isolated",
      observedConcurrentRoutes: 1,
      latencyGating: true,
      attestationContradicted: false,
      evidence: "sole route session",
    },
    liveness: { checks: 1, failures: 0 },
    wallClock: { budgetMs: 60_000, elapsedMs: 1_000, budgetExceeded: false },
    sampleManifestHash: sampleManifestHash(sample),
    ...overrides,
  };
}

const VERIFIED: CorpusProviderAttribution = {
  status: "verified",
  providerPid: 200,
  detail: "provider pid 200 (node) is a descendant of the spawned tree",
  unattributedPids: [],
  sampledProcessCount: 3,
};

describe("process-tree walking (pure)", () => {
  it("finds transitive descendants and excludes the root and unrelated processes", () => {
    expect(descendantPids(ROWS, 100).sort((a, b) => a - b)).toEqual([200, 300, 400]);
    expect(descendantPids(ROWS, 200)).toEqual([300]);
    expect(descendantPids(ROWS, 300)).toEqual([]);
    expect(descendantPids(ROWS, 100)).not.toContain(900);
    expect(descendantPids(ROWS, 100)).not.toContain(100);
  });

  it("survives a self-parented row and a parent cycle without spinning", () => {
    const cyclic: ProcessRow[] = [
      { pid: 10, ppid: 10, image: "self" },
      { pid: 20, ppid: 21, image: "a" },
      { pid: 21, ppid: 20, image: "b" },
    ];
    expect(descendantPids(cyclic, 10)).toEqual([]);
    expect(descendantPids(cyclic, 20)).toEqual([21]);
  });

  it("reads image names and reports absent pids as null", () => {
    expect(processImage(ROWS, 300)).toBe("node");
    expect(processImage(ROWS, 12_345)).toBeNull();
  });
});

describe("provider attribution classification (pure)", () => {
  const base = { serverPid: 100, relayPid: null, providerPid: 200, rows: ROWS, harnessPid: 999 };

  it("verifies a provider that is a real descendant of the spawned server", () => {
    const result = classifyProviderAttribution(base);
    expect(result.status).toBe("verified");
    expect(result.image).toBe("node");
  });

  it("verifies a provider owned by the relay the harness spawned (shared route)", () => {
    const result = classifyProviderAttribution({
      ...base,
      providerPid: 500,
      relayPid: 600,
    });
    expect(result.status).toBe("verified");
  });

  it("reports MISSING when no provider was ever advertised", () => {
    const result = classifyProviderAttribution({ ...base, providerPid: null });
    expect(result.status).toBe("missing");
    expect(result.detail).toContain("no provider process");
  });

  it("reports MISSING when the advertised pid is absent from the process table", () => {
    const result = classifyProviderAttribution({ ...base, providerPid: 4_242 });
    expect(result.status).toBe("missing");
    expect(result.detail).toContain("absent from the process table");
  });

  it("reports MISMATCHED when the sampled pid is the server itself", () => {
    const result = classifyProviderAttribution({ ...base, providerPid: 100 });
    expect(result.status).toBe("mismatched");
    expect(result.detail).toContain("verter-lsp process itself");
  });

  it("reports MISMATCHED when the sampled pid is the harness itself", () => {
    const result = classifyProviderAttribution({ ...base, providerPid: 999, harnessPid: 999 });
    expect(result.status).toBe("mismatched");
    expect(result.detail).toContain("HARNESS process itself");
  });

  it("reports MISMATCHED for a same-named process outside the spawned tree", () => {
    // pid 900 is a `node` process — identical image to the real provider, so a
    // name-matching check would pass it. Parentage is what discriminates.
    const result = classifyProviderAttribution({ ...base, providerPid: 900 });
    expect(result.status).toBe("mismatched");
    expect(result.detail).toContain("not a descendant");
  });

  it("reports UNOBSERVABLE when the platform cannot enumerate processes", () => {
    const result = classifyProviderAttribution({ ...base, rows: null });
    expect(result.status).toBe("unobservable");
    expect(result.detail).toContain("UNENFORCED");
  });
});

describe("unsampled tree members (pure)", () => {
  const now = 1_000_000;
  it("reports a member that never sampled while others sampled fine", () => {
    expect(
      unsampledTreeMembers(
        [
          { pid: 100, discoveredAtMs: now - 60_000, maxRssBytes: 5 },
          { pid: 300, discoveredAtMs: now - 60_000, maxRssBytes: null },
        ],
        now,
        5_000,
      ),
    ).toEqual([300]);
  });

  it("does not accuse a member discovered inside the grace window", () => {
    expect(
      unsampledTreeMembers(
        [
          { pid: 100, discoveredAtMs: now - 60_000, maxRssBytes: 5 },
          { pid: 300, discoveredAtMs: now - 100, maxRssBytes: null },
        ],
        now,
        5_000,
      ),
    ).toEqual([]);
  });

  it("stays silent when NOTHING sampled (that is platform unobservability, not a gap)", () => {
    expect(
      unsampledTreeMembers(
        [
          { pid: 100, discoveredAtMs: now - 60_000, maxRssBytes: null },
          { pid: 300, discoveredAtMs: now - 60_000, maxRssBytes: null },
        ],
        now,
        5_000,
      ),
    ).toEqual([]);
  });
});

describe("attribution is enforced by the acceptance bar", () => {
  it("passes a verified attribution", () => {
    expect(evaluateRoute(reportWith(VERIFIED), THRESHOLDS)).toEqual([]);
  });

  it("FAILS loudly when the attribution record is missing entirely", () => {
    const failures = evaluateRoute(reportWith(undefined), THRESHOLDS);
    expect(failures).toContainEqual(
      expect.stringContaining("provider attribution was never recorded"),
    );
  });

  it("FAILS loudly on a missing provider process", () => {
    const failures = evaluateRoute(
      reportWith({ ...VERIFIED, status: "missing", detail: "no provider advertised" }),
      THRESHOLDS,
    );
    expect(failures).toContainEqual(expect.stringContaining("provider attribution MISSING"));
  });

  it("FAILS loudly on a mis-attributed provider process", () => {
    const failures = evaluateRoute(
      reportWith({
        ...VERIFIED,
        status: "mismatched",
        detail: "pid 900 is not a descendant of the spawned server",
      }),
      THRESHOLDS,
    );
    expect(failures).toContainEqual(expect.stringContaining("provider attribution MISMATCHED"));
  });

  it("FAILS loudly when a tree member was discovered but never sampled", () => {
    const failures = evaluateRoute(
      reportWith({ ...VERIFIED, unattributedPids: [300, 301] }),
      THRESHOLDS,
    );
    expect(failures).toContainEqual(expect.stringContaining("never sampled"));
    expect(failures.some((failure) => failure.includes("unbounded"))).toBe(true);
  });

  it("records platform unobservability as an explicit ADVISORY, never a silent pass", () => {
    const unobservable = reportWith({
      ...VERIFIED,
      status: "unobservable",
      detail: "this platform could not enumerate the process table",
    });
    expect(evaluateRoute(unobservable, THRESHOLDS)).toEqual([]);
    const advisories = evaluateRouteAdvisories(unobservable, THRESHOLDS);
    expect(advisories).toContainEqual(
      expect.stringContaining("ADVISORY [tsserver] provider attribution unobservable"),
    );
  });

  it("bounds a FAT DESCENDANT the old single-pid sampler could not see", () => {
    // The observed defect, reproduced: the advertised provider is thin (357 MB)
    // while its own child holds 3.9 GB. Tree coverage puts the child under the
    // per-process ceiling, so the run FAILS instead of passing blind.
    const withFatChild = reportWith(VERIFIED, {
      memory: [
        {
          label: "provider",
          pid: 200,
          supported: true,
          sampleCount: 9,
          firstRssBytes: 300_000_000,
          lastRssBytes: 357_000_000,
          maxRssBytes: 357_000_000,
          samples: [],
          role: "provider",
          parentPid: 100,
          image: "node",
        },
        {
          label: "provider-child",
          pid: 300,
          supported: true,
          sampleCount: 9,
          firstRssBytes: 500_000_000,
          lastRssBytes: 3_900_000_000,
          maxRssBytes: 4_400_000_000,
          samples: [],
          role: "provider",
          parentPid: 200,
          image: "node",
        },
      ],
    });
    const failures = evaluateRoute(withFatChild, THRESHOLDS);
    expect(failures).toContainEqual(expect.stringContaining("provider-child max RSS 4400000000"));

    // Discrimination: with ONLY the thin provider recorded — the pre-fix shape —
    // nothing breaches, which is exactly the blind pass this fix removes.
    const thinOnly = reportWith(VERIFIED, {
      memory: [
        {
          label: "provider",
          pid: 200,
          supported: true,
          sampleCount: 9,
          firstRssBytes: 300_000_000,
          lastRssBytes: 357_000_000,
          maxRssBytes: 357_000_000,
          samples: [],
          role: "provider",
          parentPid: 100,
          image: "node",
        },
      ],
    });
    expect(evaluateRoute(thinOnly, THRESHOLDS)).toEqual([]);
  });
});

describe("ProcessTreeSampler against REAL processes", () => {
  const spawned: ChildProcess[] = [];
  afterAll(() => {
    for (const child of spawned) {
      try {
        child.kill();
      } catch {
        // Already gone.
      }
    }
  });

  /**
   * A THREE-level tree — server → provider → provider's own child — mirroring
   * the real shape: the advertised provider is thin, its child holds the
   * memory. Both inner pids are reported on the inherited stdout.
   */
  async function spawnThreeLevelTree(): Promise<{
    parentPid: number;
    childPid: number;
    grandchildPid: number;
  }> {
    const grandchildScript =
      "const buffers = []; for (let i = 0; i < 8; i += 1) buffers.push(Buffer.alloc(1024 * 1024, 1)); " +
      "setTimeout(() => process.exit(0), 15000);";
    const childScript =
      "const { spawn } = require('node:child_process'); " +
      `const grandchild = spawn(process.execPath, ['-e', ${JSON.stringify(grandchildScript)}], { stdio: 'ignore' }); ` +
      "process.stdout.write('G:' + grandchild.pid + '\\n'); " +
      "setTimeout(() => process.exit(0), 15000);";
    const parentScript =
      "const { spawn } = require('node:child_process'); " +
      `const child = spawn(process.execPath, ['-e', ${JSON.stringify(childScript)}], { stdio: ['ignore', 'inherit', 'ignore'] }); ` +
      "process.stdout.write('C:' + child.pid + '\\n'); " +
      "setTimeout(() => process.exit(0), 15000);";
    const parent = spawn(process.execPath, ["-e", parentScript], {
      stdio: ["ignore", "pipe", "ignore"],
    });
    spawned.push(parent);
    const pids = await new Promise<{ childPid: number; grandchildPid: number }>(
      (resolve, reject) => {
        let buffer = "";
        const timer = setTimeout(() => reject(new Error("tree pids never arrived")), 20_000);
        parent.stdout?.on("data", (chunk: Buffer) => {
          buffer += chunk.toString("utf8");
          const child = /C:(\d+)/.exec(buffer);
          const grandchild = /G:(\d+)/.exec(buffer);
          if (child && grandchild) {
            clearTimeout(timer);
            resolve({ childPid: Number(child[1]), grandchildPid: Number(grandchild[1]) });
          }
        });
      },
    );
    return { parentPid: parent.pid as number, ...pids };
  }

  it(
    "samples the provider's OWN CHILD, which a single-pid sampler never sees",
    { timeout: 120_000 },
    async () => {
      const { parentPid, childPid, grandchildPid } = await spawnThreeLevelTree();
      const table = await snapshotProcessTable();
      if (table === null) {
        // Honest, explicit: this platform cannot enumerate processes, so the
        // gate must SAY the ceiling is unenforced rather than pass blind.
        const sampler = new ProcessTreeSampler(1_000);
        sampler.setRoots({ serverPid: parentPid, relayPid: null, providerPid: childPid });
        await sampler.refreshTopology();
        sampler.stop();
        expect(sampler.attribution().status).toBe("unobservable");
        return;
      }

      const sampler = new ProcessTreeSampler(1_000);
      // Exactly the production wiring: the server advertises ONE provider pid.
      sampler.setRoots({ serverPid: parentPid, relayPid: null, providerPid: childPid });
      sampler.start();
      await new Promise((resolve) => setTimeout(resolve, 4_000));
      await sampler.refreshTopology();
      sampler.stop();

      const trends = sampler.trends();
      const pids = trends.map((trend) => trend.pid);
      expect(pids).toContain(parentPid);
      expect(pids).toContain(childPid);
      expect(pids, "the provider's own child must be sampled — that is the whole fix").toContain(
        grandchildPid,
      );
      expect(trends.find((trend) => trend.pid === parentPid)?.role).toBe("server");
      expect(trends.find((trend) => trend.pid === childPid)?.role).toBe("provider");
      // The fat child counts as provider memory, not as an untracked stranger.
      expect(trends.find((trend) => trend.pid === grandchildPid)?.role).toBe("provider");

      const attribution = sampler.attribution();
      expect(attribution.status).toBe("verified");
      expect(attribution.providerPid).toBe(childPid);
      expect(attribution.unattributedPids).toEqual([]);
      expect(attribution.sampledProcessCount).toBeGreaterThanOrEqual(3);
      // RSS actually read on this platform (the sampler is not a no-op).
      expect(sampler.maxObservedRssBytes()).toBeGreaterThan(0);
    },
  );

  it(
    "reports MISMATCHED when pointed at a live process outside the spawned tree",
    { timeout: 120_000 },
    async () => {
      const { parentPid } = await spawnThreeLevelTree();
      const table = await snapshotProcessTable();
      const sampler = new ProcessTreeSampler(1_000);
      // The harness process is alive and real — but it is not the provider.
      sampler.setRoots({ serverPid: parentPid, relayPid: null, providerPid: process.pid });
      await sampler.refreshTopology();
      sampler.stop();
      expect(sampler.attribution().status).toBe(table === null ? "unobservable" : "mismatched");
    },
  );
});
