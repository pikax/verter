import { describe, expect, it } from "vitest";

import { classifyDefinitionSample } from "../src/collectors/index.js";
import type { CollectorEventKey } from "../src/collectors/index.js";
import { parseSourceMap } from "../src/index.js";
import type { CanonicalDefinitionTarget, ExpectedDefinition } from "../src/index.js";
import { buildSourceMapJson } from "./_sourceMap.js";

const key: CollectorEventKey = {
  scenario: "minimal-member-access",
  editStepIndex: 0,
  driver: "rawLsp",
  provider: "tsgo",
  probe: "definition-on-ident",
  version: 1,
  anchor: "ident",
};

describe("classifyDefinitionSample — symbol identity, NEVER line === 0", () => {
  it("does NOT fail a precise LINE-0 target that matches the expected identity", () => {
    const target: CanonicalDefinitionTarget = {
      uri: "App.vue",
      range: { start: { line: 0, character: 6 }, end: { line: 0, character: 14 } },
    };
    const expected: ExpectedDefinition = { ...target };
    const events = classifyDefinitionSample({ key, verter: [target], expected });
    expect(events.every((e) => e.ok)).toBe(true);
    expect(events.some((e) => e.signal === "definition_parity" && e.ok)).toBe(true);
  });

  it("flags a target in the wrong file/symbol as user-visible wrongTarget", () => {
    const target: CanonicalDefinitionTarget = {
      uri: "Other.vue",
      range: { start: { line: 3, character: 0 }, end: { line: 3, character: 4 } },
    };
    const expected: ExpectedDefinition = {
      uri: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 1, character: 9 } },
    };
    const events = classifyDefinitionSample({ key, verter: [target], expected });
    const fail = events.filter((e) => !e.ok);
    expect(fail).toHaveLength(1);
    expect(fail[0].severity).toBe("userVisible");
    expect((fail[0].data as { class?: string }).class).toBe("wrongTarget");
  });

  it("flags a generated-only target with no mapping back as unmappedGenerated", () => {
    const target: CanonicalDefinitionTarget = {
      uri: "file:///proj/App.vue.tsx",
      range: { start: { line: 9, character: 0 }, end: { line: 9, character: 5 } },
      fromGenerated: true,
    };
    // A map with NO segment covering line 9 → unprojectable.
    const map = parseSourceMap(buildSourceMapJson(["App.vue"], [[]]));
    const events = classifyDefinitionSample({ key, verter: [target], map });
    const fail = events.filter((e) => !e.ok);
    expect(fail.some((e) => (e.data as { class?: string }).class === "unmappedGenerated")).toBe(
      true,
    );
    expect(fail.every((e) => e.severity === "userVisible")).toBe(true);
  });

  it("accepts a generated target whose PROJECTED Vue range matches the expectation", () => {
    const map = parseSourceMap(buildSourceMapJson(["App.vue"], [[], [], [], [], [[8, 0, 1, 6]]]));
    const target: CanonicalDefinitionTarget = {
      uri: "file:///proj/App.vue.tsx",
      range: { start: { line: 4, character: 8 }, end: { line: 4, character: 11 } },
      fromGenerated: true,
    };
    const expected: ExpectedDefinition = {
      uri: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 1, character: 9 } },
    };
    const events = classifyDefinitionSample({ key, verter: [target], expected, map });
    expect(events.every((e) => e.ok)).toBe(true);
  });
});
