import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  loadManifest,
  loadProducts,
  mandatoryCases,
  parseWorkspacePackageEntries,
  selectedCaseIds,
  validate,
} from "./verify.mjs";

const clean = loadProducts();

const cloneProducts = () => structuredClone(clean);

test("ARH0-ratification: clean products validate and cover every mandatory case surface", () => {
  const result = validate(clean);
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual(mandatoryCases().sort(), [
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

test("ARH0-ratification dirty twin: manifest recording an unimplemented case is rejected (AC1)", () => {
  const dirtyManifest = structuredClone(loadManifest());
  dirtyManifest.cases.push({
    id: "ARH0-phantom",
    disposition: "reject",
    twins: ["clean products"],
  });
  const result = validate(cloneProducts(), dirtyManifest);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH0-ratification" && e.code === "manifest-case-drift"),
    JSON.stringify(result.errors),
  );
  assert.ok(selectedCaseIds(result).includes("ARH0-ratification"));
});

test("ARH0-ratification dirty twin: manifest dropping an implemented case is rejected (AC1)", () => {
  const dirtyManifest = structuredClone(loadManifest());
  const idx = dirtyManifest.cases.findIndex((c) => c.id === "ARH0-god-evidence");
  dirtyManifest.cases.splice(idx, 1);
  const result = validate(cloneProducts(), dirtyManifest);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH0-ratification" &&
        e.code === "manifest-case-drift" &&
        e.detail.includes("ARH0-god-evidence"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-ratification dirty twin: manifest verify command naming an absent script is rejected", () => {
  const dirtyManifest = structuredClone(loadManifest());
  dirtyManifest.verify = "node tests/architecture-health/ARH0/gone.mjs";
  const result = validate(cloneProducts(), dirtyManifest);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH0-ratification" && e.code === "manifest-command-drift",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-ratification dirty twin: a different existing script is not the canonical verify command", () => {
  const dirtyManifest = structuredClone(loadManifest());
  // Shape-valid and on disk, but it does not run the ARH0 verifier.
  dirtyManifest.verify = "node scripts/affected-tests.mjs";
  const result = validate(cloneProducts(), dirtyManifest);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH0-ratification" &&
        e.code === "manifest-command-drift" &&
        e.detail.includes("canonical"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-god-evidence dirty twin: size-only god module is rejected (AC2)", () => {
  const dirty = cloneProducts();
  const row = dirty["responsibility-map"].godModuleCandidates[0];
  row.responsibilities = ["one responsibility"]; // below the >=2 bar
  row.evidenceKinds = ["size"];
  row.couplingEvidence = {};
  const result = validate(dirty);
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
  const result = validate(dirty);
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

test("ARH0-god-evidence dirty twin: zero coupling count is not coupling evidence (AC2)", () => {
  const dirty = cloneProducts();
  const row = dirty["responsibility-map"].godModuleCandidates.find(
    (r) => r.path === "crates/verter_session/src/semantic_query.rs",
  );
  // Measured evidence of NO coupling must not qualify as a god module.
  row.couplingEvidence = { fanIn: 0, touchesSinceJune: row.couplingEvidence.touchesSinceJune };
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH0-god-evidence" && e.code === "god-without-responsibility-evidence",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-god-evidence dirty twin: negative shared-commit count is not coupling evidence (AC2)", () => {
  const dirty = cloneProducts();
  const row = dirty["responsibility-map"].godModuleCandidates[0];
  row.couplingEvidence = { sharedCommits: -3 };
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH0-god-evidence" && e.code === "god-without-responsibility-evidence",
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
  const result = validate(dirty);
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
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH0-capability" && e.code === "version-not-pinned-in-source",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-capability dirty twin: another dependency's version in the same file is rejected", () => {
  const dirty = cloneProducts();
  const row = dirty["capability-matrix"].rows.find(
    (r) => r.capability === "svelte-compilation-conformance",
  );
  // 3.6.0-rc.5 IS in package.json — but as the Vue pin, not Svelte's. The
  // binding is the named property, so the same-file occurrence must fail.
  row.version = "3.6.0-rc.5";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH0-capability" &&
        e.code === "version-not-pinned-in-source" &&
        e.detail.includes("devDependencies.svelte"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-capability dirty twin: implemented row without a versionProperty binding is rejected", () => {
  const dirty = cloneProducts();
  const row = dirty["capability-matrix"].rows.find((r) => r.capability === "typescript-tsgo-plane");
  delete row.versionProperty;
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH0-capability" && e.code === "version-without-property",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-capability dirty twin: build identity missing from its source is rejected", () => {
  const dirty = cloneProducts();
  const row = dirty["capability-matrix"].rows.find((r) => r.capability === "transport-schemas");
  row.buildIdentity.value = "protoc-gen-go";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH0-capability" && e.code === "build-identity-not-in-source",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-capability dirty twin: traversal versionSource escaping the repo tree is rejected", () => {
  const dirty = cloneProducts();
  const row = dirty["capability-matrix"].rows.find((r) => r.capability === "rust-parser-substrate");
  // `..` normalizes to the worktree parent, which exists — a bare existence
  // check accepts it, but the row must bind the repository tree.
  row.versionSource = "..";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH0-capability" &&
        e.code === "missing-version-source" &&
        e.detail.includes("rust-parser-substrate"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-capability dirty twin: null versionSource without database provenance is rejected", () => {
  const dirty = cloneProducts();
  const row = dirty["capability-matrix"].rows.find((r) => r.capability === "solid-2-target");
  assert.equal(row.provenance, "tama-dag");
  row.provenance = undefined;
  row.versionSource = null;
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH0-capability" && e.code === "missing-version-source",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-debt dirty twin: disposition owner outside the id discipline is rejected", () => {
  const dirty = cloneProducts();
  dirty["debt-register"].rows[0].owner = { id: "simp99", kind: "node" };
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH0-debt" && e.code === "malformed-disposition-owner"),
    JSON.stringify(result.errors),
  );
});

test("ARH0-debt dirty twin: null candidatePath without database provenance is rejected", () => {
  const dirty = cloneProducts();
  const row = dirty["debt-register"].rows.find((r) => r.id === "ARH0-DEBT-2");
  assert.equal(row.provenance, "tama-dag");
  row.provenance = undefined;
  row.candidatePath = null;
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH0-debt" && e.code === "missing-candidate-path"),
    JSON.stringify(result.errors),
  );
});

test("ARH0-ownership dirty twin: owner train id outside the dotted-id discipline is rejected", () => {
  const dirty = cloneProducts();
  dirty["responsibility-map"].owners.push({
    module: "crates/verter_parser",
    owner: { id: "expansion", kind: "train" },
    responsibility: ["invented owner"],
    evidence: ["fabricated"],
  });
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH0-ownership" && e.code === "malformed-owner"),
    JSON.stringify(result.errors),
  );
});

test("ARH0-ownership dirty twin: inventory module in neither owners nor debt-register is rejected", () => {
  const dirty = cloneProducts();
  const owners = dirty["responsibility-map"].owners;
  const idx = owners.findIndex((r) => r.module === "crates/verter_parser");
  owners.splice(idx, 1);
  const result = validate(dirty);
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

test("ARH0-ownership dirty twin: pnpm workspace package outside the inventory populations cannot be silently absorbed", () => {
  const dirty = cloneProducts();
  const owners = dirty["responsibility-map"].owners;
  const idx = owners.findIndex((r) => r.module === "docs");
  assert.ok(idx !== -1, "docs owner row missing from the clean products");
  owners.splice(idx, 1);
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH0-ownership" &&
        e.code === "workspace-package-unowned" &&
        e.detail.startsWith("docs"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH0-ownership: workspace package parsing survives comments, quotes and inline comments", () => {
  const yaml = [
    "packages:",
    "  # pnpm globs expand to inventory population rows",
    '  - "packages/*"',
    "  - 'docs' # single-quoted literal with an inline comment",
    "  - examples # unquoted literal with an inline comment",
    "  - 'packages/native/npm/*'",
    "",
    "onlyBuiltDependencies:",
    '  - "@swc/core"',
    "  - esbuild",
  ].join("\n");
  // Entries after a comment line and in non-double-quote styles must still
  // reach the coverage join; entries of other lists must not leak into it.
  assert.deepEqual(parseWorkspacePackageEntries(yaml), ["docs", "examples"]);
  // A list terminated by the next key still ends: onlyBuiltDependencies
  // members are not workspace packages.
  const real = parseWorkspacePackageEntries(
    fs.readFileSync(new URL("../../../pnpm-workspace.yaml", import.meta.url), "utf8"),
  );
  assert.deepEqual(real, ["docs"]);
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
  const result = validate(dirty);
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
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH0-inventory" && e.code === "totals-mismatch"),
    JSON.stringify(result.errors),
  );
});
