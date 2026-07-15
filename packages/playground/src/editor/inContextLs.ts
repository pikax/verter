/**
 * The in-context `ts.LanguageService` over Verter carriers — the REAL
 * TypeScript JS LanguageService running against the in-memory carrier store
 * (never Verter's internal semantic resolver).
 *
 * Shared by the browser worker (bundled TS6 engine + virtual `node_modules`)
 * and the hermetic guards (disk-backed `ts.sys` fallback): ONE host
 * implementation, the `ts` module INJECTED (no singleton import), every
 * carrier rule sourced from the shared CORE:
 *
 * - roots       — user files + ready IDE companions (`selfDiagnosticRoot`);
 *                 declaration carriers are import-driven, API carriers
 *                 redirect-reached (CORE root-membership policy);
 * - resolution  — a bare `./X.vue` / `./X.svelte` import routes to the CORE
 *                 extension-MIDDLE declaration carrier (`X.d.vue.ts`) and
 *                 FAILS CLOSED when it is not published (no fallthrough to
 *                 `.tsx` / `.verter.ts`); everything else uses TypeScript's
 *                 own resolution;
 * - script kind — the CORE carrier script-kind policy with the injected `ts`;
 * - options     — parity with the Rust host's synthesized compiler options.
 */
import type * as tsNs from "typescript";
import {
  normalizePath,
  scriptKindForCarrier,
  toDeclarationCarrierFileName,
  type CarrierStoreReader,
} from "@verter/language-shared";

/** A mutable user (non-generated) program member. */
export interface UserFileEntry {
  content: string;
  version: number;
}

/** The layered fallback file system (disk `ts.sys` in tests; virtual in the worker). */
export interface FallbackFs {
  fileExists(path: string): boolean;
  readFile(path: string): string | undefined;
  directoryExists?(path: string): boolean;
  getDirectories?(path: string): string[];
  realpath?(path: string): string;
  useCaseSensitiveFileNames?: boolean;
  getDefaultLibFileName?(options: tsNs.CompilerOptions): string;
}

export interface InContextLsOptions {
  /** The INJECTED TypeScript module (the pinned bundled engine). */
  ts: typeof tsNs;
  /** The published carrier surfaces (the shared CORE reader contract). */
  store: CarrierStoreReader;
  /** User program members (plain `.ts` files, scratch files), by absolute path. */
  userFiles: Map<string, UserFileEntry>;
  currentDirectory: string;
  fallbackFs?: FallbackFs;
  /** Virtual lib directory for the worker (`/node_modules/typescript/lib`). */
  defaultLibDir?: string;
}

export interface InContextLs {
  languageService: tsNs.LanguageService;
  host: tsNs.LanguageServiceHost;
  compilerOptions: tsNs.CompilerOptions;
}

/** `"6.0.3"` / `"7.0.1-rc"` → the numeric major. `NaN`-safe (returns 0). */
export function tsMajorOf(version: string): number {
  const major = Number.parseInt(version.split(".")[0] ?? "", 10);
  return Number.isNaN(major) ? 0 : major;
}

/**
 * The fail-closed WASM surface capability gate: the in-context JS
 * LanguageService serves TS majors BELOW 7, and the external-tsgo engine is
 * NEVER available in the WASM/browser surface (`tsgo: false` for every
 * major — there is no WASM build of tsgo and Verter does not attempt one).
 * For TS>=7 the playground surface offers carrier generation + Verter-native
 * diagnostics ONLY — no external-TS path is produced.
 */
export function capabilityForWasm(tsMajor: number): { inContextLS: boolean; tsgo: false } {
  return { inContextLS: tsMajor < 7, tsgo: false };
}

/**
 * Capability-gated construction: returns `null` WITHOUT touching the engine
 * (no `ts.createLanguageService`, no host construction) when the injected
 * engine's major is gated off — the produce path is never invoked closed.
 */
export function createGatedInContextLanguageService(
  options: InContextLsOptions,
): InContextLs | null {
  if (!capabilityForWasm(tsMajorOf(options.ts.version)).inContextLS) {
    return null;
  }
  return createInContextLanguageService(options);
}

/**
 * Compiler-option parity with the Rust host's synthesized options
 * (`verter_lsp::extension_provider::configure_paths`), plus the local-service
 * necessities (`noEmit`, `skipLibCheck`).
 */
export function synthesizedCompilerOptions(ts: typeof tsNs): tsNs.CompilerOptions {
  return {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    jsx: ts.JsxEmit.Preserve,
    jsxImportSource: "vue",
    allowImportingTsExtensions: true,
    allowJs: true,
    checkJs: true,
    strict: true,
    allowArbitraryExtensions: true,
    noEmit: true,
    skipLibCheck: true,
  };
}

/** A `ts.sys`-backed fallback (the hermetic guard configuration). */
export function fallbackFromSys(ts: typeof tsNs): FallbackFs {
  const sys = ts.sys;
  return {
    fileExists: (path) => sys.fileExists(path),
    readFile: (path) => sys.readFile(path),
    directoryExists: (path) => sys.directoryExists(path),
    getDirectories: (path) => sys.getDirectories(path),
    realpath: sys.realpath?.bind(sys),
    useCaseSensitiveFileNames: sys.useCaseSensitiveFileNames,
    getDefaultLibFileName: (options) => ts.getDefaultLibFilePath(options),
  };
}

/** Build the carrier-serving host + REAL `ts.createLanguageService` over it. */
export function createInContextLanguageService(options: InContextLsOptions): InContextLs {
  const { ts, store, userFiles, fallbackFs } = options;
  const compilerOptions = synthesizedCompilerOptions(ts);
  const currentDirectory = normalizePath(options.currentDirectory);

  const carrierContent = (fileName: string): string | undefined => {
    const ready = store.readyFile(fileName);
    if (ready === undefined) return undefined;
    return store.readBlobSync(ready.blob_rel, fileName);
  };

  const readLayered = (fileName: string): string | undefined => {
    const normalized = normalizePath(fileName);
    const user = userFiles.get(normalized);
    if (user !== undefined) return user.content;
    const carrier = carrierContent(normalized);
    if (carrier !== undefined) return carrier;
    return fallbackFs?.readFile(fileName);
  };

  const existsLayered = (fileName: string): boolean => {
    const normalized = normalizePath(fileName);
    if (userFiles.has(normalized)) return true;
    if (store.readyFile(normalized) !== undefined) return true;
    return fallbackFs?.fileExists(fileName) ?? false;
  };

  const virtualDirectoryExists = (directory: string): boolean => {
    const prefix = `${normalizePath(directory).replace(/\/+$/, "")}/`;
    for (const path of userFiles.keys()) {
      if (path.startsWith(prefix)) return true;
    }
    for (const owned of store.ownedSources()) {
      if (owned.provider_uri.startsWith(prefix)) return true;
    }
    return false;
  };

  const resolutionHost: tsNs.ModuleResolutionHost = {
    fileExists: existsLayered,
    readFile: readLayered,
    directoryExists: (directory) =>
      virtualDirectoryExists(directory) || (fallbackFs?.directoryExists?.(directory) ?? false),
    getDirectories: (directory) => fallbackFs?.getDirectories?.(directory) ?? [],
    realpath: fallbackFs?.realpath,
    getCurrentDirectory: () => currentDirectory,
    useCaseSensitiveFileNames: fallbackFs?.useCaseSensitiveFileNames ?? true,
  };

  /**
   * The fail-closed bare-carrier redirect: a RELATIVE `./X.vue` / `./X.svelte`
   * import resolves to the published extension-MIDDLE declaration carrier or
   * NOT AT ALL. `null` = not a bare-carrier import (use default resolution).
   */
  const resolveBareCarrierImport = (
    specifier: string,
    containingFile: string,
  ): tsNs.ResolvedModuleFull | undefined | null => {
    if (!specifier.startsWith("./") && !specifier.startsWith("../")) {
      return null;
    }
    if (toDeclarationCarrierFileName(specifier) === null) {
      return null;
    }
    const baseDir = posixDirname(normalizePath(containingFile));
    const absolute = posixResolveFrom(baseDir, specifier);
    const declPath = toDeclarationCarrierFileName(absolute);
    if (declPath !== null && store.readyFile(declPath) !== undefined) {
      return {
        resolvedFileName: declPath,
        extension: ts.Extension.Dts,
        isExternalLibraryImport: false,
      };
    }
    // Fail closed: the declaration carrier is not published — the import
    // stays unresolved rather than falling through to `.tsx`/`.verter.ts`.
    return undefined;
  };

  const host: tsNs.LanguageServiceHost & {
    /**
     * Consulted by the service's program-reuse check (`synchronizeHostData`)
     * at runtime even though TypeScript 6 dropped it from the public
     * `LanguageServiceHost` type — declared explicitly so the intent
     * typechecks against the pinned engine.
     */
    hasInvalidatedResolutions?: (path: string) => boolean;
  } = {
    getCompilationSettings: () => compilerOptions,
    getScriptFileNames: () => [...userFiles.keys(), ...store.readyIdeCompanions()],
    getScriptVersion: (fileName) => {
      const normalized = normalizePath(fileName);
      const user = userFiles.get(normalized);
      if (user !== undefined) return `u${user.version}`;
      const ready = store.readyFile(normalized);
      if (ready !== undefined) return `c${ready.version}`;
      return "0";
    },
    getScriptSnapshot: (fileName) => {
      const content = readLayered(fileName);
      return content === undefined ? undefined : ts.ScriptSnapshot.fromString(content);
    },
    getScriptKind: (fileName) =>
      scriptKindForCarrier(fileName, ts) ?? inferScriptKind(ts, fileName),
    getCurrentDirectory: () => currentDirectory,
    getDefaultLibFileName: (opts) =>
      fallbackFs?.getDefaultLibFileName?.(opts) ??
      `${options.defaultLibDir ?? "/lib"}/${ts.getDefaultLibFileName(opts)}`,
    fileExists: existsLayered,
    readFile: readLayered,
    directoryExists: resolutionHost.directoryExists,
    getDirectories: resolutionHost.getDirectories,
    realpath: fallbackFs?.realpath,
    useCaseSensitiveFileNames: () => fallbackFs?.useCaseSensitiveFileNames ?? true,
    // The store epoch moves under the service (carrier publishes/removals);
    // always re-resolving keeps resolution truth-tracking the store.
    hasInvalidatedResolutions: () => true,
    resolveModuleNameLiterals: (moduleLiterals, containingFile) =>
      moduleLiterals.map((literal): tsNs.ResolvedModuleWithFailedLookupLocations => {
        const redirected = resolveBareCarrierImport(literal.text, containingFile);
        if (redirected !== null) {
          return { resolvedModule: redirected };
        }
        return ts.resolveModuleName(literal.text, containingFile, compilerOptions, resolutionHost);
      }),
  };

  const languageService = ts.createLanguageService(host);
  return { languageService, host, compilerOptions };
}

/** Non-carrier script-kind inference by trailing extension. */
function inferScriptKind(ts: typeof tsNs, fileName: string): tsNs.ScriptKind {
  const normalized = normalizePath(fileName);
  if (normalized.endsWith(".tsx")) return ts.ScriptKind.TSX;
  if (normalized.endsWith(".ts")) return ts.ScriptKind.TS;
  if (normalized.endsWith(".jsx")) return ts.ScriptKind.JSX;
  if (normalized.endsWith(".js")) return ts.ScriptKind.JS;
  if (normalized.endsWith(".json")) return ts.ScriptKind.JSON;
  return ts.ScriptKind.Unknown;
}

/** POSIX dirname over a forward-slash-normalised path. */
function posixDirname(path: string): string {
  const idx = path.lastIndexOf("/");
  if (idx === -1) return ".";
  if (idx === 0) return "/";
  return path.slice(0, idx);
}

/** Pure-POSIX relative resolution (browser-safe; no Node `path`). */
function posixResolveFrom(baseDir: string, relativePath: string): string {
  const joined = relativePath.startsWith("/") ? relativePath : `${baseDir}/${relativePath}`;
  const absolute = joined.startsWith("/");
  const segments: string[] = [];
  for (const segment of joined.split("/")) {
    if (segment === "" || segment === ".") continue;
    if (segment === "..") {
      if (segments.length > 0 && segments[segments.length - 1] !== "..") {
        segments.pop();
      } else if (!absolute) {
        segments.push("..");
      }
      continue;
    }
    segments.push(segment);
  }
  return (absolute ? "/" : "") + segments.join("/");
}
