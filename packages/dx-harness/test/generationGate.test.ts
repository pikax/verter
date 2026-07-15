import { describe, expect, it } from "vitest";

import { GenerationGate, evaluateGenerationGate } from "../src/core/generationGate.js";

describe("evaluateGenerationGate — both-channels-at-newest matching", () => {
  it("is NOT satisfied on `ready` alone, only once the matching-gen `sync` arrives", () => {
    // `$/verter/ready` is non-semantic — a probe must not proceed on it before
    // the same-generation `typeProviderSyncComplete`.
    const afterReady = evaluateGenerationGate([{ channel: "ready", generation: 7 }]);
    expect(afterReady.satisfied).toBe(false);
    expect(afterReady.matchedGeneration).toBeNull();
    // Negative: a lone ready must never advertise a matched generation.
    expect(afterReady.maxReadyGeneration).toBe(7);
    expect(afterReady.maxSyncGeneration).toBeNull();

    const afterSync = evaluateGenerationGate([
      { channel: "ready", generation: 7 },
      { channel: "sync", generation: 7 },
    ]);
    expect(afterSync.satisfied).toBe(true);
    expect(afterSync.matchedGeneration).toBe(7);
  });

  it("is order-independent: `sync` arriving before `ready` still matches", () => {
    // The server emits `sync` from a task spawned off the scanner oneshot, so it
    // races `ready` and can land first (background_init.rs:380-470 vs :551-553).
    const decision = evaluateGenerationGate([
      { channel: "sync", generation: 4 },
      { channel: "ready", generation: 4 },
    ]);
    expect(decision.satisfied).toBe(true);
    expect(decision.matchedGeneration).toBe(4);
  });

  it("is NOT satisfied when ready and sync belong to DIFFERENT generations", () => {
    const decision = evaluateGenerationGate([
      { channel: "ready", generation: 5 },
      { channel: "sync", generation: 4 },
    ]);
    expect(decision.satisfied).toBe(false);
    expect(decision.matchedGeneration).toBeNull();
    // Newest seen is 5 (on ready); sync has not caught up.
    expect(decision.newestGeneration).toBe(5);
  });

  it("re-arms on a newer generation: a later `ready(N+1)` supersedes a matched pair at N", () => {
    const reArmed = evaluateGenerationGate([
      { channel: "ready", generation: 3 },
      { channel: "sync", generation: 3 },
      { channel: "ready", generation: 4 },
    ]);
    // Was matched at 3, but ready(4) supersedes — gate must NOT stay satisfied.
    expect(reArmed.satisfied).toBe(false);
    expect(reArmed.matchedGeneration).toBeNull();
    expect(reArmed.newestGeneration).toBe(4);

    const reMatched = evaluateGenerationGate([
      { channel: "ready", generation: 3 },
      { channel: "sync", generation: 3 },
      { channel: "ready", generation: 4 },
      { channel: "sync", generation: 4 },
    ]);
    expect(reMatched.satisfied).toBe(true);
    expect(reMatched.matchedGeneration).toBe(4);
  });

  it("discards a superseded generation: a stale lower-gen event never un-matches a newer pair", () => {
    const decision = evaluateGenerationGate([
      { channel: "ready", generation: 6 },
      { channel: "sync", generation: 6 },
      // late, out-of-order leftovers from the superseded generation 5
      { channel: "sync", generation: 5 },
      { channel: "ready", generation: 5 },
    ]);
    expect(decision.satisfied).toBe(true);
    expect(decision.matchedGeneration).toBe(6);
  });

  it("never satisfies on a lone sync, and ignores malformed generations", () => {
    expect(evaluateGenerationGate([{ channel: "sync", generation: 9 }]).satisfied).toBe(false);
    // Negative: NaN / negative generations are not real init generations.
    const ignored = evaluateGenerationGate([
      { channel: "ready", generation: Number.NaN },
      { channel: "sync", generation: -1 },
    ]);
    expect(ignored.maxReadyGeneration).toBeNull();
    expect(ignored.maxSyncGeneration).toBeNull();
    expect(ignored.satisfied).toBe(false);
  });
});

describe("GenerationGate — incremental observation", () => {
  it("flips satisfied as the matching sync arrives and back off on supersession", () => {
    const gate = new GenerationGate();
    gate.observeReady(2);
    expect(gate.satisfied).toBe(false);
    gate.observeSync(2);
    expect(gate.satisfied).toBe(true);
    expect(gate.matchedGeneration).toBe(2);

    // A superseding init: ready(3) re-arms until sync(3).
    gate.observeReady(3);
    expect(gate.satisfied).toBe(false);
    expect(gate.matchedGeneration).toBeNull();
    gate.observeSync(3);
    expect(gate.matchedGeneration).toBe(3);
  });
});
