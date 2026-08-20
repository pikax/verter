// Self-test: assembly composition (requirement 5) and generated-only ranges
// (requirement 6) of the authored-source mapping oracle.
//
// The Vue harness publishes ONE assembled module built from TWO independent
// official fragments (compileScript's script half, compileTemplate's render
// half). This file validates each fragment map against the AUTHORED fixture
// in its own coordinate space FIRST, and only then requires the assembled
// map to be a pure coordinate TRANSLATION of those already-validated
// fragment mappings — never a resynthesis.
//
// The fragments are produced here by driving the pinned official compiler
// directly, because `compileVueFixture` publishes only the composed map. The
// rebuild is proven faithful before anything is concluded from it: its
// assembled code and composed map must be byte-identical to the production
// path's.

import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";

import {
  assembleAndValidate,
  compileVueFixture,
  vueTemplateCompileOptions,
} from "../src/invoke-vue-oracle.mjs";
import { oracleRequire } from "../src/oracle-install.mjs";
import { composeAssembledModuleMap, decodeMappings, encodeMappings } from "../src/sourcemap.mjs";
import { HARNESS_ROOT } from "../src/paths.mjs";
import {
  FIXTURE_ANCHORS,
  MAPPING_PROFILES,
  generatedOnlyRanges,
  validateAuthoredMapping,
} from "../src/mapping-oracle.mjs";

const FIXTURE = "fixtures/vue/basic-interpolation.vue";
const FIXTURE_ABS = path.join(HARNESS_ROOT, FIXTURE);
const SOURCE = readFileSync(FIXTURE_ABS, "utf8");

const rules = (result) => result.violations.map((v) => v.rule);

/**
 * Rebuilds one Vue compilation from the official pieces, exposing the
 * per-fragment maps and the assembly geometry the production path consumes
 * internally. The `compileTemplate` option set is NOT re-authored here: it
 * comes from the production builder (`vueTemplateCompileOptions`), so this
 * mirror cannot drift into a silently narrower copy of the official
 * invocation — the byte-fidelity assertion in the first test only catches
 * drift that is observable on this one fixture.
 */
function rebuild(backend) {
  const { parse, compileScript, compileTemplate } = oracleRequire("vue", "@vue/compiler-sfc");
  const ssr = backend === "ssr";
  const vapor = backend === "vapor";
  const { descriptor, errors } = parse(SOURCE, { filename: FIXTURE, sourceMap: true });
  expect(errors).toEqual([]);
  const script = compileScript(descriptor, {
    id: FIXTURE,
    inlineTemplate: false,
    sourceMap: true,
    isProd: false,
    vapor,
    templateOptions: { ssr },
  });
  const template = compileTemplate(
    vueTemplateCompileOptions({
      descriptor,
      filename: FIXTURE,
      ssr,
      vapor,
      isProd: false,
      sourceMap: true,
      scriptBindings: script.bindings,
    }),
  );
  const assembly = assembleAndValidate({
    scriptCode: script.content,
    renderCode: template.code,
    ssr,
    vapor,
  });
  expect(assembly.fragmentDiagnostics).toEqual([]);
  const parts = assembly.parts.map((part) => ({
    ...part,
    map:
      part.role === "script"
        ? (script.map ?? null)
        : part.role === "render"
          ? (template.map ?? null)
          : null,
  }));
  return { assembly, parts, scriptMap: script.map ?? null, templateMap: template.map ?? null };
}

/** Line offset of each assembly part within the assembled module. */
function partLineOffsets(parts) {
  const offsets = new Map();
  let lineOffset = 0;
  for (const part of parts) {
    offsets.set(part.role, lineOffset);
    lineOffset += part.postEditCode.split("\n").length;
  }
  return offsets;
}

function fragmentInput(map, code, overrides = {}) {
  return {
    code,
    map,
    sourceMapRequested: true,
    fixture: { path: FIXTURE, absolutePath: FIXTURE_ABS },
    sourceResolveBases: [HARNESS_ROOT, path.dirname(FIXTURE_ABS)],
    profile: MAPPING_PROFILES["vue:vdom"],
    anchors: [],
    ...overrides,
  };
}

describe("requirement 5 — fragment maps validate first, assembly is pure translation", () => {
  for (const backend of ["vdom", "vapor", "ssr"]) {
    it(`${backend}: the rebuilt fragments reproduce the production artifact byte-for-byte`, () => {
      const production = compileVueFixture(SOURCE, FIXTURE, {
        backend,
        sourceMap: true,
        isProd: false,
      });
      const { assembly, parts } = rebuild(backend);
      expect(assembly.code).toBe(production.code);
      expect(composeAssembledModuleMap(parts)).toEqual(production.map);
    });

    it(`${backend}: EACH fragment map is valid against the authored fixture in its OWN coordinate space`, () => {
      const { parts } = rebuild(backend);
      let validated = 0;
      for (const part of parts) {
        if (part.map === null) continue;
        validated += 1;
        const result = validateAuthoredMapping(
          fragmentInput(part.map, part.preEditCode, {
            profile: MAPPING_PROFILES[`vue:${backend}`],
          }),
        );
        expect(result.violations, `${part.role} fragment`).toEqual([]);
      }
      // Both halves must genuinely carry a map — a silently map-less
      // fragment would make this whole requirement vacuous.
      expect(validated).toBe(2);
    });

    it(`${backend}: every fragment segment survives assembly as the SAME original tuple, shifted by exactly the fragment's line offset`, () => {
      const { parts } = rebuild(backend);
      const assembled = composeAssembledModuleMap(parts);
      const assembledSegments = decodeMappings(assembled.mappings);
      const offsets = partLineOffsets(parts);
      let translated = 0;
      for (const part of parts) {
        if (part.map === null) continue;
        const lineOffset = offsets.get(part.role);
        // The single-line keyword splice the assembler applies shifts
        // columns on ONE line; every other line must translate exactly.
        const editLine =
          part.edit === null
            ? null
            : part.preEditCode.slice(0, part.edit.start).split("\n").length - 1;
        const sourceIndexOf = (spelling) => assembled.sources.indexOf(spelling);
        for (const segment of decodeMappings(part.map.mappings ?? "")) {
          if (segment.srcIdx === null || segment.genLine === editLine) continue;
          const expectedSourceIndex = sourceIndexOf(part.map.sources[segment.srcIdx]);
          expect(expectedSourceIndex).toBeGreaterThan(-1);
          const match = assembledSegments.find(
            (candidate) =>
              candidate.genLine === segment.genLine + lineOffset &&
              candidate.genCol === segment.genCol &&
              candidate.srcIdx === expectedSourceIndex &&
              candidate.srcLine === segment.srcLine &&
              candidate.srcCol === segment.srcCol,
          );
          expect(
            match,
            `${part.role} segment ${segment.genLine}:${segment.genCol} -> ${segment.srcLine}:${segment.srcCol}`,
          ).toBeDefined();
          translated += 1;
        }
      }
      expect(translated).toBeGreaterThan(20);
    });

    it(`${backend}: every required anchor resolves identically in the fragment map and in the assembled map`, () => {
      const { parts } = rebuild(backend);
      const assembled = composeAssembledModuleMap(parts);
      const assembledSegments = decodeMappings(assembled.mappings);
      const offsets = partLineOffsets(parts);
      const anchors = FIXTURE_ANCHORS[FIXTURE].filter((anchor) =>
        anchor.requiredFor.includes(`vue:${backend}`),
      );
      expect(anchors.length).toBeGreaterThan(0);
      for (const anchor of anchors) {
        const carriers = parts
          .filter((part) => part.map !== null)
          .flatMap((part) =>
            decodeMappings(part.map.mappings ?? "")
              .filter(
                (segment) =>
                  segment.srcIdx !== null &&
                  segment.srcLine === anchor.line &&
                  segment.srcCol === anchor.column,
              )
              .map((segment) => ({ part, segment })),
          );
        expect(carriers.length, anchor.id).toBeGreaterThan(0);
        for (const { part, segment } of carriers) {
          const shifted = segment.genLine + offsets.get(part.role);
          const inAssembled = assembledSegments.some(
            (candidate) =>
              candidate.genLine === shifted &&
              candidate.srcLine === anchor.line &&
              candidate.srcCol === anchor.column,
          );
          expect(inAssembled, `${anchor.id} in ${part.role}`).toBe(true);
        }
      }
    });
  }
});

describe("requirement 6 — generated-only ranges carry no authored provenance", () => {
  it("the generated-only ranges are derived from the module's own syntax tree, not from text", () => {
    const assembled = compileVueFixture(SOURCE, FIXTURE, {
      backend: "vdom",
      sourceMap: true,
      isProd: false,
    });
    const ranges = generatedOnlyRanges(assembled.code, MAPPING_PROFILES["vue:vdom"]);
    expect(ranges.length).toBeGreaterThanOrEqual(3);
    expect(ranges.some((range) => range.label.startsWith("runtime-helper import"))).toBe(true);
    expect(ranges.some((range) => range.label.startsWith("generated plumbing"))).toBe(true);
    expect(ranges.some((range) => range.label.startsWith("emitted declaration"))).toBe(true);
    // Ground truth: every derived range really does span the text it claims,
    // read back out of the assembled module by the range's own coordinates.
    const lines = assembled.code.split("\n");
    const textOf = (range) =>
      range.startLine === range.endLine
        ? lines[range.startLine].slice(range.startColumn, range.endColumn)
        : lines[range.startLine].slice(range.startColumn);
    for (const range of ranges) {
      const text = textOf(range);
      if (range.label.startsWith("runtime-helper import")) {
        expect(text.startsWith("import ")).toBe(true);
        expect(text).toContain(" as _");
      } else if (range.label.startsWith("generated plumbing")) {
        expect(text).toBe("_sfc_main.render = render");
      } else if (range.label.startsWith("generated default export")) {
        expect(text).toBe("export default _sfc_main");
      } else if (range.label.startsWith("generated helper call")) {
        expect(`generated helper call ${text.split(".")[0]}`).toBe(range.label);
      } else if (range.label.startsWith("generated helper argument")) {
        expect(`generated helper argument ${text}`).toBe(range.label);
      } else if (range.label.startsWith("generated return")) {
        expect(`generated return ${text.slice("return ".length)}`).toBe(range.label);
      } else {
        // An emitted declaration range spans exactly its own identifier.
        expect(`emitted declaration ${text}`).toBe(range.label);
      }
    }
    // The AUTHORED import is not swept in: it binds an authored local.
    expect(lines[0]).toContain('import { ref } from "vue"');
    expect(ranges.some((range) => range.startLine === 0)).toBe(false);
  });

  it("the `generated default export` class is genuinely produced, not merely tolerated", () => {
    // The check above is an `else if` chain: with the default-export rule
    // removed the branch is simply never entered and every other assertion
    // still holds. This names the class directly.
    const assembled = compileVueFixture(SOURCE, FIXTURE, {
      backend: "vdom",
      sourceMap: true,
      isProd: false,
    });
    const lines = assembled.code.split("\n");
    const exportLine = lines.findIndex((line) => line.startsWith("export default _sfc_main"));
    expect(exportLine).toBeGreaterThan(-1);
    const ranges = generatedOnlyRanges(assembled.code, MAPPING_PROFILES["vue:vdom"]);
    const defaultExport = ranges.filter((range) =>
      range.label.startsWith("generated default export"),
    );
    expect(defaultExport).toHaveLength(1);
    expect(defaultExport[0].label).toBe("generated default export _sfc_main");
    expect(defaultExport[0].startLine).toBe(exportLine);
    expect(defaultExport[0].startColumn).toBe(0);
    // A statement-level range keeps the no-inherited-provenance requirement.
    expect(defaultExport[0].boundary).not.toBe(false);
  });

  it("the `VariableDeclarator.id` binding class is genuinely produced", () => {
    // The largest range class by count, and the one S1/S3 do NOT reach: they
    // cover an object-literal KEY (`__name:`) and a function PARAMETER
    // (`setup(__props)`) respectively.
    const assembled = compileVueFixture(SOURCE, FIXTURE, {
      backend: "vdom",
      sourceMap: true,
      isProd: false,
    });
    const ranges = generatedOnlyRanges(assembled.code, MAPPING_PROFILES["vue:vdom"]);
    const lines = assembled.code.split("\n");
    const declaratorIds = ranges.filter((range) => {
      if (!range.label.startsWith("emitted declaration ")) return false;
      const before = lines[range.startLine].slice(0, range.startColumn);
      return /\b(const|let|var)\s+$/.test(before);
    });
    expect(declaratorIds.length).toBeGreaterThan(0);
    for (const name of ["_sfc_main", "_hoisted_1"]) {
      expect(declaratorIds.some((range) => range.label === `emitted declaration ${name}`)).toBe(
        true,
      );
    }
  });

  it("every enumerated BINDING position is covered, and a reference position is not", () => {
    const profile = MAPPING_PROFILES["vue:vdom"];
    const labelsFor = (code) => generatedOnlyRanges(code, profile).map((range) => range.label);
    // Each of these introduces the name at that position; no authored token
    // can sit behind it. Every one was silently absent.
    expect(labelsFor("try { go(); } catch (__err) { report(__err); }\n")).toContain(
      "emitted declaration __err",
    );
    expect(labelsFor("function _sfc_render() {}\n")).toContain("emitted declaration _sfc_render");
    expect(labelsFor("class _Sfc {}\n")).toContain("emitted declaration _Sfc");
    expect(labelsFor("class C { _m() {} }\n")).toContain("emitted declaration _m");
    expect(labelsFor('import { toDisplayString as _toDisplayString } from "vue";\n')).toContain(
      "emitted declaration _toDisplayString",
    );
    expect(labelsFor('import * as _runtime from "vue";\n')).toContain(
      "emitted declaration _runtime",
    );
    expect(labelsFor("function f(_a = 1) { return _a; }\n")).toContain("emitted declaration _a");
    expect(labelsFor("const [_first] = source;\n")).toContain("emitted declaration _first");
    expect(labelsFor("const { ..._rest } = source;\n")).toContain("emitted declaration _rest");
    // A REFERENCE to an emitted binding is NOT a declaration site: the pinned
    // Vue compiler really does map its helper call sites and hoisted-node
    // arguments back to the authored template, so claiming them would reject
    // correct maps. This is the stated boundary, asserted rather than assumed.
    expect(labelsFor("out(_createElementVNode(_hoisted_1));\n")).not.toContain(
      "emitted declaration _hoisted_1",
    );
  });

  it("the ObjectPattern-vs-ObjectLiteral distinction is enforced in BOTH directions", () => {
    const profile = MAPPING_PROFILES["vue:vdom"];
    // A pattern property's VALUE is a binding …
    const pattern = generatedOnlyRanges("const { a: _bound } = source;\n", profile);
    expect(pattern.map((range) => range.label)).toContain("emitted declaration _bound");
    // … while an object LITERAL property's value is a reference to something
    // declared elsewhere, and is NOT claimed. (Its KEY is, which is what
    // makes `__name:` reachable — so the two must be told apart.)
    const literal = generatedOnlyRanges("const o = { a: _reference };\n", profile);
    expect(literal.map((range) => range.label)).not.toContain("emitted declaration _reference");
    expect(generatedOnlyRanges("const o = { _key: 1 };\n", profile).map((r) => r.label)).toContain(
      "emitted declaration _key",
    );
  });

  it("the helper-import rule requires EVERY specifier to be emitted-shaped, not merely one", () => {
    const profile = MAPPING_PROFILES["vue:vdom"];
    // A MIXED import — one emitted-shaped local, one authored-shaped local
    // from the SAME statement. `.some` would sweep the whole statement in and
    // make the authored `ref` binding unmappable; `.every` keeps it out.
    const mixed = generatedOnlyRanges(
      'import { ref, toDisplayString as _toDisplayString } from "vue";\n',
      profile,
    );
    expect(mixed.some((range) => range.label.startsWith("runtime-helper import"))).toBe(false);
    // The all-emitted control from the same module specifier IS swept in.
    const pure = generatedOnlyRanges(
      'import { toDisplayString as _toDisplayString } from "vue";\n',
      profile,
    );
    expect(pure.some((range) => range.label.startsWith("runtime-helper import"))).toBe(true);
  });

  it("the helper-import rule requires a RUNTIME module source, not merely emitted-shaped locals", () => {
    const profile = MAPPING_PROFILES["vue:vdom"];
    // An authored import from an unrelated module whose local happens to be
    // underscore-prefixed is authored code, and a real segment over it must
    // stay mappable.
    expect(
      generatedOnlyRanges('import { thing as _thing } from "./my-utils.js";\n', profile).some(
        (range) => range.label.startsWith("runtime-helper import"),
      ),
    ).toBe(false);
    expect(
      generatedOnlyRanges('import { thing as _thing } from "vue";\n', profile).some((range) =>
        range.label.startsWith("runtime-helper import"),
      ),
    ).toBe(true);
  });

  it("a ZERO-specifier side-effect import of the framework runtime is generated-only", () => {
    // `import 'svelte/internal/disclose-version'` binds nothing, so a
    // specifier-based rule cannot see it; the closed runtime-module set can.
    const svelte = MAPPING_PROFILES["svelte:client"];
    expect(
      generatedOnlyRanges("import 'svelte/internal/disclose-version';\n", svelte).map(
        (range) => range.label,
      ),
    ).toContain('runtime-helper import "svelte/internal/disclose-version"');
    // An authored side-effect import of an unrelated module is not.
    expect(generatedOnlyRanges("import './styles.css';\n", svelte)).toEqual([]);
  });

  for (const backend of ["vdom", "vapor", "ssr"]) {
    it(`${backend}: the real assembled artifact maps nothing over its generated-only ranges`, () => {
      const { assembly, parts } = rebuild(backend);
      const composed = composeAssembledModuleMap(parts);
      const result = validateAuthoredMapping(
        fragmentInput(composed, assembly.code, {
          profile: MAPPING_PROFILES[`vue:${backend}`],
          anchors: FIXTURE_ANCHORS[FIXTURE],
        }),
      );
      expect(result.violations).toEqual([]);
      // The rail was derived and non-empty: a silently empty range list would
      // make this whole requirement vacuous, which is how it came to be inert
      // on the acceptance path.
      expect(result.stats.syntheticRanges).toBeGreaterThanOrEqual(3);
    });
  }

  it("MUTATION: a fabricated source-bearing segment over the helper-import block fails", () => {
    const { assembly, parts } = rebuild("vdom");
    const composed = composeAssembledModuleMap(parts);
    const ranges = generatedOnlyRanges(assembly.code, MAPPING_PROFILES["vue:vdom"]);
    const helperImport = ranges.find((range) => range.label.startsWith("runtime-helper import"));
    expect(helperImport).toBeDefined();
    const segments = decodeMappings(composed.mappings);
    // Prove the plant is genuinely NEW: nothing maps there today.
    expect(
      segments.some(
        (segment) => segment.genLine === helperImport.startLine && segment.srcIdx !== null,
      ),
    ).toBe(false);
    const planted = [
      ...segments,
      {
        genLine: helperImport.startLine,
        genCol: 0,
        srcIdx: 0,
        srcLine: 3,
        srcCol: 0,
        nameIdx: null,
      },
    ];
    const mutatedMappings = encodeMappings(planted);
    expect(mutatedMappings).not.toBe(composed.mappings);
    const mutated = { ...composed, mappings: mutatedMappings };
    // The plant is present in the mutated input.
    expect(
      decodeMappings(mutated.mappings).some(
        (segment) => segment.genLine === helperImport.startLine && segment.srcIdx !== null,
      ),
    ).toBe(true);
    const result = validateAuthoredMapping(
      fragmentInput(mutated, assembly.code, { anchors: FIXTURE_ANCHORS[FIXTURE] }),
    );
    expect(rules(result)).toContain("synthetic-provenance");
    // Restored input is clean again.
    expect(
      validateAuthoredMapping(
        fragmentInput(composed, assembly.code, { anchors: FIXTURE_ANCHORS[FIXTURE] }),
      ).violations,
    ).toEqual([]);
  });

  it("MUTATION: removing the boundary segment before generated-only code makes a lookup BLEED into authored source", () => {
    // Neither pinned official compiler emits boundary (source-less)
    // segments, and their maps therefore let a mapped region's provenance
    // run on to the end of its generated line. So the candidate here is a
    // constructed one — which is exactly the population this oracle exists
    // for: it must be able to accept a candidate that DOES terminate its
    // mapped regions, and to reject the same candidate once that boundary
    // is deleted.
    const { assembly, parts } = rebuild("vdom");
    const composed = composeAssembledModuleMap(parts);
    const segments = decodeMappings(composed.mappings);
    const lines = assembly.code.split("\n");
    // A generated line whose LAST mapped segment is followed by more text:
    // the tail after that segment is compiler-only patch-flag material.
    const anchorSegment = [...segments]
      .reverse()
      .find(
        (segment) =>
          segment.srcIdx !== null &&
          segment.genCol + 4 < lines[segment.genLine].length &&
          !segments.some(
            (other) => other.genLine === segment.genLine && other.genCol > segment.genCol,
          ),
      );
    expect(anchorSegment).toBeDefined();
    const boundaryColumn = anchorSegment.genCol + 1;
    const extraSyntheticRanges = [
      {
        label: "generated-only line tail",
        startLine: anchorSegment.genLine,
        startColumn: boundaryColumn,
        endLine: anchorSegment.genLine,
        endColumn: lines[anchorSegment.genLine].length + 1,
      },
    ];
    const boundary = {
      genLine: anchorSegment.genLine,
      genCol: boundaryColumn,
      srcIdx: null,
      srcLine: null,
      srcCol: null,
      nameIdx: null,
    };
    // GREEN arm: a candidate that terminates the mapped region passes.
    const terminated = { ...composed, mappings: encodeMappings([...segments, boundary]) };
    expect(terminated.mappings).not.toBe(composed.mappings);
    const green = validateAuthoredMapping(
      fragmentInput(terminated, assembly.code, { extraSyntheticRanges }),
    );
    expect(green.violations).toEqual([]);

    // RED arm: the SAME candidate with exactly that boundary removed.
    const withoutBoundary = decodeMappings(terminated.mappings).filter(
      (segment) =>
        !(
          segment.genLine === boundary.genLine &&
          segment.genCol === boundary.genCol &&
          segment.srcIdx === null
        ),
    );
    expect(withoutBoundary.length).toBe(decodeMappings(terminated.mappings).length - 1);
    const red = validateAuthoredMapping(
      fragmentInput({ ...composed, mappings: encodeMappings(withoutBoundary) }, assembly.code, {
        extraSyntheticRanges,
      }),
    );
    expect(rules(red)).toContain("synthetic-boundary");
  });
});

// The three properties the derived rail asserts about ITSELF: which ranges
// carry the no-inherited-provenance requirement, which module specifiers are
// runtime helpers, and which identifier positions are binding positions.
// Each was silently unasserted, and each had a live consequence.

describe("a generated-only range carries the boundary requirement when it STARTS ITS OWN LINE", () => {
  const VUE = MAPPING_PROFILES["vue:vdom"];
  const SVELTE = MAPPING_PROFILES["svelte:client"];
  const labelled = (code, profile, label) => {
    const found = generatedOnlyRanges(code, profile).filter((range) => range.label === label);
    expect(found.length, `${label} in ${JSON.stringify(code)}`).toBeGreaterThan(0);
    return found;
  };

  it("a helper call and a return that begin their own line keep the requirement", () => {
    // Nothing but whitespace precedes them, so a consumer lookup at their
    // start column cannot legitimately inherit an enclosing expression's
    // provenance: a segment one column to the LEFT is a fabrication.
    expect(labelled("  __expose();\n", VUE, "generated helper call __expose")[0].boundary).toBe(
      true,
    );
    expect(
      labelled(
        "function f() {\n  return __returned__;\n}\n",
        VUE,
        "generated return __returned__",
      )[0].boundary,
    ).toBe(true);
    expect(labelled("\t$.pop();\n", SVELTE, "generated helper call $")[0].boundary).toBe(true);
  });

  it("a range that begins MID-LINE keeps the inline exemption", () => {
    // The real official Svelte maps carry segments over exactly this shape
    // (`if (count > 0) $$render(consequent); else $$render(alternate, -1);`
    // in the committed basic-runes client goldens), so the enclosing mapped
    // expression legitimately supplies the provenance at these columns.
    const nested = labelled(
      "if (count > 0) $$render(consequent); else $$render(alternate, -1);\n",
      SVELTE,
      "generated helper call $$render",
    );
    expect(nested).toHaveLength(2);
    for (const range of nested) expect(range.boundary).toBe(false);
    // A direct call ARGUMENT never starts its own line — the callee text
    // precedes it — so it is exempt by the same rule, not by a special case.
    expect(
      labelled(
        "  Object.defineProperty(__returned__, 'x', {});\n",
        VUE,
        "generated helper argument __returned__",
      )[0].boundary,
    ).toBe(false);
  });
});

describe("the runtime-module set is CLOSED by exact membership, not by namespace", () => {
  const VUE = MAPPING_PROFILES["vue:vdom"];
  const SVELTE = MAPPING_PROFILES["svelte:client"];
  const swept = (code, profile) =>
    generatedOnlyRanges(code, profile).some((range) =>
      range.label.startsWith("runtime-helper import"),
    );

  it("each of the six measured specifiers IS a runtime module", () => {
    for (const specifier of ["vue", "vue/server-renderer"]) {
      expect(swept(`import { x as _x } from ${JSON.stringify(specifier)};\n`, VUE), specifier).toBe(
        true,
      );
    }
    for (const specifier of [
      "svelte/internal/client",
      "svelte/internal/server",
      "svelte/internal/flags/legacy",
      "svelte/internal/disclose-version",
    ]) {
      expect(swept(`import ${JSON.stringify(specifier)};\n`, SVELTE), specifier).toBe(true);
    }
  });

  it("an UNLISTED specifier under the same namespace is NOT swept in, in either import form", () => {
    // A namespace-prefix rule claims all of these. They are authored-facing
    // public entry points a future fixture may legitimately import, and the
    // pinned compilers emit none of them. The SIDE-EFFECT form is the one
    // with no escape hatch: it binds no local, so the authored-name
    // subtraction that saves `import { x } from "svelte/store"` cannot apply.
    for (const specifier of ["vue/reactivity", "@vue/shared", "@vue/reactivity", "vue/dist/vue"]) {
      expect(swept(`import { x as _x } from ${JSON.stringify(specifier)};\n`, VUE), specifier).toBe(
        false,
      );
      expect(swept(`import ${JSON.stringify(specifier)};\n`, VUE), `${specifier} side-effect`).toBe(
        false,
      );
    }
    for (const specifier of ["svelte", "svelte/store", "svelte/motion", "svelte/internal"]) {
      expect(
        swept(`import { x as _x } from ${JSON.stringify(specifier)};\n`, SVELTE),
        specifier,
      ).toBe(false);
      expect(
        swept(`import ${JSON.stringify(specifier)};\n`, SVELTE),
        `${specifier} side-effect`,
      ).toBe(false);
    }
  });
});

describe("an ObjectPattern property KEY is not a binding position", () => {
  const VUE = MAPPING_PROFILES["vue:vdom"];
  const labels = (code) => generatedOnlyRanges(code, VUE).map((range) => range.label);

  it("the KEY of a destructuring property is not claimed; its VALUE still is", () => {
    // `authored` is the name this statement binds. `_sourceKey` is the
    // SOURCE object's property name — authored material the compiler carried
    // through, which real official maps do map (a Vue `v-for="{ _sourceKey:
    // authored } in items"` lowers to exactly this shape).
    expect(labels("const { _sourceKey: authored } = source;\n")).not.toContain(
      "emitted declaration _sourceKey",
    );
    expect(labels("const { _sourceKey: _bound } = source;\n")).toContain(
      "emitted declaration _bound",
    );
    expect(labels("const { _sourceKey: _bound } = source;\n")).not.toContain(
      "emitted declaration _sourceKey",
    );
  });

  it("an object-LITERAL key and a pattern SHORTHAND are still claimed", () => {
    // The literal-key arm is what makes Vue's synthesized `__name:`
    // reachable; shorthand `{ _x }` binds through the property VALUE (the
    // same node the key hangs on), so it survives the key exclusion.
    expect(labels("const o = { _key: 1 };\n")).toContain("emitted declaration _key");
    expect(labels("const { _shorthand } = source;\n")).toContain("emitted declaration _shorthand");
  });
});
