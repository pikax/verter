import fs from "fs";
import path from "path";

/**
 * Strip JSON comments (line and block) for JSONC parsing.
 */
function stripJsonComments(text: string): string {
  return text.replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, "");
}

/**
 * Load Nuxt path aliases from `.nuxt/tsconfig.json`.
 * Returns a map of alias patterns to absolute resolved paths, or null if the file doesn't exist.
 */
export function loadNuxtPathAliases(root: string): Map<string, string[]> | null {
  const tsconfigPath = path.join(root, ".nuxt", "tsconfig.json");
  let raw: string;
  try {
    raw = fs.readFileSync(tsconfigPath, "utf-8");
  } catch {
    return null;
  }

  let parsed: any;
  try {
    parsed = JSON.parse(stripJsonComments(raw));
  } catch {
    return null;
  }

  const paths: Record<string, string[]> | undefined = parsed?.compilerOptions?.paths;
  if (!paths) return null;

  const baseUrl = parsed.compilerOptions?.baseUrl ?? ".";
  // baseUrl is relative to the tsconfig location (.nuxt/), so resolve from .nuxt dir
  const nuxtDir = path.join(root, ".nuxt");
  const baseDir = path.resolve(nuxtDir, baseUrl);

  const result = new Map<string, string[]>();
  for (const [alias, targets] of Object.entries(paths)) {
    const resolved = targets.map((t) => path.resolve(baseDir, t));
    result.set(alias, resolved);
  }

  return result;
}

/** Extensions to try when resolving wildcard aliases without explicit extension. */
const RESOLVE_EXTENSIONS = ["", ".ts", ".js", ".vue", ".mjs", ".d.ts"];
const INDEX_FILES = ["index.ts", "index.js"];

/** Hardcoded core Nuxt aliases as fallback when .nuxt/tsconfig.json is unavailable. */
const HARDCODED_ALIASES: Record<string, string> = {
  "#imports": "imports.d.ts",
  "#components": "components.d.ts",
  "#app": "nuxt.d.ts",
  "#app/composables": "imports.d.ts",
  "#build": "",
};

/**
 * Resolve a Nuxt `#`-prefixed import to an absolute file path.
 *
 * @param id - The import specifier (e.g., `#imports`, `#ui/components/Button.vue`)
 * @param aliases - Map from `loadNuxtPathAliases`, or null for hardcoded fallback
 * @param root - Project root directory
 * @returns Resolved absolute path, or null if not found
 */
export function resolveNuxtAlias(
  id: string,
  aliases: Map<string, string[]> | null,
  root: string,
): string | null {
  if (aliases === null) {
    return resolveHardcodedAlias(id, root);
  }

  // Exact match
  const exact = aliases.get(id);
  if (exact) {
    for (const target of exact) {
      if (tryExists(target)) return target;
    }
  }

  // Wildcard match: find patterns like "#ui/*"
  for (const [pattern, targets] of aliases) {
    if (!pattern.endsWith("/*")) continue;
    const prefix = pattern.slice(0, -2); // "#ui"
    if (!id.startsWith(prefix + "/")) continue;

    const rest = id.slice(prefix.length + 1); // "components/Button.vue"
    for (const target of targets) {
      const targetBase = target.slice(0, -2); // strip trailing "/*"
      const candidate = path.join(targetBase, rest);
      for (const ext of RESOLVE_EXTENSIONS) {
        const full = candidate + ext;
        if (tryExists(full)) return full;
      }
      // Try index files (e.g., #shared/auth → shared/auth/index.ts)
      for (const idx of INDEX_FILES) {
        const full = path.join(candidate, idx);
        if (tryExists(full)) return full;
      }
    }
  }

  return null;
}

function resolveHardcodedAlias(id: string, root: string): string | null {
  const nuxtDir = path.join(root, ".nuxt");

  // Direct match
  if (id in HARDCODED_ALIASES) {
    const target = path.join(nuxtDir, HARDCODED_ALIASES[id]);
    if (tryExists(target)) return target;
  }

  // Prefix match
  for (const [prefix, file] of Object.entries(HARDCODED_ALIASES)) {
    if (id.startsWith(prefix + "/")) {
      const rest = id.slice(prefix.length + 1);
      const base = path.join(nuxtDir, path.dirname(file));
      const candidate = path.join(base, rest);
      for (const ext of ["", ".ts", ".d.ts", ".js", ".mjs"]) {
        const full = candidate + ext;
        if (tryExists(full)) return full;
      }
    }
  }

  return null;
}

function tryExists(filePath: string): boolean {
  try {
    const stat = fs.statSync(filePath, { throwIfNoEntry: false });
    return stat != null && stat.isFile();
  } catch {
    return false;
  }
}
