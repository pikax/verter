import assert from "node:assert/strict";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  runFocusedRustGuards,
  terminalSummary,
  verifyLedger,
  verifyNoLegacyExtensionAuthority,
  verifySchemaCohort,
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
  assert.ok(verifyLedger() > 0, "ledger phase must report the rows it validated");
  assert.ok(verifySchemaCohort() > 0, "schema phase must report the fields it validated");
});

/// Fake child-process runner: every command "passes" with the given cargo
/// stdout, so each control below isolates exactly one execution-proof rail.
function fakeExec(stdout, status = 0) {
  return () => ({ status, stdout, stderr: "" });
}

test("rust guard phase fails on a zero-selection child instead of silently passing", () => {
  // GREEN control: a passing child with real selected work yields receipts.
  const receipts = runFocusedRustGuards(
    ".",
    fakeExec("running 3 tests\ntest result: ok. 3 passed; 0 failed; 0 ignored\n"),
  );
  assert.ok(receipts.length > 0);
  assert.ok(receipts.every((r) => r.exit_code === 0 && r.tests_passed === 3));

  // RED control: exit 0 with ZERO selected tests is a silent skip, not a pass.
  assert.throws(
    () =>
      runFocusedRustGuards(".", fakeExec("running 0 tests\ntest result: ok. 0 passed; 0 failed\n")),
    /ZERO-SELECTION/,
  );
  // RED control: no cargo summary line at all is likewise zero proven work,
  // but distinguishably so — this is "never ran" rather than "ran and matched
  // nothing", so it must fail on the no-parseable-summary message instead.
  assert.throws(
    () => runFocusedRustGuards(".", fakeExec("compiled fine, no tests emitted\n")),
    /cannot prove it ran/,
  );
});

test("rust guard phase captures and fails on a non-zero child exit code", () => {
  assert.throws(
    () => runFocusedRustGuards(".", fakeExec("test result: FAILED. 2 passed; 1 failed\n", 101)),
    /exit 101/,
  );
});

test("terminal summary refuses a missing or zero-work phase", () => {
  const completePhases = {
    inputs: { selected: 4 },
    prerequisites: { selected: 5 },
    "extension-authority": { selected: 3 },
    "scanner-free-boundary": { selected: 5 },
    ledger: { selected: 47 },
    "schema-cohort": { selected: 8 },
    "rust-guards": { selected: 21 },
  };
  const tip = "0123456789abcdef0123456789abcdef01234567";
  // GREEN control: the complete input-bound summary is accepted.
  const summary = terminalSummary({ profile: "b4", tip, phases: completePhases });
  assert.equal(summary.tip, tip);

  // RED control: a silently dropped phase must fail, never summarize as clean.
  const { "rust-guards": _dropped, ...missingPhase } = completePhases;
  assert.throws(
    () => terminalSummary({ profile: "b4", tip, phases: missingPhase }),
    /missing phase rust-guards/,
  );
  // RED control: a phase that recorded zero work must fail at the summary too.
  assert.throws(
    () =>
      terminalSummary({
        profile: "b4",
        tip,
        phases: { ...completePhases, ledger: { selected: 0 } },
      }),
    /missing phase ledger|zero work/,
  );
  // RED control: an unbound summary (no tip sha) is a receipt of nothing.
  assert.throws(() => terminalSummary({ profile: "b4", tip: "", phases: completePhases }), /tip/);
});

test("legacy extension reference guard rejects a package regression", () => {
  const root = mkdtempSync(join(tmpdir(), "verter-b4-"));
  try {
    const files = writeHealthyTree(root);
    // CONTROL: the healthy tree passes — a planted failure below therefore
    // discriminates the plant, not fixture noise.
    verifyNoLegacyExtensionAuthority(root, files);

    const plantedReference = "extensions/vscode";
    assert.equal(
      readFileSync(join(root, "package.json"), "utf8").includes(plantedReference),
      false,
      "package reference plant must be new",
    );
    writeFileSync(join(root, "package.json"), `{"scripts":{"package":"cd ${plantedReference}"}}`);
    const plantedSource = readFileSync(join(root, "package.json"), "utf8");
    assert.equal(plantedSource.includes(plantedReference), true, "package reference plant applied");
    assert.equal(
      plantedSource.split(plantedReference).length - 1,
      1,
      "package reference plant must be unique",
    );
    assert.throws(
      () => verifyNoLegacyExtensionAuthority(root, files),
      /package.json references extensions\/vscode/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("legacy extension generator provenance guard rejects each planted regression", () => {
  const root = mkdtempSync(join(tmpdir(), "verter-b4-"));
  try {
    const files = writeHealthyTree(root);
    verifyNoLegacyExtensionAuthority(root, files);

    for (const relative of [
      "packages/playground/scripts/generate-vue-language.ts",
      "packages/playground/src/editor/vueLanguage.ts",
    ]) {
      const absolute = join(root, relative);
      const healthy = readFileSync(absolute, "utf8");
      const plantedReference = "extensions/vue-vscode";
      assert.equal(healthy.includes(plantedReference), false, `${relative} plant must be new`);
      writeFileSync(absolute, plantedReference);
      const planted = readFileSync(absolute, "utf8");
      assert.equal(planted.includes(plantedReference), true, `${relative} plant applied`);
      assert.equal(
        planted.split(plantedReference).length - 1,
        1,
        `${relative} plant must be unique`,
      );
      assert.throws(
        () => verifyNoLegacyExtensionAuthority(root, files),
        new RegExp(
          `${relative.replaceAll("/", "\\/")} (lacks the current grammar authority|references extensions\\/vue-vscode)`,
        ),
      );
      writeFileSync(absolute, healthy);
      verifyNoLegacyExtensionAuthority(root, files);
    }
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
