import assert from "node:assert/strict";
import test from "node:test";

import {
  ACCEPTED_PRODUCTS,
  DIRTY_CACHE,
  STP9_MANDATORY_CASES,
  assertCompleteCachePolicy,
  assertTypeFree,
  cloneJson,
  evaluateRejectTwins,
  evaluateStp9,
  loadStp9Product,
  productionPlanSource,
  validateStp9Products,
} from "./protocol.mjs";

test("STP9 products name the charter plan contracts", () => {
  const product = loadStp9Product();
  assert.equal(product.schema, "SourceBackedProjectionPlan");
  assert.deepEqual([...product.products].sort(), [...ACCEPTED_PRODUCTS].sort());
  const errors = validateStp9Products(product);
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
});

test("STP9-type-free: plan construction does not call TypeInfo or assignability", () => {
  const errors = assertTypeFree(productionPlanSource());
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
  const dirty = `${productionPlanSource()}\nuse verter_semantic::type_info::TypeInfoCore;\n`;
  assert.ok(
    assertTypeFree(dirty).some((error) => error.caseId === "STP9-type-free"),
    JSON.stringify(assertTypeFree(dirty)),
  );
});

test("STP9-complete-cache: malformed syntax cannot warm a complete plan cache", () => {
  const product = loadStp9Product();
  assert.equal(assertCompleteCachePolicy(product).length, 0);
  const dirty = cloneJson(product);
  Object.assign(dirty.completeCache, DIRTY_CACHE);
  assert.ok(
    assertCompleteCachePolicy(dirty).some((error) => error.caseId === "STP9-complete-cache"),
    JSON.stringify(assertCompleteCachePolicy(dirty)),
  );
});

test("STP9 dirty twins for type-free and complete-cache are rejected", () => {
  const errors = evaluateRejectTwins();
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
});

test("STP9 evaluate joins rust identity/shadow/determinism cases", async () => {
  const result = await evaluateStp9();
  assert.equal(result.errors.length, 0, JSON.stringify(result.errors, null, 2));
  assert.deepEqual(
    [...STP9_MANDATORY_CASES],
    ["STP9-ids", "STP9-shadow", "STP9-type-free", "STP9-complete-cache", "STP9-determinism"],
  );
});
