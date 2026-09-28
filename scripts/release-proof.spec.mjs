import assert from "node:assert/strict";
import test from "node:test";
import { landedPullProof, releaseProof } from "./release-proof.mjs";

const SHA = "a".repeat(40),
  HEAD = "b".repeat(40),
  TREE = "c".repeat(40);
const REHEARSAL = "Release Check";
const ARTIFACTS = ["native-*", "lsp-*", "wasm", "vsix"];

/** A fake GitHub API for one merged release pull request; `patch` edits a response before it is served. */
function github(patch = {}) {
  const body = {
    pulls: [
      {
        number: 705,
        title: "release: v0.0.1-beta.6",
        merged_at: "2026-09-28T20:00:00Z",
        merge_commit_sha: SHA,
        head: { sha: HEAD },
      },
    ],
    tagged: { tree: { sha: TREE } },
    tested: { tree: { sha: TREE } },
    runs: {
      total_count: 2,
      workflow_runs: [
        { id: 11, run_number: 1, conclusion: "success", head_sha: HEAD },
        { id: 12, run_number: 2, conclusion: "success", head_sha: HEAD },
      ],
    },
    jobs: {
      total_count: 2,
      jobs: [
        { name: "Release Check / Validate Tag", conclusion: "success" },
        { name: "CI Required", conclusion: "success" },
      ],
    },
    artifacts: {
      total_count: 5,
      artifacts: [
        { name: "native-x86_64-unknown-linux-gnu", expired: false },
        { name: "lsp-linux-x64-gnu", expired: false },
        { name: "wasm", expired: false },
        { name: "vsix", expired: false },
        { name: "ci-native-node", expired: false },
      ],
    },
    ...patch,
  };
  const seen = [];
  const api = async (path) => {
    seen.push(path);
    if (path === `/repos/o/r/commits/${SHA}/pulls`) return body.pulls;
    if (path === `/repos/o/r/git/commits/${SHA}`) return body.tagged;
    if (path === `/repos/o/r/git/commits/${HEAD}`) return body.tested;
    if (
      path.startsWith(
        `/repos/o/r/actions/workflows/ci.yml/runs?head_sha=${HEAD}&event=pull_request&status=success`,
      )
    )
      return body.runs;
    if (path.startsWith("/repos/o/r/actions/runs/12/jobs")) return body.jobs;
    if (path.startsWith("/repos/o/r/actions/runs/12/artifacts")) return body.artifacts;
    throw new Error(`unexpected ${path}`);
  };
  return { api, seen };
}
const prove = (patch, extra = {}) =>
  releaseProof({
    repo: "o/r",
    sha: SHA,
    titlePrefix: "release: ",
    rehearsal: REHEARSAL,
    artifacts: ARTIFACTS,
    api: github(patch).api,
    ...extra,
  });

test("proves a tag is its release pull request's tested tree and names the newest successful run to reuse", async () => {
  assert.deepEqual(await prove(), { pullRequest: 705, head: HEAD, runId: 12 });
});

test("refuses a tag that is not the squash of a merged release pull request", async () => {
  await assert.rejects(
    prove({ pulls: [] }),
    /is not the merge of a pull request; a release lands through its release pull request/u,
  );
  await assert.rejects(
    prove({
      pulls: [
        {
          number: 9,
          title: "release: v1",
          merged_at: null,
          merge_commit_sha: SHA,
          head: { sha: HEAD },
        },
      ],
    }),
    /not the merge of a pull request/u,
  );
  await assert.rejects(
    prove({
      pulls: [
        {
          number: 9,
          title: "fix: something",
          merged_at: "x",
          merge_commit_sha: SHA,
          head: { sha: HEAD },
        },
      ],
    }),
    /not a "release: …" release pull request/u,
  );
  // An editors release is not a project release, and the other way round.
  await assert.rejects(
    prove({
      pulls: [
        {
          number: 9,
          title: "release(ide): v0.0.4",
          merged_at: "x",
          merge_commit_sha: SHA,
          head: { sha: HEAD },
        },
      ],
    }),
    /not a "release: …"/u,
  );
});

test("refuses a tagged tree that is not the tree the pull request's CI tested", async () => {
  await assert.rejects(
    prove({ tested: { tree: { sha: "d".repeat(40) } } }),
    /is not pull request #705's tested head tree/u,
  );
});

test("refuses without a successful CI run for the head, or when that run skipped the rehearsal", async () => {
  await assert.rejects(
    prove({ runs: { total_count: 0, workflow_runs: [] } }),
    /no successful ci\.yml run/u,
  );
  await assert.rejects(
    prove({
      runs: {
        total_count: 1,
        workflow_runs: [{ id: 12, run_number: 2, conclusion: "success", head_sha: "e".repeat(40) }],
      },
    }),
    /no successful/u,
  );
  await assert.rejects(
    prove({ jobs: { total_count: 1, jobs: [{ name: "CI Required", conclusion: "success" }] } }),
    /did not run the "Release Check" rehearsal \(its "Release Check \/ Validate Tag" job is absent\)/u,
  );
  await assert.rejects(
    prove({
      jobs: {
        total_count: 1,
        jobs: [{ name: "Release Check / Validate Tag", conclusion: "skipped" }],
      },
    }),
    /job is skipped/u,
  );
});

test("refuses when an artifact family the release publishes is missing or expired, never matching CI's own artifacts", async () => {
  const without = (name) => ({
    artifacts: {
      total_count: 4,
      artifacts: [
        { name: "native-x86_64-unknown-linux-gnu", expired: false },
        { name: "lsp-linux-x64-gnu", expired: false },
        { name: "wasm", expired: false },
        { name: "vsix", expired: false },
      ].filter((row) => row.name !== name),
    },
  });
  await assert.rejects(prove(without("vsix")), /holds no unexpired vsix/u);
  await assert.rejects(
    prove({
      artifacts: {
        total_count: 2,
        artifacts: [
          { name: "ci-native-node", expired: false },
          { name: "native-x86_64-unknown-linux-gnu", expired: true },
        ],
      },
    }),
    /holds no unexpired native-\*, lsp-\*, wasm, vsix/u,
  );
});

test("refuses a sha that is not a full commit id without asking GitHub", async () => {
  const { api, seen } = github();
  await assert.rejects(
    releaseProof({
      repo: "o/r",
      sha: "abc",
      titlePrefix: "release: ",
      rehearsal: REHEARSAL,
      artifacts: ARTIFACTS,
      api,
    }),
    /not a full commit sha/u,
  );
  assert.deepEqual(seen, []);
});

// CI on a push to main runs no lane for a commit the proof vouches for, and runs
// as usual for anything else: a direct push or a force-push was never tested as
// a pull request head.
test("proves a pull request's merge landed the tree its CI tested, and nothing else", async () => {
  assert.deepEqual(await landedPullProof({ repo: "o/r", sha: SHA, api: github().api }), {
    pullRequest: 705,
    title: "release: v0.0.1-beta.6",
    head: HEAD,
    runId: 12,
  });
  // Any pull request qualifies here; only the release proof asks for a release title.
  const ordinary = github({
    pulls: [
      {
        number: 42,
        title: "fix: something",
        merged_at: "x",
        merge_commit_sha: SHA,
        head: { sha: HEAD },
      },
    ],
  });
  assert.equal(
    (await landedPullProof({ repo: "o/r", sha: SHA, api: ordinary.api })).pullRequest,
    42,
  );
  // A direct push: no merged pull request names this commit.
  await assert.rejects(
    landedPullProof({ repo: "o/r", sha: SHA, api: github({ pulls: [] }).api }),
    /not the merge of a pull request/u,
  );
  // A force-push that kept a merged pull request's commit message but not its tree.
  await assert.rejects(
    landedPullProof({
      repo: "o/r",
      sha: SHA,
      api: github({ tagged: { tree: { sha: "f".repeat(40) } } }).api,
    }),
    /is not pull request #705's tested head tree/u,
  );
  // A pull request merged while its CI had not passed (an admin override).
  await assert.rejects(
    landedPullProof({
      repo: "o/r",
      sha: SHA,
      api: github({ runs: { total_count: 0, workflow_runs: [] } }).api,
    }),
    /no successful ci\.yml run/u,
  );
});
