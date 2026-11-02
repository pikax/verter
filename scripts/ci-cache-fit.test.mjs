// Self-tests for scripts/ci-cache-fit.mjs. The service is a scripted fake and
// waiting is injected, so nothing here reaches the network or really waits.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  ALLOWANCE_READINGS,
  classifyKey,
  collect,
  createApi,
  evaluateFit,
  listCaches,
  mainReachOf,
  observe,
  scanWorkflow,
  summarize,
} from "./ci-cache-fit.mjs";

const HASH = "a".repeat(64);
const RUST_KEY = "v0-rust-rust-test-rust-test-build-Linux-x64-986f06eb-ea442a38";
const CONTRACT_KEY = `Linux-X64-compiler-contracts-v2-session-observe-${HASH}-${HASH}-${"b".repeat(40)}`;

function entry(id, key, bytes, { ref = "refs/heads/main", created = "2026-01-01T00:00:00Z" } = {}) {
  return { id, key, ref, size_in_bytes: bytes, created_at: created };
}

function repositoryRecord(entries, at = "2026-01-02T00:00:00Z") {
  return { at, scope: "repository", listing: { complete: true, entries } };
}

test("classifyKey names every consumer and keeps an unknown key countable", () => {
  const table = [
    [RUST_KEY, "rust-target", true, "rust-target:rust-test-rust-test-build@Linux-x64#986f06eb"],
    [CONTRACT_KEY, "compile-contracts", true, "compile-contracts:session-observe@Linux-X64"],
    [
      `Linux-perf-gate-cargo-${HASH}`,
      "explicit-cargo",
      true,
      "explicit-cargo:perf-gate-cargo@Linux",
    ],
    ["sccache/9/f/f/abc", "sccache", false, "sccache"],
    [`node-cache-Linux-x64-pnpm-${HASH}`, "node-package-manager", false, "node-package-manager"],
    ["vscode-host-Linux-X64-1.135.0-test-electron-3.1.0", "vscode-host", false, "vscode-host"],
    [`Linux-gradle-${HASH}`, "gradle", false, "gradle"],
    [`Linux-playwright-1.2-chromium`, "playwright", false, "playwright"],
    [`Linux-pnpm-${HASH}`, "pnpm-store", false, "pnpm-store"],
    [`something-new-${HASH}`, "unclassified", false, "unclassified:something-new-<hash>"],
  ];
  for (const [key, family, candidate, identity] of table) {
    assert.deepEqual(classifyKey(key), { family, candidate, identity }, key);
  }
});

test("scanWorkflow reads save-capable declarations and expands contract groups", () => {
  const rows = scanWorkflow(
    "ci.yml",
    `name: CI
on:
  push:
    branches: main
env:
  key: not-a-step
jobs:
  plain:
    steps:
      # - uses: Swatinem/rust-cache@v2
      - uses: Swatinem/rust-cache@v2
        with:
          save-if: \${{ github.ref == 'refs/heads/main' }}
          key: plain-key
  shared:
    steps:
      - uses: Swatinem/rust-cache@v2
        with:
          shared-key: napi-release
  disabled:
    steps:
      - uses: Swatinem/rust-cache@v2
        with:
          save-if: false
          key: off
  contracts:
    strategy:
      matrix:
        group: [core, session]
    steps:
      - uses: actions/cache/restore@v5
        id: compiler-contract-cache
        with:
          key: restore-only-stores-nothing
      - uses: actions/cache/save@v5
        if: github.ref == 'refs/heads/main' && steps.compiler-contract-cache.outputs.cache-hit != 'true'
        with:
          key: \${{ steps.compiler-contract-cache.outputs.cache-primary-key }}
  gradle:
    steps:
      - name: Save Gradle caches
        if: github.ref == 'refs/heads/main'
        uses: actions/cache/save@v5
        with:
          key: \${{ steps.gradle-cache.outputs.cache-primary-key }}
  bundled:
    steps:
      - uses: actions/cache@v5
        with:
          key: \${{ runner.os }}-perf-gate-cargo-\${{ hashFiles('**/Cargo.lock') }}
`,
  );
  assert.ok(rows.every(({ mainReach }) => mainReach === "automatic"));
  assert.deepEqual(
    rows.map(({ job, savePolicy, identity }) => [job, savePolicy, identity]),
    [
      ["plain", "main-only", "rust-target:plain-key-plain"],
      ["shared", "any-ref", "rust-target:napi-release"],
      ["disabled", "never", "rust-target:off-disabled"],
      ["contracts", "main-only", "compile-contracts:core"],
      ["contracts", "main-only", "compile-contracts:session"],
      ["gradle", "main-only", "preserved:gradle"],
      ["bundled", "any-ref", "explicit-cargo:perf-gate-cargo"],
    ],
  );
});

test("mainReachOf separates workflows that save from main on their own", () => {
  const table = [
    ["on:\n  push:\n    branches: main\n  pull_request:\njobs:\n", "automatic"],
    ["on:\n  push:\n    branches: [main]\njobs:\n", "automatic"],
    ["on:\n  push:\n    branches:\n      - main\n    paths:\n      - x\njobs:\n", "automatic"],
    ["on:\n  schedule:\n    - cron: '0 6 * * *'\n  workflow_dispatch:\njobs:\n", "automatic"],
    ["on:\n  workflow_dispatch:\n  issue_comment:\n    types: [created]\njobs:\n", "automatic"],
    [
      "on:\n  push:\n    tags:\n      - 'v*'\n  workflow_call:\n  workflow_dispatch:\njobs:\n",
      "manual",
    ],
    [
      "on:\n  push:\n    tags:\n      - 'main-v*'\n    paths:\n      - main\n  workflow_call:\njobs:\n",
      "none",
    ],
    ["on:\n  pull_request:\n    paths:\n      - main\njobs:\n", "none"],
    ["on:\n  push:\n  pull_request:\njobs:\n", "automatic"],
    ["on:\n  push:\n    paths: [a]\njobs:\n", "automatic"],
    ["on: [push, pull_request]\njobs:\n", "automatic"],
    ["on: push\njobs:\n", "automatic"],
    ["on:\n  push:\n    branches-ignore:\n      - main\njobs:\n", "none"],
    ["on:\n  push:\n    branches-ignore: [dev]\njobs:\n", "automatic"],
    ["on:\n  push:\n    branches: [dev]\njobs:\n", "none"],
    ["on: [pull_request, workflow_dispatch]\njobs:\n", "manual"],
  ];
  for (const [text, expected] of table) assert.equal(mainReachOf(text), expected, text);
});

test("createApi only reads, sends a token only when given and turns failures into data", async () => {
  const requests = [];
  const fetchImpl = async (url, init) => {
    requests.push({ url, method: init.method, authorization: init.headers.Authorization });
    return { ok: false, status: 401, json: async () => ({ message: "Requires authentication" }) };
  };
  const anonymous = await createApi({ repo: "o/r", fetchImpl })("actions/cache/usage");
  assert.deepEqual(anonymous, { ok: false, status: 401, message: "Requires authentication" });
  await createApi({ repo: "o/r", token: "secret", fetchImpl })("actions/cache/usage");
  assert.deepEqual(requests, [
    {
      url: "https://api.github.com/repos/o/r/actions/cache/usage",
      method: "GET",
      authorization: undefined,
    },
    {
      url: "https://api.github.com/repos/o/r/actions/cache/usage",
      method: "GET",
      authorization: "Bearer secret",
    },
  ]);

  const unreachable = createApi({
    repo: "o/r",
    fetchImpl: async () => {
      throw new Error("connection refused");
    },
  });
  assert.deepEqual(await unreachable("x"), { ok: false, status: 0, message: "connection refused" });
});

test("listCaches pages to the end and refuses a listing that moved or failed", async () => {
  const full = Array.from({ length: 100 }, (_, index) => entry(index, "k", 1));
  const pages = (bodies) => async (path) => {
    const page = Number(/[?&]page=(\d+)/.exec(path)[1]);
    return bodies[page - 1];
  };
  const ok = (total, items) => ({
    ok: true,
    status: 200,
    body: { total_count: total, actions_caches: items },
  });

  const whole = await listCaches(pages([ok(101, full), ok(101, [entry(100, "k", 1)])]));
  assert.equal(whole.complete, true);
  assert.equal(whole.entries.length, 101);

  const moved = await listCaches(pages([ok(101, full), ok(102, [entry(100, "k", 1)])]));
  assert.equal(moved.complete, false, "a total that changes between pages is not a whole listing");

  const shifted = await listCaches(pages([ok(101, full), ok(101, [full[99]])]));
  assert.equal(shifted.complete, false, "an entry repeated across pages hides another");

  const failed = await listCaches(
    pages([ok(101, full), { ok: false, status: 403, message: "rate limit" }]),
  );
  assert.deepEqual(
    [failed.complete, failed.error],
    [false, { page: 2, status: 403, message: "rate limit" }],
  );
});

test("observe records an unavailable endpoint by status, and main samples stay cheap", async () => {
  const paths = [];
  const get = async (path) => {
    paths.push(path);
    if (path === "actions/cache/usage") {
      return {
        ok: true,
        status: 200,
        body: { active_caches_size_in_bytes: 7, active_caches_count: 1 },
      };
    }
    if (path === "actions/cache/storage-limit")
      return { ok: false, status: 401, message: "Requires authentication" };
    return { ok: true, status: 200, body: { total_count: 0, actions_caches: [] } };
  };
  const now = () => new Date("2026-01-01T00:00:00Z");

  const full = await observe(get, { full: true, now });
  assert.deepEqual(full.limit, { status: 401, message: "Requires authentication" });
  assert.deepEqual(full.usage, { bytes: 7, count: 1 });

  paths.length = 0;
  const main = await observe(get, { full: false, now });
  assert.equal(main.scope, "main");
  assert.deepEqual(paths, [
    "actions/caches?per_page=100&page=1&sort=size_in_bytes&direction=desc&ref=refs%2Fheads%2Fmain",
  ]);
});

test("collect takes full observations on its cadence and waits only between samples", async () => {
  const waits = [];
  const scopes = [];
  await collect(
    async () => ({ ok: true, status: 200, body: { total_count: 0, actions_caches: [] } }),
    {
      samples: 4,
      intervalMs: 1000,
      fullEvery: 3,
      wait: async (ms) => waits.push(ms),
      emit: (record) => scopes.push(record.scope),
    },
  );
  assert.deepEqual(scopes, ["repository", "main", "main", "repository"]);
  assert.deepEqual(waits, [1000, 1000, 1000]);
});

test("evaluateFit certifies a fit only against an upper bound on preserved occupancy", () => {
  const small = { identity: "small", bytes: 10 };
  const large = { identity: "large", bytes: 30 };
  const fit = (allowanceBytes, candidates, unmeasured = [], upper = 60) =>
    evaluateFit({
      allowanceBytes,
      preservedLowerBytes: 60,
      preservedUpperBytes: upper,
      candidates,
      unmeasured,
    });

  const exact = fit(100, [large, small]);
  assert.deepEqual(
    [exact.outcome, exact.bounded.headroomBytes, exact.bounded.fitting],
    ["all-fit", 0, ["small", "large"]],
  );

  const short = fit(99, [large, small]);
  assert.deepEqual([short.outcome, short.bounded.fitting], ["unestablished", ["small"]]);
  assert.deepEqual(short.bounded.excluded, [{ identity: "large", bytes: 30, budgetLeftBytes: 29 }]);

  const unknown = fit(1000, [small], ["never-observed"]);
  assert.equal(unknown.outcome, "unestablished", "an unmeasured identity is not assumed to fit");

  const over = fit(50, [small]);
  assert.deepEqual(
    [over.outcome, over.bounded.budgetBytes, over.bounded.fitting],
    ["unestablished", -10, []],
  );

  // A lower bound on preserved bytes admits more than the sampled moments allow.
  const lowerOnly = fit(100, [large, small], [], 80);
  assert.equal(lowerOnly.provisional.fitting.length, 2, "the lower bound is only a ceiling");
  assert.deepEqual(
    [lowerOnly.outcome, lowerOnly.bounded.fitting],
    ["unestablished", ["small"]],
    "the upper bound admits only what held at the sampled moments",
  );

  const unbounded = fit(100, [large, small], [], null);
  assert.deepEqual([unbounded.outcome, unbounded.bounded], ["unestablished", null]);
});

test("summarize refuses to report occupancy without a complete repository listing", () => {
  const partial = {
    at: "2026-01-01T00:00:00Z",
    scope: "repository",
    usage: { status: 401, message: "Bad credentials" },
    listing: {
      complete: false,
      error: { page: 1, status: 401, message: "Bad credentials" },
      entries: [],
    },
  };
  const summary = summarize([partial], []);
  assert.equal(summary.available, false);
  assert.equal(summary.occupancy, undefined);
  assert.equal(summary.fit, undefined);
  assert.deepEqual(
    summary.failures.map(({ source, status }) => [source, status]),
    [
      ["listing", 401],
      ["usage", 401],
    ],
  );
});

test("summarize attributes newest saves, counts everything in occupancy and leaves gaps unmeasured", () => {
  const sccache = (id, bytes) => entry(id, `sccache/${id}`, bytes, { ref: "refs/pull/1/merge" });
  const older = entry(1, RUST_KEY.replace("ea442a38", "11111111"), 500, {
    created: "2026-01-01T00:00:00Z",
  });
  const newer = entry(2, RUST_KEY, 300, { created: "2026-01-02T00:00:00Z" });
  const otherEnvironment = entry(6, RUST_KEY.replace("986f06eb", "22222222"), 40);
  const transient = entry(3, `Linux-X64-compiler-contracts-v2-core-${HASH}-${HASH}`, 700);
  const unknown = entry(4, `something-new-${HASH}`, 11);
  const prTarget = entry(5, RUST_KEY.replace("rust-test-rust-test-build", "other-job"), 13, {
    ref: "refs/pull/1/merge",
  });
  const cutShort = entry(99, `Linux-X64-compiler-contracts-v2-session-${HASH}-${HASH}`, 5);

  const observations = [
    repositoryRecord(
      [older, newer, otherEnvironment, transient, sccache(10, 2000), unknown, prTarget],
      "2026-01-02T00:00:00Z",
    ),
    { at: "2026-01-02T01:00:00Z", scope: "main", listing: { complete: true, entries: [newer] } },
    {
      at: "2026-01-02T02:00:00Z",
      scope: "main",
      listing: { complete: false, reportedTotals: [1, 2], entries: [cutShort] },
    },
    {
      at: "2026-01-02T02:30:00Z",
      scope: "repository",
      usage: { bytes: 90_000, count: 3 },
      listing: {
        complete: false,
        error: { page: 2, status: 403, message: "rate limit" },
        entries: [sccache(12, 3000)],
      },
    },
    repositoryRecord([older, newer, sccache(11, 1000), unknown], "2026-01-02T03:00:00Z"),
  ];
  const declarations = [
    {
      savePolicy: "main-only",
      mainReach: "automatic",
      identity: "rust-target:rust-test-rust-test-build",
    },
    {
      savePolicy: "any-ref",
      mainReach: "automatic",
      identity: "rust-target:missing-${{ matrix.target }}-matrix",
    },
    { savePolicy: "main-only", mainReach: "automatic", identity: "compile-contracts:session" },
    { savePolicy: "main-only", mainReach: "automatic", identity: "compile-contracts:absent" },
    { savePolicy: "never", mainReach: "automatic", identity: "rust-target:off-off" },
    { savePolicy: "any-ref", mainReach: "none", identity: "rust-target:tag-only-release" },
    { savePolicy: "main-only", mainReach: "automatic", identity: "preserved:gradle" },
    { savePolicy: "any-ref", mainReach: "automatic", identity: "preserved:sccache" },
  ];
  const summary = summarize(observations, declarations);

  assert.deepEqual(
    summary.identities.map(({ identity, bytes, generations, residentAtEnd, samplesSeen }) => [
      identity,
      bytes,
      generations,
      residentAtEnd,
      samplesSeen,
    ]),
    [
      ["compile-contracts:session@Linux-X64", 5, 1, false, 1],
      ["rust-target:rust-test-rust-test-build@Linux-x64#22222222", 40, 1, false, 1],
      ["rust-target:rust-test-rust-test-build@Linux-x64#986f06eb", 300, 2, true, 3],
      ["compile-contracts:core@Linux-X64", 700, 1, false, 1],
    ],
    "an older generation never adds to its identity, another environment is another identity, and a listing cut short still records the sizes it read",
  );

  const { occupancy, window } = summary;
  assert.deepEqual(
    [window.completeRepositoryListings, window.repositoryListings, window.completeMainListings],
    [2, 3, 1],
  );
  assert.equal(occupancy.latest.at, "2026-01-02T03:00:00Z");
  assert.equal(occupancy.latest.listedBytes, 500 + 300 + 1000 + 11);
  assert.equal(occupancy.peakListedBytes, 500 + 300 + 40 + 700 + 2000 + 11 + 13);
  assert.equal(occupancy.peakUsageBytes, 90_000, "reported usage survives an incomplete listing");
  assert.equal(occupancy.latest.byFamily.unclassified.bytes, 11);
  assert.equal(occupancy.latest.byFamily.sccache.otherRefBytes, 1000);
  assert.equal(occupancy.latest.byFamily["rust-target"].oldestSave, "2026-01-01T00:00:00Z");
  assert.equal(occupancy.latest.byFamily["rust-target"].newestSave, "2026-01-02T00:00:00Z");
  assert.equal(
    occupancy.peakByFamily.sccache,
    3000,
    "a listing cut short is a lower bound, so it can raise a peak",
  );
  assert.equal(
    occupancy.minListedBytes,
    1811,
    "a listing cut short never lowers the listed occupancy",
  );
  assert.deepEqual(
    occupancy.series.map(({ at, complete, preservedBytes }) => [
      at.slice(11, 16),
      complete,
      preservedBytes,
    ]),
    [
      ["00:00", true, 2000 + 11 + 13],
      ["02:30", false, 3000],
      ["03:00", true, 1000 + 11],
    ],
    "preserved occupancy is taken one moment at a time and counts a candidate saved on another ref",
  );
  assert.equal(
    occupancy.headroomAtLatest["10 GB (decimal)"],
    ALLOWANCE_READINGS["10 GB (decimal)"] - 1811,
  );

  assert.deepEqual(summary.unmeasured, [
    "compile-contracts:absent",
    "rust-target:missing-${{ matrix.target }}-matrix",
  ]);
  assert.deepEqual(summary.unmeasuredPreserved, ["preserved:gradle"]);

  const fit = summary.fit["10 GB (decimal)"];
  assert.equal(
    fit.preservedLowerBytes,
    3000,
    "the peaks of different moments are never added together",
  );
  assert.equal(
    fit.preservedUpperBytes,
    3000 + (90_000 - 3000),
    "a listing cut short is bounded from above by the usage it did not attribute",
  );
  assert.equal(fit.outcome, "unestablished", "sampled occupancy never certifies a partial fit");
  const synthetic = [
    "compile-contracts:session@Linux-X64",
    "rust-target:rust-test-rust-test-build@Linux-x64#22222222",
    "rust-target:rust-test-rust-test-build@Linux-x64#986f06eb",
    "compile-contracts:core@Linux-X64",
  ];
  assert.deepEqual(fit.provisional.fitting, synthetic);
  assert.deepEqual(fit.bounded.fitting, synthetic);
  assert.deepEqual(
    occupancy.series.map(({ preservedUpperBytes }) => preservedUpperBytes),
    [2000 + 11 + 13, 3000 + 87_000, 1000 + 11],
    "a listing with no usage reading is exact when complete; one with usage adds the bytes it never attributed",
  );
  assert.deepEqual(
    occupancy.series.map(({ usageMatchesListing }) => usageMatchesListing),
    [null, false, null],
    "usage is compared with the listed bytes wherever both were read",
  );
});

test("a complete listing whose bytes fall short of the usage never certifies a fit", () => {
  // A swap between pages (one entry evicted, another saved) leaves the counts
  // consistent, so the listing reads complete while the usage figure holds bytes
  // the listing never attributed. Those bytes may be preserved.
  const preserved = entry(1, "sccache/0/0/0/a", 9_000_000_000);
  const candidate = entry(2, RUST_KEY, 900_000_000);
  const declarations = [
    {
      savePolicy: "main-only",
      mainReach: "automatic",
      identity: "rust-target:rust-test-rust-test-build",
    },
  ];
  const run = (usageBytes) => {
    const record = repositoryRecord([preserved, candidate]);
    if (usageBytes !== undefined) record.usage = { bytes: usageBytes, count: 2 };
    return summarize([record], declarations);
  };

  const mismatched = run(10_500_000_000);
  const fit = mismatched.fit["10 GB (decimal)"];
  assert.equal(fit.preservedUpperBytes, 9_000_000_000 + 600_000_000);
  assert.equal(fit.outcome, "unestablished");
  assert.equal(mismatched.occupancy.series[0].usageMatchesListing, false);

  const matched = run(9_900_000_000);
  assert.equal(matched.occupancy.series[0].usageMatchesListing, true);
  assert.equal(matched.fit["10 GB (decimal)"].outcome, "all-fit");
});

test("collect refuses sample options that would silently measure nothing", () => {
  const script = fileURLToPath(new URL("./ci-cache-fit.mjs", import.meta.url));
  for (const option of [
    ["--samples", "x"],
    ["--samples", "0"],
    ["--full-every", "0"],
    ["--interval-seconds=0"],
  ]) {
    const run = spawnSync(
      process.execPath,
      [script, "collect", "--out", "unused.jsonl", ...option],
      {
        encoding: "utf8",
      },
    );
    assert.equal(run.status, 1, option.join(" "));
    assert.match(run.stderr, /must be a positive integer/, option.join(" "));
  }
});
