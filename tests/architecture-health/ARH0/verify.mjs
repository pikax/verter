#!/usr/bin/env node
/**
 * ARH0 inventory verifier — the sole owning interface of the live
 * responsibility and debt inventory.
 *
 * Validates the four products against each other and against the working
 * tree: internal consistency (schema, totals, duplicate ids), on-disk path
 * existence inside the repository tree (rows whose paths bind TAMA-database
 * DAG records instead of repo tree paths must say so via provenance
 * "tama-dag"), exact version pins bound to their named source property
 * (parsed, never text-searched, so another dependency's version in the same
 * file cannot satisfy the binding), and the inventory-to-ownership coverage
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

/**
 * A repo-relative path binds the repository tree only when it resolves back
 * inside REPO_ROOT. `path.join` normalizes `..` segments, so a traversal value
 * such as `..` would otherwise resolve to an existing directory outside the
 * tree and pass a bare existence check; an absolute value is rejected by
 * `path.resolve` the same way. Callers that validate path-typed product fields
 * (modules, consumers, version sources, command targets) all route through
 * this gate.
 */
function existsRel(rel) {
  if (typeof rel !== "string" || rel.length === 0) return false;
  const resolved = path.resolve(REPO_ROOT, rel);
  const rootPrefix = REPO_ROOT.endsWith(path.sep) ? REPO_ROOT : REPO_ROOT + path.sep;
  if (resolved !== REPO_ROOT && !resolved.startsWith(rootPrefix)) return false;
  return fs.existsSync(resolved);
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

  // The manifest must record this verifier's canonical commands verbatim.
  // A shape-plus-existence check would accept any existing script (e.g.
  // `node scripts/affected-tests.mjs`), claiming a check that never runs.
  // The strings are derived from this module's own location so they cannot
  // drift when the tree moves.
  const toRepoPosix = (abs) => path.relative(REPO_ROOT, abs).split(path.sep).join("/");
  const canonical = [
    ["verify", manifest.verify, `node ${toRepoPosix(fileURLToPath(import.meta.url))}`],
    ["test", manifest.test, `node --test ${toRepoPosix(path.join(HERE, "arh0.test.mjs"))}`],
  ];
  for (const [key, command, expected] of canonical) {
    if (command !== expected) {
      errors.push({
        caseId,
        code: "manifest-command-drift",
        detail: `manifest ${key} command ${JSON.stringify(command)} is not the canonical ${JSON.stringify(expected)}`,
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
    // is churn, not coupling, and a zero/negative/non-integer count is
    // measured evidence of no coupling, not of a god module.
    const isPositiveCount = (value) =>
      typeof value === "number" && Number.isInteger(value) && value > 0;
    const hasCouplingCount =
      isPositiveCount(row.couplingEvidence?.fanIn) ||
      isPositiveCount(row.couplingEvidence?.sharedCommits);
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

/**
 * Reads the exact pinned value a capability row claims, from the named
 * property of its source — never by searching the whole file, so another
 * dependency's version in the same file cannot satisfy the binding.
 *
 * Binding modes by source shape:
 * - `*.json` — `versionProperty` is a dotted path into the parsed JSON
 *   (e.g. `devDependencies.typescript`).
 * - `pnpm-lock.yaml` — `versionProperty` is
 *   `importers.<importer>.<section>.<dependency>`; the importer's resolved
 *   `version:` is returned with any peer-resolution suffix `(...)` stripped.
 *   Importer paths and dependency names here contain no dots.
 * - other text (Cargo.toml) — either a bare `property = "value"` declaration
 *   line (e.g. `oxc_parser`), or a `section.property` lookup resolved inside
 *   the named `[section]` table (e.g. `workspace.package.version`).
 */
function readPinnedVersion(versionSource, versionProperty, errors, caseId, capability) {
  if (typeof versionProperty !== "string" || versionProperty.length === 0) {
    errors.push({
      caseId,
      code: "version-without-property",
      detail: `${capability}: implemented rows must bind version to a named source property`,
    });
    return { ok: false };
  }
  const text = fs.readFileSync(path.join(REPO_ROOT, versionSource), "utf8");
  let actual;
  if (versionSource.endsWith(".json")) {
    let data;
    try {
      data = JSON.parse(text);
    } catch {
      data = null;
    }
    actual =
      data == null
        ? undefined
        : versionProperty
            .split(".")
            .reduce((obj, key) => (obj == null ? undefined : obj[key]), data);
  } else if (versionSource === "pnpm-lock.yaml") {
    actual = lockfileResolvedVersion(text, versionProperty);
  } else {
    actual = textDeclarationValue(text, versionProperty);
  }
  return { ok: true, actual };
}

/** pnpm-lock resolved version for `importers.<importer>.<section>.<dep>`. */
function lockfileResolvedVersion(text, versionProperty) {
  const parts = versionProperty.split(".");
  if (parts.length !== 4 || parts[0] !== "importers") return undefined;
  const [, importer, section, dep] = parts;
  const lines = text.split(/\r?\n/);
  let i = 0;
  // Importer block header: exactly two-space indented `path:`.
  while (i < lines.length && !/^ {2}(?:"[^"]+"|'[^']+'|\S+):\s*$/.test(lines[i])) i++;
  for (; i < lines.length; i++) {
    const header = lines[i].match(/^ {2}(?:"([^"]+)"|'([^']+)'|(\S+)):\s*$/);
    if (!header) continue;
    const name = header[1] ?? header[2] ?? header[3];
    if (name !== importer) continue;
    // Inside the importer: find the section then the dependency key.
    const sectionRe = new RegExp(`^ {4}${escapeRegExp(section)}:\\s*$`);
    const depRe = new RegExp(`^ {6}(?:"([^"]+)"|'([^']+)'|(\\S+)):\\s*$`);
    let j = i + 1;
    let inSection = false;
    for (; j < lines.length; j++) {
      const line = lines[j];
      if (/^ {0,2}\S/.test(line)) return undefined; // left importers block
      if (sectionRe.test(line)) {
        inSection = true;
        continue;
      }
      if (/^ {4}\S/.test(line)) inSection = false; // a different section
      if (!inSection) continue;
      const depMatch = line.match(depRe);
      const depName = depMatch ? (depMatch[1] ?? depMatch[2] ?? depMatch[3]) : null;
      if (depName !== dep) continue;
      const versionLine = lines.slice(j + 1, j + 4).find((l) => /^ {8}version: /.test(l));
      const value = versionLine?.match(/^ {8}version: (\S+)(?:\s|$)/)?.[1];
      // Peer-resolution suffixes (`8.0.14(@types/node@...)`) are install
      // graph annotations, not part of the resolved version.
      return value?.replace(/\(.*\)$/, "");
    }
    return undefined;
  }
  return undefined;
}

/** `<property> = "value"` declaration, or `section.property` inside [section]. */
function textDeclarationValue(text, versionProperty) {
  const parts = versionProperty.split(".");
  const property = parts[parts.length - 1];
  const section = parts.length > 1 ? parts.slice(0, -1).join(".") : null;
  const declRe = new RegExp(`^\\s*${escapeRegExp(property)}\\s*=\\s*"([^"]+)"`, "m");
  if (section === null) {
    return text.match(declRe)?.[1];
  }
  const sectionStart = text.indexOf(`[${section}]`);
  if (sectionStart === -1) return undefined;
  const after = text.slice(sectionStart);
  const nextTable = after.slice(1).search(/^\[/m);
  const body = nextTable === -1 ? after : after.slice(0, nextTable + 1);
  return body.match(declRe)?.[1];
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
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
      const pin = readPinnedVersion(
        row.versionSource,
        row.versionProperty,
        errors,
        caseId,
        row.capability,
      );
      if (pin.ok && (typeof pin.actual !== "string" || pin.actual !== row.version)) {
        errors.push({
          caseId,
          code: "version-not-pinned-in-source",
          detail: `${row.capability}: ${row.versionSource} property ${row.versionProperty} pins ${JSON.stringify(pin.actual)}, not ${JSON.stringify(row.version)}`,
        });
      }
      // Executable names and compilation targets are build identity, not
      // tool revisions; they are validated as text anchors in their own
      // source file and must never occupy the version field.
      const identity = row.buildIdentity;
      if (identity != null) {
        const malformed =
          typeof identity !== "object" ||
          typeof identity.kind !== "string" ||
          typeof identity.value !== "string" ||
          typeof identity.source !== "string";
        if (malformed || !existsRel(identity?.source)) {
          errors.push({
            caseId,
            code: "build-identity-not-in-source",
            detail: `${row.capability}: buildIdentity needs kind, value and an in-repo source`,
          });
        } else if (
          !fs.readFileSync(path.join(REPO_ROOT, identity.source), "utf8").includes(identity.value)
        ) {
          errors.push({
            caseId,
            code: "build-identity-not-in-source",
            detail: `${row.capability}: build identity "${identity.value}" does not appear in ${identity.source}`,
          });
        }
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
  return parseWorkspacePackageEntries(
    fs.readFileSync(path.join(REPO_ROOT, "pnpm-workspace.yaml"), "utf8"),
  );
}

/**
 * Literal (non-`packages/` glob) entries of the `packages:` list. Comments
 * (full-line and inline) and single- or double-quoted entries are part of the
 * supported syntax: a comment line inside the list must not end it, and every
 * entry after one must still be checked by the coverage join. `#` never
 * occurs inside pnpm package patterns, so comment stripping is textual.
 */
export function parseWorkspacePackageEntries(text) {
  const entries = [];
  let inPackages = false;
  for (const raw of text.split(/\r?\n/)) {
    const comment = raw.indexOf("#");
    const line = comment === -1 ? raw : raw.slice(0, comment);
    if (!inPackages) {
      if (/^packages:\s*$/.test(line)) inPackages = true;
      continue;
    }
    const item = line.match(/^\s+-\s+(?:"([^"\s]+)"|'([^'\s]+)'|([^\s'"]+))\s*$/);
    if (item) {
      entries.push(item[1] ?? item[2] ?? item[3]);
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
