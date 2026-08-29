import fs from "node:fs";
import path from "node:path";

import { listGitHubIssues, loadAuthority } from "../../roadmap/0.1.0-tama/tools/lib.mjs";
import {
  AI_OWNED_LABELS,
  MAINTAINER_IGNORE_LABEL,
  aiOwnedLabel,
  assertAiIssueVerdict,
  assertApplyClearance,
  assertIssueNumber,
  assertMutationMode,
  parseIssuePayload,
} from "./adapter.mjs";
import {
  AmbiguousAiLabelError,
  DuplicateError,
  GitHubAdapterError,
  IgnoredIssueError,
  NotFoundError,
} from "./errors.mjs";
import { COMMIT_DATE_PATTERN, loadLedgerFile } from "./ledger-write.mjs";

export const FEEDBACK_REPORT_HEADINGS = Object.freeze([
  "Issue identity",
  "Inspection date",
  "Classification",
  "Reproduction",
  "Code paths",
  "Commands",
  "Verdict",
  "Confidence / ambiguity",
  "Owner hint",
  "Recommendation",
]);

function fieldText(value) {
  return typeof value === "string" ? value : "";
}

function pad2(value) {
  return String(value).padStart(2, "0");
}

export function formatInspectedAt(date = new Date()) {
  const offsetMin = -date.getTimezoneOffset();
  const sign = offsetMin >= 0 ? "+" : "-";
  const abs = Math.abs(offsetMin);
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}T${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(date.getSeconds())}${sign}${pad2(Math.floor(abs / 60))}:${pad2(abs % 60)}`;
}

function resolveInspectedAt(value) {
  if (value == null) return formatInspectedAt();
  if (typeof value !== "string" || !COMMIT_DATE_PATTERN.test(value)) {
    throw new GitHubAdapterError("inspectedAt must match the timezone-bearing date pattern");
  }
  return value;
}

function resolveReportDir(reportDir) {
  if (typeof reportDir === "string" && reportDir.length > 0) return reportDir;
  return path.join(process.cwd(), ".feedback", "issues");
}

function loadInspectLedger(ledgerPath) {
  if (typeof ledgerPath === "string" && ledgerPath.length > 0) {
    return loadLedgerFile(ledgerPath);
  }
  return loadAuthority().ledger;
}

function mappingPolicyForIssue(ledger, number) {
  const rows = listGitHubIssues(ledger).filter((row) => row.gh_issue === number);
  if (rows.length > 1) throw new DuplicateError(`duplicate GitHub issue mapping ${number}`);
  if (rows.length === 0) return { policy: "unmapped", mapping: null };
  return {
    policy: rows[0].sync_to_github ? "opt-in" : "protected",
    mapping: rows[0],
  };
}

export function renderFeedbackReport(fields) {
  const number = assertIssueNumber(fields.issue);
  const values = {
    "Issue identity": `${number} ${fieldText(fields.title)}`.trimEnd(),
    "Inspection date": fields.inspectedAt,
    Classification: fieldText(fields.classification),
    Reproduction: fieldText(fields.reproduction),
    "Code paths": fieldText(fields.codePaths),
    Commands: fieldText(fields.commands),
    Verdict: fields.verdict,
    "Confidence / ambiguity": fieldText(fields.confidence),
    "Owner hint": fieldText(fields.ownerHint),
    Recommendation: fieldText(fields.recommendation),
  };
  const parts = [`# Issue #${number}`, ""];
  for (const heading of FEEDBACK_REPORT_HEADINGS) {
    parts.push(`## ${heading}`, "", values[heading] ?? "", "");
  }
  return `${parts.join("\n").trimEnd()}\n`;
}

function readIssue(adapter, number) {
  let payload;
  try {
    payload = adapter.getIssue(number);
  } catch (error) {
    if (error instanceof NotFoundError) payload = null;
    else throw error;
  }
  if (payload == null) throw new NotFoundError(`issue #${number} is missing`);
  return parseIssuePayload(payload, number);
}

export function inspectIssue(options) {
  if (!options?.adapter) throw new GitHubAdapterError("adapter is required");
  const mode = assertMutationMode(options.mode);
  const number = assertIssueNumber(options.issue);
  const verdict = assertAiIssueVerdict(options.verdict);
  const label = aiOwnedLabel(verdict);
  if (mode === "apply") {
    assertApplyClearance(mode, options.clearance, "issues", options.adapter);
  }
  const { policy } = mappingPolicyForIssue(loadInspectLedger(options.ledgerPath), number);
  const issue = readIssue(options.adapter, number);
  const labels = options.adapter.getIssueLabels(number);
  if (labels.includes(MAINTAINER_IGNORE_LABEL)) {
    throw new IgnoredIssueError(`issue #${number} is labeled ${MAINTAINER_IGNORE_LABEL}`);
  }
  const currentAi = labels.filter((name) => AI_OWNED_LABELS.includes(name));
  if (currentAi.length > 1) {
    throw new AmbiguousAiLabelError(`issue #${number} has multiple AI-result labels`);
  }
  const reportFile = path.join(resolveReportDir(options.reportDir), `${number}.md`);
  if (mode === "check") {
    return {
      mode,
      ok: true,
      issue: number,
      verdict,
      policy,
      report_written: false,
      report_path: null,
      label_written: false,
      label,
    };
  }
  const text = renderFeedbackReport({
    issue: number,
    title: issue.title,
    inspectedAt: resolveInspectedAt(options.inspectedAt),
    verdict,
    classification: options.classification,
    reproduction: options.reproduction,
    codePaths: options.codePaths,
    commands: options.commands,
    confidence: options.confidence,
    ownerHint: options.ownerHint,
    recommendation: options.recommendation,
  });
  fs.mkdirSync(path.dirname(reportFile), { recursive: true });
  fs.writeFileSync(reportFile, text);
  let labelWritten = false;
  if (policy !== "protected") {
    const mutation = options.adapter.setAiResultLabel({
      number,
      verdict,
      mode: "apply",
      clearance: options.clearance,
    });
    labelWritten = mutation.applied === true && mutation.unchanged !== true;
  }
  return {
    mode,
    ok: true,
    issue: number,
    verdict,
    policy,
    report_written: true,
    report_path: reportFile,
    label_written: labelWritten,
    label,
  };
}
