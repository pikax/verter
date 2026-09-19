import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  DEFAULT_INSTANCE_PRINT,
  DIRTY_ADDED_MANDATORY_CASE,
  DIRTY_FABRICATED_ROW,
  DIRTY_SURFACE,
  DIRTY_TOY_ENGINES,
  DIRTY_UNCITED_ROW,
  assertAbiContamination,
  assertCompleteEvidence,
  assertEvidencePins,
  assertInferenceContract,
  assertPredecessorJoins,
  assertRatifiedFixture,
  cloneJson,
  evaluateRejectTwins,
  evaluateStp8,
  loadEngineMatrix,
  loadInferenceWitnessSelection,
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

test("STP8-complete-evidence: required rows are derived, not self-authored", async () => {
  const { NODE_MANDATORY_CASES } = await import("../../../scripts/sfc-projection/verify-node.mjs");
  // A predecessor ledger gaining a mandatory case must be demanded without any
  // edit to STP8: the required-row denominator comes from the canonical list.
  const added = {
    ...NODE_MANDATORY_CASES,
    STP3: Object.freeze([...NODE_MANDATORY_CASES.STP3, DIRTY_ADDED_MANDATORY_CASE]),
  };
  const errors = assertCompleteEvidence(ABI(), { mandatoryCases: added });
  assert.ok(
    errors.some(
      (error) =>
        error.caseId === "STP8-complete-evidence" &&
        error.code === "unmatched-row" &&
        error.message.includes(`STP3:${DIRTY_ADDED_MANDATORY_CASE}`),
    ),
    JSON.stringify(errors),
  );
  // A charter §4 named predecessor product going missing (here the STP5
  // MapperCapabilityEvidence) fails ratification even though the STP5 case ids
  // live on in the manifest.
  const realRead = (rel) => {
    try {
      return JSON.parse(fs.readFileSync(path.resolve(REPO_ROOT, rel), "utf8"));
    } catch {
      return null;
    }
  };
  const missingMapper = assertCompleteEvidence(ABI(), {
    readJson: (rel) =>
      rel === "tests/sfc-projection/STP5/products/mapper-capability-evidence.json"
        ? null
        : realRead(rel),
  });
  assert.ok(
    missingMapper.some(
      (error) =>
        error.caseId === "STP8-complete-evidence" && error.code === "missing-predecessor-product",
    ),
    JSON.stringify(missingMapper),
  );
  // A predecessor ledger emptied of its cases fails every row citing it.
  const emptiedLedger = assertCompleteEvidence(ABI(), {
    readJson: (rel) => {
      const loaded = realRead(rel);
      if (rel !== "tests/sfc-projection/STP3/products/coupled-inference-evidence.json" || !loaded) {
        return loaded;
      }
      return { ...loaded, cases: [] };
    },
  });
  assert.ok(
    emptiedLedger.some(
      (error) => error.caseId === "STP8-complete-evidence" && error.code === "uncited-row",
    ),
    JSON.stringify(emptiedLedger),
  );
});

test("STP8-complete-evidence: the frozen topology joins its predecessor policy products", () => {
  const topology = loadStp8Product("accepted-topology-matrix.json");
  assert.equal(
    assertPredecessorJoins(topology).length,
    0,
    JSON.stringify(assertPredecessorJoins(topology), null, 2),
  );
  const drifted = cloneJson(topology);
  drifted.supplementalFiles.namedImportTarget = true;
  assert.ok(
    assertPredecessorJoins(drifted).some(
      (error) => error.caseId === "STP8-complete-evidence" && error.code === "predecessor-drift",
    ),
    JSON.stringify(assertPredecessorJoins(drifted)),
  );
  const driftedSpanKinds = cloneJson(topology);
  driftedSpanKinds.editProvenance.spanKinds.alias.notRenameCodec = false;
  assert.ok(
    assertPredecessorJoins(driftedSpanKinds).some((error) => error.code === "predecessor-drift"),
    JSON.stringify(assertPredecessorJoins(driftedSpanKinds)),
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
  // utilitiesUntouched is only measurable while the observing recipe exists:
  // claiming it without createApp/h/TSX and a wrong-prop dirty twin is itself
  // a contamination the case must reject.
  const noProbe = assertAbiContamination(ABI().publicSurface, {
    utilityProbe: { clean: "", dirty: "", exercisesUtilities: false, carriesWrongPropTwin: false },
  });
  assert.ok(
    noProbe.some(
      (error) =>
        error.caseId === "STP8-abi-contamination" && error.code === "utility-probe-missing",
    ),
    JSON.stringify(noProbe),
  );
});

test("STP8-inference-contract: postponed event/slot contributors are rejected", () => {
  assert.equal(
    assertInferenceContract(ABI()).length,
    0,
    JSON.stringify(assertInferenceContract(ABI()), null, 2),
  );
  const postponedEvent = cloneJson(ABI());
  postponedEvent.inference.postSpecializationOnly.push("onChange");
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

test("STP8-inference-contract: the transaction joins the STP3 InferenceWitnessSelection", () => {
  const selection = loadInferenceWitnessSelection();
  assert.equal(selection?.schema, "InferenceWitnessSelection");
  // A parallel catalog (channels the predecessor selection never recorded)
  // must be rejected instead of restated.
  const minusChannel = cloneJson(selection);
  minusChannel.inferenceTransaction = minusChannel.inferenceTransaction.filter(
    (channel) => channel !== "rows",
  );
  assert.ok(
    assertInferenceContract(ABI(), { witnessSelection: minusChannel }).some(
      (error) => error.code === "parallel-catalog",
    ),
    JSON.stringify(assertInferenceContract(ABI(), { witnessSelection: minusChannel })),
  );
  // A predecessor channel dropped from the ABI transaction is postponed.
  const plusChannel = cloneJson(selection);
  plusChannel.inferenceTransaction = [
    ...plusChannel.inferenceTransaction,
    "post-specialization-extra",
  ];
  assert.ok(
    assertInferenceContract(ABI(), { witnessSelection: plusChannel }).some(
      (error) => error.code === "postponed-contributor",
    ),
    JSON.stringify(assertInferenceContract(ABI(), { witnessSelection: plusChannel })),
  );
  // The recorded predecessor witness must stay the selected one.
  const driftedWitness = cloneJson(selection);
  driftedWitness.selectedWitness = "split-inference-plus-post-specialization";
  assert.ok(
    assertInferenceContract(ABI(), { witnessSelection: driftedWitness }).some(
      (error) => error.code === "witness-drift",
    ),
  );
});

test("STP8 fixture: one two-binder public constructor, merged witness, no callable default", () => {
  const source = fs.readFileSync(
    path.join(HERE, "probes", "components", "Ratified.vue.d.ts"),
    "utf8",
  );
  assert.equal(
    assertRatifiedFixture(source).length,
    0,
    JSON.stringify(assertRatifiedFixture(source), null, 2),
  );
  const oneBinder = source.replaceAll("U = unknown", "").replaceAll(", U>", ">");
  assert.ok(
    assertRatifiedFixture(oneBinder).some(
      (error) => error.caseId === "STP8-complete-evidence" && error.code === "binder-family",
    ),
    JSON.stringify(assertRatifiedFixture(oneBinder)),
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
      "constructor(props?: RatifiedProps<T, U>);",
      "constructor(props?: RatifiedProps<T, U>);\n  constructor(props?: any);",
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

test("STP8 live ts-js: coupled T+U inference, merged interface print, Vue utility recipe", () => {
  const tsPath = path.join(REPO_ROOT, "packages", "playground", "node_modules", "typescript");
  const require = createRequire(path.join(tsPath, "package.json"));
  const ts = require(path.join(tsPath, "lib", "typescript.js"));
  const probesDir = path.join(HERE, "probes");
  const cfg = ts.readConfigFile(path.join(probesDir, "tsconfig.json"), ts.sys.readFile);
  const parsed = ts.parseJsonConfigFileContent(cfg.config, ts.sys, probesDir);
  const host = ts.createCompilerHost(parsed.options, true);
  const program = ts.createProgram({
    rootNames: parsed.fileNames,
    options: parsed.options,
    host,
  });
  const checker = program.getTypeChecker();

  const describe = (diags) =>
    diags.map((diag) => ({
      code: diag.code,
      message: ts.flattenDiagnosticMessageText(diag.messageText, "\n"),
    }));
  const fileDiags = (file) =>
    describe(
      ts
        .getPreEmitDiagnostics(program)
        .filter((diag) => diag.file && path.resolve(diag.file.fileName) === path.resolve(file)),
    );
  const positive = path.join(probesDir, "positive.ts");
  const negative = path.join(probesDir, "negative.ts");
  const utilities = path.join(probesDir, "utilities.tsx");
  const utilitiesError = path.join(probesDir, "utilities-error.tsx");

  assert.equal(fileDiags(positive).length, 0, JSON.stringify(fileDiags(positive), null, 2));
  assert.equal(fileDiags(utilities).length, 0, JSON.stringify(fileDiags(utilities), null, 2));

  const negativeDiags = fileDiags(negative);
  assert.ok(
    negativeDiags.every((diag) => diag.code === 2345),
    JSON.stringify(negativeDiags.map((diag) => diag.code)),
  );
  assert.ok(negativeDiags.length > 0);

  // The utility dirty twin must be RED on the selected encoding: h() and TSX
  // observe the ratified constructor's props surface (the wrong `rows` payload
  // misses readonly T[]; the wrong `project` param misses the T the rows fixed).
  const utilityErrors = fileDiags(utilitiesError);
  assert.ok(utilityErrors.length > 0, "wrong-prop utility payloads must be rejected");
  assert.ok(
    utilityErrors.every((diag) => [2322, 2345, 2769].includes(diag.code)),
    JSON.stringify(utilityErrors),
  );
  assert.ok(
    utilityErrors.some((diag) => /readonly unknown\[\]/.test(diag.message)),
    JSON.stringify(utilityErrors),
  );
  assert.ok(
    utilityErrors.some((diag) => /'string' is not assignable to type 'number'/.test(diag.message)),
    JSON.stringify(utilityErrors),
  );

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
  // STP8-inference-contract live proof: T and U infer together from one
  // whole-signature construction of the two-binder family.
  assert.equal(printType(positive, "coupled"), "Comp<string, number>");
  assert.equal(printType(positive, "coupledValue"), "number | undefined");
  assert.equal(printType(positive, "inferredValue"), "unknown");
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

test("STP8 live ts-js: the utility dirty twin discriminates a respelled public surface", () => {
  const tsPath = path.join(REPO_ROOT, "packages", "playground", "node_modules", "typescript");
  const require = createRequire(path.join(tsPath, "package.json"));
  const ts = require(path.join(tsPath, "lib", "typescript.js"));
  const probesDir = path.join(HERE, "probes");
  const cfg = ts.readConfigFile(path.join(probesDir, "tsconfig.json"), ts.sys.readFile);
  const parsed = ts.parseJsonConfigFileContent(cfg.config, ts.sys, probesDir);
  const fixture = path.join(probesDir, "components", "Ratified.vue.d.ts");
  const real = fs.readFileSync(fixture, "utf8");
  // Checker-only respell control: the public spelling shape survives, but the
  // props surface stops observing the authored channels. The wrong-prop twin
  // must go green under this shape — proving that its redness on the real
  // fixture observes the ratified surface and cannot be satisfied by a
  // checker-only projection.
  const respelled = real.replace(
    /type RatifiedProps<T, U> = \{[\s\S]*?\};/,
    "type RatifiedProps<T, U> = Record<string, unknown>;",
  );
  assert.notEqual(respelled, real, "respell control must actually change the fixture");
  const host = ts.createCompilerHost(parsed.options, true);
  const baseReadFile = host.readFile.bind(host);
  host.readFile = (file) =>
    path.resolve(file) === path.resolve(fixture) ? respelled : baseReadFile(file);
  const program = ts.createProgram({
    rootNames: parsed.fileNames,
    options: parsed.options,
    host,
  });
  const utilitiesError = path.join(probesDir, "utilities-error.tsx");
  const diags = ts
    .getPreEmitDiagnostics(program)
    .filter(
      (diag) => diag.file && path.resolve(diag.file.fileName) === path.resolve(utilitiesError),
    );
  assert.equal(
    diags.length,
    0,
    `under a respelled public surface the wrong-prop twin must be green, proving it observes the ratified one: ${diags
      .map((diag) => ts.flattenDiagnosticMessageText(diag.messageText, " "))
      .join(" | ")}`,
  );
});
