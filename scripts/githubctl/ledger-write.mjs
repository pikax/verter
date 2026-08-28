import fs from "node:fs";

import { parseToml } from "../../roadmap/0.1.0-tama/tools/lib.mjs";
import { assertIssueNumber } from "./adapter.mjs";
import { DuplicateError, MappingMismatchError, MissingAncestorError } from "./errors.mjs";

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
