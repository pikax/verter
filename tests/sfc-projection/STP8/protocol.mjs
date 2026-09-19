/**
 * STP8 evidence-based ABI and topology ratification helpers.
 *
 * Ratifies the constructor-first Vue projection ABI (the two-binder Comp<T, U>
 * family selected from the STP3 coupled-inference evidence) and the frozen
 * topology against every predecessor feasibility row and the selected real
 * engine/Vue pins. Required rows are derived from the canonical
 * NODE_MANDATORY_CASES ledgers and joined to the charter §4 named predecessor
 * products, never restated here. Rejects toy-evidence ratification,
 * checker-only public shape contamination (observed through the inherited
 * STP2 vue-utilities recipe), and postponed inference contributors. No
 * production emit.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { NODE_MANDATORY_CASES } from "../../../scripts/sfc-projection/node-mandatory-cases.mjs";

const STP8_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(STP8_DIR, "../../..");

export const PROTOCOL_VERSION = 1;

export const CANONICAL_INSTANCE_TYPE = "InstanceType<typeof Comp>";
export const DEFAULT_INSTANCE_PRINT = "Comp<unknown, unknown>";

/** The job baseline (STP7 merge) this node's branch started from. */
export const BASELINE_INPUT_SNAPSHOT = "6e777f11e3db1d8615d813d36d161aaa986c6dc8";
/** The commit that introduced the ratified STP8 products. */
export const RATIFICATION_CANDIDATE = "95175044e97296c6d07045e5e4d4fdc562904e50";

export const STP8_MANDATORY_CASES = Object.freeze([
  "STP8-complete-evidence",
  "STP8-partial-ratify",
  "STP8-abi-contamination",
  "STP8-inference-contract",
]);

/**
 * Charter §4 named predecessor products per node. `caseField` names the array
 * that records that node's mandatory case ids: the evidence ledgers hold
 * `{ id }` objects in `cases`/`rows`, while the STP5 manifest holds a plain
 * string list in `mandatoryCases`.
 */
export const PREDECESSOR_PRODUCTS = Object.freeze({
  STP2: Object.freeze([
    Object.freeze({
      file: "tests/sfc-projection/STP2/products/constructor-compatibility-evidence.json",
      schema: "ConstructorCompatibilityEvidence",
    }),
    Object.freeze({
      file: "tests/sfc-projection/STP2/products/instance-type-compatibility-table.json",
      schema: "InstanceTypeCompatibilityTable",
      caseField: "rows",
    }),
  ]),
  STP3: Object.freeze([
    Object.freeze({
      file: "tests/sfc-projection/STP3/products/coupled-inference-evidence.json",
      schema: "CoupledInferenceEvidence",
      caseField: "cases",
    }),
    Object.freeze({
      file: "tests/sfc-projection/STP3/products/inference-witness-selection.json",
      schema: "InferenceWitnessSelection",
    }),
  ]),
  STP4: Object.freeze([
    Object.freeze({
      file: "tests/sfc-projection/STP4/products/dialect-topology-evidence.json",
      schema: "DialectTopologyEvidence",
      caseField: "cases",
    }),
    Object.freeze({
      file: "tests/sfc-projection/STP4/products/projection-topology-decision-inputs.json",
      schema: "ProjectionTopologyDecisionInputs",
    }),
  ]),
  STP5: Object.freeze([
    Object.freeze({
      file: "tests/sfc-projection/STP5/products/mapper-capability-evidence.json",
      schema: "MapperCapabilityEvidence",
    }),
    Object.freeze({
      file: "tests/sfc-projection/STP5/products/diagnostic-origin-policy.json",
      schema: "DiagnosticOriginPolicy",
    }),
    Object.freeze({
      file: "tests/sfc-projection/STP5/products/observation-role-policy.json",
      schema: "ObservationRolePolicy",
    }),
    Object.freeze({
      file: "tests/sfc-projection/STP5/manifest.json",
      schema: "ProbeManifest",
      caseField: "mandatoryCases",
    }),
  ]),
  STP6: Object.freeze([
    Object.freeze({
      file: "tests/sfc-projection/STP6/products/packed-consumer-feasibility.json",
      schema: "PackedConsumerFeasibility",
      caseField: "cases",
    }),
    Object.freeze({
      file: "tests/sfc-projection/STP6/products/public-dependency-closure-policy.json",
      schema: "PublicDependencyClosurePolicy",
    }),
  ]),
  STP7: Object.freeze([
    Object.freeze({
      file: "tests/sfc-projection/STP7/products/second-framework-boundary-evidence.json",
      schema: "SecondFrameworkBoundaryEvidence",
      caseField: "cases",
    }),
    Object.freeze({
      file: "tests/sfc-projection/STP7/products/execution-context-boundary-contract.json",
      schema: "ExecutionContextBoundaryContract",
    }),
  ]),
});

const PREDECESSOR_NODES = Object.freeze(Object.keys(PREDECESSOR_PRODUCTS));

function ledgerEntryFor(node) {
  return (PREDECESSOR_PRODUCTS[node] || []).find((product) => product.caseField);
}

/**
 * Required feasibility rows derived from the canonical mandatory-case ledgers
 * (verify-node NODE_MANDATORY_CASES), never a list authored in this node: a
 * predecessor gaining a mandatory case fails STP8 without any edit here.
 */
export function requiredPredecessorRows(mandatoryCases = NODE_MANDATORY_CASES) {
  const rows = [];
  for (const node of PREDECESSOR_NODES) {
    for (const caseId of mandatoryCases[node] || []) {
      rows.push({ node, case: caseId });
    }
  }
  return rows;
}

export function predecessorCaseNames(mandatoryCases = NODE_MANDATORY_CASES) {
  return new Set(requiredPredecessorRows(mandatoryCases).map((row) => row.case));
}

function caseIdsRecordedIn(ledger, caseField) {
  if (!ledger || typeof ledger !== "object") return new Set();
  const entries = ledger[caseField];
  if (!Array.isArray(entries)) return new Set();
  if (caseField === "mandatoryCases") {
    return new Set(entries.filter((entry) => typeof entry === "string"));
  }
  return new Set(entries.map((entry) => entry?.id).filter(Boolean));
}

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

function defaultReadJson(repoRoot) {
  return (rel) => {
    try {
      return JSON.parse(fs.readFileSync(path.resolve(repoRoot, rel), "utf8"));
    } catch {
      return null;
    }
  };
}

export function loadPredecessorProduct(
  node,
  schema,
  { repoRoot = REPO_ROOT, readJson = defaultReadJson(repoRoot) } = {},
) {
  const entry = (PREDECESSOR_PRODUCTS[node] || []).find((product) => product.schema === schema);
  if (!entry) return null;
  return readJson(entry.file);
}

/**
 * STP8-complete-evidence: every predecessor mandatory case (derived from the
 * canonical ledgers) is matched to a feasibility row that cites that node's
 * case-bearing ledger, the ledger structurally records the case, and every
 * charter §4 named predecessor product exists with its schema. Uncited,
 * fabricated, unknown, or unbacked rows are rejected.
 */
export function assertCompleteEvidence(
  abi,
  {
    repoRoot = REPO_ROOT,
    mandatoryCases = NODE_MANDATORY_CASES,
    readJson = defaultReadJson(repoRoot),
  } = {},
) {
  const errors = [];
  const rows = Array.isArray(abi?.feasibilityRows) ? abi.feasibilityRows : [];
  const required = requiredPredecessorRows(mandatoryCases);
  const known = new Set(required.map((row) => `${row.node}:${row.case}`));
  const matched = new Set(rows.map((row) => `${row.node}:${row.case}`));
  for (const row of required) {
    if (!matched.has(`${row.node}:${row.case}`)) {
      errors.push(
        err(
          "STP8-complete-evidence",
          "unmatched-row",
          `feasibility row ${row.node}:${row.case} has no selected candidate evidence`,
        ),
      );
    }
  }
  for (const row of rows) {
    const key = `${row.node}:${row.case}`;
    if (!known.has(key)) {
      errors.push(
        err(
          "STP8-complete-evidence",
          "unknown-row",
          `feasibility row ${key} is not a predecessor mandatory case`,
        ),
      );
      continue;
    }
    const ledgerEntry = ledgerEntryFor(row.node);
    if (!ledgerEntry || row.source !== ledgerEntry.file) {
      errors.push(
        err(
          "STP8-complete-evidence",
          "uncited-row",
          `feasibility row ${key} must cite its node's mandatory-case ledger ${ledgerEntry?.file}, got ${row.source}`,
        ),
      );
      continue;
    }
    const ledger = readJson(row.source);
    if (!ledger || ledger.schema !== ledgerEntry.schema) {
      errors.push(
        err(
          "STP8-complete-evidence",
          "uncited-row",
          `evidence ledger missing or lost its schema at ${row.source}`,
        ),
      );
      continue;
    }
    if (!caseIdsRecordedIn(ledger, ledgerEntry.caseField).has(row.case)) {
      errors.push(
        err(
          "STP8-complete-evidence",
          "uncited-row",
          `${row.source} does not record case ${row.case}`,
        ),
      );
    }
  }
  for (const node of PREDECESSOR_NODES) {
    for (const product of PREDECESSOR_PRODUCTS[node]) {
      const loaded = readJson(product.file);
      if (!loaded || loaded.schema !== product.schema) {
        errors.push(
          err(
            "STP8-complete-evidence",
            "missing-predecessor-product",
            `${node} product ${product.schema} is missing or lost its schema at ${product.file}`,
          ),
        );
      }
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

function readUtilityProbe(repoRoot = REPO_ROOT) {
  const read = (rel) => {
    try {
      return fs.readFileSync(path.resolve(repoRoot, rel), "utf8");
    } catch {
      return "";
    }
  };
  const clean = read("tests/sfc-projection/STP8/probes/utilities.tsx");
  const dirty = read("tests/sfc-projection/STP8/probes/utilities-error.tsx");
  const againstRatified = 'from "./components/Ratified.vue"';
  return {
    clean,
    dirty,
    exercisesUtilities:
      clean.includes("createApp(") &&
      clean.includes("h(") &&
      clean.includes("<Comp") &&
      clean.includes(againstRatified),
    carriesWrongPropTwin:
      dirty.includes("h(") && dirty.includes("<Comp") && dirty.includes(againstRatified),
  };
}

/**
 * STP8-abi-contamination: a checker-only public shape must not change
 * InstanceType or Vue utility behavior. Every consumer surface observes the
 * identical public spelling, and `utilitiesUntouched` is only a measurable
 * claim while the inherited STP2 vue-utilities recipe (createApp/h/TSX plus a
 * wrong-prop dirty twin against the ratified fixture) is present to observe it.
 */
export function assertAbiContamination(surface, { utilityProbe = readUtilityProbe() } = {}) {
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
  const claimsUntouchedUtilities = (surface.surfaces || []).some(
    (row) => row.utilitiesUntouched === true,
  );
  if (claimsUntouchedUtilities && !utilityProbe.exercisesUtilities) {
    errors.push(
      err(
        "STP8-abi-contamination",
        "utility-probe-missing",
        "utilitiesUntouched is claimed without the createApp/h/TSX recipe against the ratified fixture",
      ),
    );
  }
  if (claimsUntouchedUtilities && !utilityProbe.carriesWrongPropTwin) {
    errors.push(
      err(
        "STP8-abi-contamination",
        "utility-probe-missing",
        "the Vue utility recipe has no wrong-prop dirty twin, so a checker-only respell could stay green unnoticed",
      ),
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
 * be postponed until after specialization, and the inference transaction must
 * BE the STP3 InferenceWitnessSelection channel set — joined, not a parallel
 * catalog restated by this node.
 */
export function assertInferenceContract(
  abi,
  { witnessSelection = loadInferenceWitnessSelection() } = {},
) {
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
  if (!witnessSelection || witnessSelection.schema !== "InferenceWitnessSelection") {
    errors.push(
      err(
        "STP8-inference-contract",
        "witness-selection-missing",
        "the STP3 InferenceWitnessSelection product is missing or lost its schema",
      ),
    );
    return errors;
  }
  const selected = new Set(witnessSelection.inferenceTransaction || []);
  for (const channel of selected) {
    if (!transaction.has(channel)) {
      errors.push(
        err(
          "STP8-inference-contract",
          "postponed-contributor",
          `InferenceWitnessSelection channel ${channel} was postponed out of the inference transaction`,
        ),
      );
    }
  }
  for (const channel of transaction) {
    if (!selected.has(channel)) {
      errors.push(
        err(
          "STP8-inference-contract",
          "parallel-catalog",
          `inference transaction channel ${channel} is not an InferenceWitnessSelection contributor; join the predecessor selection instead of restating a parallel catalog`,
        ),
      );
    }
  }
  if (inference.predecessorWitness !== witnessSelection.selectedWitness) {
    errors.push(
      err(
        "STP8-inference-contract",
        "witness-drift",
        `inference.predecessorWitness must record the selected STP3 witness ${witnessSelection.selectedWitness}`,
      ),
    );
  }
  for (const deferred of witnessSelection.postSpecializationOnly || []) {
    if (transaction.has(deferred)) {
      errors.push(
        err(
          "STP8-inference-contract",
          "postponed-contributor",
          `InferenceWitnessSelection defers ${deferred} until after specialization but the ABI transaction includes it`,
        ),
      );
    }
  }
  return errors;
}

/** The ratified fixture keeps the accepted encoding: one two-binder public constructor. */
export function assertRatifiedFixture(source) {
  const errors = [];
  if (!source || typeof source !== "string") {
    return [err("STP8-complete-evidence", "missing-fixture", "ratified fixture is missing")];
  }
  if (!/export declare class Comp<T = unknown, U = unknown>/.test(source)) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "binder-family",
        "fixture must spell the two-binder Comp<T = unknown, U = unknown> family the STP3 coupled evidence selected",
      ),
    );
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

function stableStringify(value) {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${stableStringify(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
}

function deepEqual(left, right) {
  return stableStringify(left) === stableStringify(right);
}

/**
 * STP8-complete-evidence topology join: the frozen AcceptedTopologyMatrix
 * restates predecessor policy values, so every restated value must still equal
 * the named predecessor product it freezes. A predecessor policy change that
 * is not re-ratified fails here instead of silently diverging.
 */
export function assertPredecessorJoins(
  topology,
  { repoRoot = REPO_ROOT, readJson = defaultReadJson(repoRoot) } = {},
) {
  const errors = [];
  const at = (node, schema) => loadPredecessorProduct(node, schema, { repoRoot, readJson });
  const drift = (message) =>
    err(
      "STP8-complete-evidence",
      "predecessor-drift",
      `frozen topology diverged from its predecessor product: ${message}`,
    );

  const stp4 = at("STP4", "ProjectionTopologyDecisionInputs");
  if (stp4) {
    if (
      topology?.supplementalFiles?.namedImportTarget !== stp4.supplementalFiles?.namedImportTarget
    ) {
      errors.push(drift("supplementalFiles.namedImportTarget vs ProjectionTopologyDecisionInputs"));
    }
    if (
      topology?.supplementalFiles?.sharedLexicalScope !== stp4.supplementalFiles?.sharedLexicalScope
    ) {
      errors.push(
        drift("supplementalFiles.sharedLexicalScope vs ProjectionTopologyDecisionInputs"),
      );
    }
    for (const field of [
      "canonicalImportIdentity",
      "hostAdmittedVirtualUnit",
      "compilerNamedSpecifier",
    ]) {
      if (topology?.supplementalFiles?.[field] !== stp4.supplementalFiles?.[field]) {
        errors.push(drift(`supplementalFiles.${field} vs ProjectionTopologyDecisionInputs`));
      }
    }
    for (const field of ["ownership", "checkedOncePerIdentity", "importersDoNotDuplicateBody"]) {
      if (topology?.externalScripts?.[field] !== stp4.externalScripts?.[field]) {
        errors.push(drift(`externalScripts.${field} vs ProjectionTopologyDecisionInputs`));
      }
    }
    if (topology?.vueLegality?.scriptSetupSrc !== stp4.vueLegality?.scriptSetupSrc) {
      errors.push(drift("vueLegality.scriptSetupSrc vs ProjectionTopologyDecisionInputs"));
    }
    if (!deepEqual(topology?.checkJs, stp4.checkJs)) {
      errors.push(drift("checkJs vs ProjectionTopologyDecisionInputs"));
    }
    if (topology?.normalScriptPlusSetup !== stp4.normalScriptPlusSetup) {
      errors.push(drift("normalScriptPlusSetup vs ProjectionTopologyDecisionInputs"));
    }
    const tsx = (topology?.dialects || []).find((row) => row.id === "tsx");
    if (tsx?.moduleShape !== stp4.jsxRewrite?.tsx) {
      errors.push(
        drift("dialects[tsx].moduleShape vs ProjectionTopologyDecisionInputs.jsxRewrite.tsx"),
      );
    }
    if (tsx?.angleAssertions !== stp4.jsxRewrite?.tsAngleAssertion) {
      errors.push(
        drift(
          "dialects[tsx].angleAssertions vs ProjectionTopologyDecisionInputs.jsxRewrite.tsAngleAssertion",
        ),
      );
    }
    const js = (topology?.dialects || []).find((row) => row.id === "js");
    if (js?.policy !== stp4.checkJs?.perFilePolicy) {
      errors.push(
        drift("dialects[js].policy vs ProjectionTopologyDecisionInputs.checkJs.perFilePolicy"),
      );
    }
  }

  const stp5d = at("STP5", "DiagnosticOriginPolicy");
  if (stp5d) {
    if (topology?.diagnosticOrigins?.stockCliSource !== stp5d.stockCliSource) {
      errors.push(drift("diagnosticOrigins.stockCliSource vs DiagnosticOriginPolicy"));
    }
    if (
      topology?.diagnosticOrigins?.featureMaskDoesNotSuppress !==
      stp5d.featureMaskDoesNotSuppressDiagnostics
    ) {
      errors.push(drift("diagnosticOrigins.featureMaskDoesNotSuppress vs DiagnosticOriginPolicy"));
    }
    if (topology?.diagnosticOrigins?.synthesized !== stp5d.synthesizedDiagnostics) {
      errors.push(
        drift("diagnosticOrigins.synthesized vs DiagnosticOriginPolicy.synthesizedDiagnostics"),
      );
    }
  }

  const stp5o = at("STP5", "ObservationRolePolicy");
  if (stp5o) {
    if (!deepEqual(topology?.editProvenance?.spanKinds, stp5o.spanKinds)) {
      errors.push(drift("editProvenance.spanKinds vs ObservationRolePolicy.spanKinds"));
    }
    if (topology?.editProvenance?.synthesized !== stp5o.synthesized) {
      errors.push(drift("editProvenance.synthesized vs ObservationRolePolicy.synthesized"));
    }
  }

  const stp6 = at("STP6", "PublicDependencyClosurePolicy");
  const stp6Packed = at("STP6", "PackedConsumerFeasibility");
  if (stp6) {
    if (
      !deepEqual(
        topology?.publicDependencyClosure?.resolutionModes,
        (stp6Packed?.resolutionModes || []).map((row) => row.id),
      )
    ) {
      errors.push(
        drift(
          "publicDependencyClosure.resolutionModes vs PackedConsumerFeasibility.resolutionModes",
        ),
      );
    }
    for (const field of [
      "liftIntoPublishedDeclarations",
      "retainWithoutSecondBodyCheck",
      "forbidInPublishedDeclarations",
      "typeOnlyCheckingContract",
    ]) {
      if (!deepEqual(topology?.publicDependencyClosure?.[field], stp6[field])) {
        errors.push(drift(`publicDependencyClosure.${field} vs PublicDependencyClosurePolicy`));
      }
    }
    if (
      topology?.publicDependencyClosure?.hiddenMetadataInExports !== stp6.hiddenMetadata?.inExports
    ) {
      errors.push(
        drift("publicDependencyClosure.hiddenMetadataInExports vs PublicDependencyClosurePolicy"),
      );
    }
    if (topology?.publicDependencyClosure?.hiddenMetadataInFiles !== stp6.hiddenMetadata?.inFiles) {
      errors.push(
        drift("publicDependencyClosure.hiddenMetadataInFiles vs PublicDependencyClosurePolicy"),
      );
    }
    const mapsToShipped =
      stp6.declarationMaps?.requiredWhenShipped === true &&
      stp6.declarationMaps?.sourcesMustBePackRelativeAuthoredFiles === true;
    if (
      topology?.publicDependencyClosure?.declarationMapsToShippedAuthoredSource !== mapsToShipped
    ) {
      errors.push(
        drift(
          "publicDependencyClosure.declarationMapsToShippedAuthoredSource vs PublicDependencyClosurePolicy.declarationMaps",
        ),
      );
    }
  }

  const stp7 = at("STP7", "ExecutionContextBoundaryContract");
  if (stp7) {
    if (
      topology?.executionContexts?.separateSupplementalFilesIsolateAmbient !==
      stp7.sameProgram?.separateSupplementalFilesIsolateAmbient
    ) {
      errors.push(
        drift(
          "executionContexts.separateSupplementalFilesIsolateAmbient vs ExecutionContextBoundaryContract",
        ),
      );
    }
  }
  return errors;
}

export function loadInferenceWitnessSelection(options = {}) {
  return loadPredecessorProduct("STP3", "InferenceWitnessSelection", options);
}

// The harness prints InstanceType<typeof Comp> for the default specialization
// as the merged interface reference Comp<unknown, unknown>; evaluateStp8 pins the
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
  if (
    !/declare class Comp<T = unknown, U = unknown>/.test(
      String(abi?.selectedCandidate?.spelling || ""),
    )
  ) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "removed-fixture",
        "missing two-binder selected constructor spelling",
      ),
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
  errors.push(...assertPredecessorJoins(topology));
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
  const caseNames = predecessorCaseNames();
  for (const row of helperAbi?.helpers || []) {
    if (!caseNames.has(row.case)) {
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
  // Completion evidence must label the baseline input snapshot and the
  // ratification candidate separately; the baseline SHA is never the candidate.
  const baselineMatch = /Input snapshot \(job baseline[^`\n]*`([0-9a-f]{40})`/.exec(evidenceText);
  const candidateMatch = /Ratification candidate: `([0-9a-f]{40})`/.exec(evidenceText);
  if (!baselineMatch || baselineMatch[1] !== BASELINE_INPUT_SNAPSHOT) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "baseline-mislabel",
        `evidence index must record the job baseline input snapshot ${BASELINE_INPUT_SNAPSHOT} under its own label`,
      ),
    );
  }
  if (
    !candidateMatch ||
    candidateMatch[1] === BASELINE_INPUT_SNAPSHOT ||
    !/^[0-9a-f]{40}$/.test(candidateMatch[1])
  ) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "candidate-mislabel",
        `evidence index must record a ratification candidate revision distinct from the baseline (found ${candidateMatch?.[1] ?? "none"})`,
      ),
    );
  }
  if (/Input snapshot[^)\n]*\(captured STP8 candidate\)/.test(evidenceText)) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "candidate-mislabel",
        "evidence index still labels the baseline input snapshot as the STP8 candidate",
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

export const DIRTY_ADDED_MANDATORY_CASE = "STP3-freshly-mandatory";

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
  predecessorWitness: "whole-signature-contextual-construction",
  inferenceTransaction: ["rows", "project", "modelValue", "expose-value"],
  contributingChannels: [
    "rows",
    "project",
    "modelValue",
    "onChange",
    "onUpdate:modelValue",
    "slot-default",
    "expose-value",
  ],
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

function withMandatoryCaseAdded(caseId) {
  return {
    ...NODE_MANDATORY_CASES,
    STP3: Object.freeze([...NODE_MANDATORY_CASES.STP3, caseId]),
  };
}

function withPredecessorProductDropped(schema) {
  const read = defaultReadJson(REPO_ROOT);
  return (rel) => {
    const entry = Object.values(PREDECESSOR_PRODUCTS)
      .flat()
      .find((product) => product.schema === schema && product.file === rel);
    if (entry) return null;
    return read(rel);
  };
}

function withLedgerEmptied(file) {
  const read = defaultReadJson(REPO_ROOT);
  return (rel) => {
    const loaded = read(rel);
    if (rel !== file || !loaded) return loaded;
    return { ...loaded, cases: [] };
  };
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
  // A predecessor gaining a mandatory case must fail without any edit here.
  const addedCase = assertCompleteEvidence(abi, {
    mandatoryCases: withMandatoryCaseAdded(DIRTY_ADDED_MANDATORY_CASE),
  });
  if (!addedCase.some((error) => error.code === "unmatched-row")) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "missed-row",
        "a newly mandatory predecessor case was not demanded",
      ),
    );
  }
  // A deleted or schema-drifted STP5 policy product must fail even though the
  // STP5 case ids live on in the manifest.
  const missingMapper = assertCompleteEvidence(abi, {
    readJson: withPredecessorProductDropped("MapperCapabilityEvidence"),
  });
  if (!missingMapper.some((error) => error.code === "missing-predecessor-product")) {
    errors.push(
      err("STP8-complete-evidence", "missed-row", "a dropped STP5 policy product was not rejected"),
    );
  }
  // A predecessor ledger emptied of its cases must fail every row citing it.
  const emptiedLedger = assertCompleteEvidence(abi, {
    readJson: withLedgerEmptied(
      "tests/sfc-projection/STP3/products/coupled-inference-evidence.json",
    ),
  });
  if (!emptiedLedger.some((error) => error.code === "uncited-row")) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "missed-row",
        "a predecessor ledger emptied of its cases was not rejected",
      ),
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
  // utilitiesUntouched without the observing recipe must not pass.
  const noUtilityProbe = assertAbiContamination(abi.publicSurface, {
    utilityProbe: { clean: "", dirty: "", exercisesUtilities: false, carriesWrongPropTwin: false },
  });
  if (!noUtilityProbe.some((error) => error.code === "utility-probe-missing")) {
    errors.push(
      err(
        "STP8-abi-contamination",
        "missed-contamination",
        "an unmeasured utilitiesUntouched claim was not rejected",
      ),
    );
  }

  if (assertInferenceContract(abi).length) {
    errors.push(...assertInferenceContract(abi));
  }
  const postponed = cloneJson(abi);
  postponed.inference.postSpecializationOnly.push("onChange");
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
  // The transaction must track the joined STP3 selection, not a parallel catalog.
  const witnessMinus = cloneJson(loadInferenceWitnessSelection());
  witnessMinus.inferenceTransaction = witnessMinus.inferenceTransaction.filter(
    (channel) => channel !== "rows",
  );
  const minusErrors = assertInferenceContract(abi, { witnessSelection: witnessMinus });
  if (!minusErrors.some((error) => error.code === "parallel-catalog")) {
    errors.push(
      err(
        "STP8-inference-contract",
        "missed-postponed",
        "a parallel inference catalog was not rejected",
      ),
    );
  }
  const witnessPlus = cloneJson(loadInferenceWitnessSelection());
  witnessPlus.inferenceTransaction = [
    ...witnessPlus.inferenceTransaction,
    "post-specialization-extra",
  ];
  const plusErrors = assertInferenceContract(abi, { witnessSelection: witnessPlus });
  if (!plusErrors.some((error) => error.code === "postponed-contributor")) {
    errors.push(
      err(
        "STP8-inference-contract",
        "missed-postponed",
        "a witness-selection channel drop was not rejected",
      ),
    );
  }

  const topology = loadStp8Product("accepted-topology-matrix.json");
  const driftedTopology = cloneJson(topology);
  driftedTopology.supplementalFiles.namedImportTarget = true;
  if (assertPredecessorJoins(driftedTopology).length === 0) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "missed-row",
        "a topology field diverging from its predecessor product was not rejected",
      ),
    );
  }
  const driftedCheckJs = cloneJson(topology);
  driftedCheckJs.checkJs.off = "rewritten-off-semantics";
  if (assertPredecessorJoins(driftedCheckJs).length === 0) {
    errors.push(
      err(
        "STP8-complete-evidence",
        "missed-row",
        "a checkJs policy body diverging from ProjectionTopologyDecisionInputs was not rejected",
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
