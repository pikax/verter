#!/usr/bin/env node
/**
 * ARH1 contract verifier — the sole owning interface of the responsibility,
 * visibility and dependency contracts.
 *
 * Validates the two products against the shipped ARH0 predecessor products
 * and the working tree: every ARH0 god-module candidate carries exactly one
 * hotspot contract (and vice versa) with no dropped or invented
 * responsibility, each surviving authority/cohesive module exists, the
 * declared import direction equals the import tree actually measured in the
 * live source (and the forbidden directions are absent from the source and
 * the owning crate's Cargo.toml), layer rules match the live crate
 * dependency manifests, constructors and their capability anchors are real
 * declarations, state-lifetime rows name live identifiers and one sole
 * owner, surface declarations and retained/narrowed items bind the live
 * source, no cohesive split retains shared unrestricted state (ARH1-AC2),
 * and every route ARH0 assigned to this node (plus every narrowing route
 * declared here) has exactly one cutover disposition. The program DAG is
 * database-owned by the TAMA controller, so owner/heir ids are checked
 * structurally only and no DAG file is read; the ARH0 products are joined as
 * shipped predecessor evidence, never re-derived here. ARH1-ratification
 * fails when the manifest and the verifier disagree about the case contract,
 * so the manifest cannot claim checks that do not run.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = path.resolve(HERE, "../../..");
const ARH0_DIR = path.resolve(HERE, "../ARH0");

export const PRODUCT_FILES = Object.freeze(["dependency-contracts.json", "cutover-register.json"]);

const TAMA_DAG_PROVENANCE = "tama-dag";
const SKIPPED_ROOTS = new Set(["std", "core", "alloc", "crate", "super", "self"]);
const LIFETIMES = new Set([
  "process",
  "session",
  "request",
  "dispatch-transaction",
  "content-version",
  "snapshot-epoch",
]);
const NARROW_TARGETS = new Set(["pub(crate)", "test-configuration", "feature-gated"]);
const OWNERSHIP_MODES = new Set(["sole", "readonly", "none"]);
// State-name tokens that name the concept, never a live identifier.
const STATE_TOKEN_STOPWORDS = new Set([
  "state",
  "owner",
  "lifetime",
  "the",
  "and",
  "with",
  "tokens",
  "mutable",
  "every",
  "once",
  "frames",
  "friends",
]);

export function loadProducts() {
  const products = {};
  for (const file of PRODUCT_FILES) {
    products[file.replace(/\.json$/, "")] = JSON.parse(
      fs.readFileSync(path.join(HERE, "products", file), "utf8"),
    );
  }
  return products;
}

/** Shipped predecessor evidence: measured on the ARH0 candidate, joined
 * here as products, never re-derived from the live tree. */
export function loadArh0Products() {
  return {
    "responsibility-map": JSON.parse(
      fs.readFileSync(path.join(ARH0_DIR, "products", "responsibility-map.json"), "utf8"),
    ),
    "debt-register": JSON.parse(
      fs.readFileSync(path.join(ARH0_DIR, "products", "debt-register.json"), "utf8"),
    ),
  };
}

export function loadManifest() {
  return JSON.parse(fs.readFileSync(path.join(HERE, "manifest.json"), "utf8"));
}

function existsRel(rel) {
  if (typeof rel !== "string" || rel.length === 0) return false;
  const resolved = path.resolve(REPO_ROOT, rel);
  const rootPrefix = REPO_ROOT.endsWith(path.sep) ? REPO_ROOT : REPO_ROOT + path.sep;
  if (resolved !== REPO_ROOT && !resolved.startsWith(rootPrefix)) return false;
  return fs.existsSync(resolved);
}

function readRel(rel) {
  return fs.readFileSync(path.join(REPO_ROOT, rel), "utf8");
}

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

function bindsRepoPath(value, provenance) {
  if (value != null) return { ok: typeof value === "string" && existsRel(value) };
  return { ok: provenance === TAMA_DAG_PROVENANCE, tamaDag: true };
}

export function validate(products, manifest = loadManifest(), arh0 = loadArh0Products()) {
  const errors = [];
  const contracts = products["dependency-contracts"];
  const cutover = products["cutover-register"];
  if (contracts.schema !== "ARH1DependencyContracts") {
    errors.push({ caseId: "ARH1-hotspot-coverage", code: "schema", detail: contracts.schema });
    return { ok: false, errors };
  }
  if (cutover.schema !== "ARH1CutoverRegister") {
    errors.push({ caseId: "ARH1-cutover", code: "schema", detail: cutover.schema });
    return { ok: false, errors };
  }
  validateHotspotCoverage(contracts, arh0, errors);
  validateImportDirection(contracts, errors);
  validateConstructors(contracts, errors);
  validateStateLifetimes(contracts, errors);
  validateSurface(contracts, errors);
  validateSplit(contracts, arh0, errors);
  validateCutover(contracts, cutover, arh0, errors);
  validateRatification(products, manifest, errors);
  return { ok: errors.length === 0, errors };
}

/**
 * ARH1-AC1: the contract set is exactly the ARH0 inventory's god-module
 * population, and every inventoried responsibility survives under exactly
 * one authority. A hotspot the inventory does not carry, a dropped
 * responsibility or an invented one all fail here.
 */
function validateHotspotCoverage(contracts, arh0, errors) {
  const caseId = "ARH1-hotspot-coverage";
  const godRows = arh0["responsibility-map"].godModuleCandidates.filter(
    (r) => r.classification === "god-module-candidate",
  );
  const godByPath = new Map(godRows.map((r) => [r.path, r]));
  const contractPaths = new Set(contracts.hotspots.map((h) => h.path));
  for (const row of godRows) {
    if (!contractPaths.has(row.path)) {
      errors.push({ caseId, code: "hotspot-without-contract", detail: row.path });
    }
  }
  for (const hotspot of contracts.hotspots) {
    const god = godByPath.get(hotspot.path);
    if (!god) {
      errors.push({ caseId, code: "contract-without-inventory-row", detail: hotspot.path });
      continue;
    }
    if (!existsRel(hotspot.path)) {
      errors.push({ caseId, code: "missing-hotspot", detail: hotspot.path });
    }
    const inventoried = new Set(god.responsibilities);
    const claimed = hotspot.authority.map((a) => a.responsibility);
    for (const responsibility of inventoried) {
      if (!claimed.includes(responsibility)) {
        errors.push({
          caseId,
          code: "responsibility-dropped",
          detail: `${hotspot.path}: ${responsibility} has no surviving authority`,
        });
      }
    }
    const seen = new Set();
    for (const a of hotspot.authority) {
      if (seen.has(a.responsibility)) {
        errors.push({
          caseId,
          code: "authority-double-claimed",
          detail: `${hotspot.path}: ${a.responsibility} claimed twice`,
        });
      }
      seen.add(a.responsibility);
      if (!inventoried.has(a.responsibility)) {
        errors.push({
          caseId,
          code: "responsibility-invented",
          detail: `${hotspot.path}: ${a.responsibility} is not an inventoried responsibility`,
        });
      }
      if (!existsRel(a.survivingOwner)) {
        errors.push({
          caseId,
          code: "missing-authority-owner",
          detail: `${a.responsibility} -> ${a.survivingOwner}`,
        });
      }
    }
  }
}

/** Crate-level verter_* dependencies declared in [dependencies] tables. */
function crateVerterDeps(crateDir) {
  const toml = readRel(path.join(crateDir, "Cargo.toml"));
  const deps = new Set();
  for (const m of toml.matchAll(/^\[(?:workspace\.)?dependencies\]\s*$/gm)) {
    const start = m.index + m[0].length;
    const next = toml.slice(start).search(/^\[/m);
    const body = next === -1 ? toml.slice(start) : toml.slice(start, start + next);
    for (const d of body.matchAll(/^(verter_[a-z_0-9]+)\s*=/gm)) deps.add(d[1]);
  }
  return deps;
}

function owningCrateDir(hotspotPath) {
  const parts = hotspotPath.split("/");
  return parts.slice(0, 2).join("/"); // crates/<crate>
}

/**
 * Measures the import tree of one source file the way the contract declares
 * it: `use`/`pub use` statements (statement text from the keyword to the
 * terminating semicolon, brace groups split into path fragments), first path
 * segment per fragment. Fragments without `::` are brace-list items (they
 * inherit their prefix), not import roots, so they are never classified;
 * `crate::` roots become the crate-internal set; module names declared
 * inside the file (its inline `mod` items) are internal re-exports, not
 * external crates; `super::`/`self::` stay internal without naming a root.
 */
/**
 * Splits a `use` statement body into full import paths: comma-separated at
 * brace depth zero, with each nested group inheriting the path prefix in
 * front of its brace (`a::{b::c, d}` -> `a::b::c`, `a::d`). Renames
 * (`as`) and trailing `::` are trimmed before classification.
 */
export function expandUsePaths(text) {
  const splitTop = (s) => {
    const parts = [];
    let depth = 0;
    let current = "";
    for (const ch of s) {
      if (ch === "{") depth++;
      if (ch === "}") depth--;
      if (ch === "," && depth === 0) {
        parts.push(current);
        current = "";
      } else {
        current += ch;
      }
    }
    if (current.trim()) parts.push(current);
    return parts;
  };
  const walk = (s, prefix) => {
    const out = [];
    for (const part of splitTop(s)) {
      const m = part.match(/^([^{}]*?)\s*\{(.*)\}\s*$/s);
      if (m) {
        // `a::{b}` yields the prefix segment "a::" (the lazy group keeps the
        // colons); strip trailing/leading separators before joining.
        const segment = m[1].trim().replace(/:+$/, "");
        const next = segment ? (prefix ? `${prefix}::${segment}` : segment) : prefix;
        out.push(...walk(m[2], next));
      } else {
        const path = part
          .trim()
          .replace(/\s+as\s+\w+$/, "")
          .replace(/::$/, "");
        if (path) out.push(prefix ? `${prefix}::${path}` : path);
      }
    }
    return out;
  };
  return walk(text, "");
}

/**
 * Measures the import tree of one source file the way the contract declares
 * it: `use`/`pub use` statements expanded into full paths (nested groups
 * inherit their prefix), classified by first path segment. `crate::` roots
 * become the crate-internal set; module names declared inside the file (its
 * inline `mod` items) are internal re-exports, not external crates;
 * `super::`/`self::` stay internal without naming a root.
 */
export function measureImports(text) {
  // Doc and line comments regularly contain the word "use" followed by a
  // semicolon later in the paragraph ("...use a cache id whose value type
  // is stable..."); they are prose, not import statements, so they are
  // stripped before measuring. Block comments are stripped non-nested.
  const source = text.replace(/\/\/[^\n]*/g, "").replace(/\/\*[\s\S]*?\*\//g, "");
  const fileMods = new Set();
  for (const m of source.matchAll(/\b(?:pub(?:\(crate\))? )?mod\s+([a-z_0-9]+)\s*;/g)) {
    fileMods.add(m[1]);
  }
  // Local macro re-exports (`macro_rules! x; pub(crate) use x;`) name file
  // locals, not external crates.
  for (const m of source.matchAll(/\bmacro_rules!\s+(\w+)/g)) {
    fileMods.add(m[1]);
  }
  const verter = new Set();
  const external = new Set();
  const internal = new Set();
  for (const sm of source.matchAll(/\b(?:pub\s+)?use\s([^;]*);/g)) {
    for (const usePath of expandUsePaths(sm[1])) {
      const first = usePath.split("::")[0];
      if (!/^[a-z_][a-z_0-9]*$/.test(first)) continue;
      if (first === "crate") {
        const root = usePath.split("::")[1];
        if (root) internal.add(root);
        continue;
      }
      if (SKIPPED_ROOTS.has(first)) continue; // std/super/self: unnamed internal
      if (first.startsWith("verter_")) verter.add(first);
      else if (!fileMods.has(first)) external.add(first);
    }
  }
  return { verter, external, internal };
}

/**
 * ARH1-AC1: the declared import direction is the measured one — drift in
 * either direction fails, the forbidden directions are absent from the live
 * source and the owning crate's manifest, and the layer rules equal the live
 * crate dependency manifests. A compile-time boundary is the preferred
 * enforcement (ARH12's obligation); this verifier is the contract's
 * owning interface until then.
 */
function validateImportDirection(contracts, errors) {
  const caseId = "ARH1-import-direction";
  for (const hotspot of contracts.hotspots) {
    const declared = hotspot.allowedImportDirection;
    const measured = measureImports(readRel(hotspot.path));
    const cmp = (label, declaredArr, measuredSet) => {
      for (const v of declaredArr) {
        if (!measuredSet.has(v)) {
          errors.push({
            caseId,
            code: "import-drift",
            detail: `${hotspot.path}: declared ${label} import ${v} is not measured in the source`,
          });
        }
      }
      for (const v of measuredSet) {
        if (!declaredArr.includes(v)) {
          errors.push({
            caseId,
            code: "import-drift",
            detail: `${hotspot.path}: measured ${label} import ${v} is not declared`,
          });
        }
      }
    };
    cmp("verter", declared.verter, measured.verter);
    cmp("external", declared.external, measured.external);
    for (const root of declared.crateInternal) {
      if (!measured.internal.has(root)) {
        errors.push({
          caseId,
          code: "import-drift",
          detail: `${hotspot.path}: declared crate-internal import ${root} is not measured in the source`,
        });
      }
    }
    for (const forbidden of declared.mustNotImport) {
      if (measured.verter.has(forbidden) || measured.external.has(forbidden)) {
        errors.push({
          caseId,
          code: "forbidden-import",
          detail: `${hotspot.path}: imports forbidden root ${forbidden}`,
        });
      }
    }
    const crateDir = owningCrateDir(hotspot.path);
    const crateDeps = crateVerterDeps(crateDir);
    for (const v of measured.verter) {
      // A verter root imported by the file must be reachable through the
      // owning crate's manifest: an undeclared dependency would not compile,
      // so this catches a stale contract rather than a broken build.
      if (!crateDeps.has(v)) {
        errors.push({
          caseId,
          code: "import-drift",
          detail: `${crateDir}/Cargo.toml has no dependency for measured import ${v}`,
        });
      }
    }
    const moduleToken = path.basename(hotspot.path).replace(/\.rs$/, "");
    for (const importer of declared.importers || []) {
      if (!existsRel(importer)) {
        errors.push({ caseId, code: "missing-importer", detail: importer });
        continue;
      }
      if (!readRel(importer).includes(moduleToken)) {
        errors.push({
          caseId,
          code: "importer-without-reference",
          detail: `${importer} does not reference ${moduleToken}`,
        });
      }
    }
  }
  for (const rule of contracts.layerRules) {
    if (!existsRel(rule.crate)) {
      errors.push({ caseId, code: "layer-crate-missing", detail: rule.crate });
      continue;
    }
    const deps = crateVerterDeps(rule.crate);
    for (const dep of deps) {
      if (!rule.mayImport.includes(dep)) {
        errors.push({
          caseId,
          code: "layer-violation",
          detail: `${rule.crate} depends on ${dep}, outside the allowed import set`,
        });
      }
      if (rule.mustNotImport.includes(dep)) {
        errors.push({
          caseId,
          code: "layer-violation",
          detail: `${rule.crate} depends on forbidden root ${dep}`,
        });
      }
    }
    for (const forbidden of rule.mustNotImport) {
      if (rule.mayImport.includes(forbidden)) {
        errors.push({
          caseId,
          code: "layer-rule-contradiction",
          detail: `${rule.id}: ${forbidden} is both allowed and forbidden`,
        });
      }
    }
    resolveOwner(rule.heir, errors, caseId);
  }
}

/**
 * ARH1-AC1: constructor capabilities bind real declarations. Each declared
 * constructor exists as a visibility-carrying fn, its signature anchors are
 * present in the declaration span (so the capability talks about the real
 * parameter list, not a wish), and a hotspot without constructors records
 * why. Test-only seams must be named as such.
 */
function validateConstructors(contracts, errors) {
  const caseId = "ARH1-constructor";
  for (const hotspot of contracts.hotspots) {
    const rows = hotspot.constructorCapabilities || [];
    if (rows.length === 0) {
      if (
        typeof hotspot.constructorRationale !== "string" ||
        hotspot.constructorRationale.length === 0
      ) {
        errors.push({
          caseId,
          code: "constructor-rationale-missing",
          detail: `${hotspot.path}: no constructors declared without a rationale`,
        });
      }
      continue;
    }
    const text = readRel(hotspot.path);
    for (const row of rows) {
      const decl = new RegExp(
        `(?:pub|pub\\(crate\\)) (?:const )?fn ${row.constructor}\\s*[<(]`,
      ).exec(text);
      if (!decl) {
        errors.push({
          caseId,
          code: "missing-constructor",
          detail: `${hotspot.path}: constructor ${row.constructor} is not a declared fn`,
        });
        continue;
      }
      // The declaration span runs to the next item-level fn or 3000 chars,
      // whichever first: signature text (parameters may span lines) plus the
      // first statements, never the whole file.
      const rest = text.slice(decl.index);
      const nextItem = rest.slice(1).search(/\n {4}(?:pub |pub\(crate\) )?(?:const )?fn /);
      const span = nextItem === -1 ? rest.slice(0, 3000) : rest.slice(0, nextItem + 1);
      for (const anchor of row.signatureAnchors || []) {
        if (!span.includes(anchor)) {
          errors.push({
            caseId,
            code: "constructor-anchor-missing",
            detail: `${row.constructor}: anchor "${anchor}" not in the declaration span`,
          });
        }
      }
      if (!Array.isArray(row.rules) || row.rules.length === 0) {
        errors.push({
          caseId,
          code: "constructor-without-rules",
          detail: `${hotspot.path}: ${row.constructor}`,
        });
      }
      if (row.testOnly && !row.constructor.startsWith("test_")) {
        errors.push({
          caseId,
          code: "test-hook-unmarked",
          detail: `${row.constructor} is marked testOnly but not named as a test hook`,
        });
      }
    }
  }
}

/**
 * ARH1-AC1/AC3: every state-lifetime row names an identifier that exists in
 * the live source, a lifetime inside the contract vocabulary and one sole
 * owner, so the heirs inherit a state map, not poetry.
 */
function validateStateLifetimes(contracts, errors) {
  const caseId = "ARH1-state-lifetimes";
  for (const hotspot of contracts.hotspots) {
    const text = readRel(hotspot.path);
    for (const row of hotspot.stateLifetimes) {
      if (!LIFETIMES.has(row.lifetime)) {
        errors.push({
          caseId,
          code: "bad-lifetime",
          detail: `${hotspot.path}: ${row.state} lifetime ${row.lifetime}`,
        });
      }
      if (typeof row.soleOwner !== "string" || row.soleOwner.length === 0) {
        errors.push({
          caseId,
          code: "state-without-owner",
          detail: `${hotspot.path}: ${row.state}`,
        });
      } else if (row.soleOwner.endsWith(".rs") && !existsRel(row.soleOwner)) {
        errors.push({
          caseId,
          code: "state-owner-missing",
          detail: `${row.state}: sole owner ${row.soleOwner}`,
        });
      }
      const tokens = row.state
        .split(/[^A-Za-z_0-9]+/)
        .filter((t) => t.length >= 4 && !STATE_TOKEN_STOPWORDS.has(t.toLowerCase()));
      if (tokens.length === 0 || !tokens.some((t) => text.includes(t))) {
        errors.push({
          caseId,
          code: "state-not-in-source",
          detail: `${hotspot.path}: no identifier of "${row.state}" appears in the source`,
        });
      }
    }
  }
}

/**
 * ARH1-AC1: minimal public surfaces bind the live tree. Module visibility
 * declarations are pinned verbatim at their declaration site, retained
 * items are real declarations, cross-crate retained types are actually
 * imported by a declared consumer, and narrowing rows target a legal
 * visibility with real consumers.
 */
function validateSurface(contracts, errors) {
  const caseId = "ARH1-surface";
  for (const hotspot of contracts.hotspots) {
    const text = readRel(hotspot.path);
    const crossCrate = hotspot.surfaceDeclarations.some((d) => /^pub mod /.test(d.declaration));
    // The consumed-by-consumer proof runs only where the importers list is
    // the complete cross-crate population for the retained surface.
    const verifyConsumed =
      hotspot.minimalPublicSurface.crossCrateRetainedVerifiedByConsumers === true;
    for (const decl of hotspot.surfaceDeclarations) {
      if (!existsRel(decl.file)) {
        errors.push({ caseId, code: "declaration-file-missing", detail: decl.file });
        continue;
      }
      if (!readRel(decl.file).includes(decl.declaration)) {
        errors.push({
          caseId,
          code: "surface-declaration-drift",
          detail: `${decl.file} no longer carries "${decl.declaration}"`,
        });
      }
    }
    for (const fn of hotspot.minimalPublicSurface.retainedFns || []) {
      if (!new RegExp(`(?:pub|pub\\(crate\\)) (?:const )?fn ${fn}\\s*[<(]`).test(text)) {
        errors.push({ caseId, code: "surface-item-missing", detail: `${hotspot.path}: fn ${fn}` });
      }
    }
    for (const type of hotspot.minimalPublicSurface.retainedTypes || []) {
      if (
        !new RegExp(
          `(?:pub|pub\\(crate\\)) (?:struct|enum|trait|type|const|static) ${type}\\b`,
        ).test(text)
      ) {
        errors.push({
          caseId,
          code: "surface-item-missing",
          detail: `${hotspot.path}: type ${type}`,
        });
      }
    }
    if (
      crossCrate &&
      verifyConsumed &&
      (hotspot.minimalPublicSurface.retainedTypes || []).length > 0
    ) {
      const importers = hotspot.allowedImportDirection.importers || [];
      for (const type of hotspot.minimalPublicSurface.retainedTypes) {
        const consumed = importers.some((imp) => existsRel(imp) && readRel(imp).includes(type));
        if (!consumed) {
          errors.push({
            caseId,
            code: "retained-item-unconsumed",
            detail: `${hotspot.path}: retained type ${type} is imported by no declared consumer`,
          });
        }
      }
    }
    for (const row of hotspot.minimalPublicSurface.narrow || []) {
      if (!NARROW_TARGETS.has(row.to)) {
        errors.push({
          caseId,
          code: "bad-narrow-target",
          detail: `${hotspot.path}: ${row.item} -> ${row.to}`,
        });
      }
      if (row.kind === "fn" && !new RegExp(`pub (?:const )?fn ${row.item}\\s*[<(]`).test(text)) {
        errors.push({
          caseId,
          code: "surface-item-missing",
          detail: `${hotspot.path}: fn ${row.item}`,
        });
      }
      if (row.kind === "field" && !new RegExp(`pub ${row.item}\\s*:`).test(text)) {
        errors.push({
          caseId,
          code: "surface-item-missing",
          detail: `${hotspot.path}: field ${row.item}`,
        });
      }
      for (const consumer of row.consumersAffected || []) {
        if (!existsRel(consumer)) {
          errors.push({
            caseId,
            code: "missing-narrow-consumer",
            detail: `${row.item}: ${consumer}`,
          });
        }
      }
    }
  }
}

/**
 * ARH1-AC2 discriminator: moving methods into files while retaining
 * unrestricted shared state does not satisfy a responsibility boundary. A
 * cohesive split must declare per-module state ownership inside the
 * contract's vocabulary, a state may be claimed sole by at most one module,
 * and "shared"/"inherited" ownership modes are rejected outright.
 */
function validateSplit(contracts, arh0, errors) {
  const caseId = "ARH1-split";
  for (const hotspot of contracts.hotspots) {
    const god = arh0["responsibility-map"].godModuleCandidates.find((r) => r.path === hotspot.path);
    const inventoried = new Set(god ? god.responsibilities : []);
    const declaredStates = new Set(hotspot.stateLifetimes.map((s) => s.state));
    const soleClaims = new Map();
    const modules = hotspot.cohesiveModules || [];
    if (modules.length === 0) {
      const rationale = hotspot.minimalPublicSurface?.rationale ?? hotspot.singleModuleRationale;
      if (typeof rationale !== "string" || rationale.length === 0) {
        errors.push({
          caseId,
          code: "split-rationale-missing",
          detail: `${hotspot.path}: no cohesive modules and no single-module rationale`,
        });
      }
    }
    for (const mod of modules) {
      if (!existsRel(mod.module)) {
        errors.push({ caseId, code: "missing-cohesive-module", detail: mod.module });
        continue;
      }
      if (mod.responsibility != null && !inventoried.has(mod.responsibility)) {
        errors.push({
          caseId,
          code: "cohesive-responsibility-invented",
          detail: `${mod.module}: ${mod.responsibility}`,
        });
      }
      for (const claim of mod.stateOwnership || []) {
        if (!OWNERSHIP_MODES.has(claim.mode)) {
          errors.push({
            caseId,
            code: "split-retains-shared-state",
            detail: `${mod.module} claims "${claim.state}" with mode "${claim.mode}": shared unrestricted state does not satisfy a responsibility boundary`,
          });
          continue;
        }
        if (!declaredStates.has(claim.state)) {
          errors.push({
            caseId,
            code: "state-ownership-undeclared",
            detail: `${mod.module} claims undeclared state "${claim.state}"`,
          });
          continue;
        }
        if (claim.mode === "sole") {
          if (soleClaims.has(claim.state)) {
            errors.push({
              caseId,
              code: "state-double-owner",
              detail: `${claim.state} claimed sole by both ${soleClaims.get(claim.state)} and ${mod.module}`,
            });
          } else {
            soleClaims.set(claim.state, mod.module);
          }
          const stateRow = hotspot.stateLifetimes.find((s) => s.state === claim.state);
          const ownerBase = path.basename(stateRow.soleOwner || "");
          if (ownerBase && path.basename(mod.module) !== ownerBase) {
            errors.push({
              caseId,
              code: "state-ownership-undeclared",
              detail: `${mod.module} claims "${claim.state}" sole but the state row names ${stateRow.soleOwner}`,
            });
          }
        }
      }
    }
  }
}

/**
 * ARH1-AC1: every route has exactly one surviving owner and a concrete
 * disposition. This includes the decisions ARH0 explicitly assigned to this
 * node (its debt rows name ARH1 as decision owner) and every narrowing
 * route the contracts declare for the heirs.
 */
function validateCutover(contracts, cutover, arh0, errors) {
  const caseId = "ARH1-cutover";
  if (
    typeof cutover.emptyDeletionSetRationale !== "string" ||
    cutover.emptyDeletionSetRationale.length === 0
  ) {
    errors.push({
      caseId,
      code: "deletion-rationale-missing",
      detail: "empty deletion set without rationale",
    });
  }
  const ids = new Set();
  for (const row of cutover.rows) {
    if (ids.has(row.id)) errors.push({ caseId, code: "duplicate-id", detail: row.id });
    ids.add(row.id);
    if (!bindsRepoPath(row.candidatePath, row.provenance).ok) {
      errors.push({ caseId, code: "missing-candidate-path", detail: row.id });
    }
    if (typeof row.decision !== "string" || row.decision.length === 0) {
      errors.push({ caseId, code: "cutover-without-decision", detail: row.id });
    }
    if (typeof row.disposition !== "string" || row.disposition.length === 0) {
      errors.push({ caseId, code: "cutover-without-disposition", detail: row.id });
    }
    resolveOwner(row.owner, errors, caseId, "malformed-disposition-owner");
  }
  // ARH0 debt rows that name ARH1 as decision owner must be decided here.
  const satisfied = new Set(cutover.rows.map((r) => r.satisfies).filter(Boolean));
  for (const debt of arh0["debt-register"].rows) {
    if (debt.owner?.kind === "node" && debt.owner?.id === "ARH1" && !satisfied.has(debt.id)) {
      errors.push({
        caseId,
        code: "arh0-debt-undecided",
        detail: `${debt.id} names ARH1 as decision owner but no cutover row satisfies it`,
      });
    }
  }
  // Every narrowing route declared in the contracts has a register row with
  // the same candidate and executing heir.
  for (const hotspot of contracts.hotspots) {
    const narrow = hotspot.minimalPublicSurface.narrow || [];
    if (narrow.length === 0) continue;
    const heir = hotspot.heirs?.narrowing;
    const covered = cutover.rows.some(
      (r) =>
        r.candidatePath === hotspot.path &&
        heir &&
        r.owner.id === heir.id &&
        r.owner.kind === "node",
    );
    if (!covered) {
      errors.push({
        caseId,
        code: "cutover-route-missing",
        detail: `${hotspot.path} declares narrowing rows but no register row binds the route to heir ${heir ? heir.id : "(none)"}`,
      });
    }
  }
}

/**
 * ARH1-AC1/AC4: the manifest documents this node's case contract, products,
 * commands and producer obligations; the verifier is the sole owning
 * interface that runs them, so prose cannot outlive the checks.
 */
function validateRatification(products, manifest, errors) {
  const caseId = "ARH1-ratification";
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

  const toRepoPosix = (abs) => path.relative(REPO_ROOT, abs).split(path.sep).join("/");
  const canonical = [
    ["verify", manifest.verify, `node ${toRepoPosix(fileURLToPath(import.meta.url))}`],
    ["test", manifest.test, `node --test ${toRepoPosix(path.join(HERE, "arh1.test.mjs"))}`],
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

  for (const obligation of products["dependency-contracts"].ac4Obligations || []) {
    resolveOwner(obligation.producer, errors, caseId, "obligation-producer-malformed");
    if (typeof obligation.obligation !== "string" || obligation.obligation.length === 0) {
      errors.push({
        caseId,
        code: "obligation-without-text",
        detail: `producer ${obligation.producer?.id ?? "(no id)"} records no obligation`,
      });
    }
  }
}

export function mandatoryCases() {
  return [
    "ARH1-hotspot-coverage",
    "ARH1-import-direction",
    "ARH1-constructor",
    "ARH1-state-lifetimes",
    "ARH1-surface",
    "ARH1-split",
    "ARH1-cutover",
    "ARH1-ratification",
  ];
}

/** Case ids with at least one recorded error (dirty-twin selection helper). */
export function selectedCaseIds(result) {
  return [...new Set(result.errors.map((e) => e.caseId))];
}

// The manifest documents `node tests/architecture-health/ARH1/verify.mjs` as
// this node's verify command; it must validate the real products, not no-op.
const isMain =
  process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url));
if (isMain) {
  const result = validate(loadProducts());
  if (!result.ok) {
    console.error(result.errors.map((e) => `${e.caseId}/${e.code}: ${e.detail}`).join("\n"));
    process.exit(1);
  }
  console.log(`ARH1 verify: PASS cases=${mandatoryCases().join(",")}`);
}
