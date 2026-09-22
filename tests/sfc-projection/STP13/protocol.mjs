/**
 * STP13 Classic Options API and combined-script compatibility protocol.
 *
 * Validates the named products exist on the dormant projection backend and
 * runs the owning Rust tests per mandatory case: Options `this` members,
 * mixin/extends inheritance, combined coexistence without template leakage,
 * shadowed-macro rejection, and constructor-shaped InstanceType.
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { NODE_MANDATORY_CASES } from "../../../scripts/sfc-projection/node-mandatory-cases.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const OPTIONS_API_RS = "crates/verter_compiler/src/ide/vue_projection/options_api.rs";
const BACKEND_RS = "crates/verter_compiler/src/framework_common/vue_projection_backend.rs";

export const STP13_MANDATORY_CASES = Object.freeze([
  "STP13-options-this",
  "STP13-mixins",
  "STP13-combined",
  "STP13-shadowed-macro",
  "STP13-options-instance",
]);

export const ACCEPTED_PRODUCTS = Object.freeze([
  "OptionsComponentProjection",
  "CombinedScriptProjection",
  "OptionsTemplateBindingView",
]);

const RUST_CASES = Object.freeze({
  "STP13-options-this": ["stp13_options_this_members_keep_contextual_kinds"],
  "STP13-mixins": [
    "stp13_mixins_extends_components_and_directives_recorded",
    "stp13_identifier_mixins_are_opaque_not_dropped",
  ],
  "STP13-combined": [
    "stp13_combined_coexists_without_template_leakage",
    "stp13_opaque_sources_recorded_never_invented",
    "options_projection_reads_admitted_carrier_blocks",
  ],
  "STP13-shadowed-macro": [
    "stp13_shadowed_macro_is_an_ordinary_call",
    "stp13_vue_macro_import_stays_a_macro",
    "stp13_type_only_binding_does_not_shadow",
    "stp13_local_wrapper_binding_is_an_ordinary_call",
    "stp13_vue_import_alias_still_unwraps_by_binding",
    "stp13_vue_import_masquerade_stays_an_ordinary_call",
  ],
  "STP13-options-instance": [
    "stp13_options_instance_preserves_constructor_shape",
    "stp13_plain_object_export_is_unwrapped_without_wrapper",
  ],
});

function err(caseId, code, message) {
  return { caseId, code, message };
}

export function validateStp13Products({ repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  for (const rel of [OPTIONS_API_RS, BACKEND_RS]) {
    if (!fs.existsSync(path.join(repoRoot, rel))) {
      errors.push(err("STP13-combined", "removed-fixture", `missing ${rel}`));
    }
  }
  const optionsAbs = path.join(repoRoot, OPTIONS_API_RS);
  if (fs.existsSync(optionsAbs)) {
    const options = fs.readFileSync(optionsAbs, "utf8");
    // Products must be exported items (`pub struct`), not passing mentions
    // in comments or string literals; the owning entry must be public too.
    // Syntactic validity itself is proven physically by the cargo test runs
    // below, which compile this crate.
    for (const product of ACCEPTED_PRODUCTS) {
      const exported = new RegExp(`pub\\s+struct\\s+${product}\\b`).test(options);
      if (!exported) {
        errors.push(
          err("STP13-combined", "removed-fixture", `missing exported product ${product}`),
        );
      }
    }
    if (!/pub\s+fn\s+project_options_pair\s*\(/.test(options)) {
      errors.push(err("STP13-combined", "removed-fixture", "project_options_pair is missing"));
    }
  }
  const backendAbs = path.join(repoRoot, BACKEND_RS);
  if (fs.existsSync(backendAbs)) {
    const backend = fs.readFileSync(backendAbs, "utf8");
    if (!/pub\s+fn\s+options_projection\s*\(/.test(backend)) {
      errors.push(
        err(
          "STP13-combined",
          "removed-fixture",
          "VueProjectionBackend::options_projection is missing",
        ),
      );
    }
    if (!/pub\s+struct\s+VueProjectionBackend\b/.test(backend)) {
      errors.push(
        err("STP13-combined", "removed-fixture", "VueProjectionBackend is missing"),
      );
    }
    if (!/fn\s+project_ide\s*\(/.test(backend)) {
      errors.push(err("STP13-combined", "removed-fixture", "existing project_ide route missing"));
    }
  }
  return errors;
}

export function runRustCases(repoRoot = REPO_ROOT) {
  // Both runs compile the owning crate (physical syntactic proof) and
  // execute the Options suites: unit facts plus the production
  // `options_projection` path over real admitted `.vue` carrier bytes.
  const invocations = [
    ["test", "-p", "verter_compiler", "--lib", "ide::vue_projection::options_api"],
    [
      "test",
      "-p",
      "verter_compiler",
      "--test",
      "main",
      "options_projection_reads_admitted_carrier_blocks",
    ],
  ];
  const results = invocations.map((args) =>
    spawnSync("cargo", [...args, "--", "--test-threads=1"], {
      cwd: repoRoot,
      encoding: "utf8",
      windowsHide: true,
      timeout: 300000,
      env: process.env,
    }),
  );
  return {
    status: results.every((result) => result.status === 0) ? 0 : 1,
    error: results.find((result) => result.error)?.error,
    stdout: results.map((result) => `${result.stdout || ""}${result.stderr || ""}`).join("\n"),
  };
}

export function assertRustCases(run) {
  const errors = [];
  if (run.error) {
    return STP13_MANDATORY_CASES.map((id) => err(id, "missing-check", run.error.message));
  }
  const output = String(run.stdout || "");
  if (run.status !== 0 || !/test result: ok\. [1-9]\d* passed; 0 failed/.test(output)) {
    return STP13_MANDATORY_CASES.map((id) =>
      err(id, "rust-case", `cargo test did not complete the Options cases (status=${run.status})`),
    );
  }
  for (const [id, names] of Object.entries(RUST_CASES)) {
    for (const name of names) {
      if (!output.includes(`${name} ... ok`) || output.includes(`${name} ... FAILED`)) {
        errors.push(err(id, "rust-case", `cargo test did not pass ${name}`));
      }
    }
  }
  return errors;
}

export async function evaluateStp13({ repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  errors.push(...validateStp13Products({ repoRoot }));
  for (const id of NODE_MANDATORY_CASES.STP13 || []) {
    if (!STP13_MANDATORY_CASES.includes(id)) {
      errors.push(err("STP13-combined", "unknown-row", `unowned mandatory case ${id}`));
    }
  }
  errors.push(...assertRustCases(runRustCases(repoRoot)));
  return { errors };
}
