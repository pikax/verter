import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { DiskCarrierStoreReader } from "./carrierStore";
import {
  type Manifest,
  clearCarrierMapCache,
  isCarrierCompanionPath,
  isModuleLevelDefinition,
  remapDocumentSpan,
  remapDocumentSpans,
  remapModuleLevelCompanionToSource,
  remapReferencedSymbol,
  remapFileTextChanges,
  remapAllFileTextChanges,
  rewriteInsertedSpecifier,
  sourceForCarrierCompanion,
  type CarrierRemapContext,
} from "@verter/language-shared";

/**
 * Discriminating tests for the companion→source RESPONSE mappers. Each asserts
 * BOTH directions: a mappable companion entry is rewritten to the `.vue`/
 * `.svelte` SOURCE path with a remapped span, AND an unmappable companion entry
 * fails CLOSED (dropped / `undefined`, never a companion path or a mis-mapped
 * source span). A mis-map (returning the companion path, or pairing a source
 * path with a generated offset, or keeping an unmappable entry) FAILS these.
 */

/**
 * A V3 map whose only mapping is on generated LINE 1: gen (1,0) → src (A.vue
 * 1,0). `findOrigin(1, *)` resolves; nothing before it. The MAPPABLE fixture.
 */
const MAPPABLE_V3 = {
  version: 3,
  sources: ["A.vue"],
  names: [],
  mappings: "AAAA",
};

/**
 * A V3 map whose only mapping is on generated LINE 2 (`;AAAA` — a leading `;`
 * skips line 1). A query on LINE 1 (companion offset 0) has NO origin →
 * `findOrigin` returns `{}` → `remapCarrierSpan` is `null` → the entry is a
 * GENERATED-ONLY region that must fail closed. The UNMAPPABLE fixture.
 */
const UNMAPPABLE_AT_LINE1_V3 = {
  version: 3,
  sources: ["U.vue"],
  names: [],
  mappings: ";AAAA",
};

/**
 * The Svelte-shaped pair (a `.svelte.tsx` companion → `.svelte` source):
 * gen (1,0) → src (W.svelte 2,0) — `AACA` = VLQ[0,0,1,0] — the companion's
 * script statement maps onto the source's `const bar = 1;` line (line 2, after
 * the `<script>` opener), so a token span maps end-to-end within one source
 * line under strict BOTH-endpoint span mapping.
 */
const SVELTE_MAPPABLE_V3 = {
  version: 3,
  sources: ["W.svelte"],
  names: [],
  mappings: "AACA",
};

function writeStore(manifest: Manifest, files: Record<string, string>): string {
  const dir = mkdtempSync(join(tmpdir(), "verter-remap-resp-store-"));
  mkdirSync(join(dir, "blobs"), { recursive: true });
  mkdirSync(join(dir, "maps"), { recursive: true });
  for (const [rel, content] of Object.entries(files)) {
    const abs = join(dir, rel);
    mkdirSync(join(abs, ".."), { recursive: true });
    writeFileSync(abs, content, "utf8");
  }
  writeFileSync(join(dir, "manifest.json"), JSON.stringify(manifest), "utf8");
  return dir;
}

/**
 * A manifest owning:
 *  - `A.vue` → `A.vue.tsx`  (ready, MAPPABLE map)        — companion-maps-to-source
 *  - `U.vue` → `U.vue.tsx`  (ready, UNMAPPABLE-at-line1) — fail-closed
 *  - `W.svelte` → `W.svelte.tsx` (ready, MAPPABLE map)   — Svelte parity
 */
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
            source_uri: "d:/ws/src/W.svelte",
            provider_uri: "d:/ws/src/W.svelte.tsx",
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
            map_hash: "ma",
            blob_rel: "blobs/A.vue.tsx",
            map_rel: "maps/A.vue.json",
          },
          "d:/ws/src/U.vue.tsx": {
            content_hash: "u1",
            version: 1,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "mu",
            blob_rel: "blobs/U.vue.tsx",
            map_rel: "maps/U.vue.json",
          },
          "d:/ws/src/W.svelte.tsx": {
            content_hash: "w1",
            version: 1,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "mw",
            blob_rel: "blobs/W.svelte.tsx",
            map_rel: "maps/W.svelte.json",
          },
        },
      },
    },
  };
}

/** The companion/source blob set the maps above reference. */
function blobs(): Record<string, string> {
  return {
    // Companion line 1 holds the identifier the MAPPABLE map points at.
    "blobs/A.vue.tsx": "const foo = 1;\n",
    "maps/A.vue.json": JSON.stringify(MAPPABLE_V3),
    // Companion: line 1 is a generated-only helper (no mapping), line 2 maps.
    "blobs/U.vue.tsx": "/* gen helper */\nconst real = 1;\n",
    "maps/U.vue.json": JSON.stringify(UNMAPPABLE_AT_LINE1_V3),
    "blobs/W.svelte.tsx": "const bar = 1;\n",
    "maps/W.svelte.json": JSON.stringify(SVELTE_MAPPABLE_V3),
  };
}

let dirs: string[] = [];
beforeEach(() => {
  dirs = [];
  clearCarrierMapCache();
});
afterEach(() => {
  for (const d of dirs) rmSync(d, { recursive: true, force: true });
});
function track(d: string): string {
  dirs.push(d);
  return d;
}

/**
 * Build a context whose companion/source reads come from a fixed table — the
 * `.tsx` companion text is the same the map indexes, the `.vue`/`.svelte`
 * source is a longer text so the source-side line/column→offset succeeds.
 */
function ctxFor(dir: string, extraSources: Record<string, string> = {}): CarrierRemapContext {
  const reader = new DiskCarrierStoreReader(dir);
  const companions: Record<string, string> = {
    "d:/ws/src/A.vue.tsx": "const foo = 1;\n",
    "d:/ws/src/U.vue.tsx": "/* gen helper */\nconst real = 1;\n",
    "d:/ws/src/W.svelte.tsx": "const bar = 1;\n",
  };
  const sources: Record<string, string> = {
    // The map's `sources` are bare names; `remapCarrierSpan` normalizes the
    // origin filename and reads source text by that key.
    "A.vue": "<template/>\n<script setup>\nconst foo = 1;\n</script>\n",
    "U.vue": "<template/>\n<script setup>\nconst real = 1;\n</script>\n",
    "W.svelte": "<script>\nconst bar = 1;\n</script>\n",
    ...extraSources,
  };
  // The host existence predicate. `containingFileAwareExists` resolves a
  // RELATIVE backing candidate (`./Widget.svelte`) against the containing dir
  // with the POSIX resolver, which — like the real tsserver host — produces a
  // canonicalized path; a real host answers by canonical identity, so match on
  // the trailing carrier path. `Comp.vue` / `Widget.svelte` are real carrier
  // sources (their ambiguous companion suffix MAY strip); `store.svelte.ts` is
  // a real Svelte rune module that has NO `store.svelte` carrier, so its
  // ambiguous `.svelte.ts` specifier must be left intact.
  const realFiles = ["/Comp.vue", "/Widget.svelte", "/store.svelte.ts"];
  return {
    reader,
    readCompanion: (p) => companions[p],
    readSource: (s) => sources[s],
    fileExists: (candidate) => {
      const c = candidate.replace(/\\/g, "/");
      return realFiles.some((f) => c.endsWith(f));
    },
  };
}

describe("isCarrierCompanionPath", () => {
  it("classifies a ready companion provider path as a companion (vue + svelte)", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    expect(isCarrierCompanionPath(ctx.reader, "d:/ws/src/A.vue.tsx")).toBe(true);
    expect(isCarrierCompanionPath(ctx.reader, "d:/ws/src/W.svelte.tsx")).toBe(true);
  });

  it("does NOT classify a bare SOURCE path or a plain .ts as a companion", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    // A `.vue` SOURCE path is the map-TO target, not a companion.
    expect(isCarrierCompanionPath(ctx.reader, "d:/ws/src/A.vue")).toBe(false);
    expect(isCarrierCompanionPath(ctx.reader, "d:/ws/src/W.svelte")).toBe(false);
    // A real user `.ts` is never a companion.
    expect(isCarrierCompanionPath(ctx.reader, "d:/ws/src/Consumer.ts")).toBe(false);
  });
});

describe("remapDocumentSpan (definition / reference / rename location)", () => {
  it("rewrites a mappable companion DefinitionInfo to the .vue SOURCE + remapped span", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const def = {
      fileName: "d:/ws/src/A.vue.tsx",
      textSpan: { start: 6, length: 3 }, // "foo" in the companion line 1
      kind: "const",
      name: "foo",
      containerKind: "",
      containerName: "",
    };
    const mapped = remapDocumentSpan(ctx, def);
    expect(mapped).toBeDefined();
    // Mapped to SOURCE, never the companion.
    expect(mapped!.fileName).toBe("A.vue");
    expect(mapped!.fileName).not.toContain(".vue.tsx");
    // Source span is real (start derived from the map, length carried).
    expect(mapped!.textSpan.length).toBe(3);
  });

  it("rewrites a mappable companion DefinitionInfo for SVELTE to the .svelte SOURCE", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const def = {
      fileName: "d:/ws/src/W.svelte.tsx",
      textSpan: { start: 6, length: 3 },
      kind: "const",
      name: "bar",
      containerKind: "",
      containerName: "",
    };
    const mapped = remapDocumentSpan(ctx, def);
    expect(mapped).toBeDefined();
    expect(mapped!.fileName).toBe("W.svelte");
    expect(mapped!.fileName).not.toContain(".svelte.tsx");
    // The span lands EXACTLY on `bar` inside the source's script line.
    expect(mapped!.textSpan).toEqual({ start: 15, length: 3 });
  });

  it("FAILS CLOSED (undefined) for an unmappable companion span (generated-only region)", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const def = {
      fileName: "d:/ws/src/U.vue.tsx",
      textSpan: { start: 0, length: 3 }, // companion line 1 — no mapping
      kind: "const",
      name: "x",
      containerKind: "",
      containerName: "",
    };
    const mapped = remapDocumentSpan(ctx, def);
    // Dropped: NEVER the companion path, NEVER a source path with a generated span.
    expect(mapped).toBeUndefined();
  });

  it("passes a NON-companion (real .ts) span through UNCHANGED", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const def = {
      fileName: "d:/ws/src/Consumer.ts",
      textSpan: { start: 10, length: 4 },
      kind: "const",
      name: "comp",
      containerKind: "",
      containerName: "",
    };
    const mapped = remapDocumentSpan(ctx, def);
    expect(mapped).toBe(def);
    expect(mapped!.fileName).toBe("d:/ws/src/Consumer.ts");
    expect(mapped!.textSpan).toEqual({ start: 10, length: 4 });
  });

  it("drops the originalFileName/originalTextSpan redirect metadata on a companion remap", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const def: {
      fileName: string;
      textSpan: { start: number; length: number };
      originalFileName?: string;
      originalTextSpan?: { start: number; length: number };
    } = {
      fileName: "d:/ws/src/A.vue.tsx",
      textSpan: { start: 6, length: 3 },
      originalFileName: "d:/ws/src/A.vue.tsx",
      originalTextSpan: { start: 6, length: 3 },
    };
    const mapped = remapDocumentSpan(ctx, def);
    expect(mapped).toBeDefined();
    expect(mapped!.originalFileName).toBeUndefined();
    expect(mapped!.originalTextSpan).toBeUndefined();
  });
});

describe("remapDocumentSpans (references / rename arrays)", () => {
  it("keeps real-.ts + mappable-companion entries, DROPS the unmappable companion entry", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const entries = [
      // real .ts — passthrough
      {
        fileName: "d:/ws/src/Consumer.ts",
        textSpan: { start: 0, length: 4 },
        isWriteAccess: false,
      },
      // mappable companion → source
      {
        fileName: "d:/ws/src/A.vue.tsx",
        textSpan: { start: 6, length: 3 },
        isWriteAccess: false,
      },
      // unmappable companion → DROP
      {
        fileName: "d:/ws/src/U.vue.tsx",
        textSpan: { start: 0, length: 3 },
        isWriteAccess: false,
      },
    ];
    const out = remapDocumentSpans(ctx, entries);
    expect(out).toHaveLength(2);
    expect(out.map((e) => e.fileName).sort()).toEqual(["A.vue", "d:/ws/src/Consumer.ts"]);
    // The unmappable companion is gone — never surfaced as a companion or mis-mapped.
    expect(out.some((e) => e.fileName.includes(".vue.tsx"))).toBe(false);
    expect(out.some((e) => e.fileName === "U.vue")).toBe(false);
  });
});

describe("remapReferencedSymbol (findReferences grouping)", () => {
  it("remaps the definition + references, dropping unmappable companion refs", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const symbol = {
      definition: {
        fileName: "d:/ws/src/A.vue.tsx",
        textSpan: { start: 6, length: 3 },
        displayParts: [],
        kind: "const",
        name: "foo",
        containerKind: "",
        containerName: "",
      },
      references: [
        {
          fileName: "d:/ws/src/Consumer.ts",
          textSpan: { start: 0, length: 3 },
          isWriteAccess: false,
        },
        {
          fileName: "d:/ws/src/A.vue.tsx",
          textSpan: { start: 6, length: 3 },
          isWriteAccess: false,
        },
        {
          fileName: "d:/ws/src/U.vue.tsx",
          textSpan: { start: 0, length: 3 },
          isWriteAccess: false,
        },
      ],
    };
    const out = remapReferencedSymbol(ctx, symbol);
    expect(out).toBeDefined();
    expect(out!.definition.fileName).toBe("A.vue");
    expect(out!.references).toHaveLength(2);
    expect(out!.references.some((r) => r.fileName.includes(".vue.tsx"))).toBe(false);
  });

  it("DROPS the whole symbol when its definition is an unmappable NON-module companion (kind const)", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const symbol = {
      definition: {
        fileName: "d:/ws/src/U.vue.tsx",
        textSpan: { start: 0, length: 3 },
        displayParts: [],
        kind: "const",
        name: "x",
        containerKind: "",
        containerName: "",
      },
      references: [
        {
          fileName: "d:/ws/src/Consumer.ts",
          textSpan: { start: 0, length: 3 },
          isWriteAccess: false,
        },
      ],
    };
    expect(remapReferencedSymbol(ctx, symbol)).toBeUndefined();
  });

  it("MODULE-LEVEL alias def whose span is UNMAPPABLE → DEFINITION maps to .vue source (not dropped)", () => {
    // The find-all-references analog of the import-specifier bug: the component's
    // resolved declaration is the carrier's synthesized default-export re-export
    // (`kind: "alias"`) whose generated token has NO faithful source mapping. The
    // DEFINITION must map to the .vue SOURCE (the user navigates to the component
    // file), never be dropped.
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const symbol = {
      definition: {
        fileName: "d:/ws/src/U.vue.tsx",
        textSpan: { start: 0, length: 7 }, // the unmappable generated re-export token
        displayParts: [],
        kind: "alias",
        name: "Comp",
        containerKind: "",
        containerName: "",
      },
      references: [
        // A real `.ts` usage — passes through.
        {
          fileName: "d:/ws/src/Consumer.ts",
          textSpan: { start: 0, length: 4 },
          isWriteAccess: false,
        },
        // The same-companion unmappable reference (the re-export token again).
        {
          fileName: "d:/ws/src/U.vue.tsx",
          textSpan: { start: 0, length: 7 },
          isWriteAccess: false,
        },
      ],
    };
    const out = remapReferencedSymbol(ctx, symbol);
    expect(out).toBeDefined();
    // The definition REACHES the .vue source @ file start.
    expect(out!.definition.fileName).toBe("d:/ws/src/U.vue");
    expect(out!.definition.fileName).not.toContain(".vue.tsx");
    expect(out!.definition.textSpan).toEqual({ start: 0, length: 0 });
  });

  it("MODULE-LEVEL alias symbol KEEPS an unmappable same-companion reference (component stays REACHABLE)", () => {
    // The component reference (the carrier's own re-export token) has no faithful
    // per-token source mapping. Because the symbol's definition resolved to the
    // carrier SOURCE (it IS the carrier's module identity), the unmappable
    // same-companion reference is KEPT on the companion path rather than dropped —
    // so find-all-references still REACHES the component (the upstream LSP maps a
    // carrier-companion reference back to source; a bare-source@0 reference is
    // rejected by tsserver's own reference post-processing).
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const symbol = {
      definition: {
        fileName: "d:/ws/src/U.vue.tsx",
        textSpan: { start: 0, length: 7 },
        displayParts: [],
        kind: "alias",
        name: "Comp",
        containerKind: "",
        containerName: "",
      },
      references: [
        {
          fileName: "d:/ws/src/Consumer.ts",
          textSpan: { start: 0, length: 4 },
          isWriteAccess: false,
        },
        {
          fileName: "d:/ws/src/U.vue.tsx",
          textSpan: { start: 0, length: 7 },
          isWriteAccess: false,
        },
      ],
    };
    const out = remapReferencedSymbol(ctx, symbol);
    expect(out).toBeDefined();
    const refPaths = out!.references.map((r) => r.fileName);
    // The real `.ts` ref passes through; the component stays reachable via the
    // kept companion reference.
    expect(refPaths).toContain("d:/ws/src/Consumer.ts");
    expect(refPaths.some((p) => p.includes("U.vue.tsx"))).toBe(true);
    expect(out!.references).toHaveLength(2);
  });

  it("the companion-keep is scoped to the moved def's OWN companion (a different companion still fail-closes)", () => {
    // The companion-keep only applies to a reference in the SAME companion the
    // definition moved from. A reference in a DIFFERENT, unmappable companion is
    // still dropped (fail closed). Here the def maps to A.vue source, but the
    // reference is an unmappable U.vue.tsx (a different carrier) → dropped.
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const symbol = {
      definition: {
        fileName: "d:/ws/src/A.vue.tsx",
        textSpan: { start: 6, length: 3 },
        displayParts: [],
        kind: "const",
        name: "foo",
        containerKind: "",
        containerName: "",
      },
      references: [
        {
          fileName: "d:/ws/src/Consumer.ts",
          textSpan: { start: 0, length: 3 },
          isWriteAccess: false,
        },
        // Unmappable DIFFERENT companion (not the moved def's companion) → dropped.
        {
          fileName: "d:/ws/src/U.vue.tsx",
          textSpan: { start: 0, length: 3 },
          isWriteAccess: false,
        },
      ],
    };
    const out = remapReferencedSymbol(ctx, symbol);
    expect(out).toBeDefined();
    // Def maps to A.vue source (mappable). The unmappable U.vue.tsx ref is dropped.
    expect(out!.definition.fileName).toBe("A.vue");
    const refPaths = out!.references.map((r) => r.fileName);
    expect(refPaths.some((p) => p.includes(".vue.tsx"))).toBe(false);
    expect(refPaths).toContain("d:/ws/src/Consumer.ts");
  });
});

describe("remapFileTextChanges (code-action / refactor / rename edits)", () => {
  it("rewrites a companion file edit to the SOURCE path when EVERY change maps", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const change = {
      fileName: "d:/ws/src/A.vue.tsx",
      textChanges: [
        // mappable (companion line 1) → source span
        { span: { start: 6, length: 3 }, newText: "renamed" },
        // a second mappable change asserts ordering is preserved.
        { span: { start: 0, length: 5 }, newText: "const " },
      ],
    };
    const out = remapFileTextChanges(ctx, change);
    expect(out).toBeDefined();
    expect(out!.fileName).toBe("A.vue");
    expect(out!.fileName).not.toContain(".vue.tsx");
    expect(out!.textChanges.length).toBe(2);
  });

  it("DROPS the WHOLE file edit when ANY change is unmappable (no partial source edit)", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const change = {
      fileName: "d:/ws/src/U.vue.tsx",
      textChanges: [
        // Mappable: companion line 2 (`const real = 1;`) maps to the source.
        { span: { start: 17, length: 4 }, newText: "renamed" },
        // Unmappable: companion line 1 is a generated-only helper region.
        { span: { start: 0, length: 3 }, newText: "gen" },
      ],
    };
    // A half-applied rename/refactor is a correctness hazard: one unmappable
    // change poisons the file's ENTIRE change set (fail closed at file
    // granularity), never a partial edit.
    expect(remapFileTextChanges(ctx, change)).toBeUndefined();
  });

  it("DROPS an unmappable change and the whole edit when ALL changes are unmappable", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    // Every change targets the U companion's line-1 generated-only region.
    const allUnmappable = {
      fileName: "d:/ws/src/U.vue.tsx",
      textChanges: [
        { span: { start: 0, length: 3 }, newText: "a" },
        { span: { start: 4, length: 3 }, newText: "b" },
      ],
    };
    expect(remapFileTextChanges(ctx, allUnmappable)).toBeUndefined();
  });

  it("keeps a real-.ts file edit, rewriting a companion import specifier in newText", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const change = {
      fileName: "d:/ws/src/Consumer.ts",
      textChanges: [
        { span: { start: 0, length: 0 }, newText: 'import Comp from "./Comp.vue.tsx";\n' },
      ],
    };
    const out = remapFileTextChanges(ctx, change);
    expect(out).toBeDefined();
    // Real file path is preserved; the specifier is rewritten to the bare .vue.
    expect(out!.fileName).toBe("d:/ws/src/Consumer.ts");
    expect(out!.textChanges[0].newText).toBe('import Comp from "./Comp.vue";\n');
    expect(out!.textChanges[0].newText).not.toContain(".vue.tsx");
  });

  it("remapAllFileTextChanges drops the wholly-unmappable companion edit, keeps the rest", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const changes = [
      {
        fileName: "d:/ws/src/A.vue.tsx",
        textChanges: [{ span: { start: 6, length: 3 }, newText: "x" }],
      },
      {
        fileName: "d:/ws/src/U.vue.tsx",
        textChanges: [{ span: { start: 0, length: 3 }, newText: "y" }],
      },
    ];
    const out = remapAllFileTextChanges(ctx, changes);
    expect(out).toHaveLength(1);
    expect(out[0].fileName).toBe("A.vue");
  });
});

describe("rewriteInsertedSpecifier (companion → bare carrier import path)", () => {
  it("rewrites a Vue .vue.tsx companion specifier to the bare .vue", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const out = rewriteInsertedSpecifier(
      ctx,
      'import Comp from "./Comp.vue.tsx";',
      "d:/ws/src/Consumer.ts",
    );
    expect(out).toBe('import Comp from "./Comp.vue";');
  });

  it("rewrites a Vue .vue.verter.ts API-carrier specifier to the bare .vue", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const out = rewriteInsertedSpecifier(
      ctx,
      'import Comp from "./Comp.vue.verter.ts";',
      "d:/ws/src/Consumer.ts",
    );
    // The `.verter.ts` API-carrier suffix strips back to the bare `.vue`.
    expect(out).toContain("./Comp.vue");
    expect(out).not.toContain(".verter");
    expect(out).not.toContain(".vue.tsx");
  });

  it("rewrites a Svelte .svelte.tsx companion specifier to the bare .svelte", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const out = rewriteInsertedSpecifier(
      ctx,
      'import Widget from "./Widget.svelte.tsx";',
      "d:/ws/src/Consumer.ts",
    );
    expect(out).toBe('import Widget from "./Widget.svelte";');
  });

  it("does NOT mangle a real Svelte rune module specifier (store.svelte.ts has no carrier)", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    // `store.svelte.ts` is a REAL rune module (it exists as a self-file), NOT a
    // `store.svelte` carrier's companion — the ambiguous suffix must be left intact.
    const out = rewriteInsertedSpecifier(
      ctx,
      'import { s } from "./store.svelte.ts";',
      "d:/ws/src/Consumer.ts",
    );
    expect(out).toBe('import { s } from "./store.svelte.ts";');
  });

  it("leaves a plain .ts specifier untouched", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const out = rewriteInsertedSpecifier(
      ctx,
      'import { x } from "./utils";',
      "d:/ws/src/Consumer.ts",
    );
    expect(out).toBe('import { x } from "./utils";');
  });
});

// ── module-level companion definition (import-specifier go-to-def) ──────────
//
// A module-level definition (go-to-def on a `./Comp.vue` import specifier, or a
// default-export component reference) targets the companion AS A FILE: its
// `textSpan` is the module start (`{ start: 0, length: 1 }`) which legitimately
// has no specific source mapping, and TS stamps `kind: "module"`. The CORRECT
// navigation target is the `.vue`/`.svelte` SOURCE FILE — `fileName` → source,
// span → source-file start. This is DISTINCT from a specific-token unmappable
// companion span (a generated-only region), which still fails closed (dropped).

describe("sourceForCarrierCompanion", () => {
  it("resolves a companion provider path to its .vue / .svelte SOURCE (store authority)", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    expect(sourceForCarrierCompanion(ctx.reader, "d:/ws/src/A.vue.tsx")).toBe("d:/ws/src/A.vue");
    expect(sourceForCarrierCompanion(ctx.reader, "d:/ws/src/W.svelte.tsx")).toBe(
      "d:/ws/src/W.svelte",
    );
  });

  it("returns undefined for a bare SOURCE path or a real .ts (not a companion)", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    // A `.vue` source path is the map-TO target, not a companion.
    expect(sourceForCarrierCompanion(ctx.reader, "d:/ws/src/A.vue")).toBeUndefined();
    // A real user `.ts` is never a companion.
    expect(sourceForCarrierCompanion(ctx.reader, "d:/ws/src/Consumer.ts")).toBeUndefined();
    // An unknown companion-shaped path the store does not own.
    expect(sourceForCarrierCompanion(ctx.reader, "d:/ws/src/Nope.vue.tsx")).toBeUndefined();
  });
});

describe("isModuleLevelDefinition", () => {
  it("is true for kind 'module' / 'script' / 'alias' regardless of the span", () => {
    expect(isModuleLevelDefinition({ kind: "module", textSpan: { start: 42, length: 3 } })).toBe(
      true,
    );
    expect(isModuleLevelDefinition({ kind: "script", textSpan: { start: 99, length: 0 } })).toBe(
      true,
    );
    // `alias` is the resolved declaration of an imported binding — for a default
    // `.vue`/`.svelte` import it is the carrier's synthesized default-export
    // re-export (the component's module-level identity).
    expect(isModuleLevelDefinition({ kind: "alias", textSpan: { start: 2434, length: 7 } })).toBe(
      true,
    );
  });

  it("is FALSE for a token-level kind, even at the module start (start === 0)", () => {
    // The critical discriminator: a `ReferenceEntry`/`RenameLocation` carrying no
    // module/script kind, OR a token def at offset 0, is NOT module-level — a
    // generated-only region at offset 0 must FAIL CLOSED, not be reinterpreted as
    // a file-level navigation.
    expect(isModuleLevelDefinition({ kind: "const", textSpan: { start: 0, length: 1 } })).toBe(
      false,
    );
    expect(isModuleLevelDefinition({ textSpan: { start: 0, length: 0 } })).toBe(false);
  });

  it("is FALSE for a specific-token span (start > 0) that is not module/script kind", () => {
    expect(isModuleLevelDefinition({ kind: "const", textSpan: { start: 6, length: 3 } })).toBe(
      false,
    );
  });
});

describe("remapModuleLevelCompanionToSource", () => {
  it("rewrites a module-level Vue companion def to the .vue SOURCE @ file start", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const def = {
      fileName: "d:/ws/src/A.vue.tsx",
      textSpan: { start: 0, length: 1 },
      contextSpan: { start: 0, length: 1 },
      kind: "module",
      name: "./A.vue",
    };
    const mapped = remapModuleLevelCompanionToSource(ctx.reader, def);
    expect(mapped).toBeDefined();
    // The SOURCE .vue, NOT the companion.
    expect(mapped!.fileName).toBe("d:/ws/src/A.vue");
    expect(mapped!.fileName).not.toContain(".vue.tsx");
    // Span + context span collapse to the source-file start caret.
    expect(mapped!.textSpan).toEqual({ start: 0, length: 0 });
    expect(mapped!.contextSpan).toEqual({ start: 0, length: 0 });
  });

  it("rewrites a module-level SVELTE companion def to the .svelte SOURCE @ file start", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const def = {
      fileName: "d:/ws/src/W.svelte.tsx",
      textSpan: { start: 0, length: 1 },
      kind: "module",
      name: "./W.svelte",
    };
    const mapped = remapModuleLevelCompanionToSource(ctx.reader, def);
    expect(mapped).toBeDefined();
    expect(mapped!.fileName).toBe("d:/ws/src/W.svelte");
    expect(mapped!.fileName).not.toContain(".svelte.tsx");
    expect(mapped!.textSpan).toEqual({ start: 0, length: 0 });
  });

  it("FAILS CLOSED (undefined) when the source for the companion can't be resolved", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    // A companion-shaped path the store does not own → no source path → drop
    // (never surface the companion path).
    const def = {
      fileName: "d:/ws/src/Unknown.vue.tsx",
      textSpan: { start: 0, length: 1 },
      kind: "module",
      name: "./Unknown.vue",
    };
    expect(remapModuleLevelCompanionToSource(ctx.reader, def)).toBeUndefined();
  });
});

describe("remapDocumentSpan: module-level companion vs specific-token discrimination", () => {
  it("maps a MODULE-LEVEL companion def whose span is UNMAPPABLE to the .vue SOURCE @ file start", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    // `U.vue.tsx` line 1 (offset 0) has NO source mapping (the carrier
    // prelude) — the EXACT bug scenario. Because the def is module-level
    // (`kind: "module"`), it lands in the .vue source @ file start rather than
    // failing closed on the companion path.
    const def = {
      fileName: "d:/ws/src/U.vue.tsx",
      textSpan: { start: 0, length: 1 },
      contextSpan: { start: 0, length: 1 },
      kind: "module",
      name: "./U.vue",
    };
    const mapped = remapDocumentSpan(ctx, def);
    expect(mapped).toBeDefined();
    // The SOURCE .vue (store-owned source URI), NOT the companion.
    expect(mapped!.fileName).toBe("d:/ws/src/U.vue");
    expect(mapped!.fileName).not.toContain(".vue.tsx");
    expect(mapped!.textSpan).toEqual({ start: 0, length: 0 });
    expect(mapped!.contextSpan).toEqual({ start: 0, length: 0 });
  });

  it("still FAILS CLOSED for a SPECIFIC-token (non-module-kind) unmappable companion span", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    // `U.vue.tsx` line 1 (offset 0) is a generated-only region — but this entry
    // is a TOKEN-level ref (kind not module/script), so it must DROP, never be
    // reinterpreted as a file-level navigation. This is the fail-closed contract
    // a `ReferenceEntry`/`RenameLocation` at offset 0 depends on.
    const def = {
      fileName: "d:/ws/src/U.vue.tsx",
      textSpan: { start: 0, length: 3 },
      kind: "const",
      name: "real",
    };
    expect(remapDocumentSpan(ctx, def)).toBeUndefined();
  });

  it("a reference entry with NO kind at offset 0 still FAILS CLOSED (not reinterpreted)", () => {
    const ctx = ctxFor(track(writeStore(manifest(), blobs())));
    const ref = {
      fileName: "d:/ws/src/U.vue.tsx",
      textSpan: { start: 0, length: 3 },
      isWriteAccess: false,
    };
    expect(remapDocumentSpan(ctx, ref)).toBeUndefined();
  });
});

// ── source-text fallback: in-memory carrier whose source is not on disk ─────
//
// A carrier-edit remap (rename / remove-unused / references) needs the carrier
// SOURCE text for the inverse line/column→offset conversion. When the source is
// open in-memory (not on disk in the tsserver process — the LSP holds it), the
// host `readFile` returns undefined. The published map's `sourcesContent` is the
// EXACT source bytes the mappings were produced against, so the remap reads from
// it and still succeeds — without it the edit fails closed and the whole
// response is dropped (the unused-`<script>`-decl quick-fix regression).

describe("source-text from map sourcesContent (in-memory carrier, no disk source)", () => {
  /**
   * A manifest owning one Vue carrier whose map carries `sourcesContent` — the
   * inline source the offsets index. The companion line-1 token maps to it.
   */
  function manifestWithContent(): Manifest {
    return {
      epoch: 1,
      host_version: "test",
      projects: {
        "d:/ws/tsconfig.json": {
          owned_sources: [
            {
              source_uri: "d:/ws/src/Mem.vue",
              provider_uri: "d:/ws/src/Mem.vue.tsx",
              role: "CarrierIde",
              script_kind: "TSX",
            },
          ],
          ready_files: {
            "d:/ws/src/Mem.vue.tsx": {
              content_hash: "m1",
              version: 1,
              script_kind: "TSX",
              role: "CarrierIde",
              map_hash: "mm",
              blob_rel: "blobs/Mem.vue.tsx",
              map_rel: "maps/Mem.vue.json",
            },
          },
        },
      },
    };
  }

  // gen line 1 → src (Mem.vue line 1 col 0). The map embeds the source text.
  const MAP_WITH_CONTENT = {
    version: 3,
    sources: ["d:/ws/src/Mem.vue"],
    sourcesContent: ["const memBinding = 1;\nconst other = 2;\n"],
    names: [],
    mappings: "AAAA",
  };

  function memBlobs(): Record<string, string> {
    return {
      "blobs/Mem.vue.tsx": "const memBinding = 1;\n",
      "maps/Mem.vue.json": JSON.stringify(MAP_WITH_CONTENT),
    };
  }

  /** A context whose host `readSource` ALWAYS returns undefined (no disk copy). */
  function ctxNoDiskSource(dir: string): CarrierRemapContext {
    const reader = new DiskCarrierStoreReader(dir);
    return {
      reader,
      readCompanion: (p) => (p.endsWith("Mem.vue.tsx") ? "const memBinding = 1;\n" : undefined),
      // The carrier source is in-memory only — never readable from disk here.
      readSource: () => undefined,
      fileExists: () => false,
    };
  }

  it("remaps a companion edit to source using sourcesContent when the host has no disk source", () => {
    const ctx = ctxNoDiskSource(track(writeStore(manifestWithContent(), memBlobs())));
    // A code-action / rename edit on `memBinding` (companion offset 6, len 10).
    const change = {
      fileName: "d:/ws/src/Mem.vue.tsx",
      textChanges: [{ span: { start: 6, length: 10 }, newText: "renamedBinding" }],
    };
    const out = remapFileTextChanges(ctx, change);
    // Without the sourcesContent fallback this returns undefined (edit dropped).
    expect(out).toBeDefined();
    expect(out!.fileName).toBe("d:/ws/src/Mem.vue");
    expect(out!.fileName).not.toContain(".vue.tsx");
    // The source offset is computed from sourcesContent (offset 6 = `memBinding`).
    expect(out!.textChanges).toHaveLength(1);
    expect(out!.textChanges[0].span.start).toBe(6);
  });

  it("still FAILS CLOSED when neither the host NOR sourcesContent provides the source", () => {
    // A map WITHOUT sourcesContent + no disk source → the inverse conversion has
    // no source text → the edit is correctly dropped (fail closed, no mis-map).
    const noContentManifest = manifestWithContent();
    const dir = track(
      writeStore(noContentManifest, {
        "blobs/Mem.vue.tsx": "const memBinding = 1;\n",
        "maps/Mem.vue.json": JSON.stringify({
          version: 3,
          sources: ["d:/ws/src/Mem.vue"],
          names: [],
          mappings: "AAAA",
        }),
      }),
    );
    const ctx = ctxNoDiskSource(dir);
    const change = {
      fileName: "d:/ws/src/Mem.vue.tsx",
      textChanges: [{ span: { start: 6, length: 10 }, newText: "x" }],
    };
    expect(remapFileTextChanges(ctx, change)).toBeUndefined();
  });
});
