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
export declare function compileBatch(
  files: BatchFile[],
  options?: BatchOptions,
): BatchResult[];

// =============================================================================
// VerterHost (in-memory virtual file host)
//
// Shared types re-exported from host-types.ts. Native-specific overrides
// (Buffer support) for HostUpsertRequest, HostStyleOverrideEntry, and
// HostStyleOverrideRequest are defined below.
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

export declare class VerterHost {
  constructor(config?: import("./host-types").HostConfig);
  resolve(rawId: string): import("./host-types").HostResolvedId | null;
  upsert(request: HostUpsertRequest): import("./host-types").HostUpdateResult;
  /** @deprecated Use `applyBlockOverrides` instead — unified API for all block types. */
  applyStyleOverrides(request: HostStyleOverrideRequest): import("./host-types").HostUpdateResult;
  applyBlockOverrides(request: NativeBlockOverrideRequest): import("./host-types").HostUpdateResult;
  getPublicApi(
    canonicalId: string,
    mode?: HostPublicApiMode,
  ): { code: string; sourceMap?: string } | null;
  getIde(canonicalId: string, profile?: import("./host-types").HostCompileProfile): import("./host-types").HostIdeResponse | null;
  getVirtualFile(query: import("./host-types").HostVirtualQuery): import("./host-types").HostVirtualFileResponse | null;
  listVirtualFiles(canonicalId: string): import("./host-types").HostVirtualNodeKind[];
  remove(canonicalOrAlias: string): import("./host-types").HostRemoveResult | null;
  /**
   * Returns the analysis snapshot for a file as a JSON string, or null if the file
   * doesn't exist. When `analysisLevel` is not "full", computes analysis on demand.
   */
  getAnalysis(canonicalOrAlias: string): string | null;
  /**
   * Sets the resolved import dependencies for a file, enabling Tier 2/3
   * smart invalidation (cross-file change tracking).
   */
  setImportDependencies(canonicalOrAlias: string, resolutions: import("./host-types").HostDependencyResolution[]): void;
  collectResolvableModuleReferenceSpecifiers(moduleReferences: import("./host-types").HostModuleReference[]): string[];
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
}

export {};
