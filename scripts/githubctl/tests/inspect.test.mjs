import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { createGhApiTransport } from "../adapter.mjs";
import {
  AmbiguousAiLabelError,
  DoctorRequiredError,
  FakeGitHubAdapter,
  GitHubAdapter,
  GitHubDoctor,
  IgnoredIssueError,
  NotFoundError,
  PermissionDeniedError,
  UnsupportedVerdictError,
  UnstructuredGitHubOutputError,
  inspectIssue,
} from "../index.mjs";
import { writeLedgerFixture } from "./ledger-fixture.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CLI = path.join(HERE, "../githubctl.mjs");

const REPORT_HEADINGS = [
  "## Issue identity",
  "## Inspection date",
  "## Classification",
  "## Reproduction",
  "## Code paths",
  "## Commands",
  "## Verdict",
  "## Confidence / ambiguity",
  "## Owner hint",
  "## Recommendation",
];

const INSPECTED_AT = "2026-08-29T12:00:00+01:00";
const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:Z|[+-]\d{2}:\d{2})$/u;

// FB1-AC3 N/A: inspect does not own cache, incremental, or warm-admission authority.
// FB1-AC4 N/A: inspect is an occasional CLI mutation, not a hot parse/resolve path.

function fake(options = {}) {
  return new FakeGitHubAdapter({ owner: "pikax", repo: "verter", ...options });
}

function clearanceFor(adapter, require = ["issues"]) {
  const report = new GitHubDoctor(adapter).check({ require });
  assert.equal(report.ok, true, report.errors?.join?.("; ") ?? "doctor failed");
  return report.clearance;
}

function writeLedger(options = {}) {
  return writeLedgerFixture("githubctl-inspect-", {
    implemented: options.implemented ?? ["ORC0", "GH0", "GH1", "FB0"],
    issues: options.issues ?? [],
  });
}

function reportDir() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-feedback-"));
}

function reportPath(dir, issue) {
  return path.join(dir, `${issue}.md`);
}

function seedIssue(options = {}) {
  return {
    number: options.number ?? 42,
    title: options.title ?? "non-DAG report",
    body: options.body ?? "stale prose that says this is already rejected",
    comments: options.comments ?? [{ id: 1, body: "discussion" }],
    milestone: options.milestone ?? null,
    labels: options.labels ?? ["bug", "help wanted"],
  };
}

function inspectOptions(adapter, extra = {}) {
  const issue = extra.issue ?? 42;
  return {
    adapter,
    mode: extra.mode ?? "apply",
    issue,
    verdict: extra.verdict ?? "confirmed",
    ledgerPath: extra.ledgerPath,
    reportDir: extra.reportDir,
    classification: extra.classification ?? "bug",
    reproduction: extra.reproduction ?? "see failing test",
    codePaths: extra.codePaths ?? "scripts/githubctl/inspect.mjs",
    commands: extra.commands ?? "node --test scripts/githubctl/tests/inspect.test.mjs",
    confidence: extra.confidence ?? "high",
    ownerHint: extra.ownerHint ?? "governance.feedback-intake",
    recommendation: extra.recommendation ?? "keep the local report; do not author a DAG node",
    inspectedAt: extra.inspectedAt ?? INSPECTED_AT,
    clearance: extra.clearance,
  };
}

function aiOwned(labels) {
  return labels.filter((name) =>
    ["ai:unchecked", "ai:confirmed", "ai:rejected", "ai:fixed", "ai:needs-human"].includes(name),
  );
}

function ghApiIncludeStdout(httpStatus, body, options = {}) {
  const newline = options.newline ?? "\n";
  const headers = [`HTTP/2.0 ${httpStatus} X`, "Content-Type: application/json"];
  if (typeof options.link === "string" && options.link.length > 0) {
    headers.push(`Link: ${options.link}`);
  }
  const serialized = httpStatus === 204 ? "" : JSON.stringify(body);
  return `${headers.join(newline)}${newline}${newline}${serialized}`;
}

const PAGED_LABELS_PATH = "/repos/pikax/verter/issues/42/labels?per_page=100";
const PAGED_LABELS_PAGE2_PATH = "/repos/pikax/verter/issues/42/labels?per_page=100&page=2";
const PAGED_LABELS_PAGE2_URL = `https://api.github.com${PAGED_LABELS_PAGE2_PATH}`;

function spawnGhApi(routes, seen) {
  return (_command, args) => {
    seen.push(args);
    const method = args[args.indexOf("-X") + 1];
    const apiPath =
      args.find((arg) => typeof arg === "string" && arg.startsWith("/")) ??
      (args.includes("graphql") ? "graphql" : undefined);
    const spec = routes[`${method} ${apiPath}`];
    if (!spec) {
      return { status: 1, stdout: "", stderr: `unexpected ${method} ${apiPath}` };
    }
    return {
      status: spec.status ?? 0,
      stdout: spec.stdout ?? ghApiIncludeStdout(spec.httpStatus ?? 200, spec.body ?? {}, spec),
      stderr: spec.stderr ?? "",
    };
  };
}

function liveInspectRoutes(labelPages) {
  return {
    "GET /user": { body: { login: "alice" } },
    "GET /repos/pikax/verter": {
      body: {
        full_name: "pikax/verter",
        has_issues: true,
        permissions: { push: true },
      },
    },
    "POST graphql": {
      body: {
        data: {
          organization: { projectV2: { id: "PVT_test", number: 3 } },
          user: { projectV2: null },
        },
      },
    },
    "GET /repos/pikax/verter/issues/42": {
      body: { number: 42, title: "paged labels", body: "source" },
    },
    "GET /repos/pikax/verter/issues/42/labels": labelPages.page1,
    [`GET ${PAGED_LABELS_PATH}`]: labelPages.page1,
    [`GET ${PAGED_LABELS_PAGE2_PATH}`]: labelPages.page2,
  };
}

function thirtyLabelNames() {
  return Array.from({ length: 30 }, (_, index) => ({ name: `label-${index}` }));
}

function labelMutationSeen(seen) {
  return seen.some((args) => {
    const method = args[args.indexOf("-X") + 1];
    const apiPath = args.find((arg) => typeof arg === "string" && arg.includes("/labels"));
    return Boolean(apiPath) && method !== "GET";
  });
}

test("FB1-AC1 protected mapping apply writes a local report and refuses GitHub writes", () => {
  const issue = seedIssue({
    number: 7,
    labels: ["bug", "ai:unchecked", "priority"],
    milestone: "v0.1.0",
  });
  const adapter = fake({ issues: [issue] });
  const ledgerPath = writeLedger({
    issues: [{ node_id: "GH0", gh_issue: 7, sync_to_github: false }],
  });
  const ledgerBefore = fs.readFileSync(ledgerPath, "utf8");
  const dir = reportDir();
  const report = inspectIssue(
    inspectOptions(adapter, {
      issue: 7,
      ledgerPath,
      reportDir: dir,
      clearance: clearanceFor(adapter),
    }),
  );
  assert.equal(report.ok, true);
  assert.equal(report.policy, "protected");
  assert.equal(report.report_written, true);
  assert.equal(report.label_written, false);
  const stored = adapter.getIssue(7);
  assert.equal(stored.title, issue.title);
  assert.equal(stored.body, issue.body);
  assert.deepEqual(stored.comments, issue.comments);
  assert.equal(stored.milestone, "v0.1.0");
  assert.deepEqual(adapter.getIssueLabels(7), ["bug", "ai:unchecked", "priority"]);
  assert.equal(adapter.labelWrites.length, 0);
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), ledgerBefore);
  assert.match(fs.readFileSync(reportPath(dir, 7), "utf8"), /^## Issue identity$/mu);
  assert.doesNotMatch(fs.readFileSync(reportPath(dir, 7), "utf8"), /node_id|\[\[implemented\]\]/u);
});

test("FB1-AC1 ai:ignore is a complete no-op with zero report or label mutation", () => {
  const adapter = fake({
    issues: [seedIssue({ labels: ["ai:ignore", "bug", "ai:unchecked"] })],
  });
  const dir = reportDir();
  const ledgerPath = writeLedger();
  const ledgerBefore = fs.readFileSync(ledgerPath, "utf8");
  assert.throws(
    () =>
      inspectIssue(
        inspectOptions(adapter, {
          ledgerPath,
          reportDir: dir,
          clearance: clearanceFor(adapter),
        }),
      ),
    IgnoredIssueError,
  );
  assert.equal(fs.existsSync(reportPath(dir, 42)), false);
  assert.deepEqual(adapter.getIssueLabels(42), ["ai:ignore", "bug", "ai:unchecked"]);
  assert.equal(adapter.labelWrites.length, 0);
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), ledgerBefore);
});

test("FB1-AC1 inspect never creates dag labels, closes, comments, or rewrites the issue", () => {
  const adapter = fake({ issues: [seedIssue({ labels: ["bug"] })] });
  const dir = reportDir();
  inspectIssue(
    inspectOptions(adapter, {
      ledgerPath: writeLedger(),
      reportDir: dir,
      clearance: clearanceFor(adapter),
    }),
  );
  const labels = adapter.getIssueLabels(42);
  assert.deepEqual(aiOwned(labels), ["ai:confirmed"]);
  assert.equal(
    labels.some((name) => name.startsWith("dag:")),
    false,
  );
  const stored = adapter.getIssue(42);
  assert.equal(stored.body.includes("already rejected"), true);
  assert.deepEqual(stored.comments, [{ id: 1, body: "discussion" }]);
  assert.equal(stored.milestone, null);
  assert.throws(
    () =>
      inspectIssue(
        inspectOptions(adapter, {
          verdict: "checked",
          ledgerPath: writeLedger(),
          reportDir: reportDir(),
          clearance: clearanceFor(adapter),
        }),
      ),
    UnsupportedVerdictError,
  );
  assert.throws(
    () =>
      inspectIssue(
        inspectOptions(adapter, {
          verdict: "ai:confirmed",
          ledgerPath: writeLedger(),
          reportDir: reportDir(),
          clearance: clearanceFor(adapter),
        }),
      ),
    UnsupportedVerdictError,
  );
});

test("FB1-AC2 unmapped apply sets exactly one AI-result label and writes required headings", () => {
  const adapter = fake({
    issues: [seedIssue({ labels: ["bug", "ai:unchecked", "help wanted"] })],
  });
  const dir = reportDir();
  const report = inspectIssue(
    inspectOptions(adapter, {
      ledgerPath: writeLedger(),
      reportDir: dir,
      clearance: clearanceFor(adapter),
    }),
  );
  assert.equal(report.ok, true);
  assert.equal(report.policy, "unmapped");
  assert.equal(report.report_written, true);
  assert.equal(report.label_written, true);
  const labels = adapter.getIssueLabels(42);
  assert.deepEqual(aiOwned(labels), ["ai:confirmed"]);
  assert.equal(labels.includes("bug"), true);
  assert.equal(labels.includes("help wanted"), true);
  assert.equal(labels.includes("ai:unchecked"), false);
  const text = fs.readFileSync(reportPath(dir, 42), "utf8");
  for (const heading of REPORT_HEADINGS) assert.match(text, new RegExp(`^${heading}$`, "mu"));
  assert.match(text, /^42 /mu);
  assert.match(text, /^2026-08-29T12:00:00\+01:00$/mu);
  assert.match(text, /^confirmed$/mu);
  assert.doesNotMatch(text, /already rejected/u);
});

test("FB1-AC2 opt-in apply sets one AI-result label the same way as unmapped", () => {
  const adapter = fake({ issues: [seedIssue({ number: 9, labels: ["triage"] })] });
  const dir = reportDir();
  const report = inspectIssue(
    inspectOptions(adapter, {
      issue: 9,
      verdict: "needs-human",
      ledgerPath: writeLedger({
        issues: [{ node_id: "GH0", gh_issue: 9, sync_to_github: true }],
      }),
      reportDir: dir,
      clearance: clearanceFor(adapter),
    }),
  );
  assert.equal(report.policy, "opt-in");
  assert.equal(report.report_written, true);
  assert.equal(report.label_written, true);
  assert.deepEqual(aiOwned(adapter.getIssueLabels(9)), ["ai:needs-human"]);
  assert.equal(adapter.getIssueLabels(9).includes("triage"), true);
  assert.match(fs.readFileSync(reportPath(dir, 9), "utf8"), /^needs-human$/mu);
});

test("FB1-AC2 check plans without writing the report or mutating labels", () => {
  const adapter = fake({ issues: [seedIssue({ labels: ["bug"] })] });
  const dir = reportDir();
  const before = adapter.inspectState();
  const report = inspectIssue(
    inspectOptions(adapter, {
      mode: "check",
      ledgerPath: writeLedger(),
      reportDir: dir,
    }),
  );
  assert.equal(report.ok, true);
  assert.equal(report.report_written, false);
  assert.equal(report.label_written, false);
  assert.equal(fs.existsSync(reportPath(dir, 42)), false);
  assert.deepEqual(adapter.getIssueLabels(42), ["bug"]);
  assert.deepEqual(adapter.inspectState().issues, before.issues);
  assert.equal(adapter.labelWrites.length, 0);
});

test("setAiResultLabel apply is doctor-gated and preserves unrelated labels on the fake", () => {
  const adapter = fake({ issues: [seedIssue({ labels: ["bug"] })] });
  assert.throws(
    () => adapter.setAiResultLabel({ number: 42, verdict: "fixed", mode: "apply" }),
    DoctorRequiredError,
  );
  const plan = adapter.setAiResultLabel({ number: 42, verdict: "fixed", mode: "check" });
  assert.equal(plan.kind, "set-ai-result-label");
  assert.equal(plan.applied, false);
  assert.deepEqual(adapter.getIssueLabels(42), ["bug"]);
  const clearance = clearanceFor(adapter);
  const applied = adapter.setAiResultLabel({
    number: 42,
    verdict: "fixed",
    mode: "apply",
    clearance,
  });
  assert.equal(applied.applied, true);
  assert.equal(applied.label, "ai:fixed");
  assert.deepEqual(adapter.getIssueLabels(42), ["bug", "ai:fixed"]);
  adapter.permissions.issues = false;
  assert.throws(
    () =>
      adapter.setAiResultLabel({
        number: 42,
        verdict: "rejected",
        mode: "apply",
        clearance,
      }),
    PermissionDeniedError,
  );
  assert.deepEqual(adapter.getIssueLabels(42), ["bug", "ai:fixed"]);
});

test("ambiguous AI-result labels abort before report or replacement", () => {
  const adapter = fake({
    issues: [seedIssue({ labels: ["ai:unchecked", "ai:confirmed"] })],
  });
  const dir = reportDir();
  assert.throws(
    () =>
      inspectIssue(
        inspectOptions(adapter, {
          ledgerPath: writeLedger(),
          reportDir: dir,
          clearance: clearanceFor(adapter),
        }),
      ),
    AmbiguousAiLabelError,
  );
  assert.equal(fs.existsSync(reportPath(dir, 42)), false);
  assert.deepEqual(adapter.getIssueLabels(42), ["ai:unchecked", "ai:confirmed"]);
});

test("missing issues abort without a report file", () => {
  const adapter = fake();
  const dir = reportDir();
  assert.throws(
    () =>
      inspectIssue(
        inspectOptions(adapter, {
          ledgerPath: writeLedger(),
          reportDir: dir,
          clearance: clearanceFor(adapter),
        }),
      ),
    NotFoundError,
  );
  assert.equal(fs.existsSync(reportPath(dir, 42)), false);
});

test("live getIssueLabels and setAiResultLabel use add/remove JSON, never whole-set PUT", () => {
  const doctorRoutes = {
    "GET /user": { login: "alice" },
    "GET /repos/pikax/verter": {
      full_name: "pikax/verter",
      has_issues: true,
      permissions: { push: true },
    },
    "POST graphql": {
      data: {
        organization: { projectV2: { id: "PVT_test", number: 3 } },
        user: { projectV2: null },
      },
    },
  };
  const calls = [];
  const routes = {
    ...doctorRoutes,
    "GET /repos/pikax/verter/issues/42/labels?per_page=100": [
      { name: "bug" },
      { name: "ai:unchecked" },
    ],
    "POST /repos/pikax/verter/issues/42/labels": [{ name: "bug" }, { name: "ai:confirmed" }],
    "DELETE /repos/pikax/verter/issues/42/labels/ai%3Aunchecked": null,
  };
  const adapter = new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport: {
      request(req) {
        calls.push(req);
        const key = `${req.method} ${req.path}`;
        if (!Object.hasOwn(routes, key)) throw new Error(`unexpected ${key}`);
        const hit = routes[key];
        if (hit instanceof Error) throw hit;
        return hit;
      },
    },
  });
  const names = adapter.getIssueLabels(42);
  assert.deepEqual(names, ["bug", "ai:unchecked"]);
  const checkPlan = adapter.setAiResultLabel({ number: 42, verdict: "confirmed", mode: "check" });
  assert.equal(checkPlan.applied, false);
  assert.equal(
    calls.some((call) => call.method !== "GET" && call.path.includes("/labels")),
    false,
  );
  const clearance = new GitHubDoctor(adapter).check({ require: ["issues"] }).clearance;
  const mutationCalls = [];
  calls.length = 0;
  adapter.setAiResultLabel({
    number: 42,
    verdict: "confirmed",
    mode: "apply",
    clearance,
  });
  for (const call of calls) mutationCalls.push(`${call.method} ${call.path}`);
  assert.equal(mutationCalls.includes("PUT /repos/pikax/verter/issues/42/labels"), false);
  assert.equal(mutationCalls.includes("POST /repos/pikax/verter/issues/42/labels"), true);
  assert.equal(
    mutationCalls.includes("DELETE /repos/pikax/verter/issues/42/labels/ai%3Aunchecked"),
    true,
  );
  const post = calls.find((call) => call.method === "POST");
  assert.deepEqual(post.body, { labels: ["ai:confirmed"] });
});

test("page-2 ai:ignore behind Link rel=next is a complete no-op", () => {
  const seen = [];
  const adapter = new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport: createGhApiTransport({
      spawn: spawnGhApi(
        liveInspectRoutes({
          page1: {
            body: thirtyLabelNames(),
            link: `<${PAGED_LABELS_PAGE2_URL}>; rel="next", <${PAGED_LABELS_PAGE2_URL}>; rel="last"`,
          },
          page2: { body: [{ name: "ai:ignore" }] },
        }),
        seen,
      ),
    }),
  });
  const dir = reportDir();
  const ledgerPath = writeLedger();
  const ledgerBefore = fs.readFileSync(ledgerPath, "utf8");
  assert.throws(
    () =>
      inspectIssue(
        inspectOptions(adapter, {
          ledgerPath,
          reportDir: dir,
          clearance: clearanceFor(adapter),
        }),
      ),
    IgnoredIssueError,
  );
  assert.equal(fs.existsSync(reportPath(dir, 42)), false);
  assert.equal(labelMutationSeen(seen), false);
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), ledgerBefore);
  assert.equal(
    seen.some((args) => args.includes(PAGED_LABELS_PATH)),
    true,
  );
  assert.equal(
    seen.some((args) => args.includes(PAGED_LABELS_PAGE2_PATH)),
    true,
  );
});

test("ai:* labels split across Link pages abort as ambiguous before report or replacement", () => {
  const seen = [];
  const adapter = new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport: createGhApiTransport({
      spawn: spawnGhApi(
        liveInspectRoutes({
          page1: {
            body: [...thirtyLabelNames().slice(0, 29), { name: "ai:unchecked" }],
            link: `<${PAGED_LABELS_PAGE2_URL}>; rel="next"`,
          },
          page2: { body: [{ name: "ai:confirmed" }] },
        }),
        seen,
      ),
    }),
  });
  const dir = reportDir();
  const ledgerPath = writeLedger();
  const ledgerBefore = fs.readFileSync(ledgerPath, "utf8");
  assert.throws(
    () =>
      inspectIssue(
        inspectOptions(adapter, {
          ledgerPath,
          reportDir: dir,
          clearance: clearanceFor(adapter),
        }),
      ),
    AmbiguousAiLabelError,
  );
  assert.equal(fs.existsSync(reportPath(dir, 42)), false);
  assert.equal(labelMutationSeen(seen), false);
  assert.equal(fs.readFileSync(ledgerPath, "utf8"), ledgerBefore);
});

test("live labels parser rejects unstructured payloads and 204 DELETE is empty success", () => {
  const unstructured = new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport: {
      request() {
        return { name: "bug" };
      },
    },
  });
  assert.throws(() => unstructured.getIssueLabels(3), UnstructuredGitHubOutputError);

  const seen = [];
  const spawn = (_command, args) => {
    seen.push(args);
    if (args.includes("-X") && args[args.indexOf("-X") + 1] === "DELETE") {
      return {
        status: 0,
        stdout: "HTTP/2.0 204 X\nContent-Type: application/json\n\n",
        stderr: "",
      };
    }
    return {
      status: 0,
      stdout: "HTTP/2.0 200 X\nContent-Type: application/json\n\n[]",
      stderr: "",
    };
  };
  const transport = createGhApiTransport({ spawn });
  const payload = transport.request({
    method: "DELETE",
    path: "/repos/pikax/verter/issues/3/labels/ai%3Aunchecked",
  });
  assert.equal(payload, null);
  assert.equal(seen[0].includes("--include"), true);
});

test("generated inspection date is timezone-bearing when not supplied", () => {
  const adapter = fake({ issues: [seedIssue()] });
  const dir = reportDir();
  inspectIssue({
    ...inspectOptions(adapter, {
      ledgerPath: writeLedger(),
      reportDir: dir,
      clearance: clearanceFor(adapter),
    }),
    inspectedAt: undefined,
  });
  const text = fs.readFileSync(reportPath(dir, 42), "utf8");
  const dateLine = text.split("\n## Inspection date\n")[1]?.split("\n## ")[0]?.trim();
  assert.match(dateLine ?? "", DATE_PATTERN);
});

test("inspect CLI requires issue, verdict, and exactly one mutation mode", () => {
  const help = spawnSync(process.execPath, [CLI, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /inspect --check\|--apply --issue/u);

  const unknown = spawnSync(process.execPath, [CLI, "inspect"], { encoding: "utf8" });
  assert.notEqual(unknown.status, 0);
  assert.match(unknown.stderr, /exactly one of --check or --apply/u);

  const missingIssue = spawnSync(
    process.execPath,
    [CLI, "inspect", "--check", "--verdict", "confirmed", "--fake"],
    { encoding: "utf8" },
  );
  assert.notEqual(missingIssue.status, 0);
  assert.match(missingIssue.stderr, /--issue/u);

  const badVerdict = spawnSync(
    process.execPath,
    [CLI, "inspect", "--check", "--issue", "12", "--verdict", "checked", "--fake"],
    { encoding: "utf8" },
  );
  assert.notEqual(badVerdict.status, 0);
  assert.match(badVerdict.stderr, /unsupported|AiIssueVerdict|verdict/iu);
});
