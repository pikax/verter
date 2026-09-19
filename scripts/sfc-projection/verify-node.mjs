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

// The canonical per-node mandatory-case table lives in its own module so node
// protocols (e.g. tests/sfc-projection/STP8/protocol.mjs) can derive required
// rows from it without a circular import back into this CLI entry point,
// which evaluates under a top-level await.
import {
  MANDATORY_CASES,
  NODE_MANDATORY_CASES,
  STP3_MANDATORY_CASES,
  STP4_MANDATORY_CASES,
  STP5_MANDATORY_CASES,
  STP6_MANDATORY_CASES,
  STP7_MANDATORY_CASES,
  STP8_MANDATORY_CASES,
  STP9_MANDATORY_CASES,
  STS0_MANDATORY_CASES,
} from "./node-mandatory-cases.mjs";

export {
  MANDATORY_CASES,
  NODE_MANDATORY_CASES,
  STP3_MANDATORY_CASES,
  STP4_MANDATORY_CASES,
  STP5_MANDATORY_CASES,
  STP6_MANDATORY_CASES,
  STP7_MANDATORY_CASES,
  STP8_MANDATORY_CASES,
  STP9_MANDATORY_CASES,
  STS0_MANDATORY_CASES,
};

export function canonicalMandatoryCases(nodeId) {
  return NODE_MANDATORY_CASES[nodeId] || [];
}

function usesCaseFileRunner(nodeId) {
  return nodeId === "STP2" || nodeId === "STP3" || nodeId === "STP4" || nodeId === "STP6";
}

export function mandatoryCasesFor(nodeManifest) {
  const nodeId = nodeManifest?.node;
  const canonical = canonicalMandatoryCases(nodeId);
  if (canonical.length) return [...canonical];
  if (!nodeId || nodeId === "STP1") return [...MANDATORY_CASES];
  return [...(nodeManifest?.mandatoryCases || [])];
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
      const required = canonical.length ? canonical : mandatoryCasesFor(doc);
      for (const id of required) {
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
  errors.push(...assertFooSyntaxControl(evidence));
  if (!String(evidence.publicOptionsStaticBehavior?.rationale || "").trim()) {
    errors.push(
      err(
        "STP2-instance-concrete",
        "removed-fixture",
        "missing public options/static behavior rationale",
      ),
    );
  }
  if (!String(evidence.componentPublicInstanceAssignability?.rationale || "").trim()) {
    errors.push(
      err(
        "STP2-instance-concrete",
        "removed-fixture",
        "missing ComponentPublicInstance assignability rationale",
      ),
    );
  }
  return errors;
}

export function assertFooSyntaxControl(evidence) {
  if (!String(evidence?.syntaxControl?.spelling || "").includes("declare class Foo")) {
    return [err("STP2-instance-explicit", "removed-fixture", "missing Foo syntax control")];
  }
  return [];
}

const STP3_BINDERS = Object.freeze([
  "default",
  "const",
  "variadic",
  "dependent",
  "higher-rank-slot",
  "generic-parent",
]);

const STP3_INFERENCE_CHANNELS = Object.freeze(["rows", "project", "modelValue"]);

export function assertStp3Products(repoRoot, nodeManifest) {
  const errors = [];
  const declared = new Set(nodeManifest?.products || []);
  for (const product of ["CoupledInferenceEvidence", "InferenceWitnessSelection"]) {
    if (!declared.has(product)) {
      errors.push(err("STP3-coupled", "removed-fixture", `missing product ${product}`));
    }
  }
  const evidenceRel = "tests/sfc-projection/STP3/products/coupled-inference-evidence.json";
  const selectionRel = "tests/sfc-projection/STP3/products/inference-witness-selection.json";
  const evidenceAbs = repoPath(repoRoot, evidenceRel);
  const selectionAbs = repoPath(repoRoot, selectionRel);
  if (!fs.existsSync(evidenceAbs)) {
    errors.push(err("STP3-coupled", "removed-fixture", `missing ${evidenceRel}`));
    return errors;
  }
  if (!fs.existsSync(selectionAbs)) {
    errors.push(err("STP3-coupled", "removed-fixture", `missing ${selectionRel}`));
    return errors;
  }
  const evidence = readJson(evidenceAbs);
  const selection = readJson(selectionAbs);
  if (evidence.schema !== "CoupledInferenceEvidence") {
    errors.push(err("STP3-coupled", "removed-fixture", "CoupledInferenceEvidence schema"));
  }
  if (selection.schema !== "InferenceWitnessSelection") {
    errors.push(err("STP3-coupled", "removed-fixture", "InferenceWitnessSelection schema"));
  }
  if (selection.selectedWitness !== "whole-signature-contextual-construction") {
    errors.push(
      err(
        "STP3-coupled",
        "removed-fixture",
        "selected witness is not whole-signature construction",
      ),
    );
  }
  if (selection.rejectedWitness !== "split-inference-plus-post-specialization") {
    errors.push(
      err("STP3-inference-only-channel", "removed-fixture", "rejected split witness missing"),
    );
  }
  const binders = new Set((evidence.binders || []).map((row) => row.id));
  for (const binder of STP3_BINDERS) {
    if (!binders.has(binder)) {
      errors.push(err("STP3-coupled", "removed-fixture", `missing binder ${binder}`));
    }
  }
  const channels = new Set((evidence.channels || []).map((row) => row.id));
  for (const channel of STP3_INFERENCE_CHANNELS) {
    if (!channels.has(channel)) {
      errors.push(err("STP3-coupled", "removed-fixture", `missing inference channel ${channel}`));
    }
  }
  const evidenceIds = new Set((evidence.cases || []).map((row) => row.id));
  for (const id of NODE_MANDATORY_CASES.STP3) {
    if (!evidenceIds.has(id)) {
      errors.push(err("STP3-coupled", "removed-fixture", `coupled evidence missing ${id}`));
    }
  }
  if (!String(evidence.constructorSpelling || "").includes("declare class Comp")) {
    errors.push(err("STP3-coupled", "removed-fixture", "missing coupled constructor spelling"));
  }
  if (!String(evidence.ac3Rationale || "").trim()) {
    errors.push(err("STP3-coupled", "removed-fixture", "missing AC3 untouched-owner rationale"));
  }
  if (!String(evidence.ac4Rationale || "").trim()) {
    errors.push(err("STP3-coupled", "removed-fixture", "missing AC4 untouched-owner rationale"));
  }
  if (selection.orderedOperations?.forbiddenListenerPolicy !== "last-write-wins") {
    errors.push(
      err(
        "STP3-ordered-merge",
        "removed-fixture",
        "ordered operations must forbid last-write-wins",
      ),
    );
  }
  return errors;
}

const STP4_DIALECTS = Object.freeze(["ts", "tsx", "js", "jsx"]);

export function assertStp4Products(repoRoot, nodeManifest) {
  const errors = [];
  const declared = new Set(nodeManifest?.products || []);
  for (const product of ["DialectTopologyEvidence", "ProjectionTopologyDecisionInputs"]) {
    if (!declared.has(product)) {
      errors.push(err("STP4-js-unchecked", "removed-fixture", `missing product ${product}`));
    }
  }
  const evidenceRel = "tests/sfc-projection/STP4/products/dialect-topology-evidence.json";
  const inputsRel = "tests/sfc-projection/STP4/products/projection-topology-decision-inputs.json";
  const evidenceAbs = repoPath(repoRoot, evidenceRel);
  const inputsAbs = repoPath(repoRoot, inputsRel);
  if (!fs.existsSync(evidenceAbs)) {
    errors.push(err("STP4-js-unchecked", "removed-fixture", `missing ${evidenceRel}`));
    return errors;
  }
  if (!fs.existsSync(inputsAbs)) {
    errors.push(err("STP4-js-unchecked", "removed-fixture", `missing ${inputsRel}`));
    return errors;
  }
  const evidence = readJson(evidenceAbs);
  const inputs = readJson(inputsAbs);
  if (evidence.schema !== "DialectTopologyEvidence") {
    errors.push(err("STP4-js-unchecked", "removed-fixture", "DialectTopologyEvidence schema"));
  }
  if (inputs.schema !== "ProjectionTopologyDecisionInputs") {
    errors.push(
      err("STP4-js-unchecked", "removed-fixture", "ProjectionTopologyDecisionInputs schema"),
    );
  }
  const dialects = new Set((evidence.dialects || []).map((row) => row.id));
  for (const dialect of STP4_DIALECTS) {
    if (!dialects.has(dialect)) {
      errors.push(err("STP4-tsx-authored", "removed-fixture", `missing dialect ${dialect}`));
    }
  }
  const checkJs = new Set((evidence.checkJs || []).map((row) => row.id));
  if (!checkJs.has("off")) {
    errors.push(err("STP4-js-unchecked", "removed-fixture", "missing checkJs off policy"));
  }
  if (!checkJs.has("on")) {
    errors.push(err("STP4-js-checked", "removed-fixture", "missing checkJs on policy"));
  }
  const evidenceIds = new Set((evidence.cases || []).map((row) => row.id));
  for (const id of NODE_MANDATORY_CASES.STP4) {
    if (!evidenceIds.has(id)) {
      errors.push(err("STP4-js-unchecked", "removed-fixture", `dialect evidence missing ${id}`));
    }
  }
  if (!String(evidence.constructorSpelling || "").includes("declare class Comp")) {
    errors.push(
      err("STP4-js-unchecked", "removed-fixture", "missing dialect constructor spelling"),
    );
  }
  if (!String(evidence.ac3Rationale || "").trim()) {
    errors.push(
      err("STP4-js-unchecked", "removed-fixture", "missing AC3 untouched-owner rationale"),
    );
  }
  if (!String(evidence.ac4Rationale || "").trim()) {
    errors.push(
      err("STP4-js-unchecked", "removed-fixture", "missing AC4 untouched-owner rationale"),
    );
  }
  if (inputs.supplementalFiles?.namedImportTarget !== false) {
    errors.push(
      err(
        "STP4-supplemental-import",
        "removed-fixture",
        "supplemental files must not be named import targets",
      ),
    );
  }
  if (inputs.supplementalFiles?.sharedLexicalScope !== false) {
    errors.push(
      err(
        "STP4-supplemental-import",
        "removed-fixture",
        "supplemental files must not share a lexical scope",
      ),
    );
  }
  if (inputs.externalScripts?.checkedOncePerIdentity !== true) {
    errors.push(
      err("STP4-external-owner", "removed-fixture", "external scripts must be checked once"),
    );
  }
  if (inputs.vueLegality?.scriptSetupSrc !== "reject") {
    errors.push(err("STP4-illegal-vue", "removed-fixture", "script-setup src must stay rejected"));
  }
  if (inputs.vueLegality?.authority !== "framework-legality") {
    errors.push(
      err("STP4-illegal-vue", "removed-fixture", "script-setup src must be framework legality"),
    );
  }
  if (inputs.vueLegality?.typescriptAcceptsGenerated !== "not-sufficient") {
    errors.push(
      err(
        "STP4-illegal-vue",
        "removed-fixture",
        "TypeScript accepting generated code is not Vue legality",
      ),
    );
  }
  if (inputs.jsxRewrite?.tsAngleAssertion !== "no-jsx-rewrite") {
    errors.push(
      err("STP4-tsx-authored", "removed-fixture", "TS angle assertions must not be JSX-rewritten"),
    );
  }
  return errors;
}

const STP6_CONSUMPTION_MODES = Object.freeze([
  "direct-import",
  "alias",
  "barrel",
  "namespace",
  "project-references",
  "package-exports",
]);

const STP6_RESOLUTION_MODES = Object.freeze(["bundler", "node16", "nodenext"]);

export function assertStp6Products(repoRoot, nodeManifest) {
  const errors = [];
  const declared = new Set(nodeManifest?.products || []);
  for (const product of ["PackedConsumerFeasibility", "PublicDependencyClosurePolicy"]) {
    if (!declared.has(product)) {
      errors.push(err("STP6-package-instance", "removed-fixture", `missing product ${product}`));
    }
  }
  const feasibilityRel = "tests/sfc-projection/STP6/products/packed-consumer-feasibility.json";
  const closureRel = "tests/sfc-projection/STP6/products/public-dependency-closure-policy.json";
  const feasibilityAbs = repoPath(repoRoot, feasibilityRel);
  const closureAbs = repoPath(repoRoot, closureRel);
  if (!fs.existsSync(feasibilityAbs)) {
    errors.push(err("STP6-package-instance", "removed-fixture", `missing ${feasibilityRel}`));
    return errors;
  }
  if (!fs.existsSync(closureAbs)) {
    errors.push(err("STP6-closure", "removed-fixture", `missing ${closureRel}`));
    return errors;
  }
  const feasibility = readJson(feasibilityAbs);
  const closure = readJson(closureAbs);
  if (feasibility.schema !== "PackedConsumerFeasibility") {
    errors.push(
      err("STP6-package-instance", "removed-fixture", "PackedConsumerFeasibility schema"),
    );
  }
  if (closure.schema !== "PublicDependencyClosurePolicy") {
    errors.push(err("STP6-closure", "removed-fixture", "PublicDependencyClosurePolicy schema"));
  }
  const modes = new Set((feasibility.consumptionModes || []).map((row) => row.id));
  for (const mode of STP6_CONSUMPTION_MODES) {
    if (!modes.has(mode)) {
      errors.push(
        err("STP6-package-instance", "removed-fixture", `missing consumption mode ${mode}`),
      );
    }
  }
  const resolutions = new Set((feasibility.resolutionModes || []).map((row) => row.id));
  for (const mode of STP6_RESOLUTION_MODES) {
    if (!resolutions.has(mode)) {
      errors.push(err("STP6-resolution", "removed-fixture", `missing resolution mode ${mode}`));
    }
  }
  const evidenceIds = new Set((feasibility.cases || []).map((row) => row.id));
  for (const id of NODE_MANDATORY_CASES.STP6) {
    if (!evidenceIds.has(id)) {
      errors.push(err("STP6-package-instance", "removed-fixture", `feasibility missing ${id}`));
    }
  }
  if (!String(feasibility.constructorSpelling || "").includes("declare class Comp")) {
    errors.push(
      err("STP6-package-instance", "removed-fixture", "missing packed constructor spelling"),
    );
  }
  if (feasibility.package?.producerWorkspaceResolution !== false) {
    errors.push(
      err(
        "STP6-package-instance",
        "removed-fixture",
        "packed consumer must not use producer workspace resolution",
      ),
    );
  }
  if (!String(feasibility.ac3Rationale || "").trim()) {
    errors.push(
      err("STP6-package-instance", "removed-fixture", "missing AC3 untouched-owner rationale"),
    );
  }
  if (!String(feasibility.ac4Rationale || "").trim()) {
    errors.push(
      err("STP6-package-instance", "removed-fixture", "missing AC4 untouched-owner rationale"),
    );
  }
  if (closure.typeOnlyCheckingContract?.required !== false) {
    errors.push(
      err(
        "STP6-hidden-metadata",
        "removed-fixture",
        "type-only checking contract must not be required for precision",
      ),
    );
  }
  if (closure.declarationMaps?.sourcesMustBePackRelativeAuthoredFiles !== true) {
    errors.push(
      err(
        "STP6-decl-map",
        "removed-fixture",
        "declaration maps must target shipped authored source",
      ),
    );
  }
  if (closure.hiddenMetadata?.inExports !== false || closure.hiddenMetadata?.inFiles !== false) {
    errors.push(
      err(
        "STP6-hidden-metadata",
        "removed-fixture",
        "unpublished sidecar must stay out of the pack",
      ),
    );
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

function identifierPattern(name) {
  const escaped = String(name).replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`(?<![A-Za-z0-9_$])${escaped}(?![A-Za-z0-9_$])`);
}

export function hasIdentifier(text, name) {
  if (!name) return false;
  return identifierPattern(name).test(text);
}

export function identifierOffset(text, name) {
  if (!name) return -1;
  const match = identifierPattern(name).exec(text);
  return match ? match.index : -1;
}

export function resolveDefinitionName(text, probes = {}) {
  const needle = probes.definitionNeedle;
  if (needle && hasIdentifier(text, needle)) return needle;
  if (hasIdentifier(text, "Comp")) return "Comp";
  return null;
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

function runJsFile(ts, fileAbs, options, checkCounts, fileKey, probes = {}) {
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
  const defName = resolveDefinitionName(text, probes);
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
  const defName = resolveDefinitionName(text, probes);
  let defPos = null;
  if (defName) {
    const identPos = identifierOffset(text, defName);
    defPos = identPos >= 0 ? identPos : offsetOf(text, defName);
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

function extraExpectedCode(row, extraRel) {
  const key = posix(extraRel);
  const keyed = row.fileExpectedCodes?.[key] ?? row.fileExpectedCodes?.[extraRel];
  if (Number.isInteger(keyed)) return keyed;
  if (Number.isInteger(row.expectedCode)) return row.expectedCode;
  return null;
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
      } else if (
        probes.definitionNeedle &&
        primary.observations.definition.name !== probes.definitionNeedle
      ) {
        errors.push(
          err(
            caseId,
            "missing-definition",
            `${engineId} definition target ${primary.observations.definition.name} is not ${probes.definitionNeedle}`,
          ),
        );
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
      if (Array.isArray(row.files)) {
        for (const extraRel of row.files) {
          measured.push(extraRel);
          const extra = fileResult(run, extraRel);
          const expectedCode = extraExpectedCode(row, extraRel);
          if (!Number.isInteger(expectedCode)) {
            errors.push(
              err(
                caseId,
                "missing-negative",
                `${engineId} ${extraRel} has no expectedCode/fileExpectedCodes entry`,
              ),
            );
            continue;
          }
          const anchored = (extra?.diags || []).filter((diag) => diag.code === expectedCode);
          if (anchored.length === 0) {
            errors.push(
              err(
                caseId,
                "missing-negative",
                `${engineId} ${extraRel} lacked TS${expectedCode}; got ${JSON.stringify(extra?.diags || [])}`,
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
      if (Array.isArray(row.files)) {
        for (const extraRel of row.files) {
          measured.push(extraRel);
          const extra = fileResult(run, extraRel);
          const extraCode = extraExpectedCode(row, extraRel);
          if (!Number.isInteger(extraCode)) {
            errors.push(
              err(
                caseId,
                "missing-negative",
                `${engineId} ${extraRel} has no expectedCode/fileExpectedCodes entry`,
              ),
            );
            continue;
          }
          const extraAnchored = (extra?.diags || []).filter((diag) => diag.code === extraCode);
          if (extraAnchored.length === 0) {
            errors.push(
              err(
                caseId,
                "missing-negative",
                `${engineId} ${extraRel} lacked TS${extraCode}; got ${JSON.stringify(extra?.diags || [])}`,
              ),
            );
          }
        }
      }
    }
  }
  errors.push(...assertCheckCounts(run.checkCounts, engineId, measured, maxChecksPerFile));
  return errors;
}

function evaluateHarnessRun(
  run,
  probes,
  engineId,
  { maxChecksPerFile = 1, requireInstanceType = true } = {},
) {
  const errors = [];
  errors.push(...assertCleanTwin(run.positive.diags, { fileLabel: `${engineId} clean twin` }));
  const instance = run.positive.observations.types.Instance;
  if (requireInstanceType) {
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
  } else if (
    probes.definitionNeedle &&
    run.positive.observations.definition.name !== probes.definitionNeedle
  ) {
    errors.push(
      err(
        "STP1-harness",
        "missing-definition",
        `${engineId} definition target ${run.positive.observations.definition.name} is not ${probes.definitionNeedle}`,
      ),
    );
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
  if (nodeId === "STP3") {
    errors.push(...assertStp3Products(repoRoot, nodeLoad.manifest));
  }
  if (nodeId === "STP4") {
    errors.push(...assertStp4Products(repoRoot, nodeLoad.manifest));
  }
  if (nodeId === "STP6") {
    errors.push(...assertStp6Products(repoRoot, nodeLoad.manifest));
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
    const caseFiles = usesCaseFileRunner(nodeId);
    for (const engine of resolvedEngines) {
      const run = caseFiles
        ? engine.kind === "javascript"
          ? runJsStp2(engine, probes, repoRoot, cases)
          : await runNativeStp2(engine, probes, repoRoot, cases)
        : engine.kind === "javascript"
          ? runJsEngine(engine, probes, repoRoot)
          : await runNativeEngine(engine, probes, repoRoot);
      harnessRuns.push(run);
      if (caseFiles) {
        errors.push(...evaluateStp2Run(run, probes, cases, engine.id, { maxChecksPerFile }));
      } else {
        errors.push(
          ...evaluateHarnessRun(run, probes, engine.id, {
            maxChecksPerFile,
            // The harness Instance observation is Vue-constructor-shaped
            // (InstanceType<typeof Comp>); it does not apply to the
            // function-shaped Svelte Component of STP7/STS0, whose manifests
            // pin expectedInstanceType to the declared interface instead.
            requireInstanceType: nodeId !== "STP7" && nodeId !== "STS0",
          }),
        );
      }
    }
    if (harnessRuns.length === 0) {
      errors.push(err("STP1-harness", "missing-probes", "zero probe executions"));
    }
  }

  if (options.requireAll) {
    const required = mandatoryCasesFor(nodeLoad.manifest);
    for (const id of required) {
      if (!selected.includes(id)) {
        errors.push(
          err("STP1-zero-selection", "zero-cases", `mandatory case ${id} was not selected`),
        );
      }
    }
  }

  let capabilityRows = [];
  if (nodeId === "STP5") {
    const stp5 = await evaluateStp5Node({
      repoRoot,
      resolvedEngines,
      harnessRuns,
      skipProbes: options.skipProbes,
      runnable,
    });
    errors.push(...stp5.errors);
    capabilityRows = stp5.capabilityRows;
  }

  if (nodeId === "STP6") {
    const stp6 = await evaluateStp6Node({
      repoRoot,
      resolvedEngines,
      skipProbes: options.skipProbes,
    });
    errors.push(...stp6.errors);
  }

  if (nodeId === "STP7") {
    const stp7 = await evaluateStp7Node({
      repoRoot,
      resolvedEngines,
      skipProbes: options.skipProbes,
      runnable,
    });
    errors.push(...stp7.errors);
  }

  if (nodeId === "STP8") {
    const stp8 = await evaluateStp8Node({ repoRoot });
    errors.push(...stp8.errors);
  }

  if (nodeId === "STP9") {
    const stp9 = await evaluateStp9Node({ repoRoot });
    errors.push(...stp9.errors);
  }

  if (nodeId === "STS0") {
    const sts0 = await evaluateSts0Node({ repoRoot, resolvedEngines });
    errors.push(...sts0.errors);
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
    capabilityRows,
  });
}

export function probeMapperHost(engine, { helpTextHasMapperHost }) {
  const id = engine?.id || "unknown";
  const version = engine?.version || "unknown";
  if (!engine?.executable) {
    return {
      present: false,
      performed: false,
      method: null,
      evidence: `${id}@${version} mapper-host probe skipped: no executable`,
    };
  }
  const spawned =
    engine.kind === "native"
      ? spawnSync(engine.executable, ["--help"], {
          encoding: "utf8",
          windowsHide: true,
          timeout: 20000,
        })
      : spawnSync(process.execPath, [engine.executable, "--help"], {
          encoding: "utf8",
          windowsHide: true,
          timeout: 20000,
        });
  if (spawned.error && !spawned.stdout && !spawned.stderr) {
    return {
      present: false,
      performed: false,
      method: "tsc --help",
      evidence: `${id}@${version} tsc --help probe failed: ${spawned.error.message}`,
    };
  }
  const text = `${spawned.stdout || ""}${spawned.stderr || ""}`;
  const present = helpTextHasMapperHost(text);
  return {
    present,
    performed: true,
    method: "tsc --help",
    evidence: present
      ? "executable-selected-build"
      : `${id}@${version} tsc --help has no content mapper host (--runExternalCode / contentMappers)`,
  };
}

function observationFromRun(run) {
  return {
    diagnostics: (run?.negative?.diags || []).length > 0,
    hover: Boolean(run?.positive?.observations?.hover),
    definition: Boolean(run?.positive?.observations?.definition),
    references: (run?.positive?.observations?.references || 0) > 0,
    edits: (run?.positive?.observations?.edits || []).length > 0,
  };
}

async function evaluateStp7Node({ repoRoot, resolvedEngines, skipProbes, runnable }) {
  const protocolHref = pathToFileURL(
    repoPath(repoRoot, "tests/sfc-projection/STP7/protocol.mjs"),
  ).href;
  const protocol = await import(protocolHref);
  const errors = [];
  if (skipProbes) {
    const stp7 = await protocol.evaluateStp7();
    errors.push(...stp7.errors);
    return { errors };
  }
  if (!runnable) {
    errors.push(err("STP7-svelte-shape", "missing-probes", "STP7 omitted runnable probes"));
    return { errors };
  }
  if (resolvedEngines.length === 0) {
    errors.push(
      err("STP7-svelte-shape", "missing-probes", "STP7 selected zero engines for boundary proof"),
    );
    return { errors };
  }
  const jsEngine = resolvedEngines.find((engine) => engine.kind === "javascript");
  if (!jsEngine) {
    errors.push(
      err(
        "STP7-realm",
        "missing-js-engine",
        "STP7 live same-program ambient-leak check needs a javascript-kind engine; " +
          "run with --engine all instead of silently skipping assertRealmLeak",
      ),
    );
    return { errors };
  }
  const ts = loadJsTypeScript(jsEngine, repoRoot);
  const stp7 = await protocol.evaluateStp7({ ts });
  errors.push(...stp7.errors);
  return { errors };
}

async function evaluateStp8Node({ repoRoot }) {
  const protocolHref = pathToFileURL(
    repoPath(repoRoot, "tests/sfc-projection/STP8/protocol.mjs"),
  ).href;
  const protocol = await import(protocolHref);
  const stp8 = await protocol.evaluateStp8();
  return { errors: stp8.errors };
}

async function evaluateStp9Node({ repoRoot }) {
  const protocolHref = pathToFileURL(
    repoPath(repoRoot, "tests/sfc-projection/STP9/protocol.mjs"),
  ).href;
  const protocol = await import(protocolHref);
  const stp9 = await protocol.evaluateStp9({ repoRoot });
  return { errors: stp9.errors };
}

async function evaluateSts0Node({ repoRoot }) {
  const protocolHref = pathToFileURL(
    repoPath(repoRoot, "tests/sfc-projection/STS0/protocol.mjs"),
  ).href;
  const protocol = await import(protocolHref);
  // STS0's live profile behavior runs through the owned CCA1I backend gate
  // in verter_session on BOTH claimed engines (ts-js 6.0.3 and ts-native
  // 7.0.2); the protocol executes it, so no single JS engine is selected
  // for the profile path here.
  const sts0 = await protocol.evaluateSts0({ repoRoot });
  return { errors: sts0.errors };
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
    errors.push(...protocol.validateStp5Products());
    return { errors, capabilityRows: [] };
  }
  if (!runnable) {
    errors.push(err("STP5-capability", "missing-probes", "STP5 omitted runnable probes"));
    return { errors, capabilityRows: [] };
  }
  if (resolvedEngines.length === 0) {
    errors.push(
      err("STP5-capability", "missing-probes", "STP5 selected zero engines for capability rows"),
    );
    return { errors, capabilityRows: [] };
  }
  const engines = resolvedEngines.map((engine) => {
    const probe = probeMapperHost(engine, protocol);
    const run = harnessRuns.find((row) => row.engine === engine.id);
    return {
      mapperHostPresent: probe.present,
      mapperHostEvidence: probe.evidence,
      observation: observationFromRun(run),
      engineId: engine.id,
      engineVersion: engine.version,
    };
  });
  const stp5 = await protocol.evaluateStp5({ engines });
  errors.push(...stp5.errors);
  return { errors, capabilityRows: stp5.capabilityRows };
}

async function evaluateStp6Node({ repoRoot, resolvedEngines, skipProbes }) {
  const protocolHref = pathToFileURL(
    repoPath(repoRoot, "tests/sfc-projection/STP6/protocol.mjs"),
  ).href;
  const protocol = await import(protocolHref);
  const jsPin = resolvedEngines.find((engine) => engine.kind === "javascript");
  let ts = null;
  if (jsPin) {
    ts = loadJsTypeScript(jsPin, repoRoot);
  }
  const stp6 = await protocol.evaluateStp6({
    ts,
    repoRoot,
    skipLive: skipProbes || !ts,
  });
  const errors = [...stp6.errors];
  if (!skipProbes && !ts) {
    errors.push(
      err(
        "STP6-resolution",
        "missing-probes",
        "STP6 live packed-consumer resolution needs the ts-js engine",
      ),
    );
  }
  return { errors };
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
  capabilityRows = [],
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
  const probeFiles = [
    nodeManifest?.probes?.positive,
    nodeManifest?.probes?.negative,
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
    "tests/sfc-projection/STP3/probes/accept-coupled.ts",
    "tests/sfc-projection/STP3/probes/reject-wrong-channel.ts",
    "tests/sfc-projection/STP3/probes/reject-inference-only-split.ts",
    "tests/sfc-projection/STP3/probes/reject-order-independent.ts",
    "tests/sfc-projection/STP3/probes/reject-ordered-merge.ts",
    "tests/sfc-projection/STP3/probes/reject-fresh-uses.ts",
    "tests/sfc-projection/STP4/probes/accept-js-unchecked.ts",
    "tests/sfc-projection/STP4/probes/accept-js-checked.ts",
    "tests/sfc-projection/STP4/probes/accept-tsx-authored.tsx",
    "tests/sfc-projection/STP4/probes/reject-supplemental-import.ts",
    "tests/sfc-projection/STP4/probes/accept-external-owner.ts",
    "tests/sfc-projection/STP4/probes/reject-illegal-vue.ts",
    "tests/sfc-projection/STP6/probes/accept-package-instance.ts",
    "tests/sfc-projection/STP6/probes/accept-package-generics.ts",
    "tests/sfc-projection/STP6/probes/accept-decl-map.ts",
    "tests/sfc-projection/STP6/probes/accept-resolution.ts",
    "tests/sfc-projection/STP6/probes/reject-hidden-metadata.ts",
    "tests/sfc-projection/STP6/probes/reject-closure.ts",
    "tests/sfc-projection/STP7/probes/positive.ts",
    "tests/sfc-projection/STP7/probes/negative.ts",
    "tests/sfc-projection/STP8/probes/positive.ts",
    "tests/sfc-projection/STP8/probes/negative.ts",
    "tests/sfc-projection/STP9/probes/positive.ts",
    "tests/sfc-projection/STP9/probes/negative.ts",
    "tests/sfc-projection/STS0/probes/positive.ts",
    "tests/sfc-projection/STS0/probes/negative.ts",
    "tests/sfc-projection/STS0/probes/state-module.svelte.ts",
  ].filter(Boolean);
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
    capabilityRows,
  };
}

export function selectedCaseIds(result) {
  return [...(result.selectedCaseIds || [])];
}

const HELP = `ProjectionProbeRunner — SFC projection probe harness (STP1 inventory, STP2 constructor, STP3 coupled inference, STP4 dialect topology, STP5 mapper, STP6 packed consumer, STP7 svelte boundary, STP8 ABI ratification, STP9 projection plan, STS0 Svelte profile lock)

USAGE
  node scripts/sfc-projection/verify-node.mjs --node STP1|STP2|STP3|STP4|STP5|STP6|STP7|STP8|STP9|STS0 [--engine all|ts-js|ts-native] [--require-all] [--json]

Rejects absent/empty manifests, zero selected cases, missing inventory fixtures,
vacuous any/never type matches, unrelated clean-twin diagnostics, a substituted
executable under the same engine label, omitted probes, non-exact type matches,
missing references, and duplicate file checks. STP5 additionally rejects Alias
rename codecs, query-snapshot reuse, Verter-as-stock-CLI claims, duplicate
guards, version-label capability, and dormant-product complete claims. STP6
rejects unpublished-metadata precision and private virtual declaration paths.
STP7 rejects Vue-constructor shims on Svelte Component, Vue-only shared records,
one-way hole maps, same-program ambient-isolation claims, and full Astro/MDX/Lit
support advertisements; its live realm check requires a javascript-kind engine,
so a native-only --engine selection is rejected rather than skipped. STP8
rejects unmatched, fabricated, or uncited feasibility rows; TypeScript 5.8-only
toy evidence or a shrunken engine denominator; checker-only public shapes that
respell InstanceType or remap Vue utilities; and event/slot inference
contributors postponed until after specialization. STP9 rejects TypeInfo or
assignability during plan construction, complete-cache admission of malformed
or unknown syntax, and use identities that shift under comment or unrelated
sibling insertion. STS0 rejects a current
supported Svelte feature with no mandatory owning row, runes mislabeled as
legacy, a Vue constructor or Vue event/model/ref convention required for a
modern Svelte Component, unspecified profile checking/publishing behavior,
options silently ignored instead of failing closed, and latest-tool or floating
engine/framework claims without pinned provenance.
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
