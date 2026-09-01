import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  CiFailedError,
  DoctorRequiredError,
  FakeGitHubAdapter,
  GitHubAdapter,
  GitHubDoctor,
  MappingMismatchError,
  MissingIssueMappingError,
  PartialFailureError,
  ciResult,
  finalizeLedger,
  squashLand,
} from "../index.mjs";
import { implementedRows, parseLedgerText } from "../../../roadmap/0.1.0-tama/tools/ledger.mjs";
import { ledgerText } from "./ledger-fixture.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const CLI = path.join(HERE, "../githubctl.mjs");
const TOOLS = path.join(REPO_ROOT, "roadmap/0.1.0-tama/tools");
const LIVE_LEDGER = path.join(REPO_ROOT, "roadmap/0.1.0-tama/authority/state/implemented.toml");
const CONTRACT = path.join(REPO_ROOT, "roadmap/0.1.0-tama/contracts/github-control-plane.md");
const TITLE = "feat(ci): report pull-request checks and squash-land through GitHub";
const DATE = "2026-08-29T03:20:00+01:00";
const PR_NUMBER = 10;
const HEAD_SHA = "0123456789abcdef0123456789abcdef01234567";
const TAMA_ROADMAP = "Tama Roadmap";

// GH5-AC3 N/A: ci-result/finalize-ledger/squash-land do not own cache, incremental, or warm-admission authority.
// GH5-AC4 N/A: these commands are occasional CLI coordination, not a hot parse/resolve path.

function fake(options = {}) {
  return new FakeGitHubAdapter({ owner: "pikax", repo: "verter", ...options });
}

function clearanceFor(adapter, require = ["issues", "pullRequests", "projects"]) {
  const report = new GitHubDoctor(adapter).check({ require });
  assert.equal(report.ok, true, report.errors?.join?.("; ") ?? "doctor failed");
  return report.clearance;
}

function writeLedger(options = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-ci-land-"));
  const file = path.join(dir, "implemented.toml");
  const implemented = options.implemented ?? ["ORC0", "GH0", "GH1", "GH2", "GH3", "GH4", "GH5"];
  const issueNode = options.issueNode ?? implemented.at(-1);
  const issues = options.issues ?? [{ node_id: issueNode, gh_issue: 4, sync_to_github: false }];
  fs.writeFileSync(
    file,
    ledgerText({
      implemented,
      pending: options.pending ?? [],
      locators: options.locators ?? {},
      messages: options.messages ?? {},
      dates: options.dates ?? {},
      issues,
    }),
  );
  return file;
}

function readLedger(file) {
  const parsed = parseLedgerText(fs.readFileSync(file, "utf8"));
  return { ...parsed, implemented: implementedRows(parsed) };
}

function seeded(options = {}) {
  return fake({
    nextNumber: options.nextNumber ?? 11,
    issues: options.issues,
    projectItems: options.projectItems,
    projectStatuses: options.projectStatuses,
    failOnApply: options.failOnApply,
    pullRequests: options.pullRequests ?? [
      {
        number: PR_NUMBER,
        title: TITLE,
        body: "Closes #4\n",
        head: "train/example",
        base: "main",
        closes: 4,
        checkRuns: options.checkRuns ?? [
          { name: TAMA_ROADMAP, conclusion: "success" },
          { name: "Rust Test", conclusion: "success" },
        ],
      },
    ],
  });
}

function publicKeys(value, found = []) {
  if (Array.isArray(value)) {
    for (const row of value) publicKeys(row, found);
    return found;
  }
  if (value === null || typeof value !== "object") return found;
  for (const [key, child] of Object.entries(value)) {
    found.push(key);
    publicKeys(child, found);
  }
  return found;
}

function assertNoShaIdentity(value) {
  for (const key of publicKeys(value)) {
    assert.doesNotMatch(key, /^(?:head_)?(?:commit_)?sha$/iu, key);
    assert.doesNotMatch(key, /landed_sha|candidate_sha|tree_sha/iu, key);
  }
}

function assertCiResultShape(report) {
  assert.equal(typeof report.ok, "boolean");
  assert.equal(report.pr, PR_NUMBER);
  assert.equal(Array.isArray(report.jobs), true);
  for (const job of report.jobs) {
    assert.equal(typeof job.name, "string");
    assert.equal(typeof job.conclusion, "string");
    assert.equal(typeof job.skipped, "boolean");
  }
  assertNoShaIdentity(report);
}

function liveTransport(routes) {
  const calls = [];
  const withProject = {
    "POST graphql": {
      data: {
        organization: { projectV2: { id: "PVT_test", number: 3 } },
        user: { projectV2: null },
      },
    },
    ...routes,
  };
  return {
    calls,
    request(req) {
      calls.push(req);
      const key = `${req.method} ${req.path}`;
      if (!Object.hasOwn(withProject, key)) throw new Error(`unexpected ${key}`);
      const hit = withProject[key];
      if (hit instanceof Error) throw hit;
      return hit;
    },
  };
}

const WRITABLE_REPO = {
  full_name: "pikax/verter",
  has_issues: true,
  permissions: { admin: false, maintain: false, push: true, triage: false, pull: true },
};

function pullPayload() {
  return {
    number: PR_NUMBER,
    title: TITLE,
    body: "Closes #4\n",
    head: { ref: "train/example", sha: HEAD_SHA },
    base: { ref: "main" },
  };
}

function ciOptions(adapter, extra = {}) {
  return {
    adapter,
    pr: extra.pr ?? PR_NUMBER,
    requiredJobs: extra.requiredJobs,
    tamaChanged: extra.tamaChanged,
    owner: extra.owner,
    repo: extra.repo,
    mode: extra.mode ?? "check",
  };
}

function squashOptions(adapter, extra = {}) {
  return {
    adapter,
    pr: extra.pr ?? PR_NUMBER,
    node: extra.node ?? "GH5",
    requiredJobs: extra.requiredJobs,
    tamaChanged: extra.tamaChanged,
    ledgerPath: extra.ledgerPath,
    owner: extra.owner,
    repo: extra.repo,
    clearance: extra.clearance,
    authority: extra.authority,
    squashPrerequisites: extra.squashPrerequisites,
    mode: extra.mode,
  };
}

test("GH5-AC1 missing required job fails CiResult", () => {
  const adapter = seeded({
    checkRuns: [{ name: "Rust Test", conclusion: "success" }],
  });
  const report = ciResult(ciOptions(adapter, { requiredJobs: [TAMA_ROADMAP], tamaChanged: true }));
  assert.equal(report.ok, false);
  assert.deepEqual(report.missing, [TAMA_ROADMAP]);
  assert.equal(
    report.jobs.some((job) => job.name === TAMA_ROADMAP),
    false,
  );
  assertCiResultShape(report);
});

test("GH5-AC1 unexpected skip of a required job fails CiResult", () => {
  const adapter = seeded({
    checkRuns: [
      { name: TAMA_ROADMAP, conclusion: "skipped" },
      { name: "Rust Test", conclusion: "success" },
    ],
  });
  const report = ciResult(ciOptions(adapter, { requiredJobs: [TAMA_ROADMAP] }));
  assert.equal(report.ok, false);
  assert.deepEqual(report.unexpected_skips, [TAMA_ROADMAP]);
  const tama = report.jobs.find((job) => job.name === TAMA_ROADMAP);
  assert.equal(tama.skipped, true);
  assert.equal(tama.conclusion, "skipped");
  assertCiResultShape(report);
});

test("GH5-AC1 finalize-ledger aborts when the implemented row is missing", () => {
  const ledgerPath = writeLedger({ implemented: ["ORC0", "GH0", "GH1", "GH2", "GH3", "GH4"] });
  const before = fs.readFileSync(ledgerPath, "utf8");
  assert.throws(
    () =>
      finalizeLedger({
        node: "GH5",
        message: TITLE,
        date: DATE,
        pr: PR_NUMBER,
        ledgerPath,
      }),
    MappingMismatchError,
  );
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), before);
  assert.equal(
    readLedger(ledgerPath).implemented.some((row) => row.node_id === "GH5"),
    false,
  );
});

test("GH5-AC1 squash-land with failed CI aborts without merging", () => {
  const adapter = seeded({
    checkRuns: [
      { name: TAMA_ROADMAP, conclusion: "failure" },
      { name: "Rust Test", conclusion: "success" },
    ],
  });
  const ledgerPath = writeLedger({ locators: { GH5: PR_NUMBER } });
  const before = adapter.inspectState();
  const beforeLedger = fs.readFileSync(ledgerPath, "utf8");
  assert.throws(
    () =>
      squashLand(
        squashOptions(adapter, {
          mode: "apply",
          ledgerPath,
          requiredJobs: [TAMA_ROADMAP],
          clearance: clearanceFor(adapter, ["pullRequests"]),
        }),
      ),
    CiFailedError,
  );
  assert.deepEqual(adapter.inspectState(), before);
  assert.equal(adapter.inspectState().merges.length, 0);
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), beforeLedger);
});

test("squash landing refuses a candidate without a mapped issue", () => {
  const adapter = seeded();
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-missing-mapping-"));
  const ledgerPath = path.join(dir, "implemented.toml");
  fs.writeFileSync(
    ledgerPath,
    ledgerText({ implemented: ["WORK"], locators: { WORK: PR_NUMBER } }),
  );
  const authority = {
    nodes: [{ id: "WORK", predecessors: [], train: "test" }],
    ledgerFile: path.join(dir, "live-implemented.toml"),
  };
  assert.throws(
    () =>
      squashLand(
        squashOptions(adapter, {
          mode: "check",
          node: "WORK",
          ledgerPath,
          authority,
          squashPrerequisites: [],
          requiredJobs: [TAMA_ROADMAP],
        }),
      ),
    MissingIssueMappingError,
  );
  assert.deepEqual(adapter.inspectState().merges, []);
});

test("post-merge failure reports both merge and completed child status", () => {
  const adapter = seeded({
    issues: [
      { number: 4, title: "Work", body: "work", parent: 5 },
      { number: 5, title: "Parent", body: "parent", subIssues: [4] },
    ],
    projectItems: [4, 5],
    projectStatuses: { 4: "In Progress", 5: "In Progress" },
    failOnApply: 2,
  });
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-partial-status-"));
  const ledgerPath = path.join(dir, "implemented.toml");
  fs.writeFileSync(
    ledgerPath,
    ledgerText({
      implemented: ["WORK"],
      locators: { WORK: PR_NUMBER },
      issues: [{ node_id: "WORK", gh_issue: 4, sync_to_github: true }],
    }),
  );
  const authority = {
    nodes: [{ id: "WORK", predecessors: [], train: "test" }],
    ledgerFile: path.join(dir, "live-implemented.toml"),
  };
  assert.throws(
    () =>
      squashLand(
        squashOptions(adapter, {
          mode: "apply",
          node: "WORK",
          ledgerPath,
          authority,
          squashPrerequisites: [],
          requiredJobs: [TAMA_ROADMAP],
          clearance: clearanceFor(adapter, ["pullRequests", "projects"]),
        }),
      ),
    (error) => {
      assert.equal(error instanceof PartialFailureError, true, `${error.name}: ${error.message}`);
      assert.deepEqual(
        error.succeeded.map((row) => [row.kind, row.number]),
        [
          ["squash-merge", PR_NUMBER],
          ["set-project-status", 4],
        ],
      );
      return true;
    },
  );
  assert.equal(adapter.getProjectStatus(4), "Done");
  assert.equal(adapter.getProjectStatus(5), "In Progress");
});

test("GH5-AC1 CiResult public fields omit SHA identity and landing receipts", () => {
  const adapter = seeded({
    checkRuns: [
      { name: TAMA_ROADMAP, conclusion: "success", head_sha: HEAD_SHA },
      { name: "Rust Test", conclusion: "success", head_sha: HEAD_SHA },
    ],
  });
  const report = ciResult(ciOptions(adapter, { requiredJobs: [TAMA_ROADMAP] }));
  assert.equal(report.ok, true);
  assertCiResultShape(report);
  assert.equal(Object.hasOwn(report, "sha"), false);
  assert.equal(Object.hasOwn(report, "head_sha"), false);
  const sources = fs
    .readdirSync(path.join(HERE, ".."))
    .filter((name) => name.endsWith(".mjs"))
    .map((name) => ({
      name,
      text: fs.readFileSync(path.join(HERE, "..", name), "utf8"),
    }));
  for (const source of sources) {
    assert.doesNotMatch(source.text, /landing[-_]?receipt/iu, source.name);
    assert.doesNotMatch(source.text, /landed_sha|candidate_sha/iu, source.name);
  }
});

test("GH5-AC1 squash-land apply does not insert an implemented row or write a receipt", () => {
  const adapter = seeded();
  const ledgerPath = writeLedger({
    implemented: ["ORC0", "GH0", "GH1", "GH2", "GH3", "GH4"],
    locators: { GH4: 9 },
  });
  const beforeLedger = fs.readFileSync(ledgerPath, "utf8");
  const tmp = path.dirname(ledgerPath);
  assert.throws(
    () =>
      squashLand(
        squashOptions(adapter, {
          mode: "apply",
          node: "GH5",
          ledgerPath,
          requiredJobs: [TAMA_ROADMAP],
          clearance: clearanceFor(adapter, ["pullRequests"]),
        }),
      ),
    MappingMismatchError,
  );
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), beforeLedger);
  assert.equal(
    readLedger(ledgerPath).implemented.some((row) => row.node_id === "GH5"),
    false,
  );
  assert.equal(adapter.inspectState().merges.length, 0);
  assert.deepEqual(
    fs.readdirSync(tmp).filter((name) => /receipt/iu.test(name)),
    [],
  );
});

test("GH5-AC2 CiResult lists jobs in deterministic name order", () => {
  const adapter = seeded({
    checkRuns: [
      { name: "Rust Test", conclusion: "success" },
      { name: TAMA_ROADMAP, conclusion: "success" },
      { name: "Detect Changes", conclusion: "success" },
    ],
  });
  const report = ciResult(ciOptions(adapter, { requiredJobs: [TAMA_ROADMAP] }));
  assert.equal(report.ok, true);
  assert.deepEqual(
    report.jobs.map((job) => job.name),
    ["Detect Changes", "Rust Test", TAMA_ROADMAP],
  );
  assertCiResultShape(report);
});

test("GH5-AC2 finalize-ledger updates message, date, and pull_request on an existing row", () => {
  const ledgerPath = writeLedger({
    messages: { GH5: "placeholder title" },
    dates: { GH5: "2026-08-28T00:00:00+00:00" },
  });
  const report = finalizeLedger({
    node: "GH5",
    message: TITLE,
    date: DATE,
    pr: PR_NUMBER,
    ledgerPath,
  });
  assert.equal(report.written, true);
  assert.equal(report.node_id, "GH5");
  assert.equal(report.pull_request, PR_NUMBER);
  const row = readLedger(ledgerPath).implemented.find((item) => item.node_id === "GH5");
  assert.deepEqual(row, {
    node_id: "GH5",
    commit_message: TITLE,
    commit_date: DATE,
    pull_request: PR_NUMBER,
  });
  assert.equal(
    readLedger(ledgerPath).implemented.filter((item) => item.node_id === "GH5").length,
    1,
  );
});

test("GH5-AC2 squash-land check is non-mutating", () => {
  const adapter = seeded();
  const ledgerPath = writeLedger({ locators: { GH5: PR_NUMBER } });
  const beforeState = adapter.inspectState();
  const beforeLedger = fs.readFileSync(ledgerPath, "utf8");
  const report = squashLand(
    squashOptions(adapter, {
      mode: "check",
      ledgerPath,
      requiredJobs: [TAMA_ROADMAP],
    }),
  );
  assert.equal(report.ok, true);
  assert.equal(report.applied, false);
  assert.equal(report.merge_method, "squash");
  assert.equal(report.kind, "squash-merge");
  assert.equal(report.number, PR_NUMBER);
  assert.deepEqual(adapter.inspectState(), beforeState);
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), beforeLedger);
  assertNoShaIdentity(report);
});

test("GH5-AC2 successful fake merge records squash without a ledger write", () => {
  const adapter = seeded();
  const ledgerPath = writeLedger({ locators: { GH5: PR_NUMBER } });
  const beforeLedger = fs.readFileSync(ledgerPath, "utf8");
  const report = squashLand(
    squashOptions(adapter, {
      mode: "apply",
      ledgerPath,
      requiredJobs: [TAMA_ROADMAP],
      clearance: clearanceFor(adapter, ["pullRequests"]),
    }),
  );
  assert.equal(report.ok, true);
  assert.equal(report.applied, true);
  assert.equal(report.merge_method, "squash");
  assert.equal(report.kind, "squash-merge");
  assert.equal(report.number, PR_NUMBER);
  assert.deepEqual(adapter.inspectState().merges, [{ number: PR_NUMBER, merge_method: "squash" }]);
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), beforeLedger);
  assertNoShaIdentity(report);
});

test("GH5 expected skip of a non-required job still succeeds", () => {
  const adapter = seeded({
    checkRuns: [
      { name: TAMA_ROADMAP, conclusion: "success" },
      { name: "Playground", conclusion: "skipped" },
    ],
  });
  const report = ciResult(ciOptions(adapter, { requiredJobs: [TAMA_ROADMAP] }));
  assert.equal(report.ok, true);
  assert.deepEqual(report.missing, []);
  assert.deepEqual(report.unexpected_skips, []);
  const skipped = report.jobs.find((job) => job.name === "Playground");
  assert.equal(skipped.skipped, true);
});

test("GH5 tamaChanged requires the Tama Roadmap job", () => {
  const adapter = seeded({
    checkRuns: [{ name: "Rust Test", conclusion: "success" }],
  });
  const report = ciResult(ciOptions(adapter, { tamaChanged: true }));
  assert.equal(report.ok, false);
  assert.deepEqual(report.missing, [TAMA_ROADMAP]);
});

test("GH5 finalize-ledger rejects a date without a timezone offset", () => {
  const ledgerPath = writeLedger();
  const before = fs.readFileSync(ledgerPath, "utf8");
  assert.throws(
    () =>
      finalizeLedger({
        node: "GH5",
        message: TITLE,
        date: "2026-08-29",
        pr: PR_NUMBER,
        ledgerPath,
      }),
    /commit_date/u,
  );
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), before);
});

test("GH5 squash-land apply without doctor pullRequests clearance is refused", () => {
  const adapter = seeded();
  const ledgerPath = writeLedger({ locators: { GH5: PR_NUMBER } });
  const before = adapter.inspectState();
  assert.throws(
    () =>
      squashLand(
        squashOptions(adapter, {
          mode: "apply",
          ledgerPath,
          requiredJobs: [TAMA_ROADMAP],
        }),
      ),
    DoctorRequiredError,
  );
  assert.deepEqual(adapter.inspectState(), before);
});

test("GH5 live adapter reads check-runs and squash-merges without exposing SHA", () => {
  const checkRunsPath = `/repos/pikax/verter/commits/${HEAD_SHA}/check-runs?per_page=100`;
  const mergePath = `/repos/pikax/verter/pulls/${PR_NUMBER}/merge`;
  const transport = liveTransport({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": WRITABLE_REPO,
    [`GET /repos/pikax/verter/pulls/${PR_NUMBER}`]: pullPayload(),
    [`GET ${checkRunsPath}`]: {
      total_count: 2,
      check_runs: [
        {
          name: "Rust Test",
          conclusion: "success",
          head_sha: HEAD_SHA,
        },
        {
          name: TAMA_ROADMAP,
          conclusion: "success",
          head_sha: HEAD_SHA,
        },
      ],
    },
    [`PUT ${mergePath}`]: {
      merged: true,
      sha: HEAD_SHA,
      message: "Pull Request successfully merged",
    },
  });
  const live = new GitHubAdapter({ owner: "pikax", repo: "verter", transport });
  const ledgerPath = writeLedger({ locators: { GH5: PR_NUMBER } });
  const ci = ciResult(ciOptions(live, { mode: "check", requiredJobs: [TAMA_ROADMAP] }));
  assert.equal(ci.ok, true);
  assert.deepEqual(
    ci.jobs.map((job) => job.name),
    ["Rust Test", TAMA_ROADMAP],
  );
  assertCiResultShape(ci);
  transport.calls.length = 0;
  const merged = squashLand(
    squashOptions(live, {
      mode: "apply",
      ledgerPath,
      requiredJobs: [TAMA_ROADMAP],
      clearance: clearanceFor(live, ["pullRequests"]),
    }),
  );
  assert.equal(merged.applied, true);
  assert.equal(merged.merge_method, "squash");
  const put = transport.calls.find((row) => row.method === "PUT");
  assert.equal(put.path, mergePath);
  assert.deepEqual(put.body, { merge_method: "squash" });
  assert.equal(Object.hasOwn(put.body, "sha"), false);
  assertNoShaIdentity(merged);
});

test("GH5 CLI check/apply flags, help, and contract name CiResult", () => {
  const ledgerPath = writeLedger({ locators: { GH5: PR_NUMBER } });
  const missingMode = spawnSync(
    process.execPath,
    [CLI, "ci-result", "--fake", "--pr", String(PR_NUMBER)],
    { encoding: "utf8" },
  );
  assert.notEqual(missingMode.status, 0);
  assert.match(missingMode.stderr, /--check|--apply/u);
  const check = spawnSync(
    process.execPath,
    [
      CLI,
      "ci-result",
      "--check",
      "--fake",
      "--pr",
      String(PR_NUMBER),
      "--required",
      TAMA_ROADMAP,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(check.status, 0);
  assert.match(check.stderr, /not found|missing/iu);
  const finalizeMissing = spawnSync(
    process.execPath,
    [
      CLI,
      "finalize-ledger",
      "--node",
      "GH9",
      "--message",
      TITLE,
      "--date",
      DATE,
      "--pr",
      String(PR_NUMBER),
      "--ledger",
      ledgerPath,
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(finalizeMissing.status, 0);
  const landCheck = spawnSync(
    process.execPath,
    [
      CLI,
      "squash-land",
      "--check",
      "--fake",
      "--pr",
      String(PR_NUMBER),
      "--node",
      "GH5",
      "--required",
      TAMA_ROADMAP,
      "--ledger",
      ledgerPath,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(landCheck.status, 0);
  const help = spawnSync(process.execPath, [CLI, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /ci-result/u);
  assert.match(help.stdout, /finalize-ledger/u);
  assert.match(help.stdout, /squash-land/u);
  for (const name of ["programctl.mjs", "lib.mjs"]) {
    const text = fs.readFileSync(path.join(TOOLS, name), "utf8");
    assert.doesNotMatch(text, /\bgh\s+api\b/u);
    assert.doesNotMatch(text, /githubctl/u);
  }
  const contract = fs.readFileSync(CONTRACT, "utf8");
  assert.match(contract, /^## CiResult$/mu);
  const heading = contract.indexOf("## CiResult");
  const next = contract.indexOf("\n## ", heading + 1);
  const section = contract.slice(heading, next === -1 ? contract.length : next);
  assert.match(section, /githubctl ci-result/u);
  assert.match(section, /githubctl finalize-ledger/u);
  assert.match(section, /githubctl squash-land/u);
  assert.match(section, /merge_method/u);
  assert.match(section, /Tama Roadmap/u);
  assert.match(section, /no landing receipt/iu);
  assert.match(section, /must not store/iu);
});

test("GH5 finalize-ledger in tests refuses the live ledger path", () => {
  const before = fs.readFileSync(LIVE_LEDGER, "utf8");
  assert.throws(
    () =>
      finalizeLedger({
        node: "GH5",
        message: TITLE,
        date: DATE,
        pr: PR_NUMBER,
        ledgerPath: LIVE_LEDGER,
      }),
    /tests must pass --ledger/i,
  );
  assert.equal(fs.readFileSync(LIVE_LEDGER, "utf8"), before);
});
