import { describe, it, expect } from "vitest";
import { encode, type SourceMapMappings } from "@jridgewell/sourcemap-codec";
import { CarrierMapper, CarrierMapperSet } from "./mapper";

/**
 * Discriminating tests for the STRICT fail-closed V3 carrier mapper. Every
 * negative case here is constructed so a naive "closest segment" mapper (the
 * playground's `tsxOffsetToVueOffset` heuristic: nearest segment + column
 * delta extrapolation) or a forward-snapping `LEAST_UPPER_BOUND` lookup WOULD
 * return a concrete (wrong) source offset — the strict mapper must return
 * `null` instead. The positive cases prove the mapper is not over-strict: a
 * position inside a mapped token maps to the EXACT source offset.
 */

/** Encode absolute decoded mappings into a real V3 `mappings` string. */
function v3(sources: string[], decoded: SourceMapMappings, sourcesContent?: (string | null)[]) {
  return {
    version: 3 as const,
    sources,
    names: [],
    mappings: encode(decoded),
    ...(sourcesContent ? { sourcesContent } : {}),
  };
}

/**
 * The MAIN fixture. Generated carrier text (line/col layout is load-bearing):
 *
 * ```text
 * line0: a block-comment helper prelude (12 chars) — synthetic, NO segments
 * line1: "foo(); bar();"    — [0 → A.vue L0C0] then a SOURCELESS closer at col 6
 * line2: "     tail"        — [5 → A.vue L1C0]
 * line3: ""                 — empty trailing line, no segments
 * ```
 *
 * Source `A.vue` = "foo();\ntail\n" (line0 "foo();", line1 "tail").
 */
const GEN_TEXT = "/* helper */\nfoo(); bar();\n     tail\n";
const SOURCE_A = "foo();\ntail\n";
const MAIN_MAP = v3(
  ["A.vue"],
  [
    [], // line0: no segments
    [
      [0, 0, 0, 0],
      [6], // sourceless closer: "foo();"'s generated extent ends at col 6
    ],
    [[5, 0, 1, 0]],
  ],
);

function mainMapper(): CarrierMapper {
  return new CarrierMapper({
    map: MAIN_MAP,
    generatedText: GEN_TEXT,
    readSourceText: (source) => (source === "A.vue" ? SOURCE_A : undefined),
  });
}

describe("CarrierMapper.mapGeneratedOffsetToSource — strict no-snap", () => {
  it("returns null for an offset on a generated line with NO segments (never snaps to another line)", () => {
    const m = mainMapper();
    // Offset 3 is inside the synthetic "/* helper */" prelude line. A
    // nearest-segment mapper snaps to line1's [0 → A.vue L0C0] mapping and
    // returns a concrete offset; the strict mapper fails closed.
    expect(m.mapGeneratedOffsetToSource(3)).toBeNull();
    // The empty trailing line (offset 37 = text end, line3) has no segments.
    expect(m.mapGeneratedOffsetToSource(37)).toBeNull();
  });

  it("returns null in the unmapped space AFTER a token (sourceless closer bounds the extent)", () => {
    const m = mainMapper();
    // Offset 21 = line1 col 8, inside " bar();" — the greatest-lower-bound
    // segment is the SOURCELESS closer at col 6. A closest-segment mapper
    // extrapolates from [0 → A.vue L0C0] with delta 8 and returns source
    // offset 8 (a position INSIDE "tail"'s line region — a mis-map); the
    // strict mapper returns null.
    expect(m.mapGeneratedOffsetToSource(21)).toBeNull();
    // Exactly AT the sourceless segment (col 6) is equally unmapped.
    expect(m.mapGeneratedOffsetToSource(19)).toBeNull();
  });

  it("returns null BEFORE the first segment on a line (LEAST_UPPER_BOUND forward snap is forbidden)", () => {
    const m = mainMapper();
    // Offset 29 = line2 col 2, in the indentation BEFORE the mapped "tail"
    // token at col 5. A LEAST_UPPER_BOUND (forward-snap) mapper returns the
    // col-5 segment's source position (offset 7); the strict mapper: null.
    expect(m.mapGeneratedOffsetToSource(29)).toBeNull();
  });

  it("returns null for out-of-range offsets", () => {
    const m = mainMapper();
    expect(m.mapGeneratedOffsetToSource(-1)).toBeNull();
    expect(m.mapGeneratedOffsetToSource(GEN_TEXT.length + 1)).toBeNull();
  });

  it("maps an offset INSIDE a mapped token to the EXACT source offset (not over-strict)", () => {
    const m = mainMapper();
    // Offset 15 = line1 col 2, inside "foo();" (extent [0, 6)): delta 2 →
    // A.vue line0 col 2 → source offset 2. Exact, not approximate.
    expect(m.mapGeneratedOffsetToSource(15)).toEqual({
      source: "A.vue",
      offset: 2,
      line: 1,
      column: 2,
    });
    // Offset 34 = line2 col 7, inside "tail" (delta 2) → A.vue line1 col 2 →
    // source offset 7 (line start) + 2 = 9.
    expect(m.mapGeneratedOffsetToSource(34)).toEqual({
      source: "A.vue",
      offset: 9,
      line: 2,
      column: 2,
    });
  });

  it("anti-extrapolation at the line tail: the exclusive end boundary maps, PAST it fails", () => {
    // CRLF fixture: generated "ab\r\ncd", line0 "ab" mapped from source line0.
    const m = new CarrierMapper({
      map: v3(["A.vue"], [[[0, 0, 0, 0]], [[0, 0, 1, 0]]]),
      generatedText: "ab\r\ncd",
      readSourceText: () => "ab\ncd",
    });
    // Col 2 is the exclusive one-past-last-char boundary of "ab" — a span end
    // there covers only mapped characters, so it maps (delta 2).
    expect(m.mapGeneratedOffsetToSource(2)).toEqual({
      source: "A.vue",
      offset: 2,
      line: 1,
      column: 2,
    });
    // Col 3 (the '\n' of the CRLF terminator) lies BEYOND the line's content
    // extent — a naive delta-extrapolating mapper would invent source col 3;
    // the strict mapper returns null.
    expect(m.mapGeneratedOffsetToSource(3)).toBeNull();
  });

  it("maps by UTF-16 code units, not bytes or code points (astral character)", () => {
    // Generated "\u{1F600}foo": the emoji is TWO UTF-16 code units, so "foo"
    // starts at genCol 2 (the segment column is UTF-16 per the V3 contract).
    const m = new CarrierMapper({
      map: v3(["A.vue"], [[[2, 0, 0, 0]]]),
      generatedText: "\u{1F600}foo\n",
      readSourceText: () => "foo\n",
    });
    // Offset 3 = the first 'o' (emoji occupies offsets 0-1, 'f' is 2). Delta
    // 1 → source offset 1. A UTF-8-byte mapper ('f' at byte 4) or a
    // code-point mapper ('f' at position 1) computes a different answer.
    expect(m.mapGeneratedOffsetToSource(3)).toEqual({
      source: "A.vue",
      offset: 1,
      line: 1,
      column: 1,
    });
    expect(m.mapGeneratedOffsetToSource(2)).toEqual({
      source: "A.vue",
      offset: 0,
      line: 1,
      column: 0,
    });
    // Inside the (unmapped) emoji, BEFORE the token: no snap forward.
    expect(m.mapGeneratedOffsetToSource(1)).toBeNull();
  });

  it("falls back to the map's own sourcesContent when no host source text is available", () => {
    const withContent = new CarrierMapper({
      map: v3(["A.vue"], [[[0, 0, 0, 0]]], [SOURCE_A]),
      generatedText: "foo();\n",
      // NO readSourceText — the embedded sourcesContent is the authority.
    });
    expect(withContent.mapGeneratedOffsetToSource(2)).toEqual({
      source: "A.vue",
      offset: 2,
      line: 1,
      column: 2,
    });
    // Without sourcesContent AND without a host reader there is no source
    // text to compute an offset against → fail closed.
    const withoutContent = new CarrierMapper({
      map: v3(["A.vue"], [[[0, 0, 0, 0]]]),
      generatedText: "foo();\n",
    });
    expect(withoutContent.mapGeneratedOffsetToSource(2)).toBeNull();
  });
});

describe("CarrierMapper.mapGeneratedSpanToSource — same-source endpoints or DROP", () => {
  /** Two sources on one generated line: "aabb" = A[0,2) + B[2,4). */
  function mixedMapper(): CarrierMapper {
    return new CarrierMapper({
      map: v3(
        ["A.vue", "B.vue"],
        [
          [
            [0, 0, 0, 0],
            [2, 1, 0, 0],
          ],
        ],
      ),
      generatedText: "aabb\n",
      readSourceText: (source) =>
        source === "A.vue" ? "aa\n" : source === "B.vue" ? "bb\n" : undefined,
    });
  }

  it("DROPS a span whose endpoints map to DIFFERENT sources (mixed-source)", () => {
    const m = mixedMapper();
    // Start (col 1) maps into A.vue, end (col 3) maps into B.vue. A naive
    // mapper returns a stitched span; the strict mapper drops the whole span.
    expect(m.mapGeneratedSpanToSource(1, 3)).toBeNull();
  });

  it("maps a same-source span exactly", () => {
    const m = mixedMapper();
    // [2, 4) is entirely B.vue: start col2 → B offset 0, end col4 (the line's
    // exclusive end boundary) → B offset 2.
    expect(m.mapGeneratedSpanToSource(2, 4)).toEqual({ source: "B.vue", start: 0, end: 2 });
  });

  it("maps a whole token ending at the line's exclusive end boundary", () => {
    const m = mainMapper();
    // [32, 36) covers exactly "tail" on line2 (cols 5..9, end at the EOL
    // boundary) → A.vue [7, 11).
    expect(m.mapGeneratedSpanToSource(32, 36)).toEqual({ source: "A.vue", start: 7, end: 11 });
  });

  // @ai-generated - Distinguishes an exact mapped-token end from included synthetic bytes.
  it("maps an exact token whose exclusive end begins a sourceless segment", () => {
    const m = mainMapper();
    // [13, 19) covers exactly `foo();`. Column 6 is also the start of the
    // sourceless ` bar();` extent, so point mapping at 19 remains null while
    // the span's end-exclusive boundary is valid.
    expect(m.mapGeneratedOffsetToSource(19)).toBeNull();
    expect(m.mapGeneratedSpanToSource(13, 19)).toEqual({
      source: "A.vue",
      start: 0,
      end: 6,
    });
    // Extending one code unit into synthetic text must still fail closed.
    expect(m.mapGeneratedSpanToSource(13, 20)).toBeNull();
  });

  it("maps a zero-length span (caret) at a mapped point", () => {
    const m = mainMapper();
    expect(m.mapGeneratedSpanToSource(15, 15)).toEqual({ source: "A.vue", start: 2, end: 2 });
  });

  it("DROPS a span when either endpoint is unmappable, and an inverted span", () => {
    const m = mainMapper();
    // End in the unmapped " bar();" region → whole span dropped.
    expect(m.mapGeneratedSpanToSource(15, 21)).toBeNull();
    // Start on the segment-less prelude line → dropped.
    expect(m.mapGeneratedSpanToSource(3, 15)).toBeNull();
    // Inverted input range → dropped.
    expect(m.mapGeneratedSpanToSource(17, 15)).toBeNull();
  });
});

describe("CarrierMapper.mapWorkspaceEditToSource — atomic all-or-nothing", () => {
  it("suppresses the WHOLE edit when ANY span fails to map", () => {
    const m = mainMapper();
    // First span is perfectly mappable; second lands in the unmapped
    // " bar();" region. A partially-applied edit is forbidden → null.
    expect(
      m.mapWorkspaceEditToSource([
        { start: 15, end: 17 },
        { start: 21, end: 23 },
      ]),
    ).toBeNull();
  });

  it("maps an edit whose every span maps", () => {
    const m = mainMapper();
    expect(
      m.mapWorkspaceEditToSource([
        { start: 15, end: 17 },
        { start: 32, end: 36 },
      ]),
    ).toEqual([
      { source: "A.vue", start: 2, end: 4 },
      { source: "A.vue", start: 7, end: 11 },
    ]);
  });
});

describe("CarrierMapperSet — per-carrier keying", () => {
  it("returns the carrier's own mapper (path-normalized) and undefined for an unknown carrier", () => {
    const set = new CarrierMapperSet();
    const m = mainMapper();
    set.set("d:/ws/A.vue.tsx", m);
    expect(set.forCarrier("d:/ws/A.vue.tsx")).toBe(m);
    // Backslash spelling resolves to the same mapper.
    expect(set.forCarrier("d:\\ws\\A.vue.tsx")).toBe(m);
    // Unknown carrier → undefined (the caller drops the result).
    expect(set.forCarrier("d:/ws/Unknown.vue.tsx")).toBeUndefined();
  });

  it("suppresses a cross-file edit when ANY file's carrier is unknown or any span fails", () => {
    const set = new CarrierMapperSet();
    set.set("d:/ws/A.vue.tsx", mainMapper());
    // Unknown carrier file in the edit → the WHOLE edit is suppressed.
    expect(
      set.mapWorkspaceEditToSource([
        { carrierPath: "d:/ws/A.vue.tsx", spans: [{ start: 15, end: 17 }] },
        { carrierPath: "d:/ws/Unknown.vue.tsx", spans: [{ start: 0, end: 1 }] },
      ]),
    ).toBeNull();
    // Unmappable span in a known carrier → suppressed too.
    expect(
      set.mapWorkspaceEditToSource([
        {
          carrierPath: "d:/ws/A.vue.tsx",
          spans: [
            { start: 15, end: 17 },
            { start: 21, end: 23 },
          ],
        },
      ]),
    ).toBeNull();
  });

  it("maps a fully-mappable cross-file edit", () => {
    const set = new CarrierMapperSet();
    set.set("d:/ws/A.vue.tsx", mainMapper());
    expect(
      set.mapWorkspaceEditToSource([
        { carrierPath: "d:/ws/A.vue.tsx", spans: [{ start: 32, end: 36 }] },
      ]),
    ).toEqual([
      { carrierPath: "d:/ws/A.vue.tsx", spans: [{ source: "A.vue", start: 7, end: 11 }] },
    ]);
  });
});

describe("CarrierMapper.mapSourceOffsetToGenerated — strict fail-closed forward mapping", () => {
  // The forward (source → generated) direction the in-context LanguageService
  // hosts use to translate an editor (carrier-source) offset into the
  // generated-carrier offset a provider query is issued at. The SAME
  // strictness contract as the backward direction: greatest-lower-bound on
  // the source side, generated-extent bounded, never a nearest-segment /
  // biased snap, fail closed (`null`) in unmapped source space.

  it("maps an offset INSIDE a mapped token to the EXACT generated offset", () => {
    const m = mainMapper();
    // A.vue offset 2 = source line0 col2, inside "foo();" ([0 → gen L1C0],
    // generated extent [0, 6)): delta 2 → generated line1 col2 → offset 15.
    expect(m.mapSourceOffsetToGenerated(2)).toEqual({ offset: 15, line: 2, column: 2 });
    // A.vue offset 9 = source line1 col2, inside "tail" ([0 → gen L2C5]):
    // delta 2 → generated line2 col7 → offset 34.
    expect(m.mapSourceOffsetToGenerated(9)).toEqual({ offset: 34, line: 3, column: 7 });
  });

  it("returns null past the mapped token's generated extent (no delta extrapolation)", () => {
    const m = mainMapper();
    // A.vue offset 6 would be source line0 col6 — the "foo();" segment's
    // generated extent is [0, 6) closed by the sourceless segment at gen
    // col6, so col6 no longer maps through it. A closest-segment forward
    // mapper extrapolates to generated col6; the strict mapper: null.
    expect(m.mapSourceOffsetToGenerated(6)).toBeNull();
  });

  it("returns null on a source line with no mapped segments (never snaps across lines)", () => {
    // Source has a line2 ("rest") that no segment maps.
    const m = new CarrierMapper({
      map: v3(["A.vue"], [[[0, 0, 0, 0]]]),
      generatedText: "foo();\n",
      readSourceText: (s) => (s === "A.vue" ? "foo();\nbar();\n" : undefined),
    });
    // Offset 8 is on source line1 ("bar();") which has NO segments. A
    // nearest-line mapper snaps to line0's segment; strict: null.
    expect(m.mapSourceOffsetToGenerated(8)).toBeNull();
  });

  it("returns null BEFORE the first mapped source column on a line (no forward snap)", () => {
    const m = new CarrierMapper({
      map: v3(["A.vue"], [[[0, 0, 0, 4]]]),
      generatedText: "tail\n",
      readSourceText: (s) => (s === "A.vue" ? "    tail\n" : undefined),
    });
    // Source col2 sits in the indentation BEFORE the mapped "tail" at col4.
    expect(m.mapSourceOffsetToGenerated(2)).toBeNull();
    // Inside the token maps exactly: col5 → delta 1 → generated offset 1.
    expect(m.mapSourceOffsetToGenerated(5)).toEqual({ offset: 1, line: 1, column: 1 });
  });

  it("resolves duplicate emissions to the EARLIEST generated position deterministically", () => {
    // One source token emitted twice (template bindings are re-emitted):
    // both gen L0C0 and gen L1C4 map from A.vue L0C0.
    const m = new CarrierMapper({
      map: v3(["A.vue"], [[[0, 0, 0, 0]], [[4, 0, 0, 0]]]),
      generatedText: "msg;\nuse msg;\n",
      readSourceText: (s) => (s === "A.vue" ? "msg\n" : undefined),
    });
    expect(m.mapSourceOffsetToGenerated(1)).toEqual({ offset: 1, line: 1, column: 1 });
  });

  // @ai-generated - Proves linked projections retain both the original run and a narrower alias.
  it("enumerates every containing generated run for an authored source position", () => {
    const m = new CarrierMapper({
      map: v3(["A.vue"], [[[0, 0, 0, 0]], [[4, 0, 0, 6], [14]]]),
      generatedText: "const typedValue\n    typedValue }\n",
      readSourceText: (source) => (source === "A.vue" ? "const typedValue\n" : undefined),
    });
    expect(m.mapSourceOffsetToGeneratedAll(6)).toEqual([
      { offset: 6, line: 1, column: 6 },
      { offset: 21, line: 2, column: 4 },
    ]);
    expect(m.mapSourceOffsetToGeneratedAll(18)).toEqual([]);
  });

  it("fails closed for out-of-range offsets and unknown sources", () => {
    const m = mainMapper();
    expect(m.mapSourceOffsetToGenerated(-1)).toBeNull();
    expect(m.mapSourceOffsetToGenerated(SOURCE_A.length + 1)).toBeNull();
    expect(m.mapSourceOffsetToGenerated(2, "Other.vue")).toBeNull();
  });

  it("requires an explicit source name when the map carries multiple sources", () => {
    const m = new CarrierMapper({
      map: v3(
        ["A.vue", "B.vue"],
        [
          [
            [0, 0, 0, 0],
            [11, 1, 0, 0],
          ],
        ],
      ),
      generatedText: "aaaaaaaaaa bbbb\n",
      readSourceText: (s) => (s === "A.vue" ? "aaaa\n" : s === "B.vue" ? "bbbb\n" : undefined),
    });
    // Ambiguous without a name → fail closed.
    expect(m.mapSourceOffsetToGenerated(1)).toBeNull();
    // Explicit per-source queries map exactly.
    expect(m.mapSourceOffsetToGenerated(1, "A.vue")).toEqual({ offset: 1, line: 1, column: 1 });
    expect(m.mapSourceOffsetToGenerated(1, "B.vue")).toEqual({ offset: 12, line: 1, column: 12 });
  });
});
