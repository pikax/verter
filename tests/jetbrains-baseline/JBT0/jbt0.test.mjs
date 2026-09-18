import assert from "node:assert/strict";
import test from "node:test";

import {
  cloneProducts,
  loadContracts,
  loadProducts,
  loadRepositoryFacts,
  mandatoryCases,
  selectedCaseIds,
  validate,
} from "./verify.mjs";
import { loadAuthority } from "../../../roadmap/0.1.0-tama/tools/lib.mjs";

const authority = loadAuthority();
const facts = loadRepositoryFacts();
const clean = loadProducts();
const contracts = loadContracts();

function defaultComparator(products) {
  const row = products.manifest.comparators.find((candidate) => candidate.alternate === false);
  assert.ok(row, "default comparator must exist");
  return row;
}

test("JBT0-ratification: clean products validate against the live repository", () => {
  const result = validate(clean, contracts, authority, facts);
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual(mandatoryCases().sort(), [
    "JBT0-AC1",
    "JBT0-AC2",
    "JBT0-AC3",
    "JBT0-ratification",
  ]);
  assert.ok(selectedCaseIds({ errors: [] }).length === 0);
});

test("JBT0-AC1: a comparator with diagnostics disabled is invalid", () => {
  const dirty = cloneProducts(clean);
  defaultComparator(dirty).settings.diagnosticsEnabled = false;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC1" && error.code === "comparator-diagnostics-disabled",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC1: an optional diagnostics attestation is invalid", () => {
  const dirty = cloneProducts(clean);
  defaultComparator(dirty).settings.attestation.required = false;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC1" && error.code === "comparator-attestation-optional",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC1: a captured pin without a value is rejected", () => {
  const dirty = cloneProducts(clean);
  defaultComparator(dirty).build.captureState = "captured";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC1" && error.code === "captured-pin-without-value",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC1: a pending pin that already carries a value is rejected", () => {
  const dirty = cloneProducts(clean);
  defaultComparator(dirty).build.value = "guessed-2026.1";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC1" && error.code === "pending-pin-with-value",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC1: dropping the recorded alternate service-powered mode is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.manifest.comparators = dirty.manifest.comparators.filter(
    (candidate) => candidate.alternate !== true,
  );
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC1" && error.code === "missing-alternate-mode-row",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC1: a second semantic engine inside the client is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.manifest.adapterState.semanticEntry.state = "embedded-engine";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC1" && error.code === "second-semantic-engine",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC1: evaluating superiority against uncaptured pins is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.predicate.evaluationRequiresCapturedPins = false;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC1" && error.code === "pending-pin-superiority",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC2: a single microbenchmark certifying superiority is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.predicate.clauses["measured-advantage"].singleBenchmarkCertifies = true;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC2" && error.code === "single-benchmark-superiority",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC2: dropping a predeclared metric from the set is rejected", () => {
  const dirty = cloneProducts(clean);
  const advantage = dirty.predicate.clauses["measured-advantage"];
  advantage.metricSet = advantage.metricSet.filter((metric) => metric !== "retained-memory");
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC2" && error.code === "metric-population-drift",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC2: allowing post-hoc metric subset selection is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.predicate.clauses["measured-advantage"].noPostHocSubsetSelection = false;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC2" && error.code === "post-hoc-metric-subset",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC2: guessed zeros for unavailable metrics are rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.predicate.methodology.unavailableMetrics.guessedZeros = true;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC2" && error.code === "guessed-missing-metrics",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC2: a thin LSP plugin claimed as superiority is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.predicate.thinLspPlugin.status = "satisfies-predicate";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC2" && error.code === "thin-plugin-superiority",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC2: reducing the superiority conjunction to one clause is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.predicate.conjunction = ["measured-advantage"];
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC2" && error.code === "predicate-conjunction-drift",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC3: removing a required workflow from the matrix is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.matrix.workflows = dirty.matrix.workflows.filter((row) => row.id !== "inlays");
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) =>
        error.caseId === "JBT0-AC3" &&
        error.code === "workflow-population-drift" &&
        error.message.includes("inlays"),
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC3: a workflow outside the JBT0.2 population is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.matrix.workflows.push({
    id: "invented-workflow",
    officialBaseline: "none",
    passPredicate: "none",
    nonRegressionConditions: ["none"],
  });
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) =>
        error.caseId === "JBT0-AC3" &&
        error.code === "workflow-population-drift" &&
        error.message.includes("outside the JBT0.2 population"),
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC3: speed buying out a missing required workflow is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.matrix.promotionGate.missingWorkflowBlocksPromotion = false;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC3" && error.code === "speed-buys-out-parity",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC3: collapsing candidate states is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.matrix.candidateStates = ["parity", "gap"];
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC3" && error.code === "collapsed-candidate-states",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-AC3: parity outcomes recorded outside JBT9 are rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.matrix.measuredOutcomeOwner = "JBT10";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "JBT0-AC3" && error.code === "wrong-parity-owner",
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-ratification: a missing receiving amendment is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.ownership.receivingAmendments = dirty.ownership.receivingAmendments.filter(
    (row) => row.receiver !== "JBT1H",
  );
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "missing-amendment"),
    JSON.stringify(result.errors),
  );
});

test("JBT0-ratification: an unknown owner is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.ownership.outcomes[0].finalOwner = "INVENTED-OWNER";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "unknown-owner"),
    JSON.stringify(result.errors),
  );
});

test("JBT0-ratification: premature retirement is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.ownership.deletionPopulationThisNode = ["official-vue-plugin-route"];
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "premature-retirement"),
    JSON.stringify(result.errors),
  );
});

test("JBT0-ratification: an evidence file that does not exist is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.manifest.evidence = ["extensions/jetbrains/madeUp.md"];
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "missing-evidence-file"),
    JSON.stringify(result.errors),
  );
});

test("JBT0-ratification: a stale adapter-state row against the live repository is rejected", () => {
  const drifted = { ...facts, extensionsJetbrainsExists: true };
  const result = validate(clean, contracts, authority, drifted);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "stale-adapter-state"),
    JSON.stringify(result.errors),
  );
});

test("JBT0-ratification: an unregistered conflict domain is rejected", () => {
  const drifted = { ...facts, domainIds: new Set() };
  const result = validate(clean, contracts, authority, drifted);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.code === "missing-owner" && error.message.includes("jetbrains_product"),
    ),
    JSON.stringify(result.errors),
  );
});

test("JBT0-ratification: contract markers are enforced", () => {
  const stripped = { ...contracts };
  stripped["jetbrains-baseline.md"] = stripped["jetbrains-baseline.md"].replaceAll(
    "JetBrainsBaselineManifest",
    "X",
  );
  const result = validate(clean, stripped, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "missing-contract-marker"),
    JSON.stringify(result.errors),
  );
});

test("JBT0-ratification: train plan markers are enforced", () => {
  const stripped = { ...contracts };
  stripped["expansion-jetbrains-product.md"] = stripped[
    "expansion-jetbrains-product.md"
  ].replaceAll("JBT1H", "X");
  const result = validate(clean, stripped, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "missing-plan"),
    JSON.stringify(result.errors),
  );
});

test("JBT0-ratification: an unimplemented DX0 predecessor is rejected", () => {
  const orphan = {
    ...authority,
    ledger: {
      ...authority.ledger,
      implemented: authority.ledger.implemented.filter((row) => row.node_id !== "DX0"),
    },
  };
  const result = validate(clean, contracts, orphan, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "predecessor-not-implemented"),
    JSON.stringify(result.errors),
  );
});
