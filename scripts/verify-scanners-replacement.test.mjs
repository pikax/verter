import assert from "node:assert/strict";
import { existsSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  verifyLedger,
  verifyNoLegacyExtensionAuthority,
  verifySchemaAndGuardInventory,
} from "./verify-scanners-replacement.mjs";

/// Minimal healthy fixture tree: the playground grammar files plus a clean
/// package.json. Returns the tracked-file inventory the verifier is fed.
function writeHealthyTree(root) {
  mkdirSync(join(root, "packages/playground/scripts"), { recursive: true });
  mkdirSync(join(root, "packages/playground/src/editor"), { recursive: true });
  writeFileSync(join(root, "package.json"), '{"scripts":{"build":"tsc -b"}}');
  writeFileSync(
    join(root, "packages/playground/scripts/generate-vue-language.ts"),
    "packages/vue-vscode",
  );
  writeFileSync(join(root, "packages/playground/src/editor/vueLanguage.ts"), "packages/vue-vscode");
  return [
    "package.json",
    "packages/playground/scripts/generate-vue-language.ts",
    "packages/playground/src/editor/vueLanguage.ts",
  ];
}

test("repository scanner-replacement metadata is internally total", () => {
  verifyLedger();
  verifySchemaAndGuardInventory();
});

test("legacy extension reference guard rejects a package regression", () => {
  const root = mkdtempSync(join(tmpdir(), "verter-b4-"));
  try {
    const files = writeHealthyTree(root);
    // CONTROL: the healthy tree passes — a planted failure below therefore
    // discriminates the plant, not fixture noise.
    verifyNoLegacyExtensionAuthority(root, files);

    writeFileSync(join(root, "package.json"), '{"scripts":{"package":"cd extensions/vscode"}}');
    assert.throws(
      () => verifyNoLegacyExtensionAuthority(root, files),
      /package.json references extensions\/vscode/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("legacy extension residue guard fires on planted physical and tracked regressions", () => {
  const root = mkdtempSync(join(tmpdir(), "verter-b4-"));
  try {
    const files = writeHealthyTree(root);
    // CONTROL: the healthy tree passes, so each planted failure below is
    // attributable to its plant alone.
    verifyNoLegacyExtensionAuthority(root, files);

    // PHYSICAL plant: a retired extension tree reappears on disk. Prove the
    // plant is NEW (absent before), then APPLIED (present after), before
    // trusting the run.
    const plantedDir = join(root, "extensions/typescript-plugin");
    assert.equal(existsSync(plantedDir), false, "physical plant must be new");
    mkdirSync(plantedDir, { recursive: true });
    writeFileSync(join(plantedDir, "index.js"), "// planted B-70 regression");
    assert.equal(existsSync(join(plantedDir, "index.js")), true, "physical plant must apply");
    assert.throws(
      () => verifyNoLegacyExtensionAuthority(root, files),
      /extensions\/typescript-plugin still exists/,
    );
    // Remove the plant and prove the tree returns to green — the failure was
    // the plant, not a latent fixture defect.
    rmSync(join(root, "extensions"), { recursive: true, force: true });
    assert.equal(existsSync(plantedDir), false, "physical plant must be removed");
    verifyNoLegacyExtensionAuthority(root, files);

    // TRACKED plant (retired tree): the tracked inventory itself carries the
    // regression; nothing exists on disk, so only the tracked arm can fire.
    const trackedPath = "extensions/typescript-plugin/index.js";
    assert.equal(files.includes(trackedPath), false, "tracked plant must be new");
    const trackedPlant = [...files, trackedPath];
    assert.equal(
      trackedPlant.filter((path) => path === trackedPath).length,
      1,
      "tracked plant must be unique",
    );
    assert.throws(
      () => verifyNoLegacyExtensionAuthority(root, trackedPlant),
      /extensions\/typescript-plugin still has tracked files/,
    );

    // TRACKED plant (unknown extensions tree): residue OUTSIDE the retired
    // list still fails — single-extension authority is allowlist-shaped, not
    // a name-list of known offenders.
    const strayPath = "extensions/legacy-shim/main.js";
    assert.equal(files.includes(strayPath), false, "stray plant must be new");
    assert.throws(
      () => verifyNoLegacyExtensionAuthority(root, [...files, strayPath]),
      /outside the live allowlist: extensions\/legacy-shim\/main\.js/,
    );

    // Live allowlist member stays accepted.
    verifyNoLegacyExtensionAuthority(root, [...files, "extensions/lapce/src/lib.rs"]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
