import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = resolve(__dirname, "../../..");

export function resolveVerterCompatSourceEntry(
  root = repoRoot,
  fileExists: (path: string) => boolean = existsSync,
): string | null {
  const candidate = resolve(root, "packages", "component-meta", "src", "compat", "index.ts");
  return fileExists(candidate) ? candidate : null;
}

export async function loadVerterCompatModule() {
  const sourceEntry = resolveVerterCompatSourceEntry();
  if (sourceEntry) {
    return import(pathToFileURL(sourceEntry).href);
  }

  const require = createRequire(import.meta.url);
  return require("@verter/component-meta/compat");
}
