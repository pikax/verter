import path from "node:path";

import { loadAuthority, readToml } from "../../roadmap/0.1.0-tama/tools/lib.mjs";
import { IssueSyncError, SelectionError } from "./errors.mjs";
import { AI_GENERATED_FOOTER, countAiGeneratedFooters } from "./issue-provenance.mjs";

const CATALOG_FILE = "github-issue-content.toml";
const REQUIRED_HEADINGS = ["Problem", "Expected outcome", "Acceptance"];
const PROHIBITED_HEADINGS = [
  "Independently acceptable outcome",
  "Source-specific scope",
  "Deletions and forbidden designs",
  "Abort conditions",
  "Exact predecessor contracts",
  "Acceptance IDs and discriminating proof",
  "Budgets and mandatory rescope",
  "Targeted verification",
  "Review and finding retention",
];
const ALLOWED_FIELDS = new Set([
  "node_id",
  "title",
  "problem",
  "expected_outcome",
  "acceptance",
  "technical_context",
]);
const PROHIBITED_PROSE = [
  /\bDAG\b/iu,
  /\b(?:block|train|phase|stage)\s+[A-Z0-9][A-Z0-9-]*\b/iu,
  /\b(?:implementation|review|verification)_effort\b/iu,
  /\b(?:predecessors?|readiness)\b/iu,
  /\b(?:abort conditions?|targeted verification|review and finding retention)\b/iu,
  /^Model:\s*/imu,
  /<!--/u,
];

function requiredText(value, name) {
  if (typeof value !== "string" || value.trim().length === 0) {
    throw new IssueSyncError(`${name} must be a non-empty string`);
  }
  return value.trim();
}

function textList(value, name, minimum, maximum) {
  if (!Array.isArray(value) || value.length < minimum || value.length > maximum) {
    throw new IssueSyncError(`${name} must contain ${minimum}–${maximum} items`);
  }
  return value.map((item, index) => requiredText(item, `${name}[${index}]`));
}

export function validateIssueContentCatalog(catalog, file = CATALOG_FILE) {
  if (catalog?.schema !== 1) throw new IssueSyncError(`${file}: expected schema 1`);
  if (!Array.isArray(catalog.issue)) {
    throw new IssueSyncError(`${file}: issue must be an array`);
  }
  const byNode = new Map();
  for (const [index, raw] of catalog.issue.entries()) {
    if (raw == null || typeof raw !== "object" || Array.isArray(raw)) {
      throw new IssueSyncError(`${file}: issue[${index}] must be an object`);
    }
    for (const key of Object.keys(raw)) {
      if (!ALLOWED_FIELDS.has(key)) {
        throw new IssueSyncError(`${file}: issue[${index}] has unknown field ${key}`);
      }
    }
    const nodeId = requiredText(raw.node_id, `issue[${index}].node_id`);
    if (byNode.has(nodeId)) throw new IssueSyncError(`${file}: duplicate issue ${nodeId}`);
    const entry = Object.freeze({
      node_id: nodeId,
      title: requiredText(raw.title, `issue[${index}].title`),
      problem: requiredText(raw.problem, `issue[${index}].problem`),
      expected_outcome: requiredText(raw.expected_outcome, `issue[${index}].expected_outcome`),
      acceptance: Object.freeze(textList(raw.acceptance, `issue[${index}].acceptance`, 3, 6)),
      technical_context:
        raw.technical_context == null
          ? Object.freeze([])
          : Object.freeze(
              textList(raw.technical_context, `issue[${index}].technical_context`, 0, 3),
            ),
    });
    byNode.set(nodeId, entry);
  }
  return Object.freeze({
    schema: 1,
    file,
    byNode,
    issues: Object.freeze([...byNode.values()]),
  });
}

export function loadIssueContentCatalog(packageRoot = loadAuthority().packageRoot) {
  const file = path.join(packageRoot, "catalogs", CATALOG_FILE);
  return validateIssueContentCatalog(readToml(file), file);
}

function sectionIndex(body, heading) {
  return body.indexOf(`## ${heading}\n`);
}

function wordCount(body) {
  return body
    .replace(`\n${AI_GENERATED_FOOTER}\n`, "")
    .split(/\s+/u)
    .filter((word) => word.length > 0 && !word.startsWith("##")).length;
}

export function assertHumanIssueDescription(title, body, options = {}) {
  const normalizedTitle = requiredText(title, "issue title");
  if (normalizedTitle !== title || title.length > 120 || /[\r\n]/u.test(title)) {
    throw new IssueSyncError("issue title must be one trimmed line of at most 120 characters");
  }
  if (typeof body !== "string" || body.length === 0 || body.includes("\r")) {
    throw new IssueSyncError("issue body must be non-empty LF text");
  }
  let previous = -1;
  for (const heading of REQUIRED_HEADINGS) {
    const index = sectionIndex(body, heading);
    if (index === -1 || index <= previous || body.indexOf(`## ${heading}\n`, index + 1) !== -1) {
      throw new IssueSyncError(`issue body requires one ordered ${heading} section`);
    }
    previous = index;
  }
  for (const heading of PROHIBITED_HEADINGS) {
    if (body.includes(`## ${heading}`)) {
      throw new IssueSyncError(`issue body contains prohibited heading ${heading}`);
    }
  }
  const prose = `${title}\n${body}`;
  for (const pattern of PROHIBITED_PROSE) {
    if (pattern.test(prose)) {
      throw new IssueSyncError("issue body contains prohibited program prose");
    }
  }
  if (!body.endsWith(`\n${AI_GENERATED_FOOTER}\n`) || countAiGeneratedFooters(body) !== 1) {
    throw new IssueSyncError(`issue body must end with exactly one ${AI_GENERATED_FOOTER}`);
  }
  if (wordCount(body) > 350) throw new IssueSyncError("issue body exceeds 350 words");
  const problem = body
    .slice(
      sectionIndex(body, "Problem") + "## Problem\n".length,
      sectionIndex(body, "Expected outcome"),
    )
    .trim();
  if (
    problem.split(/\s+/u).length < 20 ||
    !/(?:caus|leav|risk|prevent|break|stale|incorrect|unreliable|misattribut|fail)/iu.test(problem)
  ) {
    throw new IssueSyncError("issue Problem must state a concrete defect and impact");
  }
  const acceptance = body
    .slice(
      sectionIndex(body, "Acceptance") + "## Acceptance\n".length,
      body.lastIndexOf(`\n${AI_GENERATED_FOOTER}`),
    )
    .trim();
  const lines = acceptance.split("\n");
  const bullets = lines.filter((line) => line.startsWith("- "));
  if (bullets.length < 3 || bullets.length > 6 || lines.length !== bullets.length) {
    throw new IssueSyncError("issue Acceptance must contain three to six standalone bullets");
  }
  if (typeof options.nodeId === "string") {
    const escaped = options.nodeId.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
    if (new RegExp(`\\b${escaped}\\b`, "u").test(`${title}\n${body}`)) {
      throw new IssueSyncError("issue prose must not contain its roadmap node id");
    }
  }
}

export function renderIssueDescription({
  nodeId,
  authority = loadAuthority(),
  contentCatalog = loadIssueContentCatalog(authority.packageRoot),
}) {
  const node = authority.nodes.find((candidate) => candidate.id === nodeId);
  if (!node) throw new SelectionError(`unknown node ${nodeId}`);
  const content = contentCatalog.byNode?.get(nodeId);
  if (!content) {
    throw new IssueSyncError(
      `${nodeId}: author stable human issue content in catalogs/${CATALOG_FILE} before issue creation or refresh`,
    );
  }
  const technical =
    content.technical_context.length === 0
      ? []
      : ["## Technical context", "", ...content.technical_context.map((item) => `- ${item}`), ""];
  const body = [
    "## Problem",
    "",
    content.problem,
    "",
    "## Expected outcome",
    "",
    content.expected_outcome,
    "",
    ...technical,
    "## Acceptance",
    "",
    ...content.acceptance.map((item) => `- ${item}`),
    "",
    AI_GENERATED_FOOTER,
    "",
  ].join("\n");
  assertHumanIssueDescription(content.title, body, { nodeId });
  return { title: content.title, body };
}
