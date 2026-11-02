#!/usr/bin/env node

/**
 * ci-concurrency-observe.mjs — read-only observer for GitHub Actions
 * cancellation and queue behaviour.
 *
 * Records each requested run attempt with its own jobs, then reports:
 *   - run, job and aggregate states, including cancelled jobs that never
 *     reached a runner and the peak number of jobs running at once
 *   - any aggregate that read as passing beside a job that did not finish well
 *   - any run cancelled while an older run of the same pull request was being
 *     re-run, which leaves the newest completed green on an older tree
 *
 * It issues GET requests only and never dispatches, cancels or re-runs work.
 * A record it could not read stays listed under `missing` and fails the exit
 * status: an unreadable observation is never a passing one.
 *
 * Usage:
 *   node scripts/ci-concurrency-observe.mjs --repo <owner/name> \
 *     --run <id>[@attempt] [--run ...] [--aggregate "CI Required"] \
 *     [--annotations] [--token-env NAME] [--max-requests N] [--out file]
 *
 * Public repositories need no token; `--token-env` names the variable holding
 * one for a private trial repository.
 */

import { writeFileSync } from "node:fs";
import process from "node:process";
import { pathToFileURL } from "node:url";

const API_ROOT = "https://api.github.com";
const DEFAULT_AGGREGATE = "CI Required";
const DEFAULT_MAX_REQUESTS = 60;
const JOBS_PER_PAGE = 100;
const CANCEL_CAUSE_RE = /^Canceling since a higher priority waiting request for (.+) exists$/;
// Conclusions that do not satisfy an obligation. `skipped` is deliberately
// absent: the aggregate accepts it, so it is reported rather than judged.
const UNSATISFIED_CONCLUSIONS = new Set([
  "action_required",
  "cancelled",
  "failure",
  "neutral",
  "stale",
  "startup_failure",
  "timed_out",
]);

class ReadUnavailable extends Error {}

function peakRunningJobs(jobs) {
  const edges = [];
  for (const job of jobs) {
    if (!job.hadRunner || !job.startedAt || !job.completedAt) continue;
    const start = Date.parse(job.startedAt);
    const end = Date.parse(job.completedAt);
    // A job cancelled before it ran can report its start after its completion.
    if (end <= start) continue;
    edges.push([start, 1], [end, -1]);
  }
  edges.sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  let running = 0;
  let peak = 0;
  for (const [, delta] of edges) {
    running += delta;
    peak = Math.max(peak, running);
  }
  return peak;
}

/** Reduces one attempt's API records to the states the evidence retains. */
export function summarizeAttempt({ run, jobs, aggregateName = DEFAULT_AGGREGATE }) {
  const ownJobs = jobs.map((job) => ({
    id: job.id,
    name: job.name,
    status: job.status,
    conclusion: job.conclusion,
    createdAt: job.created_at,
    startedAt: job.started_at,
    completedAt: job.completed_at,
    hadRunner: Boolean(job.runner_name),
  }));
  const jobCounts = {};
  for (const job of ownJobs) {
    const state = `${job.status}/${job.conclusion}`;
    jobCounts[state] = (jobCounts[state] ?? 0) + 1;
  }
  const cancelled = ownJobs.filter((job) => job.conclusion === "cancelled");
  const aggregate = ownJobs.find((job) => job.name === aggregateName) ?? null;
  return {
    runId: run.id,
    attempt: run.run_attempt,
    ref: run.head_branch,
    event: run.event,
    pullRequests: (run.pull_requests ?? []).map((pull) => pull.number),
    testedHead: run.head_sha,
    status: run.status,
    conclusion: run.conclusion,
    createdAt: run.created_at,
    attemptStartedAt: run.run_started_at,
    updatedAt: run.updated_at,
    aggregate: aggregate && { status: aggregate.status, conclusion: aggregate.conclusion },
    jobCounts,
    cancelledWhileRunning: cancelled.filter((job) => job.hadRunner).length,
    cancelledWithoutRunner: cancelled.filter((job) => !job.hadRunner).length,
    peakRunningJobs: peakRunningJobs(ownJobs),
    jobs: ownJobs,
  };
}

/** Lists every way this attempt's aggregate misreports its own jobs. */
export function findVerdictViolations(summary, aggregateName = DEFAULT_AGGREGATE) {
  if (!summary.aggregate) return ["aggregate-missing"];
  if (summary.aggregate.conclusion !== "success") return [];
  const others = summary.jobs.filter((job) => job.name !== aggregateName);
  const violations = [];
  if (summary.conclusion === "cancelled") violations.push("passing-in-cancelled-run");
  if (others.some((job) => job.conclusion === "cancelled")) {
    violations.push("passing-with-cancelled-job");
  }
  if (others.some((job) => job.status !== "completed")) {
    violations.push("passing-with-unfinished-job");
  }
  if (
    others.some(
      (job) => job.conclusion !== "cancelled" && UNSATISFIED_CONCLUSIONS.has(job.conclusion),
    )
  ) {
    violations.push("passing-with-unsatisfied-job");
  }
  return violations;
}

/**
 * Finds runs cancelled while an older run of the same ref re-ran. An attempt's
 * record carries that attempt's creation time, so run ids order the runs. The
 * API does not name the request that displaced a run, so each result is a
 * candidate supported by the attempt timing and the recorded cancel cause.
 */
export function findOlderTreeDisplacements(summaries) {
  const displacements = [];
  for (const displaced of summaries) {
    if (displaced.conclusion !== "cancelled") continue;
    const windowStart = Date.parse(displaced.createdAt);
    const windowEnd = Date.parse(displaced.updatedAt);
    for (const rerun of summaries) {
      if (rerun.ref !== displaced.ref || rerun.event !== displaced.event) continue;
      if (rerun.attempt < 2 || rerun.runId >= displaced.runId) continue;
      const started = Date.parse(rerun.attemptStartedAt);
      if (started < windowStart || started > windowEnd) continue;
      displacements.push({
        displacedRunId: displaced.runId,
        displacedAggregate: displaced.aggregate?.conclusion ?? null,
        rerunRunId: rerun.runId,
        rerunAttempt: rerun.attempt,
        rerunConclusion: rerun.conclusion,
        rerunAggregate: rerun.aggregate?.conclusion ?? null,
      });
    }
  }
  return displacements;
}

/** Extracts the concurrency group named by a job's cancellation annotation. */
export function cancelCauseGroup(annotations) {
  for (const annotation of annotations) {
    const match = CANCEL_CAUSE_RE.exec(annotation.message ?? "");
    if (match) return match[1];
  }
  return null;
}

function createReader({ repo, fetchImpl, token, maxRequests }) {
  const state = { requestsUsed: 0, rateLimitRemaining: null };
  const headers = {
    accept: "application/vnd.github+json",
    "user-agent": "verter-ci-concurrency-observe",
    ...(token ? { authorization: `Bearer ${token}` } : {}),
  };
  const attemptRead = async (path) => {
    if (state.requestsUsed >= maxRequests) {
      throw new ReadUnavailable(`request budget of ${maxRequests} exhausted`);
    }
    state.requestsUsed += 1;
    let response;
    try {
      response = await fetchImpl(`${API_ROOT}/repos/${repo}${path}`, { method: "GET", headers });
    } catch (error) {
      return { retryable: true, reason: `network error: ${error.message}` };
    }
    const remaining = response.headers.get("x-ratelimit-remaining");
    if (remaining !== null) state.rateLimitRemaining = Number(remaining);
    if (response.ok) return { body: await response.json() };
    return { retryable: response.status >= 500, reason: `HTTP ${response.status}` };
  };
  const read = async (path) => {
    let result = await attemptRead(path);
    if (result.retryable) result = await attemptRead(path);
    if (result.body === undefined) throw new ReadUnavailable(`${path}: ${result.reason}`);
    return result.body;
  };
  return { read, state };
}

async function readAttemptJobs(read, runId, attempt) {
  const jobs = [];
  for (let page = 1; ; page += 1) {
    const body = await read(
      `/actions/runs/${runId}/attempts/${attempt}/jobs?per_page=${JOBS_PER_PAGE}&page=${page}`,
    );
    jobs.push(...body.jobs);
    if (jobs.length >= body.total_count) return jobs;
    if (body.jobs.length === 0) {
      throw new ReadUnavailable(`run ${runId} attempt ${attempt}: job pages ended early`);
    }
  }
}

async function readCancelCause(read, summary, reportMissing) {
  const job = summary.jobs.find((candidate) => candidate.conclusion === "cancelled");
  if (!job) return null;
  try {
    return cancelCauseGroup(await read(`/check-runs/${job.id}/annotations`));
  } catch (error) {
    if (!(error instanceof ReadUnavailable)) throw error;
    reportMissing(error.message);
    return null;
  }
}

/** Reads every requested attempt. `requests` holds `{ runId, attempt? }`. */
export async function observe({
  repo,
  requests,
  fetchImpl = fetch,
  token,
  aggregateName = DEFAULT_AGGREGATE,
  maxRequests = DEFAULT_MAX_REQUESTS,
  annotations = false,
}) {
  const { read, state } = createReader({ repo, fetchImpl, token, maxRequests });
  const summaries = [];
  const missing = [];
  for (const { runId, attempt } of requests) {
    try {
      const run = await read(
        attempt === undefined
          ? `/actions/runs/${runId}`
          : `/actions/runs/${runId}/attempts/${attempt}`,
      );
      const jobs = await readAttemptJobs(read, runId, run.run_attempt);
      const summary = summarizeAttempt({ run, jobs, aggregateName });
      if (annotations) {
        summary.cancelledByGroup = await readCancelCause(read, summary, (reason) =>
          missing.push({ runId, attempt: attempt ?? null, reason }),
        );
      }
      summaries.push({
        ...summary,
        verdictViolations: findVerdictViolations(summary, aggregateName),
      });
    } catch (error) {
      if (!(error instanceof ReadUnavailable)) throw error;
      missing.push({ runId, attempt: attempt ?? null, reason: error.message });
    }
  }
  return {
    repo,
    summaries,
    missing,
    olderTreeDisplacements: findOlderTreeDisplacements(summaries),
    requestsUsed: state.requestsUsed,
    rateLimitRemaining: state.rateLimitRemaining,
  };
}

/** Parses the command line; throws on anything it does not understand. */
export function parseArgs(argv) {
  const options = {
    requests: [],
    aggregateName: DEFAULT_AGGREGATE,
    maxRequests: DEFAULT_MAX_REQUESTS,
    annotations: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    if (flag === "--annotations") {
      options.annotations = true;
      continue;
    }
    index += 1;
    const value = argv[index];
    if (value === undefined) throw new Error(`${flag} needs a value`);
    if (flag === "--repo") options.repo = value;
    else if (flag === "--aggregate") options.aggregateName = value;
    else if (flag === "--token-env") options.tokenEnv = value;
    else if (flag === "--out") options.out = value;
    else if (flag === "--max-requests") {
      options.maxRequests = Number(value);
      if (!Number.isInteger(options.maxRequests) || options.maxRequests < 1) {
        throw new Error(`--max-requests must be a positive integer, got ${value}`);
      }
    } else if (flag === "--run") {
      const match = /^(\d+)(?:@(\d+))?$/.exec(value);
      if (!match) throw new Error(`--run must be <id>[@attempt], got ${value}`);
      options.requests.push({
        runId: match[1],
        ...(match[2] ? { attempt: Number(match[2]) } : {}),
      });
    } else throw new Error(`unknown argument ${flag}`);
  }
  if (!options.repo) throw new Error("--repo is required");
  if (options.requests.length === 0) throw new Error("at least one --run is required");
  return options;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const result = await observe({
    ...options,
    token: options.tokenEnv ? process.env[options.tokenEnv] : undefined,
  });
  const text = `${JSON.stringify(result, null, 2)}\n`;
  if (options.out) writeFileSync(options.out, text);
  else process.stdout.write(text);
  const violated = result.summaries.some((summary) => summary.verdictViolations.length > 0);
  process.exitCode = result.missing.length > 0 || violated ? 2 : 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(`ci-concurrency-observe: ${error.message}`);
    process.exitCode = 1;
  });
}
