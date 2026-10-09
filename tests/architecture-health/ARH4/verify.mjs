#!/usr/bin/env node
/**
 * ARH4 verifier — sole owning interface of the host and session lifecycle
 * responsibility reduction.
 *
 * Joins the lifecycle-cutover product to the live tree through positive
 * ownership joins: each narrowed route names a surviving owner whose type
 * and methods exist, the recorded consumers reach that owner through its
 * call forms, and the production population reaching a call form is exactly
 * the recorded one. The ARH1 register rows this node executes are read from
 * the ARH1 products (validated live through ARH1/ARH2 `validate()`), never
 * repeated here. No check constrains spellings of retired bindings: the
 * deleted host routes are held by the compiler (they no longer exist), and
 * the independent-copy defect by the AC2 behavioral tests. Node builtins only.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { loadProducts as loadArh1Products, validate as validateArh1 } from "../ARH1/verify.mjs";
import { loadProducts as loadArh2Products, validate as validateArh2 } from "../ARH2/verify.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = path.resolve(HERE, "../../..");

export const PRODUCT_FILES = Object.freeze(["lifecycle-cutover.json"]);

const EXPECTED_CASES = Object.freeze([
  "ARH4-authority",
  "ARH4-cost",
  "ARH4-cutover",
  "ARH4-delivery",
  "ARH4-ratification",
  "ARH4-work",
]);
const INVENTORY_CONCERNS = Object.freeze([
  "construction",
  "source management",
  "observation",
  "resolver access",
  "audit access",
  "scheduler source-staging surface",
]);
const CUTOVER_IDS = Object.freeze(["ARH4-CUT-1", "ARH4-CUT-2", "ARH4-CUT-3", "ARH4-CUT-4"]);
const AC3_CONCERNS = Object.freeze([
  "fresh-versus-incremental equivalence",
  "edit/revert",
  "cancellation",
  "stale/partial rejection",
  "deterministic ordering under perturbed discovery or scheduling",
]);
const API_REFERENCE = "docs/audit-footprint/api-reference.md";

function readRel(rel) {
  return fs.readFileSync(path.join(REPO_ROOT, rel), "utf8");
}

function existsRel(rel) {
  return fs.existsSync(path.join(REPO_ROOT, rel));
}

export function loadProducts() {
  const products = {};
  for (const file of PRODUCT_FILES) {
    products[file.replace(/\.json$/, "")] = JSON.parse(
      fs.readFileSync(path.join(HERE, "products", file), "utf8"),
    );
  }
  return products;
}

export function loadManifest() {
  return JSON.parse(fs.readFileSync(path.join(HERE, "manifest.json"), "utf8"));
}

function err(errors, caseId, code, detail) {
  errors.push({ caseId, code, detail });
}

/** Rust source with line and block comments removed, so a call form in a
 * comment never counts as a consumer. */
function stripRustComments(src) {
  return src.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/[^\n]*/g, "");
}

function rustFiles(dir, out = []) {
  for (const entry of fs.readdirSync(path.join(REPO_ROOT, dir), { withFileTypes: true })) {
    const rel = `${dir}/${entry.name}`;
    if (entry.isDirectory()) {
      if (entry.name === "target" || entry.name === "node_modules") continue;
      rustFiles(rel, out);
    } else if (entry.name.endsWith(".rs")) {
      out.push(rel);
    }
  }
  return out;
}

const MOVED_TEST_DIRS = [
  "crates/verter_compiler/src/compile_tests/",
  "crates/verter_compiler/src/ide/template/tests/",
  "crates/verter_compiler/src/svelte/runtime/client_tests/",
  "crates/verter_compiler/src/template/code_gen/ssr/tests/",
  "crates/verter_execution/src/tasks/tests/",
  "crates/verter_lsp/src/server/tests/",
  "crates/verter_session/src/tests/host_manage/",
  "crates/verter_session/src/tests/meta/",
  "crates/verter_type_engine/src/project_semantic_dispatch/tests/",
  "crates/verter_type_engine/src/semantic_query_memo/tests/",
];

let productionFilesCache;
/** Production Rust sources: every `src/` file under `crates/` that is not a
 * test module file (`tests.rs`, `*_tests.rs`, or under a split test-module directory). */
function productionRustFiles() {
  if (productionFilesCache) return productionFilesCache;
  productionFilesCache = rustFiles("crates").filter(
    (rel) =>
      /^crates\/[^/]+\/src\//.test(rel) &&
      !/(?:^|\/)tests\.rs$/.test(rel) &&
      !/_tests\.rs$/.test(rel) &&
      !MOVED_TEST_DIRS.some((dir) => rel.startsWith(dir)),
  );
  return productionFilesCache;
}

/** True when `text` contains any of `forms`, ignoring all whitespace so a
 * rustfmt-wrapped method chain matches the same as a single-line one. */
function containsCallForm(text, forms) {
  const squashed = text.replace(/\s+/g, "");
  return forms.some((form) => squashed.includes(form.replace(/\s+/g, "")));
}

/** Production files reaching any of `forms` outside comments. */
export function deriveCallFormPopulation(forms) {
  return productionRustFiles()
    .filter((rel) => {
      const text = stripRustComments(readRel(rel));
      return containsCallForm(text, forms);
    })
    .sort();
}

function declaresType(src, typeName) {
  return new RegExp(
    String.raw`\b(?:struct|enum|impl(?:<[^>]*>)?)\s+(?:crate::)?${typeName}\b`,
  ).test(src);
}

function declaresFn(src, name) {
  return new RegExp(String.raw`\bfn\s+${name}\s*[<(]`).test(src);
}

/** The attribute lines directly above a `pub fn name(` declaration. */
function attributesAbove(src, name) {
  const match = new RegExp(String.raw`\n([ \t]*)pub fn ${name}\s*[<(]`).exec(src);
  if (!match) return null;
  const lines = src.slice(0, match.index).split("\n");
  const attrs = [];
  for (let i = lines.length - 1; i >= 0; i -= 1) {
    const line = lines[i].trim();
    if (line.startsWith("#[") || line.startsWith("///") || line.startsWith("//")) {
      if (line.startsWith("#[")) attrs.push(line);
      continue;
    }
    break;
  }
  return attrs;
}

function hasRustTest(rel, test) {
  return existsRel(rel) && new RegExp(String.raw`\bfn\s+${test}\s*\(`).test(readRel(rel));
}

export function mandatoryCases(manifest = loadManifest()) {
  return manifest.cases.map((c) => c.id);
}

export function selectedCaseIds(result) {
  return [...new Set(result.errors.map((e) => e.caseId))];
}

function validateRatification(cut, manifest, errors) {
  const caseId = "ARH4-ratification";
  const liveCases = [...mandatoryCases(manifest)].sort();
  if (JSON.stringify(liveCases) !== JSON.stringify([...EXPECTED_CASES])) {
    err(errors, caseId, "manifest-case-drift", `manifest cases ${JSON.stringify(liveCases)}`);
  }
  if (JSON.stringify(manifest.products) !== JSON.stringify(["ARH4LifecycleCutover"])) {
    err(
      errors,
      caseId,
      "manifest-product-drift",
      `manifest products ${JSON.stringify(manifest.products)}`,
    );
  }
  if (manifest.verify !== "node tests/architecture-health/ARH4/verify.mjs") {
    err(errors, caseId, "manifest-command-drift", `verify=${manifest.verify}`);
  }
  if (manifest.test !== "node --test tests/architecture-health/ARH4/arh4.test.mjs") {
    err(errors, caseId, "manifest-command-drift", `test=${manifest.test}`);
  }
  if (cut?.schema !== "ARH4LifecycleCutover" || cut?.contractNode !== "ARH4") {
    err(errors, caseId, "product-schema-drift", `schema=${cut?.schema} node=${cut?.contractNode}`);
  }
  const pkg = JSON.parse(readRel("package.json"));
  if (
    !(pkg.scripts?.["test:scripts"] ?? "").includes("tests/architecture-health/ARH4/arh4.test.mjs")
  ) {
    err(errors, caseId, "test-scripts-missing", "test:scripts");
  }
  const ci = readRel(".github/workflows/ci.yml");
  if (
    !ci.includes("node tests/architecture-health/ARH4/verify.mjs") ||
    !ci.includes("node --test tests/architecture-health/ARH4/arh4.test.mjs")
  ) {
    err(errors, caseId, "ci-lane-missing", "architecture-health job");
  }
}

function validateCutover(cut, arh1, errors) {
  const caseId = "ARH4-cutover";
  const arh1Result = validateArh1(arh1);
  if (!arh1Result.ok) {
    err(
      errors,
      caseId,
      "predecessor-drift",
      `ARH1 validate failed: ${arh1Result.errors.map((e) => `${e.caseId}/${e.code}`).join(",")}`,
    );
  }
  const arh2Result = validateArh2(loadArh2Products());
  if (!arh2Result.ok) {
    err(
      errors,
      caseId,
      "predecessor-drift",
      `ARH2 validate failed: ${arh2Result.errors.map((e) => `${e.caseId}/${e.code}`).join(",")}`,
    );
  }

  const boundary = cut?.boundary;
  if (
    !boundary?.path ||
    !existsRel(boundary.path) ||
    !declaresType(readRel(boundary.path), boundary.typeName)
  ) {
    err(errors, caseId, "boundary-path-missing", `${boundary?.path} ${boundary?.typeName}`);
  }

  const cutIds = (cut?.cutover ?? []).map((row) => row.id);
  if (JSON.stringify(cutIds) !== JSON.stringify([...CUTOVER_IDS])) {
    err(errors, caseId, "cutover-cardinality", `ids ${JSON.stringify(cutIds)}`);
  }

  const concerns = (cut?.inventory ?? []).map((row) => row.concern);
  if (JSON.stringify(concerns) !== JSON.stringify([...INVENTORY_CONCERNS])) {
    err(errors, caseId, "inventory-concern-drift", `concerns ${JSON.stringify(concerns)}`);
  }
  const claimed = new Map();
  for (const row of cut?.inventory ?? []) {
    const owner = row.survivingOwner;
    if (
      !owner?.path ||
      !existsRel(owner.path) ||
      !declaresType(readRel(owner.path), owner.typeName)
    ) {
      err(
        errors,
        caseId,
        "inventory-owner-missing",
        `${row.concern}: ${owner?.path} ${owner?.typeName}`,
      );
    }
    if (row.disposition === "no-residual") {
      if (!row.evidence || row.evidence.length < 60) {
        err(errors, caseId, "inventory-residual-unevidenced", row.concern);
      }
    } else if (row.disposition === "narrowed") {
      if (!row.cutover?.length) {
        err(errors, caseId, "inventory-narrowing-unbound", row.concern);
      }
      for (const id of row.cutover ?? []) {
        if (!cutIds.includes(id)) {
          err(errors, caseId, "inventory-narrowing-unbound", `${row.concern}: ${id}`);
        }
        if (claimed.has(id)) {
          err(
            errors,
            caseId,
            "inventory-narrowing-unbound",
            `${id} claimed by ${claimed.get(id)} and ${row.concern}`,
          );
        }
        claimed.set(id, row.concern);
      }
    } else {
      err(errors, caseId, "inventory-disposition-invented", `${row.concern}: ${row.disposition}`);
    }
  }
  for (const id of cutIds) {
    if (!claimed.has(id))
      err(errors, caseId, "inventory-narrowing-unbound", `${id} has no concern`);
  }

  for (const row of cut?.cutover ?? []) {
    const owner = row.survivingOwner ?? {};
    if (!owner.path || !existsRel(owner.path)) {
      err(errors, caseId, "owner-path-missing", `${row.id} ${owner.path}`);
      continue;
    }
    const ownerSrc = readRel(owner.path);
    if (!declaresType(ownerSrc, owner.typeName)) {
      err(errors, caseId, "owner-type-missing", `${row.id} ${owner.typeName}`);
    }
    for (const method of owner.methods ?? []) {
      if (!declaresFn(ownerSrc, method)) {
        err(errors, caseId, "owner-method-missing", `${row.id} ${owner.typeName}::${method}`);
      }
    }
    if (owner.constructorSignature && !ownerSrc.includes(owner.constructorSignature)) {
      err(errors, caseId, "owner-constructor-drift", `${row.id} ${owner.constructorSignature}`);
    }

    if (row.callForms?.length) {
      for (const consumer of row.consumers ?? []) {
        if (!existsRel(consumer)) {
          err(errors, caseId, "consumer-missing", `${row.id} ${consumer}`);
          continue;
        }
        const text = stripRustComments(readRel(consumer));
        if (!containsCallForm(text, row.callForms)) {
          err(errors, caseId, "consumer-bypasses-owner", `${row.id} ${consumer}`);
        }
      }
    }

    if (row.executesRegisterRow) {
      const reg = arh1["cutover-register"].rows.find((r) => r.id === row.executesRegisterRow);
      if (!reg || reg.owner?.kind !== "node" || reg.owner?.id !== "ARH4") {
        err(errors, caseId, "register-row-unowned", `${row.id} ${row.executesRegisterRow}`);
      }
      const routePath = reg?.route?.split("#")[0];
      if (routePath !== owner.path) {
        err(
          errors,
          caseId,
          "register-row-unowned",
          `${row.id} route ${reg?.route} is not ${owner.path}`,
        );
      }
      if (owner.field) {
        const visibility = owner.visibility.replace(/[()]/g, "\\$&");
        if (!new RegExp(String.raw`(?:^|\n)\s*${visibility}\s+${owner.field}\s*:`).test(ownerSrc)) {
          err(
            errors,
            caseId,
            "register-row-unexecuted",
            `${row.id} ${owner.field} is not ${owner.visibility}`,
          );
        }
      }
      if (owner.gate) {
        const contract = arh1["dependency-contracts"].hotspots.find((h) => h.path === owner.path);
        const hooks = (contract?.minimalPublicSurface?.narrow ?? []).filter(
          (n) => n.kind === "fn" && n.to === "test-configuration",
        );
        if (hooks.length === 0) {
          err(
            errors,
            caseId,
            "register-row-unexecuted",
            `${row.id} the ARH1 contract narrows no hooks`,
          );
        }
        for (const hook of hooks) {
          const attrs = attributesAbove(ownerSrc, hook.item);
          if (!attrs || !attrs.some((a) => a.startsWith("#[cfg(") && a.includes("test"))) {
            err(
              errors,
              caseId,
              "register-row-unexecuted",
              `${row.id} ${hook.item} is not test-gated`,
            );
          }
        }
      }
    }
  }

  // The production population reaching the audit runtime's drain/publish
  // forms is exactly the recorded consumer set, both ways.
  const auditRow = (cut?.cutover ?? []).find((row) => row.id === "ARH4-CUT-1");
  if (auditRow?.callForms?.length) {
    const live = deriveCallFormPopulation(auditRow.callForms);
    const recorded = [...(auditRow.consumers ?? [])].sort();
    if (JSON.stringify(live) !== JSON.stringify(recorded)) {
      err(
        errors,
        caseId,
        "consumer-population-drift",
        `live ${JSON.stringify(live)} recorded ${JSON.stringify(recorded)}`,
      );
    }
  }
}

function validateAuthority(cut, errors) {
  const caseId = "ARH4-authority";
  const ac2 = cut?.ac2;
  if (!ac2?.defect || ac2.defect.length < 40) {
    err(errors, caseId, "ac2-defect-missing", String(ac2?.defect));
  }
  const files = (ac2?.evidence ?? []).map((e) => `${e.file}::${e.test}`).sort();
  const expected = [
    "crates/verter_session/tests/cases/g_audit/audit_records_per_host_isolated.rs::audit_records_per_host_isolated_two_hosts_dedicated_record_stores",
    "crates/verter_session/tests/cases/g_audit/audit_request_ids_share_one_host_key_space.rs::unregistered_and_audited_records_share_one_host_minted_key_space",
  ];
  if (JSON.stringify(files) !== JSON.stringify(expected)) {
    err(errors, caseId, "ac2-evidence-population-drift", JSON.stringify(files));
  }
  for (const ev of ac2?.evidence ?? []) {
    if (!existsRel(ev.file)) {
      err(errors, caseId, "ac2-evidence-missing", ev.file);
    } else if (!hasRustTest(ev.file, ev.test)) {
      err(errors, caseId, "ac2-evidence-unbound", `${ev.test} in ${ev.file}`);
    }
  }
}

function validateWork(cut, errors) {
  const caseId = "ARH4-work";
  const names = (cut?.ac3?.concerns ?? []).map((c) => c.concern);
  if (JSON.stringify(names) !== JSON.stringify([...AC3_CONCERNS])) {
    err(errors, caseId, "ac3-concern-missing", `concerns ${JSON.stringify(names)}`);
  }
  for (const concern of cut?.ac3?.concerns ?? []) {
    if (concern.applicable === true) {
      if (!concern.evidence?.length) {
        err(errors, caseId, "ac3-concern-without-evidence", concern.concern);
      }
      for (const ev of concern.evidence ?? []) {
        if (!existsRel(ev.file)) {
          err(errors, caseId, "ac3-evidence-missing", `${concern.concern} ${ev.file}`);
        } else if (!hasRustTest(ev.file, ev.test)) {
          err(errors, caseId, "ac3-evidence-not-a-test", `${ev.test} missing in ${ev.file}`);
        }
      }
    } else if (concern.applicable === false) {
      if (!concern.rationale || concern.rationale.length < 40) {
        err(errors, caseId, "ac3-na-rationale-missing", concern.concern);
      }
    } else {
      err(errors, caseId, "ac3-concern-invented", concern.concern);
    }
  }
}

function validateDelivery(cut, errors) {
  const caseId = "ARH4-delivery";
  if (!cut?.ac4Rationale || cut.ac4Rationale.length < 80) {
    err(errors, caseId, "ac4-rationale-missing", "ac4Rationale");
  }
  if (!cut?.migration?.length) {
    err(errors, caseId, "migration-note-missing", "migration");
  }
  const reference = existsRel(API_REFERENCE) ? readRel(API_REFERENCE) : "";
  for (const row of cut?.migration ?? []) {
    if (!row.from || !row.to || !reference.includes(row.to)) {
      err(errors, caseId, "migration-note-undocumented", `${row.from} -> ${row.to}`);
    }
  }
}

function validateCost(cut, errors) {
  const caseId = "ARH4-cost";
  const run = cut?.evidenceRun;
  if (!run || run.id !== "arh4-perf" || typeof run.applicable !== "boolean") {
    err(errors, caseId, "evidence-run-undecided", JSON.stringify(run));
  } else if (run.applicable !== existsRel(run.manifest)) {
    err(
      errors,
      caseId,
      "evidence-run-manifest-drift",
      `applicable=${run.applicable} but ${run.manifest} ${existsRel(run.manifest) ? "exists" : "is absent"}`,
    );
  }
  if (!cut?.ac5Rationale || !cut.ac5Rationale.includes("arh4-perf")) {
    err(errors, caseId, "ac5-rationale-missing", "must record the arh4-perf disposition");
  }
  const text = JSON.stringify(cut);
  for (const key of ["wallNs", "speedup", "peakRssBytes", "durationMs"]) {
    if (text.includes(`"${key}"`)) {
      err(errors, caseId, "dimension-commits-wall-clock", key);
    }
  }
}

export function validate(products, manifest = loadManifest(), arh1 = loadArh1Products()) {
  const errors = [];
  const cut = products["lifecycle-cutover"];
  validateRatification(cut, manifest, errors);
  validateCutover(cut, arh1, errors);
  validateAuthority(cut, errors);
  validateWork(cut, errors);
  validateDelivery(cut, errors);
  validateCost(cut, errors);
  return { ok: errors.length === 0, errors };
}

const isMain =
  process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url));
if (isMain) {
  const result = validate(loadProducts());
  if (!result.ok) {
    console.error(result.errors.map((e) => `${e.caseId}/${e.code}: ${e.detail}`).join("\n"));
    process.exit(1);
  }
  console.log(`ARH4 verify: PASS cases=${mandatoryCases().join(",")}`);
}
