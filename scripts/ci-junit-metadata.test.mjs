#!/usr/bin/env node

// Tests for ci-junit-metadata.mjs. Run: node --test scripts/ci-junit-metadata.test.mjs

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  allowListedVariables,
  buildMetadata,
  checkMetadata,
  describeArgv,
  junitTestIds,
  readCheckout,
} from "./ci-junit-metadata.mjs";

const JUNIT = `<?xml version="1.0"?><testsuites><testsuite name="a">
<testcase name="m::one" classname="a" time="0.1"/>
<testcase name="m::two &amp; three" classname="a" time="0.1"><failure/></testcase>
</testsuite></testsuites>`;

const STEP_ENV = {
  CI: "true",
  NEXTEST_PROFILE: "ci",
  RUSTFLAGS: "-C instrument-coverage",
  CARGO_LLVM_COV: "1",
};

/** Writes the snapshot the way the nextest step does, from that step's own environment. */
function snapshotFile(dir, stepEnv = STEP_ENV) {
  const path = join(dir, "env.json");
  writeFileSync(path, JSON.stringify({ variables: allowListedVariables(stepEnv) }));
  return path;
}

function build(env = {}, extra = {}) {
  const dir = mkdtempSync(join(tmpdir(), "junit-meta-"));
  try {
    const junit = join(dir, "junit.xml");
    writeFileSync(junit, JUNIT);
    const meta = buildMetadata(
      {
        junit,
        profile: "ci",
        "env-snapshot": snapshotFile(dir),
        "argv-json": JSON.stringify([
          "nextest",
          "run",
          "--partition",
          "hash:1/4",
          "--features",
          "a/x,b/x",
          "-E",
          "all()",
        ]),
        tools: "node",
        ...extra,
      },
      {
        GITHUB_EVENT_NAME: "push",
        RUNNER_OS: "Linux",
        GITHUB_JOB: "j",
        GITHUB_WORKFLOW: "CI",
        GITHUB_WORKFLOW_REF: "o/r/.github/workflows/ci.yml@refs/heads/main",
        ...env,
      },
    );
    // The CI tool versions come from the runner; the build here only reads `node`.
    meta.tools = {
      ...meta.tools,
      rustc: "rustc 1",
      cargo: "cargo 1",
      "cargo-nextest": "nextest 1",
      pnpm: "10",
    };
    return meta;
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("testcases are read with XML entities decoded", () => {
  assert.deepEqual(junitTestIds(JUNIT), ["a m::one", "a m::two & three"]);
});

test("a complete document passes and every test is non-hermetic without a listing", () => {
  const meta = build();
  assert.deepEqual(checkMetadata(meta, JUNIT), []);
  assert.equal(meta.runConfiguration.workflowPath, ".github/workflows/ci.yml");
  assert.deepEqual(
    meta.tests.map((t) => t.hermetic),
    [false, false],
  );
  assert.match(meta.checkout.tree, /^[0-9a-f]{40,64}$/);
});

test("each missing required field is reported", () => {
  const valid = build();
  const mutations = [
    (m) => delete m.checkout.tree,
    (m) => (m.checkout.parents = []),
    (m) => delete m.tools["cargo-nextest"],
    (m) => delete m.tools.node,
    (m) => delete m.tools.pnpm,
    (m) => delete m.environment.variables.RUSTFLAGS,
    (m) => (m.runConfiguration.partition = ""),
    (m) => (m.runConfiguration.nextestProfile = "default"),
    (m) => (m.environment.variables.NEXTEST_PROFILE = null),
    (m) => (m.runConfiguration.testFilter = "none"),
  ];
  for (const mutate of mutations) {
    const meta = structuredClone(valid);
    mutate(meta);
    assert.notDeepEqual(checkMetadata(meta, JUNIT), [], String(mutate));
  }
});

test("the environment is the nextest step's snapshot, not the writer's own", () => {
  const meta = build({ NEXTEST_PROFILE: undefined, RUSTFLAGS: undefined });
  assert.equal(meta.environment.variables.NEXTEST_PROFILE, "ci");
  assert.equal(meta.environment.variables.RUSTFLAGS, "-C instrument-coverage");
  assert.deepEqual(checkMetadata(meta, JUNIT), []);
});

test("command, features, filter and partition are read from the run's argument vector", () => {
  const meta = build();
  assert.equal(
    meta.runConfiguration.command,
    "cargo nextest run --partition hash:1/4 --features a/x,b/x -E all()",
  );
  assert.equal(meta.runConfiguration.cargoFeatures, "a/x,b/x");
  assert.equal(meta.runConfiguration.testFilter, "all()");
  assert.equal(meta.runConfiguration.partition, "hash:1/4");
  assert.throws(() => describeArgv([]), /non-empty/);
});

test("a shallow checkout of a merge commit reports its tree and both parents", () => {
  const dir = mkdtempSync(join(tmpdir(), "junit-meta-git-"));
  try {
    const git = (cwd, ...args) =>
      execFileSync(
        "git",
        [
          "-c",
          "user.name=t",
          "-c",
          "user.email=t@example.com",
          "-c",
          "commit.gpgsign=false",
          ...args,
        ],
        { cwd, encoding: "utf8" },
      ).trim();
    const origin = join(dir, "origin");
    git(dir, "init", "-q", "-b", "main", origin);
    git(origin, "commit", "-q", "--allow-empty", "-m", "base");
    git(origin, "checkout", "-q", "-b", "side");
    git(origin, "commit", "-q", "--allow-empty", "-m", "side");
    git(origin, "checkout", "-q", "main");
    git(origin, "commit", "-q", "--allow-empty", "-m", "main");
    git(origin, "merge", "-q", "--no-ff", "-m", "merge", "side");
    const parents = git(origin, "rev-list", "--parents", "-n", "1", "HEAD").split(" ").slice(1);
    assert.equal(parents.length, 2);
    const clone = join(dir, "clone");
    git(dir, "clone", "-q", "--depth", "1", `file://${origin}`, clone);
    const checkout = readCheckout(clone);
    assert.equal(checkout.commit, git(origin, "rev-parse", "HEAD"));
    assert.equal(checkout.tree, git(origin, "rev-parse", "HEAD^{tree}"));
    assert.deepEqual(checkout.parents, parents);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("a pull_request checkout must carry two parents", () => {
  const meta = build({ GITHUB_EVENT_NAME: "pull_request" });
  meta.checkout.parents = ["a".repeat(40)];
  assert.match(checkMetadata(meta, JUNIT).join("\n"), /exactly two/);
  meta.checkout.parents.push("b".repeat(40));
  assert.deepEqual(checkMetadata(meta, JUNIT), []);
});

test("a hermetic flag not on the approved list, or a test list that drifts from the junit.xml, fails", () => {
  const flagged = build();
  flagged.tests[0].hermetic = true;
  assert.match(checkMetadata(flagged, JUNIT).join("\n"), /hermetic list/);
  const drifted = build();
  drifted.tests.pop();
  assert.match(checkMetadata(drifted, JUNIT).join("\n"), /do not match/);
});
