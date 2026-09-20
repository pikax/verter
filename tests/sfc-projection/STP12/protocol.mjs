import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { NODE_MANDATORY_CASES } from "../../../scripts/sfc-projection/node-mandatory-cases.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const PRODUCT = "tests/sfc-projection/STP12/products/javascript-projection.json";

export const STP12_MANDATORY_CASES = Object.freeze([
  "STP12-checkjs-off",
  "STP12-checkjs-on",
  "STP12-jsdoc-generic",
  "STP12-jsx",
  "STP12-suppression",
]);

const RUST_CASES = Object.freeze({
  "STP12-checkjs-off": "vue_compiler_js_unchecked_script_keeps_template_projection",
  "STP12-checkjs-on": "vue_compiler_js_check_directive_is_a_leading_pragma",
  "STP12-jsdoc-generic": "vue_compiler_jsdoc_generic_uses_public_instance_contract",
  "STP12-jsx": "vue_compiler_jsx_keeps_authored_jsx_expression",
  "STP12-suppression": "vue_compiler_js_projection_never_injects_nocheck",
});

function err(caseId, code, message) {
  return { caseId, code, message };
}

export function loadProduct() {
  return JSON.parse(fs.readFileSync(path.join(REPO_ROOT, PRODUCT), "utf8"));
}

export function validateProduct(product = loadProduct()) {
  const errors = [];
  if (product.schema !== "JavaScriptProjection") {
    errors.push(
      err("STP12-jsdoc-generic", "removed-product", "JavaScriptProjection schema missing"),
    );
  }
  for (const name of ["JsSetupProjection", "JsDiagnosticParticipation", "JsPublicContractBridge"]) {
    if (!product.products?.includes(name)) {
      errors.push(err("STP12-jsdoc-generic", "removed-product", `missing ${name}`));
    }
  }
  if (product.checkJs?.off !== "authored-ts-nocheck-leading-pragma-template-projected") {
    errors.push(err("STP12-checkjs-off", "policy-drift", "unchecked JS policy drifted"));
  }
  if (product.checkJs?.on !== "authored-ts-check-leading-pragma") {
    errors.push(err("STP12-checkjs-on", "policy-drift", "checked JS policy drifted"));
  }
  if (product.jsdoc !== "verbatim-authored-generic") {
    errors.push(err("STP12-jsdoc-generic", "policy-drift", "JSDoc generic policy drifted"));
  }
  if (product.jsx !== "authored-expression-preserved") {
    errors.push(err("STP12-jsx", "policy-drift", "JSX policy drifted"));
  }
  if (product.publicInstance !== "InstanceType-of-public-default") {
    errors.push(err("STP12-jsdoc-generic", "policy-drift", "public instance bridge drifted"));
  }
  if (product.diagnosticSuppression !== "none-generated") {
    errors.push(err("STP12-suppression", "policy-drift", "generated suppression admitted"));
  }
  return errors;
}

export function runRustCases(repoRoot = REPO_ROOT) {
  const result = spawnSync(
    "cargo",
    ["test", "-p", "verter_compiler", "--lib", "vue_compiler_js_", "--", "--test-threads=1"],
    { cwd: repoRoot, encoding: "utf8", windowsHide: true, timeout: 300000, env: process.env },
  );
  return {
    status: result.status,
    error: result.error,
    stdout: `${result.stdout || ""}${result.stderr || ""}`,
  };
}

export function assertRustCases(run) {
  const errors = [];
  if (run.error) {
    return STP12_MANDATORY_CASES.map((id) => err(id, "missing-check", run.error.message));
  }
  const output = String(run.stdout || "");
  if (run.status !== 0 || !/test result: ok\. [1-9]\d* passed; 0 failed/.test(output)) {
    return STP12_MANDATORY_CASES.map((id) =>
      err(
        id,
        "rust-case",
        `cargo test did not complete the JS projection cases (status=${run.status})`,
      ),
    );
  }
  for (const [id, name] of Object.entries(RUST_CASES)) {
    if (!output.includes(`${name} ... ok`) || output.includes(`${name} ... FAILED`)) {
      errors.push(err(id, "rust-case", `cargo test did not pass ${name}`));
    }
  }
  return errors;
}

export async function evaluateStp12({ repoRoot = REPO_ROOT } = {}) {
  const errors = [...validateProduct()];
  for (const id of NODE_MANDATORY_CASES.STP12 || []) {
    if (!STP12_MANDATORY_CASES.includes(id)) {
      errors.push(err("STP12-jsdoc-generic", "unknown-row", `unowned mandatory case ${id}`));
    }
  }
  errors.push(...assertRustCases(runRustCases(repoRoot)));
  return { errors };
}
