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
  moduleClasses: string[][];
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

export interface StripTypesResult {
  /** The JavaScript output with TypeScript syntax removed */
  code: string;
  /** Any parse errors encountered */
  errors: string[];
}

export interface HostConfig {
  devMode?: boolean;
  compileErrorPolicy?: "strict" | "strictError" | "devServeLastKnownGood";
  lspScheme?: string;
  maxProfilesPerFile?: number;
}

export interface HostCompileProfile {
  filename?: string;
  isProduction?: boolean;
  ssr?: boolean;
  hmrStrategy?: "none" | "vite" | "webpack";
  componentId?: string;
  delimiters?: [string, string];
  customElements?: string[];
  comments?: boolean;
  runtimeModuleName?: string;
  forceVapor?: boolean;
  stripTs?: boolean;
  sourceMap?: boolean;
}

export interface HostVirtualNodeKind {
  kind: "main" | "script" | "template" | "style" | "custom";
  index?: number;
}

export interface HostSliceChanges {
  scriptChanged: boolean;
  templateChanged: boolean;
  styleIndicesChanged: number[];
  customIndicesChanged: number[];
  structureChanged: boolean;
  descriptorChanged: boolean;
}

export interface HostDiagnostic {
  severity: "error" | "warning" | "info";
  code: string;
  message: string;
  spanStart?: number;
  spanEnd?: number;
}

export interface HostDiagnosticsSnapshot {
  diagnostics: HostDiagnostic[];
  hasErrors: boolean;
}

export interface HostExternalSourceRequest {
  ownerCanonicalId: string;
  blockKind: "script" | "template" | "style" | "custom";
  index: number;
  specifier: string;
  resolvedCanonicalId: string;
}

export interface HostUpdateResult {
  canonicalId: string;
  changed: boolean;
  sliceChanges: HostSliceChanges;
  changedVirtualNodes: HostVirtualNodeKind[];
  removedVirtualNodes: HostVirtualNodeKind[];
  changedVirtualIds: string[];
  removedVirtualIds: string[];
  changedLspIds: string[];
  removedLspIds: string[];
  diagnostics: HostDiagnosticsSnapshot;
  externalSourceRequests: HostExternalSourceRequest[];
}

export interface HostResolvedId {
  canonicalId: string;
  nodeKind: HostVirtualNodeKind;
  existsInHost: boolean;
  bundlerId: string;
  lspId: string;
}

export interface HostVirtualMeta {
  scopeId?: string;
  blockType?: string;
  styleIndex?: number;
  customIndex?: number;
}

export interface HostVirtualFileResponse {
  id: string;
  code: string;
  sourceMap?: string;
  lang?: string;
  stale: boolean;
  diagnostics: HostDiagnosticsSnapshot;
  meta: HostVirtualMeta;
}

export interface HostUpsertRequest {
  canonicalId?: string;
  inputId: string;
  source: string;
  fileKind?: "vue" | "sfc" | "vue_sfc" | "non_sfc" | "text" | "file";
  aliases?: string[];
  compileProfile?: HostCompileProfile;
}

export interface HostStyleOverrideEntry {
  index: number;
  code: string;
  sourceMap?: string;
}

export interface HostStyleOverrideRequest {
  canonicalId: string;
  compileProfile?: HostCompileProfile;
  overrides: HostStyleOverrideEntry[];
}

export interface HostVirtualQuery {
  rawId?: string;
  canonicalId?: string;
  nodeKind?: HostVirtualNodeKind;
  compileProfile?: HostCompileProfile;
}

export interface HostRemoveResult {
  canonicalId: string;
}

export type WasmInput = string | Uint8Array;

type WasmCompileFn = (input: string, options?: unknown) => CodegenResult;
type WasmCompileBytesFn = (input: Uint8Array, options?: unknown) => CodegenResult;
type WasmStripTypesFn = (source: string) => StripTypesResult;
type WasmInitFn = () => Promise<unknown>;
type WasmHostResolveFn = (rawId: string) => HostResolvedId | null;
type WasmHostUpsertFn = (request: HostUpsertRequest) => HostUpdateResult;
type WasmHostApplyOverridesFn = (request: HostStyleOverrideRequest) => HostUpdateResult;
type WasmHostGetVirtualFileFn = (query: HostVirtualQuery) => HostVirtualFileResponse;
type WasmHostListVirtualFilesFn = (canonicalId: string) => HostVirtualNodeKind[];
type WasmHostRemoveFn = (canonicalOrAlias: string) => HostRemoveResult | null;
interface WasmHostBinding {
  resolve: WasmHostResolveFn;
  upsert: WasmHostUpsertFn;
  applyStyleOverrides: WasmHostApplyOverridesFn;
  getVirtualFile: WasmHostGetVirtualFileFn;
  listVirtualFiles: WasmHostListVirtualFilesFn;
  remove: WasmHostRemoveFn;
}
type WasmHostCtor = new (config?: HostConfig) => WasmHostBinding;

let wasmCompile: WasmCompileFn | null = null;
let wasmCompileBytes: WasmCompileBytesFn | null = null;
let wasmStripTypes: WasmStripTypesFn | null = null;
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
    wasmStripTypes = getOptionalExport<WasmStripTypesFn>(wasm, "stripTypes");
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
 * Synchronous compile - requires initialize() to have been called first.
 *
 * @param input - The Vue SFC source code (string or Uint8Array)
 * @param options - Optional compilation options
 * @returns The compiled result with code, source map, and code with inline source map
 * @throws If the WASM module has not been initialized
 */
export function compileSync(input: WasmInput, options?: CodegenOptions): CodegenResult {
  if (!initialized || !wasmCompile) {
    throw new Error("WASM module not initialized. Call initialize() first.");
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
 * Strip TypeScript syntax from a standalone .ts/.tsx file.
 *
 * @param source - The TypeScript source code
 * @returns The stripped JavaScript code and any parse errors
 * @throws If the WASM module has not been initialized
 */
export async function stripTypes(source: string): Promise<StripTypesResult> {
  await initialize();

  if (!wasmStripTypes) {
    throw new Error("WASM module not initialized or stripTypes not available");
  }

  return wasmStripTypes(source);
}

/**
 * Synchronous stripTypes - requires initialize() to have been called first.
 *
 * @param source - The TypeScript source code
 * @returns The stripped JavaScript code and any parse errors
 * @throws If the WASM module has not been initialized
 */
export function stripTypesSync(source: string): StripTypesResult {
  if (!initialized || !wasmStripTypes) {
    throw new Error("WASM module not initialized. Call initialize() first.");
  }

  return wasmStripTypes(source);
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

  applyStyleOverrides(request: HostStyleOverrideRequest): HostUpdateResult {
    return this.inner.applyStyleOverrides(request);
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
}

export async function createHost(config?: HostConfig): Promise<Host> {
  await initialize();
  return new Host(config);
}

export { Host as VerterHost };
