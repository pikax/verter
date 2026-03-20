/**
 * Engine key computation for the MetaRuntime pool.
 *
 * The key includes every analysis-affecting input so that different
 * configurations resolve to distinct engines. Presentation-only options
 * (schema, printer, forceUseTs) are excluded.
 */

import { createHash } from "node:crypto";

export interface EngineKeyInput {
  backend: "napi" | "wasm";
  root: string;
  configKind: "tsconfig" | "inline";
  tsconfigPath?: string;
  configHash: string;
  workspaceIdentity?: string;
  nativeFlags: {
    analysisLevel: string;
    deepMacroResolutionType: boolean;
  };
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

/**
 * Compute a stable hash of an object for use as a config fingerprint.
 */
export function stableHash(input: unknown): string {
  const json = stableSerialize(input);
  return createHash("sha256").update(json).digest("hex").slice(0, 16);
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
    input.workspaceIdentity ?? "",
    input.nativeFlags.analysisLevel,
    input.nativeFlags.deepMacroResolutionType ? "1" : "0",
  ];
  return parts.join("|");
}

// Workspace identity tracking via WeakMap
const workspaceIdentities = new WeakMap<object, string>();
let nextWorkspaceId = 1;

/**
 * Get or assign a stable identity string for a workspace object.
 * Uses a WeakMap so identity doesn't prevent GC of the workspace.
 */
export function getWorkspaceIdentity(workspace: object): string {
  let id = workspaceIdentities.get(workspace);
  if (!id) {
    id = `ws-${nextWorkspaceId++}`;
    workspaceIdentities.set(workspace, id);
  }
  return id;
}
