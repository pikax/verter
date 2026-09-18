#!/usr/bin/env node
/**
 * ARH0 inventory verifier — the sole owning interface of the live
 * responsibility and debt inventory.
 *
 * Validates the four products against each other and against the working
 * tree: internal consistency (schema, totals, duplicate ids), on-disk path
 * existence (rows whose paths bind TAMA-database DAG records instead of
 * repo tree paths must say so via provenance "tama-dag"), verbatim version
 * pins in their pinned source file, and the inventory-to-ownership coverage
 * join. The program DAG is database-owned by the TAMA controller, so owner
 * ids are checked structurally only and no DAG file is read. ARH0-AC2's
 * counterexample is enforced here: a god-module row whose only evidence is
 * size is rejected, and a previously split target cannot be reclassified
 * without fresh multi-responsibility evidence. ARH0-ratification fails when
 * the manifest and the verifier disagree about the case contract, so the
 * manifest cannot claim checks that do not run.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = path.resolve(HERE, "../../..");

export const PRODUCT_FILES = Object.freeze([
  "codebase-inventory.json",
  "responsibility-map.json",
  "capability-matrix.json",
  "debt-register.json",
]);

/** Provenance marker for rows whose path fields bind TAMA-database DAG
 * records rather than repo tree paths; the repo carries no DAG copy. */
const TAMA_DAG_PROVENANCE = "tama-dag";

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

function existsRel(rel) {
  return fs.existsSync(path.join(REPO_ROOT, rel));
}

/**
 * Owner ids reference the TAMA-controller DAG (kind "train" = dotted id,
 * kind "node" = uppercase letters followed by digits). The repository holds
 * no authority copy to resolve them against, so this checks structure only.
 */
function resolveOwner(owner, errors, caseId, code = "malformed-owner") {
  if (!owner || typeof owner.id !== "string" || owner.id.length === 0) {
    errors.push({ caseId, code, detail: "owner without id" });
    return;
  }
  if (owner.kind === "train") {
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*(?:\.[a-z0-9]+(?:-[a-z0-9]+)*)+$/.test(owner.id)) {
      errors.push({
        caseId,
        code,
        detail: `train id ${owner.id} is not a dotted lowercase train reference`,
      });
    }
  } else if (owner.kind === "node") {
    if (!/^[A-Z][A-Z0-9]*[0-9]+$/.test(owner.id)) {
      errors.push({
        caseId,
        code,
        detail: `node id ${owner.id} is not an uppercase node reference`,
      });
    }
  } else {
    errors.push({ caseId, code, detail: `owner ${owner.id} has unsupported kind` });
  }
}

/**
 * A path-typed field must point at the repo tree, or the row must bind a
 * TAMA-database DAG record explicitly (null path + provenance marker).
 */
function bindsRepoPath(value, provenance) {
  if (value != null) return { ok: typeof value === "string" && existsRel(value) };
  return { ok: provenance === TAMA_DAG_PROVENANCE, tamaDag: true };
}

export function validate(products, manifest = loadManifest()) {
  const errors = [];

  validateInventory(products["codebase-inventory"], errors);
  validateResponsibilityMap(products["responsibility-map"], errors);
  validateCapabilityMatrix(products["capability-matrix"], errors);
  validateDebtRegister(products["debt-register"], errors);
  validateOwnershipCoverage(products, errors);
  validateRatification(products, manifest, errors);
  return { ok: errors.length === 0, errors };
}

/**
 * ARH0-AC1: the manifest documents this node's case contract and commands;
 * the verifier is the sole owning interface that runs them. A manifest that
 * claims a case the verifier does not implement (or omits one it does), a
 * product the inventory no longer carries, or a verify/test command whose
 * file is absent fails here, so prose cannot outlive the checks.
 */
function validateRatification(products, manifest, errors) {
  const caseId = "ARH0-ratification";
  if (!manifest || typeof manifest !== "object") {
    errors.push({ caseId, code: "manifest-case-drift", detail: "manifest missing" });
    return;
  }
  const recorded = new Set(manifest.products || []);
  const actual = new Set(
    PRODUCT_FILES.map((file) => products[file.replace(/\.json$/, "")]?.schema).filter(Boolean),
  );
  for (const schema of actual) {
    if (!recorded.has(schema)) {
      errors.push({
        caseId,
        code: "manifest-product-drift",
        detail: `product schema ${schema} is carried but not recorded in the manifest`,
      });
    }
  }
  for (const schema of recorded) {
    if (!actual.has(schema)) {
      errors.push({
        caseId,
        code: "manifest-product-drift",
        detail: `manifest records product ${schema} but no product carries that schema`,
      });
    }
  }

  const mandatory = new Set(mandatoryCases());
  const claimed = new Set((manifest.cases || []).map((c) => c.id));
  for (const id of mandatory) {
    if (!claimed.has(id)) {
      errors.push({
        caseId,
        code: "manifest-case-drift",
        detail: `verifier implements case ${id} but the manifest does not record it`,
      });
    }
  }
  for (const c of manifest.cases || []) {
    if (!mandatory.has(c.id)) {
      errors.push({
        caseId,
        code: "manifest-case-drift",
        detail: `manifest records case ${c.id} but the verifier does not implement it`,
      });
    }
    if (c.disposition !== "reject") {
      errors.push({
        caseId,
        code: "manifest-case-drift",
        detail: `case ${c.id} must be disposition reject to keep its dirty twins failing`,
      });
    }
    if (!Array.isArray(c.twins) || !c.twins.includes("clean products")) {
      errors.push({
        caseId,
        code: "manifest-case-drift",
        detail: `case ${c.id} must list the "clean products" accept twin`,
      });
    }
  }

  const commands = [
    ["verify", manifest.verify, /^node (.+)$/],
    ["test", manifest.test, /^node --test (.+)$/],
  ];
  for (const [key, command, shape] of commands) {
    const m = typeof command === "string" ? command.match(shape) : null;
    const target = m && m[1];
    if (!target || !existsRel(target)) {
      errors.push({
        caseId,
        code: "manifest-command-drift",
        detail: `manifest ${key} command ${JSON.stringify(command)} does not name an on-disk script`,
      });
    }
  }
}

function validateInventory(inv, errors) {
  const caseId = "ARH0-inventory";
  if (inv.schema !== "ARH0CodebaseInventory") {
    errors.push({ caseId, code: "schema", detail: inv.schema });
    return;
  }
  for (const [population, rowsKey] of [
    ["rustWorkspace", "crates"],
    ["typescriptPackages", "packages"],
  ]) {
    const pop = inv[population];
    const rows = inv[rowsKey];
    for (const field of ["productionLoc", "testLoc", "generatedLoc"]) {
      const sum = rows.reduce((a, r) => a + (r[field] || 0), 0);
      if (sum !== pop[field]) {
        errors.push({
          caseId,
          code: "totals-mismatch",
          detail: `${population}.${field}=${pop[field]} but rows sum to ${sum}`,
        });
      }
    }
    if (rows.length !== pop[rowsKey === "crates" ? "crates" : "packages"]) {
      errors.push({
        caseId,
        code: "row-count-mismatch",
        detail: `${population} declares ${pop[rowsKey === "crates" ? "crates" : "packages"]} rows, found ${rows.length}`,
      });
    }
    for (const row of rows) {
      if (!existsRel(row.module)) {
        errors.push({ caseId, code: "missing-module", detail: row.module });
      }
    }
  }
  if (inv.rustWorkspace.members !== 50) {
    errors.push({
      caseId,
      code: "workspace-members",
      detail: `expected 50 cargo workspace members, recorded ${inv.rustWorkspace.members}`,
    });
  }
}

function validateResponsibilityMap(map, errors) {
  const caseId = "ARH0-ownership";
  if (map.schema !== "ARH0ResponsibilityMap") {
    errors.push({ caseId, code: "schema", detail: map.schema });
    return;
  }
  const seen = new Set();
  for (const row of map.owners) {
    if (seen.has(row.module)) {
      errors.push({ caseId, code: "duplicate-module", detail: row.module });
    }
    seen.add(row.module);
    if (!existsRel(row.module)) {
      errors.push({ caseId, code: "missing-module", detail: row.module });
    }
    resolveOwner(row.owner, errors, caseId);
    if (!Array.isArray(row.responsibility) || row.responsibility.length === 0) {
      errors.push({ caseId, code: "owner-without-responsibility", detail: row.module });
    }
  }

  // ARH0-AC2: size alone never declares a god module.
  for (const row of map.godModuleCandidates) {
    if (!existsRel(row.path)) {
      errors.push({ caseId, code: "missing-module", detail: row.path });
      continue;
    }
    const hasMulti = Array.isArray(row.responsibilities) && row.responsibilities.length >= 2;
    // Coupling evidence is a shared-commit count or fan-in; a touch count
    // is churn, not coupling.
    const hasCouplingCount =
      typeof row.couplingEvidence?.fanIn === "number" ||
      typeof row.couplingEvidence?.sharedCommits === "number";
    const hasCoupling =
      hasCouplingCount &&
      Array.isArray(row.evidenceKinds) &&
      row.evidenceKinds.includes("coupling") &&
      row.evidenceKinds.includes("multi-responsibility");
    if (row.classification === "god-module-candidate" && !(hasMulti && hasCoupling)) {
      errors.push({
        caseId: "ARH0-god-evidence",
        code: "god-without-responsibility-evidence",
        detail: `${row.path}: declared god-module on ${
          !hasMulti ? "size" : !hasCouplingCount ? "touches-only" : "coupling"
        } evidence alone`,
      });
    }
  }

  // ARH0-AC2: a previously split target needs fresh evidence to come back.
  const godPaths = new Set(
    map.godModuleCandidates
      .filter((r) => r.classification === "god-module-candidate")
      .map((r) => r.path),
  );
  for (const row of map.retiredPhase11Targets) {
    if (!existsRel(row.target)) {
      errors.push({ caseId, code: "missing-module", detail: row.target });
      continue;
    }
    const reclassified = row.classification === "god-module-candidate" || godPaths.has(row.target);
    if (reclassified && !row.freshEvidence?.measured) {
      errors.push({
        caseId: "ARH0-god-evidence",
        code: "split-module-reclassified-without-new-evidence",
        detail: `${row.target}: previously split target reclassified without fresh measured multi-responsibility evidence`,
      });
    }
  }

  for (const p of map.generatedDataSources) {
    if (!existsRel(p)) errors.push({ caseId, code: "missing-module", detail: p });
  }
  for (const h of map.changeCoupling?.hotFiles || []) {
    if (!existsRel(h.path)) errors.push({ caseId, code: "missing-module", detail: h.path });
  }
}

function validateCapabilityMatrix(matrix, errors) {
  const caseId = "ARH0-capability";
  if (matrix.schema !== "ARH0CapabilityMatrix") {
    errors.push({ caseId, code: "schema", detail: matrix.schema });
    return;
  }
  for (const row of matrix.rows) {
    const binding = bindsRepoPath(row.versionSource, row.provenance);
    if (!binding.ok) {
      errors.push({
        caseId,
        code: "missing-version-source",
        detail: `${row.capability}: ${row.versionSource ?? "(no path; not marked tama-dag)"}`,
      });
      continue;
    }
    if (row.status === "implemented") {
      if (binding.tamaDag) {
        errors.push({
          caseId,
          code: "missing-version-source",
          detail: `${row.capability}: implemented pins must name a repo source file`,
        });
        continue;
      }
      const src = fs.readFileSync(path.join(REPO_ROOT, row.versionSource), "utf8");
      if (!src.includes(row.version)) {
        errors.push({
          caseId,
          code: "version-not-pinned-in-source",
          detail: `${row.capability}: version "${row.version}" does not appear in ${row.versionSource}`,
        });
      }
      if (!Array.isArray(row.consumers) || row.consumers.length === 0) {
        errors.push({ caseId, code: "implemented-without-consumers", detail: row.capability });
        continue;
      }
      for (const c of row.consumers) {
        if (!existsRel(c)) {
          errors.push({ caseId, code: "missing-consumer", detail: `${row.capability}: ${c}` });
        }
      }
    } else if (row.status === "required-planned") {
      if (row.consumers?.length) {
        errors.push({ caseId, code: "planned-with-consumers", detail: row.capability });
      }
      if (!row.uncertainty) {
        errors.push({ caseId, code: "planned-without-uncertainty", detail: row.capability });
      }
    } else {
      errors.push({ caseId, code: "bad-status", detail: `${row.capability}: ${row.status}` });
    }
  }
}

function validateDebtRegister(register, errors) {
  const caseId = "ARH0-debt";
  if (register.schema !== "ARH0DebtRegister") {
    errors.push({ caseId, code: "schema", detail: register.schema });
    return;
  }
  const ids = new Set();
  for (const row of register.rows) {
    if (ids.has(row.id)) errors.push({ caseId, code: "duplicate-id", detail: row.id });
    ids.add(row.id);
    if (!bindsRepoPath(row.candidatePath, row.provenance).ok) {
      errors.push({ caseId, code: "missing-candidate-path", detail: row.id });
    }
    if (!row.disposition) {
      errors.push({ caseId, code: "debt-without-disposition", detail: row.id });
    }
    resolveOwner(row.owner, errors, caseId, "malformed-disposition-owner");
    for (const co of row.coOwners || []) {
      resolveOwner({ id: co, kind: "node" }, errors, caseId, "malformed-disposition-owner");
    }
  }
}

/**
 * ARH0-AC1: every inventoried production module routes to exactly one
 * surviving owner or an explicit debt-row disposition; joining the inventory
 * against owners/debt-register rejects silent absorption.
 */
function validateOwnershipCoverage(products, errors) {
  const caseId = "ARH0-ownership";
  const owned = new Set(products["responsibility-map"].owners.map((r) => r.module));
  const debt = new Set(products["debt-register"].rows.map((r) => r.candidatePath));
  const checkModule = (module, unownedCode) => {
    if (owned.has(module) && debt.has(module)) {
      errors.push({
        caseId,
        code: "module-double-disposition",
        detail: `${module}: both an owner row and a debt-register row`,
      });
    } else if (!owned.has(module) && !debt.has(module)) {
      errors.push({
        caseId,
        code: unownedCode,
        detail: `${module}: in neither owners nor debt-register (silent absorption)`,
      });
    }
  };
  for (const row of [
    ...products["codebase-inventory"].crates,
    ...products["codebase-inventory"].packages,
  ]) {
    checkModule(row.module, "inventory-module-unowned");
  }
  for (const module of uninventoriedWorkspacePackages()) {
    checkModule(module, "workspace-package-unowned");
  }
}

/**
 * pnpm workspace entries (pnpm-workspace.yaml `packages:`) the inventory does
 * not enumerate as population rows: glob entries under `packages/` expand to
 * typescriptPackages rows, so only literal entries outside it (e.g. `docs`)
 * need their own owner/debt routing here.
 */
function uninventoriedWorkspacePackages() {
  const text = fs.readFileSync(path.join(REPO_ROOT, "pnpm-workspace.yaml"), "utf8");
  const entries = [];
  let inPackages = false;
  for (const line of text.split(/\r?\n/)) {
    if (!inPackages) {
      if (/^packages:\s*$/.test(line)) inPackages = true;
      continue;
    }
    const item = line.match(/^\s+-\s+"?([^"\s]+)"?\s*$/);
    if (item) {
      entries.push(item[1]);
    } else if (line.trim() !== "") {
      inPackages = false;
    }
  }
  return entries.filter((entry) => !entry.startsWith("packages/"));
}

export function mandatoryCases() {
  return [
    "ARH0-inventory",
    "ARH0-ownership",
    "ARH0-god-evidence",
    "ARH0-capability",
    "ARH0-debt",
    "ARH0-ratification",
  ];
}

/** Case ids with at least one recorded error (dirty-twin selection helper). */
export function selectedCaseIds(result) {
  return [...new Set(result.errors.map((e) => e.caseId))];
}

// The manifest documents `node tests/architecture-health/ARH0/verify.mjs` as
// this node's verify command; it must validate the real products, not no-op.
const isMain =
  process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url));
if (isMain) {
  const result = validate(loadProducts());
  if (!result.ok) {
    console.error(result.errors.map((e) => `${e.caseId}/${e.code}: ${e.detail}`).join("\n"));
    process.exit(1);
  }
  console.log(`ARH0 verify: PASS cases=${mandatoryCases().join(",")}`);
}
