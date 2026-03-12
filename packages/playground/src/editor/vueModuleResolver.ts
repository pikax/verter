/**
 * Pure functions for resolving .vue module imports in the playground's
 * virtual file system. Extracted from tsWorker.ts for testability.
 */

/**
 * Resolve a relative path against a base directory.
 * Handles `.` and `..` segments. Returns an absolute path starting with `/`.
 */
export function resolvePath(baseDir: string, relative: string): string {
  const baseParts = baseDir.split("/").filter(Boolean);
  const relParts = relative.split("/");

  for (const part of relParts) {
    if (part === "" || part === ".") {
      continue;
    } else if (part === "..") {
      if (baseParts.length > 0) {
        baseParts.pop();
      }
    } else {
      baseParts.push(part);
    }
  }

  return "/" + baseParts.join("/");
}

/**
 * Resolve a Vue module import to its `.d.ts` virtual file path.
 *
 * Handles two patterns:
 * - `./Foo.vue.ts` → strip `.ts` → `./Foo.vue` → resolve → `/Foo.vue.d.ts`
 * - `./Foo.vue` → resolve → `/Foo.vue.d.ts`
 *
 * Only matches relative imports (`./` or `../`). Returns `null` for
 * non-vue imports, non-relative imports, or when the `.d.ts` file
 * doesn't exist in the virtual file system.
 */
export function resolveVueModulePath(
  moduleName: string,
  containingFile: string,
  fileExists: (path: string) => boolean,
): string | null {
  // Only handle relative imports
  if (!moduleName.startsWith("./") && !moduleName.startsWith("../")) {
    return null;
  }

  // Normalize: strip trailing .ts from .vue.ts imports
  let vuePath = moduleName;
  if (vuePath.endsWith(".vue.ts")) {
    vuePath = vuePath.slice(0, -3); // strip ".ts" → ".vue"
  }

  // Must end with .vue after normalization
  if (!vuePath.endsWith(".vue")) {
    return null;
  }

  // Resolve against containing file's directory
  const lastSlash = containingFile.lastIndexOf("/");
  const baseDir = lastSlash >= 0 ? containingFile.slice(0, lastSlash + 1) : "/";
  const dtsPath = resolvePath(baseDir, vuePath) + ".d.ts";

  return fileExists(dtsPath) ? dtsPath : null;
}
