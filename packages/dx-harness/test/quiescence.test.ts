import { describe, expect, it } from "vitest";

import {
  countersEqual,
  decideQuiescence,
  extractQuiescenceCounters,
  isQuiescenceWarnLine,
  pollUntilQuiesced,
  REQUIRED_STABLE_INTERVALS,
  type QuiescenceCounters,
  type QuiescenceObservation,
} from "../src/core/quiescence.js";

/** Build a `$/verter/getStatistics` snapshot the way the LSP serializes it. */
function statsSnapshot(counts: { compile: number; upsert: number; cacheHits: number }) {
  // Mirrors StatisticsSnapshot { enabled, session: { byType, byFile } } with the
  // host counters keyed exactly as custom_methods/mod.rs:398-433 emits them.
  return {
    enabled: true,
    session: {
      byType: {
        "host:compile": { count: counts.compile, totalMs: 1, minMs: 0, maxMs: 0, averageMs: 0 },
        "host:upsert": { count: counts.upsert, totalMs: 1, minMs: 0, maxMs: 0, averageMs: 0 },
        "host:cache_hits": {
          count: counts.cacheHits,
          totalMs: 0,
          minMs: 0,
          maxMs: 0,
          averageMs: 0,
        },
        "lsp:hover": { count: 999 },
      },
      byFile: { "/ws/A.vue": { count: 123 } },
    },
  };
}

const C = (compile: number, upsert: number, cacheHits: number): QuiescenceCounters => ({
  compile,
  upsert,
  cacheHits,
});

const obs = (
  counters: QuiescenceCounters,
  newWarnLines: string[] = [],
  extra: Partial<QuiescenceObservation> = {},
): QuiescenceObservation => ({ counters, newWarnLines, ...extra });

describe("extractQuiescenceCounters", () => {
  it("reads the three host counters from session.byType[...].count", () => {
    const counters = extractQuiescenceCounters(
      statsSnapshot({ compile: 12, upsert: 7, cacheHits: 4 }),
    );
    expect(counters).toEqual({ compile: 12, upsert: 7, cacheHits: 4 });
  });

  it("defaults missing counters to 0 and never reads byFile or unrelated byType keys", () => {
    const counters = extractQuiescenceCounters({
      enabled: true,
      session: { byType: {}, byFile: {} },
    });
    expect(counters).toEqual({ compile: 0, upsert: 0, cacheHits: 0 });
    // Negative: a totally malformed snapshot must not throw.
    expect(extractQuiescenceCounters(undefined)).toEqual({ compile: 0, upsert: 0, cacheHits: 0 });
    expect(extractQuiescenceCounters({ session: null })).toEqual({
      compile: 0,
      upsert: 0,
      cacheHits: 0,
    });
  });
});

describe("isQuiescenceWarnLine", () => {
  it("matches scanner/drain/sync WARN lines from verter-lsp tracing output", () => {
    expect(
      isQuiescenceWarnLine("2026-06-15 WARN verter_lsp::workspace_scanner: rescan dropped"),
    ).toBe(true);
    expect(isQuiescenceWarnLine(" WARN verter_session::drain: backpressure draining queue")).toBe(
      true,
    );
    expect(isQuiescenceWarnLine("WARN sync coordinator: provider sync retried")).toBe(true);
  });

  it("does NOT match INFO-level or keyword-less lines", () => {
    // Negative: an INFO scanner line is normal progress, not a quiescence blocker.
    expect(isQuiescenceWarnLine("INFO verter_lsp::workspace_scanner: scan complete")).toBe(false);
    expect(isQuiescenceWarnLine("typeProviderSyncComplete sent (gen=3)")).toBe(false);
    // Negative: a WARN with none of the scanner/drain/sync keywords is out of scope.
    expect(isQuiescenceWarnLine("WARN verter_lsp::heartbeat: slow tick")).toBe(false);
  });
});

describe("countersEqual", () => {
  it("compares all three counters", () => {
    expect(countersEqual(C(1, 2, 3), C(1, 2, 3))).toBe(true);
    expect(countersEqual(C(1, 2, 3), C(1, 2, 4))).toBe(false);
    expect(countersEqual(C(1, 2, 3), C(9, 2, 3))).toBe(false);
  });
});

describe("decideQuiescence", () => {
  it("is NOT quiesced while any host counter is still changing", () => {
    const decision = decideQuiescence([obs(C(1, 1, 1)), obs(C(2, 1, 1)), obs(C(3, 1, 1))]);
    expect(decision.quiesced).toBe(false);
    expect(decision.stableIntervals).toBe(0);
    expect(decision.reason).toMatch(/counter/i);
  });

  it("is quiesced after counters hold for TWO consecutive intervals with no new warns", () => {
    const decision = decideQuiescence([obs(C(5, 3, 9)), obs(C(5, 3, 9)), obs(C(5, 3, 9))]);
    expect(decision.quiesced).toBe(true);
    expect(decision.stableIntervals).toBe(2);
    expect(decision.requiredStableIntervals).toBe(REQUIRED_STABLE_INTERVALS);
  });

  it("is NOT quiesced after only ONE stable interval (two samples)", () => {
    // Negative: a single unchanged interval must not be mistaken for quiescence.
    const decision = decideQuiescence([obs(C(5, 3, 9)), obs(C(5, 3, 9))]);
    expect(decision.quiesced).toBe(false);
    expect(decision.stableIntervals).toBe(1);
  });

  it("resets when a new scanner/drain/sync warn line appears inside the window", () => {
    const decision = decideQuiescence([
      obs(C(5, 3, 9)),
      obs(C(5, 3, 9)),
      obs(C(5, 3, 9), ["WARN verter_lsp::workspace_scanner: late rescan"]),
    ]);
    // Counters never moved, but the trailing interval saw a warn → not quiesced.
    expect(decision.quiesced).toBe(false);
    expect(decision.stableIntervals).toBe(0);
    expect(decision.reason).toMatch(/warn/i);
  });

  it("is NOT quiesced on fewer than two samples", () => {
    expect(decideQuiescence([]).quiesced).toBe(false);
    expect(decideQuiescence([obs(C(1, 1, 1))]).quiesced).toBe(false);
  });

  it("diagnostics variant: requires diagnostics stable AND provider queries succeeding", () => {
    // Counters + warns are clean, but a changing diagnostics fingerprint blocks.
    const diagsChanging = decideQuiescence([
      obs(C(5, 3, 9), [], { diagnosticsFingerprint: "a" }),
      obs(C(5, 3, 9), [], { diagnosticsFingerprint: "a" }),
      obs(C(5, 3, 9), [], { diagnosticsFingerprint: "b" }),
    ]);
    expect(diagsChanging.quiesced).toBe(false);
    expect(diagsChanging.reason).toMatch(/diagnostic/i);

    // A failing provider query blocks even with everything else stable.
    const providerFailing = decideQuiescence([
      obs(C(5, 3, 9), [], { diagnosticsFingerprint: "a", providerQueryOk: true }),
      obs(C(5, 3, 9), [], { diagnosticsFingerprint: "a", providerQueryOk: false }),
      obs(C(5, 3, 9), [], { diagnosticsFingerprint: "a", providerQueryOk: false }),
    ]);
    expect(providerFailing.quiesced).toBe(false);
    expect(providerFailing.reason).toMatch(/provider/i);

    // All clean for two intervals → quiesced.
    const clean = decideQuiescence([
      obs(C(5, 3, 9), [], { diagnosticsFingerprint: "a", providerQueryOk: true }),
      obs(C(5, 3, 9), [], { diagnosticsFingerprint: "a", providerQueryOk: true }),
      obs(C(5, 3, 9), [], { diagnosticsFingerprint: "a", providerQueryOk: true }),
    ]);
    expect(clean.quiesced).toBe(true);
  });
});

describe("pollUntilQuiesced (hermetic, injected clock)", () => {
  const immediateSleep = () => Promise.resolve();

  function manualClock(stepMs: number) {
    let t = 0;
    return () => {
      const v = t;
      t += stepMs;
      return v;
    };
  }

  it("polls until the counters hold for the required intervals, then resolves quiesced", async () => {
    const counters = C(10, 4, 2);
    let polls = 0;
    const result = await pollUntilQuiesced(
      async () => {
        polls++;
        return counters;
      },
      () => [],
      { intervalMs: 1, sleep: immediateSleep, now: manualClock(1), timeoutMs: 1000 },
    );
    expect(result.quiesced).toBe(true);
    expect(result.timedOut).toBe(false);
    // Need three equal samples for two stable intervals.
    expect(result.pollCount).toBe(REQUIRED_STABLE_INTERVALS + 1);
    expect(polls).toBe(REQUIRED_STABLE_INTERVALS + 1);
  });

  it("resets the stability run when counters move, then quiesces once they settle", async () => {
    const sequence = [C(1, 0, 0), C(2, 0, 0), C(2, 0, 0), C(2, 0, 0)];
    let i = 0;
    const result = await pollUntilQuiesced(
      async () => sequence[Math.min(i++, sequence.length - 1)],
      () => [],
      { intervalMs: 1, sleep: immediateSleep, now: manualClock(1), timeoutMs: 1000 },
    );
    expect(result.quiesced).toBe(true);
    // First sample (1) then three equal (2) → 4 polls total.
    expect(result.pollCount).toBe(4);
  });

  it("resets when a warn line lands mid-window", async () => {
    const counters = C(10, 4, 2);
    const warnQueue: string[][] = [
      [],
      [],
      ["WARN verter_lsp::workspace_scanner: late rescan"],
      [],
      [],
    ];
    let i = 0;
    const result = await pollUntilQuiesced(
      async () => counters,
      () => warnQueue[Math.min(i++, warnQueue.length - 1)],
      { intervalMs: 1, sleep: immediateSleep, now: manualClock(1), timeoutMs: 1000 },
    );
    expect(result.quiesced).toBe(true);
    // poll0,1 clean (interval1 stable), poll2 carries a warn (interval2 unstable →
    // reset), poll3,4 clean → quiesce at poll4 (index 4 → 5 polls).
    expect(result.pollCount).toBe(5);
  });

  it("times out (not quiesced) when the counters never settle", async () => {
    let n = 0;
    const result = await pollUntilQuiesced(
      async () => C(n++, 0, 0),
      () => [],
      { intervalMs: 10, sleep: immediateSleep, now: manualClock(10), timeoutMs: 35 },
    );
    expect(result.quiesced).toBe(false);
    expect(result.timedOut).toBe(true);
  });
});
