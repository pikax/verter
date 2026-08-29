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
  validateFindingCarryForward,
  validateSchemaObject,
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

  authority.ledger.github_issue = [{ node_id: "REL1", gh_issue: 123, sync_to_github: false }];
  assert.equal(deriveState(authority).states.get("REL1").status, "READY");
  assert.notEqual(deriveState(authority).states.get("REL1").status, "COMPLETE");
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

test("the live ledger records J1, ORC0, GH0-GH5, REL0, FB0, FB1, and FB2 with message/date locators", () => {
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
  assert.deepEqual(byId.get("GH3"), {
    node_id: "GH3",
    commit_message: "feat(ci): create final-title pull requests that close mapped issues",
    commit_date: "2026-08-29T02:28:30+01:00",
  });
  assert.deepEqual(byId.get("GH4"), {
    node_id: "GH4",
    commit_message: "feat(ci): record review history in ordinary pull-request prose",
    commit_date: "2026-08-29T02:41:41+01:00",
  });
  assert.deepEqual(byId.get("GH5"), {
    node_id: "GH5",
    commit_message: "feat(ci): report pull-request checks and squash-land through GitHub",
    commit_date: "2026-08-29T03:25:00+01:00",
  });
  assert.deepEqual(byId.get("REL0"), {
    node_id: "REL0",
    commit_message: "feat(ci): overlay READY work onto GitHub Project 3",
    commit_date: "2026-08-29T01:22:51+01:00",
  });
  assert.deepEqual(byId.get("FB0"), {
    node_id: "FB0",
    commit_message: "docs(ci): define non-DAG feedback labels and finding follow-up",
    commit_date: "2026-08-29T03:45:51+01:00",
  });
  assert.deepEqual(byId.get("FB1"), {
    node_id: "FB1",
    commit_message: "feat(ci): inspect GitHub issues and write local feedback reports",
    commit_date: "2026-08-29T04:14:40+01:00",
  });
  assert.deepEqual(byId.get("FB2"), {
    node_id: "FB2",
    commit_message: "docs(ci): require manual patches to map existing issues into the DAG",
    commit_date: "2026-08-29T04:55:09+01:00",
  });
  const state = deriveState(authority);
  assert.equal(state.states.get("GH0").status, "COMPLETE");
  assert.equal(state.states.get("ORC0").status, "COMPLETE");
  assert.equal(state.states.get("GH1").status, "COMPLETE");
  assert.equal(state.states.get("GH2").status, "COMPLETE");
  assert.equal(state.states.get("GH3").status, "COMPLETE");
  assert.equal(state.states.get("GH4").status, "COMPLETE");
  assert.equal(state.states.get("GH5").status, "COMPLETE");
  assert.equal(state.states.get("FB0").status, "COMPLETE");
  assert.equal(state.states.get("FB1").status, "COMPLETE");
  assert.equal(state.states.get("FB2").status, "COMPLETE");
  assert.notEqual(state.states.get("GH6").status, "COMPLETE");
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
  for (const syncToGithub of [true, false]) {
    const authority = smallAuthority([{ node_id: "ORC0" }, { node_id: "GH0" }]);
    assert.equal(deriveState(authority).states.get("GH1").status, "READY");
    authority.ledger.github_issue = [
      { node_id: "GH1", gh_issue: 999, sync_to_github: syncToGithub },
    ];
    assert.equal(
      deriveState(authority).states.get("GH1").status,
      "READY",
      `sync_to_github=${syncToGithub}`,
    );
    assert.notEqual(
      deriveState(authority).states.get("GH1").status,
      "COMPLETE",
      `sync_to_github=${syncToGithub}`,
    );
  }
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
    "ReviewCycleSummary",
    "CiResult",
    "GitHubIssueSync",
    "GitHubAdapter",
    "GitHubDoctor",
    "FakeGitHubAdapter",
    "ReadySchedulingPlan",
    "MilestoneOverlay",
    "ReleaseTarget",
    "AiIssueVerdict",
    "AiOwnedLabels",
    "MaintainerGuards",
    "FeedbackReport",
    "FindingCarryForward",
    "ManualDagAuthoring",
  ]) {
    assert.match(text, new RegExp(`^## ${name}$`, "mu"), `missing heading ${name}`);
  }
  assert.match(text, /GitHubIssueMapping` identity is exactly `\{node_id, gh_issue\}`/u);
  assert.doesNotMatch(text, /identity is exactly `\{node_id, gh_issue, sync_to_github\}`/u);
});

const AI_ISSUE_VERDICTS = ["unchecked", "confirmed", "rejected", "fixed", "needs-human"];
const AI_OWNED_LABELS = AI_ISSUE_VERDICTS.map((verdict) => `ai:${verdict}`);
const FORBIDDEN_FEEDBACK_LABELS = [
  "ai:checked",
  "dag:ready",
  "dag:complete",
  "dag:blocked",
  "dag:GH0",
  "dag:implemented",
];
const FORBIDDEN_FINDING_FIELDS = [
  "dag_id",
  "node_id",
  "predecessors",
  "closed",
  "labels",
  "ready",
  "implemented",
  "status",
  "train",
  "pull_request",
];

function controlPlaneContract() {
  return fs.readFileSync(path.join(PACKAGE_ROOT, "contracts", "github-control-plane.md"), "utf8");
}

function contractSection(text, name) {
  const heading = `## ${name}`;
  const start = text.indexOf(`${heading}\n`);
  assert.notEqual(start, -1, `missing heading ${name}`);
  const from = start + heading.length + 1;
  const next = text.indexOf("\n## ", from);
  return next === -1 ? text.slice(from) : text.slice(from, next);
}

function tickTokens(section) {
  return [...section.matchAll(/`([^`]+)`/gu)].map((match) => match[1]);
}

function findingSchema() {
  return JSON.parse(
    fs.readFileSync(
      path.join(PACKAGE_ROOT, "schemas", "finding-carry-forward.schema.json"),
      "utf8",
    ),
  );
}

test("AiOwnedLabels is the closed mutually exclusive AI-result set and rejects forbidden labels", () => {
  const text = controlPlaneContract();
  const ownedTokens = tickTokens(contractSection(text, "AiOwnedLabels"));
  const ownedLabels = [...new Set(ownedTokens.filter((token) => token.startsWith("ai:")))].sort();
  assert.deepEqual(ownedLabels, [...AI_OWNED_LABELS].sort());
  for (const label of FORBIDDEN_FEEDBACK_LABELS) {
    assert.equal(ownedLabels.includes(label), false, `${label} must not be AI-owned`);
  }
  assert.equal(ownedLabels.includes("ai:ignore"), false);
  assert.equal(new Set(AI_OWNED_LABELS).size, AI_ISSUE_VERDICTS.length);

  const verdictTokens = tickTokens(contractSection(text, "AiIssueVerdict"));
  for (const verdict of AI_ISSUE_VERDICTS) {
    assert.equal(verdictTokens.includes(verdict), true, `missing verdict ${verdict}`);
  }
  assert.match(contractSection(text, "AiIssueVerdict"), /`ai:checked` is rejected/u);
  assert.equal(verdictTokens.includes("checked"), false);
  assert.match(contractSection(text, "AiIssueVerdict"), /at most one AI-result label/u);

  const guardTokens = tickTokens(contractSection(text, "MaintainerGuards"));
  assert.equal(guardTokens.includes("ai:ignore"), true);
  assert.match(
    contractSection(text, "MaintainerGuards"),
    /AI cannot create, remove, or override `ai:ignore`/u,
  );
  assert.match(contractSection(text, "MaintainerGuards"), /never inferred from/u);
  assert.equal(guardTokens.includes("ai:checked"), false);

  assert.match(text, /`dag:\*` labels are forbidden/u);
  assert.match(
    contractSection(text, "FindingCarryForward"),
    /Issue closure is not finding resolution/u,
  );
  assert.match(contractSection(text, "FindingCarryForward"), /P0 and P1 remain blocking/u);
  assert.match(contractSection(text, "FeedbackReport"), /\.feedback\/issues\/<issue-number>\.md/u);
});

test("FindingCarryForward schema requires issue, severity, and owner and rejects DAG fields", () => {
  const schema = findingSchema();
  assert.equal(schema.additionalProperties, false);
  assert.deepEqual(schema.required, ["issue", "severity", "owner"]);
  for (const field of FORBIDDEN_FINDING_FIELDS) {
    assert.equal(
      Object.hasOwn(schema.properties, field),
      false,
      `schema must not declare ${field}`,
    );
  }

  const validUrl = {
    issue: "https://github.com/verter-org/verter/issues/12",
    severity: "P2",
    owner: "reviewer",
  };
  const validId = { issue: "12", severity: "P3", owner: "alice" };
  assert.deepEqual(validateFindingCarryForward(validUrl), []);
  assert.deepEqual(validateFindingCarryForward(validId), []);
  assert.deepEqual(validateSchemaObject(validUrl, schema, "finding"), []);

  for (const field of FORBIDDEN_FINDING_FIELDS) {
    const errors = validateFindingCarryForward({ ...validUrl, [field]: true });
    assert.ok(
      errors.some((error) => error.includes(`additional property ${field}`)),
      `${field} must be structurally rejected, got: ${errors.join("; ")}`,
    );
  }

  assert.ok(
    validateFindingCarryForward({ severity: "P2", owner: "reviewer" }).some((error) =>
      error.includes("missing required property issue"),
    ),
  );
  assert.ok(
    validateFindingCarryForward({ ...validUrl, severity: "blocking" }).some((error) =>
      error.includes("expected one of"),
    ),
  );
  assert.ok(
    validateFindingCarryForward({ ...validUrl, issue: "dag:GH0" }).some((error) =>
      error.includes("does not match"),
    ),
  );
  assert.ok(
    validateFindingCarryForward({ ...validUrl, issue: "0" }).some((error) =>
      error.includes("does not match"),
    ),
  );
  assert.ok(
    validateFindingCarryForward({ ...validUrl, closed: true }).some((error) =>
      error.includes("additional property closed"),
    ),
  );
});

const REPO_ROOT = path.resolve(PACKAGE_ROOT, "../..");
const GITHUBCTL = path.join(REPO_ROOT, "scripts", "githubctl", "githubctl.mjs");
const PROGRAMCTL = path.join(PACKAGE_ROOT, "tools", "programctl.mjs");
const GITHUBCTL_COMMANDS =
  "doctor, check, inspect, sync-issues, create-pr, review-summary, ci-result, finalize-ledger, squash-land, schedule";

test("FB2-AC1 githubctl has no import-dag command and does not generate DAG authority from GitHub", () => {
  const help = spawnSync(process.execPath, [GITHUBCTL, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0, help.stderr);
  assert.doesNotMatch(help.stdout, /\bimport-dag\b/u);
  assert.match(help.stdout, /never imports GitHub edits/u);

  const imported = spawnSync(process.execPath, [GITHUBCTL, "import-dag"], { encoding: "utf8" });
  assert.notEqual(imported.status, 0);
  assert.match(imported.stderr, /unknown command import-dag/u);
  assert.match(imported.stderr, new RegExp(`supported commands: ${GITHUBCTL_COMMANDS}`, "u"));

  const programImported = spawnSync(process.execPath, [PROGRAMCTL, "import-dag"], {
    encoding: "utf8",
  });
  assert.notEqual(programImported.status, 0);
  assert.match(programImported.stderr, /unknown command import-dag/u);

  const githubctlSource = fs.readFileSync(GITHUBCTL, "utf8");
  assert.match(githubctlSource, new RegExp(`supported commands: ${GITHUBCTL_COMMANDS}`, "u"));
  assert.doesNotMatch(githubctlSource, /\bimport-dag\b/u);
});

test("FB2-AC2 ManualDagAuthoring reuses the original issue and never marks the node implemented", () => {
  const section = contractSection(controlPlaneContract(), "ManualDagAuthoring");
  assert.match(section, /`\[\[github_issue\]\]`/u);
  assert.match(section, /original issue number/u);
  assert.match(section, /`sync_to_github = false`/u);
  assert.match(section, /does not mark the node implemented/u);
  assert.match(section, /no `import-dag` command/u);
  assert.match(section, /never updates a protected pre-existing issue/u);
  assert.match(section, /duplicate `gh_issue`/u);
  assert.match(section, /Issue closure cannot/u);
  assert.match(section, /P0\/P1/u);
  assert.match(section, /`dag:\*` label/u);

  const authority = loadAuthority();
  authority.ledger.github_issue = [
    ...(authority.ledger.github_issue || []),
    { node_id: "REL1", gh_issue: 4242, sync_to_github: false },
  ];
  assert.deepEqual(validateAuthority(authority), []);
  assert.equal(deriveState(authority).states.get("REL1").status, "READY");
  assert.notEqual(deriveState(authority).states.get("REL1").status, "COMPLETE");
});
