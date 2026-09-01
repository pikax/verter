import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  FakeGitHubAdapter,
  GitHubDoctor,
  PartialFailureError,
  syncIssues,
  validateIssueContentCatalog,
  validateTrainIssueCatalog,
} from "../index.mjs";
import { ledgerText } from "./ledger-fixture.mjs";

const MILESTONE = "0.0.2";

function labelCatalog() {
  return {
    managedPrefixes: ["area:", "problem:", "framework:"],
    managedExact: ["origin:ai"],
    labels: [
      { name: "area:compiler", color: "000001", description: "Compiler" },
      { name: "problem:architecture", color: "000002", description: "Architecture" },
      { name: "problem:capability", color: "000003", description: "Capability" },
      { name: "framework:shared", color: "000004", description: "Shared" },
      { name: "origin:ai", color: "000005", description: "AI" },
    ],
    areaRules: [{ label: "area:compiler", trains: ["fixture.compiler"] }],
    problemTrainRules: [],
    problemKindRules: [{ label: "problem:capability", kinds: ["implementation"] }],
    frameworkRules: [{ label: "framework:shared", trains: ["fixture.compiler"] }],
  };
}

function milestoneCatalog() {
  const milestone = { title: MILESTONE, description: "Foundation" };
  return { milestones: [milestone], byTitle: new Map([[MILESTONE, milestone]]) };
}

function contentCatalog(ids = ["WORK"]) {
  return validateIssueContentCatalog({
    schema: 1,
    issue: ids.map((node_id) => ({
      node_id,
      title: `Publish deterministic output for ${node_id.toLowerCase()}`,
      problem:
        "Current compiler behavior can publish incomplete output before every required fact is available, which causes incorrect results and leaves consumers unable to distinguish current work from stale work.",
      expected_outcome:
        "The compiler publishes one deterministic result from a coherent input snapshot and preserves the provenance required by every downstream consumer.",
      acceptance: [
        "Supported inputs produce deterministic observable output.",
        "Incomplete or stale work cannot publish a successful result.",
        "Repeated execution preserves result identity and provenance.",
      ],
    })),
  });
}

function trainCatalog() {
  return validateTrainIssueCatalog({
    schema: 1,
    train_issue: [
      {
        train: "fixture.compiler",
        title: "Unify the fixture compiler",
        problem:
          "Compiler behavior is split across unrelated execution paths, which causes observable results to diverge and prevents maintainers from proving one coherent capability contract for supported consumers.",
        expected_outcome:
          "One compiler authority owns request identity, execution, and publication while preserving explicit boundaries for consumer-specific behavior.",
        acceptance: [
          "All supported requests use one compiler authority.",
          "Consumer-specific behavior remains isolated behind explicit contracts.",
          "Repeated execution produces coherent and deterministic results.",
        ],
        problem_label: "problem:architecture",
        gh_milestone: MILESTONE,
      },
    ],
  });
}

function node(id = "WORK") {
  return {
    id,
    name: id,
    train: "fixture.compiler",
    kind: "implementation",
    semantic_role: "delivery",
    predecessors: [],
    gh_milestone: MILESTONE,
  };
}

function trainLedgerText({ implemented = ["SYNC-READY"], children = ["WORK"], parent = 20 } = {}) {
  return ledgerText({
    implemented,
    issues: children.map((nodeId, index) => ({
      node_id: nodeId,
      gh_issue: 10 + index,
      sync_to_github: true,
    })),
    trains: [{ train: "fixture.compiler", gh_issue: parent }],
  });
}

function fixture(options = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-train-sync-"));
  const ledgerPath = path.join(dir, "implemented.toml");
  const nodes = options.nodes ?? [node()];
  fs.writeFileSync(
    ledgerPath,
    trainLedgerText({
      implemented: options.implemented,
      children: nodes.map((row) => row.id),
      parent: options.parentNumber ?? 20,
    }),
  );
  const parentNumber = options.parentNumber ?? 20;
  const parentSubIssues = options.parentSubIssues ?? [];
  const issues = [
    {
      number: parentNumber,
      title: "stale parent title",
      body: "stale parent body",
      labels: [],
      milestone: null,
      subIssues: parentSubIssues,
    },
    ...nodes.map((row, index) => ({
      number: 10 + index,
      title: row.id,
      body: "stable child body",
      labels: [],
      milestone: null,
      parent: options.childParent ?? null,
    })),
    ...parentSubIssues.map((number) => ({
      number,
      title: `Manual child ${number}`,
      body: "manual child",
      parent: parentNumber,
    })),
  ];
  if (
    options.childParent != null &&
    options.childParent !== parentNumber &&
    !issues.some((issue) => issue.number === options.childParent)
  ) {
    issues.push({ number: options.childParent, title: "manual parent", body: "manual" });
  }
  const adapter = new FakeGitHubAdapter({
    owner: "pikax",
    repo: "verter",
    issues,
    repositoryLabels: labelCatalog().labels,
    milestones: milestoneCatalog().milestones,
  });
  return {
    adapter,
    ledgerPath,
    authority: {
      nodes: [{ id: "SYNC-READY", train: "fixture.sync", predecessors: [] }, ...nodes],
      ledgerFile: path.join(dir, "live.toml"),
      packageRoot: dir,
    },
  };
}

function clearanceFor(adapter) {
  const doctor = new GitHubDoctor(adapter).check({ require: ["issues", "projects"] });
  assert.equal(doctor.ok, true, doctor.errors.join("; "));
  return doctor.clearance;
}

function run(fx, extra = {}) {
  return syncIssues({
    adapter: fx.adapter,
    authority: fx.authority,
    ledgerPath: fx.ledgerPath,
    nodes: ["WORK"],
    labelCatalog: labelCatalog(),
    milestoneCatalog: milestoneCatalog(),
    issueContentCatalog: contentCatalog(fx.authority.nodes.slice(1).map((row) => row.id)),
    trainCatalog: trainCatalog(),
    syncPrerequisites: [],
    ...extra,
  });
}

test("train sync projects parent and child as Todo and attaches the native sub-issue", () => {
  const fx = fixture();
  const checked = run(fx, { mode: "check" });
  assert.equal(checked.train_parents.drift[0].gh_issue, 20);
  assert.deepEqual(checked.train_parents.sub_issues_missing, [
    { train: "fixture.compiler", node_id: "WORK", gh_issue: 10 },
  ]);
  assert.deepEqual(checked.train_parents.project_missing.sort(), [10, 20]);

  run(fx, { mode: "apply", clearance: clearanceFor(fx.adapter) });
  assert.deepEqual(fx.adapter.getProjectItems(), [10, 20]);
  assert.equal(fx.adapter.getProjectStatus(10), "Todo");
  assert.equal(fx.adapter.getProjectStatus(20), "Todo");
  assert.equal(fx.adapter.getIssue(10).parent, 20);
  assert.deepEqual(fx.adapter.getIssue(20).subIssues, [10]);
  assert.equal(fx.adapter.getIssue(10).milestone, MILESTONE);
  assert.equal(fx.adapter.getIssue(20).milestone, MILESTONE);
  assert.deepEqual(fx.adapter.getIssueLabels(20), [
    "area:compiler",
    "problem:architecture",
    "framework:shared",
    "origin:ai",
  ]);
  assert.equal(fx.adapter.getIssue(20).body, "stale parent body");

  const repeated = run(fx, { mode: "check" });
  assert.deepEqual(repeated.drift, []);
  assert.deepEqual(repeated.train_parents.drift, []);
  assert.deepEqual(repeated.train_parents.sub_issues_missing, []);
  assert.deepEqual(repeated.train_parents.sub_issues_current, [
    { train: "fixture.compiler", node_id: "WORK", gh_issue: 10 },
  ]);
});

test("explicit refresh updates stable parent prose without resetting existing Project status", () => {
  const fx = fixture({ childParent: 20 });
  const clearance = clearanceFor(fx.adapter);
  run(fx, { mode: "apply", clearance });
  fx.adapter.setIssueProjectStatus({
    number: 3,
    issueNumber: 10,
    status: "In Progress",
    mode: "apply",
    clearance,
  });
  fx.adapter.setIssueProjectStatus({
    number: 3,
    issueNumber: 20,
    status: "In Progress",
    mode: "apply",
    clearance,
  });

  run(fx, { mode: "apply", refreshContent: true, clearance });
  assert.match(fx.adapter.getIssue(20).body, /\nAI-Generated\n$/u);
  assert.equal(fx.adapter.getProjectStatus(10), "In Progress");
  assert.equal(fx.adapter.getProjectStatus(20), "In Progress");
});

test("wrong native parent fails the complete sync before mutation", () => {
  const fx = fixture({ childParent: 30 });
  const before = fx.adapter.inspectState();
  const checked = run(fx, { mode: "check" });
  assert.equal(checked.ok, false);
  assert.deepEqual(checked.train_parents.sub_issues_conflict, [
    {
      train: "fixture.compiler",
      node_id: "WORK",
      gh_issue: 10,
      current_parent: 30,
      expected_parent: 20,
    },
  ]);
  const applied = run(fx, { mode: "apply", clearance: clearanceFor(fx.adapter) });
  assert.equal(applied.ok, false);
  assert.deepEqual(fx.adapter.inspectState(), before);
});

test("completed mapped children and their parent remain untouched", () => {
  const fx = fixture({ implemented: ["SYNC-READY", "WORK"] });
  const before = fx.adapter.inspectState();
  const report = run(fx, { mode: "apply", clearance: clearanceFor(fx.adapter) });
  assert.deepEqual(report.selection, []);
  assert.deepEqual(report.skipped_completed, ["WORK"]);
  assert.deepEqual(report.train_parents.missing, []);
  assert.deepEqual(report.train_parents.drift, []);
  assert.deepEqual(report.train_parents.updated, []);
  assert.deepEqual(report.train_parents.sub_issues_missing, []);
  assert.deepEqual(fx.adapter.inspectState(), before);
});

test("a train parent refuses more than GitHub's 100 native sub-issues", () => {
  const nodes = Array.from({ length: 101 }, (_, index) => node(`N${index}`));
  const fx = fixture({ nodes, parentNumber: 200 });
  assert.throws(
    () =>
      syncIssues({
        adapter: fx.adapter,
        authority: fx.authority,
        ledgerPath: fx.ledgerPath,
        nodes: nodes.map((row) => row.id),
        labelCatalog: labelCatalog(),
        milestoneCatalog: milestoneCatalog(),
        issueContentCatalog: contentCatalog(nodes.map((row) => row.id)),
        trainCatalog: trainCatalog(),
        syncPrerequisites: [],
        mode: "check",
      }),
    /exceed 100 native sub-issues/u,
  );
});

test("the native sub-issue limit counts existing manual and completed children", () => {
  const fx = fixture({
    parentNumber: 200,
    parentSubIssues: Array.from({ length: 100 }, (_, index) => 300 + index),
  });
  const before = fx.adapter.inspectState();

  assert.throws(
    () => run(fx, { mode: "apply", clearance: clearanceFor(fx.adapter) }),
    /exceed 100 native sub-issues/u,
  );
  assert.deepEqual(fx.adapter.inspectState(), before);
});

test("repository-qualified and bidirectional parent identity is preflighted", () => {
  const fx = fixture({ childParent: 20 });
  const original = fx.adapter.getIssueProjectState.bind(fx.adapter);
  fx.adapter.getIssueProjectState = (number) => {
    const snapshot = original(number);
    if (number === 10) snapshot.parent = { ...snapshot.parent, owner: "another-owner" };
    return snapshot;
  };
  const before = fx.adapter.inspectState();

  assert.throws(
    () => run(fx, { mode: "apply", clearance: clearanceFor(fx.adapter) }),
    /outside pikax\/verter/u,
  );
  assert.deepEqual(fx.adapter.inspectState(), before);
});

test("a successful Project add is retained in partial-failure receipts", () => {
  const fx = fixture();
  const original = fx.adapter.setIssueProjectStatus.bind(fx.adapter);
  fx.adapter.setIssueProjectStatus = (request) => {
    if (request.issueNumber === 20) throw new Error("status write failed");
    return original(request);
  };

  let failure;
  try {
    run(fx, { mode: "apply", clearance: clearanceFor(fx.adapter) });
  } catch (error) {
    failure = error;
  }
  assert.equal(failure instanceof PartialFailureError, true);
  assert.equal(
    failure.succeeded.some((row) => row.kind === "add-project-item" && row.gh_issue === 20),
    true,
  );
  assert.equal(fx.adapter.getProjectItems().includes(20), true);
});
