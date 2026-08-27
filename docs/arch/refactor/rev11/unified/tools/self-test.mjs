import assert from "node:assert/strict";
import { parseToml, validateGraphModel } from "./lib.mjs";

const parsed = parseToml('schema = 4\n[[node]]\nid = "A"\npredecessors = []\n');
assert.equal(parsed.schema, 4);
assert.equal(parsed.node[0].id, "A");

const baseNode = {
  id: "A",
  name: "root",
  predecessors: [],
  conditional_predecessors: [],
  phase: "test",
  train: "test.root",
  product: "test",
  kind: "implementation",
  semantic_role: "delivery",
  class: "foundational",
  owner: "test-owner",
  conflict_domains: ["test_surface"],
  resource_class: "light",
  gate_profile: "targeted",
  review_profile: "semantic-3",
  dispatchable: true,
  optional: false,
  release_gating: "none",
  source_refs: ["test:1"],
  external_requirements: [],
  charter: "charters/test/A.md",
  size: "S",
  max_production_loc: 800,
  max_production_files: 8,
  max_related_packages: 2,
  rescope_loc: 1500,
  rescope_files: 12,
  rescope_unrelated_packages: 3,
  activation_gate: "ORC0",
};

const syntheticOptions = { skipCharters: true, skipProposalGroups: true };
assert.deepEqual(validateGraphModel([baseNode], syntheticOptions), []);
assert.match(
  validateGraphModel([baseNode, { ...baseNode }], syntheticOptions).join("\n"),
  /duplicate node id/,
);
assert.match(
  validateGraphModel([{ ...baseNode, predecessors: ["MISSING"] }], syntheticOptions).join("\n"),
  /missing predecessor/,
);

console.log("self-test: PASS");
