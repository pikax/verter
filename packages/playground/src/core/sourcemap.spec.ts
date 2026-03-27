/**
 * @ai-generated - Unit tests for source map VLQ codec, combination, and lookup utilities.
 */
import { describe, it, expect } from "vitest";
import {
  encodeVLQValue,
  parseMappings,
  encodeMappings,
  combineSourceMaps,
  lookupGenerated,
  lookupSource,
  type Segment,
} from "./sourcemap";

describe("VLQ codec", () => {
  it("encodes 0 as A", () => {
    expect(encodeVLQValue(0)).toBe("A");
  });

  it("encodes positive values", () => {
    // 1 → 0b10 → VLQ char C
    expect(encodeVLQValue(1)).toBe("C");
    // 15 → 0b11110 → VLQ char e
    expect(encodeVLQValue(15)).toBe("e");
  });

  it("encodes negative values", () => {
    // -1 → 0b11 → VLQ char D
    expect(encodeVLQValue(-1)).toBe("D");
  });

  it("round-trips through parseMappings/encodeMappings for single segment", () => {
    const segments: Segment[][] = [[[0, 0, 0, 0]]];
    const encoded = encodeMappings(segments);
    const decoded = parseMappings(encoded);
    expect(decoded).toEqual(segments);
  });

  it("round-trips through parseMappings/encodeMappings for multiple lines and segments", () => {
    const segments: Segment[][] = [
      [
        [0, 0, 0, 0],
        [5, 0, 0, 10],
      ],
      [
        [0, 0, 1, 0],
        [8, 0, 1, 4],
      ],
      [],
      [[2, 0, 3, 2]],
    ];
    const encoded = encodeMappings(segments);
    const decoded = parseMappings(encoded);
    expect(decoded).toEqual(segments);
  });

  it("round-trips segments with name indices", () => {
    const segments: Segment[][] = [
      [
        [0, 0, 0, 0, 0],
        [4, 0, 0, 4, 1],
      ],
    ];
    const encoded = encodeMappings(segments);
    const decoded = parseMappings(encoded);
    expect(decoded).toEqual(segments);
  });

  it("handles empty mappings", () => {
    expect(parseMappings("")).toEqual([]);
    expect(encodeMappings([])).toBe("");
  });

  it("handles line with no segments (just semicolons)", () => {
    const decoded = parseMappings(";;");
    expect(decoded).toEqual([[], [], []]);
  });
});

describe("combineSourceMaps", () => {
  // Helper to build a minimal source map JSON string
  function makeMap(opts: {
    mappings: string;
    sources?: string[];
    sourcesContent?: (string | null)[];
    names?: string[];
  }): string {
    return JSON.stringify({
      version: 3,
      sources: opts.sources ?? ["App.vue"],
      sourcesContent: opts.sourcesContent ?? [""],
      names: opts.names ?? [],
      mappings: opts.mappings,
    });
  }

  function makeMapFromSegments(
    segments: Segment[][],
    opts?: {
      sources?: string[];
      sourcesContent?: (string | null)[];
      names?: string[];
    },
  ): string {
    return makeMap({
      mappings: encodeMappings(segments),
      ...opts,
    });
  }

  it("returns empty string when both maps are empty", () => {
    const result = combineSourceMaps({
      scriptMap: "",
      scriptCode: "const x = 1",
      templateMap: "",
      templateCode: "",
      vueSource: "<script setup>\nconst x = 1\n</script>\n<template><div/></template>",
      finalJs: "const __sfc__ = {};\nconst x = 1",
    });
    expect(result).toBe("");
  });

  it("preserves script-only mappings when no template map", () => {
    // Script has 2 lines of output, mapping to source lines 1 and 2.
    // Script code does NOT have "export default", so mergeRenderIntoComponent
    // prepends "const __sfc__ = {};\n" (+1 line offset).
    const scriptSegments: Segment[][] = [
      [[0, 0, 1, 0]], // gen line 0 → src line 1 (inside <script setup>)
      [[0, 0, 2, 0]], // gen line 1 → src line 2
    ];
    const scriptCode = "const x = 1\nconst y = 2";
    const vueSource = `<script setup>\nconst x = 1\nconst y = 2\n</script>`;
    const finalJs = `const __sfc__ = {};\n${scriptCode}\nexport default __sfc__;\n`;

    const result = combineSourceMaps({
      scriptMap: makeMapFromSegments(scriptSegments),
      scriptCode,
      templateMap: "",
      templateCode: "",
      vueSource,
      finalJs,
    });

    expect(result).not.toBe("");
    // Script line 0 → shifted by +1 (mergeLineOffset) → gen line 1
    const gen0 = lookupGenerated(result, 1, 0);
    expect(gen0).not.toBeNull();
    expect(gen0!.line).toBe(1);

    // Reverse: gen line 1 → source line 1
    const src0 = lookupSource(result, 1, 0);
    expect(src0).not.toBeNull();
    expect(src0!.line).toBe(1);
  });

  it("preserves template-only mappings when no script map", () => {
    // Template source map has generated lines relative to full SFC.
    // SFC: line 0 is <template>, line 1 is the render function
    const vueSource = `<template>\n  <div>hello</div>\n</template>`;
    // Template source map: generated lines start at the SFC prefix
    // Line 0 = <template> (prefix lines = 0), Line 1 = the actual code
    const tplSegments: Segment[][] = [
      [], // line 0: <template> tag (no mappings)
      [[0, 0, 1, 2]], // line 1: <div>hello</div> → src line 1, col 2
    ];

    // Host prepends import line to template code
    const templateCode = `import { createElementVNode as _createElementVNode } from "vue"\nfunction render(_ctx, _cache) {\nreturn "hello"\n}`;
    // Template-only → mergeRenderIntoComponent prepends __sfc__ = {}
    const finalJs = `const __sfc__ = {};\n${templateCode}\n__sfc__.render = render;\nexport default __sfc__;\n`;

    const result = combineSourceMaps({
      scriptMap: "",
      scriptCode: "",
      templateMap: makeMapFromSegments(tplSegments),
      templateCode,
      vueSource,
      finalJs,
    });

    expect(result).not.toBe("");

    // Template segment at VLQ gen line 1:
    // sfcPrefixLines = 0, scriptLineCount = 0, hostImportOffset = 1, mergeLineOffset = 1
    // adjustedLine = 1 - 0 + 0 + 1 + 1 = 3
    // In finalJs: line 0 = "const __sfc__ = {};", line 1 = "import {...}",
    //             line 2 = "function render...", line 3 = "return..."
    const src = lookupSource(result, 3, 0);
    expect(src).not.toBeNull();
    expect(src!.line).toBe(1);
    expect(src!.col).toBe(2);
  });

  it("combines script + template maps with correct offsets", () => {
    const vueSource = [
      '<script setup lang="ts">', // line 0
      'const msg = "hello"', // line 1
      "</script>", // line 2
      "<template>", // line 3
      "  <div>{{ msg }}</div>", // line 4
      "</template>", // line 5
    ].join("\n");

    // Script virtual file code (3 lines, with "export default" → mergeLineOffset = 0)
    const scriptCode = `export default /*@__PURE__*/{\n__name: 'App',\nsetup(__props){ const msg = "hello"; return { msg } }}`;

    // Script source map: gen line 0 → src line 1
    const scriptSegs: Segment[][] = [
      [[0, 0, 1, 0]], // gen line 0 → src line 1 (const msg = "hello")
    ];

    // Template virtual file code (host-prepended import + render function)
    const templateCode = `import { toDisplayString as _toDisplayString } from "vue"\nfunction render(_ctx, _cache, $props, $setup, $data, $options) {\nreturn _toDisplayString($setup.msg)\n}`;

    // Template source map: generated lines relative to full SFC CodeTransform.
    // sfcPrefixLines = 3 (lines 0,1,2 before <template>)
    const tplSegs: Segment[][] = [
      [],
      [],
      [], // SFC lines 0-2 (script block)
      [], // SFC line 3 (<template> tag, no mappings)
      [[0, 0, 4, 2]], // SFC line 4 → src line 4, col 2 (<div>{{ msg }}</div>)
    ];

    // finalJs: mergeRenderIntoComponent replaces "export default" → "const __sfc__ ="
    const finalJs = `const __sfc__ = /*@__PURE__*/{\n__name: 'App',\nsetup(__props){ const msg = "hello"; return { msg } }}\n${templateCode}\n__sfc__.render = render;\nexport default __sfc__;\n`;

    const result = combineSourceMaps({
      scriptMap: makeMapFromSegments(scriptSegs),
      scriptCode,
      templateMap: makeMapFromSegments(tplSegs),
      templateCode,
      vueSource,
      finalJs,
    });

    expect(result).not.toBe("");

    // Script: gen line 0 → src line 1 (mergeLineOffset = 0)
    const scriptSrc = lookupSource(result, 0, 0);
    expect(scriptSrc).not.toBeNull();
    expect(scriptSrc!.line).toBe(1);

    // Reverse: src line 1 → gen line 0
    const scriptGen = lookupGenerated(result, 1, 0);
    expect(scriptGen).not.toBeNull();
    expect(scriptGen!.line).toBe(0);

    // Template mapping at VLQ gen line 4:
    //   adjustedLine = 4 - sfcPrefixLines(3) + scriptLineCount(3) + hostImportOffset(1) + mergeLineOffset(0) = 5
    // finalJs line 5: "return _toDisplayString($setup.msg)" — maps to src line 4, col 2
    const templateSrc = lookupSource(result, 5, 0);
    expect(templateSrc).not.toBeNull();
    expect(templateSrc!.line).toBe(4);
    expect(templateSrc!.col).toBe(2);

    // Reverse: src line 4, col 2 → gen line 5
    const templateGen = lookupGenerated(result, 4, 2);
    expect(templateGen).not.toBeNull();
    expect(templateGen!.line).toBe(5);
  });
});

describe("lookupGenerated", () => {
  function makeMap(segments: Segment[][]): string {
    return JSON.stringify({
      version: 3,
      sources: ["test.vue"],
      sourcesContent: [""],
      names: [],
      mappings: encodeMappings(segments),
    });
  }

  it("finds exact match", () => {
    const map = makeMap([[[0, 0, 5, 3]], [[4, 0, 10, 0]]]);
    const result = lookupGenerated(map, 5, 3);
    expect(result).toEqual({ line: 0, col: 0 });
  });

  it("finds closest column match on same source line", () => {
    const map = makeMap([
      [
        [0, 0, 5, 0],
        [10, 0, 5, 8],
      ],
    ]);
    const result = lookupGenerated(map, 5, 7);
    // col 7 is closer to col 8 (dist 1) than col 0 (dist 7)
    expect(result).not.toBeNull();
    expect(result!.line).toBe(0);
    expect(result!.col).toBe(10);
  });

  it("returns null when no matching source line", () => {
    const map = makeMap([[[0, 0, 5, 0]]]);
    expect(lookupGenerated(map, 99, 0)).toBeNull();
  });

  it("returns null for empty map", () => {
    expect(lookupGenerated("", 0, 0)).toBeNull();
  });
});

describe("lookupSource", () => {
  function makeMap(segments: Segment[][]): string {
    return JSON.stringify({
      version: 3,
      sources: ["test.vue"],
      sourcesContent: [""],
      names: [],
      mappings: encodeMappings(segments),
    });
  }

  it("finds exact match", () => {
    const map = makeMap([
      [
        [0, 0, 5, 3],
        [10, 0, 5, 10],
      ],
    ]);
    const result = lookupSource(map, 0, 0);
    expect(result).toEqual({ line: 5, col: 3 });
  });

  it("finds segment for column within range", () => {
    const map = makeMap([
      [
        [0, 0, 5, 0],
        [10, 0, 6, 0],
      ],
    ]);
    // Column 5 falls within [0, 10) → maps to the first segment
    const result = lookupSource(map, 0, 5);
    expect(result).toEqual({ line: 5, col: 0 });
  });

  it("returns null for column before first segment", () => {
    const map = makeMap([[[5, 0, 3, 0]]]);
    // Column 2 is before the first segment at col 5
    const result = lookupSource(map, 0, 2);
    expect(result).toBeNull();
  });

  it("returns null for line beyond map", () => {
    const map = makeMap([[[0, 0, 0, 0]]]);
    expect(lookupSource(map, 99, 0)).toBeNull();
  });

  it("returns null for empty line", () => {
    const map = makeMap([[], [[0, 0, 1, 0]]]);
    expect(lookupSource(map, 0, 0)).toBeNull();
  });

  it("returns null for empty/invalid map", () => {
    expect(lookupSource("", 0, 0)).toBeNull();
    expect(lookupSource("not json", 0, 0)).toBeNull();
  });

  // @ai-generated - Bidirectional round-trip: source→generated→source
  it("round-trips source→generated→source", () => {
    const segments: Segment[][] = [
      [
        [0, 0, 10, 5],
        [8, 0, 10, 12],
      ],
      [
        [0, 0, 11, 0],
        [4, 0, 11, 4],
      ],
    ];
    const map = makeMap(segments);

    // Forward: source line 10, col 5 → gen line 0, col 0
    const gen = lookupGenerated(map, 10, 5);
    expect(gen).not.toBeNull();

    // Reverse: gen position → back to source
    const src = lookupSource(map, gen!.line, gen!.col);
    expect(src).not.toBeNull();
    expect(src!.line).toBe(10);
    expect(src!.col).toBe(5);
  });

  // @ai-generated - Bidirectional round-trip: generated→source→generated
  it("round-trips generated→source→generated", () => {
    const segments: Segment[][] = [
      [[0, 0, 3, 0]],
      [
        [0, 0, 4, 2],
        [6, 0, 4, 8],
      ],
    ];
    const map = makeMap(segments);

    // Reverse: gen line 1, col 6 → source
    const src = lookupSource(map, 1, 6);
    expect(src).not.toBeNull();

    // Forward: source → gen
    const gen = lookupGenerated(map, src!.line, src!.col);
    expect(gen).not.toBeNull();
    expect(gen!.line).toBe(1);
    expect(gen!.col).toBe(6);
  });
});
