import fs from "node:fs";
import path from "node:path";

import {
  assertApplyClearance,
  assertIssueNumber,
  assertMutationMode,
  assertRepository,
  assertRequiredText,
} from "./adapter.mjs";
import {
  CiFailedError,
  DoctorRequiredError,
  DuplicateError,
  GitHubAdapterError,
  MappingMismatchError,
} from "./errors.mjs";
import {
  assertCommitDate,
  assertSyncAncestors,
  finalizeImplementedRow,
  loadLedgerFile,
} from "./ledger-write.mjs";
import { loadAuthority } from "../../roadmap/0.1.0-tama/tools/lib.mjs";

export const TAMA_ROADMAP_JOB = "Tama Roadmap";
const SQUASH_LAND_NODE_ID = "GH5";
const PASSING = new Set(["success", "neutral"]);

function isLiveLedger(ledgerPath, livePath) {
  try {
    return fs.realpathSync(ledgerPath) === fs.realpathSync(livePath);
  } catch {
    return path.resolve(ledgerPath) === path.resolve(livePath);
  }
}

function requiredJobNames(options) {
  const names = new Set();
  const injected = options.requiredJobs;
  if (injected != null) {
    const list = Array.isArray(injected)
      ? injected
      : typeof injected === "string"
        ? injected
            .split(",")
            .map((name) => name.trim())
            .filter(Boolean)
        : null;
    if (!list) throw new GitHubAdapterError("requiredJobs must be an array of job names");
    for (const name of list) {
      if (typeof name !== "string" || name.length === 0) {
        throw new GitHubAdapterError("required job name must be a string");
      }
      names.add(name);
    }
  }
  if (options.tamaChanged === true) names.add(TAMA_ROADMAP_JOB);
  return [...names].sort((left, right) => left.localeCompare(right));
}

function sortJobs(jobs) {
  return [...jobs].sort((left, right) => {
    const byName = left.name.localeCompare(right.name);
    if (byName !== 0) return byName;
    return left.conclusion.localeCompare(right.conclusion);
  });
}

export function ciResult(options) {
  const mode = assertMutationMode(options.mode);
  if (!options.adapter) throw new GitHubAdapterError("adapter is required");
  const pr = assertIssueNumber(options.pr, "pull request number");
  assertRepository(options.adapter, options);
  if (typeof options.adapter.listPullRequestCheckRuns !== "function") {
    throw new GitHubAdapterError("adapter.listPullRequestCheckRuns is required");
  }
  const jobs = sortJobs(options.adapter.listPullRequestCheckRuns(pr));
  const required = requiredJobNames(options);
  const present = new Set(jobs.map((job) => job.name));
  const missing = required.filter((name) => !present.has(name));
  const unexpected_skips = [];
  for (const job of jobs) {
    if (job.skipped && required.includes(job.name) && !unexpected_skips.includes(job.name)) {
      unexpected_skips.push(job.name);
    }
  }
  const failed = jobs.some((job) => {
    if (job.skipped) return required.includes(job.name);
    return !PASSING.has(job.conclusion);
  });
  const ok = jobs.length > 0 && missing.length === 0 && unexpected_skips.length === 0 && !failed;
  return { mode, pr, ok, jobs, missing, unexpected_skips };
}

export function finalizeLedger(options) {
  const nodeId = options.node;
  if (typeof nodeId !== "string" || nodeId.length === 0) {
    throw new MappingMismatchError("finalize-ledger requires --node");
  }
  assertRequiredText(options.message, "commit_message");
  const date = assertCommitDate(options.date);
  const pullRequest = assertIssueNumber(options.pr, "pull_request");
  const authority = options.authority ?? loadAuthority();
  const ledgerPath = options.ledgerPath ?? authority.ledgerFile;
  if (process.env.NODE_TEST_CONTEXT && isLiveLedger(ledgerPath, authority.ledgerFile)) {
    throw new GitHubAdapterError(
      "tests must pass --ledger; live GitHub mapping writes are forbidden in tests",
    );
  }
  return finalizeImplementedRow(ledgerPath, {
    nodeId,
    message: options.message,
    date,
    pullRequest,
  });
}

function implementedLocator(ledger, nodeId) {
  const matches = ledger.implemented.filter((row) => row.node_id === nodeId);
  if (matches.length > 1) throw new DuplicateError(`duplicate implemented row ${nodeId}`);
  if (matches.length === 0) {
    throw new MappingMismatchError(`implemented row ${nodeId} is missing from ledger text`);
  }
  const value = matches[0].pull_request;
  if (value == null) {
    throw new MappingMismatchError(`implemented row ${nodeId} is missing pull_request`);
  }
  return assertIssueNumber(value, "pull_request");
}

function requiredSquashAncestors(authority) {
  const node = authority.nodes.find((row) => row.id === SQUASH_LAND_NODE_ID);
  if (!node) {
    throw new GitHubAdapterError(
      `squash-land block ${SQUASH_LAND_NODE_ID} is missing from the DAG`,
    );
  }
  return [...node.predecessors];
}

export function squashLand(options) {
  const mode = assertMutationMode(options.mode);
  if (!options.adapter) throw new GitHubAdapterError("adapter is required");
  const nodeId = options.node;
  if (typeof nodeId !== "string" || nodeId.length === 0) {
    throw new MappingMismatchError("squash-land requires --node");
  }
  const pr = assertIssueNumber(options.pr, "pull request number");
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
  const ledger = loadLedgerFile(ledgerPath);
  assertSyncAncestors(ledger, requiredSquashAncestors(authority));
  const located = implementedLocator(ledger, nodeId);
  if (located !== pr) {
    throw new MappingMismatchError(
      `implemented row ${nodeId} locates pull_request ${located}, not ${pr}`,
    );
  }
  const ci = ciResult({
    adapter: options.adapter,
    pr,
    requiredJobs: options.requiredJobs,
    tamaChanged: options.tamaChanged,
    owner: options.owner,
    repo: options.repo,
    mode: "check",
  });
  if (!ci.ok) throw new CiFailedError("squash-land requires a successful CiResult");
  if (mode === "check") {
    return {
      mode,
      ok: true,
      kind: "squash-merge",
      number: pr,
      merge_method: "squash",
      applied: false,
      node_id: nodeId,
    };
  }
  if (!options.clearance) {
    throw new DoctorRequiredError("apply requires GitHubDoctor pullRequests clearance");
  }
  assertApplyClearance(mode, options.clearance, "pullRequests", options.adapter);
  if (typeof options.adapter.mergePullRequest !== "function") {
    throw new GitHubAdapterError("adapter.mergePullRequest is required");
  }
  const merged = options.adapter.mergePullRequest({
    number: pr,
    mergeMethod: "squash",
    mode: "apply",
    clearance: options.clearance,
    owner: options.owner,
    repo: options.repo,
  });
  return {
    mode,
    ok: true,
    kind: merged.kind,
    number: merged.number,
    merge_method: merged.merge_method,
    applied: merged.applied,
    node_id: nodeId,
  };
}
