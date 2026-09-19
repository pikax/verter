// node --test tests/workspace-responsiveness/WSP1B/wsp1b.test.mjs

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const products = join(here, "products");

function load(name) {
  return JSON.parse(readFileSync(join(products, name), "utf8"));
}

test("WSP1B-AC-OWNER records one final owner and the WSP1A retirement obligation", () => {
  const ownership = load("ownership-map.json");
  assert.equal(ownership.finalOwner, "expansion.workspace-responsiveness");
  assert.match(ownership.retirementObligation, /unreproduced \/ not planned/);
  assert.equal(ownership.deletionPopulationThisNode.length, 0);
  assert.deepEqual(ownership.conflictDomains, [
    "lsp_publication",
    "performance_evidence",
    "scheduler_admission",
  ]);
  const early = ownership.outcomes.find((row) => row.id === "Issue93EarlyVerification");
  const carry = ownership.outcomes.find((row) => row.id === "Issue93CarryForwardCase");
  assert.equal(early.finalOwner, "expansion.workspace-responsiveness");
  assert.equal(carry.finalOwner, "expansion.workspace-responsiveness");
});

test("WSP1B never claims historical issue 93 was repaired", () => {
  const rec = load("issue93-early-verification.v1.json");
  assert.equal(rec.historicalIssue93.state, "unreproduced");
  assert.equal(rec.historicalIssue93.notAFix, true);
  assert.equal(rec.largeRun.claimsIssue93Fixed, false);
  assert.notEqual(rec.historicalIssue93.state, "fixed");
  assert.notEqual(rec.status, "fixed");
  const carry = load("issue93-carry-forward.v1.json");
  assert.equal(carry.historicalIssue93.notAFix, true);
  assert.equal(carry.historicalIssue93.state, "unreproduced");
});

test("WSP1B pins the large-project workload as a PrimeVue-scale equivalent, not a two-file smoke", () => {
  const rec = load("issue93-early-verification.v1.json");
  assert.equal(rec.largeRun.workload.class, "large-project");
  assert.ok(rec.largeRun.workload.vueFileCount >= 1000);
  assert.equal(rec.largeRun.workload.vueFileCount, 2615);
  assert.match(rec.largeRun.workload.sourcePin, /synthetic-15k/);
  assert.match(rec.largeRun.workload.privateCorpusLabel, /PrimeVue/);
  assert.equal(rec.smallSmoke.vueFileCount, 2);
});

test("WSP1B-AC-EXPOSURE keeps promotion blocked until DX1", () => {
  const exposure = load("exposure-registration.v1.json");
  assert.equal(exposure.dx1SchemaOwner, "DX1");
  assert.equal(exposure.operations.length, 2);
  for (const op of exposure.operations) {
    assert.equal(op.playground.promotionBlocked, true);
    assert.equal(op.playground.status, "missing");
    assert.equal(op.exposure.state, "registered");
    assert.equal(op.hostExecutionClass, "NativeOnly");
  }
});

test("WSP1B-AC-RESOURCE does not guess zeros for unavailable metrics", () => {
  const rec = load("issue93-early-verification.v1.json");
  for (const metric of [
    rec.largeRun.resources.clientPaint,
    rec.largeRun.resources.providerProcessCost,
    rec.largeRun.resources.outboundBytes,
    rec.largeRun.resources.retainedMemory,
    rec.largeRun.duration,
  ]) {
    if (metric.status === "unknown") {
      assert.equal(typeof metric.reason, "string");
      assert.ok(metric.reason.length > 0);
      assert.equal(metric.value, undefined);
    } else {
      assert.equal(metric.status, "measured");
      assert.equal(typeof metric.value, "number");
    }
  }
});

test("WSP1B large-project capture is a real driven Lapce session, not a protocol smoke", () => {
  const capture = load("large-project-capture.v1.json");
  assert.equal(capture.schema, "driven-lapce-capture.v1");
  assert.match(capture.lapceClient.version, /^0\.4\.6/);
  assert.ok(capture.capturedLines.some((line) => line.includes("verter launch-stamp")));
  assert.ok(capture.capturedLines.some((line) => line.includes("verter ui-stamp")));
  const kinds = capture.correlated.steps.map((step) => step.kind);
  for (const kind of ["open", "type", "complete", "navigate", "close"]) {
    assert.ok(kinds.includes(kind), `capture drives '${kind}'`);
  }
  assert.equal(capture.correlated.steps.length, capture.correlated.serverTraces.length);
  const rec = load("issue93-early-verification.v1.json");
  assert.notEqual(rec.status, "qualified");
  assert.equal(rec.status, "unverified");
  assert.equal(rec.largeRun.realClientEvidence, true);
  assert.equal(rec.largeRun.hostKind, "real-lapce");
  assert.equal(rec.historicalIssue93.notAFix, true);
  assert.equal(rec.largeRun.sameDocumentReopen, false);
  assert.equal(rec.operatorManualProbe.lapce, "0.4.6");
  assert.equal(rec.smallSmoke.surface.includes("WSP1L"), true);
});

test("WSP1B carry-forward case is the retained scenario for WSP8", () => {
  const carry = load("issue93-carry-forward.v1.json");
  assert.equal(carry.id, "Issue93CarryForwardCase");
  assert.deepEqual(carry.reusedAfter, ["WSP8", "ED1"]);
  assert.equal(carry.reopenAfterClose, true);
  assert.notEqual(carry.qualificationStatus, "qualified");
  const kinds = carry.script.map((step) => step.kind);
  for (const kind of ["open", "type", "complete", "navigate", "close"]) {
    assert.ok(kinds.includes(kind), `script drives '${kind}'`);
  }
  assert.ok(carry.script.filter((step) => step.kind === "type").length >= 3);
  assert.ok(carry.script.some((step) => String(step.role).startsWith("reopen-")));
  assert.ok(typeof carry.diagnostics === "string" && carry.diagnostics.length > 0);
  assert.ok(typeof carry.teardown === "string" && carry.teardown.length > 0);
});
