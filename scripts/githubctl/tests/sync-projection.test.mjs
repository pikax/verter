import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  FakeGitHubAdapter,
  GitHubDoctor,
  IssueSyncError,
  syncIssues,
  validateIssueContentCatalog,
} from "../index.mjs";
import { ledgerText } from "./ledger-fixture.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const PACKAGE_ROOT = path.resolve(HERE, "../../../roadmap/0.1.0-tama");

function fake(options = {}) {
  return new FakeGitHubAdapter({ owner: "pikax", repo: "verter", ...options });
}

function clearanceFor(adapter) {
  const report = new GitHubDoctor(adapter).check({ require: ["issues"] });
  assert.equal(report.ok, true, report.errors?.join?.("; ") ?? "doctor failed");
  return report.clearance;
}

function labelCatalog() {
  return {
    managedPrefixes: ["area:", "problem:", "framework:"],
    managedExact: ["origin:ai"],
    labels: [
      { name: "area:identity", color: "000001", description: "Identity" },
      { name: "problem:capability", color: "000002", description: "Capability" },
      { name: "framework:shared", color: "000003", description: "Shared" },
      { name: "origin:ai", color: "000004", description: "AI" },
    ],
    areaRules: [{ label: "area:identity", trains: ["fixture"] }],
    problemTrainRules: [],
    problemKindRules: [{ label: "problem:capability", kinds: ["implementation"] }],
    frameworkRules: [{ label: "framework:shared", trains: ["fixture"] }],
  };
}

function milestoneCatalog() {
  return {
    milestones: [
      {
        title: "0.0.1-beta.4",
        description: "Structural correctness baseline",
      },
    ],
  };
}

function issueContentCatalog() {
  return validateIssueContentCatalog({
    schema: 1,
    issue: [
      { node_id: "ROOT", title: "Make the required source input available" },
      { node_id: "WORK", title: "Publish deterministic dependent output" },
    ].map(({ node_id, title }) => ({
      node_id,
      title,
      problem:
        "Current behavior can publish an incomplete result before its required input is available. This risks incorrect output, leaves consumers unable to distinguish provisional data, and makes repeated execution unreliable.",
      expected_outcome:
        "The required input is available before dependent work begins, and repeated execution produces the same complete result with stable provenance.",
      acceptance: [
        "Dependent work starts only after its required input is available.",
        "Incomplete input cannot publish a misleading successful result.",
        "Repeated execution preserves deterministic output and provenance.",
      ],
    })),
  });
}

function fixture(options = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-sync-projection-"));
  const ledgerPath = path.join(dir, "implemented.toml");
  const mappings = options.mappings ?? [
    { node_id: "ROOT", gh_issue: 9, sync_to_github: true },
    { node_id: "WORK", gh_issue: 10, sync_to_github: true },
  ];
  fs.writeFileSync(
    ledgerPath,
    ledgerText({
      implemented: options.implemented ?? ["SYNC-READY"],
      issues: mappings,
    }),
  );
  const nodes = [
    { id: "SYNC-READY", train: "fixture", kind: "implementation", predecessors: [] },
    { id: "ROOT", train: "fixture", kind: "implementation", predecessors: [] },
    {
      id: "WORK",
      train: "fixture",
      kind: "implementation",
      predecessors: ["ROOT"],
      gh_milestone: "0.0.1-beta.4",
    },
  ];
  const adapter = fake({
    issues: options.issues ?? [
      {
        number: 9,
        title: "Root",
        body: "root",
        labels: ["area:identity", "problem:capability", "framework:shared", "origin:ai"],
      },
      { number: 10, title: "Work", body: "work", dependencies: options.dependencies ?? [] },
    ],
    repositoryLabels: labelCatalog().labels,
    milestones: options.milestones ?? [],
  });
  return {
    adapter,
    authority: {
      nodes,
      ledgerFile: path.join(dir, "live-implemented.toml"),
      packageRoot: PACKAGE_ROOT,
    },
    ledgerPath,
  };
}

function run(fx, extra = {}) {
  return syncIssues({
    adapter: fx.adapter,
    authority: fx.authority,
    ledgerPath: fx.ledgerPath,
    nodes: ["WORK"],
    labelCatalog: labelCatalog(),
    milestoneCatalog: milestoneCatalog(),
    issueContentCatalog: issueContentCatalog(),
    syncPrerequisites: [],
    createBlockers: true,
    projectIssues: false,
    syncTrainParents: false,
    ...extra,
  });
}

test("sync applies the configured milestone and direct blocked-by edge", () => {
  const fx = fixture();
  const checked = run(fx, { mode: "check" });
  assert.deepEqual(checked.milestone_catalog.missing, ["0.0.1-beta.4"]);
  assert.deepEqual(checked.drift[0].add_blocked_by, [9]);
  assert.equal(checked.drift[0].milestone, "0.0.1-beta.4");

  run(fx, { mode: "apply", clearance: clearanceFor(fx.adapter) });
  assert.equal(fx.adapter.getIssue(10).milestone, "0.0.1-beta.4");
  assert.deepEqual(
    fx.adapter.getIssueDependencies(10).map((row) => row.number),
    [9],
  );
  const repeated = run(fx, { mode: "check" });
  assert.deepEqual(repeated.drift, []);
  assert.equal(repeated.current.find((row) => row.node_id === "WORK").gh_issue, 10);
  assert.equal(fx.adapter.milestoneWrites.length, 1);
  assert.equal(fx.adapter.dependencyWrites.filter((row) => row.kind === "add").length, 1);
});

test("missing reviewed issue content aborts before external mutation", () => {
  const fx = fixture({ mappings: [] });
  const before = fx.adapter.inspectState();
  assert.throws(
    () =>
      run(fx, {
        mode: "apply",
        clearance: clearanceFor(fx.adapter),
        issueContentCatalog: validateIssueContentCatalog({ schema: 1, issue: [] }),
      }),
    IssueSyncError,
  );
  assert.deepEqual(fx.adapter.inspectState(), before);
});

test("sync removes stale mapped edges but preserves protected and unrelated dependencies", () => {
  const mappings = [
    { node_id: "ROOT", gh_issue: 9, sync_to_github: true },
    { node_id: "WORK", gh_issue: 10, sync_to_github: true },
    { node_id: "STALE", gh_issue: 8, sync_to_github: true },
    { node_id: "PROTECTED", gh_issue: 7, sync_to_github: false },
  ];
  const fx = fixture({
    mappings,
    dependencies: [8, 7, 99],
    milestones: [{ title: "0.0.1-beta.4", description: "Structural correctness baseline" }],
    issues: [
      { number: 7, title: "Protected", body: "protected" },
      { number: 8, title: "Stale", body: "stale" },
      { number: 9, title: "Root", body: "root" },
      { number: 10, title: "Work", body: "work", dependencies: [8, 7, 99] },
      { number: 99, title: "External", body: "external" },
    ],
  });
  fx.authority.nodes.push(
    { id: "STALE", train: "fixture", kind: "implementation", predecessors: [] },
    { id: "PROTECTED", train: "fixture", kind: "implementation", predecessors: [] },
  );

  const checked = run(fx, { mode: "check" });
  const workDrift = checked.drift.find((row) => row.node_id === "WORK");
  assert.deepEqual(workDrift.add_blocked_by, [9]);
  assert.deepEqual(workDrift.remove_blocked_by, [8]);

  run(fx, { mode: "apply", clearance: clearanceFor(fx.adapter) });
  assert.deepEqual(
    fx.adapter.getIssueDependencies(10).map((row) => row.number),
    [7, 9, 99],
  );
});

test("sync reports required unresolved blocker issues and fails without explicit creation", () => {
  const fx = fixture({
    mappings: [{ node_id: "WORK", gh_issue: 10, sync_to_github: true }],
    issues: [{ number: 10, title: "Work", body: "work" }],
  });
  const before = fx.adapter.inspectState();
  const checked = run(fx, { mode: "check", createBlockers: false });
  assert.equal(checked.ok, false);
  assert.deepEqual(checked.selection, ["WORK"]);
  assert.deepEqual(checked.required_blocker_issues, [
    {
      node_id: "ROOT",
      train: "fixture",
      required_by: ["WORK"],
      gh_issue: null,
      sync_to_github: null,
    },
  ]);
  assert.deepEqual(checked.drift, []);
  const applied = run(fx, {
    mode: "apply",
    createBlockers: false,
    clearance: clearanceFor(fx.adapter),
  });
  assert.equal(applied.ok, false);
  assert.deepEqual(applied.required_blocker_issues, checked.required_blocker_issues);
  assert.deepEqual(fx.adapter.inspectState(), before);
});

test("explicit blocker creation adds unresolved issues before attaching dependencies", () => {
  const fx = fixture({
    mappings: [{ node_id: "WORK", gh_issue: 10, sync_to_github: true }],
    issues: [{ number: 10, title: "Work", body: "work" }],
  });
  const checked = run(fx, { mode: "check", createBlockers: true });
  assert.equal(checked.ok, true);
  assert.deepEqual(checked.selection, ["ROOT", "WORK"]);
  assert.deepEqual(
    checked.missing.map((row) => row.node_id),
    ["ROOT"],
  );
  assert.deepEqual(checked.drift[0].create_blocked_by, ["ROOT"]);
  const applied = run(fx, {
    mode: "apply",
    createBlockers: true,
    clearance: clearanceFor(fx.adapter),
  });
  assert.deepEqual(applied.created, [{ node_id: "ROOT", gh_issue: 11, mapping_written: true }]);
  assert.deepEqual(
    fx.adapter.getIssueDependencies(10).map((row) => row.number),
    [11],
  );
  const repeated = run(fx, { mode: "check" });
  assert.deepEqual(repeated.missing, []);
  assert.deepEqual(repeated.drift, []);
});

test("explicit blocker ignore permits a bounded update and preserves external relationships", () => {
  const fx = fixture({
    dependencies: [9],
    issues: [
      {
        number: 9,
        title: "Root",
        body: "root",
        labels: ["area:identity", "problem:capability", "framework:shared", "origin:ai"],
      },
      {
        number: 10,
        title: "Work",
        body: "work",
        milestone: "0.0.1-beta.4",
        labels: ["area:identity", "problem:capability", "framework:shared", "origin:ai"],
        dependencies: [9],
      },
    ],
    milestones: [{ title: "0.0.1-beta.4", description: "Structural correctness baseline" }],
  });
  const checked = run(fx, {
    mode: "check",
    createBlockers: false,
    ignoreBlockers: true,
  });
  assert.equal(checked.ok, true);
  assert.deepEqual(checked.selection, ["WORK"]);
  assert.deepEqual(checked.ignored_blocker_issues, [
    {
      node_id: "ROOT",
      train: "fixture",
      required_by: ["WORK"],
      gh_issue: 9,
      sync_to_github: true,
    },
  ]);
  assert.deepEqual(
    checked.current.map((row) => row.node_id),
    ["WORK"],
  );
  run(fx, {
    mode: "apply",
    createBlockers: false,
    ignoreBlockers: true,
    clearance: clearanceFor(fx.adapter),
  });
  assert.deepEqual(
    fx.adapter.getIssueDependencies(10).map((row) => row.number),
    [9],
  );
  assert.equal(fx.adapter.dependencyWrites.length, 0);
});

test("completed requirements do not cross the selection boundary or create issues", () => {
  const fx = fixture({
    implemented: ["SYNC-READY", "ROOT"],
    mappings: [{ node_id: "WORK", gh_issue: 10, sync_to_github: true }],
    issues: [
      {
        number: 10,
        title: "Work",
        body: "work",
        milestone: "0.0.1-beta.4",
        labels: ["area:identity", "problem:capability", "framework:shared", "origin:ai"],
      },
    ],
    milestones: [{ title: "0.0.1-beta.4", description: "Structural correctness baseline" }],
  });
  const checked = run(fx, { mode: "check", createBlockers: false });
  assert.equal(checked.ok, true);
  assert.deepEqual(checked.required_blocker_issues, []);
  assert.deepEqual(checked.selection, ["WORK"]);
  assert.deepEqual(checked.current[0].completed_blocked_by, ["ROOT"]);
  run(fx, {
    mode: "apply",
    createBlockers: false,
    clearance: clearanceFor(fx.adapter),
  });
  assert.equal(fx.adapter.getIssue(11), null);
  assert.deepEqual(fx.adapter.getIssueDependencies(10), []);
});

test("closed issues are reported and remain byte-for-byte untouched", () => {
  const fx = fixture({
    issues: [
      {
        number: 9,
        title: "Root",
        body: "root",
        labels: ["area:identity", "problem:capability", "framework:shared", "origin:ai"],
      },
      {
        number: 10,
        title: "Closed title",
        body: "closed body",
        state: "closed",
        labels: ["maintainer-owned"],
      },
    ],
    milestones: [{ title: "0.0.1-beta.4", description: "Structural correctness baseline" }],
  });
  const before = fx.adapter.inspectState();
  const checked = run(fx, {
    mode: "check",
    refreshContent: true,
    createBlockers: false,
    ignoreBlockers: true,
  });
  assert.deepEqual(checked.closed, [{ node_id: "WORK", gh_issue: 10 }]);
  const applied = run(fx, {
    mode: "apply",
    refreshContent: true,
    createBlockers: false,
    ignoreBlockers: true,
    clearance: clearanceFor(fx.adapter),
  });
  assert.deepEqual(applied.closed, [{ node_id: "WORK", gh_issue: 10 }]);
  assert.deepEqual(fx.adapter.inspectState(), before);
});

test("dependency reconciliation distinguishes repositories with the same issue number", () => {
  const fx = fixture({
    milestones: [{ title: "0.0.1-beta.4", description: "Structural correctness baseline" }],
  });
  fx.adapter.getIssueDependencies = () => [
    { id: 9000, number: 9, owner: "external", repo: "other" },
  ];
  const checked = run(fx, { mode: "check" });
  assert.deepEqual(checked.drift[0].add_blocked_by, [9]);
  assert.deepEqual(checked.drift[0].remove_blocked_by, []);
});

test("dependency removals are deterministic across response order", () => {
  const mappings = [
    { node_id: "ROOT", gh_issue: 9, sync_to_github: true },
    { node_id: "WORK", gh_issue: 10, sync_to_github: true },
    { node_id: "STALE_A", gh_issue: 7, sync_to_github: true },
    { node_id: "STALE_B", gh_issue: 8, sync_to_github: true },
  ];
  const reports = [
    [8, 7],
    [7, 8],
  ].map((dependencies) => {
    const fx = fixture({
      mappings,
      dependencies,
      milestones: [{ title: "0.0.1-beta.4", description: "Structural correctness baseline" }],
      issues: [
        { number: 7, title: "Stale A", body: "stale" },
        { number: 8, title: "Stale B", body: "stale" },
        { number: 9, title: "Root", body: "root" },
        { number: 10, title: "Work", body: "work", dependencies },
      ],
    });
    fx.authority.nodes.push(
      { id: "STALE_A", train: "fixture", kind: "implementation", predecessors: [] },
      { id: "STALE_B", train: "fixture", kind: "implementation", predecessors: [] },
    );
    return run(fx, { mode: "check" }).drift.find((row) => row.node_id === "WORK").remove_blocked_by;
  });
  assert.deepEqual(reports, [
    [7, 8],
    [7, 8],
  ]);
});
