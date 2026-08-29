import fs from "node:fs";

import { confinedFile, loadAuthority } from "../../roadmap/0.1.0-tama/tools/lib.mjs";
import { IssueSyncError, SelectionError } from "./errors.mjs";

const REQUIRED_HEADINGS = ["Problem", "Expected outcome", "Acceptance"];
const SOURCE_OUTCOME = "Independently acceptable outcome";
const SOURCE_SCOPE = "Source-specific scope";
const PROHIBITED_HEADINGS = [
  SOURCE_OUTCOME,
  SOURCE_SCOPE,
  "Deletions and forbidden designs",
  "Abort conditions",
  "Exact predecessor contracts",
  "Acceptance IDs and discriminating proof",
  "Budgets and mandatory rescope",
  "Targeted verification",
  "Review and finding retention",
];

function headingMatches(line, heading) {
  return (
    line === `## ${heading}` ||
    line.startsWith(`## ${heading} `) ||
    line.startsWith(`## ${heading},`) ||
    line.startsWith(`## ${heading} and`)
  );
}

function extractSection(text, headings) {
  const wanted = Array.isArray(headings) ? headings : [headings];
  const lines = text.replaceAll("\r\n", "\n").split("\n");
  const start = lines.findIndex((line) => wanted.some((heading) => headingMatches(line, heading)));
  if (start === -1) return "";
  let end = lines.length;
  for (let index = start + 1; index < lines.length; index += 1) {
    if (lines[index].startsWith("## ")) {
      end = index;
      break;
    }
  }
  return lines
    .slice(start + 1, end)
    .join("\n")
    .trim();
}

function acceptanceBullets(scope, outcome) {
  const bullets = [];
  for (const line of scope.split("\n")) {
    const match = line.match(/^-\s+(.*)$/u);
    if (!match) continue;
    const item = match[1].trim();
    if (item.length === 0) continue;
    bullets.push(`- ${item}`);
    if (bullets.length === 6) return bullets;
  }
  if (bullets.length < 3) {
    for (const paragraph of outcome.split(/\n\n/u)) {
      const item = paragraph.trim();
      if (item.length === 0) continue;
      bullets.push(`- ${item}`);
      if (bullets.length === 6) break;
    }
  }
  return bullets.slice(0, 6);
}

export function assertHumanIssueDescription(title, body) {
  if (typeof title !== "string" || title.length === 0) {
    throw new IssueSyncError("issue title is required");
  }
  if (typeof body !== "string" || body.length === 0) {
    throw new IssueSyncError("issue body is required");
  }
  for (const heading of REQUIRED_HEADINGS) {
    if (!body.includes(`## ${heading}\n`)) {
      throw new IssueSyncError(`issue body missing ${heading}`);
    }
  }
  for (const heading of PROHIBITED_HEADINGS) {
    if (body.includes(`## ${heading}`)) {
      throw new IssueSyncError(`issue body contains prohibited heading ${heading}`);
    }
  }
}

export function renderIssueDescription({ nodeId, model, authority = loadAuthority() }) {
  if (typeof model !== "string" || model.length === 0) {
    throw new IssueSyncError("model is required");
  }
  const node = authority.nodes.find((candidate) => candidate.id === nodeId);
  if (!node) throw new SelectionError(`unknown node ${nodeId}`);
  const charterPath = confinedFile(authority.packageRoot, node.charter, `${nodeId} charter`);
  const raw = fs.readFileSync(charterPath, "utf8");
  const text = raw.replace(/^<!-- unified-charter-v2\n[\s\S]*?\n-->\n*/u, "");
  const outcome = extractSection(text, SOURCE_OUTCOME);
  const scope =
    extractSection(text, SOURCE_SCOPE) ||
    extractSection(text, [
      "Concrete surfaces and APIs",
      "Exact production population and APIs",
      "Expected production surfaces and named APIs",
    ]);
  if (!outcome) {
    throw new IssueSyncError(`charter missing section ${SOURCE_OUTCOME}`);
  }
  const expected = outcome.split(/\n\n/u)[0].trim();
  const body = [
    "## Problem",
    "",
    outcome,
    "",
    "## Expected outcome",
    "",
    expected,
    "",
    "## Acceptance",
    "",
    ...acceptanceBullets(scope, outcome),
    "",
    `Model: ${model}`,
    "",
  ].join("\n");
  assertHumanIssueDescription(node.name, body);
  return { title: node.name, body };
}
