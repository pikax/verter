/**
 * STP11 statement-oriented setup and module lowering protocol.
 *
 * Joins the product declaration, rejects the forbidden designs through dirty
 * twins, and runs the owning Rust tests for universal binder, scope, await,
 * assertion grammar, and single-body placement.
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { NODE_MANDATORY_CASES } from "../../../scripts/sfc-projection/node-mandatory-cases.mjs";

const STP11_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(STP11_DIR, "../../..");
const SCRIPT_SETUP_RS = "crates/verter_compiler/src/ide/vue_projection/script_setup.rs";
const BACKEND_RS = "crates/verter_compiler/src/framework_common/vue_projection_backend.rs";

export const STP11_MANDATORY_CASES = Object.freeze([
  "STP11-universal",
  "STP11-scope",
  "STP11-await",
  "STP11-assertion",
  "STP11-one-body",
]);

export const ACCEPTED_PRODUCTS = Object.freeze([
  "TsSetupProjection",
  "ModuleScopeProjection",
  "UniversalSetupBinder",
]);

export const DIRTY_SPECIALIZED = { binder: { specializedFromParentUse: true } };
export const DIRTY_DUPLICATE_BODY = { body: { duplicateBodyInPublicAndChecking: true } };
export const DIRTY_SECOND_PARSE = { grammar: { secondTsxParseForTypeScriptGrammar: true } };
export const DIRTY_PROMISE_INSTANCE = { await: { instanceTypePromiseWrapped: true } };

const RUST_CASE_FILTERS = Object.freeze({
  "STP11-universal": "stp11_universal_binder_keeps_constraints_and_carries_no_arguments",
  "STP11-scope": "stp11_scope_module_imports_exports_and_setup_locals_keep_scope",
  "STP11-await": "stp11_await_top_level_only_and_nested_functions_excluded",
  "STP11-assertion":
    "stp11_assertion_ts_angle_assertion_and_tsx_element_parse_once_under_own_grammar",
  "STP11-one-body": "stp11_one_body_rejects_duplicate_setup_body_placement",
});

function err(caseId, code, message) {
  return { caseId, code, message };
}

export function cloneJson(value) {
  return structuredClone(value);
}

export function loadStp11Product(file = "script-setup-projection.json") {
  return JSON.parse(
    fs.readFileSync(path.join(REPO_ROOT, "tests/sfc-projection/STP11/products", file), "utf8"),
  );
}

export function assertScriptPolicy(product = loadStp11Product()) {
  const errors = [];
  if (product.binder?.specializedFromParentUse) {
    errors.push(
      err("STP11-universal", "specialized-generic-body", "body specialized from a parent use"),
    );
  }
  if (product.binder?.universal !== true || product.binder?.bodyCheckedOnce !== true) {
    errors.push(err("STP11-universal", "binder-not-universal", "binder must check the body once"));
  }
  if (product.body?.duplicateBodyInPublicAndChecking) {
    errors.push(
      err("STP11-one-body", "duplicate-body-checker", "setup body placed in public and checking"),
    );
  }
  if (product.grammar?.secondTsxParseForTypeScriptGrammar) {
    errors.push(
      err("STP11-assertion", "second-tsx-parse", "second TSX parse repairs TypeScript grammar"),
    );
  }
  if (
    product.grammar?.grammarFollowsAuthoredLang !== true ||
    product.grammar?.parsesPerBlock !== 1
  ) {
    errors.push(
      err("STP11-assertion", "grammar-policy", "one parse per block under authored lang"),
    );
  }
  if (product.await?.instanceTypePromiseWrapped) {
    errors.push(err("STP11-await", "promise-instance", "async setup exposes Promise<Instance>"));
  }
  if (product.await?.templateCallbackReturnDomainChanged) {
    errors.push(err("STP11-await", "callback-domain", "template callback return domain changed"));
  }
  if (product.macros?.sameNamedLexicalFunctionIsMacro) {
    errors.push(err("STP11-scope", "macro-by-name", "lexical function treated as a macro"));
  }
  return errors;
}

export function validateStp11Products(product = loadStp11Product()) {
  const errors = [];
  if (product.schema !== "ScriptSetupProjection") {
    errors.push(err("STP11-scope", "removed-fixture", "ScriptSetupProjection schema"));
  }
  const named = new Set(product.products || []);
  for (const id of ACCEPTED_PRODUCTS) {
    if (!named.has(id)) errors.push(err("STP11-scope", "removed-fixture", `missing product ${id}`));
  }
  for (const rel of [SCRIPT_SETUP_RS, BACKEND_RS]) {
    if (!fs.existsSync(path.join(REPO_ROOT, rel))) {
      errors.push(err("STP11-scope", "removed-fixture", `missing ${rel}`));
    }
  }
  const backendAbs = path.join(REPO_ROOT, BACKEND_RS);
  if (fs.existsSync(backendAbs)) {
    const backend = fs.readFileSync(backendAbs, "utf8");
    if (!backend.includes("fn script_projection(")) {
      errors.push(
        err("STP11-scope", "removed-fixture", "VueProjectionBackend::script_projection is missing"),
      );
    }
    if (!backend.includes("fn project_ide(")) {
      errors.push(err("STP11-scope", "removed-fixture", "existing project_ide route missing"));
    }
  }
  errors.push(...assertScriptPolicy(product));
  return errors;
}

export function evaluateRejectTwins({ product = loadStp11Product() } = {}) {
  const errors = [];
  const twins = [
    [DIRTY_SPECIALIZED, "STP11-universal"],
    [DIRTY_DUPLICATE_BODY, "STP11-one-body"],
    [DIRTY_SECOND_PARSE, "STP11-assertion"],
    [DIRTY_PROMISE_INSTANCE, "STP11-await"],
  ];
  for (const [patch, caseId] of twins) {
    const dirty = cloneJson(product);
    for (const [key, value] of Object.entries(patch)) Object.assign(dirty[key], value);
    if (!assertScriptPolicy(dirty).some((row) => row.caseId === caseId)) {
      errors.push(err(caseId, "twin-miss", `${caseId} dirty twin was not rejected`));
    }
  }
  return errors;
}

const CASE_TEST_RE = /test result: (ok|FAILED)\. (\d+) passed; (\d+) failed/;

export function parseCargoSummary(stdout) {
  const match = String(stdout || "").match(CASE_TEST_RE);
  if (!match) return null;
  return { ok: match[1] === "ok", passed: Number(match[2]), failed: Number(match[3]) };
}

export function runScriptSetupRustTests(repoRoot = REPO_ROOT) {
  const result = spawnSync(
    "cargo",
    ["test", "-p", "verter_compiler", "--lib", "ide::vue_projection::", "--", "--test-threads=1"],
    { cwd: repoRoot, encoding: "utf8", windowsHide: true, timeout: 300000, env: process.env },
  );
  const stdout = `${result.stdout || ""}${result.stderr || ""}`;
  return {
    status: result.status,
    error: result.error,
    stdout,
    ok: result.status === 0 && /test result: ok/.test(stdout),
  };
}

export function assertRustCases(run) {
  const errors = [];
  if (run.error) {
    for (const id of STP11_MANDATORY_CASES) {
      errors.push(err(id, "missing-check", `cargo test failed to spawn: ${run.error.message}`));
    }
    return errors;
  }
  const output = run.stdout || "";
  const summary = parseCargoSummary(output);
  if (!summary || run.status !== 0 || !summary.ok || summary.passed === 0) {
    const detail = summary
      ? `status=${run.status} passed=${summary.passed} failed=${summary.failed}`
      : `status=${run.status} (no cargo summary)`;
    for (const id of STP11_MANDATORY_CASES) {
      errors.push(err(id, "rust-case", `cargo test did not execute mandatory cases (${detail})`));
    }
  }
  for (const [caseId, filter] of Object.entries(RUST_CASE_FILTERS)) {
    const failed = output.includes(`${filter} ... FAILED`);
    const passed = output.includes(`${filter} ... ok`);
    if (!passed || failed) {
      errors.push(
        err(
          caseId,
          "rust-case",
          passed ? `${filter} failed` : `cargo test did not pass ${filter} (status=${run.status})`,
        ),
      );
    }
  }
  return errors;
}

export async function evaluateStp11({ skipRust = false, repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  errors.push(...validateStp11Products());
  errors.push(...evaluateRejectTwins());
  for (const id of NODE_MANDATORY_CASES.STP11 || STP11_MANDATORY_CASES) {
    if (!STP11_MANDATORY_CASES.includes(id)) {
      errors.push(err("STP11-scope", "unknown-row", `mandatory case ${id} is not owned by STP11`));
    }
  }
  if (!skipRust) errors.push(...assertRustCases(runScriptSetupRustTests(repoRoot)));
  return { errors };
}
