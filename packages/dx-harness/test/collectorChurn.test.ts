import { describe, expect, it } from "vitest";

import { classifyChurn, steadyStateCompileDelta } from "../src/collectors/index.js";
import type { ChurnPreconditions, CollectorEventKey } from "../src/collectors/index.js";
import type { QuiescenceCounters } from "../src/index.js";

const key: CollectorEventKey = {
  scenario: "minimal-member-access",
  editStepIndex: 0,
  driver: "rawLsp",
  provider: "tsgo",
  probe: "churn",
  version: 4,
  anchor: "doc",
};

const counters = (compile: number): QuiescenceCounters => ({ compile, upsert: 0, cacheHits: 0 });

const allMet: ChurnPreconditions = {
  syncGenerationMatched: true,
  quiescedBefore: true,
  quiescedAfter: true,
  singleDocumentOpen: true,
  noNewImportsMidMeasurement: true,
};

describe("steadyStateCompileDelta", () => {
  it("is the post-minus-pre compile counter delta", () => {
    expect(steadyStateCompileDelta(counters(10), counters(13))).toBe(3);
  });
});

describe("classifyChurn — steady-state per-quiesced-edit delta", () => {
  it("reports an attributable delta within threshold as healthy steady-state", () => {
    const event = classifyChurn({
      key,
      pre: counters(10),
      post: counters(11),
      preconditions: allMet,
      mode: "steadyStateQuiescedEdit",
      threshold: 2,
    });
    expect(event.ok).toBe(true);
    expect((event.data as { scope?: string }).scope).toBe("steadyStateQuiescedEdit");
    expect((event.data as { delta?: number }).delta).toBe(1);
    expect((event.data as { attributable?: boolean }).attributable).toBe(true);
  });

  it("flags an over-threshold steady-state delta as user-visible", () => {
    const event = classifyChurn({
      key,
      pre: counters(10),
      post: counters(20),
      preconditions: allMet,
      mode: "steadyStateQuiescedEdit",
      threshold: 2,
    });
    expect(event.ok).toBe(false);
    expect(event.severity).toBe("userVisible");
    expect((event.data as { delta?: number }).delta).toBe(10);
  });

  it("a BURST reports the AGGREGATE delta and does NOT claim per-character attribution", () => {
    const event = classifyChurn({
      key,
      pre: counters(10),
      post: counters(40),
      preconditions: allMet,
      mode: "burstAggregate",
      threshold: 2,
    });
    expect((event.data as { scope?: string }).scope).toBe("burstAggregate");
    expect((event.data as { delta?: number }).delta).toBe(30);
    // Honest scope: no per-character attribution is claimed from the global counter.
    expect((event.data as { perCharacterAttribution?: boolean }).perCharacterAttribution).toBe(
      false,
    );
    expect(event.severity).toBe("candidate");
  });

  it("records attribution UNCERTAINTY (not a hard failure) when a precondition is unmet", () => {
    const event = classifyChurn({
      key,
      pre: counters(10),
      post: counters(50),
      preconditions: { ...allMet, noNewImportsMidMeasurement: false },
      mode: "steadyStateQuiescedEdit",
      threshold: 2,
    });
    expect(event.signal).toBe("churn_attribution_uncertain");
    expect(event.ok).toBe(true); // honest: not attributable, so not asserted as a failure
    expect(event.severity).toBe("candidate");
    expect((event.data as { unmet?: string[] }).unmet).toContain("noNewImportsMidMeasurement");
    expect((event.data as { attributable?: boolean }).attributable).toBe(false);
  });
});
