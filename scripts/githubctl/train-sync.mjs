import { classifyIssueSubIssueState, parseIssuePayload } from "./adapter.mjs";
import { IssueSyncError, NotFoundError } from "./errors.mjs";
import { labelsForTrain, planIssueLabels } from "./issue-labels.mjs";
import { appendGitHubTrainMapping } from "./ledger-write.mjs";
import { renderTrainIssueDescription, trainIssueForTrain } from "./train-issues.mjs";

function readIssue(adapter, number) {
  let payload;
  try {
    payload = adapter.getIssue(number);
  } catch (error) {
    if (error instanceof NotFoundError) payload = null;
    else throw error;
  }
  if (payload == null) throw new IssueSyncError(`mapped train issue #${number} is missing`);
  return parseIssuePayload(payload, number);
}

function parentMappingPolicy(mapping) {
  return mapping == null ? null : { ...mapping, sync_to_github: true };
}

function projectDisposition(snapshot) {
  if (!snapshot.item) return "missing";
  if (snapshot.item.status == null) return "needs-todo";
  return "current";
}

export function ensureIssueProjectTodo(adapter, issueNumber, clearance, onApplied = () => {}) {
  let snapshot = adapter.getIssueProjectState(issueNumber);
  let added = false;
  if (!snapshot.item) {
    const result = adapter.addIssueToProject({
      number: 3,
      issueNumber,
      mode: "apply",
      clearance,
    });
    added = result.already_member !== true;
    if (added) onApplied("add-project-item");
    snapshot = adapter.getIssueProjectState(issueNumber);
  }
  let initialized = false;
  if (snapshot.item?.status == null) {
    adapter.setIssueProjectStatus({
      number: 3,
      issueNumber,
      status: "Todo",
      mode: "apply",
      clearance,
    });
    initialized = true;
    onApplied("set-project-todo");
  }
  return { issueNumber, added, initialized };
}

function milestoneForTrainIssue(row, milestoneCatalog) {
  const milestone =
    milestoneCatalog.byTitle?.get(row.gh_milestone) ??
    milestoneCatalog.milestones.find((candidate) => candidate.title === row.gh_milestone);
  if (!milestone) {
    throw new IssueSyncError(`${row.train}: unknown gh_milestone ${row.gh_milestone}`);
  }
  return milestone;
}

function activeNodesByTrain(authority, completedNodeIds, selectedTrains) {
  const byTrain = new Map([...selectedTrains].map((train) => [train, []]));
  for (const node of authority.nodes) {
    if (!byTrain.has(node.train)) continue;
    if (node.semantic_role === "history" || completedNodeIds.has(node.id)) continue;
    byTrain.get(node.train).push(node);
  }
  return byTrain;
}

export function planTrainSync({
  authority,
  adapter,
  ledger,
  mappingByNode,
  selected,
  completedNodeIds,
  currentIssueByNode,
  trainCatalog,
  labelCatalog,
  milestoneCatalog,
  refreshContent,
  plannedCreationIds,
}) {
  const selectedTrains = new Set(selected.map((node) => node.train));
  const nodesByTrain = activeNodesByTrain(authority, completedNodeIds, selectedTrains);
  const mappingByTrain = new Map(ledger.github_train_issue.map((row) => [row.train, row]));
  const report = {
    missing: [],
    drift: [],
    current: [],
    closed: [],
    created: [],
    updated: [],
    project_missing: [],
    project_needs_todo: [],
    project_current: [],
    sub_issues_missing: [],
    sub_issues_current: [],
    sub_issues_conflict: [],
  };
  const plans = [];
  for (const train of selectedTrains) {
    const content = trainIssueForTrain(train, trainCatalog);
    const rendered = renderTrainIssueDescription(content);
    const desiredLabels = labelsForTrain(content, labelCatalog);
    const milestone = milestoneForTrainIssue(content, milestoneCatalog);
    const mapping = mappingByTrain.get(train) ?? null;
    let issue = null;
    let labels = { add: desiredLabels, remove: [], changed: desiredLabels.length > 0 };
    let contentChanged = true;
    let milestoneChanged = true;
    let project = "missing";
    let parentSnapshot = null;
    if (mapping) {
      issue = readIssue(adapter, mapping.gh_issue);
      if (issue.state === "closed") {
        report.closed.push({ train, gh_issue: mapping.gh_issue });
        plans.push({ kind: "closed", train, mapping, content, nodes: nodesByTrain.get(train) });
        continue;
      }
      labels = planIssueLabels(
        adapter.getIssueLabels(mapping.gh_issue),
        desiredLabels,
        labelCatalog,
      );
      contentChanged =
        refreshContent === true && (issue.title !== rendered.title || issue.body !== rendered.body);
      milestoneChanged = issue.milestone !== milestone.title;
      parentSnapshot = adapter.getIssueProjectState(mapping.gh_issue);
      project = projectDisposition(parentSnapshot);
    }
    const childPlans = [];
    for (const node of nodesByTrain.get(train)) {
      const childMapping = mappingByNode.get(node.id);
      if (!childMapping || childMapping.sync_to_github === false) {
        if (plannedCreationIds.has(node.id)) childPlans.push({ kind: "planned", node });
        continue;
      }
      const childIssue =
        currentIssueByNode.get(node.id) ?? readIssue(adapter, childMapping.gh_issue);
      if (childIssue.state === "closed") continue;
      const childSnapshot = adapter.getIssueProjectState(childMapping.gh_issue);
      const childProject = projectDisposition(childSnapshot);
      if (childProject === "missing") report.project_missing.push(childMapping.gh_issue);
      else if (childProject === "needs-todo") {
        report.project_needs_todo.push(childMapping.gh_issue);
      } else report.project_current.push(childMapping.gh_issue);
      if (mapping && childSnapshot.parent?.number === mapping.gh_issue) {
        const state = classifyIssueSubIssueState(
          parentSnapshot,
          childSnapshot,
          mapping.gh_issue,
          childMapping.gh_issue,
          { owner: adapter.owner, repo: adapter.repo },
        );
        childPlans.push({
          kind: state === "unchanged" ? "current" : "attach",
          node,
          mapping: childMapping,
        });
        report.sub_issues_current.push({
          train,
          node_id: node.id,
          gh_issue: childMapping.gh_issue,
        });
      } else if (childSnapshot.parent == null) {
        if (mapping) {
          classifyIssueSubIssueState(
            parentSnapshot,
            childSnapshot,
            mapping.gh_issue,
            childMapping.gh_issue,
            { owner: adapter.owner, repo: adapter.repo },
          );
        }
        childPlans.push({ kind: "attach", node, mapping: childMapping });
        report.sub_issues_missing.push({
          train,
          node_id: node.id,
          gh_issue: childMapping.gh_issue,
        });
      } else {
        childPlans.push({
          kind: "conflict",
          node,
          mapping: childMapping,
          currentParent: childSnapshot.parent.number,
        });
        report.sub_issues_conflict.push({
          train,
          node_id: node.id,
          gh_issue: childMapping.gh_issue,
          current_parent: childSnapshot.parent.number,
          expected_parent: mapping?.gh_issue ?? null,
        });
      }
    }
    const existingNativeChildren = parentSnapshot?.subIssues?.length ?? 0;
    const newNativeChildren = childPlans.filter(
      (row) => row.kind === "attach" || row.kind === "planned",
    ).length;
    if (existingNativeChildren + newNativeChildren > 100) {
      throw new IssueSyncError(`${train}: GitHub parent would exceed 100 native sub-issues`);
    }
    const changed =
      !mapping || labels.changed || contentChanged || milestoneChanged || project !== "current";
    const plan = {
      kind: mapping ? (changed ? "drift" : "current") : "missing",
      train,
      mapping,
      issue,
      content,
      rendered,
      desiredLabels,
      labels,
      milestone,
      contentChanged,
      milestoneChanged,
      project,
      parentSnapshot,
      nodes: nodesByTrain.get(train),
      childPlans,
    };
    plans.push(plan);
    const summary = {
      train,
      gh_issue: mapping?.gh_issue ?? null,
      labels: labels.changed,
      content: contentChanged,
      milestone: milestoneChanged ? milestone.title : null,
      project,
    };
    report[plan.kind].push(summary);
    if (project === "missing") report.project_missing.push(mapping?.gh_issue ?? `train:${train}`);
    else if (project === "needs-todo") {
      report.project_needs_todo.push(mapping.gh_issue);
    } else report.project_current.push(mapping.gh_issue);
  }
  return {
    plans,
    report,
    mappingByTrain,
    ok: report.sub_issues_conflict.length === 0,
  };
}

function applyLabels(adapter, mapping, labels, clearance, onApplied) {
  if (labels.add.length > 0) {
    adapter.addIssueLabels({
      number: mapping.gh_issue,
      labels: labels.add,
      mapping,
      mode: "apply",
      clearance,
    });
    onApplied("add-issue-labels");
  }
  for (const label of labels.remove) {
    adapter.removeIssueLabel({
      number: mapping.gh_issue,
      label,
      mapping,
      mode: "apply",
      clearance,
    });
    onApplied("remove-issue-label");
  }
}

export function applyTrainParents({
  plans,
  adapter,
  ledgerPath,
  mappingByTrain,
  clearance,
  report,
  succeeded,
}) {
  for (const plan of plans) {
    if (plan.kind === "closed") continue;
    let mapping = parentMappingPolicy(plan.mapping);
    if (!mapping) {
      const created = adapter.createIssue({
        title: plan.rendered.title,
        body: plan.rendered.body,
        mode: "apply",
        clearance,
      });
      const identity = {
        train: plan.train,
        gh_issue: created.number,
        kind: "create-train-issue",
        mapping_written: false,
      };
      succeeded.push(identity);
      const stored = appendGitHubTrainMapping(ledgerPath, {
        train: plan.train,
        gh_issue: created.number,
      });
      identity.mapping_written = true;
      mappingByTrain.set(plan.train, stored);
      mapping = parentMappingPolicy(stored);
      report.created.push({ train: plan.train, gh_issue: created.number, mapping_written: true });
      plan.labels = { add: plan.desiredLabels, remove: [], changed: true };
      plan.milestoneChanged = true;
      plan.project = "missing";
    } else if (plan.contentChanged) {
      adapter.updateIssue({
        number: mapping.gh_issue,
        title: plan.rendered.title,
        body: plan.rendered.body,
        mapping,
        mode: "apply",
        clearance,
      });
      succeeded.push({
        train: plan.train,
        gh_issue: mapping.gh_issue,
        kind: "update-train-issue",
        mapping_written: true,
      });
    }
    applyLabels(adapter, mapping, plan.labels, clearance, (kind) => {
      succeeded.push({
        train: plan.train,
        gh_issue: mapping.gh_issue,
        kind,
        mapping_written: true,
      });
    });
    if (plan.milestoneChanged) {
      adapter.setIssueMilestone({
        issueNumber: mapping.gh_issue,
        title: plan.milestone.title,
        mapping,
        mode: "apply",
        clearance,
      });
      succeeded.push({
        train: plan.train,
        gh_issue: mapping.gh_issue,
        kind: "set-milestone",
        mapping_written: true,
      });
    }
    const projected = ensureIssueProjectTodo(adapter, mapping.gh_issue, clearance, (kind) => {
      succeeded.push({
        train: plan.train,
        gh_issue: mapping.gh_issue,
        kind,
        mapping_written: true,
      });
    });
    report.updated.push({
      train: plan.train,
      gh_issue: mapping.gh_issue,
      content: plan.contentChanged,
      labels: plan.labels.changed,
      milestone: plan.milestoneChanged,
      project_added: projected.added,
      todo_initialized: projected.initialized,
    });
  }
}

export function applyTrainChildren({
  plans,
  adapter,
  mappingByTrain,
  mappingByNode,
  clearance,
  report,
  succeeded,
}) {
  for (const plan of plans) {
    if (plan.kind === "closed") continue;
    const storedParent = mappingByTrain.get(plan.train);
    if (!storedParent) continue;
    const parentMapping = parentMappingPolicy(storedParent);
    for (const node of plan.nodes) {
      const mapping = mappingByNode.get(node.id);
      if (!mapping || mapping.sync_to_github === false) continue;
      const issue = readIssue(adapter, mapping.gh_issue);
      if (issue.state === "closed") continue;
      const projected = ensureIssueProjectTodo(adapter, mapping.gh_issue, clearance, (kind) => {
        succeeded.push({
          train: plan.train,
          node_id: node.id,
          gh_issue: mapping.gh_issue,
          kind,
          mapping_written: true,
        });
      });
      const attached = adapter.addIssueSubIssue({
        parentIssueNumber: parentMapping.gh_issue,
        subIssueNumber: mapping.gh_issue,
        parentMapping,
        subIssueMapping: mapping,
        mode: "apply",
        clearance,
      });
      if (attached.unchanged !== true) {
        succeeded.push({
          train: plan.train,
          node_id: node.id,
          gh_issue: mapping.gh_issue,
          kind: "add-issue-sub-issue",
          mapping_written: true,
        });
      }
      report.updated.push({
        train: plan.train,
        node_id: node.id,
        gh_issue: mapping.gh_issue,
        project_added: projected.added,
        todo_initialized: projected.initialized,
        sub_issue_added: attached.unchanged !== true,
      });
    }
  }
}
