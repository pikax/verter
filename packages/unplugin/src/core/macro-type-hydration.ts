/**
 * Macro type dependency hydration for the unplugin.
 *
 * When a Vue SFC uses `defineProps<ExternalType>()` where the type comes from
 * an external package (e.g. `import type { AnimationOptions } from "motion"`),
 * the host needs the `.d.ts` declaration file loaded — not just the runtime `.js`.
 *
 * This module resolves package-backed type dependencies by:
 * 1. Reading `macroTypeDeps` from the host analysis
 * 2. For each dep, resolving the specifier via bundler resolve (to get the runtime file)
 * 3. Walking up to the nearest `package.json` to find the declaration entry
 * 4. Upserting the `.d.ts` file into the host
 * 5. Recursively traversing relative imports inside hydrated declaration files
 */
import type { VerterHost, HostDependencyResolution } from "@verter/native";

type ResolveHook = (
  source: string,
  importer: string,
  options: { skipSelf: true },
) => Promise<unknown> | unknown;

interface MacroTypeDep {
  typeName: string;
  importSource: string;
  macroKind: string;
}

interface AnalysisImport {
  source: string;
}

interface FileAnalysisSnapshot {
  imports?: AnalysisImport[];
  macroTypeDeps?: MacroTypeDep[];
}

function normalizePath(p: string): string {
  return p.replace(/\\/g, "/");
}

function isRelativeImport(specifier: string): boolean {
  return specifier.startsWith(".");
}

function resolvedIdFromHookResult(result: unknown): string | null {
  if (!result) return null;
  if (typeof result === "string") {
    return result.startsWith("\0") || result.includes("?") ? null : result;
  }
  if (typeof result !== "object") return null;
  const resolved = result as { id?: unknown; external?: unknown };
  if (resolved.external) return null;
  if (typeof resolved.id !== "string") return null;
  if (resolved.id.startsWith("\0") || resolved.id.includes("?")) return null;
  return resolved.id;
}

/**
 * Find the declaration entry point for a package given the runtime entry.
 * Walks up from the resolved runtime file to find `package.json`, then
 * checks (in order): `types`, `typings`, `exports["."].types`, sibling `.d.ts`,
 * package-root `index.d.ts`.
 */
async function findPackageDeclarationEntry(
  runtimeEntry: string,
): Promise<string | null> {
  const fs = await import("fs");
  const path = await import("path");

  let dir = path.dirname(runtimeEntry);
  const root = path.parse(dir).root;

  // Walk up to find package.json
  let pkgJsonPath: string | null = null;
  let pkgDir: string | null = null;
  while (dir !== root) {
    const candidate = path.join(dir, "package.json");
    if (fs.existsSync(candidate)) {
      pkgJsonPath = candidate;
      pkgDir = dir;
      break;
    }
    dir = path.dirname(dir);
  }

  if (!pkgJsonPath || !pkgDir) return null;

  let pkg: Record<string, unknown>;
  try {
    pkg = JSON.parse(fs.readFileSync(pkgJsonPath, "utf-8"));
  } catch {
    return null;
  }

  // 1. `types` field
  if (typeof pkg.types === "string") {
    const typesPath = path.resolve(pkgDir, pkg.types);
    if (fs.existsSync(typesPath)) return normalizePath(typesPath);
  }

  // 2. `typings` field (deprecated alias)
  if (typeof pkg.typings === "string") {
    const typingsPath = path.resolve(pkgDir, pkg.typings);
    if (fs.existsSync(typingsPath)) return normalizePath(typingsPath);
  }

  // 3. `exports` with `types` condition
  if (pkg.exports && typeof pkg.exports === "object") {
    const typesFromExports = findTypesInExports(pkg.exports as Record<string, unknown>, pkgDir);
    if (typesFromExports) {
      const fs2 = await import("fs");
      if (fs2.existsSync(typesFromExports)) return normalizePath(typesFromExports);
    }
  }

  // 4. Sibling `.d.ts` next to the runtime entry
  const runtimeBase = runtimeEntry.replace(/\.(js|mjs|cjs|jsx)$/, "");
  const siblingDts = runtimeBase + ".d.ts";
  if (fs.existsSync(siblingDts)) return normalizePath(siblingDts);

  const siblingDmts = runtimeBase + ".d.mts";
  if (fs.existsSync(siblingDmts)) return normalizePath(siblingDmts);

  // 5. Package-root `index.d.ts`
  const rootDts = path.join(pkgDir, "index.d.ts");
  if (fs.existsSync(rootDts)) return normalizePath(rootDts);

  return null;
}

/**
 * Recursively search `exports` for a `types` condition.
 * Handles nested condition objects like `{ ".": { "types": "./dist/index.d.ts" } }`.
 */
function findTypesInExports(
  exports: Record<string, unknown>,
  pkgDir: string,
): string | null {
  const path = require("path") as typeof import("path");

  // Direct `types` condition
  if (typeof exports.types === "string") {
    return path.resolve(pkgDir, exports.types);
  }

  // Check "." entry
  const rootExport = exports["."];
  if (rootExport && typeof rootExport === "object") {
    const root = rootExport as Record<string, unknown>;
    if (typeof root.types === "string") {
      return path.resolve(pkgDir, root.types);
    }
    // Check nested conditions (e.g. { ".": { "import": { "types": "..." } } })
    for (const key of ["import", "require", "default"]) {
      const cond = root[key];
      if (cond && typeof cond === "object") {
        const condObj = cond as Record<string, unknown>;
        if (typeof condObj.types === "string") {
          return path.resolve(pkgDir, condObj.types);
        }
      }
    }
  }

  return null;
}

/**
 * Hydrate macro type dependencies for a Vue SFC.
 *
 * After `host.upsert()` and `resolveUpsertDependencies()`, this function
 * checks if the SFC has macro type deps that reference bare package specifiers.
 * For each, it resolves the package's declaration entry and upserts it into
 * the host so the macro type resolution can succeed.
 */
export async function hydrateMacroTypeDeps(
  host: VerterHost,
  filename: string,
  resolveId?: ResolveHook,
): Promise<void> {
  // Get the analysis for the entry file to find macro type deps.
  const analysisJson = host.getAnalysis(normalizePath(filename));
  if (!analysisJson) return;

  let analysis: FileAnalysisSnapshot;
  try {
    analysis = JSON.parse(analysisJson);
  } catch {
    return;
  }

  if (!analysis.macroTypeDeps?.length) return;

  const fs = await import("fs");
  const path = await import("path");

  // Collect unique specifiers from macro type deps.
  const specifiers = [...new Set(analysis.macroTypeDeps.map((dep) => dep.importSource))];

  // Filter to bare (non-relative) specifiers — relative ones are already handled
  // by resolveUpsertDependencies.
  const bareSpecifiers = specifiers.filter((s) => !isRelativeImport(s));
  if (bareSpecifiers.length === 0) return;

  const resolutions: HostDependencyResolution[] = [];
  const visited = new Set<string>();

  for (const specifier of bareSpecifiers) {
    // Try to resolve via bundler hook first
    let runtimeEntry: string | null = null;
    if (resolveId) {
      const result = resolvedIdFromHookResult(
        await resolveId(specifier, filename, { skipSelf: true }),
      );
      if (result) runtimeEntry = result;
    }

    // If no bundler resolve, try Node resolution
    if (!runtimeEntry) {
      try {
        const { createRequire } = await import("module");
        const require = createRequire(filename);
        runtimeEntry = require.resolve(specifier);
      } catch {
        continue;
      }
    }

    if (!runtimeEntry) continue;

    // Find the declaration entry from package.json
    const dtsPath = await findPackageDeclarationEntry(runtimeEntry);
    if (!dtsPath) {
      // Fallback: if the runtime entry IS a .ts/.d.ts file, use it directly
      if (runtimeEntry.endsWith(".ts") || runtimeEntry.endsWith(".d.ts")) {
        const normalizedId = normalizePath(runtimeEntry);
        if (!visited.has(normalizedId)) {
          visited.add(normalizedId);
          try {
            const source = fs.readFileSync(runtimeEntry);
            host.upsert({
              inputId: normalizedId,
              source,
              fileKind: "non_sfc",
            });
            resolutions.push({
              specifier,
              resolvedCanonicalId: normalizedId,
            });
          } catch {
            // Can't read the file
          }
        }
      }
      continue;
    }

    // Upsert the declaration file
    const normalizedDtsPath = normalizePath(dtsPath);
    if (visited.has(normalizedDtsPath)) continue;
    visited.add(normalizedDtsPath);

    try {
      const source = fs.readFileSync(dtsPath);
      host.upsert({
        inputId: normalizedDtsPath,
        source,
        fileKind: "non_sfc",
      });
      resolutions.push({
        specifier,
        resolvedCanonicalId: normalizedDtsPath,
      });
    } catch {
      continue;
    }

    // Recursively traverse relative imports inside the hydrated .d.ts
    const queue = [normalizedDtsPath];
    while (queue.length > 0) {
      const currentFile = queue.shift()!;

      const depAnalysisJson = host.getAnalysis(currentFile);
      if (!depAnalysisJson) continue;

      let depAnalysis: FileAnalysisSnapshot;
      try {
        depAnalysis = JSON.parse(depAnalysisJson);
      } catch {
        continue;
      }

      const depResolutions: HostDependencyResolution[] = [];

      for (const imp of depAnalysis.imports ?? []) {
        if (!isRelativeImport(imp.source)) continue;

        const absBase = path.resolve(path.dirname(currentFile), imp.source);
        // Try common declaration file extensions
        const candidates = [
          absBase + ".d.ts",
          absBase + ".ts",
          absBase + "/index.d.ts",
          absBase + "/index.ts",
          absBase,
        ];

        for (const candidate of candidates) {
          const normalized = normalizePath(candidate);
          if (visited.has(normalized)) {
            depResolutions.push({
              specifier: imp.source,
              resolvedCanonicalId: normalized,
            });
            break;
          }
          if (!fs.existsSync(candidate)) continue;

          visited.add(normalized);
          try {
            const depSource = fs.readFileSync(candidate);
            host.upsert({
              inputId: normalized,
              source: depSource,
              fileKind: "non_sfc",
            });
            depResolutions.push({
              specifier: imp.source,
              resolvedCanonicalId: normalized,
            });
            queue.push(normalized);
            break;
          } catch {
            continue;
          }
        }
      }

      if (depResolutions.length > 0) {
        host.setImportDependencies(currentFile, depResolutions);
      }
    }
  }

  // Update the entry file's import dependencies to include the hydrated resolutions
  if (resolutions.length > 0) {
    host.setImportDependencies(filename, resolutions);
  }
}
