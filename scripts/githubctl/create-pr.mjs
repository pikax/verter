import fs from "node:fs";
import path from "node:path";

import {
  assertApplyClearance,
  assertIssueNumber,
  assertMutationMode,
  assertRepository,
  assertRequiredText,
  findClosingReferences,
  hasExactMappedClosingLink,
  mappedClosingLink,
  parseIssuePayload,
} from "./adapter.mjs";
import { ensureAiGeneratedFooter } from "./issue-provenance.mjs";
import {
  ClosingLinkError,
  DoctorRequiredError,
  DuplicateError,
  GitHubAdapterError,
  MissingIssueMappingError,
  NotFoundError,
  PartialFailureError,
  SelectionError,
  UnstructuredGitHubOutputError,
} from "./errors.mjs";
import { assertSyncAncestors, loadLedgerFile, setImplementedPullRequest } from "./ledger-write.mjs";
import { loadAuthority } from "../../roadmap/0.1.0-tama/tools/lib.mjs";

const CREATE_PR_NODE_ID = "GH3";

function requiredCreatePrAncestors(authority, explicit) {
  if (explicit != null) {
    if (!Array.isArray(explicit) || explicit.some((row) => typeof row !== "string" || !row)) {
      throw new GitHubAdapterError("createPrPrerequisites must be an array of node ids");
    }
    return [...explicit];
  }
  const node = authority.nodes.find((row) => row.id === CREATE_PR_NODE_ID);
  if (!node) {
    throw new GitHubAdapterError(`create-pr block ${CREATE_PR_NODE_ID} is missing from the DAG`);
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

function mutationRecord({ nodeId, ghIssue, kind, mappingWritten }) {
  return {
    node_id: nodeId,
    gh_issue: ghIssue,
    kind,
    mapping_written: mappingWritten,
  };
}

function prBodyWithMappedClose(prose, issueNumber) {
  const link = mappedClosingLink(issueNumber);
  const prefix =
    typeof prose === "string" && prose.trim().length > 0 ? `${prose.trimEnd()}\n\n` : "";
  const body = `${prefix}${link}\n`;
  const found = findClosingReferences(body);
  if (found.length !== 1 || found[0] !== link || !hasExactMappedClosingLink(body, issueNumber)) {
    throw new ClosingLinkError(
      `pull request body must contain exactly one ${link} and no other closing links`,
    );
  }
  return body;
}

function pullsForHead(adapter, head) {
  if (typeof adapter.pullsForHead !== "function") {
    throw new GitHubAdapterError("adapter.pullsForHead is required");
  }
  return adapter.pullsForHead(head);
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

export function createPr(options) {
  const mode = assertMutationMode(options.mode);
  if (!options.adapter) throw new GitHubAdapterError("adapter is required");
  if (options.train != null || options.nodes != null) {
    throw new SelectionError("create-pr accepts exactly one --node; batch selection is forbidden");
  }
  const nodeId = options.node;
  if (typeof nodeId !== "string" || nodeId.length === 0) {
    throw new SelectionError("create-pr requires --node");
  }
  assertRepository(options.adapter, options);
  assertRequiredText(options.title, "pull request title");
  assertRequiredText(options.head, "pull request head");
  const base = typeof options.base === "string" && options.base.length > 0 ? options.base : "main";
  const writeLocator = options.writeLocator === true;
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
  const ledger = loadLedgerFile(ledgerPath);
  assertSyncAncestors(ledger, requiredCreatePrAncestors(authority, options.createPrPrerequisites));
  const mapping = ledger.github_issue.find((row) => row.node_id === nodeId);
  if (!mapping) {
    throw new MissingIssueMappingError(`create-pr requires a local issue mapping for ${nodeId}`);
  }
  const optIn = mapping.sync_to_github === true;
  const body = prBodyWithMappedClose(options.body, mapping.gh_issue);
  const existing = pullsForHead(options.adapter, options.head);
  if (existing.length === 1) {
    throw new DuplicateError(`pull request already exists for head ${options.head}`);
  }
  if (existing.length > 1) {
    throw new DuplicateError(`ambiguous existing pull requests for head ${options.head}`);
  }
  const located = implementedLocator(ledger, nodeId);
  if (located != null) {
    throw new DuplicateError(`implemented row ${nodeId} already locates pull_request ${located}`);
  }
  const request = {
    title: options.title,
    body,
    head: options.head,
    base,
    mappedIssue: mapping.gh_issue,
    owner: options.owner,
    repo: options.repo,
  };
  let issueReport;
  if (!optIn) {
    issueReport = { kind: "protected", number: mapping.gh_issue, applied: false };
  } else {
    const current = readMappedIssue(options.adapter, mapping.gh_issue);
    const normalizedBody = ensureAiGeneratedFooter(current.body);
    issueReport = {
      kind: "update-issue",
      number: mapping.gh_issue,
      title: current.title,
      body: normalizedBody,
      changed: normalizedBody !== current.body,
      applied: false,
    };
  }
  if (mode === "check") {
    const planned = options.adapter.createPullRequest({ ...request, mode: "check" });
    const implementedRow = ledger.implemented.some((row) => row.node_id === nodeId);
    return {
      mode,
      ok: true,
      node_id: nodeId,
      gh_issue: mapping.gh_issue,
      pull_request: planned,
      issue: issueReport,
      locator: {
        written: false,
        pull_request: null,
        implemented_row: implementedRow,
        requested: writeLocator,
      },
    };
  }
  if (!options.clearance) {
    throw new DoctorRequiredError("apply requires GitHubDoctor issues and pullRequests clearance");
  }
  assertApplyClearance(mode, options.clearance, "issues", options.adapter);
  assertApplyClearance(mode, options.clearance, "pullRequests", options.adapter);
  const succeeded = [];
  try {
    if (optIn && issueReport.changed) {
      options.adapter.updateIssue({
        number: mapping.gh_issue,
        title: issueReport.title,
        body: issueReport.body,
        mapping,
        mode: "apply",
        clearance: options.clearance,
        owner: options.owner,
        repo: options.repo,
      });
      succeeded.push(
        mutationRecord({
          nodeId,
          ghIssue: mapping.gh_issue,
          kind: "update-issue",
          mappingWritten: true,
        }),
      );
      issueReport = { ...issueReport, applied: true };
    }
    const created = options.adapter.createPullRequest({
      ...request,
      mode: "apply",
      clearance: options.clearance,
    });
    succeeded.push(
      mutationRecord({
        nodeId,
        ghIssue: created.number,
        kind: "create-pull-request",
        mappingWritten: false,
      }),
    );
    let locator = {
      written: false,
      pull_request: created.number,
      implemented_row: ledger.implemented.some((row) => row.node_id === nodeId),
      requested: writeLocator,
    };
    if (writeLocator) {
      const written = setImplementedPullRequest(ledgerPath, nodeId, created.number);
      locator = {
        written: written.written,
        pull_request: created.number,
        implemented_row: written.written || locator.implemented_row,
        requested: true,
      };
    }
    return {
      mode,
      ok: true,
      node_id: nodeId,
      gh_issue: mapping.gh_issue,
      pull_request: created,
      issue: issueReport,
      locator,
    };
  } catch (error) {
    if (succeeded.length === 0) throw error;
    throw new PartialFailureError({
      succeeded,
      failed: { operation: { node_id: nodeId }, error },
    });
  }
}
