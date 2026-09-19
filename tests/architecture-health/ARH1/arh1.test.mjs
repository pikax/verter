import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  loadArh0Products,
  loadManifest,
  loadProducts,
  mandatoryCases,
  measureImports,
  selectedCaseIds,
  validate,
} from "./verify.mjs";

const clean = loadProducts();
const arh0 = loadArh0Products();

const cloneProducts = () => structuredClone(clean);

const hotspot = (dirty, path) =>
  dirty["dependency-contracts"].hotspots.find((h) => h.path === path);
const SCHEDULER = "crates/verter_scheduler/src/scheduler.rs";
const FLOW_RETURN = "crates/verter_session/src/project_semantic_dispatch/flow_return.rs";
const PSD_BUILD = "crates/verter_session/src/project_semantic_dispatch/build.rs";
const SEMANTIC_QUERY = "crates/verter_session/src/semantic_query.rs";
const FLOW_SLICE = "crates/verter_session/src/flow_slice_content.rs";

test("ARH1-ratification: clean products validate and cover every mandatory case surface", () => {
  const result = validate(clean, loadManifest(), arh0);
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual(mandatoryCases().sort(), [
    "ARH1-constructor",
    "ARH1-cutover",
    "ARH1-hotspot-coverage",
    "ARH1-import-direction",
    "ARH1-ratification",
    "ARH1-split",
    "ARH1-state-lifetimes",
    "ARH1-surface",
  ]);
  const contracts = clean["dependency-contracts"];
  assert.equal(contracts.hotspots.length, 5);
  assert.ok(contracts.ac3Rationale.length > 0);
  assert.ok(contracts.ac5Rationale.length > 0);
  assert.ok(contracts.ac4Obligations.length >= 4);
  assert.ok(clean["cutover-register"].emptyDeletionSetRationale.length > 0);
  // The ARH0 god-module population is exactly the contract population.
  const godPaths = arh0["responsibility-map"].godModuleCandidates
    .filter((r) => r.classification === "god-module-candidate")
    .map((r) => r.path);
  assert.deepEqual(contracts.hotspots.map((h) => h.path).sort(), [...godPaths].sort());
});

test("ARH1-hotspot-coverage dirty twin: an ARH0 hotspot without a contract is rejected (AC1)", () => {
  const dirty = cloneProducts();
  const hotspots = dirty["dependency-contracts"].hotspots;
  const idx = hotspots.findIndex((h) => h.path === FLOW_SLICE);
  hotspots.splice(idx, 1);
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-hotspot-coverage" && e.code === "hotspot-without-contract",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-hotspot-coverage dirty twin: a contract row the inventory does not carry is rejected (AC1)", () => {
  const dirty = cloneProducts();
  const row = hotspot(dirty, SEMANTIC_QUERY);
  row.path = "crates/verter_session/src/semantic_query_memo/arena.rs";
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-hotspot-coverage" && e.code === "contract-without-inventory-row",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-hotspot-coverage dirty twin: dropped and invented responsibilities are both rejected (AC1)", () => {
  const dropped = cloneProducts();
  const row = hotspot(dropped, SCHEDULER);
  row.authority = row.authority.filter((a) => a.responsibility !== "batch coordination");
  let result = validate(dropped, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-hotspot-coverage" && e.code === "responsibility-dropped",
    ),
    JSON.stringify(result.errors),
  );

  const invented = cloneProducts();
  hotspot(invented, SCHEDULER).authority.push({
    responsibility: "typescript diagnostics formatting",
    survivingOwner: "crates/verter_scheduler/src/scheduler.rs",
    evidence: "dirty twin",
  });
  result = validate(invented, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-hotspot-coverage" && e.code === "responsibility-invented",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-import-direction dirty twin: declared-but-unmeasured and measured-but-undeclared imports are rejected", () => {
  const unmeasured = cloneProducts();
  hotspot(unmeasured, SCHEDULER).allowedImportDirection.verter.push("verter_semantic");
  let result = validate(unmeasured, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-import-direction" &&
        e.code === "import-drift" &&
        e.detail.includes("verter_semantic"),
    ),
    JSON.stringify(result.errors),
  );

  const undeclared = cloneProducts();
  const aid = hotspot(undeclared, SCHEDULER).allowedImportDirection;
  aid.verter = aid.verter.filter((v) => v !== "verter_language");
  result = validate(undeclared, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-import-direction" &&
        e.code === "import-drift" &&
        e.detail.includes("is not declared"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-import-direction dirty twin: importing a forbidden direction is rejected", () => {
  const dirty = cloneProducts();
  hotspot(dirty, SCHEDULER).allowedImportDirection.mustNotImport.push("verter_language");
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-import-direction" && e.code === "forbidden-import",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-import-direction dirty twin: a layer rule that contradicts the live manifest is rejected", () => {
  const violation = cloneProducts();
  const rule = violation["dependency-contracts"].layerRules.find((r) => r.id === "ARH1-LAYER-1");
  rule.mayImport = rule.mayImport.filter((d) => d !== "verter_language");
  let result = validate(violation, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-import-direction" && e.code === "layer-violation"),
    JSON.stringify(result.errors),
  );

  const contradiction = cloneProducts();
  const cRule = contradiction["dependency-contracts"].layerRules.find(
    (r) => r.id === "ARH1-LAYER-1",
  );
  cRule.mustNotImport.push("verter_span");
  result = validate(contradiction, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-import-direction" && e.code === "layer-rule-contradiction",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-import-direction dirty twin: an importer that stopped referencing the module is rejected", () => {
  const dirty = cloneProducts();
  hotspot(dirty, SEMANTIC_QUERY).allowedImportDirection.importers.push(
    "crates/verter_span/src/lib.rs",
  );
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-import-direction" && e.code === "importer-without-reference",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-import-direction: brace-list items inherit their prefix and never classify as roots", () => {
  const measured = measureImports(
    [
      "use std::sync::Arc;",
      "use crate::dag::{profile_hash_from_bytes, profile_hash_to_bytes};",
      "use crate::{job::CompletionHandle, node::FileNode};",
      "use verter_semantic::analysis::flow::{lower::Lower, value_descent};",
      "use dashmap::DashMap;",
      "pub use semantic_context::{project_union_order, SemanticContext};",
      "pub mod semantic_context;",
    ].join("\n"),
  );
  assert.deepEqual([...measured.verter].sort(), ["verter_semantic"]);
  assert.deepEqual([...measured.external].sort(), ["dashmap"]);
  assert.deepEqual([...measured.internal].sort(), ["dag", "job", "node"]);
});

test("ARH1-constructor dirty twin: a capability anchor missing from the declaration span is rejected", () => {
  const dirty = cloneProducts();
  const row = hotspot(dirty, SCHEDULER).constructorCapabilities.find(
    (c) => c.constructor === "new",
  );
  row.signatureAnchors.push("Arc<Mutex<Vec<Byte>>>"); // nothing rings this bell
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-constructor" && e.code === "constructor-anchor-missing",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-constructor dirty twin: a constructor that is not a declared fn is rejected", () => {
  const dirty = cloneProducts();
  hotspot(dirty, SCHEDULER).constructorCapabilities.push({
    constructor: "with_semantic_engine",
    signatureAnchors: ["Arc<dyn SourceLoader>"],
    rules: ["dirty twin"],
  });
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-constructor" && e.code === "missing-constructor"),
    JSON.stringify(result.errors),
  );
});

test("ARH1-constructor dirty twin: constructor-free hotspot without a rationale is rejected", () => {
  const dirty = cloneProducts();
  delete hotspot(dirty, FLOW_RETURN).constructorRationale;
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-constructor" && e.code === "constructor-rationale-missing",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-state-lifetimes dirty twin: lifetime outside the vocabulary, owner-less state and unknown identifiers are rejected", () => {
  const dirty = cloneProducts();
  const row = hotspot(dirty, SCHEDULER).stateLifetimes.find((s) => s.state === "Scheduler.overlay");
  row.lifetime = "forever";
  let result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-state-lifetimes" && e.code === "bad-lifetime"),
    JSON.stringify(result.errors),
  );

  const ownerless = cloneProducts();
  delete hotspot(ownerless, SCHEDULER).stateLifetimes.find((s) => s.state === "Scheduler.overlay")
    .soleOwner;
  result = validate(ownerless, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-state-lifetimes" && e.code === "state-without-owner",
    ),
    JSON.stringify(result.errors),
  );

  const unknown = cloneProducts();
  hotspot(unknown, SCHEDULER).stateLifetimes.push({
    state: "xyzzy_entanglement_ledger",
    owner: "Scheduler",
    lifetime: "session",
    soleOwner: SCHEDULER,
    evidence: "dirty twin",
  });
  result = validate(unknown, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-state-lifetimes" && e.code === "state-not-in-source",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-surface dirty twin: a drifted module visibility declaration is rejected", () => {
  const dirty = cloneProducts();
  const decl = hotspot(dirty, SEMANTIC_QUERY).surfaceDeclarations[0];
  decl.declaration = "pub(crate) mod semantic_query;";
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-surface" && e.code === "surface-declaration-drift",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-surface dirty twin: a retained item that is not a live declaration is rejected", () => {
  const dirty = cloneProducts();
  hotspot(dirty, SCHEDULER).minimalPublicSurface.retainedFns.push("submit_everything");
  let result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-surface" && e.code === "surface-item-missing"),
    JSON.stringify(result.errors),
  );

  const unconsumed = cloneProducts();
  hotspot(unconsumed, SEMANTIC_QUERY).minimalPublicSurface.retainedTypes.push(
    "TotallyInternalProjectionGizmo",
  );
  result = validate(unconsumed, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-surface" && e.code === "retained-item-unconsumed"),
    JSON.stringify(result.errors),
  );
});

test("ARH1-surface dirty twin: narrowing to an invented visibility or naming absent consumers is rejected", () => {
  const dirty = cloneProducts();
  const row = hotspot(dirty, SCHEDULER).minimalPublicSurface.narrow.find(
    (n) => n.item === "tombstones",
  );
  row.to = "pub"; // widening is not a narrowing disposition
  let result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-surface" && e.code === "bad-narrow-target"),
    JSON.stringify(result.errors),
  );

  const missing = cloneProducts();
  const hook = hotspot(missing, SCHEDULER).minimalPublicSurface.narrow.find(
    (n) => n.item === "test_new",
  );
  hook.consumersAffected.push("crates/verter_session/src/gone.rs");
  result = validate(missing, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-surface" && e.code === "missing-narrow-consumer"),
    JSON.stringify(result.errors),
  );
});

test("ARH1-split dirty twin: a split retaining shared unrestricted state is rejected (AC2)", () => {
  const dirty = cloneProducts();
  const flowReturn = hotspot(dirty, FLOW_RETURN);
  const products = flowReturn.cohesiveModules.find((m) => m.module.endsWith("flow_products.rs"));
  products.stateOwnership.push({
    state: "flow-return compute frame (demand site + MaterializedSet)",
    mode: "shared", // moving methods into files while retaining shared state
  });
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-split" &&
        e.code === "split-retains-shared-state" &&
        e.detail.includes("shared unrestricted state"),
    ),
    JSON.stringify(result.errors),
  );
  assert.ok(selectedCaseIds(result).includes("ARH1-split"));
});

test("ARH1-split dirty twin: two modules claiming the same state sole are rejected (AC2)", () => {
  const dirty = cloneProducts();
  const flowReturn = hotspot(dirty, FLOW_RETURN);
  const accessor = flowReturn.cohesiveModules.find((m) =>
    m.module.endsWith("flow_return_products.rs"),
  );
  accessor.stateOwnership = [{ state: "mutable flow product state", mode: "sole" }];
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-split" && e.code === "state-double-owner"),
    JSON.stringify(result.errors),
  );
});

test("ARH1-split dirty twin: claiming an undeclared state is rejected", () => {
  const dirty = cloneProducts();
  const dag = hotspot(dirty, SCHEDULER).cohesiveModules.find((m) => m.module.endsWith("dag.rs"));
  dag.stateOwnership.push({ state: "Scheduler.secret_vibes", mode: "sole" });
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-split" && e.code === "state-ownership-undeclared"),
    JSON.stringify(result.errors),
  );
});

test("ARH1-split dirty twin: a cohesive module with an invented responsibility or absent path is rejected", () => {
  const invented = cloneProducts();
  hotspot(invented, SCHEDULER).cohesiveModules[0].responsibility = "release notes generation";
  let result = validate(invented, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-split" && e.code === "cohesive-responsibility-invented",
    ),
    JSON.stringify(result.errors),
  );

  const absent = cloneProducts();
  hotspot(absent, SCHEDULER).cohesiveModules.push({
    module: "crates/verter_scheduler/src/quantum.rs",
    stateOwnership: [],
  });
  result = validate(absent, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-split" && e.code === "missing-cohesive-module"),
    JSON.stringify(result.errors),
  );
});

test("ARH1-cutover dirty twin: an ARH0 debt row naming ARH1 left undecided is rejected (AC1)", () => {
  const dirty = cloneProducts();
  const rows = dirty["cutover-register"].rows;
  const idx = rows.findIndex((r) => r.satisfies === "ARH0-DEBT-1");
  rows.splice(idx, 1);
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-cutover" &&
        e.code === "arh0-debt-undecided" &&
        e.detail.includes("ARH0-DEBT-1"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-cutover dirty twin: a declared narrowing route without a register row is rejected", () => {
  const dirty = cloneProducts();
  const rows = dirty["cutover-register"].rows;
  const idx = rows.findIndex((r) => r.id === "ARH1-CUT-4");
  rows.splice(idx, 1);
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-cutover" &&
        e.code === "cutover-route-missing" &&
        e.detail.includes(SEMANTIC_QUERY),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-cutover dirty twin: a route without a concrete disposition is rejected", () => {
  const dirty = cloneProducts();
  dirty["cutover-register"].rows[0].disposition = "";
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-cutover" && e.code === "cutover-without-disposition",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-ratification dirty twin: manifest recording an unimplemented case is rejected (AC1)", () => {
  const dirtyManifest = structuredClone(loadManifest());
  dirtyManifest.cases.push({
    id: "ARH1-phantom",
    disposition: "reject",
    twins: ["clean products"],
  });
  const result = validate(cloneProducts(), dirtyManifest, arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-ratification" && e.code === "manifest-case-drift"),
    JSON.stringify(result.errors),
  );
});

test("ARH1-ratification dirty twin: manifest dropping an implemented case is rejected (AC1)", () => {
  const dirtyManifest = structuredClone(loadManifest());
  const idx = dirtyManifest.cases.findIndex((c) => c.id === "ARH1-split");
  dirtyManifest.cases.splice(idx, 1);
  const result = validate(cloneProducts(), dirtyManifest, arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-ratification" &&
        e.code === "manifest-case-drift" &&
        e.detail.includes("ARH1-split"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-ratification dirty twin: a different existing script is not the canonical verify command", () => {
  const dirtyManifest = structuredClone(loadManifest());
  // Shape-valid and on disk, but it does not run the ARH1 verifier.
  dirtyManifest.verify = "node scripts/affected-tests.mjs";
  const result = validate(cloneProducts(), dirtyManifest, arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-ratification" &&
        e.code === "manifest-command-drift" &&
        e.detail.includes("canonical"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-ratification dirty twin: a producer obligation with a malformed owner is rejected (AC4)", () => {
  const dirty = cloneProducts();
  dirty["dependency-contracts"].ac4Obligations.push({
    producer: { id: "doc2", kind: "node" },
    obligation: "dirty twin",
  });
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-ratification" && e.code === "obligation-producer-malformed",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-verify CLI: the manifest verify command runs validate() and exits 0 on the clean tree", () => {
  const verifyPath = fileURLToPath(new URL("./verify.mjs", import.meta.url));
  const stdout = execFileSync(process.execPath, [verifyPath], { encoding: "utf8" });
  assert.match(stdout, /ARH1 verify: PASS/);
});
