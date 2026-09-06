import { describe, it, expect, beforeEach, afterEach } from "vitest";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  statSync,
  utimesSync,
  writeFileSync,
  rmSync,
} from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import ts from "typescript";
import init from "./index";
import { isFrameworkAttributeNamePosition } from "./cursorGeometry";
import {
  EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY,
  EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY,
  E2E_PROVIDER_ONLY_COMPLETIONS_CONFIG_KEY,
  EDITOR_TSSERVER_ATTESTATION_CONFIG_KEY,
  type Manifest,
} from "@verter/language-shared";

describe("bounded framework attribute context", () => {
  it("does not inspect a tag opener beyond 256 UTF-8 bytes", () => {
    const within = `<div ${"a".repeat(249)} x`;
    const beyond = `<div ${"é".repeat(126)} x`;
    expect(isFrameworkAttributeNamePosition(within, within.length, [[0, within.length]])).toBe(
      true,
    );
    expect(isFrameworkAttributeNamePosition(beyond, beyond.length, [[0, beyond.length]])).toBe(
      false,
    );
  });
});

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
      "/ws/tsconfig.json": {
        owned_sources: [
          {
            source_uri: "/ws/src/A.vue",
            provider_uri: "/ws/src/A.vue.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
          {
            source_uri: "/ws/src/B.vue",
            provider_uri: "/ws/src/B.vue.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
          {
            source_uri: "/ws/src/W.svelte",
            provider_uri: "/ws/src/W.svelte.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
        ],
        ready_files: {
          "/ws/src/A.vue.tsx": {
            content_hash: "a1",
            version: 5,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "0",
            blob_rel: "blobs/A.vue.tsx",
          },
          "/ws/src/W.svelte.tsx": {
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

function javascriptSvelteManifest(): Manifest {
  const manifest = vueAndSvelteManifest();
  const project = manifest.projects["/ws/tsconfig.json"];
  const owned = project.owned_sources.find((entry) => entry.source_uri.endsWith("/W.svelte"));
  if (!owned) throw new Error("fixture Svelte owner missing");
  owned.provider_uri = "/ws/src/W.svelte.jsx";
  owned.script_kind = "JSX";
  delete project.ready_files["/ws/src/W.svelte.tsx"];
  project.ready_files["/ws/src/W.svelte.jsx"] = {
    content_hash: "wj1",
    version: 3,
    script_kind: "JSX",
    role: "CarrierIde",
    map_hash: "0",
    blob_rel: "blobs/W.svelte.jsx",
  };
  return manifest;
}

// ── a minimal PluginCreateInfo over a real-disk passthrough ───────────────────

interface FakeHostState {
  diskFiles: Record<string, string>;
}

function createInfo(
  storeDir: string | undefined,
  disk: FakeHostState,
  projectName = "/ws/tsconfig.json",
) {
  const logger = { info: () => {}, msg: () => {} };
  const normalize = (f: string) => f.replace(/\\/g, "/");
  const scriptInfos = new Map<string, { fileName: string }>();

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
    getDefinitionAndBoundSpan: (...a: any[]) =>
      languageService.__lsImpl?.getDefinitionAndBoundSpan?.(...a),
    getDefinitionAtPosition: (...a: any[]) =>
      languageService.__lsImpl?.getDefinitionAtPosition?.(...a),
    getTypeDefinitionAtPosition: (...a: any[]) =>
      languageService.__lsImpl?.getTypeDefinitionAtPosition?.(...a),
    getQuickInfoAtPosition: (...a: any[]) =>
      languageService.__lsImpl?.getQuickInfoAtPosition?.(...a),
    getDocumentHighlights: (...a: any[]) => languageService.__lsImpl?.getDocumentHighlights?.(...a),
    getApplicableRefactors: (...a: any[]) =>
      languageService.__lsImpl?.getApplicableRefactors?.(...a) ?? [],
    getEncodedSemanticClassifications: (...a: any[]) =>
      languageService.__lsImpl?.getEncodedSemanticClassifications?.(...a) ?? {
        spans: [],
        endOfLineState: 0,
      },
    getCompletionEntryDetails: (...a: any[]) =>
      languageService.__lsImpl?.getCompletionEntryDetails?.(...a),
    getCompletionsAtPosition: (...a: any[]) =>
      languageService.__lsImpl?.getCompletionsAtPosition?.(...a),
    getSyntacticDiagnostics: (...a: any[]) =>
      languageService.__lsImpl?.getSyntacticDiagnostics?.(...a) ?? [],
    getSemanticDiagnostics: (...a: any[]) =>
      languageService.__lsImpl?.getSemanticDiagnostics?.(...a) ?? [],
    getSuggestionDiagnostics: (...a: any[]) =>
      languageService.__lsImpl?.getSuggestionDiagnostics?.(...a) ?? [],
    // The companion→source RESPONSE-remap surface. The plugin wraps each of
    // these; a test overrides the underlying impl via `__lsImpl` to drive a
    // companion-carrying response through the wrapper.
    getReferencesAtPosition: (...a: any[]) =>
      languageService.__lsImpl?.getReferencesAtPosition?.(...a),
    findReferences: (...a: any[]) => languageService.__lsImpl?.findReferences?.(...a),
    getImplementationAtPosition: (...a: any[]) =>
      languageService.__lsImpl?.getImplementationAtPosition?.(...a),
    getRenameInfo: (...a: any[]) => languageService.__lsImpl?.getRenameInfo?.(...a),
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
    // directory; the store paths (`/ws/...`) are independent of cwd.
    getCurrentDirectory: () => process.cwd(),
    getCompilerOptions: () => ({}),
    getProjectName: () => projectName,
    projectService: {
      logger,
      getScriptInfo: (fileName: string) => scriptInfos.get(normalize(fileName).toLowerCase()),
      getScriptInfoForNormalizedPath: (fileName: string) =>
        scriptInfos.get(normalize(fileName).toLowerCase()),
      getOrCreateScriptInfoForNormalizedPath: (fileName: string) => {
        const normalized = normalize(fileName);
        if (!serverHost.fileExists(normalized)) return undefined;
        const scriptInfo = { fileName: normalized };
        scriptInfos.set(normalized.toLowerCase(), scriptInfo);
        return scriptInfo;
      },
    },
    getRootFiles: () => [],
    getFileNames: () => [],
    containsFile: () => false,
    isClosed: () => false,
    markAsDirty: () => {},
    refreshDiagnostics: () => {},
  };
  const rootScriptInfos = new Set<unknown>();
  project.isRoot = (scriptInfo: unknown) => rootScriptInfos.has(scriptInfo);
  project.addRoot = (scriptInfo: unknown) => {
    rootScriptInfos.add(scriptInfo);
    project.markAsDirty();
  };
  project.removeFile = (scriptInfo: unknown) => {
    rootScriptInfos.delete(scriptInfo);
    project.markAsDirty();
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
beforeEach(async () => {
  dirs = [];
  delete process.env.VERTER_CARRIER_STORE_DIR;
  delete process.env.VERTER_PLUGIN_RESPONSE_REMAP;
  // A real tsserver keeps plugin module state for the life of its process. Each
  // test must therefore restore that state through the public configuration
  // callback, and wait for any coalesced project refresh from the prior test,
  // before installing new spies or environment fallbacks.
  init({ typescript: ts } as any).onConfigurationChanged!({
    carrierStoreDir: undefined,
    responseRemap: true,
  });
  await new Promise<void>((resolve) => setImmediate(resolve));
});
afterEach(() => {
  for (const d of dirs) rmSync(d, { recursive: true, force: true });
  delete process.env.VERTER_CARRIER_STORE_DIR;
  delete process.env.VERTER_PLUGIN_RESPONSE_REMAP;
});
function track(d: string): string {
  dirs.push(d);
  return d;
}

// ── tests ─────────────────────────────────────────────────────────────────

describe("host-proxy matrix: compiler options", () => {
  it("preserves TypeScript internal non-enumerable config metadata while enabling JSX", () => {
    const info = createInfo(undefined, { diskFiles: {} });
    const configFile = { configFileSpecs: { validatedIncludeSpecs: ["src/**/*"] } };
    const settings: Record<string, unknown> = { strict: true };
    Object.defineProperty(settings, "configFile", {
      value: configFile,
      enumerable: false,
      configurable: true,
      writable: true,
    });
    info.languageServiceHost.getCompilationSettings = () => settings;

    init({ typescript: ts } as any).create(info);
    const proxied = info.languageServiceHost.getCompilationSettings();

    expect(proxied).toBe(settings);
    expect(proxied.jsx).toBe(ts.JsxEmit.Preserve);
    expect(proxied.allowJs).toBeUndefined();
    expect(proxied.configFile).toBe(configFile);
    expect(Object.getOwnPropertyDescriptor(proxied, "configFile")?.enumerable).toBe(false);
  });

  it("admits a ready JavaScript carrier without enabling project-wide JS checking", () => {
    const manifest = vueAndSvelteManifest();
    const project = manifest.projects["/ws/tsconfig.json"];
    const owned = project.owned_sources.find((entry) => entry.source_uri.endsWith("/A.vue"))!;
    owned.provider_uri = "/ws/src/A.vue.jsx";
    owned.script_kind = "JSX";
    const ready = project.ready_files["/ws/src/A.vue.tsx"];
    delete project.ready_files["/ws/src/A.vue.tsx"];
    project.ready_files["/ws/src/A.vue.jsx"] = {
      ...ready,
      script_kind: "JSX",
      blob_rel: "blobs/A.vue.jsx",
    };
    const dir = track(writeStore(manifest, { "blobs/A.vue.jsx": "export const x = 1;" }));
    const info = createInfo(dir, { diskFiles: {} });
    const settings: Record<string, unknown> = { strict: true };
    info.languageServiceHost.getCompilationSettings = () => settings;

    init({ typescript: ts } as any).create(info);
    const proxied = info.languageServiceHost.getCompilationSettings();

    expect(proxied).toBe(settings);
    expect(proxied.allowJs).toBe(true);
    expect(proxied.checkJs).toBeUndefined();
  });
});

describe("editor tsserver attestation", () => {
  it("preserves configured-project evidence across plugin factory instances", () => {
    const directory = track(mkdtempSync(join(tmpdir(), "verter-editor-tsserver-test-")));
    const nonce = "0123456789abcdef0123456789abcdef";
    const config = {
      [EDITOR_TSSERVER_ATTESTATION_CONFIG_KEY]: { directory, nonce },
    };

    const configured = init({ typescript: ts } as any);
    configured.create(createInfo(undefined, { diskFiles: {} }, "/ws/tsconfig.json"));
    configured.onConfigurationChanged!(config);

    const inferred = init({ typescript: ts } as any);
    inferred.create(createInfo(undefined, { diskFiles: {} }, "/dev/null/inferredProject1*"));
    inferred.onConfigurationChanged!(config);

    const receiptFile = readdirSync(directory).find((file) => file.endsWith(".json"));
    expect(receiptFile).toBeDefined();
    const receipt = JSON.parse(readFileSync(join(directory, receiptFile!), "utf8"));
    expect(receipt.projects).toContain("/ws/tsconfig.json");
  });

  it("reconfigures the configured project when another factory receives the editor command", () => {
    const directory = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "export const generated = true;",
        "blobs/W.svelte.tsx": "export const generated = true;",
      }),
    );
    const configuredInfo = createInfo(
      undefined,
      { diskFiles: { "/ws/src/A.vue": "<template>raw</template>" } },
      "/ws/tsconfig.json",
    );
    const configured = init({ typescript: ts } as any);
    configured.create(configuredInfo);

    const commandReceiver = init({ typescript: ts } as any);
    commandReceiver.create(createInfo(undefined, { diskFiles: {} }, "/dev/null/inferredProject1*"));
    commandReceiver.onConfigurationChanged!({
      carrierStoreDir: directory,
      activeCarrierSources: ["/ws/src/A.vue"],
    });

    expect(configured.getExternalFiles!(configuredInfo.project, 0 as any)).toContain(
      "/ws/src/A.vue",
    );
    const snapshot = configuredInfo.languageServiceHost.getScriptSnapshot("/ws/src/A.vue");
    expect(snapshot.getText(0, snapshot.getLength())).toBe("export const generated = true;");
  });
});

describe("host-proxy matrix: getScriptSnapshot", () => {
  it("advances only the active managed ScriptInfo from a published store version", async () => {
    const source = "/ws/src/A.vue";
    const raw = "<template>{{ value }}</template>";
    const generated = "const value: string = missingName;\n";
    const cold = vueAndSvelteManifest();
    cold.projects["/ws/tsconfig.json"].ready_files = {};
    const dir = track(writeStore(cold, {}));
    const info = createInfo(dir, { diskFiles: { [source]: raw } });
    let scriptText = raw;
    let editCount = 0;
    const scriptInfo = {
      fileName: source,
      isScriptOpen: () => true,
      getSnapshot: () => ts.ScriptSnapshot.fromString(scriptText),
      editContent: (start: number, end: number, text: string) => {
        scriptText = scriptText.slice(0, start) + text + scriptText.slice(end);
        editCount += 1;
      },
      reloadFromFile: () => false,
      positionToLineOffset: (position: number) => ({ line: 1, offset: position + 1 }),
      lineOffsetToPosition: (_line: number, offset: number) => offset - 1,
    };
    info.project.projectService.getScriptInfo = (fileName: string) =>
      fileName === source ? scriptInfo : undefined;
    info.project.projectService.getOrCreateScriptInfoForNormalizedPath = (fileName: string) =>
      fileName === source ? scriptInfo : undefined;
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    const ready = vueAndSvelteManifest();
    ready.epoch = 2;
    writeFileSync(join(dir, "blobs/A.vue.tsx"), generated, "utf8");
    writeFileSync(join(dir, "blobs/W.svelte.tsx"), "export {};\n", "utf8");
    writeFileSync(join(dir, "manifest.json"), JSON.stringify(ready), "utf8");
    plugin.onConfigurationChanged!({
      carrierStoreDir: dir,
      carrierStoreRefreshToken: 1,
      activeCarrierSources: [source],
    });
    await new Promise<void>((resolve) => setImmediate(resolve));

    expect(scriptText).toBe(generated);
    expect(editCount).toBe(1);
  });

  it("serializes managed carrier protocol positions against the current generated version", async () => {
    const source = "/ws/src/A.vue";
    const raw = "<template>{{ value }}</template>";
    const generated = ["/* generated preamble */", "const value = 1;", "unknownValue;", ""].join(
      "\n",
    );
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": generated,
      }),
    );
    const info = createInfo(dir, { diskFiles: { [source]: raw } });
    const scriptInfo = {
      fileName: source,
      positionToLineOffset: (position: number) => ({ line: 1, offset: position + 1 }),
      lineOffsetToPosition: (_line: number, offset: number) => offset - 1,
    };
    info.project.projectService.getScriptInfo = (fileName: string) =>
      fileName === source ? scriptInfo : undefined;

    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    const snapshot = info.languageServiceHost.getScriptSnapshot(source);
    expect(snapshot.getText(0, snapshot.getLength())).toBe(generated);
    const generatedOffset = generated.indexOf("unknownValue");
    expect(scriptInfo.positionToLineOffset(generatedOffset)).toEqual({ line: 3, offset: 1 });
    expect(scriptInfo.lineOffsetToPosition(3, 1)).toBe(generatedOffset);

    const nextGenerated = "/* v2 */\r\nconst astral = '𝕏';\r\nnextUnknown;\r\n";
    const nextManifest = vueAndSvelteManifest();
    nextManifest.epoch = 2;
    nextManifest.projects["/ws/tsconfig.json"].ready_files[`${source}.tsx`] = {
      content_hash: "a2",
      version: 6,
      script_kind: "TSX",
      role: "CarrierIde",
      map_hash: "0",
      blob_rel: "blobs/A-v2.vue.tsx",
    };
    writeFileSync(join(dir, "blobs/A-v2.vue.tsx"), nextGenerated, "utf8");
    writeFileSync(join(dir, "manifest.json"), JSON.stringify(nextManifest), "utf8");
    plugin.onConfigurationChanged!({
      carrierStoreDir: dir,
      carrierStoreRefreshToken: 2,
    });
    await new Promise<void>((resolve) => setImmediate(resolve));

    const nextOffset = nextGenerated.indexOf("nextUnknown");
    expect(scriptInfo.positionToLineOffset(nextOffset)).toEqual({ line: 3, offset: 1 });
    expect(scriptInfo.lineOffsetToPosition(3, 1)).toBe(nextOffset);
  });

  it("delegates companion snapshot and version requests through the project host lifecycle", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "export const A = 1;",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    const snapshotRequests: string[] = [];
    const versionRequests: string[] = [];
    info.languageServiceHost.getScriptSnapshot = (fileName: string) => {
      snapshotRequests.push(fileName);
      return undefined;
    };
    info.languageServiceHost.getScriptVersion = (fileName: string) => {
      versionRequests.push(fileName);
      return "host-version";
    };
    init({ typescript: ts } as any).create(info);

    const companion = "/ws/src/A.vue.tsx";
    const snapshot = info.languageServiceHost.getScriptSnapshot(companion);
    const version = info.languageServiceHost.getScriptVersion(companion);

    expect(snapshotRequests).toEqual([companion]);
    expect(versionRequests).toEqual([companion]);
    expect(snapshot.getText(0, snapshot.getLength())).toBe("export const A = 1;");
    expect(version).toBe("5:a1");
  });

  it("serves the ready Vue companion blob", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), { "blobs/A.vue.tsx": "export const A = 1; // vue" }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    const snap = info.languageServiceHost.getScriptSnapshot("/ws/src/A.vue.tsx");
    expect(snap).toBeDefined();
    expect(snap.getText(0, snap.getLength())).toBe("export const A = 1; // vue");
  });

  it("serves one source-map-stable owner-bound Vue JSX authority to every host read", () => {
    const owner = track(mkdtempSync(join(tmpdir(), "verter-plugin-vue-jsx-owner-")));
    const normalize = (fileName: string) => fileName.replace(/\\/g, "/");
    const project = normalize(join(owner, "tsconfig.json"));
    const source = normalize(join(owner, "src", "A.vue"));
    const provider = `${source}.tsx`;
    const vue = join(owner, "node_modules", "vue");
    mkdirSync(join(vue, "jsx-runtime"), { recursive: true });
    writeFileSync(
      join(vue, "package.json"),
      JSON.stringify({
        name: "vue",
        version: "3.5.40",
        exports: { "./jsx-runtime": { types: "./jsx-runtime/index.d.ts" } },
      }),
      "utf8",
    );
    writeFileSync(
      join(vue, "jsx-runtime", "index.d.ts"),
      "export namespace JSX { interface Element {} interface ElementClass { $props: {} } interface ElementAttributesProperty { $props: {} } interface IntrinsicElements { div: { class?: string } } interface IntrinsicAttributes {} }\n",
      "utf8",
    );
    const tail = 'const view = <div class="card">ok</div>;\n';
    const compilerBytes = `/** @jsxImportSource vue */\n${tail}`;
    const manifest: Manifest = {
      epoch: 1,
      host_version: "test",
      projects: {
        [project]: {
          owned_sources: [
            {
              source_uri: source,
              provider_uri: provider,
              role: "CarrierIde",
              script_kind: "TSX",
            },
          ],
          ready_files: {
            [provider]: {
              content_hash: "vue-jsx",
              version: 1,
              script_kind: "TSX",
              role: "CarrierIde",
              map_hash: "map",
              blob_rel: "blobs/A.vue.tsx",
            },
          },
        },
      },
    };
    const dir = track(writeStore(manifest, { "blobs/A.vue.tsx": compilerBytes }));
    const info = createInfo(dir, { diskFiles: {} }, project);
    init({ typescript: ts } as any).create(info);

    const snapshot = info.languageServiceHost.getScriptSnapshot(provider);
    const snapshotText = snapshot.getText(0, snapshot.getLength());
    const serverText = info.serverHost.readFile(provider);

    expect(snapshotText).toBe(serverText);
    expect(snapshotText).toMatch(/^\/\*\* @jsxRuntime classic \*\//u);
    expect(snapshotText).not.toContain("@jsxImportSource vue");
    expect(snapshotText.split("\n").slice(1).join("\n")).toBe(tail);
    expect(snapshotText.split("\n")).toHaveLength(compilerBytes.split("\n").length);
  });

  it("serves the ready Svelte companion blob", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), { "blobs/W.svelte.tsx": "export const W = 1; // svelte" }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    const snap = info.languageServiceHost.getScriptSnapshot("/ws/src/W.svelte.tsx");
    expect(snap.getText(0, snap.getLength())).toBe("export const W = 1; // svelte");
  });

  it("falls through to disk for a non-companion path", () => {
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: { "/ws/src/plain.ts": "const x = 1;" } });
    init({ typescript: ts } as any).create(info);

    const snap = info.languageServiceHost.getScriptSnapshot("/ws/src/plain.ts");
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

    expect(info.languageServiceHost.getScriptVersion("/ws/src/A.vue.tsx")).toBe("5:a1");
    expect(info.languageServiceHost.getScriptVersion("/ws/src/W.svelte.tsx")).toBe("2:w1");
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

    expect(info.languageServiceHost.getScriptKind("/ws/src/A.vue.tsx")).toBe(ts.ScriptKind.TSX);
    expect(info.languageServiceHost.getScriptKind("/ws/src/W.svelte.tsx")).toBe(ts.ScriptKind.TSX);
  });

  it("falls through to the disk version/kind for a non-companion", () => {
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: { "/ws/src/plain.ts": "x" } });
    init({ typescript: ts } as any).create(info);
    expect(info.languageServiceHost.getScriptVersion("/ws/src/plain.ts")).toBe("disk-0");
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

    expect(info.serverHost.readFile("/ws/src/A.vue.tsx")).toBe("vue-content");
    expect(info.serverHost.readFile("/ws/src/W.svelte.tsx")).toBe("svelte-content");
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

    expect(info.serverHost.fileExists("/ws/src/A.vue.tsx")).toBe(true);
    expect(info.serverHost.fileExists("/ws/src/W.svelte.tsx")).toBe(true);
    expect(info.serverHost.fileExists("/ws/src/Nope.vue.tsx")).toBe(false);
  });

  it("readFile/fileExists fall through to real disk for a non-companion", () => {
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: { "/ws/src/real.ts": "real" } });
    init({ typescript: ts } as any).create(info);

    expect(info.serverHost.readFile("/ws/src/real.ts")).toBe("real");
    expect(info.serverHost.fileExists("/ws/src/real.ts")).toBe(true);
  });
});

describe("host-proxy matrix: resolveModuleNameLiterals (in-project → public-API carrier)", () => {
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

  it("ABSTAINS for a relative .vue import when the project owns no public-API carrier", () => {
    // The fixture owns only the `CarrierIde` role. An ordinary import must never
    // be handed the JSX/TSX editor surface (that is the `--jsx is not set`
    // defect); with no import surface owned the plugin fails CLOSED and lets
    // TypeScript's own resolution answer, so the later publication can heal it.
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    const resolved = resolveOne(info, "./A.vue", "/ws/src/consumer.ts");
    expect(resolved).toBeUndefined();
  });

  it("ABSTAINS for a relative .svelte import when the project owns no public-API carrier", () => {
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    const resolved = resolveOne(info, "./W.svelte", "/ws/src/consumer.ts");
    expect(resolved).toBeUndefined();
  });

  it("redirects a relative .vue import to its public API carrier when both roles exist", () => {
    const manifest = vueAndSvelteManifest();
    const project = manifest.projects["/ws/tsconfig.json"];
    project.owned_sources.push({
      source_uri: "/ws/src/A.vue",
      provider_uri: "/ws/src/A.vue.verter.ts",
      role: "CarrierApi",
      script_kind: "TS",
    });
    project.ready_files["/ws/src/A.vue.verter.ts"] = {
      content_hash: "aa1",
      version: 6,
      script_kind: "TS",
      role: "CarrierApi",
      map_hash: "0",
      blob_rel: "blobs/A.vue.verter.ts",
    };
    const dir = track(
      writeStore(manifest, { "blobs/A.vue.verter.ts": "export default class A {}" }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    const result = info.languageServiceHost.resolveModuleNameLiterals(
      [{ text: "./A.vue" }],
      "/ws/src/consumer.ts",
      undefined,
      {},
      undefined,
    )[0]?.resolvedModule;

    expect(result?.resolvedFileName).toBe("/ws/src/A.vue.verter.ts");
    expect(result?.extension).toBe(ts.Extension.Ts);
  });

  it("redirects a Svelte import to its public API carrier when both roles exist", () => {
    const manifest = javascriptSvelteManifest();
    const project = manifest.projects["/ws/tsconfig.json"];
    project.owned_sources.push({
      source_uri: "/ws/src/W.svelte",
      provider_uri: "/ws/src/W.svelte.verter.ts",
      role: "CarrierApi",
      script_kind: "TS",
    });
    project.ready_files["/ws/src/W.svelte.verter.ts"] = {
      content_hash: "wa1",
      version: 4,
      script_kind: "TS",
      role: "CarrierApi",
      map_hash: "0",
      blob_rel: "blobs/W.svelte.verter.ts",
    };
    const dir = track(
      writeStore(manifest, {
        "blobs/W.svelte.verter.ts": "export default class W { declare $props: { label: string } }",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    const result = info.languageServiceHost.resolveModuleNameLiterals(
      [{ text: "./W.svelte" }],
      "/ws/src/consumer.ts",
      undefined,
      {},
      undefined,
    )[0]?.resolvedModule;

    expect(result?.resolvedFileName).toBe("/ws/src/W.svelte.verter.ts");
    expect(result?.extension).toBe(ts.Extension.Ts);
    const snapshot = info.languageServiceHost.getScriptSnapshot(result!.resolvedFileName);
    expect(snapshot.getText(0, snapshot.getLength())).toBe(
      "export default class W { declare $props: { label: string } }",
    );

    const selfImport = info.languageServiceHost.resolveModuleNameLiterals(
      [{ text: "./W.svelte.verter.js" }],
      "/ws/src/W.svelte.tsx",
      undefined,
      {},
      undefined,
    )[0]?.resolvedModule;
    expect(selfImport?.resolvedFileName).toBe("/ws/src/W.svelte.verter.ts");
    expect(selfImport?.extension).toBe(ts.Extension.Ts);
  });

  it("resolves the Svelte projection JSX runtime from the plugin-owned package", () => {
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    const resolved = resolveOne(info, "@verter/svelte-jsx/jsx-runtime", "/ws/src/W.svelte.tsx");

    expect(resolved?.replace(/\\/g, "/")).toMatch(
      /\/node_modules\/@verter\/svelte-jsx\/jsx-runtime\.d\.ts$/,
    );
    expect(ts.sys.fileExists(resolved!)).toBe(true);
  });

  it("resolves the plugin-owned JSX runtime's Svelte imports from the owner workspace", () => {
    const owner = track(mkdtempSync(join(tmpdir(), "verter-svelte-owner-")));
    const svelte = join(owner, "node_modules", "svelte");
    mkdirSync(svelte, { recursive: true });
    writeFileSync(
      join(svelte, "package.json"),
      JSON.stringify({
        name: "svelte",
        types: "./index.d.ts",
        exports: {
          ".": { types: "./index.d.ts" },
          "./elements": { types: "./elements.d.ts" },
        },
      }),
      "utf8",
    );
    writeFileSync(join(svelte, "index.d.ts"), "export interface Snippet {}\n", "utf8");
    writeFileSync(
      join(svelte, "elements.d.ts"),
      "export interface SvelteHTMLElements { div: Record<string, unknown> }\n",
      "utf8",
    );

    const info = createInfo(undefined, { diskFiles: {} }, join(owner, "tsconfig.json"));
    info.project.getCurrentDirectory = () => owner;
    info.project.getCompilerOptions = () => ({
      module: ts.ModuleKind.ESNext,
      moduleResolution: ts.ModuleResolutionKind.Bundler,
    });
    info.serverHost.fileExists = ts.sys.fileExists;
    info.serverHost.readFile = ts.sys.readFile;
    info.serverHost.directoryExists = ts.sys.directoryExists;
    info.serverHost.getDirectories = ts.sys.getDirectories;
    info.serverHost.realpath = ts.sys.realpath;
    const priorCwd = process.cwd();
    init({ typescript: ts } as any).create(info);
    process.chdir(priorCwd);

    const runtime = resolveOne(
      info,
      "@verter/svelte-jsx/jsx-runtime",
      join(owner, "src", "W.svelte.tsx"),
    );
    expect(runtime).toBeDefined();
    const resolved = resolveOne(info, "svelte/elements", runtime!);

    expect(resolved?.replace(/\\/g, "/").toLowerCase()).toBe(
      join(svelte, "elements.d.ts").replace(/\\/g, "/").toLowerCase(),
    );
  });

  it("never hands a JavaScript .svelte carrier's JSX surface to an ordinary importer", () => {
    const dir = track(writeStore(javascriptSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    const result = info.languageServiceHost.resolveModuleNameLiterals(
      [{ text: "./W.svelte" }],
      "/ws/src/consumer.ts",
      undefined,
      {},
      undefined,
    )[0]?.resolvedModule;
    expect(result).toBeUndefined();
  });

  it("resolves a JavaScript .svelte carrier to its TS import surface, not its .jsx carrier", () => {
    const manifest = javascriptSvelteManifest();
    const project = manifest.projects["/ws/tsconfig.json"];
    project.owned_sources.push({
      source_uri: "/ws/src/W.svelte",
      provider_uri: "/ws/src/W.svelte.verter.ts",
      role: "CarrierApi",
      script_kind: "TS",
    });
    project.ready_files["/ws/src/W.svelte.verter.ts"] = {
      content_hash: "wj2",
      version: 4,
      script_kind: "TS",
      role: "CarrierApi",
      map_hash: "0",
      blob_rel: "blobs/W.svelte.verter.ts",
    };
    const dir = track(
      writeStore(manifest, { "blobs/W.svelte.verter.ts": "export default class W {}" }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    const result = info.languageServiceHost.resolveModuleNameLiterals(
      [{ text: "./W.svelte" }],
      "/ws/src/consumer.ts",
      undefined,
      {},
      undefined,
    )[0]?.resolvedModule;
    // The carrier's SOURCE dialect is an editing concern; every consumer sees
    // one TypeScript import surface.
    expect(result?.resolvedFileName).toBe("/ws/src/W.svelte.verter.ts");
    expect(result?.extension).toBe(ts.Extension.Ts);
  });

  it("never hands a JavaScript .vue carrier's manifest-owned JSX carrier to an importer", () => {
    const manifest = vueAndSvelteManifest();
    const project = manifest.projects["/ws/tsconfig.json"];
    const owned = project.owned_sources.find((entry) => entry.source_uri.endsWith("/A.vue"))!;
    owned.provider_uri = "/ws/src/A.vue.jsx";
    owned.script_kind = "JSX";
    const ready = project.ready_files["/ws/src/A.vue.tsx"];
    delete project.ready_files["/ws/src/A.vue.tsx"];
    project.ready_files["/ws/src/A.vue.jsx"] = {
      ...ready,
      script_kind: "JSX",
      blob_rel: "blobs/A.vue.jsx",
    };
    const dir = track(writeStore(manifest, { "blobs/A.vue.jsx": "export default {};" }));
    const info = createInfo(dir, { diskFiles: { "/ws/src/A.vue": "<template />" } });
    init({ typescript: ts } as any).create(info);

    const result = info.languageServiceHost.resolveModuleNameLiterals(
      [{ text: "./A.vue" }],
      "/ws/src/consumer.ts",
      undefined,
      undefined,
      undefined,
    );

    expect(result[0]?.resolvedModule).toBeUndefined();
  });

  it("leaves a plain relative .ts import to TS's own resolution", () => {
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    // Non-carrier specifier: the override returns undefined → delegate result.
    const resolved = resolveOne(info, "./plain", "/ws/src/consumer.ts");
    expect(resolved).toBeUndefined();
  });

  it("prefers an installed @verter/types package whose declarations use the package types entry", () => {
    const owner = track(mkdtempSync(join(tmpdir(), "verter-types-owner-")));
    const packageRoot = join(owner, "node_modules", "@verter", "types");
    const declarations = join(packageRoot, "dist", "index.d.ts");
    mkdirSync(join(packageRoot, "dist"), { recursive: true });
    writeFileSync(
      join(packageRoot, "package.json"),
      JSON.stringify({ name: "@verter/types", types: "./dist/index.d.ts" }),
      "utf8",
    );
    writeFileSync(declarations, "export type InstalledMarker = true;\n", "utf8");

    const info = createInfo(undefined, { diskFiles: {} }, join(owner, "tsconfig.json"));
    info.project.getCurrentDirectory = () => owner;
    info.project.getCompilerOptions = () => ({
      module: ts.ModuleKind.ESNext,
      moduleResolution: ts.ModuleResolutionKind.Bundler,
    });
    info.serverHost.fileExists = ts.sys.fileExists;
    info.serverHost.readFile = ts.sys.readFile;
    info.serverHost.directoryExists = ts.sys.directoryExists;
    info.serverHost.getDirectories = ts.sys.getDirectories;
    info.serverHost.realpath = ts.sys.realpath;
    info.languageServiceHost.resolveModuleNameLiterals = (
      literals: readonly ts.StringLiteralLike[],
      containingFile: string,
    ) =>
      literals.map(({ text }) => ({
        resolvedModule: ts.resolveModuleName(
          text,
          containingFile,
          info.project.getCompilerOptions(),
          ts.sys,
        ).resolvedModule,
      }));

    const priorCwd = process.cwd();
    init({ typescript: ts } as any).create(info);
    process.chdir(priorCwd);

    const resolved = resolveOne(info, "@verter/types", join(owner, "src", "App.vue.tsx"));
    expect(resolved?.replace(/\\/g, "/").toLowerCase()).toBe(
      declarations.replace(/\\/g, "/").toLowerCase(),
    );
    expect(info.serverHost.readFile(resolved!)).toContain("InstalledMarker");
  });

  it("preserves the project host's authoritative @verter/types resolution", () => {
    const info = createInfo(undefined, { diskFiles: {} });
    const projectResolution = "/pnp/@verter/types/index.d.ts";
    info.languageServiceHost.resolveModuleNameLiterals = (
      literals: readonly ts.StringLiteralLike[],
    ) =>
      literals.map(({ text }) => ({
        resolvedModule:
          text === "@verter/types"
            ? {
                resolvedFileName: projectResolution,
                extension: ts.Extension.Dts,
                isExternalLibraryImport: true,
              }
            : undefined,
      }));

    init({ typescript: ts } as any).create(info);

    expect(resolveOne(info, "@verter/types", "/ws/src/App.vue.tsx")).toBe(projectResolution);
  });
});

describe("getExternalFiles carrier working-set ownership", () => {
  it("advertises only active ready framework sources, never warmed storage identities", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "x",
        "blobs/W.svelte.tsx": "y",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    info.config = {
      carrierStoreDir: dir,
      activeCarrierSources: ["/ws/src/A.vue"],
    };
    const plugin = init({ typescript: ts } as any);
    // `create` records the store dir for the project before getExternalFiles.
    plugin.create(info);

    // Match official Volar/Svelte host architecture: the authored working set
    // owns Program roots. W.svelte is ready in the background store but is not
    // active and must not inflate this configured project.
    const files = plugin.getExternalFiles!(info.project, 0 as any);
    expect(files).toEqual(["/ws/src/A.vue"]);
    expect(files).not.toContain("/ws/src/W.svelte");
    // B.vue is owned but not ready, so neither identity may be advertised.
    expect(files).not.toContain("/ws/src/B.vue");
    expect(files).not.toContain("/ws/src/B.vue.tsx");
  });

  it("returns [] when the store is unavailable", () => {
    const info = createInfo(undefined, { diskFiles: {} });
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);
    expect(plugin.getExternalFiles!(info.project, 0 as any)).toEqual([]);
  });

  it("does not turn background-ready carriers into configured-project roots", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "export const A = 1;",
        "blobs/W.svelte.tsx": "export const W = 1;",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    info.config = {
      carrierStoreDir: dir,
      activeCarrierSources: [],
    };
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    expect(plugin.getExternalFiles!(info.project, 0 as any)).toEqual([]);
  });

  it("collapses case-variant spellings of one identity on a case-insensitive host", () => {
    // A store whose owned sources spell ONE carrier source with two different
    // casings (e.g. a root segment cased differently by two producers — the
    // drive-letter-case flavour of this on Windows). On a case-insensitive
    // host both raw strings name the SAME document identity; advertising both
    // would hand TS6 two external-file roots for one identity — the
    // double-identity hazard this function exists to avoid.
    const manifest = vueAndSvelteManifest();
    manifest.projects["/ws/tsconfig.json"].owned_sources.push({
      source_uri: "/WS/src/A.vue",
      provider_uri: "/ws/src/A.vue.tsx",
      role: "CarrierIde",
      script_kind: "TSX",
    });
    const dir = track(writeStore(manifest, { "blobs/A.vue.tsx": "x" }));
    const info = createInfo(dir, { diskFiles: {} });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      activeCarrierSources: ["/ws/src/A.vue"],
    };
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    // `canonical()` keys on ts.sys.useCaseSensitiveFileNames — force the
    // case-insensitive host semantics the dedupe must respect.
    const sys = ts.sys as { useCaseSensitiveFileNames?: boolean };
    const original = sys.useCaseSensitiveFileNames;
    sys.useCaseSensitiveFileNames = false;
    try {
      const files = plugin.getExternalFiles!(info.project, 0 as any);
      const aIdentities = files.filter((file) => file.toLowerCase() === "/ws/src/a.vue.tsx");
      expect(aIdentities).toEqual(["/ws/src/A.vue.tsx"]);
    } finally {
      sys.useCaseSensitiveFileNames = original;
    }
  });

  // @ai-generated - Plugin externals become Program files after the first graph
  // build; that must not make the next getExternalFiles call retract them.
  it("keeps non-root carrier externals stable across repeated project graph reads", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "export const A = 1;",
        "blobs/W.svelte.tsx": "export const W = 1;",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    info.config = {
      carrierStoreDir: dir,
      activeCarrierSources: ["/ws/src/A.vue", "/ws/src/W.svelte"],
    };
    let graphRoots: string[] = [];
    info.project.getRootFiles = () => graphRoots;
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);
    plugin.onConfigurationChanged!(info.config);

    const first = plugin.getExternalFiles!(info.project, 0 as any).sort();
    graphRoots = [...first];
    const second = plugin.getExternalFiles!(info.project, 0 as any).sort();
    graphRoots = [...second];
    const third = plugin.getExternalFiles!(info.project, 0 as any).sort();

    expect(first).toEqual(["/ws/src/A.vue", "/ws/src/W.svelte"]);
    expect(second).toEqual(first);
    expect(third).toEqual(first);
  });

  it("advertises only distinct companion roots when the editor owns source membership", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "export const A = 1;",
        "blobs/W.svelte.tsx": "export const W = 1;",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      activeCarrierSources: ["/ws/src/A.vue", "/ws/src/W.svelte"],
    };
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    expect(plugin.getExternalFiles!(info.project, 0 as any)).toEqual([
      "/ws/src/A.vue.tsx",
      "/ws/src/W.svelte.tsx",
    ]);
    const raw = info.languageServiceHost.getScriptSnapshot("/ws/src/A.vue");
    expect(raw).toBeUndefined();
    const companion = info.languageServiceHost.getScriptSnapshot("/ws/src/A.vue.tsx");
    expect(companion.getText(0, companion.getLength())).toBe("export const A = 1;");
  });

  it("assigns raw-source request membership to its exact configured companion project", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "export const A = 1;",
        "blobs/W.svelte.tsx": "export const W = 1;",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      activeCarrierSources: ["/ws/src/A.vue"],
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    init({ typescript: ts } as any).create(info);

    // TypeScript's cross-project references/rename coordinator checks project
    // membership before invoking a plugin provider. The configured companion
    // owner is the one project that can answer the source request; no sibling
    // inferred project may claim the same virtual request membership.
    expect(info.project.containsFile("/ws/src/A.vue")).toBe(true);
    expect(info.project.containsFile("/ws/src/Unknown.vue")).toBe(false);
  });

  it("does not infer source-feature ownership from carrier membership", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "export const A = 1;",
        "blobs/W.svelte.tsx": "export const W = 1;",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      activeCarrierSources: ["/ws/src/A.vue"],
    };
    init({ typescript: ts } as any).create(info);

    expect(info.project.containsFile("/ws/src/A.vue")).toBe(false);
    expect(init({ typescript: ts } as any).getExternalFiles(info.project as any)).toContain(
      "/ws/src/A.vue.tsx",
    );
  });

  it("does not invalidate a project when configuration changes no serving state", async () => {
    const info = createInfo(undefined, { diskFiles: {} });
    // The plugin configuration is process-scoped because tsserver can create a
    // separate module factory per project. Establish this project's serving
    // baseline explicitly so the assertion is independent of earlier factories.
    info.config = { carrierStoreDir: undefined, responseRemap: true };
    let dirty = 0;
    let diagnostics = 0;
    info.project.markAsDirty = () => dirty++;
    info.project.refreshDiagnostics = () => diagnostics++;
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    plugin.onConfigurationChanged!({ attestationNonce: "receipt-only" });
    await new Promise<void>((resolve) => setImmediate(resolve));

    expect(dirty).toBe(0);
    expect(diagnostics).toBe(0);
  });

  it("does not rebuild or diagnose for an inactive background carrier publication", async () => {
    const initial = vueAndSvelteManifest();
    const dir = track(
      writeStore(initial, {
        "blobs/A.vue.tsx": "export const active = true;",
        "blobs/W.svelte.tsx": "export const warmed = 1;",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      activeCarrierSources: ["/ws/src/A.vue"],
      carrierStoreRefreshToken: 1,
    };
    let dirty = 0;
    let diagnostics = 0;
    info.project.markAsDirty = () => dirty++;
    info.project.refreshDiagnostics = () => diagnostics++;
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    const next = vueAndSvelteManifest();
    next.epoch = initial.epoch + 1;
    next.projects["/ws/tsconfig.json"].ready_files["/ws/src/W.svelte.tsx"] = {
      ...next.projects["/ws/tsconfig.json"].ready_files["/ws/src/W.svelte.tsx"],
      content_hash: "warmed-2",
      version: 2,
    };
    writeFileSync(join(dir, "blobs/W.svelte.tsx"), "export const warmed = 2;", "utf8");
    writeFileSync(join(dir, "manifest.json"), JSON.stringify(next), "utf8");
    plugin.onConfigurationChanged!({
      ...info.config,
      carrierStoreRefreshToken: 2,
    });
    await new Promise<void>((resolve) => setImmediate(resolve));

    expect(dirty).toBe(0);
    expect(diagnostics).toBe(0);
    expect(plugin.getExternalFiles!(info.project, 0 as any)).toEqual(["/ws/src/A.vue.tsx"]);
  });

  it("recreates a missing active managed root when the recovery token advances", async () => {
    const source = "/ws/src/A.vue";
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "export const active = true;",
        "blobs/W.svelte.tsx": "export const warmed = true;",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    info.config = {
      carrierStoreDir: dir,
      activeCarrierSources: [source],
      carrierStoreRefreshToken: 1,
    };

    let scriptInfo: { fileName: string } | undefined = { fileName: source };
    const roots = new Set<unknown>([scriptInfo]);
    let added = 0;
    info.project.projectService.getScriptInfo = (fileName: string) =>
      fileName.replace(/\\/g, "/") === source ? scriptInfo : undefined;
    info.project.projectService.getOrCreateScriptInfoForNormalizedPath = (fileName: string) => {
      if (fileName.replace(/\\/g, "/") !== source) return undefined;
      scriptInfo = { fileName: source };
      return scriptInfo;
    };
    info.project.isRoot = (candidate: unknown) => roots.has(candidate);
    info.project.addRoot = (candidate: unknown) => {
      roots.add(candidate);
      added += 1;
    };

    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    // Model tsserver dropping the virtual ScriptInfo while the durable store,
    // active working set, and manifest version remain unchanged.
    roots.clear();
    scriptInfo = undefined;
    plugin.onConfigurationChanged!({
      ...info.config,
      carrierStoreRefreshToken: 2,
    });
    await new Promise<void>((resolve) => setImmediate(resolve));

    expect(scriptInfo).toBeDefined();
    expect(roots.has(scriptInfo)).toBe(true);
    expect(added).toBe(1);
  });

  it("rebinds an already-created editor project without refreshing inside the configure request", async () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "x",
        "blobs/W.svelte.tsx": "y",
      }),
    );
    const info = createInfo(undefined, { diskFiles: {} });
    let dirty = 0;
    let diagnostics = 0;
    info.project.markAsDirty = () => dirty++;
    info.project.refreshDiagnostics = () => diagnostics++;
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);
    expect(plugin.getExternalFiles!(info.project, 0 as any)).toEqual([]);

    plugin.onConfigurationChanged!({
      carrierStoreDir: dir,
      activeCarrierSources: ["/ws/src/A.vue", "/ws/src/W.svelte"],
    });

    expect(plugin.getExternalFiles!(info.project, 0 as any).sort()).toEqual([
      "/ws/src/A.vue",
      "/ws/src/W.svelte",
    ]);
    const snap = info.languageServiceHost.getScriptSnapshot("/ws/src/A.vue");
    expect(snap.getText(0, snap.getLength())).toBe("x");
    expect(dirty).toBe(0);
    expect(diagnostics).toBe(0);

    await new Promise<void>((resolve) => setImmediate(resolve));

    expect(dirty).toBe(2);
    expect(diagnostics).toBe(0);
  });

  it("reconciles store companions as explicit configured-project roots", async () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), {
        "blobs/A.vue.tsx": "x",
        "blobs/W.svelte.tsx": "y",
      }),
    );
    const info = createInfo(undefined, { diskFiles: {} });
    info.project.getConfigFilePath = () => "/ws/tsconfig.json";
    const addedRoots: string[] = [];
    const roots = new Set<{ fileName: string }>();
    const removedRoots: string[] = [];
    info.project.isRoot = (scriptInfo: { fileName: string }) => roots.has(scriptInfo);
    info.project.addRoot = (scriptInfo: { fileName: string }) => {
      roots.add(scriptInfo);
      addedRoots.push(scriptInfo.fileName);
    };
    info.project.removeFile = (scriptInfo: { fileName: string }) => {
      roots.delete(scriptInfo);
      removedRoots.push(scriptInfo.fileName);
    };
    let targetedReloads = 0;
    let globalReloads = 0;
    info.project.projectService.reloadFileNamesOfConfiguredProject = (project: unknown) => {
      expect(project).toBe(info.project);
      targetedReloads++;
      return true;
    };
    info.project.projectService.reloadProjects = () => globalReloads++;
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    const activeCarrierSources = ["/ws/src/A.vue", "/ws/src/W.svelte"];
    plugin.onConfigurationChanged!({ carrierStoreDir: dir, activeCarrierSources });
    expect(targetedReloads).toBe(0);

    await new Promise<void>((resolve) => setImmediate(resolve));

    expect(addedRoots.sort()).toEqual(["/ws/src/A.vue", "/ws/src/W.svelte"]);
    expect(targetedReloads).toBe(0);
    expect(globalReloads).toBe(0);

    const retracted = vueAndSvelteManifest();
    retracted.epoch = 2;
    retracted.projects["/ws/tsconfig.json"].ready_files = {};
    writeFileSync(join(dir, "manifest.json"), JSON.stringify(retracted), "utf8");
    plugin.onConfigurationChanged!({
      carrierStoreDir: dir,
      carrierStoreRefreshToken: 1,
      activeCarrierSources,
    });
    await new Promise<void>((resolve) => setImmediate(resolve));

    expect(removedRoots.sort()).toEqual(["/ws/src/A.vue", "/ws/src/W.svelte"]);
    expect(targetedReloads).toBe(0);
    expect(globalReloads).toBe(0);
  });

  it("activates a carrier published after the working-set signal without a protocol open", async () => {
    const manifest = vueAndSvelteManifest();
    manifest.projects["/ws/tsconfig.json"].ready_files = {};
    const dir = track(writeStore(manifest, {}));
    const info = createInfo(undefined, { diskFiles: {} });
    info.project.getConfigFilePath = () => "/ws/tsconfig.json";
    const addedRoots: string[] = [];
    const roots = new Set<{ fileName: string }>();
    info.project.isRoot = (scriptInfo: { fileName: string }) => roots.has(scriptInfo);
    info.project.addRoot = (scriptInfo: { fileName: string }) => {
      roots.add(scriptInfo);
      addedRoots.push(scriptInfo.fileName);
    };
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    const activeCarrierSources = ["/ws/src/A.vue"];
    plugin.onConfigurationChanged!({ carrierStoreDir: dir, activeCarrierSources });
    await new Promise<void>((resolve) => setImmediate(resolve));
    expect(addedRoots).toEqual([]);

    const published = vueAndSvelteManifest();
    published.epoch = 2;
    writeFileSync(join(dir, "blobs/A.vue.tsx"), "export const A = 1;", "utf8");
    writeFileSync(join(dir, "blobs/W.svelte.tsx"), "export const W = 1;", "utf8");
    writeFileSync(join(dir, "manifest.json"), JSON.stringify(published), "utf8");
    plugin.onConfigurationChanged!({
      carrierStoreDir: dir,
      carrierStoreRefreshToken: 1,
      activeCarrierSources,
    });
    await new Promise<void>((resolve) => setImmediate(resolve));

    expect(addedRoots).toEqual(["/ws/src/A.vue"]);
    expect(plugin.getExternalFiles!(info.project, 0 as any)).toEqual(["/ws/src/A.vue"]);
  });

  it("reloads only the changed companion snapshot when a publication advances", async () => {
    const initial = vueAndSvelteManifest();
    const dir = track(
      writeStore(initial, {
        "blobs/A.vue.tsx": "export const value = 'stale';",
        "blobs/W.svelte.tsx": "export const generated = true;",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} });
    info.config = {
      carrierStoreDir: dir,
      carrierStoreRefreshToken: 1,
    };
    info.project.getConfigFilePath = () => "/ws/tsconfig.json";
    let targetedReloads = 0;
    const reloadedScriptInfos: string[] = [];
    const refreshOrder: string[] = [];
    info.project.projectService.clearSemanticCache = (project: unknown) => {
      expect(project).toBe(info.project);
      refreshOrder.push("clear-resolution-cache");
    };
    info.project.projectService.getScriptInfo = (fileName: string) =>
      fileName === "/ws/src/A.vue"
        ? {
            reloadFromFile: () => {
              reloadedScriptInfos.push(fileName);
              refreshOrder.push("reload-script-info");
              return true;
            },
          }
        : undefined;
    info.project.projectService.reloadFileNamesOfConfiguredProject = () => {
      targetedReloads++;
      refreshOrder.push("reload-project-files");
      return true;
    };
    info.project.refreshDiagnostics = () => refreshOrder.push("diagnostics");
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    const source = "/ws/src/A.vue";
    const companion = `${source}.tsx`;
    expect(info.languageServiceHost.getScriptVersion(source)).toBe("5:a1");
    const stale = info.languageServiceHost.getScriptSnapshot(source);
    expect(stale.getText(0, stale.getLength())).toBe("export const value = 'stale';");

    const published = vueAndSvelteManifest();
    published.epoch = 2;
    published.projects["/ws/tsconfig.json"].ready_files[companion] = {
      content_hash: "a2",
      version: 5,
      script_kind: "TSX",
      role: "CarrierIde",
      map_hash: "0",
      blob_rel: "blobs/A-v2.vue.tsx",
    };
    writeFileSync(join(dir, "blobs/A-v2.vue.tsx"), "export const value = 'fresh';", "utf8");
    writeFileSync(join(dir, "manifest.json"), JSON.stringify(published), "utf8");

    plugin.onConfigurationChanged!({
      carrierStoreDir: dir,
      carrierStoreRefreshToken: 2,
    });
    expect(targetedReloads).toBe(0);
    await new Promise<void>((resolve) => setImmediate(resolve));

    expect(targetedReloads).toBe(0);
    expect(reloadedScriptInfos).toEqual([source]);
    expect(refreshOrder).toEqual(["clear-resolution-cache", "reload-script-info"]);
    expect(info.languageServiceHost.getScriptVersion(source)).toBe("5:a2");
    const fresh = info.languageServiceHost.getScriptSnapshot(source);
    expect(fresh.getText(0, fresh.getLength())).toBe("export const value = 'fresh';");
  });

  it("replaces a warm Svelte wildcard fallback with the authored Component contract through deep barrels", async () => {
    const projectKey = "/ws/tsconfig.json";
    const source = "/ws/src/Native.svelte";
    const ide = `${source}.tsx`;
    const api = `${source}.verter.ts`;
    const directConsumer = "/ws/src/direct-consumer.ts";
    const barrelOne = "/ws/src/level-one.ts";
    const barrelTwo = "/ws/src/level-two.ts";
    const barrelConsumer = "/ws/src/barrel-consumer.ts";
    const svelteTypes = "/ws/node_modules/svelte/index.d.ts";
    const diskFiles: Record<string, string> = {
      [directConsumer]:
        'import Native from "./Native.svelte";\n' +
        'import type { ComponentProps } from "svelte";\n' +
        "export const directProps: ComponentProps<typeof Native> = { contractProp: 42 };\n",
      [barrelOne]: 'export { default as Native } from "./Native.svelte";\n',
      [barrelTwo]: 'export * from "./level-one";\n',
      [barrelConsumer]:
        'import { Native } from "./level-two";\n' +
        'import type { ComponentProps } from "svelte";\n' +
        "export const barrelProps: ComponentProps<typeof Native> = { contractProp: 42 };\n",
      [svelteTypes]:
        'declare module "svelte" {\n' +
        "  export type Component<Props extends Record<string, any> = {}, Exports extends Record<string, any> = {}, Bindings extends keyof Props | '' = string> = (internals: unknown, props: Props) => Exports;\n" +
        "  export type ComponentProps<Comp> = Comp extends Component<infer Props, any, any> ? Props : Record<string, any>;\n" +
        "  export interface LegacyComponentType { readonly legacy: true }\n" +
        "}\n" +
        'declare module "*.svelte" { import { LegacyComponentType } from "svelte"; const Comp: LegacyComponentType; export default Comp; }\n',
    };
    const initial: Manifest = {
      epoch: 1,
      host_version: "test",
      projects: {
        [projectKey]: { owned_sources: [], ready_files: {} },
      },
    };
    const dir = track(writeStore(initial, {}));
    const info = createInfo(dir, { diskFiles }, projectKey);
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      carrierStoreRefreshToken: 1,
    };
    info.project.getConfigFilePath = () => projectKey;
    info.project.getCurrentDirectory = () => process.cwd();
    info.project.getCompilerOptions = () => ({
      strict: true,
      noLib: true,
      module: ts.ModuleKind.ESNext,
      moduleResolution: ts.ModuleResolutionKind.Bundler,
    });
    let projectVersion = 0;
    info.languageServiceHost.getCompilationSettings = info.project.getCompilerOptions;
    info.languageServiceHost.getCurrentDirectory = info.project.getCurrentDirectory;
    info.languageServiceHost.getDefaultLibFileName = () => "/ws/no-lib.d.ts";
    info.languageServiceHost.getProjectVersion = () => String(projectVersion);
    info.languageServiceHost.getScriptFileNames = () => Object.keys(diskFiles);
    info.languageServiceHost.fileExists = info.serverHost.fileExists;
    info.languageServiceHost.readFile = info.serverHost.readFile;
    const relativeResolutions = new Map<string, string>([
      [`${barrelTwo}|./level-one`, barrelOne],
      [`${barrelConsumer}|./level-two`, barrelTwo],
    ]);
    info.languageServiceHost.resolveModuleNameLiterals = (
      literals: readonly ts.StringLiteralLike[],
      containingFile: string,
    ) =>
      literals.map((literal) => {
        let resolved: string | undefined;
        if (literal.text === "svelte") {
          resolved = svelteTypes;
        } else if (literal.text.startsWith(".")) {
          const normalizedContaining = containingFile.replace(/\\/g, "/").toLowerCase();
          resolved = relativeResolutions.get(`${normalizedContaining}|${literal.text}`);
        }
        return {
          resolvedModule:
            resolved === undefined
              ? undefined
              : {
                  resolvedFileName: resolved,
                  extension: ts.Extension.Ts,
                  isExternalLibraryImport: resolved === svelteTypes,
                },
        };
      });

    const plugin = init({ typescript: ts } as any);
    plugin.create(info);
    expect(
      info.languageServiceHost.resolveModuleNameLiterals(
        [{ text: "./level-two" }],
        barrelConsumer,
        undefined,
        info.project.getCompilerOptions(),
        undefined,
      )[0]?.resolvedModule?.resolvedFileName,
    ).toBe(barrelTwo);
    const realLanguageService = ts.createLanguageService(info.languageServiceHost);
    info.languageService.__lsImpl = realLanguageService;
    info.project.projectService.clearSemanticCache = (project: unknown) => {
      expect(project).toBe(info.project);
      projectVersion++;
      realLanguageService.cleanupSemanticCache();
    };
    info.project.projectService.reloadFileNamesOfConfiguredProject = (project: unknown) => {
      expect(project).toBe(info.project);
      projectVersion++;
      return true;
    };

    const before = info.languageService.getSemanticDiagnostics(directConsumer);
    expect(before.some((diagnostic: ts.Diagnostic) => diagnostic.code === 2307)).toBe(false);
    expect(before.some((diagnostic: ts.Diagnostic) => diagnostic.code === 2322)).toBe(false);
    const directSource = diskFiles[directConsumer];
    const directBinding = directSource.indexOf("Native");
    const beforeQuickInfo = info.languageService.getQuickInfoAtPosition(
      directConsumer,
      directBinding + 1,
    );
    expect(ts.displayPartsToString(beforeQuickInfo?.displayParts)).toContain("LegacyComponentType");

    const published: Manifest = {
      epoch: 2,
      host_version: "test",
      projects: {
        [projectKey]: {
          owned_sources: [
            {
              source_uri: source,
              provider_uri: ide,
              role: "CarrierIde",
              script_kind: "TSX",
            },
            {
              source_uri: source,
              provider_uri: api,
              role: "CarrierApi",
              script_kind: "TS",
            },
          ],
          ready_files: {
            [ide]: {
              content_hash: "native-ide",
              version: 1,
              script_kind: "TSX",
              role: "CarrierIde",
              map_hash: "0",
              blob_rel: "blobs/Native.svelte.tsx",
            },
            [api]: {
              content_hash: "native-api",
              version: 1,
              script_kind: "TS",
              role: "CarrierApi",
              map_hash: "0",
              blob_rel: "blobs/Native.svelte.verter.ts",
            },
          },
        },
      },
    };
    writeFileSync(join(dir, "blobs/Native.svelte.tsx"), "export {};\n", "utf8");
    writeFileSync(
      join(dir, "blobs/Native.svelte.verter.ts"),
      'declare const Native: import("svelte").Component<{ contractProp: string }, {}, "">; export default Native;\n',
      "utf8",
    );
    writeFileSync(join(dir, "manifest.json"), JSON.stringify(published), "utf8");

    plugin.onConfigurationChanged!({
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      carrierStoreRefreshToken: 2,
    });
    await new Promise<void>((resolve) => setImmediate(resolve));

    const afterQuickInfo = info.languageService.getQuickInfoAtPosition(
      directConsumer,
      directBinding + 1,
    );
    const afterDisplay = ts.displayPartsToString(afterQuickInfo?.displayParts);
    expect(afterDisplay).toContain("Component<");
    expect(afterDisplay).not.toContain("LegacyComponentType");

    for (const fileName of [directConsumer, barrelConsumer]) {
      const diagnostics = info.languageService.getSemanticDiagnostics(fileName) as ts.Diagnostic[];
      expect(diagnostics.some((diagnostic) => diagnostic.code === 2307)).toBe(false);
      const hasContractError = diagnostics.some(
        (diagnostic) =>
          diagnostic.code === 2322 &&
          ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n").includes("number"),
      );
      expect(
        hasContractError,
        diagnostics
          .map(
            (diagnostic) =>
              `TS${diagnostic.code}: ${ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")}`,
          )
          .join("\n"),
      ).toBe(true);
    }
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
      "/ws/a/tsconfig.json": {
        owned_sources: [
          {
            source_uri: "/ws/a/src/A.vue",
            provider_uri: "/ws/a/src/A.vue.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
        ],
        ready_files: {
          "/ws/a/src/A.vue.tsx": {
            content_hash: "a1",
            version: 1,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "0",
            blob_rel: "blobs/A.vue.tsx",
          },
        },
      },
      "/ws/b/tsconfig.json": {
        owned_sources: [
          {
            source_uri: "/ws/b/src/B.svelte",
            provider_uri: "/ws/b/src/B.svelte.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
        ],
        ready_files: {
          "/ws/b/src/B.svelte.tsx": {
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

/**
 * The DX gate for the reported defect: a plain `.ts` file importing a framework
 * carrier from a configured project that sets NO `jsx`.
 *
 * Everything here is deliberate:
 *  - the `ts.LanguageService` is created BEFORE the plugin installs its hooks
 *    (tsserver's real lifecycle: the ConfiguredProject builds its service, then
 *    plugins are enabled on the same host object);
 *  - the project's compiler options carry NO `jsx`, matching the reporting user.
 *    Note the plugin force-sets `jsx: Preserve` on those options once loaded, so
 *    TS6142 itself is SUPPRESSED here and is only a regression guard; the
 *    load-bearing assertions are the exact resolved filename, the resolved
 *    extension being `Ts` (never `Tsx`/`Jsx` — which is what makes TS6142
 *    unreachable by construction rather than by that mutation), TS2307
 *    presence/absence, and the TS2322-mentions-`number` API witness;
 *  - the API role is never hand-injected — only the IDE role is pre-published
 *    (that is precisely the state the old fallback mis-served), and the
 *    unpublished → ready transition is driven through the real publish +
 *    `carrierStoreRefreshToken` invalidation path;
 *  - the assertions are on the EXACT resolved filename AND the resulting
 *    semantic diagnostics, so a suffix-only fix proves nothing.
 */
describe("plain .ts importing a carrier (real Program, project sets NO jsx)", () => {
  const projectKey = "/ws/tsconfig.json";
  const consumer = "/ws/src/consumer.ts";

  /** Compiler options a real user project has: `jsx` is ABSENT. */
  const noJsxOptions = (): ts.CompilerOptions => ({
    strict: true,
    noLib: true,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
  });

  /** A manifest owning ONLY the IDE role for `source` (API not published yet). */
  function ideOnlyManifest(source: string, ideProvider: string, blobRel: string): Manifest {
    return {
      epoch: 1,
      host_version: "test",
      projects: {
        [projectKey]: {
          owned_sources: [
            {
              source_uri: source,
              provider_uri: ideProvider,
              role: "CarrierIde",
              script_kind: "TSX",
            },
          ],
          ready_files: {
            [ideProvider]: {
              content_hash: "ide-1",
              version: 1,
              script_kind: "TSX",
              role: "CarrierIde",
              map_hash: "0",
              blob_rel: blobRel,
            },
          },
        },
      },
    };
  }

  interface Harness {
    info: any;
    plugin: ReturnType<typeof init>;
    /** How many times production invoked the project's cache-clear hook. */
    clearCalls(): number;
    resolve(specifier: string, containing?: string): ts.ResolvedModuleFull | undefined;
    diagnostics(fileName?: string): ts.Diagnostic[];
    codes(fileName?: string): number[];
    publish(manifest: Manifest, token: number): Promise<void>;
    dir: string;
  }

  function harness(options: {
    manifest: Manifest;
    blobs: Record<string, string>;
    diskFiles: Record<string, string>;
    compilerOptions?: ts.CompilerOptions;
  }): Harness {
    const dir = track(writeStore(options.manifest, options.blobs));
    const compilerOptions = options.compilerOptions ?? noJsxOptions();
    const info = createInfo(dir, { diskFiles: options.diskFiles }, projectKey);
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      carrierStoreRefreshToken: 1,
    };
    info.project.getConfigFilePath = () => projectKey;
    info.project.getCompilerOptions = () => compilerOptions;
    let projectVersion = 0;
    info.languageServiceHost.getCompilationSettings = () => compilerOptions;
    info.languageServiceHost.getCurrentDirectory = info.project.getCurrentDirectory;
    info.languageServiceHost.getDefaultLibFileName = () => "/ws/no-lib.d.ts";
    info.languageServiceHost.getProjectVersion = () => String(projectVersion);
    info.languageServiceHost.getScriptFileNames = () => Object.keys(options.diskFiles);
    info.languageServiceHost.fileExists = info.serverHost.fileExists;
    info.languageServiceHost.readFile = info.serverHost.readFile;
    info.languageServiceHost.resolveModuleNameLiterals = (literals: readonly { text: string }[]) =>
      literals.map(() => ({ resolvedModule: undefined }));

    // tsserver's lifecycle: the project's LanguageService exists BEFORE the
    // plugin is enabled on the same host object.
    const realLanguageService = ts.createLanguageService(info.languageServiceHost);
    info.languageService.__lsImpl = realLanguageService;

    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    // The harness stands in for tsserver's `ProjectService.clearSemanticCache`,
    // which does `project.resolutionCache.clear()` + `cleanupSemanticCache()` +
    // `markAsDirty()`. There is no `ConfiguredProject` here, so there is no
    // `ResolutionCache` to clear; what the stand-in reproduces is the OBSERVABLE
    // consequence this gate depends on — the Program is dropped and the project
    // version advances, so every module specifier is resolved again on the next
    // query. It does NOT prove tsserver's own resolution-cache clear; that leg
    // is covered by asserting production INVOKES this hook (`clearCalls` below,
    // and the cold-start / equal-stat cases), plus TypeScript's own
    // implementation of it.
    let clearCalls = 0;
    info.project.projectService.clearSemanticCache = () => {
      clearCalls += 1;
      projectVersion++;
      realLanguageService.cleanupSemanticCache();
    };
    info.project.projectService.reloadFileNamesOfConfiguredProject = () => {
      projectVersion++;
      return true;
    };

    return {
      info,
      plugin,
      clearCalls: () => clearCalls,
      dir,
      resolve(specifier: string, containing: string = consumer) {
        return info.languageServiceHost.resolveModuleNameLiterals(
          [{ text: specifier }],
          containing,
          undefined,
          compilerOptions,
          undefined,
        )[0]?.resolvedModule;
      },
      diagnostics(fileName: string = consumer) {
        return info.languageService.getSemanticDiagnostics(fileName) as ts.Diagnostic[];
      },
      codes(fileName: string = consumer) {
        return (info.languageService.getSemanticDiagnostics(fileName) as ts.Diagnostic[]).map(
          (d) => d.code,
        );
      },
      async publish(manifest: Manifest, token: number) {
        writeFileSync(join(dir, "manifest.json"), JSON.stringify(manifest), "utf8");
        plugin.onConfigurationChanged!({
          carrierStoreDir: dir,
          [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
          carrierStoreRefreshToken: token,
        });
        await new Promise<void>((resolve) => setImmediate(resolve));
      },
    };
  }

  /**
   * Both component carriers, one body. Vue and Svelte take the SAME path —
   * the import surface comes from the adapter's descriptor column, never a
   * per-framework branch in the plugin.
   */
  for (const ext of [".vue", ".svelte"] as const) {
    const source = `/ws/src/A${ext}`;
    const ideProvider = `${source}.tsx`;
    const apiProvider = `${source}.verter.ts`;

    it(`resolves a ${ext} import to the public-API carrier across the unpublished → ready transition`, async () => {
      const h = harness({
        manifest: ideOnlyManifest(source, ideProvider, `blobs/A${ext}.tsx`),
        blobs: {
          // The IDE surface types `contract` as a STRING — if an ordinary import
          // ever lands here the consumer type-checks CLEAN, which is exactly the
          // silent-wrong-surface failure this gate must catch.
          [`blobs/A${ext}.tsx`]: "declare const A: { contract: string };\nexport default A;\n",
          [`blobs/A${ext}.verter.ts`]:
            "declare const A: { contract: number };\nexport default A;\n",
        },
        diskFiles: {
          [consumer]: `import A from "./A${ext}";\nexport const label: string = A.contract;\n`,
        },
      });

      // ── phase 1: the project owns no import surface yet ──────────────────
      // FAIL CLOSED. Never the JSX/TSX editor carrier (the `--jsx is not set`
      // defect), never a fabricated `.verter.ts` the store cannot serve.
      expect(h.resolve(`./A${ext}`)).toBeUndefined();
      const cold = h.codes();
      // Regression guard only — the plugin's own `jsx: Preserve` mutation keeps
      // TS6142 unreachable while it is loaded, so nothing here rests on it.
      expect(cold).not.toContain(6142);
      // The IDE surface must not have silently type-checked the consumer.
      expect(cold).not.toContain(2322);
      // TS's own answer stands: an unresolvable module.
      expect(cold).toContain(2307);

      // ── phase 2: the owning project publishes the API companion ──────────
      const published = ideOnlyManifest(source, ideProvider, `blobs/A${ext}.tsx`);
      const project = published.projects[projectKey];
      published.epoch = 2;
      project.owned_sources.push({
        source_uri: source,
        provider_uri: apiProvider,
        role: "CarrierApi",
        script_kind: "TS",
      });
      project.ready_files[apiProvider] = {
        content_hash: "api-1",
        version: 1,
        script_kind: "TS",
        role: "CarrierApi",
        map_hash: "0",
        blob_rel: `blobs/A${ext}.verter.ts`,
      };
      await h.publish(published, 2);

      const resolved = h.resolve(`./A${ext}`);
      expect(resolved?.resolvedFileName).toBe(apiProvider);
      // A non-JSX extension is what makes TS6142 unreachable by construction —
      // the plugin must not be relying on force-enabling `jsx` for the user's
      // whole project to keep an ordinary import type-checkable.
      expect(resolved?.extension).toBe(ts.Extension.Ts);
      expect(resolved?.extension).not.toBe(ts.Extension.Tsx);
      expect(resolved?.extension).not.toBe(ts.Extension.Jsx);

      const warm = h.diagnostics();
      const warmCodes = warm.map((d) => d.code);
      const detail = warm
        .map((d) => `TS${d.code}: ${ts.flattenDiagnosticMessageText(d.messageText, "\n")}`)
        .join("\n");
      // Production reached the real invalidation hook — the resolution did not
      // simply re-run against an unchanged Program.
      expect(h.clearCalls()).toBeGreaterThan(0);
      // The module resolves, and it resolves to a NON-JSX surface.
      expect(warmCodes, detail).not.toContain(2307);
      // Regression guard only (see the `jsx: Preserve` note above).
      expect(warmCodes, detail).not.toContain(6142);
      // …and the API surface is genuinely in the Program: its `number` contract
      // is what the consumer is checked against, not the IDE surface's `string`.
      expect(
        warm.some(
          (d) =>
            d.code === 2322 &&
            ts.flattenDiagnosticMessageText(d.messageText, "\n").includes("number"),
        ),
        detail,
      ).toBe(true);
    });
  }

  it("heals a cold start: the store dir arrives after the first resolution", async () => {
    // On a cold editor start the plugin is created before the LSP has reported
    // the resolved carrier-store dir, so the first resolution has no store to
    // consult at all. It must fail closed — and the store handoff must clear
    // this project's resolution cache so the retry sees the published surface.
    const source = "/ws/src/A.vue";
    const apiProvider = `${source}.verter.ts`;
    const manifest = ideOnlyManifest(source, `${source}.tsx`, "blobs/A.vue.tsx");
    const project = manifest.projects[projectKey];
    project.owned_sources.push({
      source_uri: source,
      provider_uri: apiProvider,
      role: "CarrierApi",
      script_kind: "TS",
    });
    project.ready_files[apiProvider] = {
      content_hash: "api-1",
      version: 1,
      script_kind: "TS",
      role: "CarrierApi",
      map_hash: "0",
      blob_rel: "blobs/A.vue.verter.ts",
    };
    const dir = track(
      writeStore(manifest, {
        "blobs/A.vue.tsx": "export default {};",
        "blobs/A.vue.verter.ts": "export default class A {}",
      }),
    );

    // Constructed with NO store dir — the pre-notification cold window.
    const info = createInfo(undefined, { diskFiles: {} }, projectKey);
    let resolutionCacheCleared = 0;
    info.project.projectService.clearSemanticCache = () => {
      resolutionCacheCleared += 1;
    };
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    const resolveA = () =>
      info.languageServiceHost.resolveModuleNameLiterals(
        [{ text: "./A.vue" }],
        "/ws/src/consumer.ts",
        undefined,
        {},
        undefined,
      )[0]?.resolvedModule;

    expect(resolveA()).toBeUndefined();

    plugin.onConfigurationChanged!({
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      carrierStoreRefreshToken: 1,
    });
    await new Promise<void>((resolve) => setImmediate(resolve));

    // The store handoff invalidated module resolution…
    expect(resolutionCacheCleared).toBeGreaterThan(0);
    // …and the retry now sees the published import surface.
    const resolved = resolveA();
    expect(resolved?.resolvedFileName).toBe(apiProvider);
    expect(resolved?.extension).toBe(ts.Extension.Ts);
  });

  it("heals a publication whose manifest replacement has an IDENTICAL stat tuple", async () => {
    // The reader's change key is `(mtimeMs, size)`. The Rust publisher swaps the
    // manifest ATOMICALLY, so a publication that replaces a ready-file entry
    // rather than adding one can land at the same byte length within a single
    // filesystem timestamp tick — a stat-identical replacement. If the token
    // advance reads ready versions from the stale snapshot, nothing looks
    // changed, `clearSemanticCache` never fires, and the cached TS2307 for this
    // exact import survives until some unrelated publication. That is the
    // sticky-negative mode this whole change exists to prevent.
    const source = "/ws/src/A.vue";
    const apiProvider = `${source}.verter.ts`;

    // BEFORE: the API companion is owned but NOT ready.
    const before = ideOnlyManifest(source, `${source}.tsx`, "blobs/A.vue.tsx");
    before.projects[projectKey].owned_sources.push({
      source_uri: source,
      provider_uri: apiProvider,
      role: "CarrierApi",
      script_kind: "TS",
    });
    // AFTER: the same manifest with the API companion READY.
    const after = ideOnlyManifest(source, `${source}.tsx`, "blobs/A.vue.tsx");
    after.projects[projectKey].owned_sources.push({
      source_uri: source,
      provider_uri: apiProvider,
      role: "CarrierApi",
      script_kind: "TS",
    });
    after.projects[projectKey].ready_files[apiProvider] = {
      content_hash: "api-1",
      version: 1,
      script_kind: "TS",
      role: "CarrierApi",
      map_hash: "0",
      blob_rel: "blobs/A.vue.verter.ts",
    };
    // Both serializations padded to ONE width, and both writes pinned to ONE
    // whole-second timestamp, so the replacement is byte-for-byte stat-identical.
    const width = Math.max(JSON.stringify(before).length, JSON.stringify(after).length);
    const pad = (m: Manifest) => {
      const json = JSON.stringify(m);
      return json + " ".repeat(width - json.length);
    };
    const pinnedSeconds = Math.floor(Date.now() / 1000) - 60;

    const dir = track(
      writeStore(before, {
        "blobs/A.vue.tsx": "export default {};",
        "blobs/A.vue.verter.ts": "export default class A {}",
      }),
    );
    const manifestPath = join(dir, "manifest.json");
    writeFileSync(manifestPath, pad(before), "utf8");
    utimesSync(manifestPath, pinnedSeconds, pinnedSeconds);

    const info = createInfo(dir, { diskFiles: {} }, projectKey);
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      carrierStoreRefreshToken: 1,
    };
    let cacheClears = 0;
    info.project.projectService.clearSemanticCache = () => {
      cacheClears += 1;
    };
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    const resolveA = () =>
      info.languageServiceHost.resolveModuleNameLiterals(
        [{ text: "./A.vue" }],
        "/ws/src/consumer.ts",
        undefined,
        {},
        undefined,
      )[0]?.resolvedModule;

    // Owned-but-unready: the bounded cold read times out and the plugin
    // abstains. This ALSO seeds the reader's manifest snapshot.
    expect(resolveA()).toBeUndefined();
    const statBefore = statSync(manifestPath);

    writeFileSync(manifestPath, pad(after), "utf8");
    utimesSync(manifestPath, pinnedSeconds, pinnedSeconds);

    // The replacement really is stat-identical — otherwise this test would be
    // exercising the ordinary mtime-changed path and prove nothing.
    const statAfter = statSync(manifestPath);
    expect(statAfter.size).toBe(statBefore.size);
    expect(statAfter.mtimeMs).toBe(statBefore.mtimeMs);

    plugin.onConfigurationChanged!({
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      carrierStoreRefreshToken: 2,
    });
    await new Promise<void>((resolve) => setImmediate(resolve));

    // The token advance observed the publication despite the identical stat…
    expect(cacheClears).toBeGreaterThan(0);
    // …and the import now resolves to the public-API surface.
    const resolved = resolveA();
    expect(resolved?.resolvedFileName).toBe(apiProvider);
    expect(resolved?.extension).toBe(ts.Extension.Ts);
  });

  it("abstains for an OWNED but never-published import surface (bounded, never fabricated)", () => {
    // Ownership without content is the transient mid-publish window. The bounded
    // cold read is allowed to wait it out, but on timeout the answer is still
    // ABSTAIN: pointing TypeScript at a path the store cannot serve produces a
    // sticky TS2307 that no later publication can clear.
    const source = "/ws/src/A.vue";
    const manifest = ideOnlyManifest(source, `${source}.tsx`, "blobs/A.vue.tsx");
    manifest.projects[projectKey].owned_sources.push({
      source_uri: source,
      provider_uri: `${source}.verter.ts`,
      role: "CarrierApi",
      script_kind: "TS",
    });
    // NOTE: no `ready_files` entry for the API companion — owned, unpublished.
    const dir = track(writeStore(manifest, { "blobs/A.vue.tsx": "export default {};" }));
    const info = createInfo(dir, { diskFiles: {} }, projectKey);
    init({ typescript: ts } as any).create(info);

    const resolved = info.languageServiceHost.resolveModuleNameLiterals(
      [{ text: "./A.vue" }],
      "/ws/src/consumer.ts",
      undefined,
      {},
      undefined,
    )[0]?.resolvedModule;
    expect(resolved).toBeUndefined();
  });

  it("matches the owned import surface through the host's canonical path identity", () => {
    // A case-insensitive host (Windows / APFS): the importer spells the
    // directory differently from the manifest the LSP wrote. The redirect must
    // still find the owned import surface — a raw string comparison would not.
    const source = "/ws/src/A.vue";
    const manifest = ideOnlyManifest(source, `${source}.tsx`, "blobs/A.vue.tsx");
    manifest.projects[projectKey].owned_sources.push({
      source_uri: source,
      provider_uri: `${source}.verter.ts`,
      role: "CarrierApi",
      script_kind: "TS",
    });
    manifest.projects[projectKey].ready_files[`${source}.verter.ts`] = {
      content_hash: "api-1",
      version: 1,
      script_kind: "TS",
      role: "CarrierApi",
      map_hash: "0",
      blob_rel: "blobs/A.vue.verter.ts",
    };
    const dir = track(
      writeStore(manifest, {
        "blobs/A.vue.tsx": "export default {};",
        "blobs/A.vue.verter.ts": "export default class A {}",
      }),
    );
    const info = createInfo(dir, { diskFiles: {} }, projectKey);
    expect(info.serverHost.useCaseSensitiveFileNames).toBe(false);
    init({ typescript: ts } as any).create(info);

    const resolved = info.languageServiceHost.resolveModuleNameLiterals(
      [{ text: "./A.vue" }],
      "/WS/src/consumer.ts",
      undefined,
      {},
      undefined,
    )[0]?.resolvedModule;
    // The MANIFEST-owned identity is returned, not the importer's spelling: the
    // store, the ScriptInfo content reloads and the ready-version bookkeeping
    // all key on the path the LSP published.
    expect(resolved?.resolvedFileName).toBe("/ws/src/A.vue.verter.ts");
    expect(resolved?.extension).toBe(ts.Extension.Ts);
  });

  /**
   * The alias / `baseUrl` route (a NON-relative carrier specifier). The
   * candidate carrier source is picked out of TypeScript's own failed-lookup
   * locations, gated on `baseUrl` containment. That gate used a raw
   * `candidate.includes(baseUrl)` substring test — separator- and drive-case
   * sensitive, so a `baseUrl` spelled `D:\ws\src` rejected a `d:/ws/src/...`
   * candidate on Windows and the carrier silently failed to resolve.
   */
  describe("alias / baseUrl carrier specifier", () => {
    const source = "/ws/src/A.vue";
    const apiProvider = `${source}.verter.ts`;

    /**
     * `candidate` is the carrier source TypeScript reports in its
     * `failedLookupLocations`; the manifest owns that source's API companion, so
     * the ONLY thing under test is the `baseUrl` containment gate.
     */
    function aliasHarness(baseUrl: string | undefined, candidate: string = source) {
      const ide = `${candidate}.tsx`;
      const api = `${candidate}.verter.ts`;
      const manifest = ideOnlyManifest(candidate, ide, "blobs/A.vue.tsx");
      const project = manifest.projects[projectKey];
      project.owned_sources.push({
        source_uri: candidate,
        provider_uri: api,
        role: "CarrierApi",
        script_kind: "TS",
      });
      project.ready_files[api] = {
        content_hash: "api-1",
        version: 1,
        script_kind: "TS",
        role: "CarrierApi",
        map_hash: "0",
        blob_rel: "blobs/A.vue.verter.ts",
      };
      const dir = track(
        writeStore(manifest, {
          "blobs/A.vue.tsx": "export default {};",
          "blobs/A.vue.verter.ts": "export default class A {}",
        }),
      );
      const info = createInfo(dir, { diskFiles: { [candidate]: "<template />" } }, projectKey);
      const compilerOptions: ts.CompilerOptions = baseUrl === undefined ? {} : { baseUrl };
      info.project.getCompilerOptions = () => compilerOptions;
      // TypeScript's own resolver misses the aliased carrier and reports where
      // it looked — the exact shape the redirect consumes.
      info.languageServiceHost.resolveModuleNameLiterals = (
        literals: readonly { text: string }[],
      ) =>
        literals.map(() => ({
          resolvedModule: undefined,
          failedLookupLocations: [candidate],
        }));
      init({ typescript: ts } as any).create(info);
      return info;
    }

    function resolveAlias(info: any): ts.ResolvedModuleFull | undefined {
      return info.languageServiceHost.resolveModuleNameLiterals(
        [{ text: "@app/A.vue" }],
        "/ws/src/consumer.ts",
        undefined,
        info.project.getCompilerOptions(),
        undefined,
      )[0]?.resolvedModule;
    }

    it("resolves an aliased carrier to the public-API carrier with no baseUrl", () => {
      const resolved = resolveAlias(aliasHarness(undefined));
      expect(resolved?.resolvedFileName).toBe(apiProvider);
      expect(resolved?.extension).toBe(ts.Extension.Ts);
    });

    it("accepts a candidate under a baseUrl spelled with foreign separators", () => {
      const resolved = resolveAlias(aliasHarness("\\ws\\src"));
      expect(resolved?.resolvedFileName).toBe(apiProvider);
    });

    it("accepts a candidate under a baseUrl whose case differs on a case-insensitive host", () => {
      const resolved = resolveAlias(aliasHarness("/WS/SRC"));
      expect(resolved?.resolvedFileName).toBe(apiProvider);
    });

    it("rejects a sibling directory that merely SHARES the baseUrl prefix", () => {
      // The kill shot for the old `candidate.includes(baseUrl)` gate:
      // `"/ws/srcOther/A.vue".includes("/ws/src")` is TRUE, so the substring
      // test accepted a carrier that is NOT inside `baseUrl`. Boundary-anchored
      // containment rejects it — `/ws/srcOther` is a sibling of `/ws/src`, not a
      // child. (Reversing the roles would not discriminate: the old test's
      // base `/ws/srcOther` is already absent from `/ws/src/A.vue`.)
      const info = aliasHarness("/ws/src", "/ws/srcOther/A.vue");
      expect(resolveAlias(info)).toBeUndefined();
    });

    it("accepts a candidate nested under an ANCESTOR baseUrl directory", () => {
      // `baseUrl` is a DIRECTORY (that is the shape TypeScript parses from a
      // tsconfig), and a real project's carriers sit below it rather than at
      // it. This exercises the boundary-anchored prefix branch at a real depth.
      const resolved = resolveAlias(aliasHarness("/ws", "/ws/src/nested/A.vue"));
      expect(resolved?.resolvedFileName).toBe("/ws/src/nested/A.vue.verter.ts");
      expect(resolved?.extension).toBe(ts.Extension.Ts);
    });
  });
});

describe("getExternalFiles is project-scoped (no cross-tsconfig leak)", () => {
  it("getExternalFiles(projectA) returns ONLY projectA's carrier, never projectB's (vue + svelte)", () => {
    const dir = track(
      writeStore(twoProjectManifest(), {
        "blobs/A.vue.tsx": "x",
        "blobs/B.svelte.tsx": "y",
      }),
    );

    // Two per-project plugin instances, each `create`d for its own tsconfig.
    const infoA = createInfo(dir, { diskFiles: {} }, "/ws/a/tsconfig.json");
    const infoB = createInfo(dir, { diskFiles: {} }, "/ws/b/tsconfig.json");
    infoA.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      activeCarrierSources: ["/ws/a/src/A.vue"],
    };
    infoB.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      activeCarrierSources: ["/ws/b/src/B.svelte"],
    };
    const pluginA = init({ typescript: ts } as any);
    const pluginB = init({ typescript: ts } as any);
    pluginA.create(infoA);
    pluginB.create(infoB);

    const filesA = pluginA.getExternalFiles!(infoA.project, 0 as any);
    const filesB = pluginB.getExternalFiles!(infoB.project, 0 as any);

    // Project A sees ONLY its Vue carrier; project B sees ONLY its Svelte carrier.
    expect(filesA).toEqual(["/ws/a/src/A.vue.tsx"]);
    expect(filesB).toEqual(["/ws/b/src/B.svelte.tsx"]);
    // The leak the fix closes: neither project advertises the OTHER's carrier.
    expect(filesA).not.toContain("/ws/b/src/B.svelte");
    expect(filesB).not.toContain("/ws/a/src/A.vue");
  });

  it("a project without an explicit working-set signal advertises no carrier roots", () => {
    // `getExternalFiles` may be called for a project this plugin instance has
    // not `create`d (the env-fallback path); it still scopes by the project arg.
    const dir = track(
      writeStore(twoProjectManifest(), {
        "blobs/A.vue.tsx": "x",
        "blobs/B.svelte.tsx": "y",
      }),
    );
    process.env.VERTER_CARRIER_STORE_DIR = dir;
    const info = createInfo(dir, { diskFiles: {} }, "/ws/a/tsconfig.json");
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
    };
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    // Query for project B via a project stub carrying B's name — the store dir
    // comes from the env fallback (B was never `create`d on this instance).
    plugin.onConfigurationChanged!(info.config);
    const projectB: any = {
      getProjectName: () => "/ws/b/tsconfig.json",
      getRootFiles: () => [],
      getFileNames: () => [],
      projectService: {
        logger: { info: () => {} },
        getScriptInfo: (fileName: string) =>
          fileName === "/ws/b/src/B.svelte.tsx" ? { isScriptOpen: () => true } : undefined,
      },
    };
    const filesB = plugin.getExternalFiles!(projectB, 0 as any);
    expect(filesB).toEqual([]);
    expect(filesB).not.toContain("/ws/a/src/A.vue");
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
    const infoA = createInfo(dir, { diskFiles: {} }, "/ws/a/tsconfig.json");
    init({ typescript: ts } as any).create(infoA);

    // Project A serves its own carrier…
    expect(infoA.serverHost.readFile("/ws/a/src/A.vue.tsx")).toBe("A-content");
    expect(infoA.serverHost.fileExists("/ws/a/src/A.vue.tsx")).toBe(true);
    // …but NOT project B's carrier (it falls through to disk, which is empty).
    expect(infoA.serverHost.readFile("/ws/b/src/B.svelte.tsx")).toBeUndefined();
    expect(infoA.serverHost.fileExists("/ws/b/src/B.svelte.tsx")).toBe(false);
    expect(infoA.languageServiceHost.getScriptSnapshot("/ws/b/src/B.svelte.tsx")).toBeUndefined();
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
      "/ws/tsconfig.json": {
        owned_sources: [
          {
            source_uri: "/ws/src/A.vue",
            provider_uri: "/ws/src/A.vue.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
          {
            source_uri: "/ws/src/U.vue",
            provider_uri: "/ws/src/U.vue.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
          {
            source_uri: "/ws/src/W.svelte",
            provider_uri: "/ws/src/W.svelte.tsx",
            role: "CarrierIde",
            script_kind: "TSX",
          },
        ],
        ready_files: {
          "/ws/src/A.vue.tsx": {
            content_hash: "a1",
            version: 1,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "ma",
            blob_rel: "blobs/A.vue.tsx",
            map_rel: "maps/A.vue.json",
          },
          "/ws/src/U.vue.tsx": {
            content_hash: "u1",
            version: 1,
            script_kind: "TSX",
            role: "CarrierIde",
            map_hash: "mu",
            blob_rel: "blobs/U.vue.tsx",
            map_rel: "maps/U.vue.json",
          },
          "/ws/src/W.svelte.tsx": {
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
  sources: ["/ws/src/A.vue"],
  names: [],
  mappings: "AAAA",
});
// gen (1,0) → src (W.svelte 2,0) — `AACA` = VLQ[0,0,1,0]: the companion's
// script statement maps onto the source's `const bar = 1;` line (after the
// `<script>` opener), so a token span maps end-to-end within one source line
// under strict BOTH-endpoint span mapping.
const SVELTE_MAP = JSON.stringify({
  version: 3,
  sources: ["/ws/src/W.svelte"],
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

describe("editor-owned source diagnostic routing", () => {
  // @ai-generated - The protocol serializer can format a non-editor carrier's
  // semantic response under its configured companion request identity, while a
  // direct suggestion command cannot. Bind the two language-service passes here
  // and retain one diagnostic object for an overlap rather than double-publishing.
  it("merges non-editor carrier suggestions into the semantic response without duplication", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/A.vue";
    const generatedSourceFile = ts.createSourceFile(
      sourcePath,
      mappableBlobs()["blobs/A.vue.tsx"],
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    const semantic: ts.Diagnostic = {
      file: generatedSourceFile,
      start: 0,
      length: 5,
      category: ts.DiagnosticCategory.Error,
      code: 2322,
      messageText: "Type 'number' is not assignable to type 'string'.",
    };
    const suggestion: ts.DiagnosticWithLocation = {
      file: generatedSourceFile,
      start: 6,
      length: 3,
      category: ts.DiagnosticCategory.Suggestion,
      code: 6133,
      reportsUnnecessary: true,
      messageText: "'foo' is declared but its value is never read.",
    };
    let suggestionQueries = 0;
    const info = createInfo(dir, { diskFiles: { [sourcePath]: "const foo = 1;\n" } });
    info.languageService.__lsImpl = {
      getSemanticDiagnostics: () => [semantic],
      getSuggestionDiagnostics: () => {
        suggestionQueries += 1;
        return [suggestion, semantic as ts.DiagnosticWithLocation];
      },
    };

    init({ typescript: ts } as any).create(info);
    const diagnostics = info.languageService.getSemanticDiagnostics(sourcePath);

    expect(suggestionQueries).toBe(1);
    expect(diagnostics).toEqual([semantic, suggestion]);
    expect(diagnostics[1]).toMatchObject({
      category: ts.DiagnosticCategory.Suggestion,
      code: 6133,
      reportsUnnecessary: true,
      start: 6,
      length: 3,
    });
  });

  // @ai-generated - Svelte uses the same framework-neutral carrier-source
  // diagnostic contract; the router must not rely on a Vue-only extension test.
  it("merges non-editor Svelte suggestions into the semantic response", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/W.svelte";
    const generatedSourceFile = ts.createSourceFile(
      sourcePath,
      mappableBlobs()["blobs/W.svelte.tsx"],
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    const suggestion: ts.DiagnosticWithLocation = {
      file: generatedSourceFile,
      start: 6,
      length: 3,
      category: ts.DiagnosticCategory.Suggestion,
      code: 6133,
      reportsUnnecessary: true,
      messageText: "'bar' is declared but its value is never read.",
    };
    const info = createInfo(dir, { diskFiles: { [sourcePath]: "const bar = 1;\n" } });
    info.languageService.__lsImpl = {
      getSemanticDiagnostics: () => [],
      getSuggestionDiagnostics: () => [suggestion],
    };

    init({ typescript: ts } as any).create(info);

    expect(info.languageService.getSemanticDiagnostics(sourcePath)).toEqual([suggestion]);
  });

  // @ai-generated - Ordinary TypeScript remains a direct LanguageService
  // passthrough; carrier-specific suggestion folding must not broaden globally.
  it("does not supplement a non-carrier semantic diagnostic response", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const fileName = "/ws/src/plain.ts";
    const sourceFile = ts.createSourceFile(
      fileName,
      "const value: string = 1;\n",
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TS,
    );
    const semantic: ts.DiagnosticWithLocation = {
      file: sourceFile,
      start: 6,
      length: 5,
      category: ts.DiagnosticCategory.Error,
      code: 2322,
      messageText: "Type 'number' is not assignable to type 'string'.",
    };
    let suggestionQueries = 0;
    const info = createInfo(dir, { diskFiles: { [fileName]: sourceFile.text } });
    info.languageService.__lsImpl = {
      getSemanticDiagnostics: () => [semantic],
      getSuggestionDiagnostics: () => {
        suggestionQueries += 1;
        return [];
      },
    };

    init({ typescript: ts } as any).create(info);

    expect(info.languageService.getSemanticDiagnostics(fileName)).toEqual([semantic]);
    expect(suggestionQueries).toBe(0);
  });

  // @ai-generated - Proves the non-editor tsserver backend diagnoses the configured
  // carrier source identity whose snapshot is the plugin-served generated program.
  it("passes non-editor carrier-source diagnostics through to the configured Program identity", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/A.vue";
    const sourceText = "const foo = 1;\n";
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    const generatedSourceFile = ts.createSourceFile(
      sourcePath,
      mappableBlobs()["blobs/A.vue.tsx"],
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    let requestedFile: string | undefined;
    info.languageService.__lsImpl = {
      getSuggestionDiagnostics: (fileName: string) => {
        requestedFile = fileName;
        return [
          {
            file: generatedSourceFile,
            start: 6,
            length: 3,
            category: ts.DiagnosticCategory.Suggestion,
            code: 6133,
            reportsUnnecessary: true,
            messageText: "'foo' is declared but its value is never read.",
          },
        ];
      },
    };

    init({ typescript: ts } as any).create(info);
    const diagnostics = info.languageService.getSuggestionDiagnostics(sourcePath);

    expect(requestedFile).toBe(sourcePath);
    expect(diagnostics).toHaveLength(1);
    expect(diagnostics[0]).toMatchObject({
      file: generatedSourceFile,
      start: 6,
      length: 3,
      code: 6133,
      reportsUnnecessary: true,
    });
  });

  // Non-editor hosts use the authored carrier path as the generated Program
  // identity. Editor-only companion routing must never intercept that path.
  it("passes non-editor carrier-source semantic features through to the configured Program", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/A.vue";
    const info = createInfo(dir, { diskFiles: { [sourcePath]: "const foo = 1;\n" } });
    const requests: Array<{ method: string; fileName: string; position: number }> = [];
    const quickInfo = {
      kind: ts.ScriptElementKind.constElement,
      kindModifiers: "",
      textSpan: { start: 6, length: 3 },
      displayParts: [{ kind: "text", text: "const foo: number" }],
    };
    info.languageService.__lsImpl = {
      getQuickInfoAtPosition: (fileName: string, position: number) => {
        requests.push({ method: "quickInfo", fileName, position });
        return quickInfo;
      },
      getCompletionsAtPosition: (fileName: string, position: number) => {
        requests.push({ method: "completions", fileName, position });
        return { entries: [], isGlobalCompletion: false, isMemberCompletion: false };
      },
      getDefinitionAtPosition: (fileName: string, position: number) => {
        requests.push({ method: "definition", fileName, position });
        return [];
      },
    };

    init({ typescript: ts } as any).create(info);

    expect(info.languageService.getQuickInfoAtPosition(sourcePath, 6)).toEqual(quickInfo);
    expect(info.languageService.getCompletionsAtPosition(sourcePath, 6, {})).toMatchObject({
      entries: [],
    });
    expect(info.languageService.getDefinitionAtPosition(sourcePath, 6)).toEqual([]);
    expect(requests).toEqual(
      expect.arrayContaining([
        { method: "quickInfo", fileName: sourcePath, position: 6 },
        { method: "completions", fileName: sourcePath, position: 6 },
        { method: "definition", fileName: sourcePath, position: 6 },
      ]),
    );
    expect(
      requests.every(({ fileName, position }) => fileName === sourcePath && position === 6),
    ).toBe(true);
  });

  // The carrier source-feature arbiter must decide from a POSITIVE ownership
  // signal. These two cases differ in exactly one variable — whether a Verter
  // host has configured this project's carrier store — and pin both directions:
  // an unconfigured (unknown) surface must abstain so it never registers a
  // second provider beside the selected one, while the configured non-editor
  // surface (the verter_lsp-internal tsserver, the sole provider there) must
  // keep answering.
  const carrierArbiterProbe = (storeDir: string | undefined) => {
    const sourcePath = "/ws/src/A.vue";
    const plainPath = "/ws/src/real.ts";
    const info = createInfo(storeDir, {
      diskFiles: { [sourcePath]: "const foo = 1;\n", [plainPath]: "const bar = 2;\n" },
    });
    const requests: string[] = [];
    const quickInfo = {
      kind: ts.ScriptElementKind.constElement,
      kindModifiers: "",
      textSpan: { start: 6, length: 3 },
      displayParts: [{ kind: "text", text: "const foo: number" }],
    };
    const definitions = [
      { fileName: sourcePath, textSpan: { start: 6, length: 3 }, kind: "const", name: "foo" },
    ];
    const completions = { entries: [], isGlobalCompletion: false, isMemberCompletion: false };
    info.languageService.__lsImpl = {
      getQuickInfoAtPosition: (fileName: string) => {
        requests.push(`quickInfo:${fileName}`);
        return quickInfo;
      },
      getDefinitionAtPosition: (fileName: string) => {
        requests.push(`definition:${fileName}`);
        return definitions;
      },
      getCompletionsAtPosition: (fileName: string) => {
        requests.push(`completions:${fileName}`);
        return completions;
      },
    };
    init({ typescript: ts } as any).create(info);
    return {
      sourcePath,
      plainPath,
      info,
      requests,
      expected: { quickInfo, definitions, completions },
    };
  };

  it("abstains from carrier source features when no Verter host has configured this project", () => {
    const probe = carrierArbiterProbe(undefined);

    // The carrier subject is NOT this project's to answer: no configuration has
    // told the plugin which surface it is in, so another provider may already be
    // serving this file. Fail closed rather than produce a duplicate answer.
    expect(probe.info.languageService.getQuickInfoAtPosition(probe.sourcePath, 6)).toBeUndefined();
    expect(probe.info.languageService.getDefinitionAtPosition(probe.sourcePath, 6)).toBeUndefined();
    expect(
      probe.info.languageService.getCompletionsAtPosition(probe.sourcePath, 6, {}),
    ).toBeUndefined();
    expect(probe.requests.filter((entry) => entry.endsWith(probe.sourcePath))).toEqual([]);

    // Abstaining is scoped to carrier subjects: a plain TypeScript file in the
    // same unconfigured project is still answered normally.
    expect(probe.info.languageService.getQuickInfoAtPosition(probe.plainPath, 6)).toEqual(
      probe.expected.quickInfo,
    );
    expect(probe.requests).toContain(`quickInfo:${probe.plainPath}`);
  });

  it("still answers carrier source features for the configured non-editor host it owns", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const probe = carrierArbiterProbe(dir);

    // The verter_lsp-internal tsserver is the SOLE provider on this surface; the
    // arbiter must not mistake "no editor ownership flags" for "unknown".
    expect(probe.info.languageService.getQuickInfoAtPosition(probe.sourcePath, 6)).toEqual(
      probe.expected.quickInfo,
    );
    expect(probe.info.languageService.getDefinitionAtPosition(probe.sourcePath, 6)).toEqual(
      probe.expected.definitions,
    );
    expect(probe.info.languageService.getCompletionsAtPosition(probe.sourcePath, 6, {})).toEqual(
      probe.expected.completions,
    );
    // Every feature reached the underlying service under the carrier identity —
    // the exact evidence an abstain would erase.
    expect(probe.requests).toContain(`quickInfo:${probe.sourcePath}`);
    expect(probe.requests).toContain(`definition:${probe.sourcePath}`);
    expect(probe.requests).toContain(`completions:${probe.sourcePath}`);
  });

  it("queries the ready companion and maps its diagnostic onto the source file", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const sourceText = "const foo = 1;\n";
    const companionText = mappableBlobs()["blobs/A.vue.tsx"];
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    const sourceFile = ts.createSourceFile(
      sourcePath,
      sourceText,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    const companionFile = ts.createSourceFile(
      companionPath,
      companionText,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    info.languageService.getProgram = () => ({
      getSourceFile: (fileName: string) =>
        fileName === sourcePath
          ? sourceFile
          : fileName === companionPath
            ? companionFile
            : undefined,
    });
    let requestedFile: string | undefined;
    info.languageService.__lsImpl = {
      getSemanticDiagnostics: (fileName: string) => {
        requestedFile = fileName;
        return [
          {
            file: companionFile,
            start: 6,
            length: 3,
            category: ts.DiagnosticCategory.Error,
            code: 2322,
            messageText: "Type 'string' is not assignable to type 'number'.",
          },
        ];
      },
    };

    init({ typescript: ts } as any).create(info);
    const diagnostics = info.languageService.getSemanticDiagnostics(sourcePath);

    expect(requestedFile).toBe(companionPath);
    expect(diagnostics).toHaveLength(1);
    expect(diagnostics[0].file).toBe(sourceFile);
    expect({ start: diagnostics[0].start, length: diagnostics[0].length }).toEqual({
      start: 6,
      length: 3,
    });
  });

  it("routes an inferred source request through its exact configured-project companion", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const sourceText = "const foo = 1;\n";
    const companionText = mappableBlobs()["blobs/A.vue.tsx"];
    const configuredInfo = createInfo(
      dir,
      { diskFiles: { [sourcePath]: sourceText } },
      "/ws/tsconfig.json",
    );
    configuredInfo.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    const companionFile = ts.createSourceFile(
      companionPath,
      companionText,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    configuredInfo.languageService.getProgram = () => ({
      getSourceFile: (fileName: string) => (fileName === companionPath ? companionFile : undefined),
    });
    let configuredRequest: string | undefined;
    configuredInfo.languageService.__lsImpl = {
      getSemanticDiagnostics: (fileName: string) => {
        configuredRequest = fileName;
        return [
          {
            file: companionFile,
            start: 6,
            length: 3,
            category: ts.DiagnosticCategory.Error,
            code: 2322,
            messageText: "Type 'string' is not assignable to type 'number'.",
          },
        ];
      },
    };
    init({ typescript: ts } as any).create(configuredInfo);

    const inferredInfo = createInfo(
      dir,
      { diskFiles: { [sourcePath]: sourceText } },
      "/dev/null/inferredProject1*",
    );
    inferredInfo.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    const sourceFile = ts.createSourceFile(
      sourcePath,
      sourceText,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    inferredInfo.languageService.getProgram = () => ({
      getSourceFile: (fileName: string) => (fileName === sourcePath ? sourceFile : undefined),
    });
    init({ typescript: ts } as any).create(inferredInfo);

    expect(configuredInfo.project.containsFile(sourcePath)).toBe(true);
    expect(inferredInfo.project.containsFile(sourcePath)).toBe(false);

    const diagnostics = inferredInfo.languageService.getSemanticDiagnostics(sourcePath);

    expect(configuredRequest).toBe(companionPath);
    expect(diagnostics).toHaveLength(1);
    expect(diagnostics[0].file).toBe(sourceFile);
    expect({ start: diagnostics[0].start, length: diagnostics[0].length }).toEqual({
      start: 6,
      length: 3,
    });
  });

  /** Re-spell a POSIX-rooted fixture onto a Windows drive-letter root. */
  const onDriveLetterRoot = <T>(fixture: T): T =>
    JSON.parse(JSON.stringify(fixture).split("/ws/").join("d:/ws/")) as T;

  /**
   * The mappable fixture on a Windows drive-letter root.
   *
   * The hashes are re-spelled along with the paths: a published carrier map is
   * CONTENT-ADDRESSED and `parsedMapFor` caches the parsed `TraceMap` by
   * `map_hash` for the life of the process. Re-rooting the map's `sources`
   * changes its content, so reusing the POSIX fixture's `map_hash` would be a
   * manifest that lies about its own bytes — and the first `"ma"` parsed in the
   * process would then answer for both spellings.
   */
  const driveLetterMappableManifest = (): Manifest => {
    const manifest = onDriveLetterRoot(mappableManifest());
    for (const project of Object.values(manifest.projects)) {
      for (const ready of Object.values(project.ready_files)) {
        ready.content_hash = `${ready.content_hash}-drive`;
        ready.map_hash = `${ready.map_hash}-drive`;
      }
    }
    return manifest;
  };

  // Inferred-project → configured-owner routing, driven by a WINDOWS-SHAPED
  // request path. This runs on every host, and is not gated on `process.platform`,
  // because the identity it exercises is decided by STRING normalisation, never by
  // the host's path semantics: `canonicalPath` is `normalizePath` (a pure `\` → `/`
  // replace) plus a `toLowerCase()` whenever the host is case-insensitive, which
  // `createInfo` hard-codes. `D:\ws\src\A.vue` therefore canonicalises to
  // `d:/ws/src/a.vue` — the store's own identity — on POSIX exactly as on Windows.
  // Nothing on this path calls `path.resolve`, reads the cwd, or branches on the
  // platform, so a drive-letter request is never treated as a relative name here.
  it("normalizes a backslash-spelled, upper-cased drive request onto the configured owner runtime for references and rename", () => {
    const dir = track(
      writeStore(driveLetterMappableManifest(), onDriveLetterRoot(mappableBlobs())),
    );
    const sourcePath = "d:/ws/src/A.vue";
    // Upper-cased drive AND backslash separators — neither matches the store's
    // spelling byte-for-byte; only normalisation makes them one identity.
    const requestPath = "D:\\ws\\src\\A.vue";
    const companionPath = "d:/ws/src/A.vue.tsx";
    const sourceText = "const foo = 1;\n";
    const configuredInfo = createInfo(
      dir,
      { diskFiles: { [sourcePath]: sourceText } },
      "d:/ws/tsconfig.json",
    );
    configuredInfo.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    const configuredRequests: string[] = [];
    const companionReference = () => ({
      fileName: companionPath,
      textSpan: { start: 6, length: 3 },
      isWriteAccess: false,
    });
    configuredInfo.languageService.__lsImpl = {
      getReferencesAtPosition: (fileName: string) => {
        configuredRequests.push(`references:${fileName}`);
        return [companionReference()];
      },
      findReferences: (fileName: string) => {
        configuredRequests.push(`findReferences:${fileName}`);
        const definition = {
          ...companionReference(),
          kind: ts.ScriptElementKind.constElement,
          name: "foo",
          containerKind: ts.ScriptElementKind.unknown,
          containerName: "",
        };
        return [
          { definition, references: [companionReference()] },
          {
            definition: { ...definition, contextSpan: { start: 0, length: 12 } },
            references: [companionReference()],
          },
        ];
      },
      findRenameLocations: (fileName: string) => {
        configuredRequests.push(`rename:${fileName}`);
        return [{ ...companionReference(), prefixText: "foo: " }, companionReference()];
      },
    };
    init({ typescript: ts } as any).create(configuredInfo);

    const inferredInfo = createInfo(
      dir,
      { diskFiles: { [sourcePath]: sourceText } },
      "/dev/null/inferredProject1*",
    );
    inferredInfo.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    init({ typescript: ts } as any).create(inferredInfo);

    const references = inferredInfo.languageService.getReferencesAtPosition(requestPath, 6);
    const referencedSymbols = inferredInfo.languageService.findReferences(requestPath, 6);
    const renameLocations = inferredInfo.languageService.findRenameLocations(
      requestPath,
      6,
      false,
      false,
      {},
    );
    const companionReferences = inferredInfo.languageService.getReferencesAtPosition(
      companionPath,
      6,
    );
    const companionReferencedSymbols = inferredInfo.languageService.findReferences(
      companionPath,
      6,
    );
    const companionRenameLocations = inferredInfo.languageService.findRenameLocations(
      companionPath,
      6,
      false,
      false,
      {},
    );

    expect(configuredRequests).toEqual([
      `references:${companionPath}`,
      `findReferences:${companionPath}`,
      `rename:${companionPath}`,
      `references:${companionPath}`,
      `findReferences:${companionPath}`,
      `rename:${companionPath}`,
    ]);
    expect(references).toMatchObject([{ fileName: sourcePath, textSpan: { start: 6, length: 3 } }]);
    expect(referencedSymbols).toMatchObject([
      {
        definition: { fileName: sourcePath, textSpan: { start: 6, length: 3 } },
        references: [{ fileName: sourcePath, textSpan: { start: 6, length: 3 } }],
      },
    ]);
    expect(referencedSymbols).toHaveLength(1);
    expect(referencedSymbols[0].references).toHaveLength(1);
    expect(renameLocations).toMatchObject([
      { fileName: sourcePath, textSpan: { start: 6, length: 3 } },
    ]);
    expect(renameLocations[0]).not.toHaveProperty("prefixText");
    expect(companionReferences).toMatchObject([
      { fileName: sourcePath, textSpan: { start: 6, length: 3 } },
    ]);
    expect(companionReferencedSymbols).toMatchObject([
      {
        definition: { fileName: sourcePath, textSpan: { start: 6, length: 3 } },
        references: [{ fileName: sourcePath, textSpan: { start: 6, length: 3 } }],
      },
    ]);
    expect(companionReferencedSymbols).toHaveLength(1);
    expect(companionReferencedSymbols[0].references).toHaveLength(1);
    expect(companionRenameLocations).toMatchObject([
      { fileName: sourcePath, textSpan: { start: 6, length: 3 } },
    ]);
    expect(companionRenameLocations[0]).not.toHaveProperty("prefixText");
  });

  it("materializes the visible source diagnostic file when the configured Program omits raw Vue", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const sourceText = "const foo = 1;\n";
    const companionFile = ts.createSourceFile(
      companionPath,
      mappableBlobs()["blobs/A.vue.tsx"],
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    info.languageService.getProgram = () => ({
      getSourceFile: (fileName: string) => (fileName === companionPath ? companionFile : undefined),
    });
    info.languageService.__lsImpl = {
      getSemanticDiagnostics: () => [
        {
          file: companionFile,
          start: 6,
          length: 3,
          category: ts.DiagnosticCategory.Error,
          code: 2322,
          messageText: "Type 'string' is not assignable to type 'number'.",
        },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const diagnostics = info.languageService.getSemanticDiagnostics(sourcePath);

    expect(diagnostics).toHaveLength(1);
    expect(diagnostics[0].file?.fileName).toBe(sourcePath);
    expect(diagnostics[0].file?.text).toBe(sourceText);
    expect({ start: diagnostics[0].start, length: diagnostics[0].length }).toEqual({
      start: 6,
      length: 3,
    });
  });

  it("does not attach mapped diagnostics to a generated Program impostor under the source identity", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const sourceText = "const foo = 1;\n";
    const companionText = mappableBlobs()["blobs/A.vue.tsx"];
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    const companionFile = ts.createSourceFile(
      companionPath,
      companionText,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    // A source-identity Program may contain the generated projection. Reusing
    // this SourceFile after mapping the span to authored offsets makes VS Code
    // decode those offsets through the wrong line table.
    const generatedImpostor = ts.createSourceFile(
      sourcePath,
      "\n\n\nconst foo = 1;\n",
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    info.languageService.getProgram = () => ({
      getSourceFile: (fileName: string) =>
        fileName === sourcePath
          ? generatedImpostor
          : fileName === companionPath
            ? companionFile
            : undefined,
    });
    info.languageService.__lsImpl = {
      getSemanticDiagnostics: () => [
        {
          file: companionFile,
          start: 6,
          length: 3,
          category: ts.DiagnosticCategory.Error,
          code: 2304,
          messageText: "Cannot find name 'foo'.",
        },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const diagnostics = info.languageService.getSemanticDiagnostics(sourcePath);

    expect(diagnostics).toHaveLength(1);
    expect(diagnostics[0].file).not.toBe(generatedImpostor);
    expect(diagnostics[0].file?.fileName).toBe(sourcePath);
    expect(diagnostics[0].file?.text).toBe(sourceText);
    expect({ start: diagnostics[0].start, length: diagnostics[0].length }).toEqual({
      start: 6,
      length: 3,
    });
  });

  it("drops a diagnostic whose Program snapshot predates the published source map", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const sourceText = "const foo = 1;\n";
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };

    // The manifest/map already describe the latest companion, but TypeScript
    // can issue the geterr it scheduled for the preceding Program before the
    // companion ScriptInfo reload runs. Mapping this stale offset through the
    // latest map would publish a plausible-looking diagnostic on the wrong
    // source line. The stale result must fail closed; refreshDiagnostics will
    // ask again after the ScriptInfo advances.
    const staleCompanionFile = ts.createSourceFile(
      companionPath,
      mappableBlobs()["blobs/A.vue.tsx"],
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    // Language-service SourceFiles carry the exact `getScriptVersion` token
    // used to build them. Keep the bytes equal to prove revision identity—not
    // an O(n) content comparison—is what rejects an obsolete Program.
    (staleCompanionFile as ts.SourceFile & { version: string }).version = "0:old";
    info.languageService.getProgram = () => ({
      getSourceFile: (fileName: string) =>
        fileName === companionPath ? staleCompanionFile : undefined,
    });
    info.languageService.__lsImpl = {
      getSemanticDiagnostics: () => [
        {
          file: staleCompanionFile,
          start: 6,
          length: 3,
          category: ts.DiagnosticCategory.Error,
          code: 2304,
          messageText: "Cannot find name 'foo'.",
        },
      ],
    };
    init({ typescript: ts } as any).create(info);

    expect(info.languageService.getSemanticDiagnostics(sourcePath)).toEqual([]);
  });

  // @ai-generated - Proves every editor-facing semantic request stays on the configured
  // companion Program and that generated paths/spans never leak back to the editor.
  it("routes the semantic navigation and edit surface through the exact companion", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/W.svelte";
    const companionPath = "/ws/src/W.svelte.tsx";
    const sourceText = "<script>\nconst bar = 1;\n</script>\n";
    const sourcePosition = 15;
    const companionPosition = 6;
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };

    const requests = new Map<string, { fileName: string; start: number; end?: number }>();
    const recordPosition = (method: string, fileName: string, position: number) => {
      if (fileName !== companionPath) {
        throw new Error(`${method} received raw editor source ${fileName}`);
      }
      requests.set(method, { fileName, start: position });
    };
    const definition = () => ({
      fileName: companionPath,
      textSpan: { start: companionPosition, length: 3 },
      kind: ts.ScriptElementKind.constElement,
      name: "bar",
      containerKind: ts.ScriptElementKind.unknown,
      containerName: "",
    });
    const reference = () => ({
      fileName: companionPath,
      textSpan: { start: companionPosition, length: 3 },
      isWriteAccess: false,
    });
    const companionEdit = () => ({
      fileName: companionPath,
      textChanges: [{ span: { start: companionPosition, length: 3 }, newText: "next" }],
    });

    info.languageService.__lsImpl = {
      getDefinitionAndBoundSpan: (fileName: string, position: number) => {
        recordPosition("definitionAndBoundSpan", fileName, position);
        return {
          textSpan: { start: companionPosition, length: 3 },
          definitions: [definition()],
        };
      },
      getDefinitionAtPosition: (fileName: string, position: number) => {
        recordPosition("definition", fileName, position);
        return [definition()];
      },
      getTypeDefinitionAtPosition: (fileName: string, position: number) => {
        recordPosition("typeDefinition", fileName, position);
        return [definition()];
      },
      getReferencesAtPosition: (fileName: string, position: number) => {
        recordPosition("references", fileName, position);
        return [reference()];
      },
      findReferences: (fileName: string, position: number) => {
        recordPosition("findReferences", fileName, position);
        return [{ definition: definition(), references: [reference()] }];
      },
      getImplementationAtPosition: (fileName: string, position: number) => {
        recordPosition("implementation", fileName, position);
        return [definition()];
      },
      getRenameInfo: (fileName: string, position: number) => {
        recordPosition("renameInfo", fileName, position);
        return {
          canRename: true,
          displayName: "bar",
          fullDisplayName: "bar",
          kind: ts.ScriptElementKind.constElement,
          kindModifiers: "",
          triggerSpan: { start: companionPosition, length: 3 },
        };
      },
      findRenameLocations: (fileName: string, position: number) => {
        recordPosition("renameLocations", fileName, position);
        return [reference()];
      },
      getCodeFixesAtPosition: (fileName: string, start: number, end: number) => {
        if (fileName !== companionPath) {
          throw new Error(`codeFixes received raw editor source ${fileName}`);
        }
        requests.set("codeFixes", { fileName, start, end });
        return [{ fixName: "fix", description: "Fix", changes: [companionEdit()] }];
      },
      getEditsForRefactor: (
        fileName: string,
        _formatOptions: ts.FormatCodeSettings,
        selection: number | ts.TextRange,
      ) => {
        if (fileName !== companionPath || typeof selection === "number") {
          throw new Error(`refactor edits received an unrouted editor selection for ${fileName}`);
        }
        requests.set("refactorEdits", {
          fileName,
          start: selection.pos,
          end: selection.end,
        });
        return { edits: [companionEdit()] };
      },
      getCompletionEntryDetails: (fileName: string, position: number) => {
        recordPosition("completionDetails", fileName, position);
        return {
          name: "bar",
          kind: ts.ScriptElementKind.constElement,
          kindModifiers: "",
          displayParts: [],
          codeActions: [{ description: "Import", changes: [companionEdit()] }],
        };
      },
    };
    init({ typescript: ts } as any).create(info);

    const definitionAndBoundSpan = info.languageService.getDefinitionAndBoundSpan(
      sourcePath,
      sourcePosition,
    );
    const definitions = info.languageService.getDefinitionAtPosition(sourcePath, sourcePosition);
    const typeDefinitions = info.languageService.getTypeDefinitionAtPosition(
      sourcePath,
      sourcePosition,
    );
    const references = info.languageService.getReferencesAtPosition(sourcePath, sourcePosition);
    const referencedSymbols = info.languageService.findReferences(sourcePath, sourcePosition);
    const implementations = info.languageService.getImplementationAtPosition(
      sourcePath,
      sourcePosition,
    );
    const renameInfo = info.languageService.getRenameInfo(sourcePath, sourcePosition, {});
    const renameLocations = info.languageService.findRenameLocations(
      sourcePath,
      sourcePosition,
      false,
      false,
      {},
    );
    const fixes = info.languageService.getCodeFixesAtPosition(
      sourcePath,
      sourcePosition,
      sourcePosition + 3,
      [1],
      {},
      {},
    );
    const refactorEdits = info.languageService.getEditsForRefactor(
      sourcePath,
      {},
      { pos: sourcePosition, end: sourcePosition + 3 },
      "extract",
      "function_scope_0",
      {},
    );
    const completionDetails = info.languageService.getCompletionEntryDetails(
      sourcePath,
      sourcePosition,
      "bar",
      {},
      undefined,
      {},
      undefined,
    );

    expect([...requests.values()]).toEqual(
      expect.arrayContaining([
        { fileName: companionPath, start: companionPosition },
        { fileName: companionPath, start: companionPosition, end: companionPosition + 3 },
      ]),
    );
    expect(requests.size).toBe(11);
    expect(definitionAndBoundSpan.textSpan).toEqual({ start: sourcePosition, length: 3 });
    expect(definitionAndBoundSpan.definitions[0].fileName).toBe(sourcePath);
    expect(definitions[0]).toMatchObject({
      fileName: sourcePath,
      textSpan: { start: sourcePosition, length: 3 },
    });
    expect(typeDefinitions[0]).toMatchObject({
      fileName: sourcePath,
      textSpan: { start: sourcePosition, length: 3 },
    });
    expect(references[0]).toMatchObject({
      fileName: sourcePath,
      textSpan: { start: sourcePosition, length: 3 },
    });
    expect(referencedSymbols[0].definition.fileName).toBe(sourcePath);
    expect(referencedSymbols[0].references[0].fileName).toBe(sourcePath);
    expect(implementations[0].fileName).toBe(sourcePath);
    expect(renameInfo).toMatchObject({
      canRename: true,
      triggerSpan: { start: sourcePosition, length: 3 },
    });
    expect(renameLocations[0].fileName).toBe(sourcePath);
    expect(fixes[0].changes[0].fileName).toBe(sourcePath);
    expect(refactorEdits.edits[0].fileName).toBe(sourcePath);
    expect(completionDetails.codeActions[0].changes[0].fileName).toBe(sourcePath);
  });

  it("routes quick info through the companion and maps its span back to the source", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const info = createInfo(dir, { diskFiles: { [sourcePath]: "const foo = 1;\n" } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    let requested: { fileName: string; position: number } | undefined;
    info.languageService.__lsImpl = {
      getQuickInfoAtPosition: (fileName: string, position: number) => {
        requested = { fileName, position };
        return fileName === companionPath
          ? {
              kind: ts.ScriptElementKind.constElement,
              kindModifiers: "",
              textSpan: { start: 6, length: 3 },
              displayParts: [{ kind: "text", text: "const foo: number" }],
            }
          : undefined;
      },
    };
    init({ typescript: ts } as any).create(info);

    const quickInfo = info.languageService.getQuickInfoAtPosition(sourcePath, 6);

    expect(requested).toEqual({ fileName: companionPath, position: 6 });
    expect(quickInfo?.textSpan).toEqual({ start: 6, length: 3 });
    expect(quickInfo?.displayParts?.map((part: ts.SymbolDisplayPart) => part.text).join("")).toBe(
      "const foo: number",
    );
  });

  it("routes applicable refactors through a strictly mapped companion selection", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const info = createInfo(dir, { diskFiles: { [sourcePath]: "const foo = 1;\n" } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    let requested: { fileName: string; selection: number | ts.TextRange } | undefined;
    info.languageService.__lsImpl = {
      getApplicableRefactors: (fileName: string, selection: number | ts.TextRange) => {
        requested = { fileName, selection };
        return [{ name: "extract", description: "Extract", actions: [] }];
      },
    };
    init({ typescript: ts } as any).create(info);

    const refactors = info.languageService.getApplicableRefactors(
      sourcePath,
      { pos: 6, end: 9 },
      {},
    );

    expect(requested).toEqual({
      fileName: companionPath,
      selection: { pos: 6, end: 9 },
    });
    expect(refactors).toEqual([{ name: "extract", description: "Extract", actions: [] }]);
  });

  it("routes completions through the companion and maps replacement spans back", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const info = createInfo(dir, { diskFiles: { [sourcePath]: "const foo = 1;\n" } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    let requested: { fileName: string; position: number } | undefined;
    info.languageService.__lsImpl = {
      getCompletionsAtPosition: (fileName: string, position: number) => {
        requested = { fileName, position };
        return {
          isGlobalCompletion: false,
          isMemberCompletion: true,
          isNewIdentifierLocation: false,
          optionalReplacementSpan: { start: 6, length: 3 },
          entries: [
            {
              name: "foo",
              kind: ts.ScriptElementKind.memberVariableElement,
              kindModifiers: "",
              sortText: "11",
              replacementSpan: { start: 6, length: 3 },
            },
          ],
        };
      },
    };
    init({ typescript: ts } as any).create(info);

    const completions = info.languageService.getCompletionsAtPosition(sourcePath, 6, {});

    expect(requested).toEqual({ fileName: companionPath, position: 6 });
    expect(completions?.optionalReplacementSpan).toEqual({ start: 6, length: 3 });
    expect(completions?.entries).toHaveLength(1);
    expect(completions?.entries[0].replacementSpan).toEqual({ start: 6, length: 3 });
  });

  // @ai-generated - TypeScript replacement spans can end at a generated JSX
  // delimiter even when the authored identifier itself has an exact origin.
  it("keeps lexical completion when its generated replacement end is synthetic", () => {
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const sourceText = "{{ increment }}\n";
    const manifest = mappableManifest();
    const ready = manifest.projects["/ws/tsconfig.json"].ready_files[companionPath];
    ready.blob_rel = "blobs/A.vue.tsx";
    ready.map_hash = "completion-synthetic-end";
    const dir = track(
      writeStore(manifest, {
        ...mappableBlobs(),
        "blobs/A.vue.tsx": "increment}</>\n",
        "maps/A.vue.json": JSON.stringify({
          version: 3,
          sources: [sourcePath],
          sourcesContent: [sourceText],
          names: [],
          // Generated [0,9) maps to source [3,12); the JSX delimiter at
          // generated column 9 is deliberately unmapped.
          mappings: "AAAG,S",
        }),
      }),
    );
    const sourcePosition = sourceText.indexOf("increment") + "incr".length;
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
      [E2E_PROVIDER_ONLY_COMPLETIONS_CONFIG_KEY]: true,
    };
    info.languageService.__lsImpl = {
      getCompletionsAtPosition: () => ({
        isGlobalCompletion: false,
        isMemberCompletion: false,
        isNewIdentifierLocation: false,
        optionalReplacementSpan: { start: 0, length: "increment".length },
        entries: [
          {
            name: "increment",
            kind: ts.ScriptElementKind.constElement,
            kindModifiers: "",
            sortText: "11",
          },
        ],
      }),
    };
    init({ typescript: ts } as any).create(info);

    const result = info.languageService.getCompletionsAtPosition(sourcePath, sourcePosition, {});

    expect(result?.entries.map((entry: ts.CompletionEntry) => entry.name)).toEqual(["increment"]);
    expect(result?.optionalReplacementSpan).toEqual({
      start: sourceText.indexOf("increment"),
      length: "increment".length,
    });
  });

  // @ai-generated - The Verter LSP owns non-member carrier completion because it
  // has the source-region context required for template scope and auto-import resolve.
  // True member completion remains on the editor TypeScript route above.
  it("yields non-member completion to the carrier completion owner", () => {
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const carrierText = "const view = <>{ foo }</>;\n";
    const dir = track(
      writeStore(mappableManifest(), {
        ...mappableBlobs(),
        "blobs/A.vue.tsx": carrierText,
      }),
    );
    const position = carrierText.indexOf("foo") + 1;
    const companionFile = ts.createSourceFile(
      companionPath,
      carrierText,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    const info = createInfo(dir, { diskFiles: { [sourcePath]: carrierText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    info.languageService.getProgram = () => ({
      getSourceFile: (fileName: string) => (fileName === companionPath ? companionFile : undefined),
    });
    info.languageService.__lsImpl = {
      getCompletionsAtPosition: () => ({
        isGlobalCompletion: true,
        isMemberCompletion: false,
        isNewIdentifierLocation: false,
        entries: [
          {
            name: "AbortController",
            kind: ts.ScriptElementKind.varElement,
            kindModifiers: "declare",
            sortText: "11",
          },
        ],
      }),
    };
    init({ typescript: ts } as any).create(info);

    expect(info.languageService.getCompletionsAtPosition(sourcePath, position, {})).toBeUndefined();
  });

  it("keeps generated lexical locals for a typed template prefix without globals", () => {
    const sourcePath = "/ws/src/A.vue";
    const sourceText = "{{ sl }}\n";
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    info.languageService.__lsImpl = {
      getCompletionsAtPosition: () => ({
        isGlobalCompletion: false,
        isMemberCompletion: false,
        isNewIdentifierLocation: false,
        entries: [
          {
            name: "slotItem",
            kind: ts.ScriptElementKind.constElement,
            kindModifiers: "",
            sortText: "11",
          },
          {
            name: "slotIndex",
            kind: ts.ScriptElementKind.constElement,
            kindModifiers: "",
            sortText: "11",
          },
          {
            name: "slice",
            kind: ts.ScriptElementKind.functionElement,
            kindModifiers: "declare",
            sortText: "15",
          },
          {
            name: "__props",
            kind: ts.ScriptElementKind.constElement,
            kindModifiers: "",
            sortText: "11",
          },
          {
            name: "globalThis",
            kind: ts.ScriptElementKind.moduleElement,
            kindModifiers: "",
            sortText: "15",
          },
          {
            name: "sleep",
            kind: ts.ScriptElementKind.functionElement,
            kindModifiers: "export",
            sortText: "16",
            source: "some-package",
          },
        ],
      }),
    };
    init({ typescript: ts } as any).create(info);

    const result = info.languageService.getCompletionsAtPosition(
      sourcePath,
      sourceText.indexOf("sl") + 2,
      {},
    );

    expect(result?.entries.map((entry: ts.CompletionEntry) => entry.name)).toEqual([
      "slotItem",
      "slotIndex",
    ]);
    expect(result?.isGlobalCompletion).toBe(false);
  });

  it("keeps provider-owned lexical template scope at an empty prefix in attribution E2E", () => {
    const sourcePath = "/ws/src/A.vue";
    const sourceText = "{{  }}\n";
    const manifest = mappableManifest();
    manifest.projects["/ws/tsconfig.json"].ready_files["/ws/src/A.vue.tsx"].blob_rel =
      "blobs/A.vue.tsx";
    const dir = track(
      writeStore(manifest, {
        ...mappableBlobs(),
        "blobs/A.vue.tsx": sourceText,
      }),
    );
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
      [E2E_PROVIDER_ONLY_COMPLETIONS_CONFIG_KEY]: true,
    };
    info.languageService.__lsImpl = {
      getCompletionsAtPosition: () => ({
        isGlobalCompletion: true,
        isMemberCompletion: false,
        isNewIdentifierLocation: false,
        entries: [
          {
            name: "localValue",
            kind: ts.ScriptElementKind.constElement,
            kindModifiers: "",
            sortText: "11",
          },
          {
            name: "AbortController",
            kind: ts.ScriptElementKind.varElement,
            kindModifiers: ts.ScriptElementKindModifier.ambientModifier,
            sortText: "15",
          },
          {
            name: "computed",
            kind: ts.ScriptElementKind.alias,
            kindModifiers: "export",
            sortText: "16",
            source: "vue",
          },
        ],
      }),
    };
    init({ typescript: ts } as any).create(info);

    const result = info.languageService.getCompletionsAtPosition(sourcePath, 3, {});

    expect(result?.entries.map((entry: ts.CompletionEntry) => entry.name)).toEqual(["localValue"]);
    expect(result?.isGlobalCompletion).toBe(false);
  });

  it("normalizes TypeScript JSX prop labels for a framework attribute position", () => {
    const sourcePath = "/ws/src/A.vue";
    const sourceText = "<MyComp />\n";
    const manifest = mappableManifest();
    manifest.projects["/ws/tsconfig.json"].ready_files["/ws/src/A.vue.tsx"].blob_rel =
      "blobs/A.vue.tsx";
    // The parser-supplied opening span of `<MyComp />` — the sole tag anchor
    // the bounded attribute-name classification may lex inside.
    manifest.projects["/ws/tsconfig.json"].ready_files["/ws/src/A.vue.tsx"].structure = {
      schema_version: 1,
      artifact_token: "a".repeat(43),
      script_content_ranges: [],
      markup_opening_ranges: [[0, sourceText.indexOf("\n")]],
    };
    const dir = track(
      writeStore(manifest, {
        ...mappableBlobs(),
        "blobs/A.vue.tsx": sourceText,
      }),
    );
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
      [E2E_PROVIDER_ONLY_COMPLETIONS_CONFIG_KEY]: true,
    };
    info.languageService.__lsImpl = {
      getCompletionsAtPosition: () => ({
        isGlobalCompletion: false,
        isMemberCompletion: true,
        isNewIdentifierLocation: false,
        entries: [
          {
            name: "onCustom?",
            kind: ts.ScriptElementKind.memberVariableElement,
            kindModifiers: "optional",
            sortText: "11",
          },
          {
            name: "modelValue?",
            kind: ts.ScriptElementKind.memberVariableElement,
            kindModifiers: "optional",
            sortText: "11",
          },
        ],
      }),
    };
    init({ typescript: ts } as any).create(info);

    const result = info.languageService.getCompletionsAtPosition(sourcePath, 8, {});

    expect(result?.entries.map((entry: ts.CompletionEntry) => entry.name)).toEqual([
      "@custom",
      "model-value",
    ]);
    expect(result?.entries.map((entry: ts.CompletionEntry) => entry.kindModifiers)).toEqual([
      "",
      "",
    ]);
  });

  // The raw SFC ScriptInfo remains editor-owned, but its TypeScript semantics
  // come from the mapped companion. Script positions therefore keep the
  // complete TypeScript list (locals, globals, and actionable auto-imports),
  // while the template-only ownership rule above continues to yield bare
  // render-scope identifiers to Verter.
  it("keeps actionable non-member completions on the TypeScript route inside script", () => {
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const sourceText =
      '<script setup lang="ts">\ncomputed\n</script>\n<template>{{ computed }}</template>\n';
    const companionText = "computed\n";
    const manifest = mappableManifest();
    const ready = manifest.projects["/ws/tsconfig.json"].ready_files[companionPath];
    ready.blob_rel = "blobs/A.vue.tsx";
    ready.map_hash = "script-completion-map";
    ready.structure = {
      schema_version: 1,
      artifact_token: "a".repeat(43),
      script_content_ranges: [[sourceText.indexOf("computed"), sourceText.indexOf("</script>")]],
    };
    const dir = track(
      writeStore(manifest, {
        ...mappableBlobs(),
        "blobs/A.vue.tsx": companionText,
        "maps/A.vue.json": JSON.stringify({
          version: 3,
          sources: [sourcePath],
          names: [],
          mappings: "AACA",
        }),
      }),
    );
    const sourcePosition = sourceText.indexOf("computed") + "computed".length;
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    let requested: { fileName: string; position: number } | undefined;
    info.languageService.__lsImpl = {
      getCompletionsAtPosition: (fileName: string, position: number) => {
        requested = { fileName, position };
        return {
          isGlobalCompletion: true,
          isMemberCompletion: false,
          isNewIdentifierLocation: false,
          entries: [
            {
              name: "AbortController",
              kind: ts.ScriptElementKind.varElement,
              kindModifiers: "declare",
              sortText: "11",
            },
            {
              name: "computed",
              kind: ts.ScriptElementKind.alias,
              kindModifiers: "export",
              sortText: "16",
              source: "vue",
              hasAction: true,
            },
          ],
        };
      },
    };
    init({ typescript: ts } as any).create(info);

    const completions = info.languageService.getCompletionsAtPosition(
      sourcePath,
      sourcePosition,
      {},
    );

    expect(requested).toEqual({ fileName: companionPath, position: "computed".length });
    expect(completions?.entries.map((entry: ts.CompletionEntry) => entry.name)).toEqual([
      "AbortController",
      "computed",
    ]);
    expect(
      completions?.entries.find((entry: ts.CompletionEntry) => entry.name === "computed"),
    ).toMatchObject({
      source: "vue",
      hasAction: true,
    });
  });

  // R3-B-02: `script_content_ranges` are UTF-8 BYTE spans from the Rust
  // producer, while tsserver positions are UTF-16 code units. Astral content
  // BEFORE the script block shifts the byte span past the caret: an
  // unconverted compare denies script ownership at a genuine script position
  // and the completion collapses onto the template-only route.
  it("keeps script ownership when astral content precedes the script block", () => {
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const sourceText =
      '<!-- \u{1F49A}\u{1F49A} -->\n<script setup lang="ts">\ncomputed\n</script>\n<template>{{ computed }}</template>\n';
    const companionText = "computed\n";
    const byteAt = (index: number) => Buffer.byteLength(sourceText.slice(0, index), "utf8");
    const manifest = mappableManifest();
    const ready = manifest.projects["/ws/tsconfig.json"].ready_files[companionPath];
    ready.blob_rel = "blobs/A.vue.tsx";
    ready.map_hash = "script-completion-astral-map";
    ready.structure = {
      schema_version: 1,
      artifact_token: "a".repeat(43),
      script_content_ranges: [
        [byteAt(sourceText.indexOf("computed")), byteAt(sourceText.indexOf("</script>"))],
      ],
    };
    const dir = track(
      writeStore(manifest, {
        ...mappableBlobs(),
        "blobs/A.vue.tsx": companionText,
        "maps/A.vue.json": JSON.stringify({
          version: 3,
          sources: [sourcePath],
          names: [],
          mappings: "AAEA",
        }),
      }),
    );
    // Caret after `co` — INSIDE the script content in UTF-16, but BEFORE the
    // byte-valued span start (the two astral hearts shift bytes by 4).
    const sourcePosition = sourceText.indexOf("computed") + "co".length;
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    info.languageService.__lsImpl = {
      getCompletionsAtPosition: () => ({
        isGlobalCompletion: true,
        isMemberCompletion: false,
        isNewIdentifierLocation: false,
        entries: [
          {
            name: "AbortController",
            kind: ts.ScriptElementKind.varElement,
            kindModifiers: "declare",
            sortText: "11",
          },
          {
            name: "computed",
            kind: ts.ScriptElementKind.alias,
            kindModifiers: "export",
            sortText: "16",
            source: "vue",
            hasAction: true,
          },
        ],
      }),
    };
    init({ typescript: ts } as any).create(info);

    const completions = info.languageService.getCompletionsAtPosition(
      sourcePath,
      sourcePosition,
      {},
    );

    // Script positions keep the complete TypeScript list; the template-only
    // filter must NOT engage.
    expect(completions?.entries.map((entry: ts.CompletionEntry) => entry.name)).toEqual([
      "AbortController",
      "computed",
    ]);
  });

  it("keeps carrier membership but yields source features to a selected managed provider", () => {
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const sourceText =
      '<script setup lang="ts">\nconst value = 1\n</script>\n<template>{{ value }}</template>\n';
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: false,
      activeCarrierSources: [sourcePath],
    };
    const requests: string[] = [];
    info.languageService.__lsImpl = {
      getCompletionsAtPosition: (fileName: string) => {
        requests.push(fileName);
        return {
          isGlobalCompletion: false,
          isMemberCompletion: false,
          isNewIdentifierLocation: false,
          entries: [
            {
              name: "editor-source-control",
              kind: ts.ScriptElementKind.constElement,
              kindModifiers: "",
              sortText: "11",
            },
          ],
        };
      },
      getQuickInfoAtPosition: (fileName: string) => {
        requests.push(`hover:${fileName}`);
        return {
          kind: ts.ScriptElementKind.constElement,
          kindModifiers: "",
          textSpan: { start: 0, length: 1 },
          displayParts: [{ text: "editor-source-control", kind: "text" }],
        };
      },
      getRenameInfo: (fileName: string) => {
        requests.push(`rename-info:${fileName}`);
        return {
          canRename: true,
          kind: ts.ScriptElementKind.constElement,
          kindModifiers: "",
          displayName: "editor-source-control",
          fullDisplayName: "editor-source-control",
          triggerSpan: { start: 0, length: 1 },
        };
      },
      findRenameLocations: (fileName: string) => {
        requests.push(`rename:${fileName}`);
        return [{ fileName, textSpan: { start: 0, length: 1 } }];
      },
    };
    init({ typescript: ts } as any).create(info);

    const result = info.languageService.getCompletionsAtPosition(
      sourcePath,
      sourceText.indexOf("value"),
      {},
    );

    expect(requests).toEqual([]);
    expect(result).toBeUndefined();
    expect(info.languageService.getQuickInfoAtPosition(companionPath, 0)).toBeUndefined();
    expect(info.languageService.getRenameInfo(companionPath, 0, {}).canRename).toBe(false);
    expect(
      info.languageService.findRenameLocations(companionPath, 0, false, false, {}),
    ).toBeUndefined();
    expect(requests).toEqual([]);
    expect(init({ typescript: ts } as any).getExternalFiles(info.project as any)).toContain(
      "/ws/src/A.vue.tsx",
    );
  });

  it("maps a generated-preamble auto-import through the published insertion anchor", () => {
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const sourceText =
      '<script setup lang="ts">\nconst doubled = computed(() => 1);\n</script>\n<template>{{ doubled }}</template>\n';
    const companionText = "/** generated preamble */\nconst doubled = computed(() => 1);\n";
    const manifest = mappableManifest();
    const ready = manifest.projects["/ws/tsconfig.json"].ready_files[companionPath];
    ready.blob_rel = "blobs/A.vue.tsx";
    ready.map_hash = "auto-import-preamble-map";
    ready.structure = {
      schema_version: 1,
      artifact_token: "b".repeat(43),
      script_content_ranges: [[sourceText.indexOf("\n") + 1, sourceText.indexOf("</script>")]],
    };
    const dir = track(
      writeStore(manifest, {
        ...mappableBlobs(),
        "blobs/A.vue.tsx": companionText,
        "maps/A.vue.json": JSON.stringify({
          version: 3,
          sources: [sourcePath],
          names: [],
          x_verter_mapping_product: {
            schema_version: 1,
            insertion_anchors: [[0, sourceText.indexOf("\n") + 1]],
          },
          // Generated line 1 maps to source line 1; the generated preamble at
          // line 0 deliberately has no source origin.
          mappings: ";AACA",
        }),
      }),
    );
    const sourcePosition = sourceText.indexOf("computed") + "computed".length;
    const companionPosition = companionText.indexOf("computed") + "computed".length;
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    const companionFile = ts.createSourceFile(
      companionPath,
      companionText,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    (companionFile as ts.SourceFile & { version: string }).version = "1:a1";
    info.languageService.getProgram = () => ({
      getSourceFile: (fileName: string) => (fileName === companionPath ? companionFile : undefined),
    });
    info.languageService.__lsImpl = {
      getCompletionEntryDetails: (fileName: string, position: number) => {
        expect({ fileName, position }).toEqual({
          fileName: companionPath,
          position: companionPosition,
        });
        return {
          name: "computed",
          kind: ts.ScriptElementKind.alias,
          kindModifiers: "export",
          displayParts: [],
          codeActions: [
            {
              description: 'Add import from "vue"',
              changes: [
                {
                  fileName: companionPath,
                  textChanges: [
                    {
                      span: { start: 0, length: 0 },
                      newText: 'import { computed } from "vue";\n',
                    },
                  ],
                },
              ],
            },
          ],
        };
      },
    };
    init({ typescript: ts } as any).create(info);

    const details = info.languageService.getCompletionEntryDetails(
      sourcePath,
      sourcePosition,
      "computed",
      {},
      "vue",
      {},
      undefined,
    );

    expect(details?.codeActions).toHaveLength(1);
    expect(details!.codeActions![0].changes).toEqual([
      {
        fileName: sourcePath,
        textChanges: [
          {
            span: { start: sourceText.indexOf("\n") + 1, length: 0 },
            newText: 'import { computed } from "vue";\n',
          },
        ],
      },
    ]);
  });

  it("maps the auto-import anchor from UTF-8 to UTF-16 under an astral prefix", () => {
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const sourceText =
      '<!-- \u{1F49A}\u{1F49A} -->\n<script setup lang="ts">\nconst doubled = computed(() => 1);\n</script>\n<template>{{ doubled }}</template>\n';
    const companionText = "/** generated preamble */\nconst doubled = computed(() => 1);\n";
    const byteAt = (index: number) => Buffer.byteLength(sourceText.slice(0, index), "utf8");
    const scriptContentStart = sourceText.indexOf("const doubled");
    const manifest = mappableManifest();
    const ready = manifest.projects["/ws/tsconfig.json"].ready_files[companionPath];
    ready.blob_rel = "blobs/A.vue.tsx";
    ready.map_hash = "auto-import-preamble-astral-map";
    ready.structure = {
      schema_version: 1,
      artifact_token: "b".repeat(43),
      script_content_ranges: [
        [byteAt(scriptContentStart), byteAt(sourceText.indexOf("</script>"))],
      ],
    };
    const dir = track(
      writeStore(manifest, {
        ...mappableBlobs(),
        "blobs/A.vue.tsx": companionText,
        "maps/A.vue.json": JSON.stringify({
          version: 3,
          sources: [sourcePath],
          names: [],
          x_verter_mapping_product: {
            schema_version: 1,
            insertion_anchors: [[0, byteAt(scriptContentStart)]],
          },
          // The generated preamble at line 0 has no source origin; generated
          // line 1 maps to source line 2 (the script content line).
          mappings: ";AAEA",
        }),
      }),
    );
    const sourcePosition = sourceText.indexOf("computed") + "computed".length;
    const companionPosition = companionText.indexOf("computed") + "computed".length;
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    const companionFile = ts.createSourceFile(
      companionPath,
      companionText,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    info.languageService.getProgram = () => ({
      getSourceFile: (fileName: string) => (fileName === companionPath ? companionFile : undefined),
    });
    info.languageService.__lsImpl = {
      getCompletionEntryDetails: (fileName: string, position: number) => {
        expect({ fileName, position }).toEqual({
          fileName: companionPath,
          position: companionPosition,
        });
        return {
          name: "computed",
          kind: ts.ScriptElementKind.alias,
          kindModifiers: "export",
          displayParts: [],
          codeActions: [
            {
              description: 'Add import from "vue"',
              changes: [
                {
                  fileName: companionPath,
                  textChanges: [
                    {
                      span: { start: 0, length: 0 },
                      newText: 'import { computed } from "vue";\n',
                    },
                  ],
                },
              ],
            },
          ],
        };
      },
    };
    init({ typescript: ts } as any).create(info);

    const details = info.languageService.getCompletionEntryDetails(
      sourcePath,
      sourcePosition,
      "computed",
      {},
      "vue",
      {},
      undefined,
    );

    expect(details?.codeActions).toHaveLength(1);
    expect(details!.codeActions![0].changes).toEqual([
      {
        fileName: sourcePath,
        textChanges: [
          {
            span: { start: scriptContentStart, length: 0 },
            newText: 'import { computed } from "vue";\n',
          },
        ],
      },
    ]);
  });

  it("refuses a zero-width edit in synthesized output without a declared anchor", () => {
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const sourceText = '<script setup lang="ts">\nconst value = 1;\n</script>\n';
    const companionText = "/** generated preamble */\nconst value = 1;\n/** generated tail */\n";
    const trailingOffset = companionText.indexOf("/** generated tail */");
    const manifest = mappableManifest();
    const ready = manifest.projects["/ws/tsconfig.json"].ready_files[companionPath];
    ready.blob_rel = "blobs/A.vue.tsx";
    ready.map_hash = "auto-import-synthesized-refusal";
    const dir = track(
      writeStore(manifest, {
        ...mappableBlobs(),
        "blobs/A.vue.tsx": companionText,
        "maps/A.vue.json": JSON.stringify({
          version: 3,
          sources: [sourcePath],
          names: [],
          x_verter_mapping_product: {
            schema_version: 1,
            insertion_anchors: [[0, sourceText.indexOf("\n") + 1]],
          },
          mappings: ";AACA;A",
        }),
      }),
    );
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    info.languageService.__lsImpl = {
      getCompletionEntryDetails: () => ({
        name: "value",
        kind: ts.ScriptElementKind.alias,
        kindModifiers: "export",
        displayParts: [],
        codeActions: [
          {
            description: "generated edit",
            changes: [
              {
                fileName: companionPath,
                textChanges: [
                  {
                    span: { start: trailingOffset, length: 0 },
                    newText: "generated edit",
                  },
                ],
              },
            ],
          },
        ],
      }),
    };
    init({ typescript: ts } as any).create(info);

    const details = info.languageService.getCompletionEntryDetails(
      sourcePath,
      sourceText.indexOf("value"),
      "value",
      {},
      undefined,
      {},
      undefined,
    );

    expect(details?.codeActions?.[0].changes).toEqual([]);
  });

  // @ai-generated - Verifies that completion insertion anchors never remap a
  // response produced by a Program companion from an older published revision.
  it("drops an anchored completion edit from a stale Program companion", () => {
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const sourceText = '<script setup lang="ts">\nconst value = 1;\n</script>\n';
    const companionText = "/** generated preamble */\nconst value = 1;\n";
    const manifest = mappableManifest();
    const ready = manifest.projects["/ws/tsconfig.json"].ready_files[companionPath];
    ready.blob_rel = "blobs/A.vue.tsx";
    ready.map_hash = "auto-import-synthesized-refusal";
    const dir = track(
      writeStore(manifest, {
        ...mappableBlobs(),
        "blobs/A.vue.tsx": companionText,
        "maps/A.vue.json": JSON.stringify({
          version: 3,
          sources: [sourcePath],
          names: [],
          x_verter_mapping_product: {
            schema_version: 1,
            insertion_anchors: [[0, sourceText.indexOf("\n") + 1]],
          },
          mappings: ";AACA",
        }),
      }),
    );
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    const staleCompanionFile = ts.createSourceFile(
      companionPath,
      companionText,
      ts.ScriptTarget.Latest,
      true,
      ts.ScriptKind.TSX,
    );
    (staleCompanionFile as ts.SourceFile & { version: string }).version = "0:old";
    info.languageService.getProgram = () => ({
      getSourceFile: (fileName: string) =>
        fileName === companionPath ? staleCompanionFile : undefined,
    });
    info.languageService.__lsImpl = {
      getCompletionEntryDetails: () => ({
        name: "value",
        kind: ts.ScriptElementKind.alias,
        kindModifiers: "export",
        displayParts: [],
        codeActions: [
          {
            description: "generated edit",
            changes: [
              {
                fileName: companionPath,
                textChanges: [{ span: { start: 0, length: 0 }, newText: "generated edit" }],
              },
            ],
          },
        ],
      }),
    };
    init({ typescript: ts } as any).create(info);

    const details = info.languageService.getCompletionEntryDetails(
      sourcePath,
      sourceText.indexOf("value"),
      "value",
      {},
      undefined,
      {},
      undefined,
    );

    expect(details?.codeActions?.[0].changes).toEqual([]);
  });

  // @ai-generated - A coarse carrier mapping can make tsserver label an in-scope
  // identifier list as member completion. Mixed member/non-member kinds prove the
  // response is not safe to merge into the editor's member list.
  it("yields a falsely classified member list to the carrier completion owner", () => {
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const carrierText = "const view = <>{ foo.bar }</>;\n";
    const dir = track(
      writeStore(mappableManifest(), {
        ...mappableBlobs(),
        "blobs/A.vue.tsx": carrierText,
      }),
    );
    const position = carrierText.indexOf("bar");
    const info = createInfo(dir, { diskFiles: { [sourcePath]: carrierText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    info.languageService.__lsImpl = {
      getCompletionsAtPosition: () => ({
        isGlobalCompletion: false,
        isMemberCompletion: true,
        isNewIdentifierLocation: false,
        entries: [
          {
            name: "bar",
            kind: ts.ScriptElementKind.memberVariableElement,
            kindModifiers: "",
            sortText: "11",
          },
          {
            name: "count",
            kind: ts.ScriptElementKind.constElement,
            kindModifiers: "",
            sortText: "11",
          },
        ],
      }),
    };
    init({ typescript: ts } as any).create(info);

    expect(info.languageService.getCompletionsAtPosition(sourcePath, position, {})).toBeUndefined();
  });

  it("routes semantic classifications and maps every encoded span back", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const info = createInfo(dir, { diskFiles: { [sourcePath]: "const foo = 1;\n" } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    let requested: { fileName: string; span: ts.TextSpan } | undefined;
    info.languageService.__lsImpl = {
      getEncodedSemanticClassifications: (fileName: string, span: ts.TextSpan) => {
        requested = { fileName, span };
        return { spans: [6, 3, 42], endOfLineState: 0 };
      },
    };
    init({ typescript: ts } as any).create(info);

    const classifications = info.languageService.getEncodedSemanticClassifications(
      sourcePath,
      { start: 0, length: 14 },
      ts.SemanticClassificationFormat.TwentyTwenty,
    );

    expect(requested).toEqual({
      fileName: companionPath,
      span: { start: 0, length: 14 },
    });
    expect(classifications).toEqual({ spans: [6, 3, 42], endOfLineState: 0 });
  });

  it("routes document highlights through the companion without raw-source requests", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const sourcePath = "/ws/src/A.vue";
    const companionPath = "/ws/src/A.vue.tsx";
    const sourceText = "const foo = 1;\n";
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } });
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    const requests: Array<{ fileName: string; position: number; files: string[] }> = [];
    info.languageService.__lsImpl = {
      getDocumentHighlights: (fileName: string, position: number, files: string[]) => {
        requests.push({ fileName, position, files });
        return [
          {
            fileName: companionPath,
            highlightSpans: [
              {
                textSpan: { start: 6, length: 3 },
                kind: ts.HighlightSpanKind.writtenReference,
              },
            ],
          },
        ];
      },
    };
    init({ typescript: ts } as any).create(info);

    const highlights = info.languageService.getDocumentHighlights(sourcePath, 6, [sourcePath]);

    expect(requests).toEqual([{ fileName: companionPath, position: 6, files: [companionPath] }]);
    expect(highlights).toEqual([
      {
        fileName: sourcePath,
        highlightSpans: [
          {
            textSpan: { start: 6, length: 3 },
            kind: ts.HighlightSpanKind.writtenReference,
          },
        ],
      },
    ]);
  });
});

describe("editor-owned routing never uses a CLOSED tsserver project", () => {
  const sourcePath = "/ws/src/A.vue";
  const companionPath = "/ws/src/A.vue.tsx";
  const sourceText = "const foo = 1;\n";

  /**
   * A configured owner whose tsserver project has been CLOSED.
   *
   * `Project.close()` clears `rootFilesMap`, and every subsequent host read
   * (`getScriptSnapshot` → `getOrCreateScriptInfoAndAttachToProject`) then
   * throws `TypeError: Cannot read properties of undefined (reading 'get')`.
   * `Project.isClosed()` is exactly `rootFilesMap === undefined`, so the two
   * are modelled together here.
   */
  function closedConfiguredOwner(dir: string) {
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } }, "/ws/tsconfig.json");
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    info.languageServiceHost.getScriptSnapshot = () => {
      throw new TypeError("Cannot read properties of undefined (reading 'get')");
    };
    init({ typescript: ts } as any).create(info);
    // Closed AFTER `create`: tsserver loads the plugin for a live project and
    // closes it later, which is the sequence that leaves a dead runtime behind.
    info.project.isClosed = () => true;
    return info;
  }

  function liveInferredRequester(dir: string) {
    const info = createInfo(
      dir,
      { diskFiles: { [sourcePath]: sourceText } },
      "/dev/null/inferredProject1*",
    );
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    init({ typescript: ts } as any).create(info);
    return info;
  }

  it("drops configuration work before touching a project closed after create", async () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfo(dir, { diskFiles: { [sourcePath]: sourceText } }, "/ws/tsconfig.json");
    info.config = { carrierStoreDir: dir, activeCarrierSources: [] };
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    let closedProjectTouches = 0;
    info.project.isClosed = () => true;
    info.project.markAsDirty = () => {
      closedProjectTouches += 1;
    };
    plugin.onConfigurationChanged!({
      carrierStoreDir: dir,
      activeCarrierSources: [sourcePath],
    });
    await new Promise<void>((resolve) => setImmediate(resolve));

    expect(closedProjectTouches).toBe(0);
  });

  it("fails closed instead of throwing when the only owner runtime is closed", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    closedConfiguredOwner(dir);
    const inferredInfo = liveInferredRequester(dir);

    // Every carrier source feature routed at a closed owner must MISS, never
    // throw: a thrown tsserver request is reported to the editor as a failed
    // command, and the user sees nothing at all.
    expect(() => inferredInfo.languageService.getQuickInfoAtPosition(sourcePath, 6)).not.toThrow();
    expect(inferredInfo.languageService.getQuickInfoAtPosition(sourcePath, 6)).toBeUndefined();
    expect(() =>
      inferredInfo.languageService.getDefinitionAndBoundSpan(sourcePath, 6),
    ).not.toThrow();
    expect(inferredInfo.languageService.getDefinitionAndBoundSpan(sourcePath, 6)).toBeUndefined();
    expect(() => inferredInfo.languageService.getReferencesAtPosition(sourcePath, 6)).not.toThrow();
  });

  it("serves again from the owner tsserver re-creates after closing the previous one", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    closedConfiguredOwner(dir);
    const inferredInfo = liveInferredRequester(dir);
    // The dead owner is observed and dropped by the miss above.
    expect(inferredInfo.languageService.getQuickInfoAtPosition(sourcePath, 6)).toBeUndefined();

    // tsserver re-creates the configured project under the SAME name; the
    // replacement runtime must be the one that answers.
    const revivedInfo = createInfo(
      dir,
      { diskFiles: { [sourcePath]: sourceText } },
      "/ws/tsconfig.json",
    );
    revivedInfo.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      [EDITOR_OWNS_CARRIER_SOURCE_FEATURES_CONFIG_KEY]: true,
    };
    const asked: string[] = [];
    revivedInfo.languageService.__lsImpl = {
      getQuickInfoAtPosition: (fileName: string) => {
        asked.push(fileName);
        return {
          kind: ts.ScriptElementKind.constElement,
          kindModifiers: "",
          textSpan: { start: 6, length: 3 },
          displayParts: [{ text: "const foo: number", kind: "text" }],
        };
      },
    };
    init({ typescript: ts } as any).create(revivedInfo);

    const quickInfo = inferredInfo.languageService.getQuickInfoAtPosition(sourcePath, 6);
    expect(asked).toEqual([companionPath]);
    expect(quickInfo?.textSpan).toEqual({ start: 6, length: 3 });
  });
});

describe("companion→source RESPONSE remap wiring (the new nav hooks)", () => {
  // The disk carries the carrier SOURCES so the response remapper's source-text
  // read succeeds (it reads via the plugin's `_readFile` = serverHost.readFile).
  function diskWithSources(): Record<string, string> {
    return {
      "/ws/src/A.vue": "<template/>\n<script setup>\nconst foo = 1;\n</script>\n",
      "/ws/src/U.vue": "<template/>\n<script setup>\nconst real = 1;\n</script>\n",
      "/ws/src/W.svelte": "<script>\nconst bar = 1;\n</script>\n",
      "/ws/src/Consumer.ts": "import A from './A.vue';\n",
    };
  }

  // @ai-generated - Prevents an unmappable generated definition from leaking a
  // private companion path into the editor's merged definition picker.
  it("drops an unmappable non-module companion definition", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfo(dir, { diskFiles: diskWithSources() });
    info.languageService.__lsImpl = {
      getDefinitionAtPosition: () => [
        {
          fileName: "/ws/src/U.vue.tsx",
          textSpan: { start: 0, length: 3 },
          kind: ts.ScriptElementKind.constElement,
          name: "generated",
          containerKind: ts.ScriptElementKind.unknown,
          containerName: "",
        },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const definitions = info.languageService.getDefinitionAtPosition("/ws/src/Consumer.ts", 9);

    expect(definitions).toEqual([]);
  });

  it("uses the reconfigured carrier store for response remapping", () => {
    const initialDir = track(
      writeStore(
        {
          epoch: 1,
          host_version: "test",
          projects: {
            "/ws/tsconfig.json": {
              owned_sources: [],
              ready_files: {},
            },
          },
        },
        {},
      ),
    );
    const reconfiguredDir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfo(initialDir, { diskFiles: diskWithSources() });
    info.languageService.__lsImpl = {
      getReferencesAtPosition: () => [
        {
          fileName: "/ws/src/A.vue.tsx",
          textSpan: { start: 6, length: 3 },
          isWriteAccess: false,
        },
      ],
    };
    const plugin = init({ typescript: ts } as any);
    plugin.create(info);

    plugin.onConfigurationChanged!({
      carrierStoreDir: reconfiguredDir,
      responseRemap: true,
    });
    const refs = info.languageService.getReferencesAtPosition("/ws/src/Consumer.ts", 9);

    expect(refs).toHaveLength(1);
    expect(refs[0].fileName).toBe("/ws/src/A.vue");
    expect(refs[0].textSpan).toEqual({ start: 6, length: 3 });
  });

  it("getReferencesAtPosition: a companion entry → source, real .ts passes through", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfo(dir, { diskFiles: diskWithSources() });
    info.languageService.__lsImpl = {
      getReferencesAtPosition: () => [
        {
          fileName: "/ws/src/Consumer.ts",
          textSpan: { start: 9, length: 1 },
          isWriteAccess: false,
        },
        {
          fileName: "/ws/src/A.vue.tsx",
          textSpan: { start: 6, length: 3 },
          isWriteAccess: false,
        },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const refs = info.languageService.getReferencesAtPosition("/ws/src/Consumer.ts", 9);
    const paths = refs.map((r: any) => r.fileName);
    // The companion entry is mapped to the SOURCE .vue, the real .ts is intact.
    expect(paths).toContain("/ws/src/A.vue");
    expect(paths).toContain("/ws/src/Consumer.ts");
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
          fileName: "/ws/src/U.vue.tsx",
          textSpan: { start: 0, length: 3 },
          isWriteAccess: false,
        },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const refs = info.languageService.getReferencesAtPosition("/ws/src/Consumer.ts", 9);
    // No mappable origin → dropped: NEVER a companion path, NEVER a mis-mapped source.
    expect(refs).toHaveLength(0);
  });

  it("findRenameLocations: a Svelte companion location → .svelte source", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfo(dir, { diskFiles: diskWithSources() });
    info.languageService.__lsImpl = {
      findRenameLocations: () => [
        { fileName: "/ws/src/W.svelte.tsx", textSpan: { start: 6, length: 3 } },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const locs = info.languageService.findRenameLocations(
      "/ws/src/Consumer.ts",
      0,
      false,
      false,
      undefined,
    );
    expect(locs).toHaveLength(1);
    expect(locs[0].fileName).toBe("/ws/src/W.svelte");
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
              fileName: "/ws/src/A.vue.tsx",
              textChanges: [{ span: { start: 6, length: 3 }, newText: "renamed" }],
            },
            {
              fileName: "/ws/src/Consumer.ts",
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
      "/ws/src/Consumer.ts",
      0,
      1,
      [1],
      {},
      {},
    );
    const changePaths = fixes[0].changes.map((c: any) => c.fileName);
    // The companion edit landed on the .vue SOURCE; the real .ts edit kept its
    // path with the import specifier rewritten to the bare .vue.
    expect(changePaths).toContain("/ws/src/A.vue");
    expect(changePaths.some((p: string) => p.includes(".vue.tsx"))).toBe(false);
    const consumerEdit = fixes[0].changes.find((c: any) => c.fileName === "/ws/src/Consumer.ts");
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
              fileName: "/ws/src/A.vue.tsx",
              textChanges: [{ span: { start: 6, length: 3 }, newText: 'import "./Comp.vue.tsx";' }],
            },
          ],
        },
      ],
    });
    init({ typescript: ts } as any).create(info);

    const detail = info.languageService.getCompletionEntryDetails(
      "/ws/src/Consumer.ts",
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
    expect(action.changes[0].fileName).toBe("/ws/src/A.vue");
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
      "/ws/src/A.vue": "<template/>\n<script setup>\nconst foo = 1;\n</script>\n",
      "/ws/src/W.svelte": "<script>\nconst bar = 1;\n</script>\n",
      "/ws/src/Consumer.ts": "import A from './A.vue';\n",
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
          fileName: "/ws/src/A.vue.tsx",
          textSpan: { start: 6, length: 3 },
          isWriteAccess: false,
        },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const refs = info.languageService.getReferencesAtPosition("/ws/src/Consumer.ts", 9);
    // RAW: the companion path is preserved (Rust maps it), the span is unchanged.
    expect(refs).toHaveLength(1);
    expect(refs[0].fileName).toBe("/ws/src/A.vue.tsx");
    expect(refs[0].textSpan).toEqual({ start: 6, length: 3 });
  });

  it("findRenameLocations: a companion location passes through RAW", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfoNoRemap(dir, { diskFiles: diskWithSources() });
    info.languageService.__lsImpl = {
      findRenameLocations: () => [
        { fileName: "/ws/src/W.svelte.tsx", textSpan: { start: 6, length: 3 } },
      ],
    };
    init({ typescript: ts } as any).create(info);

    const locs = info.languageService.findRenameLocations(
      "/ws/src/Consumer.ts",
      0,
      false,
      false,
      undefined,
    );
    expect(locs).toHaveLength(1);
    expect(locs[0].fileName).toBe("/ws/src/W.svelte.tsx");
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
              fileName: "/ws/src/A.vue.tsx",
              textChanges: [{ span: { start: 6, length: 3 }, newText: "renamed" }],
            },
            {
              fileName: "/ws/src/Consumer.ts",
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
      "/ws/src/Consumer.ts",
      0,
      1,
      [1],
      {},
      {},
    );
    // RAW: the companion edit KEEPS its `.vue.tsx` path (Rust maps it) and the
    // inserted specifier is NOT rewritten (Rust owns that on the LSP surface).
    const changePaths = fixes[0].changes.map((c: any) => c.fileName);
    expect(changePaths).toContain("/ws/src/A.vue.tsx");
    const consumerEdit = fixes[0].changes.find((c: any) => c.fileName === "/ws/src/Consumer.ts");
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
              fileName: "/ws/src/A.vue.tsx",
              textChanges: [{ span: { start: 6, length: 3 }, newText: 'import "./Comp.vue.tsx";' }],
            },
          ],
        },
      ],
    });
    init({ typescript: ts } as any).create(info);

    const detail = info.languageService.getCompletionEntryDetails(
      "/ws/src/Consumer.ts",
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
    expect(action.changes[0].fileName).toBe("/ws/src/A.vue.tsx");
    expect(action.changes[0].textChanges[0].newText).toBe('import "./Comp.vue.tsx";');
  });

  it("getDefinitionAtPosition: a module-level companion def passes through RAW", () => {
    const dir = track(writeStore(mappableManifest(), mappableBlobs()));
    const info = createInfoNoRemap(dir, { diskFiles: diskWithSources() });
    // The aliased / module-nav paths are not driven here (no program); the
    // fallback `getDefinitionAtPosition` returns a companion module-level def.
    info.languageService.getDefinitionAtPosition = () => [
      {
        fileName: "/ws/src/A.vue.tsx",
        textSpan: { start: 0, length: 1 },
        kind: "module",
        name: "A",
        containerName: "",
        containerKind: "",
      },
    ];
    init({ typescript: ts } as any).create(info);

    const defs = info.languageService.getDefinitionAtPosition("/ws/src/Consumer.ts", 9);
    // RAW: the companion path is preserved (the Rust merge layer maps the
    // module-level def → `.vue` source); the plugin does NOT remap here.
    expect(defs).toHaveLength(1);
    expect(defs[0].fileName).toBe("/ws/src/A.vue.tsx");
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
      "/ws/src/A.vue": '<template/>\n<script setup lang="ts">\nconst x = 1;\n</script>\n',
      "/ws/src/W.svelte": '<script lang="ts">\nconst y = 1;\n</script>\n',
      // The carriers must exist on disk so `resolveModuleFileName` (which checks
      // `path.resolve(dir, './A.vue')`) resolves them. Module-specifier
      // NAVIGATION targets the carrier SOURCE directly — no companion is
      // involved, and no publication state is consulted.
      "/ws/src/Consumer.ts": CONSUMER,
    };
  }

  it("Vue: go-to-def on a `./A.vue` import specifier lands in the .vue SOURCE, not the companion", () => {
    const dir = track(
      writeStore(vueAndSvelteManifest(), { "blobs/A.vue.tsx": "export const A = 1;" }),
    );
    const { info, consumerSource } = infoWithProgram(
      dir,
      { diskFiles: diskWithCarrierSources() },
      "/ws/src/Consumer.ts",
      CONSUMER,
    );
    init({ typescript: ts } as any).create(info);

    // Offset inside the `./A.vue` specifier text.
    const off = consumerSource.indexOf("./A.vue") + 2;
    const defs = info.languageService.getDefinitionAtPosition("/ws/src/Consumer.ts", off);
    expect(defs).toBeDefined();
    expect(defs).toHaveLength(1);
    // Lands in the .vue SOURCE.
    expect(defs[0].fileName).toBe("/ws/src/A.vue");
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
      "/ws/src/Consumer.ts",
      CONSUMER,
    );
    init({ typescript: ts } as any).create(info);

    const off = consumerSource.indexOf("./W.svelte") + 2;
    const defs = info.languageService.getDefinitionAtPosition("/ws/src/Consumer.ts", off);
    expect(defs).toBeDefined();
    expect(defs).toHaveLength(1);
    expect(defs[0].fileName).toBe("/ws/src/W.svelte");
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
      "/ws/src/Consumer.ts",
      CONSUMER,
    );
    init({ typescript: ts } as any).create(info);

    const off = consumerSource.indexOf("./A.vue") + 2;
    const result = info.languageService.getDefinitionAndBoundSpan("/ws/src/Consumer.ts", off);
    expect(result).toBeDefined();
    expect(result.definitions).toHaveLength(1);
    expect(result.definitions[0].fileName).toBe("/ws/src/A.vue");
    expect(result.definitions[0].fileName).not.toContain(".vue.tsx");
  });
});

describe("store unavailable → fail closed", () => {
  it("serves nothing for carriers and falls through for everything else", () => {
    const info = createInfo(undefined, {
      diskFiles: { "/ws/src/A.vue.tsx": "ON-DISK", "/ws/src/real.ts": "real" },
    });
    init({ typescript: ts } as any).create(info);

    // No store → the companion path is whatever is on real disk (no fabrication).
    expect(info.serverHost.readFile("/ws/src/real.ts")).toBe("real");
    // A path that happens to look like a companion still just hits disk.
    expect(info.serverHost.readFile("/ws/src/A.vue.tsx")).toBe("ON-DISK");
  });
});

describe("carrier-path conflict honor (manifest is the authority)", () => {
  it("a source absent from the manifest → fileExists for its companion falls through to disk", () => {
    // Rust marked Foo.vue Ambiguous, so it is ABSENT from owned_sources/ready_files.
    const dir = track(writeStore(vueAndSvelteManifest(), { "blobs/A.vue.tsx": "x" }));
    const info = createInfo(dir, { diskFiles: {} });
    init({ typescript: ts } as any).create(info);

    // Foo.vue.tsx is not in the manifest → not fabricated; disk says no.
    expect(info.serverHost.fileExists("/ws/src/Foo.vue.tsx")).toBe(false);
    expect(info.languageServiceHost.getScriptSnapshot("/ws/src/Foo.vue.tsx")).toBeUndefined();
  });

  it("does not overlay-shadow a real file living at the carrier path", () => {
    // A real user file sits at Foo.vue.tsx; Rust left it out of the manifest.
    const dir = track(writeStore(vueAndSvelteManifest(), {}));
    const info = createInfo(dir, { diskFiles: { "/ws/src/Foo.vue.tsx": "REAL USER FILE" } });
    init({ typescript: ts } as any).create(info);

    // The plugin honors the manifest and returns the REAL file, not a companion.
    expect(info.serverHost.readFile("/ws/src/Foo.vue.tsx")).toBe("REAL USER FILE");
    expect(info.serverHost.fileExists("/ws/src/Foo.vue.tsx")).toBe(true);
  });
});

describe("import-path completion in a plain .ts offers owned carriers", () => {
  const projectKey = "/ws/tsconfig.json";
  const consumer = "/ws/src/consumer.ts";
  // Caret sits INSIDE the module-specifier literal, right after `./`.
  const consumerText = 'import Comp from "./";\n';
  const caret = consumerText.indexOf('"./') + '"./'.length;

  const compilerOptions = (): ts.CompilerOptions => ({
    strict: true,
    noLib: true,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
  });

  /**
   * A manifest where `A.vue` and `W.svelte` own READY public-API surfaces
   * (`CarrierApi` + ready blob — the resolvable state), while `B.vue` stays
   * IDE-role-only (an ordinary import of it ABSTAINS today, so offering it
   * would insert a specifier that does NOT resolve).
   */
  function resolvableCarrierManifest(): Manifest {
    const manifest = vueAndSvelteManifest();
    const project = manifest.projects[projectKey];
    project.owned_sources.push(
      {
        source_uri: "/ws/src/A.vue",
        provider_uri: "/ws/src/A.vue.verter.ts",
        role: "CarrierApi",
        script_kind: "TS",
      },
      {
        source_uri: "/ws/src/W.svelte",
        provider_uri: "/ws/src/W.svelte.verter.ts",
        role: "CarrierApi",
        script_kind: "TS",
      },
    );
    project.ready_files["/ws/src/A.vue.verter.ts"] = {
      content_hash: "aa1",
      version: 6,
      script_kind: "TS",
      role: "CarrierApi",
      map_hash: "0",
      blob_rel: "blobs/A.vue.verter.ts",
    };
    project.ready_files["/ws/src/W.svelte.verter.ts"] = {
      content_hash: "wa1",
      version: 4,
      script_kind: "TS",
      role: "CarrierApi",
      map_hash: "0",
      blob_rel: "blobs/W.svelte.verter.ts",
    };
    return manifest;
  }

  function completionHarness(options: { manifest: Manifest; diskFiles: Record<string, string> }) {
    const dir = track(
      writeStore(options.manifest, {
        "blobs/A.vue.verter.ts": "export default class A {}",
        "blobs/W.svelte.verter.ts": "export default class W {}",
      }),
    );
    const info = createInfo(dir, { diskFiles: options.diskFiles }, projectKey);
    info.config = {
      carrierStoreDir: dir,
      [EDITOR_OWNS_CARRIER_MEMBERSHIP_CONFIG_KEY]: true,
      carrierStoreRefreshToken: 1,
    };
    const opts = compilerOptions();
    info.project.getConfigFilePath = () => projectKey;
    info.project.getCompilerOptions = () => opts;
    info.languageServiceHost.getCompilationSettings = () => opts;
    info.languageServiceHost.getCurrentDirectory = info.project.getCurrentDirectory;
    info.languageServiceHost.getDefaultLibFileName = () => "/ws/no-lib.d.ts";
    info.languageServiceHost.getScriptFileNames = () => Object.keys(options.diskFiles);
    info.languageServiceHost.fileExists = info.serverHost.fileExists;
    info.languageServiceHost.readFile = info.serverHost.readFile;

    // tsserver's lifecycle: the project's LanguageService exists BEFORE the
    // plugin decorates the same host object. `getProgram` mirrors the real
    // service (the fake `createInfo` default pins it to `undefined`).
    const realLanguageService = ts.createLanguageService(info.languageServiceHost);
    info.languageService.__lsImpl = realLanguageService;
    info.languageService.getProgram = () => realLanguageService.getProgram();

    init({ typescript: ts } as any).create(info);
    return { info, opts };
  }

  it("offers each RESOLVABLE owned carrier by name (.vue AND .svelte) and the accepted specifier resolves", () => {
    const { info, opts } = completionHarness({
      manifest: resolvableCarrierManifest(),
      diskFiles: {
        [consumer]: consumerText,
        "/ws/src/A.vue": "<template/>",
        "/ws/src/W.svelte": "<script></script>",
        "/ws/src/B.vue": "<template/>",
        // On disk but ABSENT from the manifest: Rust marked it ambiguous
        // (carrier-path conflict), so importing it cannot resolve.
        "/ws/src/Conflicted.vue": "<template/>",
      },
    });

    const result = info.languageService.getCompletionsAtPosition(
      consumer,
      caret,
      undefined,
      undefined,
    ) as ts.CompletionInfo | undefined;
    expect(result).toBeDefined();
    const names = result!.entries.map((entry) => entry.name);

    // The user-reported defect: typing `from './` in a plain .ts must offer
    // the carrier BY NAME, for BOTH first-class frameworks.
    expect(names).toContain("A.vue");
    expect(names).toContain("W.svelte");

    // Fail closed: a carrier the manifest does not own cannot resolve, so it
    // is NOT offered — neither the conflicted one nor the IDE-role-only one.
    expect(names).not.toContain("Conflicted.vue");
    expect(names).not.toContain("B.vue");

    // Never a companion/virtual path in the offer list.
    expect(names).not.toContain("A.vue.verter.ts");
    expect(names).not.toContain("A.vue.tsx");
    expect(names).not.toContain("W.svelte.verter.ts");

    // The entry is a path completion: TypeScript's path-entry kind, the
    // carrier extension as kindModifiers, TS's location sort priority.
    const vueEntry = result!.entries.find((entry) => entry.name === "A.vue")!;
    expect(vueEntry.kind).toBe(ts.ScriptElementKind.scriptElement);
    expect(vueEntry.kindModifiers).toBe(".vue");
    expect(vueEntry.sortText).toBe("11");

    // Accepting the entry inserts the authored bare specifier `./A.vue`, and
    // that exact specifier RESOLVES through the same plugin host (to the
    // public-API surface, never the IDE companion).
    const resolved = info.languageServiceHost.resolveModuleNameLiterals(
      [{ text: "./A.vue" }],
      consumer,
      undefined,
      opts,
      undefined,
    )[0]?.resolvedModule;
    expect(resolved?.resolvedFileName).toBe("/ws/src/A.vue.verter.ts");
  });

  it("computes the replacement span from RAW source text (escaped windows separator)", () => {
    // The user's file contains: import W from ".\\W.sv";
    // Raw literal contents `.\\W.sv` (7 chars) cook to `.\W.sv` (6 chars) —
    // TypeScript collapses the escaped separator in `literal.text`. A span
    // computed from cooked offsets starts one raw character early, so
    // accepting `W.svelte` would splice `".\W.sveltev"` into the file.
    const text = 'import W from ".\\\\W.sv";\n';
    const caret = text.indexOf('W.sv"') + "W.sv".length;
    const { info } = completionHarness({
      manifest: resolvableCarrierManifest(),
      diskFiles: {
        [consumer]: text,
        "/ws/src/A.vue": "<template/>",
        "/ws/src/W.svelte": "<script></script>",
      },
    });

    const result = info.languageService.getCompletionsAtPosition(
      consumer,
      caret,
      undefined,
      undefined,
    ) as ts.CompletionInfo | undefined;
    const entry = result?.entries.find((candidate) => candidate.name === "W.svelte");
    expect(entry).toBeDefined();

    // The RAW basename `W.sv` — the actual source characters the accept must
    // replace — starts AFTER both raw backslashes.
    expect(entry!.replacementSpan).toEqual({ start: text.indexOf("W.sv"), length: 4 });

    // Accepting the entry yields a clean, still-escaped specifier — never the
    // corrupted `".\W.sveltev"`.
    const span = entry!.replacementSpan!;
    const accepted = text.slice(0, span.start) + entry!.name + text.slice(span.start + span.length);
    expect(accepted).toBe('import W from ".\\\\W.svelte";\n');
  });

  it("does not offer an owned carrier whose import surface has not published yet (warm-up)", () => {
    const manifest = resolvableCarrierManifest();
    // Ownership lands BEFORE content publishes: an owned CarrierApi row with
    // NO ready_files entry is the warm-up window. Offering it would insert a
    // specifier the non-blocking resolution arms cannot serve yet.
    manifest.projects[projectKey].owned_sources.push({
      source_uri: "/ws/src/Warm.vue",
      provider_uri: "/ws/src/Warm.vue.verter.ts",
      role: "CarrierApi",
      script_kind: "TS",
    });
    const { info } = completionHarness({
      manifest,
      diskFiles: {
        [consumer]: consumerText,
        "/ws/src/A.vue": "<template/>",
        "/ws/src/W.svelte": "<script></script>",
        "/ws/src/Warm.vue": "<template/>",
      },
    });

    const result = info.languageService.getCompletionsAtPosition(
      consumer,
      caret,
      undefined,
      undefined,
    ) as ts.CompletionInfo | undefined;
    const names = result?.entries.map((entry) => entry.name) ?? [];
    expect(names).not.toContain("Warm.vue");
    // The READY carriers in the same directory are still offered.
    expect(names).toContain("A.vue");
    expect(names).toContain("W.svelte");
  });

  it("does not offer carriers outside a module-specifier string (plain string literal)", () => {
    const text = 'const s = "./";\n';
    const pos = text.indexOf('"./') + '"./'.length;
    const { info } = completionHarness({
      manifest: resolvableCarrierManifest(),
      diskFiles: {
        "/ws/src/consumer.ts": text,
        "/ws/src/A.vue": "<template/>",
      },
    });

    const result = info.languageService.getCompletionsAtPosition(
      "/ws/src/consumer.ts",
      pos,
      undefined,
      undefined,
    ) as ts.CompletionInfo | undefined;
    const names = result?.entries.map((entry) => entry.name) ?? [];
    expect(names).not.toContain("A.vue");
  });

  it("offers carriers from a SUBDIRECTORY fragment and none from the wrong directory", () => {
    const text = 'import Comp from "./components/";\n';
    const pos = text.indexOf('"./components/') + '"./components/'.length;
    const manifest = resolvableCarrierManifest();
    const project = manifest.projects[projectKey];
    project.owned_sources.push({
      source_uri: "/ws/src/components/Nested.vue",
      provider_uri: "/ws/src/components/Nested.vue.verter.ts",
      role: "CarrierApi",
      script_kind: "TS",
    });
    project.ready_files["/ws/src/components/Nested.vue.verter.ts"] = {
      content_hash: "n1",
      version: 1,
      script_kind: "TS",
      role: "CarrierApi",
      map_hash: "0",
      blob_rel: "blobs/A.vue.verter.ts",
    };
    const { info } = completionHarness({
      manifest,
      diskFiles: {
        "/ws/src/consumer.ts": text,
        "/ws/src/A.vue": "<template/>",
        "/ws/src/components/Nested.vue": "<template/>",
      },
    });

    const result = info.languageService.getCompletionsAtPosition(
      "/ws/src/consumer.ts",
      pos,
      undefined,
      undefined,
    ) as ts.CompletionInfo | undefined;
    const names = result?.entries.map((entry) => entry.name) ?? [];
    expect(names).toContain("Nested.vue");
    // Carriers owned in the PARENT directory must not leak into the subdir list.
    expect(names).not.toContain("A.vue");
  });
});
