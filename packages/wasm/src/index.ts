export interface CodegenOptions {
  /** The filename for source map generation */
  filename?: string;
  /** Production mode - affects component ID generation and optimizations */
  isProduction?: boolean;
  /** Custom component ID (overrides auto-generation from filename) */
  componentId?: string;
  /** When true, generate TSX output. Default: false. */
  includeTsx?: boolean;
}

export interface CompiledStyleBlock {
  /** Compiled CSS code (scoped selectors, v-bind replacements, module hashing applied) */
  code: string;
  /** Whether this style block is scoped */
  scoped: boolean;
  /** Style language (css, scss, less, stylus) */
  lang: string | null;
  /** Whether this is a CSS module block */
  isModule: boolean;
  /** CSS module class mappings (each entry is [original, hashed]) */
  moduleClasses: [string, string][];
  /** CSS processing errors */
  errors: string[];
}

/** A structured diagnostic from the compiler */
export interface WasmDiagnostic {
  /** Severity level: "error", "warning", or "info" */
  severity: string;
  /** Vue-compatible error code (e.g., "XMissingEndTag") */
  code: string;
  /** Human-readable error message */
  message: string;
  /** Optional source span start (byte offset) */
  spanStart?: number;
  /** Optional source span end (byte offset) */
  spanEnd?: number;
}

export interface CodegenResult {
  /** The transformed code */
  code: string;
  /** The source map as JSON string */
  sourceMap: string;
  /** The transformed code with inline source map appended */
  codeWithSourceMap: string;
  /** Compiled CSS blocks from `<style>` tags */
  styles: CompiledStyleBlock[];
  /** Scope ID for scoped styles (e.g., "data-v-a4f2eed6"). Empty if no scoped styles. */
  scopeId: string;
  /** Compilation diagnostics (errors, warnings) */
  errors: WasmDiagnostic[];
  /** Time taken for the Rust pipeline in milliseconds */
  durationMs: number;
  /** The generated TSX code (all blocks: script + template JSX + commented styles) */
  tsx: string;
  /** Compiled CSS (scoped selectors applied, v-bind replaced) */
  css: string;
  /** Time taken for TSX generation in milliseconds */
  tsxDurationMs: number;
}

// =============================================================================
// VerterHost types — shared with @verter/native
// =============================================================================

export type {
  HostConfig,
  HostCompileProfile,
  HostIdeResponse,
  HostVirtualNodeKind,
  HostSliceChanges,
  HostDiagnostic,
  HostDiagnosticsSnapshot,
  HostExternalSourceRequest,
  HostScriptImportInfo,
  HostModuleReference,
  HostPreprocessorRequest,
  HostBlockOverrideEntry,
  HostBlockOverrideRequest,
  HostUpdateResult,
  HostResolvedId,
  HostVirtualMeta,
  HostVirtualFileResponse,
  HostUpsertRequest,
  HostStyleOverrideEntry,
  HostStyleOverrideRequest,
  HostVirtualQuery,
  HostRemoveResult,
  HostTextEdit,
  HostCodeAction,
  HostLintRuleMetadata,
  HostLintDiagnostic,
  HostDocumentSymbol,
  HostElementMatch,
  HostSelectorMatchResult,
} from "@verter/native/host-types";

import type {
  HostConfig,
  HostCompileProfile,
  HostIdeResponse,
  HostModuleReference,
  HostResolvedId,
  HostUpsertRequest,
  HostStyleOverrideRequest,
  HostBlockOverrideRequest,
  HostUpdateResult,
  HostVirtualQuery,
  HostVirtualFileResponse,
  HostVirtualNodeKind,
  HostRemoveResult,
  HostCodeAction,
  HostLintRuleMetadata,
  HostLintDiagnostic,
  HostDocumentSymbol,
  HostSelectorMatchResult,
} from "@verter/native/host-types";

// =============================================================================
// WASM compile types
// =============================================================================

export type WasmInput = string | Uint8Array;

type WasmCompileFn = (input: string, options?: CodegenOptions) => CodegenResult;
type WasmCompileBytesFn = (input: Uint8Array, options?: CodegenOptions) => CodegenResult;
type WasmInitFn = () => Promise<unknown>;
type WasmHostResolveFn = (rawId: string) => HostResolvedId | null;
type WasmHostUpsertFn = (request: HostUpsertRequest) => HostUpdateResult;
type WasmHostApplyOverridesFn = (request: HostStyleOverrideRequest) => HostUpdateResult;
type WasmHostApplyBlockOverridesFn = (request: HostBlockOverrideRequest) => HostUpdateResult;
type WasmHostGetVirtualFileFn = (query: HostVirtualQuery) => HostVirtualFileResponse;
type WasmHostListVirtualFilesFn = (canonicalId: string) => HostVirtualNodeKind[];
type WasmHostRemoveFn = (canonicalOrAlias: string) => HostRemoveResult | null;
type WasmHostGetIdeFn = (canonicalId: string, profile?: HostCompileProfile) => HostIdeResponse | null;
type WasmHostGetAnalysisFn = (canonicalOrAlias: string) => unknown | null;
type WasmHostSetImportDependenciesFn = (canonicalOrAlias: string, resolvedDeps: string[]) => void;
type WasmHostCollectResolvableModuleReferenceSpecifiersFn = (
  moduleReferences: HostModuleReference[],
) => string[];
type WasmHostResolveKnownModuleReferenceDependenciesFn = (
  ownerCanonicalId: string,
  moduleReferences: HostModuleReference[],
  knownIds: string[],
  extensions?: string[],
) => string[];
type WasmHostLintFn = (canonicalOrAlias: string, config?: unknown) => HostLintDiagnostic[];
type WasmHostGetCodeActionsFn = (canonicalOrAlias: string, offset: number) => HostCodeAction[];
type WasmHostGetLintRuleMetadataFn = () => HostLintRuleMetadata[];
type WasmHostGetDocumentSymbolsFn = (canonicalOrAlias: string) => HostDocumentSymbol[];
type WasmHostMatchCssSelectorsFn = (canonicalOrAlias: string) => HostSelectorMatchResult[];
interface WasmHostBinding {
  resolve: WasmHostResolveFn;
  upsert: WasmHostUpsertFn;
  applyStyleOverrides: WasmHostApplyOverridesFn;
  applyBlockOverrides: WasmHostApplyBlockOverridesFn;
  getIde: WasmHostGetIdeFn;
  getVirtualFile: WasmHostGetVirtualFileFn;
  listVirtualFiles: WasmHostListVirtualFilesFn;
  remove: WasmHostRemoveFn;
  getAnalysis: WasmHostGetAnalysisFn;
  setImportDependencies: WasmHostSetImportDependenciesFn;
  collectResolvableModuleReferenceSpecifiers: WasmHostCollectResolvableModuleReferenceSpecifiersFn;
  resolveKnownModuleReferenceDependencies: WasmHostResolveKnownModuleReferenceDependenciesFn;
  lint: WasmHostLintFn;
  getCodeActions: WasmHostGetCodeActionsFn;
  getLintRuleMetadata: WasmHostGetLintRuleMetadataFn;
  getDocumentSymbols: WasmHostGetDocumentSymbolsFn;
  matchCssSelectors: WasmHostMatchCssSelectorsFn;
}
type WasmHostCtor = new (config?: HostConfig) => WasmHostBinding;

let wasmCompile: WasmCompileFn | null = null;
let wasmCompileBytes: WasmCompileBytesFn | null = null;
let wasmHostCtor: WasmHostCtor | null = null;
let initialized = false;
let initPromise: Promise<void> | null = null;

function getOptionalExport<T>(mod: object, key: string): T | null {
  if (Object.prototype.hasOwnProperty.call(mod, key)) {
    return (mod as Record<string, unknown>)[key] as T;
  }
  return null;
}

function decodeUtf8(input: Uint8Array): string {
  if (typeof TextDecoder === "undefined") {
    throw new Error("TextDecoder is required to decode Uint8Array input");
  }

  return new TextDecoder("utf-8", { fatal: true }).decode(input);
}

/**
 * Initialize the WASM module. Must be called before compile().
 * Safe to call multiple times - will only initialize once.
 */
export async function initialize(): Promise<void> {
  if (initialized) return;
  if (initPromise) return initPromise;

  initPromise = (async () => {
    // Dynamic import to avoid bundler issues
    const wasm = await import("../wasm/verter_wasm.js");
    await (wasm.default as WasmInitFn)();
    wasmCompile = wasm.compile as WasmCompileFn;
    wasmCompileBytes = getOptionalExport<WasmCompileBytesFn>(wasm, "compileBytes");
    wasmHostCtor = getOptionalExport<WasmHostCtor>(wasm, "VerterHost");
    initialized = true;
  })();

  return initPromise;
}

/**
 * Check if the WASM module has been initialized.
 */
export function isInitialized(): boolean {
  return initialized;
}

function dispatchCompile(input: WasmInput, options?: CodegenOptions): CodegenResult {
  if (!wasmCompile) {
    throw new Error("WASM module not initialized");
  }

  if (typeof input === "string") {
    return wasmCompile(input, options);
  }

  if (wasmCompileBytes) {
    return wasmCompileBytes(input, options);
  }

  return wasmCompile(decodeUtf8(input), options);
}

/**
 * Compile a Vue SFC to JavaScript.
 *
 * @param input - The Vue SFC source code (string or Uint8Array)
 * @param options - Optional compilation options
 * @returns The compiled result with code, source map, and code with inline source map
 * @throws If the WASM module has not been initialized
 */
export async function compile(input: WasmInput, options?: CodegenOptions): Promise<CodegenResult> {
  await initialize();
  return dispatchCompile(input, options);
}

/**
 * Synchronous compile - requires initialize() to have been called first.
 *
 * @param input - The Vue SFC source code (string or Uint8Array)
 * @param options - Optional compilation options
 * @returns The compiled result with code, source map, and code with inline source map
 * @throws If the WASM module has not been initialized
 */
export function compileSync(input: WasmInput, options?: CodegenOptions): CodegenResult {
  if (!initialized) {
    throw new Error("WASM module not initialized. Call initialize() first.");
  }
  return dispatchCompile(input, options);
}

/**
 * In-memory host facade exposed by the WASM runtime.
 * Requires initialize() (or createHost()) before construction.
 */
export class Host {
  private readonly inner: WasmHostBinding;

  constructor(config?: HostConfig) {
    if (!initialized || !wasmHostCtor) {
      throw new Error("WASM host not initialized. Call initialize() first.");
    }
    this.inner = new wasmHostCtor(config);
  }

  resolve(rawId: string): HostResolvedId | null {
    return this.inner.resolve(rawId);
  }

  upsert(request: HostUpsertRequest): HostUpdateResult {
    return this.inner.upsert(request);
  }

  /** @deprecated Use `applyBlockOverrides` instead — unified API for all block types. */
  applyStyleOverrides(request: HostStyleOverrideRequest): HostUpdateResult {
    return this.inner.applyStyleOverrides(request);
  }

  applyBlockOverrides(request: HostBlockOverrideRequest): HostUpdateResult {
    return this.inner.applyBlockOverrides(request);
  }

  getIde(canonicalId: string, profile?: HostCompileProfile): HostIdeResponse | null {
    return this.inner.getIde(canonicalId, profile);
  }

  getVirtualFile(query: HostVirtualQuery): HostVirtualFileResponse {
    return this.inner.getVirtualFile(query);
  }

  listVirtualFiles(canonicalId: string): HostVirtualNodeKind[] {
    return this.inner.listVirtualFiles(canonicalId);
  }

  remove(canonicalOrAlias: string): HostRemoveResult | null {
    return this.inner.remove(canonicalOrAlias);
  }

  /**
   * Returns the analysis snapshot for a file as a native JS object, or null
   * if the file doesn't exist. When `analysisLevel` is not "full", computes
   * analysis on demand.
   */
  getAnalysis(canonicalOrAlias: string): unknown | null {
    return this.inner.getAnalysis(canonicalOrAlias);
  }

  /**
   * Sets the resolved import dependencies for a file, enabling Tier 2/3
   * smart invalidation (cross-file change tracking).
   */
  setImportDependencies(canonicalOrAlias: string, resolvedDeps: string[]): void {
    this.inner.setImportDependencies(canonicalOrAlias, resolvedDeps);
  }

  collectResolvableModuleReferenceSpecifiers(moduleReferences: HostModuleReference[]): string[] {
    return this.inner.collectResolvableModuleReferenceSpecifiers(moduleReferences);
  }

  resolveKnownModuleReferenceDependencies(
    ownerCanonicalId: string,
    moduleReferences: HostModuleReference[],
    knownIds: string[],
    extensions?: string[],
  ): string[] {
    return this.inner.resolveKnownModuleReferenceDependencies(
      ownerCanonicalId,
      moduleReferences,
      knownIds,
      extensions,
    );
  }

  /** Runs lint rules against a file and returns diagnostics with UTF-16 spans. */
  lint(canonicalOrAlias: string, config?: unknown): HostLintDiagnostic[] {
    return this.inner.lint(canonicalOrAlias, config);
  }

  /** Returns code actions (quick fixes) at a given UTF-16 offset. */
  getCodeActions(canonicalOrAlias: string, offset: number): HostCodeAction[] {
    return this.inner.getCodeActions(canonicalOrAlias, offset);
  }

  /** Returns metadata for all registered lint rules. */
  getLintRuleMetadata(): HostLintRuleMetadata[] {
    return this.inner.getLintRuleMetadata();
  }

  /** Returns document symbols for a file (SFC blocks → children). */
  getDocumentSymbols(canonicalOrAlias: string): HostDocumentSymbol[] {
    return this.inner.getDocumentSymbols(canonicalOrAlias);
  }

  /** Matches CSS selectors against template elements (three-valued matrix). */
  matchCssSelectors(canonicalOrAlias: string): HostSelectorMatchResult[] {
    return this.inner.matchCssSelectors(canonicalOrAlias);
  }
}

export async function createHost(config?: HostConfig): Promise<Host> {
  await initialize();
  return new Host(config);
}

export { Host as VerterHost };
