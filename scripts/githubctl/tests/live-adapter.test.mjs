import assert from "node:assert/strict";
import test from "node:test";

import { createGhApiTransport } from "../adapter.mjs";
import {
  DuplicateError,
  FakeGitHubAdapter,
  GitHubAdapter,
  GitHubDoctor,
  NotFoundError,
  PermissionDeniedError,
  ProtectedMappingError,
  UnstructuredGitHubOutputError,
} from "../index.mjs";

function transportMap(routes) {
  const calls = [];
  return {
    calls,
    request(req) {
      calls.push(req);
      const key = `${req.method} ${req.path}`;
      if (!Object.hasOwn(routes, key)) throw new Error(`unexpected ${key}`);
      const hit = routes[key];
      if (typeof hit === "function") return hit(req);
      if (hit instanceof Error) throw hit;
      return hit;
    },
  };
}

function pullsForHeadPath(owner, repo, head) {
  return `/repos/${owner}/${repo}/pulls?head=${encodeURIComponent(`${owner}:${head}`)}&per_page=100`;
}

const PROJECT_GRAPHQL_OK = {
  data: {
    repository: {
      owner: { projectV2: { id: "PVT_test", number: 3, viewerCanUpdate: true } },
    },
  },
};

function live(routes) {
  const transport = transportMap({
    "POST graphql": PROJECT_GRAPHQL_OK,
    ...routes,
  });
  const adapter = new GitHubAdapter({ owner: "pikax", repo: "verter", transport });
  return { adapter, transport };
}

function ghApiIncludeStdout(httpStatus, body, newline = "\n") {
  return `HTTP/2.0 ${httpStatus} X${newline}Content-Type: application/json${newline}${newline}${JSON.stringify(body)}`;
}

function ghApiSpawn(byKey) {
  return (command, args) => {
    assert.equal(command, "gh");
    assert.equal(args.includes("--include"), true);
    const method = args[args.indexOf("-X") + 1];
    const apiPath =
      args.find((arg) => typeof arg === "string" && arg.startsWith("/")) ??
      (args.includes("graphql") ? "graphql" : undefined);
    const spec = byKey[`${method} ${apiPath}`];
    if (!spec) {
      return { status: 1, stdout: "", stderr: `unexpected ${method} ${apiPath}` };
    }
    return {
      status: spec.status ?? 0,
      stdout: spec.stdout ?? ghApiIncludeStdout(spec.httpStatus ?? 200, spec.body ?? {}),
      stderr: spec.stderr ?? "",
      error: spec.error,
    };
  };
}

function liveFromSpawn(byKey) {
  return new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport: createGhApiTransport({
      spawn: ghApiSpawn({
        "POST graphql": { body: PROJECT_GRAPHQL_OK },
        ...byKey,
      }),
    }),
  });
}

const writableRepo = {
  full_name: "pikax/verter",
  has_issues: true,
  permissions: { admin: false, maintain: false, push: true, triage: false, pull: true },
};

test("live inspectCapabilities folds expected misses and uses distinct write signals", () => {
  const unauthorized = live({
    "GET /user": new PermissionDeniedError("Bad credentials"),
  });
  const unauthorizedCaps = unauthorized.adapter.inspectCapabilities();
  assert.equal(unauthorizedCaps.authenticated, false);
  assert.equal(unauthorizedCaps.repository, null);
  assert.equal(unauthorizedCaps.issues, false);
  assert.equal(unauthorizedCaps.pullRequests, false);
  assert.equal(unauthorizedCaps.actions, false);
  assert.equal("login" in unauthorizedCaps, false);

  const missingRepo = live({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": new NotFoundError("Not Found"),
  });
  const missingCaps = missingRepo.adapter.inspectCapabilities();
  assert.equal(missingCaps.authenticated, true);
  assert.equal(missingCaps.login, "alice");
  assert.equal(missingCaps.repository, null);

  const forbiddenRepo = live({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": new PermissionDeniedError("Resource not accessible"),
  });
  assert.equal(forbiddenRepo.adapter.inspectCapabilities().repository, null);

  const wrongName = live({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": {
      full_name: "other/repo",
      has_issues: true,
      permissions: { push: true },
    },
  });
  const wrongCaps = wrongName.adapter.inspectCapabilities();
  assert.equal(wrongCaps.repository, null);
  assert.equal(wrongCaps.issues, false);

  const issuesDisabled = live({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": {
      full_name: "pikax/verter",
      has_issues: false,
      permissions: { push: true, admin: true },
    },
  });
  const disabledCaps = issuesDisabled.adapter.inspectCapabilities();
  assert.deepEqual(disabledCaps.repository, { owner: "pikax", repo: "verter" });
  assert.equal(disabledCaps.issues, false);
  assert.equal(disabledCaps.pullRequests, true);
  assert.equal(disabledCaps.actions, true);

  const triageOnly = live({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": {
      full_name: "pikax/verter",
      has_issues: true,
      permissions: { admin: false, maintain: false, push: false, triage: true, pull: true },
    },
  });
  const triageCaps = triageOnly.adapter.inspectCapabilities();
  assert.equal(triageCaps.issues, true);
  assert.equal(triageCaps.pullRequests, false);
  assert.equal(triageCaps.actions, false);

  const unstructured = live({
    "GET /user": new UnstructuredGitHubOutputError("gh api returned non-JSON output"),
  });
  assert.throws(() => unstructured.adapter.inspectCapabilities(), UnstructuredGitHubOutputError);
});

test("GitHubDoctor folds live capability misses into a report", () => {
  const { adapter } = live({
    "GET /user": new PermissionDeniedError("Bad credentials"),
  });
  const report = new GitHubDoctor(adapter).check();
  assert.equal(report.ok, false);
  assert.equal(report.clearance, null);
  assert.equal(report.errors.includes("unauthenticated"), true);
  assert.equal(report.capabilities.authenticated, false);
});

test("Project mutation clearance requires viewerCanUpdate", () => {
  const { adapter } = live({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": writableRepo,
    "POST graphql": {
      data: {
        repository: {
          owner: { projectV2: { id: "PVT_read_only", number: 3, viewerCanUpdate: false } },
        },
      },
    },
  });
  const report = new GitHubDoctor(adapter).check({ require: ["projects"] });
  assert.equal(report.ok, false);
  assert.equal(report.capabilities.projects, false);
  assert.equal(report.clearance, null);
});

test("doctor skips Project traffic when the command does not require it", () => {
  const { adapter, transport } = live({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": writableRepo,
  });
  const report = new GitHubDoctor(adapter).check({ require: ["issues"] });
  assert.equal(report.ok, true);
  assert.equal(report.capabilities.issues, true);
  assert.equal(report.capabilities.projects, false);
  assert.equal(
    transport.calls.some((call) => call.path === "graphql"),
    false,
  );
});

test("check-mode create/update/PR plans locally; apply owns existence", () => {
  const mapping = { node_id: "D1", gh_issue: 7, sync_to_github: true };
  const rows = [
    {
      name: "createIssue",
      check: (adapter) => adapter.createIssue({ title: "T", body: "B", mode: "check" }),
      checkKind: "create-issue",
      apply: (adapter, clearance) =>
        adapter.createIssue({ title: "T", body: "B", mode: "apply", clearance }),
      applyRoute: "POST /repos/pikax/verter/issues",
      applyOk: { number: 11 },
      applyNumber: 11,
    },
    {
      name: "updateIssue",
      check: (adapter) =>
        adapter.updateIssue({
          number: 7,
          title: "T",
          body: "B",
          mapping,
          mode: "check",
        }),
      checkKind: "update-issue",
      apply: (adapter, clearance) =>
        adapter.updateIssue({
          number: 7,
          title: "T",
          body: "B",
          mapping,
          mode: "apply",
          clearance,
        }),
      applyRoute: "PATCH /repos/pikax/verter/issues/7",
      applyOk: { number: 7 },
      applyNumber: 7,
      applyMissing: new NotFoundError("Not Found"),
    },
    {
      name: "createPullRequest",
      check: (adapter) =>
        adapter.createPullRequest({
          title: "feat(ci): example",
          body: "Closes #7",
          head: "train/example",
          base: "main",
          mappedIssue: 7,
          mode: "check",
        }),
      checkKind: "create-pull-request",
      apply: (adapter, clearance) =>
        adapter.createPullRequest({
          title: "feat(ci): example",
          body: "Closes #7",
          head: "train/example",
          base: "main",
          mappedIssue: 7,
          mode: "apply",
          clearance,
        }),
      applyRoute: "POST /repos/pikax/verter/pulls",
      applyOk: { number: 12 },
      applyNumber: 12,
      applyMissing: new DuplicateError("A pull request already exists for pikax:train/example."),
    },
  ];

  for (const row of rows) {
    const doctorRoutes = {
      "GET /user": { login: "alice" },
      "GET /repos/pikax/verter": writableRepo,
    };
    const checkLive = live(doctorRoutes);
    const checkPlan = row.check(checkLive.adapter);
    assert.equal(checkPlan.kind, row.checkKind, row.name);
    assert.equal(checkPlan.applied, false, row.name);
    if (row.checkKind === "update-issue") assert.equal(checkPlan.number, 7, row.name);
    else assert.equal(checkPlan.number, undefined, row.name);
    assert.deepEqual(
      checkLive.transport.calls.map((call) => `${call.method} ${call.path}`),
      [],
      row.name,
    );

    const applyLive = live({
      ...doctorRoutes,
      [row.applyRoute]: row.applyOk,
    });
    const clearance = new GitHubDoctor(applyLive.adapter).check().clearance;
    applyLive.transport.calls.length = 0;
    const applied = row.apply(applyLive.adapter, clearance);
    assert.equal(applied.applied, true, row.name);
    assert.equal(applied.number, row.applyNumber, row.name);
    assert.equal(
      applyLive.transport.calls.map((call) => `${call.method} ${call.path}`).join("\n"),
      row.applyRoute,
      row.name,
    );

    if (row.applyMissing) {
      const missingLive = live({
        ...doctorRoutes,
        [row.applyRoute]: row.applyMissing,
      });
      const missingClearance = new GitHubDoctor(missingLive.adapter).check().clearance;
      assert.throws(
        () => row.apply(missingLive.adapter, missingClearance),
        row.applyMissing.constructor,
        row.name,
      );
    }
  }

  const fakeAdapter = new FakeGitHubAdapter({
    owner: "pikax",
    repo: "verter",
    issues: [{ number: 7, title: "seed", body: "seed" }],
  });
  const fakeClearance = new GitHubDoctor(fakeAdapter).check().clearance;
  for (const row of rows) {
    const plan = row.check(fakeAdapter);
    assert.equal(plan.kind, row.checkKind, `fake ${row.name}`);
    assert.equal(plan.applied, false, `fake ${row.name}`);
  }
  assert.equal(fakeAdapter.getIssue(7).title, "seed");
  assert.equal(fakeAdapter.getPullRequests().length, 0);
  const created = fakeAdapter.createIssue({
    title: "T",
    body: "B",
    mode: "apply",
    clearance: fakeClearance,
  });
  const updated = fakeAdapter.updateIssue({
    number: 7,
    title: "U",
    body: "B",
    mapping: { node_id: "D1", gh_issue: 7, sync_to_github: true },
    mode: "apply",
    clearance: fakeClearance,
  });
  const opened = fakeAdapter.createPullRequest({
    title: "feat(ci): example",
    body: "Closes #7",
    head: "train/example",
    base: "main",
    mappedIssue: 7,
    mode: "apply",
    clearance: fakeClearance,
  });
  assert.equal(created.applied, true);
  assert.equal(updated.applied, true);
  assert.equal(opened.applied, true);
  assert.throws(
    () =>
      fakeAdapter.updateIssue({
        number: 99,
        title: "T",
        body: "B",
        mapping: { node_id: "D1", gh_issue: 99, sync_to_github: true },
        mode: "apply",
        clearance: fakeClearance,
      }),
    NotFoundError,
  );
  assert.throws(
    () =>
      fakeAdapter.createPullRequest({
        title: "feat(ci): example",
        body: "Closes #7",
        head: "train/example",
        base: "main",
        mappedIssue: 7,
        mode: "apply",
        clearance: fakeClearance,
      }),
    DuplicateError,
  );
});

test("live spawn maps JSON 422/401/404 and non-JSON without scraping terminal prose", () => {
  const duplicateBody = {
    message: "Validation Failed",
    errors: [
      {
        resource: "PullRequest",
        code: "custom",
        message: "A pull request already exists for pikax:train/one.",
      },
    ],
  };
  const duplicate = liveFromSpawn({
    "GET /user": { body: { login: "alice" } },
    "GET /repos/pikax/verter": { body: writableRepo },
    "POST /repos/pikax/verter/pulls": { httpStatus: 422, body: duplicateBody },
  });
  const clearance = new GitHubDoctor(duplicate).check().clearance;
  assert.throws(
    () =>
      duplicate.createPullRequest({
        title: "feat(ci): one",
        body: "Closes #8",
        head: "train/one",
        base: "main",
        mappedIssue: 8,
        mode: "apply",
        clearance,
      }),
    (error) => {
      assert.equal(error instanceof DuplicateError, true);
      assert.match(error.message, /already exists/u);
      assert.equal(error.message.includes("Validation Failed"), false);
      return true;
    },
  );

  const unauthorized = liveFromSpawn({
    "GET /user": {
      httpStatus: 401,
      body: { message: "Bad credentials", documentation_url: "https://docs.github.com/rest" },
    },
  });
  const unauthorizedCaps = unauthorized.inspectCapabilities();
  assert.equal(unauthorizedCaps.authenticated, false);
  assert.equal(unauthorizedCaps.repository, null);

  const missing = liveFromSpawn({
    "GET /user": { body: { login: "alice" } },
    "GET /repos/pikax/verter": { body: writableRepo },
    "PATCH /repos/pikax/verter/issues/9": {
      httpStatus: 404,
      body: { message: "Not Found", documentation_url: "https://docs.github.com/rest" },
    },
  });
  const missingClearance = new GitHubDoctor(missing).check().clearance;
  assert.throws(
    () =>
      missing.updateIssue({
        number: 9,
        title: "T",
        body: "B",
        mapping: { node_id: "D1", gh_issue: 9, sync_to_github: true },
        mode: "apply",
        clearance: missingClearance,
      }),
    NotFoundError,
  );

  const nonJson = liveFromSpawn({
    "GET /user": { stdout: "gh: HTTP 500" },
  });
  assert.throws(() => nonJson.inspectCapabilities(), UnstructuredGitHubOutputError);
});

test("HTTP 401 without a JSON status field folds unauthenticated", () => {
  const adapter = liveFromSpawn({
    "GET /user": {
      status: 1,
      stdout: ghApiIncludeStdout(
        401,
        { message: "Bad credentials", documentation_url: "https://docs.github.com/rest" },
        "\r\n",
      ),
    },
  });
  const caps = adapter.inspectCapabilities();
  assert.equal(caps.authenticated, false);
  assert.equal(caps.repository, null);
  assert.equal(caps.issues, false);
  assert.equal("login" in caps, false);
});

test("gh api transport requires --include and reads HTTP status from the header block", () => {
  const seen = [];
  const spawn = (_command, args) => {
    seen.push(args);
    return { status: 0, stdout: ghApiIncludeStdout(200, { login: "alice" }), stderr: "" };
  };
  createGhApiTransport({ spawn }).request({ method: "GET", path: "/user" });
  assert.equal(seen.length, 1);
  assert.equal(seen[0].includes("--include"), true);
  assert.equal(seen[0][0], "api");

  const missingStatusLine = liveFromSpawn({
    "GET /user": { stdout: JSON.stringify({ login: "alice", status: "200" }) },
  });
  assert.throws(() => missingStatusLine.inspectCapabilities(), UnstructuredGitHubOutputError);

  const stderrNoise = liveFromSpawn({
    "GET /user": {
      status: 1,
      stderr: "HTTP/2.0 401 Unauthorized\nBad credentials\n",
      stdout: ghApiIncludeStdout(200, { login: "alice" }),
    },
    "GET /repos/pikax/verter": {
      status: 1,
      stderr: "Not Found",
      stdout: ghApiIncludeStdout(200, writableRepo),
    },
  });
  assert.equal(stderrNoise.inspectCapabilities().authenticated, true);
});

test("adapter.transport is not a request port", () => {
  const transport = transportMap({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": writableRepo,
    "POST graphql": PROJECT_GRAPHQL_OK,
  });
  const adapter = new GitHubAdapter({ owner: "pikax", repo: "verter", transport });
  assert.equal(Object.hasOwn(adapter, "transport"), false);
  assert.equal(adapter.transport, undefined);
  assert.equal(typeof adapter.request, "undefined");
  assert.equal(typeof adapter.spawn, "undefined");
  assert.equal(adapter.inspectCapabilities().authenticated, true);
  assert.equal(transport.calls.length > 0, true);
});

test("planted payload.status does not override the HTTP status line", () => {
  const denied = liveFromSpawn({
    "GET /user": {
      status: 0,
      stdout: ghApiIncludeStdout(401, {
        message: "Bad credentials",
        documentation_url: "https://docs.github.com/rest",
        status: 200,
      }),
    },
  });
  assert.equal(denied.inspectCapabilities().authenticated, false);

  const ok = liveFromSpawn({
    "GET /user": {
      status: 1,
      stdout: ghApiIncludeStdout(200, { login: "alice", status: 401 }),
    },
    "GET /repos/pikax/verter": {
      status: 1,
      stdout: ghApiIncludeStdout(200, { ...writableRepo, status: 401 }),
    },
  });
  const caps = ok.inspectCapabilities();
  assert.equal(caps.authenticated, true);
  assert.equal(caps.login, "alice");
  assert.deepEqual(caps.repository, { owner: "pikax", repo: "verter" });
});

test("protected update still cannot reach the network port", () => {
  const { adapter, transport } = live({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": writableRepo,
  });
  const clearance = new GitHubDoctor(adapter).check().clearance;
  transport.calls.length = 0;
  assert.throws(
    () =>
      adapter.updateIssue({
        number: 7,
        title: "rewritten",
        body: "rewritten",
        mapping: { node_id: "D1", gh_issue: 7, sync_to_github: false },
        mode: "apply",
        clearance,
      }),
    ProtectedMappingError,
  );
  assert.deepEqual(transport.calls, []);
});

test("live pullsForHead lists open pulls for a head through gh api --include", () => {
  const path = pullsForHeadPath("pikax", "verter", "train/example");
  const listed = liveFromSpawn({
    [`GET ${path}`]: {
      body: [{ number: 3, head: { ref: "train/example" } }],
    },
  }).pullsForHead("train/example");
  assert.equal(listed.length, 1);
  assert.equal(listed[0].number, 3);
  assert.equal(listed[0].head, "train/example");

  const empty = liveFromSpawn({
    [`GET ${path}`]: { body: [] },
  }).pullsForHead("train/example");
  assert.deepEqual(empty, []);

  const unstructured = liveFromSpawn({
    [`GET ${path}`]: { body: { number: 3, head: { ref: "train/example" } } },
  });
  assert.throws(() => unstructured.pullsForHead("train/example"), UnstructuredGitHubOutputError);
});

test("injected live transport returns one open PR for pullsForHead", () => {
  const path = pullsForHeadPath("pikax", "verter", "train/example");
  const { adapter, transport } = live({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": writableRepo,
    [`GET ${path}`]: [{ number: 8, head: { ref: "train/example" } }],
  });
  transport.calls.length = 0;
  const listed = adapter.pullsForHead("train/example");
  assert.equal(listed.length, 1);
  assert.equal(listed[0].number, 8);
  assert.equal(listed[0].head, "train/example");
  assert.deepEqual(
    transport.calls.map((call) => `${call.method} ${call.path}`),
    [`GET ${path}`],
  );
});

test("live milestone catalog operations use repository milestone REST fields only", () => {
  const existing = {
    number: 4,
    title: "0.0.1-beta.6",
    description: "old description",
    state: "closed",
    due_on: "2026-09-01T00:00:00Z",
  };
  const created = {
    number: 5,
    title: "0.0.1-beta.7",
    description: "Product-surface hardening",
    state: "open",
  };
  const updated = { ...existing, description: "Svelte stable-surface correctness" };
  const { adapter, transport } = live({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": writableRepo,
    "GET /repos/pikax/verter/milestones?state=all&per_page=100": [existing],
    "POST /repos/pikax/verter/milestones": (request) => {
      assert.deepEqual(request.body, {
        title: created.title,
        description: created.description,
      });
      return created;
    },
    "PATCH /repos/pikax/verter/milestones/4": (request) => {
      assert.deepEqual(request.body, {
        description: updated.description,
      });
      assert.equal("state" in request.body, false);
      assert.equal("due_on" in request.body, false);
      return updated;
    },
    "GET /repos/pikax/verter/issues/10": {
      id: 1000,
      number: 10,
      title: "Issue",
      body: "Body",
      milestone: null,
    },
    "PATCH /repos/pikax/verter/issues/10": (request) => {
      assert.deepEqual(request.body, { milestone: 4 });
      return {
        number: 10,
        title: "Issue",
        body: "Body",
        milestone: { title: updated.title },
      };
    },
  });
  const clearance = new GitHubDoctor(adapter).check({ require: ["issues"] }).clearance;
  transport.calls.length = 0;

  assert.deepEqual(adapter.getRepositoryMilestones(), [existing]);
  assert.equal(
    adapter.createRepositoryMilestone({
      milestone: { title: created.title, description: created.description },
      mode: "apply",
      clearance,
    }).milestone.number,
    5,
  );
  assert.equal(
    adapter.updateRepositoryMilestone({
      existing,
      milestone: { title: updated.title, description: updated.description },
      mode: "apply",
      clearance,
    }).milestone.description,
    updated.description,
  );
  assert.equal(
    adapter.setIssueMilestone({
      issueNumber: 10,
      title: updated.title,
      mode: "apply",
      clearance,
    }).applied,
    true,
  );
  assert.deepEqual(
    transport.calls.map((call) => `${call.method} ${call.path}`),
    [
      "GET /repos/pikax/verter/milestones?state=all&per_page=100",
      "POST /repos/pikax/verter/milestones",
      "PATCH /repos/pikax/verter/milestones/4",
      "GET /repos/pikax/verter/issues/10",
      "GET /repos/pikax/verter/milestones?state=all&per_page=100",
      "PATCH /repos/pikax/verter/issues/10",
    ],
  );
});

test("live issue dependency operations use blocker database ids", () => {
  const mapping = { node_id: "D2", gh_issue: 10, sync_to_github: true };
  const blockingMapping = { node_id: "D1", gh_issue: 9, sync_to_github: true };
  const { adapter, transport } = live({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": writableRepo,
    "GET /repos/pikax/verter/issues/10/dependencies/blocked_by?per_page=100": [
      {
        id: 9000,
        number: 9,
        repository_url: "https://api.github.com/repos/pikax/verter",
      },
    ],
    "POST /repos/pikax/verter/issues/10/dependencies/blocked_by": (request) => {
      assert.deepEqual(request.body, { issue_id: 9000 });
      return {};
    },
    "DELETE /repos/pikax/verter/issues/10/dependencies/blocked_by/9000": {},
  });
  const clearance = new GitHubDoctor(adapter).check({ require: ["issues"] }).clearance;
  transport.calls.length = 0;

  assert.deepEqual(adapter.getIssueDependencies(10), [
    { id: 9000, number: 9, owner: "pikax", repo: "verter" },
  ]);
  assert.equal(
    adapter.addIssueDependency({
      number: 10,
      blockingNumber: 9,
      blockingId: 9000,
      mapping,
      blockingMapping,
      mode: "apply",
      clearance,
    }).blockingId,
    9000,
  );
  assert.equal(
    adapter.removeIssueDependency({
      number: 10,
      blockingNumber: 9,
      blockingId: 9000,
      mapping,
      blockingMapping,
      mode: "apply",
      clearance,
    }).applied,
    true,
  );
  assert.deepEqual(
    transport.calls.map((call) => `${call.method} ${call.path}`),
    [
      "GET /repos/pikax/verter/issues/10/dependencies/blocked_by?per_page=100",
      "POST /repos/pikax/verter/issues/10/dependencies/blocked_by",
      "DELETE /repos/pikax/verter/issues/10/dependencies/blocked_by/9000",
    ],
  );
});

test("live Project status mutation resolves the configured field and option", () => {
  const graphqlCalls = [];
  const { adapter } = live({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": writableRepo,
    "POST graphql": (request) => {
      graphqlCalls.push(request.body);
      const { query, variables } = request.body;
      if (query.includes("updateProjectV2ItemFieldValue")) {
        assert.deepEqual(variables, {
          projectId: "PVT_test",
          itemId: "PVTI_10",
          fieldId: "PVTF_status",
          optionId: "status-in-progress",
        });
        return { data: { updateProjectV2ItemFieldValue: { projectV2Item: { id: "PVTI_10" } } } };
      }
      if (query.includes("subIssues(first:100)")) {
        return {
          data: {
            repository: {
              issue: {
                id: "I_10",
                number: 10,
                parent: null,
                projectItems: {
                  totalCount: 1,
                  nodes: [
                    {
                      id: "PVTI_10",
                      project: { id: "PVT_test", number: 3 },
                      fieldValueByName: { name: "Todo", optionId: "status-todo" },
                    },
                  ],
                },
                subIssues: { totalCount: 0, nodes: [] },
              },
            },
          },
        };
      }
      if (query.includes("fields(first:100)")) {
        return {
          data: {
            organization: {
              projectV2: {
                id: "PVT_test",
                number: 3,
                viewerCanUpdate: true,
                fields: {
                  totalCount: 1,
                  nodes: [
                    {
                      id: "PVTF_status",
                      name: "Status",
                      options: [
                        { id: "status-todo", name: "Todo" },
                        { id: "status-in-progress", name: "In Progress" },
                        { id: "status-done", name: "Done" },
                      ],
                    },
                  ],
                },
              },
            },
            user: { projectV2: null },
          },
        };
      }
      return PROJECT_GRAPHQL_OK;
    },
  });
  const clearance = new GitHubDoctor(adapter).check({ require: ["projects"] }).clearance;
  graphqlCalls.length = 0;

  const result = adapter.setIssueProjectStatus({
    issueNumber: 10,
    status: "In Progress",
    mode: "apply",
    clearance,
  });

  assert.equal(result.current, "Todo");
  assert.equal(result.status, "In Progress");
  assert.equal(result.applied, true);
  assert.equal(graphqlCalls.length, 4);
});
