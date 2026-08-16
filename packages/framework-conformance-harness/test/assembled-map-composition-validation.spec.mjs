// Validation order and the `UncomposableInputMap` taxonomy.
//
// Covers LAYER 1 §4.1–§4.5: the two fail-closed outcome kinds, every one of the
// 26 sub-codes individually triggered, every stage-order tie-break the
// specification calls out as deliberate, and the interoperable JSON domain.

import { describe, expect, it } from "vitest";

import { composeAssembledVueMainModule } from "../src/assembled-map-composition-reference.mjs";

/** A §3.3 `AssembleInput` with one script fragment carrying `sourceMap`. */
function scriptInput(sourceMap, code = "const x = 1\n", overrides = {}) {
  return {
    canonicalId: "Comp.vue",
    styleCount: 0,
    customBlockCount: 0,
    styleLangs: [],
    customTypes: [],
    script: { code, sourceMap },
    template: null,
    scopeId: "",
    runtimeModuleName: null,
    isProduction: false,
    ssr: false,
    ssrModuleId: null,
    emitSsrModuleRegistration: true,
    hmrStrategy: "none",
    sourceMapRequested: true,
    authored: { script: true, template: false },
    ...overrides,
  };
}

/** A well-formed v3 map document with the given members overridden. */
function mapJson(overrides) {
  return JSON.stringify({
    version: 3,
    sources: ["Comp.vue"],
    names: [],
    mappings: "",
    ...overrides,
  });
}

function reject(sourceMap, code) {
  return composeAssembledVueMainModule(scriptInput(sourceMap, code));
}

function expectCode(sourceMap, subCode, code) {
  expect(reject(sourceMap, code)).toEqual({
    outcome: "UncomposableInputMap",
    family: subCode.slice(0, 2),
    code: subCode,
    fragment: "script",
  });
}

describe("§4.2 — the two fail-closed outcome kinds", () => {
  it("a REQUIRED map that is absent is MissingRequiredInputMap, not an UncomposableInputMap family", () => {
    const result = composeAssembledVueMainModule(scriptInput(""));
    expect(result).toEqual({ outcome: "MissingRequiredInputMap", fragment: "script" });
  });

  it("neither outcome returns a partial result — no code, no empty map", () => {
    for (const result of [reject("", undefined), reject("{ not json")]) {
      expect(Object.hasOwn(result, "code") && result.outcome === "composed").toBe(false);
      expect(Object.hasOwn(result, "map")).toBe(false);
    }
  });

  it("stage 0.2 precedes 0.3 — a missing script map beats a missing template map", () => {
    const input = scriptInput("", "const x = 1\n", {
      template: { code: "function render() {}\n", imports: [], ssrImports: [], sourceMap: "" },
      authored: { script: true, template: true },
    });
    expect(composeAssembledVueMainModule(input)).toEqual({
      outcome: "MissingRequiredInputMap",
      fragment: "script",
    });
  });

  it("§3.4 — authored but NOT present requires nothing (the inline topology)", () => {
    const input = scriptInput(mapJson({}), "const x = 1\n", {
      template: null,
      authored: { script: true, template: true },
    });
    expect(composeAssembledVueMainModule(input).outcome).toBe("composed");
  });
});

describe("§4.4 U1 — malformed map JSON", () => {
  it("U1.1 map-bytes-not-json", () => expectCode("{ not json", "U1.1"));
  it("U1.2 map-root-not-object", () => expectCode("[1, 2]", "U1.2"));
  it("U1.3 mappings-member-absent — never read as an empty map", () => {
    expectCode(JSON.stringify({ version: 3, sources: [], names: [] }), "U1.3");
  });
  it("U1.4 mappings-member-not-a-string", () => expectCode(mapJson({ mappings: 3 }), "U1.4"));
  it("U1.5 sources-member-absent", () => {
    expectCode(JSON.stringify({ version: 3, names: [], mappings: "" }), "U1.5");
  });
  it("U1.5 sources-member-not-an-array", () => expectCode(mapJson({ sources: {} }), "U1.5"));
  it("U1.6 names-member-absent", () => {
    expectCode(JSON.stringify({ version: 3, sources: [], mappings: "" }), "U1.6");
  });
  it("U1.6 names-member-not-an-array", () => expectCode(mapJson({ names: "x" }), "U1.6"));

  it("U1.7 metadata-member-wrong-type — every listed shape", () => {
    expectCode(mapJson({ sourcesContent: "x" }), "U1.7");
    expectCode(mapJson({ sourcesContent: null }), "U1.7"); // "if present, is an array"
    expectCode(mapJson({ sourceRoot: 3 }), "U1.7");
    expectCode(mapJson({ file: 3 }), "U1.7");
    expectCode(mapJson({ debugId: 3 }), "U1.7");
    expectCode(mapJson({ ignoreList: "x" }), "U1.7");
    expectCode(mapJson({ ignoreList: [-1] }), "U1.7");
    expectCode(mapJson({ ignoreList: [0.5] }), "U1.7");
    expectCode(mapJson({ x_google_ignoreList: [{}] }), "U1.7");
    // Two disagreeing ignore-list spellings.
    expectCode(mapJson({ ignoreList: [0], x_google_ignoreList: [] }), "U1.7");
  });

  it("U1.7 — two AGREEING ignore-list spellings are accepted", () => {
    const sourceMap = mapJson({ ignoreList: [0], x_google_ignoreList: [0] });
    const result = composeAssembledVueMainModule(scriptInput(sourceMap));
    expect(result.outcome).toBe("composed");
    expect(result.map.ignoreList).toEqual([0]);
  });

  it("U1.8 duplicate-object-member — detected, not inherited from the parser's object model", () => {
    // `JSON.parse` collapses this silently to `{version: 3}`; `DECISION` D-2
    // requires it rejected.
    expectCode('{"version":3,"version":3,"sources":[],"names":[],"mappings":""}', "U1.8");
    // Nested, too — "any JSON object in the document".
    expectCode('{"version":3,"sources":[],"names":[],"mappings":"","x":{"a":1,"a":2}}', "U1.8");
  });

  it("U1.9 number-outside-interoperable-domain (§4.5)", () => {
    expectCode('{"version":3,"sources":[],"names":[],"mappings":"","x":1e400}', "U1.9");
    // The motivating case, on `version` itself: `JSON.parse` would coerce it to
    // `Infinity` and the document would proceed.
    expectCode('{"version":1e400,"sources":[],"names":[],"mappings":""}', "U1.9");
  });

  it("§4.5 — an in-domain number that underflows to zero is NOT rejected", () => {
    const result = composeAssembledVueMainModule(
      scriptInput('{"version":3,"sources":[],"names":[],"mappings":"","x":1e-400}'),
    );
    expect(result.outcome).toBe("composed");
  });

  it("U1.10 string-not-well-formed-unicode (§4.5)", () => {
    expectCode('{"version":3,"sources":["\\uD800"],"names":[],"mappings":""}', "U1.10");
    // A literal lone surrogate, not only an escaped one.
    expectCode(`{"version":3,"sources":["\uD800"],"names":[],"mappings":""}`, "U1.10");
    // A well-formed surrogate PAIR is fine.
    expect(
      composeAssembledVueMainModule(
        scriptInput('{"version":3,"sources":["\\uD83D\\uDE00"],"names":[],"mappings":""}'),
      ).outcome,
    ).toBe("composed");
  });

  it("`DECISION` D-7 — numbers are binary64 values, not exact decimals", () => {
    // `3.0000000000000000001` converts to exactly 3 under round-ties-to-even,
    // so it is an integral `version` of 3 and the document is ACCEPTED.
    const result = composeAssembledVueMainModule(
      scriptInput('{"version":3.0000000000000000001,"sources":[],"names":[],"mappings":""}'),
    );
    expect(result.outcome).toBe("composed");
  });
});

describe("§4.4 U2 — wrong/missing version", () => {
  it("U2.1 version-member-absent", () => {
    expectCode(JSON.stringify({ sources: [], names: [], mappings: "" }), "U2.1");
  });
  it("U2.2 version-not-an-integer", () => {
    expectCode(mapJson({ version: 3.5 }), "U2.2");
    expectCode(mapJson({ version: "3" }), "U2.2");
  });
  it("U2.3 version-not-3", () => expectCode(mapJson({ version: 2 }), "U2.3"));
});

describe("§4.4 U3 — undecodable or out-of-range wire data", () => {
  it("U3.1 vlq-invalid-character", () => expectCode(mapJson({ mappings: "A!" }), "U3.1"));
  it("U3.2 vlq-truncated-segment", () => expectCode(mapJson({ mappings: "g" }), "U3.2"));
  it("U3.3 segment-field-count", () => {
    expectCode(mapJson({ mappings: "AC" }), "U3.3");
    // A zero-field segment token between two commas.
    expectCode(mapJson({ mappings: "A,,A" }), "U3.3");
  });
  it("U3.4 vlq-field-out-of-range", () => expectCode(mapJson({ mappings: "ggggggE" }), "U3.4"));
  it("U3.5 accumulator-out-of-range", () => expectCode(mapJson({ mappings: "D" }), "U3.5"));
  it("U3.6 generated-column-accumulator-decreased (`DECISION` D-1)", () => {
    // `mappings: "K,F"` — the decision block's own example: a sourceless
    // segment at (0,5) followed by one at (0,3).
    expectCode(mapJson({ mappings: "K,F" }), "U3.6");
  });
  it("a NON-decreasing same-line sequence is accepted", () => {
    const result = composeAssembledVueMainModule(scriptInput(mapJson({ mappings: "F,K" })));
    // "F" alone would drive genCol to −2; as the second field of a rising pair
    // it is fine — this asserts the pair `K,F` above is rejected for its ORDER.
    expect(result.outcome).toBe("UncomposableInputMap");
    expect(result.code).toBe("U3.5");
    expect(composeAssembledVueMainModule(scriptInput(mapJson({ mappings: "C,C" }))).outcome).toBe(
      "composed",
    );
  });
});

describe("§4.4 U4 — malformed table rows", () => {
  it("U4.1 source-row-not-a-string", () => expectCode(mapJson({ sources: [3] }), "U4.1"));
  it("U4.2 name-row-not-a-string", () => expectCode(mapJson({ names: [3] }), "U4.2"));
  it("U4.3 sources-content-row-not-string-or-null", () => {
    expectCode(mapJson({ sourcesContent: [3] }), "U4.3");
  });
  it("U4.4 sources-content-length-mismatch", () => {
    expectCode(mapJson({ sourcesContent: [] }), "U4.4");
  });
  it("a `null` sourcesContent ROW is accepted (only a non-string, non-null row is U4.3)", () => {
    expect(
      composeAssembledVueMainModule(scriptInput(mapJson({ sourcesContent: [null] }))).outcome,
    ).toBe("composed");
  });
});

describe("§4.4 U5 / U6 / U7 / U8", () => {
  it("U5.1 sections-member-present", () => {
    expectCode(JSON.stringify({ version: 3, sections: [] }), "U5.1");
  });

  it("U6.1 source-index-out-of-table", () => expectCode(mapJson({ mappings: "ACAA" }), "U6.1"));
  it("U6.1 is guarded on a NON-NULL srcIdx — a sourceless segment is never an instance", () => {
    // A 1-field segment against an EMPTY sources table. An unguarded check
    // would reject it and take the whole sourceless-barrier algebra with it.
    const result = composeAssembledVueMainModule(
      scriptInput(mapJson({ sources: [], mappings: "A" })),
    );
    expect(result.outcome).toBe("composed");
  });
  it("U6.2 name-index-out-of-table", () => expectCode(mapJson({ mappings: "AAAAC" }), "U6.2"));
  it("U6.3 ignore-list-index-out-of-table", () => {
    expectCode(mapJson({ ignoreList: [5] }), "U6.3");
  });

  it("U7.1 generated-line-out-of-fragment", () => {
    expectCode(mapJson({ mappings: ";A" }), "U7.1", "x");
  });
  it("U7.2 generated-column-out-of-fragment", () => {
    expectCode(mapJson({ mappings: "K" }), "U7.2", "x");
  });
  it("U7.2 admits an END-OF-LINE column (a column equal to the line length)", () => {
    // `lineTable("x")[0].length` is 1, so column 1 is in-bounds (§2.1).
    expect(
      composeAssembledVueMainModule(scriptInput(mapJson({ mappings: "C" }), "x")).outcome,
    ).toBe("composed");
  });
  it("U7.3 generated-column-splits-a-surrogate-pair", () => {
    expectCode(mapJson({ mappings: "C" }), "U7.3", "\u{1D400}");
    // The `genCol ≥ 1` guard makes the predicate total: column 0 never splits.
    expect(
      composeAssembledVueMainModule(scriptInput(mapJson({ mappings: "A" }), "\u{1D400}")).outcome,
    ).toBe("composed");
  });

  it("U8.1 source-root-conflict", () => {
    const input = scriptInput(mapJson({ sourceRoot: "a" }), "const x = 1\n", {
      template: {
        code: "function render() {}\n",
        imports: [],
        ssrImports: [],
        sourceMap: mapJson({ sourceRoot: "b" }),
      },
      authored: { script: true, template: true },
    });
    expect(composeAssembledVueMainModule(input)).toEqual({
      outcome: "UncomposableInputMap",
      family: "U8",
      code: "U8.1",
      fragment: "template",
    });
  });

  it("§4.3 stage 2 runs at CARDINALITY ONE — a single map's sourceRoot carries through", () => {
    const result = composeAssembledVueMainModule(scriptInput(mapJson({ sourceRoot: "root/" })));
    expect(result.outcome).toBe("composed");
    expect(result.map.sourceRoot).toBe("root/");
  });

  it('§7.5 — `""` is a DISTINCT declared value from absent', () => {
    const empty = composeAssembledVueMainModule(scriptInput(mapJson({ sourceRoot: "" })));
    expect(Object.hasOwn(empty.map, "sourceRoot")).toBe(true);
    expect(empty.map.sourceRoot).toBe("");
    const absent = composeAssembledVueMainModule(scriptInput(mapJson({})));
    expect(Object.hasOwn(absent.map, "sourceRoot")).toBe(false);
    // …and the two therefore CONFLICT across fragments.
    const conflicting = scriptInput(mapJson({ sourceRoot: "" }), "const x = 1\n", {
      template: {
        code: "function render() {}\n",
        imports: [],
        ssrImports: [],
        sourceMap: mapJson({}),
      },
      authored: { script: true, template: true },
    });
    expect(composeAssembledVueMainModule(conflicting).code).toBe("U8.1");
  });

  it("§7.5 — a JSON-`null` sourceRoot normalises to ABSENT and agrees with an absent one", () => {
    const input = scriptInput(mapJson({ sourceRoot: null }), "const x = 1\n", {
      template: {
        code: "function render() {}\n",
        imports: [],
        ssrImports: [],
        sourceMap: mapJson({}),
      },
      authored: { script: true, template: true },
    });
    const result = composeAssembledVueMainModule(input);
    expect(result.outcome).toBe("composed");
    expect(Object.hasOwn(result.map, "sourceRoot")).toBe(false);
  });
});

describe("§4.3 — the stage-order tie-breaks, each stated as deliberate", () => {
  it("duplicate-member detection precedes every member read", () => {
    // Also a `version: 2` document: without 1.2 running first this reports U2.3.
    expectCode('{"version":2,"version":2,"sources":[],"names":[],"mappings":""}', "U1.8");
  });

  it("clause (b) precedes clause (c): an out-of-domain number beats a lone surrogate", () => {
    expectCode('{"version":3,"sources":["\\uD800"],"names":[],"mappings":"","x":1e400}', "U1.9");
  });

  it("step 1.1 precedes step 1.2: an out-of-domain number beats a duplicate member", () => {
    expectCode('{"version":3,"version":3,"sources":[],"names":[],"mappings":"","x":1e400}', "U1.9");
  });

  it("version beats indexed-map", () => {
    expectCode(JSON.stringify({ version: 2, sections: [] }), "U2.3");
  });

  it("indexed-map beats missing `mappings`", () => {
    expectCode(JSON.stringify({ version: 3, sections: [] }), "U5.1");
  });

  it("row typing beats wire decoding", () => {
    expectCode(mapJson({ sources: [3], mappings: "!!!" }), "U4.1");
  });

  it("`sources` rows beat `names` rows beat `sourcesContent` rows", () => {
    expectCode(mapJson({ sources: [3], names: [3], sourcesContent: [3] }), "U4.1");
    expectCode(mapJson({ names: [3], sourcesContent: [3] }), "U4.2");
    expectCode(mapJson({ sourcesContent: [3] }), "U4.3");
  });

  it('arity beats index bounds — `"AC"` is U3.3, never U6.1 (F5\'s distinction)', () => {
    expectCode(mapJson({ mappings: "AC" }), "U3.3");
  });

  it("arity beats every accumulator property — a 3-field segment with an underflowing field 0", () => {
    // `"DAA"` decodes to three fields whose first would drive `genCol` to −1.
    // "A 3-field segment has no defined interpretation at all", so U3.3 wins.
    expectCode(mapJson({ mappings: "DAA" }), "U3.3");
  });

  it("within phase C, range beats ordering", () => {
    // `"K,N"`: genCol 5 then 5 − 6 = −1 — out of range, so U3.5.
    expectCode(mapJson({ mappings: "K,N" }), "U3.5");
    // `"K,F"`: genCol 5 then 3 — in range but decreasing, so U3.6.
    expectCode(mapJson({ mappings: "K,F" }), "U3.6");
  });

  it("index bounds beat coordinate bounds, as a STAGE precedence across segments", () => {
    // Segment 0 is out-of-fragment (`genCol` 5 against a 2-unit line); segment
    // 1 is dangling-index. 1.22 runs over ALL segments before 1.24 does, so the
    // LATER segment's index violation wins.
    expectCode(mapJson({ mappings: "KAAA,CCAA" }), "U6.1", "ab\n");
  });

  it("script beats template", () => {
    const input = scriptInput("{ not json", "const x = 1\n", {
      template: {
        code: "function render() {}\n",
        imports: [],
        ssrImports: [],
        sourceMap: mapJson({ mappings: "ACAA" }),
      },
      authored: { script: true, template: true },
    });
    expect(composeAssembledVueMainModule(input)).toEqual({
      outcome: "UncomposableInputMap",
      family: "U1",
      code: "U1.1",
      fragment: "script",
    });
  });

  it("a TEMPLATE-only rejection names the template fragment", () => {
    const input = scriptInput(mapJson({}), "const x = 1\n", {
      template: {
        code: "function render() {}\n",
        imports: [],
        ssrImports: [],
        sourceMap: mapJson({ mappings: "ACAA" }),
      },
      authored: { script: true, template: true },
    });
    expect(composeAssembledVueMainModule(input)).toEqual({
      outcome: "UncomposableInputMap",
      family: "U6",
      code: "U6.1",
      fragment: "template",
    });
  });
});

describe("§4.4 — original coordinates are NOT validated", () => {
  it("an absurd but in-range authored coordinate is carried forward faithfully", () => {
    // `srcLine`/`srcCol` are carried opaquely; BV0A holds no authored file to
    // validate them against. The only constraint is `U3.5`'s accumulator range.
    // `"AA+/////Dw"` is not used — a plain large delta suffices.
    const result = composeAssembledVueMainModule(
      scriptInput(mapJson({ mappings: "AAggggPggggP" })),
    );
    expect(result.outcome).toBe("composed");
    expect(result.segments[0].srcLine).toBeGreaterThan(1_000_000);
  });
});

describe("§4.3 — every staged tie-break has a LIVE loser", () => {
  // The block above asserts which check WINS on an input several checks hold
  // for. On its own that is weaker than it looks: if the losing check would not
  // have fired anyway, the winner wins by default and the stage order is
  // carrying no weight — those assertions would keep passing with the order
  // reversed, or with the loser deleted.
  //
  // Each case here takes the SAME input minus the winner's own trigger and
  // asserts the loser then reports. Together the two halves say: both checks
  // are armed, and the order is what decides. Weakening or reordering either
  // check fails one half.

  it("U1.8's loser is armed — without the duplicate the document is U2.3", () => {
    expectCode('{"version":2,"version":2,"sources":[],"names":[],"mappings":""}', "U1.8");
    expectCode('{"version":2,"sources":[],"names":[],"mappings":""}', "U2.3");
  });

  it("U5.1's loser is armed — without `sections` the document is U1.3", () => {
    expectCode(JSON.stringify({ version: 3, sections: [] }), "U5.1");
    expectCode(JSON.stringify({ version: 3, sources: [], names: [] }), "U1.3");
  });

  it("U4.1's loser is armed — with typed rows the same `mappings` is U3.1", () => {
    expectCode(mapJson({ sources: [3], mappings: "!!!" }), "U4.1");
    expectCode(mapJson({ mappings: "!!!" }), "U3.1");
  });

  it("U3.3's accumulator loser is armed — at legal arity the same field is U3.5", () => {
    // `"DAA"` is three fields whose first would drive `genCol` to −1; `"D"` is
    // the one-field segment that leaves only the accumulator violation.
    expectCode(mapJson({ mappings: "DAA" }), "U3.3");
    expectCode(mapJson({ mappings: "D" }), "U3.5");
  });

  it("U3.3's index loser is armed — at legal arity the same index is U6.1", () => {
    // F5's distinction: `"AC"` is a two-field segment, `"ACAA"` the
    // well-formed four-field version of the same dangling source index.
    expectCode(mapJson({ mappings: "AC" }), "U3.3");
    expectCode(mapJson({ mappings: "ACAA" }), "U6.1");
  });

  it("U6.1's loser is armed — with the index in range the same input is U7.2", () => {
    // `"KAAA,CCAA"`: segment 0's column is out of the 2-unit fragment line and
    // segment 1's source index has no row. `"KAAA,CAAA"` keeps the coordinate
    // violation and drops the index one.
    expectCode(mapJson({ mappings: "KAAA,CCAA" }), "U6.1", "ab\n");
    expectCode(mapJson({ mappings: "KAAA,CAAA" }), "U7.2", "ab\n");
  });

  it("U8.1's companion is armed — the same two fragments compose once the roots agree", () => {
    const pair = (scriptRoot, templateRoot) =>
      scriptInput(mapJson({ sourceRoot: scriptRoot }), "const x = 1\n", {
        template: {
          code: "function render() {}\n",
          imports: [],
          ssrImports: [],
          sourceMap: mapJson({ sourceRoot: templateRoot }),
        },
        authored: { script: true, template: true },
      });

    expect(composeAssembledVueMainModule(pair("a", "b"))).toEqual({
      outcome: "UncomposableInputMap",
      family: "U8",
      code: "U8.1",
      fragment: "template",
    });

    // Stage 2 is the ONLY thing standing between this pair and a composed
    // module, so the rejection above is caused by the disagreement and by
    // nothing else about the pair.
    const agreeing = composeAssembledVueMainModule(pair("a", "a"));
    expect(agreeing.outcome).toBe("composed");
    expect(agreeing.map.sourceRoot).toBe("a");
  });
});
