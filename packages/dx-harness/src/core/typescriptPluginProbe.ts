/** @ai-generated - Centralizes hermetic validation of the repository TypeScript plugin probe. */
import { existsSync } from "node:fs";
import path from "node:path";

/** A validated tsserver plugin probe location and the entry it will load. */
export interface ResolvedPluginProbe {
  /** The directory handed to `verter-lsp` as `--plugin-path`. */
  readonly probeLocation: string;
  /** The package directory tsserver resolves under it. */
  readonly packageDirectory: string;
  /** The built entry that package's `main` points at. */
  readonly entry: string;
}

/**
 * Validate a tsserver plugin PROBE LOCATION, returning what tsserver will resolve.
 *
 * The value becomes `--pluginProbeLocations <dir>` while tsserver resolves the
 * global plugin package name below `<dir>/node_modules`. The direct check avoids
 * accepting an ancestor hit from pnpm's private hoist layout.
 */
export function resolvePluginProbeLocation(candidate: string): ResolvedPluginProbe {
  const probeLocation = path.resolve(candidate);
  const packageDirectory = path.join(probeLocation, "node_modules", "@verter", "typescript-plugin");
  if (!existsSync(packageDirectory)) {
    throw new Error(
      `plugin probe location holds no @verter/typescript-plugin: ${packageDirectory} does not ` +
        `exist, so tsserver cannot resolve the plugin from ${probeLocation}. Run: pnpm install`,
    );
  }
  const entry = path.join(packageDirectory, "dist", "index.js");
  if (!existsSync(entry)) {
    throw new Error(
      `@verter/typescript-plugin build is missing its entry: ${entry}. Produce it with: ` +
        "pnpm --filter @verter/language-shared --filter @verter/typescript-plugin build",
    );
  }
  return { probeLocation, packageDirectory, entry };
}

/** The repository-owned direct probe shared by every raw tsserver harness. */
export function repositoryTypescriptPluginProbe(repoRoot: string): ResolvedPluginProbe {
  return resolvePluginProbeLocation(path.join(repoRoot, "packages", "vue-vscode"));
}
