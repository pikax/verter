import { describe, expect, it } from "vitest";

import { compareDefinition, parseSourceMap } from "../src/differential/index.js";
import type {
  CanonicalDefinitionTarget,
  ExpectedDefinition,
  NormalizedLocation,
} from "../src/index.js";
import { buildSourceMapJson } from "./_sourceMap.js";

// `compareDefinition` projects generated targets back to authored Vue space through the source map
// alone (`map`); the baseline path converts its own byte offsets via the per-path texts it is given.

describe("compareDefinition — symbol identity by file+range, NEVER by line === 0", () => {
  it("a precise LINE-0 target that matches the expected identity -> agreement", () => {
    const target: CanonicalDefinitionTarget = {
      uri: "App.vue",
      range: { start: { line: 0, character: 6 }, end: { line: 0, character: 14 } },
    };
    const expected: ExpectedDefinition = {
      uri: "App.vue",
      range: { start: { line: 0, character: 6 }, end: { line: 0, character: 14 } },
    };
    expect(compareDefinition([target], { expected })).toEqual([]);

    // Regression guard: a NAIVE "a line-0 target is invalid" predicate would
    // wrongly reject this exact match — proving the comparator is identity-based.
    const naiveRejectsLineZero = [target].every((t) => t.range.start.line !== 0);
    expect(naiveRejectsLineZero).toBe(false);
  });

  it("a generated target whose PROJECTED Vue range matches the expected identity -> agreement", () => {
    // generated line 4 col 8 -> App.vue line 1 col 6 (a 1:1-copied identifier run).
    const map = parseSourceMap(buildSourceMapJson(["App.vue"], [[], [], [], [], [[8, 0, 1, 6]]]));
    const generated: CanonicalDefinitionTarget = {
      uri: "file:///proj/App.vue.tsx",
      range: { start: { line: 4, character: 8 }, end: { line: 4, character: 11 } },
      fromGenerated: true,
    };
    const expected: ExpectedDefinition = {
      uri: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 1, character: 9 } },
    };
    expect(compareDefinition([generated], { expected, map })).toEqual([]);
  });

  it("a target in the wrong file/symbol -> wrongTarget", () => {
    const target: CanonicalDefinitionTarget = {
      uri: "Other.vue",
      range: { start: { line: 3, character: 0 }, end: { line: 3, character: 4 } },
    };
    const expected: ExpectedDefinition = {
      uri: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 1, character: 9 } },
    };
    const out = compareDefinition([target], { expected });
    expect(out.map((d) => d.class)).toEqual(["wrongTarget"]);
  });
});

describe("compareDefinition — generated-only-unmapped is its own class, not a crash", () => {
  it("only generated targets and no source map -> unmappedGenerated (no throw)", () => {
    const generated: CanonicalDefinitionTarget = {
      uri: "file:///proj/App.vue.tsx",
      range: { start: { line: 12, character: 2 }, end: { line: 12, character: 8 } },
      fromGenerated: true,
    };
    let out: ReturnType<typeof compareDefinition> = [];
    expect(() => {
      out = compareDefinition([generated], { expected: { uri: "App.vue" } });
    }).not.toThrow();
    expect(out.map((d) => d.class)).toContain("unmappedGenerated");
  });
});

describe("compareDefinition — verter vs baseline location parity (authored space)", () => {
  // models.ts: line 2 ("hello") starts at byte 4; cols 0..5 are bytes 4..9.
  const text = "a\nb\nhello\n";

  it("verter empty where baseline resolved a target -> baselineOnly", () => {
    const locations: NormalizedLocation[] = [{ path: "models.ts", start: 4, end: 9 }];
    const out = compareDefinition([], { baseline: { locations, texts: { "models.ts": text } } });
    expect(out.map((d) => d.class)).toEqual(["baselineOnly"]);
  });

  it("verter and baseline on the same authored file+range -> agreement", () => {
    const verter: CanonicalDefinitionTarget[] = [
      {
        uri: "models.ts",
        range: { start: { line: 2, character: 0 }, end: { line: 2, character: 5 } },
      },
    ];
    const locations: NormalizedLocation[] = [{ path: "models.ts", start: 4, end: 9 }];
    expect(
      compareDefinition(verter, { baseline: { locations, texts: { "models.ts": text } } }),
    ).toEqual([]);
  });

  it("verter and baseline on the same file but a different range -> rangeMismatch", () => {
    const verter: CanonicalDefinitionTarget[] = [
      {
        uri: "models.ts",
        range: { start: { line: 2, character: 0 }, end: { line: 2, character: 5 } },
      },
    ];
    const locations: NormalizedLocation[] = [{ path: "models.ts", start: 4, end: 7 }];
    const out = compareDefinition(verter, {
      baseline: { locations, texts: { "models.ts": text } },
    });
    expect(out.map((d) => d.class)).toEqual(["rangeMismatch"]);
  });

  it("verter and baseline on different files -> verterOnly AND baselineOnly", () => {
    const verter: CanonicalDefinitionTarget[] = [
      {
        uri: "models.ts",
        range: { start: { line: 2, character: 0 }, end: { line: 2, character: 5 } },
      },
    ];
    const locations: NormalizedLocation[] = [{ path: "other.ts", start: 0, end: 3 }];
    const out = compareDefinition(verter, {
      baseline: { locations, texts: { "models.ts": text, "other.ts": "xyz\n" } },
    });
    expect(out.map((d) => d.class).sort()).toEqual(["baselineOnly", "verterOnly"]);
  });

  it("a baseline location with no supplied text is surfaced, never silently dropped", () => {
    // verter empty; the baseline resolved a target, but its file text is absent. The
    // location must be surfaced (a conservative baselineOnly), not dropped into a false
    // agreement.
    const locations: NormalizedLocation[] = [{ path: "models.ts", start: 4, end: 9 }];
    const out = compareDefinition([], { baseline: { locations, texts: {} } });
    expect(out.map((d) => d.class)).toEqual(["baselineOnly"]);
  });
});

describe("compareDefinition — a baseline GENERATED target with text but no mapping back is surfaced", () => {
  // `const x = y;` — `y` is bytes 10..11 -> generated (0,10)..(0,11) over the artifact.
  const generatedText = "const x = y;\n";

  it("does not silently drop an unprojectable generated baseline location into a false agreement", () => {
    // The baseline resolved a target inside a generated `.vue.tsx` artifact; its text IS
    // supplied, so the byte offsets DO convert to a generated position — but the source
    // map does not map that position back to authored Vue space (col 0 is a source-less
    // run), so `projectGeneratedRange` returns null. The present-but-unprojectable target
    // must be surfaced, never dropped into agreement with verter's empty result.
    const map = parseSourceMap(buildSourceMapJson(["App.vue"], [[[0]]], "App.vue.tsx"));
    const locations: NormalizedLocation[] = [{ path: "App.vue.tsx", start: 10, end: 11 }];
    const out = compareDefinition([], {
      baseline: { locations, texts: { "App.vue.tsx": generatedText } },
      map,
    });
    expect(out).not.toEqual([]);
    expect(out.map((d) => d.class)).toContain("unmappedGenerated");
  });

  it("a normally-projectable generated baseline location still compares as authored parity", () => {
    // Same generated artifact, but now the map DOES carry `y` back to App.vue (1,4)..(1,5);
    // verter resolves that same authored identity -> agreement (the fix must not over-reach
    // and flag a projectable location).
    const map = parseSourceMap(buildSourceMapJson(["App.vue"], [[[10, 0, 1, 4]]], "App.vue.tsx"));
    const locations: NormalizedLocation[] = [{ path: "App.vue.tsx", start: 10, end: 11 }];
    const verter: CanonicalDefinitionTarget[] = [
      {
        uri: "App.vue",
        range: { start: { line: 1, character: 4 }, end: { line: 1, character: 5 } },
      },
    ];
    const out = compareDefinition(verter, {
      baseline: { locations, texts: { "App.vue.tsx": generatedText } },
      map,
    });
    expect(out).toEqual([]);
  });
});

describe("compareDefinition — a generated target in an UNMAPPED run is not a false source match", () => {
  it("classifies as unmappedGenerated, never a spurious Vue identity", () => {
    // Generated line 0: a mapped token at col 10 -> App.vue (1,6), then a source-less
    // token at col 20. A generated definition target at cols 22..25 sits in the unmapped
    // run. The PRE-fix decoder would project it back through the col-10 mapped segment to
    // App.vue (1,18)..(1,21); that false identity is asserted here as `expected`, so a
    // regressed decoder would FALSELY agree.
    const map = parseSourceMap(buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6], [20]]]));
    const generated: CanonicalDefinitionTarget = {
      uri: "file:///proj/App.vue.tsx",
      range: { start: { line: 0, character: 22 }, end: { line: 0, character: 25 } },
      fromGenerated: true,
    };
    const falseProjectedIdentity: ExpectedDefinition = {
      uri: "App.vue",
      range: { start: { line: 1, character: 18 }, end: { line: 1, character: 21 } },
    };
    const out = compareDefinition([generated], { expected: falseProjectedIdentity, map });
    expect(out.map((d) => d.class)).toEqual(["unmappedGenerated"]);
    expect(out.map((d) => d.class)).not.toContain("wrongTarget");
  });
});
