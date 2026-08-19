/**
 * Guards against the launcher spawning itself.
 *
 * `resolveBinary` legitimately falls back to a bare `PATH` name when no
 * built native binary exists on disk (see `launcher.spec.ts`). Under pnpm/npm
 * script execution `PATH` includes `node_modules/.bin`, where the launcher's
 * own CLI shim is registered under the same bare name — so the naive spawn
 * runs itself, which resolves the same way again, without bound.
 *
 * These tests exercise `runLauncherCli` directly, the one place that turns a
 * resolved candidate into an actual `spawnSync`. A stub `launcher` is enough:
 * `runLauncherCli` only calls `toolName`, `resolveBinary()` and
 * `binaryCandidates()`.
 */

import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";

import { runLauncherCli } from "./cli.js";

const scratchDirs: string[] = [];
afterEach(() => {
  for (const dir of scratchDirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

function scratchDir(name: string): string {
  const dir = mkdtempSync(join(tmpdir(), name));
  scratchDirs.push(dir);
  return dir;
}

/** A marker file a spawned process writes to, proving it actually ran. */
function markerPath(): string {
  return join(scratchDir("cli-marker-"), "ran.marker");
}

/** Capture-only stdio, so assertions read the exact launcher output. */
function captureStream() {
  let text = "";
  return { write: (chunk: string) => (text += chunk), text: () => text };
}

function fakeLauncher(overrides: Partial<Parameters<typeof runLauncherCli>[0]["launcher"]>) {
  return {
    toolName: "verter-test",
    binaryCandidates: () => [{ path: "/fake/dev-build/verter-test", source: "dev-build" as const }],
    resolveBinary: () => ({ path: "/fake/dev-build/verter-test", source: "dev-build" as const }),
    ...overrides,
  };
}

const ACTIVE_ENV_VAR = "VERTER_LAUNCHER_ACTIVE";
const originalActive = process.env[ACTIVE_ENV_VAR];
beforeEach(() => {
  delete process.env[ACTIVE_ENV_VAR];
});
afterEach(() => {
  if (originalActive === undefined) delete process.env[ACTIVE_ENV_VAR];
  else process.env[ACTIVE_ENV_VAR] = originalActive;
});

describe("runLauncherCli — self-spawn refusal", () => {
  const originalPath = process.env.PATH;
  afterEach(() => {
    process.env.PATH = originalPath;
  });

  it("refuses a bare-name candidate that PATH resolves to a node shim, and never runs it", () => {
    const binDir = scratchDir("cli-shim-bin-");
    const marker = markerPath();
    // What `node_modules/.bin/verter-test` looks like for real: a node shim,
    // not a native binary. If spawned it proves it ran by writing `marker`.
    const shimPath = join(binDir, "verter-test");
    writeFileSync(
      shimPath,
      `#!/usr/bin/env node\nrequire("node:fs").writeFileSync(${JSON.stringify(marker)}, "1");\n`,
      { mode: 0o755 },
    );
    process.env.PATH = `${binDir}${delimiter}${process.env.PATH ?? ""}`;

    const launcher = fakeLauncher({
      binaryCandidates: () => [{ path: "verter-test", source: "path" as const }],
      resolveBinary: () => ({ path: "verter-test", source: "path" as const }),
    });
    const stderr = captureStream();

    const code = runLauncherCli({ launcher, argv: [], stderr: stderr as never });

    expect(code).not.toBe(0);
    expect(existsSync(marker)).toBe(false);
    expect(stderr.text()).toMatch(/verter-test/);
    expect(stderr.text()).toMatch(/not found/i);
  });

  it("refuses a candidate that resolves to the launcher's own bin script", () => {
    const pkgDir = scratchDir("cli-own-pkg-");
    const marker = markerPath();
    const selfPath = join(pkgDir, "bin", "run.js");
    mkdirSync(join(pkgDir, "bin"), { recursive: true });
    writeFileSync(
      selfPath,
      `#!/usr/bin/env node\nrequire("node:fs").writeFileSync(${JSON.stringify(marker)}, "1");\n`,
      { mode: 0o755, flag: "w" },
    );
    // Simulate resolution having (wrongly) landed back on the launcher's own
    // script — the same failure mode `binaryCandidates` producing a `path`
    // source hitting `node_modules/.bin` collapses to at spawn time.
    const launcher = fakeLauncher({
      binaryCandidates: () => [{ path: selfPath, source: "dev-build" as const }],
      resolveBinary: () => ({ path: selfPath, source: "dev-build" as const }),
    });
    const stderr = captureStream();

    const code = runLauncherCli({ launcher, argv: [], selfPath, stderr: stderr as never });

    expect(code).not.toBe(0);
    expect(existsSync(marker)).toBe(false);
    expect(stderr.text()).toMatch(/verter-test/);
  });
});

describe("runLauncherCli — re-entrancy guard", () => {
  it("fails closed when the tool is already marked active, without spawning again", () => {
    process.env[ACTIVE_ENV_VAR] = "verter-test";
    const marker = markerPath();
    const launcher = fakeLauncher({
      resolveBinary: () => ({ path: process.execPath, source: "dev-build" as const }),
    });
    const stderr = captureStream();

    const code = runLauncherCli({
      launcher,
      argv: ["-e", `require("node:fs").writeFileSync(${JSON.stringify(marker)}, "1")`],
      stderr: stderr as never,
    });

    expect(code).not.toBe(0);
    expect(existsSync(marker)).toBe(false);
    expect(stderr.text()).toMatch(/verter-test/);
    expect(stderr.text()).toMatch(/already active|re-entra/i);
  });

  it("does not block a different tool", () => {
    process.env[ACTIVE_ENV_VAR] = "verter-other";
    const marker = markerPath();
    const launcher = fakeLauncher({
      resolveBinary: () => ({ path: process.execPath, source: "dev-build" as const }),
    });

    const code = runLauncherCli({
      launcher,
      argv: ["-e", `require("node:fs").writeFileSync(${JSON.stringify(marker)}, "1")`],
    });

    expect(code).toBe(0);
    expect(existsSync(marker)).toBe(true);
  });
});

describe("runLauncherCli — legitimate spawn (no regression)", () => {
  it("still spawns a real resolved binary, and marks the tool active for the child", () => {
    const marker = markerPath();
    const launcher = fakeLauncher({
      resolveBinary: () => ({ path: process.execPath, source: "dev-build" as const }),
    });

    const code = runLauncherCli({
      launcher,
      argv: [
        "-e",
        `require("node:fs").writeFileSync(${JSON.stringify(marker)}, process.env.${ACTIVE_ENV_VAR} || "")`,
      ],
    });

    expect(code).toBe(0);
    expect(existsSync(marker)).toBe(true);
    expect(readFileSync(marker, "utf8")).toBe("verter-test");
  });
});
