/**
 * End-to-end optional-dependency fallback proof with the REAL host binary,
 * fully offline (issue #90 item 6). No fakes, no sentinel, no network.
 *
 * Steps (all local):
 *   1. `npm pack` the main `@verter/native` → a tarball that ships the
 *      wrapper + generated loader + types and NO `.node` (verified).
 *   2. Assemble the host platform package `@verter/native-<host-triple>`
 *      from its committed `npm/<triple>/package.json` template + a COPY of
 *      the real built `dist/verter-native.<host-triple>.node`, and
 *      `npm pack` that too.
 *   3. Extract BOTH tarballs into a temp install tree:
 *        <tmp>/node_modules/@verter/native
 *        <tmp>/node_modules/@verter/native-<host-triple>
 *   4. In a CHILD process (isolating the second native-addon load),
 *      `require('@verter/native')` and assert it loaded the REAL binary
 *      via the optional-dependency fallback — the main package has no
 *      `.node`, so the only way `VerterHost` exists is the fallback — and
 *      that a real call works end-to-end (`new VerterHost()` constructs and
 *      `processStyle` returns a real result).
 *
 * Platform-general: the host triple is DERIVED at runtime from
 * `process.platform`/`process.arch`/musl via `currentHostEntry()` (the same
 * matrix + musl detection the loader probe reconciles against), so this
 * smoke runs the REAL-`.node` fallback on whatever supported platform
 * executes it — Linux / macOS / Windows — not just one pinned host.
 *
 * Skip / fail discipline (issue #90 round-2 items 1+2):
 *   - GENUINELY UNSUPPORTED host (no `PLATFORM_MATRIX` row for this
 *     platform/arch) ⇒ a LOUD skip (there is nothing to build here).
 *   - SUPPORTED host (a matrix row exists) but the matching `.node` is
 *     absent/misnamed ⇒ this FAILS. The package `pretest` runs the build,
 *     so on a supported host the host `.node` MUST exist; a missing one is a
 *     real defect (bad build / wrong filename), never a green skip.
 */

import { describe, expect, it } from "vitest";
import { execFileSync, execSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { PACKAGE_DIR, currentHostEntry } from "./platforms.ts";

// The canonical matrix row for THIS host, or null when the host platform/arch
// is genuinely unsupported (no row to build). Derived from
// process.platform/arch/musl, NOT hard-pinned to one triple.
const hostEntry = currentHostEntry();
// A matrix row exists for this host ⇒ the host is SUPPORTED and the build
// (run by `pretest`) must have produced the matching `.node`.
const hostSupported = hostEntry !== null;
const hostNodePath = hostEntry ? join(PACKAGE_DIR, "dist", hostEntry.nodeFileName) : null;

/**
 * `npm pack --json --pack-destination <dest>` → absolute tarball path.
 * Uses the shell form (like `pack-shape.spec.ts`): Node refuses to spawn
 * `npm.cmd` via `execFileSync` on Windows without a shell (EINVAL), so we
 * run it through the shell, which resolves `npm` the same on every OS.
 */
function npmPack(packDir: string, dest: string): string {
  const raw = execSync(`npm pack --json --pack-destination "${dest}"`, {
    cwd: packDir,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  const parsed = JSON.parse(raw) as Array<{ filename: string }>;
  return join(dest, parsed[0].filename);
}

/** Extract an npm tarball (its `package/` root) into `destPkgDir`. */
function extractTarball(tarball: string, destPkgDir: string): void {
  mkdirSync(destPkgDir, { recursive: true });
  // npm tarballs put everything under `package/`; strip that one level.
  // GNU tar (msys2 on Windows): pass forward-slash paths so backslashes are
  // not read as escapes, and `--force-local` so the `C:` drive letter is a
  // local path, not a remote `host:path` rsh spec.
  const fwd = (p: string) => p.replace(/\\/g, "/");
  execFileSync(
    "tar",
    ["--force-local", "-xzf", fwd(tarball), "-C", fwd(destPkgDir), "--strip-components=1"],
    { stdio: ["ignore", "pipe", "pipe"] },
  );
}

// The smoke RUNS on every SUPPORTED host (a matrix row exists). On a
// genuinely unsupported host it is skipped (handled by the loud-skip suite
// below). On a supported host with a missing `.node` it does NOT skip — the
// first `it` FAILS with an actionable message (items 1+2).
describe.skipIf(!hostSupported)(
  "issue #90 — host-platform local-tarball smoke (real .node, offline)",
  () => {
    // Guard the build-before-use contract at the SUPPORTED-host boundary: a
    // matrix row exists, so the build must have produced the host `.node`.
    // A missing/misnamed binary is a FAILURE here (not a skip), naming the
    // expected path + the build command.
    it("the host .node exists (supported host ⇒ build:debug must have produced it)", () => {
      // `hostSupported` gates the suite, so these are non-null here.
      expect(hostEntry).not.toBeNull();
      expect(hostNodePath).not.toBeNull();
      expect(
        existsSync(hostNodePath!),
        `Expected host native binary at ${hostNodePath} for supported host ` +
          `${hostEntry!.napiTriple} (${process.platform}-${process.arch}), but it is missing. ` +
          `Run \`pnpm --filter @verter/native run build:debug\` to produce it. ` +
          `(The package pretest runs this build; a missing binary on a supported ` +
          `host means the build failed or emitted a differently-named file.)`,
      ).toBe(true);
    });

    it("require('@verter/native') loads the REAL binary via the optional-dependency fallback", () => {
      const entry = hostEntry!;
      const nodePath = hostNodePath!;
      // Defensive: if the build did not produce the host binary, fail with the
      // same actionable message rather than a confusing copyFileSync ENOENT.
      expect(
        existsSync(nodePath),
        `Expected host native binary at ${nodePath} for supported host ` +
          `${entry.napiTriple}; run \`pnpm --filter @verter/native run build:debug\`.`,
      ).toBe(true);

      const scratch = mkdtempSync(join(tmpdir(), "verter-issue90-tarball-"));
      try {
        const packOut = join(scratch, "tarballs");
        mkdirSync(packOut, { recursive: true });

        // 1) Pack the main package straight from the package dir.
        const mainTarball = npmPack(PACKAGE_DIR, packOut);

        // 2) Assemble + pack the host platform package from its template +
        //    a copy of the real built .node.
        const platformBuildDir = join(scratch, "platform-build");
        mkdirSync(platformBuildDir, { recursive: true });
        const templatePkgJson = join(PACKAGE_DIR, "npm", entry.napiTriple, "package.json");
        copyFileSync(templatePkgJson, join(platformBuildDir, "package.json"));
        copyFileSync(nodePath, join(platformBuildDir, entry.nodeFileName));
        const platformTarball = npmPack(platformBuildDir, packOut);

        // 3) Extract BOTH into a temp node_modules tree.
        const nmScope = join(scratch, "node_modules", "@verter");
        const mainPkgDir = join(nmScope, "native");
        const platformPkgDir = join(nmScope, `native-${entry.napiTriple}`);
        extractTarball(mainTarball, mainPkgDir);
        extractTarball(platformTarball, platformPkgDir);

        // Pre-state proof: the MAIN package carries NO .node anywhere.
        const distDir = join(mainPkgDir, "dist");
        const mainNodeFiles = existsSync(distDir)
          ? readdirSync(distDir).filter((f) => f.endsWith(".node"))
          : [];
        expect(mainNodeFiles).toEqual([]);
        // The PLATFORM package carries exactly the real binary.
        expect(existsSync(join(platformPkgDir, entry.nodeFileName))).toBe(true);

        // 4) Drive the load + a real call in a CHILD process so the second
        //    native-addon instance is isolated from this test runner.
        const driver = join(scratch, "drive.cjs");
        writeFileSync(
          driver,
          `
const path = require("node:path");
const mainEntry = ${JSON.stringify(mainPkgDir)};
const native = require(mainEntry);
const out = { ok: true };
out.hasVerterHost = typeof native.VerterHost === "function";
out.hasProcessStyle = typeof native.processStyle === "function";
// Real construction + a real call through the wrapper (string -> Buffer).
const host = new native.VerterHost();
out.constructed = host != null;
if (typeof host.close === "function") host.close();
const styled = native.processStyle("body { color: red }", { scopeId: "smoke1" });
out.styledHasCode = typeof styled.code === "string";
out.styledCodeNonEmpty = (styled.code || "").length > 0;
process.stdout.write(JSON.stringify(out));
`,
        );

        const childRaw = execFileSync(process.execPath, [driver], {
          cwd: scratch,
          encoding: "utf8",
          stdio: ["ignore", "pipe", "pipe"],
        });
        const result = JSON.parse(childRaw) as Record<string, boolean>;

        // The binding surface came from the REAL .node (only reachable via
        // the optional-dependency fallback, since main ships no .node).
        expect(result.hasVerterHost).toBe(true);
        expect(result.hasProcessStyle).toBe(true);
        // A real native call worked end-to-end.
        expect(result.constructed).toBe(true);
        expect(result.styledHasCode).toBe(true);
        expect(result.styledCodeNonEmpty).toBe(true);
      } finally {
        rmSync(scratch, { recursive: true, force: true });
      }
    }, 120_000);
  },
);

describe.skipIf(hostSupported)("issue #90 — host-platform local-tarball smoke (skipped)", () => {
  it("loudly skips because this host platform/arch is not in the supported matrix", () => {
    // Not a vacuous pass: this asserts WHY it skipped — a genuinely
    // UNSUPPORTED host (no PLATFORM_MATRIX row). A SUPPORTED host with a
    // missing binary does NOT reach here (it runs + fails above); this branch
    // is exclusively the no-matrix-row case.
    expect(hostEntry).toBeNull();
    expect(hostNodePath).toBeNull();
  });
});
