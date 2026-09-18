/**
 * DX0 constitution validator.
 *
 * Proves the four mandatory cases against machine products, the ratified
 * contract markdown, the train plan, and the live repository authority
 * (DAG, conflict-domain catalog, product-surface catalog, lint rule count,
 * evidence files). Does not execute any compiler or editor; DX1+ own runtime
 * harnesses.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { loadAuthority, readToml } from "../../../roadmap/0.1.0-tama/tools/lib.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const PRODUCTS = path.join(HERE, "products");
const ROADMAP = path.join(REPO_ROOT, "roadmap/0.1.0-tama");

const MANDATORY_CASES = ["DX0-AC1", "DX0-AC2", "DX0-AC3", "DX0-ratification"];

const CONTRACT_MD = "product-experience.md";
const PLAN_MD = "expansion-product-observability.md";

const RATIFICATION_MARKERS = [
  "FeatureExposureContract v1",
  "HostExecutionClass",
  "ProductReceiptBasis",
  "## 13. Receiving amendments",
];

const HOST_CLASSES = ["Portable", "NativeOnly", "ExternalOwner"];
const NATIVE_PRODUCER_PATTERN = /flow[_-]return|native[_-]checker/i;

/** Pinned independently of the operation rows: the AC1 gap population. */
const REQUIRED_PLAYGROUND_GAPS = ["tsc.project-check.vue", "tsc.project-check.svelte"];

/** Pinned AC3 population: one labelled inspection flow row plus semantic TS rows. */
const REQUIRED_SEMANTIC_ROWS = ["lsp.hover", "lsp.diagnostics"];
const REQUIRED_INSPECTION_FLOW_ROW = "inspection.flow-return";

/** Pinned DX0.1 populations: TypeInfo queries and mappings must have rows, not just prose. */
const REQUIRED_TYPEINFO_ROWS = [
  "typeinfo.graph-operations",
  "mcp.type-queries",
  "lsp.binding-types",
];
const REQUIRED_MAPPING_ROWS = ["vscode.source-map", "playground.source-map-view"];
const REQUIRED_TYPEINFO_EVIDENCE = [
  "crates/verter_protocol/proto/verter/v1/typeinfo.proto",
  "packages/typeinfo/package.json",
  "crates/verter_mcp/src/server.rs",
];
const REQUIRED_MAPPING_EVIDENCE = [
  "packages/vue-vscode/package.json",
  "packages/playground/src/core/sourcemap.ts",
];

/** STP0 lineage: docs-only DAG kinds with a zero production budget are not production-capable receivers. */
const DOCS_ONLY_KINDS = new Set(["lock", "contract", "constitution", "history"]);

const REQUIRED_RECEIVERS = [
  "DX1",
  "DX1G",
  "DX2",
  "DX3",
  "DX3T",
  "DX4",
  "DX5",
  "DX6",
  "DX6R",
  "DX7",
  "DX8",
  "BWH0",
  "COX0",
  "ED0",
  "EPR0",
  "JBT0",
  "LSO0",
  "LSPX0",
  "PG0",
  "PM0",
  "PUB0",
  "RFX0",
  "SVI0",
  "TIF1",
  "VSC0",
];

function err(caseId, code, message) {
  return { caseId, code, message };
}

function readJson(name) {
  return JSON.parse(fs.readFileSync(path.join(PRODUCTS, name), "utf8"));
}

export function loadProducts() {
  return {
    exposure: readJson("feature-exposure-contract.v1.json"),
    hostClass: readJson("host-execution-class.v1.json"),
    receipt: readJson("product-receipt-basis.v1.json"),
    inventory: readJson("cross-surface-inventory.v1.json"),
    rails: readJson("rails-policy.v1.json"),
    ownership: readJson("exposure-ownership-map.json"),
  };
}

export function loadContracts() {
  return {
    [CONTRACT_MD]: fs.readFileSync(path.join(ROADMAP, "contracts", CONTRACT_MD), "utf8"),
    [PLAN_MD]: fs.readFileSync(path.join(ROADMAP, "plans", PLAN_MD), "utf8"),
  };
}

export function cloneProducts(products) {
  return structuredClone(products);
}

/** Live repository facts the constitution is grounded in. */
export function loadRepositoryFacts() {
  const catalog = readToml(path.join(ROADMAP, "catalogs/product-surface-catalog.toml"));
  const surfaceIds = new Set((catalog.surface || []).map((row) => row.id));
  const conflictDomains = readToml(path.join(ROADMAP, "catalogs/conflict-domains.toml"));
  const domainIds = new Set((conflictDomains.domain || []).map((row) => row.id));
  const rulesSource = fs.readFileSync(
    path.join(REPO_ROOT, "crates/verter_diagnostics/src/rules/mod.rs"),
    "utf8",
  );
  const lintRuleCount = (rulesSource.match(/registry\.register/g) || []).length;
  // DX0-AC3 grounding: does the live playground hover provider still concatenate
  // compiler-analysis hover into the TypeScript hover contents array? Both
  // pushes live in provideHover in lspProviders.ts; the product must catalogue
  // the merge as long as the live route has it, and drop the gap once it is
  // split or labelled.
  const hoverSource = fs.readFileSync(
    path.join(REPO_ROOT, "packages/playground/src/editor/lspProviders.ts"),
    "utf8",
  );
  const playgroundHoverMergesAnalysis =
    hoverSource.includes("tsBridge.getHover") && hoverSource.includes("hoverForWord");
  return { surfaceIds, domainIds, lintRuleCount, playgroundHoverMergesAnalysis };
}

function missingEvidence(caseId, paths) {
  const errors = [];
  for (const evidence of paths) {
    if (typeof evidence !== "string" || evidence.length === 0) {
      errors.push(
        err(caseId, "missing-evidence-file", `evidence entry is not a path: ${evidence}`),
      );
    } else if (!fs.existsSync(path.join(REPO_ROOT, evidence))) {
      errors.push(
        err(caseId, "missing-evidence-file", `evidence file does not exist: ${evidence}`),
      );
    }
  }
  return errors;
}

export function validate(
  products = loadProducts(),
  contracts = loadContracts(),
  authority = loadAuthority(),
  facts = loadRepositoryFacts(),
) {
  const errors = [];
  errors.push(...validateExposure(products.exposure, products.hostClass, facts));
  errors.push(...validateHostClass(products.hostClass));
  errors.push(...validateReceipt(products.receipt));
  errors.push(...validateInventory(products.inventory, products.exposure, facts));
  errors.push(...validateRails(products.rails, products.exposure));
  errors.push(...validateOwnership(products.ownership, authority));
  errors.push(...validateRatification(products, contracts, authority, facts));
  return { ok: errors.length === 0, errors };
}

function validateExposure(exposure, hostClass, facts) {
  const errors = [];
  if (exposure?.id !== "feature-exposure-contract.v1") {
    errors.push(
      err("DX0-ratification", "missing-product", "FeatureExposureContract v1 id is absent"),
    );
    return errors;
  }
  const hostEnum = new Set(hostClass?.enum || HOST_CLASSES);
  const vocabulary = exposure.vocabulary || {};
  for (const [key, allowed] of [
    ["profiles", vocabulary.profiles],
    ["maturity", vocabulary.maturity],
    ["exposureStates", vocabulary.exposureStates],
    ["answerRails", vocabulary.answerRails],
    ["playgroundStatus", vocabulary.playgroundStatus],
  ]) {
    if (!Array.isArray(allowed) || allowed.length === 0) {
      errors.push(err("DX0-ratification", "missing-vocabulary", `vocabulary ${key} is missing`));
    }
  }
  const rows = exposure.operations || [];
  if (rows.length === 0) {
    errors.push(err("DX0-ratification", "missing-product", "exposure contract has no operations"));
  }
  const ids = new Set();
  for (const row of rows) {
    if (!row?.id) {
      errors.push(err("DX0-ratification", "missing-id", "operation row missing id"));
      continue;
    }
    if (ids.has(row.id)) {
      errors.push(err("DX0-ratification", "duplicate-id", `duplicate operation row ${row.id}`));
    }
    ids.add(row.id);
    if (row.obligation !== "RequiredCurrent") {
      errors.push(
        err(
          "DX0-ratification",
          "not-required-current",
          `operation ${row.id} is ${row.obligation || "missing"}`,
        ),
      );
    }
    if (!hostEnum.has(row.hostExecutionClass)) {
      errors.push(
        err(
          "DX0-AC2",
          "unknown-host-class",
          `operation ${row.id} has host class ${row.hostExecutionClass}`,
        ),
      );
    }
    if (Array.isArray(vocabulary.profiles) && !vocabulary.profiles.includes(row.profile)) {
      errors.push(
        err(
          "DX0-ratification",
          "unknown-profile",
          `operation ${row.id} has profile ${row.profile}`,
        ),
      );
    }
    if (Array.isArray(vocabulary.maturity) && !vocabulary.maturity.includes(row.maturity)) {
      errors.push(
        err(
          "DX0-ratification",
          "unknown-maturity",
          `operation ${row.id} has maturity ${row.maturity}`,
        ),
      );
    }
    if (!["semantic", "inspection", "comparison", "none"].includes(row.answerRail)) {
      errors.push(err("DX0-AC3", "unknown-rail", `operation ${row.id} has rail ${row.answerRail}`));
    }
    if (row.answerRail === "semantic" && row.semanticAuthority !== "typescript") {
      errors.push(
        err(
          "DX0-AC3",
          "rail-substitution",
          `semantic operation ${row.id} has authority ${row.semanticAuthority || "unspecified"}`,
        ),
      );
    }
    if (NATIVE_PRODUCER_PATTERN.test(row.producerOwner || "")) {
      if (row.answerRail !== "inspection") {
        errors.push(
          err(
            "DX0-AC3",
            "rail-substitution",
            `native-analysis producer on ${row.id} must stay on the inspection rail, not ${row.answerRail}`,
          ),
        );
      }
      if (
        row.labelled !== true ||
        typeof row.provenanceField !== "string" ||
        !row.provenanceField
      ) {
        errors.push(
          err(
            "DX0-AC3",
            "unlabelled-inspection",
            `inspection operation ${row.id} lacks labelling/provenance`,
          ),
        );
      }
    }
    for (const surfaceId of row.catalogSurfaces || []) {
      if (!facts.surfaceIds.has(surfaceId)) {
        errors.push(
          err(
            "DX0-ratification",
            "unknown-catalog-surface",
            `operation ${row.id} cites unknown catalog surface ${surfaceId}`,
          ),
        );
      }
    }
    // DX0-AC1: playground gaps block promotion until an executable route and replay case exist.
    const playground = row.playground || {};
    if (playground.status === "missing") {
      if (playground.promotionBlocked !== true) {
        errors.push(
          err(
            "DX0-AC1",
            "unblocked-playground-gap",
            `missing playground feature ${row.id} is not promotion-blocked`,
          ),
        );
      }
      const blocked = playground.blockedUntil || {};
      if (!blocked.route || !blocked.replayCase) {
        errors.push(
          err(
            "DX0-AC1",
            "unblocked-playground-gap",
            `missing playground feature ${row.id} lacks route/replayCase owners`,
          ),
        );
      }
    } else if (playground.promotionBlocked === true) {
      // A partial exposure may carry an open gap, but only with a named owner.
      if (playground.status !== "partial") {
        errors.push(
          err(
            "DX0-AC1",
            "unblocked-playground-gap",
            `exposed feature ${row.id} must not claim promotion-blocked`,
          ),
        );
      }
      const blocked = playground.blockedUntil || {};
      if (!blocked.route || !blocked.replayCase) {
        errors.push(
          err(
            "DX0-AC1",
            "unblocked-playground-gap",
            `partial blocked feature ${row.id} lacks route/replayCase owners`,
          ),
        );
      }
    }
    if (
      (playground.status === "exposed" || playground.status === "partial") &&
      !(playground.route && playground.test)
    ) {
      errors.push(
        err(
          "DX0-AC1",
          "playground-gap-claimed-exposed",
          `playground feature ${row.id} is ${playground.status} without an executable browser route and test`,
        ),
      );
    }
    // DX0-AC2 / contracts §3: a NativeOnly or ExternalOwner-native operation
    // may appear in a browser product only as a companion route or labelled
    // gap, so presenting its own browser exposure requires shipped build
    // evidence; the browser analog is a separate Portable/ExternalOwner row.
    if (
      (row.hostExecutionClass === "NativeOnly" || row.hostExecutionClass === "ExternalOwner") &&
      (playground.status === "exposed" || playground.status === "partial") &&
      !row.browserBuildEvidence
    ) {
      errors.push(
        err(
          "DX0-AC2",
          "native-playground-exposure",
          `${row.hostExecutionClass} operation ${row.id} is playground ${playground.status} without companion/build evidence`,
        ),
      );
    }
    // DX0-AC3: while the live playground hover route concatenates TypeScript
    // and compiler-analysis content into one default answer, that implicit
    // comparison must be catalogued as a blocked gap, never claimed as a clean
    // exposure; once the live route is split or labelled, the gap goes stale.
    const merge = playground.unlabelledAnalysisMerge;
    if (merge) {
      if (
        playground.status !== "partial" ||
        playground.promotionBlocked !== true ||
        !Array.isArray(merge.evidence) ||
        merge.evidence.length === 0
      ) {
        errors.push(
          err(
            "DX0-AC3",
            "implicit-comparison-default",
            `implicit-comparison gap on ${row.id} must be partial, promotion-blocked and evidence-backed`,
          ),
        );
      }
      if (!facts.playgroundHoverMergesAnalysis) {
        errors.push(
          err(
            "DX0-AC3",
            "stale-implicit-comparison-gap",
            `${row.id} still catalogues a hover merge the live route no longer has`,
          ),
        );
      }
      if (Array.isArray(merge.evidence)) {
        errors.push(...missingEvidence("DX0-AC3", merge.evidence));
      }
    } else if (
      facts.playgroundHoverMergesAnalysis &&
      row.answerRail === "semantic" &&
      typeof playground.route === "string" &&
      playground.route.includes("lspProviders.ts")
    ) {
      errors.push(
        err(
          "DX0-AC3",
          "implicit-comparison-default",
          `semantic-rail playground route on ${row.id} cites lspProviders.ts, whose provideHover concatenates TypeScript and analysis hover as the default answer`,
        ),
      );
    }
    // DX0-AC2: browser capability claims without a portable class or a shipped build are rejected.
    if (row.browserExecutable === true) {
      if (row.hostExecutionClass !== "Portable" && !row.browserBuildEvidence) {
        errors.push(
          err(
            "DX0-AC2",
            "native-only-browser-claim",
            `operation ${row.id} is browser-executable without Portable class or build evidence`,
          ),
        );
      }
    } else if (row.hostExecutionClass === "Portable") {
      errors.push(
        err(
          "DX0-AC2",
          "portable-not-browser-executable",
          `Portable operation ${row.id} is not browser-executable`,
        ),
      );
    }
    if (Array.isArray(playground.evidence)) {
      errors.push(...missingEvidence("DX0-ratification", playground.evidence));
    }
  }
  for (const id of REQUIRED_PLAYGROUND_GAPS) {
    const row = rows.find((candidate) => candidate.id === id);
    if (!row || row.playground?.status !== "missing") {
      errors.push(
        err("DX0-AC1", "missing-gap-row", `required playground gap row ${id} is missing`),
      );
    }
  }
  const gapRows = rows.filter((row) => row.playground?.status === "missing");
  if (gapRows.length === 0) {
    errors.push(
      err(
        "DX0-AC1",
        "missing-gap-row",
        "no current feature is catalogued as missing from the playground",
      ),
    );
  }
  return errors;
}

function validateHostClass(hostClass) {
  const errors = [];
  if (hostClass?.id !== "host-execution-class.v1") {
    return [err("DX0-ratification", "missing-product", "HostExecutionClass v1 id is absent")];
  }
  if (JSON.stringify(hostClass.enum) !== JSON.stringify(HOST_CLASSES)) {
    errors.push(
      err(
        "DX0-ratification",
        "unknown-host-class",
        `host class enum must be exactly ${HOST_CLASSES.join(", ")}`,
      ),
    );
  }
  for (const name of HOST_CLASSES) {
    const definition = hostClass.classes?.[name];
    if (!definition?.definition) {
      errors.push(
        err(
          "DX0-ratification",
          "missing-class-definition",
          `host class ${name} lacks a definition`,
        ),
      );
    }
    if (definition?.evidence)
      errors.push(...missingEvidence("DX0-ratification", definition.evidence));
  }
  const surfaceIds = new Set();
  for (const surface of hostClass.surfaces || []) {
    if (!surface?.id) {
      errors.push(err("DX0-ratification", "missing-id", "host class surface row missing id"));
      continue;
    }
    if (surfaceIds.has(surface.id)) {
      errors.push(
        err("DX0-ratification", "duplicate-id", `duplicate host class surface ${surface.id}`),
      );
    }
    surfaceIds.add(surface.id);
    if (!HOST_CLASSES.includes(surface.class)) {
      errors.push(
        err(
          "DX0-AC2",
          "unknown-host-class",
          `surface ${surface.id} has host class ${surface.class}`,
        ),
      );
    }
    if (surface.evidence) errors.push(...missingEvidence("DX0-ratification", surface.evidence));
  }
  return errors;
}

function validateReceipt(receipt) {
  const errors = [];
  if (receipt?.id !== "product-receipt-basis.v1") {
    return [err("DX0-ratification", "missing-product", "ProductReceiptBasis v1 id is absent")];
  }
  const expectedFields = [
    "sourceRevisions",
    "projectConfiguration",
    "engineIdentity",
    "hostIdentity",
    "completenessState",
  ];
  if (JSON.stringify(receipt.fields) !== JSON.stringify(expectedFields)) {
    errors.push(
      err(
        "DX0-ratification",
        "receipt-fields",
        `receipt fields must be exactly ${expectedFields.join(", ")}`,
      ),
    );
  }
  const states = receipt.completenessStates || [];
  for (const state of [
    "complete-empty",
    "partial",
    "pending",
    "unsupported",
    "ambiguous",
    "failed",
    "cancelled",
    "stale",
  ]) {
    if (!states.includes(state)) {
      errors.push(
        err("DX0-ratification", "completeness-states", `completeness state ${state} is missing`),
      );
    }
  }
  if (new Set(states).size !== states.length) {
    errors.push(
      err("DX0-ratification", "completeness-states", "completeness states contain duplicates"),
    );
  }
  for (const substrate of receipt.currentSubstrate || []) {
    if (substrate.evidence) errors.push(...missingEvidence("DX0-ratification", substrate.evidence));
  }
  return errors;
}

function validateInventory(inventory, exposure, facts) {
  const errors = [];
  if (inventory?.id !== "cross-surface-inventory.v1") {
    return [err("DX0-ratification", "missing-product", "cross-surface inventory id is absent")];
  }
  const preserved = inventory.preservedClients || {};
  for (const client of ["vscode", "nvim", "helix", "lapce", "zed"]) {
    if (!(preserved.ids || []).includes(client)) {
      errors.push(
        err(
          "DX0-ratification",
          "missing-preserved-client",
          `preserved client ${client} is missing from the inventory`,
        ),
      );
    }
    const row = (inventory.clients || []).find((candidate) => candidate.id === client);
    if (!row) {
      errors.push(
        err("DX0-ratification", "missing-preserved-client", `client row ${client} is missing`),
      );
    } else if (row.evidence) {
      errors.push(...missingEvidence("DX0-ratification", row.evidence));
    }
  }
  const shared = inventory.sharedLaunchContract || {};
  if (
    JSON.stringify(shared.initOptionKeys) !==
    JSON.stringify(["lint", "inlayHints", "viteConfig", "experimental", "hover", "statistics"])
  ) {
    errors.push(
      err(
        "DX0-ratification",
        "launch-contract-keys",
        "shared launch contract keys must be the six server-read keys",
      ),
    );
  }
  if (shared.evidence) errors.push(...missingEvidence("DX0-ratification", shared.evidence));
  const providerModes = inventory.analysis?.providerModes;
  const expectedModes = [
    "auto",
    "tsgo",
    "shared-tsgo",
    "tsserver",
    "editor-tsserver",
    "extension",
    "off",
  ];
  if (JSON.stringify(providerModes) !== JSON.stringify(expectedModes)) {
    errors.push(
      err(
        "DX0-ratification",
        "provider-modes",
        `provider modes must be exactly ${expectedModes.join(", ")}`,
      ),
    );
  }
  if (inventory.analysis?.lint?.ruleCount !== facts.lintRuleCount) {
    errors.push(
      err(
        "DX0-ratification",
        "lint-count-drift",
        `inventory lint rule count ${inventory.analysis?.lint?.ruleCount} != live ${facts.lintRuleCount}`,
      ),
    );
  }
  if (inventory.analysis?.lint?.evidence) {
    errors.push(...missingEvidence("DX0-ratification", inventory.analysis.lint.evidence));
  }
  // DX0.1: the capturedAt note claims TypeInfo queries and mappings are
  // inventoried, so the categories, their evidence, and exposure rows must
  // exist; the claim may not stay prose-only.
  const typeInfo = inventory.analysis?.typeInfoQueries;
  if (!typeInfo) {
    errors.push(
      err(
        "DX0-ratification",
        "missing-typeinfo-inventory",
        "DX0.1 TypeInfo query inventory is absent",
      ),
    );
  } else {
    const graphOps = typeInfo.protocolGraphOperations || [];
    if (graphOps.length !== 8) {
      errors.push(
        err(
          "DX0-ratification",
          "missing-typeinfo-inventory",
          `TypeInfo graph operations must list the 8 GRAPH_OPERATION_* values, got ${graphOps.length}`,
        ),
      );
    }
    const mcpTools = typeInfo.mcpTools || [];
    for (const tool of ["get_framework_surface", "get_component_types", "get_type_errors"]) {
      if (!mcpTools.includes(tool)) {
        errors.push(
          err(
            "DX0-ratification",
            "missing-typeinfo-inventory",
            `TypeInfo MCP tool ${tool} is missing`,
          ),
        );
      }
    }
    if (!(typeInfo.lspCustomMethods || []).includes("$/verter/getBindingTypes")) {
      errors.push(
        err(
          "DX0-ratification",
          "missing-typeinfo-inventory",
          "$/verter/getBindingTypes is not inventoried as a TypeInfo custom method",
        ),
      );
    }
    for (const evidence of REQUIRED_TYPEINFO_EVIDENCE) {
      if (!(typeInfo.evidence || []).includes(evidence)) {
        errors.push(
          err(
            "DX0-ratification",
            "missing-typeinfo-inventory",
            `TypeInfo inventory does not cite ${evidence}`,
          ),
        );
      }
    }
    errors.push(...missingEvidence("DX0-ratification", typeInfo.evidence || []));
  }
  const mappings = inventory.analysis?.mappings;
  if (!mappings) {
    errors.push(
      err(
        "DX0-ratification",
        "missing-mapping-inventory",
        "DX0.1 mapping/source-map inventory is absent",
      ),
    );
  } else {
    for (const command of ["verter.showSourceMapForFile", "verter.showSourceMapVisualization"]) {
      if (!(mappings.vscodeCommands || []).includes(command)) {
        errors.push(
          err(
            "DX0-ratification",
            "missing-mapping-inventory",
            `mapping command ${command} is not inventoried`,
          ),
        );
      }
    }
    for (const evidence of REQUIRED_MAPPING_EVIDENCE) {
      if (!(mappings.evidence || []).includes(evidence)) {
        errors.push(
          err(
            "DX0-ratification",
            "missing-mapping-inventory",
            `mapping inventory does not cite ${evidence}`,
          ),
        );
      }
    }
    errors.push(...missingEvidence("DX0-ratification", mappings.evidence || []));
  }
  const rowIds = new Set((exposure?.operations || []).map((row) => row.id));
  for (const id of REQUIRED_TYPEINFO_ROWS) {
    if (!rowIds.has(id)) {
      errors.push(
        err(
          "DX0-ratification",
          "missing-typeinfo-row",
          `required TypeInfo exposure row ${id} is missing`,
        ),
      );
    }
  }
  for (const id of REQUIRED_MAPPING_ROWS) {
    if (!rowIds.has(id)) {
      errors.push(
        err(
          "DX0-ratification",
          "missing-mapping-row",
          `required mapping exposure row ${id} is missing`,
        ),
      );
    }
  }
  const gaps = inventory.playgroundGaps || [];
  if (!gaps.some((gap) => gap.status === "missing")) {
    errors.push(
      err("DX0-AC1", "missing-gap-row", "inventory catalogues no missing playground feature"),
    );
  }
  for (const gap of gaps) {
    for (const surfaceId of gap.catalogSurfaces || []) {
      if (!facts.surfaceIds.has(surfaceId)) {
        errors.push(
          err(
            "DX0-ratification",
            "unknown-catalog-surface",
            `playground gap cites unknown catalog surface ${surfaceId}`,
          ),
        );
      }
    }
    if (gap.evidence) errors.push(...missingEvidence("DX0-ratification", gap.evidence));
  }
  for (const terminal of inventory.terminals || []) {
    if (terminal.evidence) errors.push(...missingEvidence("DX0-ratification", terminal.evidence));
  }
  return errors;
}

function validateRails(rails, exposure) {
  const errors = [];
  if (rails?.id !== "rails-policy.v1") {
    return [err("DX0-ratification", "missing-product", "rails policy id is absent")];
  }
  if (rails.semanticRail?.authority !== "typescript") {
    errors.push(err("DX0-AC3", "rail-substitution", "semantic rail authority is not TypeScript"));
  }
  if (rails.inspectionRail?.labelling !== "required") {
    errors.push(
      err("DX0-AC3", "unlabelled-inspection", "inspection rail labelling is not required"),
    );
  }
  if (rails.comparisonRail?.selection !== "explicit") {
    errors.push(err("DX0-AC3", "implicit-comparison", "comparison rail selection is not explicit"));
  }
  if (
    !Array.isArray(rails.discriminators) ||
    !rails.discriminators.some((row) => row.id === "DX0-AC3")
  ) {
    errors.push(
      err("DX0-AC3", "missing-discriminator", "the DX0-AC3 same-file discriminator is missing"),
    );
  }
  // Cross-product consistency: the pinned AC3 population exists on both rails.
  const rows = exposure?.operations || [];
  for (const id of REQUIRED_SEMANTIC_ROWS) {
    const row = rows.find((candidate) => candidate.id === id);
    if (!row || row.answerRail !== "semantic" || row.semanticAuthority !== "typescript") {
      errors.push(
        err(
          "DX0-AC3",
          "rail-substitution",
          `semantic row ${id} is missing or not TypeScript-authoritative`,
        ),
      );
    }
  }
  const flowRow = rows.find((candidate) => candidate.id === REQUIRED_INSPECTION_FLOW_ROW);
  if (!flowRow || flowRow.answerRail !== "inspection" || flowRow.labelled !== true) {
    errors.push(
      err(
        "DX0-AC3",
        "unlabelled-inspection",
        "labelled inspection flow row is missing from the exposure contract",
      ),
    );
  }
  // Rail closure: the rails-policy operation lists are closed against the
  // exposure contract's answerRail values in both directions, so relabelling
  // a listed operation (or adding an unlisted rail member) is a failure.
  const byRail = (rail) =>
    new Set(rows.filter((row) => row.answerRail === rail).map((row) => row.id));
  for (const [railName, listed] of [
    ["semantic", rails.semanticRail?.operations],
    ["inspection", rails.inspectionRail?.operations],
  ]) {
    if (!Array.isArray(listed)) {
      errors.push(
        err("DX0-AC3", "rail-population-drift", `${railName} rail operations list is missing`),
      );
      continue;
    }
    const live = byRail(railName);
    for (const id of listed) {
      if (!live.has(id)) {
        errors.push(
          err(
            "DX0-AC3",
            "rail-population-drift",
            `${railName} rail lists ${id}, but its exposure row is not on the ${railName} rail`,
          ),
        );
      }
    }
    for (const id of live) {
      if (!listed.includes(id)) {
        errors.push(
          err(
            "DX0-AC3",
            "rail-population-drift",
            `${railName}-rail operation ${id} is missing from the ${railName} operations list`,
          ),
        );
      }
    }
  }
  return errors;
}

function validateOwnership(ownership, authority) {
  const errors = [];
  const dag = new Set((authority.nodes || []).map((node) => node.id));
  const planned = new Set((ownership?.receivingAmendments || []).map((row) => row.receiver));
  planned.add(ownership?.contractNode);

  if (ownership?.contractNode !== "DX0") {
    errors.push(err("DX0-ratification", "missing-owner", "ownership map is not owned by DX0"));
  }
  if (
    !ownership?.finalOwner ||
    !ownership.finalOwner.startsWith("expansion.product-observability")
  ) {
    errors.push(
      err(
        "DX0-ratification",
        "missing-owner",
        "final owner is not expansion.product-observability",
      ),
    );
  }
  for (const required of REQUIRED_RECEIVERS) {
    if (!planned.has(required)) {
      errors.push(
        err(
          "DX0-ratification",
          "missing-amendment",
          `receiving amendment for ${required} is missing`,
        ),
      );
    }
  }
  for (const row of ownership?.receivingAmendments || []) {
    if (!row.receiver || typeof row.productionCapable !== "boolean" || !row.product) {
      errors.push(
        err(
          "DX0-ratification",
          "missing-amendment",
          `receiving amendment ${row.receiver || "?"} is malformed`,
        ),
      );
    }
  }
  // DX0-AC-OWNER (STP0 DOCS_ONLY_KINDS lineage): a receiver whose DAG kind is
  // lock/contract/constitution/history with a zero production budget is not a
  // production-capable receiver; the bit is the production-authority claim
  // future nodes read.
  const nodesById = new Map((authority.nodes || []).map((node) => [node.id, node]));
  for (const row of ownership?.receivingAmendments || []) {
    const node = nodesById.get(row.receiver);
    if (!node) continue;
    const docsOnly = DOCS_ONLY_KINDS.has(node.kind) && node.max_production_loc === 0;
    if (docsOnly && row.productionCapable === true) {
      errors.push(
        err(
          "DX0-ratification",
          "docs-only-production-capable",
          `receiver ${row.receiver} is a ${node.kind} node with zero production budget and must not claim productionCapable`,
        ),
      );
    }
  }
  const populations = [...(ownership?.outcomes || [])];
  for (const row of populations) {
    if (!row?.currentOwner || !row?.finalOwner || !row?.receivingAcceptance) {
      errors.push(
        err(
          "DX0-ratification",
          "missing-owner",
          `outcome row ${row?.id || "?"} lacks current/final owner or AC`,
        ),
      );
      continue;
    }
    for (const owner of [row.currentOwner, row.finalOwner]) {
      if (!dag.has(owner) && !planned.has(owner)) {
        errors.push(
          err(
            "DX0-ratification",
            "unknown-owner",
            `owner ${owner} on ${row.id} is neither a DAG node nor a receiving amendment`,
          ),
        );
      }
    }
  }
  if (!Array.isArray(ownership?.displacedRoutes) || ownership.displacedRoutes.length !== 0) {
    errors.push(err("DX0-ratification", "premature-retirement", "DX0 must not displace routes"));
  }
  if (
    !Array.isArray(ownership?.deletionPopulationThisNode) ||
    ownership.deletionPopulationThisNode.length !== 0
  ) {
    errors.push(
      err("DX0-ratification", "premature-retirement", "DX0 deletion population must be empty"),
    );
  }
  if (
    !ownership?.acBasisDownstreamTestOwner ||
    !ownership?.acResourceRationale ||
    !ownership?.acExposureRegistration
  ) {
    errors.push(
      err(
        "DX0-ratification",
        "missing-owner",
        "AC-BASIS/AC-RESOURCE/AC-EXPOSURE obligations are not bound",
      ),
    );
  }
  return errors;
}

function validateRatification(products, contracts, authority, facts) {
  const errors = [];
  const contract = contracts[CONTRACT_MD];
  const plan = contracts[PLAN_MD];
  for (const marker of RATIFICATION_MARKERS) {
    if (!contract || !contract.includes(marker)) {
      errors.push(
        err("DX0-ratification", "missing-contract-marker", `contract missing marker ${marker}`),
      );
    }
  }
  if (!contract || !contract.includes("TypeScript")) {
    errors.push(
      err("DX0-AC3", "missing-contract-marker", "contract does not name TypeScript authority"),
    );
  }
  for (const marker of ["DX0", "DX1", "DX8", "receiving amendments"]) {
    if (!plan || !plan.includes(marker)) {
      errors.push(err("DX0-ratification", "missing-plan", `train plan missing marker ${marker}`));
    }
  }
  const node = (authority.nodes || []).find((candidate) => candidate.id === "DX0");
  if (!node) {
    errors.push(
      err("DX0-ratification", "missing-owner", "DX0 is not registered in the program DAG"),
    );
  } else {
    if (node.gate_profile !== "docs-domain" || node.review_profile !== "architecture-3") {
      errors.push(
        err(
          "DX0-ratification",
          "missing-owner",
          "DX0 gate/review profiles drifted from the charter",
        ),
      );
    }
    if (!(node.conflict_domains || []).includes("product_inspection")) {
      errors.push(
        err(
          "DX0-ratification",
          "missing-owner",
          "DX0 does not hold the product_inspection conflict domain",
        ),
      );
    }
    if (node.max_production_loc !== 0 || node.max_production_files !== 0) {
      errors.push(err("DX0-ratification", "missing-owner", "DX0 production budget must be zero"));
    }
  }
  if (!facts.domainIds.has("product_inspection")) {
    errors.push(
      err(
        "DX0-ratification",
        "missing-owner",
        "product_inspection is not registered in the conflict-domain catalog",
      ),
    );
  }
  const ledgerRow = authority.ledger.implemented.find((row) => row.node_id === "DX0");
  if (!ledgerRow || !ledgerRow.commit_message) {
    errors.push(err("DX0-ratification", "missing-owner", "DX0 has no implemented ledger row"));
  }
  return errors;
}

export function mandatoryCases() {
  return [...MANDATORY_CASES];
}

export function selectedCaseIds(result) {
  return [...new Set((result.errors || []).map((error) => error.caseId))];
}

if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))
) {
  const result = validate();
  if (!result.ok) {
    console.error(
      result.errors.map((error) => `${error.caseId}/${error.code}: ${error.message}`).join("\n"),
    );
    process.exit(1);
  }
  console.log(
    `dx0 verify: PASS cases=${MANDATORY_CASES.length} operations=${loadProducts().exposure.operations.length} receivers=${REQUIRED_RECEIVERS.length} lintRules=${loadRepositoryFacts().lintRuleCount}`,
  );
}
