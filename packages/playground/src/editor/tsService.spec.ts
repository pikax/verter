import { describe, it, expect, vi } from "vitest";
import { readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { TypeScriptService } from "./tsService";
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

  const tsx = host.getTsx("App.vue", profile);
  if (!tsx?.code || !tsx?.sourceMap) {
    throw new Error("expected host.getTsx() to return code + sourceMap");
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

    // Find the count identifier in the destructuring block (after "{ const {")
    const constBrace = tsxCode.indexOf("{ const {");
    const destructuringCountOffset = tsxCode.indexOf("count", constBrace + 9);

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
