import { createRequire } from "node:module";

/**
 * Unified host adapter interface that works with both NAPI and WASM backends.
 *
 * - NAPI: `getAnalysis()` returns a JSON string → `JSON.parse()` → snapshot
 * - WASM: `getAnalysis()` returns a native JS object directly
 * - Standalone mode: creates its own host (auto-detects NAPI vs WASM)
 * - Integrated mode: accepts an existing host reference
 */

/** Request to compile or update a file in the host. */
export interface HostUpsertRequest {
  /** File identifier (path or canonical ID). */
  inputId: string;
  /** Full source text of the file. */
  source: string;
  /** File kind hint. Defaults to auto-detection based on extension. */
  fileKind?: "vue" | "sfc" | "vue_sfc" | "non_sfc" | "text" | "file";
}

/**
 * Unified interface over NAPI and WASM host backends.
 *
 * Use `createAdapter()` for auto-detection, or `wrapNapiHost()`/`wrapWasmHost()`
 * to wrap an existing host instance.
 */
export interface VerterHostAdapter {
  /** Compile or update a file. */
  upsert(request: HostUpsertRequest): unknown;
  /** Retrieve the analysis snapshot for a file, or `null` if not found. */
  getAnalysis(canonicalOrAlias: string): unknown | null;
  /** Resolve imported types for a file's macro type dependencies. Returns JSON or null. */
  resolveImportedTypes?(canonicalOrAlias: string): string | null;
  /** Configure project-scoped path alias resolution (optional). */
  configureProjects?(
    projects: {
      root: string;
      workspaceRoot: string;
      tsconfigPath?: string;
      compilerOptions?: {
        baseUrl?: string;
        paths?: { pattern: string; targets: string[] }[];
      };
    }[],
  ): void;
}

/**
 * Wrap an existing NAPI `VerterHost` instance.
 * NAPI `getAnalysis()` returns a JSON string that must be parsed.
 */
export function wrapNapiHost(host: {
  upsert(request: HostUpsertRequest): unknown;
  getAnalysis(canonicalOrAlias: string): string | null;
  resolveImportedTypes?(canonicalOrAlias: string): string | null;
  configureProjects?(projects: unknown[]): void;
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
    resolveImportedTypes(canonicalOrAlias) {
      return host.resolveImportedTypes?.(canonicalOrAlias) ?? null;
    },
    configureProjects(projects) {
      host.configureProjects?.(projects);
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
  // @verter/native is CJS-only — use createRequire for ESM compatibility.
  const _require = typeof require === "function" ? require : createRequire(import.meta.url);
  const native = _require("@verter/native");
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
