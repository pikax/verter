import { deriveState, loadAuthority } from "../../roadmap/0.1.0-tama/tools/lib.mjs";
import { PROJECT_NUMBER, PROJECT_VIEWS, assertMutationMode } from "./adapter.mjs";
import {
  DoctorRequiredError,
  GitHubAdapterError,
  MissingIssueMappingError,
  NonReadyNodeError,
} from "./errors.mjs";
import { loadLedgerFile } from "./ledger-write.mjs";
import { selectNodes } from "./sync-issues.mjs";

function overlayMilestone(issue) {
  return typeof issue?.milestone === "string" && issue.milestone.length > 0
    ? issue.milestone
    : null;
}

export function schedule(options) {
  const mode = assertMutationMode(options.mode);
  if (!options.adapter) throw new GitHubAdapterError("adapter is required");
  const authority = options.authority ?? loadAuthority();
  const selectedAll = selectNodes(authority, options);
  const ledgerPath = options.ledgerPath ?? authority.ledgerFile;
  const ledger = loadLedgerFile(ledgerPath);
  const state = deriveState(authority, { implemented: ledger.implemented });
  const mappingByNode = new Map(ledger.github_issue.map((row) => [row.node_id, row]));
  const setMilestone =
    typeof options.setMilestone === "string" && options.setMilestone.length > 0
      ? options.setMilestone
      : null;
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
  options.adapter.getProject(PROJECT_NUMBER);
  const items = selected.map((node) => {
    const mapping = mappingByNode.get(node.id);
    return {
      node_id: node.id,
      gh_issue: mapping.gh_issue,
      project: PROJECT_NUMBER,
      status: "READY",
      milestone: overlayMilestone(options.adapter.getIssue(mapping.gh_issue)),
      already_member: false,
    };
  });
  const plan = {
    mode,
    ok: true,
    project: { number: PROJECT_NUMBER },
    views: PROJECT_VIEWS,
    selection: selected.map((node) => node.id),
    items,
    release_target: setMilestone ? { title: setMilestone, instructed: true } : null,
  };
  if (mode === "check") return plan;
  if (!options.clearance) {
    throw new DoctorRequiredError("apply requires GitHubDoctor projects clearance");
  }
  for (let index = 0; index < selected.length; index += 1) {
    const mapping = mappingByNode.get(selected[index].id);
    const added = options.adapter.addIssueToProject({
      number: PROJECT_NUMBER,
      issueNumber: mapping.gh_issue,
      mode: "apply",
      clearance: options.clearance,
    });
    if (typeof added.already_member === "boolean") {
      plan.items[index].already_member = added.already_member;
    } else {
      delete plan.items[index].already_member;
    }
    if (setMilestone) {
      options.adapter.setIssueMilestone({
        issueNumber: mapping.gh_issue,
        title: setMilestone,
        mode: "apply",
        clearance: options.clearance,
      });
    }
  }
  return plan;
}
