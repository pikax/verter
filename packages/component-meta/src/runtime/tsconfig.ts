import { readFileSync } from "node:fs";
import { dirname } from "node:path";
import { parse, type ParseError } from "jsonc-parser";
import { resolvePath, tsconfigConfigHash } from "./engine-key.js";

/** JSONC syntax shared by discovery and pooled configuration identity. */
export function parseTsconfigText(raw: string): Record<string, unknown> | null {
  const errors: ParseError[] = [];
  const value: unknown = parse(raw, errors, { allowTrailingComma: true });
  return errors.length === 0 && value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

/**
 * Fingerprint the inputs the native tsconfig loader can read, including its
 * ordered extends chain and package resolution probes. Native owns merging
 * options and membership; this reader only prevents reuse of stale engines.
 * Missing or malformed inputs retain their bytes/absence in the identity.
 */
export function hashTsconfigConfig(tsconfigPath: string): string {
  const inputs = new Map<string, string | null>();
  const visited = new Map<string, number>();
  const read = (path: string): string | null => {
    path = resolvePath(path);
    if (!inputs.has(path)) {
      try {
        inputs.set(path, readFileSync(path, "utf8"));
      } catch {
        inputs.set(path, null);
      }
    }
    return inputs.get(path)!;
  };
  const configFile = (path: string): string | undefined => {
    if (read(path) !== null) return path;
    const withJson = `${path}.json`;
    return read(withJson) !== null ? withJson : undefined;
  };
  const resolveExtends = (dir: string, spec: string): string | undefined => {
    if (spec.startsWith(".")) return configFile(resolvePath(dir, spec));
    // Match the workspace loader's package search and tsconfig field.
    for (;;) {
      const candidate = resolvePath(dir, "node_modules", spec);
      const direct = configFile(candidate);
      if (direct) return direct;
      const manifest = read(resolvePath(candidate, "package.json"));
      const target = manifest === null ? undefined : parseTsconfigText(manifest)?.tsconfig;
      if (typeof target === "string") {
        const fromPackage = configFile(resolvePath(candidate, target));
        if (fromPackage) return fromPackage;
      }
      const fallback = resolvePath(candidate, "tsconfig.json");
      if (read(fallback) !== null) return fallback;
      const parent = dirname(dir);
      if (!parent || parent === dir) return undefined;
      dir = parent;
    }
  };
  const visit = (path: string, depth: number): void => {
    path = resolvePath(path);
    // The workspace loader also bounds inheritance at depth eight.
    if (depth > 8 || (visited.get(path) ?? Infinity) <= depth) return;
    visited.set(path, depth);
    const raw = read(path);
    const config = raw === null ? null : parseTsconfigText(raw);
    const specs = config?.extends;
    for (const spec of typeof specs === "string" ? [specs] : Array.isArray(specs) ? specs : []) {
      if (typeof spec !== "string") continue;
      const target = resolveExtends(dirname(path), spec);
      if (target) visit(target, depth + 1);
    }
  };
  visit(tsconfigPath, 0);
  return tsconfigConfigHash([...inputs]);
}
