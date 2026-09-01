import path from "node:path";

import { loadAuthority, readToml } from "../../roadmap/0.1.0-tama/tools/lib.mjs";
import { assertHumanIssueDescription } from "./charter-render.mjs";
import { IssueSyncError } from "./errors.mjs";
import { AI_GENERATED_FOOTER } from "./issue-provenance.mjs";

const CATALOG_FILE = "github-train-issues.toml";
const ALLOWED_FIELDS = new Set([
  "train",
  "title",
  "problem",
  "expected_outcome",
  "acceptance",
  "problem_label",
  "gh_milestone",
]);

function requiredText(value, name) {
  if (typeof value !== "string" || value.trim().length === 0) {
    throw new IssueSyncError(`${name} must be a non-empty string`);
  }
  return value.trim();
}

function acceptanceList(value, name) {
  if (!Array.isArray(value) || value.length < 3 || value.length > 6) {
    throw new IssueSyncError(`${name} must contain 3–6 items`);
  }
  return Object.freeze(value.map((item, index) => requiredText(item, `${name}[${index}]`)));
}

export function validateTrainIssueCatalog(catalog, file = CATALOG_FILE) {
  if (catalog?.schema !== 1) throw new IssueSyncError(`${file}: expected schema 1`);
  if (!Array.isArray(catalog.train_issue)) {
    throw new IssueSyncError(`${file}: train_issue must be an array`);
  }
  const byTrain = new Map();
  for (const [index, raw] of catalog.train_issue.entries()) {
    if (raw == null || typeof raw !== "object" || Array.isArray(raw)) {
      throw new IssueSyncError(`${file}: train_issue[${index}] must be an object`);
    }
    for (const key of Object.keys(raw)) {
      if (!ALLOWED_FIELDS.has(key)) {
        throw new IssueSyncError(`${file}: train_issue[${index}] has unknown field ${key}`);
      }
    }
    const train = requiredText(raw.train, `train_issue[${index}].train`);
    if (byTrain.has(train)) throw new IssueSyncError(`${file}: duplicate train issue ${train}`);
    const problemLabel = requiredText(raw.problem_label, `train_issue[${index}].problem_label`);
    if (!problemLabel.startsWith("problem:")) {
      throw new IssueSyncError(`${file}: ${train} problem_label must start with problem:`);
    }
    const entry = Object.freeze({
      train,
      title: requiredText(raw.title, `train_issue[${index}].title`),
      problem: requiredText(raw.problem, `train_issue[${index}].problem`),
      expected_outcome: requiredText(
        raw.expected_outcome,
        `train_issue[${index}].expected_outcome`,
      ),
      acceptance: acceptanceList(raw.acceptance, `train_issue[${index}].acceptance`),
      problem_label: problemLabel,
      gh_milestone: requiredText(raw.gh_milestone, `train_issue[${index}].gh_milestone`),
    });
    byTrain.set(train, entry);
  }
  return Object.freeze({
    schema: 1,
    file,
    byTrain,
    trainIssues: Object.freeze([...byTrain.values()]),
  });
}

export function loadTrainIssueCatalog(packageRoot = loadAuthority().packageRoot) {
  const file = path.join(packageRoot, "catalogs", CATALOG_FILE);
  return validateTrainIssueCatalog(readToml(file), file);
}

export function trainIssueForTrain(train, catalog) {
  const row = catalog.byTrain?.get(train);
  if (!row) {
    throw new IssueSyncError(
      `${train}: author stable parent issue content in catalogs/${CATALOG_FILE} before sync`,
    );
  }
  return row;
}

export function renderTrainIssueDescription(row) {
  const body = [
    "## Problem",
    "",
    row.problem,
    "",
    "## Expected outcome",
    "",
    row.expected_outcome,
    "",
    "## Acceptance",
    "",
    ...row.acceptance.map((item) => `- ${item}`),
    "",
    AI_GENERATED_FOOTER,
    "",
  ].join("\n");
  assertHumanIssueDescription(row.title, body);
  return { title: row.title, body };
}
