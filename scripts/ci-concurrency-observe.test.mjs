#!/usr/bin/env node

// Tests for ci-concurrency-observe.mjs.
// Run: node --test scripts/ci-concurrency-observe.test.mjs
//
// Every test runs on synthetic API records through an injected fetch, so no
// test reaches the network and the state judgements are exact.

import assert from "node:assert/strict";
import test from "node:test";

import {
  cancelCauseGroup,
  findOlderTreeDisplacements,
  findVerdictViolations,
  observe,
  parseArgs,
  summarizeAttempt,
} from "./ci-concurrency-observe.mjs";

const REPO = "owner/trial";

function job(name, conclusion, overrides = {}) {
  return {
    id: name.length,
    name,
    status: "completed",
    conclusion,
    created_at: "2026-01-01T00:00:00Z",
    started_at: "2026-01-01T00:00:00Z",
    completed_at: "2026-01-01T00:10:00Z",
    runner_name: "hosted-1",
    ...overrides,
  };
}

function run(overrides = {}) {
  return {
    id: 1,
    run_attempt: 1,
    head_branch: "feature",
    event: "pull_request",
    pull_requests: [{ number: 7 }],
    head_sha: "tested-head",
    status: "completed",
    conclusion: "success",
    created_at: "2026-01-01T00:00:00Z",
    run_started_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:10:00Z",
    ...overrides,
  };
}

function respond(status, body, remaining = "50") {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: (key) => (key === "x-ratelimit-remaining" ? remaining : null) },
    json: async () => body,
  };
}

/** Serves `routes[path]` in order; records calls and rejects a non-GET. */
function fakeFetch(routes) {
  const calls = [];
  const impl = async (url, init) => {
    assert.equal(init.method, "GET", "the observer must only read");
    const path = url.replace(`https://api.github.com/repos/${REPO}`, "");
    calls.push(path);
    const route = routes[path];
    assert.ok(route, `unexpected request ${path}`);
    const next = Array.isArray(route) ? route.shift() : route;
    if (next instanceof Error) throw next;
    return next;
  };
  return { impl, calls };
}

test("a superseded run keeps its cancelled jobs and failing aggregate apart from a pass", () => {
  const summary = summarizeAttempt({
    run: run({ conclusion: "cancelled" }),
    jobs: [
      job("detect-changes", "cancelled"),
      job("Rust Test", "cancelled", {
        runner_name: "",
        started_at: "2026-01-01T00:10:01Z",
        completed_at: "2026-01-01T00:10:00Z",
      }),
      job("CI Required", "failure"),
    ],
  });
  assert.equal(summary.aggregate.conclusion, "failure");
  assert.equal(summary.cancelledWhileRunning, 1);
  assert.equal(summary.cancelledWithoutRunner, 1);
  assert.deepEqual(summary.jobCounts, { "completed/cancelled": 2, "completed/failure": 1 });
  assert.deepEqual(findVerdictViolations(summary), []);
});

test("an aggregate reading as passing beside unsatisfied work is a violation", () => {
  const cases = [
    {
      label: "cancelled job",
      jobs: [job("a", "cancelled"), job("CI Required", "success")],
      expected: ["passing-with-cancelled-job"],
    },
    {
      label: "pending job",
      jobs: [
        job("a", null, { status: "queued", started_at: null, completed_at: null }),
        job("CI Required", "success"),
      ],
      expected: ["passing-with-unfinished-job"],
    },
    {
      label: "timed out job",
      jobs: [job("a", "timed_out"), job("CI Required", "success")],
      expected: ["passing-with-unsatisfied-job"],
    },
    {
      label: "cancelled run",
      runOverrides: { conclusion: "cancelled" },
      jobs: [job("a", "success"), job("CI Required", "success")],
      expected: ["passing-in-cancelled-run"],
    },
    { label: "no aggregate", jobs: [job("a", "success")], expected: ["aggregate-missing"] },
    {
      label: "accepted skip",
      jobs: [job("a", "skipped"), job("CI Required", "success")],
      expected: [],
    },
  ];
  for (const { label, jobs, runOverrides, expected } of cases) {
    const summary = summarizeAttempt({ run: run(runOverrides), jobs });
    assert.deepEqual(findVerdictViolations(summary), expected, label);
  }
});

test("peak running jobs counts overlap and ignores work that never reached a runner", () => {
  const at = (minute) => `2026-01-01T00:${String(minute).padStart(2, "0")}:00Z`;
  const summary = summarizeAttempt({
    run: run(),
    jobs: [
      job("a", "success", { started_at: at(0), completed_at: at(5) }),
      job("b", "success", { started_at: at(1), completed_at: at(6) }),
      // Starts exactly as `b` ends, so it does not add to the overlap.
      job("c", "success", { started_at: at(6), completed_at: at(7) }),
      job("d", "cancelled", { runner_name: "", started_at: at(1), completed_at: at(9) }),
    ],
  });
  assert.equal(summary.peakRunningJobs, 2);
});

test("a re-run of an older run that displaces the newest push is reported", () => {
  const older = summarizeAttempt({
    run: run({
      id: 10,
      run_attempt: 3,
      // The attempts endpoint reports when this attempt was created, not the run.
      created_at: "2026-01-01T09:00:02Z",
      run_started_at: "2026-01-01T09:00:01Z",
      updated_at: "2026-01-01T09:06:00Z",
    }),
    jobs: [job("CI Required", "success")],
  });
  const newest = summarizeAttempt({
    run: run({
      id: 20,
      conclusion: "cancelled",
      created_at: "2026-01-01T09:00:00Z",
      run_started_at: "2026-01-01T09:00:00Z",
      updated_at: "2026-01-01T09:00:09Z",
    }),
    jobs: [job("CI Required", "failure")],
  });
  assert.deepEqual(findOlderTreeDisplacements([older, newest]), [
    {
      displacedRunId: 20,
      displacedAggregate: "failure",
      rerunRunId: 10,
      rerunAttempt: 3,
      rerunConclusion: "success",
      rerunAggregate: "success",
    },
  ]);

  // Ordinary supersession, a newer run's re-run, a re-run outside the window and
  // another ref are all different situations.
  const firstAttempt = { ...older, attempt: 1 };
  const newerRun = { ...older, runId: 30 };
  const outsideWindow = { ...older, attemptStartedAt: "2026-01-01T08:00:00Z" };
  const otherRef = { ...older, ref: "other" };
  for (const candidate of [firstAttempt, newerRun, outsideWindow, otherRef]) {
    assert.deepEqual(findOlderTreeDisplacements([candidate, newest]), []);
  }
});

test("the cancel cause yields its concurrency group and nothing else matches", () => {
  assert.equal(
    cancelCauseGroup([
      { message: "The operation was canceled." },
      {
        message:
          "Canceling since a higher priority waiting request for ci-refs/pull/7/merge exists",
      },
    ]),
    "ci-refs/pull/7/merge",
  );
  assert.equal(cancelCauseGroup([{ message: "The operation was canceled." }]), null);
});

test("observe reads one attempt's jobs across pages without mixing attempts", async () => {
  const firstPage = Array.from({ length: 100 }, (_, index) => job(`job-${index}`, "success"));
  const { impl, calls } = fakeFetch({
    "/actions/runs/1/attempts/2": respond(200, run({ run_attempt: 2 })),
    "/actions/runs/1/attempts/2/jobs?per_page=100&page=1": respond(200, {
      total_count: 101,
      jobs: firstPage,
    }),
    "/actions/runs/1/attempts/2/jobs?per_page=100&page=2": respond(200, {
      total_count: 101,
      jobs: [job("CI Required", "success")],
    }),
  });
  const result = await observe({
    repo: REPO,
    requests: [{ runId: "1", attempt: 2 }],
    fetchImpl: impl,
  });
  assert.equal(calls.length, 3);
  assert.equal(result.summaries[0].jobs.length, 101);
  assert.equal(result.summaries[0].aggregate.conclusion, "success");
  assert.deepEqual(result.missing, []);
  assert.equal(result.rateLimitRemaining, 50);
});

test("observe uses the latest attempt and records the cancel group on request", async () => {
  const { impl } = fakeFetch({
    "/actions/runs/1": respond(200, run({ run_attempt: 4, conclusion: "cancelled" })),
    "/actions/runs/1/attempts/4/jobs?per_page=100&page=1": respond(200, {
      total_count: 2,
      jobs: [job("a", "cancelled", { id: 55 }), job("CI Required", "failure")],
    }),
    "/check-runs/55/annotations": respond(200, [
      { message: "Canceling since a higher priority waiting request for slot-1 exists" },
    ]),
  });
  const result = await observe({
    repo: REPO,
    requests: [{ runId: "1" }],
    fetchImpl: impl,
    annotations: true,
  });
  assert.equal(result.summaries[0].attempt, 4);
  assert.equal(result.summaries[0].cancelledByGroup, "slot-1");
});

test("an unreadable cancel cause keeps the run's states and reports the gap", async () => {
  const { impl } = fakeFetch({
    "/actions/runs/1": respond(200, run({ conclusion: "cancelled" })),
    "/actions/runs/1/attempts/1/jobs?per_page=100&page=1": respond(200, {
      total_count: 2,
      jobs: [job("a", "cancelled", { id: 55 }), job("CI Required", "failure")],
    }),
    "/check-runs/55/annotations": respond(404, {}),
  });
  const result = await observe({
    repo: REPO,
    requests: [{ runId: "1" }],
    fetchImpl: impl,
    annotations: true,
  });
  assert.equal(result.summaries[0].aggregate.conclusion, "failure");
  assert.equal(result.summaries[0].cancelledByGroup, null);
  assert.deepEqual(result.missing, [
    { runId: "1", attempt: null, reason: "/check-runs/55/annotations: HTTP 404" },
  ]);
});

test("an unreadable record stays missing and never becomes a summary", async () => {
  const jobsPath = "/actions/runs/2/attempts/1/jobs?per_page=100&page=1";
  const { impl, calls } = fakeFetch({
    "/actions/runs/1": respond(401, { message: "Bad credentials" }),
    "/actions/runs/2": respond(200, run({ id: 2 })),
    [jobsPath]: [respond(502, {}), respond(502, {})],
  });
  const result = await observe({
    repo: REPO,
    requests: [{ runId: "1" }, { runId: "2" }],
    fetchImpl: impl,
  });
  assert.deepEqual(result.summaries, []);
  assert.deepEqual(
    result.missing.map(({ runId, reason }) => [runId, reason]),
    [
      ["1", "/actions/runs/1: HTTP 401"],
      ["2", `${jobsPath}: HTTP 502`],
    ],
  );
  // The authorisation failure is not retried; the server error is, once.
  assert.equal(calls.filter((path) => path === "/actions/runs/1").length, 1);
  assert.equal(calls.filter((path) => path === jobsPath).length, 2);
});

test("a transient server or network error is read again once", async () => {
  const jobsPath = "/actions/runs/1/attempts/1/jobs?per_page=100&page=1";
  const { impl } = fakeFetch({
    "/actions/runs/1": [new Error("connection reset"), respond(200, run())],
    [jobsPath]: [
      respond(502, {}),
      respond(200, { total_count: 1, jobs: [job("CI Required", "success")] }),
    ],
  });
  const result = await observe({ repo: REPO, requests: [{ runId: "1" }], fetchImpl: impl });
  assert.equal(result.summaries.length, 1);
  assert.deepEqual(result.missing, []);
});

test("the request budget stops reading and reports what it could not read", async () => {
  const { impl, calls } = fakeFetch({ "/actions/runs/1": respond(200, run()) });
  const result = await observe({
    repo: REPO,
    requests: [{ runId: "1" }],
    fetchImpl: impl,
    maxRequests: 1,
  });
  assert.equal(calls.length, 1);
  assert.deepEqual(result.summaries, []);
  assert.match(result.missing[0].reason, /request budget of 1 exhausted/);
});

test("job pages that end before the declared total are not a complete record", async () => {
  const { impl } = fakeFetch({
    "/actions/runs/1": respond(200, run()),
    "/actions/runs/1/attempts/1/jobs?per_page=100&page=1": respond(200, {
      total_count: 2,
      jobs: [job("CI Required", "success")],
    }),
    "/actions/runs/1/attempts/1/jobs?per_page=100&page=2": respond(200, {
      total_count: 2,
      jobs: [],
    }),
  });
  const result = await observe({ repo: REPO, requests: [{ runId: "1" }], fetchImpl: impl });
  assert.deepEqual(result.summaries, []);
  assert.match(result.missing[0].reason, /job pages ended early/);
});

test("parseArgs accepts run attempts and rejects malformed input", () => {
  const options = parseArgs(["--repo", REPO, "--run", "12", "--run", "13@2", "--annotations"]);
  assert.deepEqual(options.requests, [{ runId: "12" }, { runId: "13", attempt: 2 }]);
  assert.equal(options.annotations, true);
  for (const argv of [
    ["--run", "12"],
    ["--repo", REPO],
    ["--repo", REPO, "--run", "latest"],
    ["--repo", REPO, "--run", "12", "--max-requests", "0"],
    ["--repo", REPO, "--run", "12", "--dispatch", "x"],
    ["--repo"],
  ]) {
    assert.throws(() => parseArgs(argv), undefined, argv.join(" "));
  }
});
