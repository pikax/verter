// Self-test: the authored-source mapping oracle's DISCRIMINATION.
//
// Every case below plants a mutation that is proven to have applied — the
// target is asserted to exist (and, where the plant is an addition, to be
// genuinely absent beforehand), the mutated input is asserted to differ, and
// the unmutated input is re-validated clean afterwards. A green run with an
// unproven plant would be indistinguishable from a broken oracle, so the
// proof is part of each test rather than an afterthought.
//
// Covered here: shifted generated positions, shifted original positions,
// dropped script and template anchors, and the UTF-16 / CRLF text
// conventions (a fixture carrying astral-plane characters and CRLF line
// terminators, which a byte-offset or `\r`-stripping implementation gets
// wrong).

import { afterAll, describe, expect, it } from "vitest";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { decodeMappings, encodeMappings } from "../src/sourcemap.mjs";
import { HARNESS_ROOT } from "../src/paths.mjs";
import {
  FIXTURE_ANCHORS,
  MAPPING_PROFILES,
  lineTable,
  tokenAt,
  validateAuthoredMapping,
} from "../src/mapping-oracle.mjs";

const FIXTURE = "fixtures/vue/basic-interpolation.vue";
const FIXTURE_ABS = path.join(HARNESS_ROOT, FIXTURE);
const SOURCE = readFileSync(FIXTURE_ABS, "utf8");
const SCRIPT_ANCHOR = FIXTURE_ANCHORS[FIXTURE][0];
const TEMPLATE_ANCHOR = FIXTURE_ANCHORS[FIXTURE][1];

const SCRATCH = mkdtempSync(path.join(tmpdir(), "bf2-mapping-oracle-"));
afterAll(() => rmSync(SCRATCH, { recursive: true, force: true }));

const rules = (result) => result.violations.map((v) => v.rule);

function artifact() {
  const compiled = compileVueFixture(SOURCE, FIXTURE, {
    backend: "vdom",
    sourceMap: true,
    isProd: false,
  });
  expect(compiled.diagnostics).toEqual([]);
  return compiled;
}

function input(compiled, map) {
  return {
    code: compiled.code,
    map,
    sourceMapRequested: true,
    fixture: { path: FIXTURE, absolutePath: FIXTURE_ABS },
    sourceResolveBases: [HARNESS_ROOT],
    profile: MAPPING_PROFILES["vue:vdom"],
    anchors: FIXTURE_ANCHORS[FIXTURE],
  };
}

/** The unique segment carrying an anchor's exact original position. */
function anchorSegment(segments, anchor) {
  const carriers = segments.filter(
    (segment) =>
      segment.srcIdx !== null &&
      segment.srcLine === anchor.line &&
      segment.srcCol === anchor.column,
  );
  expect(carriers.length, `${anchor.id} carriers`).toBeGreaterThan(0);
  return carriers[0];
}

/** Rewrites exactly one segment, proving the target existed and changed. */
function mutateSegment(compiled, target, change) {
  const segments = decodeMappings(compiled.map.mappings);
  const index = segments.findIndex(
    (segment) =>
      segment.genLine === target.genLine &&
      segment.genCol === target.genCol &&
      segment.srcLine === target.srcLine &&
      segment.srcCol === target.srcCol,
  );
  expect(index, "plant target present exactly once").toBeGreaterThan(-1);
  expect(
    segments.filter(
      (segment) => segment.genLine === target.genLine && segment.genCol === target.genCol,
    ).length,
  ).toBe(1);
  const mutated = { ...segments[index], ...change };
  expect(JSON.stringify(mutated)).not.toBe(JSON.stringify(segments[index]));
  segments[index] = mutated;
  const mappings = encodeMappings(segments);
  expect(mappings).not.toBe(compiled.map.mappings);
  return { ...compiled.map, mappings };
}

describe("requirement 3 — shifted positions are caught, not tolerated", () => {
  it("baseline: the unmutated artifact is clean (so every failure below is the plant)", () => {
    expect(validateAuthoredMapping(input(artifact(), artifact().map)).violations).toEqual([]);
  });

  it("MUTATION: a generated COLUMN shifted by one no longer carries the authored token", () => {
    const compiled = artifact();
    const target = anchorSegment(decodeMappings(compiled.map.mappings), SCRIPT_ANCHOR);
    const mutated = mutateSegment(compiled, target, { genCol: target.genCol + 1 });
    const result = validateAuthoredMapping(input(compiled, mutated));
    expect(rules(result)).toContain("segment-provenance");
    expect(validateAuthoredMapping(input(compiled, compiled.map)).violations).toEqual([]);
  });

  it("MUTATION: a generated LINE shifted by one no longer carries the authored token", () => {
    const compiled = artifact();
    const target = anchorSegment(decodeMappings(compiled.map.mappings), SCRIPT_ANCHOR);
    const mutated = mutateSegment(compiled, target, { genLine: target.genLine + 1 });
    const result = validateAuthoredMapping(input(compiled, mutated));
    expect(rules(result)).toContain("segment-provenance");
    expect(validateAuthoredMapping(input(compiled, compiled.map)).violations).toEqual([]);
  });

  it("MUTATION: an original COLUMN shifted by one is caught by BOTH the segment and the anchor rails", () => {
    const compiled = artifact();
    const target = anchorSegment(decodeMappings(compiled.map.mappings), SCRIPT_ANCHOR);
    const mutated = mutateSegment(compiled, target, { srcCol: target.srcCol + 1 });
    const result = validateAuthoredMapping(input(compiled, mutated));
    expect(rules(result)).toContain("segment-provenance");
    expect(rules(result)).toContain("anchor-missing");
    expect(validateAuthoredMapping(input(compiled, compiled.map)).violations).toEqual([]);
  });

  it("MUTATION: an original LINE shifted by one is caught by BOTH rails", () => {
    const compiled = artifact();
    const target = anchorSegment(decodeMappings(compiled.map.mappings), SCRIPT_ANCHOR);
    const mutated = mutateSegment(compiled, target, { srcLine: target.srcLine + 1 });
    const result = validateAuthoredMapping(input(compiled, mutated));
    expect(rules(result)).toContain("segment-provenance");
    expect(rules(result)).toContain("anchor-missing");
    expect(validateAuthoredMapping(input(compiled, compiled.map)).violations).toEqual([]);
  });

  it("MUTATION: a generated identifier pointed at an UNRELATED authored identifier fails", () => {
    // The `count` binding re-pointed at the `items` binding: in bounds, on a
    // real authored identifier, and still wrong provenance.
    const compiled = artifact();
    const segments = decodeMappings(compiled.map.mappings);
    const target = anchorSegment(segments, SCRIPT_ANCHOR);
    expect(SOURCE.split("\n")[4].slice(6, 11)).toBe("items");
    const mutated = mutateSegment(compiled, target, { srcLine: 4, srcCol: 6 });
    const result = validateAuthoredMapping(input(compiled, mutated));
    expect(rules(result)).toContain("segment-provenance");
  });
});

describe("requirement 4 — a dropped anchor is a completeness gap, not a shrug", () => {
  for (const [label, anchor] of [
    ["script", SCRIPT_ANCHOR],
    ["template", TEMPLATE_ANCHOR],
  ]) {
    it(`MUTATION: dropping every segment covering the ${label} anchor fails`, () => {
      const compiled = artifact();
      const segments = decodeMappings(compiled.map.mappings);
      const surviving = segments.filter(
        (segment) =>
          !(
            segment.srcIdx !== null &&
            segment.srcLine === anchor.line &&
            segment.srcCol >= anchor.column &&
            segment.srcCol < anchor.column + anchor.text.length
          ),
      );
      // Proof the plant applies: segments really were removed.
      expect(surviving.length).toBeLessThan(segments.length);
      const mutatedMappings = encodeMappings(surviving);
      expect(mutatedMappings).not.toBe(compiled.map.mappings);
      const result = validateAuthoredMapping(
        input(compiled, { ...compiled.map, mappings: mutatedMappings }),
      );
      expect(rules(result)).toContain("anchor-missing");
      expect(rules(result)).toContain("anchor-span-coverage");
      expect(validateAuthoredMapping(input(compiled, compiled.map)).violations).toEqual([]);
    });
  }

  it("an anchor whose declared authored text is not at its declared position fails loudly", () => {
    const compiled = artifact();
    const drifted = [{ ...SCRIPT_ANCHOR, id: "drifted", column: SCRIPT_ANCHOR.column + 1 }];
    const result = validateAuthoredMapping({
      ...input(compiled, compiled.map),
      anchors: drifted,
    });
    expect(rules(result)).toContain("anchor-source-text");
  });
});

// UTF-16 code units and CRLF line terminators.
//
// The fixture is written to a scratch directory rather than committed under
// `fixtures/`, for two reasons: the committed corpus drives golden
// generation (a new fixture there would silently expand the published golden
// set), and a committed CRLF file is subject to checkout normalization,
// which would make the CRLF half of this test depend on the checkout's line
// -ending configuration instead of on the oracle.

const UNICODE_LINES = [
  "<script setup>",
  'const msg = "héllo 🎉 wörld";',
  "</script>",
  "",
  "<template>",
  "  <p>{{ msg }} 🎉 ünïcode</p>",
  "</template>",
  "",
];

function writeUnicodeFixture(eol) {
  const name = `unicode-${eol === "\r\n" ? "crlf" : "lf"}.vue`;
  const file = path.join(SCRATCH, name);
  writeFileSync(file, UNICODE_LINES.join(eol), "utf8");
  return { name, file, source: readFileSync(file, "utf8") };
}

const UNICODE_ANCHORS = [
  {
    id: "script-msg-declaration",
    region: "script",
    line: 1,
    column: 6,
    text: "msg",
    expectRelations: ["verbatim-carry"],
    requiredFor: ["vue:vdom"],
  },
  {
    id: "template-msg-interpolation",
    region: "template",
    line: 5,
    column: 8,
    text: "msg",
    expectRelations: ["verbatim-carry", "context-binding-prefix"],
    requiredFor: ["vue:vdom"],
  },
];

function unicodeInput(fixture, compiled, map) {
  return {
    code: compiled.code,
    map,
    sourceMapRequested: true,
    fixture: { path: fixture.name, absolutePath: fixture.file },
    sourceResolveBases: [SCRATCH],
    profile: MAPPING_PROFILES["vue:vdom"],
    anchors: UNICODE_ANCHORS,
  };
}

describe("requirement 1/7 — UTF-16 code-unit columns and CRLF line terminators", () => {
  it("lineTable RETAINS the carriage return, so a CRLF line's columns are the producer's columns", () => {
    const lines = lineTable("a\r\nb\r\n");
    expect(lines[0]).toBe("a\r");
    expect(lines[0].length).toBe(2);
    expect(tokenAt(lines, 0, 1)).toEqual({ kind: "punct", text: "\r", rest: "\r" });
    expect(tokenAt(lines, 0, 2).kind).toBe("eol");
    expect(tokenAt(lines, 0, 3).kind).toBe("out-of-bounds");
  });

  it("tokenAt measures columns in UTF-16 code units, so an astral character occupies two", () => {
    const lines = lineTable("a🎉b");
    expect(lines[0].length).toBe(4);
    expect(tokenAt(lines, 0, 0)).toEqual({ kind: "word-start", text: "a", rest: "a" });
    // Both halves of the surrogate pair are addressable, non-word positions.
    expect(tokenAt(lines, 0, 1).kind).toBe("punct");
    expect(tokenAt(lines, 0, 2).kind).toBe("punct");
    expect(tokenAt(lines, 0, 3)).toEqual({ kind: "word-start", text: "b", rest: "b" });
    expect(Buffer.byteLength(lines[0], "utf8")).toBe(6); // a byte oracle would disagree
  });

  for (const eol of ["\n", "\r\n"]) {
    const label = eol === "\r\n" ? "CRLF" : "LF";
    it(`${label}: a fixture with non-ASCII and astral characters validates clean end to end`, () => {
      const fixture = writeUnicodeFixture(eol);
      expect(fixture.source.includes("\r\n")).toBe(eol === "\r\n");
      expect(fixture.source).toContain("🎉");
      const compiled = compileVueFixture(fixture.source, fixture.name, {
        backend: "vdom",
        sourceMap: true,
        isProd: false,
      });
      expect(compiled.diagnostics).toEqual([]);
      const result = validateAuthoredMapping(unicodeInput(fixture, compiled, compiled.map));
      expect(result.violations).toEqual([]);
      expect(result.stats.anchors).toBe(2);
      // The astral character genuinely reaches the generated code, so the
      // column convention is actually exercised rather than assumed.
      expect(compiled.code).toContain("🎉");
    });
  }

  it("MUTATION: a generated column computed as a BYTE offset instead of a UTF-16 offset fails", () => {
    const fixture = writeUnicodeFixture("\n");
    const compiled = compileVueFixture(fixture.source, fixture.name, {
      backend: "vdom",
      sourceMap: true,
      isProd: false,
    });
    const genLines = compiled.code.split("\n");
    const segments = decodeMappings(compiled.map.mappings);
    // A segment on a generated line that carries non-ASCII text BEFORE it —
    // exactly where a byte-offset producer diverges from a code-unit one.
    const target = segments.find((segment) => {
      const prefix = genLines[segment.genLine].slice(0, segment.genCol);
      return Buffer.byteLength(prefix, "utf8") !== prefix.length;
    });
    expect(target, "a segment preceded by non-ASCII text").toBeDefined();
    const byteColumn = Buffer.byteLength(genLines[target.genLine].slice(0, target.genCol), "utf8");
    expect(byteColumn).toBeGreaterThan(target.genCol);
    const mutated = mutateSegmentIn(compiled.map, target, { genCol: byteColumn });
    const result = validateAuthoredMapping(unicodeInput(fixture, compiled, mutated));
    expect(result.ok).toBe(false);
    expect(
      rules(result).some((rule) =>
        ["generated-position-bounds", "segment-provenance"].includes(rule),
      ),
    ).toBe(true);
    expect(
      validateAuthoredMapping(unicodeInput(fixture, compiled, compiled.map)).violations,
    ).toEqual([]);
  });

  it("MUTATION: an original column past the CRLF line's true length is out of bounds", () => {
    const fixture = writeUnicodeFixture("\r\n");
    const compiled = compileVueFixture(fixture.source, fixture.name, {
      backend: "vdom",
      sourceMap: true,
      isProd: false,
    });
    const srcLines = lineTable(fixture.source);
    const segments = decodeMappings(compiled.map.mappings);
    const target = segments.find((segment) => segment.srcIdx !== null);
    expect(target).toBeDefined();
    // `\r` is a real column of the authored line: length-1 stays IN bounds
    // and length+1 does not. A `\r`-stripping table would reject the former.
    const trueLength = srcLines[target.srcLine].length;
    expect(srcLines[target.srcLine].endsWith("\r")).toBe(true);
    expect(
      validateAuthoredMapping(
        unicodeInput(
          fixture,
          compiled,
          mutateSegmentIn(compiled.map, target, {
            srcCol: trueLength - 1,
          }),
        ),
      ).violations.map((v) => v.rule),
    ).not.toContain("original-position-bounds");
    expect(
      rules(
        validateAuthoredMapping(
          unicodeInput(
            fixture,
            compiled,
            mutateSegmentIn(compiled.map, target, {
              srcCol: trueLength + 1,
            }),
          ),
        ),
      ),
    ).toContain("original-position-bounds");
  });
});

/** Rewrites one segment of an arbitrary map, proving the target existed. */
function mutateSegmentIn(map, target, change) {
  const segments = decodeMappings(map.mappings);
  const index = segments.findIndex(
    (segment) =>
      segment.genLine === target.genLine &&
      segment.genCol === target.genCol &&
      segment.srcLine === target.srcLine &&
      segment.srcCol === target.srcCol,
  );
  expect(index).toBeGreaterThan(-1);
  const mutated = { ...segments[index], ...change };
  expect(JSON.stringify(mutated)).not.toBe(JSON.stringify(segments[index]));
  segments[index] = mutated;
  const mappings = encodeMappings(segments);
  expect(mappings).not.toBe(map.mappings);
  return { ...map, mappings };
}
