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
  assert.equal(
    stricter.find((sample) => sample.head === "failed-then-rerun-green").outcome,
    "superseded",
  );
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
  const pull = (changedFiles) => {
    pr += 1;
    const head = `head-${pr}`;
    const id = pr * 10;
    runs.push(run(id, head, pr, { pull_requests: [{ number: pr }] }));
    pushEvents.push({ after: head, timestamp: at(pr - 0.5) });
    table[`repos/o/r/actions/runs/${id}/`] = [{ jobs: [job("CI Required", pr + 1, pr + 13)] }];
    table[`repos/o/r/pulls/${pr}/files`] = [changedFiles.map((filename) => ({ filename }))];
    table[`repos/o/r/pulls/${pr}`] = [{ number: pr, merged_at: at(pr + 20) }];
    return pr;
  };
  const filesOf = (scenario, sample) =>
    corpus.scenarios.find(({ id }) => id === scenario).samples.find(({ id }) => id === sample)
      .changedFiles;
  for (let repetition = 1; repetition <= 5; repetition += 1) {
    for (const scenario of corpus.scenarios.filter(({ kind }) => kind === "change")) {
      for (const sample of scenario.samples) {
        bindings.push({
          scenario: scenario.id,
          sample: sample.id,
          repetition,
          pullRequest: pull(sample.changedFiles),
        });
      }
    }
    for (const stream of corpus.streams) {
      const pullRequests = {};
      for (const [role, { scenario, sample }] of Object.entries(stream.pullRequests)) {
        pullRequests[role] = pull(filesOf(scenario, sample));
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
    const transport = (path) => {
      const reply = base(path);
      if (!path.includes("/actions/runs?")) {
        if (/\/pulls\/\d+$/.test(path)) {
          const pr = reply.pages[0];
          return {
            ...reply,
            pages: [{ ...pr, head: { ref: `branch-${pr.number}`, repo: { full_name: "o/r" } } }],
          };
        }
        return reply;
      }
      const [page] = reply.pages;
      const runs = page.workflow_runs.map((row) => {
        const number = row.pull_requests[0].number;
        return { ...row, head_branch: `branch-${number}` };
      });
      return { ...reply, pages: [{ ...page, workflow_runs: runs }] };
    };
    const out = join(dir, "run");
    await collect({ out, repo: "o/r", ...WINDOW, transport, exportDir: exports });
    const receipts = JSON.parse(readFileSync(join(out, "receipts.json"), "utf8"));
    const runsFile = receipts.find(({ family }) => family === "runs").file;
    const pages = JSON.parse(readFileSync(join(out, runsFile), "utf8"));
    for (const row of pages[0].workflow_runs) {
      if (row.pull_requests[0].number % 2 === 0) row.pull_requests = [];
    }
    writeFileSync(join(out, runsFile), JSON.stringify(pages));
    const report = reportFromDirectory(out);
    for (const { id, metrics } of report.scenarios) {
      assert.equal(metrics.pushToGreen.n, 5, id);
    }
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
