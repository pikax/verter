#!/usr/bin/env node

/**
 * ci-baseline.mjs — collect and report the current CI baseline.
 *
 * `docs/contributing/ci-baseline.v1.md` says what the evidence means; scenarios
 * and metric definitions come from the versioned change corpus.
 * Collection is read-only: every API call is a GET, every attempt leaves a
 * receipt, and a denied or rate-limited read is retained as evidence instead of
 * being retried. Results belong to the experiment that collected them, never to
 * this repository.
 *
 *   node scripts/ci-baseline.mjs collect --out <dir> --repo <owner/name>
 *     --since <iso> --until <iso> [--anonymous] [--export <dir>]
 *     [--landing-ref <ref>] [--max-requests <n>] [--corpus <file>]
 *   node scripts/ci-baseline.mjs report --in <dir> [--out <file>]
 *     [--corpus <file>] [--require-measured]
 *
 * `collect` exits 0 once its receipts are written, whatever they say: an access
 * failure is a result. `report` exits 0 on a valid report; with
 * `--require-measured` it exits 1 unless the baseline is complete, which is how
 * a comparison refuses missing data. Malformed input exits 2.
 */

import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { loadCorpus } from "./ci-change-corpus.mjs";

const REPO_ROOT = fileURLToPath(new URL("..", import.meta.url));
const DEFAULT_CORPUS = join(REPO_ROOT, "scripts", "ci-change-corpus", "scenarios.v1.json");

/** The aggregate job `ci.yml` reports; the fallback when live required contexts are unreadable. */
const SOURCE_REQUIRED_CHECK = "CI Required";
/** Runs its proof step and the aggregate even when a landed push reuses a pull request's proof. */
const LANDED_PROOF_JOBS = new Set(["detect-changes", SOURCE_REQUIRED_CHECK]);
/** The collection protocol's repetition policy for every available scenario and stream. */
const REPETITIONS = 5;
/** Enough for one read's window: the runs API itself stops at 1,000 results. */
const MAX_PAGES_PER_READ = 10;
/** A dependent job's `created_at` equals its dependency's `completed_at`, to the API's one second. */
const DEPENDENCY_TOLERANCE_MS = 2000;

/** Each telemetry family a complete baseline needs, and who can supply it when the API cannot. */
export const FAMILIES = Object.freeze({
  runs: "repository read access",
  jobs: "repository read access",
  pulls: "repository read access",
  pullFiles: "repository read access",
  pushes: "repository read access, or a push-event export",
  requiredContexts: "repository read access",
  billing: "account billing owner",
  allocation: "account billing owner",
  readyEvents: "landing controller owner",
  bindings: "whoever submits the corpus changes as pull requests",
});

/** Owner exports, copied verbatim. A file here answers its family without an API call. */
const EXPORTS = Object.freeze({
  pushes: "push-events.json",
  billing: "billing.json",
  allocation: "allocation.json",
  readyEvents: "ready-events.json",
  bindings: "bindings.json",
});

const fail = (message) => {
  throw new Error(`ci-baseline: ${message}`);
};

const textDigest = (text) =>
  createHash("sha256").update(text.replace(/\r\n/g, "\n"), "utf8").digest("hex");

const gitValue = (root, args) => {
  const { status, stdout } = spawnSync("git", args, { cwd: root, encoding: "utf8" });
  return status === 0 ? stdout.trim() : "unavailable";
};

/** Every API read goes through this argument vector, so the driver cannot write. */
export const ghArgs = (path) => ["api", "--method", "GET", "--paginate", "--slurp", path];

export function classifyAttempt(status, detail = "") {
  if (status === 200) return "ok";
  if (status === 429 || (status === 403 && /rate limit/i.test(detail))) return "rate-limited";
  if (status === 401 || status === 403) return "denied";
  if (status === 404) return "not-found";
  return "failed";
}

/** The collection protocol's `gh api` reads. A failed read returns its status and message. */
export function ghTransport(path) {
  const { status, stdout, stderr, error } = spawnSync("gh", ghArgs(path), {
    encoding: "utf8",
    maxBuffer: 512 * 1024 * 1024,
  });
  if (error) return { status: 0, pages: [], detail: error.message, requests: 1 };
  if (status === 0) return { status: 200, pages: JSON.parse(stdout), detail: "", requests: 1 };
  const code = /HTTP (\d{3})/.exec(stderr)?.[1];
  return {
    status: code ? Number(code) : 0,
    pages: [],
    detail: stderr.trim().split("\n")[0],
    requests: 1,
  };
}

/** Unauthenticated reads of public data. Stops at the page budget; never waits out a limit. */
export async function anonymousTransport(
  path,
  { maxPages = 10, stopAfter, fetchImpl = fetch } = {},
) {
  const pages = [];
  let url = `https://api.github.com/${path}`;
  while (url) {
    if (pages.length >= maxPages) {
      return {
        status: 200,
        pages,
        detail: "page budget exhausted",
        partial: true,
        requests: pages.length,
      };
    }
    const response = await fetchImpl(url, {
      headers: { accept: "application/vnd.github+json", "user-agent": "verter-ci-baseline" },
      signal: AbortSignal.timeout(30_000),
    });
    const body = await response.json().catch(() => null);
    if (response.status !== 200) {
      const reset = response.headers.get("x-ratelimit-reset");
      const detail = `${body?.message ?? response.statusText}${reset ? ` (limit resets at epoch ${reset})` : ""}`;
      return {
        status: response.status,
        pages,
        detail,
        partial: pages.length > 0,
        requests: pages.length + 1,
      };
    }
    pages.push(body);
    if (stopAfter?.(body)) break;
    url = /<([^>]+)>;\s*rel="next"/.exec(response.headers.get("link") ?? "")?.[1];
  }
  return { status: 200, pages, detail: "", requests: pages.length };
}

/** One collection: raw bytes plus one receipt per attempted read. */
class Session {
  constructor({ out, transport, maxRequests, describe }) {
    Object.assign(this, { out, transport, maxRequests, describe });
    this.receipts = [];
    this.requests = 0;
    this.stopped = "";
    this.skipped = new Map();
  }

  store(family, body) {
    const file = `${String(this.receipts.length + 1).padStart(5, "0")}-${family}.json`;
    writeFileSync(join(this.out, "raw", file), `${JSON.stringify(body)}\n`);
    return `raw/${file}`;
  }

  skip(family, reason) {
    const entry = this.skipped.get(family) ?? { reason, count: 0 };
    entry.count += 1;
    this.skipped.set(family, entry);
  }

  async get(family, path, key = {}, { stopAfter } = {}) {
    if (this.stopped) return this.skip(family, this.stopped);
    if (this.requests >= this.maxRequests) {
      return this.skip(family, `request budget of ${this.maxRequests} exhausted`);
    }
    const reply = await this.transport(path, {
      maxPages: Math.min(MAX_PAGES_PER_READ, this.maxRequests - this.requests),
      stopAfter,
    });
    this.requests += reply.requests;
    const outcome = reply.partial ? "partial" : classifyAttempt(reply.status, reply.detail);
    const receipt = {
      family,
      ...key,
      command: this.describe(path),
      status: reply.status,
      outcome,
      detail: reply.detail,
      at: new Date().toISOString(),
    };
    if (reply.pages.length > 0) receipt.file = this.store(family, reply.pages);
    this.receipts.push(receipt);
    if (outcome === "rate-limited")
      this.stopped = `not attempted after a rate limit: ${reply.detail}`;
    return reply.pages.length > 0 ? reply.pages : undefined;
  }

  record(family, outcome, detail, body, key = {}) {
    const receipt = {
      family,
      ...key,
      command: detail,
      status: null,
      outcome,
      detail,
      at: new Date().toISOString(),
    };
    if (body !== undefined) receipt.file = this.store(family, body);
    this.receipts.push(receipt);
  }

  finish() {
    for (const [family, { reason, count }] of this.skipped) {
      this.record(family, "not-attempted", `${count} read(s) ${reason}`);
    }
  }
}

/** Collect raw records. Access failures become receipts; only misuse throws. */
export async function collect({
  out,
  repo,
  since,
  until,
  transport = ghTransport,
  describe = (path) => `gh ${ghArgs(path).join(" ")}`,
  exportDir,
  landingRef,
  maxRequests = 5000,
  corpusFile = DEFAULT_CORPUS,
  root = REPO_ROOT,
}) {
  if (!/^[^/\s]+\/[^/\s]+$/.test(repo ?? "")) fail("--repo must be owner/name");
  for (const [flag, value] of [
    ["--since", since],
    ["--until", until],
  ]) {
    if (!Number.isFinite(Date.parse(value))) fail(`${flag} must be an ISO timestamp`);
  }
  if (Date.parse(until) <= Date.parse(since)) fail("--until must follow --since");
  if (existsSync(out) && readdirSync(out).length > 0) fail(`${out} already holds a collection`);
  mkdirSync(join(out, "raw"), { recursive: true });

  const { corpus, digest } = loadCorpus(corpusFile);
  const session = new Session({ out, transport, maxRequests, describe });
  const owner = repo.split("/")[0];

  const exported = {};
  for (const [family, name] of Object.entries(EXPORTS)) {
    const source = exportDir && join(exportDir, name);
    if (!source || !existsSync(source)) continue;
    const file = `raw/export-${name}`;
    copyFileSync(source, join(out, file));
    exported[family] = JSON.parse(readFileSync(source, "utf8"));
    session.receipts.push({
      family,
      command: `owner export ${name}`,
      status: null,
      outcome: "export",
      detail: "",
      file,
      at: new Date().toISOString(),
    });
  }
  for (const family of ["allocation", "readyEvents", "bindings"]) {
    if (!exported[family]) {
      session.record(
        family,
        "no-export",
        `no ${EXPORTS[family]} supplied; owner: ${FAMILIES[family]}`,
      );
    }
  }

  if (landingRef) {
    const log = spawnSync(
      "git",
      ["log", "--first-parent", "--format=%cI", `--since=${since}`, `--until=${until}`, landingRef],
      { cwd: root, encoding: "utf8" },
    );
    if (log.status === 0) {
      const dates = log.stdout.split("\n").filter(Boolean);
      session.record("localLandings", "ok", `git log --first-parent ${landingRef}`, dates);
    } else {
      session.record("localLandings", "failed", `git log ${landingRef}: ${log.stderr.trim()}`);
    }
  }

  await session.get("requiredContexts", `repos/${repo}/rules/branches/main`);
  if (!exported.billing) await session.get("billing", `users/${owner}/settings/billing/usage`);
  if (!exported.pushes) {
    // Newest first, and far longer than any window: stop once a page reaches past its start.
    const passedWindow = (page) =>
      page.some(({ timestamp }) => Date.parse(timestamp) < Date.parse(since));
    await session.get(
      "pushes",
      `repos/${repo}/activity?per_page=100`,
      {},
      { stopAfter: passedWindow },
    );
  }

  const runPages = await session.get(
    "runs",
    `repos/${repo}/actions/runs?created=${since}..${until}&per_page=100`,
  );
  const runs = (runPages ?? []).flatMap((page) => page.workflow_runs ?? []);
  if (!runPages) {
    for (const family of ["jobs", "pulls"]) session.skip(family, "unreachable without run records");
  }

  const pullNumbers = new Set(
    runs.flatMap((run) => (run.pull_requests ?? []).map((pr) => pr.number)),
  );
  // A wholly skipped run held no runner, and most of a window's runs can be these.
  for (const run of runs.filter(({ conclusion }) => conclusion !== "skipped")) {
    for (let attempt = 1; attempt <= (run.run_attempt ?? 1); attempt += 1) {
      await session.get(
        "jobs",
        `repos/${repo}/actions/runs/${run.id}/attempts/${attempt}/jobs?per_page=100`,
        { run: run.id, attempt },
      );
    }
  }
  for (const pr of pullNumbers) await session.get("pulls", `repos/${repo}/pulls/${pr}`, { pr });

  const bound = new Set(boundPullRequests(exported.bindings ?? []));
  for (const pr of bound) {
    await session.get("pullFiles", `repos/${repo}/pulls/${pr}/files?per_page=100`, { pr });
  }
  if (bound.size === 0) session.skip("pullFiles", "no bound pull requests");
  session.finish();

  const manifest = {
    repo,
    window: { since, until },
    corpus: corpus.corpus,
    corpusDigest: digest,
    definitionsDigest: textDigest(
      [
        "docs/contributing/ci-change-corpus.v1.md",
        "docs/contributing/ci-change-corpus-collection.v1.md",
      ]
        .map((path) => {
          const absolute = join(root, path);
          return existsSync(absolute) ? textDigest(readFileSync(absolute, "utf8")) : "unavailable";
        })
        .join("\n"),
    ),
    collectedAt: new Date().toISOString(),
    head: gitValue(root, ["rev-parse", "HEAD"]),
    tree: gitValue(root, ["rev-parse", "HEAD^{tree}"]),
    dirty: gitValue(root, ["status", "--porcelain"]) !== "",
    requests: session.requests,
    node: process.version,
    platform: process.platform,
  };
  writeFileSync(join(out, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  writeFileSync(join(out, "receipts.json"), `${JSON.stringify(session.receipts, null, 2)}\n`);
  return { manifest, receipts: session.receipts };
}

const boundPullRequests = (bindings) =>
  bindings.flatMap((binding) =>
    binding.stream ? Object.values(binding.pullRequests ?? {}) : [binding.pullRequest],
  );

/** Nearest rank: the one-based `ceil(p * n)`th sorted sample. No samples, no answer. */
export function nearestRank(samples, p) {
  if (samples.length === 0) return null;
  const sorted = [...samples].sort((left, right) => left - right);
  return sorted[Math.ceil(p * sorted.length) - 1];
}

/** Seconds between two timestamps. An invalid order is unknown, never zero. */
const elapsed = (from, to) => {
  const start = Date.parse(from);
  const end = Date.parse(to);
  return Number.isFinite(start) && Number.isFinite(end) && end >= start
    ? (end - start) / 1000
    : null;
};

const percentiles = (samples) => ({
  n: samples.length,
  p50: nearestRank(samples, 0.5),
  p95: nearestRank(samples, 0.95),
});

/** A distribution, or `unavailable` with its reason: an absent sample is never a zero. */
const metric = (status, samples, reason, context = {}) =>
  samples.length > 0
    ? { status, ...percentiles(samples), ...context }
    : { status: "unavailable", reason, n: 0, ...context };

/** Runner intervals for jobs: skipped jobs hold no runner, unknown orders are counted, not zeroed. */
export function runnerIntervals(jobs) {
  const intervals = [];
  const tally = { skipped: 0, invalid: 0, outstanding: 0, neverAssigned: 0 };
  for (const job of jobs) {
    if (job.conclusion === "skipped") tally.skipped += 1;
    else if (job.status !== "completed" || !job.completed_at) tally.outstanding += 1;
    else if (neverAssigned(job)) {
      tally.neverAssigned += 1;
    } else {
      const seconds = elapsed(job.started_at, job.completed_at);
      if (seconds === null) tally.invalid += 1;
      else {
        intervals.push({
          job,
          name: job.name,
          start: Date.parse(job.started_at),
          end: Date.parse(job.completed_at),
          seconds,
        });
      }
    }
  }
  return { intervals, ...tally };
}

/** Sweep a set of intervals in event order; at equal timestamps completions precede starts. */
const sweep = (intervals, visit) => {
  const events = intervals.flatMap((interval) => [
    { at: interval.start, delta: 1, interval },
    { at: interval.end, delta: -1, interval },
  ]);
  events.sort((left, right) => left.at - right.at || left.delta - right.delta);
  for (const event of events) visit(event);
};

export function peakRunners(intervals) {
  let active = 0;
  let peak = 0;
  sweep(intervals, ({ delta }) => {
    active += delta;
    peak = Math.max(peak, active);
  });
  return peak;
}

/** The maximum share of the observed pool's active runners held by `own`'s jobs. */
export function peakShare(own, pool) {
  const mine = new Set(own.map(({ job }) => job));
  let active = 0;
  let held = 0;
  let peak = null;
  const events = [];
  sweep(pool, (event) => events.push(event));
  for (let index = 0; index < events.length; index += 1) {
    const event = events[index];
    active += event.delta;
    if (mine.has(event.interval.job)) held += event.delta;
    const boundaryCloses = events[index + 1]?.at !== event.at;
    if (boundaryCloses && active > 0) peak = Math.max(peak ?? 0, held / active);
  }
  return peak;
}

const groupBy = (items, keyOf) => {
  const groups = new Map();
  for (const item of items) {
    const key = keyOf(item);
    groups.set(key, [...(groups.get(key) ?? []), item]);
  }
  return groups;
};

/** One pull request's work, or one branch's: every event and workflow on that head branch. */
const streamOf = (run) => `${run.head_repository?.full_name ?? ""}:${run.head_branch ?? ""}`;

const neverAssigned = (job) =>
  job.conclusion === "cancelled" && "runner_name" in job && !job.runner_name;

const heldRunner = (job) => job.conclusion !== "skipped" && !neverAssigned(job);

/**
 * Which population a run reports under. A push that ran no lane, which is how a
 * landed push reusing its pull request's proof looks, is never a gate sample, and
 * a push whose jobs were not read says nothing either way.
 */
export function runClass(run, jobs, { pullStream = false } = {}) {
  if (run.event === "pull_request") return "pull-request";
  if (run.event !== "push") return "other";
  if (pullStream || run.pull_requests?.length > 0) return "pull-request";
  if (run.path && !/(^|\/)ci\.yml$/.test(run.path)) return "push-other-workflow";
  if (jobs.length === 0) return "push-unclassified";
  const lanesRan = jobs.some((job) => !LANDED_PROOF_JOBS.has(job.name) && heldRunner(job));
  return run.conclusion === "success" && !lanesRan ? "push-no-lanes" : "push-executed";
}

/**
 * Attribute push-to-green along the chain of jobs that led to the verdict, walking
 * back from it through each job's `created_at`, which is when GitHub made it
 * eligible. The parts telescope, so they sum to exactly the time from push to the
 * verdict's completion. Records without those timestamps have no split at all.
 */
export function criticalPath(verdict, jobs, pushedAt) {
  const siblings = jobs.filter(
    ({ run_id: runId, attempt }) => runId === verdict.run_id && attempt === verdict.attempt,
  );
  const parts = { preAdmission: 0, dependencyWait: 0, queue: 0, execution: 0 };
  const walked = new Set();
  let current = verdict;
  for (;;) {
    walked.add(current);
    const created = Date.parse(current.created_at);
    const queue = elapsed(current.created_at, current.started_at);
    const execution = elapsed(current.started_at, current.completed_at);
    if (queue === null || execution === null) return null;
    parts.queue += queue;
    parts.execution += execution;
    const candidates = siblings.filter((job) => {
      const gap = created - Date.parse(job.completed_at);
      return !walked.has(job) && gap >= 0 && gap <= DEPENDENCY_TOLERANCE_MS;
    });
    if (candidates.length === 0) {
      if (created < pushedAt) return null;
      parts.preAdmission = (created - pushedAt) / 1000;
      return parts;
    }
    const previous = candidates.reduce((latest, job) =>
      Date.parse(job.completed_at) > Date.parse(latest.completed_at) ? job : latest,
    );
    parts.dependencyWait += (created - Date.parse(previous.completed_at)) / 1000;
    current = previous;
  }
}

/**
 * One sample per head of a stream. A head is green at the latest of each
 * required context's first success; an optional lane finishing proves nothing.
 * A later head arriving before that green supersedes it, so it never counts.
 */
export function headSamples(runs, jobsOf, { required, pushAt }) {
  const samples = [];
  for (const [stream, streamRuns] of groupBy(runs, streamOf)) {
    const heads = [...groupBy(streamRuns, (run) => run.head_sha).entries()].map(
      ([head, headRuns]) => ({
        head,
        headRuns,
        createdAt: Math.min(...headRuns.map((run) => Date.parse(run.created_at))),
      }),
    );
    heads.sort((left, right) => left.createdAt - right.createdAt);
    heads.forEach(({ head, headRuns, createdAt }, index) => {
      const jobs = headRuns.flatMap(jobsOf);
      const verdicts = required.map((context) =>
        jobs
          .filter((job) => job.name === context && job.conclusion === "success")
          .filter((job) => Number.isFinite(Date.parse(job.completed_at)))
          .reduce(
            (first, job) =>
              !first || Date.parse(job.completed_at) < Date.parse(first.completed_at) ? job : first,
            null,
          ),
      );
      const verdict = verdicts.includes(null)
        ? null
        : verdicts.reduce((last, job) =>
            Date.parse(job.completed_at) > Date.parse(last.completed_at) ? job : last,
          );
      const greenAt = verdict ? Date.parse(verdict.completed_at) : null;
      const next = heads[index + 1]?.createdAt;
      const superseded = next !== undefined && (greenAt === null || next < greenAt);
      const failed = jobs.some(
        (job) =>
          required.includes(job.name) &&
          ["failure", "cancelled", "timed_out"].includes(job.conclusion),
      );
      const outcome =
        jobs.length === 0
          ? "unread"
          : superseded
            ? "superseded"
            : greenAt !== null
              ? "green"
              : failed
                ? "failed"
                : "outstanding";
      const pushed = Date.parse(pushAt(head) ?? "");
      const counted = outcome === "green" && Number.isFinite(pushed) && greenAt >= pushed;
      samples.push({
        stream,
        head,
        outcome,
        pushToGreen: counted ? (greenAt - pushed) / 1000 : null,
        split: counted ? criticalPath(verdict, jobs, pushed) : null,
        createdToGreen:
          outcome === "green" && greenAt >= createdAt ? (greenAt - createdAt) / 1000 : null,
      });
    });
  }
  return samples;
}

/** Every metric for one population, labelled `measured` or `observational` by the caller. */
function summarize(label, runs, ctx) {
  const samples = headSamples(runs, ctx.jobsOf, ctx);
  const outcomes = (name) => samples.filter((sample) => sample.outcome === name).length;
  const counts = {
    heads: samples.length,
    green: outcomes("green"),
    failures: outcomes("failed"),
    superseded: outcomes("superseded"),
    outstanding: outcomes("outstanding"),
    unread: outcomes("unread"),
  };
  const splits = samples.flatMap(({ split }) => (split ? [split] : []));

  const allJobs = runs.flatMap(ctx.jobsOf);
  const { intervals, skipped, invalid, outstanding, neverAssigned } = runnerIntervals(allJobs);
  const queueOf = ({ job }) => elapsed(job.created_at, job.started_at);
  const known = (values) => values.filter((value) => value !== null);

  const perStream = [...groupBy(runs, streamOf)].map(([stream, streamRuns]) => {
    const own = runnerIntervals(streamRuns.flatMap(ctx.jobsOf)).intervals;
    return {
      stream,
      peakRunners: peakRunners(own),
      peakShareOfObservedPool: peakShare(own, ctx.pool),
      runnerMinutes: own.reduce((total, { seconds }) => total + seconds, 0) / 60,
    };
  });

  return {
    runs: runs.length,
    ...counts,
    pushToGreen: metric(
      label,
      samples.flatMap(({ pushToGreen }) => (pushToGreen === null ? [] : [pushToGreen])),
      ctx.pushReason,
    ),
    runCreatedToGreen: metric(
      label,
      samples.flatMap(({ createdToGreen }) => (createdToGreen === null ? [] : [createdToGreen])),
      "no head reached every required context",
      { note: "Run creation is not the source push; never read this as push-to-green." },
    ),
    queueExecutionSplit:
      splits.length > 0
        ? {
            status: label,
            n: splits.length,
            basis: "job created_at as eligibility, walked back from the verdict job",
            parts: Object.fromEntries(
              ["preAdmission", "dependencyWait", "queue", "execution"].map((part) => {
                const { p50, p95 } = percentiles(splits.map((split) => split[part]));
                return [part, { p50, p95 }];
              }),
            ),
          }
        : { status: "unavailable", reason: ctx.pushReason },
    jobQueue: metric(label, known(intervals.map(queueOf)), "no job with a recorded eligibility", {
      note: "started_at minus the job's created_at: runner wait, without dependency wait.",
    }),
    jobExecution: metric(
      label,
      intervals.map(({ seconds }) => seconds),
      "no job with a valid runner interval",
      { skipped, invalid, outstanding, neverAssigned },
    ),
    peakRunnersPerStream:
      perStream.length > 0
        ? {
            status: label,
            max: Math.max(...perStream.map(({ peakRunners: peak }) => peak)),
            perStream,
          }
        : { status: "unavailable", reason: "no runner intervals" },
    runnerMinutes: {
      status: intervals.length > 0 ? label : "unavailable",
      total:
        intervals.length > 0
          ? intervals.reduce((total, { seconds }) => total + seconds, 0) / 60
          : null,
      note: "Elapsed minutes: not billed minutes and no OS multiplier.",
    },
    lanes: Object.fromEntries(
      [...groupBy(intervals, ({ name }) => name)]
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([name, members]) => [
          name,
          {
            execution: percentiles(members.map(({ seconds }) => seconds)),
            queue: percentiles(known(members.map(queueOf))),
            minutes: members.reduce((total, { seconds }) => total + seconds, 0) / 60,
          },
        ]),
    ),
  };
}

export function familyStatus(receipts, family) {
  const rows = receipts.filter((receipt) => receipt.family === family);
  const good = rows.filter(({ outcome }) => outcome === "ok" || outcome === "export");
  const bad = rows.filter(({ outcome }) => outcome !== "ok" && outcome !== "export");
  const status =
    good.length > 0 && bad.length === 0 ? "available" : good.length > 0 ? "partial" : "unavailable";
  return {
    status,
    owner: FAMILIES[family],
    ...(bad[0] ? { reason: `${bad[0].outcome}: ${bad[0].detail}` } : {}),
    ...(rows.length === 0 ? { reason: "no read attempted" } : {}),
  };
}

const sameFiles = (left, right) =>
  JSON.stringify([...left].sort()) === JSON.stringify([...right].sort());

/** Runs count for a scenario only when a pull request really carried its exact changed files. */
function resolveBindings(corpus, bindings, filesOf) {
  const samples = corpus.scenarios
    .filter(({ kind }) => kind === "change")
    .flatMap((scenario) =>
      scenario.samples.map((sample) => ({
        scenario: scenario.id,
        sample: sample.id,
        files: sample.changedFiles,
      })),
    );
  const sampleOf = (scenario, id) =>
    samples.find((row) => row.scenario === scenario && row.sample === id);
  const accepted = new Map(samples.map((row) => [`${row.scenario}/${row.sample}`, []]));
  const streams = new Map((corpus.streams ?? []).map((stream) => [stream.id, []]));
  const rejected = [];

  const carries = (pr, sample) => {
    const files = filesOf(pr);
    if (!sample) return "names no corpus sample";
    if (!files) return `pull request ${pr} changed files are unavailable`;
    return sameFiles(files, sample.files) ? "" : `pull request ${pr} changed different files`;
  };

  for (const binding of bindings) {
    if (binding.stream) {
      const stream = (corpus.streams ?? []).find(({ id }) => id === binding.stream);
      const problems = stream
        ? Object.entries(stream.pullRequests).map(([role, { scenario, sample }]) =>
            binding.pullRequests?.[role] === undefined
              ? `role ${role} is unbound`
              : carries(binding.pullRequests[role], sampleOf(scenario, sample)),
          )
        : ["names no corpus stream"];
      const problem = problems.find(Boolean);
      if (problem) rejected.push({ ...binding, reason: problem });
      else streams.get(binding.stream).push(binding);
    } else {
      const problem = carries(binding.pullRequest, sampleOf(binding.scenario, binding.sample));
      if (problem) rejected.push({ ...binding, reason: problem });
      else accepted.get(`${binding.scenario}/${binding.sample}`).push(binding);
    }
  }
  return { accepted, streams, rejected };
}

/**
 * Rows the versioned corpus itself says no hosted run can answer today: a stream
 * naming its missing seam, and a landed release commit, since nothing may be
 * published to collect one. A binding still counts, so a seam that appears needs
 * no change here; a row the corpus does not exempt can never be excused by a collector.
 */
function unavailableByCorpus(corpus) {
  const rows = new Map();
  for (const scenario of corpus.scenarios) {
    for (const sample of scenario.samples ?? []) {
      if (sample.release === "landed") {
        rows.set(
          `${scenario.id}/${sample.id}`,
          "needs a landed release commit, and no version may be published to collect one",
        );
      }
    }
  }
  for (const stream of corpus.streams ?? []) {
    if (stream.unavailableReason) rows.set(stream.id, stream.unavailableReason);
  }
  return rows;
}

const distinctRepetitions = (bindings) =>
  new Set(bindings.map(({ repetition }) => repetition)).size;

/**
 * Turn raw records into the report. `measured` is reachable only when every
 * family is complete and every corpus sample and stream has its repetitions;
 * everything else stays `missing-data`, whatever the window's runs look like.
 */
export function buildReport({ manifest, receipts, read, corpus }) {
  const pagesOf = (family) =>
    receipts.filter((row) => row.family === family && row.file).flatMap((row) => read(row.file));
  const families = Object.fromEntries(
    Object.keys(FAMILIES).map((family) => [family, familyStatus(receipts, family)]),
  );

  const runs = [
    ...new Map(
      pagesOf("runs")
        .flatMap((page) => page.workflow_runs ?? [])
        .map((run) => [run.id, run]),
    ).values(),
  ];
  const declared = pagesOf("runs")[0]?.total_count;
  const jobsByRun = new Map();
  for (const row of receipts) {
    if (row.family !== "jobs" || !row.file) continue;
    const jobs = read(row.file).flatMap((page) => page.jobs ?? []);
    const tagged = jobs.map((job) => ({ ...job, attempt: row.attempt }));
    jobsByRun.set(row.run, [...(jobsByRun.get(row.run) ?? []), ...tagged]);
  }
  const jobsOf = (run) => jobsByRun.get(run.id) ?? [];

  const rules = pagesOf("requiredContexts").flat();
  const liveRequired = rules
    .filter(({ type }) => type === "required_status_checks")
    .flatMap((rule) => rule.parameters?.required_status_checks ?? [])
    .map(({ context }) => context);
  const required = liveRequired.length > 0 ? [...new Set(liveRequired)] : [SOURCE_REQUIRED_CHECK];

  const pushes = new Map();
  for (const event of pagesOf("pushes").flat()) {
    const after = event.after ?? event.head;
    const at = event.timestamp ?? event.pushedAt;
    // A head pushed again keeps its first arrival: that is when its wait began.
    if (after && at && !(Date.parse(pushes.get(after)) <= Date.parse(at))) pushes.set(after, at);
  }

  const { intervals: pool } = runnerIntervals(runs.flatMap(jobsOf));
  const ctx = {
    jobsOf,
    required,
    pushAt: (head) => pushes.get(head),
    pushReason:
      families.pushes.status === "unavailable"
        ? `no source push events (${families.pushes.reason})`
        : "no green head with a recorded source push",
    pool,
  };

  const pullStreams = new Set(
    runs
      .filter((run) => run.event === "pull_request" || run.pull_requests?.length > 0)
      .map(streamOf),
  );
  const classOf = (run) =>
    runClass(run, jobsOf(run), { pullStream: pullStreams.has(streamOf(run)) });
  const observational = Object.fromEntries(
    [...groupBy(runs, classOf)].map(([population, members]) => [
      population,
      summarize("observational", members, ctx),
    ]),
  );

  const filesOf = (pr) => {
    const row = receipts.find(
      (receipt) => receipt.family === "pullFiles" && receipt.pr === pr && receipt.file,
    );
    return row
      ? read(row.file)
          .flat()
          .map(({ filename }) => filename)
      : null;
  };
  const bindings = pagesOf("bindings");
  const { accepted, streams, rejected } = resolveBindings(corpus, bindings, filesOf);
  const corpusUnavailable = unavailableByCorpus(corpus);
  // The runs API leaves `pull_requests` empty on many pull request runs, so the head branch counts.
  const headBranchOf = new Map(
    pagesOf("pulls")
      .flat()
      .filter(({ head }) => head)
      .map(({ number, head }) => [number, `${head.repo?.full_name ?? ""}:${head.ref}`]),
  );
  const runsOfPull = (pr) =>
    runs.filter(
      (run) =>
        (run.pull_requests ?? []).some(({ number }) => number === pr) ||
        streamOf(run) === headBranchOf.get(pr),
    );
  const coverage = (id, bound, pullsOf) => ({
    id,
    repetitions: distinctRepetitions(bound),
    required: REPETITIONS,
    ...(bound.length === 0 && corpusUnavailable.has(id)
      ? { unavailable: corpusUnavailable.get(id) }
      : {}),
    ...(bound.length > 0
      ? { metrics: summarize("measured", bound.flatMap(pullsOf).flatMap(runsOfPull), ctx) }
      : {}),
  });
  const scenarios = [...accepted].map(([id, bound]) =>
    coverage(id, bound, ({ pullRequest }) => [pullRequest]),
  );
  const concurrent = [...streams].map(([id, bound]) =>
    coverage(id, bound, ({ pullRequests }) => Object.values(pullRequests)),
  );

  const windowDays =
    (Date.parse(manifest.window.until) - Date.parse(manifest.window.since)) / 86400000;
  const inWindow = (at) =>
    Date.parse(at) >= Date.parse(manifest.window.since) &&
    Date.parse(at) <= Date.parse(manifest.window.until);
  const ready = pagesOf("readyEvents");
  const proven = ready.filter(
    ({ exactTreeProven, readyAt, landedAt }) =>
      exactTreeProven && elapsed(readyAt, landedAt) !== null,
  );
  const merged = pagesOf("pulls")
    .flat()
    .flatMap(({ merged_at: mergedAt }) => (mergedAt && inWindow(mergedAt) ? [mergedAt] : []));
  const localLandings = pagesOf("localLandings");

  const readyToLanded = metric(
    "measured",
    proven.map(({ readyAt, landedAt }) => elapsed(readyAt, landedAt)),
    `no ready event with an exact-tree proven landing (${families.readyEvents.reason ?? "export held none"})`,
    { censored: ready.length - proven.length },
  );
  let landingsPerDay;
  if (proven.length > 0) {
    const count = proven.filter(({ landedAt }) => inWindow(landedAt)).length;
    landingsPerDay = { status: "measured", count, windowDays, perDay: count / windowDays };
  } else if (merged.length > 0 || localLandings.length > 0) {
    const source =
      merged.length > 0 ? "merged pull requests" : "first-parent commits on the landing ref";
    const count = merged.length > 0 ? merged.length : localLandings.length;
    landingsPerDay = {
      status: "observational",
      source,
      count,
      windowDays,
      perDay: count / windowDays,
      note: "No exact-tree proof confirms these as landings of the proved tree.",
    };
  } else {
    landingsPerDay = { status: "unavailable", reason: "no landing record" };
  }

  const usage = pagesOf("billing").flatMap((page) => page.usageItems ?? []);
  const actionsUsage = usage.filter(({ product }) => /actions/i.test(product ?? ""));
  const spend =
    families.billing.status === "available" && actionsUsage.length > 0
      ? {
          status: "measured",
          netAmount: actionsUsage.reduce((total, { netAmount }) => total + (netAmount ?? 0), 0),
          items: actionsUsage.length,
        }
      : {
          status: "unavailable",
          reason: families.billing.reason ?? "no Actions usage item in the billing record",
        };
  const allocationRecords = pagesOf("allocation");
  const allocation =
    allocationRecords.length > 0
      ? { status: "measured", records: allocationRecords }
      : { status: "unavailable", reason: families.allocation.reason };

  const gaps = [];
  for (const [family, { status, reason }] of Object.entries(families)) {
    if (status !== "available") gaps.push(`family ${family} is ${status}: ${reason ?? "partial"}`);
  }
  for (const { id, repetitions, unavailable } of [...scenarios, ...concurrent]) {
    if (repetitions < REPETITIONS && !unavailable)
      gaps.push(`${id} has ${repetitions} of ${REPETITIONS} bound repetitions`);
  }
  for (const { reason, scenario, sample, stream } of rejected) {
    gaps.push(`binding ${stream ?? `${scenario}/${sample}`} rejected: ${reason}`);
  }
  if (spend.status !== "measured") gaps.push(`paid spend unavailable: ${spend.reason}`);
  if (readyToLanded.status !== "measured")
    gaps.push(`ready-to-landed unavailable: ${readyToLanded.reason}`);
  if (landingsPerDay.status !== "measured") gaps.push("landings/day has no exact-tree proof");
  if (liveRequired.length === 0) gaps.push("no live required context was read");

  const limitations = [
    "Cache condition per cell stays unknown: restore state is observed only on the job that restores it.",
    "A corpus sample's selection is not its execution: only bound hosted runs carry timing.",
  ];
  if (liveRequired.length === 0) {
    limitations.push(
      `${
        families.requiredContexts.status === "available"
          ? "The readable rules name no required status check; branch protection needs admin read."
          : "Live required contexts unread."
      } Verdicts use the source-declared "${SOURCE_REQUIRED_CHECK}" alone.`,
    );
  }
  if (Number.isFinite(declared) && declared > runs.length) {
    limitations.push(
      `The window declares ${declared} runs; ${runs.length} were read. Tail runs are absent.`,
    );
  }
  const held = runs.filter(({ conclusion }) => conclusion !== "skipped").length;
  if (held > jobsByRun.size) {
    limitations.push(
      `Job records exist for ${jobsByRun.size} of ${held} runs that were not skipped.`,
    );
  }

  return {
    manifest,
    baseline: {
      status: gaps.length === 0 ? "measured" : "missing-data",
      satisfiesComparisons: gaps.length === 0,
      gaps,
      unavailableByCorpus: [...scenarios, ...concurrent].flatMap(({ id, unavailable }) =>
        unavailable ? [{ id, reason: unavailable }] : [],
      ),
    },
    families,
    requiredContexts: {
      contexts: required,
      source: liveRequired.length > 0 ? "live ruleset" : "workflow source",
    },
    scenarios,
    concurrentStreams: concurrent,
    rejectedBindings: rejected,
    observational: {
      note: "Window runs unmatched to a corpus scenario are context only, never the baseline.",
      populations: observational,
    },
    readyToLanded,
    landingsPerDay,
    paidSpend: spend,
    hostedAllocation: allocation,
    limitations,
  };
}

/** Read one collection directory. Different corpus bytes answer a different question. */
export function reportFromDirectory(dir, { corpusFile = DEFAULT_CORPUS } = {}) {
  const parse = (file) => JSON.parse(readFileSync(join(dir, file), "utf8"));
  const manifest = parse("manifest.json");
  const { corpus, digest } = loadCorpus(corpusFile);
  if (manifest.corpusDigest !== digest) {
    fail(`${dir} read corpus ${manifest.corpusDigest}, not ${digest}; pass the archived --corpus`);
  }
  return buildReport({ manifest, receipts: parse("receipts.json"), read: parse, corpus });
}

function flagsOf(argv, known, booleans = []) {
  const flags = {};
  for (let index = 0; index < argv.length; index += 1) {
    const name = argv[index].replace(/^--/, "");
    if (booleans.includes(name)) flags[name] = true;
    else if (known.includes(name) && argv[index + 1] !== undefined)
      flags[name] = argv[(index += 1)];
    else fail(`unknown or incomplete argument ${argv[index]}`);
  }
  return flags;
}

async function main(argv = process.argv.slice(2)) {
  const [command, ...rest] = argv;
  if (command === "collect") {
    const flags = flagsOf(
      rest,
      ["out", "repo", "since", "until", "export", "landing-ref", "max-requests", "corpus"],
      ["anonymous"],
    );
    if (!flags.out) fail("collect needs --out");
    const anonymous = flags.anonymous === true;
    const { receipts } = await collect({
      out: resolve(flags.out),
      repo: flags.repo,
      since: flags.since,
      until: flags.until,
      exportDir: flags.export && resolve(flags.export),
      landingRef: flags["landing-ref"],
      maxRequests: flags["max-requests"] ? Number(flags["max-requests"]) : anonymous ? 50 : 5000,
      corpusFile: flags.corpus ? resolve(flags.corpus) : DEFAULT_CORPUS,
      ...(anonymous
        ? {
            transport: anonymousTransport,
            describe: (path) => `GET https://api.github.com/${path} (anonymous)`,
          }
        : {}),
    });
    const outcomes = groupBy(receipts, ({ family, outcome }) => `${family}: ${outcome}`);
    for (const [line, rows] of outcomes) console.log(`${line} (${rows.length})`);
    return 0;
  }
  if (command === "report") {
    const flags = flagsOf(rest, ["in", "out", "corpus"], ["require-measured"]);
    if (!flags.in) fail("report needs --in");
    const report = reportFromDirectory(resolve(flags.in), {
      corpusFile: flags.corpus ? resolve(flags.corpus) : DEFAULT_CORPUS,
    });
    const text = `${JSON.stringify(report, null, 2)}\n`;
    if (flags.out) writeFileSync(resolve(flags.out), text);
    else process.stdout.write(text);
    console.error(`baseline: ${report.baseline.status} (${report.baseline.gaps.length} gaps)`);
    return flags["require-measured"] && report.baseline.status !== "measured" ? 1 : 0;
  }
  fail("usage: ci-baseline.mjs <collect|report> ...");
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().then(
    (code) => {
      process.exitCode = code;
    },
    (error) => {
      console.error(error.message);
      process.exitCode = 2;
    },
  );
}
