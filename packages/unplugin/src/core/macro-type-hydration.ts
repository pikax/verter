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
import { dirname, join, parse, resolve } from "path";
import { existsSync, readFileSync } from "fs";
import type { VerterHost, HostDependencyResolution, Workspace } from "@verter/native";

type ResolveHook = (
  source: string,
  importer: string,
  options: { skipSelf: true },
) => Promise<unknown> | unknown;

/**
 * Minimal file access interface used by this module.
 * When a `Workspace` is available, it wraps the native workspace.
 * Otherwise, falls back to synchronous `node:fs` calls.
 */
interface FileAccess {
  fileExists(path: string): boolean;
  readFile(path: string): string | null;
}

function fileAccessFromWorkspace(ws: Workspace): FileAccess {
  return {
    fileExists: (path: string) => ws.fileExists(path),
    readFile: (path: string) => ws.readFile(path),
  };
}

function fileAccessFromDisk(): FileAccess {
  return {
    fileExists: (path: string) => {
      try {
        return existsSync(path);
      } catch {
        return false;
      }
    },
    readFile: (path: string) => {
      try {
        return readFileSync(path, "utf8");
      } catch {
        return null;
      }
    },
  };
}

interface MacroTypeDep {
  typeName: string;
  importSource: string;
  macroKind: string;
}

interface AnalysisImport {
  source: string;
}

interface AnalysisModuleReference {
  syntax: string; // "StaticImport" | "ExportFrom" | "DynamicImport" | "RequireCall"
  literalSpecifier?: string;
}

interface AnalysisExportSignature {
  name: string;
  isType: boolean;
  reexportSource?: string;
  reexportLocal?: string;
}

interface FileAnalysisSnapshot {
  imports?: AnalysisImport[];
  moduleReferences?: AnalysisModuleReference[];
  macroTypeDeps?: MacroTypeDep[];
  exportSignatures?: AnalysisExportSignature[];
}

function normalizePath(p: string): string {
  return p.replace(/\\/g, "/");
}

function isRelativeImport(specifier: string): boolean {
  return specifier.startsWith(".");
}

/**
 * Parse a bare package specifier into package name + sub-path.
 * e.g. "echarts/types/dist/shared" → { pkgName: "echarts", subPath: "types/dist/shared" }
 * e.g. "@scope/pkg/foo" → { pkgName: "@scope/pkg", subPath: "foo" }
 * e.g. "lodash" → { pkgName: "lodash", subPath: null }
 */
function parseBareSpecifier(specifier: string): { pkgName: string; subPath: string | null } {
  if (specifier.startsWith("@")) {
    // Scoped package: @scope/name/sub/path
    const parts = specifier.split("/");
    if (parts.length < 2) return { pkgName: specifier, subPath: null };
    const pkgName = parts[0] + "/" + parts[1];
    const subPath = parts.length > 2 ? parts.slice(2).join("/") : null;
    return { pkgName, subPath };
  }
  const slashIdx = specifier.indexOf("/");
  if (slashIdx === -1) return { pkgName: specifier, subPath: null };
  return { pkgName: specifier.slice(0, slashIdx), subPath: specifier.slice(slashIdx + 1) };
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
function findPackageDeclarationEntry(fa: FileAccess, runtimeEntry: string): string | null {
  let dir = dirname(runtimeEntry);
  const root = parse(dir).root;

  // Walk up to find package.json
  let pkgJsonPath: string | null = null;
  let pkgDir: string | null = null;
  while (dir !== root) {
    const candidate = join(dir, "package.json");
    if (fa.fileExists(normalizePath(candidate))) {
      pkgJsonPath = candidate;
      pkgDir = dir;
      break;
    }
    dir = dirname(dir);
  }

  if (!pkgJsonPath || !pkgDir) return null;

  let pkg: Record<string, unknown>;
  try {
    const content = fa.readFile(normalizePath(pkgJsonPath));
    if (content === null) return null;
    pkg = JSON.parse(content);
  } catch {
    return null;
  }

  // 1. `types` field
  if (typeof pkg.types === "string") {
    const typesPath = resolve(pkgDir, pkg.types);
    if (fa.fileExists(normalizePath(typesPath))) return normalizePath(typesPath);
  }

  // 2. `typings` field (deprecated alias)
  if (typeof pkg.typings === "string") {
    const typingsPath = resolve(pkgDir, pkg.typings);
    if (fa.fileExists(normalizePath(typingsPath))) return normalizePath(typingsPath);
  }

  // 3. `exports` with `types` condition
  if (pkg.exports && typeof pkg.exports === "object") {
    const typesFromExports = findTypesInExports(pkg.exports as Record<string, unknown>, pkgDir);
    if (typesFromExports) {
      if (fa.fileExists(normalizePath(typesFromExports))) return normalizePath(typesFromExports);
    }
  }

  // 4. Sibling `.d.ts` next to the runtime entry
  const runtimeBase = runtimeEntry.replace(/\.(js|mjs|cjs|jsx)$/, "");
  const siblingDts = runtimeBase + ".d.ts";
  if (fa.fileExists(normalizePath(siblingDts))) return normalizePath(siblingDts);

  const siblingDmts = runtimeBase + ".d.mts";
  if (fa.fileExists(normalizePath(siblingDmts))) return normalizePath(siblingDmts);

  // 5. Package-root `index.d.ts`
  const rootDts = join(pkgDir, "index.d.ts");
  if (fa.fileExists(normalizePath(rootDts))) return normalizePath(rootDts);

  return null;
}

/**
 * Recursively search `exports` for a `types` condition.
 * Handles nested condition objects like `{ ".": { "types": "./dist/index.d.ts" } }`.
 */
function findTypesInExports(exports: Record<string, unknown>, pkgDir: string): string | null {
  // Direct `types` condition
  if (typeof exports.types === "string") {
    return resolve(pkgDir, exports.types);
  }

  // Check "." entry
  const rootExport = exports["."];
  if (rootExport && typeof rootExport === "object") {
    const root = rootExport as Record<string, unknown>;
    if (typeof root.types === "string") {
      return resolve(pkgDir, root.types);
    }
    // Check nested conditions (e.g. { ".": { "import": { "types": "..." } } })
    for (const key of ["import", "require", "default"]) {
      const cond = root[key];
      if (cond && typeof cond === "object") {
        const condObj = cond as Record<string, unknown>;
        if (typeof condObj.types === "string") {
          return resolve(pkgDir, condObj.types);
        }
      }
    }
  }

  return null;
}

async function hydrateRelativeDependencyClosure(
  host: VerterHost,
  entryFile: string,
  fa: FileAccess,
  visited: Set<string>,
): Promise<void> {
  const normalizedEntry = normalizePath(entryFile);
  if (visited.has(normalizedEntry)) return;

  const queue = [normalizedEntry];
  visited.add(normalizedEntry);

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

    // Export signatures are more precise than moduleReferences: they know exactly
    // which names are re-exported and from where.
    const importSources = (depAnalysis.imports ?? []).map((imp) => imp.source);
    const reexportSources = (depAnalysis.exportSignatures ?? [])
      .filter((sig) => sig.reexportSource)
      .map((sig) => sig.reexportSource!);
    const allSources = [...new Set([...importSources, ...reexportSources])];

    for (const source of allSources) {
      if (!isRelativeImport(source)) continue;

      const absBase = resolve(dirname(currentFile), source);
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
            specifier: source,
            resolvedCanonicalId: normalized,
          });
          break;
        }
        if (!fa.fileExists(normalized)) continue;

        // .vue files at the end of a barrel chain need their types available
        // for cross-file resolution. Upsert as SFC (not non_sfc) so script
        // analysis extracts exported types. The SFC's own transform will
        // re-upsert with the full source later, which is a no-op if unchanged.
        // Do NOT traverse further (SFC internal imports are handled separately).
        if (normalized.endsWith(".vue")) {
          visited.add(normalized);
          try {
            const vueSrc = fa.readFile(normalized);
            if (vueSrc !== null) {
              host.upsert({ inputId: normalized, source: vueSrc });
            }
          } catch {
            // If read fails, just record the resolution without upserting
          }
          depResolutions.push({
            specifier: source,
            resolvedCanonicalId: normalized,
          });
          break;
        }

        visited.add(normalized);
        try {
          const depSource = fa.readFile(normalized);
          if (depSource !== null) {
            host.upsert({
              inputId: normalized,
              source: depSource,
              fileKind: "non_sfc",
            });
            depResolutions.push({
              specifier: source,
              resolvedCanonicalId: normalized,
            });
            queue.push(normalized);
          }
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
  ws?: Workspace,
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

  // Create file access from workspace or fall back to disk
  const fa: FileAccess = ws ? fileAccessFromWorkspace(ws) : fileAccessFromDisk();

  const hydratedRelativeVisited = new Set<string>();

  // Collect unique specifiers from macro type deps.
  const specifiers = [...new Set(analysis.macroTypeDeps.map((dep) => dep.importSource))];

  const resolutions: HostDependencyResolution[] = [];

  // Phase 1: Handle specifiers that resolve to .vue files.
  // resolveUpsertDependencies records .vue resolutions but does NOT upsert the
  // source — the assumption is the bundler will process the .vue file later.
  // But macro type resolution needs the source NOW. Upsert .vue deps eagerly.
  const remaining: string[] = [];
  for (const specifier of specifiers) {
    let resolvedPath: string | null = null;

    // Try bundler resolve hook
    if (resolveId) {
      const result = resolvedIdFromHookResult(
        await resolveId(specifier, filename, { skipSelf: true }),
      );
      if (result) resolvedPath = result;
    }

    // For relative specifiers without a resolve hook, try file probing
    if (!resolvedPath && isRelativeImport(specifier)) {
      const absBase = resolve(dirname(filename), specifier);
      const candidates = [absBase, absBase + ".vue", absBase + "/index.vue"];
      for (const candidate of candidates) {
        if (fa.fileExists(normalizePath(candidate))) {
          resolvedPath = candidate;
          break;
        }
      }
    }

    if (resolvedPath) {
      const normalized = normalizePath(resolvedPath);
      if (normalized.endsWith(".vue")) {
        // Upsert the .vue file as SFC so the host can read its exported types
        try {
          const vueSrc = fa.readFile(normalized);
          if (vueSrc !== null) {
            host.upsert({ inputId: normalized, source: vueSrc });
          }
        } catch {
          // Read failed — skip
        }
        resolutions.push({ specifier, resolvedCanonicalId: normalized });
        continue;
      }
      // Non-.vue resolved file (.ts, .d.ts, etc.) — only handle relative specifiers.
      // Bare specifiers (npm packages) should go to Phase 2 which properly finds
      // the .d.ts declaration entry via package.json, not the .js runtime entry.
      if (isRelativeImport(specifier)) {
        // If the resolved file is a JS runtime file (.js, .mjs, .cjs), check for a
        // companion .d.ts/.d.mts/.d.cts file that has the actual type declarations.
        // This happens in pre-built workspace packages (dist/) where types.mjs has
        // no type info but types.d.ts has the full declarations.
        let effectivePath = resolvedPath;
        let effectiveNormalized = normalized;
        const jsExtMatch = resolvedPath.match(/\.(js|mjs|cjs)$/);
        if (jsExtMatch) {
          const base = resolvedPath.slice(0, -jsExtMatch[0].length);
          const dtsExts = [".d.ts", ".d.mts", ".d.cts"];
          for (const ext of dtsExts) {
            const candidate = base + ext;
            if (fa.fileExists(normalizePath(candidate))) {
              effectivePath = candidate;
              effectiveNormalized = normalizePath(candidate);
              break;
            }
          }
        }
        try {
          const depSrc = fa.readFile(normalizePath(effectivePath));
          if (depSrc !== null) {
            host.upsert({ inputId: effectiveNormalized, source: depSrc, fileKind: "non_sfc" });
          }
        } catch {
          // Read failed — skip
        }
        resolutions.push({ specifier, resolvedCanonicalId: effectiveNormalized });
        await hydrateRelativeDependencyClosure(
          host,
          effectiveNormalized,
          fa,
          hydratedRelativeVisited,
        );
        continue;
      }
    }

    // Relative specifier with no resolve hook hit — try file probing
    if (isRelativeImport(specifier)) {
      const absBase = resolve(dirname(filename), specifier);
      const probeCandidates = [
        absBase + ".ts",
        absBase + ".d.ts",
        absBase + ".tsx",
        absBase + "/index.ts",
        absBase + "/index.d.ts",
        absBase, // exact path
      ];
      let found = false;
      for (const candidate of probeCandidates) {
        if (fa.fileExists(normalizePath(candidate))) {
          const normalized = normalizePath(candidate);
          try {
            const depSrc = fa.readFile(normalized);
            if (depSrc !== null) {
              host.upsert({ inputId: normalized, source: depSrc, fileKind: "non_sfc" });
            }
          } catch {
            // skip
          }
          resolutions.push({ specifier, resolvedCanonicalId: normalized });
          await hydrateRelativeDependencyClosure(host, normalized, fa, hydratedRelativeVisited);
          found = true;
          break;
        }
      }
      if (found) continue;
    }

    // Not resolved — collect for Phase 2 (bare package specifier handling)
    if (!isRelativeImport(specifier)) {
      remaining.push(specifier);
    }
  }

  // Phase 2: Bare (non-relative) specifiers pointing to npm packages.
  const bareSpecifiers = remaining;

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
        // Runtime resolution failed — may be a types-only sub-path (e.g. "echarts/types/dist/shared").
        // Try to find the package directory and resolve the sub-path as .d.ts directly.
      }
    }

    let entryPath: string | null = null;

    if (runtimeEntry) {
      // Find the declaration entry from package.json, or fall back to the
      // runtime entry itself if it's a .ts/.d.ts file (local project files).
      entryPath = findPackageDeclarationEntry(fa, runtimeEntry);
      if (!entryPath && (runtimeEntry.endsWith(".ts") || runtimeEntry.endsWith(".d.ts"))) {
        entryPath = runtimeEntry;
      }
    }

    // Fallback: for sub-path specifiers where runtime resolution failed,
    // find the package directory and probe for .d.ts files at the sub-path.
    if (!entryPath) {
      const { pkgName, subPath } = parseBareSpecifier(specifier);
      if (subPath) {
        let pkgDir: string | null = null;
        try {
          const { createRequire } = await import("module");
          const req = createRequire(filename);
          const pkgJsonPath = req.resolve(pkgName + "/package.json");
          pkgDir = dirname(pkgJsonPath);
        } catch {
          // Try walking node_modules manually
          let dir = dirname(filename);
          const root = parse(dir).root;
          while (dir !== root) {
            const candidate = join(dir, "node_modules", pkgName);
            if (fa.fileExists(normalizePath(join(candidate, "package.json")))) {
              pkgDir = candidate;
              break;
            }
            dir = dirname(dir);
          }
        }

        if (pkgDir) {
          const absBase = join(pkgDir, subPath);
          const candidates = [
            absBase + ".d.ts",
            absBase + ".d.mts",
            absBase + "/index.d.ts",
            absBase + "/index.d.mts",
            absBase + ".ts",
            absBase,
          ];
          for (const candidate of candidates) {
            if (fa.fileExists(normalizePath(candidate))) {
              entryPath = normalizePath(candidate);
              break;
            }
          }
        }
      }
    }

    if (!entryPath) continue;

    // Upsert the entry file
    const normalizedEntryPath = normalizePath(entryPath);

    try {
      const source = fa.readFile(normalizedEntryPath);
      if (source !== null) {
        host.upsert({
          inputId: normalizedEntryPath,
          source,
          fileKind: "non_sfc",
        });
        resolutions.push({
          specifier,
          resolvedCanonicalId: normalizedEntryPath,
        });
      } else {
        continue;
      }
    } catch {
      continue;
    }

    await hydrateRelativeDependencyClosure(host, normalizedEntryPath, fa, hydratedRelativeVisited);
  }

  // Update the entry file's import dependencies to include the hydrated resolutions
  if (resolutions.length > 0) {
    host.setImportDependencies(filename, resolutions);
  }
}
