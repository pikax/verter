/**
 * WASM Loader for Verter Playground
 *
 * Supports loading WASM from:
 * 1. Local build ("This Build") — the default bundled version
 * 2. Nightly commits — from GitHub Release "nightly" assets
 * 3. Published releases — from jsDelivr CDN
 *
 * Key technique: wasm-bindgen glue JS has a singleton guard
 * (`if (wasm !== undefined) return wasm;`). To load a different version,
 * we fetch the glue JS as text, create a Blob URL for a fresh module,
 * and pass the WASM binary as an ArrayBuffer to bypass URL resolution.
 */

const GITHUB_REPO = "pikax/verter";
const GITHUB_RELEASE_BASE = `https://github.com/${GITHUB_REPO}/releases/download`;
const JSDELIVR_BASE = "https://cdn.jsdelivr.net/npm/@verter/wasm";

export interface WasmModule {
  VerterHost?: new (config?: unknown) => {
    resolve: (rawId: string) => unknown;
    upsert: (request: unknown) => unknown;
    getVirtualFile: (query: unknown) => unknown;
    listVirtualFiles: (canonicalId: string) => unknown;
    remove: (canonicalOrAlias: string) => unknown;
    collectResolvableModuleReferenceSpecifiers?: (moduleReferences: unknown) => unknown;
    resolveKnownModuleReferenceDependencies?: (
      ownerCanonicalId: string,
      moduleReferences: unknown,
      knownIds: string[],
      extensions?: string[],
    ) => unknown;
  };
  default: (input?: unknown) => Promise<unknown>;
}

function isNodeRuntime(): boolean {
  const maybeProcess = (globalThis as { process?: { versions?: { node?: string } } }).process;
  return typeof maybeProcess?.versions?.node === "string";
}

async function resolveLocalWasmModuleOrPath(): Promise<string | Uint8Array> {
  if (!isNodeRuntime()) {
    return "/verter_wasm_bg.wasm";
  }

  const wasmUrl = new URL("../../../wasm/wasm/verter_wasm_bg.wasm", import.meta.url);
  const nodeFs = (await import(/* @vite-ignore */ "node:fs/promises")) as unknown as {
    readFile(path: URL): Promise<Uint8Array>;
  };

  return nodeFs.readFile(wasmUrl);
}

/**
 * Load the locally bundled WASM (default "This Build" mode).
 * Uses the standard import path resolved by Vite.
 */
export async function loadLocalWasm(): Promise<WasmModule> {
  // @ts-ignore - Dynamic import of wasm glue code
  const mod = await import("verter-wasm-glue");
  await mod.default({ module_or_path: await resolveLocalWasmModuleOrPath() });
  return mod;
}

/**
 * Load WASM from a nightly commit (GitHub Release assets).
 *
 * @param shortSha - 7-char commit hash (e.g. "6178ecb")
 */
export async function loadCommitWasm(shortSha: string): Promise<WasmModule> {
  const wasmUrl = `${GITHUB_RELEASE_BASE}/nightly/verter_wasm_bg-${shortSha}.wasm`;
  const glueUrl = `${GITHUB_RELEASE_BASE}/nightly/verter_wasm-${shortSha}.js`;

  return loadRemoteWasm(glueUrl, wasmUrl);
}

/**
 * Load WASM from a published npm release (jsDelivr CDN).
 *
 * @param version - Semver version (e.g. "0.0.1-alpha.1")
 */
export async function loadReleaseWasm(version: string): Promise<WasmModule> {
  const wasmUrl = `${JSDELIVR_BASE}@${version}/wasm/verter_wasm_bg.wasm`;
  const glueUrl = `${JSDELIVR_BASE}@${version}/wasm/verter_wasm.js`;

  return loadRemoteWasm(glueUrl, wasmUrl);
}

/**
 * Generic loader for remote WASM + glue JS.
 *
 * 1. Fetch glue JS as text
 * 2. Patch singleton guard and import.meta.url default
 * 3. Create Blob URL for fresh module instance
 * 4. Fetch WASM as ArrayBuffer
 * 5. Call mod.default(arrayBuffer) — bypasses all URL resolution
 */
async function loadRemoteWasm(glueUrl: string, wasmUrl: string): Promise<WasmModule> {
  // Fetch both in parallel
  const [glueResponse, wasmResponse] = await Promise.all([
    fetch(glueUrl, { mode: "cors" }),
    fetch(wasmUrl, { mode: "cors" }),
  ]);

  if (!glueResponse.ok) {
    throw new Error(`Failed to fetch glue JS: ${glueResponse.status} ${glueResponse.statusText}`);
  }
  if (!wasmResponse.ok) {
    throw new Error(`Failed to fetch WASM: ${wasmResponse.status} ${wasmResponse.statusText}`);
  }

  let glueText = await glueResponse.text();
  const wasmBuffer = await wasmResponse.arrayBuffer();

  // Patch 1: Remove the singleton guard so we get a fresh instance
  // `if (wasm !== undefined) return wasm;` appears in both initSync and __wbg_init
  glueText = glueText.replace(
    /if\s*\(\s*wasm\s*!==\s*undefined\s*\)\s*return\s+wasm\s*;/g,
    "/* singleton guard removed */",
  );

  // Patch 2: Remove the import.meta.url default in __wbg_init
  // This prevents the module from trying to resolve a relative URL
  glueText = glueText.replace(
    /if\s*\(\s*typeof\s+module_or_path\s*===\s*['"]undefined['"]\s*\)/,
    "if (false)",
  );

  // Create a Blob URL for a fresh module instance
  const blob = new Blob([glueText], { type: "application/javascript" });
  const blobUrl = URL.createObjectURL(blob);

  try {
    const mod = await import(/* @vite-ignore */ blobUrl);
    // Pass ArrayBuffer directly — bypasses all URL/fetch resolution in __wbg_init
    await mod.default({ module_or_path: wasmBuffer });
    return mod;
  } finally {
    URL.revokeObjectURL(blobUrl);
  }
}
