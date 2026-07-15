import { readdir, readFile } from "node:fs/promises";
import { join } from "path";
import { getWorkspace } from "./compiler";

function normalizePath(p: string): string {
  return p.replace(/\\/g, "/");
}

const EXCLUDED_DIRS = new Set(["node_modules"]);

/**
 * Recursively scans a directory for files matching the given filter.
 * Excludes `node_modules` and dot-directories (`.git`, `.vite`, etc.).
 * Returns a Map of forward-slash-normalized absolute paths to file contents.
 */
export async function scanCarrierFiles(
  root: string,
  filter: (filename: string) => boolean,
): Promise<Map<string, string>> {
  const result = new Map<string, string>();
  await walkDir(root, filter, result);
  return result;
}

async function walkDir(
  dir: string,
  filter: (filename: string) => boolean,
  result: Map<string, string>,
): Promise<void> {
  let entries;
  const ws = getWorkspace();
  if (ws) {
    try {
      entries = ws.readDir(normalizePath(dir));
    } catch {
      return;
    }
  } else {
    try {
      const dirents = await readdir(dir, { withFileTypes: true });
      entries = dirents.map((entry) => ({
        path: normalizePath(join(dir, entry.name)),
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
      subdirPromises.push(walkDir(join(dir, name), filter, result));
      continue;
    }

    const absPath = normalizePath(join(dir, name));
    if (!filter(absPath)) continue;

    try {
      const content = ws ? ws.readFile(absPath) : await readFile(join(dir, name), "utf8");
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
