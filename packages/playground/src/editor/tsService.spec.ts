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

    const diagnosticTsxStart = tsxCode.lastIndexOf("msg");
    const mapper = new SourceMapMapper(sourceMap, tsxCode, vueCode);
    const expectedVueStart = mapper.tsxOffsetToVueOffset(diagnosticTsxStart);
    const expectedVueEnd = mapper.tsxOffsetToVueOffset(diagnosticTsxStart + 3);

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
});
