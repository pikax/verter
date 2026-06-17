import { describe, expect, it } from "vitest";

// There is ONE startup-gate engine. The in-host wait loop consumes the harness fold
// as an injected callback; these tests inject the SAME authoritative harness parser
// the in-host runner loads at runtime (here imported from source to stay clean-
// checkout hermetic), so the live wait path is exercised against the real engine —
// not a re-implementation.
import { evaluateGenerationGate } from "../../../dx-harness/src/core/generationGate";
import {
  parseExtensionStartupLog,
  parseStartupLogLine,
} from "../../../dx-harness/src/core/extensionStartup";
import { waitForDxReadiness } from "./dxReadiness";

// The exact strings the extension logs (packages/vue-vscode/src/extension.ts:864,869),
// with the level/timestamp prefix the output channel prepends.
const ready = (gen: number) =>
  `2026-06-16 10:00:00.001 [info] Verter ready (init generation ${gen})`;
const sync = (gen: number) =>
  `2026-06-16 10:00:00.002 [info] TypeProviderSyncComplete (init generation ${gen})`;

/** A clock that advances a fixed step per read, so the timeout is deterministic. */
function fakeClock(stepMs: number): () => number {
  let t = 0;
  return () => (t += stepMs);
}

/** Read the i-th element, clamping at the last (the log/quiescence keeps that state). */
function indexedReader<T>(states: readonly T[]): () => T {
  let i = 0;
  return () => states[Math.min(i++, states.length - 1)];
}

describe("harness startup-gate fold (the injected engine)", () => {
  it("requires a matching generation: it is NOT the weaker sync >= ready", () => {
    // ready(1) + sync(2): a `sync >= ready` gate would (wrongly) be satisfied (2>=1).
    // The matching-generation gate is satisfied ONLY on equal newest generations.
    expect(parseExtensionStartupLog([ready(1), sync(2)]).satisfied).toBe(false);
    expect(parseExtensionStartupLog([ready(1), sync(1)]).satisfied).toBe(true);
  });

  it("discards a superseded generation (ready(2) re-arms until sync(2))", () => {
    expect(parseExtensionStartupLog([ready(1), sync(1), ready(2)]).satisfied).toBe(false);
    expect(parseExtensionStartupLog([ready(1), sync(1), ready(2), sync(2)]).matchedGeneration).toBe(
      2,
    );
  });

  it("parseStartupLogLine extracts channel + generation, null otherwise", () => {
    expect(parseStartupLogLine(ready(3))).toEqual({ channel: "ready", generation: 3 });
    expect(parseStartupLogLine(sync(7))).toEqual({ channel: "sync", generation: 7 });
    expect(parseStartupLogLine("[info] Verter ready")).toBeNull();
  });
});

describe("waitForDxReadiness (live wait path, real harness fold)", () => {
  it("waits for the gate, then for diagnostics/log quiescence, before resolving", async () => {
    // Gate satisfied from the 2nd read; quiescence churns once, then settles.
    const readLog = indexedReader([
      ready(1),
      `${ready(1)}\n${sync(1)}`,
      `${ready(1)}\n${sync(1)}`,
      `${ready(1)}\n${sync(1)}`,
    ]);
    const sampleQuiescence = indexedReader([
      { diagnosticsCount: 2, logLength: 10 }, // first satisfied sample
      { diagnosticsCount: 1, logLength: 12 }, // churn → resets the stable run
      { diagnosticsCount: 1, logLength: 12 }, // stable
      { diagnosticsCount: 1, logLength: 12 }, // stable → quiesced
    ]);

    const result = await waitForDxReadiness({
      readLog,
      evaluateLog: parseExtensionStartupLog,
      sampleQuiescence,
      sleep: async () => {},
      now: fakeClock(10),
      timeoutMs: 100_000,
      intervalMs: 5,
      requiredStableSamples: 2,
    });
    expect(result.matchedGeneration).toBe(1);
  });

  it("does NOT resolve on a superseded generation appended during quiescence", async () => {
    // ready(1)+sync(1) matches (gen 1). Before quiescence completes, ready(2) is
    // appended — the live loop must re-read the FULL log, see the gate is no longer
    // satisfied for the newest generation, re-arm, and only resolve once sync(2)
    // arrives. A naive "cache the first match" implementation would resolve gen 1.
    const readLog = indexedReader([
      `${ready(1)}\n${sync(1)}`, // satisfied gen 1
      `${ready(1)}\n${sync(1)}\n${ready(2)}`, // ready(2) supersedes → unsatisfied
      `${ready(1)}\n${sync(1)}\n${ready(2)}`, // still unsatisfied
      `${ready(1)}\n${sync(1)}\n${ready(2)}\n${sync(2)}`, // satisfied gen 2
      `${ready(1)}\n${sync(1)}\n${ready(2)}\n${sync(2)}`, // stable
      `${ready(1)}\n${sync(1)}\n${ready(2)}\n${sync(2)}`, // stable → quiesced
    ]);
    // Constant quiescence: any resolution is driven purely by generation matching.
    const sampleQuiescence = () => ({ diagnosticsCount: 0, logLength: 5 });

    const result = await waitForDxReadiness({
      readLog,
      evaluateLog: parseExtensionStartupLog,
      sampleQuiescence,
      sleep: async () => {},
      now: fakeClock(10),
      timeoutMs: 100_000,
      intervalMs: 5,
      requiredStableSamples: 3,
    });
    // Resolves on the NEWEST generation, never the stale one.
    expect(result.matchedGeneration).toBe(2);
    expect(result.matchedGeneration).not.toBe(1);
  });

  it("throws on timeout when the gate is never satisfied", async () => {
    await expect(
      waitForDxReadiness({
        readLog: () => ready(1), // sync never arrives
        evaluateLog: parseExtensionStartupLog,
        sampleQuiescence: () => ({ diagnosticsCount: 0, logLength: 0 }),
        sleep: async () => {},
        now: fakeClock(50),
        timeoutMs: 200,
        intervalMs: 10,
        requiredStableSamples: 2,
      }),
    ).rejects.toThrow(/readiness|generation|timed out/i);
  });
});

describe("the injected fold matches the shared generation engine (contract)", () => {
  // parseExtensionStartupLog (lines) must agree with evaluateGenerationGate (events)
  // — they are the one engine, reached via two entrances.
  const cases: string[][] = [
    [],
    [ready(1)],
    [sync(1)],
    [ready(1), sync(1)],
    [ready(1), sync(2)],
    [ready(1), sync(1), ready(2)],
    [ready(1), sync(1), ready(2), sync(2)],
    [sync(2), ready(1), sync(1), ready(2)],
    [ready(2), sync(1)],
    [ready(5), sync(3), sync(5), ready(4)],
  ];

  it("agrees on satisfied + matchedGeneration across entrances", () => {
    for (const lines of cases) {
      const viaLines = parseExtensionStartupLog(lines);
      const events = lines
        .map(parseStartupLogLine)
        .filter((e): e is NonNullable<typeof e> => e !== null);
      const viaEvents = evaluateGenerationGate(events);
      expect(viaLines.satisfied, `lines=${JSON.stringify(lines)}`).toBe(viaEvents.satisfied);
      expect(viaLines.matchedGeneration, `lines=${JSON.stringify(lines)}`).toBe(
        viaEvents.matchedGeneration,
      );
    }
  });
});
