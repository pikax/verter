/**
 * STP15 Live template read/write views and actual usage accounting protocol.
 *
 * Validates the named products exist on the dormant projection backend and
 * runs the owning Rust tests per mandatory case: unwrapped top-level ref
 * reads, getter-only/readonly write refusals, setter-domain writes, real
 * usage accounting without synthetic reads, and live views without
 * immutable snapshots.
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

function err(caseId, code, message) {
  return { caseId, code, message };
}

export function validateStp15Products({ repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  for (const rel of [BINDING_VIEWS_RS, BACKEND_RS]) {
    if (!fs.existsSync(path.join(repoRoot, rel))) {
      errors.push(err("STP15-read-ref", "removed-fixture", `missing ${rel}`));
    }
  }
  const viewsAbs = path.join(repoRoot, BINDING_VIEWS_RS);
  if (fs.existsSync(viewsAbs)) {
    const views = fs.readFileSync(viewsAbs, "utf8");
    // Products must be exported items (`pub struct`), not passing mentions
    // in comments or string literals.
    for (const product of ACCEPTED_PRODUCTS) {
      const exported = new RegExp(`pub\\s+struct\\s+${product}\\b`).test(views);
      if (!exported) {
        errors.push(
          err("STP15-read-ref", "removed-fixture", `missing exported product ${product}`),
        );
      }
    }
    if (!/pub\s+fn\s+project_binding_views\s*\(/.test(views)) {
      errors.push(err("STP15-read-ref", "removed-fixture", "project_binding_views is missing"));
    }
    if (!/pub\s+fn\s+write_target\s*\(/.test(views)) {
      errors.push(
        err(
          "STP15-readonly-write",
          "universal-mutable-alias",
          "writes must resolve through TemplateWriteTarget::write_target",
        ),
      );
    }
    if (!/from_authored_references/.test(views)) {
      errors.push(
        err(
          "STP15-unused",
          "synthetic-usage-scaffold",
          "usage must be built from authored references only",
        ),
      );
    }
    if (!/ViewSnapshotKind::Live/.test(views)) {
      errors.push(
        err(
          "STP15-mutation",
          "immutable-snapshot",
          "read views must be live, never immutable snapshots",
        ),
      );
    }
  }
  const backendAbs = path.join(repoRoot, BACKEND_RS);
  if (fs.existsSync(backendAbs)) {
    const backend = fs.readFileSync(backendAbs, "utf8");
    if (!/pub\s+fn\s+binding_views\s*\(/.test(backend)) {
      errors.push(
        err(
          "STP15-read-ref",
          "removed-fixture",
          "VueProjectionBackend::binding_views is missing",
        ),
      );
    }
    if (!/pub\s+struct\s+VueProjectionBackend\b/.test(backend)) {
      errors.push(
        err("STP15-read-ref", "removed-fixture", "VueProjectionBackend is missing"),
      );
    }
    if (!/fn\s+project_ide\s*\(/.test(backend)) {
      errors.push(err("STP15-read-ref", "removed-fixture", "existing project_ide route missing"));
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

export function assertRustCases(run) {
  const errors = [];
  if (run.error) {
    return STP15_MANDATORY_CASES.map((id) => err(id, "missing-check", run.error.message));
  }
  const output = String(run.stdout || "");
  if (run.status !== 0 || !/test result: ok\. [1-9]\d* passed; 0 failed/.test(output)) {
    return STP15_MANDATORY_CASES.map((id) =>
      err(id, "rust-case", `cargo test did not complete the binding-views cases (status=${run.status})`),
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
