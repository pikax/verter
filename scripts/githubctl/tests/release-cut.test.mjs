import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  BlockingFindingError,
  ClosingLinkError,
  DoctorRequiredError,
  FakeGitHubAdapter,
  GitHubAdapter,
  GitHubDoctor,
  RELEASE_REHEARSAL,
  UnauthorizedReleaseError,
  createReleasePullRequest,
  releaseCut,
} from "../index.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const CLI = path.join(HERE, "../githubctl.mjs");
const CONTRACT = path.join(REPO_ROOT, "roadmap/0.1.0-tama/contracts/github-control-plane.md");
const WORKFLOWS = path.join(REPO_ROOT, ".github/workflows");
const VERSION = "0.1.0";
const TITLE = "release: v0.1.0";
const HEAD = "release/v0.1.0";
const TAMA_ROADMAP = "Tama Roadmap";

// REL2-AC3 N/A: release-cut does not own cache, incremental, or warm-admission authority.
// REL2-AC4 N/A: release-cut is occasional CLI coordination, not a hot parse/resolve path.

function fake(options = {}) {
  return new FakeGitHubAdapter({ owner: "pikax", repo: "verter", ...options });
}

function clearanceFor(adapter, require = ["pullRequests"]) {
  const report = new GitHubDoctor(adapter).check({ require });
  assert.equal(report.ok, true, report.errors?.join?.("; ") ?? "doctor failed");
  return report.clearance;
}

function productionSources() {
  return fs
    .readdirSync(path.join(HERE, ".."))
    .filter((name) => name.endsWith(".mjs"))
    .map((name) => ({
      name,
      text: fs.readFileSync(path.join(HERE, "..", name), "utf8"),
    }));
}

function cut(adapter, extra = {}) {
  return releaseCut({
    adapter,
    version: VERSION,
    head: HEAD,
    ...extra,
  });
}

function seededPull(options = {}) {
  return fake({
    nextNumber: options.nextNumber ?? 2,
    pullRequests: [
      {
        number: 1,
        title: options.title ?? TITLE,
        body: options.body ?? "",
        head: HEAD,
        base: "main",
        closes: null,
        checkRuns: options.checkRuns ?? [{ name: TAMA_ROADMAP, conclusion: "success" }],
      },
    ],
    milestones: options.milestones ?? [{ title: "v0.1.0", number: 1 }],
  });
}

test("REL2-AC1 apply without --authorize aborts and does not create a pull request", () => {
  const adapter = fake();
  assert.throws(
    () => cut(adapter, { mode: "apply", clearance: clearanceFor(adapter) }),
    UnauthorizedReleaseError,
  );
  assert.equal(adapter.getPullRequests().length, 0);
  const check = cut(adapter, { mode: "check" });
  assert.equal(check.ok, true);
  assert.equal(check.authorization.kind, "ReleaseCutAuthorization");
  assert.equal(check.authorization.authorized, false);
  assert.equal(check.pull_request.applied, false);
});

test("REL2-AC1 title that is not release: v… or that carries a PR suffix aborts", () => {
  const adapter = fake();
  const clearance = clearanceFor(adapter);
  assert.throws(
    () =>
      createReleasePullRequest(adapter, {
        title: "feat(ci): not a release",
        body: "",
        head: HEAD,
        base: "main",
        mode: "apply",
        clearance,
      }),
    /release: v/u,
  );
  assert.throws(
    () =>
      createReleasePullRequest(adapter, {
        title: "release: v0.1.0 (#12)",
        body: "",
        head: HEAD,
        base: "main",
        mode: "apply",
        clearance,
      }),
    /release: v/u,
  );
  assert.throws(() => cut(adapter, { mode: "check", version: "0.1.0 (#12)" }), /release: v/u);
  assert.equal(adapter.getPullRequests().length, 0);
});

test("REL2-AC1 GH3 createPullRequest cannot open a release PR and still requires Closes", () => {
  const adapter = fake();
  assert.throws(
    () =>
      adapter.createPullRequest({
        title: TITLE,
        body: "Closes #4\n",
        head: HEAD,
        base: "main",
        mappedIssue: 4,
        mode: "check",
      }),
    /createReleasePullRequest/u,
  );
  assert.throws(
    () =>
      adapter.createPullRequest({
        title: "feat(ci): mapped node",
        body: "no closing link",
        head: "train/example",
        base: "main",
        mappedIssue: 4,
        mode: "check",
      }),
    ClosingLinkError,
  );
  assert.equal(adapter.getPullRequests().length, 0);
});

test("REL2-AC1 release PR must not carry a GH3 Closes link", () => {
  const adapter = fake();
  const clearance = clearanceFor(adapter);
  assert.throws(
    () =>
      createReleasePullRequest(adapter, {
        title: TITLE,
        body: "Notes\n\nCloses #4\n",
        head: HEAD,
        base: "main",
        mode: "apply",
        clearance,
      }),
    ClosingLinkError,
  );
  assert.throws(
    () =>
      createReleasePullRequest(adapter, {
        title: TITLE,
        body: "Closes: #4\n",
        head: HEAD,
        base: "main",
        mode: "apply",
        clearance,
      }),
    ClosingLinkError,
  );
  assert.throws(
    () =>
      createReleasePullRequest(adapter, {
        title: TITLE,
        body: "Fixes pikax/verter#4\n",
        head: HEAD,
        base: "main",
        mode: "apply",
        clearance,
      }),
    ClosingLinkError,
  );
  assert.throws(
    () =>
      createReleasePullRequest(adapter, {
        title: TITLE,
        body: "Resolves https://github.com/pikax/verter/issues/4\n",
        head: HEAD,
        base: "main",
        mode: "apply",
        clearance,
      }),
    ClosingLinkError,
  );
  assert.throws(
    () =>
      createReleasePullRequest(adapter, {
        title: TITLE,
        body: "",
        head: HEAD,
        base: "main",
        mappedIssue: 4,
        mode: "check",
      }),
    /mapped issue|Closes/u,
  );
  assert.equal(adapter.getPullRequests().length, 0);
  assert.throws(() => cut(adapter, { mode: "check", body: "Closes #4" }), ClosingLinkError);
  assert.throws(() => cut(adapter, { mode: "check", body: "Closes: #4" }), ClosingLinkError);
  const check = spawnSync(
    process.execPath,
    [
      CLI,
      "release-cut",
      "--check",
      "--fake",
      "--version",
      VERSION,
      "--head",
      HEAD,
      "--owner",
      "pikax",
      "--repo",
      "verter",
      "--body",
      "Closes #4",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(check.status, 0);
  assert.match(check.stderr, /closing link/u);
  const lander = seededPull({ body: "Closes: #4" });
  assert.throws(
    () =>
      cut(lander, {
        mode: "apply",
        authorize: true,
        land: true,
        pr: 1,
        clearance: clearanceFor(lander),
        requiredJobs: [TAMA_ROADMAP],
      }),
    ClosingLinkError,
  );
  assert.deepEqual(lander.inspectState().merges, []);
});

test("REL2-AC1 apply never closes the milestone unless --close-milestone is explicit", () => {
  const adapter = fake({ milestones: [{ title: "v0.1.0", number: 1 }] });
  const created = cut(adapter, {
    mode: "apply",
    authorize: true,
    clearance: clearanceFor(adapter),
  });
  assert.equal(created.ok, true);
  assert.equal(created.milestone.close, false);
  assert.equal(created.milestone.applied, false);
  assert.deepEqual(adapter.milestoneCloses, []);
  assert.equal(adapter.milestoneWrites.length, 0);
  const closer = fake({ milestones: [{ title: "v0.1.0", number: 1 }] });
  const closed = cut(closer, {
    mode: "apply",
    authorize: true,
    closeMilestone: true,
    clearance: clearanceFor(closer),
  });
  assert.equal(closed.ok, true);
  assert.equal(closed.milestone.close, true);
  assert.equal(closed.milestone.applied, true);
  assert.deepEqual(closer.milestoneCloses, [{ title: "v0.1.0" }]);
});

test("REL2-AC1 P0 findings block release; GitHub closure cannot erase them", () => {
  const adapter = fake({
    milestones: [{ title: "v0.1.0", number: 1 }],
    issues: [{ number: 10, title: "finding", body: "x", milestone: "v0.1.0", state: "closed" }],
  });
  const findings = [{ issue: "10", severity: "P0", owner: "reviewer" }];
  const check = cut(adapter, { mode: "check", findings });
  assert.equal(check.ok, false);
  assert.equal(
    check.blockers.some((row) => row.reason === "finding" && row.severity === "P0"),
    true,
  );
  assert.throws(
    () =>
      cut(adapter, {
        mode: "apply",
        authorize: true,
        findings,
        clearance: clearanceFor(adapter),
      }),
    BlockingFindingError,
  );
  assert.equal(adapter.getPullRequests().length, 0);
  assert.throws(
    () => cut(adapter, { mode: "check", findings: [{ ...findings[0], closed: true }] }),
    /additional property closed/u,
  );
});

test("REL2-AC1 no duplicate tag or publish workflow is introduced", () => {
  const files = fs.readdirSync(WORKFLOWS).filter((name) => /\.ya?ml$/u.test(name));
  assert.equal(files.includes("release-tag.yml"), true);
  assert.equal(files.includes("release.yml"), true);
  assert.equal(files.includes("release-check.yml"), true);
  const taggers = files.filter((name) => {
    if (name === "release-tag.yml") return false;
    const text = fs.readFileSync(path.join(WORKFLOWS, name), "utf8");
    return /git tag -a "v\$\{\{/u.test(text) || /name:\s*release-tag\b/u.test(text);
  });
  assert.deepEqual(taggers, []);
  for (const source of productionSources()) {
    assert.doesNotMatch(source.text, /writeFile(?:Sync)?\([^;]*workflows/u, source.name);
    assert.doesNotMatch(source.text, /release-cut\.yml/u, source.name);
    assert.doesNotMatch(source.text, /release-publish\.yml/u, source.name);
  }
  assert.equal(typeof GitHubAdapter.prototype.createReleasePullRequest, "function");
  assert.equal(typeof FakeGitHubAdapter.prototype.createReleasePullRequest, "function");
  assert.equal(typeof GitHubAdapter.prototype.closeMilestone, "function");
  assert.equal(typeof FakeGitHubAdapter.prototype.closeMilestone, "function");
});

test("REL2-AC2 check plans title release: v0.1.0", () => {
  const adapter = fake();
  const report = cut(adapter, { mode: "check" });
  assert.equal(report.kind, "release-cut");
  assert.equal(report.title, TITLE);
  assert.equal(report.pull_request.kind, "create-release-pull-request");
  assert.equal(report.pull_request.title, TITLE);
  assert.equal(report.pull_request.closes, null);
  assert.equal(report.pull_request.head, HEAD);
  assert.equal(report.pull_request.base, "main");
  assert.equal(report.pull_request.applied, false);
  assert.doesNotMatch(report.pull_request.body ?? "", /Closes #\d/u);
  assert.equal(report.landing.kind, "ReleaseLanding");
  assert.equal(report.landing.commit_title, TITLE);
  assert.doesNotMatch(report.landing.commit_title, /\(#\d+\)/u);
  assert.deepEqual(
    {
      workflow: report.rehearsal.workflow,
      uses: report.rehearsal.uses,
      dry_run: report.rehearsal.dry_run,
    },
    RELEASE_REHEARSAL,
  );
  assert.equal(report.rehearsal.dispatched, false);
  assert.equal(report.rehearsal.recorded, false);
  assert.equal(adapter.getPullRequests().length, 0);
});

test("REL2-AC2 apply fake creates the PR with that title and records rehearsal without dispatch", () => {
  const adapter = fake();
  const report = cut(adapter, {
    mode: "apply",
    authorize: true,
    clearance: clearanceFor(adapter),
  });
  assert.equal(report.ok, true);
  assert.equal(report.authorization.authorized, true);
  assert.equal(report.pull_request.applied, true);
  assert.equal(report.pull_request.title, TITLE);
  assert.equal(report.pull_request.closes, null);
  assert.equal(typeof report.pull_request.number, "number");
  const stored = adapter.getPullRequest(report.pull_request.number);
  assert.equal(stored.title, TITLE);
  assert.equal(stored.closes, null);
  assert.doesNotMatch(stored.body, /Closes #\d/u);
  assert.equal(report.rehearsal.recorded, true);
  assert.equal(report.rehearsal.dispatched, false);
  assert.equal(report.rehearsal.terminal_result, "not-run");
  assert.deepEqual(adapter.workflowDispatches, []);
  assert.equal(report.landing.commit_title, TITLE);
  assert.equal(report.landing.applied, false);
});

test("REL2-AC2 squash merge preserves the exact release subject without a PR suffix", () => {
  const adapter = seededPull();
  const report = cut(adapter, {
    mode: "apply",
    authorize: true,
    land: true,
    pr: 1,
    clearance: clearanceFor(adapter),
    requiredJobs: [TAMA_ROADMAP],
  });
  assert.equal(report.ok, true);
  assert.equal(report.landing.applied, true);
  assert.equal(report.landing.commit_title, TITLE);
  assert.doesNotMatch(report.landing.commit_title, /\(#\d+\)/u);
  assert.deepEqual(adapter.inspectState().merges, [
    { number: 1, merge_method: "squash", commit_title: TITLE },
  ]);
  assert.equal(report.pull_request.applied, false);
});

test("REL2-AC2 live merge sends commit_title and createReleasePullRequest does not require Closes", () => {
  const calls = [];
  const live = new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport: {
      request(req) {
        calls.push(req);
        if (req.method === "GET" && req.path === "/user") return { login: "alice" };
        if (req.method === "GET" && req.path === "/repos/pikax/verter") {
          return {
            full_name: "pikax/verter",
            has_issues: true,
            permissions: { push: true, admin: false, maintain: false, triage: false, pull: true },
          };
        }
        if (req.method === "POST" && req.path === "graphql") {
          return {
            data: {
              organization: { projectV2: { id: "PVT_test", number: 3 } },
              user: { projectV2: null },
            },
          };
        }
        if (req.method === "POST" && req.path === "/repos/pikax/verter/pulls") {
          return { number: 7, title: req.body.title, body: req.body.body };
        }
        if (req.method === "PUT" && req.path === "/repos/pikax/verter/pulls/7/merge") {
          return { merged: true, sha: "0123456789abcdef0123456789abcdef01234567" };
        }
        throw new Error(`unexpected ${req.method} ${req.path}`);
      },
    },
  });
  const created = live.createReleasePullRequest({
    title: TITLE,
    body: "Release notes.\n",
    head: HEAD,
    base: "main",
    mode: "apply",
    clearance: clearanceFor(live, ["pullRequests"]),
  });
  assert.equal(created.kind, "create-release-pull-request");
  assert.equal(created.closes, null);
  assert.equal(created.title, TITLE);
  const post = calls.find(
    (row) => row.method === "POST" && row.path === "/repos/pikax/verter/pulls",
  );
  assert.equal(Object.hasOwn(post.body, "mappedIssue"), false);
  assert.doesNotMatch(post.body.body, /Closes #\d/u);
  calls.length = 0;
  const merged = live.mergePullRequest({
    number: 7,
    mergeMethod: "squash",
    commitTitle: TITLE,
    mode: "apply",
    clearance: clearanceFor(live, ["pullRequests"]),
  });
  assert.equal(merged.applied, true);
  const put = calls.find((row) => row.method === "PUT");
  assert.deepEqual(put.body, { merge_method: "squash", commit_title: TITLE });
  assert.doesNotMatch(put.body.commit_title, /\(#\d+\)/u);
});

test("REL2-AC2 CLI check plans release: v0.1.0; apply without --authorize aborts", () => {
  const help = spawnSync(process.execPath, [CLI, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /release-cut --check/u);
  assert.match(help.stdout, /--authorize/u);
  assert.match(help.stdout, /release: v/u);
  const check = spawnSync(
    process.execPath,
    [
      CLI,
      "release-cut",
      "--check",
      "--fake",
      "--version",
      VERSION,
      "--head",
      HEAD,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.equal(check.status, 0, check.stderr);
  const planned = JSON.parse(check.stdout);
  assert.equal(planned.title, TITLE);
  assert.equal(planned.pull_request.title, TITLE);
  assert.equal(planned.landing.commit_title, TITLE);
  const unauthorized = spawnSync(
    process.execPath,
    [
      CLI,
      "release-cut",
      "--apply",
      "--fake",
      "--version",
      VERSION,
      "--head",
      HEAD,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(unauthorized.status, 0);
  assert.match(unauthorized.stderr, /--authorize/u);
  const apply = spawnSync(
    process.execPath,
    [
      CLI,
      "release-cut",
      "--apply",
      "--authorize",
      "--fake",
      "--version",
      VERSION,
      "--head",
      HEAD,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.equal(apply.status, 0, apply.stderr);
  const created = JSON.parse(apply.stdout);
  assert.equal(created.pull_request.applied, true);
  assert.equal(created.pull_request.title, TITLE);
  assert.equal(created.pull_request.closes, null);
  const contract = fs.readFileSync(CONTRACT, "utf8");
  for (const name of ["ReleaseCutAuthorization", "ReleasePullRequest", "ReleaseLanding"]) {
    assert.match(contract, new RegExp(`^## ${name}$`, "mu"), `missing heading ${name}`);
  }
  assert.match(contract, /createReleasePullRequest/u);
  assert.match(contract, /commit_title/u);
  assert.match(contract, /--authorize/u);
  assert.match(contract, /must not auto-close/iu);
});

test("REL2 apply without doctor pullRequests clearance is refused", () => {
  const adapter = fake();
  assert.throws(() => cut(adapter, { mode: "apply", authorize: true }), DoctorRequiredError);
  assert.equal(adapter.getPullRequests().length, 0);
});
