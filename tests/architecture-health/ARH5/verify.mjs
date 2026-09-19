#!/usr/bin/env node
/**
 * ARH5 verifier — sole owning interface of the compiler-facade cutover.
 *
 * Joins the facade-cutover product to the live tree: VueCarrierCompiler
 * production inherent compile_ide/compile_bundle must be test-support
 * gated, parse/identity stay production, typed backends remain the
 * surviving owners, CCA2F retained adapters are not stolen, and the
 * existing compile-fail twins still discriminate a merged
 * frontend/runtime catalog row. Node builtins only.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { loadProducts as loadArh2Products, validate as validateArh2 } from "../ARH2/verify.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = path.resolve(HERE, "../../..");

export const PRODUCT_FILES = Object.freeze(["facade-cutover.json"]);

const TEST_SUPPORT_CFG = '#[cfg(any(test, feature = "test-support"))]';
const AC3_CONCERNS = Object.freeze([
  "fresh-versus-incremental equivalence",
  "edit/revert",
  "cancellation",
  "stale/partial rejection",
  "deterministic ordering under perturbed discovery or scheduling",
]);

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

function productionHalf(src) {
  const match = src.match(/^#\[cfg\(test\)\]\r?\nmod tests/m);
  return match ? src.slice(0, match.index) : src;
}

function implBodies(src, typeName) {
  const re = new RegExp(`impl\\s+${typeName}\\s*\\{`, "g");
  const bodies = [];
  let m;
  while ((m = re.exec(src))) {
    const start = m.index + m[0].length - 1;
    let depth = 0;
    for (let i = start; i < src.length; i++) {
      if (src[i] === "{") depth += 1;
      else if (src[i] === "}") {
        depth -= 1;
        if (depth === 0) {
          bodies.push(src.slice(start + 1, i));
          break;
        }
      }
    }
  }
  return bodies;
}

function pubFns(body) {
  const lines = body.split(/\r?\n/);
  const fns = [];
  for (let i = 0; i < lines.length; i += 1) {
    const match = lines[i].match(/^\s*pub(?:\s*\([^)]*\))?\s+fn\s+([A-Za-z0-9_]+)\s*[<(]/);
    if (!match) continue;
    const attrs = [];
    for (let j = i - 1; j >= 0; j -= 1) {
      const t = lines[j].trim();
      if (t === "") continue;
      if (t.startsWith("///") || t.startsWith("//!") || t.startsWith("*") || t.startsWith("/*")) {
        continue;
      }
      if (t.startsWith("#[")) {
        attrs.unshift(t);
        continue;
      }
      break;
    }
    fns.push({ name: match[1], attrs });
  }
  return fns;
}

function inherentPubFns(src, typeName) {
  return implBodies(productionHalf(src), typeName).flatMap(pubFns);
}

function cfgNormalized(attr) {
  return attr.replace(/\s+/g, "");
}

function isTestSupportGated(attrs) {
  const want = cfgNormalized(TEST_SUPPORT_CFG);
  return attrs.some((a) => cfgNormalized(a) === want);
}

function hasFn(src, name) {
  return new RegExp(String.raw`(?:pub(?:\s*\([^)]*\))?\s+)?fn\s+${name}\s*[<(]`).test(src);
}

function testFnExists(fileSrc, testName) {
  return new RegExp(String.raw`fn\s+${testName}\s*\(`).test(fileSrc);
}

export function mandatoryCases(manifest = loadManifest()) {
  return manifest.cases.map((c) => c.id);
}

export function selectedCaseIds(result) {
  return [...new Set(result.errors.map((e) => e.caseId))];
}

export function validate(products, manifest = loadManifest()) {
  const errors = [];
  const cut = products["facade-cutover"];
  const expectedCases = [
    "ARH5-authority",
    "ARH5-cost",
    "ARH5-cutover",
    "ARH5-delivery",
    "ARH5-ratification",
    "ARH5-work",
  ];
  const liveCases = [...mandatoryCases(manifest)].sort();
  if (JSON.stringify(liveCases) !== JSON.stringify(expectedCases)) {
    err(
      errors,
      "ARH5-ratification",
      "manifest-case-drift",
      `manifest cases ${JSON.stringify(liveCases)} !== ${JSON.stringify(expectedCases)}`,
    );
  }
  const expectedProducts = ["ARH5FacadeCutover"];
  if (JSON.stringify(manifest.products) !== JSON.stringify(expectedProducts)) {
    err(
      errors,
      "ARH5-ratification",
      "manifest-product-drift",
      `manifest products ${JSON.stringify(manifest.products)}`,
    );
  }
  const expectedVerify = "node tests/architecture-health/ARH5/verify.mjs";
  const expectedTest = "node --test tests/architecture-health/ARH5/arh5.test.mjs";
  if (manifest.verify !== expectedVerify) {
    err(errors, "ARH5-ratification", "manifest-command-drift", `verify=${manifest.verify}`);
  }
  if (manifest.test !== expectedTest) {
    err(errors, "ARH5-ratification", "manifest-command-drift", `test=${manifest.test}`);
  }
  if (cut?.schema !== "ARH5FacadeCutover" || cut?.contractNode !== "ARH5") {
    err(
      errors,
      "ARH5-ratification",
      "product-schema-drift",
      `schema=${cut?.schema} node=${cut?.contractNode}`,
    );
  }

  const arh2 = loadArh2Products();
  const arh2Result = validateArh2(arh2);
  if (!arh2Result.ok) {
    err(
      errors,
      "ARH5-cutover",
      "predecessor-drift",
      `ARH2 validate failed: ${arh2Result.errors.map((e) => `${e.caseId}/${e.code}`).join(",")}`,
    );
  }

  const boundaryPath = cut?.boundary?.path;
  if (!boundaryPath || !existsRel(boundaryPath)) {
    err(errors, "ARH5-cutover", "boundary-path-missing", String(boundaryPath));
  } else {
    const src = readRel(boundaryPath);
    const fns = inherentPubFns(src, cut.boundary.typeName);
    const byName = new Map(fns.map((f) => [f.name, f]));
    const retained = [...(cut.boundary.retainedProductionMethods ?? [])].sort();
    const displaced = [...(cut.boundary.displacedProductionMethods ?? [])].sort();
    if (JSON.stringify(displaced) !== JSON.stringify(["compile_bundle", "compile_ide"])) {
      err(
        errors,
        "ARH5-cutover",
        "displaced-population-drift",
        `displaced ${JSON.stringify(displaced)}`,
      );
    }
    if (
      JSON.stringify(retained) !== JSON.stringify(["adapter_id", "carrier_language_id", "parse"])
    ) {
      err(
        errors,
        "ARH5-cutover",
        "retained-population-drift",
        `retained ${JSON.stringify(retained)}`,
      );
    }
    for (const name of retained) {
      const fn = byName.get(name);
      if (!fn) {
        err(errors, "ARH5-cutover", "retained-method-missing", `${cut.boundary.typeName}::${name}`);
      } else if (isTestSupportGated(fn.attrs)) {
        err(
          errors,
          "ARH5-cutover",
          "retained-method-gated",
          `${name} must stay on the production parse/identity surface`,
        );
      }
    }
    for (const name of displaced) {
      const fn = byName.get(name);
      if (!fn) {
        err(
          errors,
          "ARH5-cutover",
          "displaced-method-missing",
          `${name} must remain as a test-support shim (CMP1 still owns the conversion)`,
        );
      } else if (!isTestSupportGated(fn.attrs)) {
        err(
          errors,
          "ARH5-cutover",
          "displaced-method-still-production",
          `${cut.boundary.typeName}::${name} is still a production inherent method`,
        );
      }
    }
  }

  const sveltePath = cut?.parallelAlreadyNarrow?.path;
  if (!sveltePath || !existsRel(sveltePath)) {
    err(errors, "ARH5-cutover", "parallel-path-missing", String(sveltePath));
  } else {
    const svelteFns = inherentPubFns(readRel(sveltePath), cut.parallelAlreadyNarrow.typeName);
    const combined = svelteFns.filter(
      (f) => f.name === "compile_ide" || f.name === "compile_bundle",
    );
    if (combined.length > 0) {
      err(
        errors,
        "ARH5-cutover",
        "parallel-combined-method",
        `SvelteCarrierCompiler still has ${combined.map((f) => f.name).join(",")}`,
      );
    }
  }

  const cutIds = (cut?.cutover ?? []).map((row) => row.id);
  if (JSON.stringify(cutIds) !== JSON.stringify(["ARH5-CUT-1", "ARH5-CUT-2"])) {
    err(errors, "ARH5-cutover", "cutover-cardinality", `ids ${JSON.stringify(cutIds)}`);
  }
  for (const row of cut?.cutover ?? []) {
    const ownerPath = row.survivingOwner?.path;
    if (!ownerPath || !existsRel(ownerPath)) {
      err(errors, "ARH5-cutover", "owner-path-missing", `${row.id} ${ownerPath}`);
      continue;
    }
    const ownerSrc = readRel(ownerPath);
    if (
      row.survivingOwner.typeName &&
      !ownerSrc.includes(`struct ${row.survivingOwner.typeName}`)
    ) {
      err(
        errors,
        "ARH5-cutover",
        "owner-type-missing",
        `${row.id} missing struct ${row.survivingOwner.typeName}`,
      );
    }
    if (row.survivingOwner.method && !hasFn(ownerSrc, row.survivingOwner.method)) {
      err(
        errors,
        "ARH5-cutover",
        "owner-method-missing",
        `${row.id} missing fn ${row.survivingOwner.method}`,
      );
    }
    if (row.orchestration) {
      if (
        !existsRel(row.orchestration.path) ||
        !hasFn(readRel(row.orchestration.path), row.orchestration.fn)
      ) {
        err(
          errors,
          "ARH5-cutover",
          "orchestration-missing",
          `${row.id} missing ${row.orchestration.fn}`,
        );
      }
    }
  }

  for (const adapter of cut?.retainedAdapters ?? []) {
    if (!adapter.liveWitness || !existsRel(adapter.liveWitness)) {
      err(errors, "ARH5-cutover", "retained-adapter-witness-missing", String(adapter.liveWitness));
      continue;
    }
    const witness = readRel(adapter.liveWitness);
    for (const symbol of adapter.symbols ?? []) {
      if (!witness.includes(symbol)) {
        err(
          errors,
          "ARH5-cutover",
          "retained-adapter-stolen",
          `${adapter.owner} symbol ${symbol} missing from ${adapter.liveWitness}`,
        );
      }
    }
  }
  if ((cut?.retainedAdapters ?? []).length !== 2) {
    err(
      errors,
      "ARH5-cutover",
      "retained-adapter-cardinality",
      `expected CMP1 pair, got ${(cut?.retainedAdapters ?? []).length}`,
    );
  }

  const ac2 = cut?.ac2;
  if (!ac2?.defect?.includes("merges semantic and runtime")) {
    err(errors, "ARH5-authority", "ac2-defect-missing", String(ac2?.defect));
  }
  const evidenceFiles = (ac2?.evidence ?? []).map((e) => e.file);
  const requiredEvidence = [
    "crates/verter_compiler/tests/cases/compile-fail/frontend_only_has_no_runtime_accessor.rs",
    "crates/verter_compiler/tests/cases/compile-fail/projection_only_has_no_runtime_accessor.rs",
    "crates/verter_compiler/tests/cases/compile-fail/vue_carrier_compiler_has_no_production_compile_bundle.rs",
    "crates/verter_compiler/tests/cases/compile-fail/vue_carrier_compiler_has_no_production_compile_ide.rs",
  ];
  if (JSON.stringify([...evidenceFiles].sort()) !== JSON.stringify([...requiredEvidence].sort())) {
    err(
      errors,
      "ARH5-authority",
      "ac2-evidence-population-drift",
      `evidence ${JSON.stringify(evidenceFiles)}`,
    );
  }
  for (const file of evidenceFiles) {
    if (!existsRel(file)) {
      err(errors, "ARH5-authority", "ac2-evidence-missing", file);
    }
  }
  const bundleFail = requiredEvidence[2];
  const ideFail = requiredEvidence[3];
  if (existsRel(bundleFail) && !readRel(bundleFail).includes("compile_bundle")) {
    err(errors, "ARH5-authority", "ac2-evidence-unbound", bundleFail);
  }
  if (existsRel(ideFail) && !readRel(ideFail).includes("compile_ide")) {
    err(errors, "ARH5-authority", "ac2-evidence-unbound", ideFail);
  }

  const concerns = cut?.ac3?.concerns ?? [];
  const concernNames = concerns.map((c) => c.concern);
  if (JSON.stringify(concernNames) !== JSON.stringify([...AC3_CONCERNS])) {
    err(errors, "ARH5-work", "ac3-concern-missing", `concerns ${JSON.stringify(concernNames)}`);
  }
  for (const concern of concerns) {
    if (concern.applicable === true) {
      if (!concern.evidence?.length) {
        err(errors, "ARH5-work", "ac3-concern-without-evidence", concern.concern);
      }
      for (const ev of concern.evidence ?? []) {
        if (!existsRel(ev.file)) {
          err(errors, "ARH5-work", "ac3-evidence-missing", `${concern.concern} ${ev.file}`);
          continue;
        }
        if (ev.test && !testFnExists(readRel(ev.file), ev.test)) {
          err(errors, "ARH5-work", "ac3-evidence-not-a-test", `${ev.test} missing in ${ev.file}`);
        }
      }
    } else if (concern.applicable === false) {
      if (!concern.rationale || concern.rationale.length < 20) {
        err(errors, "ARH5-work", "ac3-na-rationale-missing", concern.concern);
      }
    } else {
      err(errors, "ARH5-work", "ac3-concern-invented", concern.concern);
    }
  }

  if (!cut?.ac4Rationale || cut.ac4Rationale.length < 40) {
    err(errors, "ARH5-delivery", "ac4-rationale-missing", "ac4Rationale");
  }
  const examplesRoot = path.join(REPO_ROOT, "examples/reference");
  if (!fs.existsSync(examplesRoot)) {
    err(errors, "ARH5-delivery", "examples-reference-missing", "examples/reference");
  } else {
    const walk = (dir) => {
      let hit = false;
      for (const ent of fs.readdirSync(dir, { withFileTypes: true })) {
        const p = path.join(dir, ent.name);
        if (ent.isDirectory()) hit = walk(p) || hit;
        else if (/\.(md|ts|js|mjs)$/.test(ent.name)) {
          const text = fs.readFileSync(p, "utf8");
          if (text.includes("VueCarrierCompiler") && /compile_(ide|bundle)/.test(text)) {
            err(
              errors,
              "ARH5-delivery",
              "public-example-still-names-displaced-route",
              path.relative(REPO_ROOT, p).replaceAll("\\", "/"),
            );
            hit = true;
          }
        }
      }
      return hit;
    };
    walk(examplesRoot);
  }

  if (!cut?.ac5Rationale || !cut.ac5Rationale.includes("CMP1")) {
    err(
      errors,
      "ARH5-cost",
      "ac5-rationale-missing",
      "must record that CMP1 adapters are retained",
    );
  }
  const numericKeys = JSON.stringify(cut);
  for (const key of ["wallNs", "speedup", "peakRssBytes", "durationMs"]) {
    if (numericKeys.includes(`"${key}"`)) {
      err(errors, "ARH5-cost", "dimension-commits-wall-clock", key);
    }
  }

  const guardPath = cut?.sessionGuard?.path;
  if (!guardPath || !existsRel(guardPath)) {
    err(errors, "ARH5-cutover", "session-guard-missing", String(guardPath));
  } else {
    const guard = readRel(guardPath);
    for (const token of cut.sessionGuard.forbidden ?? []) {
      if (!guard.includes(`"${token}"`)) {
        err(errors, "ARH5-cutover", "session-guard-token-missing", token);
      }
    }
  }

  return { ok: errors.length === 0, errors };
}

const isMain =
  process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url));
if (isMain) {
  const products = loadProducts();
  const result = validate(products);
  if (!result.ok) {
    console.error(result.errors.map((e) => `${e.caseId}/${e.code}: ${e.detail}`).join("\n"));
    process.exit(1);
  }
  console.log(`ARH5 verify: PASS cases=${mandatoryCases().join(",")}`);
}
