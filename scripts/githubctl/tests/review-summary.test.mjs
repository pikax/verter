import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  BlockingFindingError,
  DoctorRequiredError,
  FakeGitHubAdapter,
  GitHubAdapter,
  GitHubDoctor,
  MappingMismatchError,
  MissingAncestorError,
  MissingIssueMappingError,
  PartialFailureError,
  PermissionDeniedError,
  countAiGeneratedFooters,
  mappedClosingLink,
  reviewSummary,
} from "../index.mjs";
import { parseToml } from "../../../roadmap/0.1.0-tama/tools/lib.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const CLI = path.join(HERE, "../githubctl.mjs");
const TOOLS = path.join(REPO_ROOT, "roadmap/0.1.0-tama/tools");
const LIVE_LEDGER = path.join(REPO_ROOT, "roadmap/0.1.0-tama/authority/state/implemented.toml");
const CONTRACT = path.join(REPO_ROOT, "roadmap/0.1.0-tama/contracts/github-control-plane.md");
const HUMAN_BODY =
  "Problem: mapping must stay local.\nScope: ordinary PR prose.\nValidation: fake adapter.\nReview: human-written.";
const ISSUE_NUMBER = 4;
const PR_NUMBER = 10;

// GH4-AC3 N/A: review-summary does not own cache, incremental, or warm-admission authority.
// GH4-AC4 N/A: review-summary is an occasional CLI mutation, not a hot parse/resolve path.

function fake(options = {}) {
  return new FakeGitHubAdapter({ owner: "pikax", repo: "verter", ...options });
}

function clearanceFor(adapter, require = ["issues", "pullRequests", "projects"]) {
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
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-review-summary-"));
  const file = path.join(dir, "implemented.toml");
  const implemented = options.implemented ?? ["ORC0", "GH0", "GH1", "GH2", "GH3"];
  const locators = options.locators ?? {};
  const issues = options.issues ?? [];
  const parts = ["schema = 1", "", ...implemented.map((id) => implementedBlock(id, locators[id]))];
  for (const row of issues) parts.push(mappingBlock(row.node_id, row.gh_issue, row.sync_to_github));
  fs.writeFileSync(file, parts.join("\n"));
  return file;
}

function readLedger(file) {
  return parseToml(fs.readFileSync(file, "utf8"));
}

function seeded(options = {}) {
  const issueBody = options.issueBody ?? "pre-existing protected prose";
  return fake({
    nextNumber: options.nextNumber ?? 11,
    failOnApply: options.failOnApply,
    failOnApplyError: options.failOnApplyError,
    issues: options.issues ?? [
      {
        number: ISSUE_NUMBER,
        title: options.issueTitle ?? "kept title",
        body: issueBody,
        comments: options.issueComments ?? [{ id: 1, body: "discussion" }],
      },
    ],
    pullRequests: options.pullRequests ?? [
      {
        number: PR_NUMBER,
        title: "feat(ci): example",
        body: options.prBody ?? `${mappedClosingLink(ISSUE_NUMBER)}\n`,
        head: "train/example",
        base: "main",
        closes: ISSUE_NUMBER,
      },
    ],
  });
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
    pr: extra.pr ?? PR_NUMBER,
    verdict: extra.verdict ?? "FAIL",
    body: extra.body ?? HUMAN_BODY,
    findings: extra.findings,
    ledgerPath: extra.ledgerPath,
    owner: extra.owner,
    repo: extra.repo,
    clearance: extra.clearance,
    mode: extra.mode,
  };
}

function assertOrdinarySummary(text) {
  assert.match(text, /^Verdict: (?:PASS|FAIL)$/mu);
  assert.match(text, /Problem:/u);
  assert.doesNotMatch(text, /<!--/u);
  assert.doesNotMatch(text, /managed region/iu);
  assert.doesNotMatch(text, /node_id/u);
  assert.doesNotMatch(text, /predecessors/u);
  assert.doesNotMatch(text, /implementation_effort/u);
  assert.doesNotMatch(text, /^effort\s*[:=]/imu);
  assert.doesNotMatch(text, /sha256:/iu);
  assert.doesNotMatch(text, /\bdigest\b/iu);
  assert.doesNotMatch(text, /\b[0-9a-f]{40}\b/u);
  assert.doesNotMatch(text, /\bGH0\b/u);
}

test("GH4-AC1 protected mapping never edits the issue and still records a PR comment", () => {
  const originalBody = "pre-existing protected prose";
  const adapter = seeded({ issueBody: originalBody });
  let updates = 0;
  const originalUpdate = adapter.updateIssue.bind(adapter);
  adapter.updateIssue = (...args) => {
    updates += 1;
    return originalUpdate(...args);
  };
  let added = 0;
  adapter.addIssueToProject = () => {
    added += 1;
    throw new Error("review-summary must not attach Project 3");
  };
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: ISSUE_NUMBER, sync_to_github: false }],
  });
  const beforeLedger = fs.readFileSync(ledgerPath, "utf8");
  const report = reviewSummary(
    baseOptions(adapter, {
      mode: "apply",
      ledgerPath,
      clearance: clearanceFor(adapter),
    }),
  );
  assert.equal(report.ok, true);
  assert.equal(report.comment.applied, true);
  assert.equal(report.comment.number, PR_NUMBER);
  assert.equal(report.issue.kind, "protected");
  assert.equal(report.issue.applied, false);
  assertOrdinarySummary(report.comment.body);
  assert.equal(updates, 0);
  assert.equal(added, 0);
  assert.deepEqual(adapter.getProjectItems(3), []);
  const issue = adapter.getIssue(ISSUE_NUMBER);
  assert.equal(issue.title, "kept title");
  assert.equal(issue.body, originalBody);
  assert.deepEqual(issue.comments, [{ id: 1, body: "discussion" }]);
  assert.equal(countAiGeneratedFooters(issue.body), 0);
  const pull = adapter.getPullRequest(PR_NUMBER);
  assert.equal(pull.comments.length, 1);
  assert.equal(pull.comments[0].body, report.comment.body);
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), beforeLedger);
  assert.equal(
    readLedger(ledgerPath).implemented.find((row) => row.node_id === "GH0").pull_request,
    undefined,
  );
});

test("GH4-AC1 P0 and P1 findings block apply without GitHub or ledger writes", () => {
  const adapter = seeded({
    issueBody: "human work\n\nModel: stale\n",
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: ISSUE_NUMBER, sync_to_github: true }],
  });
  const beforeLedger = fs.readFileSync(ledgerPath, "utf8");
  const beforeState = adapter.inspectState();
  const clearance = clearanceFor(adapter);
  for (const severity of ["P0", "P1"]) {
    assert.throws(
      () =>
        reviewSummary(
          baseOptions(adapter, {
            mode: "apply",
            verdict: "FAIL",
            findings: [{ severity, owner: "reviewer", context: "must not accept" }],
            ledgerPath,
            clearance,
          }),
        ),
      BlockingFindingError,
    );
  }
  assert.deepEqual(adapter.inspectState(), beforeState);
  assert.equal(adapter.getPullRequest(PR_NUMBER).comments?.length ?? 0, 0);
  assert.equal(adapter.getIssue(ISSUE_NUMBER).body, "human work\n\nModel: stale\n");
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), beforeLedger);
});

test("GH4-AC1 PASS report with P0 findings is rejected in check and apply", () => {
  const adapter = seeded();
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: ISSUE_NUMBER, sync_to_github: false }],
  });
  const beforeState = adapter.inspectState();
  const findings = [{ severity: "P0", owner: "reviewer", context: "blocking defect" }];
  assert.throws(
    () =>
      reviewSummary(
        baseOptions(adapter, {
          mode: "check",
          verdict: "PASS",
          findings,
          ledgerPath,
        }),
      ),
    (error) => {
      assert.equal(error instanceof BlockingFindingError, true);
      assert.match(error.message, /PASS/u);
      assert.match(error.message, /P0/u);
      return true;
    },
  );
  assert.throws(
    () =>
      reviewSummary(
        baseOptions(adapter, {
          mode: "apply",
          verdict: "PASS",
          findings,
          ledgerPath,
          clearance: clearanceFor(adapter),
        }),
      ),
    BlockingFindingError,
  );
  assert.deepEqual(adapter.inspectState(), beforeState);
});

test("GH4-AC1 summary omits managed regions, DAG metadata, effort, and SHA digest binding", () => {
  const adapter = seeded();
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: ISSUE_NUMBER, sync_to_github: false }],
  });
  const report = reviewSummary(
    baseOptions(adapter, {
      mode: "check",
      verdict: "PASS",
      ledgerPath,
    }),
  );
  assert.equal(report.comment.applied, false);
  assertOrdinarySummary(report.comment.body);
  assert.equal(adapter.getPullRequest(PR_NUMBER).comments?.length ?? 0, 0);
  assert.equal(
    readLedger(ledgerPath).implemented.some((row) => row.node_id === "GH4"),
    false,
  );
});

test("GH4-AC1 missing mapping, missing ancestor, and issue-closure findings abort without writes", () => {
  const adapter = seeded();
  const missingMapping = writeLedger();
  assert.throws(
    () =>
      reviewSummary(
        baseOptions(adapter, {
          mode: "check",
          ledgerPath: missingMapping,
        }),
      ),
    MissingIssueMappingError,
  );
  const missingAncestor = writeLedger({
    implemented: ["ORC0", "GH0", "GH1", "GH2"],
    issues: [{ node_id: "GH0", gh_issue: ISSUE_NUMBER, sync_to_github: false }],
  });
  assert.throws(
    () =>
      reviewSummary(
        baseOptions(adapter, {
          mode: "apply",
          ledgerPath: missingAncestor,
          clearance: clearanceFor(adapter),
        }),
      ),
    MissingAncestorError,
  );
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: ISSUE_NUMBER, sync_to_github: false }],
  });
  const beforeState = adapter.inspectState();
  assert.throws(
    () =>
      reviewSummary(
        baseOptions(adapter, {
          mode: "check",
          findings: [
            {
              severity: "P2",
              owner: "reviewer",
              context: "follow up later",
              resolution: "close the issue",
            },
          ],
          ledgerPath,
        }),
      ),
    /not a finding resolution/i,
  );
  assert.throws(
    () =>
      reviewSummary(
        baseOptions(adapter, {
          mode: "apply",
          findings: [
            {
              severity: "P2",
              owner: "reviewer",
              context: "follow up later",
              close_issue: true,
            },
          ],
          ledgerPath,
          clearance: clearanceFor(adapter),
        }),
      ),
    /not a finding resolution/i,
  );
  assert.deepEqual(adapter.inspectState(), beforeState);
  assert.equal(adapter.getPullRequests().length, 1);
});

test("review publication keeps human prose and normalizes provenance", () => {
  const adapter = seeded({
    issueBody:
      "human work notes\n\nModel: first\nModel: second\nEffort: high\nimplementation_effort = high\n",
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: ISSUE_NUMBER, sync_to_github: true }],
  });
  const report = reviewSummary(
    baseOptions(adapter, {
      mode: "apply",
      ledgerPath,
      clearance: clearanceFor(adapter),
    }),
  );
  assert.equal(report.ok, true);
  assert.equal(report.issue.kind, "update-issue");
  assert.equal(report.issue.applied, true);
  const issue = adapter.getIssue(ISSUE_NUMBER);
  assert.match(issue.body, /human work notes/u);
  assert.match(issue.body, /\nAI-Generated\n$/u);
  assert.equal(countAiGeneratedFooters(issue.body), 1);
  assert.doesNotMatch(issue.body, /^Effort:/mu);
  assert.doesNotMatch(issue.body, /implementation_effort/u);
  assert.doesNotMatch(issue.body, /Model: first/u);
  assert.deepEqual(issue.comments, [{ id: 1, body: "discussion" }]);
  assert.equal(adapter.getPullRequest(PR_NUMBER).comments.length, 1);
});

test("GH4-AC2 PR comment records lower findings with owner and severity", () => {
  const adapter = seeded();
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: ISSUE_NUMBER, sync_to_github: false }],
  });
  const report = reviewSummary(
    baseOptions(adapter, {
      mode: "apply",
      verdict: "PASS",
      findings: [{ severity: "P2", owner: "alice", context: "naming nit in the adapter" }],
      ledgerPath,
      clearance: clearanceFor(adapter),
    }),
  );
  assert.equal(report.ok, true);
  assert.equal(report.comment.applied, true);
  assert.match(report.comment.body, /^Verdict: PASS$/mu);
  assert.match(report.comment.body, /- P2 \(alice\): naming nit in the adapter/u);
  assert.equal(adapter.getPullRequest(PR_NUMBER).comments[0].body, report.comment.body);
  assert.equal(adapter.getIssue(ISSUE_NUMBER).body, "pre-existing protected prose");
});

test("GH4-AC2 wrong PR/issue mapping aborts; a located PR without Closes is accepted", () => {
  const wrong = seeded({
    prBody: `${mappedClosingLink(99)}\n`,
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: ISSUE_NUMBER, sync_to_github: false }],
  });
  const before = wrong.inspectState();
  assert.throws(
    () =>
      reviewSummary(
        baseOptions(wrong, {
          mode: "apply",
          ledgerPath,
          clearance: clearanceFor(wrong),
        }),
      ),
    MappingMismatchError,
  );
  assert.deepEqual(wrong.inspectState(), before);

  const located = seeded({
    prBody: "ordinary review prose without a closing link\n",
  });
  const locatedLedger = writeLedger({
    locators: { GH0: PR_NUMBER },
    issues: [{ node_id: "GH0", gh_issue: ISSUE_NUMBER, sync_to_github: false }],
  });
  const report = reviewSummary(
    baseOptions(located, {
      mode: "apply",
      ledgerPath: locatedLedger,
      clearance: clearanceFor(located),
    }),
  );
  assert.equal(report.ok, true);
  assert.equal(report.comment.applied, true);
  assert.equal(located.getPullRequest(PR_NUMBER).comments.length, 1);
  assert.doesNotMatch(located.getPullRequest(PR_NUMBER).body, /Closes #/u);
});

test("GH4-AC2 apply requires issues+pullRequests clearance, not Project 3", () => {
  const adapter = seeded({ nextNumber: 11 });
  adapter.getProject = () => {
    throw new Error("review-summary must not require Project 3");
  };
  const missingProjects = fake({
    missing: true,
    issues: [
      {
        number: ISSUE_NUMBER,
        title: "kept title",
        body: "pre-existing protected prose",
      },
    ],
    pullRequests: [
      {
        number: PR_NUMBER,
        title: "feat(ci): example",
        body: `${mappedClosingLink(ISSUE_NUMBER)}\n`,
        head: "train/example",
        base: "main",
        closes: ISSUE_NUMBER,
      },
    ],
  });
  assert.equal(missingProjects.inspectCapabilities().projects, false);
  const full = new GitHubDoctor(missingProjects).check();
  assert.equal(full.ok, false);
  const gated = new GitHubDoctor(missingProjects).check({ require: ["issues", "pullRequests"] });
  assert.equal(gated.ok, true);
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: ISSUE_NUMBER, sync_to_github: false }],
  });
  const report = reviewSummary(
    baseOptions(missingProjects, {
      mode: "apply",
      ledgerPath,
      clearance: gated.clearance,
    }),
  );
  assert.equal(report.comment.applied, true);
  assert.throws(
    () =>
      reviewSummary(
        baseOptions(missingProjects, {
          mode: "apply",
          ledgerPath,
        }),
      ),
    DoctorRequiredError,
  );
  assert.equal(adapter.getPullRequest, FakeGitHubAdapter.prototype.getPullRequest);
});

test("GH4-AC2 live adapter posts a structured PR comment and does not PATCH a protected issue", () => {
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: ISSUE_NUMBER, sync_to_github: false }],
  });
  const commentPath = `/repos/pikax/verter/issues/${PR_NUMBER}/comments`;
  const pullPath = `/repos/pikax/verter/pulls/${PR_NUMBER}`;
  const transport = liveTransport({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": WRITABLE_REPO,
    [`GET ${pullPath}`]: {
      number: PR_NUMBER,
      title: "feat(ci): example",
      body: `${mappedClosingLink(ISSUE_NUMBER)}\n`,
      head: { ref: "train/example" },
      base: { ref: "main" },
    },
    [`POST ${commentPath}`]: { id: 77, body: "ignored" },
    [`GET /repos/pikax/verter/issues/${ISSUE_NUMBER}`]: {
      number: ISSUE_NUMBER,
      title: "kept",
      body: "kept",
    },
    [`PATCH /repos/pikax/verter/issues/${ISSUE_NUMBER}`]: {
      number: ISSUE_NUMBER,
      title: "kept",
      body: "kept",
    },
  });
  const live = new GitHubAdapter({ owner: "pikax", repo: "verter", transport });
  const clearance = clearanceFor(live, ["issues", "pullRequests"]);
  transport.calls.length = 0;
  const report = reviewSummary(
    baseOptions(live, {
      mode: "apply",
      ledgerPath,
      clearance,
    }),
  );
  assert.equal(report.comment.applied, true);
  assert.equal(report.issue.kind, "protected");
  const posted = transport.calls.find((row) => row.method === "POST" && row.path === commentPath);
  assert.equal(posted.body.body, report.comment.body);
  assert.equal(
    transport.calls.some((row) => row.method === "PATCH"),
    false,
  );
  assert.equal(
    transport.calls.some((row) => row.path === `/repos/pikax/verter/issues/${ISSUE_NUMBER}`),
    false,
  );
  assertOrdinarySummary(posted.body.body);
});

test("GH4-AC2 CLI check/apply flags, help, and contract name ReviewCycleSummary", () => {
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: ISSUE_NUMBER, sync_to_github: false }],
  });
  const missingMode = spawnSync(
    process.execPath,
    [CLI, "review-summary", "--fake", "--node", "GH0", "--pr", String(PR_NUMBER)],
    { encoding: "utf8" },
  );
  assert.notEqual(missingMode.status, 0);
  assert.match(missingMode.stderr, /--check|--apply/u);
  const check = spawnSync(
    process.execPath,
    [
      CLI,
      "review-summary",
      "--check",
      "--fake",
      "--node",
      "GH0",
      "--pr",
      String(PR_NUMBER),
      "--verdict",
      "FAIL",
      "--body",
      HUMAN_BODY,
      "--ledger",
      ledgerPath,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(check.status, 0);
  assert.match(check.stderr, /not found|missing/iu);
  const help = spawnSync(process.execPath, [CLI, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /review-summary/u);
  assert.doesNotMatch(help.stdout, /not a githubctl command/u);
  for (const name of ["programctl.mjs", "lib.mjs"]) {
    const text = fs.readFileSync(path.join(TOOLS, name), "utf8");
    assert.doesNotMatch(text, /\bgh\s+api\b/u);
    assert.doesNotMatch(text, /githubctl/u);
  }
  const contract = fs.readFileSync(CONTRACT, "utf8");
  assert.match(contract, /^## ReviewCycleSummary$/mu);
  const heading = contract.indexOf("## ReviewCycleSummary");
  const next = contract.indexOf("\n## ", heading + 1);
  const section = contract.slice(heading, next === -1 ? contract.length : next);
  assert.match(section, /githubctl review-summary/u);
  assert.match(section, /AI-Generated/u);
  assert.match(section, /P0\/P1/u);
  assert.match(section, /protected/iu);
  assert.doesNotMatch(section, /digest-bind/u);
});

test("GH4 apply reports PartialFailureError after a succeeded PR comment", () => {
  const adapter = seeded({
    issueBody: "human work\n\nModel: stale\nModel: extra\n",
    failOnApply: 1,
    failOnApplyError: new PermissionDeniedError("issue update denied after comment"),
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: ISSUE_NUMBER, sync_to_github: true }],
  });
  assert.throws(
    () =>
      reviewSummary(
        baseOptions(adapter, {
          mode: "apply",
          ledgerPath,
          clearance: clearanceFor(adapter),
        }),
      ),
    (error) => {
      assert.equal(error instanceof PartialFailureError, true);
      assert.equal(error.succeeded.length, 1);
      assert.equal(error.succeeded[0].kind, "create-pull-request-comment");
      assert.equal(error.succeeded[0].number, PR_NUMBER);
      return true;
    },
  );
  assert.equal(adapter.getPullRequest(PR_NUMBER).comments.length, 1);
  assert.match(adapter.getIssue(ISSUE_NUMBER).body, /Model: stale/u);
  assert.equal(countAiGeneratedFooters(adapter.getIssue(ISSUE_NUMBER).body), 0);
});

test("GH4 apply in tests refuses the live ledger path", () => {
  const adapter = seeded();
  const before = fs.readFileSync(LIVE_LEDGER, "utf8");
  assert.throws(
    () =>
      reviewSummary(
        baseOptions(adapter, {
          mode: "apply",
          ledgerPath: LIVE_LEDGER,
          clearance: clearanceFor(adapter),
        }),
      ),
    /tests must pass --ledger/i,
  );
  assert.equal(fs.readFileSync(LIVE_LEDGER, "utf8"), before);
});
