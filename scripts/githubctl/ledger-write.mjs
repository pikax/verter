import fs from "node:fs";

import { parseToml } from "../../roadmap/0.1.0-tama/tools/lib.mjs";
import { assertIssueNumber } from "./adapter.mjs";
import {
  DuplicateError,
  GitHubAdapterError,
  MappingMismatchError,
  MissingAncestorError,
} from "./errors.mjs";

export const COMMIT_DATE_PATTERN = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:Z|[+-]\d{2}:\d{2})$/u;

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

export function loadLedgerFile(file) {
  const text = fs.readFileSync(file, "utf8");
  const parsed = parseToml(text);
  if (!Array.isArray(parsed.implemented)) {
    throw new MissingAncestorError(`${file}: missing [[implemented]] rows`);
  }
  const githubIssue = parsed.github_issue ?? [];
  assertUniqueMappings(githubIssue);
  return {
    file,
    text,
    parsed,
    implemented: parsed.implemented,
    github_issue: githubIssue,
  };
}

export function assertSyncAncestors(ledger, ancestorIds) {
  const implemented = new Set(ledger.implemented.map((row) => row.node_id));
  for (const id of ancestorIds) {
    if (!implemented.has(id)) throw new MissingAncestorError(`missing ancestor ledger row ${id}`);
  }
}

function insertPullRequestLine(text, nodeId, number) {
  const endedWithNewline = text.endsWith("\n");
  const lines = text.replaceAll("\r\n", "\n").replace(/\n$/u, "").split("\n");
  let targetLastField = -1;
  let found = false;
  let index = 0;
  while (index < lines.length) {
    if (lines[index].trim() !== "[[implemented]]") {
      index += 1;
      continue;
    }
    let isTarget = false;
    let lastField = index;
    index += 1;
    while (index < lines.length && !lines[index].trim().startsWith("[")) {
      const trimmed = lines[index].trim();
      const node = trimmed.match(/^node_id\s*=\s*"([^"]*)"\s*$/u);
      if (node?.[1] === nodeId) isTarget = true;
      if (trimmed && !trimmed.startsWith("#")) lastField = index;
      index += 1;
    }
    if (isTarget) {
      if (found) throw new DuplicateError(`duplicate implemented row ${nodeId}`);
      found = true;
      targetLastField = lastField;
    }
  }
  if (!found) {
    throw new MappingMismatchError(`implemented row ${nodeId} is missing from ledger text`);
  }
  lines.splice(targetLastField + 1, 0, `pull_request = ${number}`);
  const joined = lines.join("\n");
  return endedWithNewline || text.length === 0 ? `${joined}\n` : joined;
}

export function setImplementedPullRequest(file, nodeId, pullRequest) {
  const number = assertIssueNumber(pullRequest, "pull_request");
  if (typeof nodeId !== "string" || nodeId.length === 0) {
    throw new MappingMismatchError("node_id is required");
  }
  const loaded = loadLedgerFile(file);
  const matches = loaded.implemented.filter((row) => row.node_id === nodeId);
  if (matches.length > 1) throw new DuplicateError(`duplicate implemented row ${nodeId}`);
  if (matches.length === 0) {
    return { written: false, node_id: nodeId, pull_request: number };
  }
  const existing = matches[0].pull_request;
  if (existing != null) {
    const current = assertIssueNumber(existing, "pull_request");
    if (current !== number) {
      throw new DuplicateError(`implemented row ${nodeId} already locates pull_request ${current}`);
    }
    return { written: true, node_id: nodeId, pull_request: number };
  }
  fs.writeFileSync(file, insertPullRequestLine(loaded.text, nodeId, number));
  return { written: true, node_id: nodeId, pull_request: number };
}

function replaceImplementedFields(text, nodeId, fields) {
  const endedWithNewline = text.endsWith("\n");
  const lines = text.replaceAll("\r\n", "\n").replace(/\n$/u, "").split("\n");
  let found = false;
  const out = [];
  let index = 0;
  while (index < lines.length) {
    if (lines[index].trim() !== "[[implemented]]") {
      out.push(lines[index]);
      index += 1;
      continue;
    }
    const block = [lines[index]];
    index += 1;
    while (index < lines.length && !lines[index].trim().startsWith("[")) {
      block.push(lines[index]);
      index += 1;
    }
    const isTarget = block.some((line) => {
      const node = line.trim().match(/^node_id\s*=\s*"([^"]*)"\s*$/u);
      return node?.[1] === nodeId;
    });
    if (!isTarget) {
      out.push(...block);
      continue;
    }
    if (found) throw new DuplicateError(`duplicate implemented row ${nodeId}`);
    found = true;
    let wroteMessage = false;
    let wroteDate = false;
    let wrotePr = false;
    for (const line of block) {
      const trimmed = line.trim();
      if (/^commit_message\s*=/u.test(trimmed)) {
        out.push(`commit_message = ${JSON.stringify(fields.message)}`);
        wroteMessage = true;
      } else if (/^commit_date\s*=/u.test(trimmed)) {
        out.push(`commit_date = ${JSON.stringify(fields.date)}`);
        wroteDate = true;
      } else if (/^pull_request\s*=/u.test(trimmed)) {
        out.push(`pull_request = ${fields.pullRequest}`);
        wrotePr = true;
      } else {
        out.push(line);
      }
    }
    if (!wroteMessage) out.push(`commit_message = ${JSON.stringify(fields.message)}`);
    if (!wroteDate) out.push(`commit_date = ${JSON.stringify(fields.date)}`);
    if (!wrotePr) out.push(`pull_request = ${fields.pullRequest}`);
  }
  if (!found) {
    throw new MappingMismatchError(`implemented row ${nodeId} is missing from ledger text`);
  }
  const joined = out.join("\n");
  return endedWithNewline || text.length === 0 ? `${joined}\n` : joined;
}

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
  const matches = loaded.implemented.filter((row) => row.node_id === nodeId);
  if (matches.length > 1) throw new DuplicateError(`duplicate implemented row ${nodeId}`);
  if (matches.length === 0) {
    throw new MappingMismatchError(`implemented row ${nodeId} is missing from ledger text`);
  }
  fs.writeFileSync(
    file,
    replaceImplementedFields(loaded.text, nodeId, {
      message,
      date: stamped,
      pullRequest: number,
    }),
  );
  return { written: true, node_id: nodeId, pull_request: number };
}

export function appendGitHubIssueMapping(file, mapping) {
  if (mapping.sync_to_github !== true) {
    throw new MappingMismatchError("created mappings must set sync_to_github = true");
  }
  const loaded = loadLedgerFile(file);
  assertUniqueMappings([
    ...loaded.github_issue,
    { node_id: mapping.node_id, gh_issue: mapping.gh_issue, sync_to_github: true },
  ]);
  const block =
    `[[github_issue]]\n` +
    `node_id = "${mapping.node_id}"\n` +
    `gh_issue = ${mapping.gh_issue}\n` +
    `sync_to_github = true\n`;
  const prefix = loaded.text.length === 0 || loaded.text.endsWith("\n") ? "" : "\n";
  fs.writeFileSync(file, `${loaded.text}${prefix}${block}`);
  return { node_id: mapping.node_id, gh_issue: mapping.gh_issue, sync_to_github: true };
}
