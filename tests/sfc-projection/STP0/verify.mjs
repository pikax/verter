/**
 * STP0 constitution validator.
 *
 * Proves the four mandatory cases against machine products plus the three
 * contract markdowns. Does not execute Vue checking; STP1 owns that harness.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { loadAuthority } from "../../../roadmap/0.1.0-tama/tools/lib.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const PRODUCTS = path.join(HERE, "products");
const CONTRACTS = path.join(REPO_ROOT, "roadmap/0.1.0-tama/contracts");

const MANDATORY_CASES = [
  "STP0-authority",
  "STP0-required-current",
  "STP0-policy",
  "STP0-ratification",
];

const REQUIRED_CONTRACTS = [
  "sfc-typescript-projection.md",
  "sfc-projection-instancetype.md",
  "sfc-projection-implementation-protocol.md",
];

const RATIFICATION_MARKERS = [
  "ProjectionPolicy v1",
  "CurrentFeatureObligation",
  "InferenceParticipation",
  "ProjectionOwnershipMap",
  "## 13. Receiving amendments",
];

const ALLOWED_PARTICIPATION = new Set([
  "authored-signature",
  "contextual-consumer",
  "validation-only",
  "observation-only",
  "coupled",
]);

const FIVE_AUTHORITIES = [
  "CarrierFrontend",
  "FrameworkSemanticAuthority",
  "ProjectionBackend",
  "RuntimeCompilerBackend",
  "FrameworkHostIntegrationBackend",
];

const DOCS_ONLY_KINDS = new Set(["lock", "contract", "constitution", "history"]);

export function loadProducts() {
  return {
    policy: readJson("projection-policy.v1.json"),
    features: readJson("current-feature-obligation.json"),
    inference: readJson("inference-participation.json"),
    ownership: readJson("projection-ownership-map.json"),
  };
}

export function loadContracts() {
  const out = {};
  for (const name of REQUIRED_CONTRACTS) {
    out[name] = fs.readFileSync(path.join(CONTRACTS, name), "utf8");
  }
  return out;
}

export function cloneProducts(products) {
  return structuredClone(products);
}

export function validate(
  products = loadProducts(),
  contracts = loadContracts(),
  authority = loadAuthority(),
) {
  const errors = [];
  errors.push(...validatePolicy(products.policy));
  errors.push(...validateFeatures(products.features));
  errors.push(...validateInference(products.inference));
  errors.push(...validateOwnership(products.ownership, authority));
  errors.push(...validateRatification(products, contracts, authority));
  return { ok: errors.length === 0, errors };
}

function validatePolicy(policy) {
  const errors = [];
  if (policy?.id !== "projection-policy.v1") {
    errors.push(err("STP0-ratification", "missing-product", "ProjectionPolicy v1 id is absent"));
  }
  if (policy?.typeAuthority !== "typescript") {
    errors.push(err("STP0-authority", "native-type-answer", "type authority is not TypeScript"));
  }
  const forbidden = policy?.forbiddenTypeAuthorities || [];
  if (!forbidden.includes("native-assignability") || !forbidden.includes("native-typeinfo")) {
    errors.push(
      err("STP0-authority", "native-type-answer", "native assignability/typeinfo is not forbidden"),
    );
  }
  if (policy?.mappingOwner !== "CodeTransform" || policy?.secondMappingOwner === true) {
    errors.push(err("STP0-authority", "second-mapping-owner", "second mapping owner is present"));
  }
  if (policy?.vueDefaultExport?.shape !== "constructor") {
    errors.push(
      err("STP0-ratification", "constructor-shape", "Vue default export is not constructor-shaped"),
    );
  }
  if (policy?.vueDefaultExport?.publicCallableReplacement) {
    errors.push(
      err("STP0-policy", "callable-replacement", "public callable Vue SFC replacement is present"),
    );
  }
  if (policy?.vueDefaultExport?.syntheticRuntimeMetadataProperty) {
    errors.push(
      err(
        "STP0-policy",
        "synthetic-metadata",
        "synthetic runtime checker-metadata property is present",
      ),
    );
  }
  const inherit = policy?.inheritAttrs || {};
  if (inherit.omitted !== inherit.explicitTrue || inherit.omittedEqualsExplicitTrue !== true) {
    errors.push(
      err(
        "STP0-policy",
        "inheritattrs-default-true-split",
        "default inheritAttrs and explicit true differ without a runtime distinction",
      ),
    );
  }
  if (inherit.runtimeDistinctionFromDefaultToTrue) {
    errors.push(
      err(
        "STP0-policy",
        "inheritattrs-default-true-split",
        "default inheritAttrs and explicit true claim a runtime distinction",
      ),
    );
  }
  const authorities = policy?.compilerAuthorities || [];
  if (authorities.length !== 5 || FIVE_AUTHORITIES.some((name, i) => authorities[i] !== name)) {
    errors.push(
      err("STP0-authority", "sixth-authority", "five compiler authorities are not preserved"),
    );
  }
  if (policy?.planIsSemanticRegistry) {
    errors.push(err("STP0-authority", "sixth-authority", "plan is treated as a semantic registry"));
  }
  if (policy?.specialization?.usesPerTransaction !== 1) {
    errors.push(
      err("STP0-ratification", "specialization", "one use is not one specialization transaction"),
    );
  }
  return errors;
}

function validateFeatures(features) {
  const errors = [];
  const rows = features?.rows || [];
  if (rows.length === 0) {
    errors.push(err("STP0-required-current", "zero-rows", "no RequiredCurrent rows"));
    return errors;
  }
  const ids = new Set();
  for (const row of rows) {
    if (!row?.id) {
      errors.push(err("STP0-required-current", "missing-id", "feature row missing id"));
      continue;
    }
    if (ids.has(row.id)) {
      errors.push(err("STP0-required-current", "duplicate-id", `duplicate feature row ${row.id}`));
    }
    ids.add(row.id);
    if (row.shipped === true && row.obligation !== "RequiredCurrent") {
      errors.push(
        err(
          "STP0-required-current",
          obligationCode(row.obligation),
          `shipped valid feature ${row.id} is ${row.obligation || "missing"}`,
        ),
      );
    }
  }
  if (!ids.has("instancetype-typeof-comp")) {
    errors.push(
      err(
        "STP0-required-current",
        "removed",
        "InstanceType row instancetype-typeof-comp is missing",
      ),
    );
  }
  return errors;
}

function obligationCode(obligation) {
  if (obligation === "optional") return "optionalized";
  if (obligation === "external") return "external";
  if (!obligation || obligation === "removed") return "removed";
  return "not-required-current";
}

function validateInference(inference) {
  const errors = [];
  if (inference?.nativeTypeInfoMayComputeVariance || inference?.nativeTypeInfoMayComputeInference) {
    errors.push(
      err("STP0-authority", "native-type-answer", "native TypeInfo computes variance or inference"),
    );
  }
  const channels = inference?.channels || [];
  if (channels.length === 0) {
    errors.push(err("STP0-policy", "missing-channels", "InferenceParticipation has no channels"));
  }
  for (const channel of channels) {
    if (!ALLOWED_PARTICIPATION.has(channel?.participation)) {
      errors.push(
        err(
          "STP0-policy",
          "illegal-participation",
          `channel ${channel?.id || "?"} has participation ${channel?.participation}`,
        ),
      );
    }
  }
  return errors;
}

function validateOwnership(ownership, authority) {
  const errors = [];
  const dag = new Set((authority.nodes || []).map((node) => node.id));
  const byId = new Map((authority.nodes || []).map((node) => [node.id, node]));
  const planned = new Set((ownership?.receivingAmendments || []).map((row) => row.receiver));
  planned.add(ownership?.contractNode);

  if (ownership?.contractNode !== "STP0") {
    errors.push(err("STP0-ratification", "missing-owner", "ownership map is not owned by STP0"));
  }
  if (
    !Array.isArray(ownership?.receivingAmendments) ||
    ownership.receivingAmendments.length === 0
  ) {
    errors.push(err("STP0-ratification", "missing-amendment", "receiving amendments are missing"));
  }
  for (const required of ["STP1", "STP8", "STP16", "STP34", "STP58", "STS15"]) {
    if (!planned.has(required)) {
      errors.push(
        err(
          "STP0-ratification",
          "missing-amendment",
          `receiving amendment for ${required} is missing`,
        ),
      );
    }
  }

  const populations = [
    ...(ownership?.outcomes || []),
    ...(ownership?.consumers || []),
    ...(ownership?.displacedRoutes || []),
  ];
  if (populations.length === 0) {
    errors.push(err("STP0-ratification", "missing-owner", "ownership map has empty population"));
  }
  for (const row of populations) {
    if (!row?.currentOwner || !row?.finalOwner || !row?.receivingAcceptance) {
      errors.push(
        err(
          "STP0-ratification",
          "missing-owner",
          `row ${row?.id || "?"} lacks current/final owner or AC`,
        ),
      );
      continue;
    }
    errors.push(...checkOwner(row.currentOwner, dag, planned, row.id, "current"));
    errors.push(...checkOwner(row.finalOwner, dag, planned, row.id, "final"));
    if (row.kind === "displaced-route") {
      if (!row.deletionOwner) {
        errors.push(
          err(
            "STP0-ratification",
            "missing-owner",
            `displaced route ${row.id} lacks deletion owner`,
          ),
        );
      } else {
        errors.push(...checkOwner(row.deletionOwner, dag, planned, row.id, "deletion"));
        errors.push(
          ...checkProductionCapableDeletion(
            row.deletionOwner,
            dag,
            byId,
            planned,
            ownership,
            row.id,
          ),
        );
      }
      if (row.retiredByThisNode) {
        errors.push(
          err(
            "STP0-ratification",
            "premature-retirement",
            `displaced route ${row.id} is retired by STP0`,
          ),
        );
      }
    }
  }

  if (
    !Array.isArray(ownership?.deletionPopulationThisNode) ||
    ownership.deletionPopulationThisNode.length !== 0
  ) {
    errors.push(
      err("STP0-ratification", "premature-retirement", "STP0 deletion population must be empty"),
    );
  }
  for (const id of ownership?.ac3UntouchedOwners || []) {
    if (!dag.has(id)) {
      errors.push(
        err("STP0-ratification", "missing-owner", `AC3 untouched owner ${id} is not a DAG node`),
      );
    }
  }
  if (!ownership?.ac3Rationale || !ownership?.ac4Rationale) {
    errors.push(
      err("STP0-ratification", "missing-owner", "AC3/AC4 untouched-owner rationales are missing"),
    );
  }
  return errors;
}

function checkOwner(id, dag, planned, rowId, role) {
  if (dag.has(id) || planned.has(id)) return [];
  return [
    err(
      "STP0-ratification",
      "unknown-owner",
      `${role} owner ${id} on ${rowId} is neither a DAG node nor a receiving amendment`,
    ),
  ];
}

function checkProductionCapableDeletion(id, dag, byId, planned, ownership, rowId) {
  const amendment = (ownership.receivingAmendments || []).find((row) => row.receiver === id);
  if (amendment) {
    if (amendment.productionCapable !== true) {
      return [
        err(
          "STP0-ratification",
          "non-production-deletion-owner",
          `deletion owner ${id} on ${rowId} is not production-capable`,
        ),
      ];
    }
    return [];
  }
  const node = byId.get(id);
  if (!node) return [];
  const docsOnly = DOCS_ONLY_KINDS.has(node.kind) && node.max_production_loc === 0;
  if (docsOnly || node.semantic_role === "history") {
    return [
      err(
        "STP0-ratification",
        "non-production-deletion-owner",
        `deletion owner ${id} on ${rowId} lacks production authority`,
      ),
    ];
  }
  return [];
}

function validateRatification(products, contracts, authority) {
  const errors = [];
  const constitution = contracts["sfc-typescript-projection.md"] || "";
  for (const marker of RATIFICATION_MARKERS) {
    if (!constitution.includes(marker)) {
      errors.push(err("STP0-ratification", "missing-marker", `constitution missing ${marker}`));
    }
  }
  if (constitution.includes("TCM4") && /predecessors.*TCM4/u.test(constitution)) {
    errors.push(
      err("STP0-ratification", "reverse-edge", "constitution adds a reverse edge to TCM4"),
    );
  }
  const dagIds = new Set((authority.nodes || []).map((node) => node.id));
  if (!dagIds.has("STP0")) {
    errors.push(err("STP0-ratification", "missing-owner", "STP0 is not in the repository DAG"));
  }
  const stp0 = (authority.nodes || []).find((node) => node.id === "STP0");
  if (stp0) {
    const preds = new Set(stp0.predecessors || []);
    for (const forbidden of ["TCM4", "BR0"]) {
      if (preds.has(forbidden)) {
        errors.push(
          err("STP0-ratification", "reverse-edge", `STP0 predecessor list includes ${forbidden}`),
        );
      }
    }
    for (const required of ["ORC0", "TCM0R", "CCA1J", "B4R0"]) {
      if (!preds.has(required)) {
        errors.push(
          err("STP0-ratification", "missing-owner", `STP0 missing predecessor ${required}`),
        );
      }
    }
  }
  if (!products?.ownership?.outcomes?.length) {
    errors.push(err("STP0-ratification", "missing-owner", "mandatory outcome rows are missing"));
  }
  return errors;
}

function readJson(name) {
  return JSON.parse(fs.readFileSync(path.join(PRODUCTS, name), "utf8"));
}

function err(caseId, code, message) {
  return { caseId, code, message };
}

export function mandatoryCases() {
  return [...MANDATORY_CASES];
}

export function selectedCaseIds(result) {
  const selected = new Set();
  if (result.ok) selected.add("STP0-ratification");
  for (const error of result.errors) selected.add(error.caseId);
  return [...selected];
}

const isMain = process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const result = validate();
  if (!result.ok) {
    console.error(JSON.stringify(result.errors, null, 2));
    process.exit(1);
  }
  console.log("STP0 verify: PASS cases=" + mandatoryCases().join(","));
}
