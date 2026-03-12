/**
 * Unified host adapter interface that works with both NAPI and WASM backends.
 *
 * - NAPI: `getAnalysis()` returns a JSON string → `JSON.parse()` → snapshot
 * - WASM: `getAnalysis()` returns a native JS object directly
 * - Standalone mode: creates its own host (auto-detects NAPI vs WASM)
 * - Integrated mode: accepts an existing host reference
 */

export interface HostUpsertRequest {
  inputId: string;
  source: string;
  fileKind?: "vue" | "sfc" | "vue_sfc" | "non_sfc" | "text" | "file";
}

export interface VerterHostAdapter {
  upsert(request: HostUpsertRequest): unknown;
  getAnalysis(canonicalOrAlias: string): unknown | null;
}

/**
 * Wrap an existing NAPI `VerterHost` instance.
 * NAPI `getAnalysis()` returns a JSON string that must be parsed.
 */
export function wrapNapiHost(host: {
  upsert(request: HostUpsertRequest): unknown;
  getAnalysis(canonicalOrAlias: string): string | null;
}): VerterHostAdapter {
  return {
    upsert(request) {
      return host.upsert(request);
    },
    getAnalysis(canonicalOrAlias) {
      const result = host.getAnalysis(canonicalOrAlias);
      if (result === null || result === undefined) return null;
      return JSON.parse(result);
    },
  };
}

/**
 * Wrap an existing WASM `Host` instance.
 * WASM `getAnalysis()` returns a native JS object directly.
 */
export function wrapWasmHost(host: {
  upsert(request: HostUpsertRequest): unknown;
  getAnalysis(canonicalOrAlias: string): unknown | null;
}): VerterHostAdapter {
  return {
    upsert(request) {
      return host.upsert(request);
    },
    getAnalysis(canonicalOrAlias) {
      return host.getAnalysis(canonicalOrAlias);
    },
  };
}

/**
 * Create a standalone host adapter using the NAPI backend.
 * Lazily loads `@verter/native`.
 */
export function createNapiAdapter(): VerterHostAdapter {
  // eslint-disable-next-line @typescript-eslint/no-require-imports
  const native = require("@verter/native");
  const host = new native.VerterHost({ devMode: false, analysisLevel: "full" });
  return wrapNapiHost(host);
}

/**
 * Create a standalone host adapter using the WASM backend.
 * Requires `@verter/wasm` to be installed and initialized.
 */
export async function createWasmAdapter(): Promise<VerterHostAdapter> {
  const wasm = await import("@verter/wasm");
  const host = await wasm.createHost({ devMode: false, analysisLevel: "full" });
  return wrapWasmHost(host);
}

/**
 * Auto-detect the best available backend and create a standalone adapter.
 * Prefers NAPI (faster), falls back to WASM.
 */
export function createAdapter(): VerterHostAdapter {
  try {
    return createNapiAdapter();
  } catch {
    throw new Error(
      "Failed to create host adapter. " +
        "Install @verter/native (Node.js) or use createWasmAdapter() for browsers.",
    );
  }
}
