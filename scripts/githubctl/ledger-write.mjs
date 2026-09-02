import fs from "node:fs";

import {
  COMMIT_DATE_PATTERN as LEDGER_COMMIT_DATE_PATTERN,
  implementedRows,
  parseLedgerText,
  serializeLedger,
  setEvidence,
  transitionToImplemented,
} from "../../roadmap/0.1.0-tama/tools/ledger.mjs";
import { assertIssueNumber } from "./adapter.mjs";
import {
  DuplicateError,
  GitHubAdapterError,
  MappingMismatchError,
  MissingAncestorError,
} from "./errors.mjs";

export const COMMIT_DATE_PATTERN = LEDGER_COMMIT_DATE_PATTERN;

export function assertCommitDate(value, label = "commit_date") {
  if (typeof value !== "string" || !COMMIT_DATE_PATTERN.test(value)) {
    throw new GitHubAdapterError(`${label} must match the ledger timezone pattern`);
  }
  return value;
}

export function assertUniqueMappings(rows) {
  if (!Array.isArray(rows)) throw new MappingMismatchError("github_issue must be an array");
  const nodes = new Set();
  const issues = new Set();
  for (const row of rows) {
    if (!row || typeof row !== "object") throw new MappingMismatchError("mapping row is required");
    if (typeof row.node_id !== "string" || row.node_id.length === 0) {
      throw new MappingMismatchError("mapping.node_id is required");
    }
    assertIssueNumber(row.gh_issue, "mapping.gh_issue");
    if (typeof row.sync_to_github !== "boolean") {
      throw new MappingMismatchError("mapping.sync_to_github is required policy");
    }
    if (nodes.has(row.node_id)) throw new DuplicateError(`duplicate node ${row.node_id}`);
    if (issues.has(row.gh_issue)) throw new DuplicateError(`duplicate issue ${row.gh_issue}`);
    nodes.add(row.node_id);
    issues.add(row.gh_issue);
  }
}

export function assertUniqueTrainMappings(rows, issueMappings = []) {
  if (!Array.isArray(rows)) {
    throw new MappingMismatchError("github_train_issue must be an array");
  }
  const trains = new Set();
  const issues = new Set(issueMappings.map((row) => row.gh_issue));
  for (const row of rows) {
    if (!row || typeof row !== "object") {
      throw new MappingMismatchError("train mapping row is required");
    }
    if (typeof row.train !== "string" || row.train.length === 0) {
      throw new MappingMismatchError("mapping.train is required");
    }
    assertIssueNumber(row.gh_issue, "mapping.gh_issue");
    if (trains.has(row.train)) throw new DuplicateError(`duplicate train ${row.train}`);
    if (issues.has(row.gh_issue)) throw new DuplicateError(`duplicate issue ${row.gh_issue}`);
    trains.add(row.train);
    issues.add(row.gh_issue);
  }
}

export function loadLedgerFile(file) {
  const text = fs.readFileSync(file, "utf8");
  let parsed;
  try {
    parsed = parseLedgerText(text);
  } catch (error) {
    throw new MissingAncestorError(`${file}: ${error.message}`);
  }
  const githubIssue = parsed.github_issue ?? [];
  assertUniqueMappings(githubIssue);
  const githubTrain = parsed.github_train_issue ?? [];
  assertUniqueTrainMappings(githubTrain, githubIssue);
  return {
    file,
    text,
    parsed,
    implemented: implementedRows(parsed),
    github_issue: githubIssue,
    github_train_issue: githubTrain,
  };
}

export function assertSyncAncestors(ledger, ancestorIds) {
  const implemented = new Set(ledger.implemented.map((row) => row.node_id));
  for (const id of ancestorIds) {
    if (!implemented.has(id)) throw new MissingAncestorError(`missing ancestor ledger row ${id}`);
  }
}

function statusOf(loaded, nodeId) {
  const record = loaded.parsed.implementation[nodeId];
  return record ? record.status : null;
}

/**
 * Record the PR number on an implemented node's locator evidence. Returns
 * `{written:false}` when the node is still pending (there is no implemented
 * evidence to update yet); the implementation patch itself will carry the
 * transitioned line.
 */
export function setImplementedPullRequest(file, nodeId, pullRequest) {
  const number = assertIssueNumber(pullRequest, "pull_request");
  if (typeof nodeId !== "string" || nodeId.length === 0) {
    throw new MappingMismatchError("node_id is required");
  }
  const loaded = loadLedgerFile(file);
  const status = statusOf(loaded, nodeId);
  if (status !== "implemented") {
    // Absent or still pending: there is no implemented evidence to update;
    // the implementation patch itself carries the transitioned line.
    return { written: false, node_id: nodeId, pull_request: number };
  }
  const existing = loaded.parsed.implementation[nodeId].pull_request;
  if (existing != null) {
    const current = assertIssueNumber(existing, "pull_request");
    if (current !== number) {
      throw new DuplicateError(`implemented row ${nodeId} already locates pull_request ${current}`);
    }
    return { written: true, node_id: nodeId, pull_request: number };
  }
  const next = setEvidence(loaded.parsed, nodeId, { pullRequest: number });
  fs.writeFileSync(file, serializeLedger(next));
  return { written: true, node_id: nodeId, pull_request: number };
}

/** Update an implemented node's full locator evidence (post-landing finalize). */
export function finalizeImplementedRow(file, { nodeId, message, date, pullRequest }) {
  if (typeof nodeId !== "string" || nodeId.length === 0) {
    throw new MappingMismatchError("node_id is required");
  }
  if (typeof message !== "string" || message.length === 0) {
    throw new MappingMismatchError("commit_message is required");
  }
  const stamped = assertCommitDate(date);
  const number = assertIssueNumber(pullRequest, "pull_request");
  const loaded = loadLedgerFile(file);
  const status = statusOf(loaded, nodeId);
  if (status !== "implemented") {
    throw new MappingMismatchError(`implemented row ${nodeId} is missing from ledger text`);
  }
  const next = setEvidence(loaded.parsed, nodeId, {
    commitMessage: message,
    commitDate: stamped,
    pullRequest: number,
  });
  fs.writeFileSync(file, serializeLedger(next));
  return { written: true, node_id: nodeId, pull_request: number };
}

/**
 * Transition a predeclared pending node to implemented. This is the ledger
 * mutation the implementation patch carries; exposed for deterministic
 * tooling (never invoked implicitly by CI evidence).
 */
export function markImplemented(file, { nodeId, message, date, pullRequest }) {
  if (typeof nodeId !== "string" || nodeId.length === 0) {
    throw new MappingMismatchError("node_id is required");
  }
  if (typeof message !== "string" || message.length === 0) {
    throw new MappingMismatchError("commit_message is required");
  }
  const stamped = assertCommitDate(date);
  const number = pullRequest === undefined ? undefined : assertIssueNumber(pullRequest, "pull_request");
  const loaded = loadLedgerFile(file);
  let next;
  try {
    next = transitionToImplemented(loaded.parsed, nodeId, {
      commitMessage: message,
      commitDate: stamped,
      ...(number === undefined ? {} : { pullRequest: number }),
    });
  } catch (error) {
    throw new MappingMismatchError(error.message);
  }
  fs.writeFileSync(file, serializeLedger(next));
  return { written: true, node_id: nodeId, ...(number === undefined ? {} : { pull_request: number }) };
}

export function appendGitHubIssueMapping(file, mapping) {
  if (mapping.sync_to_github !== true) {
    throw new MappingMismatchError("created mappings must set sync_to_github = true");
  }
  const loaded = loadLedgerFile(file);
  const row = { node_id: mapping.node_id, gh_issue: mapping.gh_issue, sync_to_github: true };
  assertUniqueMappings([...loaded.github_issue, row]);
  const next = {
    ...loaded.parsed,
    github_issue: [...loaded.github_issue, row],
  };
  fs.writeFileSync(file, serializeLedger(next));
  return row;
}

export function appendGitHubTrainMapping(file, mapping) {
  const loaded = loadLedgerFile(file);
  const row = { train: mapping.train, gh_issue: mapping.gh_issue };
  assertUniqueTrainMappings([...loaded.github_train_issue, row], loaded.github_issue);
  const next = {
    ...loaded.parsed,
    github_train_issue: [...loaded.github_train_issue, row],
  };
  fs.writeFileSync(file, serializeLedger(next));
  return row;
}
