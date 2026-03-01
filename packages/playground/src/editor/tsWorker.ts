/**
 * TypeScript LanguageService running in a web worker.
 * Provides type checking, hover, and completions for Verter TSX output.
 */

/// <reference lib="webworker" />

import type ts from "typescript";

// TypeScript will be loaded dynamically via importScripts or import()
declare const self: DedicatedWorkerGlobalScope;

// ── Virtual file system ──

interface VirtualFile {
  content: string;
  version: number;
}

const files = new Map<string, VirtualFile>();
let tsLib: typeof ts | null = null;
let languageService: ts.LanguageService | null = null;

const TS_CDN_BASE = "https://cdn.jsdelivr.net/npm/typescript@5/lib/";
const VUE_CDN_BASE = "https://cdn.jsdelivr.net/npm/";

// Vue packages whose .d.ts files we need for type checking
const VUE_TYPE_PACKAGES = [
  { pkg: "vue", file: "dist/vue.d.ts", path: "/node_modules/vue/index.d.ts" },
  { pkg: "vue", file: "jsx.d.ts", path: "/node_modules/vue/jsx.d.ts" },
  { pkg: "vue", file: "jsx-runtime/index.d.ts", path: "/node_modules/vue/jsx-runtime/index.d.ts" },
  { pkg: "@vue/runtime-dom", file: "dist/runtime-dom.d.ts", path: "/node_modules/@vue/runtime-dom/index.d.ts" },
  { pkg: "@vue/runtime-core", file: "dist/runtime-core.d.ts", path: "/node_modules/@vue/runtime-core/index.d.ts" },
  { pkg: "@vue/reactivity", file: "dist/reactivity.d.ts", path: "/node_modules/@vue/reactivity/index.d.ts" },
  { pkg: "@vue/shared", file: "dist/shared.d.ts", path: "/node_modules/@vue/shared/index.d.ts" },
];

// Lib files to load for a reasonable type checking experience
const LIB_FILES = [
  "lib.es5.d.ts",
  "lib.es2015.d.ts",
  "lib.es2015.core.d.ts",
  "lib.es2015.collection.d.ts",
  "lib.es2015.iterable.d.ts",
  "lib.es2015.generator.d.ts",
  "lib.es2015.promise.d.ts",
  "lib.es2015.proxy.d.ts",
  "lib.es2015.reflect.d.ts",
  "lib.es2015.symbol.d.ts",
  "lib.es2015.symbol.wellknown.d.ts",
  "lib.es2016.d.ts",
  "lib.es2016.array.include.d.ts",
  "lib.es2017.d.ts",
  "lib.es2017.object.d.ts",
  "lib.es2017.string.d.ts",
  "lib.es2018.d.ts",
  "lib.es2018.asyncgenerator.d.ts",
  "lib.es2018.asynciterable.d.ts",
  "lib.es2018.promise.d.ts",
  "lib.es2019.d.ts",
  "lib.es2019.array.d.ts",
  "lib.es2019.object.d.ts",
  "lib.es2019.string.d.ts",
  "lib.es2019.symbol.d.ts",
  "lib.es2020.d.ts",
  "lib.es2020.bigint.d.ts",
  "lib.es2020.promise.d.ts",
  "lib.es2020.string.d.ts",
  "lib.es2020.symbol.wellknown.d.ts",
  "lib.dom.d.ts",
  "lib.dom.iterable.d.ts",
];

// ── Lib file caching via IndexedDB ──

const DB_NAME = "verter-ts-libs";
const DB_VERSION = 1;
const STORE_NAME = "libs";

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);
    req.onupgradeneeded = () => {
      req.result.createObjectStore(STORE_NAME);
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function getCachedLib(db: IDBDatabase, name: string): Promise<string | null> {
  return new Promise((resolve) => {
    const tx = db.transaction(STORE_NAME, "readonly");
    const store = tx.objectStore(STORE_NAME);
    const req = store.get(name);
    req.onsuccess = () => resolve(req.result ?? null);
    req.onerror = () => resolve(null);
  });
}

async function setCachedLib(db: IDBDatabase, name: string, content: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    store.put(content, name);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

async function fetchLibFile(name: string, db: IDBDatabase): Promise<string> {
  const cached = await getCachedLib(db, name);
  if (cached) return cached;

  const resp = await fetch(`${TS_CDN_BASE}${name}`);
  if (!resp.ok) throw new Error(`Failed to fetch ${name}: ${resp.status}`);
  const content = await resp.text();
  await setCachedLib(db, name, content).catch(() => {
    // Cache write failure is non-fatal
  });
  return content;
}

// ── LanguageServiceHost ──

function createLanguageServiceHost(): ts.LanguageServiceHost {
  const ts = tsLib!;
  return {
    getCompilationSettings() {
      return {
        target: ts.ScriptTarget.ESNext,
        module: ts.ModuleKind.ESNext,
        moduleResolution: ts.ModuleResolutionKind.Bundler,
        jsx: ts.JsxEmit.Preserve,
        jsxImportSource: "vue",
        strict: true,
        noUnusedLocals: true,
        noUnusedParameters: true,
        esModuleInterop: true,
        allowJs: true,
        noEmit: true,
        skipLibCheck: true,
        lib: ["lib.es2020.d.ts", "lib.dom.d.ts"],
      };
    },
    getScriptFileNames() {
      return [...files.keys()];
    },
    getScriptVersion(fileName) {
      return String(files.get(fileName)?.version ?? 0);
    },
    getScriptSnapshot(fileName) {
      const file = files.get(fileName);
      if (!file) return undefined;
      return ts.ScriptSnapshot.fromString(file.content);
    },
    getCurrentDirectory: () => "/",
    getDefaultLibFileName: () => "/lib/lib.es2020.d.ts",
    fileExists(fileName) {
      return files.has(fileName);
    },
    readFile(fileName) {
      return files.get(fileName)?.content;
    },
  };
}

// ── Vue type loading ──

let currentVueVersion = "3.5";

async function fetchVueTypes(vueVersion: string, db: IDBDatabase): Promise<void> {
  currentVueVersion = vueVersion;

  const typePromises = VUE_TYPE_PACKAGES.map(async ({ pkg, file, path }) => {
    const cacheKey = `vue-types:${pkg}@${vueVersion}/${file}`;
    try {
      // Try IndexedDB cache first
      const cached = await getCachedLib(db, cacheKey);
      if (cached) {
        files.set(path, { content: cached, version: 1 });
        return;
      }

      // Fetch from CDN
      const url = `${VUE_CDN_BASE}${pkg}@${vueVersion}/${file}`;
      const resp = await fetch(url);
      if (!resp.ok) throw new Error(`${resp.status}`);
      const content = await resp.text();

      files.set(path, { content, version: 1 });
      await setCachedLib(db, cacheKey, content).catch(() => {});
    } catch {
      // Non-fatal — type checking will be incomplete
    }
  });

  await Promise.all(typePromises);
}

// ── Initialization ──

async function init(payload?: { verterTypesContent?: string; vueVersion?: string }): Promise<void> {
  // Load TypeScript from CDN
  // @ts-expect-error -- dynamic import from CDN
  const tsModule = await import("https://cdn.jsdelivr.net/npm/typescript@5/+esm");
  tsLib = tsModule.default ?? tsModule;

  const db = await openDb();

  // Load lib files
  const libPromises = LIB_FILES.map(async (name) => {
    try {
      const content = await fetchLibFile(name, db);
      files.set(`/lib/${name}`, { content, version: 1 });
    } catch {
      // Skip failed lib files — type checking will be incomplete but functional
    }
  });

  // Load Vue types from CDN
  const vueVersion = payload?.vueVersion ?? "3.5";
  const vueTypesPromise = fetchVueTypes(vueVersion, db);

  await Promise.all([...libPromises, vueTypesPromise]);
  db.close();

  // Add Verter type helpers if provided
  if (payload?.verterTypesContent) {
    files.set("/node_modules/@verter/types/index.d.ts", {
      content: payload.verterTypesContent,
      version: 1,
    });
  }

  // Create the language service
  languageService = tsLib!.createLanguageService(createLanguageServiceHost());
}

async function updateVueTypes(vueVersion: string): Promise<void> {
  if (vueVersion === currentVueVersion) return;

  const db = await openDb();
  await fetchVueTypes(vueVersion, db);
  db.close();

  // Recreate language service with updated files
  if (tsLib) {
    languageService = tsLib.createLanguageService(createLanguageServiceHost());
  }
}

// ── Worker message handling ──

interface WorkerMessage {
  id: number;
  type: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  payload: any;
}

interface WorkerResponse {
  id: number;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  result?: any;
  error?: string;
}

self.onmessage = async (event: MessageEvent<WorkerMessage>) => {
  const { id, type, payload } = event.data;
  const respond = (result?: unknown, error?: string) => {
    self.postMessage({ id, result, error } satisfies WorkerResponse);
  };

  try {
    switch (type) {
      case "init": {
        await init(payload);
        respond("ok");
        break;
      }

      case "updateVueTypes": {
        const { vueVersion } = payload;
        await updateVueTypes(vueVersion);
        respond("ok");
        break;
      }

      case "openFile": {
        const { path, content } = payload;
        const existing = files.get(path);
        files.set(path, { content, version: (existing?.version ?? 0) + 1 });
        respond("ok");
        break;
      }

      case "updateFile": {
        const { path, content } = payload;
        const existing = files.get(path);
        files.set(path, { content, version: (existing?.version ?? 0) + 1 });
        respond("ok");
        break;
      }

      case "closeFile": {
        const { path } = payload;
        // Only remove user files, not lib files
        if (!path.startsWith("/lib/")) {
          files.delete(path);
        }
        respond("ok");
        break;
      }

      case "getDiagnostics": {
        if (!languageService) {
          respond([]);
          break;
        }
        const { path } = payload;
        const semantic = languageService.getSemanticDiagnostics(path);
        const syntactic = languageService.getSyntacticDiagnostics(path);
        const all = [...syntactic, ...semantic].map((d) => ({
          message: tsLib!.flattenDiagnosticMessageText(d.messageText, "\n"),
          start: d.start ?? 0,
          length: d.length ?? 0,
          category: d.category, // 0=Warning, 1=Error, 2=Suggestion, 3=Message
          code: d.code,
        }));
        respond(all);
        break;
      }

      case "getHover": {
        if (!languageService) {
          respond(null);
          break;
        }
        const { path, offset } = payload;
        const info = languageService.getQuickInfoAtPosition(path, offset);
        if (!info) {
          respond(null);
          break;
        }
        const displayParts = info.displayParts?.map((p) => p.text).join("") ?? "";
        const documentation = info.documentation?.map((p) => p.text).join("\n") ?? "";
        respond({
          text: displayParts,
          documentation,
          start: info.textSpan.start,
          length: info.textSpan.length,
        });
        break;
      }

      case "getCompletions": {
        if (!languageService) {
          respond([]);
          break;
        }
        const { path, offset } = payload;
        const completions = languageService.getCompletionsAtPosition(path, offset, undefined);
        if (!completions) {
          respond([]);
          break;
        }
        // Return a limited set to avoid huge payloads
        const items = completions.entries.slice(0, 100).map((e) => ({
          label: e.name,
          kind: e.kind, // TS ScriptElementKind string
          sortText: e.sortText,
          isRecommended: e.isRecommended,
        }));
        respond(items);
        break;
      }

      case "getDefinition": {
        if (!languageService) {
          respond(null);
          break;
        }
        const { path, offset } = payload;
        const defs = languageService.getDefinitionAtPosition(path, offset);
        if (!defs || defs.length === 0) {
          respond(null);
          break;
        }
        respond(
          defs.map((d) => ({
            fileName: d.fileName,
            start: d.textSpan.start,
            length: d.textSpan.length,
          })),
        );
        break;
      }

      case "getReferences": {
        if (!languageService) {
          respond([]);
          break;
        }
        const { path, offset } = payload;
        const refs = languageService.getReferencesAtPosition(path, offset);
        if (!refs || refs.length === 0) {
          respond([]);
          break;
        }
        respond(
          refs.map((r) => ({
            fileName: r.fileName,
            start: r.textSpan.start,
            length: r.textSpan.length,
          })),
        );
        break;
      }

      case "getDocumentHighlights": {
        if (!languageService) {
          respond([]);
          break;
        }
        const { path, offset } = payload;
        const docs = languageService.getDocumentHighlights(path, offset, [path]);
        if (!docs || docs.length === 0) {
          respond([]);
          break;
        }
        const highlights = docs.flatMap((doc) =>
          doc.highlightSpans.map((span) => ({
            fileName: doc.fileName,
            start: span.textSpan.start,
            length: span.textSpan.length,
            kind: span.kind,
          })),
        );
        respond(highlights);
        break;
      }

      case "getRenameLocations": {
        if (!languageService) {
          respond({
            canRename: false,
            localizedErrorMessage: "TypeScript service is not initialized",
            triggerSpan: null,
            locations: [],
          });
          break;
        }
        const { path, offset } = payload;
        const info = languageService.getRenameInfo(path, offset, {
          allowRenameOfImportPath: false,
        });
        if (!info.canRename) {
          respond({
            canRename: false,
            localizedErrorMessage: info.localizedErrorMessage ?? "Symbol cannot be renamed",
            triggerSpan: null,
            locations: [],
          });
          break;
        }

        const locations = languageService.findRenameLocations(path, offset, false, false, true);
        respond({
          canRename: true,
          localizedErrorMessage: null,
          triggerSpan: info.triggerSpan
            ? { start: info.triggerSpan.start, length: info.triggerSpan.length }
            : null,
          locations: (locations ?? []).map((loc) => ({
            fileName: loc.fileName,
            start: loc.textSpan.start,
            length: loc.textSpan.length,
          })),
        });
        break;
      }

      default:
        respond(undefined, `Unknown message type: ${type}`);
    }
  } catch (err) {
    respond(undefined, err instanceof Error ? err.message : String(err));
  }
};
