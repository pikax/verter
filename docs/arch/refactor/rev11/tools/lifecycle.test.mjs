/**
 * @ai-generated - Executable lifecycle and fail-closed transition tests.
 */
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import * as lib from "./lib.mjs";

function withPackageCopy(run) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "rev11 lifecycle space "));
  const packageRoot = path.join(directory, "authority package");
  try {
    fs.cpSync(lib.PACKAGE_ROOT, packageRoot, { recursive: true });
    return run(packageRoot, directory);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
}

test("the executable boundary exposes receipt/auth imports and atomic activation", () => {
  assert.equal(typeof lib.importAcceptanceReceipt, "function");
  assert.equal(typeof lib.importRuntimeArtifact, "function");
  assert.equal(typeof lib.activateProgram, "function");
});

test("a partial root-only ACTIVE flip is refused", () => {
  withPackageCopy((packageRoot) => {
    const rootFile = path.join(packageRoot, "authority/root.toml");
    fs.writeFileSync(rootFile, fs.readFileSync(rootFile, "utf8").replace('state = "DORMANT"', 'state = "ACTIVE"'));
    const errors = lib.validateAuthority(lib.loadAuthority(packageRoot), { checkGenerated: false, checkAmendments: false });
    assert.match(errors.join("\n"), /activation\/root package state mismatch|partial activation/i);
  });
});

test("the control digest is stable across exact lifecycle-only bindings", () => {
  withPackageCopy((packageRoot) => {
    const before = lib.computeAuthorityDigest(packageRoot);
    const rootFile = path.join(packageRoot, "authority/root.toml");
    const activationFile = path.join(packageRoot, "authority/state/activation.toml");
    fs.writeFileSync(rootFile, fs.readFileSync(rootFile, "utf8").replace('state = "DORMANT"', 'state = "ACTIVE"'));
    let activation = fs.readFileSync(activationFile, "utf8")
      .replace('package_state = "DORMANT"', 'package_state = "ACTIVE"')
      .replace('j1_state = "IN_FLIGHT"', 'j1_state = "LANDED_GRANDFATHERED"')
      .replace('j1_receipt = ""', `j1_receipt = "J1-LANDED-GRANDFATHERED:${"1".repeat(64)}"`)
      .replace('orc0_receipt = ""', `orc0_receipt = "ORC0:${"2".repeat(64)}"`)
      .replace('activation_authorization = ""', `activation_authorization = "maintainer_unified_v2_activation:${"3".repeat(64)}"`)
      .replace('active_authority_sha256 = ""', `active_authority_sha256 = "${before}"`)
      .replace('activation_transition = ""', `activation_transition = "ACT-TEST:${"4".repeat(64)}"`);
    fs.writeFileSync(activationFile, activation);
    assert.equal(lib.computeAuthorityDigest(packageRoot), before);
  });
});
