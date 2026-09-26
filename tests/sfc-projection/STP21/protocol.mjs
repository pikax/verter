/** STP21 event-transport physical proof. */
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { NODE_MANDATORY_CASES } from "../../../scripts/sfc-projection/node-mandatory-cases.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const PRODUCT = "crates/verter_compiler/src/ide/vue_projection/event_keys.rs";
const TEST_FILTER = "ide::vue_projection::event_keys";

export const STP21_MANDATORY_CASES = Object.freeze([
  "STP21-event-alias",
  "STP21-model-key",
  "STP21-static-handler",
  "STP21-modifier",
  "STP21-dynamic-name",
  "STP21-collision",
]);

const PRODUCTS = Object.freeze(["EventTransportPlan", "EventAliasRelation", "ListenerConsumerSet"]);
const RUST_CASES = Object.freeze({
  "STP21-event-alias": ["event_transport_keeps_event_identity_separate_from_listener_key_aliases"],
  "STP21-model-key": [
    "event_transport_preserves_model_modifiers_dynamic_unions_and_listener_objects",
  ],
  "STP21-static-handler": [
    "event_transport_keeps_event_identity_separate_from_listener_key_aliases",
  ],
  "STP21-modifier": [
    "event_transport_preserves_model_modifiers_dynamic_unions_and_listener_objects",
  ],
  "STP21-dynamic-name": [
    "event_transport_preserves_model_modifiers_dynamic_unions_and_listener_objects",
  ],
  "STP21-collision": ["event_transport_keeps_all_colliding_listener_consumers"],
});

const TWINS = Object.freeze([
  [
    "STP21-event-alias",
    "AttributeSyntax::On => {",
    "AttributeSyntax::On | AttributeSyntax::Bind => {",
    RUST_CASES["STP21-event-alias"],
  ],
  [
    "STP21-model-key",
    'format!("update:{}", camelize(argument))',
    "argument.to_string()",
    RUST_CASES["STP21-model-key"],
  ],
  [
    "STP21-modifier",
    "listener_key: listener_key.clone(),\n                        modifiers: op.modifiers.clone(),\n                        synthesized: false,",
    "listener_key: listener_key.clone(),\n                        modifiers: Vec::new(),\n                        synthesized: false,",
    RUST_CASES["STP21-modifier"],
  ],
  [
    "STP21-dynamic-name",
    ".map(|expression| literal_union(&expression.spelling))",
    ".map(|_| Vec::new())",
    RUST_CASES["STP21-dynamic-name"],
  ],
  [
    "STP21-collision",
    "set.collision = set.consumers.len() > 1;",
    "set.collision = false;",
    RUST_CASES["STP21-collision"],
  ],
]);

function err(caseId, code, message) {
  return { caseId, code, message };
}

function cargo(repoRoot, filters = []) {
  const run = spawnSync(
    "cargo",
    ["test", "-p", "verter_compiler", "--lib", TEST_FILTER, "--", "--test-threads=1", ...filters],
    { cwd: repoRoot, encoding: "utf8", timeout: 600000, env: process.env },
  );
  return { status: run.status, error: run.error, output: `${run.stdout || ""}${run.stderr || ""}` };
}

function passed(output, name) {
  return output.includes(`${name} ... ok`) && !output.includes(`${name} ... FAILED`);
}

function failed(output, name) {
  return output.includes(`${name} ... FAILED`);
}

export function validateStp21Products({ repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  for (const rel of [
    PRODUCT,
    "tests/sfc-projection/STP21/manifest.json",
    "tests/sfc-projection/STP21/probes/positive.ts",
    "tests/sfc-projection/STP21/probes/negative.ts",
  ]) {
    if (!fs.existsSync(path.join(repoRoot, rel)))
      errors.push(err("STP21-event-alias", "removed-fixture", `missing ${rel}`));
  }
  try {
    const manifest = JSON.parse(
      fs.readFileSync(path.join(repoRoot, "tests/sfc-projection/STP21/manifest.json"), "utf8"),
    );
    for (const id of STP21_MANDATORY_CASES) {
      if (!manifest.mandatoryCases?.includes(id))
        errors.push(err(id, "unknown-row", "manifest omits mandatory case"));
    }
    for (const product of PRODUCTS) {
      if (!manifest.products?.includes(product))
        errors.push(err("STP21-event-alias", "removed-fixture", `manifest omits ${product}`));
    }
  } catch {
    errors.push(err("STP21-event-alias", "removed-fixture", "manifest is not valid JSON"));
  }
  return errors;
}

function assertClean(repoRoot) {
  const run = cargo(repoRoot);
  if (run.error || run.status !== 0)
    return STP21_MANDATORY_CASES.map((id) =>
      err(id, "rust-case", "clean event transport suite failed"),
    );
  const errors = [];
  for (const [id, names] of Object.entries(RUST_CASES)) {
    for (const name of names)
      if (!passed(run.output, name)) errors.push(err(id, "rust-case", `did not pass ${name}`));
  }
  return errors;
}

function assertTwins(repoRoot) {
  const abs = path.join(repoRoot, PRODUCT);
  const original = fs.readFileSync(abs, "utf8");
  const errors = [];
  for (const [id, find, replace, names] of TWINS) {
    if (original.split(find).length !== 2) {
      errors.push(err(id, "dirty-twin-unproven", `mutation anchor is not unique: ${find}`));
      continue;
    }
    fs.writeFileSync(abs, original.replace(find, replace));
    try {
      const run = cargo(repoRoot, names);
      if (run.error || run.status === 0 || !names.every((name) => failed(run.output, name))) {
        errors.push(
          err(id, "dirty-twin-unproven", "applied mutation did not fail its discriminator"),
        );
      }
    } finally {
      fs.writeFileSync(abs, original);
    }
  }
  if (fs.readFileSync(abs, "utf8") !== original) {
    fs.writeFileSync(abs, original);
    errors.push(err("STP21-event-alias", "missing-check", "mutated source was not restored"));
  }
  return errors;
}

export async function evaluateStp21({ repoRoot = REPO_ROOT } = {}) {
  const errors = validateStp21Products({ repoRoot });
  for (const id of NODE_MANDATORY_CASES.STP21 || []) {
    if (!STP21_MANDATORY_CASES.includes(id))
      errors.push(err(id, "unknown-row", "unowned mandatory case"));
  }
  if (errors.length === 0) {
    errors.push(...assertClean(repoRoot));
    if (errors.length === 0) errors.push(...assertTwins(repoRoot));
  }
  return { errors };
}
