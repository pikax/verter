import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  DIRTY_ABI_SHAPE,
  DIRTY_INVENTORY_DROP_FEATURE,
  DIRTY_INVENTORY_FABRICATED_FEATURE,
  DIRTY_INVENTORY_MISLABELED_FEATURE,
  DIRTY_INVENTORY_MISSING_FIXTURE,
  DIRTY_PIN_DIVERGED_VERSION,
  DIRTY_PIN_FLOATING_VERSION,
  DIRTY_PIN_LATEST_VERSION,
  DIRTY_PIN_UNKNOWN_ENGINE,
  DIRTY_POLICY_REFUSED_OPTION,
  DIRTY_POLICY_UNSPECIFIED_PROFILE,
  REQUIRED_SVELTE_FEATURES,
  assertEngineFrameworkPins,
  assertInstanceShapePin,
  assertPolicyLock,
  assertPredecessorJoins,
  assertSvelteAbi,
  assertSvelteInventory,
  assertSvelteShapeSource,
  cloneJson,
  evaluateRejectTwins,
  evaluateSts0,
  loadEngineMatrix,
  loadRootPackageJson,
  loadSts0Product,
  validateSts0Products,
} from "./protocol.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));

const POLICY = () => loadSts0Product("svelte-projection-policy.json");
const INVENTORY = () => loadSts0Product("svelte-current-feature-inventory.json");
const MATRIX = () => loadSts0Product("svelte-engine-framework-matrix.json");

test("STS0-policy-lock: every supported profile has explicit checking and publishing behavior", () => {
  const errors = assertPolicyLock(POLICY());
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
  const unspecified = cloneJson(POLICY());
  unspecified.profiles.push(cloneJson(DIRTY_POLICY_UNSPECIFIED_PROFILE));
  assert.ok(
    assertPolicyLock(unspecified).some(
      (error) => error.caseId === "STS0-policy-lock" && error.code === "unspecified-behavior",
    ),
    JSON.stringify(assertPolicyLock(unspecified)),
  );
  const inventedRefusal = cloneJson(POLICY());
  inventedRefusal.refusedOptions.push(cloneJson(DIRTY_POLICY_REFUSED_OPTION));
  assert.ok(
    assertPolicyLock(inventedRefusal).some(
      (error) => error.caseId === "STS0-policy-lock" && error.code === "silently-ignored-option",
    ),
    JSON.stringify(assertPolicyLock(inventedRefusal)),
  );
});

test("STS0-policy-lock: runes selection and refusals join the official options population", () => {
  const tsvText = fs.readFileSync(
    path.resolve(
      HERE,
      "../../../packages/framework-conformance-harness/evidence/svelte-options.tsv",
    ),
    "utf8",
  );
  assert.ok(tsvText.includes("svelte:CompileOptions\trunes\tsupported canonical"));
  for (const refused of POLICY().refusedOptions) {
    assert.ok(
      tsvText.includes(`${refused.surface}\t${refused.option}\tunsupported fail-closed`),
      `${refused.surface}/${refused.option} must be classified unsupported fail-closed`,
    );
  }
});

test("STS0-svelte-inventory: every canonical feature has a mandatory owning row", () => {
  const errors = assertSvelteInventory(INVENTORY());
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
  const dropped = cloneJson(INVENTORY());
  dropped.rows = dropped.rows.filter((row) => row.id !== DIRTY_INVENTORY_DROP_FEATURE);
  assert.ok(
    assertSvelteInventory(dropped).some(
      (error) => error.caseId === "STS0-svelte-inventory" && error.code === "unowned-feature",
    ),
    JSON.stringify(assertSvelteInventory(dropped)),
  );
  const fabricated = cloneJson(INVENTORY());
  fabricated.rows.push({
    id: DIRTY_INVENTORY_FABRICATED_FEATURE,
    family: "runes",
    semantics: "runes",
    requiredCurrent: true,
    path: "crates/verter_compiler/src/svelte/ide/prelude.rs",
    surface: "verter-prelude",
  });
  assert.ok(
    assertSvelteInventory(fabricated).some(
      (error) => error.caseId === "STS0-svelte-inventory" && error.code === "unknown-row",
    ),
    JSON.stringify(assertSvelteInventory(fabricated)),
  );
  // A feature gaining canon demands its row without any edit to the validator.
  const gained = assertSvelteInventory(INVENTORY(), {
    required: [...REQUIRED_SVELTE_FEATURES, "rune-freshly-canonical"],
  });
  assert.ok(
    gained.some((error) => error.code === "unowned-feature"),
    JSON.stringify(gained),
  );
});

test("STS0-svelte-inventory: runes versus legacy semantics is carried per row", () => {
  const mislabeled = cloneJson(INVENTORY());
  const row = mislabeled.rows.find((entry) => entry.id === DIRTY_INVENTORY_MISLABELED_FEATURE);
  assert.ok(row, "rune-state row must exist for the mislabel twin");
  row.semantics = "legacy";
  assert.ok(
    assertSvelteInventory(mislabeled).some(
      (error) => error.caseId === "STS0-svelte-inventory" && error.code === "runes-mislabeled",
    ),
    JSON.stringify(assertSvelteInventory(mislabeled)),
  );
  const missingFixture = cloneJson(INVENTORY());
  const fixtureRow = missingFixture.rows.find((entry) => entry.id === "template-if-else");
  assert.ok(fixtureRow, "template-if-else row must exist for the fixture twin");
  fixtureRow.path = DIRTY_INVENTORY_MISSING_FIXTURE;
  assert.ok(
    assertSvelteInventory(missingFixture).some(
      (error) => error.caseId === "STS0-svelte-inventory" && error.code === "removed-fixture",
    ),
    JSON.stringify(assertSvelteInventory(missingFixture)),
  );
});

test("STS0-svelte-abi: a Vue constructor is not required for a modern Svelte Component", () => {
  const errors = assertSvelteAbi(POLICY().componentShape);
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
  assert.ok(
    assertSvelteAbi(DIRTY_ABI_SHAPE).some(
      (error) => error.caseId === "STS0-svelte-abi" && error.code === "vue-constructor-required",
    ),
    JSON.stringify(assertSvelteAbi(DIRTY_ABI_SHAPE)),
  );
  const positive = fs.readFileSync(path.join(HERE, "probes", "positive.ts"), "utf8");
  assert.equal(
    assertSvelteShapeSource(positive).length,
    0,
    JSON.stringify(assertSvelteShapeSource(positive)),
  );
  const twin = fs.readFileSync(path.join(HERE, "probes", "vue-constructor-twin.ts"), "utf8");
  assert.ok(
    assertSvelteShapeSource(twin).some((error) => error.code === "vue-constructor-shim"),
    "the vue-constructor dirty twin source must be rejected on sight",
  );
  const manifest = JSON.parse(fs.readFileSync(path.join(HERE, "manifest.json"), "utf8"));
  assert.equal(
    assertInstanceShapePin(positive, manifest.probes.expectedInstanceType).length,
    0,
    "manifest expectedInstanceType must pin the declared component interface",
  );
});

test("STS0-svelte-abi: predecessor joins keep the STP7 boundary and the CCA1I backend", () => {
  const errors = assertPredecessorJoins();
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
});

test("STS0-svelte-pin: latest-tool claims without pinned provenance are rejected", () => {
  const errors = assertEngineFrameworkPins(MATRIX(), {
    engineMatrix: loadEngineMatrix(),
    packageJson: loadRootPackageJson(),
  });
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
  const latest = cloneJson(MATRIX());
  latest.framework.version = DIRTY_PIN_LATEST_VERSION;
  latest.frameworkProvenance.version = DIRTY_PIN_LATEST_VERSION;
  assert.ok(
    assertEngineFrameworkPins(latest).some(
      (error) => error.caseId === "STS0-svelte-pin" && error.code === "latest-tool-claim",
    ),
    JSON.stringify(assertEngineFrameworkPins(latest)),
  );
  const floating = cloneJson(MATRIX());
  floating.framework.version = DIRTY_PIN_FLOATING_VERSION;
  assert.ok(
    assertEngineFrameworkPins(floating).some((error) => error.code === "latest-tool-claim"),
    JSON.stringify(assertEngineFrameworkPins(floating)),
  );
  const diverged = cloneJson(MATRIX());
  diverged.framework.version = DIRTY_PIN_DIVERGED_VERSION;
  assert.ok(
    assertEngineFrameworkPins(diverged).some((error) => error.code === "framework-pin-diverged"),
    JSON.stringify(assertEngineFrameworkPins(diverged)),
  );
  const shrunk = cloneJson(MATRIX());
  shrunk.engines = shrunk.engines.filter((engine) => engine.id !== "ts-native");
  shrunk.cells = shrunk.cells.filter((cell) => cell.engine !== "ts-native");
  assert.ok(
    assertEngineFrameworkPins(shrunk).some((error) => error.code === "denominator-shrunk"),
    JSON.stringify(assertEngineFrameworkPins(shrunk)),
  );
  const unknownEngine = cloneJson(MATRIX());
  unknownEngine.engines.push(cloneJson(DIRTY_PIN_UNKNOWN_ENGINE));
  assert.ok(
    assertEngineFrameworkPins(unknownEngine).some((error) => error.code === "unpinned-engine"),
    JSON.stringify(assertEngineFrameworkPins(unknownEngine)),
  );
});

test("STS0 products validate and every reject twin is rejected", () => {
  assert.equal(validateSts0Products().length, 0, JSON.stringify(validateSts0Products(), null, 2));
  assert.equal(evaluateRejectTwins().length, 0, JSON.stringify(evaluateRejectTwins(), null, 2));
  const sts0 = evaluateSts0();
  assert.equal(sts0.errors.length, 0, JSON.stringify(sts0.errors, null, 2));
});
