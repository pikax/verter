import fs from "node:fs";
import path from "node:path";

import {
  githubIssueByNumber,
  loadAuthority,
  topological,
} from "../../roadmap/0.1.0-tama/tools/lib.mjs";
import { assertMutationMode, parseIssuePayload } from "./adapter.mjs";
import { renderIssueDescription } from "./charter-render.mjs";
import {
  DoctorRequiredError,
  IssueSyncError,
  NotFoundError,
  PartialFailureError,
  SelectionError,
  UnstructuredGitHubOutputError,
} from "./errors.mjs";
import { appendGitHubIssueMapping, assertSyncAncestors, loadLedgerFile } from "./ledger-write.mjs";

export { githubIssueByNumber as lookupIssueMapping };

const SYNC_NODE_ID = "GH2";

function requiredSyncAncestors(authority) {
  const syncNode = authority.nodes.find((node) => node.id === SYNC_NODE_ID);
  if (!syncNode) throw new IssueSyncError(`sync block ${SYNC_NODE_ID} is missing from the DAG`);
  return [...syncNode.predecessors];
}

function isLiveLedger(ledgerPath, livePath) {
  try {
    return fs.realpathSync(ledgerPath) === fs.realpathSync(livePath);
  } catch {
    return path.resolve(ledgerPath) === path.resolve(livePath);
  }
}

export function selectNodes(authority, { train, nodes }) {
  const hasTrain = typeof train === "string" && train.length > 0;
  const hasNodes = Array.isArray(nodes);
  if (hasTrain === hasNodes) {
    throw new SelectionError("exactly one of --train or --nodes is required");
  }
  const { order } = topological(authority.nodes);
  if (hasTrain) {
    const selected = order.filter((node) => node.train === train);
    if (selected.length === 0) throw new SelectionError(`unknown train ${train}`);
    return selected;
  }
  if (nodes.length === 0) throw new SelectionError("nodes must not be empty");
  const seen = new Set();
  for (const id of nodes) {
    if (typeof id !== "string" || id.length === 0) {
      throw new SelectionError("node id is required");
    }
    if (seen.has(id)) throw new SelectionError(`duplicate node ${id}`);
    seen.add(id);
  }
  const byId = new Map(authority.nodes.map((node) => [node.id, node]));
  for (const id of nodes) {
    if (!byId.has(id)) throw new SelectionError(`unknown node ${id}`);
  }
  return order.filter((node) => seen.has(node.id));
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

function mutationRecord({ nodeId, ghIssue, kind, mappingWritten }) {
  return {
    node_id: nodeId,
    gh_issue: ghIssue,
    kind,
    mapping_written: mappingWritten,
  };
}

function emptyReport(mode, selection) {
  return {
    mode,
    ok: true,
    selection,
    missing: [],
    drift: [],
    protected: [],
    current: [],
    created: [],
    updated: [],
  };
}

/**
 * Check mode is the named IssueCreateOrUpdatePlan boundary
 * (`missing` / `drift` / `protected` / `current`). Apply executes that same
 * plan; it does not build a second planner.
 */
export function syncIssues(options) {
  const mode = assertMutationMode(options.mode);
  if (!options.adapter) throw new IssueSyncError("adapter is required");
  if (typeof options.model !== "string" || options.model.length === 0) {
    throw new IssueSyncError("model is required");
  }
  const authority = options.authority ?? loadAuthority();
  const selected = selectNodes(authority, options);
  const ledgerPath = options.ledgerPath ?? authority.ledgerFile;
  if (
    mode === "apply" &&
    process.env.NODE_TEST_CONTEXT &&
    isLiveLedger(ledgerPath, authority.ledgerFile)
  ) {
    throw new IssueSyncError(
      "tests must pass --ledger; live GitHub mapping writes are forbidden in tests",
    );
  }
  const ledger = loadLedgerFile(ledgerPath);
  assertSyncAncestors(ledger, requiredSyncAncestors(authority));
  const mappingByNode = new Map(ledger.github_issue.map((row) => [row.node_id, row]));
  const report = emptyReport(
    mode,
    selected.map((node) => node.id),
  );
  if (mode === "check") {
    for (const node of selected) {
      const mapping = mappingByNode.get(node.id);
      const rendered = renderIssueDescription({
        nodeId: node.id,
        model: options.model,
        authority,
      });
      if (!mapping) {
        report.missing.push({ node_id: node.id, title: rendered.title, body: rendered.body });
        continue;
      }
      if (mapping.sync_to_github === false) {
        report.protected.push({ node_id: node.id, gh_issue: mapping.gh_issue });
        continue;
      }
      const issue = readMappedIssue(options.adapter, mapping.gh_issue);
      if (issue.title === rendered.title && issue.body === rendered.body) {
        report.current.push({ node_id: node.id, gh_issue: mapping.gh_issue });
      } else {
        report.drift.push({
          node_id: node.id,
          gh_issue: mapping.gh_issue,
          title: rendered.title,
          body: rendered.body,
        });
      }
    }
    return report;
  }
  if (!options.clearance)
    throw new DoctorRequiredError("apply requires GitHubDoctor issues clearance");
  const succeeded = [];
  for (const node of selected) {
    const mapping = mappingByNode.get(node.id);
    const rendered = renderIssueDescription({
      nodeId: node.id,
      model: options.model,
      authority,
    });
    try {
      if (!mapping) {
        const created = options.adapter.createIssue({
          title: rendered.title,
          body: rendered.body,
          mode: "apply",
          clearance: options.clearance,
        });
        const identity = mutationRecord({
          nodeId: node.id,
          ghIssue: created.number,
          kind: "create-issue",
          mappingWritten: false,
        });
        succeeded.push(identity);
        appendGitHubIssueMapping(ledgerPath, {
          node_id: node.id,
          gh_issue: created.number,
          sync_to_github: true,
        });
        identity.mapping_written = true;
        mappingByNode.set(node.id, {
          node_id: node.id,
          gh_issue: created.number,
          sync_to_github: true,
        });
        report.created.push({
          node_id: node.id,
          gh_issue: created.number,
          mapping_written: true,
        });
        continue;
      }
      if (mapping.sync_to_github === false) {
        report.protected.push({ node_id: node.id, gh_issue: mapping.gh_issue });
        continue;
      }
      readMappedIssue(options.adapter, mapping.gh_issue);
      options.adapter.updateIssue({
        number: mapping.gh_issue,
        title: rendered.title,
        body: rendered.body,
        mapping,
        mode: "apply",
        clearance: options.clearance,
      });
      succeeded.push(
        mutationRecord({
          nodeId: node.id,
          ghIssue: mapping.gh_issue,
          kind: "update-issue",
          mappingWritten: true,
        }),
      );
      report.updated.push({ node_id: node.id, gh_issue: mapping.gh_issue });
    } catch (error) {
      if (succeeded.length === 0) throw error;
      throw new PartialFailureError({
        succeeded,
        failed: { operation: { node_id: node.id }, error },
      });
    }
  }
  return report;
}
