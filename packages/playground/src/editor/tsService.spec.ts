import { describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { TypeScriptService, resolveOffsetComment } from "./tsService";
import { SourceMapMapper } from "./sourceMapMapper";

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

  const profile = { sourceMap: true, target: "ide", forceJs: true };
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

  const tsx = host.getIde("App.vue", profile);
  if (!tsx?.code || !tsx?.sourceMap) {
    throw new Error("expected host.getIde() to return code + sourceMap");
  }

  return { code: tsx.code, sourceMap: tsx.sourceMap };
}

describe("TypeScriptService mapping", () => {
  it("maps Vue hover offsets to TSX offsets using real source maps", async () => {
    const vueCode = `<script setup lang=\"ts\">\nconst msg: string = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>`;
    const { code: tsxCode, sourceMap } = await generateRealTsxOutput(vueCode);

    const mapper = new SourceMapMapper(sourceMap, tsxCode, vueCode);
    const vueOffset = vueCode.indexOf("{{ msg }}") + 3;
    const expectedTsxOffset = mapper.vueOffsetToTsxOffset(vueOffset);

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.currentTsxPath = "/App.vue.tsx";
    service.currentMapper = mapper;

    const send = vi.fn(async (_type: string, payload: any) => ({
      text: "const msg: string",
      documentation: "",
      start: payload.offset,
      length: 3,
    }));
    service.send = send;

    const hover = await service.getHover("App.vue", vueOffset);

    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith("getHover", {
      path: "/App.vue.tsx",
      offset: expectedTsxOffset,
    });
    expect(hover).toContain("```typescript");
    expect(hover).toContain("const msg: string");
  });

  it("maps TypeScript diagnostics back to Vue offsets using real source maps", async () => {
    const vueCode = `<script setup lang=\"ts\">\nconst msg: string = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>`;
    const { code: tsxCode, sourceMap } = await generateRealTsxOutput(vueCode);

    const service = new TypeScriptService() as any;
    service.initialized = true;

    // Use the first (script-section) occurrence of "msg" which is reliably source-mapped
    const diagnosticTsxStart = tsxCode.indexOf("msg");
    const mapper = new SourceMapMapper(sourceMap, tsxCode, vueCode);
    const mappedVueStart = mapper.tsxOffsetToVueOffset(diagnosticTsxStart);
    const mappedVueEnd = mapper.tsxOffsetToVueOffset(diagnosticTsxStart + 3);

    // syncTsx falls back to raw TSX offsets when source map mapping returns null
    const expectedVueStart = mappedVueStart ?? diagnosticTsxStart;
    const expectedVueEnd = mappedVueEnd ?? diagnosticTsxStart + 3;

    const send = vi.fn(async (type: string) => {
      if (type === "updateFile") return undefined;
      if (type === "getDiagnostics") {
        return [
          {
            message: "mock error",
            start: diagnosticTsxStart,
            length: 3,
            category: 1,
            code: 2322,
          },
        ];
      }
      throw new Error(`unexpected call: ${type}`);
    });
    service.send = send;

    const diagnostics = await service.syncTsx("App.vue", tsxCode, vueCode, sourceMap);

    expect(send).toHaveBeenCalledWith("updateFile", {
      path: "/App.vue.tsx",
      content: tsxCode,
    });
    expect(send).toHaveBeenCalledWith("getDiagnostics", {
      path: "/App.vue.tsx",
    });

    expect(diagnostics).toHaveLength(1);
    expect(diagnostics[0].message).toBe("mock error");
    expect(diagnostics[0].severity).toBe("error");
    expect(diagnostics[0].start).toBe(expectedVueStart);
    expect(diagnostics[0].end).toBe(expectedVueEnd);
  });

  it("maps Vue completion offsets to TSX offsets using real source maps", async () => {
    const vueCode = `<script setup lang=\"ts\">\nconst message = 'hello'\n</script>\n<template><div>{{ mes }}</div></template>`;
    const { code: tsxCode, sourceMap } = await generateRealTsxOutput(vueCode);

    const mapper = new SourceMapMapper(sourceMap, tsxCode, vueCode);
    const vueOffset = vueCode.indexOf("mes") + 3;
    const expectedTsxOffset = mapper.vueOffsetToTsxOffset(vueOffset);

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.currentTsxPath = "/App.vue.tsx";
    service.currentMapper = mapper;

    const send = vi.fn(async (_type: string, payload: any) => [
      {
        label: "message",
        kind: "property",
        sortText: "0",
        requestedOffset: payload.offset,
      },
    ]);
    service.send = send;

    const completions = await service.getCompletions("App.vue", vueOffset);

    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith("getCompletions", {
      path: "/App.vue.tsx",
      offset: expectedTsxOffset,
    });
    expect(completions).toHaveLength(1);
    expect(completions[0].label).toBe("message");
    expect(completions[0].kind).toBe(9);
  });

  it("maps definition spans back to Vue offsets", async () => {
    const vueCode = `<script setup lang=\"ts\">\nconst msg: string = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>`;
    const { code: tsxCode, sourceMap } = await generateRealTsxOutput(vueCode);

    const mapper = new SourceMapMapper(sourceMap, tsxCode, vueCode);
    const vueOffset = vueCode.indexOf("msg }}");
    const expectedTsxOffset = mapper.vueOffsetToTsxOffset(vueOffset)!;
    const definitionTsxStart = tsxCode.indexOf("msg: string");
    const expectedVueStart = mapper.tsxOffsetToVueOffset(definitionTsxStart)!;
    const expectedVueEnd = mapper.tsxOffsetToVueOffset(definitionTsxStart + 3)!;

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.currentTsxPath = "/App.vue.tsx";
    service.currentMapper = mapper;
    service.send = vi.fn(async () => [
      { fileName: "/App.vue.tsx", start: definitionTsxStart, length: 3 },
    ]);

    const defs = await service.getDefinition("App.vue", vueOffset);

    expect(service.send).toHaveBeenCalledWith("getDefinition", {
      path: "/App.vue.tsx",
      offset: expectedTsxOffset,
    });
    expect(defs).toEqual([{ start: expectedVueStart, end: expectedVueEnd }]);
  });

  it("maps references back to Vue offsets and keeps definition flag", async () => {
    const vueCode = `<script setup lang=\"ts\">\nconst msg: string = 'hello'\nconsole.log(msg)\n</script>\n<template><div>{{ msg }}</div></template>`;
    const { code: tsxCode, sourceMap } = await generateRealTsxOutput(vueCode);

    const mapper = new SourceMapMapper(sourceMap, tsxCode, vueCode);
    const vueOffset = vueCode.lastIndexOf("msg") + 1;
    const tsxUseStart = tsxCode.lastIndexOf("msg");

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.currentTsxPath = "/App.vue.tsx";
    service.currentMapper = mapper;
    service.send = vi.fn(async () => [
      { fileName: "/App.vue.tsx", start: tsxUseStart, length: 3, isDefinition: false },
      { fileName: "/App.vue.tsx", start: tsxCode.indexOf("msg: string"), length: 3, isDefinition: true },
    ]);

    const refs = await service.getReferences("App.vue", vueOffset);

    expect(refs).toHaveLength(2);
    expect(refs.some((ref: any) => ref.isDefinition === true)).toBe(true);
    expect(refs.some((ref: any) => ref.isDefinition === false)).toBe(true);
  });

  it("returns null hover when source map mapper is unavailable", async () => {
    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.currentTsxPath = "/App.vue.tsx";
    service.currentMapper = null; // No source map

    const send = vi.fn();
    service.send = send;

    const hover = await service.getHover("App.vue", 42);

    // Should NOT call the worker — returns null early
    expect(send).not.toHaveBeenCalled();
    expect(hover).toBeNull();
  });

  it("returns empty completions when source map mapper is unavailable", async () => {
    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.currentTsxPath = "/App.vue.tsx";
    service.currentMapper = null;

    const send = vi.fn();
    service.send = send;

    const completions = await service.getCompletions("App.vue", 42);

    expect(send).not.toHaveBeenCalled();
    expect(completions).toEqual([]);
  });

  it("returns empty definitions when source map mapper is unavailable", async () => {
    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.currentTsxPath = "/App.vue.tsx";
    service.currentMapper = null;

    const send = vi.fn();
    service.send = send;

    const defs = await service.getDefinition("App.vue", 42);

    expect(send).not.toHaveBeenCalled();
    expect(defs).toEqual([]);
  });

  it("returns empty references when source map mapper is unavailable", async () => {
    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.currentTsxPath = "/App.vue.tsx";
    service.currentMapper = null;

    const send = vi.fn();
    service.send = send;

    const refs = await service.getReferences("App.vue", 42);

    expect(send).not.toHaveBeenCalled();
    expect(refs).toEqual([]);
  });

  it("returns empty highlights when source map mapper is unavailable", async () => {
    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.currentTsxPath = "/App.vue.tsx";
    service.currentMapper = null;

    const send = vi.fn();
    service.send = send;

    const highlights = await service.getDocumentHighlights("App.vue", 42);

    expect(send).not.toHaveBeenCalled();
    expect(highlights).toEqual([]);
  });

  it("returns canRename:false when source map mapper is unavailable", async () => {
    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.currentTsxPath = "/App.vue.tsx";
    service.currentMapper = null;

    const send = vi.fn();
    service.send = send;

    const rename = await service.getRenameLocations("App.vue", 42);

    expect(send).not.toHaveBeenCalled();
    expect(rename.canRename).toBe(false);
    expect(rename.rejectReason).toContain("Source map");
  });

  it("syncTsx sets mapper to null when sourceMap is empty", async () => {
    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.send = vi.fn(async () => []);

    await service.syncTsx("App.vue", "const x = 1;", "const x = 1;", "");

    expect(service.currentMapper).toBeNull();
    expect(service.currentTsxPath).toBe("/App.vue.tsx");
  });

  it("syncTsx sets mapper to null when sourceMap is '{}'", async () => {
    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.send = vi.fn(async () => []);

    await service.syncTsx("App.vue", "const x = 1;", "const x = 1;", "{}");

    // "{}" is length 2 — threshold is > 2
    expect(service.currentMapper).toBeNull();
  });

  it("syncTsx creates mapper when valid sourceMap is provided", async () => {
    const vueCode = `<script setup lang=\"ts\">\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>`;
    const { code: tsxCode, sourceMap } = await generateRealTsxOutput(vueCode);

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.send = vi.fn(async () => []);

    await service.syncTsx("App.vue", tsxCode, vueCode, sourceMap);

    expect(service.currentMapper).not.toBeNull();
    expect(service.currentTsxPath).toBe("/App.vue.tsx");
  });

  it("maps rename spans and propagates rejection reasons", async () => {
    const vueCode = `<script setup lang=\"ts\">\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>`;
    const { code: tsxCode, sourceMap } = await generateRealTsxOutput(vueCode);

    const mapper = new SourceMapMapper(sourceMap, tsxCode, vueCode);
    const vueOffset = vueCode.indexOf("msg }}");
    const renameTsxStart = tsxCode.indexOf("msg");

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.currentTsxPath = "/App.vue.tsx";
    service.currentMapper = mapper;
    service.send = vi.fn(async () => ({
      canRename: true,
      localizedErrorMessage: null,
      triggerSpan: { start: renameTsxStart, length: 3 },
      locations: [{ fileName: "/App.vue.tsx", start: renameTsxStart, length: 3 }],
    }));

    const rename = await service.getRenameLocations("App.vue", vueOffset);
    expect(rename.canRename).toBe(true);
    expect(rename.triggerSpan).toBeTruthy();
    expect(rename.locations.length).toBeGreaterThan(0);

    service.send = vi.fn(async () => ({
      canRename: false,
      localizedErrorMessage: "Cannot rename this symbol",
      triggerSpan: null,
      locations: [],
    }));

    const rejected = await service.getRenameLocations("App.vue", vueOffset);
    expect(rejected.canRename).toBe(false);
    expect(rejected.rejectReason).toContain("Cannot rename");
  });

  it("getDefinition falls back to offset comment when source map fails", async () => {
    const vueCode = `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>`;
    const { code: tsxCode, sourceMap } = await generateRealTsxOutput(vueCode);

    // Find offset comment pattern /*start,end*/ before "count" in destructuring
    const offsetCommentMatch = tsxCode.match(/\/\*(\d+),(\d+)\*\/\s*\n\s*count/);
    expect(offsetCommentMatch).not.toBeNull();
    const byteStart = parseInt(offsetCommentMatch![1]);
    const byteEnd = parseInt(offsetCommentMatch![2]);

    // For ASCII content, byte offsets === JS string indices
    expect(vueCode.slice(byteStart, byteEnd)).toBe("count");

    // Test the mapTsxSpanToVueSpan offset comment fallback directly
    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.currentTsxPath = "/App.vue.tsx";
    service.currentTsxCode = tsxCode;
    service.currentVueCode = vueCode;
    // Mapper that always fails TSX→Vue (forces offset comment fallback)
    service.currentMapper = { tsxOffsetToVueOffset: () => null };

    // Find the count identifier in the destructuring block (after "let {" or "const {")
    const letBrace = tsxCode.indexOf("let {");
    const constBrace = tsxCode.indexOf("{ const {");
    const braceStart = letBrace >= 0 ? letBrace : constBrace;
    expect(braceStart).toBeGreaterThanOrEqual(0);
    const destructuringCountOffset = tsxCode.indexOf("count", braceStart + 4);

    const mapped = service.mapTsxSpanToVueSpan({
      fileName: "/App.vue.tsx",
      start: destructuringCountOffset,
      length: 5,
    });

    expect(mapped).not.toBeNull();
    expect(mapped!.start).toBe(byteStart);
    expect(mapped!.end).toBe(byteEnd);
  });

  it("mapTsxSpanToVueSpan returns null when both mappings fail", async () => {
    const service = new TypeScriptService() as any;
    service.currentTsxPath = "/App.vue.tsx";
    service.currentTsxCode = null;
    service.currentVueCode = null;
    // Mapper that always returns null
    service.currentMapper = { tsxOffsetToVueOffset: () => null };
    const result = service.mapTsxSpanToVueSpan({
      fileName: "/App.vue.tsx",
      start: 100,
      length: 5,
    });
    expect(result).toBeNull();
  });

  it("maps script-section offset with syntax error (count.)", async () => {
    const vueCode = `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>
<template><div>{{ count }}</div></template>`;

    const { code: tsxCode, sourceMap } = await generateRealTsxOutput(vueCode);

    const mapper = new SourceMapMapper(sourceMap, tsxCode, vueCode);

    // Map the cursor position after "count." (right after the dot)
    const vueDotOffset = vueCode.indexOf("count.\n") + 6;
    const tsxOffset = mapper.vueOffsetToTsxOffset(vueDotOffset);

    // The mapping should not return null — the error-mode script stays at file scope
    expect(tsxOffset).not.toBeNull();

    // The mapped position should be near "count." in the TSX
    const tsxCountStart = tsxCode.indexOf("count.\n");
    expect(tsxCountStart).toBeGreaterThan(-1);
    // TSX offset should be within the "count.\n" range in the TSX
    expect(tsxOffset).toBeGreaterThanOrEqual(tsxCountStart);
    expect(tsxOffset).toBeLessThanOrEqual(tsxCountStart + 7);
  });

  it("ensureTsxCurrent updates mapper and worker file before completions", async () => {
    // Simulate the race condition: old compile → new compile → completions
    const oldVueCode = `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>`;

    const newVueCode = `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>
<template><div>{{ count }}</div></template>`;

    const oldResult = await generateRealTsxOutput(oldVueCode);
    const newResult = await generateRealTsxOutput(newVueCode);

    const service = new TypeScriptService() as any;
    service.initialized = true;

    const sendCalls: Array<{ type: string; payload: any }> = [];
    service.send = vi.fn(async (type: string, payload: any) => {
      sendCalls.push({ type, payload });
      if (type === "getCompletions") return [];
      if (type === "getDiagnostics") return [];
      return undefined;
    });

    // First sync establishes old state
    await service.syncTsx("App.vue", oldResult.code, oldVueCode, oldResult.sourceMap);

    // Verify old mapper is set
    expect(service.currentMapper).not.toBeNull();
    const oldMapper = service.currentMapper;

    // Now ensureTsxCurrent with new code (simulating what happens before completions)
    sendCalls.length = 0;
    await service.ensureTsxCurrent("App.vue", newResult.code, newVueCode, newResult.sourceMap);

    // Mapper should be updated
    expect(service.currentMapper).not.toBe(oldMapper);
    expect(service.currentTsxCode).toBe(newResult.code);

    // Worker file should be updated
    expect(sendCalls).toEqual([
      { type: "updateFile", payload: { path: "/App.vue.tsx", content: newResult.code } },
    ]);

    // Now mapping the "count." position should work with the new mapper
    const vueDotOffset = newVueCode.indexOf("count.\n") + 6;
    const tsxOffset = service.currentMapper.vueOffsetToTsxOffset(vueDotOffset);
    expect(tsxOffset).not.toBeNull();
  });

  it("ensureTsxCurrent skips update when tsx unchanged", async () => {
    const vueCode = `<script setup lang="ts">
const x = 1
</script>`;

    const result = await generateRealTsxOutput(vueCode);

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.send = vi.fn(async () => []);

    // First sync
    await service.syncTsx("App.vue", result.code, vueCode, result.sourceMap);
    service.send.mockClear();

    // ensureTsxCurrent with same code — should be a no-op
    await service.ensureTsxCurrent("App.vue", result.code, vueCode, result.sourceMap);
    expect(service.send).not.toHaveBeenCalled();
  });

  it("stale mapper returns null for new line added after last sync (race condition)", async () => {
    // Simulate: user had working code, types `.`, completions trigger BEFORE recompile
    const oldVueCode = `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>`;

    const newVueCode = `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>
<template><div>{{ count }}</div></template>`;

    const oldResult = await generateRealTsxOutput(oldVueCode);

    // Sync the OLD code (simulates last successful sync)
    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.send = vi.fn(async () => []);
    await service.syncTsx("App.vue", oldResult.code, oldVueCode, oldResult.sourceMap);

    // Now try to get completions at `count.` using the OLD mapper (no re-sync)
    // This is the race: user typed `.` but syncTsx debounce hasn't fired
    const vueDotOffset = newVueCode.indexOf("count.\n") + 6;
    const tsxOffset = service.mapVueOffsetToTsxOffset(vueDotOffset);

    // The OLD mapper can't map this position correctly — `count.` is on a new line
    // that didn't exist in the old code. The offset falls on what was `</script>`
    // in the old source, which maps to the function wrapper (not `count.`).
    // This proves the race condition causes wrong completions.
    if (tsxOffset !== null) {
      // If it maps somewhere, it maps to the WRONG place in the old TSX
      const context = oldResult.code.substring(
        Math.max(0, tsxOffset - 10),
        Math.min(oldResult.code.length, tsxOffset + 10),
      );
      // The mapped position should NOT be near "count." in old TSX
      // (because "count." doesn't exist in the old code)
      expect(context).not.toContain("count.");
    }
    // Most likely it returns null (no mapping for new line) → empty completions
    // Either way, the completions are wrong without ensureTsxCurrent
  });

  it("ensureTsxCurrent fixes stale mapper race condition", async () => {
    // Same scenario, but this time we call ensureTsxCurrent with the new TSX
    const oldVueCode = `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>`;

    const newVueCode = `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>
<template><div>{{ count }}</div></template>`;

    const oldResult = await generateRealTsxOutput(oldVueCode);
    const newResult = await generateRealTsxOutput(newVueCode);

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.send = vi.fn(async () => []);

    // Sync old code first
    await service.syncTsx("App.vue", oldResult.code, oldVueCode, oldResult.sourceMap);

    // Now simulate what ensureTsxSynced does: call ensureTsxCurrent with NEW tsx
    await service.ensureTsxCurrent(
      "App.vue",
      newResult.code,
      newVueCode,
      newResult.sourceMap,
    );

    // Now mapping should work correctly with the fresh mapper
    const vueDotOffset = newVueCode.indexOf("count.\n") + 6;
    const tsxOffset = service.mapVueOffsetToTsxOffset(vueDotOffset);

    expect(tsxOffset).not.toBeNull();

    // The mapped position should be near `count.` in the NEW TSX
    const tsxCountDot = newResult.code.indexOf("count.\n");
    expect(tsxCountDot).toBeGreaterThan(-1);
    expect(tsxOffset).toBeGreaterThanOrEqual(tsxCountDot);
    expect(tsxOffset).toBeLessThanOrEqual(tsxCountDot + 7);
  });
});

describe("multi-file support", () => {
  it("syncDtsFiles sends updateFile with .d.ts paths for each file", async () => {
    const service = new TypeScriptService() as any;
    service.initialized = true;

    const sendCalls: Array<{ type: string; payload: any }> = [];
    service.send = vi.fn(async (type: string, payload: any) => {
      sendCalls.push({ type, payload });
      return undefined;
    });

    await service.syncDtsFiles([
      { filename: "Comp.vue", dtsCode: "export default {} as any;" },
      { filename: "Utils.vue", dtsCode: "export default {} as any;" },
    ]);

    expect(sendCalls).toEqual([
      { type: "updateFile", payload: { path: "/Comp.vue.d.ts", content: "export default {} as any;" } },
      { type: "updateFile", payload: { path: "/Utils.vue.d.ts", content: "export default {} as any;" } },
    ]);
    // Must NOT use .tsx paths
    expect(sendCalls.some((c) => c.payload.path.endsWith(".tsx"))).toBe(false);
  });

  it("syncDtsFiles includes the active file (not skipped)", async () => {
    const service = new TypeScriptService() as any;
    service.initialized = true;

    const sendCalls: Array<{ type: string; payload: any }> = [];
    service.send = vi.fn(async (type: string, payload: any) => {
      sendCalls.push({ type, payload });
      return undefined;
    });

    // Set current active file
    service.currentTsxPath = "/App.vue.tsx";

    await service.syncDtsFiles([
      { filename: "App.vue", dtsCode: "export default {} as any;" }, // active — should NOT be skipped
      { filename: "Comp.vue", dtsCode: "export default {} as any;" },
    ]);

    // Both files should be sent (active file is included for .d.ts)
    expect(sendCalls).toHaveLength(2);
    expect(sendCalls[0].payload.path).toBe("/App.vue.d.ts");
    expect(sendCalls[1].payload.path).toBe("/Comp.vue.d.ts");
  });

  it("syncDtsFiles is no-op when not initialized", async () => {
    const service = new TypeScriptService() as any;
    service.initialized = false;

    const send = vi.fn();
    service.send = send;

    await service.syncDtsFiles([
      { filename: "Comp.vue", dtsCode: "export default {} as any;" },
    ]);

    expect(send).not.toHaveBeenCalled();
  });

  it("closeFile sends closeFile message to worker", async () => {
    const service = new TypeScriptService() as any;
    service.initialized = true;

    const sendCalls: Array<{ type: string; payload: any }> = [];
    service.send = vi.fn(async (type: string, payload: any) => {
      sendCalls.push({ type, payload });
      return undefined;
    });

    await service.closeFile("Comp.vue");

    expect(sendCalls).toEqual([
      { type: "closeFile", payload: { path: "/Comp.vue.tsx" } },
    ]);
  });

  it("closeFile is no-op when not initialized", async () => {
    const service = new TypeScriptService() as any;
    service.initialized = false;

    const send = vi.fn();
    service.send = send;

    await service.closeFile("Comp.vue");

    expect(send).not.toHaveBeenCalled();
  });
});

// ── Offset comment resolution and diagnostic mapping ──

describe("resolveOffsetComment", () => {
  it("resolves offset comment directly before the identifier", () => {
    const tsxCode = `const { /*125,134*/\n    increment } = ___VERTER___unwrapped;`;
    const vueCode = "x".repeat(200);

    const incrementIdx = tsxCode.indexOf("increment");
    const result = resolveOffsetComment(tsxCode, vueCode, incrementIdx);
    expect(result).not.toBeNull();
    expect(result!.start).toBe(125);
    expect(result!.end).toBe(134);
  });

  it("returns null when no offset comment is found before the position", () => {
    const tsxCode = `const x = 1;`;
    const vueCode = "hello";
    const result = resolveOffsetComment(tsxCode, vueCode, 6);
    expect(result).toBeNull();
  });

  it("returns null when comment end is at or after position", () => {
    // The comment `*​/` must end BEFORE the tsxOffset
    const tsxCode = `/*10,15*/ y`;
    const vueCode = "x".repeat(20);
    // tsxOffset at index 2 is inside the comment — resolving should fail
    const result = resolveOffsetComment(tsxCode, vueCode, 2);
    expect(result).toBeNull();
  });

  it("returns null for non-numeric comment content", () => {
    const tsxCode = `/* hello */ const x = 1;`;
    const vueCode = "test";
    const result = resolveOffsetComment(tsxCode, vueCode, 15);
    expect(result).toBeNull();
  });

  it("handles multiple offset comments and picks the closest one before position", () => {
    const tsxCode = `{ const { /*10,15*/\n    count, /*30,37*/\n    message } = ___VERTER___unwrapped; }`;
    const vueCode = "x".repeat(50);

    const messageIdx = tsxCode.indexOf("message");
    const result = resolveOffsetComment(tsxCode, vueCode, messageIdx);
    expect(result).not.toBeNull();
    expect(result!.start).toBe(30);
    expect(result!.end).toBe(37);

    const countIdx = tsxCode.indexOf("count");
    const countResult = resolveOffsetComment(tsxCode, vueCode, countIdx);
    expect(countResult).not.toBeNull();
    expect(countResult!.start).toBe(10);
    expect(countResult!.end).toBe(15);
  });

  it("handles UTF-8 multi-byte characters in Vue source", () => {
    // "日本語" = 3 chars, 3 bytes each = 9 bytes
    const tsxCode = `{ const { /*9,14*/\n    count } = ___VERTER___unwrapped; }`;
    const vueCode = "日本語count = ref(0)";

    const countIdx = tsxCode.indexOf("count");
    const result = resolveOffsetComment(tsxCode, vueCode, countIdx);
    expect(result).not.toBeNull();
    // utf8ByteOffsetToJsOffset: byte 9 = JS index 3 (after 3 CJK chars)
    expect(result!.start).toBe(3);
    // byte 14 = JS index 8 ("count" = 5 chars, 3+5=8)
    expect(result!.end).toBe(8);
  });
});

describe("syncTsx diagnostic offset comment fallback", () => {
  it("maps TS6133 on destructured binding to original declaration via offset comment", async () => {
    const vueCode = `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
function increment() {
  count.value++
}
</script>
<template><div class="app"></div></template>`;

    const { code: tsxCode } = await generateRealTsxOutput(vueCode);

    // Find the offset comment for "increment" in the destructuring
    const offsetCommentMatch = tsxCode.match(/\/\*(\d+),(\d+)\*\/\s*\n\s*increment/);
    expect(offsetCommentMatch).not.toBeNull();
    const sfcStart = parseInt(offsetCommentMatch![1]);
    const sfcEnd = parseInt(offsetCommentMatch![2]);
    // Verify offset comment points to the original declaration
    expect(vueCode.slice(sfcStart, sfcEnd)).toBe("increment");

    // Find "increment" position in the destructuring block
    const destructBlock = tsxCode.indexOf("___VERTER___unwrapped;");
    expect(destructBlock).toBeGreaterThan(-1);
    const incrementInDestruct = tsxCode.lastIndexOf("increment", destructBlock);

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.send = vi.fn(async (type: string) => {
      if (type === "updateFile") return undefined;
      if (type === "getDiagnostics") {
        return [
          {
            message: "'increment' is declared but its value is never read.",
            start: incrementInDestruct,
            length: "increment".length,
            category: 1,
            code: 6133,
          },
        ];
      }
    });

    // Use null source map to force offset comment fallback
    const diagnostics = await service.syncTsx("App.vue", tsxCode, vueCode, null);

    // Should map to the original declaration, NOT the raw TSX offset
    expect(diagnostics).toHaveLength(1);
    expect(diagnostics[0].start).toBe(sfcStart);
    expect(diagnostics[0].end).toBe(sfcEnd);
    expect(diagnostics[0].code).toBe(6133);
  });

  it("expands TS6198 on single-element destructuring to TS6133 for that binding", async () => {
    const vueCode = `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
function increment() {
  count.value++
}
</script>
<template><div class="app"></div></template>`;

    const { code: tsxCode } = await generateRealTsxOutput(vueCode);

    expect(tsxCode).toContain("/* verter-destructured-start */");
    expect(tsxCode).toContain("/* verter-destructured-end */");

    const startMarker = tsxCode.indexOf("/* verter-destructured-start */");
    const endMarker = tsxCode.indexOf("/* verter-destructured-end */");
    const incrementInDestruct = tsxCode.indexOf("increment", startMarker);
    expect(incrementInDestruct).toBeGreaterThan(startMarker);
    expect(incrementInDestruct).toBeLessThan(endMarker);

    // Find the expected SFC position for "increment"
    const sfcIncrementStart = vueCode.indexOf("increment");

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.send = vi.fn(async (type: string) => {
      if (type === "updateFile") return undefined;
      if (type === "getDiagnostics") {
        return [
          {
            message: "All destructured elements are unused.",
            start: incrementInDestruct,
            length: "increment".length,
            category: 1,
            code: 6198,
          },
        ];
      }
    });

    const diagnostics = await service.syncTsx("App.vue", tsxCode, vueCode, null);

    // TS6198 should be expanded into a TS6133-like diagnostic for "increment"
    expect(diagnostics).toHaveLength(1);
    expect(diagnostics[0].code).toBe(6133);
    expect(diagnostics[0].start).toBe(sfcIncrementStart);
    expect(diagnostics[0].message).toContain("increment");
    expect(diagnostics[0].message).toContain("declared but");
  });

  it("expands TS6198 on multi-element destructuring to TS6133 for each binding", async () => {
    const vueCode = `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
const message = ref('Hello from Verter!')
function increment() {
  count.value++
}
</script>
<template><div class="app"></div></template>`;

    const { code: tsxCode } = await generateRealTsxOutput(vueCode);

    const startMarker = tsxCode.indexOf("/* verter-destructured-start */");
    const endMarker = tsxCode.indexOf("/* verter-destructured-end */");

    // TS would report two TS6198 diagnostics:
    // 1. For "const { increment }" (single element, all unused)
    // 2. For "let { count, message }" (all elements unused, start at "count")
    const incrementInDestruct = tsxCode.indexOf("increment", startMarker);
    const countInDestruct = tsxCode.indexOf("count", startMarker + 30);
    // Make sure both are inside the markers
    expect(incrementInDestruct).toBeGreaterThan(startMarker);
    expect(incrementInDestruct).toBeLessThan(endMarker);
    expect(countInDestruct).toBeGreaterThan(startMarker);
    expect(countInDestruct).toBeLessThan(endMarker);

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.send = vi.fn(async (type: string) => {
      if (type === "updateFile") return undefined;
      if (type === "getDiagnostics") {
        return [
          {
            message: "All destructured elements are unused.",
            start: incrementInDestruct,
            length: "increment".length,
            category: 1,
            code: 6198,
          },
          {
            message: "All destructured elements are unused.",
            start: countInDestruct,
            length: "count".length,
            category: 1,
            code: 6198,
          },
        ];
      }
    });

    const diagnostics = await service.syncTsx("App.vue", tsxCode, vueCode, null);

    // Should expand to 3 individual diagnostics: increment, count, message
    expect(diagnostics).toHaveLength(3);
    const names = diagnostics.map((d: any) => vueCode.slice(d.start, d.end));
    expect(names).toContain("increment");
    expect(names).toContain("count");
    expect(names).toContain("message");
    // All should be TS6133 with proper messages
    for (const d of diagnostics) {
      expect(d.code).toBe(6133);
      expect(d.message).toContain("declared but");
    }
  });

  it("keeps TS6133 inside verter-destructured markers (mapped via offset comment)", async () => {
    const vueCode = `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
function increment() {
  count.value++
}
</script>
<template><div class="app"></div></template>`;

    const { code: tsxCode } = await generateRealTsxOutput(vueCode);

    const startMarker = tsxCode.indexOf("/* verter-destructured-start */");
    const endMarker = tsxCode.indexOf("/* verter-destructured-end */");
    const incrementInDestruct = tsxCode.indexOf("increment", startMarker);
    expect(incrementInDestruct).toBeGreaterThan(startMarker);
    expect(incrementInDestruct).toBeLessThan(endMarker);

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.send = vi.fn(async (type: string) => {
      if (type === "updateFile") return undefined;
      if (type === "getDiagnostics") {
        return [
          {
            message: "'increment' is declared but its value is never read.",
            start: incrementInDestruct,
            length: "increment".length,
            category: 1,
            code: 6133,
          },
        ];
      }
    });

    const diagnostics = await service.syncTsx("App.vue", tsxCode, vueCode, null);

    // TS6133 should be KEPT — individual unused binding diagnostics are valuable
    expect(diagnostics).toHaveLength(1);
    expect(diagnostics[0].code).toBe(6133);
  });

  it("keeps diagnostics that map through source map even when offset comment fails", async () => {
    const vueCode = `<script setup lang="ts">
const count: number = "wrong"
</script>
<template><div>{{ count }}</div></template>`;

    const { code: tsxCode, sourceMap } = await generateRealTsxOutput(vueCode);

    // The type error on 'count' in the script section IS source-mapped
    const countInTsx = tsxCode.indexOf("count: number");
    expect(countInTsx).toBeGreaterThan(-1);

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.send = vi.fn(async (type: string) => {
      if (type === "updateFile") return undefined;
      if (type === "getDiagnostics") {
        return [
          {
            message: "Type 'string' is not assignable to type 'number'.",
            start: countInTsx,
            length: 5,
            category: 1,
            code: 2322,
          },
        ];
      }
    });

    const diagnostics = await service.syncTsx("App.vue", tsxCode, vueCode, sourceMap);

    expect(diagnostics).toHaveLength(1);
    expect(diagnostics[0].code).toBe(2322);
    // The mapped position should point to "count" in the Vue source (not raw TSX offset)
    const vueCount = vueCode.indexOf("count");
    expect(diagnostics[0].start).toBe(vueCount);
  });

  it("maps TS6133 inside destructured block via offset comment even with real source map", async () => {
    const vueCode = `<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
function increment() {
  count.value++
}
</script>
<template><div class="app"></div></template>`;

    const { code: tsxCode, sourceMap } = await generateRealTsxOutput(vueCode);

    // Verify we have boundary markers and offset comments
    const startMarker = tsxCode.indexOf("/* verter-destructured-start */");
    const endMarker = tsxCode.indexOf("/* verter-destructured-end */");
    expect(startMarker).toBeGreaterThan(-1);
    expect(endMarker).toBeGreaterThan(startMarker);

    // Find the offset comment for "increment" in the destructuring
    const offsetCommentMatch = tsxCode.match(/\/\*(\d+),(\d+)\*\/\s*\n\s*increment/);
    expect(offsetCommentMatch).not.toBeNull();
    const sfcStart = parseInt(offsetCommentMatch![1]);
    const sfcEnd = parseInt(offsetCommentMatch![2]);
    expect(vueCode.slice(sfcStart, sfcEnd)).toBe("increment");

    // Find "increment" position inside the destructured block
    const incrementInDestruct = tsxCode.indexOf("increment", startMarker);
    expect(incrementInDestruct).toBeGreaterThan(startMarker);
    expect(incrementInDestruct).toBeLessThan(endMarker);

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.send = vi.fn(async (type: string) => {
      if (type === "updateFile") return undefined;
      if (type === "getDiagnostics") {
        return [
          {
            message: "'increment' is declared but its value is never read.",
            start: incrementInDestruct,
            length: "increment".length,
            category: 1,
            code: 6133,
          },
        ];
      }
    });

    // Key difference: use the REAL source map (not null)
    const diagnostics = await service.syncTsx("App.vue", tsxCode, vueCode, sourceMap);

    // Must map to the ORIGINAL declaration via offset comment, not a wrong source-map position
    expect(diagnostics).toHaveLength(1);
    expect(diagnostics[0].code).toBe(6133);
    expect(diagnostics[0].start).toBe(sfcStart);
    expect(diagnostics[0].end).toBe(sfcEnd);
  });

  it("drops diagnostics pointing to unmappable synthetic code", async () => {
    const vueCode = `<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>`;

    const { code: tsxCode } = await generateRealTsxOutput(vueCode);

    // Diagnostic on ___VERTER___shallowUnwrapRef — purely synthetic
    const syntheticPos = tsxCode.indexOf("shallowUnwrapRef");
    expect(syntheticPos).toBeGreaterThan(-1);

    const service = new TypeScriptService() as any;
    service.initialized = true;
    service.send = vi.fn(async (type: string) => {
      if (type === "updateFile") return undefined;
      if (type === "getDiagnostics") {
        return [
          {
            message: "Some hypothetical error in synthetic code",
            start: syntheticPos,
            length: 15,
            category: 1,
            code: 9999,
          },
        ];
      }
    });

    const diagnostics = await service.syncTsx("App.vue", tsxCode, vueCode, null);

    // Should be dropped — unmappable synthetic code
    expect(diagnostics).toHaveLength(0);
  });
});
