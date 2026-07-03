import { describe, it, expect, beforeEach } from "vitest";
import type { CarrierStoreReader, Manifest, OwnedSource, ReadyFile } from "./store";
import {
  clearCarrierMapCache,
  remapCarrierSpan,
  remapDocumentSpan,
  rewriteInsertedSpecifier,
  type CarrierRemapContext,
} from "./remap";
import { normalizePath } from "./naming";

/**
 * CORE remap tests over an IN-MEMORY `CarrierStoreReader` implementation.
 * This proves two things the shared extraction is FOR:
 *
 * 1. `remap` compiles and runs against the CarrierStoreReader INTERFACE, not
 *    the Node disk adapter — a browser/WASM host can implement the interface
 *    with no filesystem at all (this file imports NO Node builtin).
 * 2. The remap orchestration + the strict mapper behave identically through a
 *    non-disk reader: mappable spans map, unmappable spans fail closed.
 */
class InMemoryCarrierStoreReader implements CarrierStoreReader {
  constructor(
    private readonly manifest: Manifest,
    private readonly blobs: Record<string, string>,
    private readonly maps: Record<string, unknown>,
  ) {}

  isAvailable(): boolean {
    return true;
  }

  readManifest(): Manifest | undefined {
    return this.manifest;
  }

  currentEpoch(): number | undefined {
    return this.manifest.epoch;
  }

  private projects() {
    return Object.values(this.manifest.projects);
  }

  ownedSources(projectUri?: string): OwnedSource[] {
    if (projectUri !== undefined) {
      return this.manifest.projects[projectUri]?.owned_sources ?? [];
    }
    return this.projects().flatMap((p) => p.owned_sources);
  }

  readyFile(providerPath: string): ReadyFile | undefined {
    const key = normalizePath(providerPath);
    for (const project of this.projects()) {
      const entry = project.ready_files[key];
      if (entry) return entry;
    }
    return undefined;
  }

  readyIdeCompanions(): string[] {
    const out: string[] = [];
    for (const project of this.projects()) {
      for (const [providerUri, ready] of Object.entries(project.ready_files)) {
        if (ready.role === "CarrierIde") out.push(providerUri);
      }
    }
    return out;
  }

  readyFileForSource(sourcePath: string): ReadyFile | undefined {
    const companion = this.companionForSource(sourcePath);
    return companion === undefined ? undefined : this.readyFile(companion);
  }

  companionForSource(sourcePath: string): string | undefined {
    const key = normalizePath(sourcePath);
    for (const owned of this.ownedSources()) {
      if (normalizePath(owned.source_uri) === key) return normalizePath(owned.provider_uri);
    }
    return undefined;
  }

  ownedSourceFor(providerOrSourcePath: string): OwnedSource | undefined {
    const key = normalizePath(providerOrSourcePath);
    for (const owned of this.ownedSources()) {
      if (normalizePath(owned.provider_uri) === key || normalizePath(owned.source_uri) === key) {
        return owned;
      }
    }
    return undefined;
  }

  readBlobSync(blobRel: string): string | undefined {
    return this.blobs[blobRel];
  }

  readMapSync(mapRel: string): unknown | undefined {
    return this.maps[mapRel];
  }

  lastGoodBlobFor(): string | undefined {
    return undefined;
  }
}

/**
 * One Vue carrier `A.vue` → `A.vue.tsx`. The MAPPABLE map covers generated
 * line 1 from source line 1; the UNMAPPABLE map (`U.vue`) starts its mappings
 * on generated line 2, so a line-1 query is a generated-only region.
 */
const MAPPABLE_V3 = { version: 3, sources: ["A.vue"], names: [], mappings: "AAAA" };
const UNMAPPABLE_AT_LINE1_V3 = { version: 3, sources: ["U.vue"], names: [], mappings: ";AAAA" };

/**
 * `S.vue.tsx` maps ONLY generated line 1 (`AAAA`); its line 2 is synthetic
 * helper code with no mapping. A span STARTING in the mapped region but ENDING
 * inside the synthetic line has an unmappable END offset.
 */
const MAPPED_LINE1_ONLY_V3 = { version: 3, sources: ["S.vue"], names: [], mappings: "AAAA" };

/**
 * `X.vue.tsx` carries two segments on generated line 1 pointing at DIFFERENT
 * sources: gen col 0 → `X.vue` (col 0), gen col 8 → `Y.vue` (col 0)
 * (`AAAA,QCAA`; seg2 = VLQ[+8,+1,0,0]). A span crossing gen col 8 has its
 * START and END in different sources.
 */
const CROSS_SOURCE_V3 = {
  version: 3,
  sources: ["X.vue", "Y.vue"],
  names: [],
  mappings: "AAAA,QCAA",
};

/**
 * `M.vue.tsx` carries two segments on generated line 1 within ONE source:
 * gen col 0 → src col 2, gen col 9 → src col 8 (`AAAE,SAAM`). A span
 * [0, 9) maps to source [2, 8) — the faithful multi-character length 6.
 */
const TWO_SEGMENT_V3 = { version: 3, sources: ["M.vue"], names: [], mappings: "AAAE,SAAM" };

function manifest(): Manifest {
  return {
    epoch: 1,
    host_version: "test",
    projects: {
      "d:/ws/tsconfig.json": {
        owned_sources: [
          {
            source_uri: "d:/ws/src/A.vue",
            provider_uri: "d:/ws/src/A.vue.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
          {
            source_uri: "d:/ws/src/U.vue",
            provider_uri: "d:/ws/src/U.vue.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
          {
            source_uri: "d:/ws/src/S.vue",
            provider_uri: "d:/ws/src/S.vue.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
          {
            source_uri: "d:/ws/src/X.vue",
            provider_uri: "d:/ws/src/X.vue.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
          {
            source_uri: "d:/ws/src/M.vue",
            provider_uri: "d:/ws/src/M.vue.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
        ],
        ready_files: {
          "d:/ws/src/A.vue.tsx": {
            content_hash: "a1",
            version: 1,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "core-mem-ma",
            blob_rel: "blobs/A.vue.tsx",
            map_rel: "maps/A.vue.json",
          },
          "d:/ws/src/U.vue.tsx": {
            content_hash: "u1",
            version: 1,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "core-mem-mu",
            blob_rel: "blobs/U.vue.tsx",
            map_rel: "maps/U.vue.json",
          },
          "d:/ws/src/S.vue.tsx": {
            content_hash: "s1",
            version: 1,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "core-mem-ms",
            blob_rel: "blobs/S.vue.tsx",
            map_rel: "maps/S.vue.json",
          },
          "d:/ws/src/X.vue.tsx": {
            content_hash: "x1",
            version: 1,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "core-mem-mx",
            blob_rel: "blobs/X.vue.tsx",
            map_rel: "maps/X.vue.json",
          },
          "d:/ws/src/M.vue.tsx": {
            content_hash: "m1",
            version: 1,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "core-mem-mm",
            blob_rel: "blobs/M.vue.tsx",
            map_rel: "maps/M.vue.json",
          },
        },
      },
    },
  };
}

function inMemoryCtx(): CarrierRemapContext {
  const reader: CarrierStoreReader = new InMemoryCarrierStoreReader(
    manifest(),
    {
      "blobs/A.vue.tsx": "const foo = 1;\n",
      "blobs/U.vue.tsx": "/* gen helper */\nconst real = 1;\n",
      "blobs/S.vue.tsx": "const foo = 1;\nsynthetic();\n",
      "blobs/X.vue.tsx": "abcdefghijklmn\n",
      "blobs/M.vue.tsx": "fooBarBaz = q;\n",
    },
    {
      "maps/A.vue.json": MAPPABLE_V3,
      "maps/U.vue.json": UNMAPPABLE_AT_LINE1_V3,
      "maps/S.vue.json": MAPPED_LINE1_ONLY_V3,
      "maps/X.vue.json": CROSS_SOURCE_V3,
      "maps/M.vue.json": TWO_SEGMENT_V3,
    },
  );
  const companions: Record<string, string> = {
    "d:/ws/src/A.vue.tsx": "const foo = 1;\n",
    "d:/ws/src/U.vue.tsx": "/* gen helper */\nconst real = 1;\n",
    "d:/ws/src/S.vue.tsx": "const foo = 1;\nsynthetic();\n",
    "d:/ws/src/X.vue.tsx": "abcdefghijklmn\n",
    "d:/ws/src/M.vue.tsx": "fooBarBaz = q;\n",
  };
  const sources: Record<string, string> = {
    "A.vue": "const foo = 1;\n",
    "U.vue": "const real = 1;\n",
    "S.vue": "const foo = 1;\n",
    "X.vue": "abcdefghijkl\n",
    "Y.vue": "yz12345\n",
    "M.vue": "  fooBarBaz = q;\n",
  };
  return {
    reader,
    readCompanion: (p) => companions[p],
    readSource: (s) => sources[s],
  };
}

beforeEach(() => {
  clearCarrierMapCache();
});

describe("remap over the CarrierStoreReader INTERFACE (in-memory, no Node fs)", () => {
  it("remapCarrierSpan maps a companion span to source through a non-disk reader", () => {
    const ctx = inMemoryCtx();
    const remapped = remapCarrierSpan(
      ctx.reader,
      "d:/ws/src/A.vue.tsx",
      { start: 6, length: 3 },
      ctx.readCompanion,
      ctx.readSource,
    );
    expect(remapped).not.toBeNull();
    expect(remapped!.fileName).toBe("A.vue");
    expect(remapped!.textSpan).toEqual({ start: 6, length: 3 });
  });

  it("remapCarrierSpan fails CLOSED for a generated-only region through the interface", () => {
    const ctx = inMemoryCtx();
    const remapped = remapCarrierSpan(
      ctx.reader,
      "d:/ws/src/U.vue.tsx",
      { start: 0, length: 3 },
      ctx.readCompanion,
      ctx.readSource,
    );
    expect(remapped).toBeNull();
  });

  it("remapDocumentSpan drops an unmappable token-level companion entry", () => {
    const ctx = inMemoryCtx();
    const def = {
      fileName: "d:/ws/src/U.vue.tsx",
      textSpan: { start: 0, length: 3 },
      kind: "const",
    };
    expect(remapDocumentSpan(ctx, def)).toBeUndefined();
  });

  it("fails CLOSED when the span START maps but the END lands in a SYNTHETIC (unmapped) region", () => {
    const ctx = inMemoryCtx();
    // Start (offset 6) is inside the mapped companion line 1; the end
    // (offset 18) is inside the synthetic `synthetic();` line 2, which has no
    // mapping. Returning the carrier length here would select 12 source chars
    // from offset 6 — past the END of the 15-char source file: a corrupt
    // rename/highlight span. The whole span must DROP.
    const remapped = remapCarrierSpan(
      ctx.reader,
      "d:/ws/src/S.vue.tsx",
      { start: 6, length: 12 },
      ctx.readCompanion,
      ctx.readSource,
    );
    expect(remapped).toBeNull();
  });

  it("fails CLOSED when the span END maps into a DIFFERENT source than the START", () => {
    const ctx = inMemoryCtx();
    // Start (offset 2) maps into `X.vue`; the end (offset 10) crosses the
    // gen-col-8 segment boundary into `Y.vue`. A mixed-source span has no
    // faithful single-source length — it must DROP, never fall back to the
    // carrier length.
    const remapped = remapCarrierSpan(
      ctx.reader,
      "d:/ws/src/X.vue.tsx",
      { start: 2, length: 8 },
      ctx.readCompanion,
      ctx.readSource,
    );
    expect(remapped).toBeNull();
  });

  it("maps a multi-code-unit identifier to its FAITHFUL source length via strict end mapping", () => {
    const ctx = inMemoryCtx();
    // `fooBarBaz` spans companion [0, 9); the map's end segment puts the
    // source span at [2, 8) — the faithful length 6, NOT a collapsed 1 and
    // NOT the carrier length 9.
    const remapped = remapCarrierSpan(
      ctx.reader,
      "d:/ws/src/M.vue.tsx",
      { start: 0, length: 9 },
      ctx.readCompanion,
      ctx.readSource,
    );
    expect(remapped).not.toBeNull();
    expect(remapped!.fileName).toBe("M.vue");
    expect(remapped!.textSpan).toEqual({ start: 2, length: 6 });
  });

  it("a zero-length caret span maps to a zero-length source span", () => {
    const ctx = inMemoryCtx();
    const remapped = remapCarrierSpan(
      ctx.reader,
      "d:/ws/src/A.vue.tsx",
      { start: 6, length: 0 },
      ctx.readCompanion,
      ctx.readSource,
    );
    expect(remapped).not.toBeNull();
    expect(remapped!.textSpan).toEqual({ start: 6, length: 0 });
  });

  it("rewriteInsertedSpecifier strips the unambiguous Vue companion suffix by shape", () => {
    const ctx = inMemoryCtx();
    const out = rewriteInsertedSpecifier(
      ctx,
      'import Comp from "./Comp.vue.tsx";',
      "d:/ws/src/Consumer.ts",
    );
    expect(out).toBe('import Comp from "./Comp.vue";');
  });
});
