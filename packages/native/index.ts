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
// VerterHost (in-memory virtual file host)
//
// Shared types re-exported from host-types.ts. Native-specific overrides
// (Buffer support) for HostUpsertRequest, HostStyleOverrideEntry, and
// HostStyleOverrideRequest are defined below.
// =============================================================================

export type {
  HostConfig,
  HostCompileProfile,
  HostVirtualNodeKind,
  HostSliceChanges,
  HostDiagnostic,
  HostDiagnosticsSnapshot,
  HostExternalSourceRequest,
  HostScriptImportInfo,
  HostUpdateResult,
  HostResolvedId,
  HostVirtualMeta,
  HostVirtualFileResponse,
  HostVirtualQuery,
  HostRemoveResult,
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

export declare class VerterHost {
  constructor(config?: import("./host-types").HostConfig);
  resolve(rawId: string): import("./host-types").HostResolvedId | null;
  upsert(request: HostUpsertRequest): import("./host-types").HostUpdateResult;
  applyStyleOverrides(request: HostStyleOverrideRequest): import("./host-types").HostUpdateResult;
  getVirtualFile(query: import("./host-types").HostVirtualQuery): import("./host-types").HostVirtualFileResponse;
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
  setImportDependencies(canonicalOrAlias: string, resolvedDeps: string[]): void;
}

export {};
