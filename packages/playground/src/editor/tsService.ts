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

  async init(verterTypesContent?: string): Promise<void> {
    if (this.initialized) return;
    if (this.initPromise) return this.initPromise;

    this.initPromise = this._init(verterTypesContent);
    return this.initPromise;
  }

  private async _init(verterTypesContent?: string): Promise<void> {
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

    await this.send("init", { verterTypesContent });
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

    // Create mapper if source map is available
    this.currentMapper =
      sourceMapJson && sourceMapJson.length > 2
        ? new SourceMapMapper(sourceMapJson, tsxCode, vueCode)
        : null;

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
   * Get hover info for a Vue position, mapping through source map.
   */
  async getHover(filename: string, vueOffset: number): Promise<string | null> {
    if (!this.initialized || !this.currentTsxPath) return null;

    let tsxOffset = vueOffset;
    if (this.currentMapper) {
      const mapped = this.currentMapper.vueOffsetToTsxOffset(vueOffset);
      if (mapped != null) tsxOffset = mapped;
    }

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

    let tsxOffset = vueOffset;
    if (this.currentMapper) {
      const mapped = this.currentMapper.vueOffsetToTsxOffset(vueOffset);
      if (mapped != null) tsxOffset = mapped;
    }

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

  private mapVueOffsetToTsxOffset(vueOffset: number): number {
    if (!this.currentMapper) return vueOffset;
    const mapped = this.currentMapper.vueOffsetToTsxOffset(vueOffset);
    return mapped ?? vueOffset;
  }

  private mapTsxSpanToVueSpan(span: TsSpanLike): MappedSpan | null {
    if (!this.currentTsxPath || span.fileName !== this.currentTsxPath) return null;

    let start = span.start;
    let end = span.start + span.length;

    if (this.currentMapper) {
      const mappedStart = this.currentMapper.tsxOffsetToVueOffset(start);
      const mappedEnd = this.currentMapper.tsxOffsetToVueOffset(end);
      if (mappedStart != null) start = mappedStart;
      if (mappedEnd != null) end = mappedEnd;
    }

    return { start, end };
  }

  dispose(): void {
    this.worker?.terminate();
    this.worker = null;
    this.pending.clear();
    this.initialized = false;
    this.initPromise = null;
    this.currentMapper = null;
    this.currentTsxPath = null;
  }
}
