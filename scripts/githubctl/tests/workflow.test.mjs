import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  BlockingFindingError,
  FakeGitHubAdapter,
  GitHubDoctor,
  MINIMAL_GITHUB_WORKFLOW,
  PROJECT_NUMBER,
  UnauthorizedReleaseError,
  ciResult,
  countAiGeneratedFooters,
  createPr,
  finalizeLedger,
  inspectIssue,
  mappedClosingLink,
  releaseCut,
  releasePlan,
  reviewSummary,
  schedule,
  squashLand,
  syncIssues,
  workflowInventory,
} from "../index.mjs";
import { parseToml } from "../../../roadmap/0.1.0-tama/tools/lib.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const CLI = path.join(HERE, "../githubctl.mjs");
const CONTRACT = path.join(REPO_ROOT, "roadmap/0.1.0-tama/contracts/github-control-plane.md");
const LIVE_LEDGER = path.join(REPO_ROOT, "roadmap/0.1.0-tama/authority/state/implemented.toml");
const TITLE = "feat(ci): example final title";
const HEAD = "train/workflow-example";
const DATE = "2026-08-29T21:15:00+01:00";
const TAMA_ROADMAP = "Tama Roadmap";
const MILESTONE = "v0.1.0";
const RELEASE_VERSION = "0.1.0";
const WORKFLOW_COMMANDS = [
  "sync-issues",
  "project-status",
  "create-pr",
  "review-summary",
  "ci-result",
  "finalize-ledger",
  "squash-land",
  "inspect",
  "schedule",
  "release-plan",
  "release-cut",
];

// Composed-workflow inventory does not own cache, incremental, or warm-admission authority.
// githubctl is occasional CLI coordination, not a hot parse/resolve path.

function fake(options = {}) {
  return new FakeGitHubAdapter({
    owner: "pikax",
    repo: "verter",
    milestones: [{ title: MILESTONE, number: 1 }],
    ...options,
  });
}

function clearanceFor(adapter, require = ["issues", "pullRequests", "projects", "actions"]) {
  const report = new GitHubDoctor(adapter).check({ require });
  assert.equal(report.ok, true, report.errors?.join?.("; ") ?? "doctor failed");
  return report.clearance;
}

function implementedBlock(id) {
  return `[[implemented]]
node_id = "${id}"
commit_message = "test locator ${id}"
commit_date = "2026-08-28T00:00:00+00:00"
`;
}

function mappingBlock(nodeId, issue, syncToGithub) {
  return `[[github_issue]]
node_id = "${nodeId}"
gh_issue = ${issue}
sync_to_github = ${syncToGithub}
`;
}

function writeLedger(options = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-workflow-"));
  const file = path.join(dir, "implemented.toml");
  const implemented =
    options.implemented ??
    parseToml(fs.readFileSync(LIVE_LEDGER, "utf8")).implemented.map((row) => row.node_id);
  const issues = options.issues ?? [];
  const parts = ["schema = 1", "", ...implemented.map(implementedBlock)];
  for (const row of issues) parts.push(mappingBlock(row.node_id, row.gh_issue, row.sync_to_github));
  fs.writeFileSync(file, parts.join("\n"));
  return file;
}

function readLedger(file) {
  return parseToml(fs.readFileSync(file, "utf8"));
}

test("check prints the frozen composed-workflow inventory and keeps issue-sync available", () => {
  assert.equal(MINIMAL_GITHUB_WORKFLOW, workflowInventory());
  assert.equal(workflowInventory().kind, "MinimalGitHubWorkflow");
  assert.equal(workflowInventory().sync_issues_available, true);
  assert.deepEqual(
    workflowInventory().steps.map((step) => step.command),
    WORKFLOW_COMMANDS,
  );
  const contract = fs.readFileSync(CONTRACT, "utf8");
  for (const command of WORKFLOW_COMMANDS) {
    assert.match(contract, new RegExp(`githubctl ${command}\\b`, "u"), command);
  }
  const help = spawnSync(process.execPath, [CLI, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /\bsync-issues\b/u);
  const check = spawnSync(process.execPath, [CLI, "check"], { encoding: "utf8" });
  assert.equal(check.status, 0, check.stderr);
  const printed = JSON.parse(check.stdout);
  assert.deepEqual(printed, workflowInventory());
  assert.equal(printed.sync_issues_available, true);
});

test("one fake adapter walks issue mapping through squash landing, feedback, and release rehearsal", () => {
  const adapter = fake();
  const clearance = clearanceFor(adapter);
  const ledgerPath = writeLedger({ implemented: ["sync-capability"] });
  const reportDir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-workflow-reports-"));

  const missing = syncIssues({
    adapter,
    mode: "check",
    nodes: ["GH0", "B4R0"],
    ledgerPath,
    projectIssues: false,
    syncTrainParents: false,
    syncPrerequisites: [],
    ignoreBlockers: true,
  });
  assert.deepEqual(missing.missing.map((row) => row.node_id).sort(), ["B4R0", "GH0"]);
  assert.deepEqual(missing.protected, []);

  const synced = syncIssues({
    adapter,
    mode: "apply",
    nodes: ["GH0", "B4R0"],
    ledgerPath,
    clearance,
    projectIssues: false,
    syncTrainParents: false,
    syncPrerequisites: [],
    ignoreBlockers: true,
  });
  assert.equal(synced.ok, true);
  assert.equal(synced.created.length, 2);
  assert.equal(
    synced.created.every((row) => row.mapping_written === true),
    true,
  );
  const gh0Issue = synced.created.find((row) => row.node_id === "GH0").gh_issue;
  const readyIssue = synced.created.find((row) => row.node_id === "B4R0").gh_issue;
  assert.notEqual(gh0Issue, readyIssue);
  assert.equal(adapter.getIssue(gh0Issue).number, gh0Issue);
  assert.equal(countAiGeneratedFooters(adapter.getIssue(gh0Issue).body), 1);
  assert.doesNotMatch(
    adapter.getIssue(gh0Issue).body,
    /^(?:implementation_|review_|verification_|confirmation_)?effort(?:_(?:min|default))?\s*[:=]/imu,
  );
  const scheduled = adapter.addIssueToProject({
    number: 3,
    issueNumber: gh0Issue,
    mode: "apply",
    clearance,
  });
  assert.equal(scheduled.already_member, false);
  adapter.setIssueProjectStatus({
    issueNumber: gh0Issue,
    status: "Todo",
    mode: "apply",
    clearance,
  });

  const current = syncIssues({
    adapter,
    mode: "check",
    nodes: ["GH0", "B4R0"],
    ledgerPath,
    projectIssues: false,
    syncTrainParents: false,
    syncPrerequisites: [],
    ignoreBlockers: true,
  });
  assert.deepEqual(current.current.map((row) => row.node_id).sort(), ["B4R0", "GH0"]);
  assert.deepEqual(current.missing, []);
  assert.deepEqual(current.drift, []);

  const implementationNode = synced.created.find((row) => row.gh_issue === gh0Issue).node_id;
  const readyNode = synced.created.find((row) => row.gh_issue === readyIssue).node_id;
  fs.appendFileSync(ledgerPath, `\n${implementedBlock(implementationNode)}`);
  const present = new Set(readLedger(ledgerPath).implemented.map((row) => row.node_id));
  for (const row of parseToml(fs.readFileSync(LIVE_LEDGER, "utf8")).implemented) {
    if (row.node_id === readyNode || present.has(row.node_id)) continue;
    fs.appendFileSync(ledgerPath, `\n${implementedBlock(row.node_id)}`);
    present.add(row.node_id);
  }
  const implementedBefore = readLedger(ledgerPath).implemented.length;

  const opened = createPr({
    adapter,
    mode: "apply",
    node: "GH0",
    title: TITLE,
    head: HEAD,
    ledgerPath,
    writeLocator: true,
    clearance,
  });
  assert.equal(opened.ok, true);
  assert.equal(opened.gh_issue, gh0Issue);
  assert.equal(opened.pull_request.title, TITLE);
  assert.equal(opened.pull_request.body, `${mappedClosingLink(gh0Issue)}\n`);
  assert.equal(opened.issue.changed, false);
  assert.equal(opened.issue.applied, false);
  assert.equal(opened.locator.written, true);
  const pr = opened.pull_request.number;
  assert.equal(countAiGeneratedFooters(adapter.getIssue(gh0Issue).body), 1);
  assert.equal(adapter.getIssue(gh0Issue).number, gh0Issue);

  assert.throws(
    () =>
      reviewSummary({
        adapter,
        mode: "apply",
        node: "GH0",
        pr,
        verdict: "PASS",
        body: "Problem: blockers must stop accept.\nScope: review.\nValidation: fake.\nReview: refuse.",
        findings: [{ severity: "P0", owner: "reviewer", context: "must not accept" }],
        ledgerPath,
        clearance,
      }),
    BlockingFindingError,
  );
  assert.equal(adapter.getPullRequest(pr).comments?.length ?? 0, 0);

  const reviewed = reviewSummary({
    adapter,
    mode: "apply",
    node: "GH0",
    pr,
    verdict: "PASS",
    body: "Problem: mapping stays local.\nScope: ordinary PR prose.\nValidation: fake adapter.\nReview: human-written.",
    ledgerPath,
    clearance,
  });
  assert.equal(reviewed.ok, true);
  assert.equal(adapter.getPullRequest(pr).comments.length, 1);
  assert.match(adapter.getPullRequest(pr).comments[0].body, /^Verdict: PASS$/mu);

  adapter.setPullRequestCheckRuns(pr, [{ name: TAMA_ROADMAP, conclusion: "success" }]);
  const checks = ciResult({
    adapter,
    mode: "apply",
    pr,
    requiredJobs: [TAMA_ROADMAP],
    owner: "pikax",
    repo: "verter",
  });
  assert.equal(checks.ok, true);
  assert.deepEqual(checks.missing, []);
  assert.deepEqual(checks.unexpected_skips, []);

  const finalized = finalizeLedger({
    node: "GH0",
    message: TITLE,
    date: DATE,
    pr,
    ledgerPath,
  });
  assert.equal(finalized.written, true);
  assert.equal(finalized.pull_request, pr);
  const gh0Row = readLedger(ledgerPath).implemented.find((row) => row.node_id === "GH0");
  assert.equal(gh0Row.commit_message, TITLE);
  assert.equal(gh0Row.commit_date, DATE);
  assert.equal(gh0Row.pull_request, pr);

  const beforeMergeLedger = fs.readFileSync(ledgerPath, "utf8");
  const landed = squashLand({
    adapter,
    mode: "apply",
    pr,
    node: "GH0",
    requiredJobs: [TAMA_ROADMAP],
    ledgerPath,
    owner: "pikax",
    repo: "verter",
    clearance,
  });
  assert.equal(landed.ok, true);
  assert.equal(landed.applied, true);
  assert.equal(landed.merge_method, "squash");
  assert.deepEqual(adapter.inspectState().merges, [{ number: pr, merge_method: "squash" }]);
  assert.equal(adapter.getProjectStatus(gh0Issue), "Done");
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), beforeMergeLedger);
  assert.equal(readLedger(ledgerPath).implemented.length, implementedBefore);

  const feedbackIssue = adapter.createIssue({
    title: "unmapped product report",
    body: "a non-DAG issue",
    mode: "apply",
    clearance,
  }).number;
  const inspected = inspectIssue({
    adapter,
    mode: "apply",
    issue: feedbackIssue,
    verdict: "confirmed",
    ledgerPath,
    reportDir,
    inspectedAt: DATE,
    clearance,
  });
  assert.equal(inspected.ok, true);
  assert.equal(inspected.report_written, true);
  assert.equal(inspected.label_written, true);
  assert.deepEqual(adapter.getIssueLabels(feedbackIssue), ["ai:confirmed"]);
  assert.equal(readLedger(ledgerPath).implemented.length, implementedBefore);

  const overlay = schedule({
    adapter,
    mode: "apply",
    nodes: ["B4R0"],
    ledgerPath,
    clearance,
  });
  assert.equal(overlay.ok, true);
  assert.deepEqual(overlay.selection, ["B4R0"]);
  assert.equal(overlay.project.number, PROJECT_NUMBER);
  assert.deepEqual(
    adapter.getProjectItems().toSorted((left, right) => left - right),
    [gh0Issue, readyIssue].toSorted((left, right) => left - right),
  );
  assert.equal(adapter.getProjectStatus(readyIssue), "Todo");

  adapter.setIssueMilestone({
    issueNumber: readyIssue,
    title: MILESTONE,
    mode: "apply",
    clearance,
  });
  adapter.setIssueMilestone({
    issueNumber: gh0Issue,
    title: MILESTONE,
    mode: "apply",
    clearance,
  });
  const planned = releasePlan({
    adapter,
    mode: "apply",
    milestone: MILESTONE,
    ledgerPath,
  });
  assert.equal(planned.ok, false);
  assert.equal(
    planned.ready.some((row) => row.node_id === "GH0" && row.gh_issue === gh0Issue),
    true,
  );
  assert.equal(
    planned.blockers.some((row) => row.node_id === "B4R0" && row.reason === "unimplemented"),
    true,
  );

  assert.throws(
    () => releaseCut({ adapter, mode: "apply", version: RELEASE_VERSION, clearance }),
    UnauthorizedReleaseError,
  );
  const cut = releaseCut({
    adapter,
    mode: "apply",
    version: RELEASE_VERSION,
    authorize: true,
    clearance,
  });
  assert.equal(cut.ok, true);
  assert.equal(cut.title, `release: v${RELEASE_VERSION}`);
  assert.equal(cut.pull_request.closes, null);
  assert.doesNotMatch(cut.pull_request.body ?? "", /Closes/u);
  const releasePr = cut.pull_request.number;
  adapter.setPullRequestCheckRuns(releasePr, [{ name: TAMA_ROADMAP, conclusion: "success" }]);
  const released = releaseCut({
    adapter,
    mode: "apply",
    version: RELEASE_VERSION,
    authorize: true,
    land: true,
    pr: releasePr,
    requiredJobs: [TAMA_ROADMAP],
    clearance,
  });
  assert.equal(released.ok, true);
  assert.equal(released.landing.applied, true);
  assert.equal(released.landing.commit_title, `release: v${RELEASE_VERSION}`);
  assert.equal(readLedger(ledgerPath).implemented.length, implementedBefore);
  assert.equal(adapter.getIssue(gh0Issue).number, gh0Issue);
});

test("a protected mapping stays skipped and byte-for-byte untouched during issue-sync", () => {
  const adapter = fake({
    issues: [{ number: 7, title: "kept title", body: "kept body" }],
    nextNumber: 8,
  });
  const clearance = clearanceFor(adapter, ["issues"]);
  const ledgerPath = writeLedger({
    implemented: ["sync-capability"],
    issues: [{ node_id: "GH0", gh_issue: 7, sync_to_github: false }],
  });
  const report = syncIssues({
    adapter,
    mode: "apply",
    nodes: ["GH0"],
    ledgerPath,
    clearance,
    projectIssues: false,
    syncTrainParents: false,
    syncPrerequisites: [],
    ignoreBlockers: true,
  });
  assert.deepEqual(report.protected, [{ node_id: "GH0", gh_issue: 7 }]);
  assert.deepEqual(report.created, []);
  assert.deepEqual(report.updated, []);
  const issue = adapter.getIssue(7);
  assert.equal(issue.title, "kept title");
  assert.equal(issue.body, "kept body");
});
