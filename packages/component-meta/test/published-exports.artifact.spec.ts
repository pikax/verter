/**
 * Consumer-boundary proof for the published export map.
 *
 * Every assertion here imports the BUILT artifact through the exact
 * `package.json#exports` target a consumer resolves, so a subpath whose entry
 * points at a file that does not exist, does not load, or no longer carries the
 * export fails loudly. Reading the entry from the manifest (rather than a
 * hard-coded `dist/...` path) is what makes the export map itself the subject.
 *
 * Artifact-dependent: requires `pnpm build:native` + `pnpm build:ts`. It is
 * excluded from the package's artifact-independent `test` script and runs in
 * `test:native`, the artifact lane — the same arrangement the native-eval suite
 * uses. There is no existence probe: a missing artifact is a failure, never a
 * skip.
 */

import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { describe, expect, it } from "vitest";

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

const manifest = JSON.parse(readFileSync(resolve(packageRoot, "package.json"), "utf8")) as {
  exports?: Record<string, { import?: string; types?: string }>;
};

/** Import a subpath through its declared `exports` target. */
async function importPublishedSubpath(subpath: string): Promise<Record<string, unknown>> {
  const entry = manifest.exports?.[subpath]?.import;
  if (typeof entry !== "string") {
    throw new Error(`package.json exports has no "import" target for subpath ${subpath}`);
  }
  return (await import(pathToFileURL(resolve(packageRoot, entry)).href)) as Record<string, unknown>;
}

describe("published export map", () => {
  it("serves the compat projection helpers from the built artifact", async () => {
    const compat = await importPublishedSubpath("./compat");

    expect(typeof compat.projectDeclaredOnlyNativeResult).toBe("function");
    expect(typeof compat.projectDeclaredOnlyFromNativePayload).toBe("function");

    // Behavior through the artifact, not just the presence of a name.
    const projectResult = compat.projectDeclaredOnlyNativeResult as (v: unknown) => unknown;
    const projectPayload = compat.projectDeclaredOnlyFromNativePayload as (v: unknown) => unknown;
    expect(projectResult(null)).toBeNull();
    expect(projectPayload(null)).toBeNull();
  });

  it("serves the Volar-compatible checker entrypoints from the built artifact", async () => {
    const compat = await importPublishedSubpath("./compat");

    expect(typeof compat.createChecker).toBe("function");
    expect(typeof compat.createCheckerByJson).toBe("function");
    expect(typeof compat.mapComponentMeta).toBe("function");
    expect(typeof compat.typeDescriptorToString).toBe("function");
  });

  it("serves the root entrypoint from the built artifact", async () => {
    const root = await importPublishedSubpath(".");

    expect(typeof root.typeExprToDescriptor).toBe("function");
    expect(typeof root.getMetaOrigin).toBe("function");
  });
});
