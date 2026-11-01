import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { releaseWorkflowText, validate } from "./validate.mjs";
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

test("a wrapper cannot be retired by the node that creates it", () => {
  const candidate = structuredClone(contract);
  const route = candidate.inventory.find((row) => row.converter);
  route.owner = route.converter;
  assert.throws(() => validate(candidate), /retired by the node that creates it/);
});

test("a release-built native entry without an inventory row is rejected", () => {
  const added = `${releaseWorkflowText()}\n  run: cargo build --release --package verter_new_tool\n`;
  assert.throws(() => validate(contract, added), /crates\/verter_new_tool\/src\/main\.rs/);
  assert.throws(() => validate(contract, "steps: []"), /no native build/);
});
