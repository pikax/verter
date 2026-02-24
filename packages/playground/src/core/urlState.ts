import { zlibSync, unzlibSync, strToU8, strFromU8 } from "fflate";
import type { CompilerOptions, OutputMode } from "./types";
import type { ImportMap } from "./importMap";
import { isDefaultImport } from "./importMap";

export interface SerializedState {
  files: Record<string, string>; // filename -> code
  activeFile: string;
  outputMode: OutputMode;
  compilerOptions: CompilerOptions;
  importMap?: ImportMap;
  vueVersion?: string; // Vue runtime version (from _version)
  tsVersion?: string; // TypeScript version (from _tsVersion)
  verterVersion?: string; // Verter WASM version (from _verterVersion)
}

// Known metadata keys that map to SerializedState fields (not user files)
const METADATA_KEYS = new Set([
  "_version",
  "_tsVersion",
  "_verterVersion",
  "_activeFile",
  "_outputMode",
  "_isProduction",
  "_ssr",
]);

export function serializeToHash(state: SerializedState): void {
  const flat: Record<string, string> = {};

  // User files
  for (const [filename, code] of Object.entries(state.files)) {
    flat[filename] = code;
  }

  // Import map: strip builtin imports, only include if custom imports remain
  if (state.importMap?.imports) {
    const customImports: Record<string, string> = {};
    for (const [key, value] of Object.entries(state.importMap.imports)) {
      if (!isDefaultImport(key, value, state.vueVersion)) {
        customImports[key] = value;
      }
    }
    const hasCustomImports = Object.keys(customImports).length > 0;
    const hasScopes = state.importMap.scopes && Object.keys(state.importMap.scopes).length > 0;
    if (hasCustomImports || hasScopes) {
      const importMapObj: ImportMap = { imports: customImports };
      if (hasScopes) {
        importMapObj.scopes = state.importMap.scopes;
      }
      flat["import-map.json"] = JSON.stringify(importMapObj);
    }
  }

  // Version metadata
  if (state.vueVersion) {
    flat["_version"] = state.vueVersion;
  }
  if (state.tsVersion && state.tsVersion !== "latest") {
    flat["_tsVersion"] = state.tsVersion;
  }
  if (state.verterVersion && state.verterVersion !== "local") {
    flat["_verterVersion"] = state.verterVersion;
  }

  // Verter metadata (only non-defaults)
  if (state.activeFile && state.activeFile !== "App.vue") {
    flat["_activeFile"] = state.activeFile;
  }
  if (state.outputMode && state.outputMode !== "preview") {
    flat["_outputMode"] = state.outputMode;
  }
  if (state.compilerOptions?.isProduction) {
    flat["_isProduction"] = "true";
  }
  if (state.compilerOptions?.ssr) {
    flat["_ssr"] = "true";
  }

  // Encode: JSON → fflate zlib level 9 → base64
  const json = JSON.stringify(flat);
  const compressed = zlibSync(strToU8(json), { level: 9 });
  const base64 = btoa(strFromU8(compressed, true));
  history.replaceState(null, "", `#${base64}`);
}

export function deserializeFromHash(): SerializedState | null {
  const hash = location.hash.slice(1);
  if (!hash) return null;

  try {
    const flat = decodeHash(hash);
    if (!flat || typeof flat !== "object") return null;
    return flatToState(flat);
  } catch {
    return null;
  }
}

function decodeHash(hash: string): Record<string, string> | null {
  try {
    const binary = atob(hash);
    const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));

    let json: string;
    // Detect zlib magic byte (0x78)
    if (bytes[0] === 0x78) {
      json = strFromU8(unzlibSync(bytes));
    } else {
      // Legacy Vue format: plain base64-encoded UTF-8 (no zlib)
      json = decodeURIComponent(escape(binary));
    }

    return JSON.parse(json);
  } catch {
    return null;
  }
}

function flatToState(flat: Record<string, string>): SerializedState {
  const files: Record<string, string> = {};
  let importMap: ImportMap | undefined;

  for (const [key, value] of Object.entries(flat)) {
    if (key === "import-map.json") {
      try {
        importMap = JSON.parse(value) as ImportMap;
      } catch {
        // Invalid import map JSON — ignore
      }
    } else if (key.startsWith("_")) {
      // Metadata key — handled below, skip
    } else {
      files[key] = value;
    }
  }

  return {
    files,
    activeFile: flat["_activeFile"] || "App.vue",
    outputMode: (flat["_outputMode"] as OutputMode) || "preview",
    compilerOptions: {
      isProduction: flat["_isProduction"] === "true",
      ssr: flat["_ssr"] === "true",
    },
    importMap,
    vueVersion: flat["_version"],
    tsVersion: flat["_tsVersion"],
    verterVersion: flat["_verterVersion"],
  };
}
