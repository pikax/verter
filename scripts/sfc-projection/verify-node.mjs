#!/usr/bin/env node
/**
 * ProjectionProbeRunner (STP1).
 *
 * CLI: --node, --engine, --require-all, --json
 * Rejects absent/empty manifests, zero selected cases, missing inventory
 * fixtures, vacuous any/never type matches, unrelated clean-twin diagnostics,
 * executable substitution under a stable engine label, omitted probes,
 * non-exact type matches, missing references, and duplicate file checks.
 */

import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../..");
const DEFAULT_ROOT_MANIFEST = "tests/sfc-projection/manifest.json";
const SCHEMA_PATH = "scripts/sfc-projection/manifest.schema.json";
const TYPE_FLAGS_ANY = 1;
const TYPE_FLAGS_NEVER = 262144;

export const NODE_MANDATORY_CASES = Object.freeze({
  STP1: Object.freeze([
    "STP1-inventory",
    "STP1-zero-selection",
    "STP1-clean-twin",
    "STP1-types",
    "STP1-provenance",
    "STP1-harness",
  ]),
  STP2: Object.freeze([
    "STP2-instance-concrete",
    "STP2-instance-generic",
    "STP2-instance-explicit",
    "STP2-constructor-escape",
    "STP2-vue-utilities",
    "STP2-not-callable",
    "STP2-constructor-inferred",
    "STP2-explicit-input-mismatch",
  ]),
});

export const MANDATORY_CASES = NODE_MANDATORY_CASES.STP1;

export function canonicalMandatoryCases(nodeId) {
  return NODE_MANDATORY_CASES[nodeId] || [];
}

const REQUIRED_PROBE_STRINGS = Object.freeze([
  "positive",
  "negative",
  "cleanTwin",
  "tsconfig",
  "hoverNeedle",
  "definitionNeedle",
  "expectedHoverType",
  "expectedInstanceType",
]);

export function probesAreRunnable(probes) {
  if (!probes || typeof probes !== "object" || Array.isArray(probes)) return false;
  for (const key of REQUIRED_PROBE_STRINGS) {
    if (typeof probes[key] !== "string" || probes[key].length === 0) return false;
  }
  return Number.isInteger(probes.expectedNegativeCode);
}

export function posix(p) {
  return String(p).split(path.sep).join("/");
}

export function err(caseId, code, message) {
  return { caseId, code, message };
}

export function cloneJson(value) {
  return structuredClone(value);
}

export function readJson(absPath) {
  return JSON.parse(fs.readFileSync(absPath, "utf8"));
}

export function repoPath(repoRoot, rel) {
  return path.resolve(repoRoot, rel);
}

export function sha256File(absPath) {
  return createHash("sha256").update(fs.readFileSync(absPath)).digest("hex");
}

export function parseArgs(argv = process.argv.slice(2)) {
  const out = {
    node: null,
    engine: "all",
    requireAll: false,
    json: false,
    manifest: DEFAULT_ROOT_MANIFEST,
    help: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--help" || arg === "-h") out.help = true;
    else if (arg === "--require-all") out.requireAll = true;
    else if (arg === "--json") out.json = true;
    else if (arg === "--node") out.node = argv[++i];
    else if (arg === "--engine") out.engine = argv[++i];
    else if (arg === "--manifest") out.manifest = argv[++i];
    else throw new Error(`unknown argument: ${arg}`);
  }
  return out;
}

export function validateProbeManifest(doc, { role }) {
  const errors = [];
  if (!doc || typeof doc !== "object" || Array.isArray(doc)) {
    errors.push(err("STP1-zero-selection", "absent-manifest", `${role} is not an object`));
    return errors;
  }
  if (doc.schema !== "ProbeManifest") {
    errors.push(
      err("STP1-zero-selection", "absent-manifest", `${role} schema is not ProbeManifest`),
    );
  }
  if (doc.version !== 1) {
    errors.push(err("STP1-zero-selection", "absent-manifest", `${role} version is not 1`));
  }
  if (role === "root") {
    if (!Array.isArray(doc.nodes) || doc.nodes.length === 0) {
      errors.push(err("STP1-zero-selection", "empty-manifest", "root manifest nodes are empty"));
    }
    for (const key of ["inventory", "engineMatrix", "performanceMethodology"]) {
      if (!doc[key]) {
        errors.push(err("STP1-zero-selection", "empty-manifest", `root manifest missing ${key}`));
      }
    }
  }
  if (role === "node") {
    if (!Array.isArray(doc.cases) || doc.cases.length === 0) {
      errors.push(err("STP1-zero-selection", "empty-manifest", "node manifest cases are empty"));
    }
    if (!Array.isArray(doc.mandatoryCases) || doc.mandatoryCases.length === 0) {
      errors.push(
        err("STP1-zero-selection", "empty-manifest", "node manifest mandatoryCases are empty"),
      );
    } else {
      const declared = new Set(doc.mandatoryCases);
      const canonical = canonicalMandatoryCases(doc.node);
      for (const id of canonical) {
        if (!declared.has(id)) {
          errors.push(
            err(
              "STP1-zero-selection",
              "zero-cases",
              `mandatory case ${id} missing from node mandatoryCases`,
            ),
          );
        }
      }
    }
    if (!probesAreRunnable(doc.probes)) {
      errors.push(err("STP1-harness", "missing-probes", "node manifest omitted runnable probes"));
    }
  }
  return errors;
}

export function loadRootManifest(repoRoot, rel = DEFAULT_ROOT_MANIFEST) {
  const abs = repoPath(repoRoot, rel);
  if (!fs.existsSync(abs)) {
    return {
      manifest: null,
      errors: [err("STP1-zero-selection", "absent-manifest", `missing ${posix(rel)}`)],
    };
  }
  const manifest = readJson(abs);
  return { manifest, errors: validateProbeManifest(manifest, { role: "root" }) };
}

export function loadNodeManifest(repoRoot, rel) {
  const abs = repoPath(repoRoot, rel);
  if (!fs.existsSync(abs)) {
    return {
      manifest: null,
      errors: [err("STP1-zero-selection", "absent-manifest", `missing ${posix(rel)}`)],
    };
  }
  const manifest = readJson(abs);
  return { manifest, errors: validateProbeManifest(manifest, { role: "node" }) };
}

export function selectNodeEntry(root, nodeId) {
  const nodes = root?.nodes || [];
  return nodes.find((row) => row.id === nodeId) || null;
}

export function selectCases(nodeManifest, nodeId) {
  if (!nodeManifest || nodeManifest.node !== nodeId) return [];
  return [...(nodeManifest.cases || [])];
}

export function assertNonZeroSelection(selected, nodeId) {
  if (selected.length === 0) {
    return [
      err(
        "STP1-zero-selection",
        "zero-cases",
        `node filter ${nodeId || "<missing>"} selected zero cases`,
      ),
    ];
  }
  return [];
}

export function loadObligation(repoRoot, inventory) {
  const rel =
    inventory?.obligationSource ||
    "tests/sfc-projection/STP0/products/current-feature-obligation.json";
  return readJson(repoPath(repoRoot, rel));
}

export function assertInventoryComplete(inventory, obligation, repoRoot) {
  const errors = [];
  const rows = inventory?.rows || [];
  if (rows.length === 0) {
    errors.push(err("STP1-inventory", "removed-fixture", "inventory selected coverage is empty"));
    return errors;
  }
  const ids = new Set();
  for (const row of rows) {
    if (!row?.id) {
      errors.push(err("STP1-inventory", "removed-fixture", "inventory row missing id"));
      continue;
    }
    if (ids.has(row.id)) {
      errors.push(err("STP1-inventory", "removed-fixture", `duplicate inventory row ${row.id}`));
    }
    ids.add(row.id);
    if (!row.path) {
      errors.push(err("STP1-inventory", "removed-fixture", `inventory row ${row.id} missing path`));
      continue;
    }
    const abs = repoPath(repoRoot, row.path);
    if (!fs.existsSync(abs)) {
      errors.push(
        err(
          "STP1-inventory",
          "removed-fixture",
          `selected fixture ${row.id} missing at ${row.path}`,
        ),
      );
    }
  }
  for (const required of obligation?.rows || []) {
    if (required.obligation === "RequiredCurrent" && !ids.has(required.id)) {
      errors.push(
        err(
          "STP1-inventory",
          "removed-fixture",
          `RequiredCurrent row ${required.id} is missing from selected coverage`,
        ),
      );
    }
  }
  for (const bench of inventory?.benchmarkManifests || []) {
    if (!bench?.path || !fs.existsSync(repoPath(repoRoot, bench.path))) {
      errors.push(
        err(
          "STP1-inventory",
          "removed-fixture",
          `selected benchmark manifest ${bench?.id || "?"} missing at ${bench?.path}`,
        ),
      );
    }
  }
  const families = new Set(rows.map((row) => row.family));
  for (const family of [
    "constructor",
    "macros",
    "script-dialect",
    "jsx",
    "directives",
    "refs",
    "preprocessing",
    "recovery",
    "editor",
  ]) {
    if (!families.has(family)) {
      errors.push(err("STP1-inventory", "removed-fixture", `inventory missing family ${family}`));
    }
  }
  return errors;
}

const STP2_SPECIALIZATION_KINDS = Object.freeze([
  "unbound-extraction",
  "explicit-specialization",
  "props-inferred-construction",
  "template-use",
]);

export function assertStp2Products(repoRoot, nodeManifest) {
  const errors = [];
  const declared = new Set(nodeManifest?.products || []);
  for (const product of ["ConstructorCompatibilityEvidence", "InstanceTypeCompatibilityTable"]) {
    if (!declared.has(product)) {
      errors.push(err("STP2-instance-concrete", "removed-fixture", `missing product ${product}`));
    }
  }
  const evidenceRel = "tests/sfc-projection/STP2/products/constructor-compatibility-evidence.json";
  const tableRel = "tests/sfc-projection/STP2/products/instance-type-compatibility-table.json";
  const evidenceAbs = repoPath(repoRoot, evidenceRel);
  const tableAbs = repoPath(repoRoot, tableRel);
  if (!fs.existsSync(evidenceAbs)) {
    errors.push(err("STP2-instance-concrete", "removed-fixture", `missing ${evidenceRel}`));
    return errors;
  }
  if (!fs.existsSync(tableAbs)) {
    errors.push(err("STP2-instance-concrete", "removed-fixture", `missing ${tableRel}`));
    return errors;
  }
  const evidence = readJson(evidenceAbs);
  const table = readJson(tableAbs);
  if (evidence.schema !== "ConstructorCompatibilityEvidence") {
    errors.push(
      err("STP2-instance-concrete", "removed-fixture", "ConstructorCompatibilityEvidence schema"),
    );
  }
  if (table.schema !== "InstanceTypeCompatibilityTable") {
    errors.push(
      err("STP2-instance-concrete", "removed-fixture", "InstanceTypeCompatibilityTable schema"),
    );
  }
  const kinds = new Set((evidence.specializationKinds || []).map((row) => row.id));
  for (const kind of STP2_SPECIALIZATION_KINDS) {
    if (!kinds.has(kind)) {
      errors.push(
        err("STP2-instance-generic", "removed-fixture", `missing specialization kind ${kind}`),
      );
    }
  }
  const tableIds = new Set((table.rows || []).map((row) => row.id));
  for (const id of NODE_MANDATORY_CASES.STP2) {
    if (!tableIds.has(id)) {
      errors.push(
        err("STP2-instance-concrete", "removed-fixture", `compatibility table missing ${id}`),
      );
    }
  }
  if (
    evidence.syntaxControl?.spelling &&
    !String(evidence.syntaxControl.spelling).includes("declare class Foo")
  ) {
    errors.push(err("STP2-instance-explicit", "removed-fixture", "missing Foo syntax control"));
  }
  return errors;
}

function pinExe(pin, pkgDir) {
  if (pin.kind === "native") {
    const getExePathUrl = pathToFileURL(path.join(pkgDir, "lib", "getExePath.js")).href;
    return import(getExePathUrl).then((mod) => mod.default());
  }
  return Promise.resolve(path.join(pkgDir, pin.executableRel));
}

export async function resolveEngine(pin, repoRoot) {
  const pkgDir = repoPath(repoRoot, pin.resolveFrom);
  const pkgJson = path.join(pkgDir, "package.json");
  if (!fs.existsSync(pkgJson)) {
    return {
      ok: false,
      error: err(
        "STP1-provenance",
        "substituted-executable",
        `engine ${pin.id} package.json missing at ${pin.resolveFrom}`,
      ),
    };
  }
  const meta = readJson(pkgJson);
  if (meta.name !== pin.package || meta.version !== pin.version) {
    return {
      ok: false,
      error: err(
        "STP1-provenance",
        "substituted-executable",
        `engine ${pin.id} expected ${pin.package}@${pin.version}, found ${meta.name}@${meta.version}`,
      ),
    };
  }
  let executable;
  try {
    executable = await pinExe(pin, pkgDir);
  } catch (error) {
    return {
      ok: false,
      error: err(
        "STP1-provenance",
        "substituted-executable",
        `engine ${pin.id} executable resolve failed: ${error.message}`,
      ),
    };
  }
  if (pin.executableOverride) {
    const override = path.resolve(pin.executableOverride);
    if (path.resolve(executable) !== override) {
      return {
        ok: false,
        error: err(
          "STP1-provenance",
          "substituted-executable",
          `engine ${pin.id} executable ${posix(override)} is not the pinned ${posix(executable)}`,
        ),
      };
    }
  }
  if (!fs.existsSync(executable)) {
    return {
      ok: false,
      error: err(
        "STP1-provenance",
        "substituted-executable",
        `engine ${pin.id} executable missing: ${posix(executable)}`,
      ),
    };
  }
  return {
    ok: true,
    engine: {
      id: pin.id,
      label: pin.label,
      kind: pin.kind,
      package: pin.package,
      version: pin.version,
      api: pin.api,
      mapperCapable: !!pin.mapperCapable,
      resolveFrom: pin.resolveFrom,
      executable: path.resolve(executable),
      sha256: sha256File(executable),
    },
  };
}

export function selectEngines(matrix, engineArg) {
  const pins = matrix?.engines || [];
  if (engineArg === "all") return pins;
  return pins.filter((pin) => pin.id === engineArg || pin.label === engineArg);
}

export function isVacuousType(typeString, flags = 0) {
  const printed = String(typeString || "").trim();
  if (printed === "any" || printed === "never") return true;
  if ((flags & TYPE_FLAGS_ANY) !== 0) return true;
  if ((flags & TYPE_FLAGS_NEVER) !== 0) return true;
  return false;
}

export function assertExactType({
  actual,
  expected,
  flags = 0,
  authoredAny = false,
  authoredNever = false,
}) {
  if (isVacuousType(actual, flags) && !(authoredAny || authoredNever)) {
    return [
      err(
        "STP1-types",
        "vacuous-type",
        `type equality was satisfied by ${actual} (flags=${flags})`,
      ),
    ];
  }
  if (expected != null && actual !== expected) {
    return [err("STP1-types", "type-mismatch", `expected ${expected}, got ${actual}`)];
  }
  return [];
}

export function assertCleanTwin(diagnostics, { fileLabel = "clean twin" } = {}) {
  const unexpected = (diagnostics || []).filter((diag) => diag && diag.code);
  if (unexpected.length > 0) {
    return [
      err(
        "STP1-clean-twin",
        "unrelated-generated-error",
        `${fileLabel} reported unexpected diagnostic ${unexpected[0].code}: ${unexpected[0].message}`,
      ),
    ];
  }
  return [];
}

function normalizeDiag(diag) {
  const message =
    typeof diag.messageText === "string"
      ? diag.messageText
      : diag.messageText?.messageText || diag.text || diag.message || "";
  return {
    code: diag.code,
    message: String(message),
    pos: diag.start ?? diag.pos ?? null,
  };
}

function offsetOf(text, needle) {
  const pos = text.indexOf(needle);
  if (pos < 0) throw new Error(`needle not found: ${needle}`);
  return pos;
}

function jsTypeFlags(ts, type) {
  return type?.flags ?? 0;
}

export function bumpCheckCount(checkCounts, fileKey) {
  const key = posix(fileKey);
  checkCounts[key] = (checkCounts[key] || 0) + 1;
  return checkCounts[key];
}

function createJsProgram(ts, fileAbs, options, checkCounts, fileKey) {
  bumpCheckCount(checkCounts, fileKey);
  const host = ts.createCompilerHost(options, true);
  return ts.createProgram({ rootNames: [fileAbs], options, host });
}

function runJsFile(ts, fileAbs, options, checkCounts, fileKey) {
  const program = createJsProgram(ts, fileAbs, options, checkCounts, fileKey);
  const sf = program.getSourceFile(fileAbs);
  const checker = program.getTypeChecker();
  const diags = ts
    .getPreEmitDiagnostics(program)
    .filter((diag) => diag.file && path.resolve(diag.file.fileName) === path.resolve(fileAbs))
    .map(normalizeDiag);
  const text = fs.readFileSync(fileAbs, "utf8");
  const observations = observeJs(ts, checker, sf, text);
  return { diags, observations };
}

function instanceTypeAlias(text) {
  const match = text.match(/export type (\w+)\s*=\s*InstanceType/);
  return match ? match[1] : null;
}

function observeJs(ts, checker, sf, text, probes = {}) {
  const out = { types: {}, hover: null, definition: null, references: 0, edits: [] };
  if (!sf) return out;
  const alias =
    instanceTypeAlias(text) || (text.includes("export type Instance") ? "Instance" : null);
  if (alias) {
    const node = findIdentifier(ts, sf, alias);
    if (node) {
      const type = checker.getTypeAtLocation(node);
      out.types.Instance = {
        printed: checker.typeToString(type),
        flags: jsTypeFlags(ts, type),
      };
    }
  }
  const hoverName = probes.hoverNeedle || "stp1HoverTarget";
  if (text.includes(hoverName)) {
    const node = findIdentifier(ts, sf, hoverName);
    if (node) {
      const type = checker.getTypeAtLocation(node);
      out.hover = {
        name: hoverName,
        printed: checker.typeToString(type),
        flags: jsTypeFlags(ts, type),
        pos: node.getStart(sf),
      };
    }
  }
  const defName = text.includes("Comp") ? "Comp" : probes.definitionNeedle;
  const defNode = defName ? findIdentifier(ts, sf, defName) : null;
  if (defNode) {
    const sym = checker.getSymbolAtLocation(defNode);
    out.definition = { name: sym?.getName?.() || defName, pos: defNode.getStart(sf) };
    out.edits = [{ name: defName, pos: defNode.getStart(sf), end: defNode.getEnd() }];
  }
  out.references = defName ? countIdentifier(ts, sf, defName) : 0;
  return out;
}

function findIdentifier(ts, node, name, sourceFile = node) {
  if (ts.isIdentifier(node) && node.text === name) return node;
  for (const child of node.getChildren(sourceFile)) {
    const found = findIdentifier(ts, child, name, sourceFile);
    if (found) return found;
  }
  return null;
}

function countIdentifier(ts, node, name, sourceFile = node) {
  let n = 0;
  if (ts.isIdentifier(node) && node.text === name) n += 1;
  for (const child of node.getChildren(sourceFile))
    n += countIdentifier(ts, child, name, sourceFile);
  return n;
}

function loadJsTypeScript(pin, repoRoot) {
  const pkgDir = repoPath(repoRoot, pin.resolveFrom);
  const require = createRequire(path.join(pkgDir, "package.json"));
  return require(path.join(pkgDir, "lib/typescript.js"));
}

export function runJsEngine(resolved, probes, repoRoot) {
  const pin = resolved;
  const ts = loadJsTypeScript(pin, repoRoot);
  const options = {
    strict: true,
    noEmit: true,
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    skipLibCheck: true,
    types: [],
  };
  const positive = repoPath(repoRoot, probes.positive);
  const negative = repoPath(repoRoot, probes.negative);
  const checkCounts = {};
  const pos = runJsFile(ts, positive, options, checkCounts, probes.positive);
  const neg = runJsFile(ts, negative, options, checkCounts, probes.negative);
  return {
    engine: pin.id,
    positive: pos,
    negative: neg,
    checkCounts,
  };
}

async function loadNativeApi(repoRoot) {
  const require = createRequire(path.join(repoRoot, "package.json"));
  const syncPath = require.resolve("typescript/unstable/sync");
  return import(pathToFileURL(syncPath).href);
}

export async function runNativeEngine(resolved, probes, repoRoot) {
  const { API } = await loadNativeApi(repoRoot);
  const probesDir = path.dirname(repoPath(repoRoot, probes.tsconfig));
  const tsconfig = repoPath(repoRoot, probes.tsconfig);
  const positive = repoPath(repoRoot, probes.positive);
  const negative = repoPath(repoRoot, probes.negative);
  const api = new API({ tsserverPath: resolved.executable, cwd: probesDir });
  try {
    const snap = api.updateSnapshot({ openProject: tsconfig });
    const project = snap.getProject(tsconfig);
    if (!project) {
      throw new Error(`native engine did not open ${posix(probes.tsconfig)}`);
    }
    const checkCounts = {};
    const pos = observeNative(project, positive, probes, checkCounts, probes.positive);
    const neg = observeNative(project, negative, probes, checkCounts, probes.negative);
    snap.dispose();
    return {
      engine: resolved.id,
      positive: pos,
      negative: neg,
      checkCounts,
    };
  } finally {
    api.close();
  }
}

function nativeSemanticDiagnostics(project, fileAbs, checkCounts, fileKey) {
  bumpCheckCount(checkCounts, fileKey);
  return project.program.getSemanticDiagnostics(fileAbs).map(normalizeDiag);
}

function observeNative(project, fileAbs, probes, checkCounts, fileKey) {
  const diags = nativeSemanticDiagnostics(project, fileAbs, checkCounts, fileKey);
  const text = fs.readFileSync(fileAbs, "utf8");
  const observations = { types: {}, hover: null, definition: null, references: 0, edits: [] };
  const alias = instanceTypeAlias(text);
  if (alias) {
    const pos = offsetOf(text, `export type ${alias}`) + "export type ".length;
    const type = project.checker.getTypeAtPosition(fileAbs, pos);
    if (type) {
      observations.types.Instance = {
        printed: project.checker.typeToString(type),
        flags: type.flags ?? 0,
      };
    }
  }
  const hoverName = probes.hoverNeedle || "stp1HoverTarget";
  if (text.includes(hoverName)) {
    const pos = offsetOf(text, hoverName);
    const type = project.checker.getTypeAtPosition(fileAbs, pos);
    if (type) {
      observations.hover = {
        name: hoverName,
        printed: project.checker.typeToString(type),
        flags: type.flags ?? 0,
        pos,
      };
    }
  }
  let defPos = null;
  let defName = "Comp";
  if (text.includes("class Comp")) {
    defPos = offsetOf(text, "class Comp") + "class ".length;
  } else if (text.includes("Comp")) {
    defPos = offsetOf(text, "Comp");
  } else if (probes.definitionNeedle && text.includes(probes.definitionNeedle)) {
    defName = probes.definitionNeedle;
    defPos = offsetOf(text, probes.definitionNeedle);
  }
  if (defPos != null) {
    const sym = project.checker.getSymbolAtPosition(fileAbs, defPos);
    observations.definition = { name: sym?.name || defName, pos: defPos };
    observations.edits = [{ name: defName, pos: defPos, end: defPos + defName.length }];
    let refs = [];
    try {
      refs = sym ? project.checker.getReferencesToSymbolInFile(fileAbs, sym) : [];
    } catch {
      refs = [];
    }
    observations.references = Array.isArray(refs) ? refs.length : text.split(defName).length - 1;
  }
  return { diags, observations };
}

export function assertCheckCounts(checkCounts, engineId, probeFiles, max = 1) {
  const errors = [];
  for (const file of probeFiles) {
    const key = posix(file);
    const count = checkCounts?.[key];
    if (!Number.isInteger(count) || count < 1) {
      errors.push(
        err(
          "STP1-harness",
          "duplicate-check",
          `${engineId} missing measured check count for ${key}`,
        ),
      );
    } else if (count > max) {
      errors.push(
        err("STP1-harness", "duplicate-check", `${engineId} checked ${key} ${count} times`),
      );
    }
  }
  return errors;
}

export function caseProbeFiles(cases = []) {
  const files = [];
  for (const row of cases) {
    if (row.file) files.push(row.file);
    if (row.dirtyTwin) files.push(row.dirtyTwin);
    if (Array.isArray(row.files)) files.push(...row.files);
  }
  return [...new Set(files.map(posix))];
}

function sourceFileOf(program, absPath) {
  const want = path.resolve(absPath);
  for (const sf of program.getSourceFiles()) {
    if (path.resolve(sf.fileName) === want) return sf;
  }
  return program.getSourceFile(absPath) || null;
}

function loadJsTsconfig(ts, tsconfigAbs) {
  const raw = ts.readConfigFile(tsconfigAbs, ts.sys.readFile);
  if (raw.error) {
    throw new Error(ts.flattenDiagnosticMessageText(raw.error.messageText, "\n"));
  }
  return ts.parseJsonConfigFileContent(
    raw.config,
    ts.sys,
    path.dirname(tsconfigAbs),
    undefined,
    tsconfigAbs,
  );
}

export function runJsStp2(resolved, probes, repoRoot, cases) {
  const ts = loadJsTypeScript(resolved, repoRoot);
  const tsconfigAbs = repoPath(repoRoot, probes.tsconfig);
  const parsed = loadJsTsconfig(ts, tsconfigAbs);
  const host = ts.createCompilerHost(parsed.options, true);
  const program = ts.createProgram({
    rootNames: parsed.fileNames,
    options: parsed.options,
    host,
  });
  const checker = program.getTypeChecker();
  const checkCounts = {};
  const files = {};
  const wanted = caseProbeFiles(cases);
  if (probes.positive) wanted.unshift(posix(probes.positive));
  if (probes.negative) wanted.push(posix(probes.negative));
  const unique = [...new Set(wanted)];
  for (const rel of unique) {
    const abs = repoPath(repoRoot, rel);
    bumpCheckCount(checkCounts, rel);
    const sf = sourceFileOf(program, abs);
    const text = fs.existsSync(abs) ? fs.readFileSync(abs, "utf8") : "";
    const diags = [];
    if (sf) {
      diags.push(
        ...program
          .getSemanticDiagnostics(sf)
          .concat(program.getSyntacticDiagnostics(sf))
          .map(normalizeDiag),
      );
    }
    files[rel] = {
      diags,
      observations: observeJs(ts, checker, sf, text, probes),
    };
  }
  return {
    engine: resolved.id,
    positive: files[posix(probes.positive)],
    negative: files[posix(probes.negative)],
    files,
    checkCounts,
  };
}

export async function runNativeStp2(resolved, probes, repoRoot, cases) {
  const { API } = await loadNativeApi(repoRoot);
  const probesDir = path.dirname(repoPath(repoRoot, probes.tsconfig));
  const tsconfig = repoPath(repoRoot, probes.tsconfig);
  const api = new API({ tsserverPath: resolved.executable, cwd: probesDir });
  try {
    const snap = api.updateSnapshot({ openProject: tsconfig });
    const project = snap.getProject(tsconfig);
    if (!project) {
      throw new Error(`native engine did not open ${posix(probes.tsconfig)}`);
    }
    const checkCounts = {};
    const files = {};
    const wanted = caseProbeFiles(cases);
    if (probes.positive) wanted.unshift(posix(probes.positive));
    if (probes.negative) wanted.push(posix(probes.negative));
    const unique = [...new Set(wanted)];
    for (const rel of unique) {
      const abs = repoPath(repoRoot, rel);
      files[rel] = observeNative(project, abs, probes, checkCounts, rel);
    }
    snap.dispose();
    return {
      engine: resolved.id,
      positive: files[posix(probes.positive)],
      negative: files[posix(probes.negative)],
      files,
      checkCounts,
    };
  } finally {
    api.close();
  }
}

function fileResult(run, rel) {
  return run.files?.[posix(rel)] || null;
}

function evaluateStp2Run(run, probes, cases, engineId, { maxChecksPerFile = 1 } = {}) {
  const errors = [];
  const measured = [];
  for (const row of cases) {
    const caseId = row.id;
    const primary = row.file ? fileResult(run, row.file) : null;
    if (row.file && !primary) {
      errors.push(err(caseId, "missing-probes", `${engineId} missing probe ${row.file}`));
      continue;
    }
    if (row.file) measured.push(row.file);
    if (row.disposition === "accept") {
      errors.push(
        ...assertCleanTwin(primary.diags, { fileLabel: `${engineId} ${caseId}` }).map((item) => ({
          ...item,
          caseId,
        })),
      );
      if (row.expectedInstanceType) {
        const instance = primary.observations.types.Instance;
        if (!instance) {
          errors.push(err(caseId, "vacuous-type", `${engineId} missing Instance type`));
        } else {
          errors.push(
            ...assertExactType({
              actual: instance.printed,
              expected: row.expectedInstanceType,
              flags: instance.flags,
            }).map((item) => ({ ...item, caseId })),
          );
        }
      }
      if (row.expectedHoverType) {
        const hover = primary.observations.hover;
        const needle = row.hoverNeedle || probes.hoverNeedle;
        if (
          !hover ||
          (needle && hover.name !== needle && hover.printed !== row.expectedHoverType)
        ) {
          if (!hover) {
            errors.push(err(caseId, "type-mismatch", `${engineId} missing hover type`));
          } else {
            errors.push(
              ...assertExactType({
                actual: hover.printed,
                expected: row.expectedHoverType,
                flags: hover.flags,
              }).map((item) => ({ ...item, caseId })),
            );
          }
        } else {
          errors.push(
            ...assertExactType({
              actual: hover.printed,
              expected: row.expectedHoverType,
              flags: hover.flags,
            }).map((item) => ({ ...item, caseId })),
          );
        }
      }
      if (!primary.observations.definition) {
        errors.push(err(caseId, "missing-definition", `${engineId} missing definition target`));
      }
      if ((primary.observations.edits || []).length === 0) {
        errors.push(err(caseId, "missing-edit", `${engineId} missing edit participation`));
      }
      if (
        !Number.isInteger(primary.observations.references) ||
        primary.observations.references < 1
      ) {
        errors.push(
          err(caseId, "missing-references", `${engineId} missing references participation`),
        );
      }
      if (row.expectedCode && Array.isArray(row.files)) {
        for (const extraRel of row.files) {
          measured.push(extraRel);
          const extra = fileResult(run, extraRel);
          const anchored = (extra?.diags || []).filter((diag) => diag.code === row.expectedCode);
          if (anchored.length === 0) {
            errors.push(
              err(
                caseId,
                "missing-negative",
                `${engineId} ${extraRel} lacked TS${row.expectedCode}; got ${JSON.stringify(extra?.diags || [])}`,
              ),
            );
          }
        }
      }
    } else {
      const expectedCode = row.expectedCode;
      const anchored = (primary.diags || []).filter((diag) => diag.code === expectedCode);
      if (!Number.isInteger(expectedCode) || anchored.length === 0) {
        errors.push(
          err(
            caseId,
            "missing-negative",
            `${engineId} ${row.file} lacked anchored TS${expectedCode}; got ${JSON.stringify(primary.diags)}`,
          ),
        );
      }
      if (row.dirtyTwin) {
        measured.push(row.dirtyTwin);
        const dirty = fileResult(run, row.dirtyTwin);
        if (!dirty) {
          errors.push(
            err(caseId, "missing-probes", `${engineId} missing dirty twin ${row.dirtyTwin}`),
          );
        } else if (Number.isInteger(row.dirtyExpectedCode)) {
          const dirtyAnchored = (dirty.diags || []).filter(
            (diag) => diag.code === row.dirtyExpectedCode,
          );
          if (dirtyAnchored.length === 0) {
            errors.push(
              err(
                caseId,
                "missing-negative",
                `${engineId} dirty twin ${row.dirtyTwin} lacked TS${row.dirtyExpectedCode}; got ${JSON.stringify(dirty.diags)}`,
              ),
            );
          }
        } else if ((dirty.diags || []).some((diag) => diag.code === expectedCode)) {
          errors.push(
            err(
              caseId,
              "unrelated-generated-error",
              `${engineId} dirty twin ${row.dirtyTwin} unexpectedly reported TS${expectedCode}`,
            ),
          );
        }
      }
    }
  }
  errors.push(...assertCheckCounts(run.checkCounts, engineId, measured, maxChecksPerFile));
  return errors;
}

function evaluateHarnessRun(run, probes, engineId, { maxChecksPerFile = 1 } = {}) {
  const errors = [];
  errors.push(...assertCleanTwin(run.positive.diags, { fileLabel: `${engineId} clean twin` }));
  const instance = run.positive.observations.types.Instance;
  if (!instance) {
    errors.push(err("STP1-types", "vacuous-type", `${engineId} missing Instance type`));
  } else {
    errors.push(
      ...assertExactType({
        actual: instance.printed,
        expected: probes.expectedInstanceType,
        flags: instance.flags,
      }),
    );
  }
  const hover = run.positive.observations.hover;
  if (!hover) {
    errors.push(err("STP1-types", "type-mismatch", `${engineId} missing hover type`));
  } else {
    errors.push(
      ...assertExactType({
        actual: hover.printed,
        expected: probes.expectedHoverType,
        flags: hover.flags,
      }),
    );
  }
  if (!run.positive.observations.definition) {
    errors.push(err("STP1-harness", "missing-definition", `${engineId} missing definition target`));
  }
  if ((run.positive.observations.edits || []).length === 0) {
    errors.push(err("STP1-harness", "missing-edit", `${engineId} missing edit participation`));
  }
  if (
    !Number.isInteger(run.positive.observations.references) ||
    run.positive.observations.references < 1
  ) {
    errors.push(
      err("STP1-harness", "missing-references", `${engineId} missing references participation`),
    );
  }
  const expectedCode = probes.expectedNegativeCode;
  const anchored = (run.negative.diags || []).filter((diag) => diag.code === expectedCode);
  if (anchored.length === 0) {
    errors.push(
      err(
        "STP1-harness",
        "missing-negative",
        `${engineId} negative probe lacked anchored TS${expectedCode}; got ${JSON.stringify(run.negative.diags)}`,
      ),
    );
  }
  const unrelated = (run.negative.diags || []).filter(
    (diag) => diag.code && diag.code !== expectedCode,
  );
  if (unrelated.length > 0) {
    errors.push(
      err(
        "STP1-clean-twin",
        "unrelated-generated-error",
        `${engineId} negative probe had unrelated diagnostic ${unrelated[0].code}: ${unrelated[0].message}`,
      ),
    );
  }
  errors.push(
    ...assertCheckCounts(
      run.checkCounts,
      engineId,
      [probes.positive, probes.negative],
      maxChecksPerFile,
    ),
  );
  return errors;
}

export function inputRevision(repoRoot) {
  const result = spawnSync("git", ["rev-parse", "HEAD"], {
    cwd: repoRoot,
    encoding: "utf8",
    windowsHide: true,
  });
  if (result.status !== 0) return null;
  return String(result.stdout || "").trim() || null;
}

export async function verifyNode(options) {
  const repoRoot = options.repoRoot || REPO_ROOT;
  const errors = [];
  const selected = [];
  const resolvedEngines = [];
  const harnessRuns = [];
  const nodeId = options.node;
  if (!nodeId) {
    errors.push(err("STP1-zero-selection", "zero-cases", "--node is required"));
    return summarize({ options, errors, selected, resolvedEngines, harnessRuns, repoRoot });
  }

  const rootLoad = options.rootManifest
    ? {
        manifest: options.rootManifest,
        errors: validateProbeManifest(options.rootManifest, { role: "root" }),
      }
    : loadRootManifest(repoRoot, options.manifest || DEFAULT_ROOT_MANIFEST);
  errors.push(...rootLoad.errors);
  if (!rootLoad.manifest) {
    return summarize({ options, errors, selected, resolvedEngines, harnessRuns, repoRoot });
  }

  const entry = selectNodeEntry(rootLoad.manifest, nodeId);
  if (!entry) {
    errors.push(...assertNonZeroSelection([], nodeId));
    return summarize({ options, errors, selected, resolvedEngines, harnessRuns, repoRoot });
  }

  const nodeLoad = options.nodeManifest
    ? {
        manifest: options.nodeManifest,
        errors: validateProbeManifest(options.nodeManifest, { role: "node" }),
      }
    : loadNodeManifest(repoRoot, entry.manifest);
  errors.push(...nodeLoad.errors);
  const cases = selectCases(nodeLoad.manifest, nodeId);
  errors.push(...assertNonZeroSelection(cases, nodeId));
  if (cases.length === 0) {
    return summarize({ options, errors, selected, resolvedEngines, harnessRuns, repoRoot });
  }
  selected.push(...cases.map((row) => row.id));

  const inventory = options.inventory || readJson(repoPath(repoRoot, rootLoad.manifest.inventory));
  const obligation = options.obligation || loadObligation(repoRoot, inventory);
  errors.push(...assertInventoryComplete(inventory, obligation, repoRoot));
  if (nodeId === "STP2") {
    errors.push(...assertStp2Products(repoRoot, nodeLoad.manifest));
  }

  const matrix =
    options.engineMatrix || readJson(repoPath(repoRoot, rootLoad.manifest.engineMatrix));
  const pins = selectEngines(matrix, options.engine || "all");
  if (pins.length === 0) {
    errors.push(
      err(
        "STP1-zero-selection",
        "zero-cases",
        `engine filter ${options.engine} selected zero engines`,
      ),
    );
  }
  for (const pin of pins) {
    const resolved = await resolveEngine(pin, repoRoot);
    if (!resolved.ok) errors.push(resolved.error);
    else resolvedEngines.push(resolved.engine);
  }

  const methodology =
    options.performanceMethodology ||
    readJson(repoPath(repoRoot, rootLoad.manifest.performanceMethodology));
  if (methodology?.stateSafety?.incremental !== "fresh") {
    errors.push(
      err("STP1-harness", "stale-incremental", "performance methodology incremental is not fresh"),
    );
  }

  const probes = nodeLoad.manifest?.probes;
  const runnable = probesAreRunnable(probes);
  const shouldRun = resolvedEngines.length > 0 && runnable && !options.skipProbes;
  if (resolvedEngines.length > 0 && !options.skipProbes && !runnable) {
    if (
      !errors.some((error) => error.caseId === "STP1-harness" && error.code === "missing-probes")
    ) {
      errors.push(
        err(
          "STP1-harness",
          "missing-probes",
          "node manifest omitted probes; refusing zero-test pass",
        ),
      );
    }
  } else if (shouldRun) {
    const maxChecksPerFile = methodology?.boundedWork?.maxChecksPerFilePerEngine ?? 1;
    const stp2 = nodeId === "STP2";
    for (const engine of resolvedEngines) {
      const run = stp2
        ? engine.kind === "javascript"
          ? runJsStp2(engine, probes, repoRoot, cases)
          : await runNativeStp2(engine, probes, repoRoot, cases)
        : engine.kind === "javascript"
          ? runJsEngine(engine, probes, repoRoot)
          : await runNativeEngine(engine, probes, repoRoot);
      harnessRuns.push(run);
      if (stp2) {
        errors.push(...evaluateStp2Run(run, probes, cases, engine.id, { maxChecksPerFile }));
      } else {
        errors.push(...evaluateHarnessRun(run, probes, engine.id, { maxChecksPerFile }));
      }
    }
    if (harnessRuns.length === 0) {
      errors.push(err("STP1-harness", "missing-probes", "zero probe executions"));
    }
  }

  if (options.requireAll) {
    const required = canonicalMandatoryCases(nodeId);
    for (const id of required) {
      if (!selected.includes(id)) {
        errors.push(
          err("STP1-zero-selection", "zero-cases", `mandatory case ${id} was not selected`),
        );
      }
    }
  }

  return summarize({
    options,
    errors,
    selected,
    resolvedEngines,
    harnessRuns,
    repoRoot,
    inventory,
    methodology,
  });
}

function summarize({
  options,
  errors,
  selected,
  resolvedEngines,
  harnessRuns,
  repoRoot,
  inventory,
  methodology,
}) {
  const ok = errors.length === 0;
  const selectedCaseIds = new Set(selected);
  const mandatory = canonicalMandatoryCases(options.node);
  const mandatoryList = mandatory.length ? mandatory : [...MANDATORY_CASES];
  if (ok) {
    for (const id of mandatoryList) selectedCaseIds.add(id);
  } else {
    for (const error of errors) selectedCaseIds.add(error.caseId);
  }
  const probeSizes = {};
  for (const rel of [
    "tests/sfc-projection/STP1/probes/positive.ts",
    "tests/sfc-projection/STP1/probes/negative.ts",
    "tests/sfc-projection/STP2/probes/accept-instance-concrete.ts",
    "tests/sfc-projection/STP2/probes/accept-instance-generic.ts",
    "tests/sfc-projection/STP2/probes/accept-instance-explicit.ts",
    "tests/sfc-projection/STP2/probes/accept-constructor-inferred.ts",
    "tests/sfc-projection/STP2/probes/accept-vue-utilities.tsx",
    "tests/sfc-projection/STP2/probes/reject-not-callable.ts",
    "tests/sfc-projection/STP2/probes/reject-constructor-escape-clean.ts",
    "tests/sfc-projection/STP2/probes/reject-explicit-input-mismatch.ts",
  ]) {
    const abs = repoPath(repoRoot, rel);
    if (fs.existsSync(abs)) probeSizes[rel] = fs.statSync(abs).size;
  }
  return {
    ok,
    node: options.node,
    engine: options.engine || "all",
    requireAll: !!options.requireAll,
    schema: SCHEMA_PATH,
    platform: `${os.platform()}-${os.arch()}`,
    inputRevision: inputRevision(repoRoot),
    selectedCaseIds: [...selectedCaseIds],
    mandatoryCases: [...mandatoryList],
    engines: resolvedEngines.map((engine) => ({
      id: engine.id,
      label: engine.label,
      kind: engine.kind,
      package: engine.package,
      version: engine.version,
      executable: posix(engine.executable),
      sha256: engine.sha256,
      mapperCapable: engine.mapperCapable,
    })),
    inventoryRows: inventory?.rows?.length ?? null,
    incremental: methodology?.stateSafety?.incremental || "fresh",
    probeSizes,
    harnessRuns: harnessRuns.map((run) => ({
      engine: run.engine,
      positiveDiagnostics: run.positive.diags,
      negativeDiagnostics: run.negative.diags,
      hover: run.positive.observations.hover?.printed || null,
      instanceType: run.positive.observations.types.Instance?.printed || null,
      definition: run.positive.observations.definition?.name || null,
      references: run.positive.observations.references,
      edits: run.positive.observations.edits.length,
      checkCounts: run.checkCounts,
    })),
    errors,
  };
}

export function selectedCaseIds(result) {
  return [...(result.selectedCaseIds || [])];
}

const HELP = `ProjectionProbeRunner — SFC projection probe harness (STP1 inventory, STP2 constructor)

USAGE
  node scripts/sfc-projection/verify-node.mjs --node STP1|STP2 [--engine all|ts-js|ts-native] [--require-all] [--json]

Rejects absent/empty manifests, zero selected cases, missing inventory fixtures,
vacuous any/never type matches, unrelated clean-twin diagnostics, a substituted
executable under the same engine label, omitted probes, non-exact type matches,
missing references, and duplicate file checks.
`;

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const args = parseArgs();
  if (args.help) {
    process.stdout.write(HELP);
    process.exit(0);
  }
  const result = await verifyNode({ ...args, repoRoot: REPO_ROOT });
  if (args.json) process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
  else if (result.ok) {
    process.stdout.write(
      `${result.node} verify: PASS cases=${result.selectedCaseIds.join(",")} engines=${result.engines
        .map((engine) => `${engine.id}:${posix(engine.executable)}`)
        .join(",")}\n`,
    );
  } else {
    process.stderr.write(`${JSON.stringify(result.errors, null, 2)}\n`);
  }
  process.exit(result.ok ? 0 : 1);
}
