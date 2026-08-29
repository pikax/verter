import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  ClosingLinkError,
  DoctorRequiredError,
  DuplicateError,
  FakeGitHubAdapter,
  GitHubAdapter,
  GitHubDoctor,
  MissingAncestorError,
  MissingIssueMappingError,
  PartialFailureError,
  PermissionDeniedError,
  SelectionError,
  WrongRepositoryError,
  createPr,
  mappedClosingLink,
} from "../index.mjs";
import { parseToml } from "../../../roadmap/0.1.0-tama/tools/lib.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const CLI = path.join(HERE, "../githubctl.mjs");
const TOOLS = path.join(REPO_ROOT, "roadmap/0.1.0-tama/tools");
const LIVE_LEDGER = path.join(REPO_ROOT, "roadmap/0.1.0-tama/authority/state/implemented.toml");
const CONTRACT = path.join(REPO_ROOT, "roadmap/0.1.0-tama/contracts/github-control-plane.md");
const TITLE = "feat(ci): example final title";
const HEAD = "train/example";

// GH3-AC3 N/A: create-pr does not own cache, incremental, or warm-admission authority.
// GH3-AC4 N/A: create-pr is an occasional CLI mutation, not a hot parse/resolve path.

function fake(options = {}) {
  return new FakeGitHubAdapter({ owner: "pikax", repo: "verter", ...options });
}

function clearanceFor(adapter, require = ["issues", "pullRequests"]) {
  const report = new GitHubDoctor(adapter).check({ require });
  assert.equal(report.ok, true, report.errors?.join?.("; ") ?? "doctor failed");
  return report.clearance;
}

function implementedBlock(id, pullRequest) {
  const locator = pullRequest == null ? "" : `pull_request = ${pullRequest}\n`;
  return `[[implemented]]
node_id = "${id}"
commit_message = "test locator ${id}"
commit_date = "2026-08-28T00:00:00+00:00"
${locator}`;
}

function mappingBlock(nodeId, issue, syncToGithub) {
  return `[[github_issue]]
node_id = "${nodeId}"
gh_issue = ${issue}
sync_to_github = ${syncToGithub}
`;
}

function writeLedger(options = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-create-pr-"));
  const file = path.join(dir, "implemented.toml");
  const implemented = options.implemented ?? ["ORC0", "GH0", "GH1", "GH2"];
  const locators = options.locators ?? {};
  const issues = options.issues ?? [];
  const parts = ["schema = 1", "", ...implemented.map((id) => implementedBlock(id, locators[id]))];
  for (const row of issues) parts.push(mappingBlock(row.node_id, row.gh_issue, row.sync_to_github));
  fs.writeFileSync(file, parts.join("\n"));
  return file;
}

function pullsForHeadPath(owner, repo, head) {
  return `/repos/${owner}/${repo}/pulls?head=${encodeURIComponent(`${owner}:${head}`)}&per_page=100`;
}

function readLedger(file) {
  return parseToml(fs.readFileSync(file, "utf8"));
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
      const key = `${req.method} ${req.path}`;
      if (!Object.hasOwn(withProject, key)) throw new Error(`unexpected ${key}`);
      const hit = withProject[key];
      if (hit instanceof Error) throw hit;
      return hit;
    },
  };
}

function baseOptions(adapter, extra = {}) {
  return {
    adapter,
    node: extra.node ?? "GH0",
    title: extra.title ?? TITLE,
    head: extra.head ?? HEAD,
    base: extra.base,
    body: extra.body,
    authority: extra.authority,
    createPrPrerequisites: extra.createPrPrerequisites,
    ledgerPath: extra.ledgerPath,
    writeLocator: extra.writeLocator,
    owner: extra.owner,
    repo: extra.repo,
    clearance: extra.clearance,
    mode: extra.mode,
  };
}

test("GH3-AC1 missing mapping creates no pull request", () => {
  const adapter = fake({ nextPullNumber: 20 });
  const ledgerPath = writeLedger();
  assert.throws(
    () =>
      createPr(
        baseOptions(adapter, {
          mode: "apply",
          ledgerPath,
          clearance: clearanceFor(adapter),
        }),
      ),
    MissingIssueMappingError,
  );
  assert.equal(adapter.getPullRequests().length, 0);
  assert.equal(adapter.getIssues().length, 0);
  assert.equal(readLedger(ledgerPath).github_issue, undefined);
});

test("GH3-AC1 protected mapping never calls updateIssue and still closes the mapped issue", () => {
  const originalBody = "pre-existing protected prose";
  const adapter = fake({
    nextPullNumber: 30,
    issues: [
      {
        number: 7,
        title: "kept title",
        body: originalBody,
        comments: [{ id: 1, body: "discussion" }],
      },
    ],
  });
  let updates = 0;
  const originalUpdate = adapter.updateIssue.bind(adapter);
  adapter.updateIssue = (...args) => {
    updates += 1;
    return originalUpdate(...args);
  };
  let added = 0;
  adapter.addIssueToProject = () => {
    added += 1;
    throw new Error("create-pr must not attach Project 3");
  };
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 7, sync_to_github: false }],
  });
  const report = createPr(
    baseOptions(adapter, {
      mode: "apply",
      ledgerPath,
      clearance: clearanceFor(adapter),
    }),
  );
  assert.equal(report.ok, true);
  assert.equal(report.pull_request.applied, true);
  assert.equal(report.pull_request.number, 30);
  assert.equal(report.pull_request.title, TITLE);
  assert.equal(report.pull_request.closes, 7);
  assert.equal(report.pull_request.body, `${mappedClosingLink(7)}\n`);
  assert.equal(report.issue.kind, "protected");
  assert.equal(report.issue.applied, false);
  assert.equal(updates, 0);
  assert.equal(added, 0);
  assert.deepEqual(adapter.getProjectItems(3), []);
  const issue = adapter.getIssue(7);
  assert.equal(issue.title, "kept title");
  assert.equal(issue.body, originalBody);
  assert.deepEqual(issue.comments, [{ id: 1, body: "discussion" }]);
});

test("GH3-AC1 write-locator does not invent an implemented row", () => {
  const adapter = fake({ nextPullNumber: 8 });
  const ledgerPath = writeLedger({
    implemented: ["ORC0", "GH1", "GH2"],
    issues: [{ node_id: "GH0", gh_issue: 4, sync_to_github: false }],
  });
  const before = readLedger(ledgerPath);
  assert.equal(
    before.implemented.some((row) => row.node_id === "GH0"),
    false,
  );
  const report = createPr(
    baseOptions(adapter, {
      mode: "apply",
      ledgerPath,
      writeLocator: true,
      clearance: clearanceFor(adapter),
    }),
  );
  assert.equal(report.pull_request.number, 8);
  assert.equal(report.locator.written, false);
  assert.equal(report.locator.pull_request, 8);
  const after = readLedger(ledgerPath);
  assert.deepEqual(after.implemented, before.implemented);
  assert.equal(
    after.implemented.some((row) => row.node_id === "GH0"),
    false,
  );
  assert.equal(fs.readFileSync(ledgerPath, "utf8").includes("pull_request"), false);
});

test("GH3-AC1 live duplicate head aborts check and apply without POST /pulls", () => {
  assert.equal(typeof GitHubAdapter.prototype.getPullRequests, "undefined");
  assert.equal(typeof GitHubAdapter.prototype.pullsForHead, "function");
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 4, sync_to_github: true }],
  });
  const pullsPath = pullsForHeadPath("pikax", "verter", HEAD);
  const transport = liveTransport({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": WRITABLE_REPO,
    [`GET ${pullsPath}`]: [{ number: 3, head: { ref: HEAD } }],
    "GET /repos/pikax/verter/issues/4": { number: 4, title: "stale", body: "stale" },
    "PATCH /repos/pikax/verter/issues/4": { number: 4, title: "stale", body: "stale" },
    "POST /repos/pikax/verter/pulls": { number: 99 },
  });
  const live = new GitHubAdapter({ owner: "pikax", repo: "verter", transport });
  const clearance = clearanceFor(live);
  transport.calls.length = 0;
  assert.throws(
    () =>
      createPr(
        baseOptions(live, {
          mode: "check",
          ledgerPath,
        }),
      ),
    DuplicateError,
  );
  assert.equal(
    transport.calls.some((row) => row.method === "POST" && String(row.path).includes("/pulls")),
    false,
  );
  assert.equal(
    transport.calls.some((row) => row.method === "PATCH"),
    false,
  );
  assert.equal(
    transport.calls.some((row) => row.method === "GET" && row.path === pullsPath),
    true,
  );
  transport.calls.length = 0;
  assert.throws(
    () =>
      createPr(
        baseOptions(live, {
          mode: "apply",
          ledgerPath,
          clearance,
        }),
      ),
    DuplicateError,
  );
  assert.equal(
    transport.calls.some((row) => row.method === "POST"),
    false,
  );
  assert.equal(
    transport.calls.some((row) => row.method === "PATCH"),
    false,
  );
});

for (const row of [
  { label: "write-locator", writeLocator: true, head: "train/other" },
  { label: "default", writeLocator: false, head: "train/other" },
]) {
  test(`GH3-AC1 ${row.label} aborts before create when the implemented row already locates a pull request`, () => {
    const adapter = fake({
      nextPullNumber: 80,
      issues: [{ number: 4, title: "old title", body: "old body" }],
    });
    const ledgerPath = writeLedger({
      locators: { GH0: 55 },
      issues: [{ node_id: "GH0", gh_issue: 4, sync_to_github: true }],
    });
    const clearance = clearanceFor(adapter);
    const beforeLedger = fs.readFileSync(ledgerPath, "utf8");
    const beforeState = adapter.inspectState();
    const extra = row.writeLocator ? { writeLocator: true } : {};
    for (const mode of ["check", "apply"]) {
      assert.throws(
        () =>
          createPr(
            baseOptions(adapter, {
              mode,
              head: row.head,
              ledgerPath,
              ...extra,
              ...(mode === "apply" ? { clearance } : {}),
            }),
          ),
        (error) => {
          assert.equal(error instanceof DuplicateError, true);
          assert.match(error.message, /already locates pull_request 55/u);
          return true;
        },
      );
    }
    assert.deepEqual(adapter.inspectState(), beforeState);
    assert.equal(adapter.getPullRequests().length, 0);
    assert.equal(adapter.getIssue(4).title, "old title");
    assert.equal(adapter.getIssue(4).body, "old body");
    assert.equal(fs.readFileSync(ledgerPath, "utf8"), beforeLedger);
  });
}

test("GH3-AC1 duplicate head aborts without a second pull request", () => {
  const adapter = fake({
    pullRequests: [
      {
        number: 3,
        title: "existing",
        body: "Closes #4",
        head: HEAD,
        base: "main",
        closes: 4,
      },
    ],
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 4, sync_to_github: false }],
  });
  assert.throws(
    () =>
      createPr(
        baseOptions(adapter, {
          mode: "check",
          ledgerPath,
        }),
      ),
    DuplicateError,
  );
  assert.throws(
    () =>
      createPr(
        baseOptions(adapter, {
          mode: "apply",
          ledgerPath,
          clearance: clearanceFor(adapter),
        }),
      ),
    DuplicateError,
  );
  assert.equal(adapter.getPullRequests().length, 1);
  assert.equal(adapter.getPullRequest(3).title, "existing");
});

test("GH3-AC1 programctl stays GitHub-blind, create-pr is one node, and batch is forbidden", () => {
  for (const name of ["programctl.mjs", "lib.mjs"]) {
    const text = fs.readFileSync(path.join(TOOLS, name), "utf8");
    assert.doesNotMatch(text, /\bgh\s+api\b/u);
    assert.doesNotMatch(text, /\bfetch\s*\(/u);
    assert.doesNotMatch(text, /githubctl/u);
  }
  const adapter = fake();
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 4, sync_to_github: false }],
  });
  assert.throws(
    () =>
      createPr({
        ...baseOptions(adapter, { mode: "check", ledgerPath }),
        train: "governance.github-control-plane",
      }),
    SelectionError,
  );
  assert.throws(
    () =>
      createPr({
        ...baseOptions(adapter, { mode: "check", ledgerPath }),
        nodes: ["GH0", "GH1"],
      }),
    SelectionError,
  );
  const batched = spawnSync(
    process.execPath,
    [CLI, "create-pr", "--check", "--fake", "--train", "governance.github-control-plane"],
    { encoding: "utf8" },
  );
  assert.notEqual(batched.status, 0);
  assert.match(batched.stderr, /--node|batch/iu);
  assert.equal(adapter.getPullRequests().length, 0);
});

test("GH3-AC1 issue and PR omit DAG metadata", () => {
  const adapter = fake({
    nextPullNumber: 11,
    issues: [{ number: 9, title: "stale", body: "stale" }],
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH3", gh_issue: 9, sync_to_github: true }],
  });
  const report = createPr(
    baseOptions(adapter, {
      node: "GH3",
      mode: "apply",
      ledgerPath,
      clearance: clearanceFor(adapter),
    }),
  );
  const { title } = adapter.getIssue(9);
  const body = adapter.getIssue(9).body;
  assert.equal(title, "stale");
  assert.equal(body, "stale\n\nAI-Generated\n");
  assert.doesNotMatch(body, /\bGH3\b/u);
  assert.doesNotMatch(body, /predecessors\s*=/u);
  assert.doesNotMatch(body, /implementation_effort/u);
  assert.doesNotMatch(title, /\bGH3\b/u);
  assert.doesNotMatch(report.pull_request.body, /\bGH3\b/u);
  assert.doesNotMatch(report.pull_request.body, /predecessors\s*=/u);
  assert.doesNotMatch(report.pull_request.body, /implementation_effort/u);
  assert.doesNotMatch(report.pull_request.title, /\bGH3\b/u);
});

test("GH3-AC1 missing GH2 ancestor aborts without writing", () => {
  const adapter = fake({ nextPullNumber: 2 });
  const ledgerPath = writeLedger({
    implemented: ["ORC0", "GH0", "GH1"],
    issues: [{ node_id: "GH0", gh_issue: 4, sync_to_github: false }],
  });
  assert.throws(
    () =>
      createPr(
        baseOptions(adapter, {
          mode: "apply",
          ledgerPath,
          clearance: clearanceFor(adapter),
        }),
      ),
    MissingAncestorError,
  );
  assert.equal(adapter.getPullRequests().length, 0);
});

test("GH3-AC1 check does not mutate GitHub or the ledger", () => {
  const adapter = fake({
    nextPullNumber: 5,
    issues: [{ number: 4, title: "stale", body: "stale" }],
  });
  const ledgerPath = writeLedger({
    implemented: ["ORC0", "GH0", "GH1", "GH2", "GH3"],
    issues: [{ node_id: "GH3", gh_issue: 4, sync_to_github: true }],
  });
  const beforeState = adapter.inspectState();
  const beforeLedger = fs.readFileSync(ledgerPath, "utf8");
  const report = createPr(
    baseOptions(adapter, {
      node: "GH3",
      mode: "check",
      ledgerPath,
      writeLocator: true,
    }),
  );
  assert.equal(report.mode, "check");
  assert.equal(report.pull_request.applied, false);
  assert.equal(report.pull_request.number, undefined);
  assert.equal(report.issue.kind, "update-issue");
  assert.equal(report.issue.applied, false);
  assert.equal(report.locator.written, false);
  assert.deepEqual(adapter.inspectState(), beforeState);
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), beforeLedger);
});

test("opening a pull request preserves issue prose and normalizes provenance", () => {
  const adapter = fake({
    nextPullNumber: 21,
    issues: [
      {
        number: 12,
        title: "old title",
        body: "old body",
        comments: [
          { id: 1, body: "first" },
          { id: 2, body: "second" },
        ],
      },
    ],
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH1", gh_issue: 12, sync_to_github: true }],
  });
  const report = createPr(
    baseOptions(adapter, {
      node: "GH1",
      mode: "apply",
      ledgerPath,
      clearance: clearanceFor(adapter),
    }),
  );
  assert.equal(report.ok, true);
  assert.equal(report.issue.kind, "update-issue");
  assert.equal(report.issue.applied, true);
  assert.equal(report.issue.number, 12);
  const issue = adapter.getIssue(12);
  assert.equal(issue.number, 12);
  assert.equal(issue.title, "old title");
  assert.equal(issue.body, "old body\n\nAI-Generated\n");
  assert.deepEqual(issue.comments, [
    { id: 1, body: "first" },
    { id: 2, body: "second" },
  ]);
});

test("opening a pull request does not rewrite an already normalized issue", () => {
  const adapter = fake({
    nextPullNumber: 22,
    issues: [{ number: 13, title: "stable title", body: "stable body\n\nAI-Generated\n" }],
  });
  let updates = 0;
  const updateIssue = adapter.updateIssue.bind(adapter);
  adapter.updateIssue = (...args) => {
    updates += 1;
    return updateIssue(...args);
  };
  const ledgerPath = writeLedger({
    implemented: ["READY"],
    issues: [{ node_id: "WORK", gh_issue: 13, sync_to_github: true }],
  });
  const authority = {
    nodes: [
      { id: "READY", predecessors: [] },
      { id: "WORK", predecessors: [] },
    ],
    ledgerFile: path.join(path.dirname(ledgerPath), "live-implemented.toml"),
  };

  const report = createPr(
    baseOptions(adapter, {
      node: "WORK",
      mode: "apply",
      ledgerPath,
      authority,
      createPrPrerequisites: [],
      clearance: clearanceFor(adapter),
    }),
  );

  assert.equal(report.pull_request.applied, true);
  assert.equal(report.issue.changed, false);
  assert.equal(report.issue.applied, false);
  assert.equal(updates, 0);
  assert.equal(adapter.getIssue(13).body, "stable body\n\nAI-Generated\n");
});

test("GH3-AC2 final title and exactly one mapped Closes #n; Fixes/Close are rejected", () => {
  const adapter = fake({ nextPullNumber: 40 });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 44, sync_to_github: false }],
  });
  const link = mappedClosingLink(44);
  const report = createPr(
    baseOptions(adapter, {
      mode: "apply",
      body: "Please review this change.",
      ledgerPath,
      clearance: clearanceFor(adapter),
    }),
  );
  assert.equal(report.pull_request.title, TITLE);
  assert.equal(report.pull_request.body, `Please review this change.\n\n${link}\n`);
  assert.equal([...report.pull_request.body.matchAll(/Closes #\d+/g)].length, 1);
  assert.doesNotMatch(report.pull_request.body, /\bFixes #/u);
  assert.doesNotMatch(report.pull_request.body, /\bClose #/u);
  assert.equal(adapter.getPullRequest(40).title, TITLE);
  assert.equal(adapter.getPullRequest(40).closes, 44);
  assert.throws(
    () =>
      createPr(
        baseOptions(adapter, {
          mode: "check",
          head: "train/other",
          body: "Fixes #44",
          ledgerPath,
        }),
      ),
    ClosingLinkError,
  );
  assert.throws(
    () =>
      createPr(
        baseOptions(adapter, {
          mode: "check",
          head: "train/other",
          body: `Notes\n\n${link}`,
          ledgerPath,
        }),
      ),
    ClosingLinkError,
  );
  assert.throws(
    () =>
      createPr(
        baseOptions(adapter, {
          mode: "check",
          title: "",
          head: "train/other",
          ledgerPath,
        }),
      ),
    /title is required/i,
  );
  assert.equal(adapter.getPullRequests().length, 1);
});

test("GH3-AC2 write-locator sets pull_request on an existing implemented row", () => {
  const adapter = fake({ nextPullNumber: 55 });
  const ledgerPath = writeLedger({
    implemented: ["ORC0", "GH0", "GH1", "GH2"],
    issues: [{ node_id: "GH2", gh_issue: 6, sync_to_github: false }],
  });
  const report = createPr(
    baseOptions(adapter, {
      node: "GH2",
      mode: "apply",
      ledgerPath,
      writeLocator: true,
      clearance: clearanceFor(adapter),
    }),
  );
  assert.equal(report.locator.written, true);
  assert.equal(report.locator.pull_request, 55);
  const after = readLedger(ledgerPath);
  const row = after.implemented.find((item) => item.node_id === "GH2");
  assert.equal(row.pull_request, 55);
  assert.equal(row.commit_message, "test locator GH2");
  assert.equal(row.commit_date, "2026-08-28T00:00:00+00:00");
  assert.equal(after.implemented.filter((item) => item.node_id === "GH2").length, 1);
});

test("GH3-AC2 apply requires issues+pullRequests clearance, not Project 3", () => {
  const adapter = fake({
    missing: true,
    nextPullNumber: 18,
    issues: [{ number: 4, title: "seed", body: "seed" }],
  });
  assert.equal(adapter.inspectCapabilities().projects, false);
  const full = new GitHubDoctor(adapter).check();
  assert.equal(full.ok, false);
  assert.equal(full.errors.includes("projects"), true);
  const gated = new GitHubDoctor(adapter).check({ require: ["issues", "pullRequests"] });
  assert.equal(gated.ok, true);
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 4, sync_to_github: true }],
  });
  const report = createPr(
    baseOptions(adapter, {
      mode: "apply",
      ledgerPath,
      clearance: gated.clearance,
    }),
  );
  assert.equal(report.pull_request.applied, true);
  assert.equal(report.issue.applied, true);
  assert.deepEqual(adapter.getProjectItems(3), []);
  assert.throws(
    () =>
      createPr(
        baseOptions(adapter, {
          mode: "apply",
          head: "train/second",
          ledgerPath,
        }),
      ),
    DoctorRequiredError,
  );
});

test("GH3-AC2 wrong repository and missing GH2-style live doctor fail closed", () => {
  const adapter = fake({
    issues: [{ number: 4, title: "seed", body: "seed" }],
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 4, sync_to_github: false }],
  });
  assert.throws(
    () =>
      createPr(
        baseOptions(adapter, {
          mode: "check",
          ledgerPath,
          owner: "other",
          repo: "verter",
        }),
      ),
    WrongRepositoryError,
  );
  assert.equal(adapter.getPullRequests().length, 0);
  const transport = liveTransport({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": WRITABLE_REPO,
  });
  const live = new GitHubAdapter({ owner: "pikax", repo: "verter", transport });
  const clearance = clearanceFor(live);
  transport.calls.length = 0;
  assert.throws(
    () =>
      createPr(
        baseOptions(live, {
          mode: "apply",
          ledgerPath,
          owner: "other",
          clearance,
        }),
      ),
    WrongRepositoryError,
  );
  assert.equal(
    transport.calls.some((row) => row.method === "POST"),
    false,
  );
});

test("GH3-AC2 CLI check plans and apply creates; check is non-mutating", () => {
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 4, sync_to_github: false }],
  });
  const before = fs.readFileSync(ledgerPath, "utf8");
  const missingMode = spawnSync(process.execPath, [CLI, "create-pr", "--fake", "--node", "GH0"], {
    encoding: "utf8",
  });
  assert.notEqual(missingMode.status, 0);
  assert.match(missingMode.stderr, /--check|--apply/u);
  const check = spawnSync(
    process.execPath,
    [
      CLI,
      "create-pr",
      "--check",
      "--fake",
      "--node",
      "GH0",
      "--title",
      TITLE,
      "--head",
      HEAD,
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
  const planned = JSON.parse(check.stdout);
  assert.equal(planned.mode, "check");
  assert.equal(planned.pull_request.applied, false);
  assert.equal(planned.pull_request.title, TITLE);
  assert.equal(planned.pull_request.body, `${mappedClosingLink(4)}\n`);
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), before);
  const apply = spawnSync(
    process.execPath,
    [
      CLI,
      "create-pr",
      "--apply",
      "--fake",
      "--node",
      "GH0",
      "--title",
      TITLE,
      "--head",
      HEAD,
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
  assert.equal(applied.pull_request.applied, true);
  assert.equal(applied.pull_request.number, 1);
  assert.equal(applied.pull_request.closes, 4);
  assert.equal(applied.issue.kind, "protected");
  const help = spawnSync(process.execPath, [CLI, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /create-pr/u);
  assert.doesNotMatch(help.stdout, /not a githubctl command/u);
});

test("GH3 apply reports PartialFailureError after a succeeded issue update", () => {
  const adapter = fake({
    nextPullNumber: 21,
    issues: [{ number: 12, title: "old", body: "old" }],
    failOnApply: 1,
    failOnApplyError: new PermissionDeniedError("pulls denied after update"),
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH1", gh_issue: 12, sync_to_github: true }],
  });
  assert.throws(
    () =>
      createPr(
        baseOptions(adapter, {
          node: "GH1",
          mode: "apply",
          ledgerPath,
          clearance: clearanceFor(adapter),
        }),
      ),
    (error) => {
      assert.equal(error instanceof PartialFailureError, true);
      assert.equal(error.succeeded.length, 1);
      assert.equal(error.succeeded[0].node_id, "GH1");
      assert.equal(error.succeeded[0].gh_issue, 12);
      assert.equal(error.succeeded[0].kind, "update-issue");
      assert.equal(error.succeeded[0].title, undefined);
      assert.equal(error.message.includes("old"), false);
      return true;
    },
  );
  assert.equal(adapter.getIssue(12).title, "old");
  assert.equal(adapter.getPullRequests().length, 0);
});

test("GH3 apply in tests refuses the live ledger path for locator writes", () => {
  const adapter = fake();
  const before = fs.readFileSync(LIVE_LEDGER, "utf8");
  assert.throws(
    () =>
      createPr(
        baseOptions(adapter, {
          mode: "apply",
          ledgerPath: LIVE_LEDGER,
          writeLocator: true,
          clearance: clearanceFor(adapter),
        }),
      ),
    /tests must pass --ledger/i,
  );
  assert.equal(fs.readFileSync(LIVE_LEDGER, "utf8"), before);
});

test("GH3 contract names ExpectedPullRequestTitle create-pr ownership", () => {
  const text = fs.readFileSync(CONTRACT, "utf8");
  assert.match(text, /^## ExpectedPullRequestTitle$/mu);
  const heading = text.indexOf("## ExpectedPullRequestTitle");
  const next = text.indexOf("\n## ", heading + 1);
  const section = text.slice(heading, next === -1 ? text.length : next);
  assert.match(section, /githubctl create-pr/u);
  assert.match(section, /mappedClosingLink/u);
  assert.match(section, /Project 3 is not required/u);
  assert.match(section, /never invents a row/u);
  assert.match(section, /pullsForHead/u);
  assert.match(section, /already locates a pull request/u);
  assert.match(section, /independent of `--write-locator`/u);
});
