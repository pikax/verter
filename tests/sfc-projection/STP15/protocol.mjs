/**
 * STP15 Live template read/write views and actual usage accounting protocol.
 *
 * Structural inventory (files, manifest rows) plus fresh physical proof:
 * the owning Rust suites compile the crate and execute over real setup
 * bytes and admitted carrier blocks per mandatory case — unwrapped
 * top-level ref reads, getter-only/readonly write refusals,
 * setter-domain writes, real usage accounting without synthetic reads,
 * and live views without immutable snapshots. Each exported dirty twin
 * maps to the discriminator test that rejects it; the verifier applies
 * every twin as a source patch against the owned product, requires the
 * discriminator run to fail while it is applied, then restores the clean
 * source — the rejection is reproduced, not asserted from clean passes.
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { NODE_MANDATORY_CASES } from "../../../scripts/sfc-projection/node-mandatory-cases.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const BINDING_VIEWS_RS = "crates/verter_compiler/src/ide/vue_projection/binding_views.rs";
const BACKEND_RS = "crates/verter_compiler/src/framework_common/vue_projection_backend.rs";

export const STP15_MANDATORY_CASES = Object.freeze([
  "STP15-read-ref",
  "STP15-readonly-write",
  "STP15-setter-domain",
  "STP15-unused",
  "STP15-mutation",
]);

export const ACCEPTED_PRODUCTS = Object.freeze([
  "TemplateReadView",
  "TemplateWriteTarget",
  "BindingUsageSet",
]);

// Rejected mutations (never applied; the clean product must fail each one):
// a universal mutable alias would let readonly writes through; synthetic
// void reads would hide unused bindings; an immutable snapshot would mask
// authored mutations; a read-type write domain would reject valid setters.
export const DIRTY_UNIVERSAL_MUTABLE_ALIAS = { write: { universalMutableAlias: true } };
export const DIRTY_SYNTHETIC_VOID_READS = { usage: { syntheticVoidReads: true } };
export const DIRTY_IMMUTABLE_SNAPSHOT = { read: { snapshotKind: "immutable" } };
export const DIRTY_READ_TYPE_WRITE_DOMAIN = { write: { setterDomain: "readType" } };
export const DIRTY_CONST_PLAIN_WRITABLE = { write: { constPlainMutable: true } };

const RUST_CASES = Object.freeze({
  "STP15-read-ref": [
    "stp15_read_ref_unwraps_top_level_ref_only",
    "stp15_whole_torefs_result_reads_directly",
  ],
  "STP15-readonly-write": [
    "stp15_readonly_write_rejects_getter_computed_and_readonly_prop",
    "stp15_const_plain_bindings_refuse_template_writes",
  ],
  "STP15-setter-domain": ["stp15_setter_domain_uses_declared_setter_type"],
  "STP15-unused": [
    "stp15_unused_reports_only_authored_references",
    "stp15_usage_collects_authored_region_references",
  ],
  "STP15-mutation": ["stp15_mutation_uses_live_views_without_snapshots"],
});

// Each rejected mutation is a source patch against the owned product plus
// the discriminator tests that must FAIL while it is applied. The
// verifier writes the patch into `binding_views.rs`, runs the owning
// `cargo test` subset, requires the failure for the stated reason, then
// restores the clean source — every twin is reproduced physically.
const DIRTY_TWIN_PATCHES = Object.freeze({
  DIRTY_UNIVERSAL_MUTABLE_ALIAS: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "BindingKind::Computed { setter: false } => projection.write.getter_only.push(name),",
        replace: `BindingKind::Computed { setter: false } => projection.write.writable.push(WritableBinding {
            name,
            domain: WriteDomain::PlainAssign,
        }),`,
      }),
    ]),
    discriminators: Object.freeze([
      "stp15_readonly_write_rejects_getter_computed_and_readonly_prop",
    ]),
  }),
  DIRTY_SYNTHETIC_VOID_READS: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "if regions.template || regions.script || regions.style {",
        replace: "if true {",
      }),
    ]),
    discriminators: Object.freeze(["stp15_unused_reports_only_authored_references"]),
  }),
  DIRTY_IMMUTABLE_SNAPSHOT: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: `pub enum ViewSnapshotKind {
    /// The view names the live binding; authored mutations stay visible.
    Live,
}`,
        replace: `pub enum ViewSnapshotKind {
    /// The view names the live binding; authored mutations stay visible.
    Live,
    /// Rejected twin: an immutable snapshot copy masking authored mutations.
    Snapshot,
}`,
      }),
      Object.freeze({
        find: `    pub fn snapshot_kind(&self) -> ViewSnapshotKind {
        ViewSnapshotKind::Live
    }`,
        replace: `    pub fn snapshot_kind(&self) -> ViewSnapshotKind {
        ViewSnapshotKind::Snapshot
    }`,
      }),
    ]),
    discriminators: Object.freeze(["stp15_mutation_uses_live_views_without_snapshots"]),
  }),
  DIRTY_READ_TYPE_WRITE_DOMAIN: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "domain: WriteDomain::SetterParam(computed_domain.unwrap_or_default()),",
        replace: "domain: WriteDomain::SetterParam(String::new()),",
      }),
    ]),
    discriminators: Object.freeze(["stp15_setter_domain_uses_declared_setter_type"]),
  }),
  DIRTY_CONST_PLAIN_WRITABLE: Object.freeze({
    patches: Object.freeze([
      Object.freeze({
        find: "BindingKind::Plain => projection.write.immutable.push(name),",
        replace: `BindingKind::Plain => projection.write.writable.push(WritableBinding {
            name,
            domain: WriteDomain::PlainAssign,
        }),`,
      }),
    ]),
    discriminators: Object.freeze(["stp15_const_plain_bindings_refuse_template_writes"]),
  }),
});
const DIRTY_TWIN_DISCRIMINATORS = Object.freeze({
  DIRTY_UNIVERSAL_MUTABLE_ALIAS: Object.freeze([
    "stp15_readonly_write_rejects_getter_computed_and_readonly_prop",
    "binding_views_reads_admitted_carrier_blocks",
  ]),
  DIRTY_SYNTHETIC_VOID_READS: Object.freeze(["stp15_unused_reports_only_authored_references"]),
  DIRTY_IMMUTABLE_SNAPSHOT: Object.freeze([
    "stp15_mutation_uses_live_views_without_snapshots",
    "binding_views_reads_admitted_carrier_blocks",
  ]),
  DIRTY_READ_TYPE_WRITE_DOMAIN: Object.freeze([
    "stp15_setter_domain_uses_declared_setter_type",
    "binding_views_reads_admitted_carrier_blocks",
  ]),
  DIRTY_CONST_PLAIN_WRITABLE: Object.freeze(["stp15_const_plain_bindings_refuse_template_writes"]),
});

export function dirtyTwinExpectations() {
  return Object.fromEntries(
    Object.entries(DIRTY_TWIN_DISCRIMINATORS).map(([twin, tests]) => [twin, [...tests]]),
  );
}

function err(caseId, code, message) {
  return { caseId, code, message };
}

export function validateStp15Products({ repoRoot = REPO_ROOT } = {}) {
  // Structural inventory only: every behavior claim below is proven by
  // the fresh `cargo test` run in assertRustCases, never by matching
  // source text.
  const errors = [];
  for (const rel of [
    BINDING_VIEWS_RS,
    BACKEND_RS,
    "tests/sfc-projection/STP15/manifest.json",
    "tests/sfc-projection/STP15/probes/positive.ts",
    "tests/sfc-projection/STP15/probes/negative.ts",
    "tests/sfc-projection/STP15/probes/components/Counter.vue.d.ts",
  ]) {
    if (!fs.existsSync(path.join(repoRoot, rel))) {
      errors.push(err("STP15-read-ref", "removed-fixture", `missing ${rel}`));
    }
  }
  const manifestAbs = path.join(repoRoot, "tests/sfc-projection/STP15/manifest.json");
  if (fs.existsSync(manifestAbs)) {
    let manifest = null;
    try {
      manifest = JSON.parse(fs.readFileSync(manifestAbs, "utf8"));
    } catch {
      errors.push(err("STP15-read-ref", "removed-fixture", "manifest.json is not valid JSON"));
    }
    if (manifest !== null) {
      for (const id of STP15_MANDATORY_CASES) {
        if (!(manifest.mandatoryCases || []).includes(id)) {
          errors.push(err(id, "unknown-row", `manifest is missing mandatory case ${id}`));
        }
      }
      for (const product of ACCEPTED_PRODUCTS) {
        if (!(manifest.products || []).includes(product)) {
          errors.push(
            err("STP15-read-ref", "removed-fixture", `manifest is missing product ${product}`),
          );
        }
      }
    }
  }
  return errors;
}

export function runRustCases(repoRoot = REPO_ROOT) {
  // Both runs compile the owning crate (physical syntactic proof) and
  // execute the binding-views suites: unit facts plus the production
  // `binding_views` path over real admitted `.vue` carrier bytes. Each
  // lane keeps its own result so a zero-test lane cannot hide behind the
  // other lane's passes.
  const invocations = [
    ["test", "-p", "verter_compiler", "--lib", "ide::vue_projection::binding_views"],
    [
      "test",
      "-p",
      "verter_compiler",
      "--test",
      "main",
      "binding_views_reads_admitted_carrier_blocks",
    ],
  ];
  const lanes = invocations.map((args) => {
    const result = spawnSync("cargo", [...args, "--", "--test-threads=1"], {
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
  });
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
    return STP15_MANDATORY_CASES.map((id) => err(id, "missing-check", run.error.message));
  }
  const output = String(run.stdout || "");
  // Per-lane evidence: each cargo lane must report its own passing
  // `test result` line, so a zero-test carrier lane fails instead of
  // hiding behind the lib lane's passes.
  const lanes = Array.isArray(run.lanes) ? run.lanes : [{ status: run.status, stdout: output }];
  for (const [index, lane] of lanes.entries()) {
    const laneOutput = String(lane.stdout || "");
    if (lane.status !== 0 || !/test result: ok\. [1-9]\d* passed; 0 failed/.test(laneOutput)) {
      return STP15_MANDATORY_CASES.map((id) =>
        err(
          id,
          "rust-case",
          `cargo lane ${index} did not execute its binding-views cases (status=${lane.status})`,
        ),
      );
    }
  }
  if (!testPassed(output, "binding_views_reads_admitted_carrier_blocks")) {
    errors.push(
      err(
        "STP15-read-ref",
        "rust-case",
        "the production carrier lane did not execute binding_views_reads_admitted_carrier_blocks",
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
    DIRTY_UNIVERSAL_MUTABLE_ALIAS,
    DIRTY_SYNTHETIC_VOID_READS,
    DIRTY_IMMUTABLE_SNAPSHOT,
    DIRTY_READ_TYPE_WRITE_DOMAIN,
    DIRTY_CONST_PLAIN_WRITABLE,
  };
  for (const [twin, value] of Object.entries(twins)) {
    if (value === undefined) {
      errors.push(err("STP15-mutation", "dirty-twin-unproven", `${twin} is not exported`));
      continue;
    }
    if (DIRTY_TWIN_PATCHES[twin] === undefined) {
      errors.push(err("STP15-mutation", "dirty-twin-unproven", `${twin} has no applied patch`));
    }
  }
  return errors;
}

export function runDiscriminatorTests(repoRoot, names) {
  const result = spawnSync(
    "cargo",
    [
      "test",
      "-p",
      "verter_compiler",
      "--lib",
      "ide::vue_projection::binding_views",
      "--",
      "--test-threads=1",
      ...names,
    ],
    {
      cwd: repoRoot,
      encoding: "utf8",
      windowsHide: true,
      timeout: 300000,
      env: process.env,
    },
  );
  return {
    status: result.status,
    error: result.error,
    stdout: `${result.stdout || ""}${result.stderr || ""}`,
  };
}

export function assertDirtyTwinsRejected(repoRoot = REPO_ROOT) {
  const errors = [];
  const abs = path.join(repoRoot, BINDING_VIEWS_RS);
  let original = null;
  try {
    original = fs.readFileSync(abs, "utf8");
  } catch {
    return [err("STP15-mutation", "missing-check", `${BINDING_VIEWS_RS} is unreadable`)];
  }
  for (const [twin, spec] of Object.entries(DIRTY_TWIN_PATCHES)) {
    let mutated = original;
    let missingAnchor = null;
    for (const patch of spec.patches) {
      if (!mutated.includes(patch.find)) {
        missingAnchor = patch.find.slice(0, 80);
        break;
      }
      mutated = mutated.replace(patch.find, patch.replace);
    }
    if (missingAnchor !== null) {
      errors.push(
        err(
          "STP15-mutation",
          "dirty-twin-unproven",
          `${twin} patch anchor is missing from the owned product: ${missingAnchor}`,
        ),
      );
      continue;
    }
    fs.writeFileSync(abs, mutated);
    let run = null;
    try {
      run = runDiscriminatorTests(repoRoot, spec.discriminators);
    } finally {
      fs.writeFileSync(abs, original);
    }
    if (run.error) {
      errors.push(
        err("STP15-mutation", "missing-check", `${twin} run failed to spawn: ${run.error.message}`),
      );
      continue;
    }
    const output = String(run.stdout || "");
    const ran = spec.discriminators.some((name) => output.includes(name));
    if (!ran) {
      errors.push(
        err(
          "STP15-mutation",
          "missing-check",
          `${twin} run did not execute its discriminators (status=${run.status})`,
        ),
      );
      continue;
    }
    // Rejection proof requires each discriminator to FAIL for the stated
    // mutation: a nonzero status alone is not proof, since an unrelated
    // compile error or sibling failure also exits nonzero while the
    // owning discriminator still passes.
    const rejected =
      run.status !== 0 && spec.discriminators.every((name) => testFailed(output, name));
    if (!rejected) {
      errors.push(
        err(
          "STP15-mutation",
          "dirty-twin-unproven",
          `${twin} applied but its discriminators did not all fail: the mutation is not discriminated`,
        ),
      );
    }
  }
  if (fs.readFileSync(abs, "utf8") !== original) {
    fs.writeFileSync(abs, original);
    errors.push(err("STP15-mutation", "missing-check", "mutated source was not restored to clean"));
  }
  return errors;
}

export async function evaluateStp15({ repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  errors.push(...validateStp15Products({ repoRoot }));
  for (const id of NODE_MANDATORY_CASES.STP15 || []) {
    if (!STP15_MANDATORY_CASES.includes(id)) {
      errors.push(err("STP15-read-ref", "unknown-row", `unowned mandatory case ${id}`));
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
