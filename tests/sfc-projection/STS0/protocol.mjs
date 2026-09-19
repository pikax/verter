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

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import { createRequire } from "node:module";
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
  "template-html",
  "template-const",
  "template-attach",
  "class-directive",
  "style-directive",
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

export const REQUIRED_POPULATION_IDS = Object.freeze([
  "svelte-official-cases",
  "svelte-css-conformance",
  "framework-conformance-harness-fixtures",
  "verter-svelte-compiler",
  "pinned-official-tooling",
  "svelte-benchmarks",
]);

export const REQUIRED_POLICY_PROFILE_IDS = Object.freeze([
  "svelte-ts-instance-runes",
  "svelte-ts-instance-legacy",
  "svelte-js-instance-runes-unchecked",
  "svelte-js-instance-runes-checked",
  "svelte-js-instance-legacy-checked",
  "svelte-js-instance-legacy-unchecked",
  "svelte-ts-module-context",
  "svelte-js-module-context-unchecked",
  "svelte-js-module-context-checked",
  "svelte-ts-svelte-ts-module",
  "svelte-js-svelte-js-module-unchecked",
  "svelte-js-svelte-js-module-checked",
  "svelte-template-only",
]);

export const PINNED_DERIVED_DECLARE = "declare function $derived<T>(expression: T): T";
export const SVELTE_PRELUDE = "crates/verter_compiler/src/svelte/ide/prelude.rs";
export const PINNED_SVELTE_TYPES = "node_modules/svelte/types/index.d.ts";

export const SVELTE_OPTIONS_TSV =
  "packages/framework-conformance-harness/evidence/svelte-options.tsv";
export const SVELTE_OFFICIAL_CASES_TSV =
  "packages/framework-conformance-harness/evidence/svelte-official-cases.tsv";
export const STP1_ENGINE_MATRIX = "tests/sfc-projection/STP1/products/engine-matrix.json";
export const STP8_ARCHITECTURE =
  "tests/sfc-projection/STP8/products/accepted-projection-architecture.json";
export const STP7_BOUNDARY_EVIDENCE =
  "tests/sfc-projection/STP7/products/second-framework-boundary-evidence.json";
export const STP7_BOUNDARY_CONTRACT =
  "tests/sfc-projection/STP7/products/execution-context-boundary-contract.json";
export const CCA1I_BACKEND = "crates/verter_compiler/src/svelte/svelte_projection_backend.rs";

/**
 * Population authorities the selected membership is derived from: the
 * harness fixture directory supplies one member per .svelte file, and the
 * pinned benchmarks manifest supplies one member per deterministic smoke
 * case. Dropping any of those joins in memory must fail the inventory.
 */
export const HARNESS_SVELTE_FIXTURES_DIR = "packages/framework-conformance-harness/fixtures/svelte";
export const SVELTE_BENCHMARKS_MANIFEST = "crates/verter_validation_probe/manifest/svelte.toml";
export const SVELTE_BENCHMARKS_ADAPTER =
  "crates/verter_validation_probe/src/corpus/svelte_benchmarks.rs";

/** Shipped directive projection kinds and their mandatory owning rows. */
export const SVELTE_DIRECTIVE_PROJECTOR =
  "crates/verter_compiler/src/svelte/ide/projector/directive.rs";
export const DIRECTIVE_KIND_ROWS = Object.freeze({
  Bind: "bind-directive",
  On: "events-legacy-directive",
  Class: "class-directive",
  Style: "style-directive",
  Use: "legacy-transitions-actions-animate",
  Transition: "legacy-transitions-actions-animate",
  In: "legacy-transitions-actions-animate",
  Out: "legacy-transitions-actions-animate",
  Animate: "legacy-transitions-actions-animate",
  Let: "legacy-slots",
  // Unknown is the parse fallback, not a shipped feature.
  Unknown: null,
});

/**
 * Mode-paired compiler controls: the pinned svelte compiler decides row
 * semantics — every mode-shared feature compiles clean under BOTH runes and
 * legacy mode (attachments included), the on:click directive and <slot>
 * outlets are ACCEPTED in runes mode with deprecation warnings (legal-but-
 * deprecated, kept distinct from unsupported-mode rejection), and a
 * genuinely legacy-only construct is rejected in runes mode. These controls
 * — not the historical family prefix — decide the row semantics.
 */
export const MODE_CONTROL_RUNES_FIXTURE =
  "tests/sfc-projection/STS0/probes/modes/shared-features-runes.svelte";
export const MODE_CONTROL_LEGACY_FIXTURE =
  "tests/sfc-projection/STS0/probes/modes/shared-features-legacy.svelte";
export const MODE_CONTROL_LEGACY_ONLY_FIXTURE =
  "tests/sfc-projection/STS0/probes/modes/legacy-only-export-let.svelte";
export const MODE_CONTROL_ATTACH_RUNES_FIXTURE =
  "tests/sfc-projection/STS0/probes/modes/attach-runes.svelte";
export const MODE_CONTROL_ATTACH_LEGACY_FIXTURE =
  "tests/sfc-projection/STS0/probes/modes/attach-legacy.svelte";
export const MODE_CONTROL_EVENTS_SLOTS_LEGACY_FIXTURE =
  "tests/sfc-projection/STS0/probes/modes/events-slots-legacy-clean.svelte";
export const MODE_CONTROL_EVENTS_SLOTS_RUNES_FIXTURE =
  "tests/sfc-projection/STS0/probes/modes/events-slots-runes-deprecated.svelte";
export const MODE_CONTROL_SHARED_FEATURES = Object.freeze([
  "legacy-store-auto-subscription",
  "legacy-transitions-actions-animate",
  "template-snippet",
  "events-attribute-handlers",
  "class-directive",
  "style-directive",
  "template-attach",
]);
/** Legal in both modes, accepted in runes mode only with deprecation warnings. */
export const MODE_CONTROL_DEPRECATED_IN_RUNES = Object.freeze([
  "events-legacy-directive",
  "legacy-slots",
]);
export const MODE_DEPRECATED_WARNING_CODES = Object.freeze({
  "events-legacy-directive": "event_directive_deprecated",
  "legacy-slots": "slot_element_deprecated",
});
export const MODE_LEGACY_ONLY_FEATURE = "legacy-export-let";

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

function parseOfficialCasesTsv(text) {
  const lines = String(text || "")
    .split(/\r?\n/)
    .filter((line) => line.trim());
  if (lines.length < 2) return [];
  const header = lines[0].split("\t");
  return lines.slice(1).map((line) => {
    const cols = line.split("\t");
    const row = {};
    for (let i = 0; i < header.length; i += 1) row[header[i]] = cols[i] ?? "";
    return row;
  });
}

export function loadOfficialSvelteCases(repoRoot = REPO_ROOT) {
  return parseOfficialCasesTsv(
    fs.readFileSync(path.resolve(repoRoot, SVELTE_OFFICIAL_CASES_TSV), "utf8"),
  );
}

function parseBenchmarkSmokeCases(text) {
  const match = /smoke\s*=\s*\[([^\]]*)\]/.exec(String(text || ""));
  if (!match) return [];
  return [...match[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

/**
 * The selected population membership derived from the live authorities: one
 * member per harness fixture file and one member per pinned benchmark smoke
 * case. assertSvelteInventory requires every derived member to keep its
 * owning join, so a mapping deleted in memory cannot silently shrink the
 * selected corpus.
 */
export function deriveSelectedMembership({ repoRoot = REPO_ROOT } = {}) {
  const derived = [];
  const harnessDirAbs = path.resolve(repoRoot, HARNESS_SVELTE_FIXTURES_DIR);
  if (fs.existsSync(harnessDirAbs) && fs.statSync(harnessDirAbs).isDirectory()) {
    const files = fs
      .readdirSync(harnessDirAbs)
      .filter((name) => name.endsWith(".svelte"))
      .sort();
    for (const name of files) {
      derived.push({
        population: "framework-conformance-harness-fixtures",
        path: `${HARNESS_SVELTE_FIXTURES_DIR}/${name}`,
      });
    }
  }
  const manifestAbs = path.resolve(repoRoot, SVELTE_BENCHMARKS_MANIFEST);
  if (fs.existsSync(manifestAbs)) {
    for (const caseId of parseBenchmarkSmokeCases(fs.readFileSync(manifestAbs, "utf8"))) {
      derived.push({ population: "svelte-benchmarks", caseId });
    }
  }
  return derived;
}

/** Shipped directive projection kinds read from the directive projector source. */
export function shippedDirectiveKinds({ repoRoot = REPO_ROOT } = {}) {
  const abs = path.resolve(repoRoot, SVELTE_DIRECTIVE_PROJECTOR);
  if (!fs.existsSync(abs)) return [];
  const text = fs.readFileSync(abs, "utf8");
  return [...new Set([...text.matchAll(/SvelteDirectiveKind::(\w+)/g)].map((m) => m[1]))].sort();
}

const TS_LANG_ATTR = /\blang\s*=\s*["'](?:ts|typescript)["']/i;
const MODULE_ATTR = /\bmodule\b|context\s*=\s*["']module["']/i;
const TS_NOCHECK = /@ts-nocheck/;
const TS_CHECK = /@ts-check/;

export function classifySvelteEvidence(rel, text) {
  const posix = String(rel || "").replace(/\\/g, "/");
  if (posix.endsWith(".svelte.ts")) {
    return {
      fileKind: ".svelte.ts",
      scriptContext: "module-file",
      dialect: "ts",
      jsCheckingHint: null,
    };
  }
  if (posix.endsWith(".svelte.js")) {
    return {
      fileKind: ".svelte.js",
      scriptContext: "module-file",
      dialect: "js",
      jsCheckingHint: TS_NOCHECK.test(text)
        ? "js-unchecked"
        : TS_CHECK.test(text)
          ? "engine-checked"
          : null,
    };
  }
  if (posix.endsWith(".svelte")) {
    const scripts = [...String(text).matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)];
    if (scripts.length === 0) {
      return {
        fileKind: ".svelte",
        scriptContext: "none",
        dialect: null,
        jsCheckingHint: null,
      };
    }
    const module = scripts.some((match) => MODULE_ATTR.test(match[1]));
    const ts = scripts.some((match) => TS_LANG_ATTR.test(match[1]));
    const bodies = scripts.map((match) => `${match[1]}\n${match[2]}`).join("\n");
    return {
      fileKind: ".svelte",
      scriptContext: module ? "module" : "instance",
      dialect: ts ? "ts" : "js",
      jsCheckingHint: TS_NOCHECK.test(bodies)
        ? "js-unchecked"
        : TS_CHECK.test(bodies)
          ? "engine-checked"
          : null,
    };
  }
  return {
    fileKind: path.extname(posix) || "unknown",
    scriptContext: null,
    dialect: null,
    jsCheckingHint: null,
  };
}

export function assertRuneProbeMatchesPinnedProjection(
  probeSource,
  { preludeSource = "", svelteTypesSource = "" } = {},
) {
  const errors = [];
  if (preludeSource && !preludeSource.includes(PINNED_DERIVED_DECLARE)) {
    errors.push(
      err(
        "STS0-svelte-abi",
        "rune-signature-drift",
        "CCA1I prelude lost the pinned $derived expression signature",
      ),
    );
  }
  if (svelteTypesSource && !svelteTypesSource.includes(PINNED_DERIVED_DECLARE)) {
    errors.push(
      err(
        "STS0-svelte-abi",
        "rune-signature-drift",
        "pinned svelte/types lost $derived<T>(expression: T): T",
      ),
    );
  }
  if (!String(probeSource || "").includes(PINNED_DERIVED_DECLARE)) {
    errors.push(
      err(
        "STS0-svelte-abi",
        "incorrect-rune-signature",
        "rune probe does not use the pinned $derived expression signature",
      ),
    );
  }
  if (
    /\$derived\s*<\s*T\s*>\s*\(\s*compute\s*:/.test(probeSource) ||
    /\$derived\(\(\)\s*=>/.test(probeSource)
  ) {
    errors.push(
      err(
        "STS0-svelte-abi",
        "incorrect-rune-signature",
        "clean rune probe uses callback $derived; that form is $derived.by",
      ),
    );
  }
  return errors;
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
    // The legacy family name is historical naming only: several legacy-named
    // features (stores, transitions/actions) are mode-shared and stay
    // semantics "both" per the mode-paired compiler controls; the mode
    // classification itself is asserted by assertModeClassification.
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
  // Shipped directive projection kinds keep their owning rows: a directive
  // implemented by the projector (class/style today) cannot lose its row
  // without failing the inventory.
  for (const kind of shippedDirectiveKinds({ repoRoot })) {
    const owner = DIRECTIVE_KIND_ROWS[kind];
    if (owner === undefined) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "unowned-feature",
          `shipped directive kind ${kind} has no canonical feature mapping`,
        ),
      );
    } else if (owner && !ids.has(owner)) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "unowned-feature",
          `shipped directive kind ${kind} lost its mandatory owning row ${owner}`,
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
  const populations = Array.isArray(inventory?.populations) ? inventory.populations : [];
  if (populations.length === 0) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "removed-fixture",
        "selected fixture populations were not imported",
      ),
    );
  }
  const populationById = new Map();
  for (const population of populations) {
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
      continue;
    }
    populationById.set(population.id, population);
    // recordedRows is recorded import evidence only. Count equality is
    // deliberately NOT an acceptance gate: a live population legitimately
    // grows or shrinks without a product edit, and equal counts cannot
    // detect a replaced member anyway. Completeness below is identity-based.
  }
  for (const id of REQUIRED_POPULATION_IDS) {
    if (!populationById.has(id)) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "removed-fixture",
          `required fixture population ${id} was not imported`,
        ),
      );
    }
  }
  // The benchmarks population must cite the pinned case inventory itself,
  // not only the Rust adapter that consumes it.
  const benchmarks = populationById.get("svelte-benchmarks");
  if (benchmarks && benchmarks.path !== SVELTE_BENCHMARKS_MANIFEST) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "population-authority-missing",
        `svelte-benchmarks population cites ${benchmarks.path}; the pinned case inventory ${SVELTE_BENCHMARKS_MANIFEST} is the required authority`,
      ),
    );
  }
  const officialCases = populationById.has("svelte-official-cases")
    ? loadOfficialSvelteCases(repoRoot)
    : [];
  const selectedMembers = Array.isArray(inventory?.selectedMembers)
    ? inventory.selectedMembers
    : [];
  if (selectedMembers.length === 0) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "unowned-feature",
        "imported populations have no selected members joined to owning rows",
      ),
    );
  }
  for (const member of selectedMembers) {
    if (!member?.id || !member?.population || !populationById.has(member.population)) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "removed-fixture",
          `selected member ${member?.id || "?"} cites unknown population ${member?.population}`,
        ),
      );
      continue;
    }
    if (!member.owner || !ids.has(member.owner)) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "unowned-feature",
          `upstream selected case ${member.id} has no mandatory owning row`,
        ),
      );
    }
    if (member.path && !fs.existsSync(path.resolve(repoRoot, member.path))) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "removed-fixture",
          `selected member ${member.id} cites missing fixture ${member.path}`,
        ),
      );
    }
    if (member.suite && !officialCases.some((row) => row.suite === member.suite)) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "removed-fixture",
          `selected official suite ${member.suite} is missing from the imported official population`,
        ),
      );
    }
  }
  // Every RequiredCurrent row keeps an owning selected join. This is the
  // selected-population completeness obligation (STS0-AC1): no population's
  // ownership mappings — official, css, compiler, harness or benchmarks —
  // can silently disappear while the row, its source fixture and the
  // population record all remain.
  const citedOwners = new Set(
    selectedMembers.map((member) => member?.owner).filter((owner) => owner),
  );
  for (const row of rows) {
    if (!citedOwners.has(row.id)) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "unselected-feature-row",
          `feature row ${row.id} lost every selected member citing it as owner`,
        ),
      );
    }
  }
  // Membership of the file- and case-addressable populations is derived from
  // the live population authorities, not from
  // the mappings the product chose to supply: every harness fixture and
  // every pinned benchmark smoke case must keep an owning selected member.
  const memberKeys = new Set(
    selectedMembers.map((member) => {
      const identity = member.caseId ?? member.path;
      return `${member.population}\u0000${identity}`;
    }),
  );
  for (const requiredMember of deriveSelectedMembership({ repoRoot })) {
    const identity = requiredMember.caseId ?? requiredMember.path;
    const key = `${requiredMember.population}\u0000${identity}`;
    if (!memberKeys.has(key)) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "unselected-population-member",
          `imported population member ${identity} (${requiredMember.population}) lost its owning selected mapping`,
        ),
      );
    }
  }
  return errors;
}

/**
 * Loads `compile` from the pinned svelte toolchain (the same
 * node_modules/svelte population the inventory records).
 */
export function loadPinnedSvelteCompile({ repoRoot = REPO_ROOT } = {}) {
  const require = createRequire(path.join(repoRoot, "node_modules", "svelte", "package.json"));
  return require("svelte/compiler").compile;
}

function compileSvelteFixture(compile, source, runes) {
  try {
    const result = compile(String(source), { runes, generate: "client" });
    return { warnings: (result.warnings || []).map((w) => w.code), errors: [] };
  } catch (error) {
    return { warnings: [], errors: [error?.code || "compile-error"] };
  }
}

/**
 * STS0-svelte-inventory mode classification, pinned by the pinned compiler:
 * the mode-shared features compile clean under BOTH runes and legacy mode,
 * the deprecated-in-runes features (on:click, <slot>) are ACCEPTED in runes
 * mode with their recorded deprecation warnings rather than rejected, and a
 * genuinely legacy-only construct is rejected in runes mode. A row
 * classified against this live evidence (a mode-shared feature marked
 * single-mode, a deprecated-in-runes feature marked legacy-only, or
 * export-let marked mode-shared) fails.
 */
export function assertModeClassification(inventory, { repoRoot = REPO_ROOT, compile = null } = {}) {
  const errors = [];
  let compileFn = compile;
  if (!compileFn) {
    try {
      compileFn = loadPinnedSvelteCompile({ repoRoot });
    } catch {
      return [
        err(
          "STS0-svelte-inventory",
          "missing-engine",
          "the pinned svelte toolchain did not resolve for the mode controls",
        ),
      ];
    }
  }
  const rows = Array.isArray(inventory?.rows) ? inventory.rows : [];
  const rowsById = new Map(rows.map((row) => [row.id, row]));
  const readFixture = (rel) => {
    try {
      return fs.readFileSync(path.resolve(repoRoot, rel), "utf8");
    } catch {
      return null;
    }
  };
  const controls = [
    {
      label: "runes-mode shared-features control",
      rel: MODE_CONTROL_RUNES_FIXTURE,
      runes: true,
      expect: "clean",
    },
    {
      label: "legacy-mode shared-features control",
      rel: MODE_CONTROL_LEGACY_FIXTURE,
      runes: false,
      expect: "clean",
    },
    {
      label: "attach control under runes mode",
      rel: MODE_CONTROL_ATTACH_RUNES_FIXTURE,
      runes: true,
      expect: "clean",
    },
    {
      label: "attach control under legacy mode",
      rel: MODE_CONTROL_ATTACH_LEGACY_FIXTURE,
      runes: false,
      expect: "clean",
    },
    {
      label: "events/slots control under legacy mode",
      rel: MODE_CONTROL_EVENTS_SLOTS_LEGACY_FIXTURE,
      runes: false,
      expect: "clean",
    },
    {
      label: "events/slots control under runes mode",
      rel: MODE_CONTROL_EVENTS_SLOTS_RUNES_FIXTURE,
      runes: true,
      expect: "deprecated",
      warningCodes: [
        MODE_DEPRECATED_WARNING_CODES["events-legacy-directive"],
        MODE_DEPRECATED_WARNING_CODES["legacy-slots"],
      ],
    },
    {
      label: "legacy-only control under legacy mode",
      rel: MODE_CONTROL_LEGACY_ONLY_FIXTURE,
      runes: false,
      expect: "clean",
    },
    {
      label: "legacy-only control under runes mode",
      rel: MODE_CONTROL_LEGACY_ONLY_FIXTURE,
      runes: true,
      expect: "rejected",
    },
  ];
  for (const control of controls) {
    const source = readFixture(control.rel);
    if (source === null) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "removed-fixture",
          `mode control fixture ${control.rel} is missing`,
        ),
      );
      continue;
    }
    const outcome = compileSvelteFixture(compileFn, source, control.runes);
    if (control.expect === "clean") {
      const accepted = outcome.errors.length === 0 && outcome.warnings.length === 0;
      if (!accepted) {
        errors.push(
          err(
            "STS0-svelte-inventory",
            "mode-control-failed",
            `${control.label} was not accepted cleanly by the pinned compiler (errors ${JSON.stringify(outcome.errors)}, warnings ${JSON.stringify(outcome.warnings)})`,
          ),
        );
      }
    } else if (control.expect === "rejected") {
      if (outcome.errors.length === 0) {
        errors.push(
          err(
            "STS0-svelte-inventory",
            "mode-control-failed",
            `${control.label} was not rejected by the pinned compiler`,
          ),
        );
      }
    } else {
      // "deprecated": accepted WITHOUT errors, WITH the recorded deprecation
      // warnings — deprecation stays separate from unsupported-mode errors.
      if (outcome.errors.length > 0) {
        errors.push(
          err(
            "STS0-svelte-inventory",
            "mode-control-failed",
            `${control.label} was rejected by the pinned compiler instead of accepted with deprecation warnings (errors ${JSON.stringify(outcome.errors)})`,
          ),
        );
      }
      for (const code of control.warningCodes || []) {
        if (!outcome.warnings.includes(code)) {
          errors.push(
            err(
              "STS0-svelte-inventory",
              "mode-control-failed",
              `${control.label} did not record the expected deprecation warning ${code} (warnings ${JSON.stringify(outcome.warnings)})`,
            ),
          );
        }
      }
    }
  }
  for (const id of MODE_CONTROL_SHARED_FEATURES) {
    const row = rowsById.get(id);
    if (!row) continue; // a missing row is already flagged by the required list
    if (row.semantics !== "both") {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "mode-misclassified",
          `feature ${id} compiles clean in both runes and legacy mode under the pinned compiler but is classified ${row.semantics}`,
        ),
      );
    }
  }
  for (const id of MODE_CONTROL_DEPRECATED_IN_RUNES) {
    const row = rowsById.get(id);
    if (!row) continue; // a missing row is already flagged by the required list
    if (row.semantics !== "both") {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "mode-misclassified",
          `feature ${id} is accepted in runes mode with only deprecation warnings by the pinned compiler but is classified ${row.semantics}; deprecation must not exclude a legal mode`,
        ),
      );
    }
  }
  const legacyOnlyRow = rowsById.get(MODE_LEGACY_ONLY_FEATURE);
  if (legacyOnlyRow && legacyOnlyRow.semantics !== "legacy") {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "mode-misclassified",
        `${MODE_LEGACY_ONLY_FEATURE} is rejected in runes mode by the pinned compiler and must stay semantics legacy`,
      ),
    );
  }
  return errors;
}

/**
 * The publishing surface a profile's script context can legally claim:
 * `.svelte` instance/template surfaces publish the component declaration
 * carrier; module contexts and the `.svelte.ts`/`.svelte.js` module-file
 * kinds publish their module exports. A row claiming the other publishing
 * behavior is a contract break, not a stylistic choice.
 */
export function expectedPublishingForRow(row) {
  const moduleSurface =
    row?.fileKind === ".svelte.ts" ||
    row?.fileKind === ".svelte.js" ||
    (row?.fileKind === ".svelte" && row?.scriptContext === "module");
  return moduleSurface ? "module-exports-published" : "declarations-published";
}

/**
 * STS0-policy-lock structural contract for the live behavior path: every
 * profile row pins the declaration-visible binding names its publishing
 * surface must expose (empty only for runes instance rows without public
 * props and the template-only surface), and the
 * claimed publishing behavior must match the row's script context. The
 * behavioral proof itself runs through the owned CCA1I backend gate below —
 * this projector-free check only pins what the product claims.
 */
export function assertProfileContract(policy) {
  const errors = [];
  const rows = Array.isArray(policy?.profiles) ? policy.profiles : [];
  for (const row of rows) {
    if (!Array.isArray(row?.publishedSymbols)) {
      errors.push(
        err(
          "STS0-policy-lock",
          "publication-missing",
          `profile ${row?.id} carries no publishedSymbols pin for the live behavior path`,
        ),
      );
    } else if (
      row.publishedSymbols.length === 0 &&
      (row.publishing === "module-exports-published" ||
        (row.publishing === "declarations-published" &&
          row.scriptContext === "instance" &&
          row.semantics === "legacy"))
    ) {
      errors.push(
        err(
          "STS0-policy-lock",
          "publication-missing",
          `profile ${row?.id} claims ${row.publishing} but pins no declaration-visible symbols`,
        ),
      );
    }
    const expected = expectedPublishingForRow(row);
    if (row?.publishing && row.publishing !== expected) {
      errors.push(
        err(
          "STS0-policy-lock",
          "publication-mismatch",
          `profile ${row?.id} claims publishing ${row.publishing} but its script context publishes ${expected}`,
        ),
      );
    }
  }
  return errors;
}

/**
 * The Rust-side profile gate in `verter_session` (tests/cases/
 * sts0_profile_gate.rs): it projects every policy profile's evidence
 * fixture through the OWNED CCA1I `SvelteProjectionBackend` (project_ide),
 * checks the real generated carriers on BOTH claimed engines (ts-js 6.0.3
 * and ts-native 7.0.2), corrupts the evidence templates and the generated
 * carriers themselves (engine-checked rows must fail, js-unchecked rows
 * must not report), and consumes the declaration/module publication
 * surfaces with renaming twins. STS0's runtimeCorrectness claim rests on
 * these named tests executing — a documentation-only substitute is
 * forbidden by the frozen charter §14.
 */
export const STS0_PROFILE_GATE_TESTS = Object.freeze([
  "cases::sts0_profile_gate::backend_projection_checks_clean_on_both_claimed_engines",
  "cases::sts0_profile_gate::evidence_template_corruption_is_caught_on_both_claimed_engines",
  "cases::sts0_profile_gate::projected_carrier_corruption_is_caught_on_both_claimed_engines",
  "cases::sts0_profile_gate::pinned_declaration_symbols_appear_in_the_host_declaration_carrier",
  "cases::sts0_profile_gate::publication_surfaces_publish_and_survive_renames_on_both_claimed_engines",
]);

/**
 * Both pinned engines the live profile gate must EXECUTE on. The Rust gate
 * prints an `STS0-ENGINE <label> <version>` manifest line per resolved
 * engine, so a green receipt that cannot show both lines did not run the
 * claimed engine matrix (STS0-AC1).
 */
export const STS0_PROFILE_GATE_ENGINES = Object.freeze([
  { label: "ts-js", version: "6.0.3" },
  { label: "ts-native", version: "7.0.2" },
]);

const CARGO_SUMMARY_RE = /test result: (ok|FAILED)\. (\d+) passed; (\d+) failed/;

export function runSts0ProfileGate(repoRoot = REPO_ROOT) {
  const result = spawnSync(
    "cargo",
    [
      "test",
      "-p",
      "verter_session",
      "--test",
      "main",
      "cases::sts0_profile_gate",
      "--",
      "--test-threads=1",
      // The engine manifest lines and skip notes the receipt is validated
      // against are test stderr — libtest hides captured output of passing
      // tests, so the acceptance run must not capture it.
      "--nocapture",
    ],
    {
      cwd: repoRoot,
      encoding: "utf8",
      windowsHide: true,
      timeout: 600000,
      // Both pinned engines are REQUIRED for this acceptance invocation: a
      // missing launcher makes the Rust gate hard-fail instead of skipping,
      // so the gate cannot certify itself green on one engine (STS0-AC1).
      env: { ...process.env, VERTER_REQUIRE_TYPECHECKER: "1" },
    },
  );
  const stdout = `${result.stdout || ""}${result.stderr || ""}`;
  return { status: result.status, error: result.error, stdout };
}

function isTimeoutSpawnError(error) {
  const code = error?.code;
  if (code === "ETIMEDOUT" || code === "ERR_CHILD_PROCESS_STDIO_MAXBUFFER") return true;
  return /timed?\s*out/i.test(String(error?.message || ""));
}

export function assertProfileGateRun(run) {
  const errors = [];
  if (run.error) {
    if (run.error.code === "ENOENT") {
      errors.push(
        err(
          "STS0-policy-lock",
          "environment-unavailable",
          `the owned-backend profile gate cannot run: cargo is not available (${run.error.message})`,
        ),
      );
    } else if (isTimeoutSpawnError(run.error)) {
      errors.push(
        err(
          "STS0-policy-lock",
          "environment-unavailable",
          `the owned-backend profile gate timed out: ${run.error.message}`,
        ),
      );
    } else {
      errors.push(
        err(
          "STS0-policy-lock",
          "environment-unavailable",
          `the owned-backend profile gate failed to spawn: ${run.error.message}`,
        ),
      );
    }
    return errors;
  }
  const summary = CARGO_SUMMARY_RE.exec(run.stdout || "");
  if (run.status !== 0) {
    const detail = summary
      ? `status=${run.status} passed=${summary[2]} failed=${summary[3]}`
      : `status=${run.status} (no cargo test summary)`;
    errors.push(
      err(
        "STS0-policy-lock",
        "test-failure",
        `the owned-backend profile gate cargo run failed (${detail})`,
      ),
    );
  } else if (!summary || summary[1] !== "ok" || Number(summary[2]) === 0) {
    const detail = summary
      ? `status=${run.status} passed=${summary[2]} failed=${summary[3]}`
      : `status=${run.status} (no cargo test summary)`;
    errors.push(
      err(
        "STS0-policy-lock",
        "profile-gate-missing",
        `the owned-backend profile gate did not execute its cases (${detail})`,
      ),
    );
  }
  for (const name of STS0_PROFILE_GATE_TESTS) {
    const failed = (run.stdout || "").includes(`${name} ... FAILED`);
    const passed = (run.stdout || "").includes(`${name} ... ok`);
    if (!passed || failed) {
      errors.push(
        err(
          "STS0-policy-lock",
          "profile-gate-failed",
          passed
            ? `owned-backend profile gate test ${name} failed`
            : `owned-backend profile gate test ${name} did not pass (status=${run.status})`,
        ),
      );
    }
  }
  // A GREEN receipt must additionally prove it executed on BOTH pinned
  // engines: any skip note, or a missing engine manifest line, means the
  // run certified itself without the claimed engine matrix (STS0-AC1). Red
  // receipts are already rejected above and keep their failure
  // classification.
  if (run.status === 0) {
    const stdout = run.stdout || "";
    if (stdout.includes("a pinned STS0 engine launcher was not found")) {
      errors.push(
        err(
          "STS0-policy-lock",
          "profile-gate-missing",
          "the owned-backend profile gate skipped: a pinned STS0 engine launcher was not found (both engines are required for this acceptance run)",
        ),
      );
    }
    for (const { label, version } of STS0_PROFILE_GATE_ENGINES) {
      if (!stdout.includes(`STS0-ENGINE ${label} ${version}`)) {
        errors.push(
          err(
            "STS0-policy-lock",
            "profile-gate-missing",
            `the owned-backend profile gate did not execute on the pinned ${label} ${version} engine`,
          ),
        );
      }
    }
  }
  return errors;
}

/**
 * STS0-policy-lock live path: the claimed checking/publishing behavior is
 * proven by EXECUTION — the structural publication pins here, plus the
 * verter_session gate that drives the evidence fixtures through the owned
 * SvelteProjectionBackend on each claimed engine with corruption twins.
 */
export function assertProfileBehavior(policy, { repoRoot = REPO_ROOT } = {}) {
  const errors = [...assertProfileContract(policy)];
  errors.push(...assertProfileGateRun(runSts0ProfileGate(repoRoot)));
  return errors;
}

function loadPlaygroundTypeScript(repoRoot = REPO_ROOT) {
  const pkgDir = path.join(repoRoot, "packages", "playground", "node_modules", "typescript");
  const require = createRequire(path.join(pkgDir, "package.json"));
  return require(path.join(pkgDir, "lib", "typescript.js"));
}

function tsLiteralValue(ts, node) {
  if (!node) return undefined;
  if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) return node.text;
  if (ts.isNumericLiteral(node)) return Number(node.text);
  if (node.kind === ts.SyntaxKind.TrueKeyword) return true;
  if (node.kind === ts.SyntaxKind.FalseKeyword) return false;
  if (
    ts.isPrefixUnaryExpression(node) &&
    node.operator === ts.SyntaxKind.MinusToken &&
    ts.isNumericLiteral(node.operand)
  ) {
    return -Number(node.operand.text);
  }
  if (ts.isArrayLiteralExpression(node)) {
    return node.elements.map((element) => tsLiteralValue(ts, element));
  }
  if (ts.isObjectLiteralExpression(node)) {
    const out = {};
    for (const prop of node.properties) {
      if (!ts.isPropertyAssignment(prop)) continue;
      const name = ts.isIdentifier(prop.name)
        ? prop.name.text
        : ts.isStringLiteral(prop.name)
          ? prop.name.text
          : undefined;
      if (!name) continue;
      out[name] = tsLiteralValue(ts, prop.initializer);
    }
    return out;
  }
  if (ts.isAsExpression(node) || ts.isParenthesizedExpression(node)) {
    return tsLiteralValue(ts, node.expression);
  }
  if (typeof ts.isSatisfiesExpression === "function" && ts.isSatisfiesExpression(node)) {
    return tsLiteralValue(ts, node.expression);
  }
  return undefined;
}

/**
 * Read contract.ts exported const literals through the probe TypeScript
 * project. contract.ts re-exports Svelte probe modules, so it must not be
 * imported as a Node runtime module.
 */
export function readContractConstantsFromProbeProject({ repoRoot = REPO_ROOT } = {}) {
  const ts = loadPlaygroundTypeScript(repoRoot);
  const probesDir = path.join(STS0_DIR, "probes");
  const tsconfigAbs = path.join(probesDir, "tsconfig.json");
  const raw = ts.readConfigFile(tsconfigAbs, ts.sys.readFile);
  if (raw.error) {
    throw new Error(ts.flattenDiagnosticMessageText(raw.error.messageText, "\n"));
  }
  const parsed = ts.parseJsonConfigFileContent(
    raw.config,
    ts.sys,
    probesDir,
    undefined,
    tsconfigAbs,
  );
  const contractAbs = path.join(STS0_DIR, "contract.ts");
  const options = { ...parsed.options, noEmit: true, noResolve: true };
  const host = ts.createCompilerHost(options, true);
  const program = ts.createProgram({
    rootNames: [contractAbs],
    options,
    host,
  });
  const sourceFile = program.getSourceFile(contractAbs);
  if (!sourceFile) return null;
  const exported = {};
  for (const stmt of sourceFile.statements) {
    if (!ts.isVariableStatement(stmt)) continue;
    const isExport = stmt.modifiers?.some((mod) => mod.kind === ts.SyntaxKind.ExportKeyword);
    if (!isExport) continue;
    for (const decl of stmt.declarationList.declarations) {
      if (!ts.isIdentifier(decl.name) || !decl.initializer) continue;
      exported[decl.name.text] = tsLiteralValue(ts, decl.initializer);
    }
  }
  return exported;
}

export function assertContractConstantsJoin(policy, { repoRoot = REPO_ROOT } = {}) {
  const errors = [];
  let exported;
  try {
    exported = readContractConstantsFromProbeProject({ repoRoot });
  } catch (error) {
    errors.push(
      err(
        "STS0-policy-lock",
        "contract-join-missing",
        `contract.ts could not be read through the probe TypeScript project: ${error.message}`,
      ),
    );
    return errors;
  }
  if (!exported) {
    errors.push(
      err(
        "STS0-policy-lock",
        "contract-join-missing",
        "contract.ts was not present in the probe TypeScript project",
      ),
    );
    return errors;
  }
  if (JSON.stringify(exported.acceptedProducts) !== JSON.stringify(policy?.acceptedProducts)) {
    errors.push(
      err(
        "STS0-svelte-abi",
        "contract-join-missing",
        "contract.ts acceptedProducts drifted from policy.acceptedProducts",
      ),
    );
  }
  const shape = policy?.componentShape || {};
  const selected = exported.selectedProfile || {};
  for (const field of ["publicShape", "vueConstructorRequired", "instanceTypeRequirement"]) {
    if (selected[field] !== shape[field]) {
      errors.push(
        err(
          "STS0-svelte-abi",
          "contract-join-missing",
          `contract.ts selectedProfile.${field} drifted from policy.componentShape`,
        ),
      );
    }
  }
  if (JSON.stringify(exported.dialectFileKinds) !== JSON.stringify([...SVELTE_FILE_KINDS])) {
    errors.push(
      err(
        "STS0-policy-lock",
        "contract-join-missing",
        "contract.ts dialectFileKinds drifted from SVELTE_FILE_KINDS",
      ),
    );
  }
  if (JSON.stringify(exported.semanticsModes) !== JSON.stringify([...SEMANTICS_MODES])) {
    errors.push(
      err(
        "STS0-policy-lock",
        "contract-join-missing",
        "contract.ts semanticsModes drifted from SEMANTICS_MODES",
      ),
    );
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
    } else {
      const evidenceText = fs.readFileSync(path.resolve(repoRoot, row.evidencePath), "utf8");
      const classified = classifySvelteEvidence(row.evidencePath, evidenceText);
      if (classified.fileKind !== row.fileKind) {
        errors.push(
          err(
            "STS0-policy-lock",
            "evidence-mismatch",
            `profile ${row.id} fileKind ${row.fileKind} does not match evidence ${classified.fileKind}`,
          ),
        );
      }
      if (classified.scriptContext && classified.scriptContext !== row.scriptContext) {
        errors.push(
          err(
            "STS0-policy-lock",
            "evidence-mismatch",
            `profile ${row.id} scriptContext ${row.scriptContext} does not match evidence ${classified.scriptContext}`,
          ),
        );
      }
      if (classified.dialect && classified.dialect !== row.dialect) {
        errors.push(
          err(
            "STS0-policy-lock",
            "evidence-mismatch",
            `profile ${row.id} dialect ${row.dialect} does not match evidence ${classified.dialect}`,
          ),
        );
      }
      if (row.dialect === "js") {
        if (row.checking === "engine-checked" && row.checkJs !== true) {
          errors.push(
            err(
              "STS0-policy-lock",
              "unspecified-behavior",
              `profile ${row.id} engine-checked JS must set checkJs true`,
            ),
          );
        }
        if (classified.jsCheckingHint && classified.jsCheckingHint !== row.checking) {
          errors.push(
            err(
              "STS0-policy-lock",
              "evidence-mismatch",
              `profile ${row.id} checking ${row.checking} does not match evidence ${classified.jsCheckingHint}`,
            ),
          );
        }
        // A scriptless surface has no script body to carry a @ts-check or
        // @ts-nocheck pragma; its carrier is checked by the live provider's
        // checkJs default instead, so the hint is required only where a
        // script body exists.
        if (!classified.jsCheckingHint && classified.scriptContext !== "none") {
          errors.push(
            err(
              "STS0-policy-lock",
              "evidence-mismatch",
              `profile ${row.id} JS evidence must carry @ts-check or @ts-nocheck`,
            ),
          );
        }
      }
    }
  }
  for (const id of REQUIRED_POLICY_PROFILE_IDS) {
    if (!ids.has(id)) {
      errors.push(
        err(
          "STS0-policy-lock",
          "missing-policy",
          `supported profile ${id} is missing explicit checking and publishing behavior`,
        ),
      );
    }
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
  const refusedOptions = Array.isArray(policy.refusedOptions) ? policy.refusedOptions : [];
  for (const refused of refusedOptions) {
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
  const failClosed = Array.isArray(tsv)
    ? tsv.filter((entry) => entry.classification === "unsupported fail-closed")
    : [];
  for (const entry of failClosed) {
    const kept = refusedOptions.some(
      (refused) => refused.surface === entry.surface && refused.option === entry.option,
    );
    if (!kept) {
      errors.push(
        err(
          "STS0-policy-lock",
          "silently-ignored-option",
          `required fail-closed option ${entry.surface}/${entry.option} is missing from refusedOptions`,
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
  const cells = Array.isArray(matrix?.cells) ? matrix.cells : [];
  if (cells.length === 0) {
    errors.push(
      err(
        "STS0-svelte-pin",
        "missing-cell",
        "engine/framework matrix has no cells for the admitted pairs",
      ),
    );
  }
  for (const engine of engines) {
    if (!cells.some((cell) => cell.engine === engine.id && cell.framework === framework.id)) {
      errors.push(
        err(
          "STS0-svelte-pin",
          "missing-cell",
          `missing engine/framework cell ${engine.id}/${framework.id}`,
        ),
      );
    }
  }
  for (const cell of cells) {
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
  evidencePath: "tests/sfc-projection/STS0/probes/profiles/instance-runes.svelte",
});

export const DIRTY_POLICY_REFUSED_OPTION = Object.freeze({
  surface: "svelte:CompileOptions",
  option: "made-up-option",
  behavior: "refused-fail-closed",
});

export const DIRTY_POLICY_DROP_PROFILE = "svelte-ts-instance-runes";
export const DIRTY_POLICY_DROP_MODULE = "svelte-ts-module-context";
export const DIRTY_POLICY_DROP_UNCHECKED_LEGACY = "svelte-js-instance-legacy-unchecked";
export const DIRTY_POLICY_DROP_REFUSAL = "accessors";

export const DIRTY_INVENTORY_DROP_SELECTED_MAPPING = "harness-props-events";
/** Every selected member cites this fixture; deleting them all while the
 * file remains must trip the file-identity completeness rule. */
export const DIRTY_INVENTORY_DROP_SELECTED_MAPPING_PATH =
  "packages/framework-conformance-harness/fixtures/svelte/props-events.svelte";
export const DIRTY_INVENTORY_DROP_SMOKE_CASE = "svelte/fixtures/20/Comp00000.svelte";
export const DIRTY_INVENTORY_DROP_DIRECTIVE_ROW = "class-directive";
export const DIRTY_INVENTORY_DROP_OFFICIAL_JOIN = "official-runtime-runes";
export const DIRTY_INVENTORY_DROP_CSS_JOIN = "css-attr-selectors";
export const DIRTY_INVENTORY_DROP_COMPILER_JOIN = "template-attach";
export const DIRTY_POLICY_PUBLISHING_FLIP_PROFILE = "svelte-ts-instance-runes";

export const DIRTY_INVENTORY_UNOWNED_SELECTED = Object.freeze({
  id: "unowned-upstream-case",
  population: "framework-conformance-harness-fixtures",
  path: "packages/framework-conformance-harness/fixtures/svelte/props-events.svelte",
  owner: "not-an-inventory-row",
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
  const cleanContract = assertProfileContract(policy);
  if (cleanContract.length) errors.push(...cleanContract);
  const flippedPublishing = cloneJson(policy);
  const flippedRow = flippedPublishing.profiles.find(
    (row) => row.id === DIRTY_POLICY_PUBLISHING_FLIP_PROFILE,
  );
  if (flippedRow) flippedRow.publishing = "module-exports-published";
  if (
    !assertProfileContract(flippedPublishing).some((error) => error.code === "publication-mismatch")
  ) {
    errors.push(
      err(
        "STS0-policy-lock",
        "missed-unspecified",
        "an instance profile flipped to module-exports-published was not rejected",
      ),
    );
  }
  const missingSymbols = cloneJson(policy);
  const symbolsRow = missingSymbols.profiles.find(
    (row) => row.id === DIRTY_POLICY_PUBLISHING_FLIP_PROFILE,
  );
  if (symbolsRow) delete symbolsRow.publishedSymbols;
  if (
    !assertProfileContract(missingSymbols).some((error) => error.code === "publication-missing")
  ) {
    errors.push(
      err(
        "STS0-policy-lock",
        "missed-unspecified",
        "a profile dropping its publishedSymbols pin was not rejected",
      ),
    );
  }
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
  const droppedProfile = cloneJson(policy);
  droppedProfile.profiles = droppedProfile.profiles.filter(
    (row) => row.id !== DIRTY_POLICY_DROP_PROFILE,
  );
  if (!assertPolicyLock(droppedProfile).some((error) => error.code === "missing-policy")) {
    errors.push(
      err(
        "STS0-policy-lock",
        "missed-unspecified",
        "deleting svelte-ts-instance-runes was not rejected",
      ),
    );
  }
  const droppedModule = cloneJson(policy);
  droppedModule.profiles = droppedModule.profiles.filter(
    (row) => row.id !== DIRTY_POLICY_DROP_MODULE,
  );
  if (!assertPolicyLock(droppedModule).some((error) => error.code === "missing-policy")) {
    errors.push(
      err(
        "STS0-policy-lock",
        "missed-unspecified",
        "deleting a .svelte module-context profile was not rejected",
      ),
    );
  }
  const droppedUncheckedLegacy = cloneJson(policy);
  droppedUncheckedLegacy.profiles = droppedUncheckedLegacy.profiles.filter(
    (row) => row.id !== DIRTY_POLICY_DROP_UNCHECKED_LEGACY,
  );
  if (!assertPolicyLock(droppedUncheckedLegacy).some((error) => error.code === "missing-policy")) {
    errors.push(
      err(
        "STS0-policy-lock",
        "missed-unspecified",
        "deleting the unchecked legacy-JS instance profile was not rejected",
      ),
    );
  }
  const emptiedRefusals = cloneJson(policy);
  emptiedRefusals.refusedOptions = [];
  if (
    !assertPolicyLock(emptiedRefusals).some((error) => error.code === "silently-ignored-option")
  ) {
    errors.push(
      err(
        "STS0-policy-lock",
        "missed-unspecified",
        "clearing every required option refusal was not rejected",
      ),
    );
  }
  const droppedRefusal = cloneJson(policy);
  droppedRefusal.refusedOptions = droppedRefusal.refusedOptions.filter(
    (row) => row.option !== DIRTY_POLICY_DROP_REFUSAL,
  );
  if (!assertPolicyLock(droppedRefusal).some((error) => error.code === "silently-ignored-option")) {
    errors.push(
      err(
        "STS0-policy-lock",
        "missed-unspecified",
        "deleting one authority-backed refusal was not rejected",
      ),
    );
  }
  const emptyLegacy = cloneJson(policy);
  const emptyLegacyRow = emptyLegacy.profiles.find((row) => row.id === "svelte-ts-instance-legacy");
  if (emptyLegacyRow) emptyLegacyRow.publishedSymbols = [];
  if (!assertProfileContract(emptyLegacy).some((error) => error.code === "publication-missing")) {
    errors.push(
      err(
        "STS0-policy-lock",
        "missed-unspecified",
        "a legacy instance profile pinning no declaration-visible symbols was not rejected",
      ),
    );
  }
  const mismatchedEvidence = cloneJson(policy);
  const mismatchedRow = mismatchedEvidence.profiles.find(
    (row) => row.id === "svelte-ts-instance-runes",
  );
  if (mismatchedRow) {
    mismatchedRow.evidencePath = "tests/sfc-projection/STS0/probes/profiles/module-context.svelte";
  }
  if (!assertPolicyLock(mismatchedEvidence).some((error) => error.code === "evidence-mismatch")) {
    errors.push(
      err(
        "STS0-policy-lock",
        "missed-unspecified",
        "a profile whose evidence script context does not match the row was not rejected",
      ),
    );
  }
  const driftedSelection = cloneJson(policy);
  driftedSelection.semanticsSelection.row.option = "not-an-official-option";
  if (!assertPolicyLock(driftedSelection).some((error) => error.code === "options-join-missing")) {
    errors.push(
      err(
        "STS0-policy-lock",
        "missed-unspecified",
        "a runes selection outside the official options population was not rejected",
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
  const emptiedPopulations = cloneJson(inventory);
  emptiedPopulations.populations = [];
  if (
    !assertSvelteInventory(emptiedPopulations).some((error) => error.code === "removed-fixture")
  ) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "missed-unowned-feature",
        "empty imported populations were not rejected",
      ),
    );
  }
  // Selected ownership joins cannot disappear population-wide: deleting the
  // official, css or compiler population's mapping while its source fixture,
  // suite and population record all remain must be rejected.
  for (const [dropId, label] of [
    [DIRTY_INVENTORY_DROP_OFFICIAL_JOIN, "official suite join"],
    [DIRTY_INVENTORY_DROP_CSS_JOIN, "css corpus join"],
    [DIRTY_INVENTORY_DROP_COMPILER_JOIN, "compiler surface join"],
  ]) {
    const droppedJoin = cloneJson(inventory);
    droppedJoin.selectedMembers = droppedJoin.selectedMembers.filter(
      (member) => member.id !== dropId,
    );
    if (
      !assertSvelteInventory(droppedJoin).some((error) => error.code === "unselected-feature-row")
    ) {
      errors.push(
        err(
          "STS0-svelte-inventory",
          "missed-unowned-feature",
          `deleting the ${label} while retaining its source and population record was not rejected`,
        ),
      );
    }
  }
  const unownedSelected = cloneJson(inventory);
  unownedSelected.selectedMembers = [
    ...(unownedSelected.selectedMembers || []),
    cloneJson(DIRTY_INVENTORY_UNOWNED_SELECTED),
  ];
  if (!assertSvelteInventory(unownedSelected).some((error) => error.code === "unowned-feature")) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "missed-unowned-feature",
        "an upstream selected case without an owning row was not rejected",
      ),
    );
  }
  // Selected membership is derived from the population authorities: deleting
  // one real harness mapping while its fixture remains, shrinking the
  // selected set to a single supplied member, or dropping a pinned benchmark
  // smoke case must each be rejected.
  const droppedMapping = cloneJson(inventory);
  droppedMapping.selectedMembers = droppedMapping.selectedMembers.filter(
    (member) => member.path !== DIRTY_INVENTORY_DROP_SELECTED_MAPPING_PATH,
  );
  if (
    !assertSvelteInventory(droppedMapping).some(
      (error) => error.code === "unselected-population-member",
    )
  ) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "missed-unowned-feature",
        "removing a harness fixture's selected mapping while retaining the fixture was not rejected",
      ),
    );
  }
  const shrankSelected = cloneJson(inventory);
  shrankSelected.selectedMembers = shrankSelected.selectedMembers.slice(0, 1);
  if (
    !assertSvelteInventory(shrankSelected).some(
      (error) => error.code === "unselected-population-member",
    )
  ) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "missed-unowned-feature",
        "shrinking the selected members to one supplied row was not rejected",
      ),
    );
  }
  const droppedSmokeCase = cloneJson(inventory);
  droppedSmokeCase.selectedMembers = droppedSmokeCase.selectedMembers.filter(
    (member) => member.caseId !== DIRTY_INVENTORY_DROP_SMOKE_CASE,
  );
  if (
    !assertSvelteInventory(droppedSmokeCase).some(
      (error) => error.code === "unselected-population-member",
    )
  ) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "missed-unowned-feature",
        "dropping a pinned benchmark smoke case member was not rejected",
      ),
    );
  }
  const adapterOnlyBenchmarks = cloneJson(inventory);
  const adapterOnlyPopulation = adapterOnlyBenchmarks.populations.find(
    (population) => population.id === "svelte-benchmarks",
  );
  if (adapterOnlyPopulation) adapterOnlyPopulation.path = SVELTE_BENCHMARKS_ADAPTER;
  if (
    !assertSvelteInventory(adapterOnlyBenchmarks).some(
      (error) => error.code === "population-authority-missing",
    )
  ) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "missed-unowned-feature",
        "pointing the benchmarks population at only its adapter was not rejected",
      ),
    );
  }
  const droppedDirectiveRow = cloneJson(inventory);
  droppedDirectiveRow.rows = droppedDirectiveRow.rows.filter(
    (row) => row.id !== DIRTY_INVENTORY_DROP_DIRECTIVE_ROW,
  );
  if (
    !assertSvelteInventory(droppedDirectiveRow).some((error) => error.code === "unowned-feature")
  ) {
    errors.push(
      err(
        "STS0-svelte-inventory",
        "missed-unowned-feature",
        "a shipped directive kind losing its owning row was not rejected",
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
  const emptyCells = cloneJson(matrix);
  emptyCells.cells = [];
  if (!assertEngineFrameworkPins(emptyCells).some((error) => error.code === "missing-cell")) {
    errors.push(
      err(
        "STS0-svelte-pin",
        "missed-latest-claim",
        "an empty engine/framework cell set was not rejected",
      ),
    );
  }
  const droppedCell = cloneJson(matrix);
  droppedCell.cells = droppedCell.cells.filter((cell) => cell.engine !== "ts-native");
  if (!assertEngineFrameworkPins(droppedCell).some((error) => error.code === "missing-cell")) {
    errors.push(
      err(
        "STS0-svelte-pin",
        "missed-latest-claim",
        "removing one required engine/framework cell was not rejected",
      ),
    );
  }
  return errors;
}

export function evaluateSts0(input = {}) {
  const errors = [];
  errors.push(...validateSts0Products());
  const policy = loadSts0Product("svelte-projection-policy.json");
  const inventory = loadSts0Product("svelte-current-feature-inventory.json");
  errors.push(...assertModeClassification(inventory, { compile: input.compile ?? undefined }));
  // skipLive is the harness product-only path (skipProbes). The live cargo
  // gate remains mandatory when skipLive is unset (charter §14).
  errors.push(
    ...(input.skipLive
      ? assertProfileContract(policy)
      : assertProfileBehavior(policy, { repoRoot: input.repoRoot ?? REPO_ROOT })),
  );
  const positiveAbs = path.join(STS0_DIR, "probes", "positive.ts");
  const positiveSource = fs.readFileSync(positiveAbs, "utf8");
  errors.push(...assertSvelteShapeSource(positiveSource));
  errors.push(
    ...assertInstanceShapePin(positiveSource, loadSts0Manifest()?.probes?.expectedInstanceType),
  );
  const probeAbs = path.join(STS0_DIR, "probes", "state-module.svelte.ts");
  const preludeAbs = path.resolve(REPO_ROOT, SVELTE_PRELUDE);
  const typesAbs = path.resolve(REPO_ROOT, PINNED_SVELTE_TYPES);
  errors.push(
    ...assertRuneProbeMatchesPinnedProjection(fs.readFileSync(probeAbs, "utf8"), {
      preludeSource: fs.existsSync(preludeAbs) ? fs.readFileSync(preludeAbs, "utf8") : "",
      svelteTypesSource: fs.existsSync(typesAbs) ? fs.readFileSync(typesAbs, "utf8") : "",
    }),
  );
  const negativeAbs = path.join(STS0_DIR, "probes", "negative.ts");
  const negativeSource = fs.readFileSync(negativeAbs, "utf8");
  if (
    !negativeSource.includes(PINNED_DERIVED_DECLARE) ||
    !negativeSource.includes("$derived(() =>")
  ) {
    errors.push(
      err(
        "STS0-svelte-abi",
        "incorrect-rune-signature",
        "negative probe must keep the pinned $derived expression signature and the callback dirty twin",
      ),
    );
  }
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
