import assert from "node:assert/strict";
import test from "node:test";

import {
  STP12_MANDATORY_CASES,
  assertRustCases,
  loadProduct,
  validateProduct,
} from "./protocol.mjs";

test("JavaScript projection names its three contracts", () => {
  assert.equal(validateProduct(loadProduct()).length, 0);
  assert.deepEqual(
    [...STP12_MANDATORY_CASES],
    [
      "STP12-checkjs-off",
      "STP12-checkjs-on",
      "STP12-jsdoc-generic",
      "STP12-jsx",
      "STP12-suppression",
    ],
  );
});

test("JavaScript projection rejects incomplete Rust receipts", () => {
  const errors = assertRustCases({
    status: 0,
    error: null,
    stdout: "test unrelated ... ok\ntest result: ok. 1 passed; 0 failed\n",
  });
  assert.equal(errors.length, STP12_MANDATORY_CASES.length);
});

test("JavaScript projection accepts only a complete Rust receipt", () => {
  const names = [
    "vue_compiler_js_unchecked_script_keeps_template_projection",
    "vue_compiler_js_check_directive_is_a_leading_pragma",
    "vue_compiler_jsdoc_generic_uses_public_instance_contract",
    "vue_compiler_jsx_keeps_authored_jsx_expression",
    "vue_compiler_js_projection_never_injects_nocheck",
  ];
  const errors = assertRustCases({
    status: 0,
    error: null,
    stdout: `${names.map((name) => `test ${name} ... ok`).join("\n")}\ntest result: ok. 5 passed; 0 failed\n`,
  });
  assert.equal(errors.length, 0, JSON.stringify(errors));
});
