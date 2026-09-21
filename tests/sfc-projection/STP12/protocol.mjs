import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { NODE_MANDATORY_CASES } from "../../../scripts/sfc-projection/node-mandatory-cases.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");

export const STP12_MANDATORY_CASES = Object.freeze([
  "STP12-checkjs-off",
  "STP12-checkjs-on",
  "STP12-jsdoc-generic",
  "STP12-jsx",
  "STP12-suppression",
]);

const RUST_CASES = Object.freeze({
  "STP12-checkjs-off": ["vue_compiler_js_unchecked_script_keeps_template_projection"],
  "STP12-checkjs-on": ["vue_compiler_js_check_directive_is_a_leading_pragma"],
  "STP12-jsdoc-generic": [
    "vue_compiler_jsdoc_generic_uses_public_instance_contract",
    "javascript_setup_companions_match_the_published_consumer_carriers",
  ],
  "STP12-jsx": [
    "vue_compiler_jsx_keeps_authored_jsx_expression",
    "jsx_mode_instance_declaration_uses_public_constructor_bridge",
  ],
  "STP12-suppression": ["vue_compiler_js_projection_never_injects_nocheck"],
});

function err(caseId, code, message) {
  return { caseId, code, message };
}

export function runRustCases(repoRoot = REPO_ROOT) {
  const commands = [
    ["verter_compiler", "vue_compiler_js"],
    ["verter_compiler", "jsx_mode_instance_declaration_uses_public_constructor_bridge"],
    ["verter_session", "javascript_setup_companions_match_the_published_consumer_carriers"],
  ];
  const results = commands.map(([packageName, filter]) =>
    spawnSync("cargo", ["test", "-p", packageName, "--lib", filter, "--", "--test-threads=1"], {
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
  for (const [id, names] of Object.entries(RUST_CASES)) {
    for (const name of names) {
      if (!output.includes(`${name} ... ok`) || output.includes(`${name} ... FAILED`)) {
        errors.push(err(id, "rust-case", `cargo test did not pass ${name}`));
      }
    }
  }
  return errors;
}

export async function evaluateStp12({ repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  for (const id of NODE_MANDATORY_CASES.STP12 || []) {
    if (!STP12_MANDATORY_CASES.includes(id)) {
      errors.push(err("STP12-jsdoc-generic", "unknown-row", `unowned mandatory case ${id}`));
    }
  }
  errors.push(...assertRustCases(runRustCases(repoRoot)));
  return { errors };
}
