import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import test from "node:test";
import path from "node:path";
import { fileURLToPath } from "node:url";

import {
  loadAuthority,
  loadProducts,
  mandatoryCases,
  selectedCaseIds,
  validate,
} from "./verify.mjs";

const authority = loadAuthority();
const clean = loadProducts();

const cloneProducts = () => structuredClone(clean);

test("ARH0-ratification: clean products validate and cover every mandatory case surface", () => {
  const result = validate(clean, authority);
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual(mandatoryCases().sort(), [
    "ARH0-authority",
    "ARH0-capability",
    "ARH0-debt",
    "ARH0-god-evidence",
    "ARH0-inventory",
    "ARH0-ownership",
    "ARH0-ratification",
  ]);
  assert.ok(clean["responsibility-map"].ac3Rationale.length > 0);
  assert.ok(clean["responsibility-map"].ac4Rationale.length > 0);
  assert.ok(clean["debt-register"].emptyDeletionSetRationale.length > 0);
  // Fan-in/fan-out evidence really round-trips through the inventory product.
  const span = clean["codebase-inventory"].crates.find((c) => c.module === "crates/verter_span");
  assert.equal(span.fanIn, 24);
  const session = clean["codebase-inventory"].crates.find(
    (c) => c.module === "crates/verter_session",
  );
  assert.ok(session.productionLoc > 400_000 && session.testLoc > session.productionLoc);
});

test("ARH0-god-evidence dirty twin: size-only god module is rejected (AC2)", () => {
  const dirty = cloneProducts();
  const row = dirty["responsibility-map"].godModuleCandidates[0];
  row.responsibilities = ["one responsibility"]; // below the >=2 bar
  row.evidenceKinds = ["size"];
  row.couplingEvidence = {};
  const result = validate(dirty, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH0-god-evidence" && e.code === "god-without-responsibility-evidence",
    ),
    JSON.stringify(result.errors),
  );
  assert.ok(selectedCaseIds(result).includes("ARH0-god-evidence"));
});

test("ARH0-god-evidence dirty twin: touches-only coupling evidence is rejected (AC2)", () => {
  const dirty = cloneProducts();
  const row = dirty["responsibility-map"].godModuleCandidates.find(
    (r) => r.path === "crates/verter_session/src/flow_slice_content.rs",
  );
  // Touch count is churn, not coupling: strip fanIn/shared-commits, keep touches.
  row.couplingEvidence = { touchesSinceJune: row.couplingEvidence.touchesSinceJune };
  const result = validate(dirty, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH0-god-evidence" &&
        e.code === "god-without-responsibility-evidence" &&
        e.detail.includes("touches-only"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-god-evidence dirty twin: previously split Phase 11 target cannot return without fresh evidence (AC2)", () => {
  const dirty = cloneProducts();
  dirty["responsibility-map"].godModuleCandidates.push({
    path: "crates/verter_session/src/meta_resolve.rs",
    loc: 9999,
    classification: "god-module-candidate",
    responsibilities: ["meta resolution", "historical pre-split size"],
    couplingEvidence: { preSplitHistory: true },
    evidenceKinds: ["multi-responsibility", "coupling", "size"],
    provenance: "hand-written",
  });
  const result = validate(dirty, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH0-god-evidence" &&
        e.code === "split-module-reclassified-without-new-evidence",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-capability dirty twin: fabricated version pin is rejected", () => {
  const dirty = cloneProducts();
  const row = dirty["capability-matrix"].rows.find(
    (r) => r.capability === "svelte-compilation-conformance",
  );
  row.version = "5.99.0";
  const result = validate(dirty, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH0-capability" && e.code === "version-not-pinned-in-source",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-debt dirty twin: disposition owner outside authority and gap list is rejected", () => {
  const dirty = cloneProducts();
  dirty["debt-register"].rows[0].owner = { id: "SIMP99", kind: "node" };
  const result = validate(dirty, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH0-debt" && e.code === "unknown-disposition-owner"),
    JSON.stringify(result.errors),
  );
});

test("ARH0-ownership dirty twin: owner train without charters dir or gap annotation is rejected", () => {
  const dirty = cloneProducts();
  dirty["responsibility-map"].owners.push({
    module: "crates/verter_parser",
    owner: { id: "expansion.nope", kind: "train" },
    responsibility: ["invented owner"],
    evidence: ["fabricated"],
  });
  const result = validate(dirty, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH0-ownership" && e.code === "unknown-owner"),
    JSON.stringify(result.errors),
  );
});

test("ARH0-ownership dirty twin: inventory module in neither owners nor debt-register is rejected", () => {
  const dirty = cloneProducts();
  const owners = dirty["responsibility-map"].owners;
  const idx = owners.findIndex((r) => r.module === "crates/verter_parser");
  owners.splice(idx, 1);
  const result = validate(dirty, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH0-ownership" &&
        e.code === "inventory-module-unowned" &&
        e.detail.startsWith("crates/verter_parser"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-ownership dirty twin: module with both an owner row and a debt row is rejected", () => {
  const dirty = cloneProducts();
  dirty["debt-register"].rows.push({
    id: "ARH0-DEBT-TWIN",
    candidate: "crates/verter_parser",
    candidatePath: "crates/verter_parser",
    evidence: "dirty twin: already owned by a surviving owner",
    disposition: "twin-only",
    owner: { id: "ARH1", kind: "node" },
    class: "twin",
  });
  const result = validate(dirty, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH0-ownership" && e.code === "module-double-disposition",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-verify CLI: the manifest verify command runs validate() and exits 0 on the clean tree", () => {
  const verifyPath = fileURLToPath(new URL("./verify.mjs", import.meta.url));
  const stdout = execFileSync(process.execPath, [verifyPath], { encoding: "utf8" });
  assert.match(stdout, /ARH0 verify: PASS/);
});

test("ARH0-inventory dirty twin: drifted totals are rejected", () => {
  const dirty = cloneProducts();
  dirty["codebase-inventory"].rustWorkspace.productionLoc += 1;
  const result = validate(dirty, authority);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH0-inventory" && e.code === "totals-mismatch"),
    JSON.stringify(result.errors),
  );
});
