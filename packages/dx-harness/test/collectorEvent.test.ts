import { describe, expect, it } from "vitest";

import {
  CollectingSink,
  SEVERITIES,
  SEVERITY_RANK,
  atLeastAsSevere,
  collectorEvent,
  serializeCollectorEvent,
  toJsonl,
  type CollectorEvent,
  type CollectorEventKey,
} from "../src/collectors/index.js";

const key: CollectorEventKey = {
  scenario: "minimal-member-access",
  editStepIndex: 2,
  driver: "rawLsp",
  provider: "tsgo",
  probe: "completion-after-dot",
  version: 4,
  anchor: "cursor",
};

describe("severity ladder — descriptive names, crash/user-visible/candidate ordering", () => {
  it("ranks critical most severe, candidate least, with a total order", () => {
    expect(SEVERITIES).toEqual(["critical", "userVisible", "candidate"]);
    expect(SEVERITY_RANK.critical).toBeLessThan(SEVERITY_RANK.userVisible);
    expect(SEVERITY_RANK.userVisible).toBeLessThan(SEVERITY_RANK.candidate);
  });

  it("atLeastAsSevere is reflexive and respects the ladder", () => {
    expect(atLeastAsSevere("critical", "userVisible")).toBe(true);
    expect(atLeastAsSevere("userVisible", "userVisible")).toBe(true);
    expect(atLeastAsSevere("candidate", "userVisible")).toBe(false);
  });
});

describe("collectorEvent builder", () => {
  it("carries the full probe key, severity, provenance, and ok flag", () => {
    const event = collectorEvent({
      collector: "completion",
      signal: "no_suggestions_collapse",
      ok: false,
      severity: "candidate",
      provenance: {
        detectedBy: "rawLsp",
        confirmedBy: "extensionHost",
        escalatesTo: "userVisible",
      },
      key,
      detail: "verter returned no completions mid-typing",
    });
    expect(event.collector).toBe("completion");
    expect(event.ok).toBe(false);
    expect(event.severity).toBe("candidate");
    expect(event.provenance.detectedBy).toBe("rawLsp");
    expect(event.provenance.escalatesTo).toBe("userVisible");
    expect(event.key).toEqual(key);
  });

  it("omits an undefined data payload rather than serializing `data: undefined`", () => {
    const event = collectorEvent({
      collector: "latency",
      signal: "latency_summary",
      ok: true,
      severity: "userVisible",
      provenance: { detectedBy: "rawLsp" },
      key,
      detail: "ok",
    });
    expect("data" in event).toBe(false);
  });
});

describe("CollectingSink + JSONL serialization", () => {
  it("collects events and exposes failures only", () => {
    const sink = new CollectingSink();
    sink.emit(
      collectorEvent({
        collector: "completion",
        signal: "no_suggestions_collapse",
        ok: true,
        severity: "candidate",
        provenance: { detectedBy: "rawLsp" },
        key,
        detail: "populated",
      }),
    );
    sink.emit(
      collectorEvent({
        collector: "completion",
        signal: "no_suggestions_collapse",
        ok: false,
        severity: "candidate",
        provenance: { detectedBy: "rawLsp" },
        key: { ...key, version: 5 },
        detail: "collapsed",
      }),
    );
    expect(sink.events).toHaveLength(2);
    expect(sink.failures).toHaveLength(1);
    expect(sink.failures[0].detail).toBe("collapsed");
  });

  it("serializes one event per line, round-trippable through JSON.parse", () => {
    const event = collectorEvent({
      collector: "diagnostics",
      signal: "diagnostics_default_range",
      ok: false,
      severity: "userVisible",
      provenance: { detectedBy: "rawLsp" },
      key,
      detail: "collapsed to (0,0)",
      data: { range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } } },
    });
    const line = serializeCollectorEvent(event);
    expect(line).not.toContain("\n");
    const parsed = JSON.parse(line) as CollectorEvent;
    expect(parsed.signal).toBe("diagnostics_default_range");
    expect(parsed.key.scenario).toBe("minimal-member-access");
    expect((parsed.data as { range: unknown }).range).toBeDefined();
  });

  it("toJsonl joins events with newlines, one per record", () => {
    const a = collectorEvent({
      collector: "logs",
      signal: "server_warn",
      ok: false,
      severity: "userVisible",
      provenance: { detectedBy: "rawLsp" },
      key,
      detail: "warn",
    });
    const jsonl = toJsonl([a, a]);
    expect(jsonl.split("\n")).toHaveLength(2);
    for (const line of jsonl.split("\n")) expect(() => JSON.parse(line)).not.toThrow();
  });
});

describe("CollectorSignal — closed signal taxonomy", () => {
  it("rejects an unregistered signal literal at the type level", () => {
    const event = collectorEvent({
      collector: "diagnostics",
      // @ts-expect-error — an unregistered string is NOT a member of the closed
      // CollectorSignal union, so the builder rejects it at compile time. (With
      // `signal` typed as a bare `string` this directive would be unused → TS2578.)
      signal: "not_a_real_signal",
      ok: false,
      severity: "userVisible",
      provenance: { detectedBy: "rawLsp" },
      key,
      detail: "an unregistered signal must not typecheck",
    });
    // The runtime payload is untouched (the taxonomy is a purely compile-time gate);
    // asserting keeps the constructed event live rather than dead-code-eliminated.
    expect(event.collector).toBe("diagnostics");
    expect(event.ok).toBe(false);
  });

  it("accepts a registered CollectorSignal literal", () => {
    const event = collectorEvent({
      collector: "autoImport",
      signal: "auto_import_not_introduced",
      ok: false,
      severity: "userVisible",
      provenance: { detectedBy: "rawLsp" },
      key,
      detail: "a registered signal typechecks",
    });
    expect(event.signal).toBe("auto_import_not_introduced");
  });
});
