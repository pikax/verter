/**
 * STS0 Svelte projection profile and current-feature contract helpers.
 *
 * Ratifies the framework-specific Svelte profile — function-shaped Component
 * (no Vue constructor or Vue event/model/ref convention), runes versus
 * supported legacy semantics, legal .svelte/.svelte.ts/.svelte.js surfaces
 * with explicit checking and publishing behavior per profile — plus the
 * pinned engine/framework matrix, against the STP7 boundary, the STP8
 * architecture products, the CCA1I Svelte ProjectionBackend and the selected
 * official fixture populations. Required feature rows are derived from the
 * canonical feature list this lock establishes (the inventory this node owes);
 * pins are joined to the STP1 EngineMatrix and the live root package.json.
 * No production emit.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const STS0_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(STS0_DIR, "../../..");

export const PROTOCOL_VERSION = 1;

/** The job baseline this node's branch started from. */
export const BASELINE_INPUT_SNAPSHOT = "b9ef7e31023c6a3c06fcb1963630b8c54a17f752";
/** The commit that introduced the ratified STS0 products (recorded by the follow-up evidence commit). */
export const RATIFICATION_CANDIDATE = "063560adbe3c3cdd191d41e544e1ee29702b47b0";

export const STS0_MANDATORY_CASES = Object.freeze([
  "STS0-svelte-inventory",
  "STS0-svelte-abi",
  "STS0-svelte-pin",
  "STS0-policy-lock",
]);

/**
 * The canonical current-supported Svelte feature list this lock establishes.
 * Every id must carry a mandatory owning inventory row; a feature gaining or
 * losing canon fails STS0-svelte-inventory without any edit to the validator.
 */
export const REQUIRED_SVELTE_FEATURES = Object.freeze([
  "rune-state",
  "rune-derived",
  "rune-effect",
  "rune-props",
  "rune-bindable",
  "rune-inspect",
  "rune-host",
  "svelte-ts-modules",
  "template-each",
  "template-await",
  "template-snippet",
  "template-if-else",
  "template-interpolation",
  "template-svelte-specials",
  "events-attribute-handlers",
  "events-legacy-directive",
  "legacy-slots",
  "legacy-export-let",
  "legacy-store-auto-subscription",
  "legacy-transitions-actions-animate",
  "bind-directive",
  "script-module-context",
  "component-function-shape",
  "css-scoping-attr-selectors",
]);

export const REQUIRED_SVELTE_FAMILIES = Object.freeze([
  "runes",
  "template",
  "events",
  "legacy",
  "bindings",
  "scripts",
  "component",
  "styles",
]);

export const SVELTE_FILE_KINDS = Object.freeze([".svelte", ".svelte.ts", ".svelte.js"]);
export const SEMANTICS_MODES = Object.freeze(["runes", "legacy", "both"]);
export const CHECKING_BEHAVIORS = Object.freeze(["engine-checked", "js-unchecked"]);
export const PUBLISHING_BEHAVIORS = Object.freeze([
  "declarations-published",
  "module-exports-published",
]);

export const SVELTE_OPTIONS_TSV =
  "packages/framework-conformance-harness/evidence/svelte-options.tsv";
export const STP1_ENGINE_MATRIX = "tests/sfc-projection/STP1/products/engine-matrix.json";
export const STP8_ARCHITECTURE =
  "tests/sfc-projection/STP8/products/accepted-projection-architecture.json";
export const STP7_BOUNDARY_EVIDENCE =
  "tests/sfc-projection/STP7/products/second-framework-boundary-evidence.json";
export const STP7_BOUNDARY_CONTRACT =
  "tests/sfc-projection/STP7/products/execution-context-boundary-contract.json";
export const CCA1I_BACKEND = "crates/verter_compiler/src/svelte/svelte_projection_backend.rs";

export const SVELTE_OWNED_SOURCE_FILES = Object.freeze([
  "crates/verter_compiler/src/svelte/svelte_projection_backend.rs",
  "crates/verter_compiler/src/svelte/ide/prelude.rs",
  "crates/verter_compiler/src/svelte/semantic_authority.rs",
]);

export const VUE_CONSTRUCTOR_SOURCE_TOKENS = Object.freeze([
  "defineComponent",
  "defineProps",
  "defineEmits",
  "v-model",
  "$emit",
  "$refs",
  "vue.ref",
]);

export function err(caseId, code, message) {
  return { caseId, code, message };
}

export function repoPath(rel) {
  return path.resolve(REPO_ROOT, rel);
}

export function loadSts0Product(name) {
  return JSON.parse(fs.readFileSync(path.join(STS0_DIR, "products", name), "utf8"));
}

function loadSts0Manifest() {
  try {
    return JSON.parse(fs.readFileSync(path.join(STS0_DIR, "manifest.json"), "utf8"));
  } catch {
    return null;
  }
}

export function loadEngineMatrix() {
  return JSON.parse(fs.readFileSync(repoPath(STP1_ENGINE_MATRIX), "utf8"));
}

export function loadRootPackageJson(repoRoot = REPO_ROOT) {
  return JSON.parse(fs.readFileSync(path.join(repoRoot, "package.json"), "utf8"));
}

export function cloneJson(value) {
  return structuredClone(value);
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

function parseOptionsTsv(text) {
  const lines = String(text || "")
    .split(/\r?\n/)
    .filter((line) => line.trim());
  if (lines.length < 2) return [];
  return lines.slice(1).map((line) => {
    const [surface, option, classification] = line.split("\t");
    return { surface, option, classification };
  });
}

function loadOptionsTsv(repoRoot = REPO_ROOT) {
  return parseOptionsTsv(fs.readFileSync(path.resolve(repoRoot, SVELTE_OPTIONS_TSV), "utf8"));
}

function isExactPin(version) {
  const value = String(version || "").trim();
  if (!value) return false;
  return /^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$/.test(value);
}

/**
 * STS0-svelte-abi: the modern Svelte Component stays framework-specific and
 * function-shaped. A Vue constructor, or a Vue event/model/ref convention,
 * required for a Svelte Component is rejected, as is any
 * InstanceType-of-a-class requirement on the Svelte public shape.
 */
export function assertSvelteAbi(shape) {
  const errors = [];
  if (!shape || typeof shape !== "object") {
    return [err("STS0-svelte-abi", "missing-surface", "component shape record is missing")];
  }
  if (!String(shape.publicShape || "").includes("function-shaped")) {
    errors.push(
      err(
        "STS0-svelte-abi",
        "vue-constructor-required",
        "Svelte public shape must stay the function-shaped Component, not a Vue constructor",
      ),
    );
  }
  if (!String(shape.publicShape || "").includes('import("svelte")')) {
    errors.push(
      err(
        "STS0-svelte-abi",
        "vue-constructor-required",
        'Svelte public shape must name the import("svelte").Component family recorded by STP7',
      ),
    );
  }
  for (const field of [
    "vueConstructorRequired",
    "vueEventConvention",
    "vueModelConvention",
    "vueRefConvention",
  ]) {
    if (shape[field] !== false) {
      errors.push(
        err("STS0-svelte-abi", "vue-convention-required", `componentShape.${field} must be false`),
      );
    }
  }
  if (!String(shape.legacyClassSurface || "").includes("framework-specific")) {
    errors.push(
      err(
        "STS0-svelte-abi",
        "vue-constructor-required",
        "legacy class surfaces must stay framework-specific; no Vue constructor reuse",
      ),
    );
  }
  if (shape.instanceTypeRequirement !== "none") {
    errors.push(
      err(
        "STS0-svelte-abi",
        "instance-type-of-class",
        "no InstanceType-of-a-class requirement may be imposed on the Svelte public shape",
      ),
    );
  }
  return errors;
}

/** Source-level twin of STS0-svelte-abi: the encoding itself is scanned. */
export function assertSvelteShapeSource(source) {
  const errors = [];
  if (!source || typeof source !== "string") {
    return [err("STS0-svelte-abi", "missing-fixture", "Svelte shape fixture is missing")];
  }
  if (!source.includes("SvelteComponent") && !source.includes('import("svelte")')) {
    errors.push(
      err(
        "STS0-svelte-abi",
        "vue-constructor-shim",
        "Svelte shape fixture must spell the function-shaped SvelteComponent",
      ),
    );
  }
  if (/\bdeclare\s+class\b/.test(source) || /\bInstanceType\s*</.test(source)) {
    errors.push(
      err(
        "STS0-svelte-abi",
        "vue-constructor-shim",
        "Svelte shape fixture must not encode a constructor class or InstanceType requirement",
      ),
    );
  }
  for (const token of VUE_CONSTRUCTOR_SOURCE_TOKENS) {
    if (source.includes(token)) {
      errors.push(
        err(
          "STS0-svelte-abi",
          "vue-convention-required",
          `Svelte shape fixture encodes Vue-only ${token}`,
        ),
      );
    }
  }
  for (const needle of ["Widget", "props"]) {
    if (!source.includes(needle)) {
      errors.push(
        err("STS0-svelte-abi", "missing-fixture", `Svelte shape fixture missing ${needle}`),
      );
    }
  }
  return errors;
}

// The harness Instance observation is Vue-constructor-shaped (InstanceType<typeof Comp>)
// and is deliberately not asserted for STS0's function-shaped Component; this pin keeps
// the manifest's schema-required probes.expectedInstanceType from drifting silently
// instead; it must name the component interface the positive probe actually declares.
export function assertInstanceShapePin(source, expectedInstanceType) {
  const match = typeof source === "string" ? source.match(/^export interface (\w+)/m) : null;
  const declared = match ? match[1] : null;
  if (!declared || declared !== expectedInstanceType) {
    return [
      err(
        "STS0-svelte-abi",
        "instance-type-drift",
        `manifest probes.expectedInstanceType ${JSON.stringify(expectedInstanceType)} must name ` +
          `the function-shaped component interface declared in probes/positive.ts (found ` +
          `${JSON.stringify(declared)}); the Vue-constructor-shaped harness Instance check does ` +
          "not apply to STS0",
      ),
    ];
  }
  return [];
}

/**
 * STS0-svelte-inventory: every current supported Svelte feature carries a
 * mandatory owning row, runes versus supported legacy semantics is carried
 * per row, and every row and population cites an existing fixture.
 */
export function assertSvelteInventory(
  inventory,
  { repoRoot = REPO_ROOT, required = REQUIRED_SVELTE_FEATURES } = {},
) {
  const errors = [];
  const rows = Array.isArray(inventory?.rows) ? inventory.rows : [];
  if (rows.length === 0) {
    return [
      err("STS0-svelte-inventory", "removed-fixture", "inventory selected coverage is empty"),
    ];
  }
  const canonical = new Set(required);
  const ids = new Set();
  for (const row of rows) {
    if (!row?.id) {
      errors.push(err("STS0-svelte-inventory", "removed-fixture", "inventory row missing id"));
      continue;
    }
    if (ids.has(row.id)) {
      errors.push(
        err("STS0-svelte-inventory", "removed-fixture", `duplicate inventory row ${row.id}`),
      );
    }
    ids.add(row.id);
    if (!canonical.has(row.id)) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "unknown-row",
          `inventory row ${row.id} is not a canonical current-supported Svelte feature`,
        ),
      );
    }
    if (!row.family || !SEMANTICS_MODES.includes(row.semantics)) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "unclassified-semantics",
          `inventory row ${row.id} must distinguish runes versus legacy semantics`,
        ),
      );
    }
    if (row.family === "runes" && row.semantics !== "runes") {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "runes-mislabeled",
          `runes feature ${row.id} is mislabeled ${row.semantics}; runes semantics cannot be conflated with legacy`,
        ),
      );
    }
    if (row.family === "legacy" && row.semantics !== "legacy") {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "runes-mislabeled",
          `legacy feature ${row.id} is mislabeled ${row.semantics}`,
        ),
      );
    }
    if (row.requiredCurrent !== true) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "unowned-feature",
          `shipped valid feature ${row.id} must stay RequiredCurrent with a mandatory owning row`,
        ),
      );
    }
    if (!row.path || !fs.existsSync(path.resolve(repoRoot, row.path))) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "removed-fixture",
          `feature ${row.id} cites missing fixture ${row.path}`,
        ),
      );
    }
  }
  for (const id of required) {
    if (!ids.has(id)) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "unowned-feature",
          `current supported Svelte feature ${id} has no mandatory owning row`,
        ),
      );
    }
  }
  const families = new Set(rows.map((row) => row.family));
  for (const family of REQUIRED_SVELTE_FAMILIES) {
    if (!families.has(family)) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "unowned-feature",
          `inventory missing feature family ${family}`,
        ),
      );
    }
  }
  for (const population of inventory?.populations || []) {
    if (
      !population?.id ||
      !population?.path ||
      !fs.existsSync(path.resolve(repoRoot, population.path))
    ) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "removed-fixture",
          `selected population ${population?.id || "?"} missing at ${population?.path}`,
        ),
      );
    }
  }
  return errors;
}

/**
 * STS0-policy-lock: every supported profile/dialect has explicit checking and
 * publishing behavior, runes selection joins the official options
 * classification, and unsupported options fail closed instead of being
 * silently ignored.
 */
export function assertPolicyLock(policy, { repoRoot = REPO_ROOT, optionsTsv = null } = {}) {
  const errors = [];
  if (!policy || typeof policy !== "object") {
    return [err("STS0-policy-lock", "missing-policy", "policy record is missing")];
  }
  errors.push(...assertSvelteAbi(policy.componentShape));
  const rows = Array.isArray(policy.profiles) ? policy.profiles : [];
  if (rows.length === 0) {
    errors.push(err("STS0-policy-lock", "missing-policy", "policy profiles are empty"));
  }
  const ids = new Set();
  for (const row of rows) {
    if (!row?.id) {
      errors.push(err("STS0-policy-lock", "missing-policy", "policy profile missing id"));
      continue;
    }
    if (ids.has(row.id)) {
      errors.push(err("STS0-policy-lock", "missing-policy", `duplicate policy profile ${row.id}`));
    }
    ids.add(row.id);
    if (!CHECKING_BEHAVIORS.includes(row.checking)) {
      errors.push(
        err(
          "STS0-policy-lock",
          "unspecified-behavior",
          `profile ${row.id} checking behavior ${JSON.stringify(row.checking)} is not explicit`,
        ),
      );
    }
    if (!PUBLISHING_BEHAVIORS.includes(row.publishing)) {
      errors.push(
        err(
          "STS0-policy-lock",
          "unspecified-behavior",
          `profile ${row.id} publishing behavior ${JSON.stringify(row.publishing)} is not explicit`,
        ),
      );
    }
    if (!SVELTE_FILE_KINDS.includes(row.fileKind)) {
      errors.push(
        err(
          "STS0-policy-lock",
          "unspecified-behavior",
          `profile ${row.id} names illegal file kind ${JSON.stringify(row.fileKind)}`,
        ),
      );
    }
    if (!SEMANTICS_MODES.includes(row.semantics)) {
      errors.push(
        err(
          "STS0-policy-lock",
          "unspecified-behavior",
          `profile ${row.id} does not distinguish runes versus legacy semantics`,
        ),
      );
    }
    if (!row.scriptContext) {
      errors.push(
        err("STS0-policy-lock", "unspecified-behavior", `profile ${row.id} missing script context`),
      );
    }
    if (!row.evidencePath || !fs.existsSync(path.resolve(repoRoot, row.evidencePath))) {
      errors.push(
        err(
          "STS0-policy-lock",
          "missing-policy",
          `profile ${row.id} cites missing evidence fixture ${row.evidencePath}`,
        ),
      );
    }
  }
  const fileKinds = new Set(rows.map((row) => row?.fileKind));
  for (const kind of SVELTE_FILE_KINDS) {
    if (!fileKinds.has(kind)) {
      errors.push(
        err("STS0-policy-lock", "missing-policy", `no supported profile row for file kind ${kind}`),
      );
    }
  }
  const jsRows = rows.filter((row) => row?.dialect === "js" || row?.fileKind === ".svelte.js");
  if (!jsRows.some((row) => row.checking === "js-unchecked")) {
    errors.push(
      err("STS0-policy-lock", "missing-policy", "no checkJs-off (js-unchecked) js profile row"),
    );
  }
  if (!jsRows.some((row) => row.checking === "engine-checked")) {
    errors.push(
      err("STS0-policy-lock", "missing-policy", "no checkJs-on (engine-checked) js profile row"),
    );
  }
  const semantics = new Set(rows.map((row) => row?.semantics));
  if (!semantics.has("runes")) {
    errors.push(err("STS0-policy-lock", "missing-policy", "no runes-semantics profile row"));
  }
  if (!semantics.has("legacy")) {
    errors.push(err("STS0-policy-lock", "missing-policy", "no legacy-semantics profile row"));
  }
  if (
    !rows.some((row) => row?.scriptContext === "module" || row?.scriptContext === "module-file")
  ) {
    errors.push(
      err("STS0-policy-lock", "missing-policy", "no module-context participation profile row"),
    );
  }

  const tsv = optionsTsv || loadOptionsTsv(repoRoot);
  const selection = policy.semanticsSelection;
  if (!selection || !Array.isArray(tsv) || tsv.length === 0) {
    errors.push(
      err(
        "STS0-policy-lock",
        "options-join-missing",
        "runes selection must join the official svelte options classification",
      ),
    );
  } else {
    const row = selection.row || {};
    const joined = tsv.find(
      (entry) => entry.surface === row.surface && entry.option === row.option,
    );
    if (!joined) {
      errors.push(
        err(
          "STS0-policy-lock",
          "options-join-missing",
          `runes selection row ${row.surface}/${row.option} is not in the official options population`,
        ),
      );
    } else if (
      joined.classification !== row.classification ||
      joined.classification !== "supported canonical"
    ) {
      errors.push(
        err(
          "STS0-policy-lock",
          "options-join-missing",
          `runes selection classification drifted from the official ${joined.classification}`,
        ),
      );
    }
  }
  for (const refused of policy.refusedOptions || []) {
    if (refused?.behavior !== "refused-fail-closed") {
      errors.push(
        err(
          "STS0-policy-lock",
          "silently-ignored-option",
          `refused option ${refused?.surface}/${refused?.option} must fail closed, not be silently ignored`,
        ),
      );
    }
    const joined = Array.isArray(tsv)
      ? tsv.find((entry) => entry.surface === refused.surface && entry.option === refused.option)
      : null;
    if (!joined || joined.classification !== "unsupported fail-closed") {
      errors.push(
        err(
          "STS0-policy-lock",
          "silently-ignored-option",
          `refused option ${refused?.surface}/${refused?.option} is not classified unsupported fail-closed by the official population`,
        ),
      );
    }
  }
  const division = policy.division || {};
  const expectedDivision = {
    moduleInstanceAndGenericBinders: "STS1",
    fullSvelteTypingAndIde: "STS15",
    runtimeCompilation: "SCP",
    styles: "SST",
  };
  for (const [field, owner] of Object.entries(expectedDivision)) {
    if (division[field] !== owner) {
      errors.push(
        err(
          "STS0-policy-lock",
          "boundary-binding-missing",
          `division.${field} must bind the obligation to ${owner}`,
        ),
      );
    }
  }
  if (division.nativeNckChecking !== "not-a-prerequisite-or-fallback") {
    errors.push(
      err(
        "STS0-policy-lock",
        "boundary-binding-missing",
        "native NCK checking must stay not-a-prerequisite-or-fallback",
      ),
    );
  }
  if (division?.legacyRetirement?.svelte !== "STS15") {
    errors.push(
      err(
        "STS0-policy-lock",
        "boundary-binding-missing",
        "Svelte legacy retirement stays with STS15",
      ),
    );
  }
  if (policy.runtimeActivationAuthorized !== false) {
    errors.push(
      err(
        "STS0-policy-lock",
        "boundary-binding-missing",
        "no runtime activation is authorized by STS0",
      ),
    );
  }
  if (policy.newPublicHelperWithoutPredecessorEvidence !== false) {
    errors.push(
      err(
        "STS0-policy-lock",
        "boundary-binding-missing",
        "no new public helper shape without predecessor evidence",
      ),
    );
  }
  for (const field of ["ac2", "ac3Rationale", "ac4Rationale"]) {
    if (!String(policy[field] || "").trim()) {
      errors.push(err("STS0-policy-lock", "missing-policy", `policy missing ${field}`));
    }
  }
  return errors;
}

/**
 * STS0-svelte-pin: latest-tool claims without pinned executable/framework
 * provenance are rejected. Engines join the STP1 EngineMatrix exactly (no toy
 * substitute, no shrunken denominator) and the framework pin equals the live
 * root package.json pin.
 */
export function assertEngineFrameworkPins(
  matrix,
  { engineMatrix = loadEngineMatrix(), packageJson = loadRootPackageJson() } = {},
) {
  const errors = [];
  const engines = Array.isArray(matrix?.engines) ? matrix.engines : [];
  const matrixEngines = engineMatrix?.engines || [];
  for (const engine of engines) {
    const pin = matrixEngines.find((row) => row.id === engine.id);
    if (!pin || pin.version !== engine.version) {
      errors.push(
        err(
          "STS0-svelte-pin",
          "unpinned-engine",
          `engine ${engine.id}@${engine.version} is not a selected STP1 engine pin`,
        ),
      );
    }
    if (!isExactPin(engine.version)) {
      errors.push(
        err(
          "STS0-svelte-pin",
          "latest-tool-claim",
          `engine ${engine.id} version ${JSON.stringify(engine.version)} is not an exact pin`,
        ),
      );
    }
    if (!engine.source) {
      errors.push(
        err(
          "STS0-svelte-pin",
          "provenance-missing",
          `engine ${engine.id} cites no provenance source`,
        ),
      );
    }
  }
  for (const pin of matrixEngines) {
    if (!engines.some((engine) => engine.id === pin.id)) {
      errors.push(
        err(
          "STS0-svelte-pin",
          "denominator-shrunk",
          `selected engine ${pin.id} is missing from the Svelte engine/framework matrix`,
        ),
      );
    }
  }
  const framework = matrix?.framework || {};
  const liveSvelte = packageJson?.devDependencies?.svelte;
  if (framework.package !== "svelte" || framework.version !== liveSvelte) {
    errors.push(
      err(
        "STS0-svelte-pin",
        "framework-pin-diverged",
        `svelte pin ${framework.version} is not the live root package.json pin ${liveSvelte}`,
      ),
    );
  }
  if (!isExactPin(framework.version)) {
    errors.push(
      err(
        "STS0-svelte-pin",
        "latest-tool-claim",
        `svelte pin ${JSON.stringify(framework.version)} is floating or a latest-tool claim`,
      ),
    );
  }
  if (!framework.source || !framework.resolvedFrom) {
    errors.push(
      err("STS0-svelte-pin", "provenance-missing", "svelte framework pin cites no named source"),
    );
  }
  if (framework.resolvedFrom && !fs.existsSync(repoPath(framework.resolvedFrom))) {
    errors.push(
      err(
        "STS0-svelte-pin",
        "provenance-missing",
        `svelte framework pin does not resolve at ${framework.resolvedFrom}`,
      ),
    );
  }
  const engineIds = new Set(engines.map((engine) => engine.id));
  for (const cell of matrix?.cells || []) {
    if (!engineIds.has(cell.engine)) {
      errors.push(
        err("STS0-svelte-pin", "unpinned-engine", `cell cites unknown engine ${cell.engine}`),
      );
    }
    if (cell.framework !== framework.id) {
      errors.push(
        err(
          "STS0-svelte-pin",
          "framework-pin-diverged",
          `cell engine ${cell.engine} cites unknown framework ${cell.framework}`,
        ),
      );
    }
    const provenance = cell.provenance || {};
    for (const field of ["package", "version", "source"]) {
      if (!String(provenance[field] || "").trim()) {
        errors.push(
          err(
            "STS0-svelte-pin",
            "provenance-missing",
            `cell ${cell.engine} provenance missing ${field}`,
          ),
        );
      }
    }
    const engine = engines.find((row) => row.id === cell.engine);
    if (engine && provenance.version && provenance.version !== engine.version) {
      errors.push(
        err(
          "STS0-svelte-pin",
          "provenance-missing",
          `cell ${cell.engine} provenance version ${provenance.version} diverges from pin ${engine.version}`,
        ),
      );
    }
  }
  for (const field of [
    "noDenominatorShrinking",
    "toyEvidenceAdmitted",
    "latestToolClaimsAdmitted",
  ]) {
    const expected = field === "noDenominatorShrinking";
    if (matrix?.[field] !== expected) {
      errors.push(
        err("STS0-svelte-pin", "latest-tool-claim", `matrix ${field} must be ${expected}`),
      );
    }
  }
  return errors;
}

/**
 * Predecessor joins: the STP8 architecture names STS0 as the Svelte profile
 * owner, the STP7 boundary products keep the recorded public shape, and the
 * CCA1I Svelte ProjectionBackend exists and stays Svelte-bound.
 */
export function assertPredecessorJoins({
  repoRoot = REPO_ROOT,
  readJson = defaultReadJson(repoRoot),
} = {}) {
  const errors = [];
  const architecture = readJson(STP8_ARCHITECTURE);
  if (!architecture || architecture.schema !== "AcceptedProjectionArchitecture") {
    errors.push(
      err(
        "STS0-svelte-abi",
        "predecessor-missing",
        "STP8 AcceptedProjectionArchitecture is missing",
      ),
    );
  } else {
    if (!String(architecture.division?.svelteProfile || "").includes("STS0")) {
      errors.push(
        err(
          "STS0-svelte-abi",
          "predecessor-drift",
          "STP8 architecture no longer names STS0 as the Svelte profile owner",
        ),
      );
    }
    if (architecture.division?.legacyRetirement?.svelte !== "STS15") {
      errors.push(
        err(
          "STS0-svelte-abi",
          "predecessor-drift",
          "STP8 Svelte legacy retirement owner drifted from STS15",
        ),
      );
    }
  }
  for (const [file, schema] of [
    [STP7_BOUNDARY_EVIDENCE, "SecondFrameworkBoundaryEvidence"],
    [STP7_BOUNDARY_CONTRACT, "ExecutionContextBoundaryContract"],
  ]) {
    const loaded = readJson(file);
    if (!loaded || loaded.schema !== schema) {
      errors.push(
        err(
          "STS0-svelte-abi",
          "predecessor-missing",
          `STP7 product ${schema} is missing or lost its schema`,
        ),
      );
    }
  }
  const boundary = readJson(STP7_BOUNDARY_EVIDENCE);
  if (boundary && boundary.adapter?.publicShape !== 'import("svelte").Component') {
    errors.push(
      err(
        "STS0-svelte-abi",
        "predecessor-drift",
        'STP7 recorded Svelte public shape drifted from import("svelte").Component',
      ),
    );
  }
  if (boundary && boundary.adapter?.vueConstructorShim !== false) {
    errors.push(
      err(
        "STS0-svelte-abi",
        "predecessor-drift",
        "STP7 vueConstructorShim drifted away from false",
      ),
    );
  }
  for (const rel of SVELTE_OWNED_SOURCE_FILES) {
    const abs = path.resolve(repoRoot, rel);
    if (!fs.existsSync(abs)) {
      errors.push(
        err("STS0-svelte-abi", "missing-origin-file", `missing Svelte-owned source file ${rel}`),
      );
    }
  }
  const backendAbs = path.resolve(repoRoot, CCA1I_BACKEND);
  if (fs.existsSync(backendAbs)) {
    const backend = fs.readFileSync(backendAbs, "utf8");
    if (!backend.includes("SvelteProjectionBackend")) {
      errors.push(
        err(
          "STS0-svelte-abi",
          "missing-adapter",
          "CCA1I Svelte ProjectionBackend adapter is missing",
        ),
      );
    }
    if (backend.includes("FrameworkAdapterId::vue()")) {
      errors.push(
        err(
          "STS0-svelte-abi",
          "vue-convention-required",
          "Svelte ProjectionBackend must not bind FrameworkAdapterId::vue()",
        ),
      );
    }
  }
  const preludeAbs = path.resolve(repoRoot, "crates/verter_compiler/src/svelte/ide/prelude.rs");
  if (fs.existsSync(preludeAbs)) {
    const prelude = fs.readFileSync(preludeAbs, "utf8");
    for (const token of VUE_CONSTRUCTOR_SOURCE_TOKENS) {
      if (prelude.includes(token)) {
        errors.push(
          err(
            "STS0-svelte-abi",
            "vue-convention-required",
            `Svelte ambient prelude encodes Vue-only ${token}`,
          ),
        );
      }
    }
  }
  return errors;
}

export function validateSts0Products({
  policy = loadSts0Product("svelte-projection-policy.json"),
  inventory = loadSts0Product("svelte-current-feature-inventory.json"),
  matrix = loadSts0Product("svelte-engine-framework-matrix.json"),
  evidenceText = fs.readFileSync(path.join(STS0_DIR, "../evidence/STS0/cases.md"), "utf8"),
} = {}) {
  const errors = [];
  if (policy?.schema !== "SvelteProjectionPolicy") {
    errors.push(err("STS0-policy-lock", "removed-fixture", "SvelteProjectionPolicy schema"));
  }
  if (inventory?.schema !== "SvelteCurrentFeatureInventory") {
    errors.push(
      err("STS0-svelte-inventory", "removed-fixture", "SvelteCurrentFeatureInventory schema"),
    );
  }
  if (matrix?.schema !== "SvelteEngineFrameworkMatrix") {
    errors.push(err("STS0-svelte-pin", "removed-fixture", "SvelteEngineFrameworkMatrix schema"));
  }
  errors.push(...assertPolicyLock(policy));
  errors.push(...assertSvelteInventory(inventory));
  errors.push(...assertEngineFrameworkPins(matrix));
  errors.push(...assertPredecessorJoins());
  const predecessors = new Set((policy?.predecessors || []).map((row) => row?.node));
  for (const node of ["STP8", "STP9", "CCA1I"]) {
    if (!predecessors.has(node)) {
      errors.push(
        err("STS0-policy-lock", "missing-policy", `policy must record predecessor ${node}`),
      );
    }
  }
  // Completion evidence must label the baseline input snapshot and the
  // ratification candidate separately; the baseline SHA is never the candidate.
  const baselineMatch = /Input snapshot \(job baseline[^`\n]*`([0-9a-f]{40})`/.exec(evidenceText);
  const candidateMatch = /Ratification candidate: `([0-9a-f]{40})`/.exec(evidenceText);
  if (!baselineMatch || baselineMatch[1] !== BASELINE_INPUT_SNAPSHOT) {
    errors.push(
      err(
        "STS0-policy-lock",
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
        "STS0-policy-lock",
        "candidate-mislabel",
        `evidence index must record a ratification candidate revision distinct from the baseline (found ${candidateMatch?.[1] ?? "none"})`,
      ),
    );
  }
  if (!/6\.0\.3/.test(evidenceText) || !/7\.0\.2/.test(evidenceText)) {
    errors.push(
      err(
        "STS0-svelte-pin",
        "missing-engine-pins",
        "STS0 evidence must record ts-js 6.0.3 and ts-native 7.0.2 engine pins",
      ),
    );
  }
  if (!/svelte@? ?5\.56\.10/.test(evidenceText)) {
    errors.push(
      err(
        "STS0-svelte-pin",
        "missing-engine-pins",
        "STS0 evidence must record the svelte 5.56.10 framework pin",
      ),
    );
  }
  return errors;
}

export const DIRTY_ABI_SHAPE = Object.freeze({
  publicShape: "vue-constructor",
  vueConstructorRequired: true,
  vueEventConvention: true,
  vueModelConvention: true,
  vueRefConvention: true,
  legacyClassSurface: "reuses the Vue constructor",
  instanceTypeRequirement: "InstanceType<typeof Comp>",
});

export const DIRTY_INVENTORY_DROP_FEATURE = "rune-effect";
export const DIRTY_INVENTORY_FABRICATED_FEATURE = "rune-teleport";
export const DIRTY_INVENTORY_MISLABELED_FEATURE = "rune-state";
export const DIRTY_INVENTORY_MISSING_FIXTURE =
  "tests/sfc-projection/STS0/probes/does-not-exist.svelte";

export const DIRTY_POLICY_UNSPECIFIED_PROFILE = Object.freeze({
  id: "svelte-ts-instance-runos",
  fileKind: ".svelte",
  scriptContext: "instance",
  dialect: "ts",
  semantics: "runes",
  checking: "unspecified",
  publishing: "",
  evidencePath: "packages/framework-conformance-harness/fixtures/svelte/basic-runes.svelte",
});

export const DIRTY_POLICY_REFUSED_OPTION = Object.freeze({
  surface: "svelte:CompileOptions",
  option: "made-up-option",
  behavior: "refused-fail-closed",
});

export const DIRTY_PIN_LATEST_VERSION = "latest";
export const DIRTY_PIN_FLOATING_VERSION = "^5.0.0";
export const DIRTY_PIN_DIVERGED_VERSION = "5.35.0";
export const DIRTY_PIN_UNKNOWN_ENGINE = Object.freeze({
  id: "ts-lite",
  package: "typescript",
  version: "5.8.3",
  source: "nowhere",
});

export function evaluateRejectTwins() {
  const errors = [];
  const policy = loadSts0Product("svelte-projection-policy.json");
  const inventory = loadSts0Product("svelte-current-feature-inventory.json");
  const matrix = loadSts0Product("svelte-engine-framework-matrix.json");

  // STS0-policy-lock clean pass, then the dirty twins.
  const cleanPolicy = assertPolicyLock(policy);
  if (cleanPolicy.length) errors.push(...cleanPolicy);
  const unspecified = cloneJson(policy);
  unspecified.profiles.push(cloneJson(DIRTY_POLICY_UNSPECIFIED_PROFILE));
  if (!assertPolicyLock(unspecified).some((error) => error.code === "unspecified-behavior")) {
    errors.push(
      err(
        "STS0-policy-lock",
        "missed-unspecified",
        "unspecified checking/publishing profile was not rejected",
      ),
    );
  }
  const noCheckJs = cloneJson(policy);
  noCheckJs.profiles = noCheckJs.profiles.filter(
    (row) => row.checking !== "engine-checked" || row.dialect === "ts",
  );
  if (!assertPolicyLock(noCheckJs).some((error) => error.code === "missing-policy")) {
    errors.push(
      err(
        "STS0-policy-lock",
        "missed-unspecified",
        "a dropped checkJs-on js profile row was not rejected",
      ),
    );
  }
  const inventedRefusal = cloneJson(policy);
  inventedRefusal.refusedOptions.push(cloneJson(DIRTY_POLICY_REFUSED_OPTION));
  if (
    !assertPolicyLock(inventedRefusal).some((error) => error.code === "silently-ignored-option")
  ) {
    errors.push(
      err(
        "STS0-policy-lock",
        "missed-unspecified",
        "a refusal not classified by the official options population was not rejected",
      ),
    );
  }

  // STS0-svelte-inventory clean pass, then the dirty twins.
  const cleanInventory = assertSvelteInventory(inventory);
  if (cleanInventory.length) errors.push(...cleanInventory);
  const dropped = cloneJson(inventory);
  dropped.rows = dropped.rows.filter((row) => row.id !== DIRTY_INVENTORY_DROP_FEATURE);
  if (!assertSvelteInventory(dropped).some((error) => error.code === "unowned-feature")) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "missed-unowned-feature",
        "a current supported feature losing its owning row was not rejected",
      ),
    );
  }
  const fabricated = cloneJson(inventory);
  fabricated.rows.push({
    id: DIRTY_INVENTORY_FABRICATED_FEATURE,
    family: "runes",
    semantics: "runes",
    requiredCurrent: true,
    path: "crates/verter_compiler/src/svelte/ide/prelude.rs",
    surface: "verter-prelude",
  });
  if (!assertSvelteInventory(fabricated).some((error) => error.code === "unknown-row")) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "missed-unowned-feature",
        "a fabricated feature row outside the canonical list was not rejected",
      ),
    );
  }
  // A feature gaining canon must demand its owning row without any edit here.
  const gainedCanon = assertSvelteInventory(inventory, {
    required: [...REQUIRED_SVELTE_FEATURES, "rune-freshly-canonical"],
  });
  if (!gainedCanon.some((error) => error.code === "unowned-feature")) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "missed-unowned-feature",
        "a freshly canonical feature was not demanded as an owning row",
      ),
    );
  }
  const mislabeled = cloneJson(inventory);
  const mislabeledRow = mislabeled.rows.find(
    (row) => row.id === DIRTY_INVENTORY_MISLABELED_FEATURE,
  );
  if (mislabeledRow) mislabeledRow.semantics = "legacy";
  if (!assertSvelteInventory(mislabeled).some((error) => error.code === "runes-mislabeled")) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "missed-unowned-feature",
        "a runes feature mislabeled as legacy was not rejected",
      ),
    );
  }
  const missingFixture = cloneJson(inventory);
  const missingFixtureRow = missingFixture.rows.find((row) => row.id === "template-if-else");
  if (missingFixtureRow) missingFixtureRow.path = DIRTY_INVENTORY_MISSING_FIXTURE;
  if (!assertSvelteInventory(missingFixture).some((error) => error.code === "removed-fixture")) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "missed-unowned-feature",
        "a feature row citing a missing fixture was not rejected",
      ),
    );
  }

  // STS0-svelte-abi clean pass, then the dirty twins.
  if (assertSvelteAbi(policy.componentShape).length) {
    errors.push(...assertSvelteAbi(policy.componentShape));
  }
  if (assertSvelteAbi(DIRTY_ABI_SHAPE).length === 0) {
    errors.push(
      err(
        "STS0-svelte-abi",
        "missed-vue-constructor",
        "a Vue-constructor Svelte shape was not rejected",
      ),
    );
  }

  // STS0-svelte-pin clean pass, then the dirty twins.
  const cleanPins = assertEngineFrameworkPins(matrix);
  if (cleanPins.length) errors.push(...cleanPins);
  const latest = cloneJson(matrix);
  latest.framework.version = DIRTY_PIN_LATEST_VERSION;
  latest.frameworkProvenance.version = DIRTY_PIN_LATEST_VERSION;
  if (!assertEngineFrameworkPins(latest).some((error) => error.code === "latest-tool-claim")) {
    errors.push(
      err(
        "STS0-svelte-pin",
        "missed-latest-claim",
        "a latest-tool framework claim was not rejected",
      ),
    );
  }
  const floating = cloneJson(matrix);
  floating.framework.version = DIRTY_PIN_FLOATING_VERSION;
  if (!assertEngineFrameworkPins(floating).some((error) => error.code === "latest-tool-claim")) {
    errors.push(
      err("STS0-svelte-pin", "missed-latest-claim", "a floating framework pin was not rejected"),
    );
  }
  const diverged = cloneJson(matrix);
  diverged.framework.version = DIRTY_PIN_DIVERGED_VERSION;
  if (
    !assertEngineFrameworkPins(diverged).some((error) => error.code === "framework-pin-diverged")
  ) {
    errors.push(
      err(
        "STS0-svelte-pin",
        "missed-latest-claim",
        "a framework pin diverging from the live package.json pin was not rejected",
      ),
    );
  }
  const shrunk = cloneJson(matrix);
  shrunk.engines = shrunk.engines.filter((engine) => engine.id !== "ts-native");
  shrunk.cells = shrunk.cells.filter((cell) => cell.engine !== "ts-native");
  if (!assertEngineFrameworkPins(shrunk).some((error) => error.code === "denominator-shrunk")) {
    errors.push(
      err(
        "STS0-svelte-pin",
        "missed-latest-claim",
        "denominator-shrunk engine selection was not rejected",
      ),
    );
  }
  const unknownEngine = cloneJson(matrix);
  unknownEngine.engines.push(cloneJson(DIRTY_PIN_UNKNOWN_ENGINE));
  if (!assertEngineFrameworkPins(unknownEngine).some((error) => error.code === "unpinned-engine")) {
    errors.push(
      err(
        "STS0-svelte-pin",
        "missed-latest-claim",
        "an engine outside the STP1 matrix was not rejected",
      ),
    );
  }
  const noProvenance = cloneJson(matrix);
  const noProvenanceCell = noProvenance.cells[0];
  if (noProvenanceCell?.provenance) noProvenanceCell.provenance.source = "";
  if (
    !assertEngineFrameworkPins(noProvenance).some((error) => error.code === "provenance-missing")
  ) {
    errors.push(
      err(
        "STS0-svelte-pin",
        "missed-latest-claim",
        "a provenance-less engine/framework cell was not rejected",
      ),
    );
  }
  return errors;
}

export function evaluateSts0() {
  const errors = [];
  errors.push(...validateSts0Products());
  const positiveAbs = path.join(STS0_DIR, "probes", "positive.ts");
  const positiveSource = fs.readFileSync(positiveAbs, "utf8");
  errors.push(...assertSvelteShapeSource(positiveSource));
  errors.push(
    ...assertInstanceShapePin(positiveSource, loadSts0Manifest()?.probes?.expectedInstanceType),
  );
  // The scanned STS0-svelte-abi dirty twin must be rejected on sight.
  const twinAbs = path.join(STS0_DIR, "probes", "vue-constructor-twin.ts");
  const twinSource = fs.readFileSync(twinAbs, "utf8");
  if (assertSvelteShapeSource(twinSource).length === 0) {
    errors.push(
      err(
        "STS0-svelte-abi",
        "missed-vue-constructor",
        "the vue-constructor dirty twin source was not rejected",
      ),
    );
  }
  errors.push(...evaluateRejectTwins());
  return { errors };
}
