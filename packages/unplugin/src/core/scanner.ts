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
export async function scanVueFiles(
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
  const ws = getWorkspace()!;
  let entries;
  try {
    entries = ws.readDir(normalizePath(dir));
  } catch {
    return;
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
      const content = ws.readFile(normalizePath(join(dir, name)));
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
