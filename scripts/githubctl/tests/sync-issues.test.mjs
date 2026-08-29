import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  DuplicateError,
  FakeGitHubAdapter,
  GitHubAdapter,
  GitHubDoctor,
  labelsForNode,
  loadIssueContentCatalog,
  loadIssueLabelCatalog,
  loadIssueMilestoneCatalog,
  MissingAncestorError,
  mutationIdentity,
  NotFoundError,
  PartialFailureError,
  PermissionDeniedError,
  ProtectedMappingError,
  SelectionError,
  UnstructuredGitHubOutputError,
  lookupIssueMapping,
  renderIssueDescription,
  syncIssues as syncIssuesImpl,
} from "../index.mjs";
import {
  githubIssueByNumber,
  listGitHubIssues,
  loadAuthority,
  parseToml,
} from "../../../roadmap/0.1.0-tama/tools/lib.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const CLI = path.join(HERE, "../githubctl.mjs");
const TOOLS = path.join(REPO_ROOT, "roadmap/0.1.0-tama/tools");
const LIVE_LEDGER = path.join(REPO_ROOT, "roadmap/0.1.0-tama/authority/state/implemented.toml");
const AUTHORITY = loadAuthority();
const LABEL_CATALOG = loadIssueLabelCatalog(AUTHORITY.packageRoot);
const MILESTONE_CATALOG = loadIssueMilestoneCatalog(AUTHORITY.packageRoot);
const CLI_CONTENT_NODE = loadIssueContentCatalog(AUTHORITY.packageRoot).issues[0].node_id;
const ISSUE_CONTENT_CATALOG = {
  byNode: new Map(
    AUTHORITY.nodes.map((node) => [
      node.id,
      {
        node_id: node.id,
        title: "Provide deterministic user-visible behavior",
        problem:
          "Current behavior can return inconsistent externally visible results when the affected capability is used across supported inputs. This leaves users with stale or incorrect output, makes the operation unreliable, and prevents maintainers from distinguishing a valid result from an accidental fallback.",
        expected_outcome:
          "The capability produces one deterministic result across its supported inputs and preserves the identity and provenance needed by downstream consumers.",
        acceptance: [
          "Supported inputs produce deterministic observable results.",
          "Invalid or incomplete inputs fail without publishing a misleading result.",
          "Repeated execution preserves the same identity and provenance.",
        ],
        technical_context: [],
      },
    ]),
  ),
};
const SYNCABLE = AUTHORITY.nodes.find((item) => {
  const labels = labelsForNode(item, LABEL_CATALOG);
  return labels.includes("area:tooling") && labels.includes("problem:automation");
});
if (!SYNCABLE) throw new Error("expected an automation work item for issue sync tests");
const SYNCABLE_ID = SYNCABLE.id;
const SYNCABLE_LABELS = labelsForNode(SYNCABLE, LABEL_CATALOG);
const MODEL = "test-model";

function fake(options = {}) {
  const {
    repositoryLabels = LABEL_CATALOG.labels,
    milestones = MILESTONE_CATALOG.milestones,
    ...rest
  } = options;
  return new FakeGitHubAdapter({
    owner: "pikax",
    repo: "verter",
    repositoryLabels,
    milestones,
    ...rest,
  });
}

function clearanceFor(adapter) {
  const report = new GitHubDoctor(adapter).check({ require: ["issues"] });
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
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-sync-"));
  const file = path.join(dir, "implemented.toml");
  const implemented = options.implemented ?? ["ORC0", "GH0", "GH1"];
  const issues = options.issues ?? [];
  const parts = ["schema = 1", "", ...implemented.map(implementedBlock)];
  for (const row of issues) parts.push(mappingBlock(row.node_id, row.gh_issue, row.sync_to_github));
  fs.writeFileSync(file, parts.join("\n"));
  return file;
}

function readLedger(file) {
  return parseToml(fs.readFileSync(file, "utf8"));
}

function rendered(nodeId = "GH0") {
  return renderIssueDescription({ nodeId, contentCatalog: ISSUE_CONTENT_CATALOG });
}

function syncIssues(options) {
  return syncIssuesImpl({ issueContentCatalog: ISSUE_CONTENT_CATALOG, ...options });
}

const WRITABLE_REPO = {
  full_name: "pikax/verter",
  has_issues: true,
  permissions: { admin: false, maintain: false, push: true, triage: false, pull: true },
};

function liveTransport(routes) {
  const calls = [];
  const withProject = {
    "POST graphql": {
      data: {
        organization: { projectV2: { id: "PVT_test", number: 3 } },
        user: { projectV2: null },
      },
    },
    ...routes,
  };
  return {
    calls,
    request(req) {
      calls.push(req);
      const hit = withProject[`${req.method} ${req.path}`];
      if (hit instanceof Error) throw hit;
      if (hit) return hit;
      throw new Error(`unexpected ${req.method} ${req.path}`);
    },
  };
}

test("GH2-AC1 protected mapping is skipped without updateIssue and body is unchanged", () => {
  const originalBody = "pre-existing protected prose";
  const adapter = fake({
    issues: [
      {
        number: 7,
        title: "kept title",
        body: originalBody,
        comments: [{ id: 1, body: "discussion" }],
      },
    ],
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 7, sync_to_github: false }],
  });
  const report = syncIssues({
    adapter,
    mode: "apply",
    nodes: ["GH0"],
    model: MODEL,
    ledgerPath,
    clearance: clearanceFor(adapter),
  });
  assert.equal(report.ok, true);
  assert.equal(report.updated.length, 0);
  assert.equal(report.created.length, 0);
  assert.deepEqual(
    report.protected.map((row) => row.node_id),
    ["GH0"],
  );
  assert.equal(
    adapter.reads.some((row) => row.number === 7),
    false,
  );
  const issue = adapter.getIssue(7);
  assert.equal(issue.title, "kept title");
  assert.equal(issue.body, originalBody);
  assert.deepEqual(issue.comments, [{ id: 1, body: "discussion" }]);
  assert.equal(adapter.refusals.length, 0);
  assert.throws(
    () =>
      adapter.updateIssue({
        number: 7,
        title: "rewritten",
        body: "rewritten",
        mapping: { node_id: "GH0", gh_issue: 7, sync_to_github: false },
        mode: "apply",
        clearance: clearanceFor(adapter),
      }),
    ProtectedMappingError,
  );
  assert.equal(adapter.refusals.length, 1);
  assert.equal(adapter.getIssue(7).body, originalBody);
  assert.throws(
    () =>
      adapter.addIssueLabels({
        number: 7,
        labels: ["origin:ai"],
        mapping: { node_id: "synthetic-item", gh_issue: 7, sync_to_github: false },
        mode: "apply",
        clearance: clearanceFor(adapter),
      }),
    ProtectedMappingError,
  );
});

test("GH2-AC2 sync-issues apply requires issues clearance, not Project 3", () => {
  const adapter = fake({ missing: true, nextIssueNumber: 40 });
  assert.equal(adapter.inspectCapabilities().projects, false);
  const full = new GitHubDoctor(adapter).check();
  assert.equal(full.ok, false);
  assert.equal(full.errors.includes("projects"), true);
  assert.equal(full.clearance, null);
  const issuesOnly = new GitHubDoctor(adapter).check({ require: ["issues"] });
  assert.equal(issuesOnly.ok, true);
  assert.equal(issuesOnly.capabilities.projects, false);
  const ledgerPath = writeLedger();
  const report = syncIssues({
    adapter,
    mode: "apply",
    nodes: ["GH0"],
    model: MODEL,
    ledgerPath,
    clearance: issuesOnly.clearance,
  });
  assert.equal(report.ok, true);
  assert.equal(report.created[0].gh_issue, 40);
  assert.deepEqual(adapter.getProjectItems(3), []);
});

test("GH2-AC1 apply never writes an implemented row", () => {
  const adapter = fake({ nextIssueNumber: 40 });
  const ledgerPath = writeLedger();
  const before = readLedger(ledgerPath);
  syncIssues({
    adapter,
    mode: "apply",
    nodes: ["GH0"],
    model: MODEL,
    ledgerPath,
    clearance: clearanceFor(adapter),
  });
  const after = readLedger(ledgerPath);
  assert.deepEqual(after.implemented, before.implemented);
  assert.equal(
    after.implemented.some((row) => row.node_id === "GH0" && row.commit_message === undefined),
    false,
  );
  assert.equal(
    fs.readFileSync(ledgerPath, "utf8").match(/\[\[implemented\]\]/g)?.length,
    before.implemented.length,
  );
});

test("GH2-AC1 rendered body uses the human issue standard and omits charter headings", () => {
  const { title, body } = rendered("GH2");
  assert.equal(title, "Provide deterministic user-visible behavior");
  assert.match(body, /^## Problem\n/u);
  assert.match(body, /^## Expected outcome\n/mu);
  assert.match(body, /^## Acceptance\n/mu);
  assert.equal(body.endsWith("\nAI-Generated\n"), true);
  assert.equal([...body.matchAll(/^AI-Generated$/gmu)].length, 1);
  assert.doesNotMatch(body, /^## Independently acceptable outcome/mu);
  assert.doesNotMatch(body, /^## Source-specific scope/mu);
  assert.doesNotMatch(body, /^## Deletions and forbidden designs/mu);
  assert.doesNotMatch(body, /^## Abort conditions/mu);
  assert.doesNotMatch(body, /\bGH2\b/u);
  assert.doesNotMatch(body, /predecessors\s*=/u);
  assert.doesNotMatch(body, /implementation_effort/u);
  assert.doesNotMatch(body, /max_production_loc/u);
  assert.doesNotMatch(body, /unified-charter-v2/u);
  assert.doesNotMatch(body, /\bid=/u);
  assert.doesNotMatch(body, /<!--/u);
  assert.doesNotMatch(title, /\bGH2\b/u);
});

test("GH2-AC1 programctl stays GitHub-blind and CLI has no createPullRequest command", () => {
  for (const name of ["programctl.mjs", "lib.mjs"]) {
    const text = fs.readFileSync(path.join(TOOLS, name), "utf8");
    assert.doesNotMatch(text, /\bgh\s+api\b/u);
    assert.doesNotMatch(text, /\bfetch\s*\(/u);
    assert.doesNotMatch(text, /githubctl/u);
  }
  const missing = spawnSync(process.execPath, [CLI, "createPullRequest"], { encoding: "utf8" });
  assert.notEqual(missing.status, 0);
  assert.match(missing.stderr, /unknown command/u);
  const help = spawnSync(process.execPath, [CLI, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /sync-issues/u);
  assert.match(help.stdout, /--refresh-content/u);
  assert.doesNotMatch(help.stdout, /createPullRequest/u);
});

test("GH2-AC1 unknown or missing selection fails closed", () => {
  const adapter = fake();
  const ledgerPath = writeLedger();
  assert.throws(
    () => syncIssues({ adapter, mode: "check", model: MODEL, ledgerPath }),
    SelectionError,
  );
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "check",
        train: "not-a-train",
        nodes: ["GH0"],
        model: MODEL,
        ledgerPath,
      }),
    SelectionError,
  );
  assert.throws(
    () => syncIssues({ adapter, mode: "check", nodes: ["ZZ0"], model: MODEL, ledgerPath }),
    SelectionError,
  );
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "check",
        train: "not-a-train",
        model: MODEL,
        ledgerPath,
      }),
    SelectionError,
  );
  const cli = spawnSync(process.execPath, [CLI, "sync-issues", "--check", "--fake"], {
    encoding: "utf8",
  });
  assert.notEqual(cli.status, 0);
  assert.match(cli.stderr, /--train|selection/iu);
  assert.equal(adapter.getIssues().length, 0);
});

test("GH2-AC1 check does not mutate fake state or the ledger", () => {
  const adapter = fake({
    nextIssueNumber: 9,
    issues: [
      {
        number: 3,
        title: "stale",
        body: "stale",
        comments: [{ id: 8, body: "stay" }],
      },
    ],
  });
  const ledgerPath = writeLedger({
    issues: [
      { node_id: "GH0", gh_issue: 3, sync_to_github: true },
      { node_id: "GH1", gh_issue: 4, sync_to_github: false },
    ],
  });
  const beforeState = adapter.inspectState();
  const beforeLedger = fs.readFileSync(ledgerPath, "utf8");
  const report = syncIssues({
    adapter,
    mode: "check",
    nodes: ["GH0", "GH1", "GH2"],
    model: MODEL,
    ledgerPath,
  });
  assert.equal(
    report.missing.some((row) => row.node_id === "GH2"),
    true,
  );
  assert.equal(
    report.drift.some((row) => row.node_id === "GH0"),
    true,
  );
  assert.equal(
    report.protected.some((row) => row.node_id === "GH1"),
    true,
  );
  assert.deepEqual(adapter.inspectState(), beforeState);
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), beforeLedger);
  assert.equal(
    adapter.reads.some((row) => row.number === 4),
    false,
  );
  assert.equal(
    adapter.reads.some((row) => row.number === 3),
    true,
  );
});

test("GH2-AC2 missing node creates an issue and writes the returned mapping", () => {
  const adapter = fake({ nextIssueNumber: 40 });
  const ledgerPath = writeLedger();
  const { title, body } = rendered("GH0");
  const report = syncIssues({
    adapter,
    mode: "apply",
    nodes: ["GH0"],
    model: MODEL,
    ledgerPath,
    clearance: clearanceFor(adapter),
  });
  assert.equal(report.ok, true);
  assert.deepEqual(report.created, [{ node_id: "GH0", gh_issue: 40, mapping_written: true }]);
  const issue = adapter.getIssue(40);
  assert.equal(issue.number, 40);
  assert.equal(issue.title, title);
  assert.equal(issue.body, body);
  const mapped = listGitHubIssues(readLedger(ledgerPath));
  assert.deepEqual(mapped, [{ node_id: "GH0", gh_issue: 40, sync_to_github: true }]);
  assert.equal(mapped[0].gh_issue, 40);
  assert.equal(typeof mapped[0].gh_issue, "number");
});

test("normal issue sync reconciles owned labels without rewriting prose", () => {
  const adapter = fake({
    issues: [
      {
        number: 12,
        title: "maintainer title",
        body: "maintainer description",
        labels: ["bug", "ai:confirmed", "area:tooling", "problem:architecture"],
      },
    ],
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: SYNCABLE_ID, gh_issue: 12, sync_to_github: true }],
  });

  const report = syncIssues({
    adapter,
    mode: "apply",
    nodes: [SYNCABLE_ID],
    ledgerPath,
    clearance: clearanceFor(adapter),
  });

  assert.equal(report.ok, true);
  assert.equal(adapter.getIssue(12).title, "maintainer title");
  assert.equal(adapter.getIssue(12).body, "maintainer description");
  assert.deepEqual(adapter.getIssueLabels(12).sort(), [
    "ai:confirmed",
    "area:tooling",
    "bug",
    "origin:ai",
    "problem:automation",
  ]);
  assert.deepEqual(report.updated, [
    {
      node_id: SYNCABLE_ID,
      gh_issue: 12,
      content: false,
      labels: true,
    },
  ]);
});

test("a partially applied label reconciliation reports the completed mutation", () => {
  const adapter = fake({
    failOnApply: 1,
    failOnApplyError: new PermissionDeniedError("configured label removal failure"),
    issues: [
      {
        number: 12,
        title: "title",
        body: "body",
        labels: ["problem:architecture"],
      },
    ],
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: SYNCABLE_ID, gh_issue: 12, sync_to_github: true }],
  });

  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: [SYNCABLE_ID],
        ledgerPath,
        clearance: clearanceFor(adapter),
      }),
    (error) => {
      assert.equal(error instanceof PartialFailureError, true);
      assert.deepEqual(error.succeeded, [
        {
          node_id: SYNCABLE_ID,
          gh_issue: 12,
          kind: "add-issue-labels",
          mapping_written: true,
        },
      ]);
      assert.equal(error.failed.error instanceof PermissionDeniedError, true);
      return true;
    },
  );
});

test("explicit content refresh updates prose and managed labels together", () => {
  const adapter = fake({
    issues: [
      {
        number: 12,
        title: "maintainer title",
        body: "maintainer description",
        labels: ["problem:architecture"],
      },
    ],
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: SYNCABLE_ID, gh_issue: 12, sync_to_github: true }],
  });
  const expected = rendered(SYNCABLE_ID);

  const report = syncIssues({
    adapter,
    mode: "apply",
    nodes: [SYNCABLE_ID],
    model: MODEL,
    refreshContent: true,
    ledgerPath,
    clearance: clearanceFor(adapter),
  });

  assert.equal(adapter.getIssue(12).title, expected.title);
  assert.equal(adapter.getIssue(12).body, expected.body);
  assert.deepEqual(adapter.getIssueLabels(12).sort(), [
    "area:tooling",
    "origin:ai",
    "problem:automation",
  ]);
  assert.deepEqual(report.updated, [
    {
      node_id: SYNCABLE_ID,
      gh_issue: 12,
      content: true,
      labels: true,
    },
  ]);
});

test("content refresh does not require a model", () => {
  const adapter = fake({
    issues: [
      {
        number: 12,
        title: "title",
        body: "body",
        labels: ["area:tooling", "problem:automation", "origin:ai"],
      },
    ],
  });
  const mappedLedger = writeLedger({
    issues: [{ node_id: SYNCABLE_ID, gh_issue: 12, sync_to_github: true }],
  });
  const checked = syncIssues({
    adapter,
    mode: "check",
    nodes: [SYNCABLE_ID],
    ledgerPath: mappedLedger,
  });
  assert.deepEqual(checked.current, [
    {
      node_id: SYNCABLE_ID,
      gh_issue: 12,
      blocked_by_unmapped: [...SYNCABLE.predecessors],
      blocked_by_protected: [],
    },
  ]);
  const refreshed = syncIssues({
    adapter,
    mode: "apply",
    nodes: [SYNCABLE_ID],
    refreshContent: true,
    ledgerPath: mappedLedger,
    clearance: clearanceFor(adapter),
  });
  assert.equal(refreshed.ok, true);
  assert.match(adapter.getIssue(12).body, /\nAI-Generated\n$/u);

  const missing = syncIssues({
    adapter: fake(),
    mode: "check",
    nodes: [SYNCABLE_ID],
    ledgerPath: writeLedger(),
  });
  assert.equal(missing.missing[0].node_id, SYNCABLE_ID);
  assert.deepEqual(missing.missing[0].labels, SYNCABLE_LABELS);
  assert.equal(missing.missing[0].content_required, true);
});

test("issue sync creates and updates the versioned repository label catalog", () => {
  const adapter = fake({
    repositoryLabels: [
      {
        name: "area:tooling",
        color: "ffffff",
        description: "stale description",
      },
      { name: "bug", color: "d73a4a", description: "preserve me" },
    ],
    issues: [{ number: 12, title: "title", body: "body", labels: [] }],
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: SYNCABLE_ID, gh_issue: 12, sync_to_github: true }],
  });

  syncIssues({
    adapter,
    mode: "apply",
    nodes: [SYNCABLE_ID],
    ledgerPath,
    clearance: clearanceFor(adapter),
  });

  const labels = adapter.getRepositoryLabels();
  assert.equal(labels.length, LABEL_CATALOG.labels.length + 1);
  assert.deepEqual(
    labels.find((label) => label.name === "area:tooling"),
    LABEL_CATALOG.labels.find((label) => label.name === "area:tooling"),
  );
  assert.deepEqual(
    labels.find((label) => label.name === "bug"),
    {
      name: "bug",
      color: "d73a4a",
      description: "preserve me",
    },
  );
});

test("a partially created repository catalog reports stable label identities", () => {
  const adapter = fake({
    repositoryLabels: [],
    failOnApply: 1,
    failOnApplyError: new PermissionDeniedError("configured catalog failure"),
    issues: [
      {
        number: 12,
        title: "title",
        body: "body",
        labels: ["area:tooling", "problem:automation", "origin:ai"],
      },
    ],
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: SYNCABLE_ID, gh_issue: 12, sync_to_github: true }],
  });

  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: [SYNCABLE_ID],
        ledgerPath,
        clearance: clearanceFor(adapter),
      }),
    (error) => {
      assert.equal(error instanceof PartialFailureError, true);
      assert.deepEqual(error.succeeded, [
        { kind: "create-repository-label", label: "area:compiler" },
      ]);
      assert.deepEqual(error.failed.operation, {
        kind: "create-repository-label",
        label: "area:identity",
      });
      return true;
    },
  );
});

test("completed catalog writes remain reportable when issue assignment fails", () => {
  const adapter = fake({
    repositoryLabels: [],
    failOnApply: LABEL_CATALOG.labels.length,
    failOnApplyError: new PermissionDeniedError("configured issue assignment failure"),
    issues: [{ number: 12, title: "title", body: "body", labels: [] }],
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: SYNCABLE_ID, gh_issue: 12, sync_to_github: true }],
  });

  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: [SYNCABLE_ID],
        ledgerPath,
        clearance: clearanceFor(adapter),
      }),
    (error) => {
      assert.equal(error instanceof PartialFailureError, true);
      assert.deepEqual(
        error.succeeded,
        LABEL_CATALOG.labels.map((label) => ({
          kind: "create-repository-label",
          label: label.name,
        })),
      );
      assert.deepEqual(mutationIdentity(error.succeeded[0]), {
        kind: "create-repository-label",
        label: LABEL_CATALOG.labels[0].name,
      });
      assert.deepEqual(
        mutationIdentity({
          kind: "create-repository-milestone",
          milestone: "0.0.1-beta.6",
        }),
        {
          kind: "create-repository-milestone",
          milestone: "0.0.1-beta.6",
        },
      );
      assert.equal(error.failed.error instanceof PermissionDeniedError, true);
      return true;
    },
  );
});

test("label classification is deterministic and complete for every work item", () => {
  for (const item of AUTHORITY.nodes) {
    const labels = labelsForNode(item, LABEL_CATALOG);
    assert.equal(labels.filter((label) => label.startsWith("area:")).length, 1);
    assert.equal(labels.filter((label) => label.startsWith("problem:")).length, 1);
    assert.equal(labels.filter((label) => label.startsWith("framework:")).length <= 1, true);
    assert.equal(labels.includes("origin:ai"), true);
  }
});

test("explicit content refresh preserves issue identity and discussion", () => {
  const adapter = fake({
    issues: [
      {
        number: 12,
        title: "old title",
        body: "old body",
        comments: [
          { id: 1, body: "first" },
          { id: 2, body: "second" },
        ],
        labels: ["area:tooling", "problem:automation", "origin:ai"],
      },
    ],
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: SYNCABLE_ID, gh_issue: 12, sync_to_github: true }],
  });
  const { title, body } = rendered(SYNCABLE_ID);
  const report = syncIssues({
    adapter,
    mode: "apply",
    nodes: [SYNCABLE_ID],
    model: MODEL,
    refreshContent: true,
    ledgerPath,
    clearance: clearanceFor(adapter),
  });
  assert.equal(report.ok, true);
  assert.deepEqual(report.updated, [
    { node_id: SYNCABLE_ID, gh_issue: 12, content: true, labels: false },
  ]);
  assert.equal(
    adapter.reads.some((row) => row.kind === "get-issue" && row.number === 12),
    true,
  );
  const issue = adapter.getIssue(12);
  assert.equal(issue.number, 12);
  assert.equal(issue.title, title);
  assert.equal(issue.body, body);
  assert.deepEqual(issue.comments, [
    { id: 1, body: "first" },
    { id: 2, body: "second" },
  ]);
  assert.deepEqual(listGitHubIssues(readLedger(ledgerPath)), [
    { node_id: SYNCABLE_ID, gh_issue: 12, sync_to_github: true },
  ]);
});

test("GH2-AC2 check reports missing vs drift vs protected", () => {
  const { title, body } = rendered("GH0");
  const adapter = fake({
    issues: [
      {
        number: 10,
        title,
        body,
        labels: ["area:tooling", "problem:automation", "origin:ai"],
      },
      {
        number: 11,
        title: "stale",
        body: "stale",
        labels: ["area:tooling", "problem:automation", "origin:ai"],
      },
      { number: 12, title: "protected", body: "do not read" },
    ],
  });
  const ledgerPath = writeLedger({
    issues: [
      { node_id: "GH0", gh_issue: 10, sync_to_github: true },
      { node_id: "GH1", gh_issue: 11, sync_to_github: true },
      { node_id: "GH2", gh_issue: 12, sync_to_github: false },
    ],
  });
  const report = syncIssues({
    adapter,
    mode: "check",
    nodes: ["GH0", "GH1", "GH2", "GH3"],
    model: MODEL,
    refreshContent: true,
    ledgerPath,
  });
  assert.deepEqual(
    report.missing.map((row) => row.node_id),
    ["GH3"],
  );
  assert.deepEqual(
    report.drift.map((row) => row.node_id),
    ["GH1"],
  );
  assert.deepEqual(
    report.protected.map((row) => row.node_id),
    ["GH2"],
  );
  assert.deepEqual(
    report.current.map((row) => row.node_id),
    ["GH0"],
  );
  assert.equal(
    adapter.reads.some((row) => row.number === 12),
    false,
  );
});

test("GH2-AC2 duplicate node or duplicate issue number aborts before GitHub writes", () => {
  const adapter = fake({ nextIssueNumber: 8 });
  const duplicateNode = writeLedger({
    issues: [
      { node_id: "GH0", gh_issue: 1, sync_to_github: true },
      { node_id: "GH0", gh_issue: 2, sync_to_github: false },
    ],
  });
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH1"],
        model: MODEL,
        ledgerPath: duplicateNode,
        clearance: clearanceFor(adapter),
      }),
    /duplicate node/i,
  );
  const duplicateIssue = writeLedger({
    issues: [
      { node_id: "GH0", gh_issue: 5, sync_to_github: true },
      { node_id: "GH1", gh_issue: 5, sync_to_github: false },
    ],
  });
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH2"],
        model: MODEL,
        ledgerPath: duplicateIssue,
        clearance: clearanceFor(adapter),
      }),
    /duplicate issue/i,
  );
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "check",
        nodes: ["GH0", "GH0"],
        model: MODEL,
        ledgerPath: writeLedger(),
      }),
    SelectionError,
  );
  assert.equal(adapter.getIssues().length, 0);
});

test("GH2-AC2 partial failure keeps the first mapping and reports succeeded numbers", () => {
  const adapter = fake({
    nextIssueNumber: 21,
    failOnApply: 1,
    failOnApplyError: new PermissionDeniedError("issues denied after first create"),
  });
  const ledgerPath = writeLedger();
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH0", "GH1"],
        model: MODEL,
        ledgerPath,
        clearance: clearanceFor(adapter),
      }),
    (error) => {
      assert.equal(error instanceof PartialFailureError, true);
      assert.equal(error.succeeded.length, 1);
      assert.equal(error.succeeded[0].node_id, "GH0");
      assert.equal(error.succeeded[0].gh_issue, 21);
      assert.equal(error.succeeded[0].kind, "create-issue");
      assert.equal(error.succeeded[0].mapping_written, true);
      assert.equal(error.succeeded[0].title, undefined);
      assert.equal(error.succeeded[0].body, undefined);
      assert.equal(error.message.includes("Created"), false);
      assert.match(error.message, /21/u);
      assert.match(error.message, /GH0/u);
      return true;
    },
  );
  assert.equal(adapter.getIssue(21).title, rendered("GH0").title);
  assert.equal(adapter.getIssue(22), null);
  assert.deepEqual(listGitHubIssues(readLedger(ledgerPath)), [
    { node_id: "GH0", gh_issue: 21, sync_to_github: true },
  ]);
  assert.equal(lookupIssueMapping(readLedger(ledgerPath), 21).node_id, "GH0");
  assert.throws(() => lookupIssueMapping(readLedger(ledgerPath), 22), /not mapped/u);
});

test("GH2-AC2 reverse lookup is local table search by unique gh_issue", () => {
  const ledger = {
    github_issue: [
      { node_id: "GH1", gh_issue: 99, sync_to_github: false, title: "not identity" },
      { node_id: "GH0", gh_issue: 42, sync_to_github: true },
    ],
  };
  assert.deepEqual(lookupIssueMapping(ledger, 42), {
    node_id: "GH0",
    gh_issue: 42,
    sync_to_github: true,
  });
  assert.deepEqual(lookupIssueMapping(ledger, 99), {
    node_id: "GH1",
    gh_issue: 99,
    sync_to_github: false,
  });
  assert.equal(lookupIssueMapping, githubIssueByNumber);
  assert.throws(() => lookupIssueMapping(ledger, "42"), /positive safe integer/u);
  assert.throws(() => lookupIssueMapping(ledger, 7), /not mapped/u);
});

test("GH2-AC2 train selection enumerates only that train in deterministic order", () => {
  const adapter = fake();
  const ledgerPath = writeLedger();
  const report = syncIssues({
    adapter,
    mode: "check",
    train: "governance.github-control-plane",
    model: MODEL,
    ledgerPath,
  });
  assert.deepEqual(report.selection, ["GH0", "GH1", "GH2", "GH3", "GH4", "GH5", "GH6"]);
  assert.equal(
    report.missing.every((row) => row.node_id.startsWith("GH")),
    true,
  );
  const reversed = syncIssues({
    adapter,
    mode: "check",
    nodes: ["GH2", "GH0", "GH1"],
    model: MODEL,
    ledgerPath,
  });
  assert.deepEqual(reversed.selection, ["GH0", "GH1", "GH2"]);
  const other = syncIssues({
    adapter,
    mode: "check",
    train: "governance.feedback-intake",
    model: MODEL,
    ledgerPath,
  });
  assert.deepEqual(other.selection, ["FB0", "FB1", "FB2"]);
  assert.equal(other.selection.includes("GH0"), false);
});

test("generated issue body ends with exactly one provenance footer", () => {
  const { body } = rendered("GH0");
  const lines = body.split("\n");
  assert.equal(lines.at(-1), "");
  assert.equal(lines.at(-2), "AI-Generated");
  assert.equal(lines.filter((line) => line === "AI-Generated").length, 1);
});

test("GH2-AC1 missing GH1 ancestor aborts without writing", () => {
  const adapter = fake({ nextIssueNumber: 3 });
  const ledgerPath = writeLedger({ implemented: ["ORC0", "GH0"] });
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH0"],
        model: MODEL,
        ledgerPath,
        clearance: clearanceFor(adapter),
      }),
    MissingAncestorError,
  );
  assert.equal(adapter.getIssues().length, 0);
  assert.equal(listGitHubIssues(readLedger(ledgerPath)).length, 0);
});

test("GH2-AC1 mapped issue that cannot be read unambiguously aborts", () => {
  const adapter = fake();
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 99, sync_to_github: true }],
  });
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "check",
        nodes: ["GH0"],
        model: MODEL,
        ledgerPath,
      }),
    UnstructuredGitHubOutputError,
  );
});

test("GH2 apply aborts a missing mapped issue as unstructured without updateIssue", () => {
  const adapter = fake();
  let updates = 0;
  const original = adapter.updateIssue.bind(adapter);
  adapter.updateIssue = (...args) => {
    updates += 1;
    return original(...args);
  };
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 99, sync_to_github: true }],
  });
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH0"],
        model: MODEL,
        ledgerPath,
        clearance: clearanceFor(adapter),
      }),
    UnstructuredGitHubOutputError,
  );
  assert.equal(updates, 0);
  assert.equal(
    adapter.reads.some((row) => row.kind === "get-issue" && row.number === 99),
    true,
  );
  assert.equal(adapter.getIssues().length, 0);
});

test("GH2 apply aborts a PR-shaped mapped GET without PATCH", () => {
  const transport = liveTransport({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": WRITABLE_REPO,
    "GET /repos/pikax/verter/labels?per_page=100": LABEL_CATALOG.labels,
    "GET /repos/pikax/verter/issues/15": {
      number: 15,
      title: "PR title",
      body: "PR body",
      pull_request: { url: "https://api.github.com/repos/pikax/verter/pulls/15" },
    },
  });
  const adapter = new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport,
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 15, sync_to_github: true }],
  });
  const clearance = clearanceFor(adapter);
  transport.calls.length = 0;
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH0"],
        model: MODEL,
        ledgerPath,
        clearance,
      }),
    UnstructuredGitHubOutputError,
  );
  assert.equal(
    transport.calls.some((row) => row.method === "PATCH"),
    false,
  );
  assert.equal(
    transport.calls.some(
      (row) => row.method === "GET" && row.path === "/repos/pikax/verter/issues/15",
    ),
    true,
  );
});

test("GH2 create that cannot write its mapping still reports the GitHub identity", () => {
  const adapter = fake({ nextIssueNumber: 22 });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH1", gh_issue: 22, sync_to_github: false }],
  });
  const { title } = rendered("GH0");
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH0"],
        model: MODEL,
        ledgerPath,
        clearance: clearanceFor(adapter),
      }),
    (error) => {
      assert.equal(error instanceof PartialFailureError, true);
      assert.equal(error.succeeded.length, 1);
      assert.equal(error.succeeded[0].node_id, "GH0");
      assert.equal(error.succeeded[0].gh_issue, 22);
      assert.equal(error.succeeded[0].kind, "create-issue");
      assert.equal(error.succeeded[0].mapping_written, false);
      assert.equal(error.succeeded[0].title, undefined);
      assert.equal(error.succeeded[0].body, undefined);
      assert.match(error.message, /22/u);
      assert.match(error.message, /GH0/u);
      assert.equal(error.message.includes(title), false);
      assert.equal(error.failed.error instanceof DuplicateError, true);
      return true;
    },
  );
  assert.equal(adapter.getIssue(22).title, title);
  assert.deepEqual(listGitHubIssues(readLedger(ledgerPath)), [
    { node_id: "GH1", gh_issue: 22, sync_to_github: false },
  ]);
});

test("GH2 CLI prints PartialFailureError identity rows without titles", () => {
  const selectedId = CLI_CONTENT_NODE;
  const protectedId = AUTHORITY.nodes.find((item) => item.id !== selectedId).id;
  const ledgerPath = writeLedger({
    issues: [{ node_id: protectedId, gh_issue: 1, sync_to_github: false }],
  });
  const { title } = rendered(selectedId);
  const result = spawnSync(
    process.execPath,
    [
      CLI,
      "sync-issues",
      "--apply",
      "--fake",
      "--nodes",
      selectedId,
      "--ledger",
      ledgerPath,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /ERROR:/u);
  assert.equal(result.stderr.includes(title), false);
  const identities = result.stderr
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.startsWith("{"))
    .map((line) => JSON.parse(line));
  assert.equal(
    identities.length,
    LABEL_CATALOG.labels.length + MILESTONE_CATALOG.milestones.length + 1,
  );
  assert.deepEqual(
    identities.slice(0, LABEL_CATALOG.labels.length),
    LABEL_CATALOG.labels.map((label) => ({
      kind: "create-repository-label",
      label: label.name,
    })),
  );
  assert.deepEqual(
    identities.slice(LABEL_CATALOG.labels.length, -1),
    MILESTONE_CATALOG.milestones.map((milestone) => ({
      kind: "create-repository-milestone",
      milestone: milestone.title,
    })),
  );
  const issueIdentity = identities.at(-1);
  assert.equal(issueIdentity.node_id, selectedId);
  assert.equal(issueIdentity.number, 1);
  assert.equal(issueIdentity.mapping_written, false);
  assert.equal(issueIdentity.title, undefined);
  assert.equal(issueIdentity.body, undefined);
});

test("GH2 live getIssue reads JSON number/title/body and does not classify payload.status", () => {
  const calls = [];
  const adapter = new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport: {
      request(req) {
        calls.push(req);
        if (req.method === "GET" && req.path === "/repos/pikax/verter/issues/4") {
          return {
            number: 4,
            title: "from gitHub",
            body: "payload",
            status: 404,
            html_url: "https://github.com/pikax/verter/issues/99",
          };
        }
        if (req.method === "GET" && req.path === "/repos/pikax/verter/issues/8") {
          throw new NotFoundError("Not Found");
        }
        throw new Error(`unexpected ${req.method} ${req.path}`);
      },
    },
  });
  assert.deepEqual(adapter.getIssue(4), { number: 4, title: "from gitHub", body: "payload" });
  assert.equal(calls[0].path, "/repos/pikax/verter/issues/4");
  assert.throws(() => adapter.getIssue(8), NotFoundError);
  assert.throws(() => adapter.getIssue("4"), /positive safe integer/u);
});

test("live label mutations use additive and single-label endpoints", () => {
  const desired = LABEL_CATALOG.labels.find((label) => label.name === "area:tooling");
  const transport = liveTransport({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": WRITABLE_REPO,
    "GET /repos/pikax/verter/labels?per_page=100": [
      { name: "area:tooling", color: "ffffff", description: "old" },
    ],
    "POST /repos/pikax/verter/labels": desired,
    "PATCH /repos/pikax/verter/labels/area%3Atooling": desired,
    "POST /repos/pikax/verter/issues/4/labels": [desired],
    "DELETE /repos/pikax/verter/issues/4/labels/problem%3Aarchitecture": [desired],
  });
  const adapter = new GitHubAdapter({ owner: "pikax", repo: "verter", transport });
  const clearance = clearanceFor(adapter);
  const mapping = { node_id: "test", gh_issue: 4, sync_to_github: true };
  transport.calls.length = 0;

  assert.deepEqual(adapter.getRepositoryLabels(), [
    { name: "area:tooling", color: "ffffff", description: "old" },
  ]);
  adapter.createRepositoryLabel({ label: desired, mode: "apply", clearance });
  adapter.updateRepositoryLabel({
    existing: "area:tooling",
    label: desired,
    mode: "apply",
    clearance,
  });
  adapter.addIssueLabels({
    number: 4,
    labels: ["area:tooling"],
    mapping,
    mode: "apply",
    clearance,
  });
  adapter.removeIssueLabel({
    number: 4,
    label: "problem:architecture",
    mapping,
    mode: "apply",
    clearance,
  });

  assert.deepEqual(
    transport.calls.map(({ method, path }) => `${method} ${path}`),
    [
      "GET /repos/pikax/verter/labels?per_page=100",
      "POST /repos/pikax/verter/labels",
      "PATCH /repos/pikax/verter/labels/area%3Atooling",
      "POST /repos/pikax/verter/issues/4/labels",
      "DELETE /repos/pikax/verter/issues/4/labels/problem%3Aarchitecture",
    ],
  );
  assert.deepEqual(transport.calls[1].body, desired);
  assert.deepEqual(transport.calls[2].body, {
    new_name: desired.name,
    color: desired.color,
    description: desired.description,
  });
  assert.deepEqual(transport.calls[3].body, { labels: ["area:tooling"] });
  assert.equal(
    transport.calls.some((call) => call.method === "PUT"),
    false,
  );
});

test("GH2 CLI sync-issues --check and --apply require an explicit selection and --fake", () => {
  const ledgerPath = writeLedger();
  const missingMode = spawnSync(
    process.execPath,
    [CLI, "sync-issues", "--fake", "--nodes", CLI_CONTENT_NODE],
    {
      encoding: "utf8",
    },
  );
  assert.notEqual(missingMode.status, 0);
  assert.match(missingMode.stderr, /--check|--apply/u);
  const check = spawnSync(
    process.execPath,
    [
      CLI,
      "sync-issues",
      "--check",
      "--fake",
      "--nodes",
      CLI_CONTENT_NODE,
      "--ledger",
      ledgerPath,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.equal(check.status, 0, check.stderr);
  const checked = JSON.parse(check.stdout);
  assert.equal(checked.mode, "check");
  assert.deepEqual(
    checked.missing.map((row) => row.node_id),
    [CLI_CONTENT_NODE],
  );
  assert.equal(fs.readFileSync(ledgerPath, "utf8").includes("[[github_issue]]"), false);
  const refreshLedger = writeLedger({
    issues: [{ node_id: SYNCABLE_ID, gh_issue: 1, sync_to_github: true }],
  });
  const refreshWithoutModel = spawnSync(
    process.execPath,
    [
      CLI,
      "sync-issues",
      "--apply",
      "--fake",
      "--nodes",
      SYNCABLE_ID,
      "--refresh-content",
      "--ledger",
      refreshLedger,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(refreshWithoutModel.status, 0);
  assert.doesNotMatch(refreshWithoutModel.stderr, /requires a model/u);
  const apply = spawnSync(
    process.execPath,
    [
      CLI,
      "sync-issues",
      "--apply",
      "--fake",
      "--nodes",
      CLI_CONTENT_NODE,
      "--ledger",
      ledgerPath,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.equal(apply.status, 0, apply.stderr);
  const applied = JSON.parse(apply.stdout);
  assert.equal(applied.created[0].gh_issue, 1);
  assert.equal(applied.created[0].mapping_written, true);
  assert.deepEqual(listGitHubIssues(readLedger(ledgerPath)), [
    { node_id: CLI_CONTENT_NODE, gh_issue: 1, sync_to_github: true },
  ]);
});

test("GH2 apply in tests refuses the live ledger path", () => {
  const adapter = fake();
  assert.equal(fs.existsSync(LIVE_LEDGER), true);
  const before = fs.readFileSync(LIVE_LEDGER, "utf8");
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH0"],
        model: MODEL,
        ledgerPath: LIVE_LEDGER,
        clearance: clearanceFor(adapter),
      }),
    /tests must pass --ledger/i,
  );
  assert.equal(fs.readFileSync(LIVE_LEDGER, "utf8"), before);
});
