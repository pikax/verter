/**
 * The in-context TypeScript LanguageService web worker: the REAL
 * `ts.createLanguageService` (the pinned, statically-bundled local
 * `typescript` — never a CDN engine) running over Verter's generated
 * carriers, served through the shared CORE carrier contracts:
 *
 * - every carrier SOURCE contributes its three WASM-produced surfaces (IDE
 *   `X.vue.tsx` root, declaration `X.d.vue.ts`, API `X.vue.verter.ts`)
 *   through the in-memory `CarrierStoreReader`;
 * - plain `.ts`/`.tsx` sources are user program members as-is;
 * - lib + Vue/Verter type assets come bundled from pinned local packages
 *   (`tsLibs`), with IndexedDB as a version-keyed runtime cache only;
 * - the `capabilityForWasm` gate keeps the produce path un-invoked for a
 *   TS>=7 engine (carrier-gen + Verter-native only — no external-TS).
 */

/// <reference lib="webworker" />

import * as tsModule from "typescript";
import type * as tsNs from "typescript";
import { normalizePath } from "@verter/language-shared";
import { InMemoryCarrierStore, type CarrierSurfaces } from "./carrierStore";
import {
  createGatedInContextLanguageService,
  type FallbackFs,
  type InContextLs,
  type UserFileEntry,
} from "./inContextLs";
import { bundledWorkerFiles, seedLibCache, VUE_JSX_GLOBAL_PATH, WORKER_LIB_DIR } from "./tsLibs";

declare const self: DedicatedWorkerGlobalScope;

const ts = tsModule as typeof tsNs;

// ── Worker state ──

const store = new InMemoryCarrierStore("playground://project");
const userFiles = new Map<string, UserFileEntry>();
let inContext: InContextLs | null = null;

function setUserFile(path: string, content: string): void {
  const normalized = normalizePath(path);
  const existing = userFiles.get(normalized);
  userFiles.set(normalized, { content, version: (existing?.version ?? 0) + 1 });
}

/** Static asset tree (libs + type packages) + directory index. */
function buildStaticFs(staticFiles: Map<string, string>): FallbackFs {
  const directories = new Set<string>();
  for (const path of staticFiles.keys()) {
    let dir = path;
    while (dir.includes("/")) {
      dir = dir.slice(0, dir.lastIndexOf("/"));
      if (dir === "" || directories.has(dir)) break;
      directories.add(dir);
    }
  }
  return {
    fileExists: (path) => staticFiles.has(normalizePath(path)),
    readFile: (path) => staticFiles.get(normalizePath(path)),
    directoryExists: (path) => directories.has(normalizePath(path).replace(/\/+$/, "")),
    getDirectories: () => [],
    useCaseSensitiveFileNames: true,
    getDefaultLibFileName: (options) => `${WORKER_LIB_DIR}/${ts.getDefaultLibFileName(options)}`,
  };
}

// ── Initialization ──

function init(payload?: { verterTypesContent?: string }): {
  inContextLS: boolean;
  tsVersion: string;
} {
  const staticFiles = bundledWorkerFiles();
  if (payload?.verterTypesContent) {
    staticFiles.set(
      "/node_modules/@verter/types/package.json",
      JSON.stringify({ name: "@verter/types", version: "0.0.0-bundled", types: "index.d.ts" }),
    );
    staticFiles.set("/node_modules/@verter/types/index.d.ts", payload.verterTypesContent);
  }

  // The vue global-JSX registration must be a program ROOT (nothing imports it).
  userFiles.set(VUE_JSX_GLOBAL_PATH, {
    content: staticFiles.get(VUE_JSX_GLOBAL_PATH) ?? "",
    version: 1,
  });

  // Version-keyed runtime cache seeding — best-effort, bundle stays authoritative.
  void seedLibCache(ts.version, staticFiles);

  // The capability gate: for a TS>=7 engine this returns null WITHOUT
  // constructing the service — no produce path exists behind a closed gate.
  inContext = createGatedInContextLanguageService({
    ts,
    store,
    userFiles,
    currentDirectory: "/",
    fallbackFs: buildStaticFs(staticFiles),
    defaultLibDir: WORKER_LIB_DIR,
  });

  return { inContextLS: inContext !== null, tsVersion: ts.version };
}

// ── Query helpers ──

interface SpanOut {
  fileName: string;
  start: number;
  length: number;
}

function diagnosticsFor(path: string) {
  if (!inContext) return [];
  const semantic = inContext.languageService.getSemanticDiagnostics(path);
  const syntactic = inContext.languageService.getSyntacticDiagnostics(path);
  return [...syntactic, ...semantic].map((d) => ({
    message: ts.flattenDiagnosticMessageText(d.messageText, "\n"),
    start: d.start ?? 0,
    length: d.length ?? 0,
    category: d.category, // 0=Warning, 1=Error, 2=Suggestion, 3=Message
    code: d.code,
    fileName: d.file === undefined ? path : normalizePath(d.file.fileName),
  }));
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

self.onmessage = (event: MessageEvent<WorkerMessage>) => {
  const { id, type, payload } = event.data;
  const respond = (result?: unknown, error?: string) => {
    self.postMessage({ id, result, error } satisfies WorkerResponse);
  };
  const ls = () => inContext?.languageService ?? null;

  try {
    switch (type) {
      case "init": {
        respond(init(payload));
        break;
      }

      case "syncSource": {
        // ONE atomic message per source: either the three carrier surfaces
        // (framework carrier) or the raw content (plain user file).
        const { sourcePath, surfaces, userContent } = payload as {
          sourcePath: string;
          surfaces?: CarrierSurfaces;
          userContent?: string;
        };
        if (surfaces !== undefined) {
          store.upsertSource(sourcePath, surfaces);
        }
        if (typeof userContent === "string") {
          setUserFile(sourcePath, userContent);
        }
        respond("ok");
        break;
      }

      case "removeSource": {
        const { sourcePath } = payload as { sourcePath: string };
        store.removeSource(sourcePath);
        userFiles.delete(normalizePath(sourcePath));
        respond("ok");
        break;
      }

      case "checkStandalone": {
        // The editable output panel: a scratch STANDALONE user file checked
        // as its own program member (raw, unmapped diagnostics by design).
        const { path, content } = payload as { path: string; content: string };
        setUserFile(path, content);
        respond(ls() === null ? [] : diagnosticsFor(path));
        break;
      }

      case "getDiagnostics": {
        const { path } = payload as { path: string };
        respond(ls() === null ? [] : diagnosticsFor(path));
        break;
      }

      case "getHover": {
        const service = ls();
        if (!service) {
          respond(null);
          break;
        }
        const { path, offset } = payload;
        const info = service.getQuickInfoAtPosition(path, offset);
        if (!info) {
          respond(null);
          break;
        }
        respond({
          text: info.displayParts?.map((p) => p.text).join("") ?? "",
          documentation: info.documentation?.map((p) => p.text).join("\n") ?? "",
          start: info.textSpan.start,
          length: info.textSpan.length,
        });
        break;
      }

      case "getCompletions": {
        const service = ls();
        if (!service) {
          respond([]);
          break;
        }
        const { path, offset } = payload;
        const completions = service.getCompletionsAtPosition(path, offset, undefined);
        respond(
          (completions?.entries ?? []).slice(0, 100).map((e) => ({
            label: e.name,
            kind: e.kind,
            sortText: e.sortText,
            isRecommended: e.isRecommended,
          })),
        );
        break;
      }

      case "getDefinition": {
        const service = ls();
        if (!service) {
          respond(null);
          break;
        }
        const { path, offset } = payload;
        const defs = service.getDefinitionAtPosition(path, offset);
        if (!defs || defs.length === 0) {
          respond(null);
          break;
        }
        respond(
          defs.map(
            (d): SpanOut => ({
              fileName: normalizePath(d.fileName),
              start: d.textSpan.start,
              length: d.textSpan.length,
            }),
          ),
        );
        break;
      }

      case "getReferences": {
        const service = ls();
        if (!service) {
          respond([]);
          break;
        }
        const { path, offset } = payload;
        // findReferences: its entries carry `isDefinition` (the flat
        // getReferencesAtPosition entries dropped it from the public type).
        const symbols = service.findReferences(path, offset) ?? [];
        respond(
          symbols.flatMap((symbol) =>
            symbol.references.map((r) => ({
              fileName: normalizePath(r.fileName),
              start: r.textSpan.start,
              length: r.textSpan.length,
              isDefinition: r.isDefinition ?? false,
            })),
          ),
        );
        break;
      }

      case "getDocumentHighlights": {
        const service = ls();
        if (!service) {
          respond([]);
          break;
        }
        const { path, offset } = payload;
        const docs = service.getDocumentHighlights(path, offset, [path]) ?? [];
        respond(
          docs.flatMap((doc) =>
            doc.highlightSpans.map((span) => ({
              fileName: normalizePath(doc.fileName),
              start: span.textSpan.start,
              length: span.textSpan.length,
              kind: span.kind,
            })),
          ),
        );
        break;
      }

      case "getRenameLocations": {
        const service = ls();
        if (!service) {
          respond({
            canRename: false,
            localizedErrorMessage: "TypeScript service is not initialized",
            triggerSpan: null,
            locations: [],
          });
          break;
        }
        const { path, offset } = payload;
        const info = service.getRenameInfo(path, offset, { allowRenameOfImportPath: false });
        if (!info.canRename) {
          respond({
            canRename: false,
            localizedErrorMessage: info.localizedErrorMessage ?? "Symbol cannot be renamed",
            triggerSpan: null,
            locations: [],
          });
          break;
        }
        const locations = service.findRenameLocations(path, offset, false, false, true) ?? [];
        respond({
          canRename: true,
          localizedErrorMessage: null,
          triggerSpan: info.triggerSpan
            ? { start: info.triggerSpan.start, length: info.triggerSpan.length }
            : null,
          locations: locations.map(
            (loc): SpanOut => ({
              fileName: normalizePath(loc.fileName),
              start: loc.textSpan.start,
              length: loc.textSpan.length,
            }),
          ),
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
