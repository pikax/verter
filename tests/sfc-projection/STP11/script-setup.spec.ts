import assert from "node:assert/strict";
import test from "node:test";

import {
  ACCEPTED_PRODUCTS,
  STP11_MANDATORY_CASES,
  assertRustCases,
  assertScriptPolicy,
  cloneJson,
  evaluateRejectTwins,
  evaluateStp11,
  loadStp11Product,
  validateStp11Products,
} from "./protocol.mjs";

test("STP11 products name the charter script contracts", () => {
  const product = loadStp11Product();
  assert.equal(product.schema, "ScriptSetupProjection");
  assert.deepEqual([...product.products].sort(), [...ACCEPTED_PRODUCTS].sort());
  const errors = validateStp11Products(product);
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
});

test("STP11 dirty twins for each forbidden design are rejected", () => {
  assert.equal(assertScriptPolicy(loadStp11Product()).length, 0);
  const errors = evaluateRejectTwins();
  assert.equal(errors.length, 0, JSON.stringify(errors, null, 2));
  const macro = cloneJson(loadStp11Product());
  macro.macros.sameNamedLexicalFunctionIsMacro = true;
  assert.ok(assertScriptPolicy(macro).some((row) => row.caseId === "STP11-scope"));
});

test("STP11 evaluate joins the rust universal/scope/await/assertion/one-body cases", async () => {
  const result = await evaluateStp11();
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
  assert.equal(unrelated.length, STP11_MANDATORY_CASES.length, JSON.stringify(unrelated));
});
