// The chaining algebra, the write grammar, the boundary rules, and the output
// artifact schema.
//
// Covers LAYER 1 §2.1–§2.6, §5.1–§5.8, §6.1–§6.5, §7.1–§7.7, §8 and §11.4 —
// with the cases layer 1's own text calls out as load-bearing given their own
// tests: equal-coordinate ordering, sourceless-barrier semantics, the mapless
// present fragment (§5.8), and every row of §6.4's BR-5 case table including
// case 4′, the empty present fragment.

import { describe, expect, it } from "vitest";

import {
  chainScriptFragment,
  composeAssembledVueMainModule,
  MalformedAssembleInputError,
} from "../src/assembled-map-composition-reference.mjs";
import { lineTable, payloadAt, SegmentIndex, UNMAPPED } from "../src/assembled-map-coordinates.mjs";
import {
  buildChunks,
  chainThroughChunks,
  RENAME_PASS,
  runScriptRewritePasses,
} from "../src/assembled-map-rewrite.mjs";
import { assembleModule } from "../src/assembled-map-write-grammar.mjs";
import { encodeMappings } from "../src/assembled-map-wire.mjs";
import { decodeMappings } from "../src/sourcemap.mjs";

const segment = (
  genLine,
  genCol,
  srcLine = null,
  srcCol = null,
  srcIdx = null,
  nameIdx = null,
) => ({
  genLine,
  genCol,
  srcIdx: srcLine === null ? null : (srcIdx ?? 0),
  srcLine,
  srcCol,
  nameIdx,
});

const sourceless = (genLine, genCol) => segment(genLine, genCol);

function mapWith(segments, extra = {}) {
  return JSON.stringify({
    version: 3,
    sources: ["Comp.vue"],
    names: [],
    ...extra,
    mappings: encodeMappings(segments),
  });
}

function input(overrides) {
  return {
    canonicalId: "Comp.vue",
    styleCount: 0,
    customBlockCount: 0,
    styleLangs: [],
    customTypes: [],
    script: null,
    template: null,
    scopeId: "",
    runtimeModuleName: null,
    isProduction: false,
    ssr: false,
    ssrModuleId: null,
    emitSsrModuleRegistration: true,
    hmrStrategy: "none",
    sourceMapRequested: true,
    authored: { script: true, template: true },
    ...overrides,
  };
}

// §5 — the chain operation

describe("§5.3 / §5.5 — the chain operation", () => {
  it("§5.5 worked case: N coincident segments at a replaced range beginning at column 0", () => {
    // "Let the script code begin with `__sfc__` at offset 0 and let `M` declare
    // three segments at (0,0)… The answer is unique: two segments, (0,0) and
    // (0,9)." The (0,0) segment carries the THIRD input segment's payload,
    // because `resolveAt` takes the last applicable.
    const result = chainScriptFragment("__sfc__ = 1\n", [
      segment(0, 0, 1, 0),
      segment(0, 0, 2, 0),
      segment(0, 0, 3, 0),
    ]);
    expect(result.code).toBe("_sfc_main = 1\n");
    expect(result.segments).toEqual([segment(0, 0, 3, 0), segment(0, 9, 3, 0)]);
  });

  it("§5.5 rule 2: two DISTINCT segments strictly inside a rename range are both dropped", () => {
    // `const __sfc__ = 1` — segments at columns 7 and 9 are strictly inside
    // `[6,13)`. Exactly one `Overwritten` segment is emitted, carrying
    // `lookup(6)`, plus the resume segment.
    //
    // A DROPPED SEGMENT STILL PARTICIPATES IN `lookup`. §5.3 defines
    // `lookup(o) = resolveAt(M, pos(o))` over `M`, the pass's INPUT sequence —
    // rules (b)/(c) drop segments from EMISSION, not from `M`. So the resume
    // segment at old offset 13 resolves to the dropped (0,9), not to (0,0).
    // V1's own derivation depends on exactly this: its overwrite token takes
    // its payload from the (0,6) segment the same overwrite dropped.
    const result = chainScriptFragment("const __sfc__ = 1\n", [
      segment(0, 0, 1, 0),
      segment(0, 7, 1, 7),
      segment(0, 9, 1, 9),
    ]);
    expect(result.segments).toEqual([
      segment(0, 0, 1, 0),
      segment(0, 6, 1, 0), // the replacement segment: lookup(6) is (0,0)
      segment(0, 15, 1, 9), // the resume segment: lookup(13) is the dropped (0,9)
    ]);
  });

  it("§5.3(c) — a MID-LINE removal's resume segment fires at the removal's generated position", () => {
    // Pass 2's pattern removed from the middle of a line is impossible (its
    // pattern ends with LF), so the mid-line case is exercised through pass 1's
    // geometry: the chunk after a replacement resumes at its own start.
    const result = chainScriptFragment("a __sfc__ b\n", [segment(0, 0, 1, 0)]);
    expect(result.code).toBe("a _sfc_main b\n");
    expect(result.segments).toEqual([
      segment(0, 0, 1, 0),
      segment(0, 2, 1, 0),
      segment(0, 11, 1, 0),
    ]);
  });

  it("§5.3 — MULTIPLE same-line rename replacements each get their own segment pair", () => {
    const result = chainScriptFragment("__sfc__ __sfc__\n", [segment(0, 0, 1, 0)]);
    expect(result.code).toBe("_sfc_main _sfc_main\n");
    expect(result.segments).toEqual([
      segment(0, 0, 1, 0), // replacement 1
      segment(0, 9, 1, 0), // resume after replacement 1
      segment(0, 10, 1, 0), // replacement 2
      segment(0, 19, 1, 0), // resume after replacement 2
    ]);
  });

  it("§5.3(a) — a SOURCE-BEARING old-end transition", () => {
    // The resume segment carries `lookup(chunk start)`, source-bearing whenever
    // an applicable segment exists on that line at or before it.
    const result = chainScriptFragment("const __sfc__ = {}\n", [segment(0, 0, 4, 2)]);
    expect(result.segments[result.segments.length - 1]).toEqual(segment(0, 15, 4, 2));
  });

  it("§5.4 — the barrier holds against a source-bearing segment FURTHER RIGHT on the line", () => {
    // `resolveAt` is line-scoped and takes the last applicable at or BEFORE the
    // column; a segment to the right is not applicable.
    const result = chainScriptFragment("__sfc__ x\n", [segment(0, 8, 1, 8)]);
    // `lookup(0)` finds nothing at or before column 0 → sourceless.
    expect(result.segments[0]).toEqual(sourceless(0, 0));
  });

  it("§5.4 — the barrier does not fall through to a PREVIOUS line", () => {
    const result = chainScriptFragment("const a = 1\n__sfc__\n", [segment(0, 0, 1, 0)]);
    // The replacement is on line 1, whose only candidate would be line 0's —
    // and the lookup is line-scoped, so BOTH the replacement segment and the
    // resume segment after it are sourceless.
    expect(result.segments).toEqual([segment(0, 0, 1, 0), sourceless(1, 0), sourceless(1, 9)]);
  });

  it("§5.3(d) — rule (d) is ALWAYS live, including for a fragment not ending in LF", () => {
    // `off(0, 11)` on `"const x = 1"` is the end position; it is covered by no
    // chunk and would otherwise be silently dropped. An implementation that
    // guards rule (d) on a trailing LF loses it.
    const result = chainScriptFragment("const x = 1", [segment(0, 0, 1, 0), segment(0, 11, 1, 11)]);
    expect(result.segments).toEqual([segment(0, 0, 1, 0), segment(0, 11, 1, 11)]);
  });

  it("§5.1 / §5.3(d) — rule (d) fires with an EMPTY chunk list (empty fragment code)", () => {
    const result = chainScriptFragment("", [segment(0, 0, 1, 0)]);
    expect(result.code).toBe("");
    expect(result.segments).toEqual([segment(0, 0, 1, 0)]);
  });

  it("§5.3 — a TERMINAL removal produces no transition segment; a non-terminal one does", () => {
    const terminal = chainScriptFragment("export default __sfc__;\n", [segment(0, 0, 1, 0)]);
    expect(terminal.code).toBe("");
    expect(terminal.segments).toEqual([]);
    const nonTerminal = chainScriptFragment("export default __sfc__;\nx\n", [segment(0, 0, 1, 0)]);
    expect(nonTerminal.code).toBe("x\n");
    expect(nonTerminal.segments).toEqual([sourceless(0, 0)]);
  });

  it("§5.1 — a pass with ZERO occurrences is still a pass, and is the identity on M", () => {
    const result = chainScriptFragment("const x = 1\n", [segment(0, 0, 1, 0), segment(0, 6, 1, 6)]);
    expect(result.code).toBe("const x = 1\n");
    expect(result.segments).toEqual([segment(0, 0, 1, 0), segment(0, 6, 1, 6)]);
  });

  it("§2.5 — matching is NOT identifier-aware: `___sfc__` is rewritten at offset 1", () => {
    const result = chainScriptFragment("___sfc__\n", [segment(0, 0, 1, 0)]);
    expect(result.code).toBe("__sfc_main\n");
  });

  it("§5.3 — a chained segment carries the looked-up segment's `nameIdx` unchanged", () => {
    const withName = { ...segment(0, 6, 1, 6), nameIdx: 0 };
    const result = chainScriptFragment("const __sfc__ = {}\n", [withName]);
    expect(result.segments.map((entry) => entry.nameIdx)).toEqual([0, 0]);
  });

  it("§5.5 rule 1 — the resume segment is SUPPRESSED when a coincident input segment exists", () => {
    // The `Original` chunk resuming at offset 7 is both a replaced range's end
    // AND a declared segment position. `lookup(7)` IS that segment, so the
    // resume segment would be a byte-identical duplicate; §5.3's payload
    // precedence drops it. Without suppression there would be three segments.
    const result = chainScriptFragment("__sfc__x\n", [segment(0, 7, 1, 7)]);
    expect(result.segments).toEqual([sourceless(0, 0), segment(0, 9, 1, 7)]);
  });

  it("§5.5 rule 1 — ONE pass emits coincident segments in `M`'s order", () => {
    // This is asserted at SINGLE-PASS granularity deliberately. A wrong
    // coincident-ordering rule is applied at BOTH passes of the script chain,
    // so it self-cancels end to end: V3 and every other two-pass vector stay
    // green against an implementation that reverses coincident groups. Only an
    // odd number of applications is observable, and one pass is the smallest.
    const text = "const x = 1\n";
    const segments = [segment(0, 0, 5, 5), segment(0, 0, 1, 0), segment(0, 0, 9, 9)];
    const chained = chainThroughChunks(
      text,
      buildChunks(text, RENAME_PASS.pattern, RENAME_PASS.replacement),
      segments,
      "Script",
    );
    expect(chained.map((entry) => entry.srcLine)).toEqual([5, 1, 9]);
  });

  it("§5.3(d) — ONE pass emits coincident END-POSITION segments in `M`'s order", () => {
    // Rule (d)'s emission is a separate code path from rule (a)'s carry, so it
    // needs its own single-pass order assertion.
    const text = "const x = 1";
    const segments = [segment(0, 11, 5, 5), segment(0, 11, 1, 0)];
    const chained = chainThroughChunks(
      text,
      buildChunks(text, RENAME_PASS.pattern, RENAME_PASS.replacement),
      segments,
      "Script",
    );
    expect(chained.map((entry) => entry.srcLine)).toEqual([5, 1]);
  });

  it("§5.5 rule 5 — the TEMPLATE's coincident segments keep their order through placement", () => {
    // The template is never chained (§5.7), so its ordering rides the placement
    // path alone and needs its own assertion.
    const result = composeAssembledVueMainModule(
      input({
        script: null,
        template: {
          code: "function render() {}\n",
          imports: [],
          ssrImports: [],
          sourceMap: mapWith([segment(0, 9, 5, 5), segment(0, 9, 1, 0)]),
        },
        authored: { script: false, template: true },
      }),
    );
    expect(result.segments.map((entry) => entry.srcLine)).toEqual([5, 1, null]);
  });

  it("§5.5 rule 4 — coincident segments keep their order across the rename → removal chain", () => {
    const result = chainScriptFragment("x __sfc__\n", [segment(0, 0, 5, 5), segment(0, 0, 1, 0)]);
    expect(result.code).toBe("x _sfc_main\n");
    expect(result.segments).toEqual([
      segment(0, 0, 5, 5),
      segment(0, 0, 1, 0),
      segment(0, 2, 1, 0), // the replacement: lookup(2) is the LAST coincident
      segment(0, 11, 1, 0), // the resume
    ]);
  });

  it("§11.2 `DECISION` D-3 — no first-chunk own-start token and no interior line-start tokens", () => {
    // `CodeTransform` emits a token at the first chunk's own start and at each
    // interior LF boundary. Both are omitted: they are provably inert under
    // §2.6's standard. Lines 0 and 1 here carry NO segment at all.
    const result = chainScriptFragment("a\nb\n__sfc__\n", []);
    expect(result.code).toBe("a\nb\n_sfc_main\n");
    expect(result.segments).toEqual([sourceless(2, 0), sourceless(2, 9)]);
  });

  it("§5.3 — a genuine MID-LINE removal: pass 2's pattern starting mid-line merges the lines", () => {
    // `[2,28)` is removed, so the resume segment lands at generated (0,2) —
    // mid-line — and the following line merges into the first.
    const barrier = chainScriptFragment("x export default _sfc_main;\ny\n", [segment(0, 0, 1, 0)]);
    expect(barrier.code).toBe("x y\n");
    // `lookup(28)` is `pos = (1,0)`, whose line declares nothing → sourceless.
    expect(barrier.segments).toEqual([segment(0, 0, 1, 0), sourceless(0, 2)]);

    // With a segment declared exactly at the removal's end, that segment is
    // CARRIED to the merged position and the resume is suppressed.
    const carried = chainScriptFragment("x export default _sfc_main;\ny\n", [
      segment(0, 0, 1, 0),
      segment(1, 0, 2, 0),
    ]);
    expect(carried.segments).toEqual([segment(0, 0, 1, 0), segment(0, 2, 2, 0)]);
  });

  it("§2.1 — a CR retained before a LF occupies a real column (V6's own recorded gap)", () => {
    // V6 is non-discriminating for CR handling by its own `knownGaps`. This
    // asserts a CR-SENSITIVE coordinate: `lineTable("ab\\r\\n")[0]` is `"ab\\r"`,
    // length 3, so the end-of-line column 3 is in-bounds. Stripping the CR would
    // make it length 2 and this input would be rejected as `U7.2`.
    const result = composeAssembledVueMainModule(
      input({
        script: { code: "ab\r\n", sourceMap: mapWith([segment(0, 3, 1, 3)]) },
        template: null,
        authored: { script: true, template: false },
      }),
    );
    expect(result.outcome).toBe("composed");
    expect(result.segments[0]).toEqual(segment(0, 3, 1, 3));
  });

  it("§8 — provenance survives rewriting and chaining, and is never a serialized member", () => {
    const result = chainScriptFragment("const __sfc__ = {}\n", [segment(0, 0, 1, 0)]);
    expect(result.provenance).toEqual(["Script", "Script", "Script"]);
    for (const entry of result.segments) expect(Object.hasOwn(entry, "origin")).toBe(false);
  });
});

describe("§5.7 — the template fragment is not rewritten", () => {
  it("a template whose code contains `__sfc__` is written verbatim and placed directly", () => {
    const result = composeAssembledVueMainModule(
      input({
        script: null,
        template: {
          code: "const __sfc__ = 1\n",
          imports: [],
          ssrImports: [],
          sourceMap: mapWith([segment(0, 6, 1, 6)]),
        },
        authored: { script: false, template: true },
      }),
    );
    expect(result.code).toContain("const __sfc__ = 1\n");
    // Placed by lineOffset only — no rename, so no replacement or resume
    // segment, and the column is unchanged.
    expect(result.segments[0]).toEqual({ ...segment(0, 6, 1, 6), genLine: 2 });
  });
});

// §5.8 — a present fragment whose map is legitimately absent

describe("§5.8 — the mapless present fragment", () => {
  const maplessScript = input({
    script: { code: "const __sfc__ = {}\nexport default __sfc__;\n", sourceMap: "" },
    template: {
      code: "function render() {}\n",
      imports: [],
      ssrImports: [],
      sourceMap: mapWith([segment(0, 9, 9, 2)]),
    },
    authored: { script: false, template: true },
  });

  it("its code is still written and still rewritten by both passes", () => {
    const result = composeAssembledVueMainModule(maplessScript);
    expect(result.code.startsWith("const _sfc_main = {}\n")).toBe(true);
    expect(result.code).not.toContain("__sfc__");
  });

  it("it contributes NOTHING to the map — no segments, no rows, and no BR-3 boundary segment", () => {
    const result = composeAssembledVueMainModule(maplessScript);
    // Only the template's carried segment and the template's own BR-3 segment.
    expect(result.provenance).toEqual(["Template", "AssemblyBoundary"]);
    expect(result.map.sources).toEqual(["Comp.vue"]);
    // The script's line range (module lines 0–1) carries no segment at all: a
    // validated-but-empty `M` would have sprouted a sourceless segment at every
    // replacement and every resume position.
    expect(result.segments.every((entry) => entry.genLine >= 2)).toBe(true);
  });

  it("with ZERO contributing maps a requested map is still produced, empty (§7.7)", () => {
    const result = composeAssembledVueMainModule(
      input({
        script: { code: "const __sfc__ = {}\n", sourceMap: "" },
        template: null,
        authored: { script: false, template: false },
      }),
    );
    expect(result.map).toEqual({ version: 3, names: [], sources: [], mappings: "" });
    expect(Object.hasOwn(result.map, "sourceRoot")).toBe(false);
    expect(Object.hasOwn(result.map, "sourcesContent")).toBe(false);
    expect(Object.hasOwn(result.map, "ignoreList")).toBe(false);
  });
});

// §6.4 — the boundary rules

/**
 * §6.4 BR-5, checked over an assembled module rather than asserted: NO generated
 * position addressed by an assembly-owned byte resolves to a source-bearing
 * segment.
 *
 * A position `(line, column)` addresses a byte iff `column < line.length`; an
 * LF occupies no column (§2.1, case 2). Fragment-owned positions are computed
 * from each fragment's placement and final code — the same derivation §6.3
 * uses — and every other addressed position must be `Unmapped`.
 */
function assertBr5(result, fragments) {
  const owned = new Set();
  for (const { placement, finalCode } of fragments) {
    if (placement === null) continue;
    const lines = lineTable(finalCode);
    for (let line = 0; line < lines.length; line += 1) {
      const base = line === 0 ? placement.columnOffset : 0;
      for (let column = 0; column < lines[line].length; column += 1) {
        owned.add(`${placement.lineOffset + line}:${base + column}`);
      }
    }
  }
  const index = new SegmentIndex(result.segments);
  const moduleLines = lineTable(result.code);
  let checked = 0;
  for (let line = 0; line < moduleLines.length; line += 1) {
    for (let column = 0; column < moduleLines[line].length; column += 1) {
      if (owned.has(`${line}:${column}`)) continue;
      checked += 1;
      expect(payloadAt(index, line, column), `assembly-owned byte at ${line}:${column}`).toBe(
        UNMAPPED,
      );
    }
  }
  // The check must actually have visited assembly-owned bytes.
  expect(checked).toBeGreaterThan(0);
}

describe("§6.4 — BR-3 and the BR-5 case table", () => {
  it("BR-3's WITNESS (§6.4): a source-bearing segment on the fragment's trailing empty line", () => {
    // "Take a contributing fragment whose code ends with LF … whose map
    // declares one source-bearing segment at (1, 0), the trailing empty line."
    const template = {
      code: "function render() {}\n",
      imports: [],
      ssrImports: [],
      sourceMap: mapWith([segment(1, 0, 7, 3)]),
    };
    const result = composeAssembledVueMainModule(
      input({ script: null, template, authored: { script: false, template: true } }),
    );
    // W-08 writes `const _sfc_main = {}\n`, W-10 writes `\n`, so the template
    // sits at line 2 and its trailing empty line is module line 3.
    expect(result.segments).toEqual([{ ...segment(1, 0, 7, 3), genLine: 3 }, sourceless(3, 0)]);
    expect(result.provenance).toEqual(["Template", "AssemblyBoundary"]);

    // WITH the boundary segment, `payloadAt` is Unmapped at every column of
    // line 3, including the columns the next B write's bytes occupy.
    const withBoundary = new SegmentIndex(result.segments);
    const withoutBoundary = new SegmentIndex(result.segments.slice(0, 1));
    for (const column of [0, 5, 24]) {
      expect(payloadAt(withBoundary, 3, column)).toBe(UNMAPPED);
      // …and WITHOUT it, BR-5 is false: the carried segment wins everywhere.
      expect(payloadAt(withoutBoundary, 3, column)).toEqual({
        srcIdx: 0,
        srcLine: 7,
        srcCol: 3,
        nameIdx: null,
      });
    }
    assertBr5(result, [
      { placement: { lineOffset: 2, columnOffset: 0 }, finalCode: template.code },
    ]);
  });

  it("case 3 — a fragment NOT ending in LF gets no boundary segment", () => {
    const template = {
      code: "function render() {}",
      imports: [],
      ssrImports: [],
      sourceMap: mapWith([segment(0, 9, 9, 2)]),
    };
    const result = composeAssembledVueMainModule(
      input({ script: null, template, authored: { script: false, template: true } }),
    );
    expect(result.provenance).toEqual(["Template"]);
    // W-12 fires, so the next B write lands on line 3 — outside the fragment's
    // coordinate space, whose maximum line is 2.
    assertBr5(result, [
      { placement: { lineOffset: 2, columnOffset: 0 }, finalCode: template.code },
    ]);
  });

  it("case 4′ — the EMPTY present fragment gets NO boundary segment, and firing would be destructive", () => {
    // `lineTable("")` is `[""]`: one line, one in-bounds position (0,0), which
    // §5.3(d) carries. The cursor is at column 0 — so a "cursor column is zero"
    // predicate would fire — but `"".ends_with('\n')` is false, so BR-3 does
    // not, and must not: the boundary segment would land at the SAME
    // coordinate, after the carried segment, and shadow it.
    const result = composeAssembledVueMainModule(
      input({
        script: { code: "", sourceMap: mapWith([segment(0, 0, 4, 2)]) },
        template: null,
        authored: { script: true, template: false },
      }),
    );
    expect(result.segments).toEqual([segment(0, 0, 4, 2)]);
    expect(result.provenance).toEqual(["Script"]);
    // The faithfully composed authored position stays observable.
    expect(payloadAt(new SegmentIndex(result.segments), 0, 0)).toEqual({
      srcIdx: 0,
      srcLine: 4,
      srcCol: 2,
      nameIdx: null,
    });
    // W-07 fires (the code does not end with LF), so the next B write begins at
    // line 1 and NO assembly-owned byte occupies any column on line 0.
    expect(lineTable(result.code)[0]).toBe("");
    assertBr5(result, [{ placement: { lineOffset: 0, columnOffset: 0 }, finalCode: "" }]);
  });

  it("case 4′ is constructible from pass 2 itself: a `C₁` that is exactly the removal pattern", () => {
    const rewritten = runScriptRewritePasses("export default __sfc__;\n", null, "Script");
    expect(rewritten.code).toBe("");
  });

  it("BR-4 — no fragment-START boundary segment competes with the fragment's own first segment", () => {
    const result = composeAssembledVueMainModule(
      input({
        script: { code: "const x = 1\n", sourceMap: mapWith([segment(0, 0, 1, 0)]) },
        template: null,
        authored: { script: true, template: false },
      }),
    );
    expect(result.segments[0]).toEqual(segment(0, 0, 1, 0));
    expect(result.provenance[0]).toBe("Script");
  });

  it("BR-5 holds over a full two-fragment module with every write site populated", () => {
    const script = { code: "const __sfc__ = {}\n", sourceMap: mapWith([segment(0, 6, 1, 6)]) };
    const template = {
      code: "function render() {}\n",
      imports: ["createElementVNode"],
      ssrImports: [],
      sourceMap: mapWith([segment(0, 9, 9, 2)]),
    };
    const dto = input({
      canonicalId: "src/Comp.vue",
      styleCount: 1,
      customBlockCount: 1,
      customTypes: ["i18n"],
      script,
      template,
      hmrStrategy: "vite",
    });
    const result = composeAssembledVueMainModule(dto);
    const rewritten = runScriptRewritePasses(script.code, null, "Script").code;
    const assembled = assembleModule(dto, rewritten);
    assertBr5(result, [
      { placement: assembled.scriptPlacement, finalCode: rewritten },
      { placement: assembled.templatePlacement, finalCode: template.code },
    ]);
  });
});

describe("§6.5 — fragment line ranges are disjoint", () => {
  it("no module line carries segments from both fragments", () => {
    const result = composeAssembledVueMainModule(
      input({
        script: { code: "const __sfc__ = {}\n", sourceMap: mapWith([segment(0, 6, 1, 6)]) },
        template: {
          code: "function render() {}\n",
          imports: [],
          ssrImports: [],
          sourceMap: mapWith([segment(0, 9, 9, 2)]),
        },
      }),
    );
    const scriptLines = new Set();
    const templateLines = new Set();
    result.segments.forEach((entry, index) => {
      if (result.provenance[index] === "Script") scriptLines.add(entry.genLine);
      if (result.provenance[index] === "Template") templateLines.add(entry.genLine);
    });
    for (const line of templateLines) expect(scriptLines.has(line)).toBe(false);
    // §5.5 rule 5 — every placed script segment precedes every placed template
    // segment, and the sequence is non-decreasing.
    for (let i = 1; i < result.segments.length; i += 1) {
      const previous = result.segments[i - 1];
      const current = result.segments[i];
      expect(
        current.genLine > previous.genLine ||
          (current.genLine === previous.genLine && current.genCol >= previous.genCol),
      ).toBe(true);
    }
  });
});

// §6.2 / §6.3 — the write grammar and placement

describe("§6.2 — the exact byte grammar", () => {
  it("W-01…W-16 with a script, a template, a style, a custom block and vite HMR", () => {
    const dto = input({
      canonicalId: "src/Comp.vue",
      styleCount: 1,
      customBlockCount: 1,
      styleLangs: [],
      customTypes: ["i18n"],
      script: {
        code: "const __sfc__ = {}\nexport default __sfc__;\n",
        sourceMap: mapWith([segment(0, 6, 1, 6)]),
      },
      template: {
        code: "function render() {}\n",
        imports: ["createElementVNode", "_ctx"],
        ssrImports: [],
        sourceMap: mapWith([segment(0, 9, 9, 2)]),
      },
      hmrStrategy: "vite",
    });
    const result = composeAssembledVueMainModule(dto);
    expect(result.code).toBe(
      'import "src/Comp.vue?vue&type=style&index=0&lang.css"\n' +
        'import block0 from "src/Comp.vue?vue&type=i18n&index=0"\n' +
        "\n" +
        "const _sfc_main = {}\n" +
        'import { createElementVNode, ctx as _ctx } from "vue"\n' +
        "\n" +
        "function render() {}\n" +
        "_sfc_main.render = render\n" +
        "if (typeof block0 === 'function') block0(_sfc_main)\n" +
        '_sfc_main.__file = "src/Comp.vue"\n' +
        "/* HMR(vite) */\n" +
        "if (import.meta.hot) { import.meta.hot.accept(() => {}) }\n" +
        "export default _sfc_main",
    );
    // §6.3 — placement DERIVED from the write cursor, never supplied.
    const assembled = assembleModule(dto, "const _sfc_main = {}\n");
    expect(assembled.scriptPlacement).toEqual({ lineOffset: 3, columnOffset: 0 });
    expect(assembled.templatePlacement).toEqual({ lineOffset: 6, columnOffset: 0 });
    // …and the segments land accordingly.
    expect(result.segments).toEqual([
      segment(3, 6, 1, 6),
      segment(3, 15, 1, 6),
      sourceless(4, 0),
      { ...segment(0, 9, 9, 2), genLine: 6, srcIdx: 1 },
      sourceless(7, 0),
    ]);
  });

  it("W-05/W-08/W-09/W-13/W-17 with no script, SSR, and production", () => {
    const dto = input({
      canonicalId: "S.vue",
      scopeId: "data-v-1",
      script: null,
      template: {
        code: "function ssrRender() {}\n",
        imports: [],
        ssrImports: ["ssrRenderAttrs"],
        sourceMap: "",
      },
      isProduction: true,
      ssr: true,
      hmrStrategy: "vite",
      authored: { script: false, template: false },
    });
    expect(composeAssembledVueMainModule(dto).code).toBe(
      "const _sfc_main = {}\n" +
        '_sfc_main.__scopeId = "data-v-1"\n' +
        'import { ssrRenderAttrs } from "vue/server-renderer"\n' +
        "\n" +
        "function ssrRender() {}\n" +
        "_sfc_main.ssrRender = ssrRender\n" +
        'import { useSSRContext as __vite_useSSRContext } from "vue"\n' +
        "const _sfc_setup = _sfc_main.setup\n" +
        "_sfc_main.setup = (props, ctx) => {\n" +
        "  const ssrContext = __vite_useSSRContext()\n" +
        '  ;(ssrContext.modules || (ssrContext.modules = new Set())).add("S.vue")\n' +
        "  return _sfc_setup ? _sfc_setup(props, ctx) : undefined\n" +
        "}\n" +
        "export default _sfc_main",
    );
  });

  it("W-16′ webpack, `runtimeModuleName`, `ssrModuleId` and the `styleLangs` fallback", () => {
    const dto = input({
      canonicalId: "C.vue",
      styleCount: 2,
      styleLangs: ["scss", null],
      runtimeModuleName: "vue/dist/vue.esm-bundler.js",
      ssrModuleId: "virtual:mod",
      script: null,
      template: null,
      hmrStrategy: "webpack",
      authored: { script: false, template: false },
    });
    const code = composeAssembledVueMainModule(dto).code;
    expect(code).toContain('import "C.vue?vue&type=style&index=0&lang.scss"\n');
    expect(code).toContain('import "C.vue?vue&type=style&index=1&lang.css"\n');
    expect(code).toContain("/* HMR(webpack) */\nif (module.hot) { module.hot.accept(() => {}) }\n");
    // `ssrModuleId` only reaches W-17, which needs `ssr`.
    expect(code).not.toContain("virtual:mod");
  });

  it("W-14's `customTypes` fallback is `custom` when the index is absent", () => {
    const code = composeAssembledVueMainModule(
      input({
        canonicalId: "C.vue",
        customBlockCount: 1,
        script: null,
        template: null,
        authored: { script: false, template: false },
      }),
    ).code;
    expect(code).toContain('import block0 from "C.vue?vue&type=custom&index=0"\n');
    expect(code).toContain("if (typeof block0 === 'function') block0(_sfc_main)\n");
  });

  it("W-12 patches a template that does not end with LF", () => {
    const code = composeAssembledVueMainModule(
      input({
        script: null,
        template: {
          code: "function render() {}",
          imports: [],
          ssrImports: [],
          sourceMap: "",
        },
        isProduction: true,
        authored: { script: false, template: false },
      }),
    ).code;
    expect(code).toBe(
      "const _sfc_main = {}\n\nfunction render() {}\n_sfc_main.render = render\nexport default _sfc_main",
    );
  });

  it("W-13 beats W-13′ when the template code contains BOTH function forms", () => {
    const code = composeAssembledVueMainModule(
      input({
        script: null,
        template: {
          code: "function render() {}\nfunction ssrRender() {}\n",
          imports: [],
          ssrImports: [],
          sourceMap: "",
        },
        isProduction: true,
        authored: { script: false, template: false },
      }),
    ).code;
    expect(code).toContain("_sfc_main.ssrRender = ssrRender\n");
    expect(code).not.toContain("_sfc_main.render = render\n");
  });

  // `hmrStrategy: "none"` (the `input()` default) means "no dev-server
  // tooling requested" — official `@vitejs/plugin-vue`'s real
  // `transformMain` gates W-15 (`__file`) on `devToolsEnabled ||
  // (devServer && !isProduction)`, a live dev-server/devtools marker, not a
  // bare dev-vs-prod split. Verter has no separate `devToolsEnabled`
  // concept, so `hmrStrategy: "none"` suppresses W-15 too, not just W-16/
  // W-16′ — confirmed against the pinned rc.3 BF2 golden for
  // `basic-interpolation.vue`'s dev cell, which has neither `__file` nor
  // HMR.
  it('`hmrStrategy: "none"` writes neither W-15, W-16, nor W-16′', () => {
    const code = composeAssembledVueMainModule(
      input({ script: null, template: null, authored: { script: false, template: false } }),
    ).code;
    expect(code).toBe("const _sfc_main = {}\nexport default _sfc_main");
  });

  it("§6.2 `dbg` — the five two-character escapes, on a P1-admissible id", () => {
    const code = composeAssembledVueMainModule(
      input({
        canonicalId: 'a"b\\c.vue',
        script: null,
        template: null,
        authored: { script: false, template: false },
        hmrStrategy: "vite",
      }),
    ).code;
    // W-15 is `{:?}`-quoted; W-09's `raw` is not.
    expect(code).toContain('_sfc_main.__file = "a\\"b\\\\c.vue"\n');
  });

  it("W-07 tests the FINAL bytes: a script emptied by pass 2 receives the newline patch", () => {
    const dto = input({
      script: { code: "export default __sfc__;\n", sourceMap: "" },
      template: null,
      authored: { script: false, template: false },
    });
    // Line 0 is the newline patch alone: the script wrote zero bytes, so W-07's
    // LF terminates a line containing no characters whatsoever. `__file`
    // stays out of scope for this case (the `input()` default `hmrStrategy:
    // "none"` correctly omits it — see the W-15/W-16/W-16′ case above); this
    // test is about the newline patch, not W-15.
    expect(composeAssembledVueMainModule(dto).code).toBe("\nexport default _sfc_main");
  });

  it("W-18 has no trailing newline and is the module's last write", () => {
    const code = composeAssembledVueMainModule(
      input({ script: null, template: null, authored: { script: false, template: false } }),
    ).code;
    expect(code.endsWith("export default _sfc_main")).toBe(true);
    // §2.5 — the module's own trailing export carries no `;` and no newline, so
    // it is NOT an instance of pass 2's pattern.
    expect(code).not.toContain("export default _sfc_main;\n");
  });
});

// §7 — the output artifact

describe("§7 — the output artifact schema", () => {
  const twoFragments = (scriptExtra, templateExtra) =>
    input({
      script: {
        code: "const __sfc__ = {}\n",
        sourceMap: mapWith([{ ...segment(0, 6, 1, 6), nameIdx: 0 }], {
          names: ["count"],
          ...scriptExtra,
        }),
      },
      template: {
        code: "function render() {}\n",
        imports: [],
        ssrImports: [],
        sourceMap: mapWith([{ ...segment(0, 9, 9, 2), nameIdx: 0 }], {
          names: ["count"],
          ...templateExtra,
        }),
      },
    });

  it("§7.4 `DECISION` D-5 — stable append with NO deduplication, even for identical spellings", () => {
    const result = composeAssembledVueMainModule(twoFragments({}, {}));
    expect(result.map.sources).toEqual(["Comp.vue", "Comp.vue"]);
    expect(result.map.names).toEqual(["count", "count"]);
    // Template indices shift by the script table lengths.
    const templateSegment = result.segments[3];
    expect(templateSegment.srcIdx).toBe(1);
    expect(templateSegment.nameIdx).toBe(1);
  });

  it("§7.4 — a row no segment references is still contributed", () => {
    const result = composeAssembledVueMainModule(
      input({
        script: {
          code: "const x = 1\n",
          sourceMap: mapWith([], { sources: ["a.vue", "b.vue"], names: ["unused"] }),
        },
        template: null,
        authored: { script: true, template: false },
      }),
    );
    expect(result.map.sources).toEqual(["a.vue", "b.vue"]);
    expect(result.map.names).toEqual(["unused"]);
    // The map declares no segment, but it IS a contributing map whose final code
    // ends with LF, so BR-3 still fires — §2.6's universal half: "a rule that
    // can matter for some admissible input fires for EVERY input its syntactic
    // condition selects".
    expect(result.map.mappings).toBe(";A");
    expect(result.provenance).toEqual(["AssemblyBoundary"]);
  });

  it("§7.4 — `sourcesContent` is the parallel concatenation, present iff any entry is non-null", () => {
    const withContent = composeAssembledVueMainModule(
      twoFragments({ sourcesContent: ["<template/>"] }, {}),
    );
    // The template map declares no `sourcesContent` member, so its row is null.
    expect(withContent.map.sourcesContent).toEqual(["<template/>", null]);

    const allNull = composeAssembledVueMainModule(twoFragments({ sourcesContent: [null] }, {}));
    expect(Object.hasOwn(allNull.map, "sourcesContent")).toBe(false);

    const none = composeAssembledVueMainModule(twoFragments({}, {}));
    expect(Object.hasOwn(none.map, "sourcesContent")).toBe(false);
  });

  it("§7.3 `DECISION` D-4 — the ignore list is carried and remapped by the source base offset", () => {
    const result = composeAssembledVueMainModule(
      twoFragments({ ignoreList: [0] }, { x_google_ignoreList: [0] }),
    );
    // Script row 0 stays 0; template row 0 shifts to 1.
    expect(result.map.ignoreList).toEqual([0, 1]);
  });

  it("§7.3 — ignore status is a property of a ROW, not of a path", () => {
    // The same spelling occupies two rows, of which only one is ignored. The
    // artifact says exactly what its inputs said.
    const result = composeAssembledVueMainModule(twoFragments({ ignoreList: [0] }, {}));
    expect(result.map.sources).toEqual(["Comp.vue", "Comp.vue"]);
    expect(result.map.ignoreList).toEqual([0]);
  });

  it("§7.3 — the member is ABSENT when the resulting list is empty", () => {
    const result = composeAssembledVueMainModule(twoFragments({ ignoreList: [] }, {}));
    expect(Object.hasOwn(result.map, "ignoreList")).toBe(false);
  });

  it("§7.2 — `file`, `debugId` and unknown/extension members are DROPPED, never inherited", () => {
    const result = composeAssembledVueMainModule(
      input({
        script: {
          code: "const x = 1\n",
          sourceMap: mapWith([segment(0, 0, 1, 0)], {
            file: "Comp.vue",
            debugId: "abc",
            x_verter_helper_preamble_end: 12,
          }),
        },
        template: null,
        authored: { script: true, template: false },
      }),
    );
    expect(Object.keys(result.map).sort()).toEqual(["mappings", "names", "sources", "version"]);
  });

  it("§7.1 — `version` is always 3 and `names` is always present, possibly empty", () => {
    const result = composeAssembledVueMainModule(
      input({
        script: { code: "const x = 1\n", sourceMap: mapWith([]) },
        template: null,
        authored: { script: true, template: false },
      }),
    );
    expect(result.map.version).toBe(3);
    expect(result.map.names).toEqual([]);
  });

  it("§7.7 — with `sourceMapRequested: false` NO map is produced, and a map string is ignored", () => {
    const dto = input({
      script: { code: "const __sfc__ = {}\n", sourceMap: mapWith([segment(0, 6, 1, 6)]) },
      template: null,
      sourceMapRequested: false,
      authored: { script: true, template: false },
    });
    const result = composeAssembledVueMainModule(dto);
    expect(result.outcome).toBe("composed");
    expect(result.map).toBeNull();
    expect(result.segments).toBeNull();
    // The code is unchanged by the map being disabled — the passes still run.
    expect(result.code).toBe(
      composeAssembledVueMainModule({ ...dto, sourceMapRequested: true }).code,
    );
  });

  it("§7.7 — with `sourceMapRequested: false`, a REQUIRED-but-absent map is not a failure", () => {
    const result = composeAssembledVueMainModule(
      input({
        script: { code: "const x = 1\n", sourceMap: "" },
        template: null,
        sourceMapRequested: false,
        authored: { script: true, template: false },
      }),
    );
    expect(result.outcome).toBe("composed");
  });
});

describe("§7.6 — the canonical `mappings` encoding", () => {
  it("encodes in SEQUENCE ORDER and round-trips through the accepted decoder", () => {
    const result = composeAssembledVueMainModule(
      input({
        script: {
          code: "const __sfc__ = {}\n",
          sourceMap: mapWith([segment(0, 0, 5, 5), segment(0, 0, 1, 0)]),
        },
        template: {
          code: "function render() {}\n",
          imports: [],
          ssrImports: [],
          sourceMap: mapWith([segment(0, 9, 9, 2)]),
        },
      }),
    );
    // `src/sourcemap.mjs` is the accepted wire-format authority for what a
    // decoder reads back; a zero column delta must read as an ADDITIONAL
    // segment so equal-coordinate order survives the round trip.
    expect(decodeMappings(result.map.mappings)).toEqual(result.segments);
  });

  it("a generated line with no segments contributes an EMPTY group, and encoding stops after the last", () => {
    const segments = [segment(0, 0, 1, 0), segment(3, 2, 2, 0)];
    const encoded = encodeMappings(segments);
    expect(encoded).toBe("AACA;;;EACA");
    expect(encoded.endsWith(";")).toBe(false);
    expect(decodeMappings(encoded)).toEqual(segments);
  });

  it("an empty sequence encodes to the empty string", () => {
    expect(encodeMappings([])).toBe("");
  });

  it("a sourceless segment encodes exactly one field", () => {
    expect(encodeMappings([sourceless(0, 3)])).toBe("G");
  });
});

// §11.4 / §3.5 — a malformed DTO instance is out of layer-1 scope

describe("§11.4 — a schema- or P1-invalid DTO is NOT a composition outcome", () => {
  const valid = () =>
    input({ script: null, template: null, authored: { script: false, template: false } });

  it("a missing member, an extra member, and a bad `hmrStrategy` all raise, with no U-family", () => {
    for (const mutate of [
      (dto) => delete dto.scopeId,
      (dto) => {
        dto.extra = 1;
      },
      (dto) => {
        dto.hmrStrategy = "rollup";
      },
      (dto) => {
        dto.styleCount = -1;
      },
      (dto) => {
        dto.authored = { script: true };
      },
    ]) {
      const dto = valid();
      mutate(dto);
      expect(() => composeAssembledVueMainModule(dto)).toThrow(MalformedAssembleInputError);
    }
  });

  it("precondition P1 bounds the six embedded strings to printable ASCII (§3.5)", () => {
    for (const overrides of [
      { canonicalId: "Cömp.vue" },
      { scopeId: "data-v-é" },
      { runtimeModuleName: "vue’" },
      { ssrModuleId: "mod " },
      { styleLangs: ["scéss"] },
      { customTypes: ["i18én"] },
    ]) {
      expect(() => composeAssembledVueMainModule(input({ ...valid(), ...overrides }))).toThrow(
        MalformedAssembleInputError,
      );
    }
  });

  it("`code` and `sourceMap` are NOT subject to P1 — astral and CRLF content compose", () => {
    const result = composeAssembledVueMainModule(
      input({
        script: {
          code: "const \u{1D400} = 1\r\n",
          sourceMap: mapWith([segment(0, 6, 1, 6)], { sources: ["Cömp.vue"] }),
        },
        template: null,
        authored: { script: true, template: false },
      }),
    );
    expect(result.outcome).toBe("composed");
    expect(result.map.sources).toEqual(["Cömp.vue"]);
  });
});
