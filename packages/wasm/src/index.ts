export interface FeatureFlags {
  /** Enable Options API support (default: true) */
  optionsApi?: boolean;
  /** Enable reactive destructure for defineProps (default: true) */
  propsDestructure?: boolean;
}

export interface CodegenOptions {
  /** The filename for source map generation */
  filename?: string;
  /** Whether to include source content in the source map */
  includeSourceContent?: boolean;
  /** SSR mode */
  ssr?: boolean;
  /** Production mode - affects component ID generation and optimizations */
  isProduction?: boolean;
  /** Custom component ID (overrides auto-generation from filename) */
  componentId?: string;
  /** Feature flags for codegen */
  features?: FeatureFlags;
}

export interface CodegenResult {
  /** The transformed code */
  code: string;
  /** The source map as JSON string */
  sourceMap: string;
  /** The transformed code with inline source map appended */
  codeWithSourceMap: string;
  /** Time taken for the Rust pipeline in milliseconds */
  durationMs: number;
}

export type WasmInput = string | Uint8Array;

type WasmCompileFn = (input: string, options?: unknown) => CodegenResult;
type WasmCompileBytesFn = (input: Uint8Array, options?: unknown) => CodegenResult;
type WasmInitFn = () => Promise<unknown>;

let wasmCompile: WasmCompileFn | null = null;
let wasmCompileBytes: WasmCompileBytesFn | null = null;
let initialized = false;
let initPromise: Promise<void> | null = null;

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
    wasmCompileBytes = (wasm.compileBytes as WasmCompileBytesFn) ?? null;
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
