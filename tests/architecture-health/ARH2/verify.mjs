#!/usr/bin/env node
/**
 * ARH2 measurement verifier — the sole owning interface of the behavioral
 * characterization and complexity measurements.
 *
 * Validates the two products against the shipped ARH0/ARH1 predecessor
 * products and the working tree, and re-derives the predecessor populations
 * live by running the shipped ARH0 and ARH1 validate() implementations on
 * every execution (imported, never re-implemented, so a drifted population
 * fails this node together with the drifted predecessor). The hotspot
 * population is exactly the ARH1 hotspot contracts, which is exactly the
 * ARH0 god-module candidate population; every ARH1 authority responsibility
 * is carried by exactly one characterization row. Every behavioral pin is a
 * real cargo nextest lane: the command is canonical (`cargo nextest run`),
 * the crate is a live workspace member, the filter selects the recorded
 * witnesses under nextest substring semantics over the compiled module path
 * (`mod` / `#[path]`, never a filesystem `src::` fragment), and every
 * witness is a real #[test] function in a real file. Every cutover route a successor
 * narrows (ARH1 register rows owned by ARH3/ARH4, plus the ARH2-executed
 * deletion) is characterized exactly once, with the narrowed surface
 * derived live (the scheduler pub bookkeeping fields, the pub fn test_*
 * hook population joined to the ARH1 test-configuration rows, and the
 * semantic_query bulk route) so the pins describe the surface as it exists
 * BEFORE narrowing. The executed packages/core deletion binds its ARH0 debt
 * key, its ARH1 cutover row and the same-change ARH0 product refresh, and
 * the deleted path is absent from the tree and from every product. The
 * complexity product separates the four charter measurement dimensions
 * exactly once each, binds production behavior and test cost to the pinned
 * nextest lanes, clean/warm build time to cargo --timings recipes with both
 * cache states, and application latency to a host/session gate cell,
 * commits only deterministically re-derivable structural counts
 * (re-derived here on every run), and ratifies the god-module threshold
 * basis ARH0-DEBT-5 requires before ARH12 may extend the existing guard.
 * No wall-clock, RSS or speedup number may be committed in any dimension.
 * ARH2-ratification fails when the manifest and the verifier disagree about
 * the case contract, so the manifest cannot claim checks that do not run.
 * `verify.mjs --provenance` additionally proves the pinned candidate commit
 * is a real ancestor of HEAD.
 */

import fs from "node:fs";
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { loadProducts as loadArh0Products, validate as validateArh0 } from "../ARH0/verify.mjs";
import { loadProducts as loadArh1Products, validate as validateArh1 } from "../ARH1/verify.mjs";
import { readGatesToml } from "../../../scripts/validate-performance-gates.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = path.resolve(HERE, "../../..");

export const PRODUCT_FILES = Object.freeze([
  "characterization.json",
  "complexity-measurements.json",
]);

const DIMENSION_IDS = Object.freeze([
  "production-behavior",
  "clean-warm-build-time",
  "test-cost",
  "application-latency",
]);
const DIMENSION_MECHANISM_KIND = Object.freeze({
  "production-behavior": "cargo-nextest",
  "clean-warm-build-time": "cargo-build",
  "test-cost": "cargo-nextest",
  "application-latency": "gate-cell",
});
const CARGO_BUILD_CACHE_STATES = Object.freeze(["clean", "warm"]);
// Charter AC3 concerns, verbatim; each must carry existing named evidence.
const AC3_CONCERNS = Object.freeze([
  "fresh-versus-incremental equivalence",
  "edit/revert",
  "cancellation",
  "stale/partial rejection",
  "deterministic ordering under perturbed discovery or scheduling",
]);
// A dimension row is a separation declaration, never a measurement record.
const FORBIDDEN_NUMERIC_KEYS = Object.freeze([
  "wallNs",
  "wallMs",
  "durationMs",
  "duration",
  "seconds",
  "ms",
  "ns",
  "peakRssBytes",
  "rssBytes",
  "speedup",
]);
// Narrowing successors whose cutover routes ARH2 must characterize first.
const NARROWING_HEIRS = Object.freeze(["ARH3", "ARH4"]);

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
  if (typeof rel !== "string" || rel.length === 0) return false;
  const resolved = path.resolve(REPO_ROOT, rel);
  const rootPrefix = REPO_ROOT.endsWith(path.sep) ? REPO_ROOT : REPO_ROOT + path.sep;
  if (resolved !== REPO_ROOT && !resolved.startsWith(rootPrefix)) return false;
  return fs.existsSync(resolved);
}

function readRel(rel) {
  return fs.readFileSync(path.join(REPO_ROOT, rel), "utf8");
}

/** Rust `str::lines()` semantics: LF-split, trailing newline adds no line. */
function rustLineCount(text) {
  return text.split("\n").length - (text.endsWith("\n") ? 1 : 0);
}

/**
 * Workspace member crate names: literal entries of the root Cargo.toml
 * members list plus glob entries expanded against directories that carry a
 * Cargo.toml (this workspace declares `crates/*`, not literal crates).
 */
function workspaceCrates() {
  const text = readRel("Cargo.toml");
  const membersStart = text.indexOf("members = [");
  if (membersStart === -1) return new Set();
  const membersEnd = text.indexOf("]", membersStart);
  const entries = text
    .slice(membersStart, membersEnd)
    .match(/"([^"]+)"/g)
    .map((quoted) => quoted.slice(1, -1));
  const crates = new Set();
  for (const entry of entries) {
    if (!entry.includes("*")) {
      crates.add(entry.split("/").pop());
      continue;
    }
    const [dir] = entry.split("*");
    const root = path.join(REPO_ROOT, dir);
    if (!fs.existsSync(root)) continue;
    for (const child of fs.readdirSync(root, { withFileTypes: true })) {
      if (child.isDirectory() && fs.existsSync(path.join(root, child.name, "Cargo.toml"))) {
        crates.add(child.name);
      }
    }
  }
  return crates;
}

function posixRel(rel) {
  return rel.split(path.sep).join("/");
}

function posixDir(rel) {
  const i = rel.lastIndexOf("/");
  return i === -1 ? "" : rel.slice(0, i);
}

function posixJoin(dir, child) {
  if (!dir) return child.replace(/\\/g, "/");
  return `${dir}/${child.replace(/\\/g, "/")}`;
}

function sameSet(a, b) {
  if (!Array.isArray(a) || !Array.isArray(b) || a.length !== b.length) return false;
  return [...new Set(a)].sort().join("\n") === [...new Set(b)].sort().join("\n");
}

function stripComments(text) {
  return text.replace(/\/\/[^\n]*/g, "").replace(/\/\*[\s\S]*?\*\//g, "");
}

/**
 * `mod` declarations of a Rust file, including `#[path]` retargets and
 * inline `mod name { ... }` blocks. Used to reconstruct the compiled
 * nextest module path; a filesystem path under `src/` is not a module.
 */
function parseModDecls(text) {
  const stripped = stripComments(text);
  const decls = [];
  const re =
    /((?:#\[[^\]]*\]\s*)*)(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*([;{])/g;
  let match;
  while ((match = re.exec(stripped))) {
    const attrs = match[1] || "";
    const pathAttr = attrs.match(/#\[path\s*=\s*"([^"]+)"\]/);
    decls.push({
      name: match[2],
      inline: match[3] === "{",
      pathAttr: pathAttr ? pathAttr[1] : null,
    });
  }
  return decls;
}

function resolveModFile(parentFile, decl) {
  const dir = posixDir(parentFile);
  if (decl.pathAttr) return posixJoin(dir, decl.pathAttr);
  const base = parentFile.slice(parentFile.lastIndexOf("/") + 1);
  const candidates =
    base === "lib.rs" || base === "main.rs" || base === "mod.rs"
      ? [posixJoin(dir, `${decl.name}.rs`), posixJoin(dir, `${decl.name}/mod.rs`)]
      : [
          posixJoin(dir, `${base.replace(/\.rs$/, "")}/${decl.name}.rs`),
          posixJoin(dir, `${base.replace(/\.rs$/, "")}/${decl.name}/mod.rs`),
        ];
  return candidates.find((candidate) => existsRel(candidate)) || null;
}

const MODULE_INDEX_CACHE = new Map();

function indexCrateModules(crate) {
  const cached = MODULE_INDEX_CACHE.get(crate);
  if (cached) return cached;
  const map = new Map();
  const seen = new Set();
  const walk = (file, modulePath) => {
    const key = `${file}|${modulePath}`;
    if (seen.has(key) || !existsRel(file)) return;
    seen.add(key);
    const paths = map.get(file) || [];
    paths.push(modulePath);
    map.set(file, paths);
    for (const decl of parseModDecls(readRel(file))) {
      const childPath = modulePath ? `${modulePath}::${decl.name}` : decl.name;
      if (decl.inline) {
        paths.push(childPath);
        continue;
      }
      const childFile = resolveModFile(file, decl);
      if (childFile) walk(childFile, childPath);
    }
  };
  const crateRel = `crates/${crate}`;
  for (const root of [
    `${crateRel}/src/lib.rs`,
    `${crateRel}/src/main.rs`,
    `${crateRel}/tests/main.rs`,
  ]) {
    walk(root, "");
  }
  MODULE_INDEX_CACHE.set(crate, map);
  return map;
}

/**
 * Compiled nextest module paths for a witness file, derived from the crate's
 * `mod` / `#[path]` tree. Filesystem `src::…` fragments are not IDs.
 */
function compiledModulePaths(crate, file) {
  const rel = posixRel(file);
  const index = indexCrateModules(crate);
  const paths = index.get(rel);
  if (!paths || paths.length === 0) return [];
  return [...new Set(paths.filter((p) => p.length > 0))];
}

/**
 * A witness is real when its file exists, the named function is declared
 * there, and a #[test] / #[tokio::test] attribute sits within the few lines
 * above the declaration (cfg attributes and doc comments may intervene).
 */
function checkWitness(witness, errors, caseId) {
  if (!existsRel(witness.file)) {
    errors.push({ caseId, code: "witness-file-missing", detail: witness.file });
    return false;
  }
  const text = readRel(witness.file);
  const fnMatch = text.match(new RegExp(`fn\\s+${witness.test}\\s*\\(`));
  if (!fnMatch) {
    errors.push({
      caseId,
      code: "witness-test-missing",
      detail: `${witness.test} is not declared in ${witness.file}`,
    });
    return false;
  }
  const before = text.slice(0, fnMatch.index).split("\n");
  const window = before.slice(-4).join("\n");
  if (!/#\[(?:test|tokio::test)/.test(window)) {
    errors.push({
      caseId,
      code: "witness-not-a-test",
      detail: `${witness.test} in ${witness.file} has no #[test] attribute`,
    });
    return false;
  }
  return true;
}

function checkPin(pin, crate, errors, caseId) {
  const expected = `cargo nextest run -p ${crate} ${pin.filter}`;
  if (pin.command !== expected) {
    errors.push({
      caseId,
      code: "pin-command-not-canonical",
      detail: `${pin.command} is not the canonical ${expected}`,
    });
  }
  if (typeof pin.filter !== "string" || pin.filter.length === 0 || pin.filter.startsWith("-")) {
    errors.push({ caseId, code: "pin-filter-malformed", detail: pin.command });
    return;
  }
  if (!Array.isArray(pin.witnesses) || pin.witnesses.length === 0) {
    errors.push({ caseId, code: "pin-without-witnesses", detail: pin.command });
    return;
  }
  for (const witness of pin.witnesses) {
    if (!checkWitness(witness, errors, caseId)) continue;
    const modules = compiledModulePaths(crate, witness.file);
    if (modules.length === 0) {
      errors.push({
        caseId,
        code: "pin-filter-selects-nothing",
        detail: `${witness.file} is not a compiled module of ${crate}; filesystem paths are not nextest ids`,
      });
      continue;
    }
    const ids = modules.map((mod) => `${mod}::${witness.test}`);
    if (!ids.some((id) => id.includes(pin.filter))) {
      errors.push({
        caseId,
        code: "pin-filter-selects-nothing",
        detail: `filter ${pin.filter} is not a substring of the compiled nextest id ${ids[0]}`,
      });
    }
  }
}

function collectPinCommands(characterization) {
  const commands = new Set();
  for (const hotspot of characterization.hotspots || []) {
    for (const pin of hotspot.pins || []) commands.add(pin.command);
  }
  for (const route of characterization.routes || []) {
    for (const pin of route.pins || []) commands.add(pin.command);
  }
  return commands;
}

function validatePopulation(products, predecessors, errors) {
  const caseId = "ARH2-population";
  const characterization = products["characterization"];
  const measurements = products["complexity-measurements"];
  const arh0 = predecessors.arh0;
  const arh1 = predecessors.arh1;

  // Live re-derivation: the shipped predecessor verifiers must pass against
  // the current tree, or this node fails with them.
  const arh0Result = validateArh0(arh0);
  if (!arh0Result.ok) {
    errors.push({
      caseId,
      code: "predecessor-drift",
      detail: `ARH0 populations drifted: ${arh0Result.errors[0]?.caseId}/${arh0Result.errors[0]?.code}`,
    });
  }
  const arh1Result = validateArh1(arh1);
  if (!arh1Result.ok) {
    errors.push({
      caseId,
      code: "predecessor-drift",
      detail: `ARH1 populations drifted: ${arh1Result.errors[0]?.caseId}/${arh1Result.errors[0]?.code}`,
    });
  }

  // Hotspot population: exactly the ARH1 contracts, exactly the ARH0
  // god-module candidates, no dropped or invented hotspot.
  const arh1Paths = arh1["dependency-contracts"].hotspots.map((h) => h.path);
  const arh0Gods = arh0["responsibility-map"].godModuleCandidates
    .filter((r) => r.classification === "god-module-candidate")
    .map((r) => r.path);
  const ownPaths = (characterization.hotspots || []).map((h) => h.path);
  const sameSet = (a, b) =>
    a.length === b.length &&
    [...new Set(a)].sort().join("\n") === [...new Set(b)].sort().join("\n");
  if (!sameSet(ownPaths, arh1Paths)) {
    errors.push({
      caseId,
      code: "hotspot-population-drift",
      detail: "characterization hotspots are not exactly the ARH1 hotspot contracts",
    });
  }
  if (!sameSet(arh1Paths, arh0Gods)) {
    errors.push({
      caseId,
      code: "hotspot-population-drift",
      detail:
        "ARH1 hotspot contracts are not exactly the ARH0 god-module candidates (predecessor drift)",
    });
  }

  // Responsibilities join 1:1 with the ARH1 authority rows, both directions.
  for (const hotspot of characterization.hotspots || []) {
    const contract = arh1["dependency-contracts"].hotspots.find((h) => h.path === hotspot.path);
    if (!contract) continue;
    const own = new Set(
      (hotspot.responsibilities || []).map((r) => `${r.responsibility}|${r.survivingOwner}`),
    );
    const declared = new Set(
      contract.authority.map((a) => `${a.responsibility}|${a.survivingOwner}`),
    );
    for (const key of declared) {
      if (!own.has(key)) {
        errors.push({
          caseId,
          code: "responsibility-dropped",
          detail: `${hotspot.path}: ${key.split("|")[0]} has no characterization row`,
        });
      }
    }
    for (const key of own) {
      if (!declared.has(key)) {
        errors.push({
          caseId,
          code: "responsibility-invented",
          detail: `${hotspot.path}: ${key.split("|")[0]} is not an ARH1 authority responsibility`,
        });
      }
    }
  }

  // Re-derived structural populations.
  const inv = arh0["codebase-inventory"];
  const declaredPops = measurements.populations?.arh0Inventory;
  if (declaredPops?.crates !== inv.crates.length) {
    errors.push({
      caseId,
      code: "population-count-drift",
      detail: `arh0Inventory.crates=${declaredPops?.crates} but the live inventory carries ${inv.crates.length} rows`,
    });
  }
  if (declaredPops?.packages !== inv.packages.length) {
    errors.push({
      caseId,
      code: "population-count-drift",
      detail: `arh0Inventory.packages=${declaredPops?.packages} but the live inventory carries ${inv.packages.length} rows`,
    });
  }
  if (measurements.populations?.arh1HotspotContracts !== arh1Paths.length) {
    errors.push({
      caseId,
      code: "population-count-drift",
      detail: `arh1HotspotContracts=${measurements.populations?.arh1HotspotContracts} but ARH1 carries ${arh1Paths.length}`,
    });
  }
  if (measurements.populations?.arh0GodModuleCandidates !== arh0Gods.length) {
    errors.push({
      caseId,
      code: "population-count-drift",
      detail: `arh0GodModuleCandidates=${measurements.populations?.arh0GodModuleCandidates} but ARH0 carries ${arh0Gods.length}`,
    });
  }

  // Per-hotspot structural complexity, re-derived live.
  const structural = new Map((measurements.structural || []).map((r) => [r.path, r.fileLoc]));
  if (!sameSet([...structural.keys()], ownPaths)) {
    errors.push({
      caseId,
      code: "structural-population-drift",
      detail: "structural rows are not exactly the characterized hotspots",
    });
  }
  for (const [rel, recorded] of structural) {
    if (!existsRel(rel)) {
      errors.push({ caseId, code: "structural-file-missing", detail: rel });
      continue;
    }
    const live = rustLineCount(readRel(rel));
    if (live !== recorded) {
      errors.push({
        caseId,
        code: "structural-loc-drift",
        detail: `${rel}: recorded ${recorded} fileLoc, live ${live}`,
      });
    }
  }
}

function validateDeletion(products, predecessors, errors) {
  const caseId = "ARH2-deletion";
  const deletion = products["characterization"].deletion;
  if (!deletion || typeof deletion !== "object") {
    errors.push({ caseId, code: "deletion-block-missing", detail: "no deletion record" });
    return;
  }
  if (existsRel(deletion.deletedPath)) {
    errors.push({
      caseId,
      code: "deletion-not-executed",
      detail: `${deletion.deletedPath} still exists in the tree`,
    });
  }
  // The deleted path is gone from every product's path-typed fields.
  const arh0 = predecessors.arh0;
  const arh1 = predecessors.arh1;
  for (const row of arh0["codebase-inventory"].packages) {
    if (row.module === deletion.deletedPath) {
      errors.push({
        caseId,
        code: "deletion-inventory-row-stale",
        detail: `${deletion.deletedPath} still carries an ARH0 inventory row`,
      });
    }
  }
  for (const row of arh0["debt-register"].rows) {
    if (row.candidatePath === deletion.deletedPath) {
      errors.push({
        caseId,
        code: "deletion-debt-row-stale",
        detail: `${row.id} still binds the deleted path as its candidate`,
      });
    }
  }
  for (const row of arh1["cutover-register"].rows) {
    if (row.candidatePath === deletion.deletedPath) {
      errors.push({
        caseId,
        code: "deletion-cutover-row-stale",
        detail: `${row.id} still binds the deleted path as its candidate`,
      });
    }
  }
  // The deletion binds its ARH0 debt key and its ARH1 cutover row, and the
  // cutover row's executing owner is ARH2.
  const debt = arh0["debt-register"].rows.find((r) => r.id === deletion.satisfies);
  if (!debt) {
    errors.push({
      caseId,
      code: "deletion-debt-unknown",
      detail: `${deletion.satisfies} is not a shipped ARH0 debt row`,
    });
  }
  const cutover = arh1["cutover-register"].rows.find((r) => r.id === deletion.cutoverRow);
  if (!cutover) {
    errors.push({
      caseId,
      code: "deletion-cutover-unknown",
      detail: `${deletion.cutoverRow} is not a shipped ARH1 cutover row`,
    });
  } else {
    if (cutover.satisfies !== deletion.satisfies) {
      errors.push({
        caseId,
        code: "deletion-key-mismatch",
        detail: `${cutover.id} satisfies ${cutover.satisfies}, not ${deletion.satisfies}`,
      });
    }
    if (cutover.owner?.id !== "ARH2" || cutover.owner?.kind !== "node") {
      errors.push({
        caseId,
        code: "deletion-owner-mismatch",
        detail: `${cutover.id} executing owner is not ARH2`,
      });
    }
  }
  for (const refreshed of deletion.sameChangeRefresh || []) {
    if (!existsRel(refreshed)) {
      errors.push({ caseId, code: "deletion-refresh-path-missing", detail: refreshed });
    }
  }
  if (typeof deletion.rationale !== "string" || deletion.rationale.length === 0) {
    errors.push({ caseId, code: "deletion-rationale-missing", detail: deletion.deletedPath });
  }
}

function validateCharacterization(products, predecessors, errors) {
  const caseId = "ARH2-characterization";
  const characterization = products["characterization"];
  const arh1 = predecessors.arh1;
  const crates = workspaceCrates();

  for (const hotspot of characterization.hotspots || []) {
    if (!existsRel(hotspot.path)) {
      errors.push({ caseId, code: "hotspot-missing", detail: hotspot.path });
      continue;
    }
    if (!crates.has(hotspot.crate)) {
      errors.push({
        caseId,
        code: "hotspot-crate-unknown",
        detail: `${hotspot.crate} is not a workspace member crate`,
      });
    }
    if (!Array.isArray(hotspot.pins) || hotspot.pins.length === 0) {
      errors.push({ caseId, code: "hotspot-without-pins", detail: hotspot.path });
      continue;
    }
    for (const pin of hotspot.pins) checkPin(pin, hotspot.crate, errors, caseId);
  }

  // Every narrowing cutover route (ARH3/ARH4 heirs) plus this node's own
  // execution row is characterized exactly once; no invented route.
  const register = arh1["cutover-register"].rows;
  const requiredRoutes = register
    .filter((r) => r.owner?.kind === "node" && NARROWING_HEIRS.includes(r.owner?.id))
    .map((r) => r.id);
  const ownRoutes = (characterization.routes || []).map((r) => r.cutoverRow);
  for (const id of requiredRoutes) {
    if (ownRoutes.filter((x) => x === id).length !== 1) {
      errors.push({
        caseId,
        code: "route-characterization-cardinality",
        detail: `${id} must be characterized exactly once before its heir narrows it`,
      });
    }
  }
  for (const id of new Set(ownRoutes)) {
    if (!requiredRoutes.includes(id)) {
      errors.push({
        caseId,
        code: "route-characterization-invented",
        detail: `${id} is not a narrowing cutover row of a characterized heir`,
      });
    }
  }
  for (const route of characterization.routes || []) {
    const row = register.find((r) => r.id === route.cutoverRow);
    if (!row) continue;
    if (route.path !== row.candidatePath) {
      errors.push({
        caseId,
        code: "route-path-mismatch",
        detail: `${route.cutoverRow} narrows ${row.candidatePath}, not ${route.path}`,
      });
    }
    if (route.narrowingOwner?.id !== row.owner?.id) {
      errors.push({
        caseId,
        code: "route-owner-mismatch",
        detail: `${route.cutoverRow} heir is ${row.owner?.id}, not ${route.narrowingOwner?.id}`,
      });
    }
    if (!Array.isArray(route.pins) || route.pins.length === 0) {
      errors.push({ caseId, code: "route-without-pins", detail: route.cutoverRow });
    } else {
      for (const pin of route.pins) {
        const crate = route.path.includes("verter_scheduler")
          ? "verter_scheduler"
          : "verter_session";
        checkPin(pin, crate, errors, caseId);
      }
    }
  }

  // The narrowed surfaces are characterized as they exist BEFORE narrowing:
  // derived live and joined to the ARH1 narrowing populations.
  const schedulerRel = "crates/verter_scheduler/src/scheduler.rs";
  const schedulerText = existsRel(schedulerRel) ? readRel(schedulerRel) : "";
  const fieldsRoute = (characterization.routes || []).find((r) => r.cutoverRow === "ARH1-CUT-2");
  if (fieldsRoute) {
    const contract = arh1["dependency-contracts"].hotspots.find((h) => h.path === schedulerRel);
    const declaredFields = (contract?.minimalPublicSurface?.narrow || [])
      .filter((r) => r.kind === "field")
      .map((r) => r.item);
    const items = fieldsRoute.surface?.items || [];
    if (items.length !== declaredFields.length || !items.every((i) => declaredFields.includes(i))) {
      errors.push({
        caseId,
        code: "route-surface-drift",
        detail: "ARH1-CUT-2 items are not exactly the ARH1 field narrowing population",
      });
    }
    for (const item of items) {
      if (!new RegExp(`pub\\s+${item}\\s*:`).test(schedulerText)) {
        errors.push({
          caseId,
          code: "route-surface-not-live",
          detail: `${item} is not a live pub field of ${schedulerRel}; the surface must be characterized before narrowing`,
        });
      }
    }
  }
  const hooksRoute = (characterization.routes || []).find((r) => r.cutoverRow === "ARH1-CUT-3");
  if (hooksRoute) {
    const contract = arh1["dependency-contracts"].hotspots.find((h) => h.path === schedulerRel);
    const declaredHooks = (contract?.minimalPublicSurface?.narrow || [])
      .filter((r) => r.to === "test-configuration")
      .map((r) => r.item);
    const liveHooks = (schedulerText.match(/pub fn test_\w+/g) || []).map((s) =>
      s.replace("pub fn ", ""),
    );
    if (declaredHooks.length !== liveHooks.length) {
      errors.push({
        caseId,
        code: "route-surface-drift",
        detail: `ARH1 declares ${declaredHooks.length} test-configuration hooks but the live scheduler carries ${liveHooks.length} pub fn test_* hooks`,
      });
    }
  }
  const bulkRoute = (characterization.routes || []).find((r) => r.cutoverRow === "ARH1-CUT-4");
  if (bulkRoute) {
    const queryRel = "crates/verter_session/src/semantic_query.rs";
    const contract = arh1["dependency-contracts"].hotspots.find((h) => h.path === queryRel);
    const declared = contract?.minimalPublicSurface || {};
    const bulkRow = (declared.narrow || []).find((r) => r.kind === "bulk");
    const surface = bulkRoute.surface || {};
    if (surface.kind !== "bulk") {
      errors.push({
        caseId,
        code: "route-surface-drift",
        detail: "ARH1-CUT-4 surface kind is not the ARH1 bulk narrowing",
      });
    }
    if (!sameSet(surface.retainedTypes, declared.retainedTypes || [])) {
      errors.push({
        caseId,
        code: "route-surface-drift",
        detail: "ARH1-CUT-4 retainedTypes are not exactly the ARH1 retained envelope types",
      });
    }
    if (!sameSet(surface.retainedAssocItems, declared.retainedAssocItems || [])) {
      errors.push({
        caseId,
        code: "route-surface-drift",
        detail: "ARH1-CUT-4 retainedAssocItems are not exactly the ARH1 retained assoc items",
      });
    }
    if (!sameSet(surface.consumersAffected, bulkRow?.consumersAffected || [])) {
      errors.push({
        caseId,
        code: "route-surface-drift",
        detail:
          "ARH1-CUT-4 consumersAffected are not exactly the ARH1 bulk-row consumer population",
      });
    }
  }

  // AC3: every charter concern carries existing named evidence.
  const concerns = new Map((characterization.ac3?.concerns || []).map((c) => [c.concern, c]));
  for (const concern of AC3_CONCERNS) {
    const row = concerns.get(concern);
    if (!row) {
      errors.push({
        caseId,
        code: "ac3-concern-missing",
        detail: `${concern} carries no existing-evidence binding`,
      });
      continue;
    }
    if (!Array.isArray(row.evidence) || row.evidence.length === 0) {
      errors.push({ caseId, code: "ac3-concern-without-evidence", detail: concern });
      continue;
    }
    for (const witness of row.evidence) checkWitness(witness, errors, caseId);
  }
  for (const concern of concerns.keys()) {
    if (!AC3_CONCERNS.includes(concern)) {
      errors.push({
        caseId,
        code: "ac3-concern-invented",
        detail: `${concern} is not a charter AC3 concern`,
      });
    }
  }
}

function validateSeparation(products, predecessors, errors) {
  const caseId = "ARH2-separation";
  const measurements = products["complexity-measurements"];
  const characterization = products["characterization"];
  const gatesText = readRel("performance-gates.toml");
  const gateIds = new Set([...gatesText.matchAll(/^id = "([^"]+)"/gm)].map((m) => m[1]));

  const dimensions = measurements.dimensions || [];
  const ids = dimensions.map((d) => d.id);
  for (const id of DIMENSION_IDS) {
    if (ids.filter((x) => x === id).length !== 1) {
      errors.push({
        caseId,
        code: "dimension-cardinality",
        detail: `${id} must appear exactly once (charter separation)`,
      });
    }
  }
  for (const id of new Set(ids)) {
    if (!DIMENSION_IDS.includes(id)) {
      errors.push({
        caseId,
        code: "dimension-invented",
        detail: `${id} is not one of the four charter measurement dimensions`,
      });
    }
  }
  for (const dimension of dimensions) {
    if (typeof dimension.separates !== "string" || dimension.separates.length === 0) {
      errors.push({
        caseId,
        code: "dimension-without-separation",
        detail: `${dimension.id} does not declare what it separates from`,
      });
    }
    if (typeof dimension.measures !== "string" || dimension.measures.length === 0) {
      errors.push({
        caseId,
        code: "dimension-without-measures",
        detail: `${dimension.id} does not declare what it measures`,
      });
    }
    for (const key of Object.keys(dimension)) {
      if (FORBIDDEN_NUMERIC_KEYS.includes(key)) {
        errors.push({
          caseId,
          code: "dimension-commits-wall-clock",
          detail: `${dimension.id} commits ${key}; measured numbers live in gate receipts, never in products`,
        });
      }
    }
    const requiredKind = DIMENSION_MECHANISM_KIND[dimension.id];
    if (requiredKind && dimension.mechanismKind !== requiredKind) {
      errors.push({
        caseId,
        code: "dimension-mechanism-kind-mismatch",
        detail: `${dimension.id} must use mechanismKind ${requiredKind}, not ${JSON.stringify(dimension.mechanismKind)}`,
      });
    }
    const mechanisms = dimension.mechanisms;
    if (!Array.isArray(mechanisms) || mechanisms.length === 0) {
      errors.push({
        caseId,
        code: "mechanism-binding-missing",
        detail: `${dimension.id} has no nonempty measurement mechanism binding`,
      });
      continue;
    }
    if (dimension.mechanismKind === "gate-cell") {
      let cellsById = new Map();
      try {
        cellsById = new Map(readGatesToml(gatesText).cells.map((cell) => [cell.id, cell]));
      } catch {
        cellsById = new Map();
      }
      for (const cellId of mechanisms) {
        if (typeof cellId !== "string" || !gateIds.has(cellId)) {
          errors.push({
            caseId,
            code: "gate-cell-unknown",
            detail: `${cellId} is not a locked cell of performance-gates.toml`,
          });
          continue;
        }
        if (dimension.id === "application-latency") {
          const cell = cellsById.get(cellId);
          const owner = String(cell?.owner || "");
          const hay = `${cell?.operation || ""} ${cell?.execution_profile || ""} ${cell?.result_contract || ""}`;
          if ((owner !== "verter_session" && owner !== "verter_scheduler") || !/host/i.test(hay)) {
            errors.push({
              caseId,
              code: "gate-cell-wrong-operation",
              detail: `${cellId} does not measure the characterized host/session path`,
            });
          }
        }
      }
    }
    if (dimension.mechanismKind === "cargo-build") {
      const states = new Set();
      for (const recipe of mechanisms) {
        if (
          !recipe ||
          typeof recipe !== "object" ||
          typeof recipe.command !== "string" ||
          !recipe.command.startsWith("cargo build") ||
          !recipe.command.includes("--timings")
        ) {
          errors.push({
            caseId,
            code: "cargo-build-recipe-malformed",
            detail: `${dimension.id} recipes must be cargo build --timings commands with a cacheState`,
          });
          continue;
        }
        if (!CARGO_BUILD_CACHE_STATES.includes(recipe.cacheState)) {
          errors.push({
            caseId,
            code: "cargo-build-cache-state-unknown",
            detail: `${dimension.id} recipe cacheState ${JSON.stringify(recipe.cacheState)} is not clean or warm`,
          });
          continue;
        }
        states.add(recipe.cacheState);
      }
      if (!states.has("clean") || !states.has("warm")) {
        errors.push({
          caseId,
          code: "dimension-cache-state-incomplete",
          detail: `${dimension.id} must bind both clean and warm repository-build cache states`,
        });
      }
    }
  }

  const pinned = collectPinCommands(characterization);
  const joinPinnedLanes = (dimension, unboundCode, inventedCode) => {
    if (!dimension || !Array.isArray(dimension.mechanisms) || dimension.mechanisms.length === 0) {
      return;
    }
    const declared = new Set(dimension.mechanisms);
    for (const command of pinned) {
      if (!declared.has(command)) {
        errors.push({
          caseId,
          code: unboundCode,
          detail: `pinned lane ${command} is not recorded in ${dimension.id}`,
        });
      }
    }
    for (const command of declared) {
      if (!pinned.has(command)) {
        errors.push({
          caseId,
          code: inventedCode,
          detail: `${command} is recorded as a ${dimension.id} lane but no pin carries it`,
        });
      }
    }
  };
  joinPinnedLanes(
    dimensions.find((d) => d.id === "production-behavior"),
    "behavior-lane-unbound",
    "behavior-lane-invented",
  );
  joinPinnedLanes(
    dimensions.find((d) => d.id === "test-cost"),
    "test-cost-lane-unbound",
    "test-cost-lane-invented",
  );

  // The runner class is the locked one; a different class is a recalibration.
  const runnerClass = gatesText.match(/^\[runner\][\s\S]*?^class = "([^"]+)"/m)?.[1];
  if (measurements.numberPolicy?.runnerClass !== runnerClass) {
    errors.push({
      caseId,
      code: "runner-class-unbound",
      detail: `numberPolicy.runnerClass must equal the locked [runner] class ${runnerClass}`,
    });
  }

  // Threshold ratification: the guard exists, its ceiling matches the live
  // declaration, and the measured basis is re-derived live.
  const basis = measurements.godModuleBasis;
  const thresholdRows = measurements.thresholds || [];
  if (!Array.isArray(thresholdRows) || thresholdRows.length === 0) {
    errors.push({
      caseId,
      code: "threshold-block-missing",
      detail: "ARH0-DEBT-5 requires a ratified threshold basis before ARH12 extends the guard",
    });
  }
  const guardFile = thresholdRows[0]?.guardFile;
  if (!existsRel(guardFile)) {
    errors.push({ caseId, code: "threshold-guard-missing", detail: guardFile });
  } else {
    const guardText = readRel(guardFile);
    if (!guardText.includes(`fn ${thresholdRows[0].guard}(`)) {
      errors.push({
        caseId,
        code: "threshold-guard-unknown",
        detail: `${thresholdRows[0].guard} is not a function of ${guardFile}`,
      });
    }
    const liveCeiling = Number(guardText.match(/DEFAULT_MAX_LINES: usize = (\d+)/)?.[1]);
    if (thresholdRows[0].defaultMaxLines !== liveCeiling || basis?.thresholdLoc !== liveCeiling) {
      errors.push({
        caseId,
        code: "threshold-ceiling-mismatch",
        detail: `ratified ceiling must equal the live guard ceiling ${liveCeiling}`,
      });
    }
  }
  const isTestFixture = (rel) =>
    rel.endsWith("_tests.rs") || rel.endsWith("/tests.rs") || rel.includes("/tests/");
  const overThreshold = [];
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const abs = path.join(dir, entry.name);
      const rel = path.relative(REPO_ROOT, abs).split(path.sep).join("/");
      if (entry.isDirectory()) walk(abs);
      else if (
        entry.name.endsWith(".rs") &&
        rel.startsWith("crates/") &&
        rel.includes("/src/") &&
        !isTestFixture(rel) &&
        rustLineCount(readRel(rel)) > basis.thresholdLoc
      ) {
        overThreshold.push(rel);
      }
    }
  };
  walk(path.join(REPO_ROOT, "crates"));
  if (basis?.productionFilesOverThreshold !== overThreshold.length) {
    errors.push({
      caseId,
      code: "threshold-basis-drift",
      detail: `recorded ${basis?.productionFilesOverThreshold} production files over ${basis?.thresholdLoc} LOC, live derivation finds ${overThreshold.length}`,
    });
  }
  const hotspotLocs = new Map((measurements.structural || []).map((r) => [r.path, r.fileLoc]));
  const hotspotsOver = [...hotspotLocs.values()].filter((loc) => loc > basis?.thresholdLoc).length;
  if (basis?.hotspotsWithinPopulation !== hotspotsOver) {
    errors.push({
      caseId,
      code: "threshold-basis-drift",
      detail: `recorded ${basis?.hotspotsWithinPopulation} hotspots in the over-threshold population, live derivation finds ${hotspotsOver}`,
    });
  }
}

function validateRatification(products, manifest, errors) {
  const caseId = "ARH2-ratification";
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
    ["test", manifest.test, `node --test ${toRepoPosix(path.join(HERE, "arh2.test.mjs"))}`],
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

  const candidates = PRODUCT_FILES.map((file) => products[file.replace(/\.json$/, "")]?.candidate);
  if (new Set(candidates).size !== 1 || !/^[0-9a-f]{40}$/.test(candidates[0] ?? "")) {
    errors.push({
      caseId,
      code: "candidate-basis-drift",
      detail: "both products must pin the same 40-hex candidate basis",
    });
  }
}

export function validate(products, manifest = loadManifest()) {
  const errors = [];
  const predecessors = { arh0: loadArh0Products(), arh1: loadArh1Products() };

  validatePopulation(products, predecessors, errors);
  validateDeletion(products, predecessors, errors);
  validateCharacterization(products, predecessors, errors);
  validateSeparation(products, predecessors, errors);
  validateRatification(products, manifest, errors);
  return { ok: errors.length === 0, errors };
}

export function mandatoryCases() {
  return [
    "ARH2-population",
    "ARH2-deletion",
    "ARH2-characterization",
    "ARH2-separation",
    "ARH2-ratification",
  ];
}

/** Case ids with at least one recorded error (dirty-twin selection helper). */
export function selectedCaseIds(result) {
  return [...new Set(result.errors.map((e) => e.caseId))];
}

export function validateProvenance(candidate) {
  if (!/^[0-9a-f]{40}$/.test(candidate ?? "")) {
    return {
      ok: false,
      reason: `candidate ${JSON.stringify(candidate)} is not a 40-hex git commit`,
    };
  }
  const git = (args) => {
    try {
      execFileSync("git", ["-C", REPO_ROOT, ...args], {
        stdio: ["ignore", "ignore", "ignore"],
      });
      return true;
    } catch {
      return false;
    }
  };
  if (!git(["cat-file", "-e", `${candidate}^{commit}`])) {
    return { ok: false, reason: `candidate ${candidate} is not a commit of this repository` };
  }
  if (!git(["merge-base", "--is-ancestor", candidate, "HEAD"])) {
    return { ok: false, reason: `candidate ${candidate} is not an ancestor of HEAD` };
  }
  return { ok: true };
}

// The manifest documents `node tests/architecture-health/ARH2/verify.mjs` as
// this node's verify command; it must validate the real products, not no-op.
const isMain =
  process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url));
if (isMain) {
  const products = loadProducts();
  const result = validate(products);
  if (process.argv.includes("--provenance")) {
    const provenance = validateProvenance(products["characterization"].candidate);
    if (!provenance.ok) {
      result.errors.push({
        caseId: "ARH2-ratification",
        code: "candidate-basis-drift",
        detail: provenance.reason,
      });
      result.ok = false;
    }
  }
  if (!result.ok) {
    console.error(result.errors.map((e) => `${e.caseId}/${e.code}: ${e.detail}`).join("\n"));
    process.exit(1);
  }
  console.log(`ARH2 verify: PASS cases=${mandatoryCases().join(",")}`);
}
