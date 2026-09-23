/**
 * STP17 Ordered Vue attribute operations and runtime-key interpretation protocol.
 *
 * Structural inventory (files, manifest rows) plus fresh physical proof:
 * the owning Rust suites compile the crate and execute over real template
 * bytes and admitted carrier blocks per mandatory case — raw spellings
 * with distinct runtime keys and consumer channels, listeners accumulating
 * across an interleaved spread, later definite overwrites, opaque spreads
 * kept as possible writers, and prop/event collisions validating every
 * reachable channel. Each exported dirty twin maps to the discriminator
 * test that rejects it; the verifier applies every twin as a source patch
 * against the owned product, requires the discriminator run to fail while
 * it is applied, then restores the clean source — the rejection is
 * reproduced, not asserted from clean passes.
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { NODE_MANDATORY_CASES } from "../../../scripts/sfc-projection/node-mandatory-cases.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const PRODUCT_RS = "crates/verter_compiler/src/ide/vue_projection/attribute_operations.rs";
const BACKEND_RS = "crates/verter_compiler/src/framework_common/vue_projection_backend.rs";
const LIB_FILTER = "ide::vue_projection::attribute_operations";

export const STP17_MANDATORY_CASES = Object.freeze([
  "STP17-spellings",
  "STP17-merge",
  "STP17-overwrite",
  "STP17-optional-spread",
  "STP17-collision",
]);

export const ACCEPTED_PRODUCTS = Object.freeze([
  "VueAttributeSequence",
  "RuntimePropertyKeyPlan",
  "AttributeConsumerRelation",
]);

// Rejected mutations (never applied to the landed tree; the clean product
// must fail each one): normalizing `:on-save` as the `save` listener,
// last-wins listeners, an earlier write surviving a later definite
// overwrite, an opaque spread treated as an unconditional overwrite, and
// validating only the first reachable channel of a colliding key.
export const DIRTY_BOUND_KEY_CAMELIZED = { key: { bindAlwaysCamelized: true } };
export const DIRTY_LAST_WINS_LISTENERS = { merge: { listenerRule: "overwrite" } };
export const DIRTY_EARLIER_WRITE_SURVIVES = { merge: { overwriteDrain: false } };
export const DIRTY_SPREAD_UNCONDITIONAL = { spread: { certainty: "definite" } };
export const DIRTY_EASIER_CHANNEL_ONLY = { relation: { channels: "first" } };

const CARRIER_TEST = "attribute_operations_reads_admitted_carrier_blocks";

const RUST_CASES = Object.freeze({
  "STP17-spellings": [
    "attribute_ops_spellings_keep_raw_runtime_prop_and_event_lookup_distinct",
    "attribute_ops_sequence_keeps_models_directives_and_modifiers_ordered",
    "attribute_ops_dynamic_component_is_selects_instead_of_writing",
  ],
  "STP17-merge": ["attribute_ops_merge_listeners_accumulate_across_interleaved_v_bind"],
  "STP17-overwrite": [
    "attribute_ops_overwrite_later_definite_prop_wins",
    "attribute_ops_products_are_deterministic_and_incomplete_plans_stay_incomplete",
  ],
  "STP17-optional-spread": ["attribute_ops_optional_spread_keeps_earlier_key_possible"],
  "STP17-collision": ["attribute_ops_collision_validates_every_reachable_channel"],
});

// Each rejected mutation is a source patch against the owned product plus
// the discriminator tests that must FAIL while it is applied: lib
// discriminators run in the unit lane, the carrier test in the production
// `--test main` lane.
const DIRTY_TWIN_PATCHES = Object.freeze({
  DIRTY_BOUND_KEY_CAMELIZED: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: 'let mut key = if has("camel") {',
        replace: "let mut key = if true {",
      }),
    ]),
    discriminators: Object.freeze([
      "attribute_ops_spellings_keep_raw_runtime_prop_and_event_lookup_distinct",
      CARRIER_TEST,
    ]),
  }),
  DIRTY_LAST_WINS_LISTENERS: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "_ if is_on(key) => MergeRule::Accumulate,",
        replace: "_ if is_on(key) => MergeRule::Overwrite,",
      }),
    ]),
    discriminators: Object.freeze([
      "attribute_ops_merge_listeners_accumulate_across_interleaved_v_bind",
      CARRIER_TEST,
    ]),
  }),
  DIRTY_EARLIER_WRITE_SURVIVES: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "overridden.extend(contributors.drain(..last).map(|c| c.op_index));",
        replace: "let _ = last;",
      }),
    ]),
    discriminators: Object.freeze([
      "attribute_ops_overwrite_later_definite_prop_wins",
      CARRIER_TEST,
    ]),
  }),
  DIRTY_SPREAD_UNCONDITIONAL: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "ordered.push((spread.group, possible(spread.op_index)));",
        replace: "ordered.push((spread.group, definite(spread.op_index)));",
      }),
    ]),
    discriminators: Object.freeze([
      "attribute_ops_optional_spread_keeps_earlier_key_possible",
      CARRIER_TEST,
    ]),
  }),
  DIRTY_EASIER_CHANNEL_ONLY: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "for channel in &channels {",
        replace: "for channel in channels.iter().take(1) {",
      }),
    ]),
    discriminators: Object.freeze([
      "attribute_ops_collision_validates_every_reachable_channel",
      CARRIER_TEST,
    ]),
  }),
});

export function dirtyTwinExpectations() {
  return Object.fromEntries(
    Object.entries(DIRTY_TWIN_PATCHES).map(([twin, spec]) => [twin, [...spec.discriminators]]),
  );
}

function err(caseId, code, message) {
  return { caseId, code, message };
}

export function validateStp17Products({ repoRoot = REPO_ROOT } = {}) {
  // Structural inventory only: every behavior claim below is proven by
  // the fresh `cargo test` run in assertRustCases, never by matching
  // source text.
  const errors = [];
  for (const rel of [
    PRODUCT_RS,
    BACKEND_RS,
    "tests/sfc-projection/STP17/manifest.json",
    "tests/sfc-projection/STP17/probes/positive.ts",
    "tests/sfc-projection/STP17/probes/negative.ts",
    "tests/sfc-projection/STP17/probes/components/Saver.vue.d.ts",
  ]) {
    if (!fs.existsSync(path.join(repoRoot, rel))) {
      errors.push(err("STP17-spellings", "removed-fixture", `missing ${rel}`));
    }
  }
  const manifestAbs = path.join(repoRoot, "tests/sfc-projection/STP17/manifest.json");
  if (fs.existsSync(manifestAbs)) {
    let manifest = null;
    try {
      manifest = JSON.parse(fs.readFileSync(manifestAbs, "utf8"));
    } catch {
      errors.push(err("STP17-spellings", "removed-fixture", "manifest.json is not valid JSON"));
    }
    if (manifest !== null) {
      const mandatoryCases = Array.isArray(manifest.mandatoryCases) ? manifest.mandatoryCases : [];
      const products = Array.isArray(manifest.products) ? manifest.products : [];
      for (const id of STP17_MANDATORY_CASES) {
        if (!mandatoryCases.includes(id)) {
          errors.push(err(id, "unknown-row", `manifest is missing mandatory case ${id}`));
        }
      }
      for (const product of ACCEPTED_PRODUCTS) {
        if (!products.includes(product)) {
          errors.push(
            err("STP17-spellings", "removed-fixture", `manifest is missing product ${product}`),
          );
        }
      }
    }
  }
  return errors;
}

function runCargo(repoRoot, args) {
  const result = spawnSync("cargo", args, {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    timeout: 300000,
    env: process.env,
  });
  return {
    status: result.status,
    error: result.error,
    stdout: `${result.stdout || ""}${result.stderr || ""}`,
  };
}

export function runRustCases(repoRoot = REPO_ROOT) {
  // Both runs compile the owning crate (physical syntactic proof) and
  // execute the attribute-operations suites: unit facts over real template
  // bytes plus the production `attribute_operations` path over admitted
  // `.vue` carrier bytes. Each lane keeps its own result so a zero-test
  // lane cannot hide behind the other lane's passes.
  const lanes = [
    ["test", "-p", "verter_compiler", "--lib", LIB_FILTER, "--", "--test-threads=1"],
    ["test", "-p", "verter_compiler", "--test", "main", CARRIER_TEST, "--", "--test-threads=1"],
  ].map((args) => runCargo(repoRoot, args));
  return {
    status: lanes.every((lane) => lane.status === 0) ? 0 : 1,
    error: lanes.find((lane) => lane.error)?.error,
    stdout: lanes.map((lane) => lane.stdout).join("\n"),
    lanes,
  };
}

function testPassed(output, name) {
  return output.includes(`${name} ... ok`) && !output.includes(`${name} ... FAILED`);
}

function testFailed(output, name) {
  return output.includes(`${name} ... FAILED`);
}

export function assertRustCases(run) {
  const errors = [];
  if (run.error) {
    return STP17_MANDATORY_CASES.map((id) => err(id, "missing-check", run.error.message));
  }
  const output = String(run.stdout || "");
  // Per-lane evidence: each cargo lane must report its own passing
  // `test result` line, so a zero-test lane fails instead of hiding.
  const lanes = Array.isArray(run.lanes) ? run.lanes : [{ status: run.status, stdout: output }];
  for (const [index, lane] of lanes.entries()) {
    const laneOutput = String(lane.stdout || "");
    if (lane.status !== 0 || !/test result: ok\. [1-9]\d* passed; 0 failed/.test(laneOutput)) {
      return STP17_MANDATORY_CASES.map((id) =>
        err(
          id,
          "rust-case",
          `cargo lane ${index} did not execute its attribute-operations cases (status=${lane.status})`,
        ),
      );
    }
  }
  if (!testPassed(output, CARRIER_TEST)) {
    errors.push(
      err(
        "STP17-spellings",
        "rust-case",
        `the production carrier lane did not pass ${CARRIER_TEST}`,
      ),
    );
  }
  for (const [id, names] of Object.entries(RUST_CASES)) {
    for (const name of names) {
      if (!testPassed(output, name)) {
        errors.push(err(id, "rust-case", `cargo test did not pass ${name}`));
      }
    }
  }
  // Consume every exported dirty twin so an unused export fails loudly;
  // physical rejection is proven separately by assertDirtyTwinsRejected.
  const twins = {
    DIRTY_BOUND_KEY_CAMELIZED,
    DIRTY_LAST_WINS_LISTENERS,
    DIRTY_EARLIER_WRITE_SURVIVES,
    DIRTY_SPREAD_UNCONDITIONAL,
    DIRTY_EASIER_CHANNEL_ONLY,
  };
  for (const [twin, value] of Object.entries(twins)) {
    if (value === undefined || DIRTY_TWIN_PATCHES[twin] === undefined) {
      errors.push(
        err("STP17-optional-spread", "dirty-twin-unproven", `${twin} has no applied patch`),
      );
    }
  }
  return errors;
}

export function assertDirtyTwinsRejected(repoRoot = REPO_ROOT) {
  const errors = [];
  const abs = path.join(repoRoot, PRODUCT_RS);
  let original = null;
  try {
    original = fs.readFileSync(abs, "utf8");
  } catch {
    return [err("STP17-optional-spread", "missing-check", `${PRODUCT_RS} is unreadable`)];
  }
  for (const [twin, spec] of Object.entries(DIRTY_TWIN_PATCHES)) {
    let mutated = original;
    let badAnchor = null;
    for (const patch of spec.patches) {
      // The anchor must occur exactly once so the plant provably lands on
      // the intended site and nowhere else.
      if (mutated.split(patch.find).length !== 2) {
        badAnchor = patch.find.slice(0, 80);
        break;
      }
      mutated = mutated.replace(patch.find, patch.replace);
    }
    if (badAnchor !== null || mutated === original) {
      errors.push(
        err(
          "STP17-optional-spread",
          "dirty-twin-unproven",
          `${twin} patch anchor is not unique in the owned product: ${badAnchor}`,
        ),
      );
      continue;
    }
    const libNames = spec.discriminators.filter((name) => name !== CARRIER_TEST);
    const carrierNames = spec.discriminators.filter((name) => name === CARRIER_TEST);
    let run = null;
    let carrierRun = null;
    fs.writeFileSync(abs, mutated);
    try {
      run = runCargo(repoRoot, [
        "test",
        "-p",
        "verter_compiler",
        "--lib",
        LIB_FILTER,
        "--",
        "--test-threads=1",
        ...libNames,
      ]);
      if (carrierNames.length > 0) {
        carrierRun = runCargo(repoRoot, [
          "test",
          "-p",
          "verter_compiler",
          "--test",
          "main",
          "--",
          "--test-threads=1",
          ...carrierNames,
        ]);
      }
    } finally {
      fs.writeFileSync(abs, original);
    }
    if (run.error || carrierRun?.error) {
      errors.push(
        err(
          "STP17-optional-spread",
          "missing-check",
          `${twin} run failed to spawn: ${(run.error || carrierRun.error).message}`,
        ),
      );
      continue;
    }
    const output = String(run.stdout || "");
    const carrierOutput = String(carrierRun?.stdout || "");
    const ran = libNames.every((name) => output.includes(name));
    const carrierRan = carrierNames.every((name) => carrierOutput.includes(name));
    if (!ran || !carrierRan) {
      errors.push(
        err(
          "STP17-optional-spread",
          "missing-check",
          `${twin} run did not execute its discriminators (status=${run.status})`,
        ),
      );
      continue;
    }
    // Rejection proof requires each discriminator to FAIL for the stated
    // mutation: a nonzero status alone is not proof, since an unrelated
    // compile error also exits nonzero while the discriminator never ran.
    const rejected =
      run.status !== 0 &&
      libNames.every((name) => testFailed(output, name)) &&
      (carrierRun === null ||
        (carrierRun.status !== 0 && carrierNames.every((name) => testFailed(carrierOutput, name))));
    if (!rejected) {
      errors.push(
        err(
          "STP17-optional-spread",
          "dirty-twin-unproven",
          `${twin} applied but its discriminators did not all fail: the mutation is not discriminated`,
        ),
      );
    }
  }
  if (fs.readFileSync(abs, "utf8") !== original) {
    fs.writeFileSync(abs, original);
    errors.push(
      err("STP17-optional-spread", "missing-check", "mutated source was not restored to clean"),
    );
  }
  return errors;
}

export async function evaluateStp17({ repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  errors.push(...validateStp17Products({ repoRoot }));
  for (const id of NODE_MANDATORY_CASES.STP17 || []) {
    if (!STP17_MANDATORY_CASES.includes(id)) {
      errors.push(err("STP17-spellings", "unknown-row", `unowned mandatory case ${id}`));
    }
  }
  const before = errors.length;
  errors.push(...assertRustCases(runRustCases(repoRoot)));
  if (errors.length === before) {
    // Clean discriminators pass: now prove each dirty twin fails them.
    errors.push(...assertDirtyTwinsRejected(repoRoot));
  }
  return { errors };
}
