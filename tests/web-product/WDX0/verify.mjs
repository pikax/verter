/**
 * WDX0 constitution validator.
 *
 * Proves the mandatory cases against machine products, the ratified contract
 * markdown, the train plan, and the live repository authority (DAG,
 * implementation ledger, capability-surface catalog, preserved contracts).
 * Does not execute any compiler or runtime; WDX1 owns executable fixtures
 * and WDX2/WDX3 own qualification and acceptance evidence.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { loadAuthority, readToml } from "../../../roadmap/0.1.0-tama/tools/lib.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const PRODUCTS = path.join(HERE, "products");
const ROADMAP = path.join(REPO_ROOT, "roadmap/0.1.0-tama");

const MANDATORY_CASES = [
  "WDX0-AC1",
  "WDX0-AC2",
  "WDX0-AC3",
  "WDX0-AC4",
  "WDX0-AC5",
  "WDX0-ratification",
];

const CONTRACT_MD = "web-product-expansion-v1.md";
const PLAN_MD = "expansion-web-product-convergence.md";
const EVIDENCE_FILE = "tests/web-product/evidence/WDX0/cases.md";

const RATIFICATION_MARKERS = [
  "ownership, disposition and claim constitution",
  "web-product-disposition-matrix.v1.json",
  "evidence-class-policy.v1.json",
  "web-product-ownership-map.json",
  "## 10. Receiving amendments",
  "Historical completions preserved",
];

const DISPOSITIONS = ["producing-train", "extension", "exclusion"];
const PORTFOLIO_CLASSES = [
  "framework",
  "product",
  "extension",
  "profile",
  "convergence",
  "consumer",
];
const EVIDENCE_CLASSES = ["static-proof", "runtime-observation", "estimate"];

/** Pinned recommendation population: the 41 direct successors of WDX0. */
const REQUIRED_HEADS = [
  "AGT0",
  "ALP0",
  "ANG0",
  "AST0",
  "AX0",
  "CENV0",
  "CSS0",
  "CWB0",
  "DATA0",
  "DBG0",
  "DIAL0",
  "DOC5",
  "DX1",
  "ENV0",
  "GLM0",
  "GQL0",
  "HTX0",
  "I18N0",
  "LIT0",
  "MDX0",
  "MIG0",
  "MRK0",
  "NXT0",
  "OAPI0",
  "PRE0",
  "PWA0",
  "QWK0",
  "RCT0",
  "RTI0",
  "SEC0",
  "SEO0",
  "SLD0",
  "STN0",
  "TST0",
  "TW0",
  "WBC0",
  "WDX1",
  "WPF0",
  "WSI0",
  "XEC0",
  "XSDK0",
];

/** Pinned WDX2 fan-in: WDX1 plus every portfolio train terminal. */
const REQUIRED_FAN_IN = [
  "AGT8",
  "ALP10",
  "ANG10",
  "AST10",
  "AX10",
  "CENV8",
  "CSS12",
  "CWB9",
  "DATA10",
  "DBG11",
  "DIAL10",
  "ENV9",
  "GLM10",
  "GQL6",
  "HTX10",
  "I18N6",
  "LIT10",
  "MDX10",
  "MIG10",
  "MRK10",
  "NXT8",
  "OAPI7",
  "PRE10",
  "PWA6",
  "QWK10",
  "RCT10",
  "RTI11",
  "SEC11",
  "SEO6",
  "SLD10",
  "STN10",
  "TST12",
  "TW9",
  "WBC7",
  "WDX1",
  "WPF11",
  "WSI10",
  "XEC6",
  "XSDK11",
];

const CONSUMER_HEADS = ["DOC5", "DX1"];
const CONSUMER_TRAINS = new Set(["expansion.product-observability", "expansion.documentation"]);
const CONVERGENCE_TRAIN = "expansion.web-product-convergence";

const HISTORICAL_FAMILIES = ["svelte", "vue"];
const HISTORICAL_SURFACE_COUNT = 24;

function err(caseId, code, message) {
  return { caseId, code, message };
}

function readJson(name) {
  return JSON.parse(fs.readFileSync(path.join(PRODUCTS, name), "utf8"));
}

export function loadProducts() {
  return {
    matrix: readJson("web-product-disposition-matrix.v1.json"),
    policy: readJson("evidence-class-policy.v1.json"),
    ownership: readJson("web-product-ownership-map.json"),
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
  const surfaceIds = (catalog.surface || []).map((row) => row.id);
  const families = [...new Set(surfaceIds.map((id) => id.split(".")[0]))].sort();
  return {
    surfaceIds,
    surfaceFamilies: families,
    surfaceCount: surfaceIds.length,
    evidenceFileExists: fs.existsSync(path.join(REPO_ROOT, EVIDENCE_FILE)),
    preservedContracts: [
      "contracts/product-experience.md",
      "contracts/sfc-typescript-projection.md",
      "charters/expansion-product-observability/DX0.md",
    ].map((relative) => ({ relative, exists: fs.existsSync(path.join(ROADMAP, relative)) })),
  };
}

function trainPrefixMatches(item) {
  const train = item.train;
  switch (item.portfolioClass) {
    case "framework":
      return train.startsWith("framework.");
    case "extension":
      return train.startsWith("extensions.");
    case "profile":
      return train.startsWith("profiles.");
    case "convergence":
      return train === CONVERGENCE_TRAIN;
    case "consumer":
      return CONSUMER_TRAINS.has(train);
    case "product":
      return (
        train.startsWith("expansion.") && !CONSUMER_TRAINS.has(train) && train !== CONVERGENCE_TRAIN
      );
    default:
      return false;
  }
}

function validateMatrix(matrix) {
  const errors = [];
  if (matrix?.id !== "web-product-disposition-matrix.v1") {
    errors.push(err("WDX0-AC1", "missing-product", "disposition matrix id is absent"));
    return errors;
  }
  if (JSON.stringify(matrix.dispositions) !== JSON.stringify(DISPOSITIONS)) {
    errors.push(
      err(
        "WDX0-AC1",
        "unknown-disposition",
        "disposition vocabulary is not the closed three-entry set",
      ),
    );
  }
  const items = matrix.items || [];
  const ids = items.map((item) => item.id);
  if (ids.length !== REQUIRED_HEADS.length || new Set(ids).size !== ids.length) {
    errors.push(
      err(
        "WDX0-AC1",
        "missing-recommendation-row",
        `matrix holds ${ids.length} rows with ${new Set(ids).size} unique ids; the ratified population is ${REQUIRED_HEADS.length}`,
      ),
    );
  }
  for (const required of REQUIRED_HEADS) {
    if (!ids.includes(required)) {
      errors.push(
        err(
          "WDX0-AC1",
          "missing-recommendation-row",
          `recommendation ${required} is missing from the matrix`,
        ),
      );
    }
  }
  const fanIn = matrix.qualificationFanIn || [];
  const fanInSet = new Set(fanIn);
  if (fanIn.length !== REQUIRED_FAN_IN.length || REQUIRED_FAN_IN.some((id) => !fanInSet.has(id))) {
    errors.push(
      err(
        "WDX0-AC1",
        "qualification-fan-in-drift",
        "qualification fan-in is not the pinned 39-edge set",
      ),
    );
  }
  const trains = new Map();
  for (const item of items) {
    if (!DISPOSITIONS.includes(item.disposition)) {
      errors.push(
        err(
          "WDX0-AC1",
          "unknown-disposition",
          `${item.id} carries disposition ${item.disposition}`,
        ),
      );
    }
    if (!PORTFOLIO_CLASSES.includes(item.portfolioClass)) {
      errors.push(
        err(
          "WDX0-AC1",
          "unknown-portfolio-class",
          `${item.id} carries class ${item.portfolioClass}`,
        ),
      );
    }
    // WDX0-AC2 discriminator: exactly one final owner per recommendation.
    if (item.coOwner !== undefined) {
      errors.push(
        err("WDX0-AC2", "two-final-owners", `${item.id} declares a co-owner ${item.coOwner}`),
      );
    }
    if (item.finalOwner !== item.train || !item.finalOwner) {
      errors.push(
        err(
          "WDX0-AC1",
          "owner-train-mismatch",
          `${item.id} final owner ${item.finalOwner} is not its train ${item.train}`,
        ),
      );
    }
    if (trains.has(item.train) && trains.get(item.train) !== item.id) {
      errors.push(
        err(
          "WDX0-AC2",
          "train-owns-two-recommendations",
          `train ${item.train} owns both ${trains.get(item.train)} and ${item.id}`,
        ),
      );
    } else {
      trains.set(item.train, item.id);
    }
    if (!trainPrefixMatches(item)) {
      errors.push(
        err(
          "WDX0-AC1",
          "portfolio-class-drift",
          `${item.id} class ${item.portfolioClass} does not match train ${item.train}`,
        ),
      );
    }
    if ((item.portfolioClass === "extension") !== (item.disposition === "extension")) {
      errors.push(
        err(
          "WDX0-AC1",
          "unknown-disposition",
          `${item.id} disposition ${item.disposition} contradicts class ${item.portfolioClass}`,
        ),
      );
    }
    if (item.disposition === "exclusion" && typeof item.exclusionReason !== "string") {
      errors.push(
        err(
          "WDX0-AC2",
          "exclusion-without-reason",
          `${item.id} is excluded without a recorded reason`,
        ),
      );
    }
    const isConsumer = CONSUMER_HEADS.includes(item.id);
    if (isConsumer) {
      if (item.qualificationEdge !== null) {
        errors.push(
          err(
            "WDX0-AC1",
            "head-terminal-mismatch",
            `consumer ${item.id} must not hold a qualification edge`,
          ),
        );
      }
    } else if (item.id === "WDX1") {
      if (item.qualificationEdge !== "WDX1") {
        errors.push(err("WDX0-AC1", "qualification-fan-in-drift", "WDX1 must qualify directly"));
      }
    } else {
      if (typeof item.terminalNode !== "string" || item.terminalNode === item.id) {
        errors.push(
          err("WDX0-AC1", "head-terminal-mismatch", `${item.id} has no distinct terminal node`),
        );
      }
      if (item.qualificationEdge !== item.terminalNode || !fanInSet.has(item.qualificationEdge)) {
        errors.push(
          err(
            "WDX0-AC1",
            "head-terminal-mismatch",
            `${item.id} qualification edge ${item.qualificationEdge} is not its terminal in the fan-in`,
          ),
        );
      }
    }
    // Claim law: at ratification nothing is claimed.
    if (item.claimBasisAtRatification !== "none") {
      if (item.claimBasisAtRatification === "static-proof") {
        errors.push(
          err(
            "WDX0-AC2",
            "proof-claimed-as-support",
            `${item.id} claims static proof as its ratification basis`,
          ),
        );
      } else if (item.claimBasisAtRatification === "runtime-observation") {
        errors.push(
          err(
            "WDX0-AC2",
            "runtime-claim-at-ratification",
            `${item.id} claims runtime observation before its terminal executed`,
          ),
        );
      } else {
        errors.push(
          err(
            "WDX0-AC2",
            "claim-basis-drift",
            `${item.id} carries claim basis ${item.claimBasisAtRatification}`,
          ),
        );
      }
    }
    if (item.statusAtRatification !== "planned") {
      errors.push(
        err(
          "WDX0-AC2",
          "runtime-claim-at-ratification",
          `${item.id} is not recorded as planned at ratification`,
        ),
      );
    }
  }
  if (matrix.claimBasisAtRatification !== "none" || matrix.statusAtRatification !== "planned") {
    errors.push(
      err(
        "WDX0-AC2",
        "claim-basis-drift",
        "the portfolio as a whole claims a basis beyond none/planned",
      ),
    );
  }
  return errors;
}

function validateClaims(policy, facts) {
  const errors = [];
  if (policy?.id !== "evidence-class-policy.v1") {
    errors.push(err("WDX0-AC2", "missing-product", "evidence-class policy id is absent"));
    return errors;
  }
  const classes = policy.classes || [];
  const classIds = classes.map((row) => row.id);
  if (JSON.stringify(classIds) !== JSON.stringify(EVIDENCE_CLASSES)) {
    errors.push(
      err(
        "WDX0-AC2",
        "evidence-class-drift",
        "the three evidence classes are not exactly static-proof, runtime-observation, estimate",
      ),
    );
  }
  for (const row of classes) {
    if (!row.definition || !Array.isArray(row.mayClaim) || row.mayClaim.length === 0) {
      errors.push(
        err("WDX0-AC2", "evidence-class-drift", `class ${row.id} lacks a definition or claims`),
      );
    }
    if (row.id === "estimate") {
      const joined = (row.mayNotClaim || []).join(" ");
      if (!joined.includes("completion") || !joined.includes("speedup")) {
        errors.push(
          err(
            "WDX0-AC2",
            "estimate-may-claim-completion",
            "the estimate class may leak completion or speedup claims",
          ),
        );
      }
    }
    if (row.id === "runtime-observation" && (!row.requires || row.requires.length === 0)) {
      errors.push(
        err(
          "WDX0-AC2",
          "evidence-class-drift",
          "runtime observation does not require its receipt basis",
        ),
      );
    }
  }
  if (!Array.isArray(policy.claimLaws) || policy.claimLaws.length < 4) {
    errors.push(err("WDX0-AC2", "claim-law-missing", "the four claim laws are not pinned"));
  } else {
    const joined = policy.claimLaws.join(" ");
    for (const marker of [
      "runtime observation",
      "estimate",
      "upstream feature",
      "truthful states",
    ]) {
      if (!joined.includes(marker)) {
        errors.push(err("WDX0-AC2", "claim-law-missing", `claim laws lose the ${marker} law`));
      }
    }
  }
  if (policy.ratificationEvidenceClass !== "static-proof") {
    errors.push(
      err(
        "WDX0-AC2",
        "evidence-class-drift",
        "this ratification must record itself as static proof",
      ),
    );
  }
  if (policy.portfolioClaimBasisAtRatification !== "none") {
    errors.push(
      err(
        "WDX0-AC2",
        "claim-basis-drift",
        "the portfolio claims a basis beyond none at ratification",
      ),
    );
  }
  errors.push(...validateHistorical(policy.historicalCompletions, facts));
  return errors;
}

function validateHistorical(historical, facts) {
  const errors = [];
  if (!historical) {
    errors.push(
      err(
        "WDX0-ratification",
        "historical-completions-missing",
        "historical completions are not recorded",
      ),
    );
    return errors;
  }
  if (!historical.authority || !historical.authority.includes("implemented.toml")) {
    errors.push(
      err(
        "WDX0-ratification",
        "historical-completions-missing",
        "the implementation ledger is not named as the completion authority",
      ),
    );
  }
  for (const anchor of historical.anchors || []) {
    if (!anchor.id || !anchor.evidenceClass || !EVIDENCE_CLASSES.includes(anchor.evidenceClass)) {
      errors.push(
        err(
          "WDX0-ratification",
          "historical-completions-missing",
          `anchor ${anchor.id || "?"} lacks an evidence class`,
        ),
      );
    }
  }
  const catalog = historical.advertisedCatalog;
  if (!catalog || catalog.surfaceCount !== facts.surfaceCount) {
    errors.push(
      err(
        "WDX0-AC2",
        "catalog-count-drift",
        `pinned catalog surface count ${catalog?.surfaceCount} != live ${facts.surfaceCount}`,
      ),
    );
  }
  if (
    catalog &&
    JSON.stringify([...catalog.families].sort()) !== JSON.stringify(facts.surfaceFamilies)
  ) {
    errors.push(
      err(
        "WDX0-AC2",
        "catalog-family-drift",
        `pinned catalog families ${catalog.families} != live ${facts.surfaceFamilies}; an added-portfolio family is advertised without its terminal`,
      ),
    );
  }
  if (catalog?.addedPortfolioSurfaces !== 0) {
    errors.push(
      err(
        "WDX0-AC2",
        "catalog-family-drift",
        "the added portfolio must start with zero advertised surfaces",
      ),
    );
  }
  return errors;
}

function validateObligations(ownership, contracts, facts) {
  const errors = [];
  // WDX0-AC3: docs-only node — the runtime correctness cases are bound to the
  // downstream owners instead of being fabricated here.
  const contract = contracts[CONTRACT_MD];
  if (
    !ownership?.acBasisDownstreamTestOwner ||
    !["WDX1", "WDX2", "WDX3"].every((id) => ownership.acBasisDownstreamTestOwner.includes(id))
  ) {
    errors.push(
      err(
        "WDX0-AC3",
        "missing-ac-basis-owner",
        "AC3 runtime cases are not bound to WDX1/WDX2/WDX3",
      ),
    );
  }
  if (!ownership?.acResourceRationale) {
    errors.push(
      err("WDX0-AC3", "missing-ac-resource-rationale", "AC5 resource rationale is not recorded"),
    );
  }
  if (!ownership?.acPublicDeliveryObligation) {
    errors.push(
      err(
        "WDX0-AC4",
        "missing-examples-obligation",
        "AC4 public delivery obligations are not recorded",
      ),
    );
  }
  if (contract) {
    for (const marker of ["DOC1-tested", "examples/reference", "VIM/DX"]) {
      if (!contract.includes(marker)) {
        errors.push(
          err(
            "WDX0-AC4",
            "missing-examples-obligation",
            `contract loses the ${marker} producer obligation`,
          ),
        );
      }
    }
  }
  if (!facts.evidenceFileExists) {
    errors.push(
      err("WDX0-AC4", "missing-evidence-file", `evidence index ${EVIDENCE_FILE} is absent`),
    );
  }
  // WDX0-AC5: additive boundary, no budget, nothing retired.
  if (!Array.isArray(ownership?.displacedRoutes) || ownership.displacedRoutes.length !== 0) {
    errors.push(err("WDX0-AC5", "displaced-route", "WDX0 must not displace routes"));
  }
  if (
    !Array.isArray(ownership?.deletionPopulationThisNode) ||
    ownership.deletionPopulationThisNode.length !== 0
  ) {
    errors.push(
      err("WDX0-AC5", "nonempty-deletion-population", "WDX0 deletion population must be empty"),
    );
  }
  return errors;
}

function validateOwnership(ownership, matrix, authority) {
  const errors = [];
  const dag = new Set((authority.nodes || []).map((node) => node.id));
  const planned = new Set((ownership?.receivingAmendments || []).map((row) => row.receiver));
  planned.add(ownership?.contractNode);

  if (ownership?.contractNode !== "WDX0") {
    errors.push(err("WDX0-ratification", "missing-owner", "ownership map is not owned by WDX0"));
  }
  if (
    !ownership?.finalOwner ||
    !ownership.finalOwner.startsWith("expansion.web-product-convergence")
  ) {
    errors.push(
      err(
        "WDX0-ratification",
        "missing-owner",
        "final owner is not expansion.web-product-convergence",
      ),
    );
  }
  const matrixIds = new Set((matrix?.items || []).map((item) => item.id));
  if (planned.size - 1 !== matrixIds.size || [...matrixIds].some((id) => !planned.has(id))) {
    errors.push(
      err(
        "WDX0-ratification",
        "receiver-count-drift",
        `receiving amendments (${planned.size - 1}) do not match the disposition matrix (${matrixIds.size})`,
      ),
    );
  }
  const itemsById = new Map((matrix?.items || []).map((item) => [item.id, item]));
  for (const row of ownership?.receivingAmendments || []) {
    if (!row.receiver || typeof row.productionCapable !== "boolean" || !row.product) {
      errors.push(
        err(
          "WDX0-ratification",
          "missing-amendment",
          `receiving amendment ${row.receiver || "?"} is malformed`,
        ),
      );
      continue;
    }
    const item = itemsById.get(row.receiver);
    if (!item) continue;
    // A constitution head with a zero production budget is not a
    // production-capable receiver; the bit is the production-authority claim
    // future nodes read.
    const docsOnly = item.kind === "constitution" && item.maxProductionLoc === 0;
    if (docsOnly && row.productionCapable === true) {
      errors.push(
        err(
          "WDX0-ratification",
          "docs-only-production-capable",
          `receiver ${row.receiver} is a ${item.kind} node with zero production budget and must not claim productionCapable`,
        ),
      );
    }
    if (!docsOnly && item.kind === "implementation" && row.productionCapable !== true) {
      errors.push(
        err(
          "WDX0-ratification",
          "docs-only-production-capable",
          `receiver ${row.receiver} is an implementation node and must claim productionCapable`,
        ),
      );
    }
  }
  for (const row of ownership?.outcomes || []) {
    if (!row?.currentOwner || !row?.finalOwner || !row?.receivingAcceptance) {
      errors.push(
        err(
          "WDX0-ratification",
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
            "WDX0-ratification",
            "unknown-owner",
            `owner ${owner} on ${row.id} is neither a DAG node nor a receiving amendment`,
          ),
        );
      }
    }
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
        err("WDX0-ratification", "missing-contract-marker", `contract missing marker ${marker}`),
      );
    }
  }
  for (const marker of ["static proof", "runtime observation", "estimate"]) {
    if (!contract || !contract.includes(marker)) {
      errors.push(
        err(
          "WDX0-AC2",
          "missing-contract-marker",
          `contract does not name the ${marker} evidence class`,
        ),
      );
    }
  }
  for (const marker of ["WDX0", "WDX1", "WDX2", "WDX3", "disposition matrix"]) {
    if (!plan || !plan.includes(marker)) {
      errors.push(err("WDX0-ratification", "missing-plan", `train plan missing marker ${marker}`));
    }
  }
  const node = (authority.nodes || []).find((candidate) => candidate.id === "WDX0");
  if (!node) {
    errors.push(
      err("WDX0-ratification", "missing-owner", "WDX0 is not registered in the program DAG"),
    );
  } else {
    if (node.gate_profile !== "docs-domain" || node.review_profile !== "architecture-3") {
      errors.push(
        err(
          "WDX0-ratification",
          "node-profile-drift",
          "WDX0 gate/review profiles drifted from the charter",
        ),
      );
    }
    for (const domain of ["capability_catalog", "validation_observability"]) {
      if (!(node.conflict_domains || []).includes(domain)) {
        errors.push(
          err(
            "WDX0-ratification",
            "node-profile-drift",
            `WDX0 does not hold the ${domain} conflict domain`,
          ),
        );
      }
    }
    if (node.max_production_loc !== 0 || node.max_production_files !== 0) {
      errors.push(
        err("WDX0-ratification", "zero-budget-violation", "WDX0 production budget must be zero"),
      );
    }
    if (
      node.train !== "expansion.web-product-convergence" ||
      node.product !== "web_product_convergence"
    ) {
      errors.push(
        err(
          "WDX0-ratification",
          "node-profile-drift",
          "WDX0 train/product drifted from the charter",
        ),
      );
    }
    if (JSON.stringify(node.predecessors) !== JSON.stringify(["DX0"])) {
      errors.push(
        err("WDX0-ratification", "node-profile-drift", "WDX0 predecessors must be exactly DX0"),
      );
    }
  }
  // implementedRows() already selects status="implemented" rows; presence in
  // authority.ledger.implemented means implemented.
  const ledgerRow = authority.ledger.implemented.find((row) => row.node_id === "WDX0");
  if (!ledgerRow || !ledgerRow.commit_message) {
    errors.push(
      err("WDX0-ratification", "missing-ledger-row", "WDX0 has no implemented ledger row"),
    );
  }
  // Historical completions preserved: the named anchors stay implemented in
  // the live ledger and the preserved contracts stay on disk.
  const implementedIds = new Set(authority.ledger.implemented.map((row) => row.node_id));
  for (const anchor of products.policy?.historicalCompletions?.anchors || []) {
    if (!implementedIds.has(anchor.id)) {
      errors.push(
        err(
          "WDX0-ratification",
          "historical-anchor-not-implemented",
          `historical anchor ${anchor.id} is not implemented in the live ledger`,
        ),
      );
    }
  }
  for (const preserved of facts.preservedContracts) {
    if (!preserved.exists) {
      errors.push(
        err(
          "WDX0-ratification",
          "preserved-contract-missing",
          `preserved historical contract ${preserved.relative} is absent`,
        ),
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
  errors.push(...validateMatrix(products.matrix));
  errors.push(...validateClaims(products.policy, facts));
  errors.push(...validateObligations(products.ownership, contracts, facts));
  errors.push(...validateOwnership(products.ownership, products.matrix, authority));
  errors.push(...validateRatification(products, contracts, authority, facts));
  return { ok: errors.length === 0, errors };
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
  const products = loadProducts();
  const facts = loadRepositoryFacts();
  console.log(
    `wdx0 verify: PASS cases=${MANDATORY_CASES.length} recommendations=${products.matrix.items.length} fanIn=${products.matrix.qualificationFanIn.length} receivers=${products.ownership.receivingAmendments.length} surfaces=${facts.surfaceCount}(${facts.surfaceFamilies.join("+")})`,
  );
}
