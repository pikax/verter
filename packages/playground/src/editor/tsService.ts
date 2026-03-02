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
  "getter": 9, // Property
  "setter": 9,
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

  async init(options?: { verterTypesContent?: string; vueVersion?: string }): Promise<void> {
    if (this.initialized) return;
    if (this.initPromise) return this.initPromise;

    this.initPromise = this._init(options);
    return this.initPromise;
  }

  private async _init(options?: { verterTypesContent?: string; vueVersion?: string }): Promise<void> {
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
  ): Promise<MappedDiagnostic[]> {
    if (!this.initialized) return [];

    const tsxPath = `/${vueFilename}.tsx`;
    this.currentTsxPath = tsxPath;
    this.currentTsxCode = tsxCode;
    this.currentVueCode = vueCode;

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

    return diagnostics.map((d) => {
      const tsxStart = d.start;
      const tsxEnd = d.start + d.length;

      let vueStart = tsxStart;
      let vueEnd = tsxEnd;

      if (this.currentMapper) {
        const mappedStart = this.currentMapper.tsxOffsetToVueOffset(tsxStart);
        const mappedEnd = this.currentMapper.tsxOffsetToVueOffset(tsxEnd);
        if (mappedStart != null) vueStart = mappedStart;
        if (mappedEnd != null) vueEnd = mappedEnd;
      }

      return {
        message: d.message,
        start: vueStart,
        end: vueEnd,
        severity: d.category === 1 ? "error" : d.category === 0 ? "warning" : "info",
        code: d.code,
      };
    });
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
  ): Promise<void> {
    if (!this.initialized) return;
    // Skip if nothing changed
    if (this.currentTsxCode === tsxCode && this.currentVueCode === vueCode) return;

    const tsxPath = `/${vueFilename}.tsx`;
    this.currentTsxPath = tsxPath;
    this.currentTsxCode = tsxCode;
    this.currentVueCode = vueCode;

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

    // Source map failed — try offset comment fallback
    const resolved = this.resolveFromOffsetComment(start);
    if (resolved) return resolved;

    return null;
  }

  /**
   * Convert a UTF-8 byte offset to a JavaScript string index (UTF-16 code units).
   */
  private utf8ByteOffsetToJsOffset(source: string, byteOffset: number): number {
    let jsIdx = 0;
    let byteIdx = 0;
    while (byteIdx < byteOffset && jsIdx < source.length) {
      const codePoint = source.codePointAt(jsIdx)!;
      const charByteLen =
        codePoint <= 0x7f ? 1 : codePoint <= 0x7ff ? 2 : codePoint <= 0xffff ? 3 : 4;
      byteIdx += charByteLen;
      jsIdx += codePoint > 0xffff ? 2 : 1; // surrogate pair = 2 UTF-16 code units
    }
    return jsIdx;
  }

  /**
   * Search backward from a TSX offset for a /*start,end*​/ offset comment pattern.
   * Returns the mapped Vue span if found, or null.
   */
  private resolveFromOffsetComment(tsxOffset: number): MappedSpan | null {
    if (!this.currentTsxCode || !this.currentVueCode) return null;
    // Search backward from tsxOffset for the closest /* comment
    const before = this.currentTsxCode.lastIndexOf("/*", tsxOffset);
    if (before === -1) return null;
    const commentEnd = this.currentTsxCode.indexOf("*/", before);
    if (commentEnd === -1 || commentEnd >= tsxOffset) return null;
    const content = this.currentTsxCode.slice(before + 2, commentEnd);
    const match = /^(\d+),(\d+)$/.exec(content);
    if (!match) return null;
    return {
      start: this.utf8ByteOffsetToJsOffset(this.currentVueCode, parseInt(match[1], 10)),
      end: this.utf8ByteOffsetToJsOffset(this.currentVueCode, parseInt(match[2], 10)),
    };
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
  }
}
