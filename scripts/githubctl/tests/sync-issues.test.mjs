import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  DuplicateError,
  FakeGitHubAdapter,
  GitHubAdapter,
  GitHubDoctor,
  MissingAncestorError,
  NotFoundError,
  PartialFailureError,
  PermissionDeniedError,
  ProtectedMappingError,
  SelectionError,
  UnstructuredGitHubOutputError,
  lookupIssueMapping,
  renderIssueDescription,
  syncIssues,
} from "../index.mjs";
import {
  githubIssueByNumber,
  listGitHubIssues,
  parseToml,
} from "../../../roadmap/0.1.0-tama/tools/lib.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const CLI = path.join(HERE, "../githubctl.mjs");
const TOOLS = path.join(REPO_ROOT, "roadmap/0.1.0-tama/tools");
const LIVE_LEDGER = path.join(REPO_ROOT, "roadmap/0.1.0-tama/authority/state/implemented.toml");
const MODEL = "gh2-test-model";

function fake(options = {}) {
  return new FakeGitHubAdapter({ owner: "pikax", repo: "verter", ...options });
}

function clearanceFor(adapter) {
  const report = new GitHubDoctor(adapter).check();
  assert.equal(report.ok, true, report.errors?.join?.("; ") ?? "doctor failed");
  return report.clearance;
}

function implementedBlock(id) {
  return `[[implemented]]
node_id = "${id}"
commit_message = "test locator ${id}"
commit_date = "2026-08-28T00:00:00+00:00"
`;
}

function mappingBlock(nodeId, issue, syncToGithub) {
  return `[[github_issue]]
node_id = "${nodeId}"
gh_issue = ${issue}
sync_to_github = ${syncToGithub}
`;
}

function writeLedger(options = {}) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-sync-"));
  const file = path.join(dir, "implemented.toml");
  const implemented = options.implemented ?? ["ORC0", "GH0", "GH1"];
  const issues = options.issues ?? [];
  const parts = ["schema = 1", "", ...implemented.map(implementedBlock)];
  for (const row of issues) parts.push(mappingBlock(row.node_id, row.gh_issue, row.sync_to_github));
  fs.writeFileSync(file, parts.join("\n"));
  return file;
}

function readLedger(file) {
  return parseToml(fs.readFileSync(file, "utf8"));
}

function rendered(nodeId = "GH0") {
  return renderIssueDescription({ nodeId, model: MODEL });
}

const WRITABLE_REPO = {
  full_name: "pikax/verter",
  has_issues: true,
  permissions: { admin: false, maintain: false, push: true, triage: false, pull: true },
};

function liveTransport(routes) {
  const calls = [];
  return {
    calls,
    request(req) {
      calls.push(req);
      const hit = routes[`${req.method} ${req.path}`];
      if (hit instanceof Error) throw hit;
      if (hit) return hit;
      throw new Error(`unexpected ${req.method} ${req.path}`);
    },
  };
}

test("GH2-AC1 protected mapping is skipped without updateIssue and body is unchanged", () => {
  const originalBody = "pre-existing protected prose";
  const adapter = fake({
    issues: [
      {
        number: 7,
        title: "kept title",
        body: originalBody,
        comments: [{ id: 1, body: "discussion" }],
      },
    ],
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 7, sync_to_github: false }],
  });
  const report = syncIssues({
    adapter,
    mode: "apply",
    nodes: ["GH0"],
    model: MODEL,
    ledgerPath,
    clearance: clearanceFor(adapter),
  });
  assert.equal(report.ok, true);
  assert.equal(report.updated.length, 0);
  assert.equal(report.created.length, 0);
  assert.deepEqual(
    report.protected.map((row) => row.node_id),
    ["GH0"],
  );
  assert.equal(
    adapter.reads.some((row) => row.number === 7),
    false,
  );
  const issue = adapter.getIssue(7);
  assert.equal(issue.title, "kept title");
  assert.equal(issue.body, originalBody);
  assert.deepEqual(issue.comments, [{ id: 1, body: "discussion" }]);
  assert.equal(adapter.refusals.length, 0);
  assert.throws(
    () =>
      adapter.updateIssue({
        number: 7,
        title: "rewritten",
        body: "rewritten",
        mapping: { node_id: "GH0", gh_issue: 7, sync_to_github: false },
        mode: "apply",
        clearance: clearanceFor(adapter),
      }),
    ProtectedMappingError,
  );
  assert.equal(adapter.refusals.length, 1);
  assert.equal(adapter.getIssue(7).body, originalBody);
});

test("GH2-AC1 apply never writes an implemented row", () => {
  const adapter = fake({ nextIssueNumber: 40 });
  const ledgerPath = writeLedger();
  const before = readLedger(ledgerPath);
  syncIssues({
    adapter,
    mode: "apply",
    nodes: ["GH0"],
    model: MODEL,
    ledgerPath,
    clearance: clearanceFor(adapter),
  });
  const after = readLedger(ledgerPath);
  assert.deepEqual(after.implemented, before.implemented);
  assert.equal(
    after.implemented.some((row) => row.node_id === "GH0" && row.commit_message === undefined),
    false,
  );
  assert.equal(
    fs.readFileSync(ledgerPath, "utf8").match(/\[\[implemented\]\]/g)?.length,
    before.implemented.length,
  );
});

test("GH2-AC1 rendered body omits DAG metadata, effort, and charter header keys", () => {
  const { title, body } = rendered("GH2");
  assert.equal(title, "Occasional issue-sync command and local mapping");
  assert.match(body, /^## Independently acceptable outcome\n/u);
  assert.match(body, /^## Source-specific scope\n/mu);
  assert.match(body, /^## Deletions and forbidden designs\n/mu);
  assert.match(body, /^## Abort conditions\n/mu);
  assert.match(body, /\nModel: gh2-test-model\n$/u);
  assert.equal([...body.matchAll(/^Model: /gmu)].length, 1);
  assert.doesNotMatch(body, /\bGH2\b/u);
  assert.doesNotMatch(body, /predecessors\s*=/u);
  assert.doesNotMatch(body, /implementation_effort/u);
  assert.doesNotMatch(body, /max_production_loc/u);
  assert.doesNotMatch(body, /unified-charter-v2/u);
  assert.doesNotMatch(body, /\bid=/u);
  assert.doesNotMatch(body, /<!--/u);
  assert.doesNotMatch(title, /\bGH2\b/u);
});

test("GH2-AC1 programctl stays GitHub-blind and CLI has no createPullRequest command", () => {
  for (const name of ["programctl.mjs", "lib.mjs"]) {
    const text = fs.readFileSync(path.join(TOOLS, name), "utf8");
    assert.doesNotMatch(text, /\bgh\s+api\b/u);
    assert.doesNotMatch(text, /\bfetch\s*\(/u);
    assert.doesNotMatch(text, /githubctl/u);
  }
  const missing = spawnSync(process.execPath, [CLI, "createPullRequest"], { encoding: "utf8" });
  assert.notEqual(missing.status, 0);
  assert.match(missing.stderr, /unknown command/u);
  const help = spawnSync(process.execPath, [CLI, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /sync-issues/u);
  assert.doesNotMatch(help.stdout, /createPullRequest/u);
});

test("GH2-AC1 unknown or missing selection fails closed", () => {
  const adapter = fake();
  const ledgerPath = writeLedger();
  assert.throws(
    () => syncIssues({ adapter, mode: "check", model: MODEL, ledgerPath }),
    SelectionError,
  );
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "check",
        train: "not-a-train",
        nodes: ["GH0"],
        model: MODEL,
        ledgerPath,
      }),
    SelectionError,
  );
  assert.throws(
    () => syncIssues({ adapter, mode: "check", nodes: ["ZZ0"], model: MODEL, ledgerPath }),
    SelectionError,
  );
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "check",
        train: "not-a-train",
        model: MODEL,
        ledgerPath,
      }),
    SelectionError,
  );
  const cli = spawnSync(process.execPath, [CLI, "sync-issues", "--check", "--fake"], {
    encoding: "utf8",
  });
  assert.notEqual(cli.status, 0);
  assert.match(cli.stderr, /--train|selection/iu);
  assert.equal(adapter.getIssues().length, 0);
});

test("GH2-AC1 check does not mutate fake state or the ledger", () => {
  const adapter = fake({
    nextIssueNumber: 9,
    issues: [
      {
        number: 3,
        title: "stale",
        body: "stale",
        comments: [{ id: 8, body: "stay" }],
      },
    ],
  });
  const ledgerPath = writeLedger({
    issues: [
      { node_id: "GH0", gh_issue: 3, sync_to_github: true },
      { node_id: "GH1", gh_issue: 4, sync_to_github: false },
    ],
  });
  const beforeState = adapter.inspectState();
  const beforeLedger = fs.readFileSync(ledgerPath, "utf8");
  const report = syncIssues({
    adapter,
    mode: "check",
    nodes: ["GH0", "GH1", "GH2"],
    model: MODEL,
    ledgerPath,
  });
  assert.equal(
    report.missing.some((row) => row.node_id === "GH2"),
    true,
  );
  assert.equal(
    report.drift.some((row) => row.node_id === "GH0"),
    true,
  );
  assert.equal(
    report.protected.some((row) => row.node_id === "GH1"),
    true,
  );
  assert.deepEqual(adapter.inspectState(), beforeState);
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), beforeLedger);
  assert.equal(
    adapter.reads.some((row) => row.number === 4),
    false,
  );
  assert.equal(
    adapter.reads.some((row) => row.number === 3),
    true,
  );
});

test("GH2-AC2 missing node creates an issue and writes the returned mapping", () => {
  const adapter = fake({ nextIssueNumber: 40 });
  const ledgerPath = writeLedger();
  const { title, body } = rendered("GH0");
  const report = syncIssues({
    adapter,
    mode: "apply",
    nodes: ["GH0"],
    model: MODEL,
    ledgerPath,
    clearance: clearanceFor(adapter),
  });
  assert.equal(report.ok, true);
  assert.deepEqual(report.created, [{ node_id: "GH0", gh_issue: 40, mapping_written: true }]);
  const issue = adapter.getIssue(40);
  assert.equal(issue.number, 40);
  assert.equal(issue.title, title);
  assert.equal(issue.body, body);
  const mapped = listGitHubIssues(readLedger(ledgerPath));
  assert.deepEqual(mapped, [{ node_id: "GH0", gh_issue: 40, sync_to_github: true }]);
  assert.equal(mapped[0].gh_issue, 40);
  assert.equal(typeof mapped[0].gh_issue, "number");
});

test("GH2-AC2 opt-in drift updates in place and preserves number and comments", () => {
  const adapter = fake({
    issues: [
      {
        number: 12,
        title: "old title",
        body: "old body",
        comments: [
          { id: 1, body: "first" },
          { id: 2, body: "second" },
        ],
      },
    ],
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH1", gh_issue: 12, sync_to_github: true }],
  });
  const { title, body } = rendered("GH1");
  const report = syncIssues({
    adapter,
    mode: "apply",
    nodes: ["GH1"],
    model: MODEL,
    ledgerPath,
    clearance: clearanceFor(adapter),
  });
  assert.equal(report.ok, true);
  assert.deepEqual(report.updated, [{ node_id: "GH1", gh_issue: 12 }]);
  assert.equal(
    adapter.reads.some((row) => row.kind === "get-issue" && row.number === 12),
    true,
  );
  const issue = adapter.getIssue(12);
  assert.equal(issue.number, 12);
  assert.equal(issue.title, title);
  assert.equal(issue.body, body);
  assert.deepEqual(issue.comments, [
    { id: 1, body: "first" },
    { id: 2, body: "second" },
  ]);
  assert.deepEqual(listGitHubIssues(readLedger(ledgerPath)), [
    { node_id: "GH1", gh_issue: 12, sync_to_github: true },
  ]);
});

test("GH2-AC2 check reports missing vs drift vs protected", () => {
  const { title, body } = rendered("GH0");
  const adapter = fake({
    issues: [
      { number: 10, title, body },
      { number: 11, title: "stale", body: "stale" },
      { number: 12, title: "protected", body: "do not read" },
    ],
  });
  const ledgerPath = writeLedger({
    issues: [
      { node_id: "GH0", gh_issue: 10, sync_to_github: true },
      { node_id: "GH1", gh_issue: 11, sync_to_github: true },
      { node_id: "GH2", gh_issue: 12, sync_to_github: false },
    ],
  });
  const report = syncIssues({
    adapter,
    mode: "check",
    nodes: ["GH0", "GH1", "GH2", "GH3"],
    model: MODEL,
    ledgerPath,
  });
  assert.deepEqual(
    report.missing.map((row) => row.node_id),
    ["GH3"],
  );
  assert.deepEqual(
    report.drift.map((row) => row.node_id),
    ["GH1"],
  );
  assert.deepEqual(
    report.protected.map((row) => row.node_id),
    ["GH2"],
  );
  assert.deepEqual(
    report.current.map((row) => row.node_id),
    ["GH0"],
  );
  assert.equal(
    adapter.reads.some((row) => row.number === 12),
    false,
  );
});

test("GH2-AC2 duplicate node or duplicate issue number aborts before GitHub writes", () => {
  const adapter = fake({ nextIssueNumber: 8 });
  const duplicateNode = writeLedger({
    issues: [
      { node_id: "GH0", gh_issue: 1, sync_to_github: true },
      { node_id: "GH0", gh_issue: 2, sync_to_github: false },
    ],
  });
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH1"],
        model: MODEL,
        ledgerPath: duplicateNode,
        clearance: clearanceFor(adapter),
      }),
    /duplicate node/i,
  );
  const duplicateIssue = writeLedger({
    issues: [
      { node_id: "GH0", gh_issue: 5, sync_to_github: true },
      { node_id: "GH1", gh_issue: 5, sync_to_github: false },
    ],
  });
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH2"],
        model: MODEL,
        ledgerPath: duplicateIssue,
        clearance: clearanceFor(adapter),
      }),
    /duplicate issue/i,
  );
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "check",
        nodes: ["GH0", "GH0"],
        model: MODEL,
        ledgerPath: writeLedger(),
      }),
    SelectionError,
  );
  assert.equal(adapter.getIssues().length, 0);
});

test("GH2-AC2 partial failure keeps the first mapping and reports succeeded numbers", () => {
  const adapter = fake({
    nextIssueNumber: 21,
    failOnApply: 1,
    failOnApplyError: new PermissionDeniedError("issues denied after first create"),
  });
  const ledgerPath = writeLedger();
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH0", "GH1"],
        model: MODEL,
        ledgerPath,
        clearance: clearanceFor(adapter),
      }),
    (error) => {
      assert.equal(error instanceof PartialFailureError, true);
      assert.equal(error.succeeded.length, 1);
      assert.equal(error.succeeded[0].node_id, "GH0");
      assert.equal(error.succeeded[0].gh_issue, 21);
      assert.equal(error.succeeded[0].kind, "create-issue");
      assert.equal(error.succeeded[0].mapping_written, true);
      assert.equal(error.succeeded[0].title, undefined);
      assert.equal(error.succeeded[0].body, undefined);
      assert.equal(error.message.includes("Created"), false);
      assert.match(error.message, /21/u);
      assert.match(error.message, /GH0/u);
      return true;
    },
  );
  assert.equal(adapter.getIssue(21).title, rendered("GH0").title);
  assert.equal(adapter.getIssue(22), null);
  assert.deepEqual(listGitHubIssues(readLedger(ledgerPath)), [
    { node_id: "GH0", gh_issue: 21, sync_to_github: true },
  ]);
  assert.equal(lookupIssueMapping(readLedger(ledgerPath), 21).node_id, "GH0");
  assert.throws(() => lookupIssueMapping(readLedger(ledgerPath), 22), /not mapped/u);
});

test("GH2-AC2 reverse lookup is local table search by unique gh_issue", () => {
  const ledger = {
    github_issue: [
      { node_id: "GH1", gh_issue: 99, sync_to_github: false, title: "not identity" },
      { node_id: "GH0", gh_issue: 42, sync_to_github: true },
    ],
  };
  assert.deepEqual(lookupIssueMapping(ledger, 42), {
    node_id: "GH0",
    gh_issue: 42,
    sync_to_github: true,
  });
  assert.deepEqual(lookupIssueMapping(ledger, 99), {
    node_id: "GH1",
    gh_issue: 99,
    sync_to_github: false,
  });
  assert.equal(lookupIssueMapping, githubIssueByNumber);
  assert.throws(() => lookupIssueMapping(ledger, "42"), /positive safe integer/u);
  assert.throws(() => lookupIssueMapping(ledger, 7), /not mapped/u);
});

test("GH2-AC2 train selection enumerates only that train in deterministic order", () => {
  const adapter = fake();
  const ledgerPath = writeLedger();
  const report = syncIssues({
    adapter,
    mode: "check",
    train: "governance.github-control-plane",
    model: MODEL,
    ledgerPath,
  });
  assert.deepEqual(report.selection, ["GH0", "GH1", "GH2", "GH3", "GH4", "GH5", "GH6"]);
  assert.equal(
    report.missing.every((row) => row.node_id.startsWith("GH")),
    true,
  );
  const reversed = syncIssues({
    adapter,
    mode: "check",
    nodes: ["GH2", "GH0", "GH1"],
    model: MODEL,
    ledgerPath,
  });
  assert.deepEqual(reversed.selection, ["GH0", "GH1", "GH2"]);
  const other = syncIssues({
    adapter,
    mode: "check",
    train: "governance.feedback-intake",
    model: MODEL,
    ledgerPath,
  });
  assert.deepEqual(other.selection, ["FB0", "FB1", "FB2"]);
  assert.equal(other.selection.includes("GH0"), false);
});

test("GH2-AC2 body ends with exactly one Model line from --model", () => {
  const { body } = rendered("GH0");
  const lines = body.split("\n");
  assert.equal(lines.at(-1), "");
  assert.equal(lines.at(-2), `Model: ${MODEL}`);
  assert.equal(lines.filter((line) => /^Model: /u.test(line)).length, 1);
});

test("GH2-AC1 missing GH1 ancestor aborts without writing", () => {
  const adapter = fake({ nextIssueNumber: 3 });
  const ledgerPath = writeLedger({ implemented: ["ORC0", "GH0"] });
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH0"],
        model: MODEL,
        ledgerPath,
        clearance: clearanceFor(adapter),
      }),
    MissingAncestorError,
  );
  assert.equal(adapter.getIssues().length, 0);
  assert.equal(listGitHubIssues(readLedger(ledgerPath)).length, 0);
});

test("GH2-AC1 mapped issue that cannot be read unambiguously aborts", () => {
  const adapter = fake();
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 99, sync_to_github: true }],
  });
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "check",
        nodes: ["GH0"],
        model: MODEL,
        ledgerPath,
      }),
    UnstructuredGitHubOutputError,
  );
});

test("GH2 apply aborts a missing mapped issue as unstructured without updateIssue", () => {
  const adapter = fake();
  let updates = 0;
  const original = adapter.updateIssue.bind(adapter);
  adapter.updateIssue = (...args) => {
    updates += 1;
    return original(...args);
  };
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 99, sync_to_github: true }],
  });
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH0"],
        model: MODEL,
        ledgerPath,
        clearance: clearanceFor(adapter),
      }),
    UnstructuredGitHubOutputError,
  );
  assert.equal(updates, 0);
  assert.equal(
    adapter.reads.some((row) => row.kind === "get-issue" && row.number === 99),
    true,
  );
  assert.equal(adapter.getIssues().length, 0);
});

test("GH2 apply aborts a PR-shaped mapped GET without PATCH", () => {
  const transport = liveTransport({
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": WRITABLE_REPO,
    "GET /repos/pikax/verter/issues/15": {
      number: 15,
      title: "PR title",
      body: "PR body",
      pull_request: { url: "https://api.github.com/repos/pikax/verter/pulls/15" },
    },
  });
  const adapter = new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport,
  });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 15, sync_to_github: true }],
  });
  const clearance = clearanceFor(adapter);
  transport.calls.length = 0;
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH0"],
        model: MODEL,
        ledgerPath,
        clearance,
      }),
    UnstructuredGitHubOutputError,
  );
  assert.equal(
    transport.calls.some((row) => row.method === "PATCH"),
    false,
  );
  assert.equal(
    transport.calls.some(
      (row) => row.method === "GET" && row.path === "/repos/pikax/verter/issues/15",
    ),
    true,
  );
});

test("GH2 create that cannot write its mapping still reports the GitHub identity", () => {
  const adapter = fake({ nextIssueNumber: 22 });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH1", gh_issue: 22, sync_to_github: false }],
  });
  const { title } = rendered("GH0");
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH0"],
        model: MODEL,
        ledgerPath,
        clearance: clearanceFor(adapter),
      }),
    (error) => {
      assert.equal(error instanceof PartialFailureError, true);
      assert.equal(error.succeeded.length, 1);
      assert.equal(error.succeeded[0].node_id, "GH0");
      assert.equal(error.succeeded[0].gh_issue, 22);
      assert.equal(error.succeeded[0].kind, "create-issue");
      assert.equal(error.succeeded[0].mapping_written, false);
      assert.equal(error.succeeded[0].title, undefined);
      assert.equal(error.succeeded[0].body, undefined);
      assert.match(error.message, /22/u);
      assert.match(error.message, /GH0/u);
      assert.equal(error.message.includes(title), false);
      assert.equal(error.failed.error instanceof DuplicateError, true);
      return true;
    },
  );
  assert.equal(adapter.getIssue(22).title, title);
  assert.deepEqual(listGitHubIssues(readLedger(ledgerPath)), [
    { node_id: "GH1", gh_issue: 22, sync_to_github: false },
  ]);
});

test("GH2 CLI prints PartialFailureError identity rows without titles", () => {
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH1", gh_issue: 1, sync_to_github: false }],
  });
  const { title } = rendered("GH0");
  const result = spawnSync(
    process.execPath,
    [
      CLI,
      "sync-issues",
      "--apply",
      "--fake",
      "--nodes",
      "GH0",
      "--model",
      MODEL,
      "--ledger",
      ledgerPath,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /ERROR:/u);
  assert.equal(result.stderr.includes(title), false);
  const identities = result.stderr
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.startsWith("{"))
    .map((line) => JSON.parse(line));
  assert.equal(identities.length, 1);
  assert.equal(identities[0].node_id, "GH0");
  assert.equal(identities[0].number, 1);
  assert.equal(identities[0].mapping_written, false);
  assert.equal(identities[0].title, undefined);
  assert.equal(identities[0].body, undefined);
});

test("GH2 live getIssue reads JSON number/title/body and does not classify payload.status", () => {
  const calls = [];
  const adapter = new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport: {
      request(req) {
        calls.push(req);
        if (req.method === "GET" && req.path === "/repos/pikax/verter/issues/4") {
          return {
            number: 4,
            title: "from gitHub",
            body: "payload",
            status: 404,
            html_url: "https://github.com/pikax/verter/issues/99",
          };
        }
        if (req.method === "GET" && req.path === "/repos/pikax/verter/issues/8") {
          throw new NotFoundError("Not Found");
        }
        throw new Error(`unexpected ${req.method} ${req.path}`);
      },
    },
  });
  assert.deepEqual(adapter.getIssue(4), { number: 4, title: "from gitHub", body: "payload" });
  assert.equal(calls[0].path, "/repos/pikax/verter/issues/4");
  assert.throws(() => adapter.getIssue(8), NotFoundError);
  assert.throws(() => adapter.getIssue("4"), /positive safe integer/u);
});

test("GH2 CLI sync-issues --check and --apply require an explicit selection and --fake", () => {
  const ledgerPath = writeLedger();
  const missingMode = spawnSync(
    process.execPath,
    [CLI, "sync-issues", "--fake", "--nodes", "GH0"],
    {
      encoding: "utf8",
    },
  );
  assert.notEqual(missingMode.status, 0);
  assert.match(missingMode.stderr, /--check|--apply/u);
  const check = spawnSync(
    process.execPath,
    [
      CLI,
      "sync-issues",
      "--check",
      "--fake",
      "--nodes",
      "GH0",
      "--model",
      MODEL,
      "--ledger",
      ledgerPath,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.equal(check.status, 0, check.stderr);
  const checked = JSON.parse(check.stdout);
  assert.equal(checked.mode, "check");
  assert.deepEqual(
    checked.missing.map((row) => row.node_id),
    ["GH0"],
  );
  assert.equal(fs.readFileSync(ledgerPath, "utf8").includes("[[github_issue]]"), false);
  const apply = spawnSync(
    process.execPath,
    [
      CLI,
      "sync-issues",
      "--apply",
      "--fake",
      "--nodes",
      "GH0",
      "--model",
      MODEL,
      "--ledger",
      ledgerPath,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.equal(apply.status, 0, apply.stderr);
  const applied = JSON.parse(apply.stdout);
  assert.equal(applied.created[0].gh_issue, 1);
  assert.equal(applied.created[0].mapping_written, true);
  assert.deepEqual(listGitHubIssues(readLedger(ledgerPath)), [
    { node_id: "GH0", gh_issue: 1, sync_to_github: true },
  ]);
});

test("GH2 apply in tests refuses the live ledger path", () => {
  const adapter = fake();
  assert.equal(fs.existsSync(LIVE_LEDGER), true);
  const before = fs.readFileSync(LIVE_LEDGER, "utf8");
  assert.throws(
    () =>
      syncIssues({
        adapter,
        mode: "apply",
        nodes: ["GH0"],
        model: MODEL,
        ledgerPath: LIVE_LEDGER,
        clearance: clearanceFor(adapter),
      }),
    /tests must pass --ledger/i,
  );
  assert.equal(fs.readFileSync(LIVE_LEDGER, "utf8"), before);
  assert.equal(listGitHubIssues(readLedger(LIVE_LEDGER)).length, 0);
});
