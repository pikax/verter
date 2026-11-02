// The rule/action inventory is the contract a later action-model or edit
// cutover reads to learn which routes it must retire. A code-action producer
// missing from it survives those cutovers with no exact source or map basis.
// This reads contract data only; general ownership validation has its own home.
import assert from "node:assert/strict";
import fs from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { matchPathFilters } from "../../../scripts/ci-impact.mjs";

const ROOT = fileURLToPath(new URL("../../../", import.meta.url));
const load = () =>
  JSON.parse(
    fs.readFileSync(new URL("./products/rules-actions-inventory.v1.json", import.meta.url), "utf8"),
  );

// What handle_code_action_attempt calls today. A direct producer builds its own
// transport action, so it binds one model row and one edit row; a bridged
// producer reaches the shared engine. These values are written out here rather
// than derived from the inventory, because the inventory is what is under test.
const HANDLER_PRODUCERS = new Map([
  [
    "organize_imports_actions",
    {
      requestKind: "source.organizeImports",
      reach: "direct",
      consumer: "LRA-C14",
      routes: ["LRA-D12", "LRA-D13"],
    },
  ],
  [
    "extract_component_action",
    {
      requestKind: "refactor.extract",
      reach: "direct",
      consumer: "LRA-C15",
      routes: ["LRA-D14", "LRA-D15"],
    },
  ],
  [
    "macro_code_actions",
    {
      requestKind: "quickfix",
      reach: "direct",
      consumer: "LRA-C11",
      routes: ["LRA-D10", "LRA-D11"],
    },
  ],
  [
    "component_code_actions",
    {
      requestKind: "quickfix",
      reach: "direct",
      consumer: "LRA-C12",
      routes: ["LRA-D10", "LRA-D11"],
    },
  ],
  [
    "suggest_matching_props",
    {
      requestKind: "quickfix",
      reach: "direct",
      consumer: "LRA-C12",
      routes: ["LRA-D10", "LRA-D11"],
    },
  ],
  [
    "event_type_hint_actions",
    {
      requestKind: "quickfix",
      reach: "direct",
      consumer: "LRA-C13",
      routes: ["LRA-D10", "LRA-D11"],
    },
  ],
  [
    "merge_code_actions",
    {
      requestKind: "quickfix",
      reach: "direct",
      consumer: "LRA-C16",
      routes: ["LRA-D16", "LRA-D17"],
    },
  ],
  [
    "action_engine_fixes",
    {
      requestKind: "quickfix",
      reach: "bridged",
      consumer: "LRA-C06",
      routes: ["LRA-D04", "LRA-D05"],
    },
  ],
  [
    "action_engine_refactorings",
    {
      requestKind: "refactor",
      reach: "bridged",
      consumer: "LRA-C06",
      routes: ["LRA-D04", "LRA-D05"],
    },
  ],
]);

const FEATURES = "crates/verter_lsp/src/features";
const PROVIDER_MERGE = "crates/verter_lsp/src/type_provider/merge/feature_merges.rs";
// The model owner runs after the edit owner it consumes.
const MODEL_OWNER = { owner: "ACT1", path: ["LRA0", "LSO8", "ACT1"], acceptance: "ACT1-AC1" };
const EDIT_OWNER = { owner: "LSO8", path: ["LRA0", "LSO8"], acceptance: "LSO8-AC1" };

// Every consumer and displaced route the audit above binds, with its receiving
// owner, the path that reaches that owner, its acceptance and its source.
const BOUND_ROWS = new Map([
  [
    "LRA-C06",
    {
      owner: "LNT3",
      path: ["LRA0", "LNTCFG0", "LNT2", "LNTV0", "LNT3"],
      acceptance: "LNT3-AC2",
      source: `${FEATURES}/diagnostics_bridge.rs`,
    },
  ],
  ["LRA-C11", { ...MODEL_OWNER, source: `${FEATURES}/macro_actions.rs` }],
  ["LRA-C12", { ...MODEL_OWNER, source: `${FEATURES}/component_actions.rs` }],
  ["LRA-C13", { ...MODEL_OWNER, source: `${FEATURES}/event_type_hints.rs` }],
  ["LRA-C14", { ...MODEL_OWNER, source: `${FEATURES}/organize_imports.rs` }],
  ["LRA-C15", { ...MODEL_OWNER, source: `${FEATURES}/extract_component.rs` }],
  ["LRA-C16", { ...MODEL_OWNER, source: PROVIDER_MERGE }],
  ["LRA-D04", { ...MODEL_OWNER, source: "crates/verter_actions/src/engine.rs" }],
  [
    "LRA-D05",
    { ...MODEL_OWNER, acceptance: "ACT1-AC2", source: "crates/verter_actions/src/types.rs" },
  ],
  ["LRA-D10", { ...MODEL_OWNER, source: "crates/verter_lsp/src/server/aux_features.rs" }],
  ["LRA-D11", { ...EDIT_OWNER, source: `${FEATURES}/action_utils.rs` }],
  ["LRA-D12", { ...MODEL_OWNER, source: `${FEATURES}/organize_imports.rs` }],
  ["LRA-D13", { ...EDIT_OWNER, source: `${FEATURES}/organize_imports.rs` }],
  ["LRA-D14", { ...MODEL_OWNER, source: `${FEATURES}/extract_component.rs` }],
  ["LRA-D15", { ...EDIT_OWNER, source: `${FEATURES}/extract_component.rs` }],
  ["LRA-D16", { ...MODEL_OWNER, source: PROVIDER_MERGE }],
  ["LRA-D17", { ...EDIT_OWNER, source: PROVIDER_MERGE }],
]);

function rowErrors(row, owner, expected) {
  const errors = [];
  if (owner !== expected.owner) errors.push(`${row.id}: owned by ${owner}, not ${expected.owner}`);
  if (JSON.stringify(row.successorPath) !== JSON.stringify(expected.path)) {
    errors.push(`${row.id}: successor path is not ${expected.path.join(" -> ")}`);
  }
  if (row.receivingAcceptance !== expected.acceptance) {
    errors.push(`${row.id}: acceptance ${row.receivingAcceptance} is not ${expected.acceptance}`);
  }
  if (row.source !== expected.source) {
    errors.push(`${row.id}: source ${row.source} is not ${expected.source}`);
  }
  if (!row.source || !fs.existsSync(ROOT + row.source)) {
    errors.push(`${row.id}: source ${row.source} is not in the tree`);
  }
  return errors;
}

function auditErrors(inventory) {
  const audit = inventory.codeActionRouteAudit;
  if (!audit) return ["no code-action route audit"];
  const errors = [];
  const indexRows = (table, label) => {
    const index = new Map();
    for (const row of table) {
      if (index.has(row.id)) errors.push(`${row.id}: more than one ${label} row`);
      index.set(row.id, row);
    }
    return index;
  };
  const consumers = indexRows(inventory.consumers, "consumer");
  const routes = indexRows(inventory.displacedRoutes, "displaced route");
  const listed = new Map();
  for (const entry of audit.producers) {
    if (listed.has(entry.producer)) errors.push(`${entry.producer}: audited more than once`);
    listed.set(entry.producer, entry);
  }
  for (const [producer, expected] of HANDLER_PRODUCERS) {
    const entry = listed.get(producer);
    if (!entry) {
      errors.push(`${producer}: not audited`);
      continue;
    }
    if (entry.requestKind !== expected.requestKind) {
      errors.push(`${producer}: answers ${expected.requestKind}, not ${entry.requestKind}`);
    }
    if (entry.reach !== expected.reach) {
      errors.push(`${producer}: reach is ${entry.reach}, not ${expected.reach}`);
    }
    if (entry.consumer !== expected.consumer) {
      errors.push(`${producer}: consumer is ${entry.consumer}, not ${expected.consumer}`);
    }
    if (JSON.stringify(entry.routes) !== JSON.stringify(expected.routes)) {
      errors.push(`${producer}: routes are ${entry.routes}, not ${expected.routes}`);
    }
    if (!consumers.has(expected.consumer)) {
      errors.push(`${producer}: no consumer row ${expected.consumer}`);
    }
    for (const id of expected.routes) {
      if (!routes.has(id)) errors.push(`${producer}: no displaced route ${id}`);
    }
  }
  for (const producer of listed.keys()) {
    if (!HANDLER_PRODUCERS.has(producer)) errors.push(`${producer}: not a handler producer`);
  }
  for (const [id, expected] of BOUND_ROWS) {
    const consumer = consumers.get(id);
    const route = routes.get(id);
    if (consumer) errors.push(...rowErrors(consumer, consumer.implementationOwner, expected));
    if (route) errors.push(...rowErrors(route, route.deletionOwner, expected));
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

test("a change to a bound row's source reaches this test in the script lane", () => {
  for (const { source } of BOUND_ROWS.values()) {
    assert.equal(matchPathFilters([source]).hits.scripts, true, source);
  }
});

const editProducer = (producer, patch) => (inventory) => {
  Object.assign(
    inventory.codeActionRouteAudit.producers.find((entry) => entry.producer === producer),
    patch,
  );
};
const applyAll =
  (...mutations) =>
  (inventory) => {
    for (const mutate of mutations) mutate(inventory);
  };

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
    /LRA-D15: owned by ACT1, not LSO8/,
  ],
  [
    "an owner whose path does not reach it",
    editRow("displacedRoutes", "LRA-D15", { successorPath: ["LRA0", "ACT1"] }),
    /LRA-D15: successor path is not LRA0 -> LSO8/,
  ],
  [
    "a model route that skips the edit owner it consumes",
    editRow("displacedRoutes", "LRA-D14", { successorPath: ["LRA0", "ACT1"] }),
    /LRA-D14: successor path is not LRA0 -> LSO8 -> ACT1/,
  ],
  [
    "an acceptance another owner holds",
    editRow("displacedRoutes", "LRA-D14", { receivingAcceptance: "LSO8-AC1" }),
    /LRA-D14: acceptance LSO8-AC1 is not ACT1-AC1/,
  ],
  [
    "a nonexistent acceptance of the right owner",
    editRow("displacedRoutes", "LRA-D17", { receivingAcceptance: "LSO8-AC9" }),
    /LRA-D17: acceptance LSO8-AC9 is not LSO8-AC1/,
  ],
  [
    "a different acceptance of the provider model owner",
    editRow("displacedRoutes", "LRA-D16", { receivingAcceptance: "ACT1-AC2" }),
    /LRA-D16: acceptance ACT1-AC2 is not ACT1-AC1/,
  ],
  [
    "a different acceptance on an already bound edit route",
    editRow("displacedRoutes", "LRA-D11", { receivingAcceptance: "LSO8-AC2" }),
    /LRA-D11: acceptance LSO8-AC2 is not LSO8-AC1/,
  ],
  [
    "the extract consumer moved to the edit owner",
    editRow("consumers", "LRA-C15", {
      implementationOwner: "LSO8",
      successorPath: ["LRA0", "LSO8"],
      receivingAcceptance: "LSO8-AC1",
    }),
    /LRA-C15: owned by LSO8, not ACT1/,
  ],
  [
    "the provider owners swapped with consistent paths",
    applyAll(
      editRow("displacedRoutes", "LRA-D16", {
        deletionOwner: "LSO8",
        successorPath: ["LRA0", "LSO8"],
        receivingAcceptance: "LSO8-AC1",
      }),
      editRow("displacedRoutes", "LRA-D17", {
        deletionOwner: "ACT1",
        successorPath: ["LRA0", "LSO8", "ACT1"],
        receivingAcceptance: "ACT1-AC1",
      }),
    ),
    /LRA-D16: owned by LSO8, not ACT1/,
  ],
  [
    "a source path outside the tree",
    editRow("consumers", "LRA-C15", { source: "crates/verter_lsp/src/features/absent.rs" }),
    /LRA-C15: source .* is not in the tree/,
  ],
  [
    "a route pointed at an unrelated file that exists",
    editRow("displacedRoutes", "LRA-D17", { source: "crates/verter_lsp/src/lib.rs" }),
    /LRA-D17: source crates\/verter_lsp\/src\/lib\.rs is not /,
  ],
  [
    "the provider producer relabelled as bridged with its rows removed",
    applyAll(
      editProducer("merge_code_actions", {
        reach: "bridged",
        consumer: "LRA-C06",
        routes: ["LRA-D04", "LRA-D05"],
      }),
      removeRow("consumers", "LRA-C16"),
      removeRow("displacedRoutes", "LRA-D16"),
      removeRow("displacedRoutes", "LRA-D17"),
    ),
    /merge_code_actions: reach is bridged, not direct/,
  ],
  [
    "the provider producer bound to another producer's routes",
    editProducer("merge_code_actions", { routes: ["LRA-D14", "LRA-D15"] }),
    /merge_code_actions: routes are LRA-D14,LRA-D15, not LRA-D16,LRA-D17/,
  ],
  [
    "the provider producer bound to another producer's consumer",
    applyAll(
      editProducer("merge_code_actions", { consumer: "LRA-C15" }),
      removeRow("consumers", "LRA-C16"),
    ),
    /merge_code_actions: consumer is LRA-C15, not LRA-C16/,
  ],
  [
    "a direct producer relabelled as bridged",
    editProducer("organize_imports_actions", { reach: "bridged" }),
    /organize_imports_actions: reach is bridged, not direct/,
  ],
  [
    "a bridged producer relabelled as direct",
    editProducer("action_engine_fixes", { reach: "direct" }),
    /action_engine_fixes: reach is direct, not bridged/,
  ],
  [
    "a bridge with no routes",
    editProducer("action_engine_fixes", { routes: [] }),
    /action_engine_fixes: routes are , not LRA-D04,LRA-D05/,
  ],
  [
    "a producer answering another request kind",
    editProducer("extract_component_action", { requestKind: "quickfix" }),
    /extract_component_action: answers refactor\.extract, not quickfix/,
  ],
  [
    "a producer the handler does not call",
    (inventory) => {
      inventory.codeActionRouteAudit.producers.push({ producer: "unknown_action", routes: [] });
    },
    /unknown_action: not a handler producer/,
  ],
  [
    "a producer audited twice, the later copy correct",
    (inventory) => {
      const { producers } = inventory.codeActionRouteAudit;
      const entry = producers.find((row) => row.producer === "merge_code_actions");
      producers.unshift({ ...entry, reach: "bridged" });
    },
    /merge_code_actions: audited more than once/,
  ],
  [
    "a route row shadowed by a later correct copy",
    (inventory) => {
      const row = inventory.displacedRoutes.find((candidate) => candidate.id === "LRA-D15");
      inventory.displacedRoutes.unshift({ ...row, deletionOwner: "ACT1" });
    },
    /LRA-D15: more than one displaced route row/,
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
