/**
 * WSP0 constitution validator.
 *
 * Proves the mandatory cases against the machine products, the ratified
 * contract markdown, the train plan, and the live repository authority (DAG,
 * implementation ledger, conflict-domain catalog, MEM0 workload corpus).
 * Does not execute any compiler, editor or runtime; WSP1 owns every new
 * performance instrument, the reference-machine recordings and the real
 * project pins.
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
  "WSP0-AC1",
  "WSP0-AC2",
  "WSP0-AC3",
  "WSP0-AC-OWNER",
  "WSP0-AC-BASIS",
  "WSP0-AC-RESOURCE",
  "WSP0-AC-EXPOSURE",
  "WSP0-ratification",
];

const CONTRACT_MD = "workspace-responsiveness-v1.md";
const PLAN_MD = "expansion-workspace-responsiveness.md";
const EVIDENCE_FILE = "tests/workspace-responsiveness/evidence/WSP0/cases.md";

const RATIFICATION_MARKERS = [
  "issue-93 investigation and interactive SLO constitution",
  "workspace-responsiveness-contract.v1.json",
  "reference-machine-manifests.v1.json",
  "issue93-evidence-plan.v1.json",
  "interactive-slo-catalog.v1.json",
  "stays labelled unverified",
  "pinned-candidate-target",
];

/** Pinned measurement population: the charter's WSP0.3 definitions. */
const REQUIRED_MEASUREMENTS = [
  "client_input_to_paint",
  "server_request_to_response",
  "process_tree_rss",
  "language_service_incremental_rss",
  "protocol_bytes_and_messages",
  "decode_apply_time",
  "worker_queue_depth_and_wait",
  "provider_work",
  "cancellation_waste",
];

/** Pinned workload population: the charter's WSP0.2 coverage. */
const REQUIRED_WORKLOADS = [
  "authored-1k",
  "authored-10k",
  "authored-50k",
  "monorepo-references",
  "single-1mib-source",
  "high-diagnostic-density",
  "unicode-crlf",
  "ignored-dependency-trees",
  "edit-storm",
  "slow-reading-client",
];

/** Pinned interactive operations: existing product-surface families only. */
const REQUIRED_SLO_OPERATIONS = [
  "editor.typing_input_to_paint",
  "editor.hover_present",
  "editor.completion_present",
  "editor.diagnostic_paint_after_edit",
  "editor.save_to_lint_report",
  "editor.rename_apply",
];

const REQUIRED_SLO_TIERS = ["1k", "10k", "50k"];

/** Pinned manifest requirement fields: a machine is comparable only when complete. */
const REQUIRED_MANIFEST_FIELDS = [
  "cpuClass",
  "physicalCores",
  "memoryGiB",
  "storageClass",
  "os",
  "client.identity",
  "client.version",
  "server.binary",
  "server.version",
  "providerEngines[].engine",
  "providerEngines[].version",
  "providerEngines[].mode",
  "enabledFeatures",
  "workloadIds",
];

/** Pinned MEM0 equal-work corpus fixtures (memory side of the denominators). */
const REQUIRED_WORKLOAD_FIXTURES = [
  "shared/props-base.ts",
  "shared/theme.ts",
  "svelte/list.svelte",
  "svelte/malformed.svelte",
  "svelte/panel.svelte",
  "vue/card.vue",
  "vue/malformed.vue",
  "vue/oversize.vue",
  "vue/table.vue",
];

const CONFLICT_DOMAINS = ["lsp_publication", "performance_evidence", "scheduler_admission"];

const FORBIDDEN_IMPROVEMENTS = [
  "reduced-feature-set",
  "hidden-error-truncation",
  "reduced-project-membership",
  "dropped-diagnostics",
];

const EQUIVALENT_WORK_TERMS = [
  "same workload tier",
  "same project membership",
  "same enabled features",
  "same diagnostic publication",
  "same machine manifest",
];

const CORRECTNESS_DENOMINATOR = {
  featuresEnabled: "all-required-current",
  diagnostics: "full-publication",
  membership: "full-project-membership",
  truncation: "forbidden",
};

const TIMELINE_DOMAINS = ["client", "server", "provider"];

function err(caseId, code, message) {
  return { caseId, code, message };
}

function readJson(name) {
  return JSON.parse(fs.readFileSync(path.join(PRODUCTS, name), "utf8"));
}

export function loadProducts() {
  return {
    contract: readJson("workspace-responsiveness-contract.v1.json"),
    machines: readJson("reference-machine-manifests.v1.json"),
    evidencePlan: readJson("issue93-evidence-plan.v1.json"),
    sloCatalog: readJson("interactive-slo-catalog.v1.json"),
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

function listFilesRecursive(root, prefix = "") {
  const out = [];
  for (const entry of fs.readdirSync(root, { withFileTypes: true })) {
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.isDirectory()) out.push(...listFilesRecursive(path.join(root, entry.name), relative));
    else if (entry.isFile()) out.push(relative);
  }
  return out;
}

/** Live repository facts the constitution is grounded in. */
export function loadRepositoryFacts() {
  const domains = readToml(path.join(ROADMAP, "catalogs/conflict-domains.toml"));
  const domainRows = new Map((domains.domain || []).map((row) => [row.id, row]));
  const workloadRoot = path.join(ROADMAP, "catalogs/semantic-memory-workload/sources");
  const fixtures = fs.existsSync(workloadRoot) ? listFilesRecursive(workloadRoot).sort() : [];
  const issueContent = readToml(path.join(ROADMAP, "catalogs/github-issue-content.toml"));
  const trainIssues = readToml(path.join(ROADMAP, "catalogs/github-train-issues.toml"));
  return {
    domainRows,
    workloadFixtures: fixtures,
    issueNodeIds: (issueContent.issue || []).map((row) => row.node_id),
    trainIssueTrains: (trainIssues.train_issue || []).map((row) => row.train),
    evidenceFileExists: fs.existsSync(path.join(REPO_ROOT, EVIDENCE_FILE)),
    productExperienceExists: fs.existsSync(path.join(ROADMAP, "contracts/product-experience.md")),
  };
}

function implementedRow(authority, nodeId) {
  return authority.ledger.implemented.find((row) => row.node_id === nodeId);
}

function validateContractProduct(products, facts) {
  const errors = [];
  const product = products.contract;
  if (product?.schema !== "workspace-responsiveness-contract.v1") {
    return [err("WSP0-ratification", "missing-product", "contract product schema id is absent")];
  }

  // WSP0-AC-OWNER: one final owner with a stated retirement obligation.
  if (product.ownership?.finalOwner !== "expansion.workspace-responsiveness") {
    errors.push(err("WSP0-AC-OWNER", "missing-final-owner", "final owner is not the train owner"));
  }
  if (!product.ownership?.retirementObligation) {
    errors.push(err("WSP0-AC-OWNER", "missing-retirement-obligation", "no retirement obligation"));
  }
  if (JSON.stringify(product.conflictDomains || []) !== JSON.stringify(CONFLICT_DOMAINS)) {
    errors.push(
      err("WSP0-AC-OWNER", "conflict-domain-drift", "conflict domains are not the pinned three"),
    );
  }

  // WSP0-AC1: UI responsiveness is independent of server response time.
  const measurements = product.measurementDefinitions || [];
  const measurementIds = measurements.map((row) => row.id);
  if (
    measurementIds.length !== REQUIRED_MEASUREMENTS.length ||
    REQUIRED_MEASUREMENTS.some((id) => !measurementIds.includes(id))
  ) {
    errors.push(
      err(
        "WSP0-AC-RESOURCE",
        "missing-measurement-definition",
        `measurement population is not the pinned ${REQUIRED_MEASUREMENTS.length}-row set`,
      ),
    );
  }
  const uiMetric = measurements.find((row) => row.id === "client_input_to_paint");
  if (uiMetric && uiMetric.independentOf !== "server_request_to_response") {
    errors.push(
      err("WSP0-AC1", "server-dependent-ui-metric", "client_input_to_paint lost its independence"),
    );
  }

  // WSP0-AC-RESOURCE: RSS classes split; every metric labelled, none guessed.
  const incremental = measurements.find((row) => row.id === "language_service_incremental_rss");
  if (incremental && incremental.separateFrom !== "process_tree_rss") {
    errors.push(
      err(
        "WSP0-AC-RESOURCE",
        "rss-classes-merged",
        "incremental RSS is no longer split from the process tree",
      ),
    );
  }
  for (const row of measurements) {
    if (row.instrumentationStatus !== "not-instrumented") {
      errors.push(
        err("WSP0-AC-RESOURCE", "unlabelled-metric", `${row.id} is not labelled not-instrumented`),
      );
      if (row.instrumentationStatus === "instrumented") {
        errors.push(
          err(
            "WSP0-AC-RESOURCE",
            "fabricated-instrumentation",
            `${row.id} claims instrumentation no WSP1 delivery provides`,
          ),
        );
      }
    }
    if (row.instrumentationOwner !== "WSP1") {
      errors.push(
        err("WSP0-AC-RESOURCE", "unlabelled-metric", `${row.id} does not name WSP1 as owner`),
      );
    }
  }

  // WSP0-AC-RESOURCE / AC2: closed workload population with unpinned real projects.
  const workloads = product.workloadMatrix || [];
  const workloadIds = workloads.map((row) => row.id);
  if (
    workloadIds.length !== REQUIRED_WORKLOADS.length ||
    REQUIRED_WORKLOADS.some((id) => !workloadIds.includes(id))
  ) {
    errors.push(
      err(
        "WSP0-AC-RESOURCE",
        "workload-population-drift",
        `workload matrix is not the pinned ${REQUIRED_WORKLOADS.length}-row population`,
      ),
    );
  }
  for (const row of workloads) {
    const binding = row.realProjectBinding;
    if (binding && binding.status === "pinned") {
      errors.push(
        err(
          "WSP0-AC-RESOURCE",
          "pin-without-machine",
          `${row.id} claims a real project pin without any recorded machine`,
        ),
      );
    }
  }

  // WSP0-AC2: correctness denominator is part of every budget.
  for (const [key, expected] of Object.entries(CORRECTNESS_DENOMINATOR)) {
    if (product.correctnessDenominator?.[key] !== expected) {
      const code =
        key === "truncation"
          ? "diagnostic-truncation"
          : key === "membership"
            ? "unequal-work-denominator"
            : key === "featuresEnabled"
              ? "reduced-feature-set"
              : "unequal-work-denominator";
      errors.push(err("WSP0-AC2", code, `correctness denominator ${key} is not ${expected}`));
    }
  }
  const improvements = product.budgetLaw?.forbiddenImprovements || [];
  if (
    improvements.length !== FORBIDDEN_IMPROVEMENTS.length ||
    FORBIDDEN_IMPROVEMENTS.some((entry) => !improvements.includes(entry))
  ) {
    errors.push(
      err(
        "WSP0-AC2",
        "forbidden-improvement-vocabulary-drift",
        "forbidden-improvement vocabulary is not the pinned four-entry set",
      ),
    );
  }

  // WSP0-AC-RESOURCE: the memory-side equal-work corpus is the live MEM0 workload.
  const pinnedCount = product.equalWorkCorpus?.pinnedFixtureCount;
  if (pinnedCount !== facts.workloadFixtures.length) {
    errors.push(
      err(
        "WSP0-AC-RESOURCE",
        "corpus-drift",
        `pinned fixture count ${pinnedCount} != live ${facts.workloadFixtures.length}`,
      ),
    );
  }
  const fixtureSet = new Set(facts.workloadFixtures);
  for (const fixture of REQUIRED_WORKLOAD_FIXTURES) {
    if (!fixtureSet.has(fixture)) {
      errors.push(err("WSP0-AC-RESOURCE", "corpus-drift", `MEM0 fixture ${fixture} is missing`));
    }
  }

  // WSP0-AC-BASIS: receipt vocabulary reuses the product-experience contract.
  if (product.basisLaw?.receiptVocabulary !== "contracts/product-experience.md#4") {
    errors.push(
      err(
        "WSP0-AC-BASIS",
        "basis-vocabulary-drift",
        "receipt vocabulary no longer binds product-experience §4",
      ),
    );
  }
  if (!product.basisLaw?.incrementalVsFresh?.includes("same")) {
    errors.push(
      err(
        "WSP0-AC-BASIS",
        "basis-vocabulary-drift",
        "incremental-vs-fresh same-basis rule is gone",
      ),
    );
  }

  // WSP0-AC-EXPOSURE: no new operation is introduced by this node.
  if ((product.exposureLaw?.introducedOperations || []).length !== 0) {
    errors.push(
      err(
        "WSP0-AC-EXPOSURE",
        "unregistered-introduced-operation",
        "a constitution node introduces operations; DX1 registration law applies",
      ),
    );
  }
  return errors;
}

function validateMachines(products) {
  const errors = [];
  const machines = products.machines;
  if (machines?.schema !== "reference-machine-manifests.v1") {
    return [err("WSP0-ratification", "missing-product", "machine manifest schema id is absent")];
  }

  // WSP0-AC-RESOURCE: the requirement set pins every field a comparable machine records.
  const required = (machines.manifestRequirements || [])
    .filter((row) => row.required === true || row.required === "reference-client-machine")
    .map((row) => row.field);
  if (
    required.length !== REQUIRED_MANIFEST_FIELDS.length ||
    REQUIRED_MANIFEST_FIELDS.some((field) => !required.includes(field))
  ) {
    errors.push(
      err(
        "WSP0-AC-RESOURCE",
        "manifest-requirement-drift",
        "manifest requirements are not the pinned field set",
      ),
    );
  }
  for (const row of machines.manifestRequirements || []) {
    if (row.field === "enabledFeatures" && row.mustBe !== "all-required-current") {
      errors.push(
        err("WSP0-AC2", "reduced-feature-set", "machine manifests may not enable fewer features"),
      );
    }
  }

  // WSP0-AC-RESOURCE: population truthful at ratification; any recorded machine is complete.
  if (
    machines.populationStatus === "empty-at-ratification" &&
    (machines.machines || []).length > 0
  ) {
    errors.push(
      err(
        "WSP0-AC-RESOURCE",
        "incomplete-machine-manifest",
        "population says empty but machines exist",
      ),
    );
  }
  if ((machines.machines || []).length > 0) {
    for (const machine of machines.machines) {
      for (const field of ["cpuClass", "physicalCores", "memoryGiB", "storageClass", "os"]) {
        if (machine[field] === undefined) {
          errors.push(
            err(
              "WSP0-AC-RESOURCE",
              "incomplete-machine-manifest",
              `machine ${machine.id || "?"} is missing ${field}`,
            ),
          );
        }
      }
    }
  }
  if (!machines.comparisonLaw?.sameBasisOnly) {
    errors.push(
      err("WSP0-AC-BASIS", "basis-vocabulary-drift", "cross-manifest comparison law is gone"),
    );
  }
  return errors;
}

function validateEvidencePlan(products) {
  const errors = [];
  const plan = products.evidencePlan;
  if (plan?.schema !== "issue93-evidence-plan.v1") {
    return [err("WSP0-ratification", "missing-product", "evidence plan schema id is absent")];
  }

  // WSP0-AC3: the original issue stays an unverified hypothesis.
  const issue = plan.originalIssue || {};
  if (issue.disposition !== "unverified") {
    errors.push(
      err(
        "WSP0-AC3",
        "unverified-issue-claim",
        `disposition is ${issue.disposition}, not unverified`,
      ),
    );
  }
  if (issue.rootCauseStatus !== "not-proved") {
    errors.push(
      err("WSP0-AC3", "unverified-issue-claim", "a root cause is claimed without reproduction"),
    );
  }
  if (issue.opened !== "2026-07-25" || !Array.isArray(issue.reportedFacts)) {
    errors.push(
      err("WSP0-AC3", "unverified-issue-claim", "original issue facts are not the recorded ones"),
    );
  }

  // WSP0-AC3: versions are captured before reproduction, never fabricated here.
  for (const row of plan.versionCapture || []) {
    if (row.version !== null || row.status !== "to-record-before-reproduction") {
      errors.push(
        err(
          "WSP0-AC3",
          "fabricated-version-capture",
          `${row.item} records a version without a reproduction basis`,
        ),
      );
    }
  }

  // WSP0-AC1: three clock domains; a server timeline never certifies interactivity.
  const domains = (plan.timelines || []).map((row) => row.domain).sort();
  if (JSON.stringify(domains) !== JSON.stringify([...TIMELINE_DOMAINS].sort())) {
    errors.push(
      err(
        "WSP0-AC1",
        "server-certified-interactivity",
        "timeline domains are not client/server/provider",
      ),
    );
  }
  if (plan.timelineLaw?.serverAloneCannotCertify !== true) {
    errors.push(
      err(
        "WSP0-AC1",
        "server-certified-interactivity",
        "server-alone-cannot-certify law is not pinned",
      ),
    );
  }

  // WSP0-AC-BASIS: WSP1 is the bound downstream runtime-test owner.
  const verify = (plan.phases || []).find((phase) => phase.id === "verify");
  if (!verify || !verify.owner.includes("WSP1")) {
    errors.push(
      err("WSP0-AC-BASIS", "downstream-owner-drift", "verify phase no longer binds WSP1"),
    );
  }
  const reproduce = (plan.phases || []).find((phase) => phase.id === "reproduce");
  if (!reproduce || reproduce.owner !== "WSP1") {
    errors.push(
      err("WSP0-AC-BASIS", "downstream-owner-drift", "reproduce phase no longer binds WSP1"),
    );
  }
  return errors;
}

function validateSloCatalog(products, facts) {
  const errors = [];
  const catalog = products.sloCatalog;
  if (catalog?.schema !== "interactive-slo-catalog.v1") {
    return [err("WSP0-ratification", "missing-product", "SLO catalog schema id is absent")];
  }

  if (JSON.stringify(catalog.tiers || []) !== JSON.stringify(REQUIRED_SLO_TIERS)) {
    errors.push(
      err("WSP0-AC-RESOURCE", "workload-population-drift", "SLO tiers are not 1k/10k/50k"),
    );
  }

  // WSP0-AC1: every row carries a client-domain budget for every tier.
  const rows = catalog.rows || [];
  const operations = rows.map((row) => row.operation);
  if (
    operations.length !== REQUIRED_SLO_OPERATIONS.length ||
    REQUIRED_SLO_OPERATIONS.some((operation) => !operations.includes(operation))
  ) {
    errors.push(
      err(
        "WSP0-AC-EXPOSURE",
        "slo-population-drift",
        "SLO rows are not the pinned existing-operation population",
      ),
    );
  }
  for (const row of rows) {
    for (const tier of REQUIRED_SLO_TIERS) {
      const budget = row.clientBudget?.[tier];
      if (!budget || typeof budget.p50 !== "number" || typeof budget.p99 !== "number") {
        errors.push(
          err(
            "WSP0-AC1",
            "server-only-responsiveness",
            `${row.operation} has no client budget for tier ${tier}`,
          ),
        );
      }
      if (budget && !(budget.p50 > 0 && budget.p99 >= budget.p50)) {
        errors.push(
          err(
            "WSP0-AC-RESOURCE",
            "unlabelled-metric",
            `${row.operation}/${tier} client budget is not a positive ordered pair`,
          ),
        );
      }
    }
  }

  // WSP0-AC2: equivalent-work denominators are part of the SLO.
  const equivalentWork = catalog.denominators?.equivalentWork || [];
  if (
    equivalentWork.length !== EQUIVALENT_WORK_TERMS.length ||
    EQUIVALENT_WORK_TERMS.some((term) => !equivalentWork.includes(term))
  ) {
    errors.push(
      err(
        "WSP0-AC2",
        "unequal-work-denominator",
        "equivalent-work denominators are not the pinned five-term set",
      ),
    );
  }
  if (!String(catalog.denominators?.correctness || "").includes("no truncation")) {
    errors.push(
      err(
        "WSP0-AC2",
        "diagnostic-truncation",
        "correctness denominator no longer forbids truncation",
      ),
    );
  }

  // WSP0-AC-RESOURCE: at ratification nothing is measured; no fabricated provenance.
  if (catalog.provenanceRules?.allowedAtRatification !== "pinned-candidate-target") {
    errors.push(
      err(
        "WSP0-AC-RESOURCE",
        "fabricated-measurement",
        "budgets claim a provenance no reference machine recorded",
      ),
    );
  }
  if ((products.machines.machines || []).length === 0) {
    if (!String(catalog.provenanceRules?.promotion || "").includes("WSP1")) {
      errors.push(
        err(
          "WSP0-AC-RESOURCE",
          "unlabelled-metric",
          "promotion rule no longer names a WSP1 measured gate",
        ),
      );
    }
  }

  // WSP0-AC-BASIS: downstream runtime-test ownership stays with WSP1.
  if (!String(catalog.acceptance?.acBasis || "").includes("WSP1")) {
    errors.push(
      err(
        "WSP0-AC-BASIS",
        "downstream-owner-drift",
        "SLO catalog no longer binds WSP1 as test owner",
      ),
    );
  }

  // WSP0-AC-EXPOSURE: rows measure existing surface families only.
  for (const row of rows) {
    if (row.surfaceFamily !== "editor" || !row.operation.startsWith("editor.")) {
      errors.push(
        err(
          "WSP0-AC-EXPOSURE",
          "unregistered-introduced-operation",
          `${row.operation} is not an existing product-surface family`,
        ),
      );
    }
  }
  return errors;
}

function validateRatification(products, contracts, authority, facts) {
  const errors = [];
  const contractMd = contracts[CONTRACT_MD] || "";
  const planMd = contracts[PLAN_MD] || "";

  for (const marker of RATIFICATION_MARKERS) {
    if (!contractMd.includes(marker)) {
      errors.push(
        err("WSP0-ratification", "missing-contract-marker", `contract misses marker ${marker}`),
      );
    }
  }
  if (!planMd.includes("WSP1") || !planMd.includes("implemented (this delivery)")) {
    errors.push(
      err(
        "WSP0-ratification",
        "missing-plan",
        "train plan is missing or does not map the delivery",
      ),
    );
  }

  const node = authority.nodes.find((candidate) => candidate.id === "WSP0");
  if (!node) {
    errors.push(
      err("WSP0-ratification", "node-profile-drift", "WSP0 is not registered in the repo DAG"),
    );
    return errors;
  }
  if (node.train !== "expansion.workspace-responsiveness") {
    errors.push(err("WSP0-ratification", "node-profile-drift", "DAG train differs"));
  }
  if (node.product !== "wsp_product" || node.kind !== "constitution") {
    errors.push(err("WSP0-ratification", "node-profile-drift", "DAG product/kind differ"));
  }
  if (!String(node.owner || "").startsWith(products.contract.ownership?.finalOwner || "?")) {
    errors.push(
      err("WSP0-ratification", "node-profile-drift", "DAG owner differs from the product owner"),
    );
  }
  if (node.gate_profile !== "docs-domain" || node.review_profile !== "architecture-3") {
    errors.push(err("WSP0-ratification", "node-profile-drift", "DAG gate/review profiles differ"));
  }
  if (node.max_production_loc !== 0 || node.max_production_files !== 0) {
    errors.push(
      err("WSP0-ratification", "zero-budget-violation", "contract-only budget is not zero"),
    );
  }
  if (!fs.existsSync(path.join(ROADMAP, node.charter))) {
    errors.push(err("WSP0-ratification", "missing-plan", "charter file is missing"));
  }

  // Ledger: this node delivered; both predecessors settled.
  if (!implementedRow(authority, "WSP0")) {
    errors.push(
      err("WSP0-ratification", "missing-ledger-row", "WSP0 has no implemented ledger row"),
    );
  }
  for (const predecessor of ["ORC0", "MEM0"]) {
    if (!implementedRow(authority, predecessor)) {
      errors.push(
        err(
          "WSP0-ratification",
          "predecessor-not-settled",
          `${predecessor} is not implemented in the ledger`,
        ),
      );
    }
  }

  // Conflict domains: registered in the live catalog with real path roots.
  for (const domain of CONFLICT_DOMAINS) {
    const row = facts.domainRows.get(domain);
    if (!row) {
      errors.push(
        err(
          "WSP0-AC-OWNER",
          "conflict-domain-unregistered",
          `${domain} is not in conflict-domains.toml`,
        ),
      );
    } else if (!(row.path_roots || []).some((root) => root.includes("crates/"))) {
      errors.push(
        err("WSP0-AC-OWNER", "conflict-domain-unregistered", `${domain} has no crate path roots`),
      );
    }
  }

  // Catalogs and evidence wiring.
  if (!facts.issueNodeIds.includes("WSP0")) {
    errors.push(
      err(
        "WSP0-ratification",
        "missing-issue-catalog-row",
        "github-issue-content.toml has no WSP0 row",
      ),
    );
  }
  if (!facts.trainIssueTrains.includes("expansion.workspace-responsiveness")) {
    errors.push(
      err(
        "WSP0-ratification",
        "missing-issue-catalog-row",
        "github-train-issues.toml has no expansion.workspace-responsiveness row",
      ),
    );
  }
  if (!facts.evidenceFileExists) {
    errors.push(
      err("WSP0-ratification", "missing-evidence-file", "evidence index file is missing"),
    );
  }
  if (!facts.productExperienceExists) {
    errors.push(
      err("WSP0-AC-BASIS", "basis-vocabulary-drift", "contracts/product-experience.md is gone"),
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
  errors.push(...validateContractProduct(products, facts));
  errors.push(...validateMachines(products));
  errors.push(...validateEvidencePlan(products));
  errors.push(...validateSloCatalog(products, facts));
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
    `wsp0 verify: PASS cases=${MANDATORY_CASES.length} measurements=${products.contract.measurementDefinitions.length} workloads=${products.contract.workloadMatrix.length} sloRows=${products.sloCatalog.rows.length} machines=${products.machines.machines.length} fixtures=${facts.workloadFixtures.length}`,
  );
}
