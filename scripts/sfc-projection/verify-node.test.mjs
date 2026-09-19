import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  MANDATORY_CASES,
  NODE_MANDATORY_CASES,
  STP3_MANDATORY_CASES,
  STP4_MANDATORY_CASES,
  STP5_MANDATORY_CASES,
  STP6_MANDATORY_CASES,
  STP7_MANDATORY_CASES,
  STP8_MANDATORY_CASES,
  STP9_MANDATORY_CASES,
  STS0_MANDATORY_CASES,
  assertCheckCounts,
  assertCleanTwin,
  assertExactType,
  assertFooSyntaxControl,
  assertInventoryComplete,
  assertNonZeroSelection,
  assertStp2Products,
  assertStp3Products,
  assertStp4Products,
  assertStp6Products,
  cleanTwinTarget,
  cloneJson,
  harnessProbeFiles,
  isVacuousType,
  loadNodeManifest,
  loadObligation,
  loadRootManifest,
  parseArgs,
  probeMapperHost,
  probesAreRunnable,
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
  const printed = assertExactType({ actual: "never", expected: "Comp" });
  assert.ok(
    printed.some((error) => error.caseId === "STP1-types" && error.code === "vacuous-type"),
  );
  // The flag branch alone: TypeFlags.Never (1 << 17) on a non-never spelling.
  const flagged = assertExactType({ actual: "Comp", expected: "Comp", flags: 1 << 17 });
  assert.ok(
    flagged.some((error) => error.caseId === "STP1-types" && error.code === "vacuous-type"),
    JSON.stringify(flagged),
  );
});

test("STP1-types clean twin: a type parameter is not a vacuous type", () => {
  // TypeFlags.TypeParameter is 1 << 18, adjacent to Never; it must not read as vacuous.
  assert.equal(isVacuousType("T", 1 << 18), false);
  assert.equal(isVacuousType("T", 1 << 17), true);
  assert.equal(isVacuousType("T", 1), true);
  const errors = assertExactType({ actual: "T", expected: "T", flags: 1 << 18 });
  assert.equal(errors.length, 0, JSON.stringify(errors));
});

test("STP1-zero-selection: a missing or malformed product file is a structured rejection", async () => {
  const products = loadProducts();
  const missingInventory = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP1",
    engine: "all",
    rootManifest: { ...products.root, inventory: "tests/sfc-projection/absent-inventory.json" },
  });
  assert.equal(missingInventory.ok, false);
  assert.ok(
    missingInventory.errors.some(
      (error) =>
        error.caseId === "STP1-zero-selection" &&
        error.code === "absent-manifest" &&
        String(error.message).includes("absent-inventory.json"),
    ),
    JSON.stringify(missingInventory.errors),
  );
  const malformed = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "stp1-matrix-")), "m.json");
  fs.writeFileSync(malformed, "{ not json");
  try {
    const malformedMatrix = await verifyNode({
      repoRoot: REPO_ROOT,
      node: "STP1",
      engine: "all",
      rootManifest: { ...products.root, engineMatrix: malformed },
    });
    assert.equal(malformedMatrix.ok, false);
    assert.ok(
      malformedMatrix.errors.some(
        (error) =>
          error.caseId === "STP1-zero-selection" &&
          error.code === "absent-manifest" &&
          String(error.message).startsWith("malformed engine matrix"),
      ),
      JSON.stringify(malformedMatrix.errors),
    );
    assert.equal(malformedMatrix.engines.length, 0);
  } finally {
    fs.rmSync(path.dirname(malformed), { recursive: true, force: true });
  }
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

function posixPath(p) {
  return String(p).split(path.sep).join("/");
}

test("STP1-clean-twin: the configured clean twin is executed, not the positive probe", async () => {
  const products = loadProducts();
  assert.deepEqual(cleanTwinTarget(products.node.probes), {
    rel: "tests/sfc-projection/STP1/probes/positive.ts",
    sharedWith: "positive",
  });
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "stp1-clean-twin-"));
  const twin = path.join(dir, "clean-twin.ts");
  fs.writeFileSync(twin, "export const stray: string = 1;\n");
  try {
    const node = cloneJson(products.node);
    node.probes.cleanTwin = twin;
    assert.deepEqual(cleanTwinTarget(node.probes), { rel: posixPath(twin), sharedWith: null });
    assert.equal(harnessProbeFiles(node.probes).length, 3);
    const result = await verifyNode({
      repoRoot: REPO_ROOT,
      node: "STP1",
      engine: "ts-js",
      nodeManifest: node,
      json: true,
    });
    assert.equal(result.ok, false);
    assert.ok(
      result.errors.some(
        (error) =>
          error.caseId === "STP1-clean-twin" &&
          error.code === "unrelated-generated-error" &&
          String(error.message).includes("2322"),
      ),
      JSON.stringify(result.errors),
    );
    const run = result.harnessRuns[0];
    assert.equal(run.positiveDiagnostics.length, 0, JSON.stringify(run));
    assert.ok(run.cleanTwinDiagnostics.some((diag) => diag.code === 2322));
    assert.equal(run.checkCounts[posixPath(twin)], 1, JSON.stringify(run.checkCounts));
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
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
  assert.equal(result.probesSkipped, false);
  assert.equal(result.qualified, true);
});

test("STP1-harness: a probes-skipped run validates products but never qualifies", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP1",
    engine: "all",
    requireAll: true,
    skipProbes: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.equal(result.harnessRuns.length, 0);
  assert.equal(result.probesSkipped, true);
  assert.equal(result.qualified, false);
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

test("manifest schema probes contract matches the runner and every node manifest", () => {
  const schema = readJson(repoPath(REPO_ROOT, "scripts/sfc-projection/manifest.schema.json"));
  const probesSchema = schema.properties.probes;
  const known = new Set(Object.keys(probesSchema.properties));
  assert.deepEqual([...probesSchema.required].sort(), [
    "cleanTwin",
    "definitionNeedle",
    "expectedHoverType",
    "expectedInstanceType",
    "expectedNegativeCode",
    "hoverNeedle",
    "negative",
    "positive",
    "tsconfig",
  ]);
  const root = loadRootManifest(REPO_ROOT);
  assert.equal(root.errors.length, 0, JSON.stringify(root.errors));
  for (const entry of root.manifest.nodes) {
    const node = loadNodeManifest(REPO_ROOT, entry.manifest);
    const probes = node.manifest?.probes;
    assert.ok(probes && typeof probes === "object", `${entry.id} has no probes`);
    for (const key of probesSchema.required) {
      const expected = probesSchema.properties[key].type;
      assert.equal(
        typeof probes[key],
        expected === "integer" ? "number" : expected,
        `${entry.id} probes.${key}`,
      );
    }
    for (const key of Object.keys(probes)) {
      assert.ok(known.has(key), `${entry.id} probes.${key} is outside the schema`);
    }
    assert.equal(probesAreRunnable(probes), true, `${entry.id} probes are not runnable`);
  }
  // A manifest the schema rejects (empty probes) is one the runner rejects too.
  assert.equal(probesAreRunnable({}), false);
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

test("STP5 node manifest is runnable without STP1 mandatory cases", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP5/manifest.json");
  assert.equal(node.errors.length, 0, JSON.stringify(node.errors));
  for (const id of STP5_MANDATORY_CASES) {
    assert.ok(node.manifest.mandatoryCases.includes(id), `missing ${id}`);
  }
  assert.ok(!node.manifest.mandatoryCases.includes("STP1-harness"));
});

test("STP5 verify: encoding, reject twins, and selected-build capability on both engines", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP5",
    engine: "all",
    requireAll: true,
    json: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual([...STP5_MANDATORY_CASES].sort(), [...result.mandatoryCases].sort());
  for (const id of STP5_MANDATORY_CASES) {
    assert.ok(selectedCaseIds(result).includes(id), `missing selected case ${id}`);
  }
  assert.equal(result.engines.length, 2, JSON.stringify(result.engines));
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
  }
  assert.equal(result.incremental, "fresh");
  const jsInit = result.capabilityRows.find(
    (row) => row.operation === "initialize" && row.engineId === "ts-js",
  );
  const nativeInit = result.capabilityRows.find(
    (row) => row.operation === "initialize" && row.engineId === "ts-native",
  );
  assert.ok(jsInit, JSON.stringify(result.capabilityRows));
  assert.ok(nativeInit, JSON.stringify(result.capabilityRows));
  assert.equal(jsInit.status, "blocking-upstream-defect");
  assert.equal(nativeInit.status, "blocking-upstream-defect");
  assert.match(jsInit.evidence, /ts-js@6\.0\.3 tsc --help/);
  assert.match(nativeInit.evidence, /ts-native@7\.0\.2 tsc --help/);
  assert.equal(
    result.capabilityRows.filter((row) => row.operation === "encoding-negotiation").length,
    2,
  );
});

test("STP5 mapper-host probe uses tsc --help on the selected engine", () => {
  const skipped = probeMapperHost(
    { id: "ts-js", version: "6.0.3", kind: "javascript" },
    { helpTextHasMapperHost: () => true },
  );
  assert.equal(skipped.performed, false);
  assert.match(skipped.evidence, /ts-js@6\.0\.3 mapper-host probe skipped/);
  assert.doesNotMatch(skipped.evidence, /help\/API/);
});

test("STP5 --require-all does not demand STP1 cases", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP5",
    engine: "all",
    requireAll: true,
    skipProbes: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.ok(!selectedCaseIds(result).includes("STP1-harness"));
  for (const id of STP5_MANDATORY_CASES) {
    assert.ok(selectedCaseIds(result).includes(id), `missing ${id}`);
  }
});

test("STP3 node manifest is schema-valid and lists every mandatory case", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP3/manifest.json");
  assert.equal(node.errors.length, 0, JSON.stringify(node.errors));
  const ids = node.manifest.cases.map((row) => row.id).sort();
  assert.deepEqual(ids, [...NODE_MANDATORY_CASES.STP3].sort());
  assert.deepEqual([...STP3_MANDATORY_CASES].sort(), [...NODE_MANDATORY_CASES.STP3].sort());
});

test("STP3 products name every mandatory case, binder, and selected witness", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP3/manifest.json");
  const errors = assertStp3Products(REPO_ROOT, node.manifest);
  assert.equal(errors.length, 0, JSON.stringify(errors));
});

test("STP3-wrong-channel dirty twin is the broad any constructor", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP3/manifest.json");
  const row = node.manifest.cases.find((entry) => entry.id === "STP3-wrong-channel");
  assert.equal(row.disposition, "reject");
  assert.equal(row.expectedCode, 2551);
  assert.match(row.dirtyTwin, /reject-wrong-channel-any\.ts$/);
});

test("STP3-order-independent dirty twin permutes unannotated whole-signature", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP3/manifest.json");
  const row = node.manifest.cases.find((entry) => entry.id === "STP3-order-independent");
  assert.equal(row.disposition, "reject");
  assert.equal(row.expectedCode, 18046);
  assert.match(row.dirtyTwin, /accept-order-independent\.ts$/);
  const dirty = fs.readFileSync(repoPath(REPO_ROOT, row.dirtyTwin), "utf8");
  assert.match(dirty, /new Comp\(\{\s*project:\s*\(row\)\s*=>\s*row\.name,\s*rows:/s);
  assert.doesNotMatch(dirty, /project:\s*\(row:/);
});

test("STP3-ordered-merge dirty twin is last-write-wins, not accumulation", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP3/manifest.json");
  const row = node.manifest.cases.find((entry) => entry.id === "STP3-ordered-merge");
  assert.equal(row.disposition, "reject");
  assert.equal(row.expectedCode, 2322);
  assert.match(row.dirtyTwin, /reject-ordered-merge-lww\.ts$/);
  const clean = fs.readFileSync(repoPath(REPO_ROOT, row.file), "utf8");
  const dirty = fs.readFileSync(repoPath(REPO_ROOT, row.dirtyTwin), "utf8");
  assert.match(clean, /typeof spread/);
  assert.match(clean, /&\s*\{\s*onChange:\s*SecondListener/);
  assert.doesNotMatch(clean, /void spread/);
  assert.match(dirty, /\.\.\.spread/);
  assert.doesNotMatch(dirty, /void spread/);
});

test("STP3 verify: all mandatory cases run on each admitted engine", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP3",
    engine: "all",
    requireAll: true,
    json: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual([...NODE_MANDATORY_CASES.STP3].sort(), [...result.mandatoryCases].sort());
  for (const id of NODE_MANDATORY_CASES.STP3) {
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

test("STP3 --require-all does not demand STP1 cases", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP3",
    engine: "all",
    requireAll: true,
    skipProbes: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.ok(!selectedCaseIds(result).includes("STP1-harness"));
  for (const id of STP3_MANDATORY_CASES) {
    assert.ok(selectedCaseIds(result).includes(id), `missing ${id}`);
  }
});

test("STP4 node manifest is schema-valid and lists every mandatory case", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP4/manifest.json");
  assert.equal(node.errors.length, 0, JSON.stringify(node.errors));
  const ids = node.manifest.cases.map((row) => row.id).sort();
  assert.deepEqual(ids, [...NODE_MANDATORY_CASES.STP4].sort());
  assert.deepEqual([...STP4_MANDATORY_CASES].sort(), [...NODE_MANDATORY_CASES.STP4].sort());
});

test("STP4 products name every mandatory case, dialect, and topology decision", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP4/manifest.json");
  const errors = assertStp4Products(REPO_ROOT, node.manifest);
  assert.equal(errors.length, 0, JSON.stringify(errors));
});

test("STP4 products dirty twins: dialect, checkJs and external-script values are compared", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP4/manifest.json");
  const evidence = readJson(
    repoPath(REPO_ROOT, "tests/sfc-projection/STP4/products/dialect-topology-evidence.json"),
  );
  const inputs = readJson(
    repoPath(
      REPO_ROOT,
      "tests/sfc-projection/STP4/products/projection-topology-decision-inputs.json",
    ),
  );
  assert.equal(assertStp4Products(REPO_ROOT, node.manifest, { evidence, inputs }).length, 0);

  const dialect = cloneJson(evidence);
  dialect.dialects.find((row) => row.id === "tsx").lang = "js";
  const dialectErrors = assertStp4Products(REPO_ROOT, node.manifest, { evidence: dialect, inputs });
  assert.ok(
    dialectErrors.some(
      (error) => error.caseId === "STP4-tsx-authored" && error.code === "dialect-drift",
    ),
    JSON.stringify(dialectErrors),
  );

  const policy = cloneJson(evidence);
  policy.checkJs.find((row) => row.id === "off").policy = "@ts-check";
  const policyErrors = assertStp4Products(REPO_ROOT, node.manifest, { evidence: policy, inputs });
  assert.ok(
    policyErrors.some(
      (error) => error.caseId === "STP4-js-unchecked" && error.code === "policy-drift",
    ),
    JSON.stringify(policyErrors),
  );

  const suppressed = cloneJson(evidence);
  suppressed.checkJs.find((row) => row.id === "on").scriptDiagnostics = "suppressed";
  const suppressedErrors = assertStp4Products(REPO_ROOT, node.manifest, {
    evidence: suppressed,
    inputs,
  });
  assert.ok(
    suppressedErrors.some(
      (error) => error.caseId === "STP4-js-checked" && error.code === "policy-drift",
    ),
    JSON.stringify(suppressedErrors),
  );

  for (const [field, value] of [
    ["ownership", "importer"],
    ["importersDoNotDuplicateBody", false],
  ]) {
    const external = cloneJson(inputs);
    external.externalScripts[field] = value;
    const errors = assertStp4Products(REPO_ROOT, node.manifest, { evidence, inputs: external });
    assert.ok(
      errors.some((error) => error.caseId === "STP4-external-owner"),
      `${field}: ${JSON.stringify(errors)}`,
    );
  }
});

test("STP4-tsx-authored: the angle-assertion fixture is a valid conversion", () => {
  const fixture = fs.readFileSync(
    repoPath(REPO_ROOT, "tests/sfc-projection/STP4/probes/angle-assertion.ts"),
    "utf8",
  );
  assert.match(fixture, /<number>/);
  assert.doesNotMatch(fixture, /<number>"/);
});

test("STP4-supplemental-import dirty twin is the public SFC module", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP4/manifest.json");
  const row = node.manifest.cases.find((entry) => entry.id === "STP4-supplemental-import");
  assert.equal(row.disposition, "reject");
  assert.equal(row.expectedCode, 2306);
  assert.match(row.dirtyTwin, /accept-public-import\.ts$/);
  const dirty = fs.readFileSync(repoPath(REPO_ROOT, row.dirtyTwin), "utf8");
  assert.match(dirty, /from "\.\/components\/JsUnchecked\.vue"/);
  assert.doesNotMatch(dirty, /vue\.__template/);
});

test("STP4-illegal-vue dirty twin is generated TypeScript, not Vue legality", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP4/manifest.json");
  const row = node.manifest.cases.find((entry) => entry.id === "STP4-illegal-vue");
  assert.equal(row.disposition, "reject");
  assert.equal(row.expectedCode, 2307);
  assert.match(row.dirtyTwin, /illegal-setup-src\.generated\.ts$/);
  const dirty = fs.readFileSync(repoPath(REPO_ROOT, row.dirtyTwin), "utf8");
  assert.match(dirty, /export default Comp/);
  assert.doesNotMatch(dirty, /IllegalSetupSrc\.vue/);
});

test("STP4 verify: all mandatory cases run on each admitted engine", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP4",
    engine: "all",
    requireAll: true,
    json: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual([...NODE_MANDATORY_CASES.STP4].sort(), [...result.mandatoryCases].sort());
  for (const id of NODE_MANDATORY_CASES.STP4) {
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

test("STP4 --require-all does not demand STP1 cases", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP4",
    engine: "all",
    requireAll: true,
    skipProbes: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.ok(!selectedCaseIds(result).includes("STP1-harness"));
  for (const id of STP4_MANDATORY_CASES) {
    assert.ok(selectedCaseIds(result).includes(id), `missing ${id}`);
  }
});

test("STP6 node manifest is schema-valid and lists every mandatory case", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP6/manifest.json");
  assert.equal(node.errors.length, 0, JSON.stringify(node.errors));
  const ids = node.manifest.cases.map((row) => row.id).sort();
  assert.deepEqual(ids, [...NODE_MANDATORY_CASES.STP6].sort());
  assert.deepEqual([...STP6_MANDATORY_CASES].sort(), [...NODE_MANDATORY_CASES.STP6].sort());
});

test("STP6 products name every mandatory case, consumption mode, and resolution mode", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP6/manifest.json");
  const errors = assertStp6Products(REPO_ROOT, node.manifest);
  assert.equal(errors.length, 0, JSON.stringify(errors));
});

test("STP6-hidden-metadata dirty twin is the packed public consumer", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP6/manifest.json");
  const row = node.manifest.cases.find((entry) => entry.id === "STP6-hidden-metadata");
  assert.equal(row.disposition, "reject");
  assert.equal(row.expectedCode, 2307);
  assert.match(row.dirtyTwin, /accept-package-instance\.ts$/);
  const dirty = fs.readFileSync(repoPath(REPO_ROOT, row.dirtyTwin), "utf8");
  assert.match(dirty, /from "@stp6\/lib"/);
  assert.doesNotMatch(dirty, /unpublished-meta/);
});

test("STP6-closure dirty twin is a closed packed import", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP6/manifest.json");
  const row = node.manifest.cases.find((entry) => entry.id === "STP6-closure");
  assert.equal(row.disposition, "reject");
  assert.equal(row.expectedCode, 2307);
  assert.match(row.dirtyTwin, /reject-closure-clean\.ts$/);
  const dirty = fs.readFileSync(repoPath(REPO_ROOT, row.dirtyTwin), "utf8");
  assert.match(dirty, /from "@stp6\/lib"/);
  assert.doesNotMatch(dirty, /__virtual/);
});

test("STP6 verify: all mandatory cases run on each admitted engine", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP6",
    engine: "all",
    requireAll: true,
    json: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual([...NODE_MANDATORY_CASES.STP6].sort(), [...result.mandatoryCases].sort());
  for (const id of NODE_MANDATORY_CASES.STP6) {
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

test("STP6 --require-all does not demand STP1 cases", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP6",
    engine: "all",
    requireAll: true,
    skipProbes: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.ok(!selectedCaseIds(result).includes("STP1-harness"));
  for (const id of STP6_MANDATORY_CASES) {
    assert.ok(selectedCaseIds(result).includes(id), `missing ${id}`);
  }
});

test("STP7 node manifest is runnable without STP1 mandatory cases", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP7/manifest.json");
  assert.equal(node.errors.length, 0, JSON.stringify(node.errors));
  for (const id of STP7_MANDATORY_CASES) {
    assert.ok(node.manifest.mandatoryCases.includes(id), `missing ${id}`);
  }
  assert.ok(!node.manifest.mandatoryCases.includes("STP1-harness"));
});

test("STP7 verify: svelte shape, holes, realm, reuse, and scope on both engines", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP7",
    engine: "all",
    requireAll: true,
    json: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual([...STP7_MANDATORY_CASES].sort(), [...result.mandatoryCases].sort());
  for (const id of STP7_MANDATORY_CASES) {
    assert.ok(selectedCaseIds(result).includes(id), `missing selected case ${id}`);
  }
  assert.equal(result.engines.length, 2, JSON.stringify(result.engines));
  assert.equal(result.harnessRuns.length, 2);
  for (const run of result.harnessRuns) {
    assert.equal(run.positiveDiagnostics.length, 0, JSON.stringify(run));
    assert.ok(
      run.negativeDiagnostics.some((diag) => diag.code === 2322),
      JSON.stringify(run.negativeDiagnostics),
    );
    assert.equal(run.hover, "number", JSON.stringify(run));
    assert.equal(run.instanceType, null, JSON.stringify(run));
    assert.ok(run.definition);
    assert.ok(run.references >= 1, JSON.stringify(run));
    assert.ok(run.edits >= 1);
  }
  assert.equal(result.incremental, "fresh");
});

test("STP7 verify: engine selection without a javascript engine is rejected", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP7",
    engine: "ts-native",
    requireAll: true,
  });
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "STP7-realm" && error.code === "missing-js-engine",
    ),
    JSON.stringify(result.errors),
  );
});

test("STP7 --require-all does not demand STP1 cases", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP7",
    engine: "all",
    requireAll: true,
    skipProbes: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.ok(!selectedCaseIds(result).includes("STP1-harness"));
  for (const id of STP7_MANDATORY_CASES) {
    assert.ok(selectedCaseIds(result).includes(id), `missing ${id}`);
  }
});

test("STP8 node manifest is runnable without STP1 mandatory cases", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP8/manifest.json");
  assert.equal(node.errors.length, 0, JSON.stringify(node.errors));
  for (const id of STP8_MANDATORY_CASES) {
    assert.ok(node.manifest.mandatoryCases.includes(id), `missing ${id}`);
  }
  assert.ok(!node.manifest.mandatoryCases.includes("STP1-harness"));
});

test("STP8 verify: ratified ABI and topology on both engines", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP8",
    engine: "all",
    requireAll: true,
    json: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual([...STP8_MANDATORY_CASES].sort(), [...result.mandatoryCases].sort());
  for (const id of STP8_MANDATORY_CASES) {
    assert.ok(selectedCaseIds(result).includes(id), `missing selected case ${id}`);
  }
  assert.equal(result.engines.length, 2, JSON.stringify(result.engines));
  assert.equal(result.harnessRuns.length, 2);
  for (const run of result.harnessRuns) {
    assert.equal(run.positiveDiagnostics.length, 0, JSON.stringify(run));
    assert.ok(
      run.negativeDiagnostics.some((diag) => diag.code === 2345),
      JSON.stringify(run.negativeDiagnostics),
    );
    assert.equal(run.hover, "number", JSON.stringify(run));
    assert.equal(run.instanceType, "Comp<unknown, unknown>", JSON.stringify(run));
    assert.ok(run.definition, JSON.stringify(run));
    assert.ok(run.references >= 1, JSON.stringify(run));
    assert.ok(run.edits >= 1);
  }
  assert.equal(result.incremental, "fresh");
});

test("STP8 --require-all does not demand STP1 cases", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP8",
    engine: "all",
    requireAll: true,
    skipProbes: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.ok(!selectedCaseIds(result).includes("STP1-harness"));
  for (const id of STP8_MANDATORY_CASES) {
    assert.ok(selectedCaseIds(result).includes(id), `missing ${id}`);
  }
});

test("STP9 node manifest is runnable without STP1 mandatory cases", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STP9/manifest.json");
  assert.equal(node.errors.length, 0, JSON.stringify(node.errors));
  for (const id of STP9_MANDATORY_CASES) {
    assert.ok(node.manifest.mandatoryCases.includes(id), `missing ${id}`);
  }
  assert.ok(!node.manifest.mandatoryCases.includes("STP1-harness"));
});

test("STP9 verify: source-backed plan identities on both engines", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP9",
    engine: "all",
    requireAll: true,
    json: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual([...STP9_MANDATORY_CASES].sort(), [...result.mandatoryCases].sort());
  for (const id of STP9_MANDATORY_CASES) {
    assert.ok(selectedCaseIds(result).includes(id), `missing selected case ${id}`);
  }
  assert.equal(result.engines.length, 2, JSON.stringify(result.engines));
  assert.equal(result.harnessRuns.length, 2);
  for (const run of result.harnessRuns) {
    assert.equal(run.positiveDiagnostics.length, 0, JSON.stringify(run));
    assert.ok(
      run.negativeDiagnostics.some((diag) => diag.code === 2345),
      JSON.stringify(run.negativeDiagnostics),
    );
    assert.equal(run.hover, "number", JSON.stringify(run));
    assert.equal(run.instanceType, "Comp<unknown, unknown>", JSON.stringify(run));
    assert.ok(run.definition, JSON.stringify(run));
    assert.ok(run.references >= 1, JSON.stringify(run));
    assert.ok(run.edits >= 1);
  }
  assert.equal(result.incremental, "fresh");
});

test("STP9 --require-all does not demand STP1 cases", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STP9",
    engine: "all",
    requireAll: true,
    skipProbes: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.ok(!selectedCaseIds(result).includes("STP1-harness"));
  for (const id of STP9_MANDATORY_CASES) {
    assert.ok(selectedCaseIds(result).includes(id), `missing ${id}`);
  }
});

test("STS0 node manifest is runnable without STP1 mandatory cases", () => {
  const node = loadNodeManifest(REPO_ROOT, "tests/sfc-projection/STS0/manifest.json");
  assert.equal(node.errors.length, 0, JSON.stringify(node.errors));
  for (const id of STS0_MANDATORY_CASES) {
    assert.ok(node.manifest.mandatoryCases.includes(id), `missing ${id}`);
  }
  assert.ok(!node.manifest.mandatoryCases.includes("STP1-harness"));
});

test("STS0 verify: ratified Svelte profile lock on both engines", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STS0",
    engine: "all",
    requireAll: true,
    json: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual([...STS0_MANDATORY_CASES].sort(), [...result.mandatoryCases].sort());
  for (const id of STS0_MANDATORY_CASES) {
    assert.ok(selectedCaseIds(result).includes(id), `missing selected case ${id}`);
  }
  assert.equal(result.engines.length, 2, JSON.stringify(result.engines));
  assert.equal(result.harnessRuns.length, 2);
  for (const run of result.harnessRuns) {
    assert.equal(run.positiveDiagnostics.length, 0, JSON.stringify(run));
    assert.ok(
      run.negativeDiagnostics.some((diag) => diag.code === 2322),
      JSON.stringify(run.negativeDiagnostics),
    );
    assert.equal(run.hover, "number", JSON.stringify(run));
    // The Vue-constructor-shaped harness Instance check does not apply to the
    // function-shaped Svelte Component; the manifest pin is protocol-owned.
    assert.equal(run.instanceType, null, JSON.stringify(run));
    assert.ok(run.definition, JSON.stringify(run));
    assert.ok(run.references >= 1, JSON.stringify(run));
    assert.ok(run.edits >= 1);
  }
  assert.equal(result.incremental, "fresh");
});

test("STS0 --require-all does not demand STP1 cases", async () => {
  const result = await verifyNode({
    repoRoot: REPO_ROOT,
    node: "STS0",
    engine: "all",
    requireAll: true,
    skipProbes: true,
  });
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.ok(!selectedCaseIds(result).includes("STP1-harness"));
  for (const id of STS0_MANDATORY_CASES) {
    assert.ok(selectedCaseIds(result).includes(id), `missing ${id}`);
  }
});
