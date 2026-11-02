import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  anonymousTransport,
  buildReport,
  classifyAttempt,
  collect,
  criticalPath,
  FAMILIES,
  familyStatus,
  ghArgs,
  ghTransport,
  headSamples,
  nearestRank,
  peakRunners,
  peakShare,
  reportFromDirectory,
  runClass,
  runnerIntervals,
} from "./ci-baseline.mjs";
import { loadCorpus } from "./ci-change-corpus.mjs";

const SCRIPT = fileURLToPath(new URL("./ci-baseline.mjs", import.meta.url));
const { corpus } = loadCorpus();
const WINDOW = { since: "2026-10-01T00:00:00Z", until: "2026-10-08T00:00:00Z" };
const at = (minutes) => new Date(Date.parse(WINDOW.since) + minutes * 60_000).toISOString();

async function withTempDir(body) {
  const dir = mkdtempSync(join(tmpdir(), "ci-baseline-"));
  try {
    return await body(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

/** The collector records whether its tree was dirty; a measured fixture asserts a clean one. */
function markManifest(out, change) {
  const file = join(out, "manifest.json");
  writeFileSync(file, JSON.stringify({ ...JSON.parse(readFileSync(file, "utf8")), ...change }));
}
const markClean = (out) => markManifest(out, { dirty: false });

const job = (name, start, end, extra = {}) => ({
  name,
  status: "completed",
  conclusion: "success",
  created_at: at(start),
  started_at: at(start),
  completed_at: at(end),
  attempt: 1,
  ...extra,
});

const run = (id, head, created, extra = {}) => ({
  id,
  head_sha: head,
  event: "pull_request",
  created_at: at(created),
  run_attempt: 1,
  head_branch: "topic",
  head_repository: { full_name: "o/r" },
  pull_requests: [{ number: 1 }],
  ...extra,
});

/** Answers every read from a table; anything else gets the supplied status. */
const tableTransport = (
  table,
  fallback = { status: 401, detail: "gh: Bad credentials (HTTP 401)" },
) => {
  const seen = [];
  const transport = (path) => {
    seen.push(path);
    const key = Object.keys(table).find((prefix) => path.startsWith(prefix));
    return key
      ? { status: 200, pages: table[key], detail: "", requests: 1 }
      : { pages: [], requests: 1, ...fallback };
  };
  return { transport, seen };
};

test("percentiles are nearest rank, so p95 of five samples is the maximum", () => {
  assert.equal(nearestRank([5, 1, 4, 2, 3], 0.5), 3);
  assert.equal(nearestRank([5, 1, 4, 2, 3], 0.95), 5);
  assert.equal(nearestRank([10, 20], 0.5), 10);
  assert.equal(nearestRank([], 0.5), null);
});

test("runner intervals count failed work and keep unknown orders out of the totals", () => {
  const { intervals, skipped, invalid, outstanding, neverAssigned } = runnerIntervals([
    job("ran", 0, 10),
    job("failed", 0, 5, { conclusion: "failure" }),
    job("canceled while running", 0, 2, { conclusion: "cancelled", runner_name: "Hosted 3" }),
    job("skipped", 3, 3, { conclusion: "skipped" }),
    job("clock went backwards", 9, 1),
    job("queued cancel", 1, 1, { conclusion: "cancelled", runner_name: "" }),
    job("running", 0, 0, { status: "in_progress", conclusion: null, completed_at: null }),
  ]);
  assert.deepEqual(
    intervals.map(({ name, seconds }) => [name, seconds]),
    [
      ["ran", 600],
      ["failed", 300],
      ["canceled while running", 120],
    ],
  );
  assert.deepEqual(
    { skipped, invalid, outstanding, neverAssigned },
    { skipped: 1, invalid: 1, outstanding: 1, neverAssigned: 1 },
  );
});

test("peak runners process completions before starts at an equal timestamp", () => {
  const { intervals } = runnerIntervals([job("a", 0, 10), job("b", 10, 20), job("c", 5, 15)]);
  assert.equal(peakRunners(intervals), 2);
});

test("peak share divides by the whole observed pool at each boundary", () => {
  const { intervals: alone } = runnerIntervals([job("mine", 0, 10), job("other", 5, 15)]);
  assert.equal(
    peakShare(
      alone.filter(({ name }) => name === "mine"),
      alone,
    ),
    1,
  );
  const { intervals: late } = runnerIntervals([job("other", 0, 20), job("mine", 5, 10)]);
  assert.equal(
    peakShare(
      late.filter(({ name }) => name === "mine"),
      late,
    ),
    0.5,
  );
  // Two starts at one timestamp are one boundary: the share is read after both.
  const { intervals: together } = runnerIntervals([job("mine", 0, 10), job("other", 0, 10)]);
  assert.equal(
    peakShare(
      together.filter(({ name }) => name === "mine"),
      together,
    ),
    0.5,
  );
  assert.equal(peakShare([], []), null);
});

test("a push that ran no lane is separated from a push that ran its lanes", () => {
  const proof = [
    job("detect-changes", 0, 1),
    job("rust-test", 1, 1, { conclusion: "skipped" }),
    job("CI Required", 1, 2),
  ];
  const executed = [job("detect-changes", 0, 1), job("rust-test", 1, 9), job("CI Required", 9, 10)];
  const canceled = [
    job("rust-test", 1, 1, { conclusion: "cancelled", runner_name: "" }),
    job("CI Required", 1, 2),
  ];
  const push = { event: "push", conclusion: "success", path: ".github/workflows/ci.yml" };

  assert.equal(runClass(push, proof), "push-no-lanes");
  assert.equal(runClass(push, executed), "push-executed");
  // Canceled before any lane got a runner: nothing ran, but nothing was proved either.
  assert.equal(runClass({ ...push, conclusion: "cancelled" }, canceled), "push-executed");
  // No job record says nothing about whether lanes ran, so it never reads as reuse.
  assert.equal(runClass(push, []), "push-unclassified");
  // A workflow with no aggregate is never a gate sample for this tree.
  const tagging = { ...push, path: ".github/workflows/release-tag.yml" };
  assert.equal(runClass(tagging, [job("tag", 0, 1)]), "push-other-workflow");
  // Work a pull request's branch started on push is that pull request's work.
  assert.equal(runClass(push, executed, { pullStream: true }), "pull-request");
  assert.equal(runClass({ event: "pull_request" }, proof), "pull-request");
  assert.equal(runClass({ event: "schedule" }, executed), "other");
});

test("a main push naming a pull request does not fold the other proof-only pushes out", () => {
  const proof = [job("detect-changes", 0, 1), job("CI Required", 1, 2)];
  const push = (id, extra = {}) => ({
    ...run(id, `main-${id}`, 0),
    event: "push",
    conclusion: "success",
    head_branch: "main",
    path: ".github/workflows/ci.yml",
    pull_requests: [],
    ...extra,
  });
  const report = buildReport({
    manifest: { window: WINDOW },
    receipts: [
      { family: "runs", outcome: "ok", file: "runs" },
      { family: "jobs", outcome: "ok", file: "jobs1", run: 1, attempt: 1 },
      { family: "jobs", outcome: "ok", file: "jobs2", run: 2, attempt: 1 },
    ],
    read: (file) =>
      file === "runs"
        ? [
            {
              total_count: 2,
              workflow_runs: [push(1), push(2, { pull_requests: [{ number: 7 }] })],
            },
          ]
        : [{ jobs: proof }],
    corpus,
  });
  assert.ok(report.observational.populations["push-no-lanes"]);
});

test("the gh transport pages one request at a time and honours the budget and window stop", () => {
  const reply = (body, link) => ({
    status: 0,
    stdout: `HTTP/2.0 200 OK\r\n${link ? `Link: <https://api.github.com/p${link}>; rel="prev", <https://api.github.com/p${link}>; rel="next"\r\n` : ""}\r\n${JSON.stringify(body)}`,
    stderr: "",
  });
  const script = [reply([1], 2), reply([2], 3), reply([3])];
  const calls = [];
  const run = (_command, args) => {
    calls.push(args.at(-1));
    return script[calls.length - 1];
  };
  const whole = ghTransport("x", { run });
  assert.deepEqual(whole.pages, [[1], [2], [3]]);
  assert.equal(whole.requests, 3);
  assert.deepEqual(calls, ["x", "https://api.github.com/p2", "https://api.github.com/p3"]);

  calls.length = 0;
  const limited = ghTransport("x", { run, maxPages: 2 });
  assert.equal(limited.partial, true);
  assert.equal(limited.requests, 2);
  assert.equal(calls.length, 2);

  calls.length = 0;
  const stopped = ghTransport("x", { run, stopAfter: (page) => page[0] === 2 });
  assert.deepEqual(stopped.pages, [[1], [2]]);
  assert.equal(stopped.partial, undefined);

  const denied = ghTransport("x", {
    run: () => ({ status: 1, stdout: "", stderr: "gh: Not Found (HTTP 404)\n" }),
  });
  assert.equal(denied.status, 404);
});

test("a collection under other definitions or from a dirty tree refuses comparison", async () => {
  await withTempDir(async (dir) => {
    for (const change of [{ definitionsDigest: "other" }, { dirty: true }]) {
      const { exports, transport } = completeCollection(dir);
      const out = join(dir, `run-${Object.keys(change)[0]}`);
      await collect({ out, repo: "o/r", ...WINDOW, transport, exportDir: exports });
      markClean(out);
      markManifest(out, change);
      const { baseline } = reportFromDirectory(out);
      assert.equal(baseline.satisfiesComparisons, false);
      assert.equal(baseline.status, "missing-data");
      assert.equal(baseline.gaps.length, 1);
    }
  });
});

test("the split walks eligibility back from the verdict and sums to push-to-green", () => {
  const stamp = (name, created, started, completed) =>
    job(name, started, completed, { created_at: at(created) });
  const jobs = [
    stamp("detect-changes", 1, 4, 5),
    stamp("build", 5, 15, 25),
    // Finished earlier on a parallel branch: never on the causal path.
    stamp("lint", 5, 6, 7),
    stamp("test", 25, 30, 40),
    stamp("CI Required", 40, 41, 42),
  ];
  const verdict = jobs.at(-1);
  const pushedAt = Date.parse(at(0));
  const parts = criticalPath(verdict, jobs, pushedAt);
  assert.deepEqual(parts, {
    preAdmission: 60,
    dependencyWait: 0,
    queue: (3 + 10 + 5 + 1) * 60,
    execution: (1 + 10 + 10 + 1) * 60,
  });
  assert.equal(
    parts.preAdmission + parts.dependencyWait + parts.queue + parts.execution,
    (Date.parse(verdict.completed_at) - pushedAt) / 1000,
  );

  // Records with no eligibility have no split rather than a false one.
  const { created_at: _dropped, ...without } = verdict;
  assert.equal(criticalPath(without, jobs, pushedAt), null);
  // A job created before its source push cannot be attributed.
  assert.equal(criticalPath(verdict, jobs, Date.parse(at(2))), null);
});

test("green needs every required context, and only an unsuperseded head counts", () => {
  const jobs = new Map([
    [1, [job("CI Required", 1, 11, { conclusion: "failure" })]],
    [2, [job("CI Required", 12, 20)]],
    [3, [job("CI Required", 22, 30), job("Optional lane", 22, 40)]],
    [4, [job("CI Required", 41, 60)]],
    [5, [job("Optional lane", 62, 70)]],
  ]);
  const runs = [
    run(1, "failed-then-rerun-green", 0),
    run(2, "failed-then-rerun-green", 0),
    run(3, "green", 21),
    // Overtaken by the next head before its own verdict.
    run(4, "superseded", 41),
    run(5, "only-optional-lane-finished", 50),
  ];
  const pushed = {
    "failed-then-rerun-green": at(0),
    superseded: at(41),
    "only-optional-lane-finished": at(50),
  };
  const samples = headSamples(runs, ({ id }) => jobs.get(id), {
    required: ["CI Required"],
    pushAt: (head) => pushed[head],
  });
  const of = (head) => samples.find((sample) => sample.head === head);

  assert.equal(of("failed-then-rerun-green").outcome, "green");
  assert.equal(of("failed-then-rerun-green").pushToGreen, 1200);
  // Green, but its source push was never observed: unknown, not run-creation time and not zero.
  assert.equal(of("green").outcome, "green");
  assert.equal(of("green").pushToGreen, null);
  assert.equal(of("green").createdToGreen, 540);
  assert.equal(of("superseded").outcome, "superseded");
  assert.equal(of("superseded").pushToGreen, null);
  assert.equal(of("only-optional-lane-finished").outcome, "outstanding");
  assert.equal(of("failed-then-rerun-green").split.execution, 8 * 60);
  assert.equal(of("green").split, null);

  // A head whose jobs were never read is not an outstanding head.
  const unread = headSamples([run(9, "unread-head", 0)], () => [], {
    required: ["CI Required"],
    pushAt: () => at(0),
  });
  assert.equal(unread[0].outcome, "unread");

  const stricter = headSamples(runs, ({ id }) => jobs.get(id), {
    required: ["CI Required", "Optional lane"],
    pushAt: (head) => pushed[head],
  });
  assert.equal(stricter.find((sample) => sample.head === "green").createdToGreen, 1140);
  // A failure that completed before the next head arrived is a failure, not a supersession.
  assert.equal(
    stricter.find((sample) => sample.head === "failed-then-rerun-green").outcome,
    "failed",
  );
  const overtaken = headSamples(
    [run(1, "slow", 0), run(2, "next", 5)],
    ({ id }) =>
      id === 1
        ? [job("CI Required", 1, 11, { conclusion: "failure" })]
        : [job("CI Required", 6, 20)],
    { required: ["CI Required"], pushAt: () => at(0) },
  );
  // The next head arrived at minute 5, before the failure at minute 11.
  assert.equal(overtaken.find((sample) => sample.head === "slow").outcome, "superseded");
});

test("access failures become receipts, never records, and the baseline stays missing", async () => {
  await withTempDir(async (dir) => {
    const out = join(dir, "run");
    const { transport, seen } = tableTransport({});
    await collect({ out, repo: "o/r", ...WINDOW, transport });
    const report = reportFromDirectory(out);

    assert.equal(report.baseline.status, "missing-data");
    assert.equal(report.baseline.satisfiesComparisons, false);
    assert.ok(
      seen.every((path) => !path.includes("/jobs")),
      "no job read without a run record",
    );
    assert.equal(report.families.runs.status, "unavailable");
    assert.match(report.families.runs.reason, /denied: gh: Bad credentials \(HTTP 401\)/);
    assert.match(report.families.jobs.reason, /unreachable without run records/);
    assert.match(report.families.allocation.reason, /no allocation\.json supplied/);
    for (const family of Object.keys(FAMILIES)) {
      assert.notEqual(report.families[family].status, "available", family);
    }
    // Nothing fabricated: every headline figure is absent, not zero.
    assert.deepEqual(report.observational.populations, {});
    for (const metric of [
      report.readyToLanded,
      report.landingsPerDay,
      report.paidSpend,
      report.hostedAllocation,
    ]) {
      assert.equal(metric.status, "unavailable");
      assert.ok(!("p50" in metric) && !("perDay" in metric) && !("netAmount" in metric));
    }
    assert.ok(report.limitations.some((line) => /Live required contexts unread/.test(line)));
    assert.ok(report.baseline.gaps.includes("no live required context was read"));
  });
});

test("a rate limit stops collection at once and leaves what it skipped on record", async () => {
  await withTempDir(async (dir) => {
    const out = join(dir, "run");
    const { transport, seen } = tableTransport(
      {},
      { status: 403, detail: "API rate limit exceeded (limit resets at epoch 1)" },
    );
    await collect({ out, repo: "o/r", ...WINDOW, transport });
    assert.equal(seen.length, 1);
    const receipts = JSON.parse(readFileSync(join(out, "receipts.json"), "utf8"));
    assert.match(familyStatus(receipts, "requiredContexts").reason, /^rate-limited:/);
    assert.match(familyStatus(receipts, "runs").reason, /^not-attempted: .*after a rate limit/);
  });
});

test("attempts classify by status, and a rate limit is not a denial", () => {
  assert.equal(classifyAttempt(200), "ok");
  assert.equal(classifyAttempt(401), "denied");
  assert.equal(classifyAttempt(403, "Resource not accessible"), "denied");
  assert.equal(classifyAttempt(403, "API rate limit exceeded"), "rate-limited");
  assert.equal(classifyAttempt(429), "rate-limited");
  assert.equal(classifyAttempt(404), "not-found");
  assert.equal(classifyAttempt(0, "spawn gh ENOENT"), "failed");
});

test("every API read is an explicit GET", () => {
  const args = ghArgs("repos/o/r/actions/runs");
  assert.deepEqual(args.slice(0, 3), ["api", "--method", "GET"]);
  assert.ok(!args.some((arg) => /^(-f|-F|--field|--raw-field|--input)$/.test(arg)));
});

test("the anonymous transport reports a limit without waiting and marks a truncated read", async () => {
  const reply = (status, body, headers = {}) => ({
    status,
    statusText: "",
    json: async () => body,
    headers: new Headers(headers),
  });
  const limited = await anonymousTransport("x", {
    fetchImpl: async () =>
      reply(403, { message: "API rate limit exceeded" }, { "x-ratelimit-reset": "99" }),
  });
  assert.equal(limited.status, 403);
  assert.match(limited.detail, /rate limit exceeded \(limit resets at epoch 99\)/);
  assert.equal(classifyAttempt(limited.status, limited.detail), "rate-limited");

  const urls = [];
  const paged = await anonymousTransport("x", {
    maxPages: 1,
    fetchImpl: async (url) => {
      urls.push(url);
      return reply(200, [1], { link: '<https://api.github.com/next>; rel="next"' });
    },
  });
  assert.equal(paged.partial, true);
  assert.deepEqual(urls, ["https://api.github.com/x"]);
});

test("a window read in part is reported as partial, never as complete", async () => {
  await withTempDir(async (dir) => {
    const runs = [run(1, "h", 1), run(2, "h", 2)];
    const out = join(dir, "run");
    const { transport } = tableTransport({
      "repos/o/r/actions/runs?": [{ total_count: 7, workflow_runs: runs }],
      "repos/o/r/actions/runs/1/": [{ jobs: [job("CI Required", 2, 3)] }],
    });
    await collect({ out, repo: "o/r", ...WINDOW, transport });
    const report = reportFromDirectory(out);
    assert.equal(report.families.jobs.status, "partial");
    assert.equal(report.baseline.status, "missing-data");
    assert.ok(report.limitations.some((line) => /declares 7 runs; 2 were read/.test(line)));
    assert.ok(report.limitations.some((line) => /Job records exist for 1 of 2 runs/.test(line)));
  });
});

/** A complete collection: every corpus sample and stream bound, every owner export present. */
function completeCollection(dir, { mutate = (bindings) => bindings } = {}) {
  const exports = join(dir, "exports");
  mkdirSync(exports, { recursive: true });
  const table = {};
  const runs = [];
  const bindings = [];
  const pushEvents = [];
  let pr = 0;
  const pull = (changedFiles, release = "") => {
    pr += 1;
    const head = `head-${pr}`;
    const id = pr * 10;
    runs.push(run(id, head, pr, { pull_requests: [{ number: pr }] }));
    pushEvents.push({ after: head, timestamp: at(pr - 0.5) });
    table[`repos/o/r/actions/runs/${id}/`] = [{ jobs: [job("CI Required", pr + 1, pr + 13)] }];
    table[`repos/o/r/pulls/${pr}/files`] = [changedFiles.map((filename) => ({ filename }))];
    const title = release === "ide" ? "release(ide): v1" : release ? "release: v1" : "feat: change";
    table[`repos/o/r/pulls/${pr}`] = [{ number: pr, title, merged_at: at(pr + 20) }];
    return pr;
  };
  const filesOf = (scenario, sample) =>
    corpus.scenarios.find(({ id }) => id === scenario).samples.find(({ id }) => id === sample)
      .changedFiles;
  const releaseOf = (scenario, sample) =>
    corpus.scenarios.find(({ id }) => id === scenario).samples.find(({ id }) => id === sample)
      .release;
  for (let repetition = 1; repetition <= 5; repetition += 1) {
    for (const scenario of corpus.scenarios.filter(({ kind }) => kind === "change")) {
      for (const sample of scenario.samples) {
        bindings.push({
          scenario: scenario.id,
          sample: sample.id,
          repetition,
          pullRequest: pull(sample.changedFiles, sample.release),
        });
      }
    }
    for (const stream of corpus.streams) {
      const pullRequests = {};
      for (const [role, { scenario, sample }] of Object.entries(stream.pullRequests)) {
        pullRequests[role] = pull(filesOf(scenario, sample), releaseOf(scenario, sample));
      }
      bindings.push({ stream: stream.id, repetition, pullRequests });
    }
  }
  table["repos/o/r/actions/runs?"] = [{ total_count: runs.length, workflow_runs: runs }];
  table["repos/o/r/rules/branches/main"] = [
    [
      {
        type: "required_status_checks",
        parameters: { required_status_checks: [{ context: "CI Required" }] },
      },
    ],
  ];
  // Reads share prefixes (`pulls/1`, `pulls/10`, `pulls/1/files`), so longer keys answer first.
  const ordered = Object.fromEntries(
    Object.entries(table).sort(([left], [right]) => right.length - left.length),
  );
  const write = (name, value) => writeFileSync(join(exports, name), JSON.stringify(value));
  write("push-events.json", pushEvents);
  write("billing.json", { usageItems: [{ product: "Actions", netAmount: 0 }] });
  write("allocation.json", { concurrentJobs: 20, source: "account owner" });
  write("ready-events.json", [
    { pullRequest: 1, head: "head-1", readyAt: at(20), landedAt: at(30), exactTreeProven: true },
    { pullRequest: 2, head: "head-2", readyAt: at(25), exactTreeProven: false },
  ]);
  write("bindings.json", mutate(bindings));
  return { exports, ...tableTransport(ordered) };
}

test("a complete collection is measured, and its scenario metrics are real", async () => {
  await withTempDir(async (dir) => {
    const { exports, transport } = completeCollection(dir);
    const out = join(dir, "run");
    await collect({ out, repo: "o/r", ...WINDOW, transport, exportDir: exports });
    markClean(out);
    const report = reportFromDirectory(out);

    assert.deepEqual(report.baseline.gaps, []);
    assert.equal(report.baseline.status, "measured");
    assert.equal(report.requiredContexts.source, "live ruleset");
    const docs = report.scenarios.find(({ id }) => id === "CH01/docs");
    assert.equal(docs.repetitions, 5);
    assert.equal(docs.metrics.pushToGreen.status, "measured");
    assert.equal(docs.metrics.pushToGreen.n, 5);
    assert.equal(docs.metrics.pushToGreen.p95, 13.5 * 60);
    assert.equal(docs.metrics.runnerMinutes.total, 60);
    assert.equal(docs.metrics.queueExecutionSplit.status, "measured");
    assert.equal(docs.metrics.queueExecutionSplit.n, 5);
    assert.equal(docs.metrics.queueExecutionSplit.parts.execution.p95, 12 * 60);
    assert.equal(report.concurrentStreams.length, corpus.streams.length);
    assert.equal(report.readyToLanded.status, "measured");
    assert.equal(report.readyToLanded.censored, 1);
    assert.equal(report.landingsPerDay.status, "measured");
    assert.equal(report.paidSpend.status, "measured");
    assert.equal(report.hostedAllocation.status, "measured");
  });
});

test("losing a repetition, or binding the wrong change, keeps the baseline missing", async () => {
  const dropOne = (bindings) => bindings.slice(1);
  const wrongFiles = (bindings) => {
    const [first, second, ...rest] = bindings;
    return [{ ...first, pullRequest: second.pullRequest }, second, ...rest];
  };
  await withTempDir(async (dir) => {
    for (const [name, mutate, expected] of [
      ["dropped", dropOne, /CH01\/docs has 4 of 5 bound repetitions/],
      ["wrong", wrongFiles, /binding CH01\/docs rejected: pull request \d+ changed different/],
    ]) {
      const sub = join(dir, name);
      const { exports, transport } = completeCollection(sub, { mutate });
      const out = join(sub, "run");
      await collect({ out, repo: "o/r", ...WINDOW, transport, exportDir: exports });
      const { baseline } = reportFromDirectory(out);
      assert.equal(baseline.status, "missing-data", name);
      assert.ok(
        baseline.gaps.some((gap) => expected.test(gap)),
        `${name}: ${baseline.gaps.join("; ")}`,
      );
    }
  });
});

test("only rows the corpus itself exempts may stay unbound in a measured baseline", async () => {
  const exempt = (binding) => binding.stream || binding.sample === "landed-release-commit";
  await withTempDir(async (dir) => {
    const { exports, transport } = completeCollection(dir, {
      mutate: (bindings) => bindings.filter((binding) => !exempt(binding)),
    });
    const out = join(dir, "run");
    await collect({ out, repo: "o/r", ...WINDOW, transport, exportDir: exports });
    markClean(out);
    const { baseline } = reportFromDirectory(out);
    assert.deepEqual(baseline.gaps, []);
    assert.equal(baseline.status, "measured");
    assert.deepEqual(baseline.unavailableByCorpus.map(({ id }) => id).sort(), [
      "CC01",
      "CC02",
      "CH17/landed-release-commit",
    ]);
  });
  await withTempDir(async (dir) => {
    // A partly bound exempt row is a real shortfall, not an exemption.
    const { exports, transport } = completeCollection(dir, {
      mutate: (bindings) =>
        bindings.filter((binding) => binding.stream !== "CC01" || binding.repetition !== 5),
    });
    const out = join(dir, "run");
    await collect({ out, repo: "o/r", ...WINDOW, transport, exportDir: exports });
    const { baseline } = reportFromDirectory(out);
    assert.ok(baseline.gaps.includes("CC01 has 4 of 5 bound repetitions"));
  });
});

test("a run whose API record names no pull request still counts through its head branch", async () => {
  await withTempDir(async (dir) => {
    const { exports, transport: base } = completeCollection(dir);
    // The records are emptied at collection time, so the pull record is the only way back to a run.
    const transport = (path) => {
      const reply = base(path);
      if (/\/pulls\/\d+$/.test(path)) {
        const pr = reply.pages[0];
        return {
          ...reply,
          pages: [{ ...pr, head: { ref: `branch-${pr.number}`, repo: { full_name: "o/r" } } }],
        };
      }
      if (!path.includes("/actions/runs?")) return reply;
      const [page] = reply.pages;
      const runs = page.workflow_runs.map((row) => ({
        ...row,
        head_branch: `branch-${row.pull_requests[0].number}`,
        pull_requests: [],
      }));
      return { ...reply, pages: [{ ...page, workflow_runs: runs }] };
    };
    const out = join(dir, "run");
    await collect({ out, repo: "o/r", ...WINDOW, transport, exportDir: exports });
    markClean(out);
    const report = reportFromDirectory(out);
    assert.deepEqual(report.baseline.gaps, []);
    for (const { id, metrics } of report.scenarios) {
      assert.equal(metrics.pushToGreen.n, 5, id);
    }
  });
});

test("jobs with no recorded eligibility keep the baseline missing but the observations usable", async () => {
  await withTempDir(async (dir) => {
    const { exports, transport: base } = completeCollection(dir);
    const transport = (path) => {
      const reply = base(path);
      if (!/\/actions\/runs\/\d+\/.*jobs/.test(path)) return reply;
      return {
        ...reply,
        pages: reply.pages.map((page) => ({
          ...page,
          jobs: page.jobs.map(({ created_at: _dropped, ...rest }) => rest),
        })),
      };
    };
    const out = join(dir, "run");
    await collect({ out, repo: "o/r", ...WINDOW, transport, exportDir: exports });
    const { baseline, scenarios } = reportFromDirectory(out);
    assert.equal(baseline.status, "missing-data");
    assert.equal(baseline.satisfiesComparisons, false);
    assert.ok(
      baseline.gaps.some((gap) => /CH01\/docs has no queueExecutionSplit sample/.test(gap)),
    );
    assert.ok(baseline.gaps.some((gap) => /CH01\/docs has no jobQueue sample/.test(gap)));
    const docs = scenarios.find(({ id }) => id === "CH01/docs");
    assert.equal(docs.metrics.pushToGreen.n, 5);
    assert.equal(docs.metrics.jobExecution.status, "measured");
  });
});

test("collection skips job reads for skipped runs and stops paging past its window", async () => {
  await withTempDir(async (dir) => {
    const out = join(dir, "run");
    const runs = [run(1, "h", 1), run(2, "h", 2, { conclusion: "skipped" })];
    const { transport, seen } = tableTransport({
      "repos/o/r/actions/runs?": [{ total_count: 2, workflow_runs: runs }],
      "repos/o/r/actions/runs/1/": [{ jobs: [job("CI Required", 2, 3)] }],
    });
    await collect({ out, repo: "o/r", ...WINDOW, transport });
    assert.ok(!seen.some((path) => path.includes("/runs/2/")));
    assert.ok(!reportFromDirectory(out).limitations.some((line) => /Job records/.test(line)));

    const urls = [];
    const reply = (body) => ({
      status: 200,
      statusText: "",
      json: async () => body,
      headers: new Headers({ link: '<https://api.github.com/next>; rel="next"' }),
    });
    const read = await anonymousTransport("activity", {
      stopAfter: (page) => page.some(({ old }) => old),
      fetchImpl: async (url) => {
        urls.push(url);
        return reply([{ old: urls.length === 2 }]);
      },
    });
    assert.equal(read.partial, undefined);
    assert.equal(urls.length, 2);
  });
});

test("window runs unmatched to a scenario stay observational and never make a baseline", () => {
  const receipts = [
    { family: "runs", outcome: "ok", file: "runs" },
    { family: "jobs", outcome: "ok", file: "jobs", run: 1, attempt: 1 },
  ];
  const bodies = {
    runs: [{ total_count: 1, workflow_runs: [run(1, "h", 0)] }],
    jobs: [{ jobs: [job("CI Required", 1, 3)] }],
  };
  const report = buildReport({
    manifest: { window: WINDOW },
    receipts,
    read: (file) => bodies[file],
    corpus,
  });
  assert.equal(report.baseline.status, "missing-data");
  const observed = report.observational.populations["pull-request"];
  assert.equal(observed.runCreatedToGreen.status, "observational");
  assert.equal(observed.pushToGreen.status, "unavailable");
  assert.equal(observed.jobExecution.p50, 120);
  assert.equal(observed.jobQueue.p50, 0);
  assert.equal(observed.queueExecutionSplit.status, "unavailable");
  assert.equal(observed.peakRunnersPerStream.max, 1);
  assert.equal(observed.peakRunnersPerStream.perStream[0].peakShareOfObservedPool, 1);
  assert.deepEqual(observed.lanes["CI Required"], {
    execution: { n: 1, p50: 120, p95: 120 },
    queue: { n: 1, p50: 0, p95: 0 },
    minutes: 2,
  });
});

test("a directory already holding a collection is refused, and other corpus bytes too", async () => {
  await withTempDir(async (dir) => {
    const out = join(dir, "run");
    const { transport } = tableTransport({});
    await collect({ out, repo: "o/r", ...WINDOW, transport });
    await assert.rejects(
      collect({ out, repo: "o/r", ...WINDOW, transport }),
      /already holds a collection/,
    );
    await assert.rejects(
      collect({ out: join(dir, "b"), repo: "nope", ...WINDOW, transport }),
      /--repo must be owner\/name/,
    );

    const other = join(dir, "other-corpus.json");
    writeFileSync(other, JSON.stringify({ ...corpus, corpus: "other" }));
    assert.throws(
      () => reportFromDirectory(out, { corpusFile: other }),
      /pass the archived --corpus/,
    );
  });
});

test("the report command refuses missing data only when a measured baseline is required", async () => {
  await withTempDir(async (dir) => {
    const out = join(dir, "run");
    const { transport } = tableTransport({});
    await collect({ out, repo: "o/r", ...WINDOW, transport });
    const reportFile = join(dir, "report.json");
    const options = { encoding: "utf8" };
    const plain = spawnSync(
      process.execPath,
      [SCRIPT, "report", "--in", out, "--out", reportFile],
      options,
    );
    assert.equal(plain.status, 0, plain.stderr);
    assert.match(plain.stderr, /baseline: missing-data/);
    assert.equal(JSON.parse(readFileSync(reportFile, "utf8")).baseline.status, "missing-data");
    const strict = spawnSync(
      process.execPath,
      [SCRIPT, "report", "--in", out, "--require-measured"],
      options,
    );
    assert.equal(strict.status, 1);
    const misuse = spawnSync(process.execPath, [SCRIPT, "report"], options);
    assert.equal(misuse.status, 2);
  });
});

test("local landing history is observational: no exact-tree proof backs it", async () => {
  await withTempDir(async (dir) => {
    const out = join(dir, "run");
    const { transport } = tableTransport({});
    await collect({
      out,
      repo: "o/r",
      since: "2000-01-01T00:00:00Z",
      until: "2100-01-01T00:00:00Z",
      transport,
      landingRef: "HEAD",
    });
    const { landingsPerDay, baseline } = reportFromDirectory(out);
    assert.equal(landingsPerDay.status, "observational");
    assert.ok(landingsPerDay.count > 0);
    assert.equal(baseline.status, "missing-data");
    assert.ok(baseline.gaps.includes("landings/day has no exact-tree proof"));
  });
});

/** Collect a complete fixture after `change` has rewritten its exports, and report it. */
async function reportAfter(dir, change = () => {}, options = {}) {
  const { exports, transport } = completeCollection(dir, options);
  const rewrite = (name, value) => writeFileSync(join(exports, name), JSON.stringify(value));
  const exported = (name) => JSON.parse(readFileSync(join(exports, name), "utf8"));
  change({ rewrite, exported });
  const out = join(dir, "run");
  await collect({ out, repo: "o/r", ...WINDOW, transport, exportDir: exports });
  markClean(out);
  return reportFromDirectory(out);
}

test("a network failure on the anonymous route is a receipt, not a lost collection", async () => {
  const refused = await anonymousTransport("x", {
    fetchImpl: async () => {
      throw new TypeError("fetch failed");
    },
  });
  assert.equal(refused.status, 0);
  assert.match(refused.detail, /fetch failed/);
  const notJson = await anonymousTransport("x", {
    fetchImpl: async () => ({
      status: 200,
      statusText: "",
      json: async () => {
        throw new SyntaxError("Unexpected token <");
      },
      headers: new Headers(),
    }),
  });
  assert.equal(notJson.status, 0);
  assert.equal(classifyAttempt(notJson.status, notJson.detail), "failed");

  await withTempDir(async (dir) => {
    const out = join(dir, "run");
    const transport = () => {
      throw new Error("socket hang up");
    };
    await collect({ out, repo: "o/r", ...WINDOW, transport });
    const receipts = JSON.parse(readFileSync(join(out, "receipts.json"), "utf8"));
    assert.match(familyStatus(receipts, "runs").reason, /^failed: socket hang up/);
    assert.equal(reportFromDirectory(out).baseline.status, "missing-data");
  });
});

test("a malformed owner export is refused before any read and leaves the directory reusable", async () => {
  await withTempDir(async (dir) => {
    const exports = join(dir, "exports");
    mkdirSync(exports);
    writeFileSync(join(exports, "bindings.json"), JSON.stringify({ not: "an array" }));
    const { transport, seen } = tableTransport({});
    const out = join(dir, "run");
    await assert.rejects(
      collect({ out, repo: "o/r", ...WINDOW, transport, exportDir: exports }),
      /bindings\.json must be a JSON array/,
    );
    assert.equal(seen.length, 0);
    writeFileSync(join(exports, "bindings.json"), "[]");
    await collect({ out, repo: "o/r", ...WINDOW, transport, exportDir: exports });
  });
});

test("one pull request bound for every repetition is one repetition, not five", async () => {
  await withTempDir(async (dir) => {
    const report = await reportAfter(dir, () => {}, {
      mutate: (bindings) => {
        const docs = bindings.filter(({ sample }) => sample === "docs");
        return bindings.map((binding) =>
          binding.sample === "docs" ? { ...binding, pullRequest: docs[0].pullRequest } : binding,
        );
      },
    });
    assert.equal(report.baseline.status, "missing-data");
    const docs = report.scenarios.find(({ id }) => id === "CH01/docs");
    assert.equal(docs.repetitions, 1);
    assert.equal(docs.metrics.runnerMinutes.total, 12);
    assert.equal(report.rejectedBindings.length, 4);
    assert.match(report.rejectedBindings[0].reason, /already bound to another repetition/);
  });
  await withTempDir(async (dir) => {
    const report = await reportAfter(dir, () => {}, {
      mutate: (bindings) =>
        bindings.map((binding, index) => (index === 0 ? { ...binding, repetition: 9 } : binding)),
    });
    assert.ok(
      report.baseline.gaps.some((gap) => /repetition must be an integer from 1 to 5/.test(gap)),
    );
  });
});

test("an ordinary pull request touching the release manifest is not a release sample", async () => {
  await withTempDir(async (dir) => {
    const ordinary = (bindings) => {
      const manifestPr = bindings.find(({ scenario }) => scenario === "CH12").pullRequest;
      return bindings.map((binding) =>
        binding.sample === "project-release-pr" && binding.repetition === 1
          ? { ...binding, pullRequest: manifestPr }
          : binding,
      );
    };
    const report = await reportAfter(dir, () => {}, { mutate: ordinary });
    assert.ok(
      report.baseline.gaps.some((gap) =>
        /CH17\/project-release-pr rejected: pull request \d+ is release kind "", not "project"/.test(
          gap,
        ),
      ),
    );
  });
});

test("a baseline needs every bound pull request to reach a push-to-green sample", async () => {
  await withTempDir(async (dir) => {
    const report = await reportAfter(dir, ({ rewrite }) => rewrite("push-events.json", []));
    assert.equal(report.baseline.status, "missing-data");
    assert.equal(report.baseline.satisfiesComparisons, false);
    assert.ok(
      report.baseline.gaps.some((gap) =>
        /^CH01\/docs pull request\(s\) .* no push-to-green/.test(gap),
      ),
    );
  });
  await withTempDir(async (dir) => {
    // Only some runs fall in the window: the rest are bound but absent.
    const report = await reportAfter(dir, ({ exported, rewrite }) => {
      rewrite("push-events.json", exported("push-events.json").slice(0, 10));
    });
    assert.equal(report.baseline.status, "missing-data");
  });
});

test("a window whose runs were read in part is a gap, whatever else is complete", async () => {
  await withTempDir(async (dir) => {
    const { exports, transport: base } = completeCollection(dir);
    const transport = (path) => {
      const reply = base(path);
      return path.includes("/actions/runs?")
        ? { ...reply, pages: [{ ...reply.pages[0], total_count: 5000 }] }
        : reply;
    };
    const out = join(dir, "run");
    await collect({ out, repo: "o/r", ...WINDOW, transport, exportDir: exports });
    const { baseline } = reportFromDirectory(out);
    assert.equal(baseline.status, "missing-data");
    assert.ok(baseline.gaps.some((gap) => /runs read in part: the window declares 5000/.test(gap)));
  });
});

test("landing figures need a literal proof, the window, and a corpus-bound pull request", async () => {
  const proven = (extra) => ({
    pullRequest: 1,
    head: "head-1",
    readyAt: at(20),
    landedAt: at(30),
    exactTreeProven: true,
    ...extra,
  });
  for (const [name, events] of [
    [
      "before the window",
      [proven({ readyAt: "2020-01-01T00:00:00Z", landedAt: "2020-01-02T00:00:00Z" })],
    ],
    ["a truthy string", [proven({ exactTreeProven: "false" })]],
    ["a pull request outside the corpus", [proven({ pullRequest: 99999 })]],
  ]) {
    await withTempDir(async (dir) => {
      const report = await reportAfter(dir, ({ rewrite }) => rewrite("ready-events.json", events));
      assert.equal(report.readyToLanded.status, "unavailable", name);
      assert.equal(report.landingsPerDay.status, "observational", name);
      assert.equal(report.baseline.status, "missing-data", name);
    });
  }
  await withTempDir(async (dir) => {
    const report = await reportAfter(dir);
    const docs = report.scenarios.find(({ id }) => id === "CH01/docs");
    assert.equal(docs.landings, 1);
    assert.equal(docs.readyToLanded.p50, 600);
    assert.equal(report.scenarios.find(({ id }) => id === "CH02/wasm-root").landings, 0);
    assert.equal(report.landingsPerDay.count, 1);
  });
});

test("spend with no amount and an empty allocation are gaps, never zeros", async () => {
  await withTempDir(async (dir) => {
    const report = await reportAfter(dir, ({ rewrite }) => {
      rewrite("billing.json", { usageItems: [{ product: "Actions" }] });
      rewrite("allocation.json", {});
    });
    assert.equal(report.hostedAllocation.status, "unavailable");
    assert.equal(report.paidSpend.status, "unavailable");
    assert.match(report.paidSpend.reason, /no net amount/);
    assert.ok(report.baseline.gaps.some((gap) => /^hosted allocation unavailable/.test(gap)));
  });
  await withTempDir(async (dir) => {
    const report = await reportAfter(dir, ({ rewrite }) =>
      rewrite("billing.json", {
        timePeriod: { year: 2026, month: 10 },
        usageItems: [{ product: "Actions", netAmount: 3.5 }],
      }),
    );
    assert.deepEqual(report.paidSpend.periods, [{ year: 2026, month: 10 }]);
    assert.equal(report.paidSpend.netAmount, 3.5);
  });
});

test("required contexts can come from an owner export or from branch protection", async () => {
  await withTempDir(async (dir) => {
    const { exports, transport } = completeCollection(dir);
    writeFileSync(join(exports, "required-contexts.json"), JSON.stringify(["CI Required", "Lint"]));
    const out = join(dir, "run");
    await collect({ out, repo: "o/r", ...WINDOW, transport, exportDir: exports });
    const report = reportFromDirectory(out);
    assert.deepEqual(report.requiredContexts.contexts, ["CI Required", "Lint"]);
    assert.equal(report.families.requiredContexts.status, "available");
  });
  await withTempDir(async (dir) => {
    const { exports, transport: base } = completeCollection(dir);
    const transport = (path) =>
      path.endsWith("/rules/branches/main")
        ? { status: 200, pages: [[]], detail: "", requests: 1 }
        : path.endsWith("/protection/required_status_checks")
          ? {
              status: 200,
              pages: [{ contexts: ["CI Required"], checks: [] }],
              detail: "",
              requests: 1,
            }
          : base(path);
    const out = join(dir, "run");
    await collect({ out, repo: "o/r", ...WINDOW, transport, exportDir: exports });
    const report = reportFromDirectory(out);
    assert.deepEqual(report.requiredContexts.contexts, ["CI Required"]);
    assert.ok(!report.baseline.gaps.includes("no live required context was read"));
  });
});

test("a rerun's copied jobs add no runner time, and a head's push time is its own branch's", () => {
  const original = job("build", 0, 10, { id: 7, attempt: 1 });
  // The live shape: the copy carries a new id and a later created_at, but the same name and interval.
  const copy = { ...original, id: 9, attempt: 2, created_at: at(60) };
  const rerun = [original, copy, job("test", 12, 14, { id: 8, attempt: 2 })];
  const { intervals, copied } = runnerIntervals(rerun);
  assert.equal(copied, 1);
  assert.equal(
    intervals.reduce((total, { seconds }) => total + seconds, 0),
    12 * 60,
  );
  assert.equal(peakRunners(intervals), 1);

  const report = buildReport({
    manifest: { window: WINDOW },
    receipts: [
      { family: "runs", outcome: "ok", file: "runs" },
      { family: "jobs", outcome: "ok", file: "jobs", run: 1, attempt: 1 },
      { family: "pushes", outcome: "export", file: "pushes" },
    ],
    read: (file) =>
      ({
        runs: [{ total_count: 1, workflow_runs: [run(1, "same", 599, { head_branch: "second" })] }],
        jobs: [{ jobs: [job("CI Required", 600, 602)] }],
        pushes: [
          [
            { after: "same", ref: "refs/heads/first", timestamp: at(0) },
            { after: "same", ref: "refs/heads/second", timestamp: at(599) },
          ],
        ],
      })[file],
    corpus,
  });
  assert.equal(report.observational.populations["pull-request"].pushToGreen.p50, 180);
});

test("a zone-offset window is queried in UTC and a non-ISO window is refused", async () => {
  await withTempDir(async (dir) => {
    const { transport, seen } = tableTransport({});
    await collect({
      out: join(dir, "run"),
      repo: "o/r",
      since: "2026-10-01T01:00:00+01:00",
      until: "2026-10-08T00:00:00Z",
      transport,
    });
    const runsRead = seen.find((path) => path.includes("/actions/runs?"));
    assert.ok(runsRead.includes("created=2026-10-01T00:00:00Z..2026-10-08T00:00:00Z"), runsRead);
    assert.ok(!runsRead.includes("+"), runsRead);
    await assert.rejects(
      collect({
        out: join(dir, "other"),
        repo: "o/r",
        since: "October 1 2026",
        until: WINDOW.until,
        transport,
      }),
      /--since must be an ISO 8601 timestamp/,
    );
  });
});

test("malformed flags are misuse", async () => {
  await withTempDir(async (dir) => {
    const out = join(dir, "run");
    await collect({ out, repo: "o/r", ...WINDOW, transport: tableTransport({}).transport });
    const run = (argv) => spawnSync(process.execPath, [SCRIPT, ...argv], { encoding: "utf8" });
    assert.equal(run(["report", "--in", out]).status, 0);
    // A name without its dashes, or a flag swallowing the next flag as its value, is not a flag.
    assert.equal(run(["report", "in", out]).status, 2);
    assert.equal(run(["report", "--in", "--require-measured"]).status, 2);
    assert.equal(
      run(["collect", "--out", join(dir, "x"), "--repo", "o/r", "--max-requests", "many"]).status,
      2,
    );
  });
});
