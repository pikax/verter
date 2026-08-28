import assert from "node:assert/strict";
import path from "node:path";
import test from "node:test";
import {
  PACKAGE_ROOT,
  deriveState,
  explainNode,
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

  authority.ledger.github_issue = [{ node_id: "GH0", gh_issue: 123, sync_to_github: false }];
  assert.equal(deriveState(authority).states.get("GH0").status, "READY");
});

test("the live ledger records J1 and ORC0 with message/date locators", () => {
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
  const state = deriveState(authority);
  assert.equal(state.states.get("GH0").status, "READY");
  assert.equal(state.states.get("ORC0").status, "COMPLETE");
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
