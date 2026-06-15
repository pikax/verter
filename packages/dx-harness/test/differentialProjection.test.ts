import { describe, expect, it } from "vitest";

import {
  GeneratedDocument,
  baselineByteToPosition,
  baselineRangeToPosition,
  parseSourceMap,
  projectGeneratedPosition,
  projectGeneratedRange,
} from "../src/differential/projection.js";
import { buildSourceMapJson } from "./_sourceMap.js";

// The V3 mappings are authored from ABSOLUTE segments by the shared, separately-authored
// `buildSourceMapJson` helper (single source — `./_sourceMap.ts`), so the decode/projection
// under test is still checked against an independent encoder (a decoder bug fails here).
// Segments are absolute `[genCol, srcIdx, srcLine, srcCol]`; genCol resets per generated
// line, the source fields stay cumulative across lines (standard V3).
function fixtureMap(): string {
  // Generated line 3, col 10 -> source 0, line 1, col 6.
  return buildSourceMapJson(["App.vue"], [[], [], [], [[10, 0, 1, 6]]], "App.vue.tsx");
}

describe("projection — baseline byte offset <-> LSP position (UTF-16, multi-line, surrogate pair)", () => {
  // 😀 = U+1F600 — one surrogate PAIR (2 UTF-16 units, 4 UTF-8 bytes).
  const tsx = 'const a = 1;\nconst smiley = "😀";\nconst b = 2;\n';

  it("converts a UTF-8 byte offset to a UTF-16 {line,character} and round-trips", () => {
    // Byte 33 is the closing quote after the emoji on line 1.
    const closing = baselineByteToPosition(tsx, 33);
    expect(closing).toEqual({ line: 1, character: 18 });

    // The `b` identifier on line 2 — character counts UTF-16 units from line start.
    const lineTwoStart = tsx.indexOf("const b");
    const bByte = Buffer.byteLength(tsx.slice(0, lineTwoStart), "utf-8") + 6;
    const bPos = baselineByteToPosition(tsx, bByte);
    expect(bPos).toEqual({ line: 2, character: 6 });
  });

  it("projects a baseline byte range into an LSP range", () => {
    const range = baselineRangeToPosition(tsx, 29, 33);
    // bytes 29..33 are exactly the surrogate-pair emoji: chars 16..18 on line 1.
    expect(range).toEqual({
      start: { line: 1, character: 16 },
      end: { line: 1, character: 18 },
    });
  });

  it("GeneratedDocument answers many offsets from one prepared, reused converter", () => {
    // Built once; queried per probe — the per-call rebuild of the one-shot helpers is
    // gone. Surrogate-pair / multi-line math matches the one-shot convenience exactly.
    const doc = new GeneratedDocument(tsx);
    expect(doc.byteToPosition(33)).toEqual({ line: 1, character: 18 });
    expect(doc.byteRangeToPosition(29, 33)).toEqual({
      start: { line: 1, character: 16 },
      end: { line: 1, character: 18 },
    });
    // A further query on the SAME instance resolves against the reused line index.
    expect(doc.byteToPosition(0)).toEqual({ line: 0, character: 0 });
  });
});

describe("projection — generated position -> authored Vue position via the source map", () => {
  it("lands on the authored position at a mapped segment", () => {
    const map = parseSourceMap(fixtureMap());
    const at = projectGeneratedPosition(map, { line: 3, character: 10 });
    expect(at).toEqual({ source: "App.vue", line: 1, character: 6 });
  });

  it("interpolates within the copied run of the covering segment", () => {
    const map = parseSourceMap(fixtureMap());
    // 3 units past the segment start -> 3 units past the authored col.
    const at = projectGeneratedPosition(map, { line: 3, character: 13 });
    expect(at).toEqual({ source: "App.vue", line: 1, character: 9 });
  });

  it("returns null before any segment and on an unmapped/out-of-range line", () => {
    const map = parseSourceMap(fixtureMap());
    expect(projectGeneratedPosition(map, { line: 3, character: 5 })).toBeNull();
    expect(projectGeneratedPosition(map, { line: 0, character: 0 })).toBeNull();
    expect(projectGeneratedPosition(map, { line: 99, character: 0 })).toBeNull();
  });

  it("throws on a malformed map and on an unsupported version", () => {
    expect(() => parseSourceMap("not json")).toThrow();
    expect(() =>
      parseSourceMap(JSON.stringify({ version: 2, sources: [], mappings: "" })),
    ).toThrow();
  });
});

describe("projection — a source-less (unmapped) segment after a mapped one projects to null", () => {
  // Generated line 0: a mapped token at col 10 -> App.vue (1,6), then a source-less
  // (1-field) token at col 20 — the shape verter emits for inserted/generated text.
  // A position at/after col 20 is in the unmapped run and MUST NOT fall back to the
  // earlier mapped segment.
  function unmappedAfterMappedMap(): string {
    return buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6], [20]]], "App.vue.tsx");
  }

  it("projects within the mapped run, then null inside the trailing unmapped run", () => {
    const map = parseSourceMap(unmappedAfterMappedMap());
    // Inside the mapped run (cols 10..19): interpolates against the mapped segment.
    expect(projectGeneratedPosition(map, { line: 0, character: 12 })).toEqual({
      source: "App.vue",
      line: 1,
      character: 8,
    });
    // Inside the trailing unmapped run (cols >= 20): no authored source.
    expect(projectGeneratedPosition(map, { line: 0, character: 25 })).toBeNull();
  });
});

// ── The closed content-span matrix ───────────────────────────────────────────
// `projectGeneratedRange` is a SINGLE generated-content span walker: it proves every content
// column [start .. end-1] is one contiguous same-source 1:1 mapping (authored-column contiguity
// within a generated line, authored-line continuity across a line break) and never samples only
// the endpoints. The ten rows below are the closed coverage of that contract: an endpoint-sampling
// implementation passes some rows and fails others; only the full-span walk passes all ten. The
// exclusive-end column is NOT content (so a token whose end abuts an unmapped boundary still
// projects), and an end at column 0 of a later generated line takes its authored exclusive end from
// the boundary itself — the START of the next authored line.
describe("projectGeneratedRange — closed content-span matrix", () => {
  it("row 1: all mapped contiguous -> projects the full authored span", () => {
    const map = parseSourceMap(
      buildSourceMapJson(
        ["App.vue"],
        [
          [
            [10, 0, 1, 6],
            [13, 0, 1, 9],
          ],
        ],
        "App.vue.tsx",
      ),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 0, character: 16 },
      }),
    ).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 1, character: 12 } },
    });
  });

  it("row 2: exclusive end abuts an unmapped boundary -> still projects (end is not content)", () => {
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6], [13]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 0, character: 13 },
      }),
    ).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 1, character: 9 } },
    });
  });

  it("row 3: trailing unmapped content -> null", () => {
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6], [13]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 0, character: 15 },
      }),
    ).toBeNull();
  });

  it("row 4: leading unmapped content -> null", () => {
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10], [13, 0, 1, 9]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 0, character: 15 },
      }),
    ).toBeNull();
  });

  it("row 5: interior unmapped, mapping resumed before the end -> null", () => {
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6], [13], [16, 0, 1, 12]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 0, character: 18 },
      }),
    ).toBeNull();
  });

  it("row 6: interior different source, mapping resumed before the end -> null", () => {
    const map = parseSourceMap(
      buildSourceMapJson(
        ["A.vue", "B.vue"],
        [
          [
            [10, 0, 1, 6],
            [13, 1, 1, 0],
            [16, 0, 1, 12],
          ],
        ],
        "A.vue.tsx",
      ),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 0, character: 18 },
      }),
    ).toBeNull();
  });

  it("row 7: same source but non-contiguous authored columns -> null", () => {
    const map = parseSourceMap(
      buildSourceMapJson(
        ["App.vue"],
        [
          [
            [10, 0, 1, 6],
            [13, 0, 1, 20],
          ],
        ],
        "App.vue.tsx",
      ),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 0, character: 16 },
      }),
    ).toBeNull();
  });

  it("row 8: multi-line end.character === 0 mapped -> end is the next authored line start", () => {
    // Content runs through generated line 0's EOL; the exclusive end at generated line 1 col 0 is
    // the boundary, which a faithful break carries to authored line 2 col 0.
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6]], [[0, 0, 2, 0]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 0 },
      }),
    ).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 2, character: 0 } },
    });
  });

  it("row 9: multi-line end.character === 0 with interior unmapped pre-break content -> null", () => {
    const map = parseSourceMap(
      buildSourceMapJson(
        ["App.vue"],
        [[[10, 0, 1, 6], [13], [16, 0, 1, 12]], [[0, 0, 2, 0]]],
        "App.vue.tsx",
      ),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 0 },
      }),
    ).toBeNull();
  });

  it("row 10: multi-line end.character === 0 with interior different source -> null", () => {
    const map = parseSourceMap(
      buildSourceMapJson(
        ["A.vue", "B.vue"],
        [
          [
            [10, 0, 1, 6],
            [13, 1, 1, 0],
            [16, 0, 1, 12],
          ],
          [[0, 0, 2, 0]],
        ],
        "A.vue.tsx",
      ),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 0 },
      }),
    ).toBeNull();
  });
});

describe("projectGeneratedRange — zero-width and single-column edge cases", () => {
  function mappedThenUnmappedMap(): string {
    return buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6], [13]]], "App.vue.tsx");
  }

  it("a zero-width range at a mapped position projects to the zero-width authored point", () => {
    const map = parseSourceMap(mappedThenUnmappedMap());
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 11 },
        end: { line: 0, character: 11 },
      }),
    ).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 7 }, end: { line: 1, character: 7 } },
    });
  });

  it("a zero-width range at an unmapped position stays unprojectable", () => {
    const map = parseSourceMap(mappedThenUnmappedMap());
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 14 },
        end: { line: 0, character: 14 },
      }),
    ).toBeNull();
  });

  it("a single-column interior hole abutting a mapped end-1 is rejected", () => {
    // Mapped 10..12 -> App.vue (1,6); source-less at 13; mapped 14.. -> App.vue (1,10). For range
    // [10,15) the lone content col 13 is unmapped, yet start@10 and end-1@14 both map. The
    // endpoint-only check would fabricate App.vue (1,6)..(1,11).
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6], [13], [14, 0, 1, 10]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 0, character: 15 },
      }),
    ).toBeNull();
  });
});

describe("projectGeneratedRange — multi-line authored-line continuity across a generated break", () => {
  // Within a generated line the walk verifies authored-column contiguity; across a generated-line
  // break it must also verify authored-LINE continuity (advance exactly one authored line) AND that
  // the next authored line restarts at column 0 (else the skipped authored columns are fabricated).
  // These rows extend the closed matrix into the real-column and >=3-line shapes.

  it("a real-column multi-line range over ADJACENT authored lines projects", () => {
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6]], [[0, 0, 2, 0]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 3 },
      }),
    ).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 2, character: 3 } },
    });
  });

  it("a column-0 range whose break jumps to a NON-ADJACENT authored line is rejected", () => {
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6]], [[0, 0, 99, 0]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 0 },
      }),
    ).toBeNull();
  });

  it("a real-column range whose interior break jumps to a non-adjacent authored line is rejected", () => {
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6]], [[0, 0, 99, 0]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 3 },
      }),
    ).toBeNull();
  });

  it("a column-0 boundary that is a source-less token is rejected (not a faithful wrap)", () => {
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6]], [[0]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 0 },
      }),
    ).toBeNull();
  });

  it("a column-0 exclusive end mapping to authored column > 0 is rejected, not fabricated", () => {
    // The boundary continues authored line 2 but opens at col 5, so authored (2,0)..(2,4) are
    // uncovered. Without the column-0 gate the span fabricated App.vue (1,6)..(2,5).
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6]], [[0, 0, 2, 5]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 0 },
      }),
    ).toBeNull();
  });

  it("a real-column range whose next generated line opens at authored column > 0 is rejected", () => {
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6]], [[0, 0, 2, 5]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 3 },
      }),
    ).toBeNull();
  });

  it("a three-line column-0 range whose MIDDLE line opens at authored column > 0 is rejected", () => {
    // The in-loop column-0 gate must reject across the interior of a >=3-line walk, not only at the
    // final boundary — the endpoint-only fabrication was App.vue (1,6)..(4,0).
    const map = parseSourceMap(
      buildSourceMapJson(
        ["App.vue"],
        [[[10, 0, 1, 6]], [[0, 0, 2, 3]], [[0, 0, 3, 0]], [[0, 0, 4, 0]]],
        "App.vue.tsx",
      ),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 3, character: 0 },
      }),
    ).toBeNull();
  });

  it("a three-line column-0 range that restarts every authored line at column 0 projects to the boundary", () => {
    // Every generated line break restarts the next authored line at column 0 (authored 1 -> 2 -> 3),
    // a faithful multi-line 1:1 copy, so the in-loop gate must NOT over-reject. The exclusive end is
    // the column-0 boundary at generated line 3 -> authored line 4 col 0.
    const map = parseSourceMap(
      buildSourceMapJson(
        ["App.vue"],
        [[[10, 0, 1, 6]], [[0, 0, 2, 0]], [[0, 0, 3, 0]], [[0, 0, 4, 0]]],
        "App.vue.tsx",
      ),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 3, character: 0 },
      }),
    ).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 4, character: 0 } },
    });
  });

  it("a real-column multi-line range over-claims an intermediate line's authored suffix (accepted)", () => {
    // The projector owns the source map but NOT the authored Vue text, so it cannot prove a
    // NON-FINAL generated line copied through the end of its authored line. A span whose first
    // generated line copied only a prefix projects and over-claims that line's authored suffix
    // rather than rejecting; the authored-text-aware layer one level up closes this.
    const map = parseSourceMap(
      buildSourceMapJson(
        ["App.vue"],
        [[[10, 0, 1, 6]], [[0, 0, 2, 0]], [[0, 0, 3, 0]]],
        "App.vue.tsx",
      ),
    );
    expect(
      projectGeneratedRange(map, {
        start: { line: 0, character: 10 },
        end: { line: 2, character: 3 },
      }),
    ).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 3, character: 3 } },
    });
  });
});
