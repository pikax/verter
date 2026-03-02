/**
 * Main-thread bridge to tsgo WASM running in a web worker.
 * Implements TypeScriptServiceBridge for seamless integration with lspProviders.
 */
import { SourceMapMapper } from "./sourceMapMapper";
import type { TypeScriptServiceBridge } from "./lspProviders";
import type { MappedDiagnostic, MappedSpan, MappedReference, RenameLocations } from "./tsService";

interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
}

interface LspPosition {
  line: number;
  character: number;
}

interface LspRange {
  start: LspPosition;
  end: LspPosition;
}

interface LspHoverResponse {
  result?: {
    contents: { kind: string; value: string } | string;
    range?: LspRange;
  } | null;
}

interface LspCompletionResponse {
  result?: {
    items?: Array<{
      label: string;
      kind?: number;
      detail?: string;
      insertText?: string;
    }>;
  } | null;
}

interface LspDefinitionResponse {
  result?: Array<{
    uri: string;
    range: LspRange;
  }> | null;
}

interface PublishDiagnosticsParams {
  uri: string;
  diagnostics: Array<{
    range: LspRange;
    severity?: number;
    code?: number | string;
    message: string;
  }>;
}

export class TsgoService implements TypeScriptServiceBridge {
  private worker: Worker | null = null;
  private requestId = 0;
  private pending = new Map<number, PendingRequest>();
  private initialized = false;
  private initPromise: Promise<void> | null = null;
  private available = false;

  // Current file state
  private currentMapper: SourceMapMapper | null = null;
  private currentTsxPath: string | null = null;
  private currentVueCode: string | null = null;
  private currentTsxCode: string | null = null;
  private fileVersions = new Map<string, number>();

  // Diagnostic callback
  private onDiagnostics: ((diagnostics: MappedDiagnostic[]) => void) | null = null;

  /** Whether tsgo WASM loaded successfully */
  get isAvailable(): boolean {
    return this.available;
  }

  /** Set a callback for push-based diagnostics from tsgo */
  setDiagnosticsCallback(cb: (diagnostics: MappedDiagnostic[]) => void): void {
    this.onDiagnostics = cb;
  }

  async init(): Promise<void> {
    if (this.initialized) return;
    if (this.initPromise) return this.initPromise;

    this.initPromise = this._init();
    return this.initPromise;
  }

  private async _init(): Promise<void> {
    // Check SharedArrayBuffer support (requires COOP/COEP headers)
    if (typeof SharedArrayBuffer === "undefined") {
      console.warn("[TsgoService] SharedArrayBuffer not available — tsgo requires COOP/COEP headers");
      this.available = false;
      this.initialized = true;
      return;
    }

    try {
      this.worker = new Worker(new URL("./tsgoWorker.ts", import.meta.url), {
        type: "classic", // Go wasm_exec.js needs classic worker
      });

      this.worker.onmessage = (e: MessageEvent) => {
        const data = e.data;

        if (data.type === "diagnostics") {
          this.handlePushDiagnostics(data.params as PublishDiagnosticsParams);
          return;
        }

        if (data.type === "status") {
          console.log("[tsgo]", data.message);
          return;
        }

        if (data.type === "error") {
          console.error("[tsgo]", data.message);
          return;
        }

        if (data.type === "ready") {
          return;
        }

        // Request/response
        const { id, result, error } = data;
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

      const sharedBuffer = new SharedArrayBuffer(64 * 1024);
      await this.send("init", { sharedBuffer });

      this.available = true;
      this.initialized = true;
    } catch (err) {
      console.warn("[TsgoService] Failed to initialize tsgo WASM:", err);
      this.available = false;
      this.initialized = true;
    }
  }

  private send(type: string, payload?: unknown): Promise<unknown> {
    return new Promise((resolve, reject) => {
      if (!this.worker) {
        reject(new Error("Worker not initialized"));
        return;
      }
      const id = ++this.requestId;
      this.pending.set(id, { resolve, reject });

      // For init, transfer SharedArrayBuffer
      if (type === "init" && payload && (payload as Record<string, unknown>).sharedBuffer) {
        this.worker.postMessage({ id, type, payload });
      } else {
        this.worker.postMessage({ id, type, payload });
      }
    });
  }

  /**
   * Sync TSX output to tsgo and get diagnostics mapped to Vue positions.
   */
  async syncTsx(
    vueFilename: string,
    tsxCode: string,
    vueCode: string,
    sourceMapJson: string | null,
  ): Promise<MappedDiagnostic[]> {
    if (!this.initialized || !this.available) return [];

    const tsxPath = `/${vueFilename}.tsx`;
    this.currentTsxPath = tsxPath;
    this.currentVueCode = vueCode;
    this.currentTsxCode = tsxCode;

    // Create mapper if source map is available
    this.currentMapper =
      sourceMapJson && sourceMapJson.length > 2
        ? new SourceMapMapper(sourceMapJson, tsxCode, vueCode)
        : null;

    const version = (this.fileVersions.get(tsxPath) ?? 0) + 1;
    this.fileVersions.set(tsxPath, version);

    if (version === 1) {
      await this.send("openFile", { path: tsxPath, content: tsxCode });
    } else {
      await this.send("updateFile", { path: tsxPath, content: tsxCode, version });
    }

    // tsgo sends diagnostics via publishDiagnostics notification
    // We return empty here — diagnostics arrive asynchronously via the callback
    return [];
  }

  /**
   * Ensure the worker file and source map mapper are up to date before LSP operations.
   */
  async ensureTsxCurrent(
    vueFilename: string,
    tsxCode: string,
    vueCode: string,
    sourceMapJson: string | null,
  ): Promise<void> {
    if (!this.initialized || !this.available) return;
    if (this.currentTsxCode === tsxCode && this.currentVueCode === vueCode) return;

    const tsxPath = `/${vueFilename}.tsx`;
    this.currentTsxPath = tsxPath;
    this.currentVueCode = vueCode;
    this.currentTsxCode = tsxCode;

    this.currentMapper =
      sourceMapJson && sourceMapJson.length > 2
        ? new SourceMapMapper(sourceMapJson, tsxCode, vueCode)
        : null;

    const version = (this.fileVersions.get(tsxPath) ?? 0) + 1;
    this.fileVersions.set(tsxPath, version);

    if (version === 1) {
      await this.send("openFile", { path: tsxPath, content: tsxCode });
    } else {
      await this.send("updateFile", { path: tsxPath, content: tsxCode, version });
    }
  }

  private handlePushDiagnostics(params: PublishDiagnosticsParams): void {
    if (!this.onDiagnostics || !this.currentVueCode) return;

    const mapped: MappedDiagnostic[] = params.diagnostics.map((d) => {
      // Convert LSP position to offset
      const start = this.lspPositionToOffset(d.range.start);
      const end = this.lspPositionToOffset(d.range.end);

      let vueStart = start;
      let vueEnd = end;

      if (this.currentMapper) {
        const mappedStart = this.currentMapper.tsxOffsetToVueOffset(start);
        const mappedEnd = this.currentMapper.tsxOffsetToVueOffset(end);
        if (mappedStart != null) vueStart = mappedStart;
        if (mappedEnd != null) vueEnd = mappedEnd;
      }

      return {
        message: d.message,
        start: vueStart,
        end: vueEnd,
        severity: d.severity === 1 ? "error" : d.severity === 2 ? "warning" : "info",
        code: typeof d.code === "number" ? d.code : 0,
      };
    });

    this.onDiagnostics(mapped);
  }

  async getHover(filename: string, vueOffset: number): Promise<string | null> {
    if (!this.initialized || !this.available || !this.currentTsxPath) return null;

    let tsxOffset = vueOffset;
    if (this.currentMapper) {
      const mapped = this.currentMapper.vueOffsetToTsxOffset(vueOffset);
      if (mapped != null) tsxOffset = mapped;
    }

    const position = this.offsetToLspPosition(tsxOffset);

    try {
      const response = (await this.send("getHover", {
        path: this.currentTsxPath,
        ...position,
      })) as LspHoverResponse;

      const result = response?.result;
      if (!result) return null;

      const content =
        typeof result.contents === "string"
          ? result.contents
          : result.contents?.value ?? null;

      if (!content) return null;

      // Wrap in markdown code block if not already
      if (content.startsWith("```")) return content;
      return `\`\`\`typescript\n${content}\n\`\`\``;
    } catch {
      return null;
    }
  }

  async getCompletions(
    filename: string,
    vueOffset: number,
  ): Promise<Array<{ label: string; kind: number; detail?: string; insertText?: string }>> {
    if (!this.initialized || !this.available || !this.currentTsxPath) return [];

    let tsxOffset = vueOffset;
    if (this.currentMapper) {
      const mapped = this.currentMapper.vueOffsetToTsxOffset(vueOffset);
      if (mapped != null) tsxOffset = mapped;
    }

    const position = this.offsetToLspPosition(tsxOffset);

    try {
      const response = (await this.send("getCompletions", {
        path: this.currentTsxPath,
        ...position,
      })) as LspCompletionResponse;

      const items = response?.result?.items ?? [];
      return items.slice(0, 100).map((item) => ({
        label: item.label,
        kind: item.kind ?? 9,
        detail: item.detail,
        insertText: item.insertText ?? item.label,
      }));
    } catch {
      return [];
    }
  }

  async getDefinition(filename: string, vueOffset: number): Promise<MappedSpan[]> {
    if (!this.initialized || !this.available || !this.currentTsxPath) return [];

    let tsxOffset = vueOffset;
    if (this.currentMapper) {
      const mapped = this.currentMapper.vueOffsetToTsxOffset(vueOffset);
      if (mapped != null) tsxOffset = mapped;
    }

    const position = this.offsetToLspPosition(tsxOffset);

    try {
      const response = (await this.send("getDefinition", {
        path: this.currentTsxPath,
        ...position,
      })) as LspDefinitionResponse;

      const defs = response?.result ?? [];
      return defs.map((def) => {
        let start = this.lspPositionToOffset(def.range.start);
        let end = this.lspPositionToOffset(def.range.end);

        if (this.currentMapper) {
          const mappedStart = this.currentMapper.tsxOffsetToVueOffset(start);
          const mappedEnd = this.currentMapper.tsxOffsetToVueOffset(end);
          if (mappedStart != null) start = mappedStart;
          if (mappedEnd != null) end = mappedEnd;
        }

        return { start, end };
      });
    } catch {
      return [];
    }
  }

  async getReferences(
    filename: string,
    vueOffset: number,
  ): Promise<Array<{ start: number; end: number; isDefinition: boolean }>> {
    // tsgo LSP doesn't support textDocument/references yet in WASM mode
    return [];
  }

  async getDocumentHighlights(
    filename: string,
    vueOffset: number,
  ): Promise<Array<{ start: number; end: number }>> {
    // tsgo LSP doesn't support textDocument/documentHighlight yet in WASM mode
    return [];
  }

  async getRenameLocations(
    filename: string,
    vueOffset: number,
  ): Promise<RenameLocations> {
    // tsgo LSP doesn't support textDocument/rename yet in WASM mode
    return {
      canRename: false,
      rejectReason: "Rename not available with tsgo",
      triggerSpan: null,
      locations: [],
    };
  }

  /** Convert a byte offset in TSX to LSP line/character (0-based). */
  private offsetToLspPosition(offset: number): { line: number; character: number } {
    const src = this.currentTsxCode;
    if (!src) return { line: 0, character: offset };

    let line = 0;
    let lineStart = 0;
    for (let i = 0; i < offset && i < src.length; i++) {
      if (src.charCodeAt(i) === 10) {
        line++;
        lineStart = i + 1;
      }
    }
    return { line, character: offset - lineStart };
  }

  /** Convert an LSP position (0-based line/character) to a byte offset. */
  private lspPositionToOffset(position: LspPosition): number {
    const src = this.currentTsxCode;
    if (!src) return position.character;

    let line = 0;
    for (let i = 0; i < src.length; i++) {
      if (line === position.line) {
        return i + position.character;
      }
      if (src.charCodeAt(i) === 10) {
        line++;
      }
    }
    return src.length;
  }

  dispose(): void {
    this.worker?.terminate();
    this.worker = null;
    this.pending.clear();
    this.initialized = false;
    this.available = false;
    this.initPromise = null;
    this.currentMapper = null;
    this.currentTsxPath = null;
    this.currentVueCode = null;
    this.currentTsxCode = null;
    this.fileVersions.clear();
    this.onDiagnostics = null;
  }
}
