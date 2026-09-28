#!/usr/bin/env node
/**
 * Reusing a pull request's green CI instead of repeating it.
 *
 * `main` requires every change to land through a pull request whose `CI
 * Required` passed, and each pull request run records the tree it tested (a
 * `ci-tested` annotation), so a squash whose tree is that tree was already
 * tested. Two consumers
 * rely on proving exactly that, from GitHub's own records, instead of testing
 * the same tree again:
 *
 * - `--landed`: CI on a push to main. A proven commit runs no lane (its
 *   `CI Required` still reports, so main keeps its green tick). Anything the
 *   proof cannot vouch for, a force-push or a direct push, runs CI as usual.
 * - the release: a tag publishes the artifacts its release pull request's CI
 *   rehearsal built (`release: v…` → release.yml, `release(ide): v…` →
 *   release-ide.yml), after proving that run passed for the tagged tree and
 *   still holds every artifact family it publishes. It fails closed.
 *
 * usage: node scripts/release-proof.mjs --landed --sha <commit>
 *        node scripts/release-proof.mjs --sha <tagged commit> --title-prefix "release: "
 *          --rehearsal "Release Check" --artifacts "native-*,wasm,vsix,…"
 * env:   GITHUB_REPOSITORY, GH_TOKEN (or GITHUB_TOKEN), GITHUB_API_URL, GITHUB_OUTPUT
 */
import { appendFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

/** The workflow whose pull request runs hold the tests (and a release's artifacts). */
export const CI_WORKFLOW = "ci.yml";

/**
 * Every page of a GitHub list endpoint whose body is `{ total_count, [key]: [...] }`.
 * @param {(path: string) => Promise<any>} api
 */
async function listAll(api, path, key) {
  const items = [];
  for (let page = 1; page <= 20; page += 1) {
    const joiner = path.includes("?") ? "&" : "?";
    const body = await api(`${path}${joiner}per_page=100&page=${page}`);
    const rows = Array.isArray(body?.[key]) ? body[key] : [];
    items.push(...rows);
    if (rows.length < 100 || items.length >= (body?.total_count ?? 0)) break;
  }
  return items;
}

/** An artifact family is an exact name, or a prefix ending in `*`. */
function matches(family, name) {
  return family.endsWith("*") ? name.startsWith(family.slice(0, -1)) : name === family;
}

/** The annotation a `ci.yml` pull request run's `detect-changes` job leaves. */
export const TESTED_ANNOTATION = "ci-tested";
const TESTED_JOB = "detect-changes";

/**
 * What a `ci.yml` run recorded it tested: `{ pull, tree }` from its
 * `detect-changes` job's `ci-tested` annotation, or null. A `pull_request` run
 * tests `refs/pull/N/merge` (the head merged into the base as it was then), not
 * the head, and GitHub empties a run's `pull_requests` once the pull request
 * merges, so the run itself records which pull request and which tree it tested.
 * @param {{ repo: string; runId: number; api: (path: string) => Promise<any> }} input
 */
export async function testedByRun({ repo, runId, api }) {
  const jobs = await listAll(api, `/repos/${repo}/actions/runs/${runId}/jobs`, "jobs");
  const job = jobs.find((row) => row?.name === TESTED_JOB);
  if (!job?.id) return null;
  const annotations = await api(`/repos/${repo}/check-runs/${job.id}/annotations?per_page=100`);
  for (const row of Array.isArray(annotations) ? annotations : []) {
    if (row?.title !== TESTED_ANNOTATION) continue;
    const found = /\bpull=(\d+)\s+tree=([0-9a-f]{40})\b/u.exec(String(row.message ?? ""));
    if (found) return { pull: Number(found[1]), tree: found[2] };
  }
  return null;
}

/**
 * The pull request `sha` merged, when its CI already tested `sha`'s tree:
 * `sha` is that pull request's merge commit (its squash), and a successful
 * `ci.yml` run for its head recorded that it ran for THIS pull request (not
 * another one sharing the head) and tested exactly `sha`'s tree. Its lanes were
 * selected from that pull request's own diff, the selection its green check
 * already vouched for. A run that recorded nothing (a fork's, or one from
 * before the record existed) proves nothing, so its merge runs CI as usual.
 * @param {{ repo: string; sha: string; api: (path: string) => Promise<any> }} input
 * @returns {Promise<{ pullRequest: number; title: string; head: string; runId: number }>}
 */
export async function landedPullProof({ repo, sha, api }) {
  if (!/^[0-9a-f]{40}$/u.test(sha))
    throw new Error(`release-proof: ${sha} is not a full commit sha`);
  const pulls = await api(`/repos/${repo}/commits/${sha}/pulls`);
  const pull = (Array.isArray(pulls) ? pulls : []).find(
    (row) => row?.merged_at && row.merge_commit_sha === sha,
  );
  if (!pull)
    throw new Error(`release-proof: ${sha.slice(0, 12)} is not the merge of a pull request`);
  const head = pull.head?.sha;
  const tree = (await api(`/repos/${repo}/git/commits/${sha}`))?.tree?.sha;
  if (!tree) throw new Error(`release-proof: ${sha.slice(0, 12)} has no tree`);
  const runs = (
    await listAll(
      api,
      `/repos/${repo}/actions/workflows/${CI_WORKFLOW}/runs?head_sha=${head}&event=pull_request&status=success`,
      "workflow_runs",
    )
  )
    .filter((row) => row?.conclusion === "success" && row.head_sha === head)
    .sort((a, b) => b.run_number - a.run_number);
  for (const run of runs) {
    const tested = await testedByRun({ repo, runId: run.id, api });
    if (tested?.pull === pull.number && tested.tree === tree)
      return { pullRequest: pull.number, title: String(pull.title ?? ""), head, runId: run.id };
  }
  throw new Error(
    `release-proof: no successful ${CI_WORKFLOW} run of pull request #${pull.number} (head ${String(head).slice(0, 12)}) recorded testing ${sha.slice(0, 12)}'s tree ${tree.slice(0, 12)}`,
  );
}

/**
 * A tag's release: the landed proof, for a pull request titled like the
 * release, whose run ran `rehearsal` and still holds every artifact family.
 * @param {{ repo: string; sha: string; titlePrefix: string; rehearsal: string; artifacts: string[];
 *   api: (path: string) => Promise<any> }} input
 * @returns {Promise<{ pullRequest: number; head: string; runId: number }>}
 */
export async function releaseProof({ repo, sha, titlePrefix, rehearsal, artifacts, api }) {
  const proof = await landedPullProof({ repo, sha, api }).catch((error) => {
    throw new Error(`${error.message}; a release lands through its release pull request`);
  });
  if (!proof.title.startsWith(titlePrefix))
    throw new Error(
      `release-proof: pull request #${proof.pullRequest} is "${proof.title}", not a "${titlePrefix}…" release pull request`,
    );
  const jobs = await listAll(api, `/repos/${repo}/actions/runs/${proof.runId}/jobs`, "jobs");
  const validated = jobs.find((job) => job?.name === `${rehearsal} / Validate Tag`);
  if (validated?.conclusion !== "success")
    throw new Error(
      `release-proof: CI run ${proof.runId} did not run the "${rehearsal}" rehearsal (its "${rehearsal} / Validate Tag" job is ${validated?.conclusion ?? "absent"})`,
    );
  const stored = (
    await listAll(api, `/repos/${repo}/actions/runs/${proof.runId}/artifacts`, "artifacts")
  ).filter((row) => row && !row.expired);
  const missing = artifacts.filter((family) => !stored.some((row) => matches(family, row.name)));
  if (missing.length)
    throw new Error(
      `release-proof: CI run ${proof.runId} holds no unexpired ${missing.join(", ")}; run the release pull request's CI again before tagging`,
    );
  return { pullRequest: proof.pullRequest, head: proof.head, runId: proof.runId };
}

function argValue(argv, flag) {
  const index = argv.indexOf(flag);
  return index >= 0 ? argv[index + 1] : undefined;
}

/** A GitHub REST reader authenticated with a token. */
export function tokenApi(token, base = "https://api.github.com") {
  return async (path) => {
    const response = await fetch(`${base}${path}`, {
      headers: {
        authorization: `Bearer ${token}`,
        accept: "application/vnd.github+json",
        "x-github-api-version": "2022-11-28",
      },
    });
    if (!response.ok) throw new Error(`release-proof: GET ${path} answered ${response.status}`);
    return response.json();
  };
}

export async function main(argv = process.argv.slice(2), env = process.env) {
  const sha = argValue(argv, "--sha"),
    landedOnly = argv.includes("--landed");
  const repo = env.GITHUB_REPOSITORY,
    token = env.GH_TOKEN || env.GITHUB_TOKEN;
  const out = (lines) => {
    if (env.GITHUB_OUTPUT) appendFileSync(env.GITHUB_OUTPUT, `${lines.join("\n")}\n`);
  };
  if (!sha || !repo || !token) {
    process.stderr.write(
      "usage: release-proof.mjs [--landed] --sha <sha> … (GITHUB_REPOSITORY and GH_TOKEN set)\n",
    );
    return 2;
  }
  const api = tokenApi(token, env.GITHUB_API_URL || undefined);
  if (landedOnly) {
    // Never fails CI: without a proof, the lanes simply run.
    try {
      const proof = await landedPullProof({ repo, sha, api });
      out([
        "landed=true",
        `pull-request=${proof.pullRequest}`,
        `head=${proof.head}`,
        `run=${proof.runId}`,
      ]);
      process.stdout.write(
        `Landed from pull request #${proof.pullRequest}: CI run ${proof.runId} passed for this tree (head ${proof.head.slice(0, 12)}); no lane runs again\n`,
      );
    } catch (error) {
      out(["landed=false"]);
      process.stdout.write(
        `::notice::${error instanceof Error ? error.message : String(error)}; running CI\n`,
      );
    }
    return 0;
  }
  const titlePrefix = argValue(argv, "--title-prefix"),
    rehearsal = argValue(argv, "--rehearsal");
  const artifacts = (argValue(argv, "--artifacts") ?? "")
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);
  if (!titlePrefix || !rehearsal || !artifacts.length) {
    process.stderr.write(
      "usage: release-proof.mjs --sha <sha> --title-prefix <prefix> --rehearsal <job prefix> --artifacts <a,b*>\n",
    );
    return 2;
  }
  try {
    const proof = await releaseProof({ repo, sha, titlePrefix, rehearsal, artifacts, api });
    out([
      `pull-request=${proof.pullRequest}`,
      `head=${proof.head}`,
      `artifacts-run=${proof.runId}`,
    ]);
    process.stdout.write(
      `Release proven by pull request #${proof.pullRequest} (head ${proof.head.slice(0, 12)}); publishing the artifacts of CI run ${proof.runId}\n`,
    );
    return 0;
  } catch (error) {
    process.stdout.write(`::error::${error instanceof Error ? error.message : String(error)}\n`);
    return 1;
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url)
  process.exitCode = await main();
