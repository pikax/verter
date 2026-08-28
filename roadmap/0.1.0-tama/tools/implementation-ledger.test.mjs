import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import {
  PACKAGE_ROOT,
  deriveState,
  explainNode,
  githubIssueByNumber,
  listGitHubIssues,
  loadAuthority,
  packetFor,
  readToml,
  validateAuthority,
  validateGitHubProgramCatalog,
} from "./lib.mjs";

function smallAuthority(implemented = []) {
  const node = (id, predecessors = []) => ({
    id,
    name: id,
    predecessors,
    dispatchable: true,
  });
  return {
    nodes: [node("ORC0"), node("GH0", ["ORC0"]), node("GH1", ["GH0"])],
    ledger: { implemented },
  };
}

test("frontier is derived only from implemented ancestors", () => {
  const initial = deriveState(smallAuthority());
  assert.equal(initial.states.get("ORC0").status, "READY");
  assert.equal(initial.states.get("GH0").status, "BLOCKED");

  const afterOrc0 = deriveState(smallAuthority([{ node_id: "ORC0" }]));
  assert.equal(afterOrc0.states.get("ORC0").status, "COMPLETE");
  assert.equal(afterOrc0.states.get("GH0").status, "READY");
  assert.equal(afterOrc0.states.get("GH1").status, "BLOCKED");
});

test("locator hints are required strings but are not matched to Git", () => {
  const authority = loadAuthority();
  authority.ledger.implemented[0].commit_message = "a deliberately inexact search hint";
  authority.ledger.implemented[0].commit_date = "2026-08-28T18:00:00+01:00";
  assert.deepEqual(validateAuthority(authority), []);

  authority.ledger.implemented[0].commit_date = "2026-08-28";
  assert.ok(validateAuthority(authority).some((error) => error.includes("commit_date")));
});

test("GitHub mappings are unique and never mark implementation complete", () => {
  const authority = loadAuthority();
  authority.ledger.github_issue = [
    { node_id: "GH0", gh_issue: 123, sync_to_github: true },
    { node_id: "GH0", gh_issue: 124, sync_to_github: false },
    { node_id: "GH1", gh_issue: 123, sync_to_github: true },
  ];
  const errors = validateAuthority(authority);
  assert.ok(errors.includes("GitHub issue ledger: duplicate node GH0"));
  assert.ok(errors.includes("GitHub issue ledger: duplicate issue 123"));

  authority.ledger.github_issue = [{ node_id: "GH3", gh_issue: 123, sync_to_github: false }];
  assert.equal(deriveState(authority).states.get("GH3").status, "READY");
  assert.notEqual(deriveState(authority).states.get("GH3").status, "COMPLETE");
});

test("same node and issue with opposite sync_to_github is a duplicate pair", () => {
  const authority = loadAuthority();
  authority.ledger.github_issue = [
    { node_id: "D1", gh_issue: 99, sync_to_github: true },
    { node_id: "D1", gh_issue: 99, sync_to_github: false },
  ];
  const errors = validateAuthority(authority);
  assert.ok(errors.includes("GitHub issue ledger: duplicate node D1"), errors.join("; "));
  assert.ok(errors.includes("GitHub issue ledger: duplicate issue 99"), errors.join("; "));
});

test("the live ledger records J1, ORC0, GH0, GH1, and GH2 with message/date locators", () => {
  const authority = loadAuthority();
  const byId = new Map(authority.ledger.implemented.map((row) => [row.node_id, row]));
  assert.deepEqual(byId.get("J1"), {
    node_id: "J1",
    commit_message: "refactor(core): cut CSS public routes over to StyleSyntaxIr",
    commit_date: "2026-08-28T16:41:34+01:00",
  });
  assert.deepEqual(byId.get("ORC0"), {
    node_id: "ORC0",
    commit_message: "fix(orchestration): project trusted successor landings",
    commit_date: "2026-08-28T13:06:16+01:00",
  });
  assert.deepEqual(byId.get("GH0"), {
    node_id: "GH0",
    commit_message: "docs(ci): ratify local GitHub issue mapping",
    commit_date: "2026-08-28T21:45:48+01:00",
  });
  assert.deepEqual(byId.get("GH1"), {
    node_id: "GH1",
    commit_message: "feat(ci): add a structured GitHub adapter and deterministic fake",
    commit_date: "2026-08-28T23:08:16+01:00",
  });
  assert.deepEqual(byId.get("GH2"), {
    node_id: "GH2",
    commit_message: "feat(ci): add one-way GitHub issue sync from the local ledger",
    commit_date: "2026-08-29T00:05:33+01:00",
  });
  assert.deepEqual(byId.get("REL0"), {
    node_id: "REL0",
    commit_message: "feat(ci): overlay READY work onto GitHub Project 3",
    commit_date: "2026-08-29T01:22:51+01:00",
  });
  const state = deriveState(authority);
  assert.equal(state.states.get("GH0").status, "COMPLETE");
  assert.equal(state.states.get("ORC0").status, "COMPLETE");
  assert.equal(state.states.get("GH1").status, "COMPLETE");
  assert.equal(state.states.get("GH2").status, "COMPLETE");
  assert.equal(state.states.get("GH3").status, "READY");
  assert.equal(explainNode(authority, state, "ORC0").commit.pull_request, null);
});

test("packets add the trusted row before squash and review", () => {
  const authority = loadAuthority();
  const packet = packetFor(authority, deriveState(authority), "D1");
  assert.match(packet, /Before squashing or starting review/u);
  assert.match(packet, /planned squash commit message/u);
  assert.match(packet, /approximate squash date with timezone/u);
  assert.match(packet, /does not resolve or validate/u);
});

test("strict validation cheaply covers schemas, charters, catalogs, and GitHub nodes", () => {
  const authority = loadAuthority();
  assert.deepEqual(validateAuthority(authority, { strict: true }), []);

  authority.moduleModels[0].model.node[0].semantic_role = "not-a-role";
  authority.nodes[0].semantic_role = "not-a-role";
  assert.ok(
    validateAuthority(authority, { strict: true }).some((error) => error.includes("semantic_role")),
  );

  const catalog = readToml(
    path.join(PACKAGE_ROOT, "catalogs", "github-control-plane-program.toml"),
  );
  catalog.node = catalog.node.filter((row) => row.id !== "GH0");
  assert.ok(
    validateGitHubProgramCatalog(loadAuthority(), catalog).includes(
      "GitHub program catalog: missing node GH0",
    ),
  );
});

const GITHUB_ISSUE_IDENTITY_KEYS = ["node_id", "gh_issue"];
const GITHUB_ISSUE_STORED_FIELDS = [...GITHUB_ISSUE_IDENTITY_KEYS, "sync_to_github"];

function clonedMapping(extra = {}) {
  return { node_id: "GH0", gh_issue: 123, sync_to_github: true, ...extra };
}

function mappingErrors(row) {
  const authority = loadAuthority();
  authority.ledger.github_issue = [row];
  return validateAuthority(authority);
}

test("GitHub issue rows reject DAG metadata and other additional properties", () => {
  const extras = [
    ["predecessors", ["ORC0"]],
    ["effort", "high"],
    ["dag_id", "GH0"],
    ["labels", ["complete"]],
    ["closed", true],
    ["managed_region", "begin"],
    ["marker", "tama-node"],
    ["title", "not identity"],
  ];
  for (const [field, value] of extras) {
    const errors = mappingErrors(clonedMapping({ [field]: value }));
    assert.ok(
      errors.some((error) => error.includes(`additional property ${field}`)),
      `${field} must be structurally rejected, got: ${errors.join("; ")}`,
    );
  }
});

test("GitHub issue rows require sync_to_github", () => {
  const errors = mappingErrors({ node_id: "GH0", gh_issue: 123 });
  assert.ok(
    errors.some((error) => error.includes("missing required property sync_to_github")),
    errors.join("; "),
  );
});

test("a GitHub issue mapping does not complete an unimplemented READY node", () => {
  const authority = smallAuthority([{ node_id: "ORC0" }, { node_id: "GH0" }]);
  assert.equal(deriveState(authority).states.get("GH1").status, "READY");
  authority.ledger.github_issue = [{ node_id: "GH1", gh_issue: 999, sync_to_github: true }];
  assert.equal(deriveState(authority).states.get("GH1").status, "READY");
  assert.notEqual(deriveState(authority).states.get("GH1").status, "COMPLETE");
});

test("GitHub issue mappings reject unknown node_id", () => {
  const errors = mappingErrors({ node_id: "ZZ0", gh_issue: 1, sync_to_github: true });
  assert.ok(errors.includes("GitHub issue ledger: unknown node ZZ0"), errors.join("; "));
});

test("GitHub completeness signals cannot satisfy unimplemented ancestors", () => {
  const authority = smallAuthority();
  authority.ledger.github_issue = [
    {
      node_id: "ORC0",
      gh_issue: 11,
      sync_to_github: true,
      closed: true,
      labels: ["complete"],
      pull_request: 99,
    },
    {
      node_id: "GH0",
      gh_issue: 12,
      sync_to_github: true,
      closed: true,
      labels: ["complete"],
      pull_request: 100,
    },
  ];
  const state = deriveState(authority);
  assert.equal(state.states.get("ORC0").status, "READY");
  assert.equal(state.states.get("GH0").status, "BLOCKED");
  assert.deepEqual(state.states.get("GH0").missing_ancestors, ["ORC0"]);
});

test("sync_to_github is preserved as mutation policy and does not change readiness", () => {
  const base = smallAuthority([{ node_id: "ORC0" }]);
  const ready = deriveState(base).states.get("GH0").status;
  assert.equal(ready, "READY");
  for (const syncToGithub of [true, false]) {
    const authority = smallAuthority([{ node_id: "ORC0" }]);
    authority.ledger.github_issue = [{ node_id: "GH0", gh_issue: 7, sync_to_github: syncToGithub }];
    const live = loadAuthority();
    live.ledger.github_issue = [{ node_id: "GH0", gh_issue: 7, sync_to_github: syncToGithub }];
    assert.deepEqual(validateAuthority(live), []);
    assert.equal(deriveState(authority).states.get("GH0").status, ready);
    assert.equal(listGitHubIssues(authority.ledger)[0].sync_to_github, syncToGithub);
  }
});

test("GitHub issue lookup is bidirectional, sorted, and lists stored fields", () => {
  const ledger = {
    github_issue: [
      { node_id: "GH1", gh_issue: 999, sync_to_github: false, title: "not identity" },
      { node_id: "GH0", gh_issue: 42, sync_to_github: true },
    ],
  };
  const listed = listGitHubIssues(ledger);
  assert.deepEqual(
    listed.map((row) => row.node_id),
    ["GH0", "GH1"],
  );
  for (const row of listed) {
    assert.deepEqual(Object.keys(row), GITHUB_ISSUE_STORED_FIELDS);
  }
  assert.deepEqual(githubIssueByNumber(ledger, 42), {
    node_id: "GH0",
    gh_issue: 42,
    sync_to_github: true,
  });
  assert.deepEqual(githubIssueByNumber(ledger, 999), {
    node_id: "GH1",
    gh_issue: 999,
    sync_to_github: false,
  });
  assert.throws(() => githubIssueByNumber(ledger, 7), /GitHub issue #7 is not mapped/u);
});

test("githubIssueByNumber rejects non-integers as a type error, not as unmapped", () => {
  const ledger = {
    github_issue: [{ node_id: "GH0", gh_issue: 99, sync_to_github: true }],
  };
  const typeError = /GitHub issue lookup requires a positive safe integer/u;
  assert.deepEqual(githubIssueByNumber(ledger, 99), {
    node_id: "GH0",
    gh_issue: 99,
    sync_to_github: true,
  });
  for (const bad of ["99", 0, -1, 1.5]) {
    assert.throws(() => githubIssueByNumber(ledger, bad), typeError);
    assert.throws(
      () => githubIssueByNumber(ledger, bad),
      (error) => !/not mapped/u.test(error.message),
    );
  }
  assert.throws(
    () => githubIssueByNumber({ github_issue: [] }, 999),
    /GitHub issue #999 is not mapped/u,
  );
});

test("programctl github-issue fails closed on an unmapped number", () => {
  const programctl = path.join(PACKAGE_ROOT, "tools", "programctl.mjs");
  const listed = spawnSync(process.execPath, [programctl, "github-issues"], { encoding: "utf8" });
  assert.equal(listed.status, 0, listed.stderr);
  const rows = JSON.parse(listed.stdout);
  assert.equal(Array.isArray(rows), true);
  for (const row of rows) {
    assert.deepEqual(Object.keys(row).sort(), ["gh_issue", "node_id", "sync_to_github"]);
  }
  const missing = spawnSync(process.execPath, [programctl, "github-issue", "999"], {
    encoding: "utf8",
  });
  assert.notEqual(missing.status, 0);
  assert.match(missing.stderr, /GitHub issue #999 is not mapped/u);
});

test("github control plane contract names the mapping boundaries", () => {
  const text = fs.readFileSync(
    path.join(PACKAGE_ROOT, "contracts", "github-control-plane.md"),
    "utf8",
  );
  for (const name of [
    "GitHubIssueMapping",
    "GitHubIssueDescription",
    "ExpectedPullRequestTitle",
    "GitHubIssueSync",
    "GitHubAdapter",
    "GitHubDoctor",
    "FakeGitHubAdapter",
    "ReadySchedulingPlan",
    "MilestoneOverlay",
    "ReleaseTarget",
  ]) {
    assert.match(text, new RegExp(`^## ${name}$`, "mu"), `missing heading ${name}`);
  }
  assert.match(text, /GitHubIssueMapping` identity is exactly `\{node_id, gh_issue\}`/u);
  assert.doesNotMatch(text, /identity is exactly `\{node_id, gh_issue, sync_to_github\}`/u);
});
