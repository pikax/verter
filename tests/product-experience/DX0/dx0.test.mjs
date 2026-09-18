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

function operation(products, id) {
  const row = products.exposure.operations.find((candidate) => candidate.id === id);
  assert.ok(row, `operation ${id} must exist`);
  return row;
}

test("DX0-ratification: clean products validate against the live repository", () => {
  const result = validate(clean, contracts, authority, facts);
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual(mandatoryCases().sort(), ["DX0-AC1", "DX0-AC2", "DX0-AC3", "DX0-ratification"]);
  assert.ok(selectedCaseIds({ errors: [] }).length === 0);
});

test("DX0-AC1: a playground gap claimed exposed without a route is rejected", () => {
  const dirty = cloneProducts(clean);
  const row = operation(dirty, "tsc.project-check.vue");
  row.playground = { status: "exposed", evidence: row.playground.evidence };
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "DX0-AC1" && error.code === "playground-gap-claimed-exposed",
    ),
    JSON.stringify(result.errors),
  );
});

test("DX0-AC1: an unblocked playground gap is rejected", () => {
  const dirty = cloneProducts(clean);
  operation(dirty, "mcp.lint-project").playground.promotionBlocked = false;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "DX0-AC1" && error.code === "unblocked-playground-gap",
    ),
    JSON.stringify(result.errors),
  );
});

test("DX0-AC1: dropping the pinned gap population is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.exposure.operations = dirty.exposure.operations.filter(
    (row) => !["tsc.project-check.vue", "tsc.project-check.svelte"].includes(row.id),
  );
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.caseId === "DX0-AC1" && error.code === "missing-gap-row"),
    JSON.stringify(result.errors),
  );
});

test("DX0-AC2: a native-only operation claimed browser-local without a build is rejected", () => {
  const dirty = cloneProducts(clean);
  operation(dirty, "tsc.project-check.vue").browserExecutable = true;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "DX0-AC2" && error.code === "native-only-browser-claim",
    ),
    JSON.stringify(result.errors),
  );
});

test("DX0-AC2: a hidden replacement engine class is rejected", () => {
  const dirty = cloneProducts(clean);
  operation(dirty, "playground.compile-carrier").hostExecutionClass = "WasmTsgo";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "DX0-AC2" && error.code === "unknown-host-class",
    ),
    JSON.stringify(result.errors),
  );
});

test("DX0-AC2: a Portable operation denied browser execution is rejected", () => {
  const dirty = cloneProducts(clean);
  operation(dirty, "playground.compile-carrier").browserExecutable = false;
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "DX0-AC2" && error.code === "portable-not-browser-executable",
    ),
    JSON.stringify(result.errors),
  );
});

test("DX0-AC3: routing native flow facts onto the semantic rail is rejected", () => {
  const dirty = cloneProducts(clean);
  const row = operation(dirty, "inspection.flow-return");
  row.answerRail = "semantic";
  row.semanticAuthority = "verter-flow";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.caseId === "DX0-AC3" && error.code === "rail-substitution"),
  );
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "DX0-AC3" && error.code === "unlabelled-inspection",
    ),
  );
});

test("DX0-AC3: unlabelling the inspection rail is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.rails.inspectionRail.labelling = "optional";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "DX0-AC3" && error.code === "unlabelled-inspection",
    ),
    JSON.stringify(result.errors),
  );
});

test("DX0-AC3: a semantic row without TypeScript authority is rejected", () => {
  const dirty = cloneProducts(clean);
  operation(dirty, "lsp.hover").semanticAuthority = "native-flow";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.caseId === "DX0-AC3" && error.code === "rail-substitution"),
    JSON.stringify(result.errors),
  );
});

test("DX0-ratification: a shipped operation optionalized is rejected", () => {
  const dirty = cloneProducts(clean);
  operation(dirty, "lsp.hover").obligation = "optional";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "not-required-current"),
    JSON.stringify(result.errors),
  );
});

test("DX0-ratification: a missing receiving amendment is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.ownership.receivingAmendments = dirty.ownership.receivingAmendments.filter(
    (row) => row.receiver !== "DX1",
  );
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "missing-amendment"),
    JSON.stringify(result.errors),
  );
});

test("DX0-ratification: an unknown owner is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.ownership.outcomes[0].finalOwner = "INVENTED-OWNER";
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "unknown-owner"),
    JSON.stringify(result.errors),
  );
});

test("DX0-ratification: premature retirement is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.ownership.deletionPopulationThisNode = ["playground-component-meta-panel"];
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "premature-retirement"),
    JSON.stringify(result.errors),
  );
});

test("DX0-ratification: an unknown catalog surface citation is rejected", () => {
  const dirty = cloneProducts(clean);
  operation(dirty, "lsp.hover").catalogSurfaces = ["vue.language_service.telepathy"];
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "unknown-catalog-surface"),
    JSON.stringify(result.errors),
  );
});

test("DX0-ratification: an evidence file that does not exist is rejected", () => {
  const dirty = cloneProducts(clean);
  operation(dirty, "lsp.hover").playground.evidence = ["packages/playground/src/editor/madeUp.ts"];
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "missing-evidence-file"),
    JSON.stringify(result.errors),
  );
});

test("DX0-ratification: inventory lint rule count drift is rejected", () => {
  const drifted = { ...facts, lintRuleCount: facts.lintRuleCount + 1 };
  const result = validate(clean, contracts, authority, drifted);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "lint-count-drift"),
    JSON.stringify(result.errors),
  );
});

test("DX0-ratification: contract and plan markers are enforced", () => {
  const stripped = { ...contracts };
  stripped["product-experience.md"] = stripped["product-experience.md"].replaceAll(
    "HostExecutionClass",
    "X",
  );
  const result = validate(clean, stripped, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "missing-contract-marker"),
    JSON.stringify(result.errors),
  );
});

test("DX0-ratification: preserved clients and provider modes stay pinned", () => {
  const dirty = cloneProducts(clean);
  dirty.inventory.preservedClients.ids = dirty.inventory.preservedClients.ids.filter(
    (id) => id !== "zed",
  );
  dirty.inventory.analysis.providerModes = ["auto", "tsgo"];
  const result = validate(dirty, contracts, authority, facts);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((error) => error.code === "missing-preserved-client"),
    JSON.stringify(result.errors),
  );
  assert.ok(
    result.errors.some((error) => error.code === "provider-modes"),
    JSON.stringify(result.errors),
  );
});
