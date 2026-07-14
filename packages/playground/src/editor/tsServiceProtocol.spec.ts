/**
 * Guards for the main-thread `TypeScriptService` carrier protocol (the bridge
 * to the in-context LanguageService worker):
 *
 * - sync model — ONE atomic `syncSource` per source: a framework carrier
 *   pushes its three WASM surfaces (ide/decl/api), a plain `.ts` user file
 *   pushes raw content; unchanged sources are not re-sent; a removed source
 *   sends `removeSource`.
 * - result mapping — every worker result span maps through the CORE strict
 *   per-carrier mapper registered for THAT span's `fileName`; a span with no
 *   mapper, in synthetic (unmapped) generated space, or landing in a
 *   different source DROPS (fail closed) — never a closest-segment snap,
 *   never a single-active-file assumption.
 * - query direction — a source offset translates through the strict
 *   source→generated direction of the active carrier's mapper onto the IDE
 *   carrier path; an unmapped source position fails closed WITHOUT calling
 *   the worker.
 *
 * Pinned to COMMITTED real WASM-produced fixtures (`wasm-carriers.json`).
 */
import { describe, it, expect, vi } from "vitest";
import { TypeScriptService } from "./tsService";
import { fixtures } from "./__fixtures__/wasmLsKit";

const fx = fixtures.compVue;

interface SentMessage {
  type: string;
  payload: unknown;
}

/** A service with a captured `send` (no real worker) marked initialized. */
function stubbedService(respond?: (type: string, payload: unknown) => unknown) {
  const service = new TypeScriptService();
  const sent: SentMessage[] = [];
  const send = vi.fn(async (type: string, payload?: unknown) => {
    sent.push({ type, payload });
    return respond ? respond(type, payload) : "ok";
  });
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const anyService = service as any;
  anyService.initialized = true;
  anyService.send = send;
  return { service, sent, send };
}

function compVueFile() {
  return {
    filename: "Comp.vue",
    code: fx.source,
    compiled: {
      types: fx.ide!.code,
      typesSourceMap: fx.ide!.sourceMap ?? "",
      declCode: fx.decl!.code,
      declSourceMap: fx.decl!.sourceMap ?? "",
      tscCode: fx.api!.code,
    },
  };
}

function userTsFile(code = "export const one = 1;\n") {
  return {
    filename: "utils.ts",
    code,
    compiled: { types: "", typesSourceMap: "", declCode: "", declSourceMap: "", tscCode: "" },
  };
}

// Real-fixture generated offsets (see wasmCarrierMapping.spec.ts):
// a genuinely MAPPED token …
const GEN_COUNT_IN_TYPE = fx.ide!.code.indexOf("{ count: number }") + 2;
const GEN_COUNT_IN_TEMPLATE = fx.ide!.code.indexOf("__props.count") + 8;
// … and SYNTHETIC generated space (drops under the strict mapper).
const GEN_SYNTHETIC = fx.ide!.code.indexOf("const ___VERTER___props");
// The matching Vue-source offsets.
const SRC_COUNT_IN_TYPE = fx.source.indexOf("count");
const SRC_COUNT_IN_TEMPLATE = fx.source.indexOf("count", fx.source.indexOf("<template>"));
// Source markup with NO generated correlate (the <template> tag itself).
const SRC_UNMAPPED = fx.source.indexOf("<template>");

describe("TypeScriptService sync model (atomic syncSource per source)", () => {
  it("pushes a carrier's three surfaces and a plain .ts file's raw content", async () => {
    const { service, sent } = stubbedService();
    await service.syncWorkspace([compVueFile(), userTsFile()]);

    expect(sent).toEqual([
      {
        type: "syncSource",
        payload: {
          sourcePath: "/Comp.vue",
          surfaces: {
            ide: { code: fx.ide!.code, sourceMap: fx.ide!.sourceMap },
            decl: { code: fx.decl!.code, sourceMap: fx.decl!.sourceMap ?? null },
            api: { code: fx.api!.code, sourceMap: null },
          },
        },
      },
      {
        type: "syncSource",
        payload: { sourcePath: "/utils.ts", userContent: "export const one = 1;\n" },
      },
    ]);
    // The legacy per-file messages no longer exist on the wire.
    expect(sent.some((m) => m.type === "updateFile" || m.type === "updateVueTypes")).toBe(false);
  });

  it("does NOT re-send unchanged sources; re-sends on content change", async () => {
    const { service, sent } = stubbedService();
    await service.syncWorkspace([compVueFile(), userTsFile()]);
    const afterFirst = sent.length;

    await service.syncWorkspace([compVueFile(), userTsFile()]);
    expect(sent.length).toBe(afterFirst);

    await service.syncWorkspace([compVueFile(), userTsFile("export const two = 2;\n")]);
    expect(sent.length).toBe(afterFirst + 1);
    expect(sent[sent.length - 1]).toEqual({
      type: "syncSource",
      payload: { sourcePath: "/utils.ts", userContent: "export const two = 2;\n" },
    });
  });

  it("a source missing from a later syncWorkspace is retired via removeSource", async () => {
    const { service, sent } = stubbedService();
    await service.syncWorkspace([compVueFile(), userTsFile()]);
    await service.syncWorkspace([compVueFile()]);
    expect(sent[sent.length - 1]).toEqual({
      type: "removeSource",
      payload: { sourcePath: "/utils.ts" },
    });
  });

  it("removeSource retires the source AND its mapper (queries fail closed afterwards)", async () => {
    const { service, sent } = stubbedService(() => []);
    await service.syncWorkspace([compVueFile()]);
    await service.removeSource("Comp.vue");
    expect(sent.some((m) => m.type === "removeSource")).toBe(true);

    const before = sent.length;
    // No synced source, no mapper — the query never reaches the worker.
    expect(await service.getDefinition("Comp.vue", SRC_COUNT_IN_TYPE)).toEqual([]);
    expect(sent.length).toBe(before);
  });

  it("routes a Svelte source identically (descriptor-driven, no Vue literal): decl/api surfaces sync, an absent IDE surface is omitted", async () => {
    const sv = fixtures.compSvelte;
    const { service, sent } = stubbedService(() => []);
    await service.syncWorkspace([
      {
        filename: "Comp.svelte",
        code: sv.source,
        compiled: {
          types: "", // IDE surface unavailable in the fixture (ideUnavailable)
          typesSourceMap: "",
          declCode: sv.decl!.code,
          declSourceMap: sv.decl!.sourceMap ?? "",
          tscCode: sv.api!.code,
        },
      },
    ]);
    expect(sent).toEqual([
      {
        type: "syncSource",
        payload: {
          sourcePath: "/Comp.svelte",
          surfaces: {
            ide: undefined,
            decl: { code: sv.decl!.code, sourceMap: null },
            api: { code: sv.api!.code, sourceMap: null },
          },
        },
      },
    ]);
    // With no IDE surface there is no mapper: queries fail closed, worker untouched.
    const before = sent.length;
    expect(await service.getHover("Comp.svelte", 0)).toBeNull();
    expect(await service.getDiagnostics("Comp.svelte")).toEqual([]);
    expect(sent.length).toBe(before);
  });

  it("a non-syncable file (import-map.json) is skipped entirely", async () => {
    const { service, sent } = stubbedService();
    await service.syncWorkspace([
      { filename: "import-map.json", code: "{}", compiled: compVueFile().compiled },
    ]);
    expect(sent).toEqual([]);
  });
});

describe("TypeScriptService diagnostics — strict per-carrier mapping", () => {
  it("requests the IDE carrier path, maps carrier diagnostics into Vue space, DROPS synthetic + unknown-file spans", async () => {
    const { service, sent } = stubbedService((type) => {
      if (type !== "getDiagnostics") return "ok";
      return [
        // Mapped: sits on the defineProps type-argument token.
        {
          message: "mapped",
          start: GEN_COUNT_IN_TYPE,
          length: 5,
          category: 1,
          code: 1001,
          fileName: "/Comp.vue.tsx",
        },
        // Synthetic generated space: DROPS (a snap mapper would emit a mis-map).
        {
          message: "synthetic",
          start: GEN_SYNTHETIC,
          length: 5,
          category: 1,
          code: 1002,
          fileName: "/Comp.vue.tsx",
        },
        // Unknown file: no registered mapper — DROPS.
        {
          message: "foreign",
          start: 0,
          length: 1,
          category: 1,
          code: 1003,
          fileName: "/Other.vue.tsx",
        },
      ];
    });
    await service.syncWorkspace([compVueFile()]);
    const diagnostics = await service.getDiagnostics("Comp.vue");

    const request = sent.find((m) => m.type === "getDiagnostics");
    expect(request?.payload).toEqual({ path: "/Comp.vue.tsx" });

    expect(diagnostics).toEqual([
      {
        message: "mapped",
        start: SRC_COUNT_IN_TYPE,
        end: SRC_COUNT_IN_TYPE + 5,
        severity: "error",
        code: 1001,
      },
    ]);
    expect(diagnostics.some((d) => d.code === 1002)).toBe(false);
    expect(diagnostics.some((d) => d.code === 1003)).toBe(false);
  });

  it("a plain .ts user file's own diagnostics pass through unmapped; carrier-space spans still drop", async () => {
    const { service, sent } = stubbedService((type) => {
      if (type !== "getDiagnostics") return "ok";
      return [
        { message: "raw", start: 13, length: 3, category: 0, code: 6133, fileName: "/utils.ts" },
        // A span in ANOTHER file's carrier maps into that OTHER source — dropped here.
        {
          message: "cross",
          start: GEN_COUNT_IN_TYPE,
          length: 5,
          category: 1,
          code: 2322,
          fileName: "/Comp.vue.tsx",
        },
      ];
    });
    await service.syncWorkspace([compVueFile(), userTsFile()]);
    const diagnostics = await service.getDiagnostics("utils.ts");

    const request = sent.find((m) => m.type === "getDiagnostics");
    expect(request?.payload).toEqual({ path: "/utils.ts" });
    expect(diagnostics).toEqual([
      { message: "raw", start: 13, end: 16, severity: "warning", code: 6133 },
    ]);
  });

  it("a carrier WITHOUT a usable source map yields NO diagnostics (fail closed, never raw offsets)", async () => {
    const { service } = stubbedService((type) =>
      type === "getDiagnostics"
        ? [{ message: "x", start: 0, length: 1, category: 1, code: 1, fileName: "/Comp.vue.tsx" }]
        : "ok",
    );
    const mapless = compVueFile();
    mapless.compiled.typesSourceMap = "";
    await service.syncWorkspace([mapless]);
    expect(await service.getDiagnostics("Comp.vue")).toEqual([]);
  });
});

describe("TypeScriptService query direction — source→generated through the strict mapper", () => {
  it("translates a mapped source offset onto the IDE carrier path + generated offset", async () => {
    const { service, sent } = stubbedService((type) =>
      type === "getHover"
        ? { text: "(property) count: number", documentation: "", start: 0, length: 5 }
        : "ok",
    );
    await service.syncWorkspace([compVueFile()]);
    const hover = await service.getHover("Comp.vue", SRC_COUNT_IN_TYPE);
    expect(hover).toContain("(property) count: number");

    const request = sent.find((m) => m.type === "getHover");
    const payload = request?.payload as { path: string; offset: number };
    expect(payload.path).toBe("/Comp.vue.tsx");
    // The generated offset lands EXACTLY on the "count" token in the carrier.
    expect(fx.ide!.code.slice(payload.offset, payload.offset + 5)).toBe("count");
  });

  it("an UNMAPPED source position fails closed WITHOUT calling the worker (no snap)", async () => {
    const { service, sent } = stubbedService(() => null);
    await service.syncWorkspace([compVueFile()]);
    const before = sent.length;
    expect(await service.getHover("Comp.vue", SRC_UNMAPPED)).toBeNull();
    expect(await service.getCompletions("Comp.vue", SRC_UNMAPPED)).toEqual([]);
    expect(sent.length).toBe(before);
  });

  it("maps result spans back per-fileName: references across script AND template both map; synthetic drops", async () => {
    const { service } = stubbedService((type) =>
      type === "getReferences"
        ? [
            { fileName: "/Comp.vue.tsx", start: GEN_COUNT_IN_TYPE, length: 5, isDefinition: true },
            {
              fileName: "/Comp.vue.tsx",
              start: GEN_COUNT_IN_TEMPLATE,
              length: 5,
              isDefinition: false,
            },
            { fileName: "/Comp.vue.tsx", start: GEN_SYNTHETIC, length: 5, isDefinition: false },
            { fileName: "/Other.vue.tsx", start: 0, length: 5, isDefinition: false },
          ]
        : "ok",
    );
    await service.syncWorkspace([compVueFile()]);
    const refs = await service.getReferences("Comp.vue", SRC_COUNT_IN_TYPE);
    // Script + template occurrences of the SAME binding, in Vue-source space.
    expect(refs).toEqual([
      { start: SRC_COUNT_IN_TYPE, end: SRC_COUNT_IN_TYPE + 5, isDefinition: true },
      { start: SRC_COUNT_IN_TEMPLATE, end: SRC_COUNT_IN_TEMPLATE + 5, isDefinition: false },
    ]);
  });

  it("rename: trigger span + locations map back strictly; synthetic locations drop", async () => {
    const { service } = stubbedService((type) =>
      type === "getRenameLocations"
        ? {
            canRename: true,
            localizedErrorMessage: null,
            triggerSpan: { start: GEN_COUNT_IN_TYPE, length: 5 },
            locations: [
              { fileName: "/Comp.vue.tsx", start: GEN_COUNT_IN_TYPE, length: 5 },
              { fileName: "/Comp.vue.tsx", start: GEN_COUNT_IN_TEMPLATE, length: 5 },
              { fileName: "/Comp.vue.tsx", start: GEN_SYNTHETIC, length: 5 },
            ],
          }
        : "ok",
    );
    await service.syncWorkspace([compVueFile()]);
    const rename = await service.getRenameLocations("Comp.vue", SRC_COUNT_IN_TYPE);
    expect(rename.canRename).toBe(true);
    expect(rename.triggerSpan).toEqual({ start: SRC_COUNT_IN_TYPE, end: SRC_COUNT_IN_TYPE + 5 });
    expect(rename.locations).toEqual([
      { start: SRC_COUNT_IN_TYPE, end: SRC_COUNT_IN_TYPE + 5 },
      { start: SRC_COUNT_IN_TEMPLATE, end: SRC_COUNT_IN_TEMPLATE + 5 },
    ]);
  });
});

describe("TypeScriptService checkStandalone — the ONLY raw path (editable output panel)", () => {
  it("sends checkStandalone and returns raw, unmapped TSX-space diagnostics", async () => {
    const { service, sent } = stubbedService((type) =>
      type === "checkStandalone"
        ? [{ message: "raw", start: 7, length: 2, category: 1, code: 2304, fileName: "/x.tsx" }]
        : "ok",
    );
    const diagnostics = await service.checkStandalone("const x: y = 1;");
    const request = sent.find((m) => m.type === "checkStandalone");
    expect(request).toBeDefined();
    expect((request?.payload as { content: string }).content).toBe("const x: y = 1;");
    expect(diagnostics).toEqual([
      { message: "raw", start: 7, end: 9, severity: "error", code: 2304 },
    ]);
  });
});
