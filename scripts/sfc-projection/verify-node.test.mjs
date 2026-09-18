import assert from "node:assert/strict";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  MANDATORY_CASES,
  NODE_MANDATORY_CASES,
  assertCheckCounts,
  assertCleanTwin,
  assertExactType,
  assertFooSyntaxControl,
  assertInventoryComplete,
  assertNonZeroSelection,
  assertStp2Products,
  cloneJson,
  loadNodeManifest,
  loadObligation,
  loadRootManifest,
  parseArgs,
  readJson,
  repoPath,
  resolveDefinitionName,
  resolveEngine,
  selectCases,
  selectEngines,
  selectedCaseIds,
  validateProbeManifest,
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

test("STP1-types dirty twin: non-primitive expected types must match exactly", () => {
  const mismatch = assertExactType({ actual: "number", expected: "Comp" });
  assert.ok(
    mismatch.some((error) => error.caseId === "STP1-types" && error.code === "type-mismatch"),
    JSON.stringify(mismatch),
  );
  const unknown = assertExactType({ actual: "unknown", expected: "Comp" });
  assert.ok(
    unknown.some((error) => error.caseId === "STP1-types" && error.code === "type-mismatch"),
    JSON.stringify(unknown),
  );
  const exact = assertExactType({ actual: "Comp", expected: "Comp" });
  assert.equal(exact.length, 0, JSON.stringify(exact));
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
    assert.equal(run.instanceType, "Comp", JSON.stringify(run));
    assert.ok(run.definition);
    assert.ok(run.references >= 1, JSON.stringify(run));
    assert.ok(run.edits >= 1);
    for (const count of Object.values(run.checkCounts)) {
      assert.equal(count, 1, JSON.stringify(run.checkCounts));
    }
  }
  assert.equal(result.incremental, "fresh");
});

test("STP1-harness: omitted probes is a zero-test pass rejection", async () => {
  const products = loadProducts();
  const node = cloneJson(products.node);
  delete node.probes;
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP1",
    engine: "all",
    requireAll: true,
    nodeManifest: node,
  });
  assert.equal(result.ok, false);
  assert.equal(result.harnessRuns.length, 0);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "STP1-harness" && error.code === "missing-probes",
    ),
    JSON.stringify(result.errors),
  );
});

test("--require-all enforces canonical MANDATORY_CASES, not the manifest list", async () => {
  const products = loadProducts();
  const node = cloneJson(products.node);
  node.mandatoryCases = node.mandatoryCases.filter((id) => id !== "STP1-harness");
  node.cases = node.cases.filter((row) => row.id !== "STP1-harness");
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP1",
    engine: "all",
    requireAll: true,
    skipProbes: true,
    nodeManifest: node,
  });
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) =>
        error.caseId === "STP1-zero-selection" && String(error.message).includes("STP1-harness"),
    ),
    JSON.stringify(result.errors),
  );
});

test("node manifest dropping a canonical mandatory case is rejected", () => {
  const products = loadProducts();
  const node = cloneJson(products.node);
  node.mandatoryCases = node.mandatoryCases.filter((id) => id !== "STP1-harness");
  const errors = validateProbeManifest(node, { role: "node" });
  assert.ok(
    errors.some(
      (error) =>
        error.caseId === "STP1-zero-selection" && String(error.message).includes("STP1-harness"),
    ),
    JSON.stringify(errors),
  );
});

test("duplicate-check guard fires on measured counts above the bound", () => {
  const files = [
    "tests/sfc-projection/STP1/probes/positive.ts",
    "tests/sfc-projection/STP1/probes/negative.ts",
  ];
  assert.equal(assertCheckCounts({ [files[0]]: 1, [files[1]]: 1 }, "ts-js", files).length, 0);
  const duplicate = assertCheckCounts({ [files[0]]: 2, [files[1]]: 1 }, "ts-js", files);
  assert.ok(
    duplicate.some((error) => error.caseId === "STP1-harness" && error.code === "duplicate-check"),
    JSON.stringify(duplicate),
  );
  const missing = assertCheckCounts({}, "ts-js", files);
  assert.ok(
    missing.some((error) => error.caseId === "STP1-harness" && error.code === "duplicate-check"),
    JSON.stringify(missing),
  );
});

test("engine filter all admits both pins", () => {
  const products = loadProducts();
  const pins = selectEngines(products.engineMatrix, "all");
  assert.deepEqual(
    pins.map((pin) => pin.id),
    ["ts-js", "ts-native"],
  );
});

test("STP2 node manifest is schema-valid and lists every mandatory case", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP2/manifest.json");
  assert.equal(node.errors.length, 0, JSON.stringify(node.errors));
  const ids = node.manifest.cases.map((row) => row.id).sort();
  assert.deepEqual(ids, [...NODE_MANDATORY_CASES.STP2].sort());
});

test("STP2 products name every mandatory case and specialization kind", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP2/manifest.json");
  const errors = assertStp2Products(REPO_ROOT, node.manifest);
  assert.equal(errors.length, 0, JSON.stringify(errors));
});

test("STP2 Foo syntax control is required, not optional", () => {
  const present = assertFooSyntaxControl({
    syntaxControl: {
      spelling: "declare class Foo<T = unknown> { constructor(props?: { test: T }) }",
    },
  });
  assert.equal(present.length, 0, JSON.stringify(present));
  const absent = assertFooSyntaxControl({});
  assert.ok(
    absent.some(
      (error) =>
        error.caseId === "STP2-instance-explicit" && error.message.includes("Foo syntax control"),
    ),
    JSON.stringify(absent),
  );
  const wrong = assertFooSyntaxControl({ syntaxControl: { spelling: "declare class Bar" } });
  assert.ok(wrong.length > 0, JSON.stringify(wrong));
});

test("definition observation prefers definitionNeedle over Comp substring", () => {
  const text = `import { type Component } from "vue";\nimport Comp from "./components/Concrete.vue";\nexport const stp2DefinitionTarget: typeof Comp = Comp;\n`;
  assert.equal(
    resolveDefinitionName(text, { definitionNeedle: "stp2DefinitionTarget" }),
    "stp2DefinitionTarget",
  );
  assert.equal(resolveDefinitionName("import Comp from './x'", {}), "Comp");
  assert.equal(resolveDefinitionName("import { type Component } from 'vue'", {}), null);
});

test("STP2-constructor-escape dirty twin is the broad overload, not the typed constructor", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP2/manifest.json");
  const row = node.manifest.cases.find((entry) => entry.id === "STP2-constructor-escape");
  assert.equal(row.disposition, "reject");
  assert.equal(row.expectedCode, 2353);
  assert.match(row.dirtyTwin, /reject-constructor-escape\.ts$/);
});

test("STP2 verify: all mandatory cases run on each admitted engine", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP2",
    engine: "all",
    requireAll: true,
    json: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual([...NODE_MANDATORY_CASES.STP2].sort(), [...result.mandatoryCases].sort());
  for (const id of NODE_MANDATORY_CASES.STP2) {
    assert.ok(selectedCaseIds(result).includes(id), `missing selected case ${id}`);
  }
  assert.equal(result.engines.length, 2, JSON.stringify(result.engines));
  assert.equal(result.harnessRuns.length, 2);
  assert.equal(result.incremental, "fresh");
  for (const run of result.harnessRuns) {
    for (const count of Object.values(run.checkCounts)) {
      assert.equal(count, 1, JSON.stringify(run.checkCounts));
    }
  }
});
