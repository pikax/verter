import { readdir, readFile } from "node:fs/promises";
import { join } from "path";
import { canonicalizeId } from "@verter/native";
import type { Workspace } from "@verter/native";

const EXCLUDED_DIRS = new Set(["node_modules"]);

/**
 * Recursively scans a directory for files matching the given filter.
 * Excludes `node_modules` and dot-directories (`.git`, `.vite`, etc.).
 * Returns a Map of forward-slash-normalized absolute paths to file contents.
 * Reads through `workspace` when one is given, otherwise from disk.
 */
export async function scanCarrierFiles(
  root: string,
  filter: (filename: string) => boolean,
  workspace: Workspace | null,
): Promise<Map<string, string>> {
  const result = new Map<string, string>();
  await walkDir(root, filter, workspace, result);
  return result;
}

async function walkDir(
  dir: string,
  filter: (filename: string) => boolean,
  ws: Workspace | null,
  result: Map<string, string>,
): Promise<void> {
  let entries;
  if (ws) {
    try {
      // Workspace.readDir is async (libuv thread pool).
      entries = await ws.readDir(canonicalizeId(dir));
    } catch {
      return;
    }
  } else {
    try {
      const dirents = await readdir(dir, { withFileTypes: true });
      entries = dirents.map((entry) => ({
        path: canonicalizeId(join(dir, entry.name)),
        isDir: entry.isDirectory(),
      }));
    } catch {
      return;
    }
  }

  const subdirPromises: Promise<void>[] = [];

  for (const entry of entries) {
    const name = entry.path.split("/").pop()!;

    if (entry.isDir) {
      // Skip node_modules and dot-directories
      if (EXCLUDED_DIRS.has(name) || name.startsWith(".")) continue;
      subdirPromises.push(walkDir(join(dir, name), filter, ws, result));
      continue;
    }

    const absPath = canonicalizeId(join(dir, name));
    if (!filter(absPath)) continue;

    try {
      // Workspace.readFile is async (libuv thread pool).
      const content = ws ? await ws.readFile(absPath) : await readFile(join(dir, name), "utf8");
      if (content !== null) {
        result.set(absPath, content);
      }
    } catch {
      // Skip unreadable files
    }
  }

  // Process subdirectories in parallel
  if (subdirPromises.length > 0) {
    await Promise.all(subdirPromises);
  }
}
