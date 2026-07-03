import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import ts from "typescript";
import init from "./index";
import type { Manifest } from "@verter/language-shared";

// ── fixture store ───────────────────────────────────────────────────────────

function writeStore(manifest: Manifest, blobs: Record<string, string>): string {
  const dir = mkdtempSync(join(tmpdir(), "verter-plugin-store-"));
  mkdirSync(join(dir, "blobs"), { recursive: true });
  mkdirSync(join(dir, "maps"), { recursive: true });
  for (const [rel, content] of Object.entries(blobs)) {
    const abs = join(dir, rel);
    mkdirSync(join(abs, ".."), { recursive: true });
    writeFileSync(abs, content, "utf8");
  }
  writeFileSync(join(dir, "manifest.json"), JSON.stringify(manifest), "utf8");
  return dir;
}

/**
 * A manifest with one READY Vue companion (`A.vue` → `A.vue.tsx`), one READY
 * Svelte companion (`W.svelte` → `W.svelte.tsx`), and one OWNED-but-NOT-READY
 * Vue companion (`B.vue` → `B.vue.tsx`).
 */
function vueAndSvelteManifest(): Manifest {
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
            source_uri: "d:/ws/src/B.vue",
            provider_uri: "d:/ws/src/B.vue.tsx",
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
            version: 5,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "0",
            blob_rel: "blobs/A.vue.tsx",
          },
          "d:/ws/src/W.svelte.tsx": {
            content_hash: "w1",
            version: 2,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "0",
            blob_rel: "blobs/W.svelte.tsx",
          },
        },
      },
    },
  };
}

// ── a minimal PluginCreateInfo over a real-disk passthrough ───────────────────

interface FakeHostState {
  diskFiles: Record<string, string>;
}

function createInfo(
  storeDir: string | undefined,
  disk: FakeHostState,
  projectName = "d:/ws/tsconfig.json",
) {
  const logger = { info: () => {}, msg: () => {} };
  const normalize = (f: string) => f.replace(/\\/g, "/");

  const serverHost: any = {
    useCaseSensitiveFileNames: false,
    fileExists: (f: string) => normalize(f) in disk.diskFiles,
    readFile: (f: string) => disk.diskFiles[normalize(f)],
    directoryExists: () => true,
    getDirectories: () => [],
    realpath: (f: string) => f,
  };

  const languageServiceHost: any = {
    getCompilationSettings: () => ({}),
    getScriptSnapshot: (f: string) => {
      const content = disk.diskFiles[normalize(f)];
      return content !== undefined ? ts.ScriptSnapshot.fromString(content) : undefined;
    },
    getScriptVersion: () => "disk-0",
    getScriptKind: () => ts.ScriptKind.Unknown,
    resolveModuleNameLiterals: (literals: any[]) =>
      literals.map(() => ({ resolvedModule: undefined })),
    resolveTypeReferenceDirectiveReferences: (refs: any[]) =>
      refs.map(() => ({ resolvedTypeReferenceDirective: undefined })),
    directoryExists: () => true,
    getDirectories: () => [],
    realpath: (f: string) => f,
  };

  const languageService: any = {
    getProgram: () => undefined,
    getDefinitionAndBoundSpan: () => undefined,
    getDefinitionAtPosition: () => undefined,
    getTypeDefinitionAtPosition: () => undefined,
    getQuickInfoAtPosition: () => undefined,
    getCompletionEntryDetails: () => undefined,
    getCompletionsAtPosition: () => undefined,
    // The companion→source RESPONSE-remap surface. The plugin wraps each of
    // these; a test overrides the underlying impl via `__lsImpl` to drive a
    // companion-carrying response through the wrapper.
    getReferencesAtPosition: (...a: any[]) =>
      languageService.__lsImpl?.getReferencesAtPosition?.(...a),
    findReferences: (...a: any[]) => languageService.__lsImpl?.findReferences?.(...a),
    getImplementationAtPosition: (...a: any[]) =>
      languageService.__lsImpl?.getImplementationAtPosition?.(...a),
    findRenameLocations: (...a: any[]) => languageService.__lsImpl?.findRenameLocations?.(...a),
    getCodeFixesAtPosition: (...a: any[]) =>
      languageService.__lsImpl?.getCodeFixesAtPosition?.(...a) ?? [],
    getCombinedCodeFix: (...a: any[]) => languageService.__lsImpl?.getCombinedCodeFix?.(...a),
    getEditsForRefactor: (...a: any[]) => languageService.__lsImpl?.getEditsForRefactor?.(...a),
    getEditsForFileRename: (...a: any[]) =>
      languageService.__lsImpl?.getEditsForFileRename?.(...a) ?? [],
    __lsImpl: undefined as any,
  };

  const project: any = {
    // The plugin `process.chdir`s to this on startup, so it must be a real
    // directory; the store paths (`d:/ws/...`) are independent of cwd.
    getCurrentDirectory: () => process.cwd(),
    getCompilerOptions: () => ({}),
    getProjectName: () => projectName,
    projectService: { logger },
    getFileNames: () => [],
  };

  return {
    config: storeDir === undefined ? undefined : { carrierStoreDir: storeDir },
    languageService,
    languageServiceHost,
    serverHost,
    project,
  } as any;
}

let dirs: string[] = [];
beforeEach(() => {
  dirs = [];
});
afterEach(() => {
  for (const d of dirs) rmSync(d, { recursive: true, force: true });
  delete process.env.VERTER_CARRIER_STORE_DIR;
});
function track(d: string): string {
  dirs.push(d);
  return d;
}

// ── tests ─────────────────────────────────────────────────────────────────

describe("host-proxy matrix: getScriptSnapshot", () => {
  it("serves the ready Vue companion blob", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), { "blobs/A.vue.tsx": "export const A = 1; // vue" }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    const snap = info.languageServiceHost.getScriptSnapshot("d:/ws/src/A.vue.tsx");
    expect(snap).toBeDefined();
    expect(snap.getText(0, snap.getLength())).toBe("export const A = 1; // vue");
  });

  it("serves the ready Svelte companion blob", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), { "blobs/W.svelte.tsx": "export const W = 1; // svelte" }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    const snap = info.languageServiceHost.getScriptSnapshot("d:/ws/src/W.svelte.tsx");
    expect(snap.getText(0, snap.getLength())).toBe("export const W = 1; // svelte");
  });

  it("falls through to disk for a non-companion path", () => {
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: { "d:/ws/src/plain.ts": "const x = 1;" } });
    init({ typescript: ts } as any).create(info);

    const snap = info.languageServiceHost.getScriptSnapshot("d:/ws/src/plain.ts");
    expect(snap.getText(0, snap.getLength())).toBe("const x = 1;");
  });
});

describe("host-proxy matrix: getScriptVersion / getScriptKind", () => {
  it("returns the manifest version for a ready companion (vue + svelte)", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "x",
        "blobs/W.svelte.tsx": "y",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    expect(info.languageServiceHost.getScriptVersion("d:/ws/src/A.vue.tsx")).toBe("5");
    expect(info.languageServiceHost.getScriptVersion("d:/ws/src/W.svelte.tsx")).toBe("2");
  });

  it("maps the manifest script kind to ts.ScriptKind (vue + svelte)", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "x",
        "blobs/W.svelte.tsx": "y",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    expect(info.languageServiceHost.getScriptKind("d:/ws/src/A.vue.tsx")).toBe(ts.ScriptKind.TSX);
    expect(info.languageServiceHost.getScriptKind("d:/ws/src/W.svelte.tsx")).toBe(
      ts.ScriptKind.TSX,
    );
  });

  it("falls through to the disk version/kind for a non-companion", () => {
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: { "d:/ws/src/plain.ts": "x" } });
    init({ typescript: ts } as any).create(info);
    expect(info.languageServiceHost.getScriptVersion("d:/ws/src/plain.ts")).toBe("disk-0");
  });
});

describe("host-proxy matrix: readFile / fileExists", () => {
  it("readFile serves the companion blob (vue + svelte)", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "vue-content",
        "blobs/W.svelte.tsx": "svelte-content",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    expect(info.serverHost.readFile("d:/ws/src/A.vue.tsx")).toBe("vue-content");
    expect(info.serverHost.readFile("d:/ws/src/W.svelte.tsx")).toBe("svelte-content");
  });

  it("fileExists is true for a ready companion (vue + svelte), false for unknown", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "x",
        "blobs/W.svelte.tsx": "y",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    expect(info.serverHost.fileExists("d:/ws/src/A.vue.tsx")).toBe(true);
    expect(info.serverHost.fileExists("d:/ws/src/W.svelte.tsx")).toBe(true);
    expect(info.serverHost.fileExists("d:/ws/src/Nope.vue.tsx")).toBe(false);
  });

  it("readFile/fileExists fall through to real disk for a non-companion", () => {
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: { "d:/ws/src/real.ts": "real" } });
    init({ typescript: ts } as any).create(info);

    expect(info.serverHost.readFile("d:/ws/src/real.ts")).toBe("real");
    expect(info.serverHost.fileExists("d:/ws/src/real.ts")).toBe(true);
  });
});

describe("host-proxy matrix: resolveModuleNameLiterals (in-project → IDE carrier)", () => {
  function resolveOne(info: any, specifier: string, containing: string): string | undefined {
    const result = info.languageServiceHost.resolveModuleNameLiterals(
      [{ text: specifier }],
      containing,
      undefined,
      {},
      undefined,
    );
    return result[0]?.resolvedModule?.resolvedFileName;
  }

  it("redirects a relative .vue import to the .vue.tsx IDE carrier (NOT .verter.ts)", () => {
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    const resolved = resolveOne(info, "./A.vue", "d:/ws/src/consumer.ts");
    expect(resolved).toBe("d:/ws/src/A.vue.tsx");
    expect(resolved).not.toContain(".verter.ts");
  });

  it("redirects a relative .svelte import to the .svelte.tsx IDE carrier", () => {
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    const resolved = resolveOne(info, "./W.svelte", "d:/ws/src/consumer.ts");
    expect(resolved).toBe("d:/ws/src/W.svelte.tsx");
    expect(resolved).not.toContain(".verter.ts");
  });

  it("leaves a plain relative .ts import to TS's own resolution", () => {
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    // Non-carrier specifier: the override returns undefined → delegate result.
    const resolved = resolveOne(info, "./plain", "d:/ws/src/consumer.ts");
    expect(resolved).toBeUndefined();
  });
});

describe("getExternalFiles = ready IDE companions only", () => {
  it("returns ONLY the IDE companion paths of ready carriers, never not-ready ones", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "x",
        "blobs/W.svelte.tsx": "y",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    const plugin = init({ typescript: ts } as any);
    // `create` records the store dir for the project before getExternalFiles.
    plugin.create(info);

    // The advertised member identity is the IDE COMPANION (`.vue.tsx`), not the
    // bare source — it is what the host hooks serve + what the LSP queries, so a
    // `.tsx` companion becomes a configured-project member with the project's
    // own compiler options.
    const files = plugin.getExternalFiles!(info.project, 0 as any);
    expect(files.sort()).toEqual(["d:/ws/src/A.vue.tsx", "d:/ws/src/W.svelte.tsx"]);
    // B.vue.tsx is OWNED but NOT ready — it must NOT be advertised.
    expect(files).not.toContain("d:/ws/src/B.vue.tsx");
    // The bare source path is NOT the advertised member identity.
    expect(files).not.toContain("d:/ws/src/A.vue");
  });

  it("returns [] when the store is unavailable", () => {
    const info = createInfo(undefined, { diskFiles: {} });
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);
    expect(plugin.getExternalFiles!(info.project, 0 as any)).toEqual([]);
  });
});

/**
 * A manifest with TWO configured projects, each owning + publishing a DIFFERENT
 * carrier: project A owns a Vue carrier, project B owns a Svelte carrier. The
 * cross-project-leak guard.
 */
function twoProjectManifest(): Manifest {
  return {
    epoch: 2,
    host_version: "test",
    projects: {
      "d:/ws/a/tsconfig.json": {
        owned_sources: [
          {
            source_uri: "d:/ws/a/src/A.vue",
            provider_uri: "d:/ws/a/src/A.vue.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
        ],
        ready_files: {
          "d:/ws/a/src/A.vue.tsx": {
            content_hash: "a1",
            version: 1,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "0",
            blob_rel: "blobs/A.vue.tsx",
          },
        },
      },
      "d:/ws/b/tsconfig.json": {
        owned_sources: [
          {
            source_uri: "d:/ws/b/src/B.svelte",
            provider_uri: "d:/ws/b/src/B.svelte.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
        ],
        ready_files: {
          "d:/ws/b/src/B.svelte.tsx": {
            content_hash: "b1",
            version: 1,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "0",
            blob_rel: "blobs/B.svelte.tsx",
          },
        },
      },
    },
  };
}

describe("getExternalFiles is project-scoped (no cross-tsconfig leak)", () => {
  it("getExternalFiles(projectA) returns ONLY projectA's carrier, never projectB's (vue + svelte)", () => {
    const dir = track(
      writeStore(twoProjectManifest(), {
        "blobs/A.vue.tsx": "x",
        "blobs/B.svelte.tsx": "y",
      }),
    );

    // Two per-project plugin instances, each `create`d for its own tsconfig.
    const infoA = createInfo(dir, { diskFiles: {} }, "d:/ws/a/tsconfig.json");
    const infoB = createInfo(dir, { diskFiles: {} }, "d:/ws/b/tsconfig.json");
    const pluginA = init({ typescript: ts } as any);
    const pluginB = init({ typescript: ts } as any);
    pluginA.create(infoA);
    pluginB.create(infoB);

    const filesA = pluginA.getExternalFiles!(infoA.project, 0 as any);
    const filesB = pluginB.getExternalFiles!(infoB.project, 0 as any);

    // Project A sees ONLY its Vue carrier; project B sees ONLY its Svelte carrier.
    expect(filesA).toEqual(["d:/ws/a/src/A.vue.tsx"]);
    expect(filesB).toEqual(["d:/ws/b/src/B.svelte.tsx"]);
    // The leak the fix closes: neither project advertises the OTHER's carrier.
    expect(filesA).not.toContain("d:/ws/b/src/B.svelte.tsx");
    expect(filesB).not.toContain("d:/ws/a/src/A.vue.tsx");
  });

  it("a single plugin instance, queried for a DIFFERENT project, scopes to that project", () => {
    // `getExternalFiles` may be called for a project this plugin instance has
    // not `create`d (the env-fallback path); it still scopes by the project arg.
    const dir = track(
      writeStore(twoProjectManifest(), {
        "blobs/A.vue.tsx": "x",
        "blobs/B.svelte.tsx": "y",
      }),
    );
    process.env.VERTER_CARRIER_STORE_DIR = dir;
    const info = createInfo(dir, { diskFiles: {} }, "d:/ws/a/tsconfig.json");
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    // Query for project B via a project stub carrying B's name — the store dir
    // comes from the env fallback (B was never `create`d on this instance).
    const projectB: any = {
      getProjectName: () => "d:/ws/b/tsconfig.json",
      projectService: { logger: { info: () => {} } },
    };
    const filesB = plugin.getExternalFiles!(projectB, 0 as any);
    expect(filesB).toEqual(["d:/ws/b/src/B.svelte.tsx"]);
    expect(filesB).not.toContain("d:/ws/a/src/A.vue.tsx");
  });

  it("host hooks for project A never serve project B's carrier content (reader is scoped)", () => {
    // Even the per-file host hooks are project-scoped: project A's plugin must
    // not serve project B's companion content/version/kind/existence.
    const dir = track(
      writeStore(twoProjectManifest(), {
        "blobs/A.vue.tsx": "A-content",
        "blobs/B.svelte.tsx": "B-content",
      }),
    );
    const infoA = createInfo(dir, { diskFiles: {} }, "d:/ws/a/tsconfig.json");
    init({ typescript: ts } as any).create(infoA);

    // Project A serves its own carrier…
    expect(infoA.serverHost.readFile("d:/ws/a/src/A.vue.tsx")).toBe("A-content");
    expect(infoA.serverHost.fileExists("d:/ws/a/src/A.vue.tsx")).toBe(true);
    // …but NOT project B's carrier (it falls through to disk, which is empty).
    expect(infoA.serverHost.readFile("d:/ws/b/src/B.svelte.tsx")).toBeUndefined();
    expect(infoA.serverHost.fileExists("d:/ws/b/src/B.svelte.tsx")).toBe(false);
    expect(infoA.languageServiceHost.getScriptSnapshot("d:/ws/b/src/B.svelte.tsx")).toBeUndefined();
  });
});

/**
 * A manifest with a ready Vue companion (`A.vue` → `A.vue.tsx`) carrying a REAL
 * source map (so a companion-carrying response can be mapped back to source),
 * plus a ready Svelte companion (`W.svelte` → `W.svelte.tsx`) with a map.
 */
function mappableManifest(): Manifest {
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

// gen (1,0) → src (line1,col0). The companion's line-1 identifier maps. The map
// `sources` are the ABSOLUTE source paths the plugin's `readSource` (the host
// `readFile`) resolves on disk — exactly how a published carrier map names its
// source so the response remap reads the real `.vue`/`.svelte` text.
const MAPPABLE_MAP = JSON.stringify({
  version: 3,
  sources: ["d:/ws/src/A.vue"],
  names: [],
  mappings: "AAAA",
});
// gen (1,0) → src (W.svelte 2,0) — `AACA` = VLQ[0,0,1,0]: the companion's
// script statement maps onto the source's `const bar = 1;` line (after the
// `<script>` opener), so a token span maps end-to-end within one source line
// under strict BOTH-endpoint span mapping.
const SVELTE_MAP = JSON.stringify({
  version: 3,
  sources: ["d:/ws/src/W.svelte"],
  names: [],
  mappings: "AACA",
});

/**
 * `U.vue.tsx`'s map only maps generated LINE 2 (`;AAAA`); its line 1 is a
 * generated-only helper with NO source origin — a span there fails closed.
 */
const UNMAPPABLE_MAP = JSON.stringify({
  version: 3,
  sources: ["U.vue"],
  names: [],
  mappings: ";AAAA",
});

/** The blob+map+source set the mappable manifest references. */
function mappableBlobs(): Record<string, string> {
  return {
    "blobs/A.vue.tsx": "const foo = 1;\n",
    "maps/A.vue.json": MAPPABLE_MAP,
    "blobs/U.vue.tsx": "/* gen helper */\nconst real = 1;\n",
    "maps/U.vue.json": UNMAPPABLE_MAP,
    "blobs/W.svelte.tsx": "const bar = 1;\n",
    "maps/W.svelte.json": SVELTE_MAP,
  };
}

describe("companion→source RESPONSE remap wiring (the new nav hooks)", () => {
  // The disk carries the carrier SOURCES so the response remapper's source-text
  // read succeeds (it reads via the plugin's `_readFile` = serverHost.readFile).
  function diskWithSources(): Record<string, string> {
    return {
      "d:/ws/src/A.vue": "<template/>\n<script setup>\nconst foo = 1;\n</script>\n",
      "d:/ws/src/U.vue": "<template/>\n<script setup>\nconst real = 1;\n</script>\n",
      "d:/ws/src/W.svelte": "<script>\nconst bar = 1;\n</script>\n",
      "d:/ws/src/Consumer.ts": "import A from './A.vue';\n",
    };
  }

  it("getReferencesAtPosition: a companion entry → source, real .ts passes through", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfo(dir, { diskFiles: diskWithSources() });
    info.languageService.__lsImpl = {
      getReferencesAtPosition: () => [
        {
          fileName: "d:/ws/src/Consumer.ts",
          textSpan: { start: 9, length: 1 },
          isWriteAccess: false,
        },
        {
          fileName: "d:/ws/src/A.vue.tsx",
          textSpan: { start: 6, length: 3 },
          isWriteAccess: false,
        },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const refs = info.languageService.getReferencesAtPosition("d:/ws/src/Consumer.ts", 9);
    const paths = refs.map((r: any) => r.fileName);
    // The companion entry is mapped to the SOURCE .vue, the real .ts is intact.
    expect(paths).toContain("d:/ws/src/A.vue");
    expect(paths).toContain("d:/ws/src/Consumer.ts");
    expect(paths.some((p: string) => p.includes(".vue.tsx"))).toBe(false);
  });

  it("getReferencesAtPosition: an UNMAPPABLE companion entry is DROPPED (fail closed)", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfo(dir, { diskFiles: diskWithSources() });
    info.languageService.__lsImpl = {
      getReferencesAtPosition: () => [
        // `U.vue.tsx` line 1 (offset 0) is a generated-only helper region — its
        // map only covers line 2, so this span has NO source origin.
        {
          fileName: "d:/ws/src/U.vue.tsx",
          textSpan: { start: 0, length: 3 },
          isWriteAccess: false,
        },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const refs = info.languageService.getReferencesAtPosition("d:/ws/src/Consumer.ts", 9);
    // No mappable origin → dropped: NEVER a companion path, NEVER a mis-mapped source.
    expect(refs).toHaveLength(0);
  });

  it("findRenameLocations: a Svelte companion location → .svelte source", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfo(dir, { diskFiles: diskWithSources() });
    info.languageService.__lsImpl = {
      findRenameLocations: () => [
        { fileName: "d:/ws/src/W.svelte.tsx", textSpan: { start: 6, length: 3 } },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const locs = info.languageService.findRenameLocations(
      "d:/ws/src/Consumer.ts",
      0,
      false,
      false,
      undefined,
    );
    expect(locs).toHaveLength(1);
    expect(locs[0].fileName).toBe("d:/ws/src/W.svelte");
    expect(locs[0].fileName).not.toContain(".svelte.tsx");
    // The rename span lands EXACTLY on `bar` inside the source's script line.
    expect(locs[0].textSpan).toEqual({ start: 15, length: 3 });
  });

  it("getCodeFixesAtPosition: a companion file edit → source path; companion specifier → bare .vue", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfo(dir, { diskFiles: diskWithSources() });
    info.languageService.__lsImpl = {
      getCodeFixesAtPosition: () => [
        {
          fixName: "x",
          description: "fix",
          changes: [
            {
              fileName: "d:/ws/src/A.vue.tsx",
              textChanges: [{ span: { start: 6, length: 3 }, newText: "renamed" }],
            },
            {
              fileName: "d:/ws/src/Consumer.ts",
              textChanges: [
                { span: { start: 0, length: 0 }, newText: 'import C from "./Comp.vue.tsx";\n' },
              ],
            },
          ],
        },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const fixes = info.languageService.getCodeFixesAtPosition(
      "d:/ws/src/Consumer.ts",
      0,
      1,
      [1],
      {},
      {},
    );
    const changePaths = fixes[0].changes.map((c: any) => c.fileName);
    // The companion edit landed on the .vue SOURCE; the real .ts edit kept its
    // path with the import specifier rewritten to the bare .vue.
    expect(changePaths).toContain("d:/ws/src/A.vue");
    expect(changePaths.some((p: string) => p.includes(".vue.tsx"))).toBe(false);
    const consumerEdit = fixes[0].changes.find((c: any) => c.fileName === "d:/ws/src/Consumer.ts");
    expect(consumerEdit.textChanges[0].newText).toBe('import C from "./Comp.vue";\n');
  });

  it("getCompletionEntryDetails: companion code-action edit → source + specifier rewrite", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfo(dir, { diskFiles: diskWithSources() });
    // Override the (already-wrapped) detail impl to return a companion edit.
    info.languageService.getCompletionEntryDetails = () => ({
      name: "Comp",
      kind: "alias",
      kindModifiers: "",
      displayParts: [],
      codeActions: [
        {
          description: 'Add import from "./Comp.vue.tsx"',
          changes: [
            {
              fileName: "d:/ws/src/A.vue.tsx",
              textChanges: [{ span: { start: 6, length: 3 }, newText: 'import "./Comp.vue.tsx";' }],
            },
          ],
        },
      ],
    });
    init({ typescript: ts } as any).create(info);

    const detail = info.languageService.getCompletionEntryDetails(
      "d:/ws/src/Consumer.ts",
      0,
      "Comp",
      {},
      undefined,
      undefined,
      undefined,
    );
    const action = detail.codeActions[0];
    // The description display-cleanup strips the companion suffix…
    expect(action.description).toBe('Add import from "./Comp.vue"');
    // …and the edit maps to the .vue SOURCE with the specifier rewritten.
    expect(action.changes[0].fileName).toBe("d:/ws/src/A.vue");
    expect(action.changes[0].textChanges[0].newText).toBe('import "./Comp.vue";');
    expect(action.changes[0].textChanges[0].newText).not.toContain(".vue.tsx");
  });
});

// The verter_lsp-INTERNAL backend disables the plugin's response remap
// (`responseRemap: false` / `VERTER_PLUGIN_RESPONSE_REMAP=0`): the Rust merge
// layer is the sole companion→source mapper there, so the plugin must return
// RAW companion responses (no path/span remap, no specifier rewrite) to avoid
// double-mapping. The DISPLAY-only description cleanup still runs (cosmetic).
describe("responseRemap = false (verter_lsp-internal backend) → RAW companion responses", () => {
  function diskWithSources(): Record<string, string> {
    return {
      "d:/ws/src/A.vue": "<template/>\n<script setup>\nconst foo = 1;\n</script>\n",
      "d:/ws/src/W.svelte": "<script>\nconst bar = 1;\n</script>\n",
      "d:/ws/src/Consumer.ts": "import A from './A.vue';\n",
    };
  }

  /** A `createInfo` whose plugin config disables response remap. */
  function createInfoNoRemap(storeDir: string, disk: { diskFiles: Record<string, string> }) {
    const info = createInfo(storeDir, disk);
    info.config = { ...info.config, responseRemap: false };
    return info;
  }

  it("getReferencesAtPosition: a companion entry passes through RAW (companion path kept)", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfoNoRemap(dir, { diskFiles: diskWithSources() });
    info.languageService.__lsImpl = {
      getReferencesAtPosition: () => [
        {
          fileName: "d:/ws/src/A.vue.tsx",
          textSpan: { start: 6, length: 3 },
          isWriteAccess: false,
        },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const refs = info.languageService.getReferencesAtPosition("d:/ws/src/Consumer.ts", 9);
    // RAW: the companion path is preserved (Rust maps it), the span is unchanged.
    expect(refs).toHaveLength(1);
    expect(refs[0].fileName).toBe("d:/ws/src/A.vue.tsx");
    expect(refs[0].textSpan).toEqual({ start: 6, length: 3 });
  });

  it("findRenameLocations: a companion location passes through RAW", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfoNoRemap(dir, { diskFiles: diskWithSources() });
    info.languageService.__lsImpl = {
      findRenameLocations: () => [
        { fileName: "d:/ws/src/W.svelte.tsx", textSpan: { start: 6, length: 3 } },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const locs = info.languageService.findRenameLocations(
      "d:/ws/src/Consumer.ts",
      0,
      false,
      false,
      undefined,
    );
    expect(locs).toHaveLength(1);
    expect(locs[0].fileName).toBe("d:/ws/src/W.svelte.tsx");
  });

  it("getCodeFixesAtPosition: companion edits + specifier pass through RAW (no rewrite)", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfoNoRemap(dir, { diskFiles: diskWithSources() });
    info.languageService.__lsImpl = {
      getCodeFixesAtPosition: () => [
        {
          fixName: "x",
          description: "fix",
          changes: [
            {
              fileName: "d:/ws/src/A.vue.tsx",
              textChanges: [{ span: { start: 6, length: 3 }, newText: "renamed" }],
            },
            {
              fileName: "d:/ws/src/Consumer.ts",
              textChanges: [
                { span: { start: 0, length: 0 }, newText: 'import C from "./Comp.vue.tsx";\n' },
              ],
            },
          ],
        },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const fixes = info.languageService.getCodeFixesAtPosition(
      "d:/ws/src/Consumer.ts",
      0,
      1,
      [1],
      {},
      {},
    );
    // RAW: the companion edit KEEPS its `.vue.tsx` path (Rust maps it) and the
    // inserted specifier is NOT rewritten (Rust owns that on the LSP surface).
    const changePaths = fixes[0].changes.map((c: any) => c.fileName);
    expect(changePaths).toContain("d:/ws/src/A.vue.tsx");
    const consumerEdit = fixes[0].changes.find((c: any) => c.fileName === "d:/ws/src/Consumer.ts");
    expect(consumerEdit.textChanges[0].newText).toBe('import C from "./Comp.vue.tsx";\n');
  });

  it("getCompletionEntryDetails: edits pass through RAW; description cleanup STILL runs", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfoNoRemap(dir, { diskFiles: diskWithSources() });
    info.languageService.getCompletionEntryDetails = () => ({
      name: "Comp",
      kind: "alias",
      kindModifiers: "",
      displayParts: [],
      codeActions: [
        {
          description: 'Add import from "./Comp.vue.tsx"',
          changes: [
            {
              fileName: "d:/ws/src/A.vue.tsx",
              textChanges: [{ span: { start: 6, length: 3 }, newText: 'import "./Comp.vue.tsx";' }],
            },
          ],
        },
      ],
    });
    init({ typescript: ts } as any).create(info);

    const detail = info.languageService.getCompletionEntryDetails(
      "d:/ws/src/Consumer.ts",
      0,
      "Comp",
      {},
      undefined,
      undefined,
      undefined,
    );
    const action = detail.codeActions[0];
    // DISPLAY-only description cleanup is cosmetic and stays ON both surfaces.
    expect(action.description).toBe('Add import from "./Comp.vue"');
    // The actual EDIT passes through RAW: companion path kept, specifier NOT
    // rewritten — the Rust completion/merge layer owns the LSP-surface mapping.
    expect(action.changes[0].fileName).toBe("d:/ws/src/A.vue.tsx");
    expect(action.changes[0].textChanges[0].newText).toBe('import "./Comp.vue.tsx";');
  });

  it("getDefinitionAtPosition: a module-level companion def passes through RAW", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfoNoRemap(dir, { diskFiles: diskWithSources() });
    // The aliased / module-nav paths are not driven here (no program); the
    // fallback `getDefinitionAtPosition` returns a companion module-level def.
    info.languageService.getDefinitionAtPosition = () => [
      {
        fileName: "d:/ws/src/A.vue.tsx",
        textSpan: { start: 0, length: 1 },
        kind: "module",
        name: "A",
        containerName: "",
        containerKind: "",
      },
    ];
    init({ typescript: ts } as any).create(info);

    const defs = info.languageService.getDefinitionAtPosition("d:/ws/src/Consumer.ts", 9);
    // RAW: the companion path is preserved (the Rust merge layer maps the
    // module-level def → `.vue` source); the plugin does NOT remap here.
    expect(defs).toHaveLength(1);
    expect(defs[0].fileName).toBe("d:/ws/src/A.vue.tsx");
  });
});

describe("module-level companion definition remap (import-specifier go-to-def)", () => {
  // The marquee §2.9 bug: go-to-def on a `./Comp.vue` import specifier from a
  // plain `.ts` resolves to the IDE companion (`Comp.vue.tsx`) and mints a
  // MODULE-LEVEL DefinitionInfo (`kind: "module"`, span at the module start). The
  // span has no specific source mapping (it is the carrier prelude), so the
  // remap must rewrite it to the `.vue`/`.svelte` SOURCE — never leave the
  // companion path (a `.vue.tsx` not on disk).

  /** A minimal program stub exposing `getSourceFile` + a no-symbol checker. */
  function programFor(sourceFile: ts.SourceFile) {
    const checker = {
      // No aliased symbol → `getAliasedNavigationResult` bails, so the module
      // specifier navigation path runs (the path under test).
      getSymbolAtLocation: () => undefined,
      getAliasedSymbol: () => undefined,
    } as unknown as ts.TypeChecker;
    return {
      getSourceFile: (f: string) =>
        f.replace(/\\/g, "/") === sourceFile.fileName ? sourceFile : undefined,
      getTypeChecker: () => checker,
    } as unknown as ts.Program;
  }

  function infoWithProgram(
    storeDir: string,
    disk: FakeHostState,
    consumerPath: string,
    consumerSource: string,
  ) {
    const info = createInfo(storeDir, disk);
    const sourceFile = ts.createSourceFile(
      consumerPath,
      consumerSource,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TS,
    );
    const program = programFor(sourceFile);
    info.languageService.getProgram = () => program;
    // The fallback path is never reached for a module-specifier hit, but keep it
    // returning nothing so a non-specifier position fails closed.
    info.languageService.getDefinitionAtPosition = () => undefined;
    info.languageService.getDefinitionAndBoundSpan = () => undefined;
    return { info, consumerSource };
  }

  /** A consumer `.ts` that imports both a Vue and a Svelte carrier. */
  const CONSUMER = 'import Comp from "./A.vue";\nimport Widget from "./W.svelte";\n';

  function diskWithCarrierSources(): Record<string, string> {
    return {
      "d:/ws/src/A.vue": '<template/>\n<script setup lang="ts">\nconst x = 1;\n</script>\n',
      "d:/ws/src/W.svelte": '<script lang="ts">\nconst y = 1;\n</script>\n',
      // The carriers must exist on disk so `resolveModuleFileName` (which checks
      // `path.resolve(dir, './A.vue')`) resolves them; the plugin redirects to
      // the IDE companion via `toIdeCarrierFileName`.
      "d:/ws/src/Consumer.ts": CONSUMER,
    };
  }

  it("Vue: go-to-def on a `./A.vue` import specifier lands in the .vue SOURCE, not the companion", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), { "blobs/A.vue.tsx": "export const A = 1;" }),
    );
    const { info, consumerSource } = infoWithProgram(
      dir,
      { diskFiles: diskWithCarrierSources() },
      "d:/ws/src/Consumer.ts",
      CONSUMER,
    );
    init({ typescript: ts } as any).create(info);

    // Offset inside the `./A.vue` specifier text.
    const off = consumerSource.indexOf("./A.vue") + 2;
    const defs = info.languageService.getDefinitionAtPosition("d:/ws/src/Consumer.ts", off);
    expect(defs).toBeDefined();
    expect(defs).toHaveLength(1);
    // Lands in the .vue SOURCE.
    expect(defs[0].fileName).toBe("d:/ws/src/A.vue");
    // NEVER the companion path.
    expect(defs[0].fileName).not.toContain(".vue.tsx");
    expect(defs[0].fileName).not.toContain(".verter.ts");
    // Source-file start caret.
    expect(defs[0].textSpan).toEqual({ start: 0, length: 0 });
  });

  it("Svelte: go-to-def on a `./W.svelte` import specifier lands in the .svelte SOURCE", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), { "blobs/W.svelte.tsx": "export const W = 1;" }),
    );
    const { info, consumerSource } = infoWithProgram(
      dir,
      { diskFiles: diskWithCarrierSources() },
      "d:/ws/src/Consumer.ts",
      CONSUMER,
    );
    init({ typescript: ts } as any).create(info);

    const off = consumerSource.indexOf("./W.svelte") + 2;
    const defs = info.languageService.getDefinitionAtPosition("d:/ws/src/Consumer.ts", off);
    expect(defs).toBeDefined();
    expect(defs).toHaveLength(1);
    expect(defs[0].fileName).toBe("d:/ws/src/W.svelte");
    expect(defs[0].fileName).not.toContain(".svelte.tsx");
    expect(defs[0].fileName).not.toContain(".verter.ts");
  });

  it("getDefinitionAndBoundSpan: the bound span is the specifier, the def lands in the .vue source", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), { "blobs/A.vue.tsx": "export const A = 1;" }),
    );
    const { info, consumerSource } = infoWithProgram(
      dir,
      { diskFiles: diskWithCarrierSources() },
      "d:/ws/src/Consumer.ts",
      CONSUMER,
    );
    init({ typescript: ts } as any).create(info);

    const off = consumerSource.indexOf("./A.vue") + 2;
    const result = info.languageService.getDefinitionAndBoundSpan("d:/ws/src/Consumer.ts", off);
    expect(result).toBeDefined();
    expect(result.definitions).toHaveLength(1);
    expect(result.definitions[0].fileName).toBe("d:/ws/src/A.vue");
    expect(result.definitions[0].fileName).not.toContain(".vue.tsx");
  });
});

describe("store unavailable → fail closed", () => {
  it("serves nothing for carriers and falls through for everything else", () => {
    const info = createInfo(undefined, {
      diskFiles: { "d:/ws/src/A.vue.tsx": "ON-DISK", "d:/ws/src/real.ts": "real" },
    });
    init({ typescript: ts } as any).create(info);

    // No store → the companion path is whatever is on real disk (no fabrication).
    expect(info.serverHost.readFile("d:/ws/src/real.ts")).toBe("real");
    // A path that happens to look like a companion still just hits disk.
    expect(info.serverHost.readFile("d:/ws/src/A.vue.tsx")).toBe("ON-DISK");
  });
});

describe("carrier-path conflict honor (manifest is the authority)", () => {
  it("a source absent from the manifest → fileExists for its companion falls through to disk", () => {
    // Rust marked Foo.vue Ambiguous, so it is ABSENT from owned_sources/ready_files.
    const dir = track(writeStore(vueAndSvelteManifest(), { "blobs/A.vue.tsx": "x" }));
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    // Foo.vue.tsx is not in the manifest → not fabricated; disk says no.
    expect(info.serverHost.fileExists("d:/ws/src/Foo.vue.tsx")).toBe(false);
    expect(info.languageServiceHost.getScriptSnapshot("d:/ws/src/Foo.vue.tsx")).toBeUndefined();
  });

  it("does not overlay-shadow a real file living at the carrier path", () => {
    // A real user file sits at Foo.vue.tsx; Rust left it out of the manifest.
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: { "d:/ws/src/Foo.vue.tsx": "REAL USER FILE" } });
    init({ typescript: ts } as any).create(info);

    // The plugin honors the manifest and returns the REAL file, not a companion.
    expect(info.serverHost.readFile("d:/ws/src/Foo.vue.tsx")).toBe("REAL USER FILE");
    expect(info.serverHost.fileExists("d:/ws/src/Foo.vue.tsx")).toBe(true);
  });
});
