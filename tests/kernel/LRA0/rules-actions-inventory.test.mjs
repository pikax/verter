// The rule/action inventory is the contract a later action-model or edit
// cutover reads to learn which routes it must retire. A code-action producer
// missing from it survives those cutovers with no exact source or map basis.
// This reads contract data only; general ownership validation has its own home.
import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("../../../", import.meta.url));
const load = () =>
  JSON.parse(
    fs.readFileSync(new URL("./products/rules-actions-inventory.v1.json", import.meta.url), "utf8"),
  );

// What handle_code_action_attempt calls today, with the kind each answers.
const HANDLER_PRODUCERS = new Map([
  ["organize_imports_actions", "source.organizeImports"],
  ["extract_component_action", "refactor.extract"],
  ["macro_code_actions", "quickfix"],
  ["component_code_actions", "quickfix"],
  ["suggest_matching_props", "quickfix"],
  ["event_type_hint_actions", "quickfix"],
  ["merge_code_actions", "quickfix"],
  ["action_engine_fixes", "quickfix"],
  ["action_engine_refactorings", "refactor"],
]);

function rowErrors(row, owner, contractNode) {
  const errors = [];
  const path = row.successorPath ?? [];
  if (path[0] !== contractNode || path.at(-1) !== owner) {
    errors.push(`${row.id}: successor path does not run from ${contractNode} to ${owner}`);
  }
  if (!row.receivingAcceptance?.startsWith(`${owner}-`)) {
    errors.push(`${row.id}: acceptance ${row.receivingAcceptance} is not ${owner}'s`);
  }
  if (!row.source || !fs.existsSync(ROOT + row.source)) {
    errors.push(`${row.id}: source ${row.source} is not in the tree`);
  }
  return errors;
}

function auditErrors(inventory) {
  const audit = inventory.codeActionRouteAudit;
  if (!audit) return ["no code-action route audit"];
  const consumers = new Map(inventory.consumers.map((row) => [row.id, row]));
  const routes = new Map(inventory.displacedRoutes.map((row) => [row.id, row]));
  const errors = [];
  const listed = new Map(audit.producers.map((entry) => [entry.producer, entry]));
  for (const [producer, kind] of HANDLER_PRODUCERS) {
    const entry = listed.get(producer);
    if (!entry) {
      errors.push(`${producer}: not audited`);
      continue;
    }
    if (entry.requestKind !== kind) errors.push(`${producer}: answers ${kind}`);
    const consumer = consumers.get(entry.consumer);
    if (consumer) {
      errors.push(...rowErrors(consumer, consumer.implementationOwner, inventory.contractNode));
    } else {
      errors.push(`${producer}: no consumer row ${entry.consumer}`);
    }
    const bound = entry.routes.map((id) => routes.get(id));
    for (const [index, row] of bound.entries()) {
      if (row) errors.push(...rowErrors(row, row.deletionOwner, inventory.contractNode));
      else errors.push(`${producer}: no displaced route ${entry.routes[index]}`);
    }
    if (entry.reach === "direct") {
      const owners = bound
        .map((row) => row?.deletionOwner)
        .sort()
        .join();
      if (owners !== "ACT1,LSO8") {
        errors.push(`${producer}: direct route needs one ACT1 and one LSO8 row, has ${owners}`);
      }
    } else if (entry.reach !== "bridged") {
      errors.push(`${producer}: unknown reach ${entry.reach}`);
    }
  }
  for (const producer of listed.keys()) {
    if (!HANDLER_PRODUCERS.has(producer)) errors.push(`${producer}: not a handler producer`);
  }
  return errors;
}

const removeRow = (table, id) => (inventory) => {
  inventory[table] = inventory[table].filter((row) => row.id !== id);
};
const editRow = (table, id, patch) => (inventory) => {
  Object.assign(
    inventory[table].find((row) => row.id === id),
    patch,
  );
};

test("every producer the code-action handler calls is bound to its owners", () => {
  assert.deepEqual(auditErrors(load()), []);
});

test("extract-to-component binds the action model to ACT1 and its edits to LSO8", () => {
  const inventory = load();
  const entry = inventory.codeActionRouteAudit.producers.find(
    (candidate) => candidate.producer === "extract_component_action",
  );
  const owners = Object.fromEntries(
    entry.routes.map((id) => {
      const row = inventory.displacedRoutes.find((candidate) => candidate.id === id);
      assert.equal(row.source, "crates/verter_lsp/src/features/extract_component.rs");
      return [row.deletionOwner, row.receivingAcceptance];
    }),
  );
  assert.deepEqual(owners, { ACT1: "ACT1-AC1", LSO8: "LSO8-AC1" });
});

for (const [name, mutate, expected] of [
  [
    "an inventory with no route audit",
    (inventory) => {
      delete inventory.codeActionRouteAudit;
    },
    /no code-action route audit/,
  ],
  [
    "a producer dropped from the audit",
    (inventory) => {
      inventory.codeActionRouteAudit.producers = inventory.codeActionRouteAudit.producers.filter(
        (entry) => entry.producer !== "extract_component_action",
      );
    },
    /extract_component_action: not audited/,
  ],
  ["a missing extract consumer row", removeRow("consumers", "LRA-C15"), /no consumer row LRA-C15/],
  [
    "a missing extract model row",
    removeRow("displacedRoutes", "LRA-D14"),
    /no displaced route LRA-D14/,
  ],
  [
    "a missing extract edit row",
    removeRow("displacedRoutes", "LRA-D15"),
    /no displaced route LRA-D15/,
  ],
  [
    "extract edits handed to the model owner",
    editRow("displacedRoutes", "LRA-D15", {
      deletionOwner: "ACT1",
      successorPath: ["LRA0", "LSO8", "ACT1"],
      receivingAcceptance: "ACT1-AC1",
    }),
    /extract_component_action: direct route needs one ACT1 and one LSO8 row/,
  ],
  [
    "an owner whose path does not reach it",
    editRow("displacedRoutes", "LRA-D15", { successorPath: ["LRA0", "ACT1"] }),
    /LRA-D15: successor path does not run from LRA0 to LSO8/,
  ],
  [
    "an acceptance another owner holds",
    editRow("displacedRoutes", "LRA-D14", { receivingAcceptance: "LSO8-AC1" }),
    /LRA-D14: acceptance LSO8-AC1 is not ACT1's/,
  ],
  [
    "a source path outside the tree",
    editRow("consumers", "LRA-C15", { source: "crates/verter_lsp/src/features/absent.rs" }),
    /LRA-C15: source .* is not in the tree/,
  ],
]) {
  test(`the route audit rejects ${name}`, () => {
    const dirty = load();
    mutate(dirty);
    const errors = auditErrors(dirty);
    assert.ok(
      errors.some((error) => expected.test(error)),
      JSON.stringify(errors),
    );
  });
}
