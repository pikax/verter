/**
 * STP18 One component-use inference transaction and specialized observations protocol.
 *
 * Structural inventory (files, manifest rows) plus fresh physical proof:
 * the owning Rust suites compile the crate and execute over real template
 * bytes and admitted carrier blocks per mandatory case — one construction
 * per use carrying every channel, observations read from the use's own
 * witness, contextual callbacks kept inside the construction, static
 * discriminants kept literal, collected listeners validated against the
 * specialized contract, per-use offset-free specialization keys and
 * independent sibling witnesses. Each exported dirty twin maps to the
 * discriminator test that rejects it; the verifier applies every twin as a
 * source patch against the owned product, requires the discriminator run to
 * fail while it is applied, then restores the clean source — the rejection
 * is reproduced, not asserted from clean passes.
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { NODE_MANDATORY_CASES } from "../../../scripts/sfc-projection/node-mandatory-cases.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const PRODUCT_RS = "crates/verter_compiler/src/ide/vue_projection/component_use.rs";
const BACKEND_RS = "crates/verter_compiler/src/framework_common/vue_projection_backend.rs";
const LIB_FILTER = "ide::vue_projection::component_use";

export const STP18_MANDATORY_CASES = Object.freeze([
  "STP18-single-witness",
  "STP18-uncoupled",
  "STP18-contextual",
  "STP18-literal",
  "STP18-handler-check",
  "STP18-fresh-id",
  "STP18-script-template-parity",
]);

export const ACCEPTED_PRODUCTS = Object.freeze([
  "ComponentUseWitness",
  "SpecializedUseObservation",
  "InferenceTransaction",
]);

// Rejected mutations (never applied to the landed tree; the clean product
// must fail each one): a model value split out of the construction, slot /
// event / model observations read from the uninstantiated component, bound
// callbacks erased to `any` so they lose their contextual type, a static
// discriminant widened to `string`, collected listeners emitted as repeated
// construction members, a specialization key that follows source offsets,
// and one witness shared by sibling uses.
export const DIRTY_SPLIT_MODEL_CHANNEL = { transaction: { model: "post-check" } };
export const DIRTY_UNINSTANTIATED_OBSERVATION = { observation: { owner: "component" } };
export const DIRTY_CALLBACK_CONTEXT_ERASED = { member: { expression: "as-any" } };
export const DIRTY_WIDENED_DISCRIMINANT = { member: { staticText: "string" } };
export const DIRTY_REPEATED_LISTENER_MEMBER = { listener: { collected: "member" } };
export const DIRTY_OFFSET_KEYED_SPECIALIZATION = { specialization: { key: "offset" } };
export const DIRTY_SHARED_SIBLING_WITNESS = { witness: { binding: "component" } };

const CARRIER_TEST = "component_uses_reads_admitted_carrier_blocks";

const RUST_CASES = Object.freeze({
  "STP18-single-witness": [
    "component_use_single_witness_carries_every_channel_once",
    "probe_fixtures_are_the_rendered_products",
    "table_probe_fixture_is_the_rendered_declaration",
  ],
  "STP18-uncoupled": ["component_use_observations_read_the_specialized_witness"],
  "STP18-contextual": ["component_use_contextual_callbacks_stay_in_the_construction"],
  "STP18-literal": ["component_use_static_discriminant_stays_a_literal"],
  "STP18-handler-check": [
    "component_use_collected_listeners_validate_against_the_specialized_contract",
    "component_use_listener_contracts_follow_the_runtime_listener_key",
    "component_use_non_contributors_are_excluded_with_reasons",
  ],
  "STP18-fresh-id": [
    "component_use_specialization_is_per_use_and_offset_free",
    "component_use_products_are_deterministic_and_incomplete_plans_stay_incomplete",
  ],
  "STP18-script-template-parity": ["component_use_sibling_uses_keep_independent_witnesses"],
});

// Each rejected mutation is a source patch against the owned product plus
// the discriminator tests that must FAIL while it is applied: lib
// discriminators run in the unit lane, the carrier test in the production
// `--test main` lane.
const DIRTY_TWIN_PATCHES = Object.freeze({
  DIRTY_SPLIT_MODEL_CHANNEL: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "WriteValue::ModelUpdate => Some(ExclusionReason::ModelUpdate),",
        replace:
          "WriteValue::ModelUpdate => Some(ExclusionReason::ModelUpdate),\n                WriteValue::Expression if write.syntax == AttributeSyntax::Model => Some(ExclusionReason::ModelUpdate),",
      }),
    ]),
    discriminators: Object.freeze([
      "component_use_single_witness_carries_every_channel_once",
      CARRIER_TEST,
    ]),
  }),
  DIRTY_UNINSTANTIATED_OBSERVATION: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: 'let witness = format!("typeof {binding}");',
        replace:
          'let witness = format!("InstanceType<typeof {}>", binding.trim_start_matches(WITNESS_PREFIX));',
      }),
    ]),
    discriminators: Object.freeze([
      "component_use_observations_read_the_specialized_witness",
      CARRIER_TEST,
    ]),
  }),
  DIRTY_CALLBACK_CONTEXT_ERASED: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: 'Self::Expression { spelling, .. } => format!("({spelling})"),',
        replace: 'Self::Expression { spelling, .. } => format!("(({spelling}) as any)"),',
      }),
    ]),
    discriminators: Object.freeze([
      "component_use_contextual_callbacks_stay_in_the_construction",
      CARRIER_TEST,
    ]),
  }),
  DIRTY_WIDENED_DISCRIMINANT: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "Self::StaticText(text) => quote(text),",
        replace: 'Self::StaticText(text) => format!("String({})", quote(text)),',
      }),
    ]),
    discriminators: Object.freeze([
      "component_use_static_discriminant_stays_a_literal",
      CARRIER_TEST,
    ]),
  }),
  DIRTY_REPEATED_LISTENER_MEMBER: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "MergeRule::Accumulate if constructible && !placed(listeners) => {",
        replace: "MergeRule::Accumulate if constructible => {",
      }),
    ]),
    discriminators: Object.freeze([
      "component_use_collected_listeners_validate_against_the_specialized_contract",
      CARRIER_TEST,
    ]),
  }),
  DIRTY_OFFSET_KEYED_SPECIALIZATION: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "let specialization = specialization_key(use_, &component, &transaction, &observations);",
        replace:
          'let specialization = specialization_key(use_, &format!("{component}@{}", occurrence.start), &transaction, &observations);',
      }),
    ]),
    discriminators: Object.freeze(["component_use_specialization_is_per_use_and_offset_free"]),
  }),
  DIRTY_SHARED_SIBLING_WITNESS: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: 'let binding = format!("{WITNESS_PREFIX}{}", &use_.id.digest_hex()[..16]);',
        replace: 'let binding = format!("{WITNESS_PREFIX}{component}");',
      }),
    ]),
    discriminators: Object.freeze([
      "component_use_sibling_uses_keep_independent_witnesses",
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

export function validateStp18Products({ repoRoot = REPO_ROOT } = {}) {
  // Structural inventory only: every behavior claim below is proven by
  // the fresh `cargo test` run in assertRustCases, never by matching
  // source text.
  const errors = [];
  for (const rel of [
    PRODUCT_RS,
    BACKEND_RS,
    "tests/sfc-projection/STP18/manifest.json",
    "tests/sfc-projection/STP18/probes/positive.ts",
    "tests/sfc-projection/STP18/probes/negative.ts",
    "tests/sfc-projection/STP18/probes/components/Table.vue.ts",
  ]) {
    if (!fs.existsSync(path.join(repoRoot, rel))) {
      errors.push(err("STP18-single-witness", "removed-fixture", `missing ${rel}`));
    }
  }
  const manifestAbs = path.join(repoRoot, "tests/sfc-projection/STP18/manifest.json");
  if (fs.existsSync(manifestAbs)) {
    let manifest = null;
    try {
      manifest = JSON.parse(fs.readFileSync(manifestAbs, "utf8"));
    } catch {
      errors.push(
        err("STP18-single-witness", "removed-fixture", "manifest.json is not valid JSON"),
      );
    }
    if (manifest !== null) {
      const mandatoryCases = Array.isArray(manifest.mandatoryCases) ? manifest.mandatoryCases : [];
      const products = Array.isArray(manifest.products) ? manifest.products : [];
      for (const id of STP18_MANDATORY_CASES) {
        if (!mandatoryCases.includes(id)) {
          errors.push(err(id, "unknown-row", `manifest is missing mandatory case ${id}`));
        }
      }
      for (const product of ACCEPTED_PRODUCTS) {
        if (!products.includes(product)) {
          errors.push(
            err(
              "STP18-single-witness",
              "removed-fixture",
              `manifest is missing product ${product}`,
            ),
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
  // execute the component-use suites: unit facts over real template bytes
  // plus the production `component_uses` path over admitted `.vue`
  // carrier bytes. Each lane keeps its own result so a zero-test
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
    return STP18_MANDATORY_CASES.map((id) => err(id, "missing-check", run.error.message));
  }
  const output = String(run.stdout || "");
  // Per-lane evidence: each cargo lane must report its own passing
  // `test result` line, so a zero-test lane fails instead of hiding.
  const lanes = Array.isArray(run.lanes) ? run.lanes : [{ status: run.status, stdout: output }];
  for (const [index, lane] of lanes.entries()) {
    const laneOutput = String(lane.stdout || "");
    if (lane.status !== 0 || !/test result: ok\. [1-9]\d* passed; 0 failed/.test(laneOutput)) {
      return STP18_MANDATORY_CASES.map((id) =>
        err(
          id,
          "rust-case",
          `cargo lane ${index} did not execute its component-use cases (status=${lane.status})`,
        ),
      );
    }
  }
  if (!testPassed(output, CARRIER_TEST)) {
    errors.push(
      err(
        "STP18-single-witness",
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
    DIRTY_SPLIT_MODEL_CHANNEL,
    DIRTY_UNINSTANTIATED_OBSERVATION,
    DIRTY_CALLBACK_CONTEXT_ERASED,
    DIRTY_WIDENED_DISCRIMINANT,
    DIRTY_REPEATED_LISTENER_MEMBER,
    DIRTY_OFFSET_KEYED_SPECIALIZATION,
    DIRTY_SHARED_SIBLING_WITNESS,
  };
  for (const [twin, value] of Object.entries(twins)) {
    if (value === undefined || DIRTY_TWIN_PATCHES[twin] === undefined) {
      errors.push(
        err("STP18-handler-check", "dirty-twin-unproven", `${twin} has no applied patch`),
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
    return [err("STP18-handler-check", "missing-check", `${PRODUCT_RS} is unreadable`)];
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
          "STP18-handler-check",
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
          "STP18-handler-check",
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
          "STP18-handler-check",
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
          "STP18-handler-check",
          "dirty-twin-unproven",
          `${twin} applied but its discriminators did not all fail: the mutation is not discriminated`,
        ),
      );
    }
  }
  if (fs.readFileSync(abs, "utf8") !== original) {
    fs.writeFileSync(abs, original);
    errors.push(
      err("STP18-handler-check", "missing-check", "mutated source was not restored to clean"),
    );
  }
  return errors;
}

export async function evaluateStp18({ repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  errors.push(...validateStp18Products({ repoRoot }));
  for (const id of NODE_MANDATORY_CASES.STP18 || []) {
    if (!STP18_MANDATORY_CASES.includes(id)) {
      errors.push(err("STP18-single-witness", "unknown-row", `unowned mandatory case ${id}`));
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
