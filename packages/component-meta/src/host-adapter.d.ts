/**
 * Internal adapter contract used by the compat checker.
 *
 * This file is intentionally not re-exported from the package root.
 */

/** Request to compile or update a file in the host. */
export interface HostUpsertRequest {
  /** File identifier (path or canonical ID). */
  inputId: string;
  /** Full source text of the file. */
  source: string;
  /** File kind hint. Defaults to auto-detection based on extension. */
  fileKind?: "vue" | "sfc" | "vue_sfc" | "non_sfc" | "text" | "file";
}

/** Internal checker adapter contract over native host/session backends. */
export interface VerterHostAdapter {
  /** Compile or update a file. */
  upsert(request: HostUpsertRequest): unknown;
  /** Remove a file from the host when the backend supports true deletion. */
  remove?(canonicalOrAlias: string): unknown;
  /** Retrieve the analysis snapshot for a file, or `null` if not found. */
  getAnalysis(canonicalOrAlias: string): unknown | null;
  /** Release host-backed resources when the backend exposes lifecycle control. */
  close?(): void;
  /** Resolve imported types for a file's macro type dependencies. Returns JSON or null. */
  resolveImportedTypes?(canonicalOrAlias: string): string | null;
  /** Evaluate type annotations using the native lightweight evaluator. Returns JSON or null. */
  evaluateTypes?(canonicalOrAlias: string): string | null;
  /** Configure project-scoped path alias resolution (optional). */
  configureProjects?(
    projects: {
      root: string;
      workspaceRoot: string;
      tsconfigPath?: string;
      compilerOptions?: {
        baseUrl?: string;
        paths?: { pattern: string; targets: string[] }[];
      };
    }[],
  ): void;
}
