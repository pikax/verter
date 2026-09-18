import assert from "node:assert/strict";
import test from "node:test";

import {
  cloneProducts,
  loadContracts,
  loadProducts,
  mandatoryCases,
  selectedCaseIds,
  validate,
} from "./verify.mjs";
import { loadAuthority } from "../../../roadmap/0.1.0-tama/tools/lib.mjs";

const authority = loadAuthority();
const clean = loadProducts();
const contracts = loadContracts();

test("STP0-ratification: clean products name owners, mandatory rows, and receiving amendments", () => {
  const result = validate(clean, contracts, authority);
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual(mandatoryCases().sort(), [
    "STP0-authority",
    "STP0-policy",
    "STP0-ratification",
    "STP0-required-current",
  ]);
  assert.ok(selectedCaseIds(result).includes("STP0-ratification"));
  const instance = clean.features.rows.find((row) => row.id === "instancetype-typeof-comp");
  assert.equal(instance?.obligation, "RequiredCurrent");
  assert.ok(clean.ownership.receivingAmendments.some((row) => row.receiver === "STP58"));
  assert.ok(clean.ownership.ac3Rationale.length > 0);
  assert.ok(clean.ownership.ac4Rationale.length > 0);
});

test("STP0-ratification dirty twin: missing receiving amendment is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.ownership.receivingAmendments = dirty.ownership.receivingAmendments.filter(
    (row) => row.receiver !== "STP1",
  );
  const result = validate(dirty, contracts, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "STP0-ratification" && error.code === "missing-amendment",
    ),
    JSON.stringify(result.errors),
  );
});

test("STP0-ratification dirty twin: docs-only deletion owner is rejected", () => {
  const dirty = cloneProducts(clean);
  const route = dirty.ownership.displacedRoutes[0];
  route.deletionOwner = "A0";
  route.finalOwner = "A0";
  const result = validate(dirty, contracts, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) =>
        error.caseId === "STP0-ratification" && error.code === "non-production-deletion-owner",
    ),
    JSON.stringify(result.errors),
  );
});

test("STP0-authority dirty twin: native assignability type owner is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.policy.typeAuthority = "native-assignability";
  dirty.policy.forbiddenTypeAuthorities = dirty.policy.forbiddenTypeAuthorities.filter(
    (name) => name !== "native-assignability",
  );
  const result = validate(dirty, contracts, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "STP0-authority" && error.code === "native-type-answer",
    ),
    JSON.stringify(result.errors),
  );
});

test("STP0-authority dirty twin: second mapping owner is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.policy.secondMappingOwner = true;
  dirty.policy.mappingOwner = "native-assignability";
  const result = validate(dirty, contracts, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "STP0-authority" && error.code === "second-mapping-owner",
    ),
    JSON.stringify(result.errors),
  );
});

test("STP0-required-current dirty twin: removing InstanceType is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.features.rows = dirty.features.rows.filter((row) => row.id !== "instancetype-typeof-comp");
  const result = validate(dirty, contracts, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "STP0-required-current" && error.code === "removed",
    ),
    JSON.stringify(result.errors),
  );
});

test("STP0-required-current dirty twin: optionalizing a shipped row is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.features.rows.find((row) => row.id === "define-props").obligation = "optional";
  const result = validate(dirty, contracts, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "STP0-required-current" && error.code === "optionalized",
    ),
    JSON.stringify(result.errors),
  );
});

test("STP0-required-current dirty twin: marking a shipped row external is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.features.rows.find((row) => row.id === "define-emits").obligation = "external";
  const result = validate(dirty, contracts, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "STP0-required-current" && error.code === "external",
    ),
    JSON.stringify(result.errors),
  );
});

test("STP0-required-current dirty twin: removing a non-InstanceType shipped row is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.features.rows = dirty.features.rows.filter((row) => row.id !== "define-props");
  const result = validate(dirty, contracts, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) =>
        error.caseId === "STP0-required-current" &&
        error.code === "removed" &&
        String(error.message).includes("define-props"),
    ),
    JSON.stringify(result.errors),
  );
});

test("STP0-required-current dirty twin: unshipping a row and marking it external is rejected", () => {
  const dirty = cloneProducts(clean);
  const row = dirty.features.rows.find((entry) => entry.id === "define-slots");
  row.shipped = false;
  row.obligation = "external";
  const result = validate(dirty, contracts, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "STP0-required-current" && error.code === "external",
    ),
    JSON.stringify(result.errors),
  );
});

test("STP0-ratification dirty twin: emptying displacedRoutes is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.ownership.displacedRoutes = [];
  const result = validate(dirty, contracts, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) =>
        error.caseId === "STP0-ratification" &&
        error.code === "missing-owner" &&
        /displaced route/u.test(error.message),
    ),
    JSON.stringify(result.errors),
  );
});

test("STP0-policy dirty twin: default inheritAttrs vs explicit true split is rejected", () => {
  const dirty = cloneProducts(clean);
  dirty.policy.inheritAttrs.explicitTrue = "no-inherit";
  dirty.policy.inheritAttrs.omittedEqualsExplicitTrue = false;
  const result = validate(dirty, contracts, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (error) => error.caseId === "STP0-policy" && error.code === "inheritattrs-default-true-split",
    ),
    JSON.stringify(result.errors),
  );
});

test("STP0-AC1: all four cases are selected; zero-test pass is impossible", () => {
  assert.equal(mandatoryCases().length, 4);
  const result = validate(clean, contracts, authority);
  assert.equal(result.ok, true);
  const dirty = cloneProducts(clean);
  dirty.policy.typeAuthority = "native-typeinfo";
  dirty.features.rows = [];
  dirty.policy.inheritAttrs.omittedEqualsExplicitTrue = false;
  dirty.ownership.receivingAmendments = [];
  const failed = validate(dirty, contracts, authority);
  const selected = new Set(selectedCaseIds(failed));
  for (const id of mandatoryCases()) {
    assert.ok(selected.has(id), `missing selected case ${id}: ${JSON.stringify(failed.errors)}`);
  }
});
