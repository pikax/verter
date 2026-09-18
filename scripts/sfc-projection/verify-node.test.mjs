import assert from "node:assert/strict";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  MANDATORY_CASES,
  assertCleanTwin,
  assertExactType,
  assertInventoryComplete,
  assertNonZeroSelection,
  cloneJson,
  loadNodeManifest,
  loadObligation,
  loadRootManifest,
  parseArgs,
  readJson,
  repoPath,
  resolveEngine,
  selectCases,
  selectEngines,
  selectedCaseIds,
  verifyNode,
} from "./verify-node.mjs";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

function loadProducts() {
  const root = loadRootManifest(REPO_ROOT);
  assert.equal(root.errors.length, 0, JSON.stringify(root.errors));
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP1/manifest.json");
  assert.equal(node.errors.length, 0, JSON.stringify(node.errors));
  return {
    root: root.manifest,
    node: node.manifest,
    inventory: readJson(repoPath(REPO_ROOT, root.manifest.inventory)),
    engineMatrix: readJson(repoPath(REPO_ROOT, root.manifest.engineMatrix)),
    obligation: loadObligation(REPO_ROOT, readJson(repoPath(REPO_ROOT, root.manifest.inventory))),
  };
}

test("parseArgs accepts --node --engine --require-all --json", () => {
  const args = parseArgs(["--node", "STP1", "--engine", "all", "--require-all", "--json"]);
  assert.equal(args.node, "STP1");
  assert.equal(args.engine, "all");
  assert.equal(args.requireAll, true);
  assert.equal(args.json, true);
});

test("STP1-zero-selection: missing --node is rejected", async () => {
  const result = await verifyNode({ repoRoot: REPO_ROOT, node: null });
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "STP1-zero-selection" && error.code === "zero-cases",
    ),
    JSON.stringify(result.errors),
  );
});

test("STP1-zero-selection: node filter selecting zero cases is rejected", async () => {
  const result = await verifyNode({ repoRoot: REPO_ROOT, node: "STP99" });
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "STP1-zero-selection" && error.code === "zero-cases",
    ),
    JSON.stringify(result.errors),
  );
});

test("STP1-zero-selection: absent manifest is rejected", () => {
  const loaded = loadRootManifest(REPO_ROOT, "tests/sfc-projection/missing-manifest.json");
  assert.ok(loaded.errors.some((error) => error.code === "absent-manifest"));
});

test("STP1-zero-selection: empty node cases are rejected", () => {
  const empty = {
    schema: "ProbeManifest",
    version: 1,
    node: "STP1",
    cases: [],
    mandatoryCases: [],
  };
  assert.equal(selectCases(empty, "STP1").length, 0);
  const errors = assertNonZeroSelection([], "STP1");
  assert.ok(errors.some((error) => error.caseId === "STP1-zero-selection"));
});

test("STP1-inventory dirty twin: removing one selected fixture is rejected", () => {
  const products = loadProducts();
  const dirty = cloneJson(products.inventory);
  const removed = dirty.rows.find((row) => row.id === "instancetype-typeof-comp");
  dirty.rows = dirty.rows.filter((row) => row.id !== "instancetype-typeof-comp");
  const errors = assertInventoryComplete(dirty, products.obligation, REPO_ROOT);
  assert.ok(removed);
  assert.ok(
    errors.some(
      (error) =>
        error.caseId === "STP1-inventory" &&
        error.code === "removed-fixture" &&
        String(error.message).includes("instancetype-typeof-comp"),
    ),
    JSON.stringify(errors),
  );
});

test("STP1-inventory clean twin: selected coverage includes every RequiredCurrent row", () => {
  const products = loadProducts();
  const errors = assertInventoryComplete(products.inventory, products.obligation, REPO_ROOT);
  assert.equal(errors.length, 0, JSON.stringify(errors));
  for (const row of products.obligation.rows) {
    assert.ok(
      products.inventory.rows.some((entry) => entry.id === row.id),
      `missing RequiredCurrent ${row.id}`,
    );
  }
});

test("STP1-types dirty twin: any cannot satisfy a type equality", () => {
  const errors = assertExactType({ actual: "any", expected: "number", flags: 1 });
  assert.ok(errors.some((error) => error.caseId === "STP1-types" && error.code === "vacuous-type"));
});

test("STP1-types dirty twin: vacuous never cannot satisfy a type equality", () => {
  const errors = assertExactType({ actual: "never", expected: "Comp", flags: 262144 });
  assert.ok(errors.some((error) => error.caseId === "STP1-types" && error.code === "vacuous-type"));
});

test("STP1-clean-twin dirty twin: unrelated generated error is rejected", () => {
  const errors = assertCleanTwin([{ code: 2304, message: "Cannot find name 'injected'." }]);
  assert.ok(
    errors.some(
      (error) => error.caseId === "STP1-clean-twin" && error.code === "unrelated-generated-error",
    ),
  );
});

test("STP1-provenance dirty twin: a different executable under the same label is rejected", async () => {
  const products = loadProducts();
  const js = products.engineMatrix.engines.find((engine) => engine.id === "ts-js");
  const native = cloneJson(
    products.engineMatrix.engines.find((engine) => engine.id === "ts-native"),
  );
  native.executableOverride = path.join(REPO_ROOT, js.resolveFrom, js.executableRel);
  const resolved = await resolveEngine(native, REPO_ROOT);
  assert.equal(resolved.ok, false);
  assert.equal(resolved.error.caseId, "STP1-provenance");
  assert.equal(resolved.error.code, "substituted-executable");
});

test("STP1-harness: one positive and one anchored negative execute on each admitted engine", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP1",
    engine: "all",
    requireAll: true,
    json: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual([...MANDATORY_CASES].sort(), [...result.mandatoryCases].sort());
  for (const id of MANDATORY_CASES) {
    assert.ok(selectedCaseIds(result).includes(id), `missing selected case ${id}`);
  }
  assert.equal(result.engines.length, 2, JSON.stringify(result.engines));
  const ids = result.engines.map((engine) => engine.id).sort();
  assert.deepEqual(ids, ["ts-js", "ts-native"]);
  for (const engine of result.engines) {
    assert.ok(engine.executable, `${engine.id} missing executable`);
    assert.equal(engine.sha256.length, 64, `${engine.id} missing sha256`);
  }
  assert.equal(result.harnessRuns.length, 2);
  for (const run of result.harnessRuns) {
    assert.equal(run.positiveDiagnostics.length, 0, JSON.stringify(run));
    assert.ok(
      run.negativeDiagnostics.some((diag) => diag.code === 2322),
      JSON.stringify(run.negativeDiagnostics),
    );
    assert.equal(run.hover, "number", JSON.stringify(run));
    assert.ok(run.instanceType && run.instanceType !== "any" && run.instanceType !== "never");
    assert.ok(run.definition);
    assert.ok(run.edits >= 1);
  }
  assert.equal(result.incremental, "fresh");
});

test("engine filter all admits both pins", () => {
  const products = loadProducts();
  const pins = selectEngines(products.engineMatrix, "all");
  assert.deepEqual(
    pins.map((pin) => pin.id),
    ["ts-js", "ts-native"],
  );
});
