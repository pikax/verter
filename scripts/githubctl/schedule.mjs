import { deriveState, loadAuthority } from "../../roadmap/0.1.0-tama/tools/lib.mjs";
import { PROJECT_NUMBER, PROJECT_VIEWS, assertMutationMode, mappingPolicy } from "./adapter.mjs";
import {
  DoctorRequiredError,
  GitHubAdapterError,
  MissingIssueMappingError,
  NonReadyNodeError,
  PartialFailureError,
  ProtectedMappingError,
} from "./errors.mjs";
import { loadLedgerFile } from "./ledger-write.mjs";
import { selectNodes } from "./sync-issues.mjs";

function overlayMilestone(issue) {
  return typeof issue?.milestone === "string" && issue.milestone.length > 0
    ? issue.milestone
    : null;
}

function sameRepository(left, right) {
  return (
    left.owner.toLowerCase() === right.owner.toLowerCase() &&
    left.repo.toLowerCase() === right.repo.toLowerCase()
  );
}

export function schedulePreflight(options) {
  const authority = options.authority ?? loadAuthority();
  const selectedAll = selectNodes(authority, options);
  const ledgerPath = options.ledgerPath ?? authority.ledgerFile;
  const ledger = loadLedgerFile(ledgerPath);
  const state = deriveState(authority, { implemented: ledger.implemented });
  const mappingByNode = new Map(ledger.github_issue.map((row) => [row.node_id, row]));
  const hasTrain = typeof options.train === "string" && options.train.length > 0;
  let selected;
  if (hasTrain) {
    selected = selectedAll.filter((node) => state.states.get(node.id).status === "READY");
    if (selected.length === 0) throw new NonReadyNodeError("schedule requires READY nodes");
  } else {
    const notReady = selectedAll.filter((node) => state.states.get(node.id).status !== "READY");
    if (notReady.length > 0) {
      throw new NonReadyNodeError(
        `schedule requires READY nodes; not READY: ${notReady
          .map((node) => `${node.id}:${state.states.get(node.id).status}`)
          .join(", ")}`,
      );
    }
    selected = selectedAll;
  }
  const unmapped = selected.filter((node) => !mappingByNode.has(node.id));
  if (unmapped.length > 0) {
    throw new MissingIssueMappingError(
      `schedule requires a local issue mapping for ${unmapped.map((node) => node.id).join(", ")}`,
    );
  }
  const protectedNodes = selected.filter((node) => {
    const mapping = mappingByNode.get(node.id);
    return mappingPolicy(mapping, mapping.gh_issue) === "protected";
  });
  if (protectedNodes.length > 0) {
    throw new ProtectedMappingError(
      `schedule refuses protected issue mapping(s): ${protectedNodes
        .map((node) => node.id)
        .join(", ")}`,
    );
  }
  return { authority, selected, ledgerPath, ledger, mappingByNode };
}

export function schedule(options) {
  const mode = assertMutationMode(options.mode);
  if (!options.adapter) throw new GitHubAdapterError("adapter is required");
  const preflight = options.preflight ?? schedulePreflight(options);
  const { authority, selected, mappingByNode } = preflight;
  options.adapter.getProject(PROJECT_NUMBER);
  const mappingByIssue = new Map(
    [...mappingByNode.values()].map((mapping) => [mapping.gh_issue, mapping]),
  );
  const parentsByIssue = new Map();
  const parentSkippedByIssue = new Map();
  const items = selected.map((node) => {
    const mapping = mappingByNode.get(node.id);
    const snapshot = options.adapter.getIssueProjectState(mapping.gh_issue);
    if (snapshot.parent != null) {
      if (!sameRepository(snapshot.parent, options.adapter)) {
        throw new GitHubAdapterError(
          `parent issue #${snapshot.parent.number} belongs to ${snapshot.parent.owner}/${snapshot.parent.repo}`,
        );
      }
      const trainMappings = authority.nodes
        .filter((candidate) => candidate.train === node.train)
        .map((candidate) => mappingByNode.get(candidate.id))
        .filter(Boolean);
      const protectedFamily = trainMappings.some(
        (candidate) => mappingPolicy(candidate, candidate.gh_issue) === "protected",
      );
      const parentMapping = mappingByIssue.get(snapshot.parent.number);
      const protectedParent =
        parentMapping != null &&
        mappingPolicy(parentMapping, parentMapping.gh_issue) === "protected";
      if (protectedFamily || protectedParent) {
        parentsByIssue.delete(snapshot.parent.number);
        parentSkippedByIssue.set(snapshot.parent.number, {
          gh_issue: snapshot.parent.number,
          reason: "protected-mapping",
        });
      } else if (
        !parentSkippedByIssue.has(snapshot.parent.number) &&
        !parentsByIssue.has(snapshot.parent.number)
      ) {
        const parentSnapshot = options.adapter.getIssueProjectState(snapshot.parent.number);
        if (parentSnapshot.id !== snapshot.parent.id) {
          throw new GitHubAdapterError(
            `parent issue #${snapshot.parent.number} identity changed during scheduling`,
          );
        }
        parentsByIssue.set(snapshot.parent.number, {
          gh_issue: snapshot.parent.number,
          project: PROJECT_NUMBER,
          status: "Todo",
          status_changed: parentSnapshot.item == null,
          already_member: parentSnapshot.item != null,
          children: [mapping.gh_issue],
        });
      } else if (parentsByIssue.has(snapshot.parent.number)) {
        parentsByIssue.get(snapshot.parent.number).children.push(mapping.gh_issue);
      }
    }
    return {
      node_id: node.id,
      gh_issue: mapping.gh_issue,
      project: PROJECT_NUMBER,
      status: "Todo",
      status_changed: snapshot.item == null,
      milestone: overlayMilestone(options.adapter.getIssue(mapping.gh_issue)),
      already_member: snapshot.item != null,
    };
  });
  const parents = [...parentsByIssue.values()].sort(
    (left, right) => left.gh_issue - right.gh_issue,
  );
  const parentSkipped = [...parentSkippedByIssue.values()].sort(
    (left, right) => left.gh_issue - right.gh_issue,
  );
  const plan = {
    mode,
    ok: true,
    project: { number: PROJECT_NUMBER },
    views: PROJECT_VIEWS,
    selection: selected.map((node) => node.id),
    items,
    parents,
    parent_skipped: parentSkipped,
    release_target: null,
  };
  if (mode === "check") return plan;
  if (!options.clearance) {
    throw new DoctorRequiredError("apply requires GitHubDoctor projects clearance");
  }
  const succeeded = [];
  const operations = [
    ...selected.map((node, index) => ({
      nodeId: node.id,
      issueNumber: mappingByNode.get(node.id).gh_issue,
      planItem: plan.items[index],
      addKind: "add-project-item",
      statusKind: "set-project-status",
      failureKind: "schedule-project-item",
    })),
    ...parents.map((parent) => ({
      nodeId: null,
      issueNumber: parent.gh_issue,
      planItem: parent,
      addKind: "add-parent-project-item",
      statusKind: "set-parent-project-status",
      failureKind: "schedule-parent-project-item",
    })),
  ];
  for (const operation of operations) {
    try {
      const added = options.adapter.addIssueToProject({
        number: PROJECT_NUMBER,
        issueNumber: operation.issueNumber,
        mode: "apply",
        clearance: options.clearance,
        owner: options.owner,
        repo: options.repo,
      });
      if (typeof added.already_member === "boolean") {
        operation.planItem.already_member = added.already_member;
      } else {
        delete operation.planItem.already_member;
      }
      operation.planItem.status_changed = added.already_member === false;
      if (added.already_member === false) {
        succeeded.push({
          ...(operation.nodeId == null ? {} : { node_id: operation.nodeId }),
          number: operation.issueNumber,
          kind: operation.addKind,
        });
        options.adapter.setIssueProjectStatus({
          issueNumber: operation.issueNumber,
          status: "Todo",
          mode: "apply",
          clearance: options.clearance,
          owner: options.owner,
          repo: options.repo,
        });
        succeeded.push({
          ...(operation.nodeId == null ? {} : { node_id: operation.nodeId }),
          number: operation.issueNumber,
          kind: operation.statusKind,
        });
      }
    } catch (error) {
      if (succeeded.length === 0) throw error;
      throw new PartialFailureError({
        succeeded,
        failed: {
          operation: {
            ...(operation.nodeId == null ? {} : { node_id: operation.nodeId }),
            number: operation.issueNumber,
            kind: operation.failureKind,
          },
          error,
        },
      });
    }
  }
  return plan;
}
