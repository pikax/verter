/**
 * Main-thread bridge to the in-context TypeScript LanguageService worker
 * (`tsWorker.ts`). Speaks the carrier protocol:
 *
 * - **Sync model** — ONE atomic `syncSource` per source: a framework carrier
 *   (`.vue`/`.svelte`) pushes its three WASM-produced surfaces (IDE
 *   `X.vue.tsx`, declaration `X.d.vue.ts`, API `X.vue.verter.ts`); a plain
 *   `.ts`/`.js` file pushes its raw content as a user program member. A
 *   removed source sends `removeSource`.
 * - **Result mapping** — every worker result span carries a `fileName` and
 *   maps through the CORE strict `CarrierMapper` registered for THAT file in
 *   a `CarrierMapperSet`. A span with no mapper, in synthetic (unmapped)
 *   generated space, or landing in a different source DROPS (fail closed) —
 *   never a closest-segment snap, never a single-active-file assumption.
 * - **Query direction** — a source offset translates through the strict
 *   source→generated direction of the active carrier's mapper onto the IDE
 *   carrier path; an unmapped source position fails closed without touching
 *   the worker.
 * - **`checkStandalone`** — the editable-output panel's scratch file, checked
 *   raw and unmapped BY DESIGN (it edits generated TSX, not a carrier
 *   source). This is the ONLY raw path.
 */
import { CarrierMapperSet, normalizePath, toIdeCarrierFileName } from "@verter/language-shared";
import { createCarrierMapper } from "./carrierMappers";
import type { CarrierSurfaces } from "./carrierStore";
import type { TypeScriptServiceBridge } from "./lspProviders";

interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
}

/** One diagnostic as the worker reports it (generated/user-file space). */
interface WorkerDiagnostic {
  message: string;
  start: number;
  length: number;
  category: number; // 0=Warning, 1=Error, 2=Suggestion, 3=Message
  code: number;
  /** The file the diagnostic belongs to — results map BY this. */
  fileName: string;
}

interface TsHoverInfo {
  text: string;
  documentation: string;
  start: number;
  length: number;
}

interface TsCompletionEntry {
  label: string;
  kind: string;
  sortText: string;
  isRecommended?: boolean;
}

interface TsSpanLike {
  fileName: string;
  start: number;
  length: number;
}

interface TsReferenceLike extends TsSpanLike {
  isDefinition?: boolean;
}

interface TsRenameResponse {
  canRename: boolean;
  localizedErrorMessage: string | null;
  triggerSpan: { start: number; length: number } | null;
  locations: TsSpanLike[];
}

export interface RawTsDiagnostic {
  message: string;
  /** TSX offset (start) — raw, NOT mapped (editable output panel only). */
  start: number;
  /** TSX offset (end). */
  end: number;
  severity: "error" | "warning" | "info";
  code: number;
}

export interface MappedDiagnostic {
  message: string;
  /** Source (`.vue`/`.svelte`/user-file) UTF-16 offset (start). */
  start: number;
  /** Source UTF-16 offset (end). */
  end: number;
  severity: "error" | "warning" | "info";
  code: number;
}

export interface MappedSpan {
  start: number;
  end: number;
}

export interface MappedReference extends MappedSpan {
  isDefinition: boolean;
}

export interface RenameLocations {
  canRename: boolean;
  rejectReason?: string;
  triggerSpan: MappedSpan | null;
  locations: MappedSpan[];
}

/** The compiled surfaces the sync model reads off a playground file. */
export interface SyncableCompiledSurfaces {
  types: string;
  typesSourceMap: string;
  declCode: string;
  declSourceMap: string;
  tscCode: string;
}

/** A playground workspace file, structurally (`core/types.File` satisfies it). */
export interface SyncableFile {
  filename: string;
  code: string;
  compiled: SyncableCompiledSurfaces;
}

/** The last payload pushed for a source (skip re-sends of unchanged content). */
type PushedPayload =
  | {
      kind: "carrier";
      types: string;
      typesSourceMap: string;
      declCode: string;
      declSourceMap: string;
      tscCode: string;
      sourceCode: string;
    }
  | { kind: "user"; content: string };

// TS ScriptElementKind → Monaco CompletionItemKind (approximate)
const TS_KIND_TO_MONACO: Record<string, number> = {
  keyword: 17, // Keyword
  script: 17,
  module: 8, // Module
  class: 5, // Class
  "local class": 5,
  interface: 7, // Interface
  type: 7,
  enum: 15, // Enum
  "enum member": 19, // EnumMember
  variable: 11, // Variable
  "local variable": 11,
  function: 2, // Function
  "local function": 2,
  method: 1, // Method
  getter: 9, // Property
  setter: 9,
  property: 9,
  constructor: 3, // Constructor
  parameter: 11,
  "type parameter": 24, // TypeParameter
  "primitive type": 14, // Value
  label: 0, // Text
  alias: 7,
  const: 20, // Constant
  let: 11,
  warning: 0,
  string: 14,
  directory: 18, // Folder
  "external module name": 8,
  "JSX attribute": 9,
  link: 0,
};

const USER_FILE_EXTENSIONS = /\.(ts|tsx|js|jsx)$/;

/** The editable-output panel's scratch path (raw diagnostics by design). */
const STANDALONE_SCRATCH_PATH = "/__standalone__/direct-edit.tsx";

function categoryToSeverity(category: number): MappedDiagnostic["severity"] {
  return category === 1 ? "error" : category === 0 ? "warning" : "info";
}

export class TypeScriptService implements TypeScriptServiceBridge {
  private worker: Worker | null = null;
  private requestId = 0;
  private pending = new Map<number, PendingRequest>();
  private initialized = false;
  private initPromise: Promise<void> | null = null;

  /** Per-carrier strict mappers, keyed by the IDE carrier provider path. */
  private readonly mappers = new CarrierMapperSet();
  /** sourcePath → last pushed payload. */
  private readonly synced = new Map<string, PushedPayload>();

  async init(options?: { verterTypesContent?: string; vueVersion?: string }): Promise<void> {
    if (this.initialized) return;
    if (this.initPromise) return this.initPromise;

    this.initPromise = this._init(options);
    return this.initPromise;
  }

  private async _init(options?: {
    verterTypesContent?: string;
    vueVersion?: string;
  }): Promise<void> {
    this.worker = new Worker(new URL("./tsWorker.ts", import.meta.url), {
      type: "module",
    });

    this.worker.onmessage = (e: MessageEvent<{ id: number; result?: unknown; error?: string }>) => {
      const { id, result, error } = e.data;
      const pending = this.pending.get(id);
      if (pending) {
        this.pending.delete(id);
        if (error) {
          pending.reject(new Error(error));
        } else {
          pending.resolve(result);
        }
      }
    };

    await this.send("init", { verterTypesContent: options?.verterTypesContent });
    this.initialized = true;
  }

  private send(type: string, payload?: unknown): Promise<unknown> {
    return new Promise((resolve, reject) => {
      if (!this.worker) {
        reject(new Error("Worker not initialized"));
        return;
      }
      const id = ++this.requestId;
      this.pending.set(id, { resolve, reject });
      this.worker.postMessage({ id, type, payload });
    });
  }

  // ── Sync model ──

  /** Worker-absolute path for a playground filename. */
  private sourcePathFor(filename: string): string {
    const normalized = normalizePath(filename);
    return normalized.startsWith("/") ? normalized : `/${normalized}`;
  }

  /** How a workspace file enters the worker program (`null` = not syncable). */
  private routeFor(sourcePath: string): "carrier" | "user" | null {
    if (toIdeCarrierFileName(sourcePath) !== null) return "carrier";
    if (USER_FILE_EXTENSIONS.test(sourcePath)) return "user";
    return null;
  }

  /**
   * Push the whole workspace: one atomic `syncSource` per changed source,
   * `removeSource` for sources that vanished. Unchanged sources are skipped.
   */
  async syncWorkspace(files: Iterable<SyncableFile>): Promise<void> {
    if (!this.initialized) return;
    const seen = new Set<string>();
    for (const file of files) {
      const sourcePath = this.sourcePathFor(file.filename);
      if (this.routeFor(sourcePath) === null) continue;
      seen.add(sourcePath);
      await this.syncSource(file);
    }
    for (const sourcePath of [...this.synced.keys()]) {
      if (!seen.has(sourcePath)) {
        await this.removeSourceByPath(sourcePath);
      }
    }
  }

  /** Push ONE source's current content/surfaces (no-op when unchanged). */
  async syncSource(file: SyncableFile): Promise<void> {
    if (!this.initialized) return;
    const sourcePath = this.sourcePathFor(file.filename);
    const route = this.routeFor(sourcePath);
    if (route === null) return;

    if (route === "user") {
      const payload: PushedPayload = { kind: "user", content: file.code };
      const previous = this.synced.get(sourcePath);
      if (previous?.kind === "user" && previous.content === payload.content) return;
      await this.send("syncSource", { sourcePath, userContent: file.code });
      this.synced.set(sourcePath, payload);
      return;
    }

    const { types, typesSourceMap, declCode, declSourceMap, tscCode } = file.compiled;
    const payload: PushedPayload = {
      kind: "carrier",
      types,
      typesSourceMap,
      declCode,
      declSourceMap,
      tscCode,
      sourceCode: file.code,
    };
    const previous = this.synced.get(sourcePath);
    if (
      previous?.kind === "carrier" &&
      previous.types === types &&
      previous.typesSourceMap === typesSourceMap &&
      previous.declCode === declCode &&
      previous.declSourceMap === declSourceMap &&
      previous.tscCode === tscCode &&
      previous.sourceCode === file.code
    ) {
      return;
    }

    // A surface the compile no longer produces is OMITTED — the worker store
    // retires it atomically (fail closed; no stale carrier lingers).
    const surfaces: CarrierSurfaces = {
      ide: types ? { code: types, sourceMap: typesSourceMap || null } : undefined,
      decl: declCode ? { code: declCode, sourceMap: declSourceMap || null } : undefined,
      api: tscCode ? { code: tscCode, sourceMap: null } : undefined,
    };
    await this.send("syncSource", { sourcePath, surfaces });
    this.synced.set(sourcePath, payload);
    this.refreshCarrierMapper(sourcePath, file);
  }

  /** Retire a source (delete / rename cleanup): all carriers + the user file. */
  async removeSource(filename: string): Promise<void> {
    if (!this.initialized) return;
    await this.removeSourceByPath(this.sourcePathFor(filename));
  }

  private async removeSourceByPath(sourcePath: string): Promise<void> {
    await this.send("removeSource", { sourcePath });
    this.synced.delete(sourcePath);
    const carrierPath = toIdeCarrierFileName(sourcePath);
    if (carrierPath !== null) {
      this.mappers.delete(carrierPath);
    }
  }

  /**
   * Ensure ONE source's surfaces are current before an LSP query (called by
   * `lspProviders` after a recompile; no-op when nothing changed).
   */
  async ensureSourceCurrent(file: SyncableFile): Promise<void> {
    await this.syncSource(file);
  }

  /** (Re)register the CORE strict mapper for a carrier's IDE surface. */
  private refreshCarrierMapper(sourcePath: string, file: SyncableFile): void {
    const carrierPath = toIdeCarrierFileName(sourcePath);
    if (carrierPath === null) return;
    const sourceName = normalizePath(file.filename);
    const sourceCode = file.code;
    const mapper = file.compiled.types
      ? createCarrierMapper({
          providerPath: carrierPath,
          code: file.compiled.types,
          sourceMap: file.compiled.typesSourceMap || null,
          // The live source text the surfaces were compiled from — consulted
          // before the map's own sourcesContent. Exact-name only (fail closed).
          readSourceText: (source) => {
            const normalized = normalizePath(source);
            return normalized === sourceName || normalized === sourcePath ? sourceCode : undefined;
          },
        })
      : null;
    if (mapper !== null) {
      this.mappers.set(carrierPath, mapper);
    } else {
      // Mapless/torn ⇒ NO mapper: every span for this carrier drops.
      this.mappers.delete(carrierPath);
    }
  }

  // ── Result mapping (generated → source, strict, fail closed) ──

  /** Whether a map source name spells the requested playground file. */
  private isSameSource(mapSource: string, filename: string): boolean {
    const normalized = normalizePath(mapSource);
    return normalized === normalizePath(filename) || normalized === this.sourcePathFor(filename);
  }

  /**
   * Map one worker result span into the REQUESTED source's space, or `null`
   * (drop): a plain user file's own spans pass through raw; every other span
   * maps through the mapper registered for ITS OWN `fileName` and must land
   * in the requested source.
   */
  private mapResultSpanToSource(filename: string, span: TsSpanLike): MappedSpan | null {
    const sourcePath = this.sourcePathFor(filename);
    const spanFile = normalizePath(span.fileName);
    if (this.synced.get(sourcePath)?.kind === "user" && spanFile === sourcePath) {
      return { start: span.start, end: span.start + span.length };
    }
    const mapper = this.mappers.forCarrier(spanFile);
    if (mapper === undefined) return null;
    const mapped = mapper.mapGeneratedSpanToSource(span.start, span.start + span.length);
    if (mapped === null) return null;
    if (!this.isSameSource(mapped.source, filename)) return null;
    return { start: mapped.start, end: mapped.end };
  }

  // ── Query direction (source → generated, strict, fail closed) ──

  /**
   * The worker file + offset a source-space query targets: a user file
   * queries itself raw; a carrier queries its IDE carrier at the strictly
   * forward-mapped generated offset — or not at all (`null`).
   */
  private queryTarget(
    filename: string,
    sourceOffset: number,
  ): { path: string; offset: number } | null {
    const sourcePath = this.sourcePathFor(filename);
    const state = this.synced.get(sourcePath);
    if (state === undefined) return null;
    if (state.kind === "user") {
      return { path: sourcePath, offset: sourceOffset };
    }
    const carrierPath = toIdeCarrierFileName(sourcePath);
    if (carrierPath === null) return null;
    const mapper = this.mappers.forCarrier(carrierPath);
    if (mapper === undefined) return null;
    const generated = mapper.mapSourceOffsetToGenerated(sourceOffset);
    if (generated === null) return null;
    return { path: carrierPath, offset: generated.offset };
  }

  // ── Diagnostics ──

  /**
   * The requested source's diagnostics, strictly mapped into its own space.
   * Unmapped/synthetic/foreign spans DROP.
   */
  async getDiagnostics(filename: string): Promise<MappedDiagnostic[]> {
    if (!this.initialized) return [];
    const sourcePath = this.sourcePathFor(filename);
    const state = this.synced.get(sourcePath);
    if (state === undefined) return [];
    const queryPath = state.kind === "user" ? sourcePath : toIdeCarrierFileName(sourcePath);
    if (queryPath === null) return [];
    // A carrier with no registered mapper (no IDE surface / mapless / torn
    // map) can never surface a mapped diagnostic — fail closed without
    // querying the worker.
    if (state.kind === "carrier" && this.mappers.forCarrier(queryPath) === undefined) {
      return [];
    }

    const raw = (await this.send("getDiagnostics", { path: queryPath })) as WorkerDiagnostic[];
    const mapped: MappedDiagnostic[] = [];
    for (const d of raw) {
      const span = this.mapResultSpanToSource(filename, {
        fileName: d.fileName,
        start: d.start,
        length: d.length,
      });
      if (span === null) continue;
      mapped.push({
        message: d.message,
        start: span.start,
        end: span.end,
        severity: categoryToSeverity(d.category),
        code: d.code,
      });
    }
    return mapped;
  }

  /**
   * Check a standalone scratch file and return RAW (unmapped) diagnostics —
   * the editable output panel edits generated TSX directly, by design the
   * only raw path.
   */
  async checkStandalone(tsxCode: string): Promise<RawTsDiagnostic[]> {
    if (!this.initialized) return [];
    const raw = (await this.send("checkStandalone", {
      path: STANDALONE_SCRATCH_PATH,
      content: tsxCode,
    })) as WorkerDiagnostic[];
    return raw.map((d) => ({
      message: d.message,
      start: d.start,
      end: d.start + d.length,
      severity: categoryToSeverity(d.category),
      code: d.code,
    }));
  }

  // ── LSP queries ──

  async getHover(filename: string, sourceOffset: number): Promise<string | null> {
    if (!this.initialized) return null;
    const target = this.queryTarget(filename, sourceOffset);
    if (target === null) return null;

    const info = (await this.send("getHover", target)) as TsHoverInfo | null;
    if (!info || !info.text) return null;

    const lines = [`\`\`\`typescript\n${info.text}\n\`\`\``];
    if (info.documentation) {
      lines.push(info.documentation);
    }
    return lines.join("\n\n");
  }

  async getCompletions(
    filename: string,
    sourceOffset: number,
  ): Promise<Array<{ label: string; kind: number; detail?: string; insertText?: string }>> {
    if (!this.initialized) return [];
    const target = this.queryTarget(filename, sourceOffset);
    if (target === null) return [];

    const entries = (await this.send("getCompletions", target)) as TsCompletionEntry[];
    return entries.map((e) => ({
      label: e.label,
      kind: TS_KIND_TO_MONACO[e.kind] ?? 9, // default to Property
      insertText: e.label,
    }));
  }

  async getDefinition(filename: string, sourceOffset: number): Promise<MappedSpan[]> {
    if (!this.initialized) return [];
    const target = this.queryTarget(filename, sourceOffset);
    if (target === null) return [];

    const defs = (await this.send("getDefinition", target)) as TsSpanLike[] | null;
    if (!defs || defs.length === 0) return [];

    const mapped: MappedSpan[] = [];
    for (const def of defs) {
      const location = this.mapResultSpanToSource(filename, def);
      if (location) mapped.push(location);
    }
    return mapped;
  }

  async getReferences(filename: string, sourceOffset: number): Promise<MappedReference[]> {
    if (!this.initialized) return [];
    const target = this.queryTarget(filename, sourceOffset);
    if (target === null) return [];

    const refs = (await this.send("getReferences", target)) as TsReferenceLike[];
    const mapped: MappedReference[] = [];
    for (const ref of refs) {
      const location = this.mapResultSpanToSource(filename, ref);
      if (!location) continue;
      mapped.push({
        ...location,
        isDefinition: ref.isDefinition ?? false,
      });
    }
    return mapped;
  }

  async getDocumentHighlights(filename: string, sourceOffset: number): Promise<MappedSpan[]> {
    if (!this.initialized) return [];
    const target = this.queryTarget(filename, sourceOffset);
    if (target === null) return [];

    const highlights = (await this.send("getDocumentHighlights", target)) as TsSpanLike[];
    const mapped: MappedSpan[] = [];
    for (const highlight of highlights) {
      const location = this.mapResultSpanToSource(filename, highlight);
      if (location) mapped.push(location);
    }
    return mapped;
  }

  async getRenameLocations(filename: string, sourceOffset: number): Promise<RenameLocations> {
    if (!this.initialized) {
      return {
        canRename: false,
        rejectReason: "TypeScript service is not initialized",
        triggerSpan: null,
        locations: [],
      };
    }
    const target = this.queryTarget(filename, sourceOffset);
    if (target === null) {
      return {
        canRename: false,
        rejectReason: "Position has no mapped generated correlate",
        triggerSpan: null,
        locations: [],
      };
    }

    const response = (await this.send("getRenameLocations", target)) as TsRenameResponse;
    if (!response.canRename) {
      return {
        canRename: false,
        rejectReason: response.localizedErrorMessage ?? "Symbol cannot be renamed",
        triggerSpan: null,
        locations: [],
      };
    }

    const triggerSpan =
      response.triggerSpan != null
        ? this.mapResultSpanToSource(filename, {
            fileName: target.path,
            start: response.triggerSpan.start,
            length: response.triggerSpan.length,
          })
        : null;

    const locations: MappedSpan[] = [];
    for (const loc of response.locations) {
      const mapped = this.mapResultSpanToSource(filename, loc);
      if (mapped) locations.push(mapped);
    }

    return {
      canRename: true,
      triggerSpan,
      locations,
    };
  }

  dispose(): void {
    this.worker?.terminate();
    this.worker = null;
    this.pending.clear();
    this.initialized = false;
    this.initPromise = null;
    this.mappers.clear();
    this.synced.clear();
  }
}
