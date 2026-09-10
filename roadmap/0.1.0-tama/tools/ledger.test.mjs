import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  cancelledRows,
  implementedRows,
  ledgerErrors,
  markCancelled,
  mergeLedgerTexts,
  markPending,
  parseLedgerText,
  predeclareMissing,
  serializeLedger,
  setEvidence,
  transitionToImplemented,
} from "./ledger.mjs";
import { parseToml } from "./toml.mjs";
import { deriveState, loadAuthority, validateAuthority, validateSchemaObject } from "./lib.mjs";

const TOOLS_DIR = path.dirname(fileURLToPath(import.meta.url));

function doc(implementation, extra = {}) {
  return { schema: 2, implementation, ...extra };
}

const EVIDENCE = {
  status: "implemented",
  commit_message: "feat(core): land the thing",
  commit_date: "2026-09-01T10:00:00+01:00",
};

function textOf(implementation, extra = {}) {
  return serializeLedger(doc(implementation, extra));
}

test("every node appears exactly once; unknown and missing nodes are invalid", () => {
  const known = new Set(["A0", "D4", "D5"]);
  const valid = doc({ A0: EVIDENCE, D4: { status: "pending" }, D5: { status: "pending" } });
  assert.deepEqual(ledgerErrors(valid, { knownNodeIds: known }), []);

  const unknown = doc({ ...valid.implementation, ZZ9: { status: "pending" } });
  assert.ok(
    ledgerErrors(unknown, { knownNodeIds: known }).some((error) => error.includes("unknown node ZZ9")),
  );

  const missing = doc({ A0: EVIDENCE, D4: { status: "pending" } });
  assert.ok(
    ledgerErrors(missing, { knownNodeIds: known }).some((error) =>
      error.includes("missing predeclared node D5"),
    ),
  );
});

test("pending rows carry no evidence; implemented rows require evidence", () => {
  const pendingWithEvidence = doc({
    D4: { status: "pending", commit_message: "sneaky" },
  });
  assert.ok(
    ledgerErrors(pendingWithEvidence).some((error) => error.includes("pending rows carry no commit_message")),
  );

  const implementedWithout = doc({ D4: { status: "implemented" } });
  const errors = ledgerErrors(implementedWithout);
  assert.ok(errors.some((error) => error.includes("require commit_message")));
  assert.ok(errors.some((error) => error.includes("commit_date")));

  const badStatus = doc({ D4: { status: "CLAIMED" } });
  assert.ok(
    ledgerErrors(badStatus).some((error) => error.includes('status must be "pending", "implemented" or "cancelled"')),
    "transient orchestration states are structurally invalid in the ledger",
  );
});

test("cancelled rows carry only an optional reason and are neither implemented nor pending", () => {
  const cancelled = doc({ A0: EVIDENCE, D4: { status: "cancelled", reason: "superseded by D5" }, D5: { status: "cancelled" } });
  assert.deepEqual(ledgerErrors(cancelled), []);
  assert.deepEqual(implementedRows(cancelled).map((row) => row.node_id), ["A0"]);
  assert.deepEqual(cancelledRows(cancelled), [{ node_id: "D4", reason: "superseded by D5" }, { node_id: "D5" }]);

  const withEvidence = doc({ D4: { status: "cancelled", commit_message: "sneaky" } });
  assert.ok(ledgerErrors(withEvidence).some((error) => error.includes("cancelled rows carry no commit_message")));
  const emptyReason = doc({ D4: { status: "cancelled", reason: "" } });
  assert.ok(ledgerErrors(emptyReason).some((error) => error.includes("reason must be a non-empty string")));
  const implementedWithReason = doc({ A0: { ...EVIDENCE, reason: "why" } });
  assert.ok(ledgerErrors(implementedWithReason).some((error) => error.includes("implemented rows carry no reason")));

  // The transition is a one-line diff like every other, round-trips through the canonical text, and is reversible.
  const before = textOf({ A0: EVIDENCE, D4: { status: "pending" } });
  const after = serializeLedger(markCancelled(parseLedgerText(before), "D4", { reason: "superseded by D5" }));
  assert.match(after, /^"D4" = \{ status = "cancelled", reason = "superseded by D5" \}$/mu);
  assert.equal(after.split("\n").filter((line) => !before.split("\n").includes(line)).length, 1);
  assert.deepEqual(markPending(parseLedgerText(after), "D4").implementation.D4, { status: "pending" });
  assert.throws(() => markCancelled(parseLedgerText(before), "A0"), /is implemented; mark it pending before cancelling/u);
  assert.throws(
    () => transitionToImplemented(parseLedgerText(after), "D4", { commitMessage: "m", commitDate: "2026-09-01T10:00:00+01:00" }),
    /is cancelled; mark it pending before recording an implementation/u,
  );
});

test("serialization is canonical, sorted, and one line per node", () => {
  const a = textOf({ D5: { status: "pending" }, A0: EVIDENCE, D4: { status: "pending" } });
  const b = textOf({ A0: EVIDENCE, D4: { status: "pending" }, D5: { status: "pending" } });
  assert.equal(a, b);
  const lines = a.split("\n");
  const nodeLines = lines.filter((line) => line.startsWith('"'));
  assert.deepEqual(
    nodeLines.map((line) => line.slice(0, line.indexOf("=")).trim()),
    ['"A0"', '"D4"', '"D5"'],
  );
  // Round-trips through the parser.
  const reparsed = parseLedgerText(a);
  assert.deepEqual(implementedRows(reparsed), [
    {
      node_id: "A0",
      commit_message: EVIDENCE.commit_message,
      commit_date: EVIDENCE.commit_date,
    },
  ]);
});

test("a single transition produces a one-line diff", () => {
  const before = textOf({ A0: EVIDENCE, D4: { status: "pending" }, D5: { status: "pending" } });
  const transitioned = transitionToImplemented(parseLedgerText(before), "D4", {
    commitMessage: "feat(core): narrowing and structural returns",
    commitDate: "2026-09-02T12:00:00+01:00",
    pullRequest: 512,
  });
  const after = serializeLedger(transitioned);
  const beforeLines = before.split("\n");
  const afterLines = after.split("\n");
  assert.equal(beforeLines.length, afterLines.length);
  const changed = beforeLines.filter((line, index) => line !== afterLines[index]);
  assert.equal(changed.length, 1);
  assert.match(changed[0], /^"D4" = \{ status = "pending" \}$/u);
});

test("transition is idempotent for identical evidence and fails closed on conflicts", () => {
  const parsed = parseLedgerText(textOf({ D4: { status: "pending" } }));
  const evidence = {
    commitMessage: "feat(core): d4",
    commitDate: "2026-09-02T12:00:00+01:00",
  };
  const once = transitionToImplemented(parsed, "D4", evidence);
  const twice = transitionToImplemented(once, "D4", evidence);
  assert.deepEqual(once, twice);
  assert.throws(
    () => transitionToImplemented(once, "D4", { ...evidence, commitMessage: "different" }),
    /already implemented with different evidence/u,
  );
  assert.throws(() => transitionToImplemented(parsed, "ZZ9", evidence), /unknown node ZZ9/u);
});

test("independent concurrent transitions merge mechanically: latest main + candidate transition", () => {
  const base = textOf({ D4: { status: "pending" }, D5: { status: "pending" }, TE2: { status: "pending" } });
  // main landed D5
  const ours = serializeLedger(
    transitionToImplemented(parseLedgerText(base), "D5", {
      commitMessage: "feat(core): closure capture freshness effects",
      commitDate: "2026-09-02T09:00:00+01:00",
      pullRequest: 511,
    }),
  );
  // candidate landed D4
  const theirs = serializeLedger(
    transitionToImplemented(parseLedgerText(base), "D4", {
      commitMessage: "feat(core): narrowing and structural returns",
      commitDate: "2026-09-02T12:00:00+01:00",
      pullRequest: 512,
    }),
  );
  const merged = mergeLedgerTexts({ base, ours, theirs });
  assert.equal(merged.ok, true);
  const rows = implementedRows(parseLedgerText(merged.text));
  assert.deepEqual(
    rows.map((row) => row.node_id),
    ["D4", "D5"],
  );
  assert.equal(rows.find((row) => row.node_id === "D4").pull_request, 512);
  assert.equal(rows.find((row) => row.node_id === "D5").pull_request, 511);
});

test("incompatible changes to the same node fail closed", () => {
  const base = textOf({ D4: { status: "pending" } });
  const ours = serializeLedger(
    transitionToImplemented(parseLedgerText(base), "D4", {
      commitMessage: "ours",
      commitDate: "2026-09-02T09:00:00+01:00",
    }),
  );
  const theirs = serializeLedger(
    transitionToImplemented(parseLedgerText(base), "D4", {
      commitMessage: "theirs",
      commitDate: "2026-09-02T10:00:00+01:00",
    }),
  );
  const merged = mergeLedgerTexts({ base, ours, theirs });
  assert.equal(merged.ok, false);
  assert.ok(merged.conflicts.some((conflict) => conflict.includes("node D4")));
});

test("merge unions github_issue rows and rejects duplicate issue numbers", () => {
  const base = textOf({ D4: { status: "pending" }, D5: { status: "pending" } });
  const ours = textOf(
    { D4: { status: "pending" }, D5: { status: "pending" } },
    { github_issue: [{ node_id: "D4", gh_issue: 40, sync_to_github: true }] },
  );
  const theirs = textOf(
    { D4: { status: "pending" }, D5: { status: "pending" } },
    { github_issue: [{ node_id: "D5", gh_issue: 50, sync_to_github: true }] },
  );
  const merged = mergeLedgerTexts({ base, ours, theirs });
  assert.equal(merged.ok, true);
  const rows = parseLedgerText(merged.text).github_issue;
  assert.deepEqual(
    rows.map((row) => row.node_id).sort(),
    ["D4", "D5"],
  );

  const clashing = textOf(
    { D4: { status: "pending" }, D5: { status: "pending" } },
    { github_issue: [{ node_id: "D5", gh_issue: 40, sync_to_github: true }] },
  );
  const clash = mergeLedgerTexts({ base, ours, theirs: clashing });
  assert.equal(clash.ok, false);
  assert.ok(clash.conflicts.some((conflict) => conflict.includes("duplicate gh_issue 40")));
});

test("merge-ledger CLI merges to stdout and fails closed with exit 1", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tama-ledger-"));
  const base = textOf({ D4: { status: "pending" }, D5: { status: "pending" } });
  const ours = serializeLedger(
    transitionToImplemented(parseLedgerText(base), "D5", {
      commitMessage: "ours d5",
      commitDate: "2026-09-02T09:00:00+01:00",
    }),
  );
  const theirs = serializeLedger(
    transitionToImplemented(parseLedgerText(base), "D4", {
      commitMessage: "theirs d4",
      commitDate: "2026-09-02T10:00:00+01:00",
    }),
  );
  const write = (name, text) => {
    const file = path.join(dir, name);
    fs.writeFileSync(file, text);
    return file;
  };
  const cli = path.join(TOOLS_DIR, "merge-ledger.mjs");
  const ok = spawnSync(
    process.execPath,
    [cli, write("base.toml", base), write("ours.toml", ours), write("theirs.toml", theirs)],
    { encoding: "utf8" },
  );
  assert.equal(ok.status, 0, ok.stderr);
  const rows = implementedRows(parseLedgerText(ok.stdout));
  assert.deepEqual(
    rows.map((row) => row.node_id),
    ["D4", "D5"],
  );

  const conflictTheirs = serializeLedger(
    transitionToImplemented(parseLedgerText(base), "D5", {
      commitMessage: "conflicting d5",
      commitDate: "2026-09-02T11:00:00+01:00",
    }),
  );
  const bad = spawnSync(
    process.execPath,
    [cli, path.join(dir, "base.toml"), path.join(dir, "ours.toml"), write("conflict.toml", conflictTheirs)],
    { encoding: "utf8" },
  );
  assert.equal(bad.status, 1);
  assert.match(bad.stderr, /CONFLICT/u);

  // --driver mode writes into ours (git merge driver contract).
  const oursCopy = write("ours-driver.toml", ours);
  const driver = spawnSync(
    process.execPath,
    [cli, path.join(dir, "base.toml"), oursCopy, path.join(dir, "theirs.toml"), "--driver"],
    { encoding: "utf8" },
  );
  assert.equal(driver.status, 0, driver.stderr);
  const written = implementedRows(parseLedgerText(fs.readFileSync(oursCopy, "utf8")));
  assert.deepEqual(
    written.map((row) => row.node_id),
    ["D4", "D5"],
  );
});

test("markPending deliberately un-implements and predeclareMissing adds pending rows", () => {
  const parsed = parseLedgerText(textOf({ A0: EVIDENCE }));
  const back = markPending(parsed, "A0");
  assert.deepEqual(back.implementation.A0, { status: "pending" });
  const expanded = predeclareMissing(back, ["A0", "NEW1"]);
  assert.deepEqual(expanded.implementation.NEW1, { status: "pending" });
});

test("setEvidence updates locator fields on an implemented node only", () => {
  const parsed = parseLedgerText(textOf({ A0: EVIDENCE, D4: { status: "pending" } }));
  const updated = setEvidence(parsed, "A0", { pullRequest: 777 });
  assert.equal(updated.implementation.A0.pull_request, 777);
  assert.equal(updated.implementation.A0.commit_message, EVIDENCE.commit_message);
  assert.throws(() => setEvidence(parsed, "D4", { pullRequest: 1 }), /not implemented/u);
});

test("live authority loads the schema-2 ledger, derives state, and validates clean", () => {
  const authority = loadAuthority();
  assert.equal(authority.ledger.schema, 2);
  assert.ok(Object.keys(authority.ledger.implementation).length >= authority.ledger.implemented.length);
  // Every DAG node is predeclared exactly once.
  const nodeIds = new Set(authority.nodes.map((node) => node.id));
  assert.equal(Object.keys(authority.ledger.implementation).length, nodeIds.size);
  assert.deepEqual(validateAuthority(authority, { strict: true }), []);
  const state = deriveState(authority);
  assert.equal(state.states.get("A0").status, "COMPLETE");
});

test("transient orchestration states never appear in the live ledger", () => {
  const authority = loadAuthority();
  // pending, implemented and cancelled are durable ledger facts; anything else is runtime state.
  for (const [nodeId, record] of Object.entries(authority.ledger.implementation)) {
    assert.ok(
      ["pending", "implemented", "cancelled"].includes(record.status),
      `${nodeId} carries runtime state ${record.status}`,
    );
  }
});

test("inline-table parser rejects duplicates and prototype-bearing keys", () => {
  assert.throws(() => parseToml('[implementation]\n"D4" = { status = "pending", status = "pending" }'), /duplicate/u);
  assert.throws(() => parseToml('[implementation]\n"__proto__" = { status = "pending" }'), /unsafe/u);
  const nested = parseToml('a = { b = { c = 1 }, d = "x, y = }" }');
  assert.deepEqual(nested, { a: { b: { c: 1 }, d: "x, y = }" } });
});

test("the ledger schema keeps the per-status shapes mutually exclusive, like recordErrors", () => {
  const schema = JSON.parse(fs.readFileSync(path.join(TOOLS_DIR, "..", "schemas", "implementation-ledger.schema.json"), "utf8"));
  const check = (implementation) => validateSchemaObject({ schema: 2, implementation }, schema, "ledger");
  assert.deepEqual(check({ A0: EVIDENCE, D4: { status: "pending" }, D5: { status: "cancelled", reason: "superseded" }, D6: { status: "cancelled" } }), []);
  assert.ok(check({ A0: { ...EVIDENCE, reason: "why" } }).length, "implemented rows reject reason");
  assert.ok(check({ D4: { status: "cancelled", commit_message: "sneaky" } }).length, "cancelled rows reject evidence");
  assert.ok(check({ D4: { status: "pending", reason: "why" } }).length, "pending rows reject reason");
  assert.ok(check({ D4: { status: "implemented" } }).length, "implemented rows require evidence");
  assert.ok(check({ D4: { status: "CLAIMED" } }).length, "unknown statuses match no branch");
});
