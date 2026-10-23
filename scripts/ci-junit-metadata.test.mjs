#!/usr/bin/env node

// Tests for ci-junit-metadata.mjs. Run: node --test scripts/ci-junit-metadata.test.mjs

import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { buildMetadata, checkMetadata, junitTestIds } from "./ci-junit-metadata.mjs";

const JUNIT = `<?xml version="1.0"?><testsuites><testsuite name="a">
<testcase name="m::one" classname="a" time="0.1"/>
<testcase name="m::two &amp; three" classname="a" time="0.1"><failure/></testcase>
</testsuite></testsuites>`;

function build(env = {}) {
  const dir = mkdtempSync(join(tmpdir(), "junit-meta-"));
  try {
    const junit = join(dir, "junit.xml");
    writeFileSync(junit, JUNIT);
    const meta = buildMetadata(
      { junit, command: "cargo nextest run", profile: "ci", tools: "node", filter: "all()" },
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
  for (const mutate of [
    (m) => delete m.checkout.tree,
    (m) => (m.checkout.parents = []),
    (m) => delete m.tools["cargo-nextest"],
    (m) => delete m.environment.variables.RUSTFLAGS,
    (m) => (m.runConfiguration.partition = ""),
    (m) => (m.runConfiguration.nextestProfile = "default"),
  ]) {
    const meta = build();
    mutate(meta);
    assert.notDeepEqual(checkMetadata(meta, JUNIT), []);
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
