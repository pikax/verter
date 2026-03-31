/**
 * Engine key computation for the MetaRuntime pool.
 *
 * The key includes every analysis-affecting input so that different
 * configurations resolve to distinct engines. Presentation-only options
 * (schema, printer, forceUseTs) are excluded.
 */

import { createHash } from "node:crypto";
import { dirname as nativeDirname, resolve as nativeResolve, win32 } from "node:path";

export interface EngineKeyInput {
  backend: "napi" | "wasm";
  root: string;
  configKind: "tsconfig" | "inline";
  tsconfigPath?: string;
  configHash: string;
  nativeFlags: {
    analysisLevel: string;
  };
  typeExpansionBackend?: string;
}

/**
 * Normalize a file path for cross-platform key stability.
 * - Forward slashes
 * - Lowercase drive letter on Windows
 * - No trailing slash
 */
export function normalizePath(p: string): string {
  let norm = p.replace(/\\/g, "/").replace(/\/+$/, "");
  // Lowercase Windows drive letter (C: → c:)
  if (/^[A-Z]:/.test(norm)) {
    norm = norm[0].toLowerCase() + norm.slice(1);
  }
  return norm;
}

function looksLikeWindowsPath(p: string): boolean {
  return /^[A-Za-z]:([\\/]|$)/.test(p);
}

export function resolvePath(root: string, ...segments: string[]): string {
  const parts = [root, ...segments].filter((part) => part.length > 0);
  const resolved = parts.some(looksLikeWindowsPath)
    ? win32.resolve(...parts)
    : nativeResolve(...parts);
  return normalizePath(resolved);
}

export function dirnamePath(p: string): string {
  const dir = looksLikeWindowsPath(p) ? win32.dirname(p) : nativeDirname(p);
  return normalizePath(dir);
}

/**
 * Compute a stable hash of an object for use as a config fingerprint.
 */
export function stableHash(input: unknown): string {
  const json = stableSerialize(input);
  return createHash("sha256").update(json).digest("hex").slice(0, 16);
}

/**
 * Selective loading makes per-request discovery filters non-semantic for
 * engine reuse. Strip those keys before hashing so engine identity stays
 * stable across different `include` arrays for the same root/config.
 */
export function stableSelectiveConfigHash(input: unknown): string {
  if (!input || typeof input !== "object" || Array.isArray(input)) {
    return stableHash(input);
  }

  const normalized = { ...(input as Record<string, unknown>) };
  delete normalized.include;
  return stableHash(normalized);
}

function stableSerialize(input: unknown): string {
  if (input === null || typeof input !== "object") {
    return JSON.stringify(input);
  }

  if (Array.isArray(input)) {
    return `[${input.map((item) => stableSerialize(item)).join(",")}]`;
  }

  const entries = Object.entries(input as Record<string, unknown>)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, value]) => `${JSON.stringify(key)}:${stableSerialize(value)}`);

  return `{${entries.join(",")}}`;
}

/**
 * Compute the engine key from normalized inputs.
 * Two calls with the same inputs must produce the same string.
 */
export function computeEngineKey(input: EngineKeyInput): string {
  const parts = [
    input.backend,
    normalizePath(input.root),
    input.configKind,
    input.tsconfigPath ? normalizePath(input.tsconfigPath) : "",
    input.configHash,
    input.nativeFlags.analysisLevel,
    input.typeExpansionBackend ?? "verter",
  ];
  return parts.join("|");
}
