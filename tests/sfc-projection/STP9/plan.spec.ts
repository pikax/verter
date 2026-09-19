import assert from "node:assert/strict";
import test from "node:test";

import {
  ACCEPTED_PRODUCTS,
  DIRTY_CACHE,
  STP9_MANDATORY_CASES,
  assertCompleteCachePolicy,
  assertRustCases,
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

test("mandatory rust cases fail closed on zero-selection, ignored, and unrelated-only cargo output", () => {
  const zero = assertRustCases({
    status: 0,
    error: null,
    stdout: "test result: ok. 0 passed; 0 failed\n",
    ok: true,
  });
  assert.ok(zero.length > 0, "zero-selection cargo ok must not pass");
  const ignored = assertRustCases({
    status: 0,
    error: null,
    stdout: [
      "test framework_common::projection_plan::tests::stp9_ids_comment_and_unrelated_sibling_preserve_use_identities ... ok",
      "test framework_common::projection_plan::tests::stp9_shadow_nested_slot_and_loop_origins_are_distinct ... ok",
      "test framework_common::projection_plan::tests::stp9_type_free_plan_module_does_not_call_typeinfo ... ok",
      "test framework_common::projection_plan::tests::stp9_determinism_fresh_matches_incremental ... ok",
      "test result: ok. 4 passed; 0 failed",
    ].join("\n"),
    ok: true,
  });
  assert.ok(
    ignored.some((row) => row.caseId === "STP9-complete-cache"),
    JSON.stringify(ignored),
  );
  const unrelated = assertRustCases({
    status: 0,
    error: null,
    stdout: "test some_other_filter ... ok\ntest result: ok. 1 passed; 0 failed\n",
    ok: true,
  });
  assert.equal(unrelated.length, STP9_MANDATORY_CASES.length, JSON.stringify(unrelated));
});
