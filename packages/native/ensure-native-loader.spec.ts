/**
 * Discriminating tests for `scripts/ensure-native-loader.mjs` — the
 * build-before-use contract (issue #90 item 7).
 *
 * The guard runs as the package `pretest`. Contract:
 *   - if `<packageDir>/dist/index.js` is ABSENT, exit non-zero with an
 *     ACTIONABLE message naming the build command (not a bare
 *     MODULE_NOT_FOUND);
 *   - if it is PRESENT, exit zero silently.
 *
 * Both branches run hermetically: the script derives `dist` from its own
 * location (parent-of-scripts), so we plant a copy of the script in a temp
 * `scripts/` dir and toggle the sibling `dist/index.js`.
 */

import { describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { PACKAGE_DIR } from "./platforms.ts";

const SCRIPT_SRC = join(PACKAGE_DIR, "scripts", "ensure-native-loader.mjs");

/** Run the guard in a temp package layout; return { code, stderr }. */
function runGuard(withLoader: boolean): { code: number; stderr: string; stdout: string } {
  const scratch = mkdtempSync(join(tmpdir(), "verter-ensure-loader-"));
  try {
    const scriptsDir = join(scratch, "scripts");
    const distDir = join(scratch, "dist");
    mkdirSync(scriptsDir, { recursive: true });
    mkdirSync(distDir, { recursive: true });
    writeFileSync(join(scriptsDir, "ensure-native-loader.mjs"), readFileSync(SCRIPT_SRC, "utf8"));
    if (withLoader) {
      writeFileSync(join(distDir, "index.js"), "/* generated loader */");
    }

    try {
      const stdout = execFileSync(
        process.execPath,
        [join(scriptsDir, "ensure-native-loader.mjs")],
        { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
      );
      return { code: 0, stderr: "", stdout };
    } catch (err: any) {
      return {
        code: typeof err.status === "number" ? err.status : -1,
        stderr: err.stderr?.toString() ?? "",
        stdout: err.stdout?.toString() ?? "",
      };
    }
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

describe("ensure-native-loader.mjs — build-before-use contract", () => {
  it("FAILS with an actionable message when dist/index.js is absent", () => {
    const { code, stderr } = runGuard(false);
    expect(code).not.toBe(0);
    // Actionable: names the missing file and the build command.
    expect(stderr).toMatch(/dist\/index\.js/);
    expect(stderr).toMatch(/build:debug/);
    expect(stderr).toMatch(/@verter\/native/);
    // NOT a bare module-not-found.
    expect(stderr).not.toMatch(/MODULE_NOT_FOUND/);
  });

  it("PASSES silently (exit 0) when dist/index.js is present", () => {
    const { code, stderr, stdout } = runGuard(true);
    expect(code).toBe(0);
    // "Silently" is part of the contract: NOTHING on stderr AND nothing on
    // stdout — the guard must not chatter on the happy path.
    expect(stderr).toBe("");
    expect(stdout).toBe("");
  });
});
