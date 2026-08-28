import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  DoctorRequiredError,
  FakeGitHubAdapter,
  GitHubAdapter,
  GitHubDoctor,
  LiveGitHubForbiddenInTestsError,
  MutationModeRequiredError,
  ProtectedMappingError,
  parseGitHubResourceNumber,
} from "../index.mjs";
import { createGhApiTransport } from "../adapter.mjs";
import * as publicApi from "../index.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const CLI = path.join(HERE, "../githubctl.mjs");
const TOOLS = path.join(REPO_ROOT, "roadmap/0.1.0-tama/tools");

function read(file) {
  return fs.readFileSync(file, "utf8");
}

function productionSources() {
  return fs
    .readdirSync(path.join(HERE, ".."))
    .filter((name) => name.endsWith(".mjs"))
    .map((name) => ({
      name,
      text: read(path.join(HERE, "..", name)),
    }));
}

function clearanceFor(adapter) {
  const report = new GitHubDoctor(adapter).check();
  assert.equal(report.ok, true, report.errors?.join?.("; ") ?? "doctor failed");
  return report.clearance;
}

test("programctl stays local and GitHub-blind", () => {
  for (const name of ["programctl.mjs", "lib.mjs"]) {
    const text = read(path.join(TOOLS, name));
    assert.doesNotMatch(text, /\bgh\s+api\b/u);
    assert.doesNotMatch(text, /\bgh\s+issue\b/u);
    assert.doesNotMatch(text, /\bgh\s+pr\b/u);
    assert.doesNotMatch(text, /\bfetch\s*\(/u);
    assert.doesNotMatch(text, /https:\/\/api\.github/u);
    assert.doesNotMatch(text, /githubctl/u);
  }
});

test("constructing the live adapter in tests without a fake fails closed", () => {
  assert.equal(Boolean(process.env.NODE_TEST_CONTEXT), true);
  assert.throws(
    () => new GitHubAdapter({ owner: "pikax", repo: "verter" }),
    LiveGitHubForbiddenInTestsError,
  );
  assert.equal(Object.hasOwn(publicApi, "createGhApiTransport"), false);
  assert.throws(() => createGhApiTransport(), LiveGitHubForbiddenInTestsError);
  assert.throws(
    () =>
      new GitHubAdapter({
        owner: "pikax",
        repo: "verter",
        transport: createGhApiTransport(),
      }),
    LiveGitHubForbiddenInTestsError,
  );
  const live = spawnSync(
    process.execPath,
    [CLI, "doctor", "--owner", "pikax", "--repo", "verter"],
    {
      encoding: "utf8",
      env: process.env,
    },
  );
  assert.notEqual(live.status, 0);
  assert.match(live.stderr, /not a test substrate/u);
  assert.doesNotMatch(live.stderr, /gh: /u);
});

test("resource identity is the JSON number field, never a URL tail", () => {
  assert.equal(
    parseGitHubResourceNumber({ number: 12, url: "https://github.com/o/r/issues/99" }),
    12,
  );
  assert.throws(() => parseGitHubResourceNumber("https://github.com/o/r/issues/12"));
  assert.throws(() => parseGitHubResourceNumber({ html_url: "https://github.com/o/r/issues/12" }));
  assert.throws(() => parseGitHubResourceNumber({ number: "12" }));
  for (const source of productionSources()) {
    assert.doesNotMatch(source.text, /html_url/u, source.name);
    assert.doesNotMatch(source.text, /\/issues\/\\d/u, source.name);
    assert.doesNotMatch(source.text, /split\(\s*["']\/["']\s*\)\.pop/u, source.name);
    assert.doesNotMatch(source.text, /new URL\([^)]*\)\.pathname/u, source.name);
  }
});

test("production sources do not persist or log credentials", () => {
  for (const source of productionSources()) {
    assert.doesNotMatch(source.text, /writeFile(?:Sync)?\([^;]*token/iu, source.name);
    assert.doesNotMatch(source.text, /GH_TOKEN\s*=/u, source.name);
    assert.doesNotMatch(source.text, /console\.\w+\([^;]*GH_TOKEN/u, source.name);
  }
});

test("a protected mapping cannot update an issue", () => {
  const adapter = new FakeGitHubAdapter({
    owner: "pikax",
    repo: "verter",
    issues: [
      {
        number: 7,
        title: "kept title",
        body: "kept body",
        comments: [{ id: 1, body: "discussion" }],
      },
    ],
  });
  const clearance = clearanceFor(adapter);
  const mapping = { node_id: "D1", gh_issue: 7, sync_to_github: false };
  assert.throws(
    () =>
      adapter.updateIssue({
        number: 7,
        title: "rewritten",
        body: "rewritten",
        mapping,
        mode: "apply",
        clearance,
      }),
    ProtectedMappingError,
  );
  assert.throws(
    () =>
      adapter.updateIssue({
        number: 7,
        title: "rewritten",
        body: "rewritten",
        mapping,
        mode: "check",
        clearance,
      }),
    ProtectedMappingError,
  );
  const issue = adapter.getIssue(7);
  assert.equal(issue.title, "kept title");
  assert.equal(issue.body, "kept body");
  assert.deepEqual(issue.comments, [{ id: 1, body: "discussion" }]);
  assert.equal(adapter.refusals.length, 2);
  assert.equal(adapter.refusals[0].kind, "protected-mapping");
  assert.equal(adapter.refusals[0].number, 7);
});

test("apply-mode mutation without doctor clearance is refused", () => {
  const adapter = new FakeGitHubAdapter({ owner: "pikax", repo: "verter" });
  assert.throws(() => adapter.createIssue({ title: "T", body: "B" }), MutationModeRequiredError);
  assert.throws(
    () => adapter.createIssue({ title: "T", body: "B", mode: "apply" }),
    DoctorRequiredError,
  );
  const plan = adapter.createIssue({ title: "T", body: "B", mode: "check" });
  assert.equal(plan.applied, false);
  assert.equal(plan.number, undefined);
  assert.equal(adapter.getIssues().length, 0);
});

test("unknown commands fail closed", () => {
  const bogus = spawnSync(process.execPath, [CLI, "not-a-command"], { encoding: "utf8" });
  assert.notEqual(bogus.status, 0);
  assert.match(bogus.stderr, /unknown command/u);
  assert.doesNotMatch(bogus.stdout, /created issue/iu);
  const help = spawnSync(process.execPath, [CLI, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /doctor/u);
});

test("owner and repo assignment cannot rebind the adapter", () => {
  const fakeAdapter = new FakeGitHubAdapter({ owner: "pikax", repo: "verter" });
  const liveAdapter = new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport: {
      request() {
        throw new Error("unused");
      },
    },
  });
  for (const adapter of [fakeAdapter, liveAdapter]) {
    assert.equal(adapter.owner, "pikax", adapter.constructor.name);
    assert.equal(adapter.repo, "verter", adapter.constructor.name);
    assert.throws(() => {
      adapter.owner = "other";
    }, TypeError);
    assert.throws(() => {
      adapter.repo = "elsewhere";
    }, TypeError);
    assert.equal(adapter.owner, "pikax", adapter.constructor.name);
    assert.equal(adapter.repo, "verter", adapter.constructor.name);
  }
});

test("GitHubAdapter and FakeGitHubAdapter share the mutation surface", () => {
  for (const name of [
    "inspectCapabilities",
    "createIssue",
    "updateIssue",
    "createPullRequest",
    "getIssue",
  ]) {
    assert.equal(typeof GitHubAdapter.prototype[name], "function", name);
    assert.equal(typeof FakeGitHubAdapter.prototype[name], "function", name);
  }
  assert.equal(typeof GitHubDoctor.prototype.check, "function");
  assert.equal(GitHubDoctor.prototype.createIssue, undefined);
});
