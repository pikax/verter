/**
 * Discriminating tests for `scripts/clean-dist.mjs`.
 *
 * `clean-dist` runs at the start of every `@verter/native` build. Its
 * contract: remove EVERY `*.node` from `dist/` — including stale
 * legacy-named artifacts (`verter_napi.*.node`, `verter.*.node`) left by
 * an older binaryName or a direct `cargo build` — so a build never
 * leaves an orphaned binary that the pack-shape guard would reject or
 * that the generated loader would mis-prefer.
 *
 * It must NOT delete the generated loader (`dist/index.js`) or the
 * emitted type declarations (`dist/*.d.ts`) — those are non-binary
 * artifacts the published package needs.
 */

import { describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const packageDir = dirname(__filename);
const CLEAN_DIST_SCRIPT = join(packageDir, "scripts", "clean-dist.mjs");

describe("clean-dist.mjs", () => {
  it("removes every .node (incl. stale legacy names) but preserves loader + .d.ts", () => {
    const scratch = mkdtempSync(join(tmpdir(), "verter-clean-dist-"));
    try {
      // Reconstruct the package layout the script expects: it derives
      // dist as `<packageDir>/dist`, where packageDir is the parent of
      // the script's own dir. So plant the script at
      // `<scratch>/scripts/clean-dist.mjs` and dist at `<scratch>/dist`.
      const fakeScriptsDir = join(scratch, "scripts");
      const fakeDistDir = join(scratch, "dist");
      mkdirSync(fakeScriptsDir, { recursive: true });
      mkdirSync(fakeDistDir, { recursive: true });
      writeFileSync(
        join(fakeScriptsDir, "clean-dist.mjs"),
        readFileSync(CLEAN_DIST_SCRIPT, "utf8"),
      );

      // Binaries that MUST be removed — current name, stale legacy
      // binaryName, and the bare crate-name dll-as-node.
      //
      // The legacy-named fixtures (`verter_napi.*.node` from an older
      // `binaryName`, `verter.*.node` from a direct `cargo build`) are
      // DELIBERATE and must NOT be removed from this test: they are exactly
      // the stale artifacts a real build could leave behind, and the whole
      // point of `clean-dist` is to delete them so the pack-shape guard and
      // the generated loader never trip over an orphaned binary. Dropping
      // these fixture names would silently stop characterising the
      // legacy-cleanup contract. (Test fixture, not production source — the
      // no-phase-archaeology rule does not apply here.)
      const currentNode = join(fakeDistDir, "verter-native.win32-x64-msvc.node");
      const legacyNode = join(fakeDistDir, "verter_napi.win32-x64-msvc.node");
      const olderAliasNode = join(fakeDistDir, "verter.linux-x64-gnu.node");
      writeFileSync(currentNode, "BINARY");
      writeFileSync(legacyNode, "BINARY");
      writeFileSync(olderAliasNode, "BINARY");

      // Non-binary artifacts that MUST survive.
      const loader = join(fakeDistDir, "index.js");
      const dts = join(fakeDistDir, "index.d.ts");
      const auditDts = join(fakeDistDir, "audit.d.ts");
      writeFileSync(loader, "/* loader */");
      writeFileSync(dts, "// types");
      writeFileSync(auditDts, "// audit types");

      execFileSync(process.execPath, [join(fakeScriptsDir, "clean-dist.mjs")], { stdio: "pipe" });

      // All .node removed.
      expect(existsSync(currentNode)).toBe(false);
      expect(existsSync(legacyNode)).toBe(false);
      expect(existsSync(olderAliasNode)).toBe(false);

      // Loader + type declarations preserved.
      expect(existsSync(loader)).toBe(true);
      expect(existsSync(dts)).toBe(true);
      expect(existsSync(auditDts)).toBe(true);
    } finally {
      rmSync(scratch, { recursive: true, force: true });
    }
  });
});
