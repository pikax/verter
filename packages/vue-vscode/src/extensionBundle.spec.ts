// Artifact-level guard: the packed extension must not carry the TypeScript
// compiler.
//
// The extension's TypeScript language service resolves TypeScript from the
// USER'S workspace at runtime; shipping a second compiler inside the bundle is
// dead weight (it measured 68% of the production bundle) and, worse, a wrong
// answer source — a bundled compiler resolves its default libs next to the
// bundle, where no `lib.*.d.ts` is packed.
//
// The guard runs the REAL bundler over the SHIPPED entry point, using the very
// options `esbuild.mjs` builds `dist/extension.js` with (imported, not copied,
// so the guard cannot drift from the artifact users install), and asserts on
// esbuild's own dependency graph — a tool-based structural check on the emitted
// artifact, not a text scan. A static `require`/`import` of the `typescript`
// package from ANY module reachable from the entry puts the compiler back among
// the bundle inputs and fails this.

import esbuild from "esbuild";
import { describe, expect, it } from "vitest";

import { productionBundleConfig } from "../esbuild.config.mjs";

describe("extension bundle composition", () => {
  // A full production esbuild pass over the extension graph legitimately
  // exceeds the 5s framework default on a loaded parallel test run; the
  // assertions below, not the duration, are the discriminators.
  it("bundles no TypeScript compiler into the packed extension", { timeout: 60_000 }, async () => {
    const result = await esbuild.build({
      ...productionBundleConfig({ production: true, sourcemap: false }),
      write: false,
      metafile: true,
    });

    const inputs = Object.values(result.metafile.outputs).flatMap((output) =>
      Object.keys(output.inputs),
    );

    // Coverage proof: the guard is only meaningful if its graph actually reaches
    // the module that hosts the language service. If a refactor detaches it from
    // the entry, this fails LOUDLY instead of passing vacuously.
    expect(
      inputs.some((input) => /extensionTsService(Registry)?\.ts$/.test(input)),
      `the production graph must include the extension TypeScript service, found: ${inputs.length} inputs`,
    ).toBe(true);

    const bundledTypeScript = inputs.filter((input) =>
      /node_modules[\\/]typescript[\\/]/.test(input),
    );
    expect(
      bundledTypeScript,
      `no typescript package files may be bundled, found: ${bundledTypeScript.join(", ")}`,
    ).toEqual([]);
  });
});
