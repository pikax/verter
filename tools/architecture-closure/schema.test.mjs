import assert from "node:assert/strict";
import test from "node:test";

import { checkSchema } from "./schema-check.mjs";
import { loadFixture } from "./test-support.mjs";
import { REPORT_SCHEMA, validateClosure } from "./validate.mjs";

const closure = loadFixture("report-closure.json");

test("the complete passing fixtures satisfy the schema", () => {
  assert.deepEqual(checkSchema(REPORT_SCHEMA, closure), []);
  assert.deepEqual(checkSchema(REPORT_SCHEMA, loadFixture("report-owner-lock.json")), []);
});

const MALFORMED = [
  ["a non-object report", () => "report", "/", "type"],
  ["a report without owners", (r) => delete r.owners, "/", "required"],
  ["a report without inventories", (r) => delete r.inventories, "/", "required"],
  ["an unknown mode", (r) => (r.mode = "partial"), "/mode", "enum"],
  ["an unknown top-level section", (r) => (r.extra = []), "/extra", "additionalProperties"],
  ["owners that are not a list", (r) => (r.owners = {}), "/owners", "type"],
  ["an owner row without a slug", (r) => delete r.owners[1].slug, "/owners/1", "required"],
  ["an empty owner slug", (r) => (r.owners[1].slug = ""), "/owners/1/slug", "minLength"],
  [
    "an unknown owner field",
    (r) => (r.owners[1].note = "x"),
    "/owners/1/note",
    "additionalProperties",
  ],
  [
    "an unknown route disposition",
    (r) => (r.owners[2].displacedRoutes[0].disposition = "kept"),
    "/owners/2/displacedRoutes/0/disposition",
    "enum",
  ],
  [
    "an unknown stability class",
    (r) => (r.capabilities[0].stabilityClass = "beta"),
    "/capabilities/0/stabilityClass",
    "enum",
  ],
  ["a textual metric result", (r) => (r.metrics[0].result = "ok"), "/metrics/0/result", "enum"],
  [
    "a worker without tags",
    (r) => delete r.metrics[0].worker.tags,
    "/metrics/0/worker",
    "required",
  ],
  [
    "a fractional execution count",
    (r) => (r.evidence[0].terminalSummary.executed = 1.5),
    "/evidence/0/terminalSummary/executed",
    "type",
  ],
  [
    "a negative execution count",
    (r) => (r.evidence[0].terminalSummary.selected = -1),
    "/evidence/0/terminalSummary/selected",
    "minimum",
  ],
  [
    "an unknown executor kind",
    (r) => (r.evidence[0].executor.kind = "manual"),
    "/evidence/0/executor/kind",
    "enum",
  ],
  [
    "inventories without the observe section",
    (r) => delete r.inventories.observe,
    "/inventories",
    "required",
  ],
  [
    "an unknown observe classification",
    (r) => (r.inventories.observe.files[0].rows[0].classification = "MAYBE"),
    "/inventories/observe/files/0/rows/0/classification",
    "enum",
  ],
  [
    "an unknown admission class",
    (r) => (r.inventories.reuse.admissions[0].class = "Cached"),
    "/inventories/reuse/admissions/0/class",
    "enum",
  ],
  [
    "an exception without a subject",
    (r) => delete r.exceptions[0].subject,
    "/exceptions/0",
    "required",
  ],
];

for (const [name, mutate, path, keyword] of MALFORMED) {
  test(`the schema rejects ${name}`, () => {
    const report = structuredClone(closure);
    const replaced = mutate(report);
    const subject = typeof replaced === "string" && path === "/" ? replaced : report;
    const violations = checkSchema(REPORT_SCHEMA, subject);
    assert.ok(
      violations.some((v) => (v.path || "/") === path && v.keyword === keyword),
      JSON.stringify(violations),
    );
  });
}

test("the validator reports a malformed report as schema-invalid, before any population rule", () => {
  const report = structuredClone(closure);
  delete report.owners;
  const decision = validateClosure({
    mode: "closure",
    plan: loadFixture("plan-closure.json"),
    report,
  });
  assert.equal(decision.ok, false);
  assert.deepEqual([...new Set(decision.errors.map((e) => e.code))], ["schema-invalid"]);
});

test("the schema checker refuses a keyword it does not enforce", () => {
  for (const schema of [
    { type: "string", pattern: "^a" },
    { oneOf: [{ type: "string" }] },
    { properties: { a: { type: "string" } } },
    { minLength: 1 },
    { $ref: "#/$defs/absent" },
    { type: "number" },
    { type: "array", items: { pattern: "^a" } },
    { type: "object", properties: { unreached: { format: "uri" } } },
    { $defs: { unreferenced: { oneOf: [] } } },
  ]) {
    assert.throws(() => checkSchema(schema, "a"), /^Error: schema: /, JSON.stringify(schema));
  }
});
