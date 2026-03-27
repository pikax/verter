/**
 * Main-thread bridge to the TypeScript LanguageService web worker.
 * Provides async methods for type checking, hover, and completions.
 */
import { SourceMapMapper } from "./sourceMapMapper";
import type { TypeScriptServiceBridge } from "./lspProviders";

interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
}

interface TsDiagnostic {
  message: string;
  start: number;
  length: number;
  category: number; // 0=Warning, 1=Error, 2=Suggestion, 3=Message
  code: number;
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

interface TsHighlightLike extends TsSpanLike {
  kind?: string;
}

interface TsRenameResponse {
  canRename: boolean;
  localizedErrorMessage: string | null;
  triggerSpan: { start: number; length: number } | null;
  locations: TsSpanLike[];
}

export interface RawTsDiagnostic {
  message: string;
  /** TSX byte offset (start) — NOT mapped through source map */
  start: number;
  /** TSX byte offset (end) */
  end: number;
  severity: "error" | "warning" | "info";
  code: number;
}

export interface MappedDiagnostic {
  message: string;
  /** Vue source byte offset (start) */
  start: number;
  /** Vue source byte offset (end) */
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

/** Structured metadata for the destructured block, provided by the Rust compiler. */
export interface DestructuredBinding {
  name: string;
  /** SFC-absolute start offset (UTF-16 code units). */
  sourceStart: number;
  /** SFC-absolute end offset (UTF-16 code units). */
  sourceEnd: number;
}

/** Metadata for the destructured block region in the generated TSX. */
export interface DestructuredBlockMeta {
  bindings: DestructuredBinding[];
  /** Start offset of the destructured block in the generated TSX output (UTF-16). */
  blockStart: number;
  /** End offset of the destructured block in the generated TSX output (UTF-16). */
  blockEnd: number;
}

const DESTRUCTURED_START = "/* verter-destructured-start */";
const DESTRUCTURED_END = "/* verter-destructured-end */";

/** Check if a TSX offset falls inside the destructured block. */
function isInsideDestructuredBlock(
  meta: DestructuredBlockMeta | null,
  tsxCode: string,
  tsxOffset: number,
): boolean {
  if (meta) {
    return tsxOffset >= meta.blockStart && tsxOffset < meta.blockEnd;
  }
  // Fallback: string search for boundary markers
  const start = tsxCode.indexOf(DESTRUCTURED_START);
  if (start === -1) return false;
  const end = tsxCode.indexOf(DESTRUCTURED_END, start);
  if (end === -1) return false;
  return tsxOffset >= start && tsxOffset < end + DESTRUCTURED_END.length;
}

/**
 * Resolve a TSX offset inside the destructured block to its SFC source span.
 * Uses structured metadata (pre-converted to UTF-16) — no comment parsing needed.
 */
function resolveDestructuredBinding(
  meta: DestructuredBlockMeta | null,
  tsxCode: string,
  tsxOffset: number,
): MappedSpan | null {
  if (!meta) return null;
  // Find the closest binding whose name appears before tsxOffset
  // by searching for each binding name near the offset
  for (let i = meta.bindings.length - 1; i >= 0; i--) {
    const b = meta.bindings[i];
    const namePos = tsxCode.lastIndexOf(b.name, tsxOffset);
    if (namePos !== -1 && namePos + b.name.length >= tsxOffset && namePos >= meta.blockStart) {
      return { start: b.sourceStart, end: b.sourceEnd };
    }
  }
  return null;
}

/**
 * Expand a TS6198 ("All destructured elements are unused") diagnostic into
 * individual TS6133-like diagnostics for each binding, using structured metadata.
 */
function expandTs6198ToIndividualDiagnostics(
  meta: DestructuredBlockMeta | null,
  tsxCode: string,
  tsxStart: number,
  severity: MappedDiagnostic["severity"],
  vueCode: string,
): MappedDiagnostic[] {
  if (!meta || meta.bindings.length === 0) return [];

  // Find which destructuring statement this diagnostic belongs to.
  // Look for the "const {" or "let {" before tsxStart and the "___VERTER___unwrapped" after.
  const unwrappedMarker = "___VERTER___unwrapped";
  const stmtEnd = tsxCode.indexOf(unwrappedMarker, tsxStart);
  if (stmtEnd === -1) return [];

  const constIdx = tsxCode.lastIndexOf("const {", tsxStart);
  const letIdx = tsxCode.lastIndexOf("let {", tsxStart);
  const stmtStart = Math.max(constIdx, letIdx);
  if (stmtStart === -1) return [];

  // Collect all bindings whose names appear in this statement range
  const stmtText = tsxCode.slice(stmtStart, stmtEnd);
  const diagnostics: MappedDiagnostic[] = [];
  for (const b of meta.bindings) {
    if (stmtText.includes(b.name)) {
      const name = vueCode.slice(b.sourceStart, b.sourceEnd);
      diagnostics.push({
        message: `'${name}' is declared but its value is never read.`,
        start: b.sourceStart,
        end: b.sourceEnd,
        severity,
        code: 6133,
      });
    }
  }
  return diagnostics;
}

export class TypeScriptService implements TypeScriptServiceBridge {
  private worker: Worker | null = null;
  private requestId = 0;
  private pending = new Map<number, PendingRequest>();
  private initialized = false;
  private initPromise: Promise<void> | null = null;

  // Current file state
  private currentMapper: SourceMapMapper | null = null;
  private currentTsxPath: string | null = null;
  private currentTsxCode: string | null = null;
  private currentVueCode: string | null = null;
  private currentDestructuredBlock: DestructuredBlockMeta | null = null;

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
      if (typeof console !== "undefined") {
        console.debug("[tsc] <-", { id, error: error ?? undefined, hasResult: result != null });
      }
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

    await this.send("init", options);
    this.initialized = true;
  }

  async updateVueTypes(vueVersion: string): Promise<void> {
    if (!this.initialized) return;
    await this.send("updateVueTypes", { vueVersion });
  }

  private send(type: string, payload?: unknown): Promise<unknown> {
    return new Promise((resolve, reject) => {
      if (!this.worker) {
        reject(new Error("Worker not initialized"));
        return;
      }
      const id = ++this.requestId;
      this.pending.set(id, { resolve, reject });
      if (typeof console !== "undefined") {
        console.debug("[tsc] ->", type, { id });
      }
      this.worker.postMessage({ id, type, payload });
    });
  }

  /**
   * Sync TSX output to the worker and get diagnostics mapped to Vue positions.
   */
  async syncTsx(
    vueFilename: string,
    tsxCode: string,
    vueCode: string,
    sourceMapJson: string | null,
    destructuredBlock?: DestructuredBlockMeta | null,
  ): Promise<MappedDiagnostic[]> {
    if (!this.initialized) return [];

    const tsxPath = `/${vueFilename}.tsx`;
    this.currentTsxPath = tsxPath;
    this.currentTsxCode = tsxCode;
    this.currentVueCode = vueCode;
    this.currentDestructuredBlock = destructuredBlock ?? null;

    // Create mapper if source map is available
    if (sourceMapJson && sourceMapJson.length > 2) {
      this.currentMapper = new SourceMapMapper(sourceMapJson, tsxCode, vueCode);
    } else {
      this.currentMapper = null;
      if (typeof console !== "undefined") {
        console.debug(
          "[verter] TSX source map unavailable — hover/completions disabled. " +
            `sourceMap length: ${sourceMapJson?.length ?? 0}, tsx length: ${tsxCode.length}`,
        );
      }
    }

    await this.send("updateFile", { path: tsxPath, content: tsxCode });

    const diagnostics = (await this.send("getDiagnostics", { path: tsxPath })) as TsDiagnostic[];

    const mapped: MappedDiagnostic[] = [];
    for (const d of diagnostics) {
      const tsxStart = d.start;
      const tsxEnd = d.start + d.length;

      // Expand TS6198 ("All destructured elements are unused") inside the
      // verter-destructured block into individual TS6133-like diagnostics for
      // each binding, using structured metadata.
      if (
        d.code === 6198 &&
        tsxCode &&
        isInsideDestructuredBlock(this.currentDestructuredBlock, tsxCode, tsxStart)
      ) {
        const expanded = expandTs6198ToIndividualDiagnostics(
          this.currentDestructuredBlock,
          tsxCode,
          tsxStart,
          d.category === 1 ? "error" : d.category === 0 ? "warning" : "info",
          vueCode,
        );
        mapped.push(...expanded);
        continue;
      }

      let vueStart: number | null = null;
      let vueEnd: number | null = null;

      // For positions inside the destructured block, skip the source map —
      // it can't properly map synthetic code. Use structured metadata instead.
      const inDestructuredBlock =
        tsxCode && isInsideDestructuredBlock(this.currentDestructuredBlock, tsxCode, tsxStart);

      if (!inDestructuredBlock && this.currentMapper) {
        vueStart = this.currentMapper.tsxOffsetToVueOffset(tsxStart);
        vueEnd = this.currentMapper.tsxOffsetToVueOffset(tsxEnd);
      }

      // Fall back to structured metadata when source map fails or was skipped
      if (vueStart == null && this.currentTsxCode) {
        const resolved = resolveDestructuredBinding(
          this.currentDestructuredBlock,
          this.currentTsxCode,
          tsxStart,
        );
        if (resolved) {
          vueStart = resolved.start;
          vueEnd = resolved.end;
        }
      }

      // Drop diagnostics that can't be mapped — they're in synthetic code
      if (vueStart == null) continue;

      mapped.push({
        message: d.message,
        start: vueStart,
        end: vueEnd ?? tsxEnd,
        severity: d.category === 1 ? "error" : d.category === 0 ? "warning" : "info",
        code: d.code,
      });
    }
    return mapped;
  }

  /**
   * Ensure the worker file and source map mapper are up to date before LSP operations.
   * This is called by LSP providers (completions, hover, etc.) to avoid using
   * a stale mapper when the TSX has changed but syncTsx hasn't fired yet (debounced).
   */
  async ensureTsxCurrent(
    vueFilename: string,
    tsxCode: string,
    vueCode: string,
    sourceMapJson: string | null,
    destructuredBlock?: DestructuredBlockMeta | null,
  ): Promise<void> {
    if (!this.initialized) return;
    // Skip if nothing changed
    if (this.currentTsxCode === tsxCode && this.currentVueCode === vueCode) return;

    const tsxPath = `/${vueFilename}.tsx`;
    this.currentTsxPath = tsxPath;
    this.currentTsxCode = tsxCode;
    this.currentVueCode = vueCode;
    this.currentDestructuredBlock = destructuredBlock ?? null;

    if (sourceMapJson && sourceMapJson.length > 2) {
      this.currentMapper = new SourceMapMapper(sourceMapJson, tsxCode, vueCode);
    } else {
      this.currentMapper = null;
    }

    await this.send("updateFile", { path: tsxPath, content: tsxCode });
  }

  /**
   * Sync all files' .d.ts declarations to the worker for cross-file import resolution.
   * Uses tscCode (which has `export default`) as virtual .d.ts files.
   */
  async syncDtsFiles(files: Array<{ filename: string; dtsCode: string }>): Promise<void> {
    if (!this.initialized) return;

    for (const { filename, dtsCode } of files) {
      const dtsPath = `/${filename}.d.ts`;
      await this.send("updateFile", { path: dtsPath, content: dtsCode });
    }
  }

  /**
   * Remove a file from the worker's virtual FS.
   */
  async closeFile(filename: string): Promise<void> {
    if (!this.initialized) return;
    const tsxPath = `/${filename}.tsx`;
    await this.send("closeFile", { path: tsxPath });
  }

  /**
   * Send TSX code directly to the worker and return raw (unmapped) diagnostics.
   * Used by the editable output panel — does NOT touch currentMapper or currentTsxPath.
   */
  async syncTsxDirect(tsxCode: string): Promise<RawTsDiagnostic[]> {
    if (!this.initialized) return [];

    const directPath = "/direct-edit.tsx";
    await this.send("updateFile", { path: directPath, content: tsxCode });
    const diagnostics = (await this.send("getDiagnostics", { path: directPath })) as TsDiagnostic[];

    return diagnostics.map((d) => ({
      message: d.message,
      start: d.start,
      end: d.start + d.length,
      severity: d.category === 1 ? "error" : d.category === 0 ? "warning" : "info",
      code: d.code,
    }));
  }

  /**
   * Get hover info for a Vue position, mapping through source map.
   */
  async getHover(filename: string, vueOffset: number): Promise<string | null> {
    if (!this.initialized || !this.currentTsxPath) return null;

    const tsxOffset = this.mapVueOffsetToTsxOffset(vueOffset);
    if (tsxOffset === null) return null;

    const info = (await this.send("getHover", {
      path: this.currentTsxPath,
      offset: tsxOffset,
    })) as TsHoverInfo | null;

    if (!info || !info.text) return null;

    const lines = [`\`\`\`typescript\n${info.text}\n\`\`\``];
    if (info.documentation) {
      lines.push(info.documentation);
    }
    return lines.join("\n\n");
  }

  /**
   * Get completions for a Vue position, mapping through source map.
   */
  async getCompletions(
    filename: string,
    vueOffset: number,
  ): Promise<Array<{ label: string; kind: number; detail?: string; insertText?: string }>> {
    if (!this.initialized || !this.currentTsxPath) return [];

    const tsxOffset = this.mapVueOffsetToTsxOffset(vueOffset);
    if (tsxOffset === null) return [];

    const entries = (await this.send("getCompletions", {
      path: this.currentTsxPath,
      offset: tsxOffset,
    })) as TsCompletionEntry[];

    return entries.map((e) => ({
      label: e.label,
      kind: TS_KIND_TO_MONACO[e.kind] ?? 9, // default to Property
      insertText: e.label,
    }));
  }

  /**
   * Get definition locations for a Vue position.
   */
  async getDefinition(filename: string, vueOffset: number): Promise<MappedSpan[]> {
    if (!this.initialized || !this.currentTsxPath) return [];
    const tsxOffset = this.mapVueOffsetToTsxOffset(vueOffset);
    if (tsxOffset === null) return [];
    const defs = (await this.send("getDefinition", {
      path: this.currentTsxPath,
      offset: tsxOffset,
    })) as TsSpanLike[] | null;
    if (!defs || defs.length === 0) return [];

    const mapped: MappedSpan[] = [];
    for (const def of defs) {
      const location = this.mapTsxSpanToVueSpan(def);
      if (location) mapped.push(location);
    }
    return mapped;
  }

  /**
   * Get references for a Vue position.
   */
  async getReferences(filename: string, vueOffset: number): Promise<MappedReference[]> {
    if (!this.initialized || !this.currentTsxPath) return [];
    const tsxOffset = this.mapVueOffsetToTsxOffset(vueOffset);
    if (tsxOffset === null) return [];
    const refs = (await this.send("getReferences", {
      path: this.currentTsxPath,
      offset: tsxOffset,
    })) as TsReferenceLike[];

    const mapped: MappedReference[] = [];
    for (const ref of refs) {
      const location = this.mapTsxSpanToVueSpan(ref);
      if (!location) continue;
      mapped.push({
        ...location,
        isDefinition: ref.isDefinition ?? false,
      });
    }
    return mapped;
  }

  /**
   * Get highlight spans for a Vue position.
   */
  async getDocumentHighlights(filename: string, vueOffset: number): Promise<MappedSpan[]> {
    if (!this.initialized || !this.currentTsxPath) return [];
    const tsxOffset = this.mapVueOffsetToTsxOffset(vueOffset);
    if (tsxOffset === null) return [];
    const highlights = (await this.send("getDocumentHighlights", {
      path: this.currentTsxPath,
      offset: tsxOffset,
    })) as TsHighlightLike[];

    const mapped: MappedSpan[] = [];
    for (const highlight of highlights) {
      const location = this.mapTsxSpanToVueSpan(highlight);
      if (location) mapped.push(location);
    }
    return mapped;
  }

  /**
   * Get all rename locations for a Vue position.
   */
  async getRenameLocations(filename: string, vueOffset: number): Promise<RenameLocations> {
    if (!this.initialized || !this.currentTsxPath) {
      return {
        canRename: false,
        rejectReason: "TypeScript service is not initialized",
        triggerSpan: null,
        locations: [],
      };
    }

    const tsxOffset = this.mapVueOffsetToTsxOffset(vueOffset);
    if (tsxOffset === null) {
      return {
        canRename: false,
        rejectReason: "Source map mapping unavailable",
        triggerSpan: null,
        locations: [],
      };
    }
    const response = (await this.send("getRenameLocations", {
      path: this.currentTsxPath,
      offset: tsxOffset,
    })) as TsRenameResponse;

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
        ? this.mapTsxSpanToVueSpan({
            fileName: this.currentTsxPath,
            start: response.triggerSpan.start,
            length: response.triggerSpan.length,
          })
        : null;

    const locations: MappedSpan[] = [];
    for (const loc of response.locations) {
      const mapped = this.mapTsxSpanToVueSpan(loc);
      if (mapped) locations.push(mapped);
    }

    return {
      canRename: true,
      triggerSpan,
      locations,
    };
  }

  private mapVueOffsetToTsxOffset(vueOffset: number): number | null {
    if (!this.currentMapper) return null;
    return this.currentMapper.vueOffsetToTsxOffset(vueOffset);
  }

  private mapTsxSpanToVueSpan(span: TsSpanLike): MappedSpan | null {
    if (!this.currentTsxPath || span.fileName !== this.currentTsxPath) return null;

    const start = span.start;
    const end = span.start + span.length;

    if (this.currentMapper) {
      const mappedStart = this.currentMapper.tsxOffsetToVueOffset(start);
      const mappedEnd = this.currentMapper.tsxOffsetToVueOffset(end);
      if (mappedStart != null && mappedEnd != null) {
        return { start: mappedStart, end: mappedEnd };
      }
      if (mappedStart != null) return { start: mappedStart, end: mappedEnd ?? end };
      if (mappedEnd != null) return { start: mappedStart ?? start, end: mappedEnd };
    }

    // Source map failed — try structured metadata fallback
    if (this.currentTsxCode && this.currentDestructuredBlock) {
      const resolved = resolveDestructuredBinding(
        this.currentDestructuredBlock,
        this.currentTsxCode,
        start,
      );
      if (resolved) return resolved;
    }

    return null;
  }

  dispose(): void {
    this.worker?.terminate();
    this.worker = null;
    this.pending.clear();
    this.initialized = false;
    this.initPromise = null;
    this.currentMapper = null;
    this.currentTsxPath = null;
    this.currentTsxCode = null;
    this.currentVueCode = null;
    this.currentDestructuredBlock = null;
  }
}
