/// <reference types="node" />

// =============================================================================
// Standalone CSS Style Processing (for preprocessed CSS from Vite plugin)
// =============================================================================

/**
 * Options for processing a CSS style block
 */
export interface ProcessStyleOptions {
  /**
   * Scope ID string (e.g., "a4f2eed6")
   */
  scopeId: string;
  /**
   * Whether this style block is scoped
   */
  scoped?: boolean;
  /**
   * Whether this is a CSS module block
   */
  isModule?: boolean;
  /**
   * Custom module name (None = "$style")
   */
  moduleName?: string;
  /**
   * Source filename for source map generation
   */
  filename?: string;
  /**
   * Whether to generate source maps
   */
  sourcemap?: boolean;
}

/**
 * A v-bind() expression that was replaced with a CSS variable
 */
export interface ProcessStyleVBind {
  /**
   * The original expression text (e.g., "color" or "theme.color")
   */
  expression: string;
  /**
   * The generated CSS variable name (e.g., "--a4f2eed6-color")
   */
  varName: string;
}

/**
 * Result of processing a CSS style block
 */
export interface ProcessStyleResult {
  /**
   * Transformed CSS code
   */
  code: string;
  /**
   * Source map as JSON string (if sourcemap was requested)
   */
  sourceMap?: string;
  /**
   * CSS module class mappings (each entry is [original, hashed])
   */
  moduleClasses: [string, string][];
  /**
   * Resolved CSS module name (e.g., "$style" or a custom name)
   */
  moduleName?: string;
  /**
   * v-bind() expressions found and replaced
   */
  vBindVars: ProcessStyleVBind[];
}

/**
 * Process a CSS style block: apply scoping, CSS modules, and v-bind replacement.
 *
 * Called by the Vite plugin after preprocessing SCSS/Less/Stylus to valid CSS.
 * For plain CSS blocks, the Rust compiler handles this inline during compilation.
 *
 * @param css - Valid CSS as a string or Buffer (UTF-8 bytes).
 * @param options - Processing options (scope ID, scoped, modules, etc.)
 * @returns Processed CSS with scoping/modules applied, plus v-bind metadata
 */
export declare function processStyle(
  css: string | Buffer,
  options: ProcessStyleOptions,
): ProcessStyleResult;

// =============================================================================
// Batch Compilation (Rayon parallel)
// =============================================================================

export interface BatchFile {
  filename: string;
  source: string;
}

export interface BatchOptions {
  /** Number of Rayon threads (0 or undefined = all logical CPUs) */
  threads?: number;
}

export interface BatchResult {
  filename: string;
  /** Combined script + template code */
  code: string;
  /** First error message if compilation failed */
  error?: string;
  durationMs: number;
}

/**
 * Compile a batch of Vue SFC files in parallel using Rayon.
 *
 * Each file is compiled independently with its own allocator — no shared
 * mutable state. No caching, no analysis — compile-only for maximum throughput.
 *
 * Equivalent to Vize's `compileSfcBatch` for fair benchmark comparison.
 */
export declare function compileBatch(files: BatchFile[], options?: BatchOptions): BatchResult[];

// =============================================================================
// VerterHost (in-memory virtual file host)
//
// Shared types re-exported from host-types.ts. Native-specific overrides
// (Buffer support) for HostUpsertRequest are defined below.
// HostStyleOverrideEntry and HostStyleOverrideRequest are kept for
// compatibility with @verter/wasm and host-types re-exports.
// =============================================================================

export type {
  HostConfig,
  HostCompileProfile,
  HostIdeResponse,
  HostVirtualNodeKind,
  HostSliceChanges,
  HostDiagnostic,
  HostDiagnosticsSnapshot,
  HostExternalSourceRequest,
  HostScriptImportInfo,
  HostModuleReference,
  HostPreprocessorRequest,
  HostBlockOverrideEntry,
  HostBlockOverrideRequest,
  HostExportSignature,
  HostResolvedExport,
  HostUpdateResult,
  HostResolvedId,
  HostVirtualMeta,
  HostVirtualFileResponse,
  HostVirtualQuery,
  HostRemoveResult,
  HostTextEdit,
  HostCodeAction,
  HostLintRuleMetadata,
  HostLintDiagnostic,
  HostDocumentSymbol,
  HostElementMatch,
  HostSelectorMatchResult,
  HostDependencyResolution,
  HostIdeProjectConfig,
} from "./host-types";

import type { HostCompileProfile } from "./host-types";

// ---------------------------------------------------------------------------
// Native-specific overrides: accept Buffer in addition to string
// ---------------------------------------------------------------------------

export interface HostUpsertRequest {
  canonicalId?: string;
  inputId: string;
  /** SFC source code. Accepts a string or a Buffer (UTF-8 bytes from `fs.readFileSync(path)`). */
  source: string | Buffer;
  fileKind?: "vue" | "sfc" | "vue_sfc" | "non_sfc" | "text" | "file";
  aliases?: string[];
}

export interface HostStyleOverrideEntry {
  index: number;
  /** Preprocessed CSS. Accepts a string or a Buffer (UTF-8 bytes). */
  code: string | Buffer;
  sourceMap?: string;
}

export interface HostStyleOverrideRequest {
  canonicalId: string;
  compileProfile?: HostCompileProfile;
  overrides: HostStyleOverrideEntry[];
}

export interface NativeBlockOverrideEntry {
  /** Block type: "template", "script", "style", or "custom". */
  blockType: "template" | "script" | "style" | "custom";
  /** Block index (0 for template/script, 0..N for styles/custom blocks). */
  index: number;
  /** Preprocessed code. Accepts a string or a Buffer (UTF-8 bytes). */
  code: string | Buffer;
  /** Source map from the preprocessor, if available. */
  sourceMap?: string;
}

export interface NativeBlockOverrideRequest {
  canonicalId: string;
  compileProfile?: HostCompileProfile;
  overrides: NativeBlockOverrideEntry[];
}

export type HostPublicApiMode = "public" | "testing";

// =============================================================================
// Workspace (filesystem-backed VFS)
// =============================================================================

/**
 * Workspace backed by a `FilesystemWorkspace`.
 *
 * Provides file access, import resolution, and project configuration.
 * Construct first, then pass to `VerterHost.withWorkspace()`.
 */
/** Directory entry returned by `Workspace.readDir()`. */
export interface WorkspaceDirEntry {
  path: string;
  isDir: boolean;
}

export declare class Workspace {
  /**
   * Create a new workspace rooted at the given directories.
   * Auto-discovers tsconfigs, builds the project graph, and populates the resolver.
   */
  constructor(roots: string[]);

  // ── File reads (async — runs on libuv thread pool) ──

  /** Read a file from the workspace (overlay → snapshot → disk). */
  readFile(path: string): Promise<string | null>;
  /** Check if a file exists in the workspace. */
  fileExists(path: string): Promise<boolean>;
  /** Check if a path is a directory. */
  isDir(path: string): Promise<boolean>;
  /** Resolve symlinks to real path. Returns null if not found. */
  realpath(path: string): Promise<string | null>;

  // ── Directory listing (async) ──

  /** List entries in a directory. */
  readDir(dir: string): Promise<WorkspaceDirEntry[]>;
  /**
   * Recursively walk a directory. Returns matching file paths.
   * @param excludeDirs - Directory names to skip (e.g., ["node_modules", ".git"])
   * @param extensions - File extensions to include (e.g., [".vue", ".ts"]). Omit for all files.
   */
  walk(root: string, excludeDirs: string[], extensions?: string[]): Promise<string[]>;

  // ── File writes (async) ──

  /** Write file content. Creates parent directories as needed. */
  writeFile(path: string, content: string): Promise<void>;
  /** Create a directory and all parent directories. */
  createDirAll(path: string): Promise<void>;
  /** Delete a file. */
  deleteFile(path: string): Promise<void>;
  /** Delete a directory and all its contents. */
  deleteDirAll(path: string): Promise<void>;
  /** Copy a file from src to dst. */
  copyFile(src: string, dst: string): Promise<void>;

  // ── Resolution (async) ──

  /**
   * Resolve an import specifier with context.
   * @param phase - "codegen" (default) or "provider"
   * @param kind - "esm" (default), "type", "require", or "src"
   */
  resolveImport(
    importer: string,
    specifier: string,
    phase?: string,
    kind?: string,
  ): Promise<string | null>;

  /**
   * Configure project resolver from tsconfig/alias data.
   * Replaces (not merges with) any auto-discovered graph.
   */
  configureProjects(projects: import("./host-types").HostIdeProjectConfig[]): void;

  // ── Editor lifecycle ──

  /** Notify workspace that an editor buffer is open/changed. */
  notifyUpsert(canonicalId: string, source: Buffer): void;
  /** Notify workspace that an editor buffer was closed. */
  notifyClose(canonicalId: string): void;
  /** Notify workspace that a file was deleted. */
  notifyDelete(canonicalId: string): void;
}

// =============================================================================
// VerterHost
// =============================================================================

export declare class VerterHost {
  constructor(config?: import("./host-types").HostConfig);

  /**
   * Create a host backed by the given workspace.
   * The workspace handles all file access and import resolution.
   * Use `workspace.configureProjects()` before calling this.
   */
  static withWorkspace(
    config: import("./host-types").HostConfig | undefined,
    workspace: Workspace,
  ): VerterHost;

  resolve(rawId: string): import("./host-types").HostResolvedId | null;
  upsert(request: HostUpsertRequest): import("./host-types").HostUpdateResult;
  applyBlockOverrides(request: NativeBlockOverrideRequest): import("./host-types").HostUpdateResult;
  getPublicApi(
    canonicalId: string,
    mode?: HostPublicApiMode,
  ): { code: string; sourceMap?: string } | null;
  getIde(
    canonicalId: string,
    profile?: import("./host-types").HostCompileProfile,
  ): import("./host-types").HostIdeResponse | null;
  getVirtualFile(
    query: import("./host-types").HostVirtualQuery,
  ): import("./host-types").HostVirtualFileResponse | null;
  listVirtualFiles(canonicalId: string): import("./host-types").HostVirtualNodeKind[];
  remove(canonicalOrAlias: string): import("./host-types").HostRemoveResult | null;
  /**
   * Returns the analysis snapshot for a file as a JSON string, or null if the file
   * doesn't exist. When `analysisLevel` is not "full", computes analysis on demand.
   */
  getAnalysis(canonicalOrAlias: string): string | null;
  /**
   * Returns all exports of a file, following re-export chains to their ultimate source.
   * For barrel files, resolves through chains to return the ultimate source file and name.
   */
  resolveExports(canonicalOrAlias: string): import("./host-types").HostResolvedExport[];
  /**
   * Sets the resolved import dependencies for a file, enabling Tier 2/3
   * smart invalidation (cross-file change tracking).
   */
  setImportDependencies(
    canonicalOrAlias: string,
    resolutions: import("./host-types").HostDependencyResolution[],
  ): void;
  collectResolvableModuleReferenceSpecifiers(
    moduleReferences: import("./host-types").HostModuleReference[],
  ): string[];
  resolveKnownModuleReferenceDependencies(
    ownerCanonicalId: string,
    moduleReferences: import("./host-types").HostModuleReference[],
    knownIds: string[],
    extensions?: string[],
  ): string[];
  /**
   * Runs lint rules against a file's analysis data and returns diagnostics.
   * @param config - Optional JSON string with lint config. Pass undefined for defaults.
   */
  lint(canonicalOrAlias: string, config?: string): import("./host-types").HostLintDiagnostic[];
  /**
   * Returns code actions (quick fixes) available at a given UTF-16 offset.
   */
  getCodeActions(canonicalOrAlias: string, offset: number): import("./host-types").HostCodeAction[];
  /**
   * Returns metadata for all registered lint rules.
   */
  getLintRuleMetadata(): import("./host-types").HostLintRuleMetadata[];
  /**
   * Returns document symbols for a file (outline / Ctrl+Shift+O).
   */
  getDocumentSymbols(canonicalOrAlias: string): import("./host-types").HostDocumentSymbol[];
  /**
   * Matches CSS selectors against template elements, returning a match matrix.
   */
  matchCssSelectors(canonicalOrAlias: string): import("./host-types").HostSelectorMatchResult[];
  /**
   * Release all cached data (files, aliases, dependency graph).
   * Call before dropping the host to prevent process exit hangs.
   */
  close(): void;

  /**
   * Configure project-scoped path alias resolution.
   *
   * Pass an empty array to clear the resolver.
   *
   * @deprecated Use `workspace.configureProjects()` instead when using `withWorkspace()`.
   */
  configureProjects(projects: import("./host-types").HostIdeProjectConfig[]): void;

  /**
   * Resolve an import specifier through the VFS resolution chain.
   *
   * @param phase - "codegen" (default) or "provider"
   * @param kind - "esm" (default), "type", "require", or "src"
   */
  resolveImport(importer: string, specifier: string, phase?: string, kind?: string): string | null;

  /**
   * Evaluate type annotations for a file's component metadata using the
   * lightweight native evaluator.
   *
   * Returns JSON `{ props, emits, slotBindings, bindings }` or `null`.
   */
  evaluateTypes(canonicalOrAlias: string): string | null;
}

// =============================================================================
// MetaProject / MetaSession — pooled runtime for component-meta
// =============================================================================

/**
 * A shared, long-lived project wrapping one native host.
 *
 * Multiple sessions can be opened against the same project. The project
 * owns the host, base file caches, and session management. Create one
 * per tsconfig / project root and reuse it across checkers.
 */
export declare class MetaProject {
  constructor(config?: import("./host-types").HostConfig);

  /**
   * Create a MetaProject backed by an existing Workspace.
   */
  static withWorkspace(
    config: import("./host-types").HostConfig | undefined,
    workspace: Workspace,
  ): MetaProject;

  /** Load a file into the base project (shared across all sessions). */
  upsertBase(canonicalId: string, source: string | Buffer): void;

  /** Ensure a workspace-backed file is loaded into the shared base project. */
  ensureLoaded(canonicalId: string): boolean;

  /** Refresh a shared base file from the current workspace. */
  refreshBase(canonicalId: string): boolean;

  /** Configure project-scoped path alias resolution. */
  configureProjects(projects: import("./host-types").HostIdeProjectConfig[]): void;

  /** Open a new isolated session against this project. */
  openSession(): MetaSession;

  /** Clear shared analysis caches without shutting down. */
  clearCaches(): void;

  /** Terminal shutdown. Stops the host and invalidates all sessions. */
  shutdown(): void;

  /** Whether this project has been shut down. */
  readonly isShutdown: boolean;

  /** Number of active sessions. */
  readonly sessionCount: number;

  /** Returns canonical IDs in the base file index. */
  baseFileIds(): string[];
}

/**
 * A lightweight session handle with isolated file overlays.
 *
 * Overlays are private to this session. `upsert()` and `delete()`
 * in one session never affect another session's view. Queries resolve
 * through `session overlay → shared base`.
 */
export declare class MetaSession {
  /** Store a file overlay in this session. */
  upsert(canonicalId: string, source: string | Buffer): void;

  /** Tombstone a file in this session (mark as deleted). */
  delete(canonicalId: string): void;

  /** Single native component-meta query. Returns null if the file is missing. */
  getComponentMeta(canonicalOrAlias: string): string | null;

  /**
   * Get effective source for a file (overlay → base).
   * Returns null for tombstoned or non-existent files.
   */
  getEffectiveSource(canonicalId: string): string | null;

  /** Check if a file is visible in this session (not tombstoned). */
  hasFile(canonicalId: string): boolean;

  /** Returns canonical IDs of all files visible to this session. */
  trackedFileIds(): string[];

  /** Return provenance counters for observability. */
  getProvenance(): string;

  /** Close the session, releasing the overlay and lease. Idempotent. */
  close(): void;

  /** Whether this session has been closed. */
  readonly isClosed: boolean;

  /** The overlay generation counter for this session. */
  readonly overlayGeneration: number;
}

export {};
