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
 * Package-backed classification for a RESOLVED filesystem path: true when a
 * `node_modules` path SEGMENT is present. Segment-exact (never a substring
 * match on other names). TS-side stand-in for the workspace ownership
 * oracle (`workspace_is_package_backed`) until the native workspace exposes
 * it to the plugin layer.
 */
function isPackageBackedPath(normalized: string): boolean {
  return normalized.split("/").includes("node_modules");
}

/**
 * Per-host memo of files already hydrated + closure-walked. Keeps the
 * transform hook demand-driven: each dependency file is read, upserted, and
 * traversed ONCE per host generation instead of once per importing SFC
 * transform. [`evictHydratedPath`] drops a single entry when the watcher
 * reports a change so the next transform re-hydrates fresh content.
 */
const hydratedClosureByHost = new WeakMap<object, Set<string>>();

function hydratedSetFor(host: VerterHost): Set<string> {
  let set = hydratedClosureByHost.get(host as unknown as object);
  if (!set) {
    set = new Set();
    hydratedClosureByHost.set(host as unknown as object, set);
  }
  return set;
}

/**
 * React to a watcher change/delete of `path`. When the file is part of the
 * host's hydrated graph, the WHOLE memo clears so the next transform
 * re-walks and re-reads fresh content — a changed leaf is only reachable
 * through its importers, so per-entry eviction could never re-reach it.
 * Changes to files outside the hydrated graph leave the memo untouched.
 */
export function evictHydratedPath(host: VerterHost, path: string): void {
  const set = hydratedClosureByHost.get(host as unknown as object);
  if (set?.has(normalizePath(path))) {
    set.clear();
  }
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

/**
 * Resolve a type-import specifier to an absolute filesystem path.
 *
 * Order:
 * 1. Bundler `resolveId` (path aliases like `@/…`, package subpaths)
 * 2. Relative filesystem probing (`.d.ts` / `.ts` / `.vue` / index)
 *
 * Returns null when the specifier cannot be resolved to a local file.
 */
async function resolveTypeImportPath(
  source: string,
  importer: string,
  fa: FileAccess,
  resolveId?: ResolveHook,
): Promise<string | null> {
  if (resolveId) {
    const result = resolvedIdFromHookResult(await resolveId(source, importer, { skipSelf: true }));
    if (result) {
      const normalizedResult = normalizePath(result);
      // Closure traversal never walks into packages: a non-relative
      // specifier resolving into `node_modules` is a BARE package import.
      // Package declaration entries are hydrated exactly once by the
      // entry-level bare-specifier phase of `hydrateMacroTypeDeps` —
      // re-walking them per transform is the eager-crawl anti-pattern.
      // Aliases (`@/…`) resolving to project-local files stay in scope.
      if (!isRelativeImport(source) && isPackageBackedPath(normalizedResult)) {
        return null;
      }
      // Prefer companion .d.ts next to a JS runtime entry (pre-built packages).
      const jsExtMatch = result.match(/\.(js|mjs|cjs)$/);
      if (jsExtMatch) {
        const base = result.slice(0, -jsExtMatch[0].length);
        for (const ext of [".d.ts", ".d.mts", ".d.cts"]) {
          const candidate = base + ext;
          if (fa.fileExists(normalizePath(candidate))) {
            return normalizePath(candidate);
          }
        }
      }
      return normalizedResult;
    }
  }

  if (!isRelativeImport(source)) return null;

  const absBase = resolve(dirname(importer), source);
  const candidates = [
    absBase + ".d.ts",
    absBase + ".ts",
    absBase + ".tsx",
    absBase + ".vue",
    absBase + "/index.d.ts",
    absBase + "/index.ts",
    absBase + "/index.vue",
    absBase,
  ];
  for (const candidate of candidates) {
    const normalized = normalizePath(candidate);
    if (fa.fileExists(normalized)) return normalized;
  }
  return null;
}

/**
 * Upsert a resolved type dependency and return whether it should be enqueued
 * for further import-graph traversal.
 */
function upsertResolvedTypeFile(host: VerterHost, fa: FileAccess, resolvedPath: string): boolean {
  const normalized = normalizePath(resolvedPath);
  try {
    const src = fa.readFile(normalized);
    if (src === null) return false;
    if (normalized.endsWith(".vue")) {
      // SFC so script analysis extracts exported types / heritage imports.
      host.upsert({ inputId: normalized, source: src });
      return true;
    }
    host.upsert({
      inputId: normalized,
      source: src,
      fileKind: "non_sfc",
    });
    return true;
  } catch {
    return false;
  }
}

/**
 * Walk the type-import graph starting at `entryFile`.
 *
 * Handles both relative imports and bundler-resolved aliases (`@/…`) so that
 * intermediate `.vue` type deps (reka-ui / radix-vue pattern) load nested
 * heritage types such as `PrimitiveProps` from `@/Primitive`.
 *
 * Bare npm packages that only resolve via package.json `types` are handled
 * by the entry-level bare-specifier phase of `hydrateMacroTypeDeps`, not here.
 */
async function hydrateDependencyClosure(
  host: VerterHost,
  entryFile: string,
  fa: FileAccess,
  visited: Set<string>,
  resolveId?: ResolveHook,
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

    // Imports + reexports + macro type deps all contribute to type resolution.
    const importSources = (depAnalysis.imports ?? []).map((imp) => imp.source);
    const reexportSources = (depAnalysis.exportSignatures ?? [])
      .filter((sig) => sig.reexportSource)
      .map((sig) => sig.reexportSource!);
    const macroSources = (depAnalysis.macroTypeDeps ?? []).map((dep) => dep.importSource);
    const allSources = [...new Set([...importSources, ...reexportSources, ...macroSources])];

    for (const source of allSources) {
      const resolved = await resolveTypeImportPath(source, currentFile, fa, resolveId);
      if (!resolved) continue;

      const normalized = normalizePath(resolved);
      if (visited.has(normalized)) {
        depResolutions.push({
          specifier: source,
          resolvedCanonicalId: normalized,
        });
        continue;
      }

      if (!fa.fileExists(normalized)) continue;

      if (!upsertResolvedTypeFile(host, fa, normalized)) {
        // Record resolution even if upsert failed so the host has a route.
        depResolutions.push({
          specifier: source,
          resolvedCanonicalId: normalized,
        });
        continue;
      }

      visited.add(normalized);
      depResolutions.push({
        specifier: source,
        resolvedCanonicalId: normalized,
      });
      // Traverse further: intermediate .vue files may import heritage via @/
      // and .ts/.d.ts files may re-export or import more type modules.
      queue.push(normalized);
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
 *
 * Intermediate `.vue` type deps (and their nested `@/` / relative type imports)
 * are walked via `hydrateDependencyClosure` so heritage types like
 * `PrimitiveProps` from `@/Primitive` load before runtime props emission.
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

  // Demand gate: hydration exists FOR macro type resolution. An SFC whose
  // analysis demands no macro type deps hydrates nothing — the tiered
  // macro-dep analysis already includes LOCAL declaration heritage
  // (`interface SeparatorProps extends ImportedBase` under
  // `defineProps<SeparatorProps>()` records the heritage import as a
  // SURFACE dep), so macroTypeDeps is the complete demand set and
  // broadening to ALL imports/re-exports would turn every transform into
  // an eager dependency-graph crawl.
  if (!analysis.macroTypeDeps?.length) return;

  // Create file access from workspace or fall back to disk
  const fa: FileAccess = ws ? fileAccessFromWorkspace(ws) : fileAccessFromDisk();

  // Per-host memo: dependency files already hydrated + walked stay walked
  // across transforms (watcher eviction re-opens single entries).
  const hydratedVisited = hydratedSetFor(host);

  const specifiers = [...new Set(analysis.macroTypeDeps.map((dep) => dep.importSource))];

  const resolutions: HostDependencyResolution[] = [];

  // Phase 1: Handle specifiers that resolve to .vue files.
  // resolveUpsertDependencies records .vue resolutions but does NOT upsert the
  // source — the assumption is the bundler will process the .vue file later.
  // But macro type resolution needs the source NOW. Upsert .vue deps eagerly
  // and walk their nested type-import graph (including path aliases).
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
        // Per-host memo: an already-hydrated dep only records its
        // resolution for THIS entry file.
        if (hydratedVisited.has(normalized)) {
          resolutions.push({ specifier, resolvedCanonicalId: normalized });
          continue;
        }
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
        // Nested heritage: intermediate SFC may import types via @/ aliases.
        await hydrateDependencyClosure(host, normalized, fa, hydratedVisited, resolveId);
        continue;
      }
      // Non-.vue resolved file (.ts, .d.ts, etc.) — only handle relative specifiers.
      // Bare specifiers (npm packages) should go to the bare-package step
      // below, which properly finds the .d.ts declaration entry via
      // package.json, not the .js runtime entry.
      // Exception: path aliases (`@/…`) that resolve to project-local .ts files
      // are also hydrated here (same as relative), since they are not packages.
      const isProjectLocal =
        isRelativeImport(specifier) ||
        (resolvedPath !== null &&
          !isPackageBackedPath(normalized) &&
          (normalized.endsWith(".ts") ||
            normalized.endsWith(".tsx") ||
            normalized.endsWith(".d.ts") ||
            normalized.endsWith(".d.mts") ||
            normalized.endsWith(".d.cts") ||
            normalized.endsWith(".mts") ||
            normalized.endsWith(".cts")));
      if (isProjectLocal) {
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
        // Per-host memo: an already-hydrated dep only records its
        // resolution for THIS entry file.
        if (hydratedVisited.has(effectiveNormalized)) {
          resolutions.push({ specifier, resolvedCanonicalId: effectiveNormalized });
          continue;
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
        await hydrateDependencyClosure(host, effectiveNormalized, fa, hydratedVisited, resolveId);
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
          await hydrateDependencyClosure(host, normalized, fa, hydratedVisited, resolveId);
          found = true;
          break;
        }
      }
      if (found) continue;
    }

    // Not resolved — collect for the bare-package specifier handling step
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
      // Path-alias / project-local resolution that landed here as "bare":
      // e.g. `@/components/base` resolved by the bundler to a workspace .ts file.
      // Prefer that over package.json walk when the path is outside node_modules.
      if (
        !entryPath &&
        runtimeEntry &&
        !isPackageBackedPath(normalizePath(runtimeEntry)) &&
        fa.fileExists(normalizePath(runtimeEntry))
      ) {
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
    const isVueEntry = normalizedEntryPath.endsWith(".vue");

    // Per-host memo: an already-hydrated package declaration entry only
    // records its resolution for THIS entry file.
    if (hydratedVisited.has(normalizedEntryPath)) {
      resolutions.push({ specifier, resolvedCanonicalId: normalizedEntryPath });
      continue;
    }

    try {
      const source = fa.readFile(normalizedEntryPath);
      if (source !== null) {
        if (isVueEntry) {
          host.upsert({ inputId: normalizedEntryPath, source });
        } else {
          host.upsert({
            inputId: normalizedEntryPath,
            source,
            fileKind: "non_sfc",
          });
        }
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

    await hydrateDependencyClosure(host, normalizedEntryPath, fa, hydratedVisited, resolveId);
  }

  // Update the entry file's import dependencies to include the hydrated resolutions
  if (resolutions.length > 0) {
    host.setImportDependencies(filename, resolutions);
  }
}
