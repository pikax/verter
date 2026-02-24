/**
 * @ai-generated - Tests for SourceMapMapper VLQ decoding and offset mapping.
 */
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { SourceMapMapper } from "./sourceMapMapper";

// Helper: create a minimal V3 source map JSON
function makeSourceMap(
  mappings: string,
  sources = ["input.vue"],
  sourcesContent?: string[],
): string {
  return JSON.stringify({
    version: 3,
    sources,
    mappings,
    names: [],
    ...(sourcesContent ? { sourcesContent } : {}),
  });
}

async function generateRealTsxOutput(vueSource: string): Promise<{ code: string; sourceMap: string }> {
  const thisDir = dirname(fileURLToPath(import.meta.url));
  const wasmJs = resolve(thisDir, "../../../wasm/wasm/verter_wasm.js");
  const wasmBin = resolve(thisDir, "../../../wasm/wasm/verter_wasm_bg.wasm");

  const wasmModule = (await import(pathToFileURL(wasmJs).href)) as any;
  const wasmBytes = readFileSync(wasmBin);
  await wasmModule.default({ module_or_path: wasmBytes });

  const host = new wasmModule.VerterHost({
    devMode: true,
    compileErrorPolicy: "devServeLastKnownGood",
    maxProfilesPerFile: 8,
  });

  const profile = { sourceMap: true, enableTypes: true, forceJs: true };
  host.upsert({
    inputId: "App.vue",
    source: vueSource,
    fileKind: "vue",
    aliases: [],
    compileProfile: profile,
  });

  host.getVirtualFile({
    rawId: "App.vue",
    compileProfile: profile,
  });

  const tsx = host.getTsx("App.vue", profile);
  if (!tsx?.code || !tsx?.sourceMap) {
    throw new Error("expected host.getTsx() to return code + sourceMap");
  }

  return { code: tsx.code, sourceMap: tsx.sourceMap };
}

describe("SourceMapMapper", () => {
  describe("VLQ decoding", () => {
    it("handles empty mappings", () => {
      const mapper = new SourceMapMapper(makeSourceMap(""), "abc", "abc");
      expect(mapper.tsxOffsetToVueOffset(0)).toBeNull();
    });

    it("handles identity mapping (AAAA)", () => {
      // AAAA = genCol:0, sourceIdx:0, sourceLine:0, sourceCol:0
      const mapper = new SourceMapMapper(makeSourceMap("AAAA"), "hello", "hello");
      expect(mapper.tsxOffsetToVueOffset(0)).toBe(0);
      expect(mapper.tsxOffsetToVueOffset(3)).toBe(3);
    });

    it("handles multi-line mappings with semicolons", () => {
      // Line 0: AAAA (col 0 → source line 0, col 0)
      // Line 1: AAAA (col 0 → source line 0, col 0 + relative)
      // Actually AACA means genCol:0, sourceIdx:0, sourceLine:+1, sourceCol:0
      const tsxCode = "line0\nline1";
      const vueCode = "vue0\nvue1";
      const mapper = new SourceMapMapper(makeSourceMap("AAAA;AACA"), tsxCode, vueCode);

      // First line maps to Vue line 0
      expect(mapper.tsxOffsetToVueOffset(0)).toBe(0);
      // Second line (offset 6 in TSX = line 1, col 0) maps to Vue line 1
      expect(mapper.tsxOffsetToVueOffset(6)).toBe(5);
    });

    it("handles segments with column offsets", () => {
      // AAAE = genCol:0, sourceIdx:0, sourceLine:0, sourceCol:2
      // Maps TSX col 0 → Vue col 2
      const tsxCode = "ab";
      const vueCode = "xxab";
      const mapper = new SourceMapMapper(makeSourceMap("AAAE"), tsxCode, vueCode);
      expect(mapper.tsxOffsetToVueOffset(0)).toBe(2);
      expect(mapper.tsxOffsetToVueOffset(1)).toBe(3);
    });
  });

  describe("tsxOffsetToVueOffset", () => {
    it("applies delta for offset beyond mapped segment on same line", () => {
      // AAAA identity maps col 0 → col 0. Offset 100 is line 0, col 100.
      // Delta from segment gives Vue col 100 = offset 100. Source map mappers
      // don't bounds-check against actual text length.
      const mapper = new SourceMapMapper(makeSourceMap("AAAA"), "a", "a");
      expect(mapper.tsxOffsetToVueOffset(100)).toBe(100);
    });

    it("returns null for line with no segments", () => {
      // Only line 0 has segments. Line 1 (after semicolon) has none.
      const mapper = new SourceMapMapper(makeSourceMap("AAAA;"), "a\nb", "a\nb");
      expect(mapper.tsxOffsetToVueOffset(2)).toBeNull(); // offset 2 = line 1, col 0
    });

    it("maps with delta from closest segment", () => {
      // EAAA = genCol:2, sourceIdx:0, sourceLine:0, sourceCol:0
      // TSX col 2 → Vue col 0, so TSX col 3 → Vue col 1
      const tsxCode = "xxhello";
      const vueCode = "hello";
      const mapper = new SourceMapMapper(makeSourceMap("EAAA"), tsxCode, vueCode);
      expect(mapper.tsxOffsetToVueOffset(2)).toBe(0);
      expect(mapper.tsxOffsetToVueOffset(4)).toBe(2);
    });
  });

  describe("vueOffsetToTsxOffset", () => {
    it("maps Vue offset to TSX offset", () => {
      // AAAA = identity mapping
      const mapper = new SourceMapMapper(makeSourceMap("AAAA"), "hello", "hello");
      expect(mapper.vueOffsetToTsxOffset(0)).toBe(0);
      expect(mapper.vueOffsetToTsxOffset(2)).toBe(2);
    });

    it("returns null when no mapping exists for that line", () => {
      // Maps only line 0
      const tsxCode = "a\nb";
      const vueCode = "a\nb\nc";
      const mapper = new SourceMapMapper(makeSourceMap("AAAA"), tsxCode, vueCode);
      // Vue line 2 (offset for 'c') has no mapping
      const result = mapper.vueOffsetToTsxOffset(4);
      // Should return null since line 2 has no segment
      expect(result).toBeNull();
    });

    it("handles reverse mapping with column offset", () => {
      // EAAA: TSX col 2 → Vue col 0
      const tsxCode = "xxhello";
      const vueCode = "hello";
      const mapper = new SourceMapMapper(makeSourceMap("EAAA"), tsxCode, vueCode);
      // Vue offset 0 (line 0, col 0) → TSX offset 2
      expect(mapper.vueOffsetToTsxOffset(0)).toBe(2);
      expect(mapper.vueOffsetToTsxOffset(3)).toBe(5);
    });
  });

  describe("roundtrip", () => {
    it("tsx→vue→tsx roundtrips for identity mapping", () => {
      const mapper = new SourceMapMapper(makeSourceMap("AAAA"), "hello", "hello");
      for (let i = 0; i < 5; i++) {
        const vue = mapper.tsxOffsetToVueOffset(i);
        expect(vue).not.toBeNull();
        const tsx = mapper.vueOffsetToTsxOffset(vue!);
        expect(tsx).toBe(i);
      }
    });
  });

  describe("real host map", () => {
    it("maps real TSX source map generated by host", async () => {
      const vueCode = `<script setup lang=\"ts\">
const msg: string = 'hello'
</script>
<template><div>{{ msg }}</div></template>`;

      const { code: tsxCode, sourceMap } = await generateRealTsxOutput(vueCode);
      const mapper = new SourceMapMapper(sourceMap, tsxCode, vueCode);

      const vueTemplateMsgOffset = vueCode.indexOf("{{ msg }}") + 3;
      const mappedTsxOffset = mapper.vueOffsetToTsxOffset(vueTemplateMsgOffset);

      expect(mappedTsxOffset).not.toBeNull();
      expect(mappedTsxOffset!).toBeGreaterThanOrEqual(0);

      const roundtripVueOffset = mapper.tsxOffsetToVueOffset(mappedTsxOffset!);
      expect(roundtripVueOffset).not.toBeNull();
      expect(Math.abs((roundtripVueOffset ?? 0) - vueTemplateMsgOffset)).toBeLessThanOrEqual(2);
    });
  });
});
