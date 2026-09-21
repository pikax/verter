import assert from "node:assert/strict";
import test from "node:test";

import {
  ACCEPTED_PRODUCTS,
  STP14_MANDATORY_CASES,
  assertCapturePolicy,
  assertRustCases,
  cloneJson,
  evaluateRejectTwins,
  evaluateStp14,
  loadStp14Product,
  validateStp14Products,
} from "./protocol.mjs";

test("STP14 products name the charter capture contracts", () => {
  const product = loadStp14Product();
  assert.equal(product.schema, "BinderCapturePlan");
  assert.deepEqual([...product.products].sort(), [...ACCEPTED_PRODUCTS].sort());
  const errors = validateStp14Products(product);
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
});

test("STP14 dirty twins for each forbidden design are rejected", () => {
  assert.equal(assertCapturePolicy(loadStp14Product()).length, 0);
  const errors = evaluateRejectTwins();
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
  const overLifting = cloneJson(loadStp14Product());
  overLifting.lifting.minimalReachableOnly = false;
  assert.ok(assertCapturePolicy(overLifting).some((row) => row.code === "over-lifting"));
});

test("STP14 evaluate joins the rust capture, closure, typeof, cycle and duplicate cases", async () => {
  const result = await evaluateStp14();
  assert.equal(result.errors.length, 0, JSON.stringify(result.errors, null, 2));
});

test("mandatory rust cases fail closed on zero-selection and unrelated-only cargo output", () => {
  const zero = assertRustCases({
    status: 0,
    error: null,
    stdout: "test result: ok. 0 passed; 0 failed\n",
    ok: true,
  });
  assert.ok(zero.length > 0, "zero-selection cargo ok must not pass");
  const unrelated = assertRustCases({
    status: 0,
    error: null,
    stdout: "test some_other_filter ... ok\ntest result: ok. 1 passed; 0 failed\n",
    ok: true,
  });
  assert.equal(unrelated.length, STP14_MANDATORY_CASES.length, JSON.stringify(unrelated));
});
