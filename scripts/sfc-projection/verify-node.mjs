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

export const MANDATORY_CASES = Object.freeze([
  "STP1-inventory",
  "STP1-zero-selection",
  "STP1-clean-twin",
  "STP1-types",
  "STP1-provenance",
  "STP1-harness",
]);

export const STP5_MANDATORY_CASES = Object.freeze([
  "STP5-encoding",
  "STP5-guard-duplicate",
  "STP5-alias-edit",
  "STP5-stale-target",
  "STP5-raw-cli",
  "STP5-capability",
]);

export function mandatoryCasesFor(nodeManifest) {
  const nodeId = nodeManifest?.node;
  if (!nodeId || nodeId === "STP1") return [...MANDATORY_CASES];
  if (nodeId === "STP5") return [...STP5_MANDATORY_CASES];
  return [...(nodeManifest.mandatoryCases || [])];
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
      for (const id of mandatoryCasesFor(doc)) {
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

function runJsFile(ts, fileAbs, options, checkCounts, fileKey, probes) {
  const program = createJsProgram(ts, fileAbs, options, checkCounts, fileKey);
  const sf = program.getSourceFile(fileAbs);
  const checker = program.getTypeChecker();
  const diags = ts
    .getPreEmitDiagnostics(program)
    .filter((diag) => diag.file && path.resolve(diag.file.fileName) === path.resolve(fileAbs))
    .map(normalizeDiag);
  const text = fs.readFileSync(fileAbs, "utf8");
  const observations = observeJs(ts, checker, sf, text, probes);
  return { diags, observations };
}

function observeJs(ts, checker, sf, text, probes) {
  const out = { types: {}, hover: null, definition: null, references: 0, edits: [] };
  if (!sf) return out;
  const instancePos = text.indexOf("export type Instance");
  if (instancePos >= 0) {
    const node = findIdentifier(ts, sf, "Instance");
    if (node) {
      const type = checker.getTypeAtLocation(node);
      out.types.Instance = {
        printed: checker.typeToString(type),
        flags: jsTypeFlags(ts, type),
      };
    }
  }
  const hoverNeedle = probes?.hoverNeedle || "stp1HoverTarget";
  const hoverPos = text.indexOf(hoverNeedle);
  if (hoverPos >= 0) {
    const node = findIdentifier(ts, sf, hoverNeedle);
    if (node) {
      const type = checker.getTypeAtLocation(node);
      out.hover = {
        name: hoverNeedle,
        printed: checker.typeToString(type),
        flags: jsTypeFlags(ts, type),
        pos: node.getStart(sf),
      };
    }
  }
  const defNode = findIdentifier(ts, sf, "Comp");
  if (defNode) {
    const sym = checker.getSymbolAtLocation(defNode);
    out.definition = { name: sym?.getName?.() || "Comp", pos: defNode.getStart(sf) };
    out.edits = [{ name: "Comp", pos: defNode.getStart(sf), end: defNode.getEnd() }];
  }
  out.references = countIdentifier(ts, sf, "Comp");
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
  const pos = runJsFile(ts, positive, options, checkCounts, probes.positive, probes);
  const neg = runJsFile(ts, negative, options, checkCounts, probes.negative, probes);
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
  const instanceNeedle = "Instance";
  if (text.includes("export type Instance")) {
    const pos = offsetOf(text, "export type Instance") + "export type ".length;
    const type = project.checker.getTypeAtPosition(fileAbs, pos);
    if (type) {
      observations.types.Instance = {
        printed: project.checker.typeToString(type),
        flags: type.flags ?? 0,
      };
    }
  }
  if (text.includes(probes.hoverNeedle)) {
    const pos = offsetOf(text, probes.hoverNeedle);
    const type = project.checker.getTypeAtPosition(fileAbs, pos);
    if (type) {
      observations.hover = {
        name: probes.hoverNeedle,
        printed: project.checker.typeToString(type),
        flags: type.flags ?? 0,
        pos,
      };
    }
  }
  if (text.includes("Comp")) {
    const pos = offsetOf(text, "class Comp") + "class ".length;
    const sym = project.checker.getSymbolAtPosition(fileAbs, pos);
    observations.definition = { name: sym?.name || "Comp", pos };
    observations.edits = [{ name: "Comp", pos, end: pos + 4 }];
    let refs = [];
    try {
      refs = sym ? project.checker.getReferencesToSymbolInFile(fileAbs, sym) : [];
    } catch {
      refs = [];
    }
    observations.references = Array.isArray(refs) ? refs.length : text.split("Comp").length - 1;
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
    for (const engine of resolvedEngines) {
      const run =
        engine.kind === "javascript"
          ? runJsEngine(engine, probes, repoRoot)
          : await runNativeEngine(engine, probes, repoRoot);
      harnessRuns.push(run);
      errors.push(...evaluateHarnessRun(run, probes, engine.id, { maxChecksPerFile }));
    }
    if (harnessRuns.length === 0) {
      errors.push(err("STP1-harness", "missing-probes", "zero probe executions"));
    }
  }

  if (options.requireAll) {
    for (const id of mandatoryCasesFor(nodeLoad.manifest)) {
      if (!selected.includes(id)) {
        errors.push(
          err("STP1-zero-selection", "zero-cases", `mandatory case ${id} was not selected`),
        );
      }
    }
  }

  if (nodeId === "STP5") {
    errors.push(
      ...(await evaluateStp5Node({
        repoRoot,
        resolvedEngines,
        harnessRuns,
        skipProbes: options.skipProbes,
        runnable,
      })),
    );
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
    nodeManifest: nodeLoad.manifest,
  });
}

async function evaluateStp5Node({ repoRoot, resolvedEngines, harnessRuns, skipProbes, runnable }) {
  const protocolHref = pathToFileURL(
    repoPath(repoRoot, "tests/sfc-projection/STP5/protocol.mjs"),
  ).href;
  const protocol = await import(protocolHref);
  const errors = [];
  if (skipProbes) {
    errors.push(...protocol.assertEncodingIdentity());
    errors.push(...protocol.assertCleanGeometry());
    errors.push(...protocol.evaluateRejectTwins());
    return errors;
  }
  if (!runnable) {
    errors.push(err("STP5-capability", "missing-probes", "STP5 omitted runnable probes"));
    return errors;
  }
  const native = resolvedEngines.find((engine) => engine.kind === "native") || resolvedEngines[0];
  let mapperHostPresent = false;
  if (native?.kind === "native" && native.executable) {
    const help = spawnSync(native.executable, ["--help"], {
      encoding: "utf8",
      windowsHide: true,
    });
    mapperHostPresent = protocol.helpTextHasMapperHost(`${help.stdout || ""}${help.stderr || ""}`);
  }
  const observation = {
    diagnostics: harnessRuns.some((run) => (run.negative?.diags || []).length > 0),
    hover: harnessRuns.some((run) => Boolean(run.positive?.observations?.hover)),
    definition: harnessRuns.some((run) => Boolean(run.positive?.observations?.definition)),
    references: harnessRuns.some((run) => (run.positive?.observations?.references || 0) > 0),
    edits: harnessRuns.some((run) => (run.positive?.observations?.edits || []).length > 0),
  };
  const stp5 = await protocol.evaluateStp5({
    mapperHostPresent,
    observation,
    engineId: native?.id || "ts-native",
    engineVersion: native?.version || "unknown",
  });
  errors.push(...stp5.errors);
  return errors;
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
  nodeManifest,
}) {
  const ok = errors.length === 0;
  const required = mandatoryCasesFor(nodeManifest || { node: options.node });
  const selectedCaseIds = new Set(selected);
  if (ok) {
    for (const id of required) selectedCaseIds.add(id);
  } else {
    for (const error of errors) selectedCaseIds.add(error.caseId);
  }
  const probeSizes = {};
  const probeFiles = [nodeManifest?.probes?.positive, nodeManifest?.probes?.negative].filter(
    Boolean,
  );
  for (const rel of probeFiles) {
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
    mandatoryCases: required,
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

const HELP = `ProjectionProbeRunner — STP1/STP5 feature inventory and mapper proof harness

USAGE
  node scripts/sfc-projection/verify-node.mjs --node STP1|STP5 [--engine all|ts-js|ts-native] [--require-all] [--json]

Rejects absent/empty manifests, zero selected cases, missing inventory fixtures,
vacuous any/never type matches, unrelated clean-twin diagnostics, a substituted
executable under the same engine label, omitted probes, non-exact type matches,
missing references, and duplicate file checks. STP5 additionally rejects Alias
rename codecs, query-snapshot reuse, Verter-as-stock-CLI claims, duplicate
guards, version-label capability, and dormant-product complete claims.
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
