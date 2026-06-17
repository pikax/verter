import { describe, expect, it } from "vitest";

import {
  ExtensionStartupGate,
  parseExtensionStartupLog,
  parseStartupLogLine,
} from "../src/core/extensionStartup.js";

// The exact strings the VS Code extension logs (packages/vue-vscode/src/extension.ts:864,869):
//   log.info(`Verter ready (init generation ${params.gen})`)
//   log.info(`TypeProviderSyncComplete (init generation ${params.gen})`)
// The extension's logger prepends a level/timestamp prefix, so the parser must
// match the substring, not the whole line.
const readyLine = (gen: number) =>
  `2026-06-15 10:00:00.001 [info] Verter ready (init generation ${gen})`;
const syncLine = (gen: number) =>
  `2026-06-15 10:00:00.002 [info] TypeProviderSyncComplete (init generation ${gen})`;

describe("parseStartupLogLine", () => {
  it("extracts the generation and channel from each readiness log line", () => {
    expect(parseStartupLogLine(readyLine(3))).toEqual({ channel: "ready", generation: 3 });
    expect(parseStartupLogLine(syncLine(3))).toEqual({ channel: "sync", generation: 3 });
  });

  it("returns null for unrelated or malformed lines", () => {
    // Negative: a heartbeat / progress line is not a readiness signal.
    expect(parseStartupLogLine("2026-06-15 [trace] $/verter/heartbeat received")).toBeNull();
    // Negative: "Verter ready" without the "(init generation N)" suffix is not a match.
    expect(parseStartupLogLine("[info] Verter ready")).toBeNull();
    expect(parseStartupLogLine("")).toBeNull();
  });
});

describe("parseExtensionStartupLog", () => {
  it("is satisfied only when matching-gen ready AND sync lines are both present", () => {
    const decision = parseExtensionStartupLog([readyLine(3), syncLine(3)]);
    expect(decision.satisfied).toBe(true);
    expect(decision.matchedGeneration).toBe(3);
  });

  it("is NOT satisfied on ready-only lines", () => {
    // Negative (extension host): the `Verter ready` log line alone must not
    // signal readiness.
    const decision = parseExtensionStartupLog([readyLine(3), readyLine(3)]);
    expect(decision.satisfied).toBe(false);
    expect(decision.matchedGeneration).toBeNull();
  });

  it("is NOT satisfied when the generations are mismatched", () => {
    const decision = parseExtensionStartupLog([readyLine(3), syncLine(2)]);
    expect(decision.satisfied).toBe(false);
  });

  it("a newer generation supersedes an earlier matched pair", () => {
    const reArmed = parseExtensionStartupLog([readyLine(3), syncLine(3), readyLine(4)]);
    expect(reArmed.satisfied).toBe(false);
    expect(reArmed.newestGeneration).toBe(4);

    const reMatched = parseExtensionStartupLog([
      readyLine(3),
      syncLine(3),
      readyLine(4),
      syncLine(4),
    ]);
    expect(reMatched.satisfied).toBe(true);
    expect(reMatched.matchedGeneration).toBe(4);
  });

  it("ignores interleaved noise lines", () => {
    const decision = parseExtensionStartupLog([
      "[info] activating Verter…",
      readyLine(5),
      "[trace] $/verter/heartbeat received",
      syncLine(5),
      "[info] re-requesting diagnostics",
    ]);
    expect(decision.satisfied).toBe(true);
    expect(decision.matchedGeneration).toBe(5);
  });
});

describe("ExtensionStartupGate — streaming", () => {
  it("flips satisfied as lines arrive, returning the parsed event per line", () => {
    const gate = new ExtensionStartupGate();
    expect(gate.observeLine("[info] booting")).toBeNull();
    expect(gate.satisfied).toBe(false);

    expect(gate.observeLine(readyLine(1))).toEqual({ channel: "ready", generation: 1 });
    expect(gate.satisfied).toBe(false);

    expect(gate.observeLine(syncLine(1))).toEqual({ channel: "sync", generation: 1 });
    expect(gate.satisfied).toBe(true);
    expect(gate.matchedGeneration).toBe(1);
  });
});
