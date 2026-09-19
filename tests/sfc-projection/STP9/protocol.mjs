/**
 * STP9 source-backed projection plan protocol.
 *
 * Joins the named plan products, rejects TypeInfo/assignability in the
 * plan module, and runs the owning Rust tests for identity, shadowing,
 * complete-cache, and fresh/incremental determinism.
 */

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { NODE_MANDATORY_CASES } from "../../../scripts/sfc-projection/node-mandatory-cases.mjs";

const STP9_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(STP9_DIR, "../../..");
const PLAN_RS = "crates/verter_compiler/src/framework_common/projection_plan/mod.rs";
const BACKEND_RS = "crates/verter_compiler/src/framework_common/vue_projection_backend.rs";
const PRODUCT_REL = "tests/sfc-projection/STP9/products/source-backed-projection-plan.json";
const STP8_ABI_REL = "tests/sfc-projection/STP8/products/accepted-vue-constructor-abi.json";

export const PROTOCOL_VERSION = 1;

export const STP9_MANDATORY_CASES = Object.freeze([
  "STP9-ids",
  "STP9-shadow",
  "STP9-type-free",
  "STP9-complete-cache",
  "STP9-determinism",
]);

export const ACCEPTED_PRODUCTS = Object.freeze([
  "ProjectionPlan",
  "BindingOriginId",
  "ComponentUseId",
  "GenericBinderRef",
  "OrderedAttributeOp",
  "BranchEdge",
]);

const TYPE_FREE_NEEDLES = Object.freeze([
  "TypeInfo",
  "TypeInfoCore",
  "CompileTypeInfo",
  "assignability",
  "Assignable",
  "RelationKind",
  "type_info::",
]);

export const TYPE_FREE_CRATE_ALLOWLIST = Object.freeze([
  "crate::assembly::source_unit",
  "crate::ast::types",
  "crate::ide::event_to_jsx_name",
  "crate::ide::get_directive_name",
  "crate::parser::types",
  "crate::template::oxc",
  "crate::types",
  "crate::compile::parse_sfc",
]);

const RUST_CASE_FILTERS = Object.freeze({
  "STP9-ids": "stp9_ids_comment_and_unrelated_sibling_preserve_use_identities",
  "STP9-shadow": "stp9_shadow_nested_slot_and_loop_origins_are_distinct",
  "STP9-type-free": "stp9_type_free_plan_module_does_not_call_typeinfo",
  "STP9-complete-cache": "stp9_complete_cache_rejects_malformed_and_unknown_syntax",
  "STP9-determinism": "stp9_determinism_fresh_matches_incremental",
});

export const DIRTY_TYPEINFO = "TypeInfoCore::attempt";
export const DIRTY_TYPEINFO_HELPER = "crate::ide::template::choose_answer";
export const DIRTY_CACHE = { malformedCachedAsComplete: true };

function err(caseId, code, message) {
  return { caseId, code, message };
}

export function cloneJson(value) {
  return structuredClone(value);
}

export function loadStp9Product(file = "source-backed-projection-plan.json") {
  return JSON.parse(
    fs.readFileSync(path.join(REPO_ROOT, "tests/sfc-projection/STP9/products", file), "utf8"),
  );
}

export function productionPlanSource(src = fs.readFileSync(path.join(REPO_ROOT, PLAN_RS), "utf8")) {
  return src.split("#[cfg(test)]")[0] ?? src;
}

export function cratePathAllowed(path, allowlist = TYPE_FREE_CRATE_ALLOWLIST) {
  return allowlist.some((allow) => path === allow || path.startsWith(`${allow}::`));
}

function identLen(src) {
  if (!/^[A-Za-z_]/.test(src)) return 0;
  const match = src.match(/^[A-Za-z_][A-Za-z0-9_]*/);
  return match ? match[0].length : 0;
}

export function cratePaths(source) {
  const paths = [];
  let rest = String(source || "");
  while (rest.includes("crate::")) {
    const at = rest.indexOf("crate::");
    const from = rest.slice(at);
    let end = "crate::".length;
    for (;;) {
      const ident = identLen(from.slice(end));
      if (ident === 0) break;
      end += ident;
      if (from.slice(end).startsWith("::")) {
        end += 2;
        continue;
      }
      break;
    }
    const path = from.slice(0, end).replace(/::$/, "");
    if (from.slice(path.length).startsWith("::{")) {
      const close = from.indexOf("}", path.length + 3);
      if (close !== -1) {
        const inner = from.slice(path.length + 3, close);
        for (const item of inner.split(",")) {
          const name = item.split(/\s+as\s+/)[0].trim();
          if (name && identLen(name) === name.length) {
            paths.push(`${path}::${name}`);
          }
        }
        rest = from.slice(close + 1);
        continue;
      }
    }
    paths.push(path);
    rest = from.slice(path.length);
  }
  return paths;
}

export function assertTypeFree(source = productionPlanSource()) {
  const errors = [];
  for (const needle of TYPE_FREE_NEEDLES) {
    if (source.includes(needle)) {
      errors.push(
        err(
          "STP9-type-free",
          "native-type-answer",
          `plan construction calls ${needle} to choose an answer`,
        ),
      );
    }
  }
  for (const path of cratePaths(source)) {
    if (!cratePathAllowed(path)) {
      errors.push(
        err(
          "STP9-type-free",
          "native-type-answer",
          `plan construction reaches ${path} outside the parse-only crate path allowlist`,
        ),
      );
    }
  }
  return errors;
}

export function assertCompleteCachePolicy(product = loadStp9Product()) {
  const errors = [];
  const cache = product.completeCache || {};
  if (cache.malformedCachedAsComplete) {
    errors.push(
      err(
        "STP9-complete-cache",
        "malformed-complete-cache",
        "malformed syntax is cached as a complete plan",
      ),
    );
  }
  if (cache.unknownSyntaxCachedAsComplete) {
    errors.push(
      err(
        "STP9-complete-cache",
        "unknown-syntax-complete-cache",
        "unknown syntax is cached as a complete plan",
      ),
    );
  }
  if (cache.incompleteCannotWarm !== true) {
    errors.push(
      err(
        "STP9-complete-cache",
        "incomplete-warm",
        "incomplete observations must not warm complete caches",
      ),
    );
  }
  return errors;
}

export function validateStp9Products(product = loadStp9Product()) {
  const errors = [];
  if (product.schema !== "SourceBackedProjectionPlan") {
    errors.push(err("STP9-ids", "removed-fixture", "SourceBackedProjectionPlan schema"));
  }
  const named = new Set(product.products || []);
  for (const id of ACCEPTED_PRODUCTS) {
    if (!named.has(id)) {
      errors.push(err("STP9-ids", "removed-fixture", `missing product ${id}`));
    }
  }
  if (product.planConstruction?.typeInfo || product.planConstruction?.assignability) {
    errors.push(
      err("STP9-type-free", "native-type-answer", "product admits TypeInfo or assignability"),
    );
  }
  const binders = product.predecessors?.STP8?.genericBinders || [];
  if (!binders.includes("T") || !binders.includes("U")) {
    errors.push(err("STP9-ids", "removed-fixture", "STP8 two-binder family not joined"));
  }
  const abiAbs = path.join(REPO_ROOT, STP8_ABI_REL);
  if (!fs.existsSync(abiAbs)) {
    errors.push(err("STP9-ids", "removed-fixture", `missing ${STP8_ABI_REL}`));
  } else {
    const abi = JSON.parse(fs.readFileSync(abiAbs, "utf8"));
    if (!String(abi.selectedCandidate?.binderFamily || "").includes("two-binder")) {
      errors.push(err("STP9-ids", "removed-fixture", "STP8 ABI two-binder family missing"));
    }
  }
  const planAbs = path.join(REPO_ROOT, PLAN_RS);
  const backendAbs = path.join(REPO_ROOT, BACKEND_RS);
  if (!fs.existsSync(planAbs)) {
    errors.push(err("STP9-ids", "removed-fixture", `missing ${PLAN_RS}`));
  }
  if (!fs.existsSync(backendAbs)) {
    errors.push(err("STP9-ids", "removed-fixture", `missing ${BACKEND_RS}`));
  } else {
    const backend = fs.readFileSync(backendAbs, "utf8");
    if (!backend.includes("fn projection_plan(")) {
      errors.push(
        err("STP9-ids", "removed-fixture", "VueProjectionBackend::projection_plan is missing"),
      );
    }
    if (!backend.includes("fn project_ide(")) {
      errors.push(err("STP9-ids", "removed-fixture", "existing project_ide route missing"));
    }
  }
  errors.push(...assertCompleteCachePolicy(product));
  errors.push(...assertTypeFree());
  return errors;
}

export function evaluateRejectTwins({
  typeFreeSource = productionPlanSource(),
  product = loadStp9Product(),
} = {}) {
  const errors = [];
  const dirtySource = `${typeFreeSource}\nfn forbidden() { ${DIRTY_TYPEINFO} }`;
  const typeFree = assertTypeFree(dirtySource);
  if (!typeFree.some((row) => row.caseId === "STP9-type-free")) {
    errors.push(err("STP9-type-free", "twin-miss", "TypeInfo dirty twin was not rejected"));
  }
  const helperSource = `${typeFreeSource}\nfn forbidden() { ${DIRTY_TYPEINFO_HELPER}(); }`;
  const helper = assertTypeFree(helperSource);
  if (!helper.some((row) => row.caseId === "STP9-type-free")) {
    errors.push(
      err("STP9-type-free", "twin-miss", "type-engine helper-path dirty twin was not rejected"),
    );
  }
  const dirtyCache = cloneJson(product);
  Object.assign(dirtyCache.completeCache, DIRTY_CACHE);
  const cache = assertCompleteCachePolicy(dirtyCache);
  if (!cache.some((row) => row.caseId === "STP9-complete-cache")) {
    errors.push(
      err(
        "STP9-complete-cache",
        "twin-miss",
        "malformed-complete-cache dirty twin was not rejected",
      ),
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

export function runProjectionPlanRustTests(repoRoot = REPO_ROOT) {
  const result = spawnSync(
    "cargo",
    [
      "test",
      "-p",
      "verter_compiler",
      "--lib",
      "framework_common::projection_plan::",
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
    for (const id of STP9_MANDATORY_CASES) {
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
    for (const id of STP9_MANDATORY_CASES) {
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

export async function evaluateStp9({ skipRust = false, repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  errors.push(...validateStp9Products());
  errors.push(...evaluateRejectTwins());
  const required = NODE_MANDATORY_CASES.STP9 || STP9_MANDATORY_CASES;
  for (const id of required) {
    if (!STP9_MANDATORY_CASES.includes(id)) {
      errors.push(err("STP9-ids", "unknown-row", `mandatory case ${id} is not owned by STP9`));
    }
  }
  if (!skipRust) {
    errors.push(...assertRustCases(runProjectionPlanRustTests(repoRoot)));
  }
  return { errors };
}
