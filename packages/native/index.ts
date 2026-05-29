/// <reference types="node" />

// Re-export audit helpers so `@verter/native` consumers can import
// `whyLoaded`, `whyInstantiated`, `assertLoadedFilesExactly`, etc.
// directly from the package root. Plan §3 Commit 8.
export * from "./audit";

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
// Host-backed batch compile — VerterHost.compileMany
//
// Replaces the previous free-fn `compileBatch` (Rayon-direct,
// stateless, bypassing VerterHost). The new entry point is the
// `host.compileMany(files, options)` instance method, which routes
// through the host's scheduler + dispatch + compile_cache.
// =============================================================================

/**
 * Caller-requested compile cache mode. `"session"` (the default)
 * consults the fact-validated session cache; `"content"` the pure
 * content-addressed cache; `"stateless"` bypasses both.
 */
export type CompileCacheMode = "stateless" | "content" | "session";

/** Why a requested compile cache mode was constrained. */
export type DowngradeReason =
  | "HasExternalSrc"
  | "HasMacroTypeDeps"
  | "HasWorkspaceAlias"
  | "HasModuleAugmentation"
  | "HasBlockOverride"
  | "HasStyleOverride"
  | "HasIdeOnlyAnalysis"
  | "HasDevLastGood";

export interface CompileBatchInput {
  canonicalId: string;
  /** SFC source. Accepts a string or a Buffer (UTF-8 bytes). */
  source: string | Buffer;
  /**
   * Requested compile cache mode. Omit to inherit the batch
   * `defaultMode` (which itself defaults to "session").
   */
  requestedMode?: CompileCacheMode;
}

export interface CompileBatchOptions {
  /** Number of host orchestration threads. 0 or undefined = all logical CPUs. */
  threads?: number;
  /**
   * Scheduler priority for batch upserts. Default: "background" (yields to
   * concurrent interactive work). Use "interactive" when there is no
   * concurrent interactive work and you want full-throttle execution
   * (benchmarks, CI cold-start measurement).
   */
  priority?: "interactive" | "background";
  /**
   * Default compile cache mode for inputs whose `requestedMode` is
   * unset. Defaults to "session" (the host default).
   */
  defaultMode?: CompileCacheMode;
}

export interface CompileBatchEntry {
  canonicalId: string;
  code: string;
  sourceMap?: string;
  /** All compilation errors for this file. Empty on success. */
  errors: string[];
  durationMs: number;
  /**
   * True iff this input was served from a warm cache slot (the
   * fact-validated session slot OR the content-addressed store).
   */
  cacheHit: boolean;
  /** The compile cache mode the caller requested. */
  requestedMode: CompileCacheMode;
  /** The compile cache mode the runtime actually ran under. */
  actualMode: CompileCacheMode;
  /** Highest-priority downgrade reason, or undefined when none fired. */
  downgradeReason?: DowngradeReason;
}

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
  /**
   * Compile a batch of Vue SFC inputs through the production host
   * path (scheduler + dispatch + compile_cache).
   *
   * Returns one [`CompileBatchEntry`] per input, in the original
   * input order. Per-input panic isolation: if codegen panics for
   * one input, only that input's entry receives a `compiler panic:
   * ...` error message; the rest of the batch completes normally.
   */
  compileMany(files: CompileBatchInput[], options?: CompileBatchOptions): CompileBatchEntry[];
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

  // ===========================================================================
  // Typed audit entry-points (Wave 3 / Slice 3.H)
  //
  // Each entry-point wraps a `VerterHost::*_with_audit` Rust producer
  // and returns the produced `RequestAuditRecord` as a JSON Buffer.
  // Audit must be enabled on the host config (`auditEnabled: true`)
  // for these calls to publish a record; otherwise they short-circuit
  // to `null` (the underlying operation still runs).
  // ===========================================================================

  /**
   * Run a single type-resolution query through the shared dispatch
   * and return the produced `RequestAuditRecord` as a JSON Buffer.
   * Resolves `declName` in the top-level scope of `canonicalId`.
   * Returns `null` when audit is disabled.
   */
  resolveTypeWithAudit(canonicalId: string, declName: string): Buffer | null;

  /**
   * Compile `canonicalId` for the requested codegen target and
   * return the produced `RequestAuditRecord` as a JSON Buffer.
   * Accepted target names: `"BUNDLER"`, `"IDE"`, `"ANALYSIS"`,
   * `"META"`, `"TSX"`, `"TSC"`. Returns `null` when audit is
   * disabled.
   */
  compileWithAudit(canonicalId: string, target: string): Buffer | null;

  /**
   * Materialise the `AnalysisReady` artifact for `canonicalId` under
   * audit and return the produced `RequestAuditRecord` as a JSON
   * Buffer. Returns `null` when audit is disabled or the canonical
   * does not exist.
   */
  analyzeWithAudit(canonicalId: string): Buffer | null;

  /**
   * Drive a workspace operation under audit and return the produced
   * `RequestAuditRecord` as a JSON Buffer. The `op` argument is
   * shaped as `{ type: "AuditResolve", specifier, from } | { type:
   * "DepGraphTraverse", root } | { type: "ResolverWalk", specifier
   * }`. Always returns a record (the workspace producer drives the
   * operation regardless of audit configuration).
   */
  auditWorkspaceOp(op: WorkspaceOpArgument): Buffer;

  /**
   * Drain the most-recent `RequestAuditRecord` from the host's audit
   * store. Returns `null` when the store is empty.
   *
   * "Most recent" is defined by insertion order. The returned record
   * is removed from the store.
   */
  getLastAuditRecord(): Buffer | null;

  /**
   * Non-destructive filtered query over the host's audit store.
   * Returns a JSON Buffer carrying an array of matching records.
   *
   * Filter fields are independent — combining them narrows further:
   * - `kind`: `"ComponentMeta"`, `"TypeResolution"`,
   *   `"SemanticAnalysis"`, `"Compile"`, `"Workspace"`, `"Lsp"`,
   *   `"Mcp"`, `"BundlerBatch"`, `"Custom"`.
   * - `sinceRequestId`: minimum request id (exclusive). Decimal
   *   string matching the JSON serialization of `request_id`.
   * - `limit`: cap the returned record count (oldest-first).
   */
  getAuditRecords(filter?: AuditRecordFilter): Buffer;

  /**
   * Run the bundler-batch aggregator over the host's audit store and
   * return the produced `BundlerBatchPayload` as a JSON Buffer.
   *
   * - `kind`: `"Vite"`, `"Webpack"`, `"Rollup"`, `"Esbuild"`,
   *   `"Rolldown"`, or any other string for the `Other` variant.
   *   Defaults to `"Vite"` when absent.
   * - `sinceRequestId`: optional minimum request id watermark.
   */
  getBundlerBatchSummary(args?: BundlerBatchSummaryArgs): Buffer;

  // ===========================================================================
  // Typeinfo entry-points (Phase 4 / typeinfo plan §6.1)
  //
  // Wrap the Rust host typeinfo substrate. Used by `@verter/typeinfo`
  // for `TypeInfoSession`'s public API.
  // ===========================================================================

  /**
   * Return the top-level symbol inventory for `canonicalId`.
   *
   * JSON Buffer carrying a `Vec<FfiSymbolEntry>` (camelCase shape).
   * The call is bounded by the shallow-state size and does NOT emit
   * an audit record.
   */
  listSymbols(canonicalId: string): Buffer;

  /**
   * Resolve `name` in `canonicalId`'s top-level scope and return the
   * raised `TypeExpr` plus the per-request audit record.
   *
   * `typeArgs` is a JSON Buffer carrying an array of native
   * `TypeExpr` values; pass `null` for "no generic instantiation".
   * `mode` is one of `"identity" | "navigate" | "shallow" |
   * "expanded" | "skeleton"`; pass `null` to take the host's default
   * (Navigate for generic carriers, Expanded otherwise).
   *
   * `typeExpr` is `null` when the symbol could not be resolved.
   * `auditRecord` is `null` when audit is disabled.
   */
  resolveSymbolWithAudit(
    canonicalId: string,
    name: string,
    typeArgs: Buffer | null,
    mode: string | null,
  ): { typeExpr: Buffer | null; auditRecord: Buffer | null };

  /**
   * Evaluate a synthetic type expression in a file scope and return
   * the raised `TypeExpr` plus the per-request audit record.
   *
   * `request` is a JSON Buffer carrying a
   * `verter_protocol::typeinfo::FfiEvaluateTypeExpressionRequest`.
   *
   * `typeExpr` is `null` when the expression could not be resolved.
   * `auditRecord` is `null` when audit is disabled.
   */
  evaluateTypeExpressionWithAudit(request: Buffer): {
    typeExpr: Buffer | null;
    auditRecord: Buffer | null;
  };
}

/**
 * Workspace op argument shape for `VerterHost.auditWorkspaceOp`.
 */
export type WorkspaceOpArgument =
  | { type: "AuditResolve"; specifier: string; from: string }
  | { type: "DepGraphTraverse"; root: string }
  | { type: "ResolverWalk"; specifier: string };

/**
 * Filter argument for `VerterHost.getAuditRecords`. All fields are
 * optional; combining them narrows the result set further.
 */
export interface AuditRecordFilter {
  kind?: string;
  sinceRequestId?: string;
  limit?: number;
}

/**
 * Args for `VerterHost.getBundlerBatchSummary`.
 */
export interface BundlerBatchSummaryArgs {
  kind?: string;
  sinceRequestId?: string;
}

// =============================================================================
// ComponentMetaHost / ComponentMetaSession — component-meta host surface
// =============================================================================

/**
 * A shared, long-lived host wrapping one native component-meta engine.
 *
 * Multiple sessions can be opened against the same project. The project
 * owns the host, base file caches, and session management. Create one
 * per tsconfig / project root and reuse it across checkers.
 */
export declare class ComponentMetaHost {
  constructor(config?: import("./host-types").HostConfig);

  /**
   * Create a ComponentMetaHost backed by an existing Workspace.
   */
  static withWorkspace(
    config: import("./host-types").HostConfig | undefined,
    workspace: Workspace,
  ): ComponentMetaHost;

  /** Load a file into the base project (shared across all sessions). */
  upsertBase(canonicalId: string, source: string | Buffer): void;

  /** Ensure a workspace-backed file is loaded into the shared base project. */
  ensureLoaded(canonicalId: string): boolean;

  /** Refresh a shared base file from the current workspace. */
  refreshBase(canonicalId: string): boolean;

  /** Configure project-scoped path alias resolution. */
  configureProjects(projects: import("./host-types").HostIdeProjectConfig[]): void;

  /** Open a new isolated session against this project. */
  openSession(): ComponentMetaSession;

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
export declare class ComponentMetaSession {
  /** Store a file overlay in this session. */
  upsert(canonicalId: string, source: string | Buffer): void;

  /** Tombstone a file in this session (mark as deleted). */
  delete(canonicalId: string): void;

  /** Single native component-meta query. Returns a protobuf payload or null. */
  getComponentMeta(canonicalOrAlias: string): Buffer | null;

  /**
   * Resolved-surface native component-meta query. Returns the same
   * protobuf payload shape as `getComponentMeta`, but exposed under a
   * dedicated entry-point for callers that want to address the resolved
   * variant explicitly.
   */
  getResolvedComponentMeta(canonicalOrAlias: string): Buffer | null;

  /**
   * Tier 1B selective surface API (D32 + D101). Returns the
   * `ComponentMetaSurface` envelope: eager scalars + `NamedTypeHandle`
   * for every type-bearing field. Consumers walk the type graph one
   * layer at a time via {@link getComponentMetaTypeExpansion}. The
   * bytes are a `verter.v1.ComponentMetaSurface` protobuf message.
   *
   * Returns `null` when the canonical does not resolve to a component.
   * Returns an error envelope (first byte `0xFF`) when the bridge
   * encountered a typed `BridgeError` (D114).
   */
  getComponentMetaSurface(canonicalOrAlias: string): Buffer | null;

  /**
   * Tier 1B selective surface API (D32 + D101). Resolves a
   * `TypeHandle` (encoded as a `verter.v1.TypeHandle` protobuf
   * message) into a one-layer `verter.v1.TypeExpansion`. The optional
   * `depth` argument is currently informational; the bridge always
   * returns one layer per call.
   *
   * On error returns an error envelope (first byte `0xFF`) carrying a
   * `verter.v1.TypeHandleError` (D104 + D114).
   */
  getComponentMetaTypeExpansion(handleBuf: Buffer, depth?: number): Buffer;

  /**
   * Synchronous audit bundle — returns JSON bytes of
   * `{ analysis, resolution, record }` or `null` if the canonical does
   * not resolve.
   *
   * The host must have `audit_enabled` + `footprint_capture` set on
   * construction; otherwise this throws. Promise ergonomics (if
   * desired) live in `packages/native/audit.ts` — the Rust binding
   * itself is synchronous.
   */
  getComponentMetaWithAudit(canonicalOrAlias: string): Buffer | null;

  /**
   * Run the Rust provenance walker against a committed audit bundle
   * JSON string, rooted at `canonicalId`. Returns a
   * `ProvenanceChain` JSON string. Plan §2.8.
   */
  whyLoadedFromAuditJson(auditJson: string, canonicalId: string): string;

  /**
   * Run the Rust provenance walker rooted at the instantiation keyed
   * by `(declCanonicalId, declSymbolName, argsFingerprintHex)`.
   * `argsFingerprintHex` is the 32-character lowercase hex rendering
   * of the 16-byte `Hash16`.
   */
  whyInstantiatedFromAuditJson(
    auditJson: string,
    declCanonicalId: string,
    declSymbolName: string,
    argsFingerprintHex: string,
  ): string;

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

/** @deprecated Use ComponentMetaHost. */
export declare const MetaProject: typeof ComponentMetaHost;

/** @deprecated Use ComponentMetaSession. */
export declare const MetaSession: typeof ComponentMetaSession;

export {};
