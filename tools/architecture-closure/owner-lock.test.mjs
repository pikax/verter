import assert from "node:assert/strict";
import test from "node:test";

import { assertRejected, dropRow, findRow, loadFixture } from "./test-support.mjs";
import { validateClosure } from "./validate.mjs";

const base = {
  plan: loadFixture("plan-owner-lock.json"),
  report: loadFixture("report-owner-lock.json"),
};

const owner = (slug) => (row) => row.slug === slug;
const ownerRow = ({ report }, slug) => findRow(report.owners, owner(slug));
const ledgerStatus = ({ plan }, slug) => plan.ledger.find(owner(slug))?.status;
const planTask = ({ plan }, slug) => findRow(plan.tasks, owner(slug));
const without = (list, slug) => list.splice(list.indexOf(slug), 1);

test("pending semantic-precision, performance and evidence owners are listed and accepted", () => {
  for (const slug of ["P-COND", "P-PERF", "CP-SEM"]) {
    assert.equal(ledgerStatus(base, slug), "pending");
  }
  const decision = validateClosure({ mode: "owner-lock", ...base });
  assert.deepEqual(decision.errors, []);
  assert.equal(decision.ok, true);
  assert.deepEqual(
    decision.pending.map((row) => row.slug),
    ["CP-SEM", "ECRX2", "L4-V", "P-COND", "P-PERF", "REL3"],
  );
  assert.deepEqual(findRow(decision.pending, owner("P-PERF")), {
    slug: "P-PERF",
    train: "rev11.semantic-closure",
    status: "pending",
  });
});

test("owner-lock ignores the closure-only populations", () => {
  const inputs = structuredClone(base);
  for (const field of [
    "namedOwners",
    "requiredOutcomes",
    "capabilitySurfaces",
    "metricCells",
    "reuseAdmissions",
  ]) {
    delete inputs.plan.closure[field];
  }
  assert.equal(validateClosure({ mode: "owner-lock", ...inputs }).ok, true);
});

const CONTROLS = [
  {
    name: "a pending structural owner",
    mutate: ({ plan, report }) => {
      findRow(plan.ledger, owner("S-OBS")).status = "pending";
      findRow(report.owners, owner("S-OBS")).status = "pending";
    },
    applied: (i) => ledgerStatus(i, "S-OBS") === "pending",
    expect: ["structural-owner-pending"],
  },
  {
    name: "a pending transitive ancestor of a structural owner",
    mutate: ({ plan }) => (findRow(plan.ledger, owner("B-BRIDGE")).status = "pending"),
    applied: (i) => ledgerStatus(i, "B-BRIDGE") === "pending",
    expect: ["ancestor-pending"],
  },
  {
    name: "a structural owner with no ledger row",
    mutate: ({ plan, report }) => {
      dropRow(plan.ledger, owner("S-OBS"));
      delete findRow(report.owners, owner("S-OBS")).status;
    },
    applied: (i) => ledgerStatus(i, "S-OBS") === undefined,
    expect: ["required-owner-ledger-absent"],
  },
  {
    name: "a missing structural owner row",
    mutate: ({ report }) => dropRow(report.owners, owner("CP-STRUCT")),
    applied: ({ report }) => !report.owners.some(owner("CP-STRUCT")),
    expect: ["structural-owner-row-missing"],
  },
  {
    name: "a structural owner with no recorded path",
    mutate: (i) => delete ownerRow(i, "S-ENG").dependencyPath,
    applied: (i) => ownerRow(i, "S-ENG").dependencyPath === undefined,
    expect: ["owner-path-missing"],
  },
  {
    name: "a structural owner whose path is not made of real edges",
    mutate: (i) => (ownerRow(i, "S-ENG").dependencyPath = ["S-ENG", "K-8", "L4-A"]),
    applied: (i) => ownerRow(i, "S-ENG").dependencyPath.includes("K-8"),
    expect: ["owner-path-invalid"],
  },
  {
    name: "a report row outside the lock's population",
    mutate: ({ report }) => report.owners.push({ slug: "DETACHED" }),
    applied: ({ report }) => report.owners.some(owner("DETACHED")),
    expect: ["owner-pathless"],
  },
  {
    name: "a structural owner whose edges to the lock were dropped",
    mutate: (i) => {
      without(planTask(i, "L4-A").predecessors, "S-OBS");
      without(planTask(i, "CP-STRUCT").predecessors, "S-OBS");
      planTask(i, "L4").predecessors.push("S-OBS");
      delete ownerRow(i, "S-OBS").dependencyPath;
    },
    applied: (i) =>
      i.plan.closure.structuralOwners.includes("S-OBS") &&
      !planTask(i, "L4-A").predecessors.includes("S-OBS") &&
      !planTask(i, "CP-STRUCT").predecessors.includes("S-OBS"),
    expect: ["owner-path-missing", "owner-pathless"],
  },
  {
    name: "a structural owner the export names outside the plan",
    mutate: ({ plan }) => plan.closure.structuralOwners.push("INVENTED"),
    applied: ({ plan }) => plan.closure.structuralOwners.includes("INVENTED"),
    expect: ["owner-unknown"],
  },
  {
    name: "an export with no structural owner population",
    mutate: ({ plan }) => delete plan.closure.structuralOwners,
    applied: ({ plan }) => plan.closure.structuralOwners === undefined,
    expect: ["plan-field-missing"],
  },
  {
    name: "two discovered tests mapped onto one destination",
    mutate: ({ report }) => {
      const rows = report.inventories.engine.testMapping.rows;
      rows[1].new = rows[0].new;
    },
    applied: ({ report }) => {
      const rows = report.inventories.engine.testMapping.rows;
      return rows.length === 2 && rows[0].new === rows[1].new && rows[0].old !== rows[1].old;
    },
    expect: ["engine-inventory-incomplete"],
  },
  {
    name: "a declared relocation omitted",
    mutate: ({ plan }) =>
      plan.closure.engineRelocations.push({ from: "crates/a/src/x.rs", to: "crates/b/src/x.rs" }),
    applied: ({ report }) => report.inventories.engine.relocations.length === 1,
    expect: ["engine-inventory-incomplete"],
  },
  {
    name: "a declared affected future surface omitted",
    mutate: ({ plan }) =>
      plan.closure.futureCharterSurfaces.push({ surface: "crates/b/src/y.rs", owner: "P-PERF" }),
    applied: ({ report }) => report.inventories.engine.futureCharterSurfaces.length === 1,
    expect: ["engine-inventory-incomplete"],
  },
  {
    name: "a later owner the report does not list",
    mutate: ({ report }) => dropRow(report.owners, owner("P-PERF")),
    applied: ({ report }) => !report.owners.some(owner("P-PERF")),
    expect: ["nonstructural-owner-unlisted"],
  },
  {
    name: "a later owner claimed implemented against the ledger",
    mutate: (i) => (ownerRow(i, "P-PERF").status = "implemented"),
    applied: (i) => ownerRow(i, "P-PERF").status === "implemented",
    expect: ["owner-status-mismatch"],
  },
  {
    name: "an engine inventory with no relocations",
    mutate: ({ report }) => (report.inventories.engine.relocations = []),
    applied: ({ report }) => report.inventories.engine.relocations.length === 0,
    expect: ["engine-inventory-incomplete"],
  },
  {
    name: "an engine test mapping missing a discovered test",
    mutate: ({ report }) => report.inventories.engine.testMapping.rows.pop(),
    applied: ({ report }) => report.inventories.engine.testMapping.rows.length === 1,
    expect: ["engine-inventory-incomplete"],
  },
  {
    name: "an engine test population that changed under the mapping",
    mutate: ({ report }) => (report.inventories.engine.testMapping.discoveredAfter = 1),
    applied: ({ report }) => report.inventories.engine.testMapping.discoveredAfter === 1,
    expect: ["engine-inventory-incomplete"],
  },
  {
    name: "an engine enforcement family that is absent",
    mutate: ({ report }) => dropRow(report.inventories.engine.guards, (g) => g.kind === "feature"),
    applied: ({ report }) => !report.inventories.engine.guards.some((g) => g.kind === "feature"),
    expect: ["engine-inventory-incomplete"],
  },
  {
    name: "a format inventory not updated with the moves",
    mutate: ({ report }) => (report.inventories.engine.formatInventories[2].updated = false),
    applied: ({ report }) => report.inventories.engine.formatInventories[2].updated === false,
    expect: ["engine-inventory-incomplete"],
  },
  {
    name: "no affected future surface named",
    mutate: ({ report }) => (report.inventories.engine.futureCharterSurfaces = []),
    applied: ({ report }) => report.inventories.engine.futureCharterSurfaces.length === 0,
    expect: ["engine-inventory-incomplete"],
  },
  {
    name: "a moved guard that no longer executes",
    mutate: ({ report }) => (report.inventories.engine.guards[1].executedTests = 0),
    applied: ({ report }) => report.inventories.engine.guards[1].executedTests === 0,
    expect: ["moved-guard-not-executing"],
  },
  {
    name: "an observe inventory with no files",
    mutate: ({ report }) => (report.inventories.observe.files = []),
    applied: ({ report }) => report.inventories.observe.files.length === 0,
    expect: ["observe-inventory-incomplete"],
  },
  {
    name: "an observe row without its lifetime",
    mutate: ({ report }) => delete report.inventories.observe.files[0].rows[0].lifetime,
    applied: ({ report }) => report.inventories.observe.files[0].rows[0].lifetime === undefined,
    expect: ["observe-inventory-incomplete"],
  },
  {
    name: "an observe file outside the per-owner layout",
    mutate: ({ report }) => (report.inventories.observe.files[0].path = "docs/observe/S-OBS.md"),
    applied: ({ report }) => report.inventories.observe.files[0].path === "docs/observe/S-OBS.md",
    expect: ["observe-inventory-incomplete"],
  },
  {
    name: "optional default hot bookkeeping",
    mutate: ({ report }) => (report.inventories.observe.files[0].rows[1].gate = "always"),
    applied: ({ report }) =>
      report.inventories.observe.files[0].rows[1].classification === "OPTIONAL",
    expect: ["optional-default-hot-bookkeeping"],
  },
  {
    name: "an absent WSP6 count",
    mutate: ({ report }) => report.inventories.observe.counts.pop(),
    applied: ({ report }) => report.inventories.observe.counts.length === 1,
    expect: ["wsp6-count-absent"],
  },
  {
    name: "an export with no WSP6 count population",
    mutate: ({ plan }) => delete plan.closure.wsp6Counts,
    applied: ({ plan }) => plan.closure.wsp6Counts === undefined,
    expect: ["plan-field-missing"],
  },
  {
    name: "a report for the other mode",
    mutate: ({ report }) => (report.mode = "closure"),
    applied: ({ report }) => report.mode === "closure",
    expect: ["report-mode-mismatch"],
  },
];

for (const control of CONTROLS) {
  test(`owner-lock rejects ${control.name}`, () => {
    assertRejected("owner-lock", base, control);
  });
}

test("the same pending later owners reject closure, so only owner-lock tolerates them", () => {
  const decision = validateClosure({
    mode: "closure",
    plan: base.plan,
    report: loadFixture("report-closure.json"),
  });
  const pending = decision.errors.filter((error) => error.code === "ancestor-pending");
  assert.deepEqual(pending.map((error) => error.subject).sort(), [
    "CP-SEM",
    "ECRX2",
    "L4-A",
    "L4-V",
    "P-COND",
    "P-PERF",
    "REL3",
  ]);
});
