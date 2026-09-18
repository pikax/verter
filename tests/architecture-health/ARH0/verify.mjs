#!/usr/bin/env node
/**
 * ARH0 inventory verifier — the sole owning interface of the live
 * responsibility and debt inventory (contracts/web-product-expansion-v1.md
 * Part II, the ARH0 half of the co-owned two-part contract).
 *
 * Validates the four products against each other, against the repo authority
 * (trains = roadmap charters dirs, nodes = authority DAG ids), against the
 * working tree (paths exist; implemented capability pins appear verbatim in
 * their pinned source file) and against the contract text itself (the ARH0
 * Part II markers must be present; a merge that keeps only the WDX0 Part I
 * fails ARH0-ratification). ARH0-AC2's counterexample is enforced here:
 * a god-module row whose only evidence is size is rejected, and a previously
 * split Phase 11 target cannot be reclassified without fresh multi-
 * responsibility evidence.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = path.resolve(HERE, "../../..");
export const ROADMAP_ROOT = path.join(REPO_ROOT, "roadmap/0.1.0-tama");

export const PRODUCT_FILES = Object.freeze([
  "codebase-inventory.json",
  "responsibility-map.json",
  "capability-matrix.json",
  "debt-register.json",
]);

export const CONTRACT_FILE = "web-product-expansion-v1.md";

/**
 * Markers identifying the ARH0 half of the co-owned contract. The file is
 * namespaced in two parts (Part I = WDX0 ownership/disposition/claim law,
 * Part II = this inventory) because § numbers restart per part; losing the
 * Part II half or its namespacing must fail the ratification case.
 */
const ARH0_RATIFICATION_MARKERS = Object.freeze([
  "# Part II",
  "live responsibility and debt inventory",
  "## 5. Complexity debt and god-module evidence",
  "## Amendment rule",
]);

export function loadContract() {
  return fs.readFileSync(path.join(ROADMAP_ROOT, "contracts", CONTRACT_FILE), "utf8");
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

/** Repository authority: DAG-declared trains (dir stem <-> train id) + DAG node ids. */
export function loadAuthority() {
  const trains = new Set();
  const nodeIds = new Set();
  const dagDir = path.join(ROADMAP_ROOT, "authority/dag");
  for (const file of fs.readdirSync(dagDir)) {
    if (!file.endsWith(".toml")) continue;
    const text = fs.readFileSync(path.join(dagDir, file), "utf8");
    for (const m of text.matchAll(/^id = "([^"]+)"/gm)) nodeIds.add(m[1]);
    for (const m of text.matchAll(/^train = "([^"]+)"/gm)) trains.add(m[1]);
  }
  const chartersDir = path.join(ROADMAP_ROOT, "charters");
  const chartersStems = new Set(
    fs
      .readdirSync(chartersDir, { withFileTypes: true })
      .filter((e) => e.isDirectory())
      .map((e) => e.name),
  );
  // A train is repo-authoritative only when its charters dir exists; the
  // dir stem is the train id with its final "." rendered as "-".
  const dirStem = (trainId) => trainId.replace(/\./g, "-");
  for (const t of [...trains]) {
    if (!chartersStems.has(dirStem(t))) trains.delete(t);
  }
  return { trains, nodeIds };
}

function existsRel(rel) {
  return fs.existsSync(path.join(REPO_ROOT, rel));
}

function resolveOwner(owner, gapIds, errors, caseId, code = "unknown-owner") {
  if (!owner || typeof owner.id !== "string") {
    errors.push({ caseId, code, detail: "owner without id" });
    return;
  }
  if (owner.kind === "train") {
    if (!gapIds.has(owner.id) && !trainsHas(owner.id)) {
      errors.push({
        caseId,
        code,
        detail: `train ${owner.id} has no charters/ dir and is not declared in planAuthorityGap`,
      });
    }
  } else if (owner.kind === "node") {
    if (!gapIds.has(owner.id) && !nodesHas(owner.id)) {
      errors.push({
        caseId,
        code,
        detail: `node ${owner.id} is not in repo authority DAG and not declared in planAuthorityGap`,
      });
    }
  } else {
    errors.push({ caseId, code, detail: `owner ${owner.id} has unsupported kind` });
  }
}

// bound late to keep loadAuthority() the single authority loader
let authorityRef = null;
const trainsHas = (id) => authorityRef?.trains.has(id) ?? false;
const nodesHas = (id) => authorityRef?.nodeIds.has(id) ?? false;

export function validate(products, authority, contract = loadContract()) {
  authorityRef = authority;
  const errors = [];
  const gapIds = new Set((products["responsibility-map"].planAuthorityGap || []).map((g) => g.id));

  // A gap annotation that has meanwhile appeared in repo authority is stale.
  for (const g of products["responsibility-map"].planAuthorityGap || []) {
    if (authority.trains.has(g.id) || authority.nodeIds.has(g.id)) {
      errors.push({
        caseId: "ARH0-authority",
        code: "stale-gap-annotation",
        detail: `${g.id} is now in repo authority; remove the planAuthorityGap row`,
      });
    }
  }

  validateInventory(products["codebase-inventory"], errors);
  validateResponsibilityMap(products["responsibility-map"], gapIds, errors);
  validateCapabilityMatrix(products["capability-matrix"], errors);
  validateDebtRegister(products["debt-register"], gapIds, errors);
  validateOwnershipCoverage(products, errors);
  validateRatification(contract, errors);
  authorityRef = null;
  return { ok: errors.length === 0, errors };
}

/**
 * ARH0-AC1: the outcome is the charter + this contract's Part II + the four
 * products, validated by this sole owning interface. The verifier therefore
 * fails closed when the co-owned contract loses its ARH0 half (a merge that
 * keeps only the WDX0 Part I) or its part namespacing.
 */
function validateRatification(contract, errors) {
  const caseId = "ARH0-ratification";
  if (typeof contract !== "string" || contract.length === 0) {
    errors.push({ caseId, code: "missing-contract-marker", detail: "contract text missing" });
    return;
  }
  for (const marker of ARH0_RATIFICATION_MARKERS) {
    if (!contract.includes(marker)) {
      errors.push({
        caseId,
        code: "missing-contract-marker",
        detail: `contract missing ARH0 Part II marker ${JSON.stringify(marker)}`,
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

function validateResponsibilityMap(map, gapIds, errors) {
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
    resolveOwner(row.owner, gapIds, errors, caseId);
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
    // Contract §5: coupling evidence is a shared-commit count or fan-in; a
    // touch count is churn, not coupling.
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
    if (!existsRel(row.versionSource)) {
      errors.push({
        caseId,
        code: "missing-version-source",
        detail: `${row.capability}: ${row.versionSource}`,
      });
      continue;
    }
    if (row.status === "implemented") {
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

function validateDebtRegister(register, gapIds, errors) {
  const caseId = "ARH0-debt";
  if (register.schema !== "ARH0DebtRegister") {
    errors.push({ caseId, code: "schema", detail: register.schema });
    return;
  }
  const ids = new Set();
  for (const row of register.rows) {
    if (ids.has(row.id)) errors.push({ caseId, code: "duplicate-id", detail: row.id });
    ids.add(row.id);
    if (!row.candidatePath || !existsRel(row.candidatePath)) {
      errors.push({ caseId, code: "missing-candidate-path", detail: row.id });
    }
    if (!row.disposition) {
      errors.push({ caseId, code: "debt-without-disposition", detail: row.id });
    }
    resolveOwner(row.owner, gapIds, errors, caseId, "unknown-disposition-owner");
    for (const co of row.coOwners || []) {
      resolveOwner({ id: co, kind: "node" }, gapIds, errors, caseId, "unknown-disposition-owner");
    }
  }
}

/**
 * ARH0-AC1: every inventoried production module routes to exactly one
 * surviving owner or an explicit debt-row disposition; joining the inventory
 * against owners/debt-register rejects silent absorption (contract §2/§8).
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
    "ARH0-authority",
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
  const result = validate(loadProducts(), loadAuthority());
  if (!result.ok) {
    console.error(result.errors.map((e) => `${e.caseId}/${e.code}: ${e.detail}`).join("\n"));
    process.exit(1);
  }
  console.log(`ARH0 verify: PASS cases=${mandatoryCases().join(",")}`);
}
