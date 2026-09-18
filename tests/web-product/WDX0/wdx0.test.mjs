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

function item(products, id) {
  const row = products.matrix.items.find((candidate) => candidate.id === id);
  assert.ok(row, `matrix item ${id} must exist`);
  return row;
}

function receiver(products, id) {
  const row = products.ownership.receivingAmendments.find((candidate) => candidate.receiver === id);
  assert.ok(row, `receiving amendment ${id} must exist`);
  return row;
}

function hasError(result, caseId, code) {
  return result.errors.some((error) => error.caseId === caseId && error.code === code);
}

test("WDX0-ratification: clean products validate against the live repository", () => {
  const result = validate(clean, contracts, authority, facts);
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual(mandatoryCases().sort(), [
    "WDX0-AC1",
    "WDX0-AC2",
    "WDX0-AC3",
    "WDX0-AC4",
    "WDX0-AC5",
    "WDX0-ratification",
  ]);
  assert.ok(selectedCaseIds({ errors: [] }).length === 0);
});

test("WDX0-AC1: a recommendation with two final owners is rejected", () => {
  const dirty = cloneProducts(clean);
  item(dirty, "RCT0").coOwner = "framework.solid";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WDX0-AC2", "two-final-owners"), JSON.stringify(result.errors));
});

test("WDX0-AC1: one train owning two recommendations is rejected", () => {
  const dirty = cloneProducts(clean);
  const solid = item(dirty, "SLD0");
  solid.train = "framework.react";
  solid.finalOwner = "framework.react";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-AC2", "train-owns-two-recommendations"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-AC1: an unknown disposition is rejected", () => {
  const dirty = cloneProducts(clean);
  item(dirty, "GQL0").disposition = "dual-authority";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WDX0-AC1", "unknown-disposition"), JSON.stringify(result.errors));
});

test("WDX0-AC1: a portfolio class contradicting its train is rejected", () => {
  const dirty = cloneProducts(clean);
  item(dirty, "GQL0").portfolioClass = "framework";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WDX0-AC1", "portfolio-class-drift"), JSON.stringify(result.errors));
});

test("WDX0-AC1: a qualification edge that is not the terminal is rejected", () => {
  const dirty = cloneProducts(clean);
  item(dirty, "RCT0").qualificationEdge = "RCT0";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WDX0-AC1", "head-terminal-mismatch"), JSON.stringify(result.errors));
});

test("WDX0-AC1: a dropped fan-in edge is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.matrix.qualificationFanIn = dirty.matrix.qualificationFanIn.filter((id) => id !== "RCT10");
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-AC1", "qualification-fan-in-drift"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-AC1: a missing recommendation row is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.matrix.items = dirty.matrix.items.filter((row) => row.id !== "WDX1");
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-AC1", "missing-recommendation-row"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-AC2: a proof node claimed as the support basis is rejected", () => {
  const dirty = cloneProducts(clean);
  item(dirty, "RCT0").claimBasisAtRatification = "static-proof";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-AC2", "proof-claimed-as-support"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-AC2: a runtime-observation claim before any terminal executed is rejected", () => {
  const dirty = cloneProducts(clean);
  item(dirty, "ALP0").claimBasisAtRatification = "runtime-observation";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-AC2", "runtime-claim-at-ratification"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-AC2: a portfolio-wide claim basis beyond none is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.matrix.claimBasisAtRatification = "static-proof";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WDX0-AC2", "claim-basis-drift"), JSON.stringify(result.errors));
});

test("WDX0-AC2: an exclusion without a recorded reason is rejected", () => {
  const dirty = cloneProducts(clean);
  item(dirty, "TW0").disposition = "exclusion";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-AC2", "exclusion-without-reason"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-AC2: an estimate class allowed to claim completion or speedup is rejected", () => {
  const dirty = cloneProducts(clean);
  const estimate = dirty.policy.classes.find((row) => row.id === "estimate");
  estimate.mayNotClaim = ["only labelled estimates"];
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-AC2", "estimate-may-claim-completion"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-AC2: an added-portfolio family in the live catalog is rejected", () => {
  const driftedFacts = structuredClone(facts);
  driftedFacts.surfaceFamilies = ["react", ...facts.surfaceFamilies];
  const result = validate(clean, contracts, authority, driftedFacts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WDX0-AC2", "catalog-family-drift"), JSON.stringify(result.errors));
});

test("WDX0-AC2: catalog surface-count drift is rejected", () => {
  const driftedFacts = structuredClone(facts);
  driftedFacts.surfaceCount = facts.surfaceCount + 1;
  const result = validate(clean, contracts, authority, driftedFacts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WDX0-AC2", "catalog-count-drift"), JSON.stringify(result.errors));
});

test("WDX0-AC2: a collapsed evidence-class vocabulary is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.policy.classes = dirty.policy.classes.slice(0, 2);
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WDX0-AC2", "evidence-class-drift"), JSON.stringify(result.errors));
});

test("WDX0-AC3: the runtime-correctness cases must stay bound to WDX1/WDX2/WDX3", () => {
  const dirty = cloneProducts(clean);
  delete dirty.ownership.acBasisDownstreamTestOwner;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WDX0-AC3", "missing-ac-basis-owner"), JSON.stringify(result.errors));
});

test("WDX0-AC3: the no-budget rationale must stay recorded", () => {
  const dirty = cloneProducts(clean);
  delete dirty.ownership.acResourceRationale;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-AC3", "missing-ac-resource-rationale"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-AC4: losing the examples/reference producer obligation is rejected", () => {
  const driftedContracts = structuredClone(contracts);
  driftedContracts["web-product-expansion-v1.md"] = contracts["web-product-expansion-v1.md"]
    .replaceAll("DOC1-tested", "tested")
    .replaceAll("examples/reference", "examples");
  const result = validate(clean, driftedContracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-AC4", "missing-examples-obligation"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-AC4: losing the evidence index is rejected", () => {
  const driftedFacts = structuredClone(facts);
  driftedFacts.evidenceFileExists = false;
  const result = validate(clean, contracts, authority, driftedFacts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WDX0-AC4", "missing-evidence-file"), JSON.stringify(result.errors));
});

test("WDX0-AC5: a nonempty deletion population is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.ownership.deletionPopulationThisNode = ["informal feature list"];
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-AC5", "nonempty-deletion-population"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-AC5: a displaced route is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.ownership.displacedRoutes = ["old capability list"];
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WDX0-AC5", "displaced-route"), JSON.stringify(result.errors));
});

test("WDX0-ratification: losing a contract marker is rejected", () => {
  const driftedContracts = structuredClone(contracts);
  driftedContracts["web-product-expansion-v1.md"] = contracts[
    "web-product-expansion-v1.md"
  ].replaceAll("## 10. Receiving amendments", "## 10. Amendments");
  const result = validate(clean, driftedContracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-ratification", "missing-contract-marker"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-ratification: losing the train plan markers is rejected", () => {
  const driftedContracts = structuredClone(contracts);
  driftedContracts["expansion-web-product-convergence.md"] = contracts[
    "expansion-web-product-convergence.md"
  ].replaceAll("WDX2", "qualification");
  const result = validate(clean, driftedContracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WDX0-ratification", "missing-plan"), JSON.stringify(result.errors));
});

test("WDX0-ratification: a gate/review profile drift on the DAG node is rejected", () => {
  const driftedAuthority = structuredClone(authority);
  const node = driftedAuthority.nodes.find((candidate) => candidate.id === "WDX0");
  node.gate_profile = "canonical";
  const result = validate(clean, contracts, driftedAuthority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-ratification", "node-profile-drift"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-ratification: a nonzero production budget on the DAG node is rejected", () => {
  const driftedAuthority = structuredClone(authority);
  const node = driftedAuthority.nodes.find((candidate) => candidate.id === "WDX0");
  node.max_production_loc = 10;
  const result = validate(clean, contracts, driftedAuthority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-ratification", "zero-budget-violation"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-ratification: a missing implemented ledger row is rejected", () => {
  const driftedAuthority = structuredClone(authority);
  driftedAuthority.ledger.implemented = driftedAuthority.ledger.implemented.filter(
    (row) => row.node_id !== "WDX0",
  );
  const result = validate(clean, contracts, driftedAuthority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-ratification", "missing-ledger-row"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-ratification: receiver drift against the disposition matrix is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.ownership.receivingAmendments = dirty.ownership.receivingAmendments.filter(
    (row) => row.receiver !== "RCT0",
  );
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-ratification", "receiver-count-drift"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-ratification: a docs-only head claiming productionCapable is rejected", () => {
  const dirty = cloneProducts(clean);
  receiver(dirty, "RCT0").productionCapable = true;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-ratification", "docs-only-production-capable"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-ratification: an implementation receiver denying productionCapable is rejected", () => {
  const dirty = cloneProducts(clean);
  receiver(dirty, "WDX1").productionCapable = false;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-ratification", "docs-only-production-capable"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-ratification: a historical anchor that is no longer implemented is rejected", () => {
  const driftedAuthority = structuredClone(authority);
  driftedAuthority.ledger.implemented = driftedAuthority.ledger.implemented.filter(
    (row) => row.node_id !== "STP0",
  );
  const result = validate(clean, contracts, driftedAuthority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-ratification", "historical-anchor-not-implemented"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-ratification: a missing preserved historical contract is rejected", () => {
  const driftedFacts = structuredClone(facts);
  driftedFacts.preservedContracts[0].exists = false;
  const result = validate(clean, contracts, authority, driftedFacts);
  assert.equal(result.ok, false);
  assert.ok(
    hasError(result, "WDX0-ratification", "preserved-contract-missing"),
    JSON.stringify(result.errors),
  );
});

test("WDX0-ratification: an outcome owner outside the DAG and amendments is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.ownership.outcomes[0].finalOwner = "nobody";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(hasError(result, "WDX0-ratification", "unknown-owner"), JSON.stringify(result.errors));
});
