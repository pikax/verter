// node --test tests/workspace-responsiveness/WSP1/wsp1.test.mjs

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

test("WSP1-AC-OWNER records one final owner and WSP1L retirement", () => {
  const ownership = load("ownership-map.json");
  assert.equal(ownership.finalOwner, "expansion.workspace-responsiveness");
  assert.match(ownership.retirementObligation, /WSP1L/);
  assert.equal(ownership.deletionPopulationThisNode.length, 0);
  assert.deepEqual(ownership.conflictDomains, [
    "lsp_publication",
    "performance_evidence",
    "scheduler_admission",
  ]);
});

test("WSP1 Issue 93 stays unreproduced and is not labelled fixed", () => {
  const rec = load("issue93-reproduction.v1.json");
  assert.equal(rec.reproduction.state, "unreproduced");
  assert.equal(rec.reproduction.notAFix, true);
  assert.equal(rec.originalIssue.disposition, "unverified");
  assert.notEqual(rec.reproduction.state, "fixed");
  assert.ok(rec.reproduction.reasons.length >= 3);
  assert.ok(rec.versionCapture.every((row) => row.version === null));
});

test("WSP1-AC-EXPOSURE keeps promotion blocked until DX1", () => {
  const exposure = load("exposure-registration.v1.json");
  assert.equal(exposure.dx1SchemaOwner, "DX1");
  assert.equal(exposure.operations.length, 2);
  for (const op of exposure.operations) {
    assert.equal(op.playground.promotionBlocked, true);
    assert.equal(op.playground.status, "missing");
    assert.equal(op.exposure.state, "registered");
  }
});

test("WSP1-AC-RESOURCE does not guess zeros for unavailable metrics", () => {
  const ownership = load("ownership-map.json");
  assert.equal(
    ownership.outcomes.find((row) => row.id === "Issue93ReproCase").state,
    "unreproduced",
  );
  const rec = load("issue93-reproduction.v1.json");
  assert.equal(rec.reproduction.protocolHarness.cannotCertify, "Lapce issue 93");
});
