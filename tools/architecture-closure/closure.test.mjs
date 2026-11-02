import assert from "node:assert/strict";
import test from "node:test";

import { assertRejected, dropRow, findRow, loadFixture } from "./test-support.mjs";
import { POLICY, validateClosure } from "./validate.mjs";

const base = {
  plan: loadFixture("plan-closure.json"),
  report: loadFixture("report-closure.json"),
};

const owner = (slug) => (row) => row.slug === slug;
const ownerRow = ({ report }, slug) => findRow(report.owners, owner(slug));
const ledgerStatus = ({ plan }, slug) => plan.ledger.find(owner(slug))?.status;
const planTask = ({ plan }, slug) => findRow(plan.tasks, owner(slug));
const evidenceRow = ({ report }, id) => findRow(report.evidence, (row) => row.id === id);
const hasOwner = ({ report }, slug) => report.owners.some(owner(slug));
const metricRow = ({ report }, row) => findRow(report.metrics, (r) => r.row === row);
const reviewRow = ({ report }, train) => findRow(report.trainReviews, (r) => r.train === train);
const admission = ({ report }, id) =>
  findRow(report.inventories.reuse.admissions, (a) => a.id === id);
const countOf = (list, predicate) => list.filter(predicate).length;

test("the complete fixture is accepted with its target implemented", () => {
  assert.equal(ledgerStatus(base, "L4"), "implemented");
  const decision = validateClosure({ mode: "closure", ...base });
  assert.deepEqual(decision.errors, []);
  assert.equal(decision.ok, true);
  assert.deepEqual(decision.advisories, []);
  assert.deepEqual(decision.pending, []);
});

const CONTROLS = [
  {
    name: "target implemented while a required owner's report row is absent",
    mutate: ({ report }) => dropRow(report.owners, owner("S-OBS")),
    applied: (i) => ledgerStatus(i, "L4") === "implemented" && !hasOwner(i, "S-OBS"),
    expect: ["owner-row-missing"],
  },
  {
    name: "target implemented while a required owner's ledger row is absent",
    mutate: ({ plan }) => dropRow(plan.ledger, owner("S-OBS")),
    applied: (i) =>
      ledgerStatus(i, "L4") === "implemented" && ledgerStatus(i, "S-OBS") === undefined,
    expect: ["required-owner-ledger-absent"],
  },
  {
    name: "a pending transitive ancestor despite the target COMPLETE",
    mutate: ({ plan }) => (findRow(plan.ledger, owner("H0")).status = "pending"),
    applied: (i) => ledgerStatus(i, "L4") === "implemented" && ledgerStatus(i, "H0") === "pending",
    expect: ["ancestor-pending"],
  },
  {
    name: "a pending compiler-bridge ancestor",
    mutate: ({ plan }) => (findRow(plan.ledger, owner("B-BRIDGE")).status = "pending"),
    applied: (i) => ledgerStatus(i, "B-BRIDGE") === "pending",
    expect: ["ancestor-pending"],
  },
  {
    name: "a pathless owner named by the export",
    mutate: ({ plan }) => plan.closure.namedOwners.push("DETACHED"),
    applied: ({ plan }) => plan.closure.namedOwners.includes("DETACHED"),
    expect: ["owner-pathless"],
  },
  {
    name: "a pathless owner row in the report",
    mutate: ({ report }) => report.owners.push({ slug: "DETACHED", classification: "required" }),
    applied: (i) => hasOwner(i, "DETACHED"),
    expect: ["owner-pathless"],
  },
  {
    name: "an owner row outside the export",
    mutate: ({ report }) => report.owners.push({ slug: "INVENTED", classification: "required" }),
    applied: (i) => hasOwner(i, "INVENTED"),
    expect: ["owner-unknown"],
  },
  {
    name: "a sole final owner with no path to the target",
    mutate: (i) => (ownerRow(i, "S-OBS").soleFinalOwner = "DETACHED"),
    applied: (i) => ownerRow(i, "S-OBS").soleFinalOwner === "DETACHED",
    expect: ["owner-pathless"],
  },
  {
    name: "an absent conditional path when the export marks V9 selected",
    mutate: (i) => (planTask(i, "V9").selected = true),
    applied: (i) =>
      planTask(i, "V9").selected === true && !planTask(i, "L4").predecessors.includes("V9"),
    expect: ["conditional-owner-pathless"],
  },
  {
    name: "an unselected optional owner with no classified row",
    mutate: ({ report }) => dropRow(report.owners, owner("V9")),
    applied: (i) => !hasOwner(i, "V9"),
    expect: ["optional-row-unclassified"],
  },
  {
    name: "a historical owner not classified as historical",
    mutate: (i) => (ownerRow(i, "H0").classification = "required"),
    applied: (i) => ownerRow(i, "H0").classification === "required",
    expect: ["owner-classification-mismatch"],
  },
  {
    name: "an omitted train review",
    mutate: ({ report }) =>
      dropRow(report.trainReviews, (r) => r.train === "compiler.compiler-bridge"),
    applied: ({ report }) =>
      !report.trainReviews.some((r) => r.train === "compiler.compiler-bridge"),
    expect: ["train-review-missing"],
  },
  {
    name: "a train review of another candidate",
    mutate: ({ report }) => (report.trainReviews[0].candidate = "candidate-earlier"),
    applied: ({ report }) => report.trainReviews[0].candidate !== report.candidate,
    expect: ["train-review-stale"],
  },
  {
    name: "an uncovered checkpoint",
    mutate: ({ report }) =>
      (findRow(report.trainReviews, (r) => r.train === "rev11.structure").checkpoints = []),
    applied: ({ report }) =>
      findRow(report.trainReviews, (r) => r.train === "rev11.structure").checkpoints.length === 0,
    expect: ["train-checkpoint-uncovered"],
  },
  {
    name: "an unresolved material finding",
    mutate: ({ report }) =>
      (findRow(report.trainReviews, (r) => r.train === "rev11.structure").findings[0].status =
        "open"),
    applied: ({ report }) =>
      findRow(report.trainReviews, (r) => r.train === "rev11.structure").findings[0].status ===
      "open",
    expect: ["train-review-unresolved-finding"],
  },
  {
    name: "a deferred review finding",
    mutate: ({ report }) =>
      (findRow(report.trainReviews, (r) => r.train === "rev11.structure").findings[0].status =
        "deferred"),
    applied: ({ report }) =>
      findRow(report.trainReviews, (r) => r.train === "rev11.structure").findings[0].status ===
      "deferred",
    expect: ["train-review-open-deferral"],
  },
  {
    name: "a cumulative review that was not independent",
    mutate: ({ report }) =>
      delete findRow(report.trainReviews, (r) => r.train === "rev11.signature-kernel").independent,
    applied: ({ report }) =>
      findRow(report.trainReviews, (r) => r.train === "rev11.signature-kernel").independent ===
      undefined,
    expect: ["independent-review-missing"],
  },
  {
    name: "no terminal architecture review",
    mutate: ({ report }) =>
      delete findRow(report.trainReviews, (r) => r.train === "rev11.finalization").terminal,
    applied: ({ report }) =>
      findRow(report.trainReviews, (r) => r.train === "rev11.finalization").terminal === undefined,
    expect: ["terminal-review-missing"],
  },
  {
    name: "an omitted stable capability row",
    mutate: ({ report }) => dropRow(report.capabilities, (r) => r.surface === "vue-compile"),
    applied: ({ report }) => !report.capabilities.some((r) => r.surface === "vue-compile"),
    expect: ["capability-row-missing"],
  },
  {
    name: "an omitted preview capability row",
    mutate: ({ report }) => dropRow(report.capabilities, (r) => r.surface === "host-lint"),
    applied: ({ plan, report }) =>
      findRow(plan.closure.capabilitySurfaces, (s) => s.surface === "host-lint").class ===
        "preview" && !report.capabilities.some((r) => r.surface === "host-lint"),
    expect: ["capability-row-missing"],
  },
  {
    name: "an omitted unsupported capability cell",
    mutate: ({ report }) =>
      (findRow(report.capabilities, (r) => r.surface === "svelte-compile").cases = []),
    applied: ({ report }) =>
      findRow(report.capabilities, (r) => r.surface === "svelte-compile").cases.length === 0,
    expect: ["capability-cell-missing"],
  },
  {
    name: "a preview surface claiming complete",
    mutate: ({ report }) =>
      (findRow(report.capabilities, (r) => r.surface === "host-lint").typedResult = "complete"),
    applied: ({ report }) =>
      findRow(report.capabilities, (r) => r.surface === "host-lint").typedResult === "complete",
    expect: ["capability-preview-claims-complete"],
  },
  {
    name: "a partial result standing in for a stable surface",
    mutate: ({ report }) => {
      const row = findRow(report.capabilities, (r) => r.surface === "vue-compile");
      row.typedResult = "partial";
      row.limitation = "subset";
    },
    applied: ({ report }) =>
      findRow(report.capabilities, (r) => r.surface === "vue-compile").typedResult === "partial",
    expect: ["capability-noncomplete-stands-in"],
  },
  {
    name: "a capability class disagreeing with the catalog",
    mutate: ({ report }) =>
      (findRow(report.capabilities, (r) => r.surface === "lsp-edit-loop").stabilityClass =
        "preview"),
    applied: ({ report }) =>
      findRow(report.capabilities, (r) => r.surface === "lsp-edit-loop").stabilityClass ===
      "preview",
    expect: ["capability-class-mismatch"],
  },
  {
    name: "a missing metric cell",
    mutate: ({ report }) => dropRow(report.metrics, (r) => r.row === "wall/final"),
    applied: ({ report }) => !report.metrics.some((r) => r.row === "wall/final"),
    expect: ["metric-cell-missing"],
  },
  {
    name: "a missing performance or memory result",
    mutate: ({ report }) => delete findRow(report.metrics, (r) => r.row === "rss/soak").result,
    applied: ({ report }) =>
      findRow(report.metrics, (r) => r.row === "rss/soak").result === undefined,
    expect: ["metric-result-missing"],
  },
  {
    name: "an unavailable cell with no reason",
    mutate: ({ report }) =>
      (findRow(report.metrics, (r) => r.row === "rss/soak").result = "unavailable"),
    applied: ({ report }) =>
      findRow(report.metrics, (r) => r.row === "rss/soak").unavailableReason === undefined,
    expect: ["metric-result-missing"],
  },
  {
    name: "a metric cell measured on another worker",
    mutate: ({ report }) =>
      (findRow(report.metrics, (r) => r.row === "wall/final").worker = {
        id: "w",
        tags: ["linux"],
      }),
    applied: ({ report }) =>
      !findRow(report.metrics, (r) => r.row === "wall/final").worker.tags.includes(
        POLICY.metricWorkerTag,
      ),
    expect: ["metric-worker-not-allowed"],
  },
  {
    name: "a failing metric cell",
    mutate: ({ report }) =>
      (findRow(report.metrics, (r) => r.row === "wall/final").result = "fail"),
    applied: ({ report }) =>
      findRow(report.metrics, (r) => r.row === "wall/final").result === "fail",
    expect: ["metric-failing"],
  },
  {
    name: "a missing pressure outcome",
    mutate: ({ report }) =>
      delete findRow(report.metrics, (r) => r.row === "rss/soak").outcomes.pressure,
    applied: ({ report }) =>
      findRow(report.metrics, (r) => r.row === "rss/soak").outcomes.pressure === undefined,
    expect: ["metric-outcome-missing"],
  },
  {
    name: "a benchmark run the export declares no cell for",
    mutate: ({ plan, report }) => {
      dropRow(plan.closure.metricCells, (c) => c.evidenceRun === "skr-limits-confirm");
      dropRow(report.metrics, (r) => r.evidenceRun === "skr-limits-confirm");
    },
    applied: ({ plan }) =>
      !plan.closure.metricCells.some((c) => c.evidenceRun === "skr-limits-confirm"),
    expect: ["plan-field-missing"],
  },
  {
    name: "a missing outcome",
    mutate: (i) => delete ownerRow(i, "P-PERF").outcome,
    applied: (i) => ownerRow(i, "P-PERF").outcome === undefined,
    expect: ["owner-outcome-missing"],
  },
  {
    name: "a missing consumer",
    mutate: (i) => (ownerRow(i, "P-PERF").consumers = []),
    applied: (i) => ownerRow(i, "P-PERF").consumers.length === 0,
    expect: ["owner-consumer-missing"],
  },
  {
    name: "a missing evidence row",
    mutate: ({ report }) => dropRow(report.evidence, (r) => r.id === "ev-specialized"),
    applied: ({ report }) => !report.evidence.some((r) => r.id === "ev-specialized"),
    expect: ["owner-evidence-missing"],
  },
  {
    name: "a required outcome with no owner",
    mutate: (i) => delete ownerRow(i, "P-COND").outcomeIds,
    applied: (i) => ownerRow(i, "P-COND").outcomeIds === undefined,
    expect: ["outcome-unmapped"],
  },
  {
    name: "an outcome claimed by two owners",
    mutate: (i) =>
      (ownerRow(i, "K-8").outcomeIds = ["proposal-01", "direction-no-computation-cap"]),
    applied: (i) => ownerRow(i, "K-8").outcomeIds.includes("proposal-01"),
    expect: ["outcome-multiple-owners"],
  },
  {
    name: "a surviving route with no owner",
    mutate: (i) => delete ownerRow(i, "S-OBS").displacedRoutes[0].survivingOwner,
    applied: (i) => ownerRow(i, "S-OBS").displacedRoutes[0].survivingOwner === undefined,
    expect: ["surviving-unowned-route"],
  },
  {
    name: "a surviving route owned outside the population",
    mutate: (i) => (ownerRow(i, "S-OBS").displacedRoutes[0].survivingOwner = "DETACHED"),
    applied: (i) => ownerRow(i, "S-OBS").displacedRoutes[0].survivingOwner === "DETACHED",
    expect: ["surviving-unowned-route"],
  },
  {
    name: "the ECRV8 evidence row removed",
    mutate: ({ report }) => dropRow(report.evidence, owner2("ECRV8")),
    applied: ({ report }) => !report.evidence.some(owner2("ECRV8")),
    expect: ["ecrv8-evidence-missing"],
  },
  {
    name: "the ECRX2 evidence row removed",
    mutate: ({ report }) => dropRow(report.evidence, owner2("ECRX2")),
    applied: ({ report }) => !report.evidence.some(owner2("ECRX2")),
    expect: ["ecrx2-evidence-missing"],
  },
  {
    name: "the REL3 evidence row removed",
    mutate: ({ report }) => dropRow(report.evidence, owner2("REL3")),
    applied: ({ report }) => !report.evidence.some(owner2("REL3")),
    expect: ["rel3-evidence-missing"],
  },
  {
    name: "a required evidence lane with no row",
    mutate: ({ report }) => dropRow(report.evidence, (r) => r.lane === "wasm"),
    applied: ({ report }) => !report.evidence.some((r) => r.lane === "wasm"),
    expect: ["evidence-lane-missing"],
  },
  {
    name: "an evidence row with an incomplete terminal summary",
    mutate: (i) => (evidenceRow(i, "ev-wasm").terminalSummary.complete = false),
    applied: (i) => evidenceRow(i, "ev-wasm").terminalSummary.complete === false,
    expect: ["evidence-summary-incomplete"],
  },
  {
    name: "an evidence row that selected no work",
    mutate: (i) => (evidenceRow(i, "ev-wasm").terminalSummary.selected = 0),
    applied: (i) => evidenceRow(i, "ev-wasm").terminalSummary.selected === 0,
    expect: ["evidence-zero-work"],
  },
  {
    name: "an evidence row with an unexpected skip",
    mutate: (i) => (evidenceRow(i, "ev-wasm").terminalSummary.unexpectedSkips = 1),
    applied: (i) => evidenceRow(i, "ev-wasm").terminalSummary.unexpectedSkips === 1,
    expect: ["evidence-unexpected-skip"],
  },
  {
    name: "an evidence row that failed",
    mutate: (i) => (evidenceRow(i, "ev-wasm").terminalSummary.passed = false),
    applied: (i) => evidenceRow(i, "ev-wasm").terminalSummary.passed === false,
    expect: ["evidence-failed"],
  },
  {
    name: "an evidence row with a mismatched prerequisite",
    mutate: (i) => (evidenceRow(i, "ev-wasm").prerequisites[0].matched = false),
    applied: (i) => evidenceRow(i, "ev-wasm").prerequisites[0].matched === false,
    expect: ["evidence-prerequisite-mismatch"],
  },
  {
    name: "an evidence row with no execution owner",
    mutate: (i) => delete evidenceRow(i, "ev-wasm").executor,
    applied: (i) => evidenceRow(i, "ev-wasm").executor === undefined,
    expect: ["evidence-executor-missing"],
  },
  {
    name: "an evidence row of another candidate",
    mutate: (i) => (evidenceRow(i, "ev-wasm").candidate = "candidate-earlier"),
    applied: (i) => evidenceRow(i, "ev-wasm").candidate === "candidate-earlier",
    expect: ["evidence-stale-candidate"],
  },
  {
    name: "an evidence reference to no row",
    mutate: (i) => (ownerRow(i, "S-ENG").evidenceRefs = ["ev-canonical", "ev-absent"]),
    applied: (i) => ownerRow(i, "S-ENG").evidenceRefs.includes("ev-absent"),
    expect: ["evidence-ref-dangling"],
  },
  {
    name: "refusal-as-Complete",
    mutate: ({ report }) =>
      (findRow(report.inventories.reuse.admissions, (a) => a.id === "operation-refusal").reusedAs =
        "complete"),
    applied: ({ report }) =>
      findRow(report.inventories.reuse.admissions, (a) => a.class === "OperationRefusal")
        .reusedAs === "complete",
    expect: ["refusal-as-complete"],
  },
  {
    name: "context-poisoned reuse of a ledger refusal",
    mutate: ({ report }) =>
      (findRow(report.inventories.reuse.admissions, (a) => a.id === "root-refusal").isolatedRoot =
        false),
    applied: ({ report }) =>
      findRow(report.inventories.reuse.admissions, (a) => a.id === "root-refusal").isolatedRoot ===
      false,
    expect: ["context-poisoned-reuse"],
  },
  {
    name: "an operation refusal reused across profiles",
    mutate: ({ report }) =>
      (findRow(
        report.inventories.reuse.admissions,
        (a) => a.id === "operation-refusal",
      ).profileScoped = false),
    applied: ({ report }) =>
      findRow(report.inventories.reuse.admissions, (a) => a.id === "operation-refusal")
        .profileScoped === false,
    expect: ["context-poisoned-reuse"],
  },
  {
    name: "reuse of an execution-aborted result",
    mutate: ({ report }) =>
      (findRow(report.inventories.reuse.admissions, (a) => a.id === "cancelled").reusedAs =
        "refusal"),
    applied: ({ report }) =>
      findRow(report.inventories.reuse.admissions, (a) => a.id === "cancelled").reusedAs ===
      "refusal",
    expect: ["arbitrary-incomplete-reuse"],
  },
  {
    name: "a negative-cache dimension with no executed test",
    mutate: ({ report }) =>
      dropRow(report.inventories.reuse.negativeCacheTests, (r) => r.dimension === "edit"),
    applied: ({ report }) =>
      !report.inventories.reuse.negativeCacheTests.some((r) => r.dimension === "edit"),
    expect: ["negative-cache-test-missing"],
  },
  {
    name: "a report with no typed-reuse inventory",
    mutate: ({ report }) => delete report.inventories.reuse,
    applied: ({ report }) => report.inventories.reuse === undefined,
    expect: ["reuse-inventory-missing"],
  },
  {
    name: "unowned graph escapes",
    mutate: ({ report }) => (report.inventories.reuse.unownedGraphEscapes = 1),
    applied: ({ report }) => report.inventories.reuse.unownedGraphEscapes === 1,
    expect: ["unowned-graph-escape"],
  },
  {
    name: "inexact per-fact transport",
    mutate: ({ report }) => (report.inventories.reuse.perFactTransportExact = false),
    applied: ({ report }) => report.inventories.reuse.perFactTransportExact === false,
    expect: ["per-fact-transport-inexact"],
  },
  {
    name: "optional default hot bookkeeping",
    mutate: ({ report }) =>
      (findRow(
        report.inventories.observe.files[0].rows,
        (r) => r.classification === "OPTIONAL",
      ).gate = "always"),
    applied: ({ report }) =>
      findRow(report.inventories.observe.files[0].rows, (r) => r.classification === "OPTIONAL")
        .gate === "always",
    expect: ["optional-default-hot-bookkeeping"],
  },
  {
    name: "an absent WSP6 count",
    mutate: ({ report }) =>
      dropRow(report.inventories.observe.counts, (r) => r.name === "live-leases"),
    applied: ({ report }) =>
      !report.inventories.observe.counts.some((r) => r.name === "live-leases"),
    expect: ["wsp6-count-absent"],
  },
  {
    name: "a WSP6 count that needs capture",
    mutate: ({ report }) =>
      (findRow(
        report.inventories.observe.counts,
        (r) => r.name === "live-leases",
      ).availableWithCaptureOff = false),
    applied: ({ report }) =>
      findRow(report.inventories.observe.counts, (r) => r.name === "live-leases")
        .availableWithCaptureOff === false,
    expect: ["wsp6-count-requires-capture"],
  },
  {
    name: "no off-versus-stripped evidence",
    mutate: ({ report }) => delete report.inventories.observe.offVersusStripped,
    applied: ({ report }) => report.inventories.observe.offVersusStripped === undefined,
    expect: ["observe-off-versus-stripped-missing"],
  },
  {
    name: "a moved guard that no longer executes",
    mutate: ({ report }) => (report.inventories.engine.guards[0].executedTests = 0),
    applied: ({ report }) => report.inventories.engine.guards[0].executedTests === 0,
    expect: ["moved-guard-not-executing"],
  },
  {
    name: "a moved guard whose planted fixture is not rejected",
    mutate: ({ report }) => (report.inventories.engine.guards[0].plantedRejected = false),
    applied: ({ report }) => report.inventories.engine.guards[0].plantedRejected === false,
    expect: ["moved-guard-not-executing"],
  },
  {
    name: "an undispositioned exception",
    mutate: ({ report }) => delete report.exceptions[0].disposition,
    applied: ({ report }) => report.exceptions[0].disposition === undefined,
    expect: ["exception-undispositioned"],
  },
  {
    name: "an open deferral",
    mutate: ({ report }) => (report.exceptions[0].kind = "deferral"),
    applied: ({ report }) => report.exceptions[0].kind === "deferral",
    expect: ["open-deferral"],
  },
  {
    name: "an unexplained mismatch",
    mutate: ({ report }) => delete report.exceptions[1].rationale,
    applied: ({ report }) =>
      report.exceptions[1].kind === "mismatch" && report.exceptions[1].rationale === undefined,
    expect: ["exception-unexplained"],
  },
  {
    name: "a failing metric row shadowed by a later passing copy",
    mutate: (i) => {
      i.report.metrics.push(structuredClone(metricRow(i, "wall/final")));
      metricRow(i, "wall/final").result = "fail";
    },
    applied: ({ report }) =>
      countOf(report.metrics, (r) => r.row === "wall/final") === 2 &&
      report.metrics.find((r) => r.row === "wall/final").result === "fail",
    expect: ["metric-failing", "metric-row-duplicate"],
  },
  {
    name: "a duplicated metric row",
    mutate: (i) => i.report.metrics.push(structuredClone(metricRow(i, "wall/final"))),
    applied: ({ report }) => countOf(report.metrics, (r) => r.row === "wall/final") === 2,
    expect: ["metric-row-duplicate"],
  },
  {
    name: "a stale review with an open P0 shadowed by a clean review of the same train",
    mutate: (i) => {
      const clean = structuredClone(reviewRow(i, "rev11.structure"));
      Object.assign(reviewRow(i, "rev11.structure"), {
        candidate: "candidate-earlier",
        findings: [{ id: "x", severity: "P0", status: "open" }],
      });
      i.report.trainReviews.push(clean);
    },
    applied: ({ report }) =>
      countOf(report.trainReviews, (r) => r.train === "rev11.structure") === 2,
    expect: ["train-review-duplicate", "train-review-stale", "train-review-unresolved-finding"],
  },
  {
    name: "a duplicated train review",
    mutate: (i) => i.report.trainReviews.push(structuredClone(reviewRow(i, "rev11.structure"))),
    applied: ({ report }) =>
      countOf(report.trainReviews, (r) => r.train === "rev11.structure") === 2,
    expect: ["train-review-duplicate"],
  },
  {
    name: "a duplicated capability row",
    mutate: ({ report }) =>
      report.capabilities.push(
        structuredClone(findRow(report.capabilities, (r) => r.surface === "host-lint")),
      ),
    applied: ({ report }) => countOf(report.capabilities, (r) => r.surface === "host-lint") === 2,
    expect: ["capability-row-duplicate"],
  },
  {
    name: "a duplicated capability cell",
    mutate: ({ report }) => {
      const row = findRow(report.capabilities, (r) => r.surface === "svelte-compile");
      row.cases.push(structuredClone(row.cases[0]));
    },
    applied: ({ report }) =>
      findRow(report.capabilities, (r) => r.surface === "svelte-compile").cases.length === 2,
    expect: ["capability-cell-duplicate"],
  },
  {
    name: "a duplicated WSP6 count",
    mutate: ({ report }) =>
      report.inventories.observe.counts.push({
        name: "live-leases",
        accessor: "Leases::len",
        availableWithCaptureOff: true,
      }),
    applied: ({ report }) =>
      countOf(report.inventories.observe.counts, (r) => r.name === "live-leases") === 2,
    expect: ["wsp6-count-duplicate"],
  },
  {
    name: "a failing pressure outcome on an unavailable cell",
    mutate: (i) =>
      Object.assign(metricRow(i, "rss/soak"), {
        result: "unavailable",
        unavailableReason: "aggregate not computed",
        outcomes: { normal: "pass", pressure: "fail" },
      }),
    applied: (i) => metricRow(i, "rss/soak").outcomes.pressure === "fail",
    expect: ["metric-failing"],
  },
  {
    name: "a pass whose modes were never measured",
    mutate: (i) =>
      (metricRow(i, "rss/soak").outcomes = { normal: "unavailable", pressure: "unavailable" }),
    applied: (i) =>
      metricRow(i, "rss/soak").result === "pass" &&
      metricRow(i, "rss/soak").unavailableReason === undefined,
    expect: ["metric-result-missing", "metric-unmeasured-pass"],
  },
  {
    name: "unavailable lifecycle evidence",
    mutate: (i) =>
      Object.assign(metricRow(i, "lifecycle/soak-drain"), {
        result: "unavailable",
        unavailableReason: "not run",
        outcomes: {},
      }),
    applied: ({ plan, report }) =>
      findRow(plan.closure.metricCells, (c) => c.row === "lifecycle/soak-drain").evidenceKind ===
        "lifecycle" && metricRow({ report }, "lifecycle/soak-drain").result === "unavailable",
    expect: ["metric-required-evidence-unavailable"],
  },
  {
    name: "an unavailable soak whose run's lifecycle evidence row is absent",
    mutate: (i) => {
      for (const name of ["rss/soak", "lifecycle/soak-drain"]) {
        const row = metricRow(i, name);
        Object.assign(row, { result: "unavailable", unavailableReason: "soak not run" });
        delete row.outcomes;
      }
      dropRow(i.report.evidence, (r) => r.evidenceRun === "rev11-l1-soak");
    },
    applied: ({ report }) =>
      !report.evidence.some((r) => r.evidenceRun === "rev11-l1-soak") &&
      report.metrics.every((r) => r.evidenceRun !== "rev11-l1-soak" || r.result === "unavailable"),
    expect: ["metric-required-evidence-unavailable", "metric-run-evidence-missing"],
  },
  {
    name: "an evidence row that executed only part of its selected work",
    mutate: (i) => (evidenceRow(i, "ev-wasm").terminalSummary.executed = 1),
    applied: (i) =>
      evidenceRow(i, "ev-wasm").terminalSummary.selected === 10 &&
      evidenceRow(i, "ev-wasm").terminalSummary.executed === 1,
    expect: ["evidence-partial-execution"],
  },
  {
    name: "a preview surface reporting unsupported",
    mutate: ({ report }) =>
      (findRow(report.capabilities, (r) => r.surface === "host-lint").typedResult = "unsupported"),
    applied: ({ report }) =>
      findRow(report.capabilities, (r) => r.surface === "host-lint").typedResult === "unsupported",
    expect: ["capability-preview-not-partial"],
  },
  {
    name: "a required outcome moved onto the unselected optional owner",
    mutate: (i) => {
      ownerRow(i, "V9").outcomeIds = ownerRow(i, "P-COND").outcomeIds;
      delete ownerRow(i, "P-COND").outcomeIds;
    },
    applied: (i) =>
      planTask(i, "V9").selected === false && ownerRow(i, "V9").outcomeIds?.length > 0,
    expect: ["outcome-owner-ineligible"],
  },
  {
    name: "a detached task classed optional",
    mutate: ({ report }) => report.owners.push({ slug: "DETACHED", classification: "optional" }),
    applied: (i) => hasOwner(i, "DETACHED") && planTask(i, "DETACHED").optional === false,
    expect: ["owner-pathless"],
  },
  {
    name: "a refused result reused as a refusal under a non-refusal class",
    mutate: (i) =>
      Object.assign(admission(i, "complete-fact"), { result: "Refused", reusedAs: "refusal" }),
    applied: (i) => admission(i, "complete-fact").reusedAs === "refusal",
    expect: ["context-poisoned-reuse"],
  },
  {
    name: "an admission class disagreeing with the export",
    mutate: (i) => (admission(i, "certified").class = "CompleteFact"),
    applied: (i) => admission(i, "certified").class === "CompleteFact",
    expect: ["reuse-inventory-incomplete"],
  },
  {
    name: "a typed-reuse inventory that omits its admissions",
    mutate: ({ report }) => delete report.inventories.reuse.admissions,
    applied: ({ report }) => report.inventories.reuse.admissions === undefined,
    expect: ["reuse-inventory-incomplete"],
  },
  {
    name: "a declared guard family omitted while another of its kind remains",
    mutate: ({ plan, report }) => {
      plan.closure.engineGuardFamilies.push({ family: "feature-family-2", kind: "feature" });
      report.inventories.engine.guards.push({
        ...findRow(report.inventories.engine.guards, (g) => g.kind === "feature"),
        family: "feature-family-2",
        executedTests: 0,
      });
      dropRow(report.inventories.engine.guards, (g) => g.family === "feature-family-2");
    },
    applied: ({ plan, report }) =>
      plan.closure.engineGuardFamilies.some((g) => g.family === "feature-family-2") &&
      report.inventories.engine.guards.some((g) => g.kind === "feature"),
    expect: ["engine-inventory-incomplete"],
  },
  {
    name: "a declared observe inventory file omitted",
    mutate: ({ plan }) =>
      plan.closure.observeOwners.push({
        owner: "S-ENG",
        path: "crates/engine/observe-inventory/S-ENG.md",
      }),
    applied: ({ report }) => !report.inventories.observe.files.some((f) => f.owner === "S-ENG"),
    expect: ["observe-inventory-incomplete"],
  },
  {
    name: "an observe inventory file the export does not declare",
    mutate: ({ report }) =>
      report.inventories.observe.files.push({
        ...structuredClone(report.inventories.observe.files[0]),
        owner: "INVENTED",
        path: "crates/x/observe-inventory/INVENTED.md",
      }),
    applied: ({ report }) => report.inventories.observe.files.some((f) => f.owner === "INVENTED"),
    expect: ["observe-inventory-incomplete"],
  },
  {
    name: "a report whose status claim contradicts the ledger",
    mutate: (i) => {
      ownerRow(i, "S-OBS").status = "implemented";
      findRow(i.plan.ledger, owner("S-OBS")).status = "pending";
    },
    applied: (i) => ledgerStatus(i, "S-OBS") === "pending",
    expect: ["ancestor-pending", "owner-status-mismatch"],
  },
  {
    name: "a report for the other mode",
    mutate: ({ report }) => (report.mode = "owner-lock"),
    applied: ({ report }) => report.mode === "owner-lock",
    expect: ["report-mode-mismatch"],
  },
  {
    name: "a report for another target",
    mutate: ({ report }) => (report.target = "L4-A"),
    applied: ({ report }) => report.target === "L4-A",
    expect: ["report-target-mismatch"],
  },
];

function owner2(slug) {
  return (row) => row.owner === slug;
}

for (const control of CONTROLS) {
  test(`closure rejects ${control.name}`, () => {
    assertRejected("closure", base, control);
  });
}

const EXPORT_FIELDS = [
  ["tasks", ({ plan }) => delete plan.tasks],
  ["ledger", ({ plan }) => delete plan.ledger],
  ["closure", ({ plan }) => delete plan.closure],
  ["closure.namedOwners", ({ plan }) => delete plan.closure.namedOwners],
  ["closure.requiredOutcomes", ({ plan }) => delete plan.closure.requiredOutcomes],
  ["closure.capabilitySurfaces", ({ plan }) => delete plan.closure.capabilitySurfaces],
  ["closure.metricCells", ({ plan }) => delete plan.closure.metricCells],
  ["closure.wsp6Counts", ({ plan }) => delete plan.closure.wsp6Counts],
  ["closure.engineRelocations", ({ plan }) => delete plan.closure.engineRelocations],
  ["closure.engineGuardFamilies", ({ plan }) => delete plan.closure.engineGuardFamilies],
  ["closure.futureCharterSurfaces", ({ plan }) => delete plan.closure.futureCharterSurfaces],
  ["closure.observeOwners", ({ plan }) => delete plan.closure.observeOwners],
  ["closure.reuseAdmissions", ({ plan }) => delete plan.closure.reuseAdmissions],
  ["closure.metricCells[0].modes", ({ plan }) => delete plan.closure.metricCells[0].modes],
  [
    "closure.metricCells[0].evidenceKind",
    ({ plan }) => delete plan.closure.metricCells[0].evidenceKind,
  ],
  [
    "closure.capabilitySurfaces[1].cases",
    ({ plan }) => delete plan.closure.capabilitySurfaces[1].cases,
  ],
  ["closure.metricCells[0]", ({ plan }) => (plan.closure.metricCells[0] = null)],
  ["closure.capabilitySurfaces[0]", ({ plan }) => (plan.closure.capabilitySurfaces[0] = null)],
];

const EMPTY_POPULATIONS = ["requiredOutcomes", "namedOwners", "capabilitySurfaces", "wsp6Counts"];

for (const field of EMPTY_POPULATIONS) {
  test(`an export whose closure.${field} is empty fails closed`, () => {
    const decision = assertRejected("closure", base, {
      name: field,
      mutate: ({ plan }) => (plan.closure[field] = []),
      applied: ({ plan }) => plan.closure[field].length === 0,
      expect: ["plan-field-missing"],
    });
    assert.deepEqual(
      decision.errors.map((e) => e.subject),
      [`closure.${field}`],
    );
  });
}

for (const [subject, mutate] of EXPORT_FIELDS) {
  test(`an export without ${subject} fails closed instead of inferring the population`, () => {
    const decision = assertRejected("closure", base, {
      name: subject,
      mutate,
      applied: () => true,
      expect: ["plan-field-missing"],
    });
    assert.deepEqual(
      decision.errors.map((e) => e.subject),
      [subject],
    );
  });
}

test("an optional task with no selection mark fails closed", () => {
  const decision = assertRejected("closure", base, {
    name: "selection mark removed",
    mutate: (i) => delete planTask(i, "V9").selected,
    applied: (i) => planTask(i, "V9").selected === undefined,
    expect: ["plan-field-missing"],
  });
  assert.match(decision.errors[0].subject, /\.selected$/);
});

test("a population listing one entry twice fails closed", () => {
  const decision = assertRejected("closure", base, {
    name: "doubled outcome",
    mutate: ({ plan }) => plan.closure.requiredOutcomes.push("proposal-01"),
    applied: ({ plan }) =>
      countOf(plan.closure.requiredOutcomes, (id) => id === "proposal-01") === 2,
    expect: ["plan-duplicate-entry"],
  });
  assert.deepEqual(
    decision.errors.map((e) => e.subject),
    ["closure.requiredOutcomes[3]"],
  );
});

test("an evidence summary reconciles its declared expected skips", () => {
  const inputs = structuredClone(base);
  Object.assign(evidenceRow(inputs, "ev-wasm").terminalSummary, { executed: 9, expectedSkips: 1 });
  assert.deepEqual(validateClosure({ mode: "closure", ...inputs }).errors, []);
});

test("a predecessor outside the export and a doubled ledger row fail closed", () => {
  assertRejected("closure", base, {
    name: "dangling predecessor",
    mutate: (i) => planTask(i, "S-OBS").predecessors.push("ABSENT"),
    applied: (i) => planTask(i, "S-OBS").predecessors.includes("ABSENT"),
    expect: ["plan-dangling-predecessor"],
  });
  assertRejected("closure", base, {
    name: "doubled ledger row",
    mutate: ({ plan }) => plan.ledger.push({ slug: "H0", status: "pending" }),
    applied: ({ plan }) => plan.ledger.filter(owner("H0")).length === 2,
    expect: ["plan-duplicate-ledger-row"],
  });
});

test("a task outside the population may depend on a task outside the export", () => {
  const inputs = structuredClone(base);
  planTask(inputs, "LATER").predecessors.push("another-plan-task");
  assert.deepEqual(validateClosure({ mode: "closure", ...inputs }).errors, []);
});

test("a selected conditional owner with a real path joins the required population", () => {
  const inputs = structuredClone(base);
  planTask(inputs, "V9").selected = true;
  planTask(inputs, "L4").predecessors.push("V9");
  const unimplemented = validateClosure({ mode: "closure", ...inputs });
  assert.deepEqual(
    unimplemented.errors.map((e) => [e.code, e.subject]).filter(([, subject]) => subject === "V9"),
    [
      ["owner-classification-mismatch", "V9"],
      ["owner-consumer-missing", "V9"],
      ["owner-evidence-missing", "V9"],
      ["owner-final-owner-missing", "V9"],
      ["owner-route-disposition-missing", "V9"],
      ["required-owner-ledger-absent", "V9"],
    ],
  );
  inputs.plan.ledger.push({ slug: "V9", status: "implemented" });
  Object.assign(ownerRow(inputs, "V9"), {
    classification: "required",
    soleFinalOwner: "V9",
    consumers: ["semantic storage"],
    displacedRoutes: [],
  });
  inputs.report.evidence.push({ ...evidenceRow(inputs, "ev-wasm"), id: "ev-storage", owner: "V9" });
  assert.deepEqual(validateClosure({ mode: "closure", ...inputs }).errors, []);
});

test("an unrelated pending task and an unselected conditional owner never block closure", () => {
  assert.equal(ledgerStatus(base, "LATER"), "pending");
  assert.equal(planTask(base, "V9").selected, false);
  assert.equal(validateClosure({ mode: "closure", ...base }).ok, true);
});

test("an unavailable metric cell with a reason is accepted and reported as advisory", () => {
  const inputs = structuredClone(base);
  const row = findRow(inputs.report.metrics, (r) => r.row === "rss/soak");
  row.result = "unavailable";
  row.unavailableReason = "the worker has no process-tree sampler";
  row.worker = { id: "w", tags: ["linux"] };
  delete row.outcomes;
  const decision = validateClosure({ mode: "closure", ...inputs });
  assert.deepEqual(decision.errors, []);
  assert.deepEqual(decision.advisories, [
    { code: "metric-unavailable", subject: "rss/soak", message: row.unavailableReason },
  ]);
});

test("a dependency cycle terminates on the visited set", () => {
  const inputs = structuredClone(base);
  planTask(inputs, "H0").predecessors.push("S-ENG");
  const decision = validateClosure({ mode: "closure", ...inputs });
  assert.deepEqual(decision.errors, []);
});

test("every named closure defect keeps a negative control", () => {
  const codes = new Set(CONTROLS.flatMap((control) => control.expect));
  for (const code of [
    "owner-row-missing",
    "ancestor-pending",
    "owner-pathless",
    "conditional-owner-pathless",
    "train-review-missing",
    "capability-row-missing",
    "capability-cell-missing",
    "metric-result-missing",
    "metric-worker-not-allowed",
    "owner-outcome-missing",
    "owner-consumer-missing",
    "owner-evidence-missing",
    "surviving-unowned-route",
    "ecrv8-evidence-missing",
    "ecrx2-evidence-missing",
    "rel3-evidence-missing",
    "refusal-as-complete",
    "context-poisoned-reuse",
    "optional-default-hot-bookkeeping",
    "wsp6-count-absent",
    "moved-guard-not-executing",
    "metric-row-duplicate",
    "train-review-duplicate",
    "capability-row-duplicate",
    "metric-failing",
    "metric-required-evidence-unavailable",
    "metric-run-evidence-missing",
    "evidence-partial-execution",
    "outcome-owner-ineligible",
    "observe-inventory-incomplete",
    "reuse-inventory-incomplete",
  ]) {
    assert.ok(codes.has(code), `no control exercises ${code}`);
  }
});
