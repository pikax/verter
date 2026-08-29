import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  AmbiguousWaiverError,
  DoctorRequiredError,
  FakeGitHubAdapter,
  GitHubAdapter,
  GitHubDoctor,
  MutationModeRequiredError,
  NotFoundError,
  PROJECT_NUMBER,
  releasePlan,
} from "../index.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const CLI = path.join(HERE, "../githubctl.mjs");
const CONTRACT = path.join(REPO_ROOT, "roadmap/0.1.0-tama/contracts/github-control-plane.md");
const WORKFLOWS = path.join(REPO_ROOT, ".github/workflows");
const MILESTONE = "v0.1.0";

// REL1-AC3 N/A: release-plan does not own cache, incremental, or warm-admission authority.
// REL1-AC4 N/A: milestone planning is occasional CLI coordination, not a hot parse/resolve path.

function fake(options = {}) {
  return new FakeGitHubAdapter({ owner: "pikax", repo: "verter", ...options });
}

function clearanceFor(adapter, require = ["actions"]) {
  const report = new GitHubDoctor(adapter).check({ require });
  assert.equal(report.ok, true, report.errors?.join?.("; ") ?? "doctor failed");
  return report.clearance;
}

function writeWorkflows(repoRoot, checkYaml) {
  const dir = path.join(repoRoot, ".github", "workflows");
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, "release.yml"), "name: Release\n");
  fs.writeFileSync(path.join(dir, "release-check.yml"), checkYaml);
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
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-release-plan-"));
  const file = path.join(dir, "implemented.toml");
  const implemented = options.implemented ?? ["A"];
  const issues = options.issues ?? [];
  const parts = ["schema = 1", "", ...implemented.map(implementedBlock)];
  for (const row of issues) parts.push(mappingBlock(row.node_id, row.gh_issue, row.sync_to_github));
  fs.writeFileSync(file, parts.join("\n"));
  return file;
}

function node(id, extras = {}) {
  return {
    id,
    name: id,
    train: extras.train ?? "t",
    predecessors: extras.predecessors ?? [],
    dispatchable: extras.dispatchable !== false,
    conflict_domains: extras.conflict_domains ?? [],
    resource_class: extras.resource_class ?? "ts-heavy",
  };
}

function fixture(options = {}) {
  const nodes = options.nodes ?? [
    node("A"),
    node("B", { predecessors: ["A"] }),
    node("C", { predecessors: ["B"] }),
  ];
  const implemented = options.implemented ?? ["A"];
  const issues = options.issues ?? [
    { node_id: "B", gh_issue: 10, sync_to_github: true },
    { node_id: "C", gh_issue: 11, sync_to_github: true },
  ];
  const ledgerPath = writeLedger({ implemented, issues });
  const adapter =
    options.adapter ??
    fake({
      milestones: [{ title: MILESTONE, number: 1 }],
      issues: options.githubIssues ?? [
        {
          number: 10,
          title: "B",
          body: "b",
          milestone: MILESTONE,
          state: options.bState ?? "open",
          labels: options.bLabels ?? [],
          projectStatus: options.bProjectStatus ?? null,
        },
        {
          number: 11,
          title: "C",
          body: "c",
          milestone: MILESTONE,
          state: options.cState ?? "open",
        },
      ],
      projectItems: options.projectItems ?? [],
      ...options.fake,
    });
  return {
    adapter,
    ledgerPath,
    authority: {
      nodes,
      ledgerFile: ledgerPath,
      ledger: { implemented: implemented.map((id) => ({ node_id: id })), github_issue: issues },
    },
  };
}

function plan(fx, extra = {}) {
  return releasePlan({
    adapter: fx.adapter,
    authority: fx.authority,
    ledgerPath: fx.ledgerPath,
    milestone: MILESTONE,
    ...extra,
  });
}

function rehearsalIdentity(value) {
  return {
    workflow: value.workflow,
    uses: value.uses,
    dry_run: value.dry_run,
  };
}

function productionSources() {
  return fs
    .readdirSync(path.join(HERE, ".."))
    .filter((name) => name.endsWith(".mjs"))
    .map((name) => ({
      name,
      text: fs.readFileSync(path.join(HERE, "..", name), "utf8"),
    }));
}

test("REL1-AC1 closed GitHub issue does not complete an unimplemented node", () => {
  const fx = fixture({ bState: "closed", cState: "closed" });
  const report = plan(fx, { mode: "check" });
  assert.equal(report.ok, false);
  assert.equal(
    report.items.some((row) => row.number === 10 && row.state === "closed"),
    true,
  );
  assert.equal(
    report.ready.some((row) => row.kind === "ReleaseReadiness" && row.node_id === "B"),
    false,
  );
  assert.equal(
    report.blockers.some(
      (row) =>
        row.kind === "ReleaseBlocker" && row.reason === "unimplemented" && row.node_id === "B",
    ),
    true,
  );
});

test("REL1-AC1 Project status, labels, and milestone progress never ready a node", () => {
  const fx = fixture({
    bState: "closed",
    bLabels: ["COMPLETE", "status:Done"],
    bProjectStatus: "Done",
    projectItems: [10],
  });
  assert.deepEqual(fx.adapter.getProjectItems(PROJECT_NUMBER), [10]);
  const report = plan(fx, { mode: "check" });
  assert.equal(
    report.ready.some((row) => row.node_id === "B"),
    false,
  );
  assert.equal(
    report.blockers.some((row) => row.reason === "unimplemented" && row.node_id === "B"),
    true,
  );
});

test("REL1-AC1 missing transitive ancestor is a blocker", () => {
  const fx = fixture({ implemented: ["B", "C"] });
  const report = plan(fx, { mode: "check" });
  assert.equal(report.ok, false);
  assert.equal(
    report.blockers.some(
      (row) =>
        row.reason === "missing-predecessor" && row.node_id === "C" && row.predecessor === "A",
    ),
    true,
  );
  assert.equal(
    report.blockers.some(
      (row) =>
        row.reason === "missing-predecessor" && row.node_id === "B" && row.predecessor === "A",
    ),
    true,
  );
});

test("REL1-AC1 missing predecessor is listed and unmapped items are blockers", () => {
  const fx = fixture({
    implemented: ["A", "C"],
    githubIssues: [
      { number: 10, title: "B", body: "b", milestone: MILESTONE, state: "closed" },
      { number: 11, title: "C", body: "c", milestone: MILESTONE, state: "closed" },
      { number: 99, title: "docs", body: "unmapped", milestone: MILESTONE, state: "open" },
    ],
  });
  const report = plan(fx, { mode: "check" });
  assert.equal(report.ok, false);
  assert.equal(
    report.ready.some((row) => row.kind === "ReleaseReadiness" && row.node_id === "C"),
    true,
  );
  const reasons = report.blockers.map((row) => `${row.reason}:${row.node_id ?? row.gh_issue}`);
  assert.equal(reasons.includes("unimplemented:B"), true);
  assert.equal(reasons.includes("missing-predecessor:C"), true);
  assert.equal(
    report.blockers.some((row) => row.reason === "unmapped" && row.gh_issue === 99),
    true,
  );
  const ordered = report.blockers.map((row) => row.reason);
  assert.deepEqual(
    ordered,
    [...ordered].sort((left, right) => left.localeCompare(right)),
  );
});

test("REL1-AC1 no duplicate release validator file is created", () => {
  const files = fs.readdirSync(WORKFLOWS).filter((name) => /\.ya?ml$/u.test(name));
  assert.equal(files.includes("release-check.yml"), true);
  assert.equal(files.includes("release.yml"), true);
  const callers = files.filter((name) => {
    if (name === "release.yml") return false;
    const text = fs.readFileSync(path.join(WORKFLOWS, name), "utf8");
    return (
      /uses:\s*\.\/\.github\/workflows\/release\.yml/u.test(text) && /dry_run:\s*true/u.test(text)
    );
  });
  assert.deepEqual(callers, ["release-check.yml"]);
  for (const source of productionSources()) {
    assert.doesNotMatch(source.text, /writeFile(?:Sync)?\([^;]*workflows/u, source.name);
    assert.doesNotMatch(source.text, /release-plan\.yml/u, source.name);
    assert.doesNotMatch(source.text, /release-rehearsal\.yml/u, source.name);
  }
});

test("REL1-AC1 FindingCarryForward P0 remains a blocker after GitHub closure", () => {
  const fx = fixture({
    implemented: ["A", "B", "C"],
    bState: "closed",
  });
  const closed = plan(fx, {
    mode: "check",
    findings: [{ issue: "10", severity: "P0", owner: "reviewer" }],
  });
  assert.equal(closed.ok, false);
  assert.equal(
    closed.blockers.some(
      (row) => row.reason === "finding" && row.severity === "P0" && row.issue === "10",
    ),
    true,
  );
  assert.throws(
    () =>
      plan(fx, {
        mode: "check",
        findings: [{ issue: "10", severity: "P0", owner: "reviewer", closed: true }],
      }),
    /additional property closed/u,
  );
});

test("REL1-AC1 maintainer waiver is explicit; mapped items cannot be waived", () => {
  const fx = fixture({
    implemented: ["A", "B", "C"],
    githubIssues: [
      { number: 10, title: "B", body: "b", milestone: MILESTONE },
      { number: 11, title: "C", body: "c", milestone: MILESTONE },
      { number: 99, title: "docs", body: "unmapped", milestone: MILESTONE },
    ],
  });
  assert.throws(() => plan(fx, { mode: "check", waiveItems: [10] }), AmbiguousWaiverError);
  const waived = plan(fx, { mode: "check", waiveItems: [99] });
  assert.equal(waived.ok, true);
  assert.equal(
    waived.blockers.some((row) => row.reason === "unmapped"),
    false,
  );
  assert.deepEqual(
    waived.waived.map((row) => row.gh_issue),
    [99],
  );
  const inferred = plan(fx, { mode: "check" });
  assert.equal(inferred.ok, false);
  assert.equal(
    inferred.blockers.some((row) => row.reason === "unmapped" && row.gh_issue === 99),
    true,
  );
});

test("REL1-AC2 implemented mapped items are ready with deterministic blockers and rehearsal identity", () => {
  const fx = fixture({ implemented: ["A", "B"] });
  const report = plan(fx, { mode: "check" });
  assert.equal(report.ok, false);
  assert.deepEqual(
    report.ready.map((row) => row.node_id),
    ["B"],
  );
  assert.equal(report.ready[0].kind, "ReleaseReadiness");
  assert.equal(report.ready[0].gh_issue, 10);
  const blockerKeys = report.blockers.map(
    (row) => `${row.reason}:${row.node_id ?? row.gh_issue}:${row.predecessor ?? ""}`,
  );
  assert.deepEqual(
    blockerKeys,
    [...blockerKeys].sort((left, right) => left.localeCompare(right)),
  );
  assert.equal(
    report.blockers.some((row) => row.kind === "ReleaseBlocker" && row.reason === "unimplemented"),
    true,
  );
  assert.deepEqual(rehearsalIdentity(report.rehearsal), {
    workflow: "release-check.yml",
    uses: "release.yml",
    dry_run: true,
  });
  assert.equal(report.rehearsal.dispatched, false);
});

test("REL1-AC2 apply records rehearsal identity without dispatching and does not write GitHub", () => {
  const fx = fixture({ implemented: ["A", "B", "C"] });
  const check = plan(fx, { mode: "check" });
  const apply = plan(fx, { mode: "apply" });
  assert.equal(check.ok, true);
  assert.equal(apply.ok, true);
  assert.deepEqual(
    apply.ready.map((row) => row.node_id),
    ["B", "C"],
  );
  assert.deepEqual(rehearsalIdentity(apply.rehearsal), {
    workflow: "release-check.yml",
    uses: "release.yml",
    dry_run: true,
  });
  assert.equal(apply.rehearsal.recorded, true);
  assert.equal(check.rehearsal.recorded, false);
  assert.equal(apply.rehearsal.dispatched, false);
  assert.equal(apply.rehearsal.terminal_result, "not-run");
  assert.equal(check.rehearsal.terminal_result, "not-run");
  assert.deepEqual(fx.adapter.workflowDispatches, []);
  assert.equal(fx.adapter.milestoneWrites.length, 0);
  assert.deepEqual(fx.adapter.getProjectItems(PROJECT_NUMBER), []);
});

test("REL1-AC2 explicit --dispatch rehearses on the fake only after a ready apply", () => {
  const blocked = fixture({ implemented: ["A"] });
  const blockedPlan = plan(blocked, {
    mode: "apply",
    dispatch: true,
    clearance: clearanceFor(blocked.adapter),
  });
  assert.equal(blockedPlan.ok, false);
  assert.equal(blockedPlan.rehearsal.dispatched, false);
  assert.equal(blockedPlan.rehearsal.terminal_result, "not-run");
  assert.deepEqual(blocked.adapter.workflowDispatches, []);
  const ready = fixture({ implemented: ["A", "B", "C"] });
  assert.throws(() => plan(ready, { mode: "check", dispatch: true }), /--dispatch/u);
  const dispatched = plan(ready, {
    mode: "apply",
    dispatch: true,
    clearance: clearanceFor(ready.adapter),
  });
  assert.equal(dispatched.ok, dispatched.blockers.length === 0);
  assert.equal(dispatched.ok, true);
  assert.equal(dispatched.rehearsal.dispatched, true);
  assert.equal(dispatched.rehearsal.terminal_result, "pending");
  assert.deepEqual(ready.adapter.workflowDispatches, [
    { workflow: "release-check.yml", uses: "release.yml", dry_run: true },
  ]);
});

test("REL1-AC2 live listMilestoneIssues uses state=all and skips pull requests", () => {
  const calls = [];
  const adapter = new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport: {
      request(req) {
        calls.push(req);
        if (req.method === "GET" && req.path.includes("/milestones?")) {
          return [{ number: 1, title: MILESTONE, state: "open" }];
        }
        if (req.method === "GET" && req.path.includes("/issues?")) {
          return [
            {
              number: 10,
              title: "B",
              state: "closed",
              milestone: { title: MILESTONE },
            },
            {
              number: 11,
              title: "PR",
              state: "open",
              pull_request: { url: "https://example.invalid" },
              milestone: { title: MILESTONE },
            },
          ];
        }
        throw new Error(`unexpected ${req.method} ${req.path}`);
      },
    },
  });
  const listed = adapter.listMilestoneIssues(MILESTONE);
  assert.deepEqual(listed, [{ number: 10, title: "B", state: "closed", milestone: MILESTONE }]);
  assert.equal(
    calls.some((row) => row.path.includes("milestones?") && row.path.includes("state=all")),
    true,
  );
  assert.equal(
    calls.some(
      (row) =>
        row.path.includes("/issues?") &&
        row.path.includes("milestone=1") &&
        row.path.includes("state=all"),
    ),
    true,
  );
  assert.throws(() => adapter.listMilestoneIssues("missing"), NotFoundError);
});

test("REL1-AC2 CLI release-plan check plans; apply does not dispatch by default", () => {
  const help = spawnSync(process.execPath, [CLI, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /release-plan --check/u);
  assert.match(help.stdout, /--milestone/u);
  assert.match(help.stdout, /--dispatch is doctor-gated/u);
  const missing = spawnSync(
    process.execPath,
    [
      CLI,
      "release-plan",
      "--check",
      "--fake",
      "--milestone",
      MILESTONE,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(missing.status, 0);
  assert.match(missing.stderr, /milestone v0\.1\.0 is missing/u);
  const dispatchCheck = spawnSync(
    process.execPath,
    [
      CLI,
      "release-plan",
      "--check",
      "--dispatch",
      "--fake",
      "--milestone",
      MILESTONE,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(dispatchCheck.status, 0);
  assert.match(dispatchCheck.stderr, /--dispatch/u);
});

test("REL1 contract names ReleaseReadiness and ReleaseBlocker", () => {
  const text = fs.readFileSync(CONTRACT, "utf8");
  for (const name of ["ReleaseReadiness", "ReleaseBlocker"]) {
    assert.match(text, new RegExp(`^## ${name}$`, "mu"), `missing heading ${name}`);
  }
  assert.match(text, /release-check\.yml/u);
  assert.match(text, /dry_run: true/u);
  assert.match(text, /`\[\[implemented\]\]`/u);
  const adapterSection = text.split("## GitHubAdapter")[1]?.split(/^## /mu)[0] ?? "";
  assert.match(adapterSection, /listMilestoneIssues/u);
  assert.match(adapterSection, /dispatchReleaseRehearsal/u);
  assert.match(text, /terminal_result/u);
  assert.match(text, /Live job poll is not default/u);
});

test("REL1-AC1 comment-only dry_run true does not satisfy rehearsal identity", () => {
  const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-rehearsal-"));
  writeWorkflows(
    repoRoot,
    `name: Bad
# dry_run: true
jobs:
  dry-run:
    uses: ./.github/workflows/release.yml
    with:
      dry_run: false
`,
  );
  const fx = fixture({ implemented: ["A", "B", "C"] });
  assert.throws(() => plan(fx, { mode: "check", repoRoot }), /dry_run/u);
});

test("REL1-AC1 dispatch apply without mode or clearance is refused", () => {
  const adapter = fake();
  assert.throws(() => adapter.dispatchReleaseRehearsal(), MutationModeRequiredError);
  assert.throws(() => adapter.dispatchReleaseRehearsal({}), MutationModeRequiredError);
  assert.throws(() => adapter.dispatchReleaseRehearsal({ mode: "apply" }), DoctorRequiredError);
  const check = adapter.dispatchReleaseRehearsal({ mode: "check" });
  assert.equal(check.applied, false);
  assert.deepEqual(adapter.workflowDispatches, []);
});

test("REL1-AC1 CLI --apply --dispatch without doctor clearance is refused", () => {
  const fx = fixture({ implemented: ["A", "B", "C"] });
  assert.throws(() => plan(fx, { mode: "apply", dispatch: true }), DoctorRequiredError);
  assert.deepEqual(fx.adapter.workflowDispatches, []);
  const denied = fixture({
    implemented: ["A", "B", "C"],
    fake: { permissions: { actions: false } },
  });
  const doctor = new GitHubDoctor(denied.adapter).check({ require: ["actions"] });
  assert.equal(doctor.ok, false);
  assert.equal(doctor.errors.includes("actions"), true);
  assert.throws(
    () =>
      plan(denied, {
        mode: "apply",
        dispatch: true,
        clearance: doctor.clearance,
      }),
    DoctorRequiredError,
  );
  const cli = spawnSync(
    process.execPath,
    [
      CLI,
      "release-plan",
      "--apply",
      "--dispatch",
      "--fake",
      "--milestone",
      MILESTONE,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(cli.status, 0);
  assert.match(cli.stderr, /milestone v0\.1\.0 is missing/u);
  assert.doesNotMatch(cli.stderr, /apply requires GitHubDoctor/u);
});

test("REL1-AC2 POST 204 records dispatch accepted not rehearsal passed", () => {
  const calls = [];
  const adapter = new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport: {
      request(req) {
        calls.push(req);
        if (req.method === "GET" && req.path === "/user") return { login: "alice" };
        if (req.method === "GET" && req.path === "/repos/pikax/verter") {
          return {
            full_name: "pikax/verter",
            has_issues: true,
            permissions: { push: true, admin: false, maintain: false, triage: false, pull: true },
          };
        }
        if (req.method === "POST" && req.path === "graphql") {
          return {
            data: {
              organization: { projectV2: { id: "PVT", number: 3 } },
              user: { projectV2: null },
            },
          };
        }
        if (
          req.method === "POST" &&
          req.path === "/repos/pikax/verter/actions/workflows/release-check.yml/dispatches"
        ) {
          return null;
        }
        throw new Error(`unexpected ${req.method} ${req.path}`);
      },
    },
  });
  const doctor = new GitHubDoctor(adapter).check({ require: ["actions"] });
  assert.equal(doctor.ok, true, doctor.errors?.join?.("; ") ?? "doctor failed");
  const result = adapter.dispatchReleaseRehearsal({
    mode: "apply",
    clearance: doctor.clearance,
  });
  assert.equal(result.applied, true);
  assert.equal(
    calls.some((row) => typeof row.path === "string" && row.path.includes("/actions/runs")),
    false,
  );
});
