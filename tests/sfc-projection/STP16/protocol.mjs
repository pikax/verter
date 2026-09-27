/**
 * STP16 Vue public constructor and instance contract protocol.
 *
 * Structural inventory (files, manifest rows) plus fresh physical proof:
 * the owning Rust suites compile the crate and execute over real setup
 * bytes and admitted carrier blocks per mandatory case — one generic
 * construct signature with required props, typed public members, private
 * setup bindings kept off the instance, constraints on the single signature,
 * no call signature, authored field precision, and binder specialization of
 * every public surface. The rendered declaration is pinned byte for byte by
 * the probe fixture the harness type-checks through both engines. Each
 * exported dirty twin is applied as a source patch against the owned
 * product; the verifier requires its discriminators to fail while it is
 * applied, then restores the clean source.
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { NODE_MANDATORY_CASES } from "../../../scripts/sfc-projection/node-mandatory-cases.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const PUBLIC_CONSTRUCTOR_RS = "crates/verter_compiler/src/ide/vue_projection/public_constructor.rs";
const BACKEND_RS = "crates/verter_compiler/src/framework_common/vue_projection_backend.rs";
const CARRIER_TEST = "public_constructor_reads_admitted_carrier_blocks";
const FIXTURE_TEST = "picker_probe_fixture_is_the_rendered_declaration";

export const STP16_MANDATORY_CASES = Object.freeze([
  "STP16-required-api",
  "STP16-public-members",
  "STP16-private-leak",
  "STP16-generic-constraint",
  "STP16-public-callable",
  "STP16-type-precision",
  "STP16-public-specialization",
]);

export const ACCEPTED_PRODUCTS = Object.freeze([
  "VuePublicConstructorContract",
  "PublicInstanceProjection",
  "ConstructorCompatibilityReceipt",
]);

// Rejected designs (the clean product must fail each one): a blanket
// optional props argument, dropped exposed members, a private-binding leak,
// a permissive overload fallback, an unconstrained binder, a callable
// default export, a widened field, and unspecialized public aliases.
export const DIRTY_OPTIONAL_REQUIRED_PROPS = { constructor: { propsParameter: "optional" } };
export const DIRTY_DROPPED_EXPOSED_MEMBERS = { instance: { exposed: "dropped" } };
export const DIRTY_PRIVATE_LEAK = { instance: { privateBindings: "published" } };
export const DIRTY_OVERLOAD_FALLBACK = { constructor: { fallback: "new (...args: any[]): any" } };
export const DIRTY_UNCONSTRAINED_BINDER = { constructor: { constraints: "dropped" } };
export const DIRTY_CALLABLE_EXPORT = { constructor: { signature: "call" } };
export const DIRTY_ANY_FIELD = { props: { authoredType: "Record<string, any>" } };
export const DIRTY_UNSPECIALIZED_ALIASES = { aliases: { binderArguments: "dropped" } };

const RUST_CASES = Object.freeze({
  "STP16-required-api": [
    "required_props_keep_one_constructor_with_a_required_argument",
    FIXTURE_TEST,
    "requirement_probe_fixtures_are_the_rendered_declarations",
    "with_defaults_makes_defaulted_props_omissible",
    "required_flags_read_through_const_assertions",
    "merged_interfaces_join_their_requirements",
  ],
  "STP16-public-members": [
    "instance_publishes_typed_framework_and_exposed_members",
    "expose_provider_is_rendered_from_the_setup_statements",
  ],
  "STP16-private-leak": ["private_setup_bindings_stay_off_the_instance"],
  "STP16-generic-constraint": [
    "generic_constraints_stay_on_the_single_construct_signature",
    "refuses_what_the_setup_projection_refuses",
  ],
  "STP16-public-callable": ["default_export_is_never_callable"],
  "STP16-type-precision": ["rendered_fields_keep_authored_types"],
  "STP16-public-specialization": [
    "binder_arguments_reach_every_public_surface",
    "binder_dependent_runtime_options_are_rendered_over_the_binder",
  ],
});

// Each rejected design is a source patch against the owned product plus the
// unit discriminators that must FAIL while it is applied. The production
// carrier test runs in the clean lane only: it renders through the same
// product, so rebuilding the integration binary per twin adds cost, not
// discrimination.
const DIRTY_TWIN_PATCHES = Object.freeze({
  DIRTY_OPTIONAL_REQUIRED_PROPS: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: 'PropsRequirement::Required => format!("props: {PUBLIC_PROPS}{args}"),',
        replace: 'PropsRequirement::Required => format!("props?: {PUBLIC_PROPS}{args}"),',
      }),
    ]),
    discriminators: Object.freeze([
      "required_props_keep_one_constructor_with_a_required_argument",
      FIXTURE_TEST,
    ]),
  }),
  DIRTY_DROPPED_EXPOSED_MEMBERS: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "if !expose.members.iter().any(|member| member.name == name) {",
        replace: "if name.is_empty() && !expose.members.iter().any(|member| member.name == name) {",
      }),
    ]),
    discriminators: Object.freeze(["instance_publishes_typed_framework_and_exposed_members"]),
  }),
  DIRTY_PRIVATE_LEAK: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: `    PublicInstanceProjection {
        members,
        private_bindings: private.into_iter().map(|(_, name)| name).collect(),
    }`,
        replace: `    members.extend(private.iter().map(|(_, name)| PublicInstanceMember {
        name: name.clone(),
        origin: InstanceMemberOrigin::Exposed,
    }));
    PublicInstanceProjection {
        members,
        private_bindings: Vec::new(),
    }`,
      }),
    ]),
    discriminators: Object.freeze(["private_setup_bindings_stay_off_the_instance"]),
  }),
  DIRTY_OVERLOAD_FALLBACK: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: String.raw`"declare const {PUBLIC_COMPONENT}: {{\n  new {construct_binder}({parameter}): {PUBLIC_INSTANCE}{args};\n"`,
        replace: String.raw`"declare const {PUBLIC_COMPONENT}: {{\n  new {construct_binder}({parameter}): {PUBLIC_INSTANCE}{args};\n  new (...args: any[]): any;\n"`,
      }),
    ]),
    discriminators: Object.freeze([
      "generic_constraints_stay_on_the_single_construct_signature",
      FIXTURE_TEST,
    ]),
  }),
  DIRTY_UNCONSTRAINED_BINDER: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "if let Some(constraint) = &param.constraint {",
        replace: "if let Some(constraint) = param.constraint.as_ref().filter(|_| !with_const) {",
      }),
    ]),
    discriminators: Object.freeze([
      "generic_constraints_stay_on_the_single_construct_signature",
      FIXTURE_TEST,
    ]),
  }),
  DIRTY_CALLABLE_EXPORT: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: String.raw`"declare const {PUBLIC_COMPONENT}: {{\n  new {construct_binder}`,
        replace: String.raw`"declare const {PUBLIC_COMPONENT}: {{\n  {construct_binder}`,
      }),
    ]),
    discriminators: Object.freeze(["default_export_is_never_callable", FIXTURE_TEST]),
  }),
  DIRTY_ANY_FIELD: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "DeclaredSurface::TypeArgument { text, .. } => props.push(self.defaulted(text)),",
        replace:
          'DeclaredSurface::TypeArgument { .. } => props.push("Record<string, any>".to_string()),',
      }),
    ]),
    discriminators: Object.freeze(["rendered_fields_keep_authored_types"]),
  }),
  DIRTY_UNSPECIALIZED_ALIASES: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: 'format!("<{}>", names.join(", "))',
        replace: "names.first().map(|_| String::new()).unwrap_or_default()",
      }),
    ]),
    discriminators: Object.freeze(["binder_arguments_reach_every_public_surface", FIXTURE_TEST]),
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

export function validateStp16Products({ repoRoot = REPO_ROOT } = {}) {
  // Structural inventory only: every behavior claim is proven by the fresh
  // `cargo test` runs in assertRustCases, never by matching source text.
  const errors = [];
  for (const rel of [
    PUBLIC_CONSTRUCTOR_RS,
    BACKEND_RS,
    "tests/sfc-projection/STP16/manifest.json",
    "tests/sfc-projection/STP16/probes/positive.ts",
    "tests/sfc-projection/STP16/probes/negative.ts",
    "tests/sfc-projection/STP16/probes/components/Picker.vue.ts",
  ]) {
    if (!fs.existsSync(path.join(repoRoot, rel))) {
      errors.push(err("STP16-required-api", "removed-fixture", `missing ${rel}`));
    }
  }
  const manifestAbs = path.join(repoRoot, "tests/sfc-projection/STP16/manifest.json");
  if (fs.existsSync(manifestAbs)) {
    let manifest = null;
    try {
      manifest = JSON.parse(fs.readFileSync(manifestAbs, "utf8"));
    } catch {
      errors.push(err("STP16-required-api", "removed-fixture", "manifest.json is not valid JSON"));
    }
    if (manifest !== null) {
      const mandatoryCases = Array.isArray(manifest.mandatoryCases) ? manifest.mandatoryCases : [];
      const products = Array.isArray(manifest.products) ? manifest.products : [];
      for (const id of STP16_MANDATORY_CASES) {
        if (!mandatoryCases.includes(id)) {
          errors.push(err(id, "unknown-row", `manifest is missing mandatory case ${id}`));
        }
      }
      for (const product of ACCEPTED_PRODUCTS) {
        if (!products.includes(product)) {
          errors.push(
            err("STP16-required-api", "removed-fixture", `manifest is missing product ${product}`),
          );
        }
      }
    }
  }
  return errors;
}

function cargo(repoRoot, args) {
  const result = spawnSync("cargo", args, {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
    timeout: 600000,
    env: process.env,
  });
  return {
    status: result.status,
    error: result.error,
    stdout: `${result.stdout || ""}${result.stderr || ""}`,
  };
}

const LIB_LANE = [
  "test",
  "-p",
  "verter_compiler",
  "--lib",
  "ide::vue_projection::public_constructor",
];
const CARRIER_LANE = ["test", "-p", "verter_compiler", "--test", "main"];

export function runRustCases(repoRoot = REPO_ROOT) {
  // Both lanes compile the owning crate and execute the public-constructor
  // suites: unit facts plus the production `public_constructor` path over
  // real admitted `.vue` carrier bytes. Each lane keeps its own result so a
  // zero-test lane cannot hide behind the other lane's passes.
  const lanes = [
    cargo(repoRoot, [...LIB_LANE, "--", "--test-threads=1"]),
    cargo(repoRoot, [...CARRIER_LANE, CARRIER_TEST, "--", "--test-threads=1"]),
  ];
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
    return STP16_MANDATORY_CASES.map((id) => err(id, "missing-check", run.error.message));
  }
  const output = String(run.stdout || "");
  const lanes = Array.isArray(run.lanes) ? run.lanes : [{ status: run.status, stdout: output }];
  for (const [index, lane] of lanes.entries()) {
    const laneOutput = String(lane.stdout || "");
    if (lane.status !== 0 || !/test result: ok\. [1-9]\d* passed; 0 failed/.test(laneOutput)) {
      return STP16_MANDATORY_CASES.map((id) =>
        err(
          id,
          "rust-case",
          `cargo lane ${index} did not execute its public-constructor cases (status=${lane.status})`,
        ),
      );
    }
  }
  if (!testPassed(output, CARRIER_TEST)) {
    errors.push(
      err(
        "STP16-required-api",
        "rust-case",
        `the production carrier lane did not execute ${CARRIER_TEST}`,
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
    DIRTY_OPTIONAL_REQUIRED_PROPS,
    DIRTY_DROPPED_EXPOSED_MEMBERS,
    DIRTY_PRIVATE_LEAK,
    DIRTY_OVERLOAD_FALLBACK,
    DIRTY_UNCONSTRAINED_BINDER,
    DIRTY_CALLABLE_EXPORT,
    DIRTY_ANY_FIELD,
    DIRTY_UNSPECIALIZED_ALIASES,
  };
  for (const [twin, value] of Object.entries(twins)) {
    if (value === undefined || DIRTY_TWIN_PATCHES[twin] === undefined) {
      errors.push(err("STP16-private-leak", "dirty-twin-unproven", `${twin} has no applied patch`));
    }
  }
  return errors;
}

export function assertDirtyTwinsRejected(repoRoot = REPO_ROOT) {
  const errors = [];
  const abs = path.join(repoRoot, PUBLIC_CONSTRUCTOR_RS);
  let original = null;
  try {
    original = fs.readFileSync(abs, "utf8");
  } catch {
    return [err("STP16-private-leak", "missing-check", `${PUBLIC_CONSTRUCTOR_RS} is unreadable`)];
  }
  for (const [twin, spec] of Object.entries(DIRTY_TWIN_PATCHES)) {
    let mutated = original;
    let missingAnchor = null;
    for (const patch of spec.patches) {
      // A plant must apply exactly once: an absent or repeated anchor would
      // make a green run indistinguishable from a patch that never landed.
      if (mutated.split(patch.find).length !== 2) {
        missingAnchor = patch.find.slice(0, 80);
        break;
      }
      mutated = mutated.replace(patch.find, patch.replace);
    }
    if (missingAnchor !== null) {
      errors.push(
        err(
          "STP16-private-leak",
          "dirty-twin-unproven",
          `${twin} patch anchor is not unique in the owned product: ${missingAnchor}`,
        ),
      );
      continue;
    }
    const names = spec.discriminators;
    fs.writeFileSync(abs, mutated);
    let run = null;
    try {
      if (fs.readFileSync(abs, "utf8") !== mutated) {
        throw new Error(`${twin} mutation did not land on disk`);
      }
      run = cargo(repoRoot, [...LIB_LANE, "--", "--test-threads=1", ...names]);
    } catch (error) {
      errors.push(err("STP16-private-leak", "missing-check", String(error?.message || error)));
    } finally {
      fs.writeFileSync(abs, original);
    }
    if (run === null) {
      continue;
    }
    if (run.error) {
      errors.push(
        err(
          "STP16-private-leak",
          "missing-check",
          `${twin} run failed to spawn: ${run.error.message}`,
        ),
      );
      continue;
    }
    const output = String(run.stdout || "");
    // Rejection proof requires each named discriminator to FAIL: a nonzero
    // status alone is not proof, since a compile error or an unrelated
    // failure also exits nonzero while the owning discriminator passes.
    const rejected = run.status !== 0 && names.every((name) => testFailed(output, name));
    if (!rejected) {
      errors.push(
        err(
          "STP16-private-leak",
          "dirty-twin-unproven",
          `${twin} applied but its discriminators did not all fail (status=${run.status})`,
        ),
      );
    }
  }
  if (fs.readFileSync(abs, "utf8") !== original) {
    fs.writeFileSync(abs, original);
    errors.push(err("STP16-private-leak", "missing-check", "mutated source was not restored"));
  }
  return errors;
}

export async function evaluateStp16({ repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  errors.push(...validateStp16Products({ repoRoot }));
  for (const id of NODE_MANDATORY_CASES.STP16 || []) {
    if (!STP16_MANDATORY_CASES.includes(id)) {
      errors.push(err("STP16-required-api", "unknown-row", `unowned mandatory case ${id}`));
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
