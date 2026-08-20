// Self-test: the AUTHORED-SOURCE mapping oracle.
//
// WHY THIS ORACLE EXISTS AT ALL. The mapping axis used to compare the
// candidate's `mappings` field against the official compiler's — and that
// comparison is not merely the wrong oracle, it is structurally incapable of
// ever being the right one. A source map's `mappings` field encodes
// (generated position -> original position) correspondences over ONE
// SPECIFIC generated document. Verter's generated JS is legitimately not
// byte-identical to the official compiler's (the Compiled-Output
// Conformance rule permits cosmetic carrier differences), so the two maps
// describe DIFFERENT generated documents by construction: the comparison
// rejects a perfectly correct map whose generated layout differs, and
// accepts a wrong map whose segment shape happens to resemble official's.
//
// The replacement validates the candidate's map against the candidate's OWN
// generated code and the AUTHORED fixture source. No golden map is an input.
//
// Every negative below plants a PROVEN-applied mutation (asserted unique and
// new before the write, verified present after, and byte-restored) in the
// same discipline as normalizer-mutations.spec.mjs.

import { describe, expect, it } from "vitest";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { compileVueFixture } from "../src/invoke-vue-oracle.mjs";
import { compileSvelteFixture } from "../src/invoke-svelte-oracle.mjs";
import { decodeMappings, encodeMappings } from "../src/sourcemap.mjs";
import { GOLDENS_ROOT, HARNESS_ROOT } from "../src/paths.mjs";
import { readGoldenByName, readGoldenManifest } from "../src/golden-store.mjs";
import { checkCandidate } from "../src/check-candidate.mjs";
import {
  MAPPING_PROFILES,
  FIXTURE_ANCHORS,
  classifySegment,
  generatedOnlyRanges,
  tokenAt,
  validateAuthoredMapping,
} from "../src/mapping-oracle.mjs";

const VUE_FIXTURE = "fixtures/vue/basic-interpolation.vue";
const VUE_FIXTURE_ABS = path.join(HARNESS_ROOT, VUE_FIXTURE);
const VUE_SOURCE = readFileSync(VUE_FIXTURE_ABS, "utf8");

function vueArtifact(backend = "vdom") {
  const artifact = compileVueFixture(VUE_SOURCE, VUE_FIXTURE, {
    backend,
    sourceMap: true,
    isProd: false,
  });
  expect(artifact.diagnostics).toEqual([]);
  expect(artifact.map).not.toBeNull();
  return artifact;
}

/** The oracle input for a Vue corpus artifact (no golden map anywhere). */
function vueInput(artifact, overrides = {}) {
  return {
    code: artifact.code,
    map: artifact.map,
    sourceMapRequested: true,
    fixture: { path: VUE_FIXTURE, absolutePath: VUE_FIXTURE_ABS },
    sourceResolveBases: [HARNESS_ROOT, path.dirname(VUE_FIXTURE_ABS)],
    profile: MAPPING_PROFILES["vue:vdom"],
    anchors: FIXTURE_ANCHORS[VUE_FIXTURE],
    ...overrides,
  };
}

/** Replaces one decoded segment set on a map, preserving every other field. */
function withSegments(map, segments) {
  return { ...map, mappings: encodeMappings(segments) };
}

const rules = (result) => result.violations.map((v) => v.rule);

function goldenNameWhere(predicate) {
  const manifest = readGoldenManifest(GOLDENS_ROOT);
  const name = Object.keys(manifest.entries).find(predicate);
  expect(name).toBeDefined();
  return name;
}

describe("requirement 1 — map contract and bounds", () => {
  it("a real official artifact's own map satisfies the contract", () => {
    const result = validateAuthoredMapping(vueInput(vueArtifact()));
    expect(result.violations).toEqual([]);
    expect(result.ok).toBe(true);
    expect(result.stats.sourceBearingSegments).toBeGreaterThan(50);
  });

  it("map ABSENT while sourceMap was requested is a violation", () => {
    const result = validateAuthoredMapping(vueInput(vueArtifact(), { map: null }));
    expect(result.ok).toBe(false);
    expect(rules(result)).toContain("map-presence");
  });

  it("map PRESENT while sourceMap was NOT requested is a violation", () => {
    const result = validateAuthoredMapping(vueInput(vueArtifact(), { sourceMapRequested: false }));
    expect(result.ok).toBe(false);
    expect(rules(result)).toContain("map-presence");
  });

  it("map absent AND not requested is the compliant pair", () => {
    const artifact = compileVueFixture(VUE_SOURCE, VUE_FIXTURE, {
      backend: "vdom",
      sourceMap: false,
      isProd: false,
    });
    expect(artifact.map).toBeNull();
    const result = validateAuthoredMapping(
      vueInput(artifact, { map: null, sourceMapRequested: false }),
    );
    expect(result.violations).toEqual([]);
  });

  it("a version other than 3 is a violation", () => {
    const artifact = vueArtifact();
    const result = validateAuthoredMapping(
      vueInput(artifact, { map: { ...artifact.map, version: 4 } }),
    );
    expect(rules(result)).toContain("map-version");
  });

  it("MUTATION: a corrupted VLQ byte fails to decode instead of decoding garbage", () => {
    const artifact = vueArtifact();
    const original = artifact.map.mappings;
    // Prove the plant applies and is genuinely new.
    expect(original).not.toContain("%");
    const mutated = `${original.slice(0, 5)}%${original.slice(6)}`;
    expect(mutated).not.toBe(original);
    expect(mutated).toContain("%");
    const result = validateAuthoredMapping(
      vueInput(artifact, { map: { ...artifact.map, mappings: mutated } }),
    );
    expect(rules(result)).toContain("mappings-decode");
    // Restore + prove the restored input is clean again.
    expect(validateAuthoredMapping(vueInput(artifact)).violations).toEqual([]);
  });

  it("an out-of-bounds sourceIndex is a violation", () => {
    const artifact = vueArtifact();
    const segments = decodeMappings(artifact.map.mappings);
    const target = segments.findIndex((s) => s.srcIdx !== null);
    expect(target).toBeGreaterThan(-1);
    segments[target] = { ...segments[target], srcIdx: 7 };
    const result = validateAuthoredMapping(
      vueInput(artifact, { map: withSegments(artifact.map, segments) }),
    );
    expect(rules(result)).toContain("source-index-bounds");
  });

  it("an out-of-bounds nameIndex is a violation", () => {
    const artifact = vueArtifact();
    const segments = decodeMappings(artifact.map.mappings);
    const target = segments.findIndex((s) => s.srcIdx !== null);
    segments[target] = { ...segments[target], nameIdx: 3 };
    const result = validateAuthoredMapping(
      vueInput(artifact, { map: withSegments(artifact.map, segments) }),
    );
    expect(rules(result)).toContain("name-index-bounds");
  });

  it("a generated column past the end of its generated line is a violation", () => {
    const artifact = vueArtifact();
    const segments = decodeMappings(artifact.map.mappings);
    const lineLength = artifact.code.split("\n")[segments[0].genLine].length;
    segments[0] = { ...segments[0], genCol: lineLength + 1 };
    const result = validateAuthoredMapping(
      vueInput(artifact, { map: withSegments(artifact.map, segments) }),
    );
    expect(rules(result)).toContain("generated-position-bounds");
  });

  it("a generated line past the end of the generated code is a violation", () => {
    const artifact = vueArtifact();
    const segments = decodeMappings(artifact.map.mappings);
    segments[0] = { ...segments[0], genLine: artifact.code.split("\n").length + 5, genCol: 0 };
    const result = validateAuthoredMapping(
      vueInput(artifact, { map: withSegments(artifact.map, segments) }),
    );
    expect(rules(result)).toContain("generated-position-bounds");
  });

  it("an original position past the end of the authored fixture is a violation", () => {
    const artifact = vueArtifact();
    const segments = decodeMappings(artifact.map.mappings);
    const target = segments.findIndex((s) => s.srcIdx !== null);
    segments[target] = { ...segments[target], srcLine: VUE_SOURCE.split("\n").length + 3 };
    const result = validateAuthoredMapping(
      vueInput(artifact, { map: withSegments(artifact.map, segments) }),
    );
    expect(rules(result)).toContain("original-position-bounds");
  });
});

describe("requirement 2 — source identity against the real fixture on disk", () => {
  it("MUTATION: a renamed source spelling no longer resolves to the authored fixture", () => {
    const artifact = vueArtifact();
    expect(artifact.map.sources).toEqual([VUE_FIXTURE]);
    const mutated = { ...artifact.map, sources: ["fixtures/vue/some-other-file.vue"] };
    expect(mutated.sources).not.toEqual(artifact.map.sources);
    const result = validateAuthoredMapping(vueInput(artifact, { map: mutated }));
    expect(rules(result)).toContain("source-identity");
    expect(validateAuthoredMapping(vueInput(artifact)).violations).toEqual([]);
  });

  it("MUTATION: sourcesContent that differs from the real fixture bytes is a violation", () => {
    const artifact = vueArtifact();
    expect(artifact.map.sourcesContent).toEqual([VUE_SOURCE]);
    const stale = VUE_SOURCE.replace("const count = ref(0);", "const count = ref(1);");
    expect(stale).not.toBe(VUE_SOURCE);
    const result = validateAuthoredMapping(
      vueInput(artifact, { map: { ...artifact.map, sourcesContent: [stale] } }),
    );
    expect(rules(result)).toContain("sources-content");
    expect(validateAuthoredMapping(vueInput(artifact)).violations).toEqual([]);
  });

  it("the fixture bytes are read from DISK, never taken from the map's own sourcesContent", () => {
    // A map that agrees with itself but not with the file cannot pass: the
    // oracle's ground truth is the file.
    const artifact = vueArtifact();
    const forged = "<template><p>forged</p></template>\n";
    const result = validateAuthoredMapping(
      vueInput(artifact, { map: { ...artifact.map, sourcesContent: [forged] } }),
    );
    expect(rules(result)).toContain("sources-content");
  });

  it("an absent sourcesContent is permitted (the field is optional)", () => {
    const artifact = vueArtifact();
    const withoutContent = { ...artifact.map };
    delete withoutContent.sourcesContent;
    const result = validateAuthoredMapping(vueInput(artifact, { map: withoutContent }));
    expect(rules(result)).not.toContain("sources-content");
  });

  it("sourceRoot participates in the resolution", () => {
    const artifact = vueArtifact();
    const rooted = {
      ...artifact.map,
      sourceRoot: "fixtures/vue/",
      sources: ["basic-interpolation.vue"],
    };
    expect(validateAuthoredMapping(vueInput(artifact, { map: rooted })).violations).toEqual([]);
    const misrooted = { ...rooted, sourceRoot: "fixtures/svelte/" };
    expect(rules(validateAuthoredMapping(vueInput(artifact, { map: misrooted })))).toContain(
      "source-identity",
    );
  });
});

describe("the oracle covers the whole committed corpus, every backend and target", () => {
  for (const backend of ["vdom", "vapor", "ssr"]) {
    for (const fixture of [
      "fixtures/vue/basic-interpolation.vue",
      "fixtures/vue/props-emit.vue",
      "fixtures/vue/slots.vue",
    ]) {
      it(`vue ${backend} ${fixture}`, () => {
        const absolute = path.join(HARNESS_ROOT, fixture);
        const source = readFileSync(absolute, "utf8");
        const artifact = compileVueFixture(source, fixture, {
          backend,
          sourceMap: true,
          isProd: false,
        });
        const result = validateAuthoredMapping({
          code: artifact.code,
          map: artifact.map,
          sourceMapRequested: true,
          fixture: { path: fixture, absolutePath: absolute },
          sourceResolveBases: [HARNESS_ROOT, path.dirname(absolute)],
          profile: MAPPING_PROFILES[`vue:${backend}`],
          anchors: FIXTURE_ANCHORS[fixture],
        });
        expect(result.violations).toEqual([]);
      });
    }
  }

  for (const generate of ["client", "server"]) {
    for (const [fixture, runes] of [
      ["fixtures/svelte/basic-runes.svelte", true],
      ["fixtures/svelte/props-events.svelte", true],
      ["fixtures/svelte/legacy-slots.svelte", false],
    ]) {
      it(`svelte ${generate} ${fixture}`, () => {
        const absolute = path.join(HARNESS_ROOT, fixture);
        const source = readFileSync(absolute, "utf8");
        const artifact = compileSvelteFixture(source, fixture, {
          generate,
          runes,
          dev: false,
          sourceMap: true,
        });
        const result = validateAuthoredMapping({
          code: artifact.code,
          map: artifact.map,
          sourceMapRequested: true,
          fixture: { path: fixture, absolutePath: absolute },
          sourceResolveBases: [HARNESS_ROOT, path.dirname(absolute)],
          profile: MAPPING_PROFILES[`svelte:${generate}`],
          anchors: FIXTURE_ANCHORS[fixture],
        });
        expect(result.violations).toEqual([]);
      });
    }
  }
});

// Axis wiring, and the defect this axis replaces.

describe("the mapping axis, end to end through the acceptance primitive", () => {
  it("a committed golden accepted as its own candidate RUNS the axis over real anchors", async () => {
    const name = goldenNameWhere((n) => n === "vue/basic-interpolation__vdom__map1__prod0");
    const golden = readGoldenByName(GOLDENS_ROOT, name);
    const result = await checkCandidate({
      goldenName: name,
      candidate: { code: golden.code, map: golden.map, diagnostics: golden.diagnostics },
    });
    expect(result.reasons).toEqual([]);
    expect(result.axes.mapping.status).toBe("ran");
    // The axis genuinely did work: real segments classified, real anchors
    // required. A context that silently degraded to nothing would show zero.
    expect(result.report.mapping.stats.sourceBearingSegments).toBeGreaterThan(50);
    expect(result.report.mapping.stats.anchors).toBe(2);
  });

  it("MUTATION: a candidate whose map drops the template anchor FAILS the acceptance check", async () => {
    const name = goldenNameWhere((n) => n === "vue/basic-interpolation__vdom__map1__prod0");
    const golden = readGoldenByName(GOLDENS_ROOT, name);
    const anchor = FIXTURE_ANCHORS[VUE_FIXTURE][1];
    const segments = decodeMappings(golden.map.mappings);
    const surviving = segments.filter(
      (segment) =>
        !(
          segment.srcIdx !== null &&
          segment.srcLine === anchor.line &&
          segment.srcCol >= anchor.column &&
          segment.srcCol < anchor.column + anchor.text.length
        ),
    );
    expect(surviving.length).toBeLessThan(segments.length); // the plant applied
    const result = await checkCandidate({
      goldenName: name,
      candidate: {
        code: golden.code,
        map: { ...golden.map, mappings: encodeMappings(surviving) },
        diagnostics: golden.diagnostics,
      },
    });
    expect(result.verdict).toBe("fail");
    expect(result.axes.mapping.status).toBe("ran");
    expect(
      result.reasons.some((reason) =>
        reason.startsWith("candidate source map is not truthful about its own output"),
      ),
    ).toBe(true);
  });

  it("MUTATION: a map1 candidate that produced NO map fails on presence", async () => {
    const name = goldenNameWhere((n) => n === "vue/basic-interpolation__vdom__map1__prod0");
    const golden = readGoldenByName(GOLDENS_ROOT, name);
    expect(golden.map).not.toBeNull(); // the golden really was a mapped compile
    const result = await checkCandidate({
      goldenName: name,
      candidate: { code: golden.code, map: null, diagnostics: golden.diagnostics },
    });
    expect(result.verdict).toBe("fail");
    expect(result.reasons.join(" ")).toContain("map-presence");
  });

  it("REGRESSION: a cosmetically-different candidate with a CORRECT map passes, though its `mappings` field differs from the golden's", async () => {
    // This is the whole point of the replacement. The candidate below emits
    // the same module with two extra blank lines — a difference the
    // Compiled-Output Conformance rule explicitly permits — and carries the
    // map that correctly describes THAT text. Its `mappings` field is
    // necessarily different from the golden's, because the two maps describe
    // two different generated documents. The old candidate-vs-official
    // comparison rejected exactly this candidate; the authored-source oracle
    // accepts it, and (per the case above) still rejects a wrong map.
    const name = goldenNameWhere((n) => n === "vue/basic-interpolation__vdom__map1__prod0");
    const golden = readGoldenByName(GOLDENS_ROOT, name);
    const shiftedCode = `\n\n${golden.code}`;
    const shifted = decodeMappings(golden.map.mappings).map((segment) => ({
      ...segment,
      genLine: segment.genLine + 2,
    }));
    const shiftedMappings = encodeMappings(shifted);
    expect(shiftedMappings).not.toBe(golden.map.mappings); // genuinely different bytes
    const result = await checkCandidate({
      goldenName: name,
      candidate: {
        code: shiftedCode,
        map: { ...golden.map, mappings: shiftedMappings },
        diagnostics: golden.diagnostics,
      },
    });
    expect(result.reasons).toEqual([]);
    expect(result.verdict).toBe("pass");
    expect(result.axes.mapping.status).toBe("ran");
  });
});

// Position binding and generated-only scaffolding, driven through the REAL
// acceptance primitive.
//
// Every case below was ACCEPTED (`verdict: "pass"`, `reasons: []`) by the
// previous oracle: the two tightened relations bound their segments to a
// LINE or to a text SUFFIX rather than to a position, and requirement 6
// received an empty range list at this boundary and therefore never ran on a
// candidate at all. Each plant is asserted to change the map bytes before it
// is submitted, so a plant that failed to apply cannot read as a pass.

const VUE_GOLDEN = "vue/basic-interpolation__vdom__map1__prod0";
const SVELTE_GOLDEN = "svelte/basic-runes__client__runes1__dev0";

/** Submits `golden`'s own artifact with a mutated segment list. */
async function submitMutatedMap(name, mutate, mapOverrides = {}) {
  const golden = readGoldenByName(GOLDENS_ROOT, name);
  const segments = decodeMappings(golden.map.mappings);
  // The plant genuinely applied: a no-op plant would make every assertion
  // below a statement about the unmutated golden. Compared against the
  // RE-ENCODED original rather than the golden's raw `mappings` bytes: six
  // committed goldens carry trailing empty-line separators that an
  // encode/decode round trip drops (the decoded segment lists are
  // identical), so on those a no-op plant differs from the raw field and
  // this guard would be satisfied without any mutation having happened.
  const unmutated = encodeMappings(decodeMappings(golden.map.mappings));
  const mutatedMappings = encodeMappings(mutate(segments, golden));
  expect(mutatedMappings).not.toBe(unmutated);
  return checkCandidate({
    goldenName: name,
    candidate: {
      code: golden.code,
      map: { ...golden.map, mappings: mutatedMappings, ...mapOverrides },
      diagnostics: golden.diagnostics,
    },
  });
}

const mappingReason = (result) =>
  result.reasons.find((reason) =>
    reason.startsWith("candidate source map is not truthful about its own output"),
  ) ?? "";

describe("requirement 6 runs on a CANDIDATE, not only where assembly geometry is in scope", () => {
  it("the acceptance path derives generated-only ranges from the candidate's own code", async () => {
    const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
    const result = await checkCandidate({
      goldenName: VUE_GOLDEN,
      candidate: { code: golden.code, map: golden.map, diagnostics: golden.diagnostics },
    });
    expect(result.reasons).toEqual([]);
    // Non-zero is the whole point: an empty rail is what made the
    // requirement inert here, and it looks identical to a clean pass.
    expect(result.report.mapping.stats.syntheticRanges).toBeGreaterThanOrEqual(3);
  });

  it("MUTATION: fabricated authored provenance over a synthesized object key (`__name:`) is REJECTED", async () => {
    const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
    const lines = golden.code.split("\n");
    const genLine = lines.findIndex((line) => line.trimStart().startsWith("__name:"));
    expect(genLine).toBeGreaterThan(-1);
    const genCol = lines[genLine].indexOf("__name");
    // Nothing maps there today, so the plant is genuinely new.
    expect(
      decodeMappings(golden.map.mappings).some(
        (segment) => segment.genLine === genLine && segment.genCol === genCol,
      ),
    ).toBe(false);
    const result = await submitMutatedMap(VUE_GOLDEN, (segments) => [
      ...segments,
      { genLine, genCol, srcIdx: 0, srcLine: 3, srcCol: 6, nameIdx: null },
    ]);
    expect(result.verdict).toBe("fail");
    expect(mappingReason(result)).toContain("synthetic-provenance");
  });

  it("MUTATION: fabricated authored provenance over the module's own assembly footer is REJECTED", async () => {
    const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
    const lines = golden.code.split("\n");
    const genLine = lines.findIndex((line) => line.startsWith("_sfc_main.render = render"));
    expect(genLine).toBeGreaterThan(-1);
    const result = await submitMutatedMap(VUE_GOLDEN, (segments) => [
      ...segments,
      { genLine, genCol: 0, srcIdx: 0, srcLine: 12, srcCol: 6, nameIdx: null },
    ]);
    expect(result.verdict).toBe("fail");
    expect(mappingReason(result)).toContain("synthetic-provenance");
  });

  it("MUTATION: fabricated authored provenance over a synthesized setup parameter (`__props`) is REJECTED", async () => {
    const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
    const lines = golden.code.split("\n");
    const genLine = lines.findIndex((line) => line.includes("setup(__props"));
    expect(genLine).toBeGreaterThan(-1);
    const genCol = lines[genLine].indexOf("__props");
    const result = await submitMutatedMap(VUE_GOLDEN, (segments) => [
      ...segments,
      { genLine, genCol, srcIdx: 0, srcLine: 0, srcCol: 0, nameIdx: null },
    ]);
    expect(result.verdict).toBe("fail");
    expect(mappingReason(result)).toContain("synthetic-provenance");
  });

  it("a runtime-helper import is generated-only, but the AUTHORED import beside it is not", async () => {
    const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
    const ranges = generatedOnlyRanges(golden.code, MAPPING_PROFILES["vue:vdom"]);
    const lines = golden.code.split("\n");
    const authoredImport = lines.findIndex((line) => line.startsWith('import { ref } from "vue"'));
    const helperImport = lines.findIndex((line) => line.includes("toDisplayString as _"));
    expect(authoredImport).toBeGreaterThan(-1);
    expect(helperImport).toBeGreaterThan(-1);
    expect(ranges.some((range) => range.startLine === helperImport)).toBe(true);
    // The over-broadening control: sweeping in every top-level import would
    // make an authored `import { ref } from "vue"` unmappable.
    expect(ranges.some((range) => range.startLine === authoredImport)).toBe(false);
  });
});

/**
 * Classifies a hypothetical `(generated word-start identifier) -> (authored
 * position)` pair against a hand-authored fixture body. The fixture is a real
 * carrier document (script block included), because the binding-pattern index
 * the relation consults is derived by PARSING the authored script.
 */
function authoredProbe(srcLines) {
  return {
    classify: (genText, srcLine, srcCol) =>
      classifySegment({
        gen: { kind: "word-start", text: genText, rest: genText },
        src: tokenAt(srcLines, srcLine, srcCol),
        genLines: [genText],
        srcLines,
        segment: { genLine: 0, genCol: 0, srcLine, srcCol },
        profile: MAPPING_PROFILES["svelte:client"],
      }),
  };
}

describe("position binding: a segment must name its authored POSITION, not merely matching text", () => {
  it("MUTATION: verbatim-carry rejects two different tokens that share a trailing substring", async () => {
    const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
    const genLines = golden.code.split("\n");
    const srcLines = readFileSync(VUE_FIXTURE_ABS, "utf8").split("\n");
    // The real segment: generated `import`@5 carried from the authored
    // `import`@5 of the script block — both word-interior, same offset.
    const target = decodeMappings(golden.map.mappings).find(
      (segment) => segment.genLine === 0 && segment.genCol === 5 && segment.srcIdx !== null,
    );
    expect(target).toBeDefined();
    const real = tokenAt(srcLines, target.srcLine, target.srcCol);
    expect(real).toMatchObject({ kind: "word-interior", text: "import", rest: "t" });
    // The decoy: a DIFFERENT authored token whose tail from the mapped
    // column is byte-identical to the real one's. That shared suffix is all
    // the old relation compared.
    const decoy = { line: 0, column: 6 };
    const fake = tokenAt(srcLines, decoy.line, decoy.column);
    expect(fake.rest).toBe(real.rest); // the suffix genuinely matches …
    expect(fake.text).not.toBe(real.text); // … while the token does not
    expect(fake.kind).toBe(real.kind); // … and the KIND matches too
    const result = await submitMutatedMap(VUE_GOLDEN, (segments) =>
      segments.map((segment) =>
        segment.genLine === target.genLine && segment.genCol === target.genCol
          ? { ...segment, srcLine: decoy.line, srcCol: decoy.column }
          : segment,
      ),
    );
    expect(result.verdict).toBe("fail");
    expect(mappingReason(result)).toContain("segment-provenance");
  });

  it("MUTATION: synthesized-local-for-authored-name rejects a re-point to the wrong token on the RIGHT line", async () => {
    const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
    const srcLines = readFileSync(VUE_FIXTURE_ABS, "utf8").split("\n");
    const genLines = golden.code.split("\n");
    // A generated identifier mapped to its own authored declaration…
    const target = decodeMappings(golden.map.mappings).find(
      (segment) =>
        segment.srcIdx !== null &&
        tokenAt(genLines, segment.genLine, segment.genCol).text === "items" &&
        tokenAt(srcLines, segment.srcLine, segment.srcCol).text === "items",
    );
    expect(target).toBeDefined();
    // …re-pointed to column 0 of the SAME authored line — the `const`
    // keyword. The line still contains the word `items`, which is exactly
    // what the old line-scoped tie accepted.
    expect(srcLines[target.srcLine]).toContain("items");
    expect(tokenAt(srcLines, target.srcLine, 0).text).toBe("const");
    const result = await submitMutatedMap(VUE_GOLDEN, (segments) =>
      segments.map((segment) =>
        segment.genLine === target.genLine && segment.genCol === target.genCol
          ? { ...segment, srcCol: 0 }
          : segment,
      ),
    );
    expect(result.verdict).toBe("fail");
    expect(mappingReason(result)).toContain("segment-provenance");
  });

  it("MUTATION: a Svelte synthesized local re-pointed to column 0 of its own authored line is REJECTED", async () => {
    // The reviewer's case: generated `p_1` maps to the authored `<p>` element
    // NAME. Re-pointing only its COLUMN, to the `<` on the same line, was
    // accepted by the line-scoped tie.
    const golden = readGoldenByName(GOLDENS_ROOT, SVELTE_GOLDEN);
    const genLines = golden.code.split("\n");
    const srcLines = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/svelte/basic-runes.svelte"),
      "utf8",
    ).split("\n");
    const target = decodeMappings(golden.map.mappings).find(
      (segment) =>
        segment.srcIdx !== null &&
        tokenAt(genLines, segment.genLine, segment.genCol).text === "p_1" &&
        tokenAt(srcLines, segment.srcLine, segment.srcCol).text === "p",
    );
    expect(target).toBeDefined();
    const decoyColumn = srcLines[target.srcLine].indexOf("<");
    expect(decoyColumn).toBeGreaterThan(-1);
    expect(decoyColumn).not.toBe(target.srcCol);
    // The line still contains the whole word `p`, which is all the old tie
    // required.
    const result = await submitMutatedMap(SVELTE_GOLDEN, (segments) =>
      segments.map((segment) =>
        segment.srcIdx !== null &&
        segment.srcLine === target.srcLine &&
        segment.srcCol === target.srcCol
          ? { ...segment, srcCol: decoyColumn }
          : segment,
      ),
    );
    expect(result.verdict).toBe("fail");
    expect(mappingReason(result)).toContain("segment-provenance");
  });

  it("the destructured-binding-pattern relation is itself position-exact", () => {
    // The pinned Svelte compiler anchors a local hoisted out of a
    // destructuring pattern at the pattern's `{`. That correspondence is
    // accepted (the corpus test above covers it); the SAME relation must
    // reject the pattern brace of a pattern that does not bind the name.
    const { classify } = authoredProbe([
      "<script>",
      "  let { label, disabled = false } = $props();",
      "  let { other } = $props();",
      "</script>",
    ]);
    expect(classify("disabled", 1, 6)).toBe("destructured-binding-pattern");
    // A different pattern's brace does not bind it.
    expect(classify("disabled", 2, 6)).toBeNull();
    // Neither does a position that is not the pattern's opening brace.
    expect(classify("disabled", 1, 2)).toBeNull();
  });

  // A1: the relation is position-exact because a PARSER says the brace opens a
  // binding pattern that binds this name — not because the word appears
  // somewhere inside a brace span. Every case below survived the previous
  // brace-scan implementation.

  it("MUTATION: an ACCEPTING-but-WRONG sibling brace containing the same word is REJECTED", async () => {
    // The reviewer's exploit, end to end through the acceptance primitive:
    // the REAL official segment for the hoisted `disabled` local is
    // re-pointed from the authored destructuring pattern at 1:6 to the
    // template shorthand brace `{disabled}` at 8:8. The word `disabled` sits
    // inside that brace too, which is all the brace scan required.
    const name = "svelte/props-events__client__runes1__dev0";
    const golden = readGoldenByName(GOLDENS_ROOT, name);
    const srcLines = readFileSync(
      path.join(HARNESS_ROOT, "fixtures/svelte/props-events.svelte"),
      "utf8",
    ).split("\n");
    // The decoy really is a `{` and really does contain the word.
    expect(tokenAt(srcLines, 8, 8)).toMatchObject({ kind: "punct", text: "{" });
    expect(srcLines[8].slice(8)).toContain("disabled");
    // The real segment really does exist at the authored pattern brace.
    const real = decodeMappings(golden.map.mappings).filter(
      (segment) => segment.srcIdx !== null && segment.srcLine === 1 && segment.srcCol === 6,
    );
    expect(real.length).toBeGreaterThan(0);
    const result = await submitMutatedMap(name, (segments) =>
      segments.map((segment) =>
        segment.srcIdx !== null && segment.srcLine === 1 && segment.srcCol === 6
          ? { ...segment, srcLine: 8, srcCol: 8 }
          : segment,
      ),
    );
    expect(result.verdict).toBe("fail");
    expect(mappingReason(result)).toContain("segment-provenance");
  });

  it("MUTATION: a Vue interpolation brace containing the generated name is REJECTED", async () => {
    // The same class on the Vue profile, where this relation has zero
    // legitimate corpus use: a fabricated segment at the generated `count`
    // re-pointed to the inner `{` of `{{ count }}`.
    const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
    const genLines = golden.code.split("\n");
    const srcLines = readFileSync(VUE_FIXTURE_ABS, "utf8").split("\n");
    const brace = srcLines[9].indexOf("{{") + 1;
    expect(tokenAt(srcLines, 9, brace)).toMatchObject({ kind: "punct", text: "{" });
    expect(srcLines[9].slice(brace)).toContain("count");
    const target = decodeMappings(golden.map.mappings).find(
      (segment) =>
        segment.srcIdx !== null &&
        tokenAt(genLines, segment.genLine, segment.genCol).text === "count",
    );
    expect(target).toBeDefined();
    const result = await submitMutatedMap(VUE_GOLDEN, (segments) =>
      segments.map((segment) =>
        segment.genLine === target.genLine && segment.genCol === target.genCol
          ? { ...segment, srcLine: 9, srcCol: brace }
          : segment,
      ),
    );
    expect(result.verdict).toBe("fail");
    expect(mappingReason(result)).toContain("segment-provenance");
  });

  it("a NESTED pattern's names are not bound by the OUTER pattern's brace", () => {
    const { classify } = authoredProbe([
      "<script>",
      "  let { data: { inner }, plain } = $props();",
      "</script>",
    ]);
    // The outer brace binds `plain` at its own level …
    expect(classify("plain", 1, 6)).toBe("destructured-binding-pattern");
    // … but NOT the nested pattern's `inner`, which the brace scan accepted
    // because the word sat inside the outer span.
    expect(classify("inner", 1, 6)).toBeNull();
    // The nested brace itself is not a declaration-position pattern either.
    expect(classify("inner", 1, 14)).toBeNull();
  });

  it("a pattern's own brace span bounds it: a name in a LATER pattern is not bound", () => {
    const { classify } = authoredProbe([
      "<script>",
      "  let { first } = $props();",
      "  let { second } = $props();",
      "</script>",
    ]);
    expect(classify("first", 1, 6)).toBe("destructured-binding-pattern");
    // The old scan ran to the matching `}` — deleting that bound let it run
    // to EOF and reach `second`. The parsed index cannot reach it at all.
    expect(classify("second", 1, 6)).toBeNull();
  });

  it("a WHOLE-word binding is required: a longer name containing it does not bind", () => {
    const { classify } = authoredProbe([
      "<script>",
      "  let { disabledFlag } = $props();",
      "</script>",
    ]);
    expect(classify("disabledFlag", 1, 6)).toBe("destructured-binding-pattern");
    expect(classify("disabled", 1, 6)).toBeNull();
  });

  it("a property KEY, a default-value EXPRESSION and a COMMENT are not bindings", () => {
    // The conformance reviewer's three cases. Each has the word at brace
    // depth 1 and was accepted by the scan.
    expect(
      authoredProbe(["<script>", "  let { disabled: other } = $props();", "</script>"]).classify(
        "disabled",
        1,
        6,
      ),
    ).toBeNull();
    expect(
      authoredProbe(["<script>", "  let { other = disabled } = $props();", "</script>"]).classify(
        "disabled",
        1,
        6,
      ),
    ).toBeNull();
    expect(
      authoredProbe([
        "<script>",
        "  let { /* disabled */ other } = $props();",
        "</script>",
      ]).classify("disabled", 1, 6),
    ).toBeNull();
  });

  it("an object LITERAL brace and a non-binding-position pattern are not bindings", () => {
    expect(
      authoredProbe(["<script>", "  const state = { disabled: 1 };", "</script>"]).classify(
        "disabled",
        1,
        16,
      ),
    ).toBeNull();
    // An assignment TARGET pattern declares nothing.
    expect(
      authoredProbe([
        "<script>",
        "  let disabled;",
        "  ({ disabled } = source);",
        "</script>",
      ]).classify("disabled", 2, 3),
    ).toBeNull();
  });

  it("function and catch parameters ARE declaration-position patterns", () => {
    expect(
      authoredProbe([
        "<script>",
        "  function f({ alpha }) { return alpha; }",
        "</script>",
      ]).classify("alpha", 1, 13),
    ).toBe("destructured-binding-pattern");
    expect(
      authoredProbe([
        "<script>",
        "  try { go(); } catch ({ message }) { report(message); }",
        "</script>",
      ]).classify("message", 1, 23),
    ).toBe("destructured-binding-pattern");
  });

  it("an authored fixture whose script does not PARSE yields no bindings (fail-closed)", () => {
    expect(
      authoredProbe(["<script>", "  let { broken = = } = $props();", "</script>"]).classify(
        "broken",
        1,
        6,
      ),
    ).toBeNull();
    // …and a fixture with no script block at all likewise binds nothing.
    expect(authoredProbe(["<div>{ disabled }</div>"]).classify("disabled", 0, 5)).toBeNull();
  });
});

// A2/A3: fabricated authored provenance over compiler scaffolding, in BOTH
// frameworks. Every case below was ACCEPTED (`verdict: "pass"`, `reasons: []`)
// before the generated-only rail was widened past its four original shapes,
// and every one is driven through the real acceptance primitive with
// `candidate.code === golden.code`, so only the mapping axis can fail.

/** Adds ONE fabricated source-bearing segment at a generated position. */
async function fabricateSegmentAt(goldenName, genLine, genCol, srcLine, srcCol) {
  const golden = readGoldenByName(GOLDENS_ROOT, goldenName);
  // The plant is genuinely NEW: nothing maps to that generated position today.
  expect(
    decodeMappings(golden.map.mappings).some(
      (segment) => segment.genLine === genLine && segment.genCol === genCol,
    ),
  ).toBe(false);
  return submitMutatedMap(goldenName, (segments) => [
    ...segments,
    { genLine, genCol, srcIdx: 0, srcLine, srcCol, nameIdx: null },
  ]);
}

/** The (line, column) of `token` on the first generated line matching `where`. */
function generatedTokenAt(code, where, token) {
  const lines = code.split("\n");
  const line = lines.findIndex((text) => where.test(text));
  expect(line, `no generated line matches ${where}`).toBeGreaterThan(-1);
  const column = lines[line].indexOf(token);
  expect(column, `${token} not on line ${line}`).toBeGreaterThan(-1);
  return { line, column };
}

describe("A2 — fabricated provenance over Vue's synthesized script wrapper is REJECTED", () => {
  for (const [label, where, token] of [
    ["a bare helper call statement (`__expose();`)", /^\s*__expose\(\);?\s*$/, "__expose"],
    [
      "a generated binding handed to a helper call (`Object.defineProperty(__returned__, …)`)",
      /Object\.defineProperty\(__returned__/,
      "__returned__",
    ],
    ["a bare generated return (`return __returned__`)", /^\s*return __returned__/, "__returned__"],
  ]) {
    it(`MUTATION: ${label}`, async () => {
      const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
      const { line, column } = generatedTokenAt(golden.code, where, token);
      const result = await fabricateSegmentAt(VUE_GOLDEN, line, column, 12, 6);
      expect(result.verdict).toBe("fail");
      expect(mappingReason(result)).toContain("synthetic-provenance");
    });
  }

  it("the ARGUMENT PAYLOAD of a synthesized call stays mappable (the deliberate bound)", () => {
    // `Object.defineProperty(__returned__, '__isScriptSetup', …)`'s string
    // literal is not claimed: a literal inside a synthesized call genuinely
    // can carry authored provenance, and claiming the whole statement would
    // reject correct maps. This is the boundary the module doc states.
    const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
    const { line } = generatedTokenAt(
      golden.code,
      /Object\.defineProperty\(__returned__/,
      "__returned__",
    );
    const literal = golden.code.split("\n")[line].indexOf("'__isScriptSetup'");
    expect(literal).toBeGreaterThan(-1);
    const ranges = generatedOnlyRanges(
      golden.code,
      MAPPING_PROFILES["vue:vdom"],
      readFileSync(VUE_FIXTURE_ABS, "utf8").split("\n"),
    );
    const covers = (l, c) =>
      ranges.some(
        (range) =>
          range.startLine <= l &&
          range.endLine >= l &&
          (range.startLine < l || range.startColumn <= c) &&
          (range.endLine > l || range.endColumn > c),
      );
    expect(covers(line, literal)).toBe(false);
  });
});

describe("A3 — the generated-only rail is enforced for SVELTE, not only Vue", () => {
  const SVELTE_PROPS = "svelte/props-events__client__runes1__dev0";
  const SVELTE_SOURCE = "fixtures/svelte/props-events.svelte";

  it("Svelte ranges are genuinely derived, and name the framework's own scaffolding", () => {
    // Disabling range derivation for Svelte profiles only used to pass the
    // whole suite: every range assertion in the tree spoke about Vue.
    const golden = readGoldenByName(GOLDENS_ROOT, SVELTE_PROPS);
    const ranges = generatedOnlyRanges(
      golden.code,
      MAPPING_PROFILES["svelte:client"],
      readFileSync(path.join(HARNESS_ROOT, SVELTE_SOURCE), "utf8").split("\n"),
    );
    expect(ranges.length).toBeGreaterThanOrEqual(5);
    const labels = ranges.map((range) => range.label);
    expect(labels).toContain('runtime-helper import "svelte/internal/client"');
    expect(labels).toContain('runtime-helper import "svelte/internal/disclose-version"');
    expect(labels).toContain("generated helper call $");
    expect(labels).toContain("emitted declaration root");
    // Every derived range spans the text it claims, read back by its own
    // coordinates out of the Svelte artifact.
    const lines = golden.code.split("\n");
    for (const range of ranges) {
      const text =
        range.startLine === range.endLine
          ? lines[range.startLine].slice(range.startColumn, range.endColumn)
          : lines[range.startLine].slice(range.startColumn);
      if (range.label.startsWith("runtime-helper import"))
        expect(text.startsWith("import ")).toBe(true);
      else if (range.label.startsWith("generated helper call"))
        expect(text.split(".")[0]).toBe("$");
      else expect(range.label.endsWith(text)).toBe(true);
    }
    // The render-scope root is NOT claimed: `$$props` carries authored
    // provenance through `context-binding-prefix`.
    expect(labels.some((label) => label.endsWith("$$props"))).toBe(false);
  });

  for (const [label, where, token] of [
    ["the setup-scope push (`$.push($$props, true)`)", /\$\.push\(/, "$"],
    ["the setup-scope pop (`$.pop()`)", /\$\.pop\(/, "$"],
    ["the module-level delegation footer (`$.delegate([…])`)", /^\$\.delegate\(/, "$"],
  ]) {
    it(`MUTATION: fabricated provenance over ${label} is REJECTED`, async () => {
      const golden = readGoldenByName(GOLDENS_ROOT, SVELTE_PROPS);
      const { line, column } = generatedTokenAt(golden.code, where, token);
      const result = await fabricateSegmentAt(SVELTE_PROPS, line, column, 3, 11);
      expect(result.verdict).toBe("fail");
      expect(mappingReason(result)).toContain("synthetic-provenance");
    });
  }

  it("MUTATION: fabricated provenance inside the zero-specifier runtime import is REJECTED", async () => {
    // `import 'svelte/internal/disclose-version'` binds nothing, so a
    // specifier-based rule could not see it at all.
    const golden = readGoldenByName(GOLDENS_ROOT, SVELTE_PROPS);
    const { line } = generatedTokenAt(golden.code, /disclose-version/, "svelte/internal");
    const result = await fabricateSegmentAt(SVELTE_PROPS, line, 7, 3, 11);
    expect(result.verdict).toBe("fail");
    expect(mappingReason(result)).toContain("synthetic-provenance");
  });
});

describe("A2 — a name the AUTHOR wrote is never claimed as compiler-introduced", () => {
  const profile = MAPPING_PROFILES["vue:vdom"];
  const AUTHORED = [
    "<script setup>",
    'import { ref as _ref } from "vue";',
    "const state = { _authored: 1 };",
    "const _component = _ref(null);",
    "</script>",
    "",
    "<template><div>{{ state }}</div></template>",
  ];

  it("an authored alias, key, parameter, plumbing statement and default export all stay mappable", () => {
    const generated =
      [
        'import { ref as _ref } from "vue";',
        "const state = { _authored: 1 };",
        "_component.value = authored;",
        "export default _component;",
      ].join("\n") + "\n";
    // Without the authored side, spelling alone sweeps every one of them in —
    // which is exactly the false-REJECT class this closes.
    expect(generatedOnlyRanges(generated, profile).length).toBeGreaterThan(0);
    expect(generatedOnlyRanges(generated, profile, AUTHORED)).toEqual([]);
  });

  it("genuine compiler helpers in the SAME module are still claimed", () => {
    // The exclusion is name-scoped, not module-scoped: a helper the author
    // never wrote stays covered even beside authored underscore names.
    const generated =
      [
        'import { ref as _ref } from "vue";',
        'import { toDisplayString as _toDisplayString } from "vue";',
        "const _hoisted_1 = 1;",
      ].join("\n") + "\n";
    const labels = generatedOnlyRanges(generated, profile, AUTHORED).map((range) => range.label);
    expect(labels).toContain('runtime-helper import "vue"');
    expect(labels).toContain("emitted declaration _toDisplayString");
    expect(labels).toContain("emitted declaration _hoisted_1");
    expect(labels).not.toContain("emitted declaration _ref");
  });

  it("a REAL segment over an authored underscore binding is ACCEPTED end to end", () => {
    // The full oracle, against a real authored carrier on disk — the same
    // `validateAuthoredMapping` the acceptance primitive drives. (The
    // acceptance primitive itself reads the fixture named by the golden's
    // provenance, and no committed fixture carries an emitted-shaped authored
    // name, so the authored side is supplied as a real temp file here.)
    const directory = mkdtempSync(path.join(tmpdir(), "bf2-authored-"));
    const absolutePath = path.join(directory, "authored.vue");
    try {
      writeFileSync(absolutePath, `${AUTHORED.join("\n")}\n`, "utf8");
      const code = 'import { ref as _ref } from "vue";\nconst state = { _authored: 1 };\n';
      // A truthful map: generated `_ref`@19 <- authored `_ref`@19 on line 1,
      // and generated `_authored`@16 <- authored `_authored`@16 on line 2.
      const segments = [
        { genLine: 0, genCol: 19, srcIdx: 0, srcLine: 1, srcCol: 19, nameIdx: null },
        { genLine: 1, genCol: 16, srcIdx: 0, srcLine: 2, srcCol: 16, nameIdx: null },
      ];
      const result = validateAuthoredMapping({
        code,
        map: {
          version: 3,
          sources: ["authored.vue"],
          names: [],
          mappings: encodeMappings(segments),
        },
        sourceMapRequested: true,
        fixture: { path: "authored.vue", absolutePath },
        sourceResolveBases: [directory],
        profile,
        anchors: [],
      });
      expect(result.violations).toEqual([]);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});

describe("A5 — a map may spell its sources with the other platform's separator", () => {
  it("a `\\`-spelled sourceRoot still resolves to the authored fixture", () => {
    const artifact = vueArtifact();
    // A map produced on Windows spells `fixtures\\vue\\` and
    // `basic-interpolation.vue`. Without separator normalization the
    // posix join leaves `fixtures\\vue\\basic-interpolation.vue`, which
    // resolves to no file at all. Every other sourceRoot test in this file
    // spells its sources with `/`, so none of them can see the difference.
    const windows = {
      ...artifact.map,
      sourceRoot: "fixtures\\vue\\",
      sources: ["basic-interpolation.vue"],
    };
    expect(rules(validateAuthoredMapping(vueInput(artifact, { map: windows })))).not.toContain(
      "source-identity",
    );
    // …and a `\`-spelled SOURCE resolves too.
    const windowsSource = { ...artifact.map, sources: ["fixtures\\vue\\basic-interpolation.vue"] };
    expect(
      rules(validateAuthoredMapping(vueInput(artifact, { map: windowsSource }))),
    ).not.toContain("source-identity");
    // The control: a `\`-spelled path to a DIFFERENT file still fails.
    const wrong = { ...artifact.map, sources: ["fixtures\\vue\\slots.vue"] };
    expect(rules(validateAuthoredMapping(vueInput(artifact, { map: wrong })))).toContain(
      "source-identity",
    );
  });
});

describe("a segment's `names` entry is a claim about a symbol, and is checked as one", () => {
  /** A real segment carrying an identifier on both sides. */
  function identifierSegment(golden) {
    const genLines = golden.code.split("\n");
    const srcLines = readFileSync(VUE_FIXTURE_ABS, "utf8").split("\n");
    const segment = decodeMappings(golden.map.mappings).find((candidate) => {
      if (candidate.srcIdx === null) return false;
      const gen = tokenAt(genLines, candidate.genLine, candidate.genCol);
      const src = tokenAt(srcLines, candidate.srcLine, candidate.srcCol);
      return gen.kind === "word-start" && src.kind === "word-start" && gen.text === src.text;
    });
    expect(segment).toBeDefined();
    return { segment, name: tokenAt(genLines, segment.genLine, segment.genCol).text };
  }

  it("GREEN: a named segment whose `names` entry IS its symbol is accepted", async () => {
    const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
    expect(golden.map.names).toEqual([]); // the plant genuinely introduces the field
    const { segment, name } = identifierSegment(golden);
    const result = await submitMutatedMap(
      VUE_GOLDEN,
      (segments) =>
        segments.map((candidate) =>
          candidate.genLine === segment.genLine && candidate.genCol === segment.genCol
            ? { ...candidate, nameIdx: 0 }
            : candidate,
        ),
      { names: [name] },
    );
    expect(result.reasons).toEqual([]);
    expect(result.verdict).toBe("pass");
  });

  it("MUTATION: an EMPTY-STRING `names` entry is REJECTED", async () => {
    // A name is a claim about a SYMBOL, and `""` names none. The requirement
    // that carries this is `declared.length > 0`, and it is only observable
    // on a segment whose ORIGINAL position is end-of-line — the pinned Vue
    // compiler really does emit those, and there `src.text` is itself `""`,
    // so the token-equality readings would otherwise admit the empty name.
    const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
    const srcLines = readFileSync(VUE_FIXTURE_ABS, "utf8").split("\n");
    const atEol = decodeMappings(golden.map.mappings).find(
      (candidate) =>
        candidate.srcIdx !== null &&
        tokenAt(srcLines, candidate.srcLine, candidate.srcCol).kind === "eol",
    );
    expect(atEol).toBeDefined();
    expect(tokenAt(srcLines, atEol.srcLine, atEol.srcCol).text).toBe("");
    const result = await submitMutatedMap(
      VUE_GOLDEN,
      (segments) =>
        segments.map((candidate) =>
          candidate.genLine === atEol.genLine && candidate.genCol === atEol.genCol
            ? { ...candidate, nameIdx: 0 }
            : candidate,
        ),
      { names: [""] },
    );
    expect(result.verdict).toBe("fail");
    expect(mappingReason(result)).toContain("name-token-relation");
  });

  it("BOTH readings are exercised independently, not merely their intersection", async () => {
    // The GREEN case above picks a segment where the authored and generated
    // tokens are the SAME word, so it cannot tell the two admissible readings
    // apart. This one uses a real rewrite segment where they DIFFER:
    // generated `$setup` <- authored `count`.
    const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
    const genLines = golden.code.split("\n");
    const srcLines = readFileSync(VUE_FIXTURE_ABS, "utf8").split("\n");
    const rewrite = decodeMappings(golden.map.mappings).find((candidate) => {
      if (candidate.srcIdx === null) return false;
      const gen = tokenAt(genLines, candidate.genLine, candidate.genCol);
      const src = tokenAt(srcLines, candidate.srcLine, candidate.srcCol);
      return gen.kind === "word-start" && src.kind === "word-start" && gen.text !== src.text;
    });
    expect(rewrite).toBeDefined();
    const generatedToken = tokenAt(genLines, rewrite.genLine, rewrite.genCol).text;
    const authoredToken = tokenAt(srcLines, rewrite.srcLine, rewrite.srcCol).text;
    expect(generatedToken).not.toBe(authoredToken);
    const withName = (declared) =>
      submitMutatedMap(
        VUE_GOLDEN,
        (segments) =>
          segments.map((candidate) =>
            candidate.genLine === rewrite.genLine && candidate.genCol === rewrite.genCol
              ? { ...candidate, nameIdx: 0 }
              : candidate,
          ),
        { names: [declared] },
      );
    // Reading 1 — the AUTHORED symbol at the segment's own original position.
    expect((await withName(authoredToken)).reasons).toEqual([]);
    // Reading 2 — the GENERATED symbol at its own generated position.
    expect((await withName(generatedToken)).reasons).toEqual([]);
    // Neither: a plausible token that is genuinely present elsewhere in the
    // fixture, and is neither of this segment's two symbols.
    expect(srcLines.join("\n")).toContain("items");
    expect(["items"]).not.toContain(authoredToken);
    const wrong = await withName("items");
    expect(wrong.verdict).toBe("fail");
    expect(mappingReason(wrong)).toContain("name-token-relation");
  });

  it("MUTATION: the SAME segment with a corrupted `names` entry is REJECTED", async () => {
    const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
    const { segment, name } = identifierSegment(golden);
    const corrupted = `${name}_BOGUS`;
    expect(corrupted).not.toBe(name);
    const result = await submitMutatedMap(
      VUE_GOLDEN,
      (segments) =>
        segments.map((candidate) =>
          candidate.genLine === segment.genLine && candidate.genCol === segment.genCol
            ? { ...candidate, nameIdx: 0 }
            : candidate,
        ),
      { names: [corrupted] },
    );
    expect(result.verdict).toBe("fail");
    expect(mappingReason(result)).toContain("name-token-relation");
  });
});

// The no-inherited-provenance boundary, at the column BEFORE a scaffolding
// statement.
//
// A generated-only range spans only the construct it names, so the column
// immediately to its left is outside every range. A fabricated segment
// planted there is not caught by requirement 6's containment check — and a
// consumer resolving the range's own start column then finds it, because the
// applying segment is the last one on the line at or before that column
// (`resolveAt`). The consumer-visible result is identical to a segment
// planted ON the callee: `__expose` reports authored provenance it does not
// have. Statement-level ranges therefore keep the boundary requirement, and
// only a range that begins MID-LINE — inside a larger, legitimately mapped
// expression — is exempt.

describe("a fabricated segment ONE COLUMN LEFT of a scaffolding statement is REJECTED", () => {
  for (const [label, goldenName, where, token, srcLine, srcCol] of [
    [
      "the space before Vue's `__expose();`",
      VUE_GOLDEN,
      /^\s*__expose\(\);?\s*$/,
      "__expose",
      12,
      6,
    ],
    [
      "the tab before Svelte's `$.pop();`",
      "svelte/props-events__client__runes1__dev0",
      /^\s*\$\.pop\(\)/,
      "$",
      3,
      11,
    ],
    [
      "the tab before Svelte's `$.reset(ul);`",
      "svelte/basic-runes__client__runes1__dev1",
      /^\s*\$\.reset\(ul\)/,
      "$",
      5,
      1,
    ],
  ]) {
    it(`MUTATION: ${label}`, async () => {
      const golden = readGoldenByName(GOLDENS_ROOT, goldenName);
      const { line, column } = generatedTokenAt(golden.code, where, token);
      // The statement genuinely starts its own line, and the plant column is
      // genuinely OUTSIDE the range that covers the callee.
      expect(golden.code.split("\n")[line].slice(0, column)).toMatch(/^\s+$/);
      const result = await fabricateSegmentAt(goldenName, line, column - 1, srcLine, srcCol);
      expect(result.verdict).toBe("fail");
      // Specifically the boundary rule: the plant sits outside the range, so
      // the containment check cannot see it.
      expect(mappingReason(result)).toContain("synthetic-boundary");
    });
  }

  it("a consumer lookup at the callee inherits the plant's provenance — the exploit this closes", async () => {
    // The module's own consumer model, run over the mutated map: without the
    // boundary requirement this candidate PASSES while telling an IDE that
    // `__expose` comes from the authored fixture's line 12.
    const golden = readGoldenByName(GOLDENS_ROOT, VUE_GOLDEN);
    const { line, column } = generatedTokenAt(golden.code, /^\s*__expose\(\);?\s*$/, "__expose");
    const planted = [
      ...decodeMappings(golden.map.mappings),
      { genLine: line, genCol: column - 1, srcIdx: 0, srcLine: 12, srcCol: 6, nameIdx: null },
    ].sort((a, b) => a.genLine - b.genLine || a.genCol - b.genCol);
    const applying = planted
      .filter((segment) => segment.genLine === line && segment.genCol <= column)
      .at(-1);
    expect(applying.genCol).toBe(column - 1);
    expect(applying.srcIdx).not.toBeNull();
    const result = await submitMutatedMap(VUE_GOLDEN, () => planted);
    expect(result.verdict).toBe("fail");
  });
});

describe("an authored import from an UNLISTED same-namespace specifier stays mappable", () => {
  it("its real pinned-Svelte map segments validate clean", () => {
    // A namespace-prefix rule calls `svelte/anything` a runtime helper. This
    // authored SIDE-EFFECT import binds no local, so the authored-name
    // subtraction cannot rescue it: the compiler's four truthful segments
    // over it were rejected as fabricated provenance.
    const directory = mkdtempSync(path.join(tmpdir(), "bf2-unlisted-import-"));
    const name = "unlisted-import.svelte";
    const absolutePath = path.join(directory, name);
    try {
      const base = readFileSync(
        path.join(HARNESS_ROOT, "fixtures/svelte/props-events.svelte"),
        "utf8",
      );
      const source = base.replace(
        "<script>",
        '<script>\n\timport "svelte/not-emitted-by-pinned-compiler";',
      );
      expect(source).not.toBe(base);
      writeFileSync(absolutePath, source, "utf8");
      const artifact = compileSvelteFixture(source, name, {
        generate: "client",
        runes: true,
        dev: false,
        sourceMap: true,
      });
      expect(artifact.diagnostics).toEqual([]);
      // The compiler really does carry the authored import through with
      // truthful segments over it — the population this rule must not reject.
      const generatedLine = artifact.code
        .split("\n")
        .findIndex((text) => text.includes("svelte/not-emitted-by-pinned-compiler"));
      expect(generatedLine).toBeGreaterThan(-1);
      expect(
        decodeMappings(artifact.map.mappings).filter(
          (segment) => segment.genLine === generatedLine && segment.srcIdx !== null,
        ).length,
      ).toBeGreaterThan(0);
      const result = validateAuthoredMapping({
        code: artifact.code,
        map: artifact.map,
        sourceMapRequested: true,
        fixture: { path: name, absolutePath },
        sourceResolveBases: [directory],
        profile: MAPPING_PROFILES["svelte:client"],
        anchors: [],
      });
      expect(result.violations).toEqual([]);
      // The framework's OWN runtime import in the same module is still
      // claimed, so this is not a blanket relaxation.
      expect(
        generatedOnlyRanges(artifact.code, MAPPING_PROFILES["svelte:client"], source.split("\n"))
          .map((range) => range.label)
          .some((label) => label.startsWith('runtime-helper import "svelte/internal/')),
      ).toBe(true);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});

describe("a truthful segment over an ObjectPattern property KEY is ACCEPTED", () => {
  it("the real pinned-Vue `v-for` destructuring key is not compiler-introduced", () => {
    // `v-for="{ _sourceKey: authored } in items"` lowers to
    // `({ _sourceKey: authored }) => …`. `_sourceKey` is the SOURCE object's
    // property name — authored text, carried through verbatim — while
    // `authored` is the name the pattern binds. Claiming the key rejected a
    // correct `verbatim-carry` segment.
    const directory = mkdtempSync(path.join(tmpdir(), "bf2-pattern-key-"));
    const name = "pattern-key.vue";
    const absolutePath = path.join(directory, name);
    try {
      const source = [
        "<script setup>",
        "import { ref } from 'vue'",
        "const items = ref([])",
        "</script>",
        "",
        "<template>",
        '  <li v-for="{ _sourceKey: authored } in items" :key="authored">{{ authored }}</li>',
        "</template>",
        "",
      ].join("\n");
      writeFileSync(absolutePath, source, "utf8");
      const artifact = compileVueFixture(source, name, {
        backend: "vdom",
        sourceMap: true,
        isProd: false,
      });
      expect(artifact.diagnostics).toEqual([]);
      const genLines = artifact.code.split("\n");
      const genLine = genLines.findIndex((text) => text.includes("{ _sourceKey: authored }"));
      expect(genLine).toBeGreaterThan(-1);
      const genCol = genLines[genLine].indexOf("_sourceKey");
      const srcLines = source.split("\n");
      const srcLine = srcLines.findIndex((text) => text.includes("_sourceKey"));
      const srcCol = srcLines[srcLine].indexOf("_sourceKey");
      const input = (map) => ({
        code: artifact.code,
        map,
        sourceMapRequested: true,
        fixture: { path: name, absolutePath },
        sourceResolveBases: [directory],
        profile: MAPPING_PROFILES["vue:vdom"],
        anchors: [],
      });
      // The stock artifact is clean, so the plant below is the only variable.
      expect(validateAuthoredMapping(input(artifact.map)).violations).toEqual([]);
      const segments = decodeMappings(artifact.map.mappings);
      expect(
        segments.some((segment) => segment.genLine === genLine && segment.genCol === genCol),
      ).toBe(false);
      // The added segment is TRUTHFUL under the ordinary relation table.
      const genTable = artifact.code.split("\n");
      expect(
        classifySegment({
          gen: tokenAt(genTable, genLine, genCol),
          src: tokenAt(srcLines, srcLine, srcCol),
          genLines: genTable,
          srcLines,
          segment: { genLine, genCol, srcIdx: 0, srcLine, srcCol, nameIdx: null },
          profile: MAPPING_PROFILES["vue:vdom"],
        }),
      ).toBe("verbatim-carry");
      const planted = [
        ...segments,
        { genLine, genCol, srcIdx: 0, srcLine, srcCol, nameIdx: null },
      ].sort((a, b) => a.genLine - b.genLine || a.genCol - b.genCol);
      const result = validateAuthoredMapping(
        input({ ...artifact.map, mappings: encodeMappings(planted) }),
      );
      expect(result.violations).toEqual([]);
      // The pattern's actual BINDING target is still claimable when the
      // compiler introduces it, so the key exclusion is not a blanket one.
      expect(
        generatedOnlyRanges(
          "const { _sourceKey: _bound } = source;\n",
          MAPPING_PROFILES["vue:vdom"],
        )
          .map((range) => range.label)
          .includes("emitted declaration _bound"),
      ).toBe(true);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
  });
});
