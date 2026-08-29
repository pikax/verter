export const MINIMAL_GITHUB_WORKFLOW = Object.freeze({
  kind: "MinimalGitHubWorkflow",
  sync_issues_available: true,
  steps: Object.freeze([
    Object.freeze({ command: "sync-issues" }),
    Object.freeze({ command: "create-pr" }),
    Object.freeze({ command: "review-summary" }),
    Object.freeze({ command: "ci-result" }),
    Object.freeze({ command: "finalize-ledger" }),
    Object.freeze({ command: "squash-land" }),
    Object.freeze({ command: "inspect" }),
    Object.freeze({ command: "schedule" }),
    Object.freeze({ command: "release-plan" }),
    Object.freeze({ command: "release-cut" }),
  ]),
});

export function workflowInventory() {
  return MINIMAL_GITHUB_WORKFLOW;
}
