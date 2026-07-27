// Test-only helpers for the ExtensionTsService spec suites.
//
// `ExtensionTsService` resolves TypeScript ONLY from the workspace under test:
// it walks that workspace's OWN `node_modules` chain and anchors `createRequire`
// at the first entry that installs one — the OWNING directory's `package.json`,
// not the root's — so Node's global folders are never a source. The specs build
// fixture workspaces in the OS temp directory, whose chain installs no
// TypeScript at any level — so each fixture must materialize one explicitly,
// exactly like a real workspace's `npm install -D typescript`.

import { mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join } from "node:path";

/** Absolute path of the real `typescript` package the repo's tests run against. */
export function realTypeScriptPackageDir(): string {
  return dirname(createRequire(import.meta.url).resolve("typescript/package.json"));
}

/**
 * Materialize a workspace TypeScript install under `<root>/node_modules`.
 *
 * A directory junction keeps this cross-platform: on Windows a junction needs no
 * elevated privilege (unlike a default symlink), and POSIX treats the `junction`
 * type argument as an ordinary directory symlink.
 */
export function materializeWorkspaceTypeScript(root: string): void {
  mkdirSync(join(root, "node_modules"), { recursive: true });
  symlinkSync(realTypeScriptPackageDir(), join(root, "node_modules", "typescript"), "junction");
}

/**
 * Materialize a workspace TypeScript that RESOLVES but ships no default
 * libraries — the shape a pruned, partially-copied, or hand-vendored install
 * has. A language service built on it type-checks against no lib, so every
 * global reads as an error; the provider must refuse it rather than answer.
 */
export function materializeLibLessWorkspaceTypeScript(root: string): string {
  const pkgDir = join(root, "node_modules", "typescript");
  const libDir = join(pkgDir, "lib");
  mkdirSync(libDir, { recursive: true });
  writeFileSync(
    join(pkgDir, "package.json"),
    JSON.stringify({ name: "typescript", version: "0.0.0-libless", main: "./lib/typescript.js" }),
  );
  // Resolvable, loadable, and API-shaped like a real 5.x compiler — the ONLY
  // thing wrong with it is the empty lib directory, so the refusal under test is
  // unambiguously the library-less one.
  writeFileSync(
    join(libDir, "typescript.js"),
    "module.exports = { createLanguageService: () => ({}), createDocumentRegistry: () => ({}) };\n",
  );
  return libDir;
}

/**
 * Materialize a workspace TypeScript whose lib directory contains an entry with
 * a default-library NAME that is not a library — a directory called
 * `lib.es2025.d.ts` (the shape a partially-extracted archive or a stray output
 * directory leaves behind), plus a dangling symlink at another lib name.
 *
 * A name-only default-lib check counts both and admits a service that
 * type-checks against nothing.
 */
export function materializeLibShapedNonFileWorkspaceTypeScript(root: string): string {
  const libDir = materializeLibLessWorkspaceTypeScript(root);
  mkdirSync(join(libDir, "lib.es2025.d.ts"), { recursive: true });
  symlinkSync(join(libDir, "does-not-exist.d.ts"), join(libDir, "lib.dom.d.ts"));
  return libDir;
}

/**
 * Materialize an INSTALLED `@verter/types` under `<root>/node_modules`, with the
 * given declaration content.
 *
 * The fallback the provider serves when the package is absent must never shadow
 * a real install, so the control fixtures give the installed copy declarations
 * that are distinguishable from Verter's own.
 */
export function materializeInstalledVerterTypes(root: string, declarations: string): string {
  const pkgDir = join(root, "node_modules", "@verter", "types");
  mkdirSync(pkgDir, { recursive: true });
  writeFileSync(
    join(pkgDir, "package.json"),
    JSON.stringify({ name: "@verter/types", version: "0.0.0-fixture", types: "./index.d.ts" }),
  );
  const entry = join(pkgDir, "index.d.ts");
  writeFileSync(entry, declarations);
  return entry;
}

/**
 * Materialize a workspace TypeScript in the NATIVE-PREVIEW layout: the
 * `typescript` package is a thin launcher whose entry exposes no in-process
 * language service, and whose libraries live in a separate platform package
 * rather than beside the entry.
 *
 * It is a COMPLETE, correct install. The provider must refuse to drive it —
 * this service needs `createLanguageService` — but must not blame it for
 * missing libraries or tell the user to reinstall TypeScript.
 */
export function materializeNativePreviewWorkspaceTypeScript(root: string): string {
  const pkgDir = join(root, "node_modules", "typescript");
  const libDir = join(pkgDir, "lib");
  mkdirSync(libDir, { recursive: true });
  writeFileSync(
    join(pkgDir, "package.json"),
    JSON.stringify({ name: "typescript", version: "7.0.0-native", main: "./lib/version.cjs" }),
  );
  writeFileSync(join(libDir, "version.cjs"), 'module.exports = { version: "7.0.0" };\n');
  return libDir;
}
