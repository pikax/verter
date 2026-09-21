/**
 * STP14 binder-aware public dependency capture protocol.
 *
 * Joins the product declaration, rejects the forbidden designs through dirty
 * twins, and runs the owning Rust cases for local capture, dependent
 * defaults, `typeof` capture, alias cycles and duplicate declarations.
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { NODE_MANDATORY_CASES } from "../../../scripts/sfc-projection/node-mandatory-cases.mjs";

const STP14_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(STP14_DIR, "../../..");
const BINDER_CAPTURE_RS = "crates/verter_compiler/src/ide/vue_projection/binder_capture.rs";
const BACKEND_RS = "crates/verter_compiler/src/framework_common/vue_projection_backend.rs";

export const STP14_MANDATORY_CASES = Object.freeze([
  "STP14-local-capture",
  "STP14-dependent-default",
  "STP14-typeof-capture",
  "STP14-alias-cycle",
  "STP14-duplicate-error",
]);

export const ACCEPTED_PRODUCTS = Object.freeze([
  "PublicTypeDependencySlice",
  "BinderCapturePlan",
  "LiftedSourceDeclaration",
]);

export const DIRTY_EAGER_EXPANSION = { cycles: { eagerNativeExpansion: true } };
export const DIRTY_HIDDEN_DIAGNOSTIC = { lifting: { suppressesSourceDiagnostics: true } };
export const DIRTY_INVENTED_DUPLICATE = { lifting: { inventsSecondDeclarationForDuplicate: true } };
export const DIRTY_CAPTURING_BINDER = { binder: { alphaRenamesOnModuleScopeCollision: false } };
export const DIRTY_EVALUATING_CAPTURE = { capture: { evaluatesTypes: true } };
export const DIRTY_CROSS_FILE_ALIAS = { capture: { followsImportedAliasesIntoOtherFiles: true } };

const RUST_CASE_FILTERS = Object.freeze({
  "STP14-local-capture":
    "stp14_local_capture_keeps_binder_bound_local_selection_in_the_public_surface",
  "STP14-dependent-default": "stp14_dependent_default_keeps_the_earlier_parameter_bound",
  "STP14-typeof-capture":
    "stp14_typeof_capture_names_value_space_dependencies_in_the_emitted_declaration",
  "STP14-alias-cycle": "stp14_alias_cycle_terminates_and_is_recorded_by_declaration_identity",
  "STP14-duplicate-error": "stp14_duplicate_error_lifts_nothing_and_preserves_every_origin",
});

function err(caseId, code, message) {
  return { caseId, code, message };
}

export function cloneJson(value) {
  return structuredClone(value);
}

export function loadStp14Product(file = "binder-capture.json") {
  return JSON.parse(
    fs.readFileSync(path.join(REPO_ROOT, "tests/sfc-projection/STP14/products", file), "utf8"),
  );
}

export function assertCapturePolicy(product = loadStp14Product()) {
  const errors = [];
  if (product.cycles?.eagerNativeExpansion) {
    errors.push(
      err(
        "STP14-alias-cycle",
        "eager-native-alias-expansion",
        "recursive aliases are expanded instead of closed by declaration identity",
      ),
    );
  }
  if (product.cycles?.terminatesByDeclarationIdentity !== true) {
    errors.push(
      err("STP14-alias-cycle", "cycle-policy", "cycles must terminate by declaration identity"),
    );
  }
  if (product.lifting?.suppressesSourceDiagnostics) {
    errors.push(
      err("STP14-duplicate-error", "hidden-source-diagnostic", "lifting hides a source error"),
    );
  }
  if (product.lifting?.inventsSecondDeclarationForDuplicate) {
    errors.push(
      err(
        "STP14-duplicate-error",
        "invented-duplicate-declaration",
        "a doubly-bound name gets a second conflicting declaration",
      ),
    );
  }
  if (
    product.lifting?.preservesAuthoredVisibility !== true ||
    product.lifting?.preservesSourceOrigin !== true
  ) {
    errors.push(
      err(
        "STP14-duplicate-error",
        "lost-origin",
        "duplicate observations must preserve visibility and source origin",
      ),
    );
  }
  if (product.lifting?.minimalReachableOnly !== true) {
    errors.push(
      err("STP14-local-capture", "over-lifting", "lifting must stay minimal and reachable"),
    );
  }
  if (product.binder?.alphaRenamesOnModuleScopeCollision !== true) {
    errors.push(
      err(
        "STP14-local-capture",
        "binder-captured-by-module-scope",
        "a lifted binder parameter is not alpha-renamed on collision",
      ),
    );
  }
  if (product.binder?.scopesSetupOnly !== true || product.binder?.specializedFromParentUse) {
    errors.push(
      err("STP14-local-capture", "binder-policy", "binder scope or universality is not preserved"),
    );
  }
  if (product.binder?.constraintAndDefaultClosure !== true) {
    errors.push(
      err(
        "STP14-dependent-default",
        "missing-closure",
        "constraint and default references are not closed over",
      ),
    );
  }
  if (product.capture?.evaluatesTypes) {
    errors.push(
      err("STP14-typeof-capture", "native-type-answer", "capture evaluates types natively"),
    );
  }
  if (product.capture?.syntaxAndProvenanceOnly !== true) {
    errors.push(
      err("STP14-typeof-capture", "capture-policy", "capture must be syntax and provenance only"),
    );
  }
  if (product.capture?.followsImportedAliasesIntoOtherFiles) {
    errors.push(
      err(
        "STP14-alias-cycle",
        "cross-file-alias-walk",
        "imported aliases are followed into another file",
      ),
    );
  }
  if (product.runtimeActivationAuthorized) {
    errors.push(
      err("STP14-local-capture", "premature-activation", "runtime activation is not this node's"),
    );
  }
  return errors;
}

export function validateStp14Products(product = loadStp14Product()) {
  const errors = [];
  if (product.schema !== "BinderCapturePlan") {
    errors.push(err("STP14-local-capture", "removed-fixture", "BinderCapturePlan schema"));
  }
  const products = Array.isArray(product.products) ? product.products : [];
  const named = new Set(products);
  for (const id of ACCEPTED_PRODUCTS) {
    if (!named.has(id)) {
      errors.push(err("STP14-local-capture", "removed-fixture", `missing product ${id}`));
    }
  }
  if (
    products.length !== ACCEPTED_PRODUCTS.length ||
    named.size !== products.length ||
    products.some((id) => !ACCEPTED_PRODUCTS.includes(id))
  ) {
    errors.push(
      err("STP14-local-capture", "product-list", "products must exactly match accepted products"),
    );
  }
  for (const rel of [BINDER_CAPTURE_RS, BACKEND_RS]) {
    if (!fs.existsSync(path.join(REPO_ROOT, rel))) {
      errors.push(err("STP14-local-capture", "removed-fixture", `missing ${rel}`));
    }
  }
  const backendAbs = path.join(REPO_ROOT, BACKEND_RS);
  if (fs.existsSync(backendAbs)) {
    const backend = fs.readFileSync(backendAbs, "utf8");
    if (!backend.includes("fn public_type_dependencies(")) {
      errors.push(
        err(
          "STP14-local-capture",
          "removed-fixture",
          "VueProjectionBackend::public_type_dependencies is missing",
        ),
      );
    }
    if (!backend.includes("fn project_ide(")) {
      errors.push(
        err("STP14-local-capture", "removed-fixture", "existing project_ide route missing"),
      );
    }
  }
  errors.push(...assertCapturePolicy(product));
  return errors;
}

export function evaluateRejectTwins({ product = loadStp14Product() } = {}) {
  const errors = [];
  const twins = [
    [DIRTY_EAGER_EXPANSION, "STP14-alias-cycle"],
    [DIRTY_CROSS_FILE_ALIAS, "STP14-alias-cycle"],
    [DIRTY_HIDDEN_DIAGNOSTIC, "STP14-duplicate-error"],
    [DIRTY_INVENTED_DUPLICATE, "STP14-duplicate-error"],
    [DIRTY_CAPTURING_BINDER, "STP14-local-capture"],
    [DIRTY_EVALUATING_CAPTURE, "STP14-typeof-capture"],
  ];
  for (const [patch, caseId] of twins) {
    const dirty = cloneJson(product);
    for (const [key, value] of Object.entries(patch)) Object.assign(dirty[key], value);
    if (!assertCapturePolicy(dirty).some((row) => row.caseId === caseId)) {
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

export function runBinderCaptureRustTests(repoRoot = REPO_ROOT) {
  const result = spawnSync(
    "cargo",
    [
      "test",
      "-p",
      "verter_compiler",
      "--lib",
      "ide::vue_projection::binder_capture",
      "--",
      "--test-threads=1",
    ],
    { cwd: repoRoot, encoding: "utf8", windowsHide: true, timeout: 600000, env: process.env },
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
    for (const id of STP14_MANDATORY_CASES) {
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
    for (const id of STP14_MANDATORY_CASES) {
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

export async function evaluateStp14({ skipRust = false, repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  errors.push(...validateStp14Products());
  errors.push(...evaluateRejectTwins());
  for (const id of NODE_MANDATORY_CASES.STP14 || STP14_MANDATORY_CASES) {
    if (!STP14_MANDATORY_CASES.includes(id)) {
      errors.push(
        err("STP14-local-capture", "unknown-row", `mandatory case ${id} is not owned by STP14`),
      );
    }
  }
  if (!skipRust) errors.push(...assertRustCases(runBinderCaptureRustTests(repoRoot)));
  return { errors };
}
