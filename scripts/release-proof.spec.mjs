import assert from "node:assert/strict";
import test from "node:test";
import { landedPullProof, releaseProof } from "./release-proof.mjs";

const SHA = "a".repeat(40),
  HEAD = "b".repeat(40),
  TREE = "c".repeat(40),
  OTHER = "d".repeat(40);
const REHEARSAL = "Release Check";
const ARTIFACTS = ["native-*", "lsp-*", "wasm", "vsix"];

/** A successful ci.yml run for the head; `tested` is what its detect-changes job recorded. */
const ran = (id, tested = { pull: 705, tree: TREE }) => ({
  id,
  run_number: id - 10,
  conclusion: "success",
  head_sha: HEAD,
  pull_requests: [],
  tested,
});
const annotate = (tested) =>
  tested
    ? [
        { title: "", message: "The ubuntu-latest label will migrate", annotation_level: "notice" },
        {
          title: "ci-tested",
          message: `pull=${tested.pull} tree=${tested.tree}`,
          annotation_level: "notice",
        },
      ]
    : [];

/** A fake GitHub API for one merged release pull request; `patch` edits a response before it is served. */
function github(patch = {}) {
  const number = patch.pulls?.[0]?.number ?? 705;
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
    runs: [ran(11, { pull: number, tree: TREE }), ran(12, { pull: number, tree: TREE })],
    rehearsal: [
      { name: "Release Check / Validate Tag", conclusion: "success" },
      { name: "CI Required", conclusion: "success" },
    ],
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
    if (
      path.startsWith(
        `/repos/o/r/actions/workflows/ci.yml/runs?head_sha=${HEAD}&event=pull_request&status=success`,
      )
    )
      return { total_count: body.runs.length, workflow_runs: body.runs };
    const jobs = path.match(/^\/repos\/o\/r\/actions\/runs\/(\d+)\/jobs/u);
    if (jobs) {
      const rows = [{ id: Number(jobs[1]) * 100, name: "detect-changes", conclusion: "success" }];
      if (jobs[1] === "12") rows.push(...body.rehearsal);
      return { total_count: rows.length, jobs: rows };
    }
    const checks = path.match(/^\/repos\/o\/r\/check-runs\/(\d+)00\/annotations/u);
    if (checks) return annotate(body.runs.find((run) => run.id === Number(checks[1]))?.tested);
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
const land = (patch) => landedPullProof({ repo: "o/r", sha: SHA, api: github(patch).api });

test("proves a tag is its release pull request's tested tree and names the newest successful run to reuse", async () => {
  assert.deepEqual(await prove(), { pullRequest: 705, head: HEAD, runId: 12 });
});

test("refuses a tag that is not the squash of a merged release pull request", async () => {
  await assert.rejects(
    prove({ pulls: [] }),
    /is not the merge of a pull request; a release lands through its release pull request/u,
  );
  const pulled = (row) => ({
    pulls: [{ number: 9, merged_at: "x", merge_commit_sha: SHA, head: { sha: HEAD }, ...row }],
  });
  await assert.rejects(
    prove(pulled({ title: "release: v1", merged_at: null })),
    /not the merge of a pull request/u,
  );
  await assert.rejects(
    prove(pulled({ title: "fix: something" })),
    /not a "release: …" release pull request/u,
  );
  // An editors release is not a project release, and the other way round.
  await assert.rejects(prove(pulled({ title: "release(ide): v0.0.4" })), /not a "release: …"/u);
});

test("refuses a tagged tree that is not the tree the pull request's CI tested", async () => {
  await assert.rejects(
    prove({ tagged: { tree: { sha: OTHER } } }),
    /no successful ci\.yml run of pull request #705 \(head bbbbbbbbbbbb\) recorded testing aaaaaaaaaaaa's tree dddddddddddd/u,
  );
});

test("refuses without a successful CI run for the head, or when that run skipped the rehearsal", async () => {
  await assert.rejects(prove({ runs: [] }), /no successful ci\.yml run/u);
  await assert.rejects(
    prove({ runs: [{ ...ran(12), head_sha: "e".repeat(40) }] }),
    /no successful/u,
  );
  await assert.rejects(
    prove({ rehearsal: [{ name: "CI Required", conclusion: "success" }] }),
    /did not run the "Release Check" rehearsal \(its "Release Check \/ Validate Tag" job is absent\)/u,
  );
  await assert.rejects(
    prove({ rehearsal: [{ name: "Release Check / Validate Tag", conclusion: "skipped" }] }),
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
// a pull request's merge.
test("proves a pull request's squash landed the tree its CI tested, and nothing else", async () => {
  assert.deepEqual(await land(), {
    pullRequest: 705,
    title: "release: v0.0.1-beta.6",
    head: HEAD,
    runId: 12,
  });
  // Any pull request qualifies here; only the release proof asks for a release title.
  const ordinary = {
    pulls: [
      {
        number: 42,
        title: "fix: something",
        merged_at: "x",
        merge_commit_sha: SHA,
        head: { sha: HEAD },
      },
    ],
  };
  assert.equal((await land(ordinary)).pullRequest, 42);
  // A direct push: no merged pull request names this commit.
  await assert.rejects(land({ pulls: [] }), /not the merge of a pull request/u);
  // A force-push that kept a merged pull request's commit message but not its tree.
  await assert.rejects(land({ tagged: { tree: { sha: OTHER } } }), /recorded testing/u);
  // A pull request merged while its CI had not passed (an admin override).
  await assert.rejects(land({ runs: [] }), /no successful ci\.yml run/u);
});

// A pull_request run tests the head merged into ITS pull request's base; another
// pull request sharing the head (the same branch opened against another base)
// tested another tree, with lanes picked from another diff.
test("reuses only a run that recorded testing this pull request's landed tree", async () => {
  // The newest run belongs to another pull request: the older run of #705 is the proof.
  assert.equal((await land({ runs: [ran(11), ran(13, { pull: 900, tree: TREE })] })).runId, 11);
  // Only another pull request's run.
  await assert.rejects(
    land({ runs: [ran(13, { pull: 900, tree: TREE })] }),
    /of pull request #705/u,
  );
  // A run of this pull request that tested another tree: its base has since moved.
  await assert.rejects(land({ runs: [ran(12, { pull: 705, tree: OTHER })] }), /recorded testing/u);
  // A run that recorded nothing: a fork's, or one from before the record existed.
  await assert.rejects(land({ runs: [ran(12, null)] }), /recorded testing/u);
});
