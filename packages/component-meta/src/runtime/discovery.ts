/**
 * File discovery utilities for engine bootstrap.
 *
 * Discovers .vue files from tsconfig or project root,
 * reads tsconfig compilerOptions for path alias configuration.
 */

import { resolve, dirname } from "node:path";
import { CLIENT_CARRIER_EXTENSIONS } from "@verter/language-shared";
import type { CheckerWorkspace } from "../compat/checker.js";
import { dirnamePath, normalizePath, resolvePath } from "./engine-key.js";
import { parseTsconfigText } from "./tsconfig.js";

/**
 * Whether a file name is a framework-carrier component file — ends in ANY
 * registered carrier extension (`.vue` / `.svelte`). Carrier-generic,
 * manifest-derived, NOT a hardcoded `.endsWith(".vue")`.
 */
function isCarrierComponentFile(name: string): boolean {
  return CLIENT_CARRIER_EXTENSIONS.some((ext) => name.endsWith(ext));
}

async function readFileSafe(
  absPath: string,
  ws: Pick<CheckerWorkspace, "readFile">,
): Promise<string | null> {
  return (await ws.readFile(normalizePath(absPath))) ?? null;
}

/**
 * Parse tsconfig.json and extract compilerOptions + include patterns.
 */
export async function parseTsconfig(
  tsconfigPath: string,
  ws: Pick<CheckerWorkspace, "readFile">,
): Promise<{ config: Record<string, unknown>; dir: string } | null> {
  const absPath = resolve(tsconfigPath);
  const raw = await readFileSafe(absPath, ws);
  if (!raw) return null;
  const config = parseTsconfigText(raw);
  return config ? { config, dir: dirname(absPath) } : null;
}

/**
 * The project configuration of an INLINE (tsconfig-less) config object: its
 * path aliases over a match-all project rooted at `projectRoot`. With no
 * tsconfig file there is no configured program, so `files` / `include` /
 * `exclude` do not define the project's roots. A config read from a tsconfig
 * file goes through {@link tsconfigProjectConfig} instead.
 */
export function extractPathAliases(
  config: Record<string, unknown>,
  projectRoot: string,
): {
  root: string;
  workspaceRoot: string;
  compilerOptions?: {
    baseUrl?: string;
    paths?: { pattern: string; targets: string[] }[];
  };
} {
  const compilerOptions = (config.compilerOptions ?? {}) as Record<string, unknown>;
  const rawPaths = (compilerOptions.paths ?? {}) as Record<string, string[]>;
  const paths = Object.entries(rawPaths).map(([pattern, targets]) => ({
    pattern,
    targets,
  }));

  return {
    root: projectRoot,
    workspaceRoot: projectRoot,
    compilerOptions: {
      baseUrl: (compilerOptions.baseUrl as string) ?? undefined,
      paths: paths.length > 0 ? paths : undefined,
    },
  };
}

/**
 * The project configuration of a tsconfig file. The native workspace loader
 * reads everything else from the file itself — its `extends` chain,
 * compiler options (`paths` and `baseUrl` included), and its `files` /
 * `include` / `exclude` membership, which is the program's root files in
 * their authored order. The project is rooted at the tsconfig's directory;
 * `workspaceRoot` bounds package resolution.
 */
export function tsconfigProjectConfig(
  tsconfigPath: string,
  workspaceRoot: string,
): { root: string; workspaceRoot: string; tsconfigPath: string } {
  const absPath = resolvePath(tsconfigPath);
  return {
    root: dirnamePath(absPath),
    workspaceRoot: normalizePath(workspaceRoot),
    tsconfigPath: absPath,
  };
}

/**
 * Discover .vue files by walking directories, excluding node_modules.
 */
export async function discoverVueFiles(
  rootDir: string,
  ws: CheckerWorkspace,
  include?: string[],
): Promise<string[]> {
  const files: string[] = [];

  if (include && include.length > 0) {
    for (const pattern of include) {
      const absPattern = resolve(rootDir, pattern);

      // Specific file path
      if (/\.\w+$/.test(pattern) && !pattern.includes("*")) {
        if (await ws.fileExists(normalizePath(absPattern))) {
          files.push(absPattern);
        }
        continue;
      }

      // Glob patterns — walk the directory part
      const globIndex = pattern.indexOf("*");
      if (globIndex !== -1) {
        const dirPart = pattern.substring(0, globIndex).replace(/[/\\]+$/, "");
        const absDir = resolve(rootDir, dirPart);
        if (
          (await ws.fileExists(normalizePath(absDir))) &&
          (await ws.isDir(normalizePath(absDir)))
        ) {
          await collectVueFiles(absDir, files, ws);
        }
        continue;
      }

      // Plain directory
      if (
        (await ws.fileExists(normalizePath(absPattern))) &&
        (await ws.isDir(normalizePath(absPattern)))
      ) {
        await collectVueFiles(absPattern, files, ws);
      }
    }
  } else {
    // Default: walk the root
    await collectVueFiles(rootDir, files, ws);
  }

  return [...new Set(files)];
}

async function collectVueFiles(
  dir: string,
  files: string[],
  ws: CheckerWorkspace,
  depth = 0,
): Promise<void> {
  if (depth > 10) return;
  try {
    const entries = await ws.readDir(normalizePath(dir));
    for (const entry of entries) {
      const name = entry.path.split("/").pop() ?? "";
      if (name.startsWith(".") || name === "node_modules") continue;
      if (entry.isDir) {
        await collectVueFiles(entry.path, files, ws, depth + 1);
      } else if (isCarrierComponentFile(name)) {
        files.push(entry.path);
      }
    }
  } catch {
    // Directory not readable
  }
}
