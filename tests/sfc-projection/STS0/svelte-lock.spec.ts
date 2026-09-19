import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  DIRTY_ABI_SHAPE,
  DIRTY_INVENTORY_DROP_DIRECTIVE_ROW,
  DIRTY_INVENTORY_DROP_FEATURE,
  DIRTY_INVENTORY_DROP_SELECTED_MAPPING,
  DIRTY_INVENTORY_DROP_SMOKE_CASE,
  DIRTY_INVENTORY_FABRICATED_FEATURE,
  DIRTY_INVENTORY_MISLABELED_FEATURE,
  DIRTY_INVENTORY_MISSING_FIXTURE,
  DIRTY_PIN_DIVERGED_VERSION,
  DIRTY_PIN_FLOATING_VERSION,
  DIRTY_PIN_LATEST_VERSION,
  DIRTY_PIN_UNKNOWN_ENGINE,
  DIRTY_INVENTORY_UNOWNED_SELECTED,
  DIRTY_POLICY_DROP_MODULE,
  DIRTY_POLICY_DROP_PROFILE,
  DIRTY_POLICY_DROP_REFUSAL,
  DIRTY_POLICY_DROP_UNCHECKED_LEGACY,
  DIRTY_POLICY_REFUSED_OPTION,
  DIRTY_POLICY_UNSPECIFIED_PROFILE,
  PINNED_DERIVED_DECLARE,
  REQUIRED_POLICY_PROFILE_IDS,
  REQUIRED_SVELTE_FEATURES,
  SVELTE_BENCHMARKS_ADAPTER,
  assertModeClassification,
  assertProfileBehavior,
  assertRuneProbeMatchesPinnedProjection,
  assertEngineFrameworkPins,
  assertInstanceShapePin,
  assertPolicyLock,
  assertPredecessorJoins,
  assertSvelteAbi,
  assertSvelteInventory,
  assertSvelteShapeSource,
  checkProfileBehavior,
  cloneJson,
  evaluateRejectTwins,
  evaluateSts0,
  loadEngineMatrix,
  loadPinnedTsJs,
  loadRootPackageJson,
  loadSts0Product,
  shippedDirectiveKinds,
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
  const droppedProfile = cloneJson(POLICY());
  droppedProfile.profiles = droppedProfile.profiles.filter(
    (row) => row.id !== DIRTY_POLICY_DROP_PROFILE,
  );
  assert.ok(
    assertPolicyLock(droppedProfile).some(
      (error) => error.caseId === "STS0-policy-lock" && error.code === "missing-policy",
    ),
    JSON.stringify(assertPolicyLock(droppedProfile)),
  );
  const droppedModule = cloneJson(POLICY());
  droppedModule.profiles = droppedModule.profiles.filter(
    (row) => row.id !== DIRTY_POLICY_DROP_MODULE,
  );
  assert.ok(
    assertPolicyLock(droppedModule).some((error) => error.code === "missing-policy"),
    JSON.stringify(assertPolicyLock(droppedModule)),
  );
  const emptiedRefusals = cloneJson(POLICY());
  emptiedRefusals.refusedOptions = [];
  assert.ok(
    assertPolicyLock(emptiedRefusals).some((error) => error.code === "silently-ignored-option"),
    JSON.stringify(assertPolicyLock(emptiedRefusals)),
  );
  const droppedRefusal = cloneJson(POLICY());
  droppedRefusal.refusedOptions = droppedRefusal.refusedOptions.filter(
    (row) => row.option !== DIRTY_POLICY_DROP_REFUSAL,
  );
  assert.ok(
    assertPolicyLock(droppedRefusal).some((error) => error.code === "silently-ignored-option"),
    JSON.stringify(assertPolicyLock(droppedRefusal)),
  );
  for (const id of REQUIRED_POLICY_PROFILE_IDS) {
    assert.ok(
      POLICY().profiles.some((row) => row.id === id),
      `missing required profile ${id}`,
    );
  }
  const droppedUncheckedLegacy = cloneJson(POLICY());
  droppedUncheckedLegacy.profiles = droppedUncheckedLegacy.profiles.filter(
    (row) => row.id !== DIRTY_POLICY_DROP_UNCHECKED_LEGACY,
  );
  assert.ok(
    assertPolicyLock(droppedUncheckedLegacy).some((error) => error.code === "missing-policy"),
    "deleting the unchecked legacy-JS instance profile was not rejected",
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
  const emptiedPopulations = cloneJson(INVENTORY());
  emptiedPopulations.populations = [];
  assert.ok(
    assertSvelteInventory(emptiedPopulations).some((error) => error.code === "removed-fixture"),
    JSON.stringify(assertSvelteInventory(emptiedPopulations)),
  );
  const zeroRows = cloneJson(INVENTORY());
  const official = zeroRows.populations.find((row) => row.id === "svelte-official-cases");
  assert.ok(official, "official population must exist");
  official.recordedRows = 0;
  assert.ok(
    assertSvelteInventory(zeroRows).some((error) => error.code === "population-count-mismatch"),
    JSON.stringify(assertSvelteInventory(zeroRows)),
  );
  const unowned = cloneJson(INVENTORY());
  unowned.selectedMembers = [
    ...(unowned.selectedMembers || []),
    cloneJson(DIRTY_INVENTORY_UNOWNED_SELECTED),
  ];
  assert.ok(
    assertSvelteInventory(unowned).some((error) => error.code === "unowned-feature"),
    JSON.stringify(assertSvelteInventory(unowned)),
  );
});

test("STS0-svelte-inventory: selected membership derives from the population authorities", () => {
  const droppedMapping = cloneJson(INVENTORY());
  droppedMapping.selectedMembers = droppedMapping.selectedMembers.filter(
    (member) => member.id !== DIRTY_INVENTORY_DROP_SELECTED_MAPPING,
  );
  assert.ok(
    assertSvelteInventory(droppedMapping).some(
      (error) =>
        error.caseId === "STS0-svelte-inventory" && error.code === "unselected-population-member",
    ),
    "removing one harness fixture's selected mapping while retaining the fixture was not rejected",
  );
  const shrank = cloneJson(INVENTORY());
  shrank.selectedMembers = shrank.selectedMembers.slice(0, 1);
  assert.ok(
    assertSvelteInventory(shrank).some((error) => error.code === "unselected-population-member"),
    "shrinking the selected members to one supplied row was not rejected",
  );
  const droppedSmoke = cloneJson(INVENTORY());
  droppedSmoke.selectedMembers = droppedSmoke.selectedMembers.filter(
    (member) => member.caseId !== DIRTY_INVENTORY_DROP_SMOKE_CASE,
  );
  assert.ok(
    assertSvelteInventory(droppedSmoke).some(
      (error) => error.code === "unselected-population-member",
    ),
    "dropping a pinned benchmark smoke case member was not rejected",
  );
  const adapterOnly = cloneJson(INVENTORY());
  const benchmarkPopulation = adapterOnly.populations.find(
    (population) => population.id === "svelte-benchmarks",
  );
  assert.ok(benchmarkPopulation, "benchmark population must exist");
  benchmarkPopulation.path = SVELTE_BENCHMARKS_ADAPTER;
  assert.ok(
    assertSvelteInventory(adapterOnly).some(
      (error) => error.code === "population-authority-missing",
    ),
    "citing only the benchmark adapter without the pinned case inventory was not rejected",
  );
  const droppedDirectiveRow = cloneJson(INVENTORY());
  droppedDirectiveRow.rows = droppedDirectiveRow.rows.filter(
    (row) => row.id !== DIRTY_INVENTORY_DROP_DIRECTIVE_ROW,
  );
  assert.ok(
    assertSvelteInventory(droppedDirectiveRow).some((error) => error.code === "unowned-feature"),
    "a shipped directive kind losing its owning row was not rejected",
  );
  const kinds = shippedDirectiveKinds();
  assert.ok(
    kinds.includes("Class") && kinds.includes("Style"),
    "directive projector must expose class/style kinds",
  );
});

test("STS0-svelte-inventory: mode-shared features stay both and legacy-only stays legacy", () => {
  assert.equal(
    assertModeClassification(INVENTORY()).length,
    0,
    JSON.stringify(assertModeClassification(INVENTORY()), null, 2),
  );
  const sharedAsLegacy = cloneJson(INVENTORY());
  const storeRow = sharedAsLegacy.rows.find((row) => row.id === "legacy-store-auto-subscription");
  assert.ok(storeRow, "store row must exist");
  storeRow.semantics = "legacy";
  assert.ok(
    assertModeClassification(sharedAsLegacy).some(
      (error) => error.caseId === "STS0-svelte-inventory" && error.code === "mode-misclassified",
    ),
    "a mode-shared feature mislabeled legacy-only was not rejected",
  );
  const legacyAsBoth = cloneJson(INVENTORY());
  const exportLetRow = legacyAsBoth.rows.find((row) => row.id === "legacy-export-let");
  assert.ok(exportLetRow, "export-let row must exist");
  exportLetRow.semantics = "both";
  assert.ok(
    assertModeClassification(legacyAsBoth).some((error) => error.code === "mode-misclassified"),
    "marking export let mode-shared was not rejected",
  );
  // Historical family naming must not force the legal-mode classification.
  const clean = assertSvelteInventory(INVENTORY());
  assert.equal(clean.length, 0, JSON.stringify(clean, null, 2));
});

test("STS0-policy-lock: claimed checking and publishing behavior executes through the pinned engine", () => {
  const ts = loadPinnedTsJs();
  assert.ok(ts, "the pinned ts-js engine must resolve for live profile behavior");
  const errors = assertProfileBehavior(ts, POLICY());
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
  const runesRow = POLICY().profiles.find((row) => row.id === "svelte-ts-instance-runes");
  const evidence = fs.readFileSync(path.resolve(HERE, "../../../", runesRow.evidencePath), "utf8");
  const typeCorrupted = checkProfileBehavior(ts, runesRow, {
    publishedSymbols: ["count", "doubled"],
    evidenceText: evidence.replace(
      "let count = $state(0);",
      'let count: number = $state("corrupted");',
    ),
  });
  assert.ok(
    typeCorrupted.some((error) => error.code === "profile-behavior-failed"),
    "an invalid projected type in the evidence must fail the live checking path",
  );
  const publicationCorrupted = checkProfileBehavior(ts, runesRow, {
    publishedSymbols: ["count", "doubled"],
    evidenceText: evidence.split("count").join("count__removed"),
  });
  assert.ok(
    publicationCorrupted.length > 0,
    "losing a published declaration must fail the live publication path",
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
  const emptyCells = cloneJson(MATRIX());
  emptyCells.cells = [];
  assert.ok(
    assertEngineFrameworkPins(emptyCells).some((error) => error.code === "missing-cell"),
    JSON.stringify(assertEngineFrameworkPins(emptyCells)),
  );
  const droppedCell = cloneJson(MATRIX());
  droppedCell.cells = droppedCell.cells.filter((cell) => cell.engine !== "ts-native");
  assert.ok(
    assertEngineFrameworkPins(droppedCell).some((error) => error.code === "missing-cell"),
    JSON.stringify(assertEngineFrameworkPins(droppedCell)),
  );
});

test("STS0-svelte-abi: clean rune probe matches the pinned $derived expression signature", () => {
  const probe = fs.readFileSync(path.join(HERE, "probes", "state-module.svelte.ts"), "utf8");
  const prelude = fs.readFileSync(
    path.resolve(HERE, "../../../crates/verter_compiler/src/svelte/ide/prelude.rs"),
    "utf8",
  );
  const svelteTypes = fs.readFileSync(
    path.resolve(HERE, "../../../node_modules/svelte/types/index.d.ts"),
    "utf8",
  );
  assert.equal(
    assertRuneProbeMatchesPinnedProjection(probe, {
      preludeSource: prelude,
      svelteTypesSource: svelteTypes,
    }).length,
    0,
    JSON.stringify(
      assertRuneProbeMatchesPinnedProjection(probe, {
        preludeSource: prelude,
        svelteTypesSource: svelteTypes,
      }),
    ),
  );
  assert.ok(probe.includes(PINNED_DERIVED_DECLARE));
  const negative = fs.readFileSync(path.join(HERE, "probes", "negative.ts"), "utf8");
  assert.ok(negative.includes(PINNED_DERIVED_DECLARE));
  assert.ok(negative.includes("$derived(() =>"));
  const dirty = probe.replace(
    PINNED_DERIVED_DECLARE,
    "declare function $derived<T>(compute: () => T): T",
  );
  assert.ok(
    assertRuneProbeMatchesPinnedProjection(dirty, {
      preludeSource: prelude,
      svelteTypesSource: svelteTypes,
    }).some((error) => error.code === "incorrect-rune-signature"),
  );
});

test("STS0 products validate and every reject twin is rejected", () => {
  assert.equal(validateSts0Products().length, 0, JSON.stringify(validateSts0Products(), null, 2));
  assert.equal(evaluateRejectTwins().length, 0, JSON.stringify(evaluateRejectTwins(), null, 2));
  const sts0 = evaluateSts0();
  assert.equal(sts0.errors.length, 0, JSON.stringify(sts0.errors, null, 2));
});
