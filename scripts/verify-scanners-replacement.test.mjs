import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  verifyLedger,
  verifyNoLegacyExtensionAuthority,
  verifySchemaAndGuardInventory,
} from "./verify-scanners-replacement.mjs";

test("repository scanner-replacement metadata is internally total", () => {
  verifyLedger();
  verifySchemaAndGuardInventory();
});

test("legacy extension reference guard rejects a package regression", () => {
  const root = mkdtempSync(join(tmpdir(), "verter-b4-"));
  try {
    mkdirSync(join(root, "packages/playground/scripts"), { recursive: true });
    mkdirSync(join(root, "packages/playground/src/editor"), { recursive: true });
    writeFileSync(join(root, "package.json"), '{"scripts":{"package":"cd extensions/vscode"}}');
    writeFileSync(
      join(root, "packages/playground/scripts/generate-vue-language.ts"),
      "packages/vue-vscode",
    );
    writeFileSync(
      join(root, "packages/playground/src/editor/vueLanguage.ts"),
      "packages/vue-vscode",
    );
    assert.throws(
      () => verifyNoLegacyExtensionAuthority(root, ["package.json"]),
      /package.json references extensions\/vscode/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
