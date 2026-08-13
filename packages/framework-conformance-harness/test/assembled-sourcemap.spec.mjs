// Self-test: the published golden map describes the PUBLISHED assembled
// module, and the mapping acceptance axis compares candidate-vs-official
// on decoded segment semantics.
//
// The pre-existing harness artifact this locks out: the raw render-fragment
// map was published as "the" artifact map — its GENERATED coordinates
// addressed the standalone fragment (not the assembled module the golden's
// `code` field actually holds), its ORIGINAL coordinates were
// template-block-relative (the descriptor block map was never chained), and
// the script half was entirely unmapped. Every anchor test below fails
// against that artifact and passes against the composed map.

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { compareMappings } from "../src/compare.mjs";
import { readGoldenByName } from "../src/golden-store.mjs";
import { decodeMappings, encodeMappings, normalizedMappingSegments } from "../src/sourcemap.mjs";
import { GOLDENS_ROOT, HARNESS_ROOT } from "../src/paths.mjs";

const FIXTURE_PATH = "fixtures/vue/basic-interpolation.vue";
const FIXTURE = readFileSync(path.join(HARNESS_ROOT, FIXTURE_PATH), "utf8");
const FIXTURE_LINES = FIXTURE.split("\n");

function compileWithMap(backend) {
  const artifact = compileVueFixture(FIXTURE, FIXTURE_PATH, {
    backend,
    sourceMap: true,
    isProd: false,
  });
  expect(artifact.diagnostics).toEqual([]);
  expect(artifact.map).not.toBeNull();
  return artifact;
}

describe("mappings codec", () => {
  it("decode → encode round-trips a real official map byte-identically", () => {
    const { map } = compileWithMap("vdom");
    expect(encodeMappings(decodeMappings(map.mappings))).toBe(map.mappings);
  });

  it("rejects a non-VLQ mappings string instead of decoding garbage", () => {
    expect(() => decodeMappings("AA%A")).toThrow(/invalid VLQ/);
  });
});

describe("assembled-module map — every backend, whole-artifact soundness", () => {
  for (const backend of ["vdom", "vapor", "ssr"]) {
    it(`${backend}: single fixture source, full fixture content, every segment in bounds`, () => {
      const { code, map } = compileWithMap(backend);
      expect(map.sources).toEqual([FIXTURE_PATH]);
      expect(map.sourcesContent).toEqual([FIXTURE]);
      const codeLines = code.split("\n");
      for (const seg of decodeMappings(map.mappings)) {
        expect(seg.genLine).toBeLessThan(codeLines.length);
        expect(seg.genCol).toBeLessThanOrEqual(codeLines[seg.genLine].length);
        if (seg.srcLine !== null) {
          expect(seg.srcIdx).toBe(0);
          expect(seg.srcLine).toBeLessThan(FIXTURE_LINES.length);
          expect(seg.srcCol).toBeLessThanOrEqual(FIXTURE_LINES[seg.srcLine].length);
        }
      }
    });
  }

  it("the SCRIPT half is mapped: the setup statement's assembled line anchors to its fixture line", () => {
    const { code, map } = compileWithMap("vdom");
    const genLine = code.split("\n").findIndex((l) => l.includes("const count = ref(0)"));
    const srcLine = FIXTURE_LINES.findIndex((l) => l.includes("const count = ref(0)"));
    expect(genLine).toBeGreaterThan(-1);
    expect(srcLine).toBeGreaterThan(-1);
    const anchors = decodeMappings(map.mappings).filter(
      (seg) => seg.genLine === genLine && seg.srcLine === srcLine,
    );
    expect(anchors.length).toBeGreaterThan(0);
  });

  it("the RENDER half is mapped at ASSEMBLED generated lines with FILE-relative original lines", () => {
    const { code, map } = compileWithMap("vdom");
    // The {{ count }} interpolation: its assembled generated line must map
    // to the fixture's whole-file template line — not a block-relative line
    // (unchained block map) and not a fragment-relative generated line
    // (unanchored fragment map).
    const genLine = code.split("\n").findIndex((l) => l.includes("_toDisplayString($setup.count)"));
    const srcLine = FIXTURE_LINES.findIndex((l) => l.includes("{{ count }}"));
    expect(genLine).toBeGreaterThan(-1);
    expect(srcLine).toBeGreaterThan(-1);
    const anchors = decodeMappings(map.mappings).filter(
      (seg) => seg.genLine === genLine && seg.srcLine === srcLine,
    );
    expect(anchors.length).toBeGreaterThan(0);
  });

  it("a sourceMap:false compile still publishes no map", () => {
    const artifact = compileVueFixture(FIXTURE, FIXTURE_PATH, {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });
    expect(artifact.map).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// Column-precise re-anchoring ground truth — the control that makes the
// keyword-splice arithmetic in composeAssembledModuleMap FALSIFIABLE.
//
// The three committed corpus fixtures never place an official map segment
// on an edited line: their script halves are <script setup> (the official
// compiler SYNTHESIZES the `export default {` wrapper line, unmapped) or
// absent, and no backend maps the render fragment's `export function` line.
// On that corpus the single-line splice re-anchoring is a no-op, so no
// assertion over corpus maps can discriminate a broken column offset. This
// control compiles a plain `<script>` SFC — whose user-authored
// `export default { ... }` line IS officially mapped, column by column —
// through the real production path (compileVueFixture), for every backend,
// and pins the exact composed segment set with literal expected columns:
//   - the `export default ` keyword span (original columns 0..13) is
//     harness-replaced text, so its segments must be DROPPED;
//   - every surviving segment shifts by the splice delta
//     ("const _sfc_main = ".length 18 − "export default ".length 15 = +3).
// A one-column offset in the re-anchoring, or the re-anchoring not running
// at all, changes this exact set and fails these assertions.
// ---------------------------------------------------------------------------

const PLAIN_SCRIPT_FIXTURE = `<script>
export default { name: "plain-script", data() { return { n: 1 } } }
</script>
<template><p>{{ n }}</p></template>
`;

// The pinned official compiler's original columns on the fixture's
// `export default { ... }` line that survive the keyword-span drop
// (every raw segment at original column >= 15; verified against the raw
// compileScript map, which also carries segments at columns
// 0,1,2,3,4,5,7,8,9,10,11,12,13 inside the replaced keyword span).
const EDIT_LINE_SURVIVING_SRC_COLS = [
  15, 17, 18, 19, 20, 21, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 39, 40, 41,
  42, 43, 44, 46, 48, 49, 50, 51, 52, 53, 55, 57, 58, 60, 62, 64, 66,
];

// Literal render-half anchors per backend: the assembled generated line and
// the exact [genCol, srcLine, srcCol] of every segment on it. srcLine 3 is
// the fixture's whole-file template line — file-relative, not
// block-relative.
const PLAIN_RENDER_LINE_LITERALS = {
  vdom: {
    genLine: 6,
    segments: [
      [24, 3, 0],
      [44, 3, 14],
      [72, 3, 6],
      [79, 3, 7],
    ],
  },
  vapor: {
    genLine: 9,
    segments: [
      [52, 3, 6],
      [58, 3, 7],
    ],
  },
  ssr: {
    genLine: 9,
    segments: [
      [20, 3, 6],
      [27, 3, 7],
    ],
  },
};

describe("splice re-anchoring — exact column correspondences on the edited line, every backend", () => {
  for (const backend of ["vdom", "vapor", "ssr"]) {
    it(`${backend}: the edited export-default line's composed segments carry the exact re-anchored columns`, () => {
      const artifact = compileVueFixture(PLAIN_SCRIPT_FIXTURE, "plain-script-control.vue", {
        backend,
        sourceMap: true,
        isProd: false,
      });
      expect(artifact.diagnostics).toEqual([]);
      const lines = artifact.code.split("\n");
      const genLine = lines.findIndex((l) => l.startsWith("const _sfc_main = { name:"));
      expect(genLine).toBe(1);
      const segments = decodeMappings(artifact.map.mappings)
        .filter((seg) => seg.genLine === genLine)
        .map(({ genCol, srcIdx, srcLine, srcCol }) => ({ genCol, srcIdx, srcLine, srcCol }));
      expect(segments).toEqual(
        EDIT_LINE_SURVIVING_SRC_COLS.map((srcCol) => ({
          genCol: srcCol + 3,
          srcIdx: 0,
          srcLine: 1,
          srcCol,
        })),
      );
      // Fully literal end anchors, tied back to the actual texts: the `{`
      // opening the component object and the object's closing `}`.
      expect(segments[0]).toEqual({ genCol: 18, srcIdx: 0, srcLine: 1, srcCol: 15 });
      expect(segments[segments.length - 1]).toEqual({
        genCol: 69,
        srcIdx: 0,
        srcLine: 1,
        srcCol: 66,
      });
      expect(lines[1][18]).toBe("{");
      expect(PLAIN_SCRIPT_FIXTURE.split("\n")[1][15]).toBe("{");
    });

    it(`${backend}: a render-half line maps at its assembled line with literal file-relative columns`, () => {
      const artifact = compileVueFixture(PLAIN_SCRIPT_FIXTURE, "plain-script-control.vue", {
        backend,
        sourceMap: true,
        isProd: false,
      });
      const { genLine, segments } = PLAIN_RENDER_LINE_LITERALS[backend];
      const actual = decodeMappings(artifact.map.mappings)
        .filter((seg) => seg.genLine === genLine)
        .map((seg) => [seg.genCol, seg.srcLine, seg.srcCol]);
      expect(actual).toEqual(segments);
    });
  }
});

// The same literal-ground-truth discipline over COMMITTED corpus artifacts:
// the published golden maps themselves are pinned to exact expected column
// values (independent of both the composition arithmetic and any
// self-mutation), so a regeneration under broken arithmetic cannot silently
// become the accepted expectation.
describe("committed golden maps — literal column anchors, every backend", () => {
  const SCRIPT_IDENTITY_LINE = {
    vdom: 8,
    vapor: 9, // the script half carries the extra `__vapor: true,` line
    ssr: 8,
  };

  for (const backend of ["vdom", "vapor", "ssr"]) {
    it(`${backend}: the committed map1 golden maps \`const count = ref(0);\` identity, column by column`, () => {
      const golden = readGoldenByName(
        GOLDENS_ROOT,
        `vue/basic-interpolation__${backend}__map1__prod0`,
      );
      const genLine = SCRIPT_IDENTITY_LINE[backend];
      expect(golden.code.split("\n")[genLine]).toBe("const count = ref(0);");
      expect(FIXTURE_LINES[3]).toBe("const count = ref(0);");
      const actual = decodeMappings(golden.map.mappings)
        .filter((seg) => seg.genLine === genLine)
        .map(({ genCol, srcIdx, srcLine, srcCol }) => ({ genCol, srcIdx, srcLine, srcCol }));
      expect(actual).toEqual(
        Array.from({ length: 21 }, (_, col) => ({
          genCol: col,
          srcIdx: 0,
          srcLine: 3,
          srcCol: col,
        })),
      );
    });
  }

  it("vdom: the committed golden's interpolation render line carries its exact literal segments", () => {
    const golden = readGoldenByName(GOLDENS_ROOT, "vue/basic-interpolation__vdom__map1__prod0");
    expect(golden.code.split("\n")[26]).toContain("_toDisplayString($setup.count)");
    const actual = decodeMappings(golden.map.mappings)
      .filter((seg) => seg.genLine === 26)
      .map((seg) => [seg.genCol, seg.srcLine, seg.srcCol]);
    expect(actual).toEqual([
      [23, 9, 4],
      [43, 9, 39],
      [77, 9, 27],
      [89, 9, 32],
    ]);
  });
});

describe("mapping acceptance axis — decoded candidate-vs-official comparison", () => {
  it("representation-only differences are equal: VLQ spelling, in-line order, duplicates, trailing empty lines", () => {
    const { map } = compileWithMap("vdom");
    const segments = decodeMappings(map.mappings);
    // Re-encode the SAME correspondences differently: duplicate a segment,
    // reverse in-line order (encodeMappings re-sorts, so feed the encoder a
    // hand-rolled variant), and append trailing empty lines.
    const doubled = [...segments, segments[0]];
    const variant = `${encodeMappings(doubled)};;;`;
    expect(variant).not.toBe(map.mappings); // the plant applied
    const result = compareMappings(map, { ...map, mappings: variant });
    expect(result.fields.mappings).toBe(true);
    expect(result.equal).toBe(true);
  });

  it("a genuine anchor shift is a divergence attributed to the mappings field", () => {
    const { map } = compileWithMap("vdom");
    const segments = decodeMappings(map.mappings);
    const shifted = segments.map((seg, i) =>
      i === 0 ? { ...seg, srcLine: (seg.srcLine ?? 0) + 1 } : seg,
    );
    const result = compareMappings(map, { ...map, mappings: encodeMappings(shifted) });
    expect(result.equal).toBe(false);
    expect(result.fields.mappings).toBe(false);
    for (const field of ["version", "sources", "sourceRoot", "sourcesContent", "names"]) {
      expect(result.fields[field]).toBe(true);
    }
  });

  it("a dropped mapped position is a divergence (segment-set inequality, not spelling)", () => {
    const { map } = compileWithMap("vdom");
    const normalized = normalizedMappingSegments(map.mappings);
    const withoutLast = encodeMappings(normalized.slice(0, -1));
    const result = compareMappings(map, { ...map, mappings: withoutLast });
    expect(result.equal).toBe(false);
    expect(result.fields.mappings).toBe(false);
  });

  it("an undecodable candidate mappings field never passes against a decodable golden", () => {
    const { map } = compileWithMap("vdom");
    const result = compareMappings(map, { ...map, mappings: "%%not-vlq%%" });
    expect(result.equal).toBe(false);
    expect(result.fields.mappings).toBe(false);
  });
});
