import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { mintDoctorClearance } from "../adapter.mjs";
import {
  ClosingLinkError,
  DoctorRequiredError,
  DuplicateError,
  FakeGitHubAdapter,
  GitHubDoctor,
  InvalidIssueNumberError,
  NotFoundError,
  PartialFailureError,
  PermissionDeniedError,
  WrongRepositoryError,
} from "../index.mjs";

const CLI = path.join(path.dirname(fileURLToPath(import.meta.url)), "../githubctl.mjs");

function fake(options = {}) {
  return new FakeGitHubAdapter({ owner: "pikax", repo: "verter", ...options });
}

function clearanceFor(adapter) {
  const report = new GitHubDoctor(adapter).check();
  assert.equal(
    report.ok,
    true,
    Array.isArray(report.errors) ? report.errors.join("; ") : "doctor failed",
  );
  return report.clearance;
}

test("create issue returns deterministic numbers in the fake", () => {
  const adapter = fake({ nextIssueNumber: 40 });
  const clearance = clearanceFor(adapter);
  const first = adapter.createIssue({ title: "One", body: "a", mode: "apply", clearance });
  const second = adapter.createIssue({ title: "Two", body: "b", mode: "apply", clearance });
  assert.equal(first.kind, "create-issue");
  assert.equal(first.applied, true);
  assert.equal(first.number, 40);
  assert.equal(second.number, 41);
  assert.equal(typeof first.number, "number");
  assert.equal(adapter.getIssue(40).title, "One");
});

test("check-mode create and doctor do not mutate fake state", () => {
  const adapter = fake({
    nextIssueNumber: 9,
    issues: [{ number: 3, title: "seed", body: "seed", comments: [{ id: 8, body: "stay" }] }],
  });
  const before = adapter.inspectState();
  const report = new GitHubDoctor(adapter).check();
  assert.equal(report.ok, true);
  const plan = adapter.createIssue({ title: "planned", body: "no write", mode: "check" });
  assert.equal(plan.applied, false);
  assert.equal(plan.number, undefined);
  assert.deepEqual(adapter.inspectState(), before);
  const created = adapter.createIssue({
    title: "real",
    body: "write",
    mode: "apply",
    clearance: report.clearance,
  });
  assert.equal(created.number, 9);
});

test("opt-in update changes title and body and preserves number and comments", () => {
  const adapter = fake({
    issues: [
      {
        number: 5,
        title: "old title",
        body: "old body",
        comments: [
          { id: 1, body: "first" },
          { id: 2, body: "second" },
        ],
      },
    ],
  });
  const clearance = clearanceFor(adapter);
  const mapping = { node_id: "D1", gh_issue: 5, sync_to_github: true };
  const updated = adapter.updateIssue({
    number: 5,
    title: "new title",
    body: "new body\n\nModel: test",
    mapping,
    mode: "apply",
    clearance,
  });
  assert.equal(updated.kind, "update-issue");
  assert.equal(updated.applied, true);
  assert.equal(updated.number, 5);
  const issue = adapter.getIssue(5);
  assert.equal(issue.number, 5);
  assert.equal(issue.title, "new title");
  assert.equal(issue.body, "new body\n\nModel: test");
  assert.deepEqual(issue.comments, [
    { id: 1, body: "first" },
    { id: 2, body: "second" },
  ]);
});

test("PR create records the exact mapped Closes #n link and returns a number", () => {
  const adapter = fake({ nextPullNumber: 20 });
  const clearance = clearanceFor(adapter);
  const created = adapter.createPullRequest({
    title: "feat(ci): example",
    body: "Summary\n\nCloses #44\n",
    head: "train/example",
    base: "main",
    mappedIssue: 44,
    mode: "apply",
    clearance,
  });
  assert.equal(created.kind, "create-pull-request");
  assert.equal(created.applied, true);
  assert.equal(created.number, 20);
  assert.equal(created.closes, 44);
  assert.match(created.body, /Closes #44/u);
  const stored = adapter.getPullRequest(20);
  assert.equal(stored.closes, 44);
  assert.equal(stored.body.includes("Closes #44"), true);
});

test("PR create rejects inexact closing links without writing", () => {
  const adapter = fake();
  const clearance = clearanceFor(adapter);
  const attempts = [
    { body: "closes #3", mappedIssue: 3 },
    { body: "Closes #12", mappedIssue: 1 },
    { body: "Closes #1 extra", mappedIssue: 12 },
    { body: "See issue 3", mappedIssue: 3 },
  ];
  for (const attempt of attempts) {
    assert.throws(
      () =>
        adapter.createPullRequest({
          title: "feat(ci): example",
          body: attempt.body,
          head: "head",
          base: "main",
          mappedIssue: attempt.mappedIssue,
          mode: "apply",
          clearance,
        }),
      ClosingLinkError,
    );
  }
  assert.equal(adapter.getPullRequests().length, 0);
});

test("duplicate, wrong-repo, permission, and non-integer mapped issue errors are typed", () => {
  const adapter = fake();
  const clearance = clearanceFor(adapter);
  adapter.createPullRequest({
    title: "feat(ci): one",
    body: "Closes #8",
    head: "train/one",
    base: "main",
    mappedIssue: 8,
    mode: "apply",
    clearance,
  });
  assert.throws(
    () =>
      adapter.createPullRequest({
        title: "feat(ci): two",
        body: "Closes #9",
        head: "train/one",
        base: "main",
        mappedIssue: 9,
        mode: "apply",
        clearance,
      }),
    DuplicateError,
  );
  assert.throws(
    () =>
      adapter.createIssue({
        title: "T",
        body: "B",
        mode: "check",
        owner: "other",
        repo: "verter",
      }),
    WrongRepositoryError,
  );
  adapter.permissions.issues = false;
  assert.throws(
    () => adapter.createIssue({ title: "T", body: "B", mode: "apply", clearance }),
    PermissionDeniedError,
  );
  adapter.permissions.issues = true;
  assert.throws(
    () =>
      adapter.createPullRequest({
        title: "feat(ci): typed",
        body: "Closes #8",
        head: "train/typed",
        base: "main",
        mappedIssue: "8",
        mode: "apply",
        clearance,
      }),
    InvalidIssueNumberError,
  );
});

test("partial failure reports succeeded operations by returned number", () => {
  const adapter = fake({
    nextIssueNumber: 15,
    failOnApply: 1,
    failOnApplyError: new PermissionDeniedError("pulls denied after create"),
  });
  const clearance = clearanceFor(adapter);
  assert.throws(
    () =>
      adapter.applyOperations([
        { op: "createIssue", title: "Created", body: "body", mode: "apply", clearance },
        {
          op: "createPullRequest",
          title: "feat(ci): later",
          body: "Closes #15",
          head: "train/later",
          base: "main",
          mappedIssue: 15,
          mode: "apply",
          clearance,
        },
      ]),
    (error) => {
      assert.equal(error instanceof PartialFailureError, true);
      assert.equal(error.succeeded.length, 1);
      assert.equal(error.succeeded[0].kind, "create-issue");
      assert.equal(error.succeeded[0].number, 15);
      assert.equal(error.failed.operation.op, "createPullRequest");
      assert.equal(error.failed.error instanceof PermissionDeniedError, true);
      assert.equal(error.message.includes("Created"), false);
      return true;
    },
  );
  assert.equal(adapter.getIssue(15).title, "Created");
  assert.equal(adapter.getPullRequests().length, 0);
});

test("doctor reports missing issue capability and issues no clearance", () => {
  const adapter = fake({ permissions: { issues: false, pullRequests: true } });
  const report = new GitHubDoctor(adapter).check();
  assert.equal(report.ok, false);
  assert.equal(report.clearance, null);
  assert.equal(report.errors.includes("issues"), true);
  assert.throws(
    () =>
      adapter.createIssue({ title: "T", body: "B", mode: "apply", clearance: report.clearance }),
    DoctorRequiredError,
  );
});

test("githubctl doctor --fake and check stay offline", () => {
  const doctor = spawnSync(process.execPath, [CLI, "doctor", "--fake"], { encoding: "utf8" });
  assert.equal(doctor.status, 0, doctor.stderr);
  const report = JSON.parse(doctor.stdout);
  assert.equal(report.ok, true);
  assert.equal(report.capabilities.issues, true);
  const check = spawnSync(process.execPath, [CLI, "check"], { encoding: "utf8" });
  assert.equal(check.status, 0, check.stderr);
  assert.match(check.stdout, /createIssue/u);
  assert.match(check.stdout, /sync-issues --check/u);
});

test("expected capability misses do not throw from inspectCapabilities", () => {
  const unauthenticated = fake({ authenticated: false });
  const caps = unauthenticated.inspectCapabilities();
  assert.equal(caps.authenticated, false);
  assert.equal(caps.repository, null);
  assert.equal(caps.issues, false);
  assert.equal(caps.pullRequests, false);
  const report = new GitHubDoctor(unauthenticated).check();
  assert.equal(report.ok, false);
  assert.equal(report.clearance, null);
  assert.equal(report.errors.includes("unauthenticated"), true);
});

test("check-mode update plans without existence; apply throws NotFound", () => {
  const adapter = fake();
  const mapping = { node_id: "D1", gh_issue: 99, sync_to_github: true };
  const plan = adapter.updateIssue({
    number: 99,
    title: "T",
    body: "B",
    mapping,
    mode: "check",
  });
  assert.equal(plan.kind, "update-issue");
  assert.equal(plan.applied, false);
  assert.equal(plan.number, 99);
  assert.equal(adapter.getIssue(99), null);
  const clearance = clearanceFor(adapter);
  assert.throws(
    () =>
      adapter.updateIssue({
        number: 99,
        title: "T",
        body: "B",
        mapping,
        mode: "apply",
        clearance,
      }),
    NotFoundError,
  );
});

test("hand-built clearance objects cannot authorize apply", () => {
  const adapter = fake();
  const minted = clearanceFor(adapter);
  const forged = {
    kind: "github-doctor-clearance",
    owner: "pikax",
    repo: "verter",
    issues: true,
    pullRequests: true,
  };
  assert.throws(
    () => adapter.createIssue({ title: "T", body: "B", mode: "apply", clearance: forged }),
    DoctorRequiredError,
  );
  assert.throws(
    () =>
      adapter.createIssue({
        title: "T",
        body: "B",
        mode: "apply",
        clearance: { ...minted },
      }),
    DoctorRequiredError,
  );
  assert.throws(
    () =>
      adapter.createIssue({
        title: "T",
        body: "B",
        mode: "apply",
        clearance: JSON.parse(JSON.stringify(minted)),
      }),
    DoctorRequiredError,
  );
  assert.throws(
    () => fake().createIssue({ title: "T", body: "B", mode: "apply", clearance: minted }),
    DoctorRequiredError,
  );
  const created = adapter.createIssue({ title: "T", body: "B", mode: "apply", clearance: minted });
  assert.equal(created.applied, true);
});

test("clearance owner/repo mismatch is DoctorRequiredError", () => {
  const adapter = fake();
  const mismatched = mintDoctorClearance(
    adapter,
    Object.freeze({
      kind: "github-doctor-clearance",
      owner: "other",
      repo: "repo",
      issues: true,
      pullRequests: true,
    }),
  );
  assert.throws(
    () => adapter.createIssue({ title: "T", body: "B", mode: "apply", clearance: mismatched }),
    DoctorRequiredError,
  );
  const clearance = clearanceFor(adapter);
  assert.equal(
    adapter.createIssue({ title: "T", body: "B", mode: "apply", clearance }).applied,
    true,
  );
});

test("fake issues and pull requests share one number space", () => {
  const adapter = fake({ nextIssueNumber: 10 });
  const clearance = clearanceFor(adapter);
  const issue = adapter.createIssue({ title: "T", body: "B", mode: "apply", clearance });
  const pull = adapter.createPullRequest({
    title: "feat(ci): numbered",
    body: "Closes #10",
    head: "train/numbered",
    base: "main",
    mappedIssue: 10,
    mode: "apply",
    clearance,
  });
  assert.equal(issue.number, 10);
  assert.equal(pull.number, 11);
  assert.throws(
    () =>
      fake({
        issues: [{ number: 4, title: "i", body: "b" }],
        pullRequests: [
          { number: 4, title: "p", body: "Closes #1", head: "h", base: "main", closes: 1 },
        ],
      }),
    DuplicateError,
  );
});
