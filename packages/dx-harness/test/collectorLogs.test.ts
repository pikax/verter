import { describe, expect, it } from "vitest";

import { classifyLogs, parseMappingFailure, scanLogLines } from "../src/collectors/index.js";
import type { CollectorEventKey, SemanticFailureKey } from "../src/collectors/index.js";

const key: CollectorEventKey = {
  scenario: "minimal-member-access",
  editStepIndex: 1,
  driver: "rawLsp",
  provider: "tsgo",
  probe: "logs",
  version: 3,
  anchor: "doc",
};

describe("parseMappingFailure — extracts method + uri + position from the mapping-failure string", () => {
  it("parses a completion mapping-failure line", () => {
    const mf = parseMappingFailure("completion: position mapping failed for file:///App.vue:3,5");
    expect(mf).toEqual({
      method: "completion",
      uri: "file:///App.vue",
      line: 3,
      character: 5,
      raw: "completion: position mapping failed for file:///App.vue:3,5",
    });
  });

  it("parses a definition mapping-failure line with a COLON line/char separator", () => {
    // The definition / type_definition / references family emits `:line:char` (colon),
    // unlike completion's `:line,char` (comma) — both must parse.
    const mf = parseMappingFailure("definition: position mapping failed for file:///App.vue:7:12");
    expect(mf).toEqual({
      method: "definition",
      uri: "file:///App.vue",
      line: 7,
      character: 12,
      raw: "definition: position mapping failed for file:///App.vue:7:12",
    });
  });

  it("returns null for a non-mapping line", () => {
    expect(parseMappingFailure("INFO workspace scan complete")).toBeNull();
  });
});

describe("scanLogLines — WARN/ERROR classification", () => {
  it("classifies WARN and ERROR lines and ignores INFO/DEBUG", () => {
    const obs = scanLogLines([
      "INFO ready",
      " WARN  tsgo unavailable",
      "2026-01-01 ERROR sync_coordinator crashed",
      "DEBUG mapping detail",
    ]);
    expect(obs.map((o) => o.level)).toEqual(["warn", "error"]);
  });
});

describe("classifyLogs — mapping strings are hints only when correlated", () => {
  it("does NOT fail on a mapping-string WARN ALONE (no correlated semantic failure)", () => {
    const lines = ["WARN completion: position mapping failed for file:///App.vue:3,5"];
    const events = classifyLogs({ key, lines, semanticFailures: [] });
    expect(events.every((e) => e.ok)).toBe(true);
    expect(events.some((e) => e.signal === "mapping_failure_benign")).toBe(true);
  });

  it("emits a root-cause hint when the mapping failure CORRELATES with a semantic failure at the same (uri,pos,method)", () => {
    const lines = ["completion: position mapping failed for file:///App.vue:3,5"];
    const semanticFailures: SemanticFailureKey[] = [
      { method: "completion", uri: "file:///App.vue", line: 3, character: 5 },
    ];
    const events = classifyLogs({ key, lines, semanticFailures });
    const hint = events.find((e) => e.signal === "mapping_root_cause_hint");
    expect(hint).toBeDefined();
    expect(hint?.ok).toBe(false);
    expect(hint?.severity).toBe("candidate");
  });

  it("emits a hint for a definition `:line:char` mapping failure correlated with a semantic miss", () => {
    const lines = ["definition: position mapping failed for file:///App.vue:7:12"];
    const semanticFailures: SemanticFailureKey[] = [
      { method: "definition", uri: "file:///App.vue", line: 7, character: 12 },
    ];
    const events = classifyLogs({ key, lines, semanticFailures });
    const hint = events.find((e) => e.signal === "mapping_root_cause_hint");
    expect(hint).toBeDefined();
    expect(hint?.ok).toBe(false);
    expect(hint?.severity).toBe("candidate");
  });

  it("does NOT fail on an UNCORRELATED definition `:line:char` mapping failure (benign, never a failure alone)", () => {
    const lines = ["definition: position mapping failed for file:///App.vue:7:12"];
    const events = classifyLogs({ key, lines, semanticFailures: [] });
    expect(events.every((e) => e.ok)).toBe(true);
    expect(events.some((e) => e.signal === "mapping_failure_benign")).toBe(true);
  });

  it("does NOT correlate a mapping failure at a DIFFERENT position", () => {
    const lines = ["completion: position mapping failed for file:///App.vue:3,5"];
    const semanticFailures: SemanticFailureKey[] = [
      { method: "completion", uri: "file:///App.vue", line: 9, character: 9 },
    ];
    const events = classifyLogs({ key, lines, semanticFailures });
    expect(events.every((e) => e.ok)).toBe(true);
  });

  it("flags a non-mapping ERROR line regardless of any semantic failure", () => {
    const events = classifyLogs({
      key,
      lines: ["ERROR sync_coordinator: panic"],
      semanticFailures: [],
    });
    const fail = events.filter((e) => !e.ok);
    expect(fail).toHaveLength(1);
    expect(fail[0].signal).toBe("server_error");
    expect(fail[0].severity).toBe("userVisible");
  });

  it("flags a non-mapping WARN as a candidate", () => {
    const events = classifyLogs({
      key,
      lines: ["WARN tsgo unavailable — verter-only mode"],
      semanticFailures: [],
    });
    const fail = events.filter((e) => !e.ok);
    expect(fail).toHaveLength(1);
    expect(fail[0].signal).toBe("server_warn");
    expect(fail[0].severity).toBe("candidate");
  });
});
