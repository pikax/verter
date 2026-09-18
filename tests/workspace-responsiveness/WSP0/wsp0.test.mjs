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

function measurement(products, id) {
  const row = products.contract.measurementDefinitions.find((candidate) => candidate.id === id);
  assert.ok(row, `measurement ${id} must exist`);
  return row;
}

function workload(products, id) {
  const row = products.contract.workloadMatrix.find((candidate) => candidate.id === id);
  assert.ok(row, `workload ${id} must exist`);
  return row;
}

function hasError(result, caseId, code) {
  return result.errors.some((error) => error.caseId === caseId && error.code === code);
}

test("WSP0-ratification: clean products validate against the live repository", () => {
  const result = validate(clean, contracts, authority, facts);
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual(mandatoryCases().sort(), [
    "WSP0-AC-BASIS",
    "WSP0-AC-EXPOSURE",
    "WSP0-AC-OWNER",
    "WSP0-AC-RESOURCE",
    "WSP0-AC1",
    "WSP0-AC2",
    "WSP0-AC3",
    "WSP0-ratification",
  ]);
  assert.ok(selectedCaseIds({ errors: [] }).length === 0);
});

test("WSP0-AC1: an SLO row without a client budget is rejected", () => {
  const dirty = cloneProducts(clean);
  delete dirty.sloCatalog.rows[0].clientBudget["50k"];
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC1", "server-only-responsiveness"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC1: UI responsiveness tied to the server clock is rejected", () => {
  const dirty = cloneProducts(clean);
  delete measurement(dirty, "client_input_to_paint").independentOf;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC1", "server-dependent-ui-metric"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC1: a server timeline certifying interactivity is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.evidencePlan.timelineLaw.serverAloneCannotCertify = false;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC1", "server-certified-interactivity"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC2: allowed diagnostic truncation is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.contract.correctnessDenominator.truncation = "allowed";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WSP0-AC2", "diagnostic-truncation"), JSON.stringify(result.errors));
});

test("WSP0-AC2: reduced project membership in the denominator is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.contract.correctnessDenominator.membership = "subset";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC2", "unequal-work-denominator"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC2: a reduced feature set in the denominator is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.contract.correctnessDenominator.featuresEnabled = "reduced";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WSP0-AC2", "reduced-feature-set"), JSON.stringify(result.errors));
});

test("WSP0-AC2: dropping a forbidden-improvement entry is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.contract.budgetLaw.forbiddenImprovements.pop();
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC2", "forbidden-improvement-vocabulary-drift"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC2: an equivalent-work denominator losing a term is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.sloCatalog.denominators.equivalentWork.splice(2, 1);
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC2", "unequal-work-denominator"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC2: a machine manifest enabling fewer features is rejected", () => {
  const dirty = cloneProducts(clean);
  const row = dirty.machines.manifestRequirements.find(
    (candidate) => candidate.field === "enabledFeatures",
  );
  row.mustBe = "subset";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WSP0-AC2", "reduced-feature-set"), JSON.stringify(result.errors));
});

test("WSP0-AC3: claiming the unreproduced issue fixed is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.evidencePlan.originalIssue.disposition = "fixed";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WSP0-AC3", "unverified-issue-claim"), JSON.stringify(result.errors));
});

test("WSP0-AC3: a fabricated version capture is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.evidencePlan.versionCapture[0] = {
    item: "lapce-client",
    version: "0.4.0",
    status: "recorded",
  };
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC3", "fabricated-version-capture"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-OWNER: an empty final owner is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.contract.ownership.finalOwner = "";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-OWNER", "missing-final-owner"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-OWNER: a missing retirement obligation is rejected", () => {
  const dirty = cloneProducts(clean);
  delete dirty.contract.ownership.retirementObligation;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-OWNER", "missing-retirement-obligation"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-OWNER: conflict-domain drift in the product is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.contract.conflictDomains.pop();
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-OWNER", "conflict-domain-drift"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-OWNER: a conflict domain missing from the live catalog is rejected", () => {
  const dirtyFacts = structuredClone(facts);
  dirtyFacts.domainRows.delete("scheduler_admission");
  const result = validate(clean, contracts, authority, dirtyFacts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-OWNER", "conflict-domain-unregistered"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-BASIS: losing the WSP1 downstream binding is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.sloCatalog.acceptance.acBasis = "downstream runtime-test owner is TBD";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-BASIS", "downstream-owner-drift"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-BASIS: receipt vocabulary drift is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.contract.basisLaw.receiptVocabulary = "local-policy";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-BASIS", "basis-vocabulary-drift"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-BASIS: the verify phase drifting off WSP1 is rejected", () => {
  const dirty = cloneProducts(clean);
  const phase = dirty.evidencePlan.phases.find((candidate) => candidate.id === "verify");
  phase.owner = "WSP0";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-BASIS", "downstream-owner-drift"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-RESOURCE: dropping a measurement definition is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.contract.measurementDefinitions.pop();
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-RESOURCE", "missing-measurement-definition"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-RESOURCE: merging the RSS classes is rejected", () => {
  const dirty = cloneProducts(clean);
  delete measurement(dirty, "language_service_incremental_rss").separateFrom;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-RESOURCE", "rss-classes-merged"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-RESOURCE: fabricated instrumentation is rejected", () => {
  const dirty = cloneProducts(clean);
  measurement(dirty, "provider_work").instrumentationStatus = "instrumented";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-RESOURCE", "fabricated-instrumentation"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-RESOURCE: measured provenance with no machine is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.sloCatalog.provenanceRules.allowedAtRatification = "measured";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-RESOURCE", "fabricated-measurement"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-RESOURCE: workload population drift is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.contract.workloadMatrix.splice(
    dirty.contract.workloadMatrix.findIndex((row) => row.id === "unicode-crlf"),
    1,
  );
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-RESOURCE", "workload-population-drift"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-RESOURCE: a real project pin without a recorded machine is rejected", () => {
  const dirty = cloneProducts(clean);
  workload(dirty, "authored-10k").realProjectBinding.status = "pinned";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-RESOURCE", "pin-without-machine"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-RESOURCE: an incomplete machine manifest is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.machines.machines.push({ id: "reference-1", os: "windows" });
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-RESOURCE", "incomplete-machine-manifest"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-RESOURCE: manifest requirement drift is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.machines.manifestRequirements.splice(
    dirty.machines.manifestRequirements.findIndex(
      (row) => row.field === "providerEngines[].version",
    ),
    1,
  );
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-RESOURCE", "manifest-requirement-drift"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-RESOURCE: equal-work corpus drift against the live MEM0 fixtures is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.contract.equalWorkCorpus.pinnedFixtureCount =
    clean.contract.equalWorkCorpus.pinnedFixtureCount - 1;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WSP0-AC-RESOURCE", "corpus-drift"), JSON.stringify(result.errors));
});

test("WSP0-AC-EXPOSURE: an introduced operation is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.contract.exposureLaw.introducedOperations.push("editor.new_surface");
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-EXPOSURE", "unregistered-introduced-operation"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-EXPOSURE: a non-editor SLO surface family is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.sloCatalog.rows[1].surfaceFamily = "inspector";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-EXPOSURE", "unregistered-introduced-operation"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-AC-EXPOSURE: SLO population drift is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.sloCatalog.rows.push({
    operation: "editor.format_document",
    surfaceFamily: "editor",
    clientBudget: {
      "1k": { p50: 1, p99: 2 },
      "10k": { p50: 1, p99: 2 },
      "50k": { p50: 1, p99: 2 },
    },
    serverBudget: {
      "1k": { p50: 1, p99: 2 },
      "10k": { p50: 1, p99: 2 },
      "50k": { p50: 1, p99: 2 },
    },
    rationale: "twin",
  });
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-AC-EXPOSURE", "slo-population-drift"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-ratification: a missing product is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.contract.schema = "something-else";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-ratification", "missing-product"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-ratification: a missing train plan is rejected", () => {
  const dirtyContracts = { ...contracts };
  dirtyContracts["expansion-workspace-responsiveness.md"] = "# stub";
  const result = validate(clean, dirtyContracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WSP0-ratification", "missing-plan"), JSON.stringify(result.errors));
});

test("WSP0-ratification: a missing implemented ledger row is rejected", () => {
  const dirtyAuthority = structuredClone(authority);
  dirtyAuthority.ledger.implemented = dirtyAuthority.ledger.implemented.filter(
    (row) => row.node_id !== "WSP0",
  );
  const result = validate(clean, contracts, dirtyAuthority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-ratification", "missing-ledger-row"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-ratification: an unsettled predecessor is rejected", () => {
  const dirtyAuthority = structuredClone(authority);
  dirtyAuthority.ledger.implemented = dirtyAuthority.ledger.implemented.filter(
    (row) => row.node_id !== "MEM0",
  );
  const result = validate(clean, contracts, dirtyAuthority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-ratification", "predecessor-not-settled"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-ratification: DAG node profile drift is rejected", () => {
  const dirtyAuthority = structuredClone(authority);
  const node = dirtyAuthority.nodes.find((candidate) => candidate.id === "WSP0");
  node.gate_profile = "canonical";
  const result = validate(clean, contracts, dirtyAuthority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-ratification", "node-profile-drift"),
    JSON.stringify(result.errors),
  );
});

test("WSP0-ratification: a nonzero production budget is rejected", () => {
  const dirtyAuthority = structuredClone(authority);
  const node = dirtyAuthority.nodes.find((candidate) => candidate.id === "WSP0");
  node.max_production_files = 1;
  const result = validate(clean, contracts, dirtyAuthority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WSP0-ratification", "zero-budget-violation"),
    JSON.stringify(result.errors),
  );
});
