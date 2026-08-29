import { loadAuthority } from "../../roadmap/0.1.0-tama/tools/lib.mjs";
import {
  PROJECT_NUMBER,
  assertApplyClearance,
  assertMutationMode,
  assertRepository,
  mappingPolicy,
} from "./adapter.mjs";
import {
  DoctorRequiredError,
  GitHubAdapterError,
  MissingIssueMappingError,
  MissingProjectIdentityError,
  PartialFailureError,
  ProtectedMappingError,
  SelectionError,
} from "./errors.mjs";
import { loadLedgerFile } from "./ledger-write.mjs";

const REQUESTED_STATUS = Object.freeze({
  "in-progress": "In Progress",
  done: "Done",
});

function desiredStatus(value) {
  const status = REQUESTED_STATUS[value];
  if (!status) throw new GitHubAdapterError("status must be in-progress or done");
  return status;
}

function itemPlan(number, current, status) {
  if (current == null) {
    throw new MissingProjectIdentityError(
      `issue #${number} Project ${PROJECT_NUMBER} item is missing; schedule it first`,
    );
  }
  return {
    gh_issue: number,
    project: PROJECT_NUMBER,
    current: current?.status ?? null,
    status,
    add: false,
    changed: current?.status !== status,
  };
}

function sameRepository(left, right) {
  return (
    left.owner.toLowerCase() === right.owner.toLowerCase() &&
    left.repo.toLowerCase() === right.repo.toLowerCase()
  );
}

function parentStatus(snapshot, childNumber, childStatus, expectedChildren, repository) {
  const localSubIssues = snapshot.subIssues.filter((row) => sameRepository(row, repository));
  const child = localSubIssues.find((row) => row.number === childNumber);
  if (!child) {
    throw new GitHubAdapterError(
      `parent issue #${snapshot.number} does not list child #${childNumber}`,
    );
  }
  if (childStatus !== "Done") return "In Progress";
  const subIssueByNumber = new Map(localSubIssues.map((row) => [row.number, row]));
  const missing = expectedChildren.filter((number) => !subIssueByNumber.has(number));
  if (missing.length > 0) {
    throw new GitHubAdapterError(
      `parent issue #${snapshot.number} is missing expected train child issue(s): ${missing
        .map((number) => `#${number}`)
        .join(", ")}`,
    );
  }
  return expectedChildren.every((number) => {
    const row = subIssueByNumber.get(number);
    const status = number === childNumber ? childStatus : row.item?.status;
    return status === "Done";
  })
    ? "Done"
    : "In Progress";
}

export function projectStatusPreflight(options) {
  if (options.train != null || options.nodes != null) {
    throw new SelectionError("project-status accepts exactly one --node");
  }
  const nodeId = options.node;
  if (typeof nodeId !== "string" || nodeId.length === 0) {
    throw new SelectionError("project-status requires --node");
  }
  const status = desiredStatus(options.status);
  const authority = options.authority ?? loadAuthority();
  const node = authority.nodes.find((row) => row.id === nodeId);
  if (!node) {
    throw new SelectionError(`unknown node ${nodeId}`);
  }
  const ledger = loadLedgerFile(options.ledgerPath ?? authority.ledgerFile);
  const mappingByNode = new Map(ledger.github_issue.map((row) => [row.node_id, row]));
  const mapping = mappingByNode.get(nodeId);
  if (!mapping) {
    throw new MissingIssueMappingError(
      `project-status requires a local issue mapping for ${nodeId}`,
    );
  }
  if (mappingPolicy(mapping, mapping.gh_issue) === "protected") {
    throw new ProtectedMappingError(
      `refusing Project status update of protected issue #${mapping.gh_issue}`,
    );
  }
  return { authority, node, nodeId, status, mappingByNode, mapping };
}

export function projectStatus(options) {
  const mode = assertMutationMode(options.mode);
  if (!options.adapter) throw new GitHubAdapterError("adapter is required");
  assertRepository(options.adapter, options);
  const preflight = options.preflight ?? projectStatusPreflight(options);
  const { authority, node, nodeId, status, mappingByNode, mapping } = preflight;
  if (mode === "apply" && !options.clearance) {
    throw new DoctorRequiredError("apply requires GitHubDoctor projects clearance");
  }
  if (mode === "apply") {
    assertApplyClearance(mode, options.clearance, "projects", options.adapter);
  }
  if (typeof options.adapter.getIssueProjectState !== "function") {
    throw new GitHubAdapterError("adapter.getIssueProjectState is required");
  }
  if (typeof options.adapter.setIssueProjectStatus !== "function") {
    throw new GitHubAdapterError("adapter.setIssueProjectStatus is required");
  }

  options.adapter.getProjectStatusField(PROJECT_NUMBER);
  const snapshot = options.adapter.getIssueProjectState(mapping.gh_issue);
  const item = itemPlan(mapping.gh_issue, snapshot.item, status);
  let parent = null;
  let parentSkipped = null;
  if (snapshot.parent != null) {
    if (!sameRepository(snapshot.parent, options.adapter)) {
      throw new GitHubAdapterError(
        `parent issue #${snapshot.parent.number} belongs to ${snapshot.parent.owner}/${snapshot.parent.repo}`,
      );
    }
    const trainMappings = authority.nodes
      .filter((row) => row.train === node.train)
      .map((row) => mappingByNode.get(row.id))
      .filter(Boolean);
    const protectedTrainMappings = trainMappings.filter(
      (row) => mappingPolicy(row, row.gh_issue) === "protected",
    );
    const parentMapping = [...mappingByNode.values()].find(
      (row) => row.gh_issue === snapshot.parent.number,
    );
    if (
      (parentMapping && mappingPolicy(parentMapping, parentMapping.gh_issue) === "protected") ||
      protectedTrainMappings.length > 0
    ) {
      parentSkipped = {
        gh_issue: snapshot.parent.number,
        reason: "protected-mapping",
      };
    } else {
      const parentSnapshot = options.adapter.getIssueProjectState(snapshot.parent.number);
      if (parentSnapshot.id !== snapshot.parent.id) {
        throw new GitHubAdapterError(
          `parent issue #${snapshot.parent.number} identity changed during Project status planning`,
        );
      }
      const expectedChildren = trainMappings.map((row) => row.gh_issue);
      const rolled = parentStatus(parentSnapshot, mapping.gh_issue, status, expectedChildren, {
        owner: options.adapter.owner,
        repo: options.adapter.repo,
      });
      parent = itemPlan(snapshot.parent.number, parentSnapshot.item, rolled);
    }
  }
  const report = {
    mode,
    ok: true,
    node_id: nodeId,
    item,
    parent,
    ...(parentSkipped == null ? {} : { parent_skipped: parentSkipped }),
  };
  if (mode === "check") return report;

  const succeeded = [];
  try {
    options.adapter.setIssueProjectStatus({
      issueNumber: mapping.gh_issue,
      status,
      mode: "apply",
      clearance: options.clearance,
      owner: options.owner,
      repo: options.repo,
    });
    succeeded.push({ node_id: nodeId, number: mapping.gh_issue, kind: "set-project-status" });
    if (parent) {
      options.adapter.setIssueProjectStatus({
        issueNumber: parent.gh_issue,
        status: parent.status,
        mode: "apply",
        clearance: options.clearance,
        owner: options.owner,
        repo: options.repo,
      });
      succeeded.push({ number: parent.gh_issue, kind: "set-parent-project-status" });
    }
  } catch (error) {
    if (succeeded.length === 0) throw error;
    throw new PartialFailureError({
      succeeded,
      failed: { operation: { node_id: nodeId, kind: "set-parent-project-status" }, error },
    });
  }
  return report;
}
