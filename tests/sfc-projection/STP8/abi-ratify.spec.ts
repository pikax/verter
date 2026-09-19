import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  DEFAULT_INSTANCE_PRINT,
  DIRTY_FABRICATED_ROW,
  DIRTY_SURFACE,
  DIRTY_TOY_ENGINES,
  DIRTY_UNCITED_ROW,
  assertAbiContamination,
  assertCompleteEvidence,
  assertEvidencePins,
  assertInferenceContract,
  assertRatifiedFixture,
  cloneJson,
  evaluateRejectTwins,
  evaluateStp8,
  loadEngineMatrix,
  loadStp8Product,
  validateStp8Products,
} from "./protocol.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");

const ABI = () => loadStp8Product("accepted-vue-constructor-abi.json");

test("STP8-complete-evidence: every predecessor feasibility row is matched to cited ledger evidence", () => {
  const errors = assertCompleteEvidence(ABI());
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
  const dropped = cloneJson(ABI());
  dropped.feasibilityRows = dropped.feasibilityRows.filter(
    (row) => row.case !== "STP3-order-independent",
  );
  assert.ok(
    assertCompleteEvidence(dropped).some(
      (error) => error.caseId === "STP8-complete-evidence" && error.code === "unmatched-row",
    ),
    JSON.stringify(assertCompleteEvidence(dropped)),
  );
});

test("STP8-complete-evidence dirty twins: fabricated and uncited rows are rejected", () => {
  const fabricated = cloneJson(ABI());
  fabricated.feasibilityRows.push(DIRTY_FABRICATED_ROW);
  assert.ok(
    assertCompleteEvidence(fabricated).some(
      (error) => error.caseId === "STP8-complete-evidence" && error.code === "unknown-row",
    ),
    JSON.stringify(assertCompleteEvidence(fabricated)),
  );
  const uncited = cloneJson(ABI());
  const row = uncited.feasibilityRows.find((entry) => entry.case === DIRTY_UNCITED_ROW.case);
  assert.ok(row, "STP6-package-instance row must exist for the uncited twin");
  row.source = DIRTY_UNCITED_ROW.source;
  assert.ok(
    assertCompleteEvidence(uncited).some(
      (error) => error.caseId === "STP8-complete-evidence" && error.code === "uncited-row",
    ),
    JSON.stringify(assertCompleteEvidence(uncited)),
  );
});

test("STP8-partial-ratify: only the selected real engine and vue pins are admitted", () => {
  const errors = assertEvidencePins(ABI(), loadEngineMatrix());
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
  const toy = cloneJson(ABI());
  toy.engines = DIRTY_TOY_ENGINES;
  const toyErrors = assertEvidencePins(toy, loadEngineMatrix());
  assert.ok(
    toyErrors.some(
      (error) => error.caseId === "STP8-partial-ratify" && error.code === "toy-evidence",
    ),
    JSON.stringify(toyErrors),
  );
  assert.ok(
    toyErrors.some(
      (error) => error.caseId === "STP8-partial-ratify" && error.code === "denominator-shrunk",
    ),
    JSON.stringify(toyErrors),
  );
  const toyVue = cloneJson(ABI());
  toyVue.vue = { package: "vue", version: "3.0.0", source: "package.json" };
  assert.ok(
    assertEvidencePins(toyVue, loadEngineMatrix()).some(
      (error) => error.caseId === "STP8-partial-ratify" && error.code === "toy-vue",
    ),
    JSON.stringify(assertEvidencePins(toyVue, loadEngineMatrix())),
  );
});

test("STP8-abi-contamination: checker-only public shape changes are rejected", () => {
  assert.equal(assertAbiContamination(ABI().publicSurface).length, 0);
  const dirty = assertAbiContamination(DIRTY_SURFACE);
  assert.ok(
    dirty.some(
      (error) => error.caseId === "STP8-abi-contamination" && error.code === "checker-only-shape",
    ),
    JSON.stringify(dirty),
  );
  assert.ok(
    dirty.some(
      (error) => error.caseId === "STP8-abi-contamination" && error.code === "utility-remap",
    ),
    JSON.stringify(dirty),
  );
  const respelled = cloneJson(ABI().publicSurface);
  respelled.instanceTypeSpelling = "CompInstance";
  assert.ok(
    assertAbiContamination(respelled).some((error) => error.code === "instance-type-respelled"),
  );
});

test("STP8-inference-contract: postponed event/slot contributors are rejected", () => {
  assert.equal(
    assertInferenceContract(ABI()).length,
    0,
    JSON.stringify(assertInferenceContract(ABI())),
  );
  const postponedEvent = cloneJson(ABI());
  postponedEvent.inference.postSpecializationOnly.push("emit-change");
  assert.ok(
    assertInferenceContract(postponedEvent).some(
      (error) =>
        error.caseId === "STP8-inference-contract" && error.code === "postponed-contributor",
    ),
    JSON.stringify(assertInferenceContract(postponedEvent)),
  );
  const postponedSlot = cloneJson(ABI());
  postponedSlot.inference.inferenceTransaction =
    postponedSlot.inference.inferenceTransaction.filter((channel) => channel !== "slot-default");
  assert.ok(
    assertInferenceContract(postponedSlot).some((error) => error.code === "postponed-contributor"),
    JSON.stringify(assertInferenceContract(postponedSlot)),
  );
  const splitWitness = cloneJson(ABI());
  splitWitness.inference.selectedWitness = "split-inference-plus-post-specialization";
  assert.ok(assertInferenceContract(splitWitness).some((error) => error.code === "split-witness"));
  const runtimeWitness = cloneJson(ABI());
  runtimeWitness.richerInferenceWitness = {
    ...runtimeWitness.richerInferenceWitness,
    nonRuntime: false,
    secondCheckerAbi: true,
    perCaseFallback: true,
  };
  const runtimeErrors = assertInferenceContract(runtimeWitness);
  assert.ok(runtimeErrors.some((error) => error.code === "runtime-witness"));
  assert.ok(runtimeErrors.some((error) => error.code === "second-checker-abi"));
  assert.ok(runtimeErrors.some((error) => error.code === "per-case-fallback"));
});

test("STP8 fixture: one public constructor, merged witness, no callable default", () => {
  const source = fs.readFileSync(
    path.join(HERE, "probes", "components", "Ratified.vue.d.ts"),
    "utf8",
  );
  assert.equal(
    assertRatifiedFixture(source).length,
    0,
    JSON.stringify(assertRatifiedFixture(source)),
  );
  const callable = assertRatifiedFixture(
    `${source}\nexport default function Comp() {}\n`.replace(
      "export default Comp;",
      "// default replaced",
    ),
  );
  assert.ok(
    callable.some(
      (error) => error.caseId === "STP8-complete-evidence" && error.code === "callable-default",
    ),
    JSON.stringify(callable),
  );
  const overloaded = assertRatifiedFixture(
    source.replace(
      "constructor(props?: RatifiedProps<T>);",
      "constructor(props?: RatifiedProps<T>);\n  constructor(props?: any);",
    ),
  );
  assert.ok(
    overloaded.some(
      (error) => error.caseId === "STP8-complete-evidence" && error.code === "overload-claim",
    ),
    JSON.stringify(overloaded),
  );
});

test("STP8 products, manifest pin, and reject twins all hold together", () => {
  assert.equal(validateStp8Products().length, 0, JSON.stringify(validateStp8Products(), null, 2));
  assert.equal(evaluateRejectTwins().length, 0, JSON.stringify(evaluateRejectTwins(), null, 2));
  const result = evaluateStp8();
  assert.equal(result.errors.length, 0, JSON.stringify(result.errors, null, 2));
  const manifest = JSON.parse(fs.readFileSync(path.join(HERE, "manifest.json"), "utf8"));
  assert.equal(manifest.probes.expectedInstanceType, DEFAULT_INSTANCE_PRINT);
  assert.equal(manifest.probes.expectedHoverType, "number");
  assert.equal(manifest.probes.expectedNegativeCode, 2345);
});

test("STP8 live ts-js: InstanceType prints the merged interface, inference stays coupled", () => {
  const tsPath = path.join(REPO_ROOT, "packages", "playground", "node_modules", "typescript");
  const require = createRequire(path.join(tsPath, "package.json"));
  const ts = require(path.join(tsPath, "lib", "typescript.js"));
  const probesDir = path.join(HERE, "probes");
  const cfg = ts.readConfigFile(path.join(probesDir, "tsconfig.json"), ts.sys.readFile);
  const parsed = ts.parseJsonConfigFileContent(cfg.config, ts.sys, probesDir);
  const host = ts.createCompilerHost(parsed.options, true);
  const positive = path.join(probesDir, "positive.ts");
  const negative = path.join(probesDir, "negative.ts");
  const program = ts.createProgram({
    rootNames: [positive, negative],
    options: parsed.options,
    host,
  });
  const checker = program.getTypeChecker();

  const describe = (diags) =>
    diags.map((diag) => ({
      code: diag.code,
      message: ts.flattenDiagnosticMessageText(diag.messageText, "\n"),
    }));
  const positiveDiags = describe(
    ts
      .getPreEmitDiagnostics(program)
      .filter((diag) => diag.file && path.resolve(diag.file.fileName) === path.resolve(positive)),
  );
  assert.equal(positiveDiags.length, 0, JSON.stringify(positiveDiags, null, 2));

  const negativeDiags = describe(
    ts
      .getPreEmitDiagnostics(program)
      .filter((diag) => diag.file && path.resolve(diag.file.fileName) === path.resolve(negative)),
  );
  assert.ok(
    negativeDiags.every((diag) => diag.code === 2345),
    JSON.stringify(negativeDiags.map((diag) => diag.code)),
  );
  assert.ok(negativeDiags.length > 0);
  const printType = (file, needle) => {
    const sf = program.getSourceFile(file);
    let found = null;
    const visit = (node) => {
      if (ts.isIdentifier(node) && node.text === needle && !found) found = node;
      node.forEachChild(visit);
    };
    visit(sf);
    assert.ok(found, `needle ${needle} missing`);
    return checker.typeToString(checker.getTypeAtLocation(found));
  };
  assert.equal(printType(positive, "Instance"), DEFAULT_INSTANCE_PRINT);
  assert.equal(printType(positive, "inferredFirst"), "string | undefined");
  assert.equal(printType(positive, "explicitFirst"), "number | undefined");
  assert.equal(printType(positive, "stp8HoverTarget"), "number");

  // Definition participation: the pinned needle resolves to a real symbol
  // through the ratified declaration twin.
  const defSf = program.getSourceFile(positive);
  let defNode = null;
  const visitDef = (node) => {
    if (ts.isIdentifier(node) && node.text === "stp8DefinitionTarget" && !defNode) defNode = node;
    node.forEachChild(visitDef);
  };
  visitDef(defSf);
  assert.ok(defNode, "stp8DefinitionTarget missing");
  const symbol = checker.getSymbolAtLocation(defNode);
  assert.equal(symbol?.getName(), "stp8DefinitionTarget");
  assert.equal(checker.typeToString(checker.getTypeAtLocation(defNode)), "typeof Comp");
});
