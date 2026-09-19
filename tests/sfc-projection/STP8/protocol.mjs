/**
 * STP8 evidence-based ABI and topology ratification helpers.
 *
 * Ratifies the constructor-first Vue projection ABI and the frozen topology
 * against every predecessor feasibility row and the selected real engine/Vue
 * pins. Rejects toy-evidence ratification, checker-only public shape
 * contamination, and postponed inference contributors. No production emit.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const STP8_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(STP8_DIR, "../../..");

export const PROTOCOL_VERSION = 1;

export const CANONICAL_INSTANCE_TYPE = "InstanceType<typeof Comp>";
export const DEFAULT_INSTANCE_PRINT = "Comp<unknown>";

export const STP8_MANDATORY_CASES = Object.freeze([
  "STP8-complete-evidence",
  "STP8-partial-ratify",
  "STP8-abi-contamination",
  "STP8-inference-contract",
]);

/** Every predecessor mandatory feasibility row, with its owning ledger. */
export const PREDECESSOR_ROWS = Object.freeze([
  {
    node: "STP2",
    case: "STP2-instance-concrete",
    source: "tests/sfc-projection/STP2/products/instance-type-compatibility-table.json",
  },
  {
    node: "STP2",
    case: "STP2-instance-generic",
    source: "tests/sfc-projection/STP2/products/instance-type-compatibility-table.json",
  },
  {
    node: "STP2",
    case: "STP2-instance-explicit",
    source: "tests/sfc-projection/STP2/products/instance-type-compatibility-table.json",
  },
  {
    node: "STP2",
    case: "STP2-constructor-escape",
    source: "tests/sfc-projection/STP2/products/instance-type-compatibility-table.json",
  },
  {
    node: "STP2",
    case: "STP2-vue-utilities",
    source: "tests/sfc-projection/STP2/products/instance-type-compatibility-table.json",
  },
  {
    node: "STP2",
    case: "STP2-not-callable",
    source: "tests/sfc-projection/STP2/products/instance-type-compatibility-table.json",
  },
  {
    node: "STP2",
    case: "STP2-constructor-inferred",
    source: "tests/sfc-projection/STP2/products/instance-type-compatibility-table.json",
  },
  {
    node: "STP2",
    case: "STP2-explicit-input-mismatch",
    source: "tests/sfc-projection/STP2/products/instance-type-compatibility-table.json",
  },
  {
    node: "STP3",
    case: "STP3-coupled",
    source: "tests/sfc-projection/STP3/products/coupled-inference-evidence.json",
  },
  {
    node: "STP3",
    case: "STP3-wrong-channel",
    source: "tests/sfc-projection/STP3/products/coupled-inference-evidence.json",
  },
  {
    node: "STP3",
    case: "STP3-inference-only-channel",
    source: "tests/sfc-projection/STP3/products/coupled-inference-evidence.json",
  },
  {
    node: "STP3",
    case: "STP3-order-independent",
    source: "tests/sfc-projection/STP3/products/coupled-inference-evidence.json",
  },
  {
    node: "STP3",
    case: "STP3-ordered-merge",
    source: "tests/sfc-projection/STP3/products/coupled-inference-evidence.json",
  },
  {
    node: "STP3",
    case: "STP3-fresh-uses",
    source: "tests/sfc-projection/STP3/products/coupled-inference-evidence.json",
  },
  {
    node: "STP4",
    case: "STP4-js-unchecked",
    source: "tests/sfc-projection/STP4/products/dialect-topology-evidence.json",
  },
  {
    node: "STP4",
    case: "STP4-js-checked",
    source: "tests/sfc-projection/STP4/products/dialect-topology-evidence.json",
  },
  {
    node: "STP4",
    case: "STP4-tsx-authored",
    source: "tests/sfc-projection/STP4/products/dialect-topology-evidence.json",
  },
  {
    node: "STP4",
    case: "STP4-supplemental-import",
    source: "tests/sfc-projection/STP4/products/dialect-topology-evidence.json",
  },
  {
    node: "STP4",
    case: "STP4-external-owner",
    source: "tests/sfc-projection/STP4/products/dialect-topology-evidence.json",
  },
  {
    node: "STP4",
    case: "STP4-illegal-vue",
    source: "tests/sfc-projection/STP4/products/dialect-topology-evidence.json",
  },
  { node: "STP5", case: "STP5-encoding", source: "tests/sfc-projection/STP5/manifest.json" },
  { node: "STP5", case: "STP5-guard-duplicate", source: "tests/sfc-projection/STP5/manifest.json" },
  { node: "STP5", case: "STP5-alias-edit", source: "tests/sfc-projection/STP5/manifest.json" },
  { node: "STP5", case: "STP5-stale-target", source: "tests/sfc-projection/STP5/manifest.json" },
  { node: "STP5", case: "STP5-raw-cli", source: "tests/sfc-projection/STP5/manifest.json" },
  { node: "STP5", case: "STP5-capability", source: "tests/sfc-projection/STP5/manifest.json" },
  {
    node: "STP6",
    case: "STP6-package-instance",
    source: "tests/sfc-projection/STP6/products/packed-consumer-feasibility.json",
  },
  {
    node: "STP6",
    case: "STP6-package-generics",
    source: "tests/sfc-projection/STP6/products/packed-consumer-feasibility.json",
  },
  {
    node: "STP6",
    case: "STP6-hidden-metadata",
    source: "tests/sfc-projection/STP6/products/packed-consumer-feasibility.json",
  },
  {
    node: "STP6",
    case: "STP6-closure",
    source: "tests/sfc-projection/STP6/products/packed-consumer-feasibility.json",
  },
  {
    node: "STP6",
    case: "STP6-decl-map",
    source: "tests/sfc-projection/STP6/products/packed-consumer-feasibility.json",
  },
  {
    node: "STP6",
    case: "STP6-resolution",
    source: "tests/sfc-projection/STP6/products/packed-consumer-feasibility.json",
  },
  {
    node: "STP7",
    case: "STP7-svelte-shape",
    source: "tests/sfc-projection/STP7/products/second-framework-boundary-evidence.json",
  },
  {
    node: "STP7",
    case: "STP7-holes",
    source: "tests/sfc-projection/STP7/products/second-framework-boundary-evidence.json",
  },
  {
    node: "STP7",
    case: "STP7-realm",
    source: "tests/sfc-projection/STP7/products/second-framework-boundary-evidence.json",
  },
  {
    node: "STP7",
    case: "STP7-reuse",
    source: "tests/sfc-projection/STP7/products/second-framework-boundary-evidence.json",
  },
  {
    node: "STP7",
    case: "STP7-scope-claim",
    source: "tests/sfc-projection/STP7/products/second-framework-boundary-evidence.json",
  },
]);

const PREDECESSOR_CASE_IDS = new Set(PREDECESSOR_ROWS.map((row) => `${row.node}:${row.case}`));
const PREDECESSOR_CASE_NAMES = new Set(PREDECESSOR_ROWS.map((row) => row.case));

export function err(caseId, code, message) {
  return { caseId, code, message };
}

export function repoPath(rel) {
  return path.resolve(REPO_ROOT, rel);
}

export function loadStp8Product(name) {
  return JSON.parse(fs.readFileSync(path.join(STP8_DIR, "products", name), "utf8"));
}

function loadStp8Manifest() {
  try {
    return JSON.parse(fs.readFileSync(path.join(STP8_DIR, "manifest.json"), "utf8"));
  } catch {
    return null;
  }
}

export function loadEngineMatrix() {
  return JSON.parse(
    fs.readFileSync(repoPath("tests/sfc-projection/STP1/products/engine-matrix.json"), "utf8"),
  );
}

/**
 * STP8-complete-evidence: every predecessor mandatory feasibility row must be
 * matched to selected candidate evidence in a ledger that actually contains
 * the case. Uncited, fabricated, or unknown rows are rejected.
 */
export function assertCompleteEvidence(abi, { repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  const rows = Array.isArray(abi?.feasibilityRows) ? abi.feasibilityRows : [];
  const matched = new Set(rows.map((row) => `${row.node}:${row.case}`));
  for (const required of PREDECESSOR_ROWS) {
    if (!matched.has(`${required.node}:${required.case}`)) {
      errors.push(
        err(
          "STP8-complete-evidence",
          "unmatched-row",
          `feasibility row ${required.node}:${required.case} has no selected candidate evidence`,
        ),
      );
    }
  }
  for (const row of rows) {
    const key = `${row.node}:${row.case}`;
    if (!PREDECESSOR_CASE_IDS.has(key)) {
      errors.push(
        err(
          "STP8-complete-evidence",
          "unknown-row",
          `feasibility row ${key} is not a predecessor mandatory case`,
        ),
      );
      continue;
    }
    const abs = path.resolve(repoRoot, row.source);
    if (!fs.existsSync(abs)) {
      errors.push(
        err("STP8-complete-evidence", "uncited-row", `evidence ledger missing at ${row.source}`),
      );
      continue;
    }
    const ledger = fs.readFileSync(abs, "utf8");
    if (!ledger.includes(`"${row.case}"`)) {
      errors.push(
        err(
          "STP8-complete-evidence",
          "uncited-row",
          `${row.source} does not record case ${row.case}`,
        ),
      );
    }
  }
  return errors;
}

/**
 * STP8-partial-ratify: ratification must rest on the selected real engines
 * and Vue types from the engine matrix, never on a toy or TypeScript 5.8-only
 * substitute, and must not shrink the denominator by dropping an engine.
 */
export function assertEvidencePins(abi, matrix = loadEngineMatrix()) {
  const errors = [];
  const engines = Array.isArray(abi?.engines) ? abi.engines : [];
  const matrixEngines = matrix?.engines || [];
  for (const engine of engines) {
    const pin = matrixEngines.find((row) => row.id === engine.id);
    if (!pin || pin.version !== engine.version) {
      errors.push(
        err(
          "STP8-partial-ratify",
          "toy-evidence",
          `engine ${engine.id}@${engine.version} is not a selected real engine pin`,
        ),
      );
    }
  }
  for (const pin of matrixEngines) {
    if (!engines.some((engine) => engine.id === pin.id)) {
      errors.push(
        err(
          "STP8-partial-ratify",
          "denominator-shrunk",
          `selected engine ${pin.id} is missing from the ratification evidence`,
        ),
      );
    }
  }
  const admittedVue = matrix?.vue?.admitted?.[0];
  if (!admittedVue || abi?.vue?.version !== admittedVue.version) {
    errors.push(
      err(
        "STP8-partial-ratify",
        "toy-vue",
        `vue pin ${abi?.vue?.version} is not the admitted matrix vue ${admittedVue?.version}`,
      ),
    );
  }
  if (abi?.toyEvidenceAdmitted !== false) {
    errors.push(
      err("STP8-partial-ratify", "toy-evidence", "ratification must not admit toy evidence"),
    );
  }
  return errors;
}

/**
 * STP8-abi-contamination: a checker-only public shape must not change
 * InstanceType or Vue utility behavior. Every consumer surface observes the
 * identical public spelling and untouched utilities.
 */
export function assertAbiContamination(surface) {
  const errors = [];
  if (!surface || typeof surface !== "object") {
    return [err("STP8-abi-contamination", "missing-surface", "public surface record is missing")];
  }
  if (surface.instanceTypeSpelling !== CANONICAL_INSTANCE_TYPE) {
    errors.push(
      err(
        "STP8-abi-contamination",
        "instance-type-respelled",
        `instance type spelling must stay ${CANONICAL_INSTANCE_TYPE}`,
      ),
    );
  }
  if (surface.instanceTypePreserved !== true) {
    errors.push(
      err("STP8-abi-contamination", "instance-type-respelled", "InstanceType must be preserved"),
    );
  }
  if (surface.checkerOnlyPublicShape !== false) {
    errors.push(
      err(
        "STP8-abi-contamination",
        "checker-only-shape",
        "a checker-only public shape change is forbidden",
      ),
    );
  }
  if (surface.surfacesIdentical !== true) {
    errors.push(
      err("STP8-abi-contamination", "checker-only-shape", "consumer surfaces must be identical"),
    );
  }
  for (const row of surface.surfaces || []) {
    if (row.instanceType !== surface.instanceTypeSpelling) {
      errors.push(
        err(
          "STP8-abi-contamination",
          "checker-only-shape",
          `surface ${row.id} spells InstanceType as ${row.instanceType}`,
        ),
      );
    }
    if (row.utilitiesUntouched !== true) {
      errors.push(
        err(
          "STP8-abi-contamination",
          "utility-remap",
          `surface ${row.id} remaps Vue utility behavior`,
        ),
      );
    }
  }
  return errors;
}

/**
 * STP8-inference-contract: an event or slot inference contributor must never
 * be postponed until after specialization; contributing channels stay in the
 * inference transaction.
 */
export function assertInferenceContract(abi) {
  const errors = [];
  const inference = abi?.inference;
  const witness = abi?.richerInferenceWitness;
  if (!inference || !Array.isArray(inference.inferenceTransaction)) {
    return [
      err("STP8-inference-contract", "missing-transaction", "inference transaction is missing"),
    ];
  }
  const transaction = new Set(inference.inferenceTransaction);
  for (const channel of inference.contributingChannels || []) {
    if (!transaction.has(channel)) {
      errors.push(
        err(
          "STP8-inference-contract",
          "postponed-contributor",
          `contributing channel ${channel} was postponed out of the inference transaction`,
        ),
      );
    }
  }
  for (const deferred of inference.postSpecializationOnly || []) {
    if (transaction.has(deferred)) {
      errors.push(
        err(
          "STP8-inference-contract",
          "postponed-contributor",
          `${deferred} is both a transaction contributor and deferred post-specialization`,
        ),
      );
    }
  }
  if (inference.selectedWitness !== "single-public-constructor-whole-signature") {
    errors.push(
      err(
        "STP8-inference-contract",
        "split-witness",
        "selected witness must stay the whole-signature public constructor",
      ),
    );
  }
  if (!witness || witness.typeOnly !== true || witness.nonRuntime !== true) {
    errors.push(
      err(
        "STP8-inference-contract",
        "runtime-witness",
        "the richer inference witness must stay type-only and non-runtime",
      ),
    );
  }
  if (witness?.secondCheckerAbi !== false) {
    errors.push(
      err("STP8-inference-contract", "second-checker-abi", "no speculative second checker ABI"),
    );
  }
  if (witness?.perCaseFallback !== false) {
    errors.push(err("STP8-inference-contract", "per-case-fallback", "no silent per-case fallback"));
  }
  return errors;
}

/** The ratified fixture keeps the accepted encoding: one public constructor. */
export function assertRatifiedFixture(source) {
  const errors = [];
  if (!source || typeof source !== "string") {
    return [err("STP8-complete-evidence", "missing-fixture", "ratified fixture is missing")];
  }
  if (!source.includes("export declare class Comp")) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "constructor-first",
        "fixture must spell the constructor-first declare class Comp",
      ),
    );
  }
  const constructorCount = (source.match(/constructor\(/g) || []).length;
  if (constructorCount !== 1) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "overload-claim",
        `fixture must declare exactly one public constructor, found ${constructorCount}`,
      ),
    );
  }
  if (!/export interface Comp[^\n]*extends ComponentPublicInstance/.test(source)) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "missing-witness",
        "fixture must merge the ComponentPublicInstance inference witness",
      ),
    );
  }
  if (!source.includes("export default Comp")) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "constructor-first",
        "default export must remain the constructor itself",
      ),
    );
  }
  for (const forbidden of ["export default function", "declare function Comp", "props?: any"]) {
    if (source.includes(forbidden)) {
      errors.push(
        err("STP8-complete-evidence", "callable-default", `fixture must not encode ${forbidden}`),
      );
    }
  }
  return errors;
}

// The harness prints InstanceType<typeof Comp> for the default specialization
// as the merged interface reference Comp<unknown>; evaluateStp8 pins the
// manifest's probes.expectedInstanceType to that accepted ABI print.

export function validateStp8Products({
  abi = loadStp8Product("accepted-vue-constructor-abi.json"),
  architecture = loadStp8Product("accepted-projection-architecture.json"),
  topology = loadStp8Product("accepted-topology-matrix.json"),
  helperAbi = loadStp8Product("helper-abi.v1.json"),
  evidenceText = fs.readFileSync(path.join(STP8_DIR, "../evidence/STP8/cases.md"), "utf8"),
} = {}) {
  const errors = [];
  if (abi?.schema !== "AcceptedVueConstructorABI") {
    errors.push(
      err("STP8-complete-evidence", "removed-fixture", "AcceptedVueConstructorABI schema"),
    );
  }
  if (architecture?.schema !== "AcceptedProjectionArchitecture") {
    errors.push(
      err("STP8-complete-evidence", "removed-fixture", "AcceptedProjectionArchitecture schema"),
    );
  }
  if (topology?.schema !== "AcceptedTopologyMatrix") {
    errors.push(err("STP8-complete-evidence", "removed-fixture", "AcceptedTopologyMatrix schema"));
  }
  if (helperAbi?.schema !== "HelperABI" || helperAbi?.version !== 1) {
    errors.push(err("STP8-complete-evidence", "removed-fixture", "HelperABI v1 schema"));
  }
  if (!String(abi?.selectedCandidate?.spelling || "").includes("declare class Comp")) {
    errors.push(
      err("STP8-complete-evidence", "removed-fixture", "missing selected constructor spelling"),
    );
  }
  if (abi?.selectedCandidate?.constructorFirst !== true) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "removed-fixture",
        "selected candidate must be constructor-first",
      ),
    );
  }
  errors.push(...assertCompleteEvidence(abi));
  errors.push(...assertEvidencePins(abi));
  errors.push(...assertAbiContamination(abi?.publicSurface));
  errors.push(...assertInferenceContract(abi));
  const helperIds = new Set((helperAbi?.helpers || []).map((row) => row.id));
  for (const id of [
    "vue-createApp",
    "vue-h",
    "vue-tsx",
    "component-public-instance",
    "supplemental-template-specifier",
  ]) {
    if (!helperIds.has(id)) {
      errors.push(
        err("STP8-complete-evidence", "removed-fixture", `HelperABI v1 missing helper ${id}`),
      );
    }
  }
  for (const row of helperAbi?.helpers || []) {
    if (!PREDECESSOR_CASE_NAMES.has(row.case)) {
      errors.push(
        err(
          "STP8-complete-evidence",
          "unknown-row",
          `helper ${row.id} cites unknown case ${row.case}`,
        ),
      );
    }
  }
  if (helperAbi?.frozen !== true || topology?.frozen !== true) {
    errors.push(
      err("STP8-complete-evidence", "removed-fixture", "HelperABI v1 and topology must be frozen"),
    );
  }
  if (topology?.supplementalFiles?.namedImportTarget !== false) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "removed-fixture",
        "topology must keep supplemental files off named import targets",
      ),
    );
  }
  const dialectIds = new Set((topology?.dialects || []).map((row) => row.id));
  for (const dialect of ["ts", "tsx", "js", "jsx"]) {
    if (!dialectIds.has(dialect)) {
      errors.push(
        err("STP8-complete-evidence", "removed-fixture", `topology missing dialect ${dialect}`),
      );
    }
  }
  if (!String(abi?.ac3Rationale || "").trim() || !String(abi?.ac4Rationale || "").trim()) {
    errors.push(
      err("STP8-complete-evidence", "removed-fixture", "missing AC3/AC4 untouched-owner rationale"),
    );
  }
  if (!/[0-9a-f]{40}/.test(evidenceText)) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "missing-source-revision",
        "STP8 evidence must record a 40-character source revision",
      ),
    );
  }
  if (!/6\.0\.3/.test(evidenceText) || !/7\.0\.2/.test(evidenceText)) {
    errors.push(
      err(
        "STP8-partial-ratify",
        "missing-engine-pins",
        "STP8 evidence must record ts-js 6.0.3 and ts-native 7.0.2 engine pins",
      ),
    );
  }
  return errors;
}

export const DIRTY_DROP_ROW = Object.freeze({
  node: "STP3",
  case: "STP3-order-independent",
  source: "tests/sfc-projection/STP3/products/coupled-inference-evidence.json",
});

export const DIRTY_FABRICATED_ROW = Object.freeze({
  node: "STP2",
  case: "STP2-fabricated-row",
  source: "tests/sfc-projection/STP2/products/instance-type-compatibility-table.json",
});

export const DIRTY_UNCITED_ROW = Object.freeze({
  node: "STP6",
  case: "STP6-package-instance",
  source: "tests/sfc-projection/STP7/products/second-framework-boundary-evidence.json",
});

export const DIRTY_TOY_ENGINES = Object.freeze([
  { id: "ts-js", package: "typescript", version: "5.8.3", role: "toy" },
]);

export const DIRTY_SURFACE = Object.freeze({
  defaultExport: "constructor-shaped-declare-class",
  instanceTypeSpelling: CANONICAL_INSTANCE_TYPE,
  instanceTypePreserved: true,
  surfaces: [
    { id: "checker-projection", instanceType: "CompCheckerInstance", utilitiesUntouched: false },
    { id: "packed-consumer", instanceType: CANONICAL_INSTANCE_TYPE, utilitiesUntouched: true },
  ],
  surfacesIdentical: false,
  checkerOnlyPublicShape: true,
});

export const DIRTY_INFERENCE = Object.freeze({
  selectedWitness: "single-public-constructor-whole-signature",
  inferenceTransaction: ["props-items", "props-label", "expose-reset"],
  contributingChannels: ["props-items", "emit-change", "slot-default", "expose-first"],
  postSpecializationOnly: ["excess-key-and-fallthrough-validation", "hover-definition-references"],
});

export const DIRTY_WITNESS = Object.freeze({
  admitted: true,
  typeOnly: true,
  nonRuntime: false,
  secondCheckerAbi: true,
  perCaseFallback: true,
});

export function cloneJson(value) {
  return structuredClone(value);
}

export function evaluateRejectTwins() {
  const errors = [];
  const abi = loadStp8Product("accepted-vue-constructor-abi.json");

  const cleanEvidence = assertCompleteEvidence(abi);
  if (cleanEvidence.length) errors.push(...cleanEvidence);
  const dropped = cloneJson(abi);
  dropped.feasibilityRows = dropped.feasibilityRows.filter(
    (row) => !(row.node === DIRTY_DROP_ROW.node && row.case === DIRTY_DROP_ROW.case),
  );
  if (assertCompleteEvidence(dropped).length === 0) {
    errors.push(
      err("STP8-complete-evidence", "missed-row", "dropped feasibility row was not rejected"),
    );
  }
  const fabricated = cloneJson(abi);
  fabricated.feasibilityRows.push(DIRTY_FABRICATED_ROW);
  if (assertCompleteEvidence(fabricated).length === 0) {
    errors.push(
      err("STP8-complete-evidence", "missed-row", "fabricated feasibility row was not rejected"),
    );
  }
  const uncited = cloneJson(abi);
  const row = uncited.feasibilityRows.find((entry) => entry.case === DIRTY_UNCITED_ROW.case);
  if (row) row.source = DIRTY_UNCITED_ROW.source;
  if (assertCompleteEvidence(uncited).length === 0) {
    errors.push(
      err("STP8-complete-evidence", "missed-row", "uncited feasibility row was not rejected"),
    );
  }

  const cleanPins = assertEvidencePins(abi);
  if (cleanPins.length) errors.push(...cleanPins);
  const toy = cloneJson(abi);
  toy.engines = DIRTY_TOY_ENGINES;
  if (assertEvidencePins(toy).length === 0) {
    errors.push(
      err("STP8-partial-ratify", "missed-toy", "TypeScript 5.8-only toy evidence was not rejected"),
    );
  }
  const shrunk = cloneJson(abi);
  shrunk.engines = shrunk.engines.filter((engine) => engine.id !== "ts-native");
  if (assertEvidencePins(shrunk).length === 0) {
    errors.push(
      err(
        "STP8-partial-ratify",
        "missed-toy",
        "denominator-shrunk engine selection was not rejected",
      ),
    );
  }
  const toyVue = cloneJson(abi);
  toyVue.vue = { package: "vue", version: "3.0.0", source: "package.json" };
  if (assertEvidencePins(toyVue).length === 0) {
    errors.push(err("STP8-partial-ratify", "missed-toy", "toy vue pin was not rejected"));
  }

  if (assertAbiContamination(abi.publicSurface).length) {
    errors.push(...assertAbiContamination(abi.publicSurface));
  }
  if (assertAbiContamination(DIRTY_SURFACE).length === 0) {
    errors.push(
      err(
        "STP8-abi-contamination",
        "missed-contamination",
        "checker-only public shape was not rejected",
      ),
    );
  }

  if (assertInferenceContract(abi).length) {
    errors.push(...assertInferenceContract(abi));
  }
  const postponed = cloneJson(abi);
  postponed.inference.postSpecializationOnly.push("emit-change");
  if (assertInferenceContract(postponed).length === 0) {
    errors.push(
      err(
        "STP8-inference-contract",
        "missed-postponed",
        "postponed event contributor was not rejected",
      ),
    );
  }
  const droppedSlot = cloneJson(abi);
  droppedSlot.inference.inferenceTransaction = droppedSlot.inference.inferenceTransaction.filter(
    (channel) => channel !== "slot-default",
  );
  if (assertInferenceContract(droppedSlot).length === 0) {
    errors.push(
      err(
        "STP8-inference-contract",
        "missed-postponed",
        "postponed slot contributor was not rejected",
      ),
    );
  }
  const runtimeWitness = cloneJson(abi);
  runtimeWitness.richerInferenceWitness = DIRTY_WITNESS;
  if (assertInferenceContract(runtimeWitness).length === 0) {
    errors.push(
      err(
        "STP8-inference-contract",
        "missed-postponed",
        "runtime second-checker witness was not rejected",
      ),
    );
  }
  return errors;
}

export function evaluateStp8() {
  const errors = [];
  errors.push(...validateStp8Products());
  const fixtureAbs = path.join(STP8_DIR, "probes", "components", "Ratified.vue.d.ts");
  const fixture = fs.readFileSync(fixtureAbs, "utf8");
  errors.push(...assertRatifiedFixture(fixture));
  const expected = loadStp8Manifest()?.probes?.expectedInstanceType;
  if (expected !== DEFAULT_INSTANCE_PRINT) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "instance-type-drift",
        `manifest probes.expectedInstanceType ${JSON.stringify(expected)} must pin the default specialization print ${DEFAULT_INSTANCE_PRINT}`,
      ),
    );
  }
  errors.push(...evaluateRejectTwins());
  return { errors };
}
