/**
 * JBT0 constitution validator.
 *
 * Proves the four mandatory cases against machine products, the ratified
 * contract markdown, the train plan, and the live repository authority
 * (DAG, conflict-domain catalog, implementation ledger, adapter-presence
 * facts, evidence files). Does not execute any IDE or comparator; JBT1H
 * owns the real-IDE harness and JBT9/JBT10 own qualification.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { loadAuthority, readToml } from "../../../roadmap/0.1.0-tama/tools/lib.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const PRODUCTS = path.join(HERE, "products");
const ROADMAP = path.join(REPO_ROOT, "roadmap/0.1.0-tama");

const MANDATORY_CASES = ["JBT0-AC1", "JBT0-AC2", "JBT0-AC3", "JBT0-ratification"];

const CONTRACT_MD = "jetbrains-baseline.md";
const PLAN_MD = "expansion-jetbrains-product.md";

const RATIFICATION_MARKERS = [
  "JetBrainsBaselineManifest v1",
  "RequiredWorkflowMatrix v1",
  "SuperiorityPredicate v1",
  "pending-capture",
  "recommended/default mode",
];

const CAPTURE_STATES = ["captured", "pending-capture"];

/** JBT0.2 pins the required workflow population; drift in either direction is a failure. */
const REQUIRED_WORKFLOW_IDS = [
  "completion",
  "diagnostics",
  "source-navigation",
  "find-usages",
  "generics",
  "public-types",
  "component-extraction",
  "rename-move-import-updates",
  "formatting",
  "inlays",
  "styles",
  "run-debug-workflows",
];

const REQUIRED_CLAUSES = [
  "workflow-parity",
  "measured-advantage",
  "verter-specific-semantic-tooling",
];

const REQUIRED_METRICS = [
  "real-client-responsiveness (actual client application/paint, not server-side latency alone)",
  "provider-process-cpu (semantic provider processes, both sides)",
  "provider-process-wall",
  "outbound-bytes (wire bytes for the same interactions)",
  "retained-memory",
];

const REQUIRED_CANDIDATE_STATES = ["parity", "gap", "partial", "unsupported", "pending"];

/** JBT0 successors plus the cross-train producers the charter names. */
const REQUIRED_RECEIVERS = [
  "JBT1",
  "JBT1S",
  "JBT1H",
  "JBT2",
  "JBT3",
  "JBT4",
  "JBT4N",
  "JBT5",
  "JBT6",
  "JBT7",
  "JBT8",
  "JBT9",
  "JBT10",
  "JBT11",
  "WSP1",
  "DX1",
  "TST10J",
  "DBG9J",
];

/** STP0 lineage: docs-only DAG kinds with a zero production budget are not production-capable receivers. */
const DOCS_ONLY_KINDS = new Set(["lock", "contract", "constitution", "history"]);

function err(caseId, code, message) {
  return { caseId, code, message };
}

function readJson(name) {
  return JSON.parse(fs.readFileSync(path.join(PRODUCTS, name), "utf8"));
}

export function loadProducts() {
  return {
    manifest: readJson("jetbrains-baseline-manifest.v1.json"),
    matrix: readJson("required-workflow-matrix.v1.json"),
    predicate: readJson("superiority-predicate.v1.json"),
    ownership: readJson("baseline-ownership-map.json"),
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
  const conflictDomains = readToml(path.join(ROADMAP, "catalogs/conflict-domains.toml"));
  const domainIds = new Set((conflictDomains.domain || []).map((row) => row.id));
  return {
    domainIds,
    extensionsJetbrainsExists: fs.existsSync(path.join(REPO_ROOT, "extensions/jetbrains")),
    dxHarnessJetbrainsExists: fs.existsSync(path.join(REPO_ROOT, "packages/dx-harness/jetbrains")),
    verterLspEntryExists: fs.existsSync(path.join(REPO_ROOT, "packages/verter-lsp/package.json")),
  };
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

/** Every pin slot obeys the capture-state law: captured rows carry a value and
 * a capture route; pending rows carry an owner and never a guessed value. */
function validatePin(caseId, pin, errors, label) {
  if (!pin || typeof pin !== "object") {
    errors.push(err(caseId, "pin-without-owner", `${label} is not a pin slot`));
    return;
  }
  if (!CAPTURE_STATES.includes(pin.captureState)) {
    errors.push(
      err(caseId, "unknown-capture-state", `${label} has capture state ${pin.captureState}`),
    );
    return;
  }
  if (!pin.owner) {
    errors.push(err(caseId, "pin-without-owner", `${label} names no capture owner`));
  }
  if (pin.captureState === "captured") {
    if (pin.value === null || pin.value === undefined) {
      errors.push(
        err(caseId, "captured-pin-without-value", `${label} is captured but carries no value`),
      );
    }
    if (!pin.captureRoute) {
      errors.push(
        err(caseId, "captured-pin-without-value", `${label} is captured but names no captureRoute`),
      );
    }
  } else if (pin.value !== null && pin.value !== undefined) {
    errors.push(
      err(
        caseId,
        "pending-pin-with-value",
        `${label} is pending-capture but already carries a value; values arrive only through capture`,
      ),
    );
  }
}

function comparatorPins(comparator) {
  const rows = [
    comparator.ide,
    comparator.build,
    ...(comparator.plugins || []),
    ...(comparator.engines || []),
  ].filter((row) => row !== undefined && row !== null);
  if (comparator.settings?.attestation?.capturedProfile) {
    rows.push(comparator.settings.attestation.capturedProfile);
  }
  if (comparator.corpus?.population) {
    rows.push(comparator.corpus.population);
  }
  return rows;
}

function validateManifest(manifest, facts) {
  const errors = [];
  if (manifest?.id !== "jetbrains-baseline-manifest.v1") {
    return [
      err("JBT0-ratification", "missing-product", "JetBrainsBaselineManifest v1 id is absent"),
    ];
  }
  const comparators = manifest.comparators || [];
  const defaults = comparators.filter((row) => row.alternate === false);
  const alternates = comparators.filter((row) => row.alternate === true);
  if (defaults.length !== 1) {
    errors.push(
      err(
        "JBT0-AC1",
        "default-row-unique",
        "exactly one recommended-default comparator row is required",
      ),
    );
  }
  if (alternates.length < 1) {
    errors.push(
      err(
        "JBT0-AC1",
        "missing-alternate-mode-row",
        "the alternate service-powered official mode is not recorded as a comparator row",
      ),
    );
  }
  for (const comparator of comparators) {
    // JBT0-AC1: the comparator runs with diagnostics enabled and attests it.
    if (comparator.settings?.diagnosticsEnabled !== true) {
      errors.push(
        err(
          "JBT0-AC1",
          "comparator-diagnostics-disabled",
          `comparator ${comparator.id} disables diagnostics; every measurement from it is void`,
        ),
      );
    }
    if (comparator.settings?.attestation?.required !== true) {
      errors.push(
        err(
          "JBT0-AC1",
          "comparator-attestation-optional",
          `comparator ${comparator.id} makes the diagnostics/features attestation optional`,
        ),
      );
    }
    for (const pin of comparatorPins(comparator)) {
      validatePin("JBT0-AC1", pin, errors, `${comparator.id} pin`);
    }
  }
  // Adapter-state facts must track the live repository.
  const adapter = manifest.adapterState || {};
  const adapterPaths = [
    ["extensions/jetbrains", adapter["extensions/jetbrains"], facts.extensionsJetbrainsExists],
    [
      "packages/dx-harness/jetbrains",
      adapter["packages/dx-harness/jetbrains"],
      facts.dxHarnessJetbrainsExists,
    ],
  ];
  for (const [name, row, exists] of adapterPaths) {
    if (!row) {
      errors.push(
        err("JBT0-ratification", "missing-adapter-state", `${name} has no adapter-state row`),
      );
      continue;
    }
    const recordedAbsent = row.state === "absent";
    if (recordedAbsent !== !exists) {
      errors.push(
        err(
          "JBT0-ratification",
          "stale-adapter-state",
          `${name} is recorded ${row.state} but the live repository ${exists ? "has" : "does not have"} it`,
        ),
      );
    }
  }
  if (adapter.semanticEntry?.state !== "verter-lsp only") {
    errors.push(
      err(
        "JBT0-AC1",
        "second-semantic-engine",
        "the semantic entry is not pinned to verter-lsp; a second engine inside the client is forbidden",
      ),
    );
  }
  if (!facts.verterLspEntryExists) {
    errors.push(
      err(
        "JBT0-ratification",
        "stale-adapter-state",
        "the verter-lsp semantic entry no longer exists",
      ),
    );
  }
  errors.push(...missingEvidence("JBT0-ratification", manifest.evidence || []));
  const adapterRows = Object.values(adapter).filter((row) => row && Array.isArray(row.evidence));
  for (const row of adapterRows) errors.push(...missingEvidence("JBT0-ratification", row.evidence));
  return errors;
}

function validateMatrix(matrix) {
  const errors = [];
  if (matrix?.id !== "required-workflow-matrix.v1") {
    return [err("JBT0-ratification", "missing-product", "RequiredWorkflowMatrix v1 id is absent")];
  }
  const ids = (matrix.workflows || []).map((row) => row.id);
  for (const id of REQUIRED_WORKFLOW_IDS) {
    if (!ids.includes(id)) {
      errors.push(
        err(
          "JBT0-AC3",
          "workflow-population-drift",
          `required workflow ${id} is missing from the matrix`,
        ),
      );
    }
  }
  for (const id of ids) {
    if (!REQUIRED_WORKFLOW_IDS.includes(id)) {
      errors.push(
        err(
          "JBT0-AC3",
          "workflow-population-drift",
          `workflow ${id} is outside the JBT0.2 population`,
        ),
      );
    }
  }
  for (const row of matrix.workflows || []) {
    if (
      !row.officialBaseline ||
      !row.passPredicate ||
      !(row.nonRegressionConditions || []).length
    ) {
      errors.push(
        err(
          "JBT0-AC3",
          "workflow-row-malformed",
          `workflow ${row.id} lacks a baseline, pass predicate, or non-regression conditions`,
        ),
      );
    }
  }
  const states = matrix.candidateStates || [];
  if (JSON.stringify(states) !== JSON.stringify(REQUIRED_CANDIDATE_STATES)) {
    errors.push(
      err(
        "JBT0-AC3",
        "collapsed-candidate-states",
        "candidate states must stay the five distinct parity/gap/partial/unsupported/pending states",
      ),
    );
  }
  // JBT0-AC3: a missing required workflow blocks promotion; speed never buys out parity.
  const gate = matrix.promotionGate || {};
  if (gate.missingWorkflowBlocksPromotion !== true || gate.speedNeverBuysOutParity !== true) {
    errors.push(
      err(
        "JBT0-AC3",
        "speed-buys-out-parity",
        "the promotion gate lets a missing required workflow be bought out by speed",
      ),
    );
  }
  if (matrix.measuredOutcomeOwner !== "JBT9") {
    errors.push(
      err(
        "JBT0-AC3",
        "wrong-parity-owner",
        `measured parity outcomes are owned by JBT9, not ${matrix.measuredOutcomeOwner}`,
      ),
    );
  }
  errors.push(...missingEvidence("JBT0-ratification", matrix.evidence || []));
  return errors;
}

function validatePredicate(predicate) {
  const errors = [];
  if (predicate?.id !== "superiority-predicate.v1") {
    return [err("JBT0-ratification", "missing-product", "SuperiorityPredicate v1 id is absent")];
  }
  const conjunction = predicate.conjunction || [];
  if (JSON.stringify(conjunction) !== JSON.stringify(REQUIRED_CLAUSES)) {
    errors.push(
      err(
        "JBT0-AC2",
        "predicate-conjunction-drift",
        "superiority is the conjunction of workflow parity, measured advantage, and Verter-specific semantic tooling",
      ),
    );
  }
  // JBT0-AC2: a single selected microbenchmark cannot certify overall superiority.
  const advantage = predicate.clauses?.["measured-advantage"] || {};
  if (advantage.singleBenchmarkCertifies !== false) {
    errors.push(
      err(
        "JBT0-AC2",
        "single-benchmark-superiority",
        "a single selected microbenchmark is being allowed to certify overall superiority",
      ),
    );
  }
  if (advantage.noPostHocSubsetSelection !== true) {
    errors.push(
      err(
        "JBT0-AC2",
        "post-hoc-metric-subset",
        "metric-set subset selection after measurement is not refused",
      ),
    );
  }
  const metrics = advantage.metricSet || [];
  for (const metric of REQUIRED_METRICS) {
    if (!metrics.includes(metric)) {
      errors.push(
        err("JBT0-AC2", "metric-population-drift", `predeclared metric ${metric} is missing`),
      );
    }
  }
  for (const metric of metrics) {
    if (!REQUIRED_METRICS.includes(metric)) {
      errors.push(
        err(
          "JBT0-AC2",
          "metric-population-drift",
          `metric ${metric} is outside the predeclared population`,
        ),
      );
    }
  }
  const methodology = predicate.methodology || {};
  if (
    methodology.comparatorMode?.diagnosticsEnabled !== true ||
    methodology.comparatorMode?.weakenedComparatorAllowed !== false
  ) {
    errors.push(
      err(
        "JBT0-AC1",
        "comparator-diagnostics-disabled",
        "the measurement methodology does not pin the diagnostics-enabled unweakened comparator",
      ),
    );
  }
  const unavailable = methodology.unavailableMetrics || {};
  if (unavailable.policy !== "label-with-reason" || unavailable.guessedZeros !== false) {
    errors.push(
      err(
        "JBT0-AC2",
        "guessed-missing-metrics",
        "unavailable metrics must be labelled with a reason, never guessed as zeros",
      ),
    );
  }
  if (predicate.thinLspPlugin?.status !== "starting-point-only") {
    errors.push(
      err(
        "JBT0-AC2",
        "thin-plugin-superiority",
        "a thin LSP plugin is being treated as more than an implementation starting point",
      ),
    );
  }
  if (predicate.evaluationRequiresCapturedPins !== true) {
    errors.push(
      err(
        "JBT0-AC1",
        "pending-pin-superiority",
        "the predicate may be evaluated against uncaptured comparator pins; guessed or absent pins close the gate",
      ),
    );
  }
  if (predicate.clauses?.["workflow-parity"]?.owner !== "JBT9") {
    errors.push(
      err("JBT0-AC3", "wrong-parity-owner", "the workflow-parity clause is not owned by JBT9"),
    );
  }
  if (predicate.clauses?.["measured-advantage"]?.owner !== "JBT10") {
    errors.push(
      err(
        "JBT0-AC2",
        "wrong-measurement-owner",
        "the measured-advantage clause is not owned by JBT10",
      ),
    );
  }
  errors.push(...missingEvidence("JBT0-ratification", predicate.evidence || []));
  return errors;
}

function validateOwnership(ownership, authority) {
  const errors = [];
  const dag = new Set((authority.nodes || []).map((node) => node.id));
  const planned = new Set((ownership?.receivingAmendments || []).map((row) => row.receiver));
  planned.add(ownership?.contractNode);

  if (ownership?.contractNode !== "JBT0") {
    errors.push(err("JBT0-ratification", "missing-owner", "ownership map is not owned by JBT0"));
  }
  if (!ownership?.finalOwner || !ownership.finalOwner.startsWith("expansion.jetbrains-product")) {
    errors.push(
      err("JBT0-ratification", "missing-owner", "final owner is not expansion.jetbrains-product"),
    );
  }
  for (const required of REQUIRED_RECEIVERS) {
    if (!planned.has(required)) {
      errors.push(
        err(
          "JBT0-ratification",
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
          "JBT0-ratification",
          "missing-amendment",
          `receiving amendment ${row.receiver || "?"} is malformed`,
        ),
      );
    }
  }
  // JBT0-AC-OWNER (STP0 DOCS_ONLY_KINDS lineage): a receiver whose DAG kind is
  // lock/contract/constitution/history with a zero production budget is not a
  // production-capable receiver.
  const nodesById = new Map((authority.nodes || []).map((node) => [node.id, node]));
  for (const row of ownership?.receivingAmendments || []) {
    const node = nodesById.get(row.receiver);
    if (!node) continue;
    const docsOnly = DOCS_ONLY_KINDS.has(node.kind) && node.max_production_loc === 0;
    if (docsOnly && row.productionCapable === true) {
      errors.push(
        err(
          "JBT0-ratification",
          "docs-only-production-capable",
          `receiver ${row.receiver} is a ${node.kind} node with zero production budget and must not claim productionCapable`,
        ),
      );
    }
  }
  for (const row of ownership?.outcomes || []) {
    if (!row?.currentOwner || !row?.finalOwner || !row?.receivingAcceptance) {
      errors.push(
        err(
          "JBT0-ratification",
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
            "JBT0-ratification",
            "unknown-owner",
            `owner ${owner} on ${row.id} is neither a DAG node nor a receiving amendment`,
          ),
        );
      }
    }
  }
  if (!Array.isArray(ownership?.displacedRoutes) || ownership.displacedRoutes.length !== 0) {
    errors.push(err("JBT0-ratification", "premature-retirement", "JBT0 must not displace routes"));
  }
  if (
    !Array.isArray(ownership?.deletionPopulationThisNode) ||
    ownership.deletionPopulationThisNode.length !== 0
  ) {
    errors.push(
      err("JBT0-ratification", "premature-retirement", "JBT0 deletion population must be empty"),
    );
  }
  if (
    !ownership?.acBasisDownstreamTestOwner ||
    !ownership?.acResourceRationale ||
    !ownership?.acExposureRegistration
  ) {
    errors.push(
      err(
        "JBT0-ratification",
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
        err("JBT0-ratification", "missing-contract-marker", `contract missing marker ${marker}`),
      );
    }
  }
  if (!contract || !contract.includes("Diagnostics stay enabled")) {
    errors.push(
      err(
        "JBT0-AC1",
        "missing-contract-marker",
        "contract does not pin the diagnostics-enabled comparator law",
      ),
    );
  }
  for (const marker of ["JBT0", "JBT1", "JBT1H", "JBT9", "JBT10", "JBT11", "diagnostics enabled"]) {
    if (!plan || !plan.includes(marker)) {
      errors.push(err("JBT0-ratification", "missing-plan", `train plan missing marker ${marker}`));
    }
  }
  const node = (authority.nodes || []).find((candidate) => candidate.id === "JBT0");
  if (!node) {
    errors.push(
      err("JBT0-ratification", "missing-owner", "JBT0 is not registered in the program DAG"),
    );
  } else {
    if (node.gate_profile !== "docs-domain" || node.review_profile !== "architecture-3") {
      errors.push(
        err(
          "JBT0-ratification",
          "missing-owner",
          "JBT0 gate/review profiles drifted from the charter",
        ),
      );
    }
    if (!(node.conflict_domains || []).includes("jetbrains_product")) {
      errors.push(
        err(
          "JBT0-ratification",
          "missing-owner",
          "JBT0 does not hold the jetbrains_product conflict domain",
        ),
      );
    }
    if (node.max_production_loc !== 0 || node.max_production_files !== 0) {
      errors.push(err("JBT0-ratification", "missing-owner", "JBT0 production budget must be zero"));
    }
    if (!(node.predecessors || []).includes("DX0")) {
      errors.push(
        err("JBT0-ratification", "missing-owner", "JBT0 does not consume the DX0 constitution"),
      );
    }
  }
  for (const domain of ["jetbrains_product", "semantic_presentation"]) {
    if (!facts.domainIds.has(domain)) {
      errors.push(
        err(
          "JBT0-ratification",
          "missing-owner",
          `${domain} is not registered in the conflict-domain catalog`,
        ),
      );
    }
  }
  const ledgerRow = authority.ledger.implemented.find((row) => row.node_id === "JBT0");
  if (!ledgerRow || !ledgerRow.commit_message) {
    errors.push(err("JBT0-ratification", "missing-owner", "JBT0 has no implemented ledger row"));
  }
  const dx0 = authority.ledger.implemented.find((row) => row.node_id === "DX0");
  if (!dx0) {
    errors.push(
      err(
        "JBT0-ratification",
        "predecessor-not-implemented",
        "DX0 is not implemented in the ledger; JBT0 cannot consume an unaccepted predecessor",
      ),
    );
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
  errors.push(...validateManifest(products.manifest, facts));
  errors.push(...validateMatrix(products.matrix));
  errors.push(...validatePredicate(products.predicate));
  errors.push(...validateOwnership(products.ownership, authority));
  errors.push(...validateRatification(products, contracts, authority, facts));
  return { ok: errors.length === 0, errors };
}

export function mandatoryCases() {
  return [...MANDATORY_CASES];
}

export function selectedCaseIds(result) {
  return [...new Set((result.errors || []).map((error) => error.caseId))];
}

function pendingPinCount(manifest) {
  let count = 0;
  for (const comparator of manifest.comparators || []) {
    for (const pin of comparatorPins(comparator)) {
      if (pin && pin.captureState === "pending-capture") count += 1;
    }
  }
  return count;
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
    `jbt0 verify: PASS cases=${MANDATORY_CASES.length} workflows=${REQUIRED_WORKFLOW_IDS.length} receivers=${REQUIRED_RECEIVERS.length} pendingPins=${pendingPinCount(loadProducts().manifest)}`,
  );
}
