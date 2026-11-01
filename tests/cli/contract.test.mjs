import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { validate } from "./validate.mjs";
const contract = JSON.parse(
  readFileSync(new URL("../../docs/arch/cli/command-contract.v1.json", import.meta.url)),
);
test("command contract has complete, uniquely bound migration population", () =>
  validate(contract));
test("ownership rejects omissions, unknown/pathless owners and conflicting assignments", () => {
  for (const mutate of [
    (c) => c.inventory.pop(),
    (c) => {
      c.inventory[0].owner = "unknown";
    },
    (c) => {
      c.owners[0].path = [];
    },
    (c) => c.inventory.push({ ...c.inventory[0], owner: "conflicting-owner" }),
    (c) => {
      c.owners[0].acceptance = "missing";
    },
  ]) {
    const candidate = structuredClone(contract);
    mutate(candidate);
    assert.throws(() => validate(candidate));
  }
});

// The release workflow ships this executable separately from verter-mcp-server.
test("shipped native MCP entry has a cutover owner and cannot leave the population", () => {
  const route = contract.inventory.find((row) => row.id === "native-mcp-shipped");
  assert.equal(route?.source, "crates/verter_mcp/src/main.rs");
  assert.equal(route.owner, "CLI5");
  assert.equal(route.successor, "packages/verter/bin");
  const candidate = structuredClone(contract);
  candidate.inventory = candidate.inventory.filter((row) => row.id !== route.id);
  assert.throws(() => validate(candidate), /incomplete or conflicting inventory/);
});
