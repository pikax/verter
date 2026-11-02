import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { releaseWorkflowText, validate } from "./validate.mjs";
const schema = JSON.parse(
  readFileSync(new URL("../../docs/arch/cli/ownership-schema.v1.json", import.meta.url)),
);
const contract = JSON.parse(
  readFileSync(new URL("../../docs/arch/cli/command-contract.v1.json", import.meta.url)),
);
test("command contract has complete, uniquely bound migration population", () =>
  validate(contract));
const route = (c, id) => c.inventory.find((row) => row.id === id);
// Both reviewed files moved together: only the checks below the population pin can object.
const pinsFor = (c) => ({
  ...schema,
  inventory: c.inventory.map(({ id, owner, kind, source, converter }) => ({
    id,
    owner,
    kind,
    source,
    ...(converter && { converter }),
  })),
});
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
    (c) => {
      route(c, "tsc-wrapper").source = "packages/does-not-exist/run.js";
    },
    (c) => {
      route(c, "tsc-wrapper").source = " ";
    },
    (c) => {
      route(c, "tsc-wrapper").source = route(c, "lsp-wrapper").source;
      route(c, "lsp-wrapper").source = contract.inventory.find(
        (row) => row.id === "tsc-wrapper",
      ).source;
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
  for (const spelling of [
    "cargo build --release --package=verter_new_tool",
    "cargo build --release \\\n          --package verter_new_tool",
    "cargo build --release \\\r\n          --package verter_new_tool",
    "cross build --release -p verter_new_tool",
  ]) {
    assert.throws(
      () =>
        validate(
          contract,
          `${releaseWorkflowText()}
  run: ${spelling}
`,
        ),
      /crates\/verter_new_tool\/src\/main\.rs/,
      spelling,
    );
  }
  const commented = `${releaseWorkflowText()}
  # cargo build --release -p verter_new_tool
`;
  assert.doesNotThrow(() => validate(contract, commented));
});

test("a retirement route keeps a live production path and its release entry", () => {
  const missing = structuredClone(contract);
  route(missing, "tsc-wrapper").source = "packages/does-not-exist/run.js";
  assert.throws(
    () => validate(missing, releaseWorkflowText(), pinsFor(missing)),
    /tsc-wrapper: retirement target does not exist/,
  );

  const moved = structuredClone(contract);
  route(moved, "lapce-native-client").source = route(moved, "native-lsp-shipped").source;
  route(moved, "native-lsp-shipped").source = "crates/verter_lsp/src/lib.rs";
  assert.throws(
    () => validate(moved, releaseWorkflowText(), pinsFor(moved)),
    /release-built native entry has no inventory row: crates\/verter_lsp\/src\/main\.rs/,
  );
});
