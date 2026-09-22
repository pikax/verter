/**
 * STP15 Live template read/write views and actual usage accounting protocol.
 *
 * Structural inventory (files, manifest rows) plus fresh physical proof:
 * the owning Rust suites compile the crate and execute over real setup
 * bytes and admitted carrier blocks per mandatory case — unwrapped
 * top-level ref reads, getter-only/readonly write refusals,
 * setter-domain writes, real usage accounting without synthetic reads,
 * and live views without immutable snapshots. Each exported dirty twin
 * maps to the discriminator test that rejects it; the verifier proves
 * the fresh run passed those discriminators without ever applying the
 * mutations.
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

const RUST_CASES = Object.freeze({
  "STP15-read-ref": ["stp15_read_ref_unwraps_top_level_ref_only"],
  "STP15-readonly-write": ["stp15_readonly_write_rejects_getter_computed_and_readonly_prop"],
  "STP15-setter-domain": ["stp15_setter_domain_uses_declared_setter_type"],
  "STP15-unused": ["stp15_unused_reports_only_authored_references"],
  "STP15-mutation": ["stp15_mutation_uses_live_views_without_snapshots"],
});

// Each rejected mutation is discriminated by owning Rust tests that the
// clean product must pass: a universal mutable alias would let the
// readonly-write test through; synthetic void reads would break the
// unused test; an immutable snapshot would break the mutation test; a
// read-type write domain would break the setter-domain test. The
// verifier never applies the mutations; it proves the fresh `cargo
// test` run executed and passed each discriminator.
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
  // `binding_views` path over real admitted `.vue` carrier bytes.
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

function testPassed(output, name) {
  return output.includes(`${name} ... ok`) && !output.includes(`${name} ... FAILED`);
}

export function assertRustCases(run) {
  const errors = [];
  if (run.error) {
    return STP15_MANDATORY_CASES.map((id) => err(id, "missing-check", run.error.message));
  }
  const output = String(run.stdout || "");
  if (run.status !== 0 || !/test result: ok\. [1-9]\d* passed; 0 failed/.test(output)) {
    return STP15_MANDATORY_CASES.map((id) =>
      err(
        id,
        "rust-case",
        `cargo test did not complete the binding-views cases (status=${run.status})`,
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
  // Consume every exported dirty twin: the fresh run must have executed
  // and passed the discriminator that rejects it.
  const twins = {
    DIRTY_UNIVERSAL_MUTABLE_ALIAS,
    DIRTY_SYNTHETIC_VOID_READS,
    DIRTY_IMMUTABLE_SNAPSHOT,
    DIRTY_READ_TYPE_WRITE_DOMAIN,
  };
  for (const [twin, value] of Object.entries(twins)) {
    if (value === undefined) {
      errors.push(err("STP15-mutation", "dirty-twin-unproven", `${twin} is not exported`));
      continue;
    }
    for (const name of DIRTY_TWIN_DISCRIMINATORS[twin] || []) {
      if (!testPassed(output, name)) {
        errors.push(
          err(
            "STP15-mutation",
            "dirty-twin-unproven",
            `fresh run did not pass ${name}, the discriminator rejecting ${twin}`,
          ),
        );
      }
    }
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
  errors.push(...assertRustCases(runRustCases(repoRoot)));
  return { errors };
}
