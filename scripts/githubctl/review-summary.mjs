import fs from "node:fs";
import path from "node:path";

import {
  assertApplyClearance,
  assertIssueNumber,
  assertMutationMode,
  assertRepository,
  assertRequiredText,
  hasExactMappedClosingLink,
  parseIssuePayload,
} from "./adapter.mjs";
import {
  BlockingFindingError,
  DoctorRequiredError,
  DuplicateError,
  GitHubAdapterError,
  MappingMismatchError,
  MissingIssueMappingError,
  NotFoundError,
  PartialFailureError,
  SelectionError,
  UnstructuredGitHubOutputError,
} from "./errors.mjs";
import { assertSyncAncestors, loadLedgerFile } from "./ledger-write.mjs";
import { loadAuthority } from "../../roadmap/0.1.0-tama/tools/lib.mjs";

const REVIEW_SUMMARY_NODE_ID = "GH4";
const MODEL_LINE = /^Model:\s*/u;
const EFFORT_FIELD =
  /^(?:implementation_|review_|verification_|confirmation_)?effort(?:_(?:min|default))?\s*[:=]/iu;
const BLOCKING_SEVERITY = new Set(["P0", "P1"]);

function requiredReviewSummaryAncestors(authority) {
  const node = authority.nodes.find((row) => row.id === REVIEW_SUMMARY_NODE_ID);
  if (!node) {
    throw new GitHubAdapterError(
      `review-summary block ${REVIEW_SUMMARY_NODE_ID} is missing from the DAG`,
    );
  }
  return [...node.predecessors];
}

function isLiveLedger(ledgerPath, livePath) {
  try {
    return fs.realpathSync(ledgerPath) === fs.realpathSync(livePath);
  } catch {
    return path.resolve(ledgerPath) === path.resolve(livePath);
  }
}

function implementedLocator(ledger, nodeId) {
  const matches = ledger.implemented.filter((row) => row.node_id === nodeId);
  if (matches.length > 1) throw new DuplicateError(`duplicate implemented row ${nodeId}`);
  if (matches.length === 0) return null;
  const value = matches[0].pull_request;
  if (value == null) return null;
  return assertIssueNumber(value, "pull_request");
}

function readMappedIssue(adapter, number) {
  let payload;
  try {
    payload = adapter.getIssue(number);
  } catch (error) {
    if (error instanceof NotFoundError) payload = null;
    else throw error;
  }
  if (payload == null) {
    throw new UnstructuredGitHubOutputError(`mapped issue #${number} cannot be read unambiguously`);
  }
  return parseIssuePayload(payload, number);
}

function readPullRequest(adapter, number) {
  if (typeof adapter.getPullRequest !== "function") {
    throw new GitHubAdapterError("adapter.getPullRequest is required");
  }
  let payload;
  try {
    payload = adapter.getPullRequest(number);
  } catch (error) {
    if (error instanceof NotFoundError) payload = null;
    else throw error;
  }
  if (payload == null) throw new NotFoundError(`pull request #${number} is missing`);
  return payload;
}

function assertVerdict(value) {
  if (value === "PASS" || value === "FAIL") return value;
  throw new GitHubAdapterError("verdict must be PASS or FAIL");
}

function parseFindingsInput(raw) {
  if (raw == null || raw === "") return [];
  if (Array.isArray(raw)) return raw;
  if (typeof raw !== "string") {
    throw new GitHubAdapterError("findings must be a JSON array");
  }
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new GitHubAdapterError("findings must be a JSON array");
  }
  if (!Array.isArray(parsed)) throw new GitHubAdapterError("findings must be a JSON array");
  return parsed;
}

function forbiddenFindingResolution(row) {
  if (row.close_issue != null || row.edit_issue != null || row.write_implemented != null) {
    return true;
  }
  return (
    typeof row.resolution === "string" && /\b(?:close|edit)\b.*\bissue\b/iu.test(row.resolution)
  );
}

function normalizeFinding(row) {
  if (row == null || typeof row !== "object" || Array.isArray(row)) {
    throw new GitHubAdapterError("each finding must be an object");
  }
  if (forbiddenFindingResolution(row)) {
    throw new GitHubAdapterError("closing or editing an issue is not a finding resolution");
  }
  const severity = typeof row.severity === "string" ? row.severity.trim().toUpperCase() : "";
  if (!/^P\d+$/u.test(severity)) {
    throw new GitHubAdapterError("finding severity is required");
  }
  if (typeof row.owner !== "string" || row.owner.length === 0) {
    throw new GitHubAdapterError("finding owner is required");
  }
  if (typeof row.context !== "string" || row.context.length === 0) {
    throw new GitHubAdapterError("finding context is required");
  }
  return { severity, owner: row.owner, context: row.context };
}

function blockingFinding(findings) {
  return findings.find((row) => BLOCKING_SEVERITY.has(row.severity)) ?? null;
}

export function countModelLines(body) {
  if (typeof body !== "string" || body.length === 0) return 0;
  return body.split(/\r?\n/u).filter((line) => /^Model:\s+\S/u.test(line)).length;
}

export function ensureOneModelLine(body, model) {
  assertRequiredText(model, "model");
  if (/[\r\n]/u.test(model)) throw new GitHubAdapterError("model must be a single line");
  const lines = (typeof body === "string" ? body : "").replaceAll("\r\n", "\n").split("\n");
  const kept = lines.filter((line) => !MODEL_LINE.test(line) && !EFFORT_FIELD.test(line));
  while (kept.length > 0 && kept[kept.length - 1].trim() === "") kept.pop();
  kept.push("", `Model: ${model}`);
  return `${kept.join("\n")}\n`;
}

function buildReviewCycleSummary({ verdict, body, findings }) {
  const lines = [`Verdict: ${verdict}`, "", body.trimEnd()];
  if (findings.length > 0) {
    lines.push("", "Findings:");
    for (const finding of findings) {
      lines.push(`- ${finding.severity} (${finding.owner}): ${finding.context}`);
    }
  }
  return `${lines.join("\n")}\n`;
}

function assertPullMapsToNode(pull, mapping, located) {
  const closesMapped = hasExactMappedClosingLink(pull.body ?? "", mapping.gh_issue);
  const isLocated = located != null && located === pull.number;
  if (!closesMapped && !isLocated) {
    throw new MappingMismatchError(
      `pull request #${pull.number} does not close #${mapping.gh_issue} and is not the located pull request`,
    );
  }
}

function commentRequest(prNumber, body, extras = {}) {
  return {
    number: prNumber,
    body,
    owner: extras.owner,
    repo: extras.repo,
  };
}

export function reviewSummary(options) {
  const mode = assertMutationMode(options.mode);
  if (!options.adapter) throw new GitHubAdapterError("adapter is required");
  if (options.train != null || options.nodes != null) {
    throw new SelectionError(
      "review-summary accepts exactly one --node; batch selection is forbidden",
    );
  }
  const nodeId = options.node;
  if (typeof nodeId !== "string" || nodeId.length === 0) {
    throw new SelectionError("review-summary requires --node");
  }
  const prNumber = assertIssueNumber(options.pr, "pull request number");
  const verdict = assertVerdict(options.verdict);
  assertRequiredText(options.body, "review body");
  const findings = parseFindingsInput(options.findings).map(normalizeFinding);
  assertRepository(options.adapter, options);
  const authority = options.authority ?? loadAuthority();
  const ledgerPath = options.ledgerPath ?? authority.ledgerFile;
  if (
    mode === "apply" &&
    process.env.NODE_TEST_CONTEXT &&
    isLiveLedger(ledgerPath, authority.ledgerFile)
  ) {
    throw new GitHubAdapterError(
      "tests must pass --ledger; live GitHub mapping writes are forbidden in tests",
    );
  }
  const node = authority.nodes.find((row) => row.id === nodeId);
  if (!node) throw new SelectionError(`unknown node ${nodeId}`);
  const blocking = blockingFinding(findings);
  if (verdict === "PASS" && blocking) {
    throw new BlockingFindingError(`PASS report cannot include ${blocking.severity} findings`);
  }
  if (mode === "apply" && blocking) {
    throw new BlockingFindingError(`${blocking.severity} findings block apply and cannot accept`);
  }
  const ledger = loadLedgerFile(ledgerPath);
  assertSyncAncestors(ledger, requiredReviewSummaryAncestors(authority));
  const mapping = ledger.github_issue.find((row) => row.node_id === nodeId);
  if (!mapping) {
    throw new MissingIssueMappingError(
      `review-summary requires a local issue mapping for ${nodeId}`,
    );
  }
  const optIn = mapping.sync_to_github === true;
  if (optIn && (typeof options.model !== "string" || options.model.length === 0)) {
    throw new GitHubAdapterError(
      "review-summary opt-in mapping requires --model or GITHUBCTL_MODEL",
    );
  }
  const pull = readPullRequest(options.adapter, prNumber);
  if (pull.number !== prNumber) {
    throw new UnstructuredGitHubOutputError(
      `GitHub pull request read returned number ${pull.number}, expected ${prNumber}`,
    );
  }
  assertPullMapsToNode(pull, mapping, implementedLocator(ledger, nodeId));
  const summary = buildReviewCycleSummary({
    verdict,
    body: options.body,
    findings,
  });
  let issueReport;
  let nextIssueBody;
  let currentIssue;
  if (!optIn) {
    issueReport = { kind: "protected", number: mapping.gh_issue, applied: false };
  } else {
    currentIssue = readMappedIssue(options.adapter, mapping.gh_issue);
    nextIssueBody = ensureOneModelLine(currentIssue.body, options.model);
    issueReport = {
      kind: "update-issue",
      number: mapping.gh_issue,
      title: currentIssue.title,
      body: nextIssueBody,
      applied: false,
    };
  }
  const plannedComment = options.adapter.createPullRequestComment({
    ...commentRequest(prNumber, summary, options),
    mode: "check",
  });
  if (mode === "check") {
    return {
      mode,
      ok: true,
      node_id: nodeId,
      gh_issue: mapping.gh_issue,
      pull_request: prNumber,
      comment: plannedComment,
      issue: issueReport,
    };
  }
  if (!options.clearance) {
    throw new DoctorRequiredError("apply requires GitHubDoctor issues and pullRequests clearance");
  }
  assertApplyClearance(mode, options.clearance, "issues", options.adapter);
  assertApplyClearance(mode, options.clearance, "pullRequests", options.adapter);
  const succeeded = [];
  try {
    const created = options.adapter.createPullRequestComment({
      ...commentRequest(prNumber, summary, options),
      mode: "apply",
      clearance: options.clearance,
    });
    succeeded.push({
      kind: created.kind,
      number: created.number,
      node_id: nodeId,
      mapping_written: false,
    });
    if (optIn && nextIssueBody !== currentIssue.body) {
      options.adapter.updateIssue({
        number: mapping.gh_issue,
        title: currentIssue.title,
        body: nextIssueBody,
        mapping,
        mode: "apply",
        clearance: options.clearance,
        owner: options.owner,
        repo: options.repo,
      });
      succeeded.push({
        kind: "update-issue",
        number: mapping.gh_issue,
        node_id: nodeId,
        mapping_written: false,
      });
      issueReport = { ...issueReport, applied: true };
    }
    return {
      mode,
      ok: true,
      node_id: nodeId,
      gh_issue: mapping.gh_issue,
      pull_request: prNumber,
      comment: created,
      issue: issueReport,
    };
  } catch (error) {
    if (succeeded.length === 0) throw error;
    throw new PartialFailureError({
      succeeded,
      failed: { operation: { node_id: nodeId }, error },
    });
  }
}
