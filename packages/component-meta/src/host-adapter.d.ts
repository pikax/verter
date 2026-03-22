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
  /** Release host-backed resources when the backend exposes lifecycle control. */
  close?(): void;
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
