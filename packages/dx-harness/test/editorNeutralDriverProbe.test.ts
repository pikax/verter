/**
 * Control suite for the editor-neutral driver's tsserver plugin-probe preflight.
 *
 * The driver hands its `pluginPath` to `verter-lsp` as `--plugin-path`, which
 * reaches tsserver as `--pluginProbeLocations <dir>` alongside
 * `--globalPlugins @verter/typescript-plugin`. tsserver resolves the package NAME
 * out of `<dir>/node_modules`, so the probe must be a directory CONTAINING
 * `node_modules/@verter/typescript-plugin`.
 *
 * This suite exists because a wrong probe is INVISIBLE at runtime: Node's resolver
 * walks ancestor `node_modules`, so tsserver still finds the plugin — through
 * pnpm's private `.pnpm/node_modules` hoist directory — and the whole
 * editor-neutral contract goes green while the probe it declared does no work. The
 * preflight is the only thing standing between that and a silently vacuous gate,
 * and nothing else in CI exercises it: the contract suite never passes an explicit
 * `pluginPath`, so a regression to a probe-blind check would be caught by nobody.
 *
 * The preflight is tested through its own exported function rather than through
 * `create`, so acceptance can be asserted POSITIVELY — by the paths it resolves —
 * instead of by the absence of particular error strings. A control that only checks
 * "did not say X or Y" still passes when the preflight rejects a valid probe for
 * some third reason, which is not a control.
 */
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { afterAll, describe, expect, it } from "vitest";

import { resolvePluginProbeLocation } from "../src/editor-neutral/rawLspDriver.js";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "..", "..", "..");

const temps: string[] = [];
afterAll(() => {
  for (const dir of temps.splice(0)) rmSync(dir, { recursive: true, force: true });
});

/**
 * Build a probe location holding the package but no build output.
 *
 * Generated rather than committed: the layout's whole point is a real
 * `node_modules/@verter/typescript-plugin` directory, and the root `.gitignore`
 * ignores `node_modules`, so a committed fixture would exist only as untracked
 * workspace state — present on the machine that wrote it and ABSENT on a clean CI
 * checkout, where this suite would then fail for a reason unrelated to the code.
 */
function unbuiltProbeLocation(): string {
  const root = mkdtempSync(path.join(tmpdir(), "dx-unbuilt-probe-"));
  temps.push(root);
  const packageDirectory = path.join(root, "node_modules", "@verter", "typescript-plugin");
  mkdirSync(packageDirectory, { recursive: true });
  writeFileSync(
    path.join(packageDirectory, "package.json"),
    `${JSON.stringify(
      {
        name: "@verter/typescript-plugin",
        version: "0.0.0-fixture",
        private: true,
        main: "dist/index.js",
      },
      null,
      2,
    )}\n`,
    "utf8",
  );
  return root;
}

describe("editor-neutral driver plugin-probe preflight", () => {
  it("accepts a directory that really holds the package under node_modules", () => {
    // The POSITIVE control, asserted by what it resolves. `packages/vue-vscode` is
    // the driver's default: pnpm links the workspace package into its
    // `node_modules`, so tsserver's DIRECT candidate exists and it loads on the
    // first try rather than through the ancestor-walk fallback.
    const probe = path.join(REPO_ROOT, "packages", "vue-vscode");

    const resolved = resolvePluginProbeLocation(probe);

    expect(resolved.probeLocation).toBe(probe);
    expect(resolved.packageDirectory).toBe(
      path.join(probe, "node_modules", "@verter", "typescript-plugin"),
    );
    expect(resolved.entry).toBe(
      path.join(probe, "node_modules", "@verter", "typescript-plugin", "dist", "index.js"),
    );
    // The resolved paths are real, so acceptance is not a claim about strings.
    expect(existsSync(resolved.packageDirectory)).toBe(true);
    expect(existsSync(resolved.entry)).toBe(true);
  });

  it("refuses the package's dist directory, which is not a probe location", () => {
    // `dist` holds `index.js`, so a preflight that checks `<pluginPath>/index.js`
    // accepts it — while tsserver looks for
    // `dist/node_modules/@verter/typescript-plugin`, which does not exist.
    const dist = path.join(REPO_ROOT, "packages", "typescript-plugin", "dist");
    expect(
      existsSync(path.join(dist, "index.js")),
      "the entry a probe-blind check would have found",
    ).toBe(true);

    expect(() => resolvePluginProbeLocation(dist)).toThrow(
      /probe location holds no @verter\/typescript-plugin/,
    );
  });

  it("refuses the package root, which also holds no node_modules entry", () => {
    expect(() =>
      resolvePluginProbeLocation(path.join(REPO_ROOT, "packages", "typescript-plugin")),
    ).toThrow(/probe location holds no @verter\/typescript-plugin/);
  });

  it("refuses a probe whose package is present but unbuilt", () => {
    // Distinguishes the two failure modes: "wrong probe" and "right probe, no
    // build" must not collapse into one message, or a CI failure cannot tell a
    // contributor which one to fix.
    const unbuilt = unbuiltProbeLocation();
    expect(
      existsSync(path.join(unbuilt, "node_modules", "@verter", "typescript-plugin")),
      "the generated layout supplies the package directory without a dist",
    ).toBe(true);

    expect(() => resolvePluginProbeLocation(unbuilt)).toThrow(/build is missing its entry/);
  });
});
