#!/usr/bin/env node
/**
 * ARH1 contract verifier — the sole owning interface of the responsibility,
 * visibility and dependency contracts.
 *
 * Validates the two products against the shipped ARH0 predecessor products
 * and the working tree: every ARH0 god-module candidate carries exactly one
 * hotspot contract (and vice versa) with no dropped or invented
 * responsibility, each surviving authority/cohesive module exists, the
 * declared import direction equals the production import tree actually
 * measured in the live source — use statements AND inline crate paths
 * (macro paths like `verter_audit::attribute!`, feature-gated type aliases,
 * derive paths) measured on the production-stripped text, so an inline-only
 * crate reference is drift in every direction and a forbidden root is
 * caught in use or inline form alike (cfg(test)/cfg(all(test, ..)) items
 * are stripped before measuring, so the equality claim covers every
 * direction and never counts test configuration as production direction;
 * the forbidden directions are absent from the full source — including its
 * test configuration — and the owning crate's Cargo.toml), layer rules
 * equal the live crate dependency manifests BOTH ways with a pinned rule
 * population (every hotspot crate governed exactly once, contiguous ids,
 * non-hotspot crates declared with a rationale), constructors and their
 * capability anchors are real declarations with the constructor population
 * derived from the module's primary type (a deleted constructor row is
 * drift, not silence), state-lifetime rows name live identifiers and one
 * sole owner that DECLARES the state (a comment mention or the enclosing
 * type's name in a consumer file is not ownership), surface declarations
 * are pinned against the derived parent-module declaration, public
 * modules' complete pub-item populations must be retained/narrowed or
 * carried by a bulk row (deleted rows are drift), every narrowing consumer
 * is a live referencing file and every live referencing file is a recorded
 * consumer — fn rows by qualified reference forms, field rows by
 * type-qualified field use (an ambiguous field name is only a consumer
 * through a struct literal of the owning type), importers by the derived
 * cross-crate population for EVERY hotspot (comment mentions never count)
 * — retained assoc items must cover qualified, receiver and
 * binding-evidenced usage on retained types, no cohesive split retains
 * shared unrestricted state (ARH1-AC2), every route ARH0 assigned to this
 * node (plus every narrowing route declared here, keyed per narrow kind so
 * two routes on one file cannot merge or vanish) has exactly one cutover
 * disposition carried by a row bound to exactly one recognized route/debt
 * key, and the AC4 surface obligations name every charter-mandated surface.
 * The program DAG is database-owned by the TAMA controller, so owner/heir
 * ids are checked structurally only and no DAG file is read; the ARH0
 * products are joined as shipped predecessor evidence, never re-derived
 * here. ARH1-ratification fails when the manifest and the verifier
 * disagree about the case contract, so the manifest cannot claim checks
 * that do not run.
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
// Path roots that are primitive types, never crate names (`u64::from`, `str::from_utf8`).
const PRIMITIVE_ROOTS = new Set([
  "bool",
  "char",
  "str",
  "u8",
  "u16",
  "u32",
  "u64",
  "u128",
  "usize",
  "i8",
  "i16",
  "i32",
  "i64",
  "i128",
  "isize",
  "f32",
  "f64",
]);
// Tool-lint namespaces in attributes (`#[allow(clippy::..)]`), not crate references.
const TOOL_LINT_ROOTS = new Set(["clippy", "rustc", "rustdoc"]);
// A receiver-call chain must sit within this many lines of a qualified
// assoc reference (`T::assoc`) for the call to count as usage evidence of T.
const RECEIVER_EVIDENCE_LINE_WINDOW = 8;
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
// Charter ARH1-AC4 surfaces a contract-only node must specify for its
// producers (obligation or explicit N/A rationale naming the surface).
const AC4_SURFACES = Object.freeze([
  "VIM/DX",
  "host/profile",
  "permissions",
  "uncertainty",
  "migration",
]);
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

const readTextCache = new Map();

function readRel(rel) {
  let t = readTextCache.get(rel);
  if (t === undefined) {
    t = fs.readFileSync(path.join(REPO_ROOT, rel), "utf8");
    readTextCache.set(rel, t);
  }
  return t;
}

// ---------------------------------------------------------------------------
// Live-tree scanning substrate (shared, cached for the process lifetime —
// the tree does not change under one validate() run or one test process).
// ---------------------------------------------------------------------------

/** Every .rs file under crates/ (repo-relative, forward slashes). */
let _rustFiles;
function rustFilesUnderCrates() {
  if (_rustFiles) return _rustFiles;
  const out = [];
  const rec = (dir) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) rec(p);
      else if (e.name.endsWith(".rs"))
        out.push(path.relative(REPO_ROOT, p).split(path.sep).join("/"));
    }
  };
  rec(path.join(REPO_ROOT, "crates"));
  _rustFiles = out;
  return out;
}

const strippedTextCache = new Map();
/** File text with line and block comments stripped (doc mentions never
 * count as references). */
function strippedFileText(rel) {
  let t = strippedTextCache.get(rel);
  if (t === undefined) {
    t = stripComments(readRel(rel));
    strippedTextCache.set(rel, t);
  }
  return t;
}

const ownerTextCache = new Map();
/** Comment-stripped text of a sole-owner target: the file itself, or every
 * .rs under a module directory (recursive). Doc mentions never prove
 * ownership, so comments are stripped before any declaration check. */
function soleOwnerText(rel) {
  let t = ownerTextCache.get(rel);
  if (t !== undefined) return t;
  const abs = path.resolve(REPO_ROOT, rel);
  let out = null;
  if (fs.existsSync(abs)) {
    const stat = fs.statSync(abs);
    if (stat.isFile()) {
      out = stripComments(readRel(rel));
    } else if (stat.isDirectory()) {
      out = "";
      const rec = (dir) => {
        for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
          const p = path.join(dir, e.name);
          if (e.isDirectory()) rec(p);
          else if (e.name.endsWith(".rs")) out += stripComments(fs.readFileSync(p, "utf8"));
        }
      };
      rec(abs);
    }
  }
  ownerTextCache.set(rel, out);
  return out;
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
        const p = part
          .trim()
          .replace(/\s+as\s+\w+$/, "")
          .replace(/::$/, "");
        if (p) out.push(prefix ? `${prefix}::${p}` : p);
      }
    }
    return out;
  };
  return walk(text, "");
}

function stripComments(text) {
  return text.replace(/\/\/[^\n]*/g, "").replace(/\/\*[\s\S]*?\*\//g, "");
}

/** Replaces string literals with blanks (escape- and char-literal-aware, so
 * `'a` lifetimes stay and `'\n'` chars vanish). Path-like text inside string
 * literals is data, never an import. */
function blankStrings(text) {
  let out = "";
  let i = 0;
  while (i < text.length) {
    const ch = text[i];
    if (ch === '"') {
      i++;
      while (i < text.length && text[i] !== '"') {
        if (text[i] === "\\") i++;
        i++;
      }
      i++;
      out += " ";
    } else if (ch === "'") {
      const n = text.indexOf("'", i + 1);
      const span = n === -1 ? Infinity : n - i;
      if (n !== -1 && ((text[i + 1] === "\\" && span <= 4) || span <= 2)) {
        out += " ";
        i = n + 1;
      } else {
        out += ch;
        i++;
      }
    } else {
      out += ch;
      i++;
    }
  }
  return out;
}

/**
 * Measures the import tree of one source file the way the contract declares
 * it: `use`/`pub use` statements expanded into full paths (nested groups
 * inherit their prefix), classified by first path segment, PLUS every
 * inline crate path in code position — `verter_audit::attribute!(..)`,
 * `crossbeam_channel::Sender`, `#[derive(serde::Serialize)]`, feature-gated
 * `hotpath::wrap::..` type aliases — with string literals blanked,
 * turbofish forms (`collect::<..>`) skipped and use-bound names (imported
 * terminals and renames, file-local mods and macro_rules) never
 * misread as crate roots. `crate::` roots become the crate-internal set;
 * `super::`/`self::` stay internal without naming a root. This is the FULL
 * measurement (test configuration included); the import-direction equality
 * runs on measureProductionImports below.
 */
export function measureImports(text) {
  const source = stripComments(text);
  return measureStrippedImports(source);
}

function measureStrippedImports(source) {
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
  // Names bound by use statements (terminals and `as` renames): an inline
  // `bound_name::path` resolves to the import, not to a crate root.
  const boundNames = new Set(fileMods);
  for (const sm of source.matchAll(/\b(?:pub\s+)?use\s([^;]*);/g)) {
    for (const usePath of expandUsePaths(sm[1])) {
      const segs = usePath.split("::");
      // Every segment of a use path is import context (crate root, module,
      // terminal or rename): an inline `segment::path` reference resolves
      // through the import, never to a bare crate root.
      for (const seg of segs) {
        if (/^[a-z_][a-z_0-9]*$/.test(seg)) boundNames.add(seg);
      }
      const first = segs[0];
      if (!/^[a-z_][a-z_0-9]*$/.test(first)) continue;
      if (first === "crate") {
        const root = segs[1];
        if (root) internal.add(root);
        continue;
      }
      if (SKIPPED_ROOTS.has(first)) continue; // std/super/self: unnamed internal
      if (first.startsWith("verter_")) verter.add(first);
      else if (!fileMods.has(first)) external.add(first);
    }
    const rename = sm[1].match(/\bas\s+([A-Za-z_][A-Za-z_0-9]*)\s*$/);
    if (rename) boundNames.add(rename[1]);
  }
  // Inline crate paths in code position (macro calls, qualified types,
  // derive paths, feature-gated aliases).
  for (const m of blankStrings(source).matchAll(/(?<![A-Za-z_0-9\]:])([a-z_][a-z_0-9]*)::(?!<)/g)) {
    const first = m[1];
    if (
      SKIPPED_ROOTS.has(first) ||
      PRIMITIVE_ROOTS.has(first) ||
      TOOL_LINT_ROOTS.has(first) ||
      boundNames.has(first) ||
      internal.has(first)
    ) {
      continue;
    }
    if (first.startsWith("verter_")) verter.add(first);
    else external.add(first);
  }
  return { verter, external, internal };
}

/**
 * True when the cfg predicate makes an item test-only: exactly `test`, or
 * `all(test, ..)` — `any(test, feature = "test-support")` also compiles in
 * production test-support builds and stays measured.
 */
function isTestOnlyCfgPredicate(pred) {
  const p = pred.trim();
  if (p === "test") return true;
  if (!p.startsWith("all(")) return false;
  // First top-level comma of the all(..) argument list.
  let depth = 0;
  for (let i = 4; i < p.length; i++) {
    const ch = p[i];
    if (ch === "(") depth++;
    else if (ch === ")") {
      if (depth === 0) return false;
      depth--;
    } else if (ch === "," && depth === 0) {
      return p.slice(4, i).trim() === "test";
    }
  }
  return false;
}

function matchBracket(s, openIdx, open, close) {
  let depth = 0;
  for (let i = openIdx; i < s.length; i++) {
    const ch = s[i];
    if (ch === '"') {
      // Escape-aware string skip so braces inside literals never unbalance
      // the item span.
      i++;
      while (i < s.length && s[i] !== '"') {
        if (s[i] === "\\") i++;
        i++;
      }
      continue;
    }
    if (ch === "'") {
      // A char literal is `'x'` / `'\n'` / `'\u{7}'`; a bare `'a` that does
      // not close within a few chars is a lifetime, not a literal.
      const n = s.indexOf("'", i + 1);
      const span = n === -1 ? Infinity : n - i;
      const isEscapedLiteral = s[i + 1] === "\\" && span <= 4;
      const isPlainLiteral = span <= 2;
      if (n !== -1 && (isEscapedLiteral || isPlainLiteral)) {
        i = n;
        continue;
      }
      continue; // lifetime tick: an ordinary character
    }
    if (ch === open) depth++;
    else if (ch === close) {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/**
 * Removes every item attributed with a test-only cfg predicate (comment
 * text is stripped first, so prose attributes never match). The item span
 * runs from the attribute to its terminating `;` (statement item) or to
 * the balanced `{...}` block (mod/impl/fn item); removed spans keep their
 * line count so diagnostics stay addressable.
 */
export function stripCfgTestItems(text) {
  const src = stripComments(text);
  let out = "";
  let i = 0;
  for (;;) {
    const at = src.indexOf("#[", i);
    if (at === -1) {
      out += src.slice(i);
      break;
    }
    const close = matchBracket(src, at + 1, "[", "]");
    if (close === -1) {
      out += src.slice(i);
      break;
    }
    const attr = src.slice(at, close + 1);
    const cfg = attr.match(/^#\[cfg\(([\s\S]*)\)\]$/);
    if (!cfg || !isTestOnlyCfgPredicate(cfg[1])) {
      out += src.slice(i, close + 1);
      i = close + 1;
      continue;
    }
    out += src.slice(i, at);
    // Skip whitespace and any further attributes stacked on the same item.
    let p = close + 1;
    for (;;) {
      while (p < src.length && /\s/.test(src[p])) p++;
      if (src.startsWith("#[", p)) {
        const c2 = matchBracket(src, p + 1, "[", "]");
        if (c2 === -1) break;
        p = c2 + 1;
        continue;
      }
      break;
    }
    let end = -1;
    for (let j = p; j < src.length; j++) {
      const ch = src[j];
      if (ch === ";") {
        end = j + 1;
        break;
      }
      if (ch === "{") {
        const c3 = matchBracket(src, j, "{", "}");
        end = c3 === -1 ? src.length : c3 + 1;
        break;
      }
    }
    if (end === -1) end = src.length;
    out += "\n".repeat((src.slice(at, end).match(/\n/g) || []).length);
    i = end;
  }
  return out;
}

/**
 * Measures the PRODUCTION import tree: the full measurement minus cfg(test)
 * and cfg(all(test, ..)) items. The declared import direction equals this
 * tree, in every direction, so a live undeclared crate-internal root (test
 * configuration or not) is drift, and a declared root that only test
 * configuration imports is drift too.
 */
export function measureProductionImports(text) {
  return measureStrippedImports(stripCfgTestItems(text));
}

/**
 * ARH1-AC1: the declared import direction is the measured production one —
 * drift in either direction fails for verter, external and crate-internal
 * roots alike, the forbidden directions are absent from the live source and
 * the owning crate's manifest, and the layer rules equal the live crate
 * dependency manifests. A compile-time boundary is the preferred
 * enforcement (ARH12's obligation); this verifier is the contract's owning
 * interface until then.
 */
function validateImportDirection(contracts, errors) {
  const caseId = "ARH1-import-direction";
  for (const hotspot of contracts.hotspots) {
    const declared = hotspot.allowedImportDirection;
    const measured = measureProductionImports(readRel(hotspot.path));
    const full = measureImports(readRel(hotspot.path));
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
    cmp("crate-internal", declared.crateInternal, measured.internal);
    for (const forbidden of declared.mustNotImport) {
      if (full.verter.has(forbidden) || full.external.has(forbidden)) {
        errors.push({
          caseId,
          code: "forbidden-import",
          detail: `${hotspot.path}: imports forbidden root ${forbidden}`,
        });
      }
    }
    const crateDir = owningCrateDir(hotspot.path);
    const crateDeps = crateVerterDeps(crateDir);
    for (const v of full.verter) {
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
    for (const importer of declared.importers || []) {
      if (!existsRel(importer)) {
        errors.push({ caseId, code: "missing-importer", detail: importer });
      }
      // Reference evidence is enforced by the exact cross-crate population
      // join in validateSurface (ARH1-surface/importer-population-drift):
      // a comment mention of the module name is never a consumer.
    }
  }
  // The layer-rule population is pinned, not advisory: every hotspot's
  // owning crate is governed exactly once, ids are contiguous from 1 (so a
  // deleted middle rule leaves a gap), a crate outside every hotspot needs
  // a declared rationale to be governed at all, and mayImport equals the
  // live manifest BOTH ways (an invented allowance is drift exactly like a
  // missing one).
  const ruleCrates = new Map();
  for (const rule of contracts.layerRules) {
    ruleCrates.set(rule.crate, (ruleCrates.get(rule.crate) || 0) + 1);
  }
  for (const [crate, count] of ruleCrates) {
    if (count > 1) {
      errors.push({
        caseId,
        code: "duplicate-layer-crate",
        detail: `${crate} carries ${count} layer rules; one crate, one rule`,
      });
    }
  }
  const hotspotCrates = new Set(contracts.hotspots.map((h) => owningCrateDir(h.path)));
  for (const crate of hotspotCrates) {
    if (!ruleCrates.has(crate)) {
      errors.push({
        caseId,
        code: "missing-layer-rule",
        detail: `${crate} owns a hotspot but no layer rule governs its crate dependencies`,
      });
    }
  }
  const ruleNums = contracts.layerRules.map((r) =>
    /^ARH1-LAYER-([0-9]+)$/.test(r.id) ? Number(r.id.slice("ARH1-LAYER-".length)) : NaN,
  );
  const contiguous =
    ruleNums.every(Number.isInteger) &&
    new Set(ruleNums).size === ruleNums.length &&
    [...ruleNums].sort((a, b) => a - b).every((v, i) => v === i + 1);
  if (!contiguous) {
    errors.push({
      caseId,
      code: "layer-rule-population-drift",
      detail: `layer rule ids must be exactly ARH1-LAYER-1..n with no gap or duplicate (found ${contracts.layerRules.map((r) => r.id).join(", ")})`,
    });
  }
  for (const rule of contracts.layerRules) {
    if (!existsRel(rule.crate)) {
      errors.push({ caseId, code: "layer-crate-missing", detail: rule.crate });
      continue;
    }
    if (!hotspotCrates.has(rule.crate)) {
      if (typeof rule.nonHotspotRationale !== "string" || rule.nonHotspotRationale.length === 0) {
        errors.push({
          caseId,
          code: "layer-crate-unexplained",
          detail: `${rule.id}: ${rule.crate} owns no hotspot; a nonHotspotRationale is required to keep it governed`,
        });
      }
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
    for (const allowed of rule.mayImport) {
      if (!deps.has(allowed)) {
        errors.push({
          caseId,
          code: "layer-allowance-without-dependency",
          detail: `${rule.id}: ${rule.crate} has no dependency on ${allowed}; mayImport must equal the live manifest both ways`,
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
 * parameter list, not a wish), a hotspot without constructors records why,
 * and the constructor table is COMPLETE: for the module's primary type
 * (the retained type named after the module), every pub associated fn
 * returning Self that is not a narrowed test hook must carry a capability
 * row — deleting a row is drift, not silence. Test-only seams must be named
 * as such.
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
    // Completeness: the constructor table of the module's primary retained
    // type equals its derived Self-returning public constructors (narrowed
    // test hooks excluded — their disposition is the narrowing row).
    const base = path.basename(hotspot.path, ".rs");
    const primary = (hotspot.minimalPublicSurface?.retainedTypes || []).find(
      (t) => t.replace(/_/g, "").toLowerCase() === base.replace(/_/g, "").toLowerCase(),
    );
    if (primary) {
      const stripped = strippedFileText(hotspot.path);
      const narrowFns = new Set(
        (hotspot.minimalPublicSurface.narrow || [])
          .filter((r) => r.kind === "fn")
          .map((r) => r.item),
      );
      const derived = new Set();
      for (const m of stripped.matchAll(/\bimpl(?:<[^{]*>)?\s+([A-Za-z_][A-Za-z_0-9]*)\s*\{/g)) {
        if (m[1] !== primary) continue;
        const body = braceSlice(stripped, m.index + m[0].length - 1);
        for (const fm of body.matchAll(
          /\bpub\s+(?:const\s+)?fn\s+([a-z_][a-z_0-9]*)[^{;]*->\s*([^{;]*)/g,
        )) {
          if (/\bSelf\b/.test(fm[2]) && !narrowFns.has(fm[1])) derived.add(fm[1]);
        }
      }
      const declared = new Set(rows.map((r) => r.constructor));
      for (const fn of derived) {
        if (!declared.has(fn)) {
          errors.push({
            caseId,
            code: "constructor-population-drift",
            detail: `${hotspot.path}: pub fn ${primary}::${fn} constructs ${primary} but carries no capability row`,
          });
        }
      }
      for (const row of rows) {
        if (!derived.has(row.constructor)) {
          errors.push({
            caseId,
            code: "constructor-population-drift",
            detail: `${row.constructor} is not a derived Self-returning pub constructor of ${primary}`,
          });
        }
      }
    }
  }
}

/** Declaration forms that bind an identifier: fn/item/macro/let/field
 * declarations. A use or mention of the name is none of these, so a file
 * that merely names the enclosing type or mentions the state in prose
 * never counts as the owner. */
function declaresIdentifier(text, ident) {
  const i = ident.replace(/[^A-Za-z_0-9]/g, "");
  if (!i) return false;
  return (
    new RegExp(`\\bfn\\s+${i}\\b`).test(text) ||
    new RegExp(`\\b(?:struct|enum|trait|type|union|mod|const|static)\\s+${i}\\b`).test(text) ||
    new RegExp(`\\bmacro_rules!\\s+${i}\\b`).test(text) ||
    new RegExp(`\\blet(?:\\s+mut)?\\s+${i}\\b`).test(text) ||
    new RegExp(`(?:^|\\n)\\s*(?:pub(?:\\([a-z_]+\\))?\\s+)?${i}\\s*:\\s*[^:]`).test(text)
  );
}

/**
 * ARH1-AC1/AC3: every state-lifetime row names an identifier that exists in
 * the live source, a lifetime inside the contract vocabulary and one sole
 * owner — and the owner target DECLARES the state in code (comment-
 * stripped): for a `Type.field` state every field identifier must be
 * declared in the owner; for a prose state at least one non-enclosing
 * token (the parenthesized concrete identifier is the convention) must be
 * declared. A consumer file that constructs the type or mentions the state
 * in a doc comment is not the sole owner.
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
      if (typeof row.soleOwner !== "string" || row.soleOwner.length === 0) {
        errors.push({
          caseId,
          code: "state-without-owner",
          detail: `${hotspot.path}: ${row.state}`,
        });
        continue;
      }
      const ownerText = soleOwnerText(row.soleOwner);
      if (ownerText === null || ownerText.length === 0) {
        errors.push({
          caseId,
          code: "state-owner-missing",
          detail: `${row.state}: sole owner ${row.soleOwner}`,
        });
        continue;
      }
      const parts = row.state
        .split(/,| and /)
        .map((p) => p.trim())
        .filter(Boolean);
      const dotted = (p) => /^[A-Z][A-Za-z0-9]*\.[a-z_][a-z_0-9]*$/.test(p);
      const enclosing = new Set();
      for (const p of parts) {
        const m = p.match(/^([A-Z][A-Za-z0-9]*)\./);
        if (m) enclosing.add(m[1]);
      }
      let owned;
      if (parts.length > 0 && parts.every(dotted)) {
        // A `Type.field[, and Type.field…]` state: the owner must declare
        // every field identifier; the enclosing type's name alone is not
        // ownership evidence.
        owned = parts.every((p) => declaresIdentifier(ownerText, p.split(".")[1]));
      } else {
        const candidates = tokens.filter((t) => !enclosing.has(t));
        owned = candidates.some((t) => declaresIdentifier(ownerText, t));
      }
      if (!owned) {
        errors.push({
          caseId,
          code: "state-owner-without-declaration",
          detail: `${row.state}: sole owner ${row.soleOwner} declares no identifier of the state (comments and the enclosing type's name do not count)`,
        });
      }
    }
  }
}

// ---------------------------------------------------------------------------
// Cross-crate consumer derivation (syntax evidence, comments stripped).
// ---------------------------------------------------------------------------

const consumersCache = new Map();

/**
 * Derives the complete cross-crate consumer population of one hotspot
 * module: every .rs file under crates/ outside the owning crate's src/
 * tree whose code (never its comments) references the module through its
 * owning crate's path — directly (`verter_x::module::..`), through an alias
 * (`use verter_x as h; h::module::..`), or by importing the module itself
 * (`use verter_x::module;` + local `module::..` refs).
 *
 * Returns a Map file -> { names: Set<string>, qualified: Set<string> }
 * where names are the referenced item identifiers (use-statement terminals
 * and inline path terminals alike) and qualified are `Type::member` pairs
 * referencing retained types (assoc-item usage evidence).
 */
export function deriveModuleConsumers(hotspot) {
  const cached = consumersCache.get(hotspot.path);
  if (cached) return cached;
  const crateDir = owningCrateDir(hotspot.path); // crates/<crate>
  const crateName = crateDir.split("/")[1];
  const moduleToken = path.basename(hotspot.path).replace(/\.rs$/, "");
  const srcPrefix = `${crateDir}/src/`;
  const result = new Map();
  for (const file of rustFilesUnderCrates()) {
    if (file.startsWith(srcPrefix)) continue; // crate-internal paths never cross crates
    const text = strippedFileText(file);
    const roots = [crateName];
    for (const m of text.matchAll(
      new RegExp(`\\b(?:pub\\s+)?use\\s+${crateName}\\s+as\\s+([A-Za-z_][A-Za-z_0-9]*)\\s*;`, "g"),
    )) {
      roots.push(m[1]);
    }
    const names = new Set();
    let references = false;
    for (const root of roots) {
      const needle = `${root}::${moduleToken}`;
      // Inline and single-target path references: <root>::<module>::Item.
      const pathRe = new RegExp(
        `(?<![A-Za-z_0-9:])${root}::${moduleToken}::([A-Za-z_][A-Za-z_0-9]*)`,
        "g",
      );
      for (const m of text.matchAll(pathRe)) {
        names.add(m[1]);
        references = true;
      }
      // Use statements (brace groups included) naming any item under the module.
      for (const sm of text.matchAll(/\b(?:pub\s+)?use\s([^;]*);/g)) {
        for (const p of expandUsePaths(sm[1])) {
          const idx = p.indexOf(needle);
          if (idx === -1) continue;
          // Boundary: the needle must be a whole path segment, not a prefix
          // (`verter_session::flow_return_audit` does not consume `flow_return`).
          const after = p[idx + needle.length];
          if (after !== undefined && after !== ":") continue;
          references = true;
          const tail = p.slice(idx + needle.length).replace(/^::/, "");
          const last = tail.split("::").pop();
          if (last && /^[A-Za-z_][A-Za-z_0-9]*$/.test(last)) names.add(last);
        }
      }
      // Module-import form: use <root>::<module>; + local module::Item refs.
      if (new RegExp(`\\buse\\s+${root}::${moduleToken}\\s*;`).test(text)) {
        references = true;
        const localRe = new RegExp(`[^A-Za-z_0-9:]${moduleToken}::([A-Za-z_][A-Za-z_0-9]*)`, "g");
        for (const m of text.matchAll(localRe)) {
          if (m[1] !== moduleToken) names.add(m[1]);
        }
        names.delete(moduleToken);
      }
    }
    if (references) result.set(file, { names, qualified: collectQualifiedUsages(text) });
  }
  consumersCache.set(hotspot.path, result);
  return result;
}

/** `Type::member` usages in a text (assoc-item/variant usage evidence). */
function collectQualifiedUsages(text) {
  const out = new Set();
  for (const m of text.matchAll(/\b([A-Z][A-Za-z_0-9]*)::([A-Za-z_][A-Za-z_0-9]*)/g)) {
    out.add(`${m[1]}::${m[2]}`);
  }
  return out;
}

/**
 * Derives the exact population of files referencing a narrowed test hook
 * outside its owning crate: files whose comment-stripped text matches at
 * least one recorded reference form (`Scheduler::test_x`,
 * `.test_x(` — qualified references, so a same-named local fn in an
 * unrelated crate is never a false consumer).
 */
export function deriveHookConsumers(hotspot, referenceForms) {
  const crateDir = owningCrateDir(hotspot.path);
  const out = new Set();
  for (const file of rustFilesUnderCrates()) {
    if (file.startsWith(`${crateDir}/`)) continue; // owning crate stays on cfg(test)/test-support
    const text = strippedFileText(file);
    if (referenceForms.some((form) => text.includes(form))) out.add(file);
  }
  return out;
}

// ---------------------------------------------------------------------------
// Struct/impl/enum parsing substrate (declaration evidence for fields,
// assoc members and constructor populations).
// ---------------------------------------------------------------------------

/** Body of the balanced `{...}` block opened at openIdx (string/lifetime
 * aware via matchBracket). */
function braceSlice(text, openIdx) {
  const close = matchBracket(text, openIdx, "{", "}");
  return close === -1 ? "" : text.slice(openIdx + 1, close);
}

const structDeclsCache = new Map();

/** Struct declarations of one text (comment-stripped): array of
 * { name, isPub, fields: Set<every declared field identifier>,
 *   pubFields: Set<fully-pub field identifiers> }. A `pub` field of a
 * pub(crate)/private struct never reaches cross-crate visibility, so
 * pubFields only count inside fully-pub structs. */
function structFieldDecls(text) {
  const out = [];
  for (const m of text.matchAll(
    /\b(pub(?:\s*\([a-z_]+\))?|)\s*struct\s+([A-Za-z_][A-Za-z_0-9]*)\s*(?:<[^>]*>)?\s*\{/g,
  )) {
    const body = braceSlice(text, m.index + m[0].length - 1);
    const fields = new Set();
    const pubFields = new Set();
    for (const fm of body.matchAll(
      /(?:^|\n)\s*(pub(?:\s*\([a-z_]+\))?\s+)?([a-z_][a-z_0-9]*)\s*:\s*[^\s:]/g,
    )) {
      fields.add(fm[2]);
      if (fm[1] !== undefined && fm[1].trim() === "pub") pubFields.add(fm[2]);
    }
    out.push({ name: m[2], isPub: m[1].trim() === "pub", fields, pubFields });
  }
  return out;
}

function fileStructDecls(rel) {
  let t = structDeclsCache.get(rel);
  if (t === undefined) {
    t = structFieldDecls(strippedFileText(rel));
    structDeclsCache.set(rel, t);
  }
  return t;
}

const fieldDeclaringStructsCache = new Map();

/** How many structs under crates/ declare a field with this name. A name
 * declared by exactly one struct is unambiguous: a bare `.field` access
 * outside the owning crate can only be that field. */
function fieldDeclaringStructCount(field) {
  let n = fieldDeclaringStructsCache.get(field);
  if (n !== undefined) return n;
  n = 0;
  for (const file of rustFilesUnderCrates()) {
    for (const s of fileStructDecls(file)) {
      if (s.fields.has(field)) n++;
    }
  }
  fieldDeclaringStructsCache.set(field, n);
  return n;
}

/**
 * Exact cross-crate consumer population of a narrowed FIELD: files outside
 * the owning crate that use the field through a type-qualified form — a
 * struct literal of the owning type naming the field, plus bare `.field`
 * receiver accesses only when the field name is unambiguous repo-wide (a
 * `tombstones` field of an unrelated type is never the owning type's
 * consumer). Purely for tests: form matching on arbitrary text.
 */
export function fieldUseForms(text, ownerType, field, unambiguous) {
  const literal = new RegExp(`\\b${ownerType}\\s*\\{[\\s\\S]{0,400}?\\b${field}\\s*:`).test(text);
  const receiver = unambiguous && new RegExp(`\\.${field}\\b`).test(text);
  return { literal, receiver, any: literal || receiver };
}

export function deriveFieldConsumers(hotspot, field) {
  const crateDir = owningCrateDir(hotspot.path);
  const ownerType = fileStructDecls(hotspot.path).find((s) => s.fields.has(field))?.name;
  if (!ownerType) return { ownerType: null, consumers: new Set() };
  const unambiguous = fieldDeclaringStructCount(field) === 1;
  const out = new Set();
  for (const file of rustFilesUnderCrates()) {
    if (file.startsWith(`${crateDir}/`)) continue;
    if (fieldUseForms(strippedFileText(file), ownerType, field, unambiguous).any) out.add(file);
  }
  return { ownerType, consumers: out };
}

/** Pub assoc members declared on retained types (impl-block parse of the
 * hotspot text): Set of `T::member` for pub fns and pub consts. */
function retainedTypeMembers(stripped, retainedTypes) {
  const out = new Set();
  for (const m of stripped.matchAll(/\bimpl(?:<[^{]*>)?\s*([A-Za-z_][A-Za-z_0-9]*)\s*\{/g)) {
    if (!retainedTypes.has(m[1])) continue;
    const body = braceSlice(stripped, m.index + m[0].length - 1);
    for (const fm of body.matchAll(/\bpub\s+(?:const\s+)?fn\s+([a-z_][a-z_0-9]+)/g)) {
      out.add(`${m[1]}::${fm[1]}`);
    }
    for (const fm of body.matchAll(/\bpub\s+const\s+([A-Z_][A-Z_0-9]+)\s*:/g)) {
      out.add(`${m[1]}::${fm[1]}`);
    }
  }
  return out;
}

/** Retained-typed enum variant payloads of the hotspot text: Map
 * `Enum::Variant` -> payload type (the last CamelCase word of the payload),
 * for every pub enum on a retained type whose payload is itself retained.
 * A pattern binding `Enum::Variant(ident)` types `ident` as the payload. */
function retainedVariantPayloads(stripped, retainedTypes) {
  const out = new Map();
  for (const m of stripped.matchAll(/\bpub\s+enum\s+([A-Za-z_][A-Za-z_0-9]*)\s*\{/g)) {
    if (!retainedTypes.has(m[1])) continue;
    const body = braceSlice(stripped, m.index + m[0].length - 1);
    for (const vm of body.matchAll(/\b([A-Z][A-Za-z_0-9]*)\s*\(([^)]*)\)/g)) {
      const payload = vm[2].match(/([A-Z][A-Za-z_0-9]*)\s*[),\s]*$/);
      if (payload && retainedTypes.has(payload[1])) out.set(`${m[1]}::${vm[1]}`, payload[1]);
    }
  }
  return out;
}

/**
 * Assoc-item usage evidence on retained types from one consumer file's
 * comment-stripped text: qualified `T::member` references, receiver calls
 * on identifiers whose type is evidenced (an ascription `x: &T` or a
 * pattern binding `Enum::Variant(x)` with a retained payload type), and
 * receiver calls within a small line window of a qualified `T::assoc`
 * reference (a chain like `if x.is_empty() { T::PROPAGATED } else { x
 * }.iter()` types the call through the qualified reference without a
 * binding). Members are filtered to pub assoc members declared on
 * retained types, so a same-named method on Vec/String never manufactures
 * an obligation.
 */
export function collectAssocUsage(text, retainedTypes, members, variantPayloads) {
  const usage = new Set();
  for (const m of text.matchAll(/\b([A-Z][A-Za-z_0-9]*)::([A-Za-z_][A-Za-z_0-9]*)/g)) {
    if (members.has(`${m[1]}::${m[2]}`)) usage.add(`${m[1]}::${m[2]}`);
  }
  const typedIdents = new Map(); // ident -> Set<type>
  const addTyped = (ident, type) => {
    if (!typedIdents.has(ident)) typedIdents.set(ident, new Set());
    typedIdents.get(ident).add(type);
  };
  for (const m of text.matchAll(
    /\b([a-z_][a-z_0-9]*)\s*:\s*(?:&(?:\s+mut\s+)?|mut\s+)*([A-Z][A-Za-z_0-9]*)\b/g,
  )) {
    if (retainedTypes.has(m[2])) addTyped(m[1], m[2]);
  }
  for (const [variant, payload] of variantPayloads) {
    const re = new RegExp(
      `${variant.split("::").join("\\:\\:")}\\s*\\(\\s*(?:ref\\s+)?(?:mut\\s+)?([a-z_][a-z_0-9]*)\\s*\\)`,
      "g",
    );
    for (const m of text.matchAll(re)) addTyped(m[1], payload);
  }
  const lines = text.split("\n");
  const qualLines = [];
  lines.forEach((line, i) => {
    for (const m of line.matchAll(/\b([A-Z][A-Za-z_0-9]*)::([A-Za-z_][A-Za-z_0-9]*)/g)) {
      if (members.has(`${m[1]}::${m[2]}`)) qualLines.push({ line: i, type: m[1] });
    }
  });
  lines.forEach((line, i) => {
    for (const m of line.matchAll(/\.([a-z_][a-z_0-9]*)\s*\(/g)) {
      const seen = new Set();
      for (const q of qualLines) {
        if (Math.abs(q.line - i) <= RECEIVER_EVIDENCE_LINE_WINDOW) seen.add(q.type);
      }
      for (const t of seen) {
        if (members.has(`${t}::${m[1]}`)) usage.add(`${t}::${m[1]}`);
      }
    }
  });
  for (const [ident, types] of typedIdents) {
    for (const m of text.matchAll(
      new RegExp(`\\b${ident}\\s*\\.\\s*([a-z_][a-z_0-9]*)\\s*\\(`, "g"),
    )) {
      for (const t of types) {
        if (members.has(`${t}::${m[1]}`)) usage.add(`${t}::${m[1]}`);
      }
    }
  }
  return usage;
}

/**
 * The parent module file that must declare the hotspot module (the dir's
 * mod.rs, else its lib.rs) and the verbatim declaration line it carries.
 */
function moduleDeclarationBinding(hotspotPath) {
  const base = hotspotPath.replace(/^.*\//, "").replace(/\.rs$/, "");
  const dir = hotspotPath.slice(0, hotspotPath.lastIndexOf("/"));
  for (const parent of [`${dir}/mod.rs`, `${dir}/lib.rs`]) {
    if (!existsRel(parent)) continue;
    const m = readRel(parent).match(
      new RegExp(`^\\s*(?:pub(?:\\s*\\([a-z_]+\\))?\\s+)?mod\\s+${base}\\s*;`, "m"),
    );
    return { parent, declaration: m ? m[0].trim() : null, base };
  }
  return { parent: null, declaration: null, base };
}

/**
 * ARH1-AC1: minimal public surfaces bind the live tree. The module's
 * visibility declaration is pinned verbatim against the DERIVED parent
 * module file (an emptied or redirected surfaceDeclarations list is
 * drift); a fully public module's complete pub-item population must be
 * retained, narrowed or carried by a bulk row (a deleted row is drift),
 * while a crate-visible module carries no cross-crate retained surface at
 * all. Retained items are real declarations, and narrowing rows target a
 * legal visibility with an EXACT consumer inventory: fn rows by qualified
 * reference forms, field rows by type-qualified field use (an ambiguous
 * field name counts only through a struct literal of the owning type), and
 * — for every hotspot — the importers list is the complete derived
 * cross-crate consumer population compared in both directions (comment
 * mentions never count). Where the contract flags the importer list as the
 * complete population for the retained surface, every referenced name must
 * be retained or carried by the bulk row's consumer migration, and every
 * assoc item used on a retained type — qualified, receiver or
 * binding-evidenced — must be retained (a retained type alone does not
 * retain its methods).
 */
function validateSurface(contracts, errors) {
  const caseId = "ARH1-surface";
  for (const hotspot of contracts.hotspots) {
    const text = readRel(hotspot.path);
    const stripped = strippedFileText(hotspot.path);
    const surface = hotspot.minimalPublicSurface;
    const moduleBase = path.basename(hotspot.path, ".rs");

    // The surface boundary is pinned to the derived parent declaration.
    const binding = moduleDeclarationBinding(hotspot.path);
    if (binding.parent === null || binding.declaration === null) {
      errors.push({
        caseId,
        code: "missing-surface-declaration",
        detail: `${hotspot.path}: no parent module file declares "mod ${moduleBase};"`,
      });
    } else {
      const pinned = hotspot.surfaceDeclarations.filter(
        (d) => d.file === binding.parent && d.declaration === binding.declaration,
      );
      if (pinned.length !== 1) {
        errors.push({
          caseId,
          code: "missing-surface-declaration",
          detail: `${hotspot.path}: the ${binding.parent} declaration "${binding.declaration}" must be pinned exactly once (found ${pinned.length})`,
        });
      }
      for (const d of hotspot.surfaceDeclarations) {
        if (d.file !== binding.parent || d.declaration !== binding.declaration) {
          errors.push({
            caseId,
            code: "surface-declaration-unpinned",
            detail: `${hotspot.path}: "${d.declaration}" @ ${d.file} is not the derived module declaration of ${binding.parent}`,
          });
        }
      }
    }
    const modulePublic = binding.declaration !== null && /^pub\s+mod\b/.test(binding.declaration);
    if (!modulePublic) {
      const leaked = [
        ...(surface.retainedFns || []).map((f) => `fn ${f}`),
        ...(surface.retainedTypes || []).map((t) => `type ${t}`),
        ...(surface.retainedAssocItems || []).map((a) => `assoc ${a}`),
      ];
      if (leaked.length > 0) {
        errors.push({
          caseId,
          code: "surface-boundary-drift",
          detail: `${hotspot.path}: the module is crate-visible; a cross-crate retained surface (${leaked.join(", ")}) contradicts the boundary`,
        });
      }
    }

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
    for (const fn of surface.retainedFns || []) {
      if (!new RegExp(`(?:pub|pub\\(crate\\)) (?:const )?fn ${fn}\\s*[<(]`).test(text)) {
        errors.push({ caseId, code: "surface-item-missing", detail: `${hotspot.path}: fn ${fn}` });
      }
    }
    for (const type of surface.retainedTypes || []) {
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

    // The importers list is the exact cross-crate consumer population for
    // EVERY hotspot, both directions, comments stripped.
    const population = deriveModuleConsumers(hotspot);
    const importers = hotspot.allowedImportDirection.importers || [];
    for (const file of population.keys()) {
      if (!importers.includes(file)) {
        errors.push({
          caseId,
          code: "importer-population-drift",
          detail: `${file} references ${moduleBase} cross-crate but is not a declared importer`,
        });
      }
    }
    for (const importer of importers) {
      if (!population.has(importer)) {
        errors.push({
          caseId,
          code: "importer-population-drift",
          detail: `${importer} is declared an importer but references no ${moduleBase} item in code`,
        });
      }
    }

    const bulkRows = (surface.narrow || []).filter((r) => r.kind === "bulk");
    if (modulePublic && bulkRows.length === 0) {
      // Without a bulk row, every pub item of the module must be retained
      // or narrowed explicitly: deleting a row uncovers the item.
      const coveredFns = new Set([
        ...(surface.retainedFns || []),
        ...(hotspot.constructorCapabilities || []).map((c) => c.constructor),
        ...(surface.narrow || []).filter((r) => r.kind === "fn").map((r) => r.item),
      ]);
      for (const m of stripped.matchAll(/(?:^|\n)\s*pub\s+(?:const\s+)?fn\s+([a-z_][a-z_0-9]+)/g)) {
        if (!coveredFns.has(m[1])) {
          errors.push({
            caseId,
            code: "surface-item-uncovered",
            detail: `${hotspot.path}: pub fn ${m[1]} is neither retained, narrowed nor carried by a bulk row`,
          });
        }
      }
      const coveredTypes = new Set([...(surface.retainedTypes || [])]);
      for (const m of stripped.matchAll(
        /(?:^|\n)\s*pub\s+(?:struct|enum|trait|type|union|const|static)\s+([A-Za-z_][A-Za-z_0-9]*)/g,
      )) {
        if (!coveredTypes.has(m[1])) {
          errors.push({
            caseId,
            code: "surface-item-uncovered",
            detail: `${hotspot.path}: pub item ${m[1]} is neither retained nor carried by a bulk row`,
          });
        }
      }
      const retainedT = new Set([...(surface.retainedTypes || [])]);
      const narrowFields = new Set(
        (surface.narrow || []).filter((r) => r.kind === "field").map((r) => r.item),
      );
      for (const s of structFieldDecls(stripped)) {
        if (!s.isPub) continue; // a pub(crate) struct's fields never cross the crate
        for (const field of s.pubFields) {
          if (!retainedT.has(s.name) && !narrowFields.has(field)) {
            errors.push({
              caseId,
              code: "surface-item-uncovered",
              detail: `${hotspot.path}: pub field ${s.name}.${field} is neither part of a retained type nor narrowed`,
            });
          }
        }
      }
    }

    const verifyConsumed = surface.crossCrateRetainedVerifiedByConsumers === true;
    if (modulePublic && verifyConsumed) {
      validateExactConsumerPopulation(hotspot, text, bulkRows, population, errors);
    }
    for (const row of surface.narrow || []) {
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
      const consumers = row.consumersAffected || [];
      if (row.kind === "fn") {
        const forms = row.referenceForms;
        if (
          !Array.isArray(forms) ||
          forms.length === 0 ||
          forms.some((f) => typeof f !== "string" || f.length === 0)
        ) {
          errors.push({
            caseId,
            code: "narrow-hook-without-reference-forms",
            detail: `${hotspot.path}: ${row.item} records no qualified reference forms`,
          });
        } else {
          for (const consumer of consumers) {
            if (!existsRel(consumer)) {
              errors.push({
                caseId,
                code: "missing-narrow-consumer",
                detail: `${row.item}: ${consumer}`,
              });
              continue;
            }
            if (!forms.some((form) => strippedFileText(consumer).includes(form))) {
              errors.push({
                caseId,
                code: "narrow-consumer-without-reference",
                detail: `${row.item}: ${consumer} references no recorded form of the hook`,
              });
            }
          }
          for (const live of deriveHookConsumers(hotspot, forms)) {
            if (!consumers.includes(live)) {
              errors.push({
                caseId,
                code: "narrow-consumer-omitted",
                detail: `${row.item}: ${live} references the hook outside the owning crate but is not recorded`,
              });
            }
          }
        }
      } else if (row.kind === "field") {
        // Exact field-consumer join: type-qualified use only. An ambiguous
        // field name (declared by more than one struct repo-wide) counts
        // only through a struct literal of the owning type, so a same-named
        // field of an unrelated type is never a false consumer.
        const derived = deriveFieldConsumers(hotspot, row.item);
        if (derived.ownerType === null) {
          errors.push({
            caseId,
            code: "narrow-field-without-owner-type",
            detail: `${hotspot.path}: no struct declares field ${row.item}; the narrowing has no owning type`,
          });
        } else {
          const unambiguous = fieldDeclaringStructCount(row.item) === 1;
          for (const consumer of consumers) {
            if (!existsRel(consumer)) {
              errors.push({
                caseId,
                code: "missing-narrow-consumer",
                detail: `${row.item}: ${consumer}`,
              });
              continue;
            }
            if (
              !fieldUseForms(strippedFileText(consumer), derived.ownerType, row.item, unambiguous)
                .any
            ) {
              errors.push({
                caseId,
                code: "narrow-consumer-without-reference",
                detail: `${row.item}: ${consumer} uses no type-qualified form of ${derived.ownerType}.${row.item}`,
              });
            }
          }
          for (const live of derived.consumers) {
            if (!consumers.includes(live)) {
              errors.push({
                caseId,
                code: "narrow-consumer-omitted",
                detail: `${row.item}: ${live} uses ${derived.ownerType}.${row.item} outside the owning crate but is not recorded`,
              });
            }
          }
        }
      } else {
        for (const consumer of consumers) {
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
}

/**
 * The complete cross-crate population of a flagged hotspot module: every
 * referenced name must be retained or migrate with the bulk row; retained
 * types must be imported by a real consumer; assoc items used on retained
 * types — qualified (`T::member`), receiver calls on typed bindings and
 * calls within a line window of a qualified reference — must be retained.
 * (The importer population join itself runs for every hotspot in
 * validateSurface.)
 */
function validateExactConsumerPopulation(hotspot, text, bulkRows, population, errors) {
  const caseId = "ARH1-surface";
  const surface = hotspot.minimalPublicSurface;
  const retained = new Set(surface.retainedTypes || []);
  const assoc = new Set(surface.retainedAssocItems || []);
  const members = retainedTypeMembers(strippedFileText(hotspot.path), retained);
  const variantPayloads = retainedVariantPayloads(strippedFileText(hotspot.path), retained);
  const bulk = bulkRows[0];
  const bulkConsumers = new Set(bulk ? bulk.consumersAffected || [] : []);
  for (const [file, { names }] of population) {
    const unretained = [...names].filter((n) => !retained.has(n));
    if (unretained.length > 0 && !bulkConsumers.has(file)) {
      errors.push({
        caseId,
        code: "narrowed-item-consumer-unrecorded",
        detail: `${file} imports narrowed item(s) ${unretained.join(", ")} but no bulk row records its migration`,
      });
    }
    if (!bulkConsumers.has(file)) {
      // Assoc-item usage on retained types must stay retained for every
      // consumer that does not migrate with the bulk row — including
      // receiver-syntax calls (`reasons.is_empty()`, `.iter()` chains).
      for (const usage of collectAssocUsage(
        strippedFileText(file),
        retained,
        members,
        variantPayloads,
      )) {
        if (!assoc.has(usage)) {
          errors.push({
            caseId,
            code: "assoc-item-unretained",
            detail: `${file} uses ${usage} on a retained type; the assoc item is neither retained nor migrated`,
          });
        }
      }
    }
  }
  for (const consumer of bulkConsumers) {
    const info = population.get(consumer);
    if (!info || ![...info.names].some((n) => !retained.has(n))) {
      errors.push({
        caseId,
        code: "narrow-consumer-without-reference",
        detail: `${consumer} is recorded as a bulk-narrowing consumer but imports no narrowed item`,
      });
    }
  }
  for (const type of retained) {
    let consumed = false;
    for (const { names } of population.values()) {
      if (names.has(type)) {
        consumed = true;
        break;
      }
    }
    if (!consumed) {
      errors.push({
        caseId,
        code: "retained-item-unconsumed",
        detail: `${hotspot.path}: retained type ${type} is imported by no cross-crate consumer`,
      });
    }
  }
  for (const item of assoc) {
    const [type, member] = item.split("::");
    if (!retained.has(type)) {
      errors.push({
        caseId,
        code: "assoc-item-unretained-type",
        detail: `${item} is retained but its type is not a retained type`,
      });
      continue;
    }
    const declared = new RegExp(
      `pub(?:\\s+const)?\\s+fn ${member}\\s*[<(]|pub\\s+const\\s+${member}\\s*:`,
    ).test(text);
    if (!declared) {
      errors.push({
        caseId,
        code: "assoc-item-missing",
        detail: `${hotspot.path}: ${item} is not a declared pub member`,
      });
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
 * disposition. Routes are keyed — an ARH0 debt decision by its debt id, a
 * narrowing decision by (candidatePath, narrow kind) — so two decisions on
 * one route (duplicate dispositions, competing owners) fail, a route whose
 * only row lost the executing heir fails, and a hotspot that declares two
 * narrowing kinds (field narrowing vs test-hook gating vs bulk) needs one
 * register row per kind: deleting one row must uncover its own route, not
 * hide behind a sibling that shares the file.
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
  // Every narrowing kind a hotspot declares is a route key; register rows
  // for narrowing decisions must name one.
  const routeKindsByPath = new Map();
  for (const hotspot of contracts.hotspots) {
    const kinds = [
      ...new Set((hotspot.minimalPublicSurface.narrow || []).map((r) => r.kind)),
    ].sort();
    routeKindsByPath.set(hotspot.path, kinds);
  }
  const ids = new Set();
  const routes = new Set();
  const satisfies = new Set();
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
    // Every disposition binds EXACTLY ONE recognized key: a narrowing route
    // (path#kind) or a satisfied ARH0 debt. A row with neither key rides
    // outside every uniqueness check — the register cannot carry a second
    // authority for a candidate by leaving the key fields unset.
    const keyCount = (row.satisfies ? 1 : 0) + (row.route !== undefined ? 1 : 0);
    if (keyCount === 0) {
      errors.push({
        caseId,
        code: "cutover-row-unbound",
        detail: `${row.id}: carries neither a narrowing route nor a satisfied ARH0 debt; every disposition must bind exactly one key`,
      });
    } else if (keyCount === 2) {
      errors.push({
        caseId,
        code: "cutover-row-double-keyed",
        detail: `${row.id}: carries both route ${row.route} and satisfies ${row.satisfies}; a disposition binds exactly one key`,
      });
    }
    if (row.satisfies) {
      if (satisfies.has(row.satisfies)) {
        errors.push({
          caseId,
          code: "duplicate-satisfies",
          detail: `${row.id}: ARH0 debt ${row.satisfies} is decided twice`,
        });
      }
      satisfies.add(row.satisfies);
    }
    if (row.route !== undefined) {
      if (routes.has(row.route)) {
        errors.push({
          caseId,
          code: "duplicate-cutover-route",
          detail: `${row.id}: route ${row.route} already carries a disposition`,
        });
      }
      routes.add(row.route);
      const hashIdx = row.route.lastIndexOf("#");
      const routePath = hashIdx === -1 ? row.route : row.route.slice(0, hashIdx);
      const routeKind = hashIdx === -1 ? undefined : row.route.slice(hashIdx + 1);
      const kinds = routeKindsByPath.get(routePath);
      if (!kinds || routeKind === undefined || !kinds.includes(routeKind)) {
        errors.push({
          caseId,
          code: "cutover-route-invented",
          detail: `${row.id}: route ${row.route} matches no declared narrowing kind`,
        });
      }
    }
  }
  // ARH0 debt rows that name ARH1 as decision owner must be decided here —
  // exactly once (duplicate-satisfies above pins the cardinality).
  for (const debt of arh0["debt-register"].rows) {
    if (debt.owner?.kind === "node" && debt.owner?.id === "ARH1" && !satisfies.has(debt.id)) {
      errors.push({
        caseId,
        code: "arh0-debt-undecided",
        detail: `${debt.id} names ARH1 as decision owner but no cutover row satisfies it`,
      });
    }
  }
  // Every narrowing route declared in the contracts has exactly one
  // register row binding it to the executing heir.
  for (const hotspot of contracts.hotspots) {
    const heir = hotspot.heirs?.narrowing;
    for (const kind of routeKindsByPath.get(hotspot.path)) {
      const route = `${hotspot.path}#${kind}`;
      const binding = cutover.rows.filter(
        (r) => r.route === route && heir && r.owner.id === heir.id && r.owner.kind === "node",
      );
      if (binding.length !== 1) {
        errors.push({
          caseId,
          code: "cutover-route-missing",
          detail: `${hotspot.path} declares ${kind} narrowing but ${binding.length} register rows bind route ${route} to heir ${heir ? heir.id : "(none)"}`,
        });
      }
    }
  }
}

/**
 * ARH1-AC1/AC4: the manifest documents this node's case contract, products,
 * commands and producer obligations; the verifier is the sole owning
 * interface that runs them, so prose cannot outlive the checks. The AC4
 * obligations (or their explicit N/A rationale) must name every surface the
 * charter lists for a contract-only node.
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

  const contracts = products["dependency-contracts"];
  for (const obligation of contracts.ac4Obligations || []) {
    resolveOwner(obligation.producer, errors, caseId, "obligation-producer-malformed");
    if (typeof obligation.obligation !== "string" || obligation.obligation.length === 0) {
      errors.push({
        caseId,
        code: "obligation-without-text",
        detail: `producer ${obligation.producer?.id ?? "(no id)"} records no obligation`,
      });
    }
  }
  const ac4Text = [
    ...(contracts.ac4Obligations || []).map((o) => o.obligation || ""),
    typeof contracts.ac4Rationale === "string" ? contracts.ac4Rationale : "",
  ].join("\n");
  for (const surface of AC4_SURFACES) {
    if (!ac4Text.includes(surface)) {
      errors.push({
        caseId,
        code: "ac4-surface-uncovered",
        detail: `AC4 surface "${surface}" is named by no producer obligation and no N/A rationale`,
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
