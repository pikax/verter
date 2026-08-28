/** @ai-generated - Post-activation GitHub control-plane authority guards. */
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import * as lib from "./lib.mjs";

const EXACT_PREDECESSORS = new Map([
  ["GH0", ["ORC0"]],
  ["GH1", ["GH0"]],
  ["GH2", ["GH1"]],
  ["GH3", ["GH2"]],
  ["GH4", ["GH3"]],
  ["GH5", ["GH4"]],
  ["FB0", ["GH0"]],
  ["FB1", ["GH1", "FB0"]],
  ["FB2", ["FB1", "GH2"]],
  ["REL0", ["GH2"]],
  ["REL1", ["REL0", "GH5"]],
  ["REL2", ["REL1"]],
  ["GH6", ["GH2", "GH5", "FB2", "REL2"]],
]);

const EXPECTED_MODULES = [
  "dag/governance-feedback-intake.toml",
  "dag/governance-github-control-plane.toml",
  "dag/governance-release-control.toml",
];

test("GitHub control-plane trains preserve the exact post-ORC0 topology", () => {
  const authority = lib.loadAuthority();
  const byId = new Map(authority.nodes.map((node) => [node.id, node]));

  for (const module of EXPECTED_MODULES)
    assert.ok(authority.metadata.modules.includes(module), `${module} is registered`);
  const controlledTrains = new Set([
    "governance.github-control-plane",
    "governance.feedback-intake",
    "governance.release-control",
  ]);
  assert.deepEqual(
    authority.nodes
      .filter((node) => controlledTrains.has(node.train))
      .map((node) => node.id)
      .sort(),
    [...EXACT_PREDECESSORS.keys()].sort(),
    "the source-derived governance train contains no invented or omitted nodes",
  );
  for (const [id, predecessors] of EXACT_PREDECESSORS) {
    const node = byId.get(id);
    assert.ok(node, `${id} exists`);
    assert.deepEqual(node.predecessors, predecessors, `${id} predecessor order is exact`);
    assert.equal(node.activation_gate, "ORC0", `${id} remains activation-gated`);
    assert.deepEqual(node.source_refs, ["source:github-control-plane-program.md:L1"]);
  }
  assert.equal(byId.get("GH6").semantic_role, "convergence");
  assert.deepEqual(lib.validateGitHubControlPlaneRegistration(authority), []);
});

test("GitHub authority contract keeps lifecycle truth outside mutable GitHub state", () => {
  const authority = lib.loadAuthority();
  const contract = fs.readFileSync(
    path.join(authority.packageRoot, "contracts/github-control-plane.md"),
    "utf8",
  );

  assert.match(contract, /static Rev11 DAG, charters, and contracts are architecture authority/u);
  assert.match(
    contract,
    /immutable `programctl` receipts are lifecycle and correctness authority/u,
  );
  assert.match(contract, /GitHub closure.*cannot satisfy a DAG predecessor/isu);
  assert.match(contract, /`landed_tree == validated_integration_tree`/u);
  assert.match(
    contract,
    /binding on every acceptance in this program: GH0–GH6, FB0–FB2, and REL0–REL2/u,
  );
  assert.match(contract, /No finding is lost across acceptance\./u);
  assert.match(contract, /P0\/P1 remain non-dispositionable blockers\./u);
  assert.match(contract, /uniquely fingerprinted carry-forward obligation/u);
  assert.match(contract, /immutable resolution receipt supersedes it/u);
  assert.match(
    contract,
    /Mutable GitHub issue state, labels, milestone movement, or closure cannot erase the obligation\./u,
  );
  assert.match(
    contract,
    /Repeated carry-forward requires escalating authorization and is never implicit\./u,
  );
});

test("every GitHub control-plane charter binds activation currency and finding retention", () => {
  const authority = lib.loadAuthority();
  const byId = new Map(authority.nodes.map((node) => [node.id, node]));

  for (const id of EXACT_PREDECESSORS.keys()) {
    const charter = fs.readFileSync(path.join(authority.packageRoot, byId.get(id).charter), "utf8");
    assert.match(
      charter,
      /current accepted ORC0 activation receipt/u,
      `${id} binds current ORC0 receipt`,
    );
    assert.match(charter, /P0\/P1/u, `${id} keeps high-severity blockers`);
    assert.match(charter, /carry-forward/u, `${id} carries the finding-retention law`);
    assert.match(charter, /mutable GitHub/u, `${id} rejects mutable GitHub authority`);
  }
});
