/**
 * STP10 emission correspondence protocol.
 *
 * Joins the named mapping products, rejects overlapping virtual spans,
 * stale mapping reuse, and synthetic authored locations, and runs the
 * owning Rust tests for roundtrip, role, overlap, stale-map, and synthetic.
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { NODE_MANDATORY_CASES } from "../../../scripts/sfc-projection/node-mandatory-cases.mjs";

const STP10_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(STP10_DIR, "../../..");
const ORIGIN_RS = "crates/verter_compiler/src/framework_common/projection_plan/origin.rs";
const BACKEND_RS = "crates/verter_compiler/src/framework_common/vue_projection_backend.rs";
const PRODUCT_REL = "tests/sfc-projection/STP10/products/emission-correspondence.json";
const STP8_ABI_REL = "tests/sfc-projection/STP8/products/accepted-vue-constructor-abi.json";
const STP9_PLAN_REL = "tests/sfc-projection/STP9/products/source-backed-projection-plan.json";

export const PROTOCOL_VERSION = 1;

export const STP10_MANDATORY_CASES = Object.freeze([
  "STP10-roundtrip",
  "STP10-role",
  "STP10-overlap",
  "STP10-stale-map",
  "STP10-synthetic",
]);

export const ACCEPTED_PRODUCTS = Object.freeze([
  "ProjectionEmission",
  "ProjectionOrigin",
  "ObservationRole",
  "EditOrigin",
  "MappingProduct",
]);

const INDEPENDENT_MAP_NEEDLES = Object.freeze([
  "SourceMapBuilder",
  "oxc_sourcemap::SourceMapBuilder",
]);

export const DIRTY_OVERLAP = { overlappingVirtualSpans: true };
export const DIRTY_SYNTHETIC = { syntheticAuthoredLocation: true };
export const DIRTY_STALE = { staleMapReuse: true };
export const DIRTY_INDEPENDENT_MAP = true;

const RUST_CASE_FILTERS = Object.freeze({
  "STP10-roundtrip": "stp10_roundtrip_verbatim_unicode_and_crlf",
  "STP10-role": "stp10_role_view_property_and_binding_share_origin_not_symbol",
  "STP10-overlap": "stp10_overlap_rejects_overlapping_virtual_spans",
  "STP10-stale-map": "stp10_stale_map_rejects_reused_pre_edit_positions",
  "STP10-synthetic": "stp10_synthetic_rejects_authored_location_without_preimage",
});

function err(caseId, code, message) {
  return { caseId, code, message };
}

export function cloneJson(value) {
  return structuredClone(value);
}

export function loadStp10Product(file = "emission-correspondence.json") {
  return JSON.parse(
    fs.readFileSync(path.join(REPO_ROOT, "tests/sfc-projection/STP10/products", file), "utf8"),
  );
}

export function productionOriginSource(
  src = fs.readFileSync(path.join(REPO_ROOT, ORIGIN_RS), "utf8"),
) {
  return src.split("#[cfg(test)]")[0] ?? src;
}

export function assertNoIndependentMapGenerator(source = productionOriginSource()) {
  const errors = [];
  for (const needle of INDEPENDENT_MAP_NEEDLES) {
    if (source.includes(needle)) {
      errors.push(
        err(
          "STP10-stale-map",
          "independent-map-generator",
          `origin construction uses ${needle} instead of CodeTransform::chain_source_map`,
        ),
      );
    }
  }
  if (!source.includes("chain_source_map")) {
    errors.push(
      err(
        "STP10-stale-map",
        "independent-map-generator",
        "origin construction must compose chains through CodeTransform::chain_source_map",
      ),
    );
  }
  return errors;
}

export function assertCorrespondencePolicy(product = loadStp10Product()) {
  const errors = [];
  const correspondence = product.correspondence || {};
  if (correspondence.overlappingVirtualSpans) {
    errors.push(
      err("STP10-overlap", "overlapping-virtual-spans", "returned virtual mapping spans overlap"),
    );
  }
  if (correspondence.syntheticAuthoredLocation) {
    errors.push(
      err(
        "STP10-synthetic",
        "synthetic-authored-location",
        "scaffolding with no authored preimage fabricates an authored location",
      ),
    );
  }
  if (correspondence.staleMapReuse) {
    errors.push(
      err(
        "STP10-stale-map",
        "stale-map-reuse",
        "unchanged checking text reuses pre-edit source positions",
      ),
    );
  }
  if (correspondence.verbatimRoundtripIncludesUnicodeAndCrlf !== true) {
    errors.push(
      err(
        "STP10-roundtrip",
        "roundtrip-missing",
        "verbatim Unicode/CRLF round-trip is not required",
      ),
    );
  }
  if (product.independentMapGenerator) {
    errors.push(
      err(
        "STP10-stale-map",
        "independent-map-generator",
        "product admits an independent map generator",
      ),
    );
  }
  if (product.revisions?.checkingTextRevisionDistinctFromCorrespondenceRevision !== true) {
    errors.push(
      err(
        "STP10-stale-map",
        "revision-collapse",
        "checking-text revision must stay distinct from correspondence revision",
      ),
    );
  }
  return errors;
}

export function validateStp10Products(product = loadStp10Product()) {
  const errors = [];
  if (product.schema !== "EmissionCorrespondence") {
    errors.push(err("STP10-roundtrip", "removed-fixture", "EmissionCorrespondence schema"));
  }
  const named = new Set(product.products || []);
  for (const id of ACCEPTED_PRODUCTS) {
    if (!named.has(id)) {
      errors.push(err("STP10-roundtrip", "removed-fixture", `missing product ${id}`));
    }
  }
  const binders = product.predecessors?.STP8?.genericBinders || [];
  if (!binders.includes("T") || !binders.includes("U")) {
    errors.push(err("STP10-roundtrip", "removed-fixture", "STP8 two-binder family not joined"));
  }
  const abiAbs = path.join(REPO_ROOT, STP8_ABI_REL);
  if (!fs.existsSync(abiAbs)) {
    errors.push(err("STP10-roundtrip", "removed-fixture", `missing ${STP8_ABI_REL}`));
  } else {
    const abi = JSON.parse(fs.readFileSync(abiAbs, "utf8"));
    if (!String(abi.selectedCandidate?.binderFamily || "").includes("two-binder")) {
      errors.push(err("STP10-roundtrip", "removed-fixture", "STP8 ABI two-binder family missing"));
    }
  }
  const stp9Abs = path.join(REPO_ROOT, STP9_PLAN_REL);
  if (!fs.existsSync(stp9Abs)) {
    errors.push(err("STP10-role", "removed-fixture", `missing ${STP9_PLAN_REL}`));
  }
  const originAbs = path.join(REPO_ROOT, ORIGIN_RS);
  const backendAbs = path.join(REPO_ROOT, BACKEND_RS);
  if (!fs.existsSync(originAbs)) {
    errors.push(err("STP10-roundtrip", "removed-fixture", `missing ${ORIGIN_RS}`));
  }
  if (!fs.existsSync(backendAbs)) {
    errors.push(err("STP10-roundtrip", "removed-fixture", `missing ${BACKEND_RS}`));
  } else {
    const backend = fs.readFileSync(backendAbs, "utf8");
    if (!backend.includes("fn projection_emission(")) {
      errors.push(
        err(
          "STP10-roundtrip",
          "removed-fixture",
          "VueProjectionBackend::projection_emission is missing",
        ),
      );
    }
    if (!backend.includes("fn project_ide(")) {
      errors.push(err("STP10-roundtrip", "removed-fixture", "existing project_ide route missing"));
    }
  }
  errors.push(...assertCorrespondencePolicy(product));
  errors.push(...assertNoIndependentMapGenerator());
  return errors;
}

export function evaluateRejectTwins({
  originSource = productionOriginSource(),
  product = loadStp10Product(),
} = {}) {
  const errors = [];
  const dirtyOverlap = cloneJson(product);
  Object.assign(dirtyOverlap.correspondence, DIRTY_OVERLAP);
  const overlap = assertCorrespondencePolicy(dirtyOverlap);
  if (!overlap.some((row) => row.caseId === "STP10-overlap")) {
    errors.push(
      err("STP10-overlap", "twin-miss", "overlapping-virtual-spans dirty twin was not rejected"),
    );
  }
  const dirtySynthetic = cloneJson(product);
  Object.assign(dirtySynthetic.correspondence, DIRTY_SYNTHETIC);
  const synthetic = assertCorrespondencePolicy(dirtySynthetic);
  if (!synthetic.some((row) => row.caseId === "STP10-synthetic")) {
    errors.push(
      err(
        "STP10-synthetic",
        "twin-miss",
        "synthetic-authored-location dirty twin was not rejected",
      ),
    );
  }
  const dirtyStale = cloneJson(product);
  Object.assign(dirtyStale.correspondence, DIRTY_STALE);
  const stale = assertCorrespondencePolicy(dirtyStale);
  if (!stale.some((row) => row.caseId === "STP10-stale-map")) {
    errors.push(err("STP10-stale-map", "twin-miss", "stale-map-reuse dirty twin was not rejected"));
  }
  const dirtyMap = cloneJson(product);
  dirtyMap.independentMapGenerator = DIRTY_INDEPENDENT_MAP;
  const independent = assertCorrespondencePolicy(dirtyMap);
  if (!independent.some((row) => row.code === "independent-map-generator")) {
    errors.push(
      err("STP10-stale-map", "twin-miss", "independent-map-generator dirty twin was not rejected"),
    );
  }
  const builder = `${originSource}\nfn forbidden() { SourceMapBuilder::new(); }\n`;
  const builderErrors = assertNoIndependentMapGenerator(builder);
  if (!builderErrors.some((row) => row.code === "independent-map-generator")) {
    errors.push(
      err("STP10-stale-map", "twin-miss", "SourceMapBuilder dirty twin was not rejected"),
    );
  }
  return errors;
}

const CASE_TEST_RE = /test result: (ok|FAILED)\. (\d+) passed; (\d+) failed/;

export function parseCargoSummary(stdout) {
  const match = String(stdout || "").match(CASE_TEST_RE);
  if (!match) return null;
  return {
    ok: match[1] === "ok",
    passed: Number(match[2]),
    failed: Number(match[3]),
  };
}

export function runOriginRustTests(repoRoot = REPO_ROOT) {
  const result = spawnSync(
    "cargo",
    [
      "test",
      "-p",
      "verter_compiler",
      "--lib",
      "framework_common::projection_plan::origin::",
      "--",
      "--test-threads=1",
    ],
    {
      cwd: repoRoot,
      encoding: "utf8",
      windowsHide: true,
      timeout: 300000,
      env: process.env,
    },
  );
  const stdout = `${result.stdout || ""}${result.stderr || ""}`;
  return {
    status: result.status,
    error: result.error,
    stdout,
    ok: result.status === 0 && CASE_TEST_RE.test(stdout) && /test result: ok/.test(stdout),
  };
}

export function assertRustCases(run) {
  const errors = [];
  if (run.error) {
    for (const id of STP10_MANDATORY_CASES) {
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
    for (const id of STP10_MANDATORY_CASES) {
      errors.push(err(id, "rust-case", `cargo test did not execute mandatory cases (${detail})`));
    }
  }
  for (const [caseId, filter] of Object.entries(RUST_CASE_FILTERS)) {
    const failed = output.includes(`${filter} ... FAILED`) || output.includes(`'${filter}'`);
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

export async function evaluateStp10({ skipRust = false, repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  errors.push(...validateStp10Products());
  errors.push(...evaluateRejectTwins());
  const required = NODE_MANDATORY_CASES.STP10 || STP10_MANDATORY_CASES;
  for (const id of required) {
    if (!STP10_MANDATORY_CASES.includes(id)) {
      errors.push(
        err("STP10-roundtrip", "unknown-row", `mandatory case ${id} is not owned by STP10`),
      );
    }
  }
  if (!skipRust) {
    errors.push(...assertRustCases(runOriginRustTests(repoRoot)));
  }
  return { errors };
}
