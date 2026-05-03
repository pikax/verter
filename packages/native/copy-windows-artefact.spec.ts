/**
 * Tier 6 §8.2 / T9.1 — discriminating tests for the Windows-conditional
 * post-build copy hook in `pnpm run build:native`.
 *
 * Test list:
 *   - `windows_native_artefact_present_after_build_native` — on Windows,
 *     after `pnpm run build:native` (or an equivalent synthetic
 *     simulation), `packages/native/dist/verter-native.win32-x64-msvc.node`
 *     must exist. On non-Windows the test is skipped (NOT vacuously
 *     passing).
 *   - Predicate-discriminator companion tests pin the hook's behavior
 *     so the production `windows_native_artefact_present_after_build_native`
 *     test rejects pre-change trees (no hook → no copy, target absent).
 */

import { describe, expect, it } from "vitest";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { execFileSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const packageDir = dirname(__filename);
const repoRoot = resolve(packageDir, "..", "..");
const distDir = join(packageDir, "dist");
const TARGET_FILENAME = "verter-native.win32-x64-msvc.node";
const targetPath = join(distDir, TARGET_FILENAME);
const HOOK_SCRIPT_PATH = join(packageDir, "scripts", "copy-windows-artefact.mjs");

const isWindows = process.platform === "win32";

describe("Tier 6 §8.2 / T9.1 — windows post-build copy hook", () => {
  // ── Sanity: the hook script exists. This is the change the brief
  // requires; absence of this file proves the pre-change tree (the
  // discriminator's pre-state).
  it("hook script `copy-windows-artefact.mjs` is present in packages/native/scripts/", () => {
    expect(existsSync(HOOK_SCRIPT_PATH)).toBe(true);
  });

  // ── Sanity: the package.json `build` and `build:debug` scripts
  // chain `pnpm run copy-windows-artefact` after `napi build`.
  // Without this chain, a Windows developer who runs
  // `pnpm run build:native` after a direct `cargo build` will not
  // see the artefact land in `packages/native/dist/`.
  it("packages/native package.json `build` chains the copy hook", () => {
    const pkgJson = JSON.parse(readFileSync(join(packageDir, "package.json"), "utf8"));
    const scripts = pkgJson.scripts as Record<string, string>;
    expect(scripts.build).toMatch(/pnpm run copy-windows-artefact/);
    expect(scripts["build:debug"]).toMatch(/pnpm run copy-windows-artefact/);
    expect(scripts["copy-windows-artefact"]).toContain("copy-windows-artefact.mjs");
  });

  // ── Discriminator: on Windows, given a synthetic `target/.../release/
  // verter_napi.dll` tree, invoking the hook produces the dist
  // artefact. On non-Windows, the hook is a no-op.
  it.runIf(isWindows)(
    "synthetic build tree → hook produces dist/verter-native.win32-x64-msvc.node",
    () => {
      const scratch = mkdtempSync(join(tmpdir(), "verter-t91-hook-"));
      try {
        const fakeRepoRoot = join(scratch, "repo");
        const fakePackageDir = join(fakeRepoRoot, "packages", "native");
        const fakeScriptsDir = join(fakePackageDir, "scripts");
        const fakeTargetDir = join(fakeRepoRoot, "target", "x86_64-pc-windows-msvc", "release");
        const fakeDistDir = join(fakePackageDir, "dist");
        mkdirSync(fakeScriptsDir, { recursive: true });
        mkdirSync(fakeTargetDir, { recursive: true });
        // Plant a fake "DLL" with non-zero content.
        const fakeDll = join(fakeTargetDir, "verter_napi.dll");
        writeFileSync(fakeDll, "fake-dll-bytes\n");
        // Copy the real hook script into the synthetic package dir.
        const realHook = HOOK_SCRIPT_PATH;
        const fakeHook = join(fakeScriptsDir, "copy-windows-artefact.mjs");
        writeFileSync(fakeHook, readFileSync(realHook, "utf8"));

        execFileSync(process.execPath, [fakeHook], { stdio: "pipe" });

        const fakeTarget = join(fakeDistDir, TARGET_FILENAME);
        expect(existsSync(fakeTarget)).toBe(true);
        // Discriminator: the copied artefact has the fake DLL bytes,
        // not an empty/placeholder file. A regression that wires the
        // hook to write zero bytes (or copies the wrong file) fails
        // here.
        const copiedBytes = readFileSync(fakeTarget, "utf8");
        expect(copiedBytes).toBe("fake-dll-bytes\n");
        // Avoid an unused-variable lint for fakeDll on tooling that
        // is strict about it; the path is referenced by the hook
        // itself, not directly here.
        expect(existsSync(fakeDll)).toBe(true);
      } finally {
        rmSync(scratch, { recursive: true, force: true });
      }
    },
  );

  it.runIf(!isWindows)("hook is a silent no-op on non-Windows platforms", () => {
    // The hook must NOT touch dist on non-Windows; it returns early
    // before any filesystem call. This characterizes the
    // platform-conditional behaviour the brief specifies.
    const out = execFileSync(process.execPath, [HOOK_SCRIPT_PATH], { stdio: "pipe" });
    // Empty stdout (silent no-op).
    expect(out.toString()).toBe("");
  });

  // ── Discriminator: when the target file is already present and
  // non-empty, the hook is idempotent (does not re-copy or wipe).
  it.runIf(isWindows)("idempotent — pre-existing non-empty target is left untouched", () => {
    const scratch = mkdtempSync(join(tmpdir(), "verter-t91-idempotent-"));
    try {
      const fakeRepoRoot = join(scratch, "repo");
      const fakePackageDir = join(fakeRepoRoot, "packages", "native");
      const fakeScriptsDir = join(fakePackageDir, "scripts");
      const fakeTargetDir = join(fakeRepoRoot, "target", "x86_64-pc-windows-msvc", "release");
      const fakeDistDir = join(fakePackageDir, "dist");
      mkdirSync(fakeScriptsDir, { recursive: true });
      mkdirSync(fakeTargetDir, { recursive: true });
      mkdirSync(fakeDistDir, { recursive: true });
      const fakeTarget = join(fakeDistDir, TARGET_FILENAME);
      // Pre-populate the target with a known marker.
      writeFileSync(fakeTarget, "PRE-EXISTING-MARKER");
      // Also stage a different DLL that should NOT overwrite.
      writeFileSync(join(fakeTargetDir, "verter_napi.dll"), "FRESH-DLL-BYTES");
      const fakeHook = join(fakeScriptsDir, "copy-windows-artefact.mjs");
      writeFileSync(fakeHook, readFileSync(HOOK_SCRIPT_PATH, "utf8"));

      execFileSync(process.execPath, [fakeHook], { stdio: "pipe" });

      const bytes = readFileSync(fakeTarget, "utf8");
      // Marker preserved → hook detected the existing non-empty
      // target and skipped. A regression that unconditionally
      // overwrites would produce "FRESH-DLL-BYTES" here.
      expect(bytes).toBe("PRE-EXISTING-MARKER");
    } finally {
      rmSync(scratch, { recursive: true, force: true });
    }
  });

  // ── The plan's named discriminating test:
  // `windows_native_artefact_present_after_build_native`. On Windows
  // we assert the hook produces the artefact when given the inputs
  // a successful build would produce. We simulate the build artefact
  // (cargo's verter_napi.dll) rather than running cargo, so the test
  // is hermetic and fast.
  it.runIf(isWindows)("windows_native_artefact_present_after_build_native", () => {
    const scratch = mkdtempSync(join(tmpdir(), "verter-t91-discriminator-"));
    try {
      const fakeRepoRoot = join(scratch, "repo");
      const fakePackageDir = join(fakeRepoRoot, "packages", "native");
      const fakeScriptsDir = join(fakePackageDir, "scripts");
      const fakeTargetDir = join(fakeRepoRoot, "target", "x86_64-pc-windows-msvc", "release");
      const fakeDistDir = join(fakePackageDir, "dist");
      mkdirSync(fakeScriptsDir, { recursive: true });
      mkdirSync(fakeTargetDir, { recursive: true });
      // Synthesize the Rust build artefact napi build would produce.
      writeFileSync(join(fakeTargetDir, "verter_napi.dll"), "RUST-DLL-PAYLOAD");
      // Copy the live hook into the synthetic package layout.
      writeFileSync(
        join(fakeScriptsDir, "copy-windows-artefact.mjs"),
        readFileSync(HOOK_SCRIPT_PATH, "utf8"),
      );

      // Assert the pre-state: target is absent.
      const fakeTarget = join(fakeDistDir, TARGET_FILENAME);
      expect(existsSync(fakeTarget)).toBe(false);

      // Run the hook as the build pipeline would.
      execFileSync(process.execPath, [join(fakeScriptsDir, "copy-windows-artefact.mjs")], {
        stdio: "pipe",
      });

      // Post-state: target is present with the DLL bytes.
      expect(existsSync(fakeTarget)).toBe(true);
      const bytes = readFileSync(fakeTarget, "utf8");
      expect(bytes).toBe("RUST-DLL-PAYLOAD");
    } finally {
      rmSync(scratch, { recursive: true, force: true });
    }
  });

  // Companion: on a non-Windows host, the test is skipped (NOT
  // vacuously passing). We still pin a soft sanity check so a future
  // refactor that breaks the non-Windows no-op path surfaces here.
  it.runIf(!isWindows)(
    "windows_native_artefact_present_after_build_native (skipped: non-Windows host)",
    () => {
      // Brief gate: on non-Windows the production test is skipped.
      // We still characterize the no-op behavior to prevent silent
      // regressions on non-Windows CI runners.
      const out = execFileSync(process.execPath, [HOOK_SCRIPT_PATH], { stdio: "pipe" });
      expect(out.toString()).toBe("");
    },
  );
});
