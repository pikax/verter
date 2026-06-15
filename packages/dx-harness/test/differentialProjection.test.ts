import { describe, expect, it } from "vitest";

import {
  GeneratedDocument,
  baselineByteToPosition,
  baselineRangeToPosition,
  parseSourceMap,
  projectGeneratedPosition,
  projectGeneratedRange,
  type GeneratedProjection,
  type ParsedSourceMap,
} from "../src/differential/projection.js";
import { buildSourceMapJson } from "./_sourceMap.js";

/**
 * Pair a parsed source map with a generated-artifact document for {@link projectGeneratedRange}.
 * The document's per-line lengths are consulted ONLY by the column-0 reconstruction, which bounds
 * the last content line by its REAL generated length; a range that never reaches that path projects
 * identically regardless of the document, so non-column-0 cases pass the empty stand-in. `lineLengths`
 * builds a text whose line `i` holds exactly `lineLengths[i]` UTF-16 code units of content.
 */
function proj(map: ParsedSourceMap, lineLengths: readonly number[] = []): GeneratedProjection {
  const text = lineLengths.map((n) => "x".repeat(n)).join("\n");
  return { map, document: new GeneratedDocument(text) };
}

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

  it("projects a same-source generated range to an authored range", () => {
    const map = parseSourceMap(fixtureMap());
    const projected = projectGeneratedRange(proj(map), {
      start: { line: 3, character: 10 },
      end: { line: 3, character: 13 },
    });
    expect(projected).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 1, character: 9 } },
    });
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

describe("projection — a generated RANGE is projected by its INCLUSIVE content, not its exclusive end", () => {
  // Generated line 0: a mapped token covers cols 10..12 -> App.vue (1,6),(1,7),(1,8); a
  // source-less (unmapped) run begins at col 13. The LSP range is inclusive-start /
  // EXCLUSIVE-end, so a range's content is [start .. end-1] and the exclusive end is NOT
  // part of it — a range whose end merely abuts the unmapped boundary is still mapped.
  function mappedThenUnmappedMap(): string {
    return buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6], [13]]], "App.vue.tsx");
  }

  it("a mapped range whose EXCLUSIVE end abuts an unmapped boundary still projects", () => {
    // [10,13): content cols 10,11,12 are all mapped; the exclusive end (13) is the first
    // unmapped column but is not range content, so the range projects to its authored span
    // and is NOT rejected as unmapped.
    const map = parseSourceMap(mappedThenUnmappedMap());
    const projected = projectGeneratedRange(proj(map), {
      start: { line: 0, character: 10 },
      end: { line: 0, character: 13 },
    });
    expect(projected).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 1, character: 9 } },
    });
  });

  it("a range whose CONTENT extends into the unmapped run stays unprojectable", () => {
    // [10,15): content cols 13,14 fall inside the unmapped run -> no authored source.
    const map = parseSourceMap(mappedThenUnmappedMap());
    expect(
      projectGeneratedRange(proj(map), {
        start: { line: 0, character: 10 },
        end: { line: 0, character: 15 },
      }),
    ).toBeNull();
  });

  it("a zero-width range at a mapped position projects to the zero-width authored point", () => {
    const map = parseSourceMap(mappedThenUnmappedMap());
    const projected = projectGeneratedRange(proj(map), {
      start: { line: 0, character: 11 },
      end: { line: 0, character: 11 },
    });
    expect(projected).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 7 }, end: { line: 1, character: 7 } },
    });
  });

  it("a zero-width range at an unmapped position stays unprojectable", () => {
    const map = parseSourceMap(mappedThenUnmappedMap());
    expect(
      projectGeneratedRange(proj(map), {
        start: { line: 0, character: 14 },
        end: { line: 0, character: 14 },
      }),
    ).toBeNull();
  });
});

describe("projection — a generated RANGE rejects an interior unmapped or different-source hole", () => {
  // A range is mapped only when EVERY content position [start .. end-1] is covered by one
  // contiguous same-source mapping. A hole that mapping resumes past before the last
  // included position must reject the range, not fabricate an authored span across it —
  // checking only the start and the last-included position would miss the hole.

  it("an INTERIOR unmapped run is rejected even when start and end-1 both map", () => {
    // Generated line 0: mapped 10..12 -> App.vue (1,6); source-less 13..15; mapped 16.. ->
    // App.vue (1,12). For range [10,18) the content cols 13..15 are genuinely unmapped, yet
    // start@10 and end-1@17 both resolve to App.vue. The endpoint-only check would FABRICATE
    // App.vue (1,6)..(1,14); the content-coverage check rejects it as unprojectable.
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6], [13], [16, 0, 1, 12]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(proj(map), {
        start: { line: 0, character: 10 },
        end: { line: 0, character: 18 },
      }),
    ).toBeNull();
  });

  it("a single-column interior hole abutting a mapped end-1 is rejected", () => {
    // Generated line 0: mapped 10..12 -> App.vue (1,6); source-less at 13; mapped 14.. ->
    // App.vue (1,10). For range [10,15) the lone content col 13 is unmapped, yet start@10
    // and end-1@14 both map. The endpoint-only check would fabricate App.vue (1,6)..(1,11).
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6], [13], [14, 0, 1, 10]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(proj(map), {
        start: { line: 0, character: 10 },
        end: { line: 0, character: 15 },
      }),
    ).toBeNull();
  });

  it("an interior segment that maps to a DIFFERENT source is rejected", () => {
    // Generated line 0: 10..12 -> A.vue (1,6); 13..15 -> B.vue (1,0); 16.. -> A.vue (1,12).
    // start@10 and end-1@17 both resolve to A.vue, but the content crosses B.vue, so no
    // single A.vue range faithfully represents the generated content.
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
      projectGeneratedRange(proj(map), {
        start: { line: 0, character: 10 },
        end: { line: 0, character: 18 },
      }),
    ).toBeNull();
  });

  it("a same-source interior segment whose authored columns stay contiguous still projects", () => {
    // Two adjacent same-source segments form one 1:1-copied run: 10..12 -> App.vue (1,6) and
    // 13.. -> App.vue (1,9). The authored columns continue 6,7,8,9,... unbroken, so the
    // content-coverage check accepts it and must NOT over-reject a faithful split run.
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
      projectGeneratedRange(proj(map), {
        start: { line: 0, character: 10 },
        end: { line: 0, character: 16 },
      }),
    ).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 1, character: 12 } },
    });
  });

  it("a same-source interior segment whose authored columns JUMP is rejected", () => {
    // Same source on both segments, but the second jumps the authored column from the
    // expected 9 to 20: the generated content is not a contiguous copy, so no single
    // authored range represents it and the endpoint-only fabrication is refused.
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
      projectGeneratedRange(proj(map), {
        start: { line: 0, character: 10 },
        end: { line: 0, character: 16 },
      }),
    ).toBeNull();
  });
});

describe("projection — a multi-line RANGE ending at column 0 bounds the last content line by its REAL generated length", () => {
  // When the exclusive end sits at column 0 of the next generated line, the content runs through
  // the end of the previous generated line. The authored exclusive end is one past the authored
  // position of the LAST ACTUAL generated content column on that line — bounded by the generated
  // line's real length — NOT the authored position the column-0 boundary itself maps to. The
  // column-0 boundary is still validated as a faithful same-source line break (it must continue
  // the authored line at column 0), but it does not supply the returned end: a generated line that
  // copies only a prefix of its authored line must not have the uncovered authored suffix invented.

  it("bounds the authored end by the last content line's real length, not the column-0 boundary", () => {
    // Generated line 0 has real length 13 (cols 0..12); col 10 -> App.vue (1,6). Generated line 1
    // col 0 -> App.vue (2,0). For range [0:10, 1:0) the content is line 0 cols 10..12 -> authored
    // (1,6),(1,7),(1,8), so the authored exclusive end is (1,9). Anchoring on the column-0 boundary
    // fabricated (1,6)..(2,0) — claiming authored line 1's whole suffix the 3 generated columns
    // never covered; bounding by the real generated length yields the faithful (1,6)..(1,9).
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6]], [[0, 0, 2, 0]]], "App.vue.tsx"),
    );
    const projected = projectGeneratedRange(proj(map, [13, 5]), {
      start: { line: 0, character: 10 },
      end: { line: 1, character: 0 },
    });
    expect(projected).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 1, character: 9 } },
    });
  });

  it("stays unprojectable when the next line's column 0 is a source-less boundary", () => {
    // Same start, but generated line 1 col 0 is a source-less token: the column-0 boundary has no
    // authored source, so the line break is not a faithful same-source wrap and the range yields
    // null rather than a fabricated end. The boundary gate rejects this independently of the
    // real-length-bounded end the content would otherwise produce.
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6]], [[0]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(proj(map, [13, 5]), {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 0 },
      }),
    ).toBeNull();
  });
});

describe("projection — a multi-line RANGE ending at column 0 rejects an interior hole in the pre-break content", () => {
  // The content of a range ending at { L, 0 } is [start .. the end of line L-1], and EVERY one
  // of those content positions must be one contiguous same-source mapping — exactly the rule the
  // end.character>0 path enforces. A range whose pre-break content contains an interior unmapped
  // or different-source segment must reject, even when both the start and the column-0 exclusive
  // end map to the start source. Anchoring only on those two endpoints would fabricate an authored
  // span across the hole.

  it("an INTERIOR unmapped run on the line before the break is rejected", () => {
    // Generated line 0: mapped 10..12 -> App.vue (1,6); source-less 13..15; mapped 16.. ->
    // App.vue (1,12). Generated line 1 col 0 -> App.vue (2,0) is the column-0 exclusive end. For
    // range [0:10, 1:0) the content cols 13..15 on line 0 are genuinely unmapped, yet start@0:10
    // and the column-0 end both resolve to App.vue. An endpoint-only check would FABRICATE
    // App.vue (1,6)..(2,0); the content-coverage check rejects it as unprojectable.
    const map = parseSourceMap(
      buildSourceMapJson(
        ["App.vue"],
        [[[10, 0, 1, 6], [13], [16, 0, 1, 12]], [[0, 0, 2, 0]]],
        "App.vue.tsx",
      ),
    );
    expect(
      projectGeneratedRange(proj(map, [20, 5]), {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 0 },
      }),
    ).toBeNull();
  });

  it("an interior segment that maps to a DIFFERENT source is rejected", () => {
    // Generated line 0: 10..12 -> A.vue (1,6); 13..15 -> B.vue (1,0); 16.. -> A.vue (1,12).
    // Generated line 1 col 0 -> A.vue (2,0). start@0:10 and the column-0 end both resolve to
    // A.vue, but the pre-break content crosses B.vue, so no single A.vue range faithfully
    // represents it.
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
      projectGeneratedRange(proj(map, [20, 5]), {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 0 },
      }),
    ).toBeNull();
  });
});

describe("projection — a multi-line RANGE rejects a generated-line break onto a non-adjacent authored line", () => {
  // Within a generated line the content-coverage walk verifies authored-column contiguity; across
  // a generated-line break it must also verify authored-LINE continuity — a faithful 1:1 copy
  // advances exactly one authored line per generated line break. A range whose generated lines map
  // to non-adjacent authored lines (gen line 0 -> authored 1, gen line 1 -> authored 99) is not a
  // contiguous copy: projecting it would fabricate an authored span across the skipped lines.

  it("a column-0 range whose break jumps to a non-adjacent authored line is rejected", () => {
    // Generated line 0 col 10 -> App.vue (1,6); generated line 1 col 0 -> App.vue (99,0). The
    // column-0 exclusive end resolves to App.vue, but authored line 99 does not continue authored
    // line 1, so the endpoint-only projection App.vue (1,6)..(99,0) is refused.
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6]], [[0, 0, 99, 0]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(proj(map, [13, 5]), {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 0 },
      }),
    ).toBeNull();
  });

  it("a real-column range whose interior break jumps to a non-adjacent authored line is rejected", () => {
    // Generated line 0 col 10 -> App.vue (1,6); generated line 1 col 0 -> App.vue (99,0). For
    // range [0:10, 1:3) the content spans both generated lines; the within-line walk on each line
    // passes, but generated line 1 maps to authored line 99, not authored line 2, so the contiguous
    // copy is broken and the range is refused rather than fabricated as App.vue (1,6)..(99,3).
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6]], [[0, 0, 99, 0]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(proj(map), {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 3 },
      }),
    ).toBeNull();
  });

  it("a real-column range whose generated lines map to ADJACENT authored lines still projects", () => {
    // Generated line 0 col 10 -> App.vue (1,6); generated line 1 col 0 -> App.vue (2,0). For range
    // [0:10, 1:3) the generated lines map to authored lines 1 then 2 — a faithful 1:1 multi-line
    // copy — so the authored-line continuity check must NOT over-reject it.
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6]], [[0, 0, 2, 0]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(proj(map), {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 3 },
      }),
    ).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 2, character: 3 } },
    });
  });
});

describe("projection — a generated-line break must restart the next authored line at column 0, else the skipped authored columns are fabricated", () => {
  // A faithful generated line break maps generated { L, 0 } to authored { A+1, 0 } — the START of
  // the next authored line. When a generated line that opens a new authored line instead opens at
  // authored { A+1, C } with C > 0, the authored columns { A+1, 0 .. C-1 } precede it and are
  // covered by NO generated content; spanning the authored range across them fabricates them. The
  // span is rejected (routed to unmappedGenerated) rather than inventing the uncovered columns —
  // this gates the column-0 exclusive-end boundary AND every subsequent content line in the walk.

  it("a column-0 exclusive end mapping to authored column > 0 is rejected, not fabricated", () => {
    // Generated line 0 col 10 -> App.vue (1,6); generated line 1 col 0 -> App.vue (2,5). The
    // column-0 boundary continues authored line 2 but opens at authored col 5, so authored
    // (2,0)..(2,4) are uncovered. Without the column-0 gate the span fabricated App.vue (1,6)..(2,5).
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6]], [[0, 0, 2, 5]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(proj(map, [13, 5]), {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 0 },
      }),
    ).toBeNull();
  });

  it("a real-column range whose next generated line opens at authored column > 0 is rejected", () => {
    // The real-column analog of the column-0 boundary gap. Generated line 0 col 10 -> App.vue (1,6);
    // generated line 1 col 0 -> App.vue (2,5). For range [0:10, 1:3) the content includes generated
    // line 1 cols 0..2 -> authored (2,5)..(2,7), but the authored span would open the line at (2,0),
    // fabricating (2,0)..(2,4). Without the gate the span fabricated App.vue (1,6)..(2,8).
    const map = parseSourceMap(
      buildSourceMapJson(["App.vue"], [[[10, 0, 1, 6]], [[0, 0, 2, 5]]], "App.vue.tsx"),
    );
    expect(
      projectGeneratedRange(proj(map), {
        start: { line: 0, character: 10 },
        end: { line: 1, character: 3 },
      }),
    ).toBeNull();
  });

  it("a three-generated-line column-0 range whose middle line opens at authored column > 0 is rejected", () => {
    // Content spans generated lines 0,1,2 with the exclusive end at generated line 3 col 0. The
    // MIDDLE content line (generated line 1) opens at authored (2,3), not (2,0), so authored
    // (2,0)..(2,2) are uncovered. The in-loop column-0 gate must reject across the interior of a
    // >=3-line walk, not only at the final boundary — the endpoint-only fabrication was
    // App.vue (1,6)..(4,0).
    const map = parseSourceMap(
      buildSourceMapJson(
        ["App.vue"],
        [[[10, 0, 1, 6]], [[0, 0, 2, 3]], [[0, 0, 3, 0]], [[0, 0, 4, 0]]],
        "App.vue.tsx",
      ),
    );
    expect(
      projectGeneratedRange(proj(map, [13, 5, 5, 5]), {
        start: { line: 0, character: 10 },
        end: { line: 3, character: 0 },
      }),
    ).toBeNull();
  });

  it("a three-generated-line column-0 range that restarts every authored line at column 0 still projects", () => {
    // The must-not-over-reject companion: every generated line break restarts the next authored line
    // at column 0 (authored 1 -> 2 -> 3), a faithful multi-line 1:1 copy, so the column-0 gate must
    // NOT reject it. The 2-line faithful col-0 wrap is already pinned above; this proves the in-loop
    // gate generalizes to >=3 generated lines without over-rejecting. The authored end is bounded by
    // the last content line's (generated line 2) real length 5 -> authored (3,5), NOT the column-0
    // boundary's authored (4,0): the boundary at generated line 3 col 0 is still validated as a
    // faithful break to authored (4,0), but it only gates and never supplies the returned end.
    const map = parseSourceMap(
      buildSourceMapJson(
        ["App.vue"],
        [[[10, 0, 1, 6]], [[0, 0, 2, 0]], [[0, 0, 3, 0]], [[0, 0, 4, 0]]],
        "App.vue.tsx",
      ),
    );
    expect(
      projectGeneratedRange(proj(map, [13, 5, 5, 5]), {
        start: { line: 0, character: 10 },
        end: { line: 3, character: 0 },
      }),
    ).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 3, character: 5 } },
    });
  });
});

describe("projection — a multi-line RANGE over-claims an intermediate generated line's authored suffix (accepted)", () => {
  // The projector owns the generated artifact text and the source map, but NOT the authored Vue
  // text. It bounds the LAST content line by its real generated length, but it cannot prove that a
  // NON-FINAL (intermediate) generated line copied through the end of its authored line — that
  // needs the authored line length, which lives one layer up. So a multi-line span whose first
  // generated line copied only a prefix of its authored line projects and over-claims that line's
  // authored suffix rather than rejecting. This pins the accepted behavior; it does NOT reject. The
  // authored-text-aware generated-to-Vue projection layer must close it by checking authored line
  // lengths.

  it("projects a span whose intermediate line copied only a prefix, over-claiming its authored suffix", () => {
    // Generated line 0 (real length 13) col 10 -> App.vue (1,6): only three copied columns (10,11,12
    // -> authored 1:6,7,8) before the break. Generated line 1 col 0 -> App.vue (2,0); line 2 col 0 ->
    // App.vue (3,0). For range [0:10, 2:3) the authored end is bounded by the LAST content line
    // (generated line 2) -> authored (3,3). The intermediate generated line 0 is NOT bounded by its
    // real length: the multi-line authored span (1,6)..(3,3) claims authored line 1 from col 6
    // through its end, but only authored 1:6,7,8 were actually copied. If authored line 1 extends
    // past col 8, that suffix is over-claimed — accepted here because authored line lengths are
    // unavailable at this layer.
    const map = parseSourceMap(
      buildSourceMapJson(
        ["App.vue"],
        [[[10, 0, 1, 6]], [[0, 0, 2, 0]], [[0, 0, 3, 0]]],
        "App.vue.tsx",
      ),
    );
    expect(
      projectGeneratedRange(proj(map, [13, 5, 5]), {
        start: { line: 0, character: 10 },
        end: { line: 2, character: 3 },
      }),
    ).toEqual({
      source: "App.vue",
      range: { start: { line: 1, character: 6 }, end: { line: 3, character: 3 } },
    });
  });
});
