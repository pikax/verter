#!/usr/bin/env node

/**
 * ci-concurrency-observe.mjs — read-only observer for GitHub Actions
 * cancellation and queue behaviour.
 *
 * Records each requested run attempt with its own jobs, then reports:
 *   - run, job and aggregate states, including cancelled jobs that never
 *     reached a runner and the peak number of jobs running at once
 *   - any aggregate that read as passing beside a job that did not finish well,
 *     a skipped aggregate, or a named `--obligation` that was skipped or absent
 *   - any run cancelled while an older run of the same pull request was being
 *     re-run, which leaves the newest completed green on an older tree, with the
 *     evidence that the two share a concurrency group
 *
 * It issues GET requests only and never dispatches, cancels or re-runs work.
 * A record it could not read stays listed under `missing` and fails the exit
 * status: an unreadable observation is never a passing one.
 *
 * Usage:
 *   node scripts/ci-concurrency-observe.mjs --repo <owner/name> \
 *     --run <id>[@attempt] [--run ...] [--aggregate "CI Required"] \
 *     [--annotations | --annotations-all] [--obligation <job>]...  *     [--token-env NAME] [--max-requests N] [--out file]
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
// GitHub does not document this annotation text. The wording is the one read from
// the recorded runs listed in docs/contributing/ci-inventory/concurrency.md, so
// any other message is reported as unrecognised, never as "no cause".
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

// A job still in progress in the snapshot has not ended, so it overlaps every
// job running from its start onward. That needs no local clock, which could
// precede a runner that started while the records were being read.
function peakRunningJobs(jobs) {
  const edges = [];
  for (const job of jobs) {
    if (!job.hadRunner || !job.startedAt) continue;
    if (!job.completedAt && job.status !== "in_progress") continue;
    const start = Date.parse(job.startedAt);
    const end = job.completedAt ? Date.parse(job.completedAt) : Number.POSITIVE_INFINITY;
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
    checkRunUrl: job.check_run_url ?? null,
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
export function findVerdictViolations(
  summary,
  aggregateName = DEFAULT_AGGREGATE,
  obligations = [],
) {
  if (!summary.aggregate) {
    // Jobs appear as the run progresses, so an unfinished attempt without an
    // aggregate has not misreported anything yet.
    return summary.status === "completed" ? ["aggregate-missing"] : [];
  }
  // A finished run whose aggregate was skipped reports no verdict at all.
  if (summary.aggregate.conclusion === "skipped") return ["aggregate-skipped"];
  if (summary.aggregate.conclusion !== "success") return [];
  const others = summary.jobs.filter((job) => job.name !== aggregateName);
  const violations = [];
  // The aggregate accepts a skipped dependency, so a named obligation is judged
  // here: it must have run and succeeded for the pass to be warranted.
  for (const name of obligations) {
    const obligation = others.find((job) => job.name === name);
    if (!obligation) violations.push(`passing-without-obligation:${name}`);
    else if (obligation.conclusion !== "success") {
      violations.push(`passing-with-unmet-obligation:${name}`);
    }
  }
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

// What shows two runs share one concurrency group, strongest first. The API's
// pull request list can be empty, so the recorded cancel groups decide when they
// are known; a pull request pair with neither stays "unknown" instead of being
// claimed as shared. `null` means the records show they cannot share a group.
function sharedGroupEvidence(a, b) {
  if (a.pullRequests.length > 0 && b.pullRequests.length > 0) {
    return a.pullRequests.some((number) => b.pullRequests.includes(number)) ? "pull-request" : null;
  }
  if (a.cancelledByGroup && b.cancelledByGroup) {
    return a.cancelledByGroup === b.cancelledByGroup ? "cancel-group" : null;
  }
  // Pushes share the group of their ref; pull request runs share a merge ref.
  return a.event === "pull_request" ? "unknown" : "ref";
}

/**
 * Finds runs cancelled while an older run of the same ref re-ran. An attempt's
 * record carries that attempt's creation time, so run ids order the runs. The
 * API does not name the request that displaced a run, so each result is a
 * candidate supported by the attempt timing; `groupEvidence` says how the two
 * runs were shown to share a concurrency group.
 */
export function findOlderTreeDisplacements(summaries) {
  const displacements = [];
  for (const displaced of summaries) {
    if (displaced.conclusion !== "cancelled") continue;
    const windowStart = Date.parse(displaced.createdAt);
    const windowEnd = Date.parse(displaced.updatedAt);
    for (const rerun of summaries) {
      if (rerun.ref !== displaced.ref || rerun.event !== displaced.event) continue;
      const groupEvidence = sharedGroupEvidence(rerun, displaced);
      if (!groupEvidence) continue;
      if (rerun.attempt < 2 || rerun.runId >= displaced.runId) continue;
      // An older tree is the point: a re-run of the same head is not one.
      if (rerun.testedHead === displaced.testedHead) continue;
      const started = Date.parse(rerun.attemptStartedAt);
      if (started < windowStart || started > windowEnd) continue;
      displacements.push({
        displacedRunId: displaced.runId,
        groupEvidence,
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

/**
 * Classifies a cancelled job's annotations: the concurrency group that
 * displaced it, annotations this driver does not recognise (kept verbatim), or
 * no annotations at all.
 */
export function classifyCancelCause(annotations) {
  const messages = annotations.map((annotation) => annotation.message ?? "");
  for (const message of messages) {
    const match = CANCEL_CAUSE_RE.exec(message);
    if (match) return { kind: "group", group: match[1], messages };
  }
  return { kind: messages.length > 0 ? "unrecognised" : "none", group: null, messages };
}

/** Extracts the concurrency group named by a job's cancellation annotation. */
export function cancelCauseGroup(annotations) {
  return classifyCancelCause(annotations).group;
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

// The job record names its own check run. An Actions job id is not a check-run
// id, so a record without the URL is unreadable rather than guessed.
function annotationsPath(repo, job) {
  if (!job.checkRunUrl) {
    throw new ReadUnavailable(`job ${job.id} (${job.name}): record has no check_run_url`);
  }
  // Keep the path below the repo root.
  const { pathname } = new URL(job.checkRunUrl);
  const marker = `/repos/${repo}`;
  const at = pathname.indexOf(marker);
  const path = at === -1 ? pathname : pathname.slice(at + marker.length);
  return `${path}/annotations`;
}

/**
 * Reads the cancel cause of the first cancelled job, or of every cancelled job
 * when `all` is set so an eviction can be attributed per job.
 */
async function readCancelCauses({ read, repo, summary, all, reportMissing }) {
  const cancelled = summary.jobs.filter((candidate) => candidate.conclusion === "cancelled");
  // Keyed by Actions job id: display names repeat across a matrix or reused jobs.
  const causes = {};
  for (const job of all ? cancelled : cancelled.slice(0, 1)) {
    try {
      const cause = classifyCancelCause(await read(annotationsPath(repo, job)));
      causes[job.id] = { name: job.name, ...cause };
    } catch (error) {
      if (!(error instanceof ReadUnavailable)) throw error;
      reportMissing(error.message);
    }
  }
  return causes;
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
  allCancelCauses = false,
  obligations = [],
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
      if (annotations || allCancelCauses) {
        summary.cancelCauses = await readCancelCauses({
          read,
          repo,
          summary,
          all: allCancelCauses,
          reportMissing: (reason) => missing.push({ runId, attempt: attempt ?? null, reason }),
        });
        summary.cancelledByGroup =
          Object.values(summary.cancelCauses).find((cause) => cause.group)?.group ?? null;
      }
      summaries.push({
        ...summary,
        verdictViolations: findVerdictViolations(summary, aggregateName, obligations),
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
    allCancelCauses: false,
    obligations: [],
  };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    if (flag === "--annotations") {
      options.annotations = true;
      continue;
    }
    if (flag === "--annotations-all") {
      options.allCancelCauses = true;
      continue;
    }
    index += 1;
    const value = argv[index];
    if (value === undefined) throw new Error(`${flag} needs a value`);
    if (flag === "--repo") options.repo = value;
    else if (flag === "--aggregate") options.aggregateName = value;
    else if (flag === "--obligation") options.obligations.push(value);
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
