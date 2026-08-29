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
import {
  labelsForNode,
  loadIssueLabelCatalog,
  planIssueLabels,
  planRepositoryLabels,
} from "./issue-labels.mjs";
import {
  loadIssueMilestoneCatalog,
  milestoneForNode,
  planRepositoryMilestones,
} from "./issue-milestones.mjs";
import { appendGitHubIssueMapping, assertSyncAncestors, loadLedgerFile } from "./ledger-write.mjs";

export { githubIssueByNumber as lookupIssueMapping };

const SYNC_NODE_ID = "GH2";

function requiredSyncAncestors(authority, explicit) {
  if (explicit != null) {
    if (!Array.isArray(explicit) || explicit.some((row) => typeof row !== "string" || !row)) {
      throw new IssueSyncError("syncPrerequisites must be an array of node ids");
    }
    return [...explicit];
  }
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
    label_catalog: {
      missing: [],
      drift: [],
      current: [],
      created: [],
      updated: [],
    },
    milestone_catalog: {
      missing: [],
      drift: [],
      current: [],
      created: [],
      updated: [],
    },
  };
}

function renderNode(node, options, authority) {
  return renderIssueDescription({
    nodeId: node.id,
    authority,
    contentCatalog: options.issueContentCatalog,
  });
}

function desiredDependencyPlan(node, mappingByNode) {
  const desired = [];
  const protectedPredecessors = [];
  const unmappedPredecessors = [];
  for (const predecessor of node.predecessors) {
    const predecessorMapping = mappingByNode.get(predecessor);
    if (!predecessorMapping) {
      unmappedPredecessors.push(predecessor);
    } else if (predecessorMapping.sync_to_github === false) {
      protectedPredecessors.push(predecessor);
    } else {
      desired.push({ node_id: predecessor, mapping: predecessorMapping });
    }
  }
  return { desired, protectedPredecessors, unmappedPredecessors };
}

function compareIssueIdentity(left, right) {
  const owner = left.owner.toLowerCase().localeCompare(right.owner.toLowerCase());
  if (owner !== 0) return owner;
  const repo = left.repo.toLowerCase().localeCompare(right.repo.toLowerCase());
  if (repo !== 0) return repo;
  if (left.id !== right.id) return left.id - right.id;
  return left.number - right.number;
}

function issueIdentityKey(row) {
  return `${row.owner.toLowerCase()}/${row.repo.toLowerCase()}#${row.id}`;
}

function relationPlan({ node, mapping, adapter, mappingByNode, milestoneCatalog, currentIssue }) {
  const milestone = milestoneForNode(node, milestoneCatalog);
  const dependency = desiredDependencyPlan(node, mappingByNode);
  if (!mapping) {
    return {
      milestone,
      milestoneChanged: milestone != null,
      dependencies: [],
      addDependencies: dependency.desired,
      removeDependencies: [],
      ...dependency,
    };
  }
  const dependencies = [...adapter.getIssueDependencies(mapping.gh_issue)].sort(
    compareIssueIdentity,
  );
  const desired = dependency.desired
    .map((row) => ({
      ...row,
      identity: adapter.getIssueIdentity(row.mapping.gh_issue),
    }))
    .sort((left, right) => compareIssueIdentity(left.identity, right.identity));
  const desiredIds = new Set(desired.map((row) => issueIdentityKey(row.identity)));
  const mappingByIssue = new Map([...mappingByNode.values()].map((row) => [row.gh_issue, row]));
  const currentIds = new Set(dependencies.map(issueIdentityKey));
  const addDependencies = desired.filter((row) => !currentIds.has(issueIdentityKey(row.identity)));
  const removeDependencies = dependencies.filter((row) => {
    if (
      row.owner.toLowerCase() !== adapter.owner.toLowerCase() ||
      row.repo.toLowerCase() !== adapter.repo.toLowerCase()
    ) {
      return false;
    }
    const blockerMapping = mappingByIssue.get(row.number);
    return blockerMapping?.sync_to_github === true && !desiredIds.has(issueIdentityKey(row));
  });
  return {
    milestone,
    milestoneChanged: milestone != null && currentIssue?.milestone !== milestone.title,
    dependencies,
    addDependencies,
    removeDependencies,
    mappingByIssue,
    ...dependency,
    desired,
  };
}

function issuePlan({
  node,
  mapping,
  adapter,
  catalog,
  milestoneCatalog,
  mappingByNode,
  authority,
  options,
}) {
  const desiredLabels = labelsForNode(node, catalog);
  if (!mapping) {
    const relations = relationPlan({
      node,
      mapping,
      adapter,
      mappingByNode,
      milestoneCatalog,
      currentIssue: null,
    });
    const planned = {
      kind: "missing",
      node,
      desiredLabels,
      rendered: renderNode(node, options, authority),
      relations,
    };
    return planned;
  }
  if (mapping.sync_to_github === false) return { kind: "protected", node, mapping };

  const issue = readMappedIssue(adapter, mapping.gh_issue);
  const currentLabels = adapter.getIssueLabels(mapping.gh_issue);
  const labels = planIssueLabels(currentLabels, desiredLabels, catalog);
  let rendered = null;
  let contentChanged = false;
  if (options.refreshContent === true) {
    rendered = renderNode(node, options, authority);
    contentChanged = issue.title !== rendered.title || issue.body !== rendered.body;
  }
  const relations = relationPlan({
    node,
    mapping,
    adapter,
    mappingByNode,
    milestoneCatalog,
    currentIssue: issue,
  });
  const relationChanged =
    relations.milestoneChanged ||
    relations.addDependencies.length > 0 ||
    relations.removeDependencies.length > 0;
  return {
    kind: labels.changed || contentChanged || relationChanged ? "drift" : "current",
    node,
    mapping,
    issue,
    desiredLabels,
    labels,
    rendered,
    contentChanged,
    relations,
  };
}

function reportIssuePlan(report, plan) {
  if (plan.kind === "missing") {
    report.missing.push({
      node_id: plan.node.id,
      labels: plan.desiredLabels,
      content_required: true,
      milestone: plan.relations.milestone?.title ?? null,
      blocked_by: plan.relations.desired.map((row) => row.mapping.gh_issue),
      blocked_by_unmapped: plan.relations.unmappedPredecessors,
      blocked_by_protected: plan.relations.protectedPredecessors,
    });
  } else if (plan.kind === "protected") {
    report.protected.push({ node_id: plan.node.id, gh_issue: plan.mapping.gh_issue });
  } else if (plan.kind === "current") {
    report.current.push({
      node_id: plan.node.id,
      gh_issue: plan.mapping.gh_issue,
      blocked_by_unmapped: plan.relations.unmappedPredecessors,
      blocked_by_protected: plan.relations.protectedPredecessors,
    });
  } else {
    report.drift.push({
      node_id: plan.node.id,
      gh_issue: plan.mapping.gh_issue,
      content: plan.contentChanged,
      add_labels: plan.labels.add,
      remove_labels: plan.labels.remove,
      milestone: plan.relations.milestoneChanged ? (plan.relations.milestone?.title ?? null) : null,
      add_blocked_by: plan.relations.addDependencies.map((row) => row.mapping.gh_issue),
      remove_blocked_by: plan.relations.removeDependencies.map((row) => row.number),
      blocked_by_unmapped: plan.relations.unmappedPredecessors,
      blocked_by_protected: plan.relations.protectedPredecessors,
    });
  }
}

function reportRepositoryPlan(report, plan) {
  report.label_catalog.missing = plan.missing.map((label) => label.name);
  report.label_catalog.drift = plan.drift.map((row) => row.desired.name);
  report.label_catalog.current = [...plan.current];
}

function reportMilestoneRepositoryPlan(report, plan) {
  report.milestone_catalog.missing = plan.missing.map((row) => row.title);
  report.milestone_catalog.drift = plan.drift.map((row) => row.desired.title);
  report.milestone_catalog.current = [...plan.current];
}

function applyRepositoryPlan(adapter, clearance, plan, report, succeeded) {
  const apply = (operation, mutate, record) => {
    try {
      mutate();
      succeeded.push(operation);
      record();
    } catch (error) {
      if (succeeded.length === 0) throw error;
      throw new PartialFailureError({ succeeded, failed: { operation, error } });
    }
  };
  for (const label of plan.missing) {
    apply(
      { kind: "create-repository-label", label: label.name },
      () => adapter.createRepositoryLabel({ label, mode: "apply", clearance }),
      () => report.label_catalog.created.push(label.name),
    );
  }
  for (const row of plan.drift) {
    apply(
      { kind: "update-repository-label", label: row.desired.name },
      () =>
        adapter.updateRepositoryLabel({
          existing: row.existing,
          label: row.desired,
          mode: "apply",
          clearance,
        }),
      () => report.label_catalog.updated.push(row.desired.name),
    );
  }
}

function applyMilestoneRepositoryPlan(adapter, clearance, plan, report, succeeded) {
  const apply = (operation, mutate, record) => {
    try {
      mutate();
      succeeded.push(operation);
      record();
    } catch (error) {
      if (succeeded.length === 0) throw error;
      throw new PartialFailureError({ succeeded, failed: { operation, error } });
    }
  };
  for (const milestone of plan.missing) {
    apply(
      { kind: "create-repository-milestone", milestone: milestone.title },
      () =>
        adapter.createRepositoryMilestone({
          milestone,
          mode: "apply",
          clearance,
        }),
      () => report.milestone_catalog.created.push(milestone.title),
    );
  }
  for (const row of plan.drift) {
    apply(
      { kind: "update-repository-milestone", milestone: row.desired.title },
      () =>
        adapter.updateRepositoryMilestone({
          existing: row.existing,
          milestone: row.desired,
          mode: "apply",
          clearance,
        }),
      () => report.milestone_catalog.updated.push(row.desired.title),
    );
  }
}

function applyIssueLabels(adapter, plan, clearance, onApplied = () => {}) {
  if (plan.labels.add.length > 0) {
    adapter.addIssueLabels({
      number: plan.mapping.gh_issue,
      labels: plan.labels.add,
      mapping: plan.mapping,
      mode: "apply",
      clearance,
    });
    onApplied("add-issue-labels");
  }
  for (const label of plan.labels.remove) {
    adapter.removeIssueLabel({
      number: plan.mapping.gh_issue,
      label,
      mapping: plan.mapping,
      mode: "apply",
      clearance,
    });
    onApplied("remove-issue-label");
  }
}

function applyIssueRelations({ adapter, mapping, relations, clearance, nodeId, succeeded }) {
  if (relations.milestoneChanged) {
    adapter.setIssueMilestone({
      issueNumber: mapping.gh_issue,
      title: relations.milestone.title,
      mapping,
      mode: "apply",
      clearance,
    });
    succeeded.push(
      mutationRecord({
        nodeId,
        ghIssue: mapping.gh_issue,
        kind: "set-milestone",
        mappingWritten: true,
      }),
    );
  }
  for (const row of relations.addDependencies) {
    adapter.addIssueDependency({
      number: mapping.gh_issue,
      blockingNumber: row.mapping.gh_issue,
      blockingId: row.identity.id,
      mapping,
      blockingMapping: row.mapping,
      mode: "apply",
      clearance,
    });
    succeeded.push(
      mutationRecord({
        nodeId,
        ghIssue: mapping.gh_issue,
        kind: "add-issue-dependency",
        mappingWritten: true,
      }),
    );
  }
  for (const row of relations.removeDependencies) {
    const blockingMapping = relations.mappingByIssue?.get(row.number);
    if (!blockingMapping) continue;
    adapter.removeIssueDependency({
      number: mapping.gh_issue,
      blockingNumber: row.number,
      blockingId: row.id,
      mapping,
      blockingMapping,
      mode: "apply",
      clearance,
    });
    succeeded.push(
      mutationRecord({
        nodeId,
        ghIssue: mapping.gh_issue,
        kind: "remove-issue-dependency",
        mappingWritten: true,
      }),
    );
  }
}

export function syncIssues(options) {
  const mode = assertMutationMode(options.mode);
  if (!options.adapter) throw new IssueSyncError("adapter is required");
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
  assertSyncAncestors(ledger, requiredSyncAncestors(authority, options.syncPrerequisites));
  const mappingByNode = new Map(ledger.github_issue.map((row) => [row.node_id, row]));
  const report = emptyReport(
    mode,
    selected.map((node) => node.id),
  );
  if (mode === "apply" && !options.clearance) {
    throw new DoctorRequiredError("apply requires GitHubDoctor issues clearance");
  }
  const catalog = options.labelCatalog ?? loadIssueLabelCatalog(authority.packageRoot);
  const milestoneCatalog =
    options.milestoneCatalog ?? loadIssueMilestoneCatalog(authority.packageRoot);
  const plans = selected.map((node) =>
    issuePlan({
      node,
      mapping: mappingByNode.get(node.id),
      adapter: options.adapter,
      catalog,
      milestoneCatalog,
      mappingByNode,
      authority,
      options: { ...options, mode },
    }),
  );
  for (const plan of plans) reportIssuePlan(report, plan);
  const repositoryPlan = planRepositoryLabels(options.adapter.getRepositoryLabels(), catalog);
  const milestoneRepositoryPlan = planRepositoryMilestones(
    options.adapter.getRepositoryMilestones(),
    milestoneCatalog,
  );
  reportRepositoryPlan(report, repositoryPlan);
  reportMilestoneRepositoryPlan(report, milestoneRepositoryPlan);
  if (mode === "check") return report;
  const succeeded = [];
  applyRepositoryPlan(options.adapter, options.clearance, repositoryPlan, report, succeeded);
  applyMilestoneRepositoryPlan(
    options.adapter,
    options.clearance,
    milestoneRepositoryPlan,
    report,
    succeeded,
  );
  for (const plan of plans) {
    const node = plan.node;
    try {
      if (plan.kind === "missing") {
        const created = options.adapter.createIssue({
          title: plan.rendered.title,
          body: plan.rendered.body,
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
        const mapping = mappingByNode.get(node.id);
        const createdPlan = {
          mapping,
          labels: { add: plan.desiredLabels, remove: [] },
        };
        applyIssueLabels(options.adapter, createdPlan, options.clearance, (kind) => {
          succeeded.push(
            mutationRecord({
              nodeId: node.id,
              ghIssue: created.number,
              kind,
              mappingWritten: true,
            }),
          );
        });
        continue;
      }
      if (plan.kind === "protected" || plan.kind === "current") continue;
      if (plan.contentChanged) {
        options.adapter.updateIssue({
          number: plan.mapping.gh_issue,
          title: plan.rendered.title,
          body: plan.rendered.body,
          mapping: plan.mapping,
          mode: "apply",
          clearance: options.clearance,
        });
        succeeded.push(
          mutationRecord({
            nodeId: node.id,
            ghIssue: plan.mapping.gh_issue,
            kind: "update-issue",
            mappingWritten: true,
          }),
        );
      }
      applyIssueLabels(options.adapter, plan, options.clearance, (kind) => {
        succeeded.push(
          mutationRecord({
            nodeId: node.id,
            ghIssue: plan.mapping.gh_issue,
            kind,
            mappingWritten: true,
          }),
        );
      });
      report.updated.push({
        node_id: node.id,
        gh_issue: plan.mapping.gh_issue,
        content: plan.contentChanged,
        labels: plan.labels.changed,
      });
    } catch (error) {
      if (succeeded.length === 0) throw error;
      throw new PartialFailureError({
        succeeded,
        failed: { operation: { node_id: node.id }, error },
      });
    }
  }
  for (const plan of plans) {
    if (plan.kind === "protected") continue;
    const mapping = mappingByNode.get(plan.node.id);
    if (!mapping) continue;
    try {
      const currentIssue = readMappedIssue(options.adapter, mapping.gh_issue);
      const relations = relationPlan({
        node: plan.node,
        mapping,
        adapter: options.adapter,
        mappingByNode,
        milestoneCatalog,
        currentIssue,
      });
      applyIssueRelations({
        adapter: options.adapter,
        mapping,
        relations,
        clearance: options.clearance,
        nodeId: plan.node.id,
        succeeded,
      });
    } catch (error) {
      if (succeeded.length === 0) throw error;
      throw new PartialFailureError({
        succeeded,
        failed: { operation: { node_id: plan.node.id, kind: "sync-issue-relations" }, error },
      });
    }
  }
  return report;
}
