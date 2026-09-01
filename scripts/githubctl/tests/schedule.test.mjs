import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  DoctorRequiredError,
  FakeGitHubAdapter,
  GitHubAdapter,
  GitHubDoctor,
  MissingIssueMappingError,
  MissingProjectIdentityError,
  NonReadyNodeError,
  NotFoundError,
  PartialFailureError,
  PROJECT_NUMBER,
  PROJECT_VIEWS,
  ProtectedMappingError,
  schedule,
} from "../index.mjs";
import { deriveState, loadAuthority } from "../../../roadmap/0.1.0-tama/tools/lib.mjs";
import { writeLedgerFixture } from "./ledger-fixture.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(HERE, "../../..");
const CLI = path.join(HERE, "../githubctl.mjs");
const TOOLS = path.join(REPO_ROOT, "roadmap/0.1.0-tama/tools");
const CONTRACT = path.join(REPO_ROOT, "roadmap/0.1.0-tama/contracts/github-control-plane.md");

function fake(options = {}) {
  return new FakeGitHubAdapter({ owner: "pikax", repo: "verter", ...options });
}

function clearanceFor(adapter) {
  const report = new GitHubDoctor(adapter).check();
  assert.equal(report.ok, true, report.errors?.join?.("; ") ?? "doctor failed");
  return report.clearance;
}

function writeLedger(options = {}) {
  return writeLedgerFixture("githubctl-schedule-", {
    implemented: options.implemented ?? ["A"],
    issues: options.issues ?? [],
  });
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
    node("D", { predecessors: ["A"] }),
  ];
  const implemented = options.implemented ?? ["A"];
  const issues = options.issues ?? [
    { node_id: "B", gh_issue: 10, sync_to_github: true },
    { node_id: "D", gh_issue: 11, sync_to_github: true },
  ];
  const ledgerPath = writeLedger({ implemented, issues });
  const adapter =
    options.adapter ??
    fake({
      issues: [
        { number: 10, title: "B", body: "b", milestone: options.milestone ?? null },
        { number: 11, title: "D", body: "d" },
      ],
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

function runSchedule(fx, extra = {}) {
  return schedule({
    adapter: fx.adapter,
    authority: fx.authority,
    ledgerPath: fx.ledgerPath,
    ...extra,
  });
}

// REL0-AC3: not applicable — scheduling overlay has no incremental, cache,
// cancellation, or stale-publication authority.
// REL0-AC4: not applicable — not a compile/resolve hot path.

test("REL0-AC1 non-READY selected node aborts and does not add project items", () => {
  const fx = fixture();
  assert.throws(
    () => runSchedule(fx, { mode: "apply", nodes: ["C"], clearance: clearanceFor(fx.adapter) }),
    NonReadyNodeError,
  );
  assert.deepEqual(fx.adapter.getProjectItems(PROJECT_NUMBER), []);
  assert.equal(fx.adapter.milestoneWrites.length, 0);
});

test("REL0-AC1 COMPLETE selected node aborts because COMPLETE is not READY", () => {
  const fx = fixture();
  assert.equal(deriveState(fx.authority).states.get("A").status, "COMPLETE");
  assert.throws(() => runSchedule(fx, { mode: "check", nodes: ["A"] }), NonReadyNodeError);
  assert.throws(
    () =>
      runSchedule(fx, {
        mode: "apply",
        nodes: ["A", "B"],
        clearance: clearanceFor(fx.adapter),
      }),
    NonReadyNodeError,
  );
  assert.deepEqual(fx.adapter.getProjectItems(PROJECT_NUMBER), []);
});

test("REL0-AC1 missing Project 3 aborts", () => {
  const adapter = fake({ projectNumber: 3, missing: true });
  const fx = fixture({ adapter, fake: {} });
  assert.equal(adapter.inspectCapabilities().projects, false);
  const doctor = new GitHubDoctor(adapter).check();
  assert.equal(doctor.errors.includes("projects"), true);
  assert.throws(
    () => runSchedule(fx, { mode: "check", nodes: ["B"] }),
    MissingProjectIdentityError,
  );
  assert.throws(() => adapter.getProject(3), MissingProjectIdentityError);
  assert.throws(
    () => adapter.addIssueToProject({ number: 3, issueNumber: 10, mode: "check" }),
    MissingProjectIdentityError,
  );
  assert.deepEqual(adapter.getProjectItems(3), []);
});

test("REL0-AC1 milestone write without --set-milestone does not call setIssueMilestone", () => {
  const fx = fixture();
  const plan = runSchedule(fx, { mode: "check", nodes: ["B"] });
  assert.equal(plan.release_target, null);
  const applied = runSchedule(fx, {
    mode: "apply",
    nodes: ["B"],
    clearance: clearanceFor(fx.adapter),
  });
  assert.equal(applied.release_target, null);
  assert.equal(fx.adapter.milestoneWrites.length, 0);
  assert.equal(fx.adapter.getIssue(10).milestone ?? null, null);
});

test("REL0-AC1 programctl stays GitHub-blind", () => {
  for (const name of ["programctl.mjs", "lib.mjs"]) {
    const text = fs.readFileSync(path.join(TOOLS, name), "utf8");
    assert.doesNotMatch(text, /\bgh\s+api\b/u);
    assert.doesNotMatch(text, /\bgh\s+project\b/u);
    assert.doesNotMatch(text, /\bfetch\s*\(/u);
    assert.doesNotMatch(text, /https:\/\/api\.github/u);
    assert.doesNotMatch(text, /githubctl/u);
  }
});

test("REL0-AC1 Project Status and labels cannot READY a blocked node", () => {
  const fx = fixture({
    adapter: fake({
      issues: [
        {
          number: 12,
          title: "blocked",
          body: "body",
          labels: ["READY"],
          projectStatus: "READY",
          milestone: "v0.1.0",
        },
      ],
    }),
    issues: [{ node_id: "C", gh_issue: 12, sync_to_github: true }],
  });
  assert.equal(deriveState(fx.authority).states.get("C").status, "BLOCKED");
  assert.throws(() => runSchedule(fx, { mode: "check", nodes: ["C"] }), NonReadyNodeError);
  assert.deepEqual(fx.adapter.getProjectItems(PROJECT_NUMBER), []);
});

test("REL0-AC2 READY mapped node check plan includes issue number and Project 3", () => {
  const fx = fixture({ milestone: "v0.1.0" });
  const plan = runSchedule(fx, { mode: "check", nodes: ["B"] });
  assert.equal(plan.ok, true);
  assert.equal(plan.mode, "check");
  assert.equal(plan.project.number, PROJECT_NUMBER);
  assert.equal(PROJECT_NUMBER, 3);
  assert.deepEqual(plan.selection, ["B"]);
  assert.equal(plan.items[0].node_id, "B");
  assert.equal(plan.items[0].gh_issue, 10);
  assert.equal(plan.items[0].project, 3);
  assert.equal(plan.items[0].status, "Todo");
  assert.equal(plan.items[0].status_changed, true);
  assert.equal(plan.items[0].milestone, "v0.1.0");
  assert.deepEqual(fx.adapter.getProjectItems(3), []);
  assert.equal(fx.adapter.milestoneWrites.length, 0);
});

test("REL0-AC2 apply adds Project 3 membership and a second apply is idempotent", () => {
  const fx = fixture();
  const first = runSchedule(fx, {
    mode: "apply",
    nodes: ["B"],
    clearance: clearanceFor(fx.adapter),
  });
  assert.equal(first.ok, true);
  assert.equal(first.items[0].status_changed, true);
  assert.deepEqual(fx.adapter.getProjectItems(3), [10]);
  assert.equal(fx.adapter.getProjectStatus(10), "Todo");
  const second = runSchedule(fx, {
    mode: "apply",
    nodes: ["B"],
    clearance: clearanceFor(fx.adapter),
  });
  assert.equal(second.ok, true);
  assert.equal(second.items[0].already_member, true);
  assert.equal(second.items[0].status_changed, false);
  assert.deepEqual(fx.adapter.getProjectItems(3), [10]);
  const again = fx.adapter.addIssueToProject({
    number: 3,
    issueNumber: 10,
    mode: "apply",
    clearance: clearanceFor(fx.adapter),
  });
  assert.equal(again.applied, true);
  assert.equal(again.already_member, true);
  assert.deepEqual(fx.adapter.getProjectItems(3), [10]);
});

test("scheduling adds an eligible native parent without resetting existing status", () => {
  const nodes = [node("A"), node("B", { predecessors: ["A"] })];
  const issues = [{ node_id: "B", gh_issue: 10, sync_to_github: true }];
  const adapter = fake({
    issues: [
      { number: 10, title: "Work", body: "work", parent: 20 },
      { number: 20, title: "Parent", body: "parent", subIssues: [10] },
    ],
  });
  const getState = adapter.getIssueProjectState.bind(adapter);
  adapter.getIssueProjectState = (number) => {
    const snapshot = getState(number);
    if (snapshot.parent) {
      snapshot.parent.owner = snapshot.parent.owner.toUpperCase();
      snapshot.parent.repo = snapshot.parent.repo.toUpperCase();
    }
    return snapshot;
  };
  const fx = fixture({ nodes, implemented: ["A"], issues, adapter });

  const first = runSchedule(fx, {
    mode: "apply",
    nodes: ["B"],
    clearance: clearanceFor(adapter),
  });
  assert.deepEqual(adapter.getProjectItems(PROJECT_NUMBER), [10, 20]);
  assert.equal(adapter.getProjectStatus(10), "Todo");
  assert.equal(adapter.getProjectStatus(20), "Todo");
  assert.equal(first.parents[0].gh_issue, 20);
  assert.equal(first.parents[0].status_changed, true);

  adapter.setIssueProjectStatus({
    issueNumber: 20,
    status: "In Progress",
    mode: "apply",
    clearance: clearanceFor(adapter),
  });
  const second = runSchedule(fx, {
    mode: "apply",
    nodes: ["B"],
    clearance: clearanceFor(adapter),
  });
  assert.equal(second.parents[0].already_member, true);
  assert.equal(second.parents[0].status_changed, false);
  assert.equal(adapter.getProjectStatus(20), "In Progress");
});

test("a protected mapped sibling suppresses native parent scheduling", () => {
  const nodes = [node("A"), node("B", { predecessors: ["A"] }), node("C", { predecessors: ["A"] })];
  const issues = [
    { node_id: "B", gh_issue: 10, sync_to_github: true },
    { node_id: "C", gh_issue: 11, sync_to_github: false },
  ];
  const adapter = fake({
    issues: [
      { number: 10, title: "Work", body: "work", parent: 20 },
      { number: 11, title: "Protected", body: "protected", parent: 20 },
      { number: 20, title: "Parent", body: "parent", subIssues: [10, 11] },
    ],
  });
  const fx = fixture({ nodes, implemented: ["A"], issues, adapter });

  const report = runSchedule(fx, {
    mode: "apply",
    nodes: ["B"],
    clearance: clearanceFor(adapter),
  });
  assert.deepEqual(adapter.getProjectItems(PROJECT_NUMBER), [10]);
  assert.deepEqual(report.parents, []);
  assert.equal(report.parent_skipped[0].gh_issue, 20);
  assert.equal(
    adapter.reads.some((row) => row.kind === "get-issue-project-state" && row.number === 20),
    false,
  );
});

test("scheduling never writes milestones even when legacy input is supplied", () => {
  const fx = fixture({
    fake: {
      issues: [{ number: 10, title: "B", body: "b" }],
      milestones: [{ title: "v0.1.0", number: 1 }],
      pullRequests: [
        {
          number: 20,
          title: "pr",
          body: "Closes #10",
          head: "train/b",
          base: "main",
          closes: 10,
        },
      ],
    },
  });
  runSchedule(fx, { mode: "check", nodes: ["B"], setMilestone: "v0.1.0" });
  assert.equal(fx.adapter.milestoneWrites.length, 0);
  const applied = runSchedule(fx, {
    mode: "apply",
    nodes: ["B"],
    setMilestone: "v0.1.0",
    clearance: clearanceFor(fx.adapter),
  });
  assert.equal(applied.release_target, null);
  assert.deepEqual(fx.adapter.milestoneWrites, []);
  assert.equal(fx.adapter.getIssue(10).milestone, null);
  assert.equal(fx.adapter.getPullRequest(20).milestone, undefined);
  const after = deriveState(fx.authority);
  assert.equal(after.states.get("B").status, "READY");
  assert.equal(after.states.get("C").status, "BLOCKED");
});

test("REL0-AC2 train selection READY-only ordering is deterministic topo among READY", () => {
  const fx = fixture();
  const plan = runSchedule(fx, { mode: "check", train: "t" });
  assert.deepEqual(plan.selection, ["B", "D"]);
  assert.equal(
    plan.items.every((row) => row.status === "Todo"),
    true,
  );
  assert.equal(plan.selection.includes("A") || plan.selection.includes("C"), false);
});

test("REL0-AC2 views list is the frozen seven names", () => {
  assert.deepEqual(PROJECT_VIEWS, [
    "execution",
    "READY",
    "triage",
    "review/gate",
    "train",
    "milestone",
    "roadmap",
  ]);
  assert.ok(Object.isFrozen(PROJECT_VIEWS));
  const fx = fixture();
  const plan = runSchedule(fx, { mode: "check", nodes: ["B"] });
  assert.deepEqual(plan.views, PROJECT_VIEWS);
});

test("REL0-AC2 missing mapping for a READY node aborts", () => {
  const fx = fixture({ issues: [] });
  assert.throws(() => runSchedule(fx, { mode: "check", nodes: ["B"] }), MissingIssueMappingError);
  assert.deepEqual(fx.adapter.getProjectItems(3), []);
});

test("scheduling refuses a protected mapping before GitHub traffic", () => {
  const fx = fixture({
    issues: [{ node_id: "B", gh_issue: 10, sync_to_github: false }],
  });
  assert.throws(() => runSchedule(fx, { mode: "check", nodes: ["B"] }), ProtectedMappingError);
  assert.deepEqual(fx.adapter.reads, []);
  assert.deepEqual(fx.adapter.getProjectItems(PROJECT_NUMBER), []);
});

test("scheduling reports a completed membership add when status initialization fails", () => {
  const fx = fixture({ fake: { failOnApply: 1 } });
  assert.throws(
    () =>
      runSchedule(fx, {
        mode: "apply",
        nodes: ["B"],
        clearance: clearanceFor(fx.adapter),
      }),
    (error) => {
      assert.equal(error instanceof PartialFailureError, true);
      assert.deepEqual(error.succeeded, [{ node_id: "B", number: 10, kind: "add-project-item" }]);
      return true;
    },
  );
  assert.deepEqual(fx.adapter.getProjectItems(PROJECT_NUMBER), [10]);
});

test("REL0-AC1 fake refuses to create a project other than Project 3", () => {
  assert.throws(() => fake({ projectNumber: 4 }), /Project 3/u);
  const adapter = fake();
  const clearance = clearanceFor(adapter);
  assert.throws(
    () => adapter.addIssueToProject({ number: 4, issueNumber: 10, mode: "apply", clearance }),
    /Project 3/u,
  );
  assert.equal(adapter.createProject, undefined);
  assert.deepEqual(adapter.getProjectItems(3), []);
});

function liveGraphqlAdapter(resolveGraphql, extra = {}) {
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
            permissions: { push: true },
          };
        }
        if (req.method === "GET" && req.path.startsWith("/repos/pikax/verter/issues/")) {
          const number = Number(req.path.split("/").at(-1));
          return extra.issue ?? { number, title: "B", body: "b" };
        }
        if (req.path === "graphql") return resolveGraphql(req, calls);
        throw new Error(`unexpected ${req.method} ${req.path}`);
      },
    },
  });
  return { adapter, calls };
}

function graphqlEnvelope(query, envelopes) {
  for (const [needle, payload] of envelopes) {
    if (query.includes(needle)) return payload;
  }
  throw new Error(`unexpected graphql ${query}`);
}

const PROJECT_OK = {
  data: {
    organization: { projectV2: { id: "PVT_3", number: 3, viewerCanUpdate: true } },
    user: { projectV2: null },
  },
};

test("REL0-AC1 GraphQL 200 with errors or null project is missing Project identity", () => {
  const graphqlNull = new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport: {
      request(req) {
        assert.equal(req.path, "graphql");
        return { data: { organization: { projectV2: null }, user: { projectV2: null } } };
      },
    },
  });
  assert.throws(() => graphqlNull.getProject(3), MissingProjectIdentityError);
  const graphqlErrors = new GitHubAdapter({
    owner: "pikax",
    repo: "verter",
    transport: {
      request() {
        return { data: { organization: { projectV2: { id: "PVT_x", number: 3 } } }, errors: [{}] };
      },
    },
  });
  assert.throws(() => graphqlErrors.getProject(3), MissingProjectIdentityError);
});

test("REL0-AC1 GraphQL 200 with errors on addProjectV2ItemById aborts and schedule is not ok", () => {
  const { adapter } = liveGraphqlAdapter((req) =>
    graphqlEnvelope(req.body.query, [
      [
        "addProjectV2ItemById",
        { data: { addProjectV2ItemById: { item: { id: "PVTI_x" } } }, errors: [{}] },
      ],
      [
        "subIssues(first:100)",
        {
          data: {
            repository: {
              issue: {
                id: "I_1",
                number: 10,
                parent: null,
                projectItems: { totalCount: 0, nodes: [] },
                subIssues: { totalCount: 0, nodes: [] },
              },
            },
          },
        },
      ],
      [
        "issue(",
        {
          data: {
            repository: { issue: { id: "I_1", projectItems: { totalCount: 0, nodes: [] } } },
          },
        },
      ],
      ["projectV2", PROJECT_OK],
    ]),
  );
  const clearance = clearanceFor(adapter);
  assert.throws(
    () =>
      adapter.addIssueToProject({
        number: 3,
        issueNumber: 10,
        mode: "apply",
        clearance,
      }),
    (error) => error.name === "GitHubAdapterError",
  );
  const fx = fixture({ adapter });
  assert.throws(
    () => runSchedule(fx, { mode: "apply", nodes: ["B"], clearance }),
    (error) => error.name === "GitHubAdapterError",
  );
});

test("addIssueToProject requires a returned item identity", () => {
  const missingItem = liveGraphqlAdapter((req) =>
    graphqlEnvelope(req.body.query, [
      ["addProjectV2ItemById", { data: { addProjectV2ItemById: { item: null } } }],
      [
        "issue(",
        {
          data: {
            repository: { issue: { id: "I_1", projectItems: { totalCount: 0, nodes: [] } } },
          },
        },
      ],
      ["projectV2", PROJECT_OK],
    ]),
  );
  const clearance = clearanceFor(missingItem.adapter);
  assert.throws(
    () =>
      missingItem.adapter.addIssueToProject({
        number: 3,
        issueNumber: 10,
        mode: "apply",
        clearance,
      }),
    (error) => error.name === "GitHubAdapterError",
  );
});

test("incomplete Project membership fails closed and complete membership is exact", () => {
  const unknown = liveGraphqlAdapter((req) =>
    graphqlEnvelope(req.body.query, [
      ["addProjectV2ItemById", { data: { addProjectV2ItemById: { item: { id: "PVTI_1" } } } }],
      ["issue(", { data: { repository: { issue: { id: "I_1" } } } }],
      ["projectV2", PROJECT_OK],
    ]),
  );
  assert.throws(
    () =>
      unknown.adapter.addIssueToProject({
        number: 3,
        issueNumber: 10,
        mode: "apply",
        clearance: clearanceFor(unknown.adapter),
      }),
    /membership is incomplete/u,
  );

  const member = liveGraphqlAdapter((req) =>
    graphqlEnvelope(req.body.query, [
      [
        "issue(",
        {
          data: {
            repository: {
              issue: {
                id: "I_1",
                projectItems: {
                  totalCount: 1,
                  nodes: [{ id: "PVTI_3", project: { id: "PVT_3", number: 3 } }],
                },
              },
            },
          },
        },
      ],
      [
        "addProjectV2ItemById",
        { data: { addProjectV2ItemById: { item: { id: "PVTI_existing" } } } },
      ],
      ["projectV2", PROJECT_OK],
    ]),
  );
  const memberApplied = member.adapter.addIssueToProject({
    number: 3,
    issueNumber: 10,
    mode: "apply",
    clearance: clearanceFor(member.adapter),
  });
  assert.equal(memberApplied.applied, true);
  assert.equal(memberApplied.already_member, true);

  const fresh = liveGraphqlAdapter((req) =>
    graphqlEnvelope(req.body.query, [
      [
        "issue(",
        {
          data: {
            repository: {
              issue: { id: "I_1", projectItems: { totalCount: 0, nodes: [] } },
            },
          },
        },
      ],
      ["addProjectV2ItemById", { data: { addProjectV2ItemById: { item: { id: "PVTI_new" } } } }],
      ["projectV2", PROJECT_OK],
    ]),
  );
  const freshApplied = fresh.adapter.addIssueToProject({
    number: 3,
    issueNumber: 10,
    mode: "apply",
    clearance: clearanceFor(fresh.adapter),
  });
  assert.equal(freshApplied.applied, true);
  assert.equal(freshApplied.already_member, false);

  const proven = liveGraphqlAdapter((req) =>
    graphqlEnvelope(req.body.query, [
      ["addProjectV2ItemById", { data: { addProjectV2ItemById: { item: null } } }],
      [
        "issue(",
        {
          data: {
            repository: {
              issue: {
                id: "I_1",
                projectItems: {
                  totalCount: 1,
                  nodes: [{ id: "PVTI_3", project: { id: "PVT_3", number: 3 } }],
                },
              },
            },
          },
        },
      ],
      ["projectV2", PROJECT_OK],
    ]),
  );
  const provenApplied = proven.adapter.addIssueToProject({
    number: 3,
    issueNumber: 10,
    mode: "apply",
    clearance: clearanceFor(proven.adapter),
  });
  assert.equal(provenApplied.applied, true);
  assert.equal(provenApplied.already_member, true);
});

test("REL0-AC2 fake addIssueToProject apply 404s a missing issue", () => {
  const adapter = fake();
  const clearance = clearanceFor(adapter);
  const planned = adapter.addIssueToProject({ number: 3, issueNumber: 99, mode: "check" });
  assert.equal(planned.applied, false);
  assert.throws(
    () => adapter.addIssueToProject({ number: 3, issueNumber: 99, mode: "apply", clearance }),
    NotFoundError,
  );
  assert.deepEqual(adapter.getProjectItems(3), []);
});

test("REL0-AC2 live addIssueToProject is doctor-gated and idempotent on GraphQL", () => {
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
            permissions: { push: true },
          };
        }
        if (req.path === "graphql") {
          const query = req.body.query;
          if (query.includes("addProjectV2ItemById")) {
            return { data: { addProjectV2ItemById: { item: { id: "PVTI_1" } } } };
          }
          if (query.includes("issue(")) {
            return {
              data: {
                repository: {
                  issue: { id: "I_1", projectItems: { totalCount: 0, nodes: [] } },
                },
              },
            };
          }
          return {
            data: {
              organization: {
                projectV2: { id: "PVT_3", number: 3, viewerCanUpdate: true },
              },
              user: { projectV2: null },
            },
          };
        }
        throw new Error(`unexpected ${req.method} ${req.path}`);
      },
    },
  });
  assert.equal(adapter.inspectCapabilities().projects, true);
  const check = adapter.addIssueToProject({ number: 3, issueNumber: 10, mode: "check" });
  assert.equal(check.applied, false);
  assert.equal(
    calls.some(
      (row) => row.path === "graphql" && row.body?.query?.includes("addProjectV2ItemById"),
    ),
    false,
  );
  assert.throws(
    () => adapter.addIssueToProject({ number: 3, issueNumber: 10, mode: "apply" }),
    DoctorRequiredError,
  );
  const applied = adapter.addIssueToProject({
    number: 3,
    issueNumber: 10,
    mode: "apply",
    clearance: clearanceFor(adapter),
  });
  assert.equal(applied.applied, true);
  assert.equal(applied.number, 3);
  assert.equal(applied.issueNumber, 10);
  assert.equal(
    calls.some((row) => row.path === "graphql" && row.body?.query?.includes("projectsV2")),
    false,
  );
  assert.equal(
    calls.some(
      (row) =>
        row.path === "graphql" &&
        row.body?.query?.includes("projectItems(first:100)") &&
        row.body.query.includes("pullRequest(number:"),
    ),
    false,
  );
  assert.equal(
    calls.some(
      (row) => row.path === "graphql" && row.body?.query?.includes("addProjectV2ItemById"),
    ),
    true,
  );
});

test("CLI scheduling fails closed when the mapped issue is missing", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "githubctl-schedule-cli-"));
  const ledgerPath = path.join(dir, "implemented.toml");
  const liveLedger = fs.readFileSync(
    path.join(REPO_ROOT, "roadmap/0.1.0-tama/authority/state/implemented.toml"),
    "utf8",
  );
  const authority = loadAuthority();
  const liveStates = deriveState(authority).states;
  const readyNode = authority.ledger.github_issue
    .map((row) => row.node_id)
    .find((id) => liveStates.get(id)?.status === "READY");
  assert.notEqual(readyNode, undefined, "expected a READY node with a github_issue mapping");
  fs.writeFileSync(
    ledgerPath,
    liveLedger.replace(
      new RegExp(`node_id = "${readyNode}"\\ngh_issue = \\d+`, "u"),
      `node_id = "${readyNode}"\ngh_issue = 10`,
    ),
  );
  const check = spawnSync(
    process.execPath,
    [
      CLI,
      "schedule",
      "--check",
      "--fake",
      "--nodes",
      readyNode,
      "--ledger",
      ledgerPath,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(check.status, 0);
  assert.match(check.stderr, /issue #10 is missing/u);
  const apply = spawnSync(
    process.execPath,
    [
      CLI,
      "schedule",
      "--apply",
      "--fake",
      "--nodes",
      readyNode,
      "--ledger",
      ledgerPath,
      "--owner",
      "pikax",
      "--repo",
      "verter",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(apply.status, 0);
  assert.match(apply.stderr, /issue #10 is missing/u);
  const help = spawnSync(process.execPath, [CLI, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /schedule --check/u);
  assert.doesNotMatch(help.stdout, /createPullRequest/u);
});

test("REL0 contract names the scheduling overlay boundaries", () => {
  const text = fs.readFileSync(CONTRACT, "utf8");
  for (const name of ["MilestoneOverlay", "ReadySchedulingPlan", "ReleaseTarget"]) {
    assert.match(text, new RegExp(`^## ${name}$`, "mu"), `missing heading ${name}`);
  }
  assert.match(text, /Project 3/u);
  assert.doesNotMatch(text, /Project Status as authority/iu);
});
