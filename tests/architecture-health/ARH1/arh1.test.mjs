import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  fieldUseForms,
  loadArh0Products,
  loadManifest,
  loadProducts,
  mandatoryCases,
  measureImports,
  measureProductionImports,
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

test("ARH1-import-direction dirty twin: a declared importer that references nothing is rejected by the exact population join", () => {
  const dirty = cloneProducts();
  hotspot(dirty, SEMANTIC_QUERY).allowedImportDirection.importers.push(
    "crates/verter_span/src/lib.rs",
  );
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-surface" &&
        e.code === "importer-population-drift" &&
        e.detail.includes("declared an importer but references no"),
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

test("ARH1-state-lifetimes dirty twin: an unrelated existing file as sole owner is rejected (AC1)", () => {
  const dirty = cloneProducts();
  const row = hotspot(dirty, SCHEDULER).stateLifetimes.find((s) => s.state === "Scheduler.nodes");
  // Exists on disk, declares nothing of the scheduler state: existence is
  // not ownership.
  row.soleOwner = "crates/verter_span/src/lib.rs";
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-state-lifetimes" && e.code === "state-owner-without-declaration",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-import-direction: test configuration is stripped before measuring, so crateInternal equality is two-way", () => {
  const synth = [
    "use crate::dag::Thing;",
    "#[cfg(test)]",
    "mod t1 { use crate::VerterHost; fn f() -> VerterHost { todo!() } }",
    '#[cfg(all(test, not(target_arch = "wasm32")))]',
    "mod t2 { use crate::pool::Pool; }",
    '#[cfg(any(test, feature = "test-support"))]',
    "mod t3 { use crate::stage::S; }",
  ].join("\n");
  const prod = measureProductionImports(synth);
  assert.deepEqual([...prod.internal].sort(), ["dag", "stage"]);
  const full = measureImports(synth);
  assert.ok(full.internal.has("VerterHost"), "full measurement still sees the test-only root");
  assert.ok(full.internal.has("pool"));
});

test("ARH1-import-direction dirty twin: an undeclared live crate-internal root is rejected both ways", () => {
  const undeclared = cloneProducts();
  const aid = hotspot(undeclared, SCHEDULER).allowedImportDirection;
  aid.crateInternal = aid.crateInternal.filter((v) => v !== "dag");
  let result = validate(undeclared, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-import-direction" &&
        e.code === "import-drift" &&
        e.detail.includes("crate-internal import dag is not declared"),
    ),
    JSON.stringify(result.errors),
  );

  const declaredOnlyInTests = cloneProducts();
  const aid2 = hotspot(declaredOnlyInTests, SCHEDULER).allowedImportDirection;
  aid2.crateInternal.push("cache_id"); // imported only by scheduler.rs cfg(test) code
  result = validate(declaredOnlyInTests, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-import-direction" &&
        e.code === "import-drift" &&
        e.detail.includes("crate-internal import cache_id is not measured"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-surface dirty twin: a hook consumer that references nothing and an omitted live consumer are both rejected", () => {
  const stale = cloneProducts();
  const row = hotspot(stale, SCHEDULER).minimalPublicSurface.narrow.find(
    (n) => n.item === "test_new",
  );
  // Exists and is a real hook consumer file — of a different hook.
  row.consumersAffected.push("crates/verter_session/src/host_construction.rs");
  let result = validate(stale, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-surface" && e.code === "narrow-consumer-without-reference",
    ),
    JSON.stringify(result.errors),
  );

  const omitted = cloneProducts();
  const row2 = hotspot(omitted, SCHEDULER).minimalPublicSurface.narrow.find(
    (n) => n.item === "test_new",
  );
  row2.consumersAffected = row2.consumersAffected.filter(
    (c) => !c.endsWith("host_batch_coordinator.rs"),
  );
  result = validate(omitted, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-surface" &&
        e.code === "narrow-consumer-omitted" &&
        e.detail.includes("host_batch_coordinator.rs"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-surface dirty twin: importer population drift in both directions is rejected (comments are not consumers)", () => {
  const missing = cloneProducts();
  const aid = hotspot(missing, SEMANTIC_QUERY).allowedImportDirection;
  aid.importers = aid.importers.filter((i) => i !== "crates/verter_ffi/src/convert/typeinfo.rs");
  let result = validate(missing, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-surface" &&
        e.code === "importer-population-drift" &&
        e.detail.includes("typeinfo.rs references"),
    ),
    JSON.stringify(result.errors),
  );

  const mirror = cloneProducts();
  // Only mentions semantic_query in mirror documentation; imports nothing.
  hotspot(mirror, SEMANTIC_QUERY).allowedImportDirection.importers.push(
    "crates/verter_audit/src/payloads/tags.rs",
  );
  result = validate(mirror, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-surface" &&
        e.code === "importer-population-drift" &&
        e.detail.includes("tags.rs is declared an importer but references no"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-surface dirty twin: a used assoc item or a narrowed-name consumer missing from the contract is rejected", () => {
  const unretainedOp = cloneProducts();
  const surface = hotspot(unretainedOp, SEMANTIC_QUERY).minimalPublicSurface;
  surface.retainedAssocItems = surface.retainedAssocItems.filter(
    (i) => i !== "PartialReasonSet::PROPAGATED",
  );
  let result = validate(unretainedOp, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-surface" &&
        e.code === "assoc-item-unretained" &&
        e.detail.includes("PartialReasonSet::PROPAGATED"),
    ),
    JSON.stringify(result.errors),
  );

  const unrecorded = cloneProducts();
  const bulk = hotspot(unrecorded, SEMANTIC_QUERY).minimalPublicSurface.narrow.find(
    (n) => n.kind === "bulk",
  );
  bulk.consumersAffected = bulk.consumersAffected.filter(
    (c) => !c.endsWith("flow_literal_provenance.rs"),
  );
  result = validate(unrecorded, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-surface" &&
        e.code === "narrowed-item-consumer-unrecorded" &&
        e.detail.includes("flow_literal_provenance.rs"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-ratification dirty twin: obligations and rationale naming no AC4 surface are rejected (AC4)", () => {
  const dirty = cloneProducts();
  const c = dirty["dependency-contracts"];
  for (const obligation of c.ac4Obligations) obligation.obligation = "dirty twin";
  c.ac4Rationale = "dirty twin";
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-ratification" &&
        e.code === "ac4-surface-uncovered" &&
        e.detail.includes("VIM/DX"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-cutover dirty twin: a duplicate disposition for one route is rejected", () => {
  const dirty = cloneProducts();
  const rows = dirty["cutover-register"].rows;
  const cut4 = rows.find((r) => r.id === "ARH1-CUT-4");
  rows.push({
    ...cut4,
    id: "ARH1-CUT-5",
    disposition: "competing disposition",
    satisfies: undefined,
  });
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-cutover" && e.code === "duplicate-cutover-route"),
    JSON.stringify(result.errors),
  );
});

test("ARH1-cutover dirty twin: one scheduler route vanishing while its sibling stays is rejected", () => {
  const dirty = cloneProducts();
  const rows = dirty["cutover-register"].rows;
  const idx = rows.findIndex((r) => r.id === "ARH1-CUT-2");
  rows.splice(idx, 1); // CUT-3 (fn route) remains on the same file
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-cutover" &&
        e.code === "cutover-route-missing" &&
        e.detail.includes("#field"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-cutover dirty twin: a competing owner deciding the same ARH0 debt twice is rejected", () => {
  const dirty = cloneProducts();
  const rows = dirty["cutover-register"].rows;
  const cut1 = rows.find((r) => r.id === "ARH1-CUT-1");
  rows.push({
    ...cut1,
    id: "ARH1-CUT-6",
    decision: "retain the placeholder",
    owner: { id: "ARH11", kind: "node" },
  });
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-cutover" && e.code === "duplicate-satisfies"),
    JSON.stringify(result.errors),
  );
});

test("ARH1-verify CLI: the manifest verify command runs validate() and exits 0 on the clean tree", () => {
  const verifyPath = fileURLToPath(new URL("./verify.mjs", import.meta.url));
  const stdout = execFileSync(process.execPath, [verifyPath], { encoding: "utf8" });
  assert.match(stdout, /ARH1 verify: PASS/);
});

// ---------------------------------------------------------------------------
// Layer-rule population pins (F2/F11): mayImport equals the live manifest
// both ways and the rule population cannot shrink or widen silently.
// ---------------------------------------------------------------------------

test("ARH1-import-direction dirty twin: an invented mayImport allowance is rejected both ways", () => {
  const invented = cloneProducts();
  const rule = invented["dependency-contracts"].layerRules.find((r) => r.id === "ARH1-LAYER-1");
  rule.mayImport.push("verter_fake");
  let result = validate(invented, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-import-direction" &&
        e.code === "layer-allowance-without-dependency" &&
        e.detail.includes("verter_fake"),
    ),
    JSON.stringify(result.errors),
  );

  const unused = cloneProducts();
  unused["dependency-contracts"].layerRules
    .find((r) => r.id === "ARH1-LAYER-1")
    .mayImport.push(
      "verter_macro_dto", // a real crate, but not a dependency of verter_scheduler
    );
  result = validate(unused, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-import-direction" && e.code === "layer-allowance-without-dependency",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-import-direction dirty twin: deleting a layer rule for a hotspot crate is rejected", () => {
  const dirty = cloneProducts();
  const rules = dirty["dependency-contracts"].layerRules;
  const idx = rules.findIndex((r) => r.id === "ARH1-LAYER-1"); // governs verter_scheduler
  rules.splice(idx, 1);
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-import-direction" && e.code === "missing-layer-rule",
    ),
    JSON.stringify(result.errors),
  );
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH1-import-direction" && e.code === "layer-rule-population-drift",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-import-direction dirty twin: deleting a middle layer rule leaves an id gap and is rejected", () => {
  const dirty = cloneProducts();
  const rules = dirty["dependency-contracts"].layerRules;
  const idx = rules.findIndex((r) => r.id === "ARH1-LAYER-2"); // verter_semantic: non-hotspot crate
  rules.splice(idx, 1);
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-import-direction" &&
        e.code === "layer-rule-population-drift" &&
        e.detail.includes("ARH1-LAYER-3"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-import-direction dirty twin: a rule for an ungoverned non-hotspot crate without rationale is rejected", () => {
  const dirty = cloneProducts();
  const template = dirty["dependency-contracts"].layerRules[0];
  dirty["dependency-contracts"].layerRules.push({
    ...structuredClone(template),
    id: "ARH1-LAYER-4",
    crate: "crates/verter_lsp",
    rule: "invented twin",
    mayImport: [...template.mayImport], // contents irrelevant: the crate is unexplained
  });
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-import-direction" &&
        e.code === "layer-crate-unexplained" &&
        e.detail.includes("verter_lsp"),
    ),
    JSON.stringify(result.errors),
  );
});

// ---------------------------------------------------------------------------
// Cutover key binding (F5/F10): every disposition binds exactly one
// recognized key, so a keyless row cannot record a competing owner.
// ---------------------------------------------------------------------------

test("ARH1-cutover dirty twin: a keyless row with a competing owner is rejected", () => {
  const dirty = cloneProducts();
  const rows = dirty["cutover-register"].rows;
  const cut2 = rows.find((r) => r.id === "ARH1-CUT-2");
  rows.push({
    ...cut2,
    id: "ARH1-CUT-5",
    decision: "keep the bookkeeping fields pub",
    disposition: "competing disposition with no route",
    owner: { id: "ARH11", kind: "node" },
    route: undefined,
    satisfies: undefined,
  });
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-cutover" && e.code === "cutover-row-unbound"),
    JSON.stringify(result.errors),
  );
});

test("ARH1-cutover dirty twin: a row carrying both keys is rejected", () => {
  const dirty = cloneProducts();
  const rows = dirty["cutover-register"].rows;
  const cut2 = rows.find((r) => r.id === "ARH1-CUT-2");
  cut2.satisfies = "ARH0-DEBT-1"; // route already set
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH1-cutover" && e.code === "cutover-row-double-keyed"),
    JSON.stringify(result.errors),
  );
});

// ---------------------------------------------------------------------------
// Exact importer inventories for every hotspot (F6): comment mentions are
// never consumers and a real consumer cannot be dropped.
// ---------------------------------------------------------------------------

test("ARH1-surface dirty twin: a comment-only scheduler importer is rejected (comments never count)", () => {
  const dirty = cloneProducts();
  const aid = hotspot(dirty, SCHEDULER).allowedImportDirection;
  // footprint.rs mentions "scheduler" in doc prose only.
  aid.importers.push("crates/verter_audit/src/footprint.rs");
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-surface" &&
        e.code === "importer-population-drift" &&
        e.detail.includes("footprint.rs is declared an importer but references no"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-surface dirty twin: dropping a real scheduler importer is rejected", () => {
  const dirty = cloneProducts();
  const aid = hotspot(dirty, SCHEDULER).allowedImportDirection;
  aid.importers = aid.importers.filter((i) => i !== "crates/verter_napi/src/lib.rs");
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-surface" &&
        e.code === "importer-population-drift" &&
        e.detail.includes("crates/verter_napi/src/lib.rs references scheduler cross-crate"),
    ),
    JSON.stringify(result.errors),
  );
});

// ---------------------------------------------------------------------------
// State ownership binds to declarations (F13/F15/F23): a consumer file that
// constructs the type, or a file mentioning the state in comments, is not
// the sole owner.
// ---------------------------------------------------------------------------

test("ARH1-state-lifetimes dirty twin: a consumer file constructing the type is not the sole owner", () => {
  const dirty = cloneProducts();
  const row = hotspot(dirty, SCHEDULER).stateLifetimes.find((s) => s.state === "Scheduler.nodes");
  // host_construction.rs constructs Scheduler but declares no nodes state.
  row.soleOwner = "crates/verter_session/src/host_construction.rs";
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-state-lifetimes" &&
        e.code === "state-owner-without-declaration" &&
        e.detail.includes("host_construction.rs"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-state-lifetimes dirty twin: an unrelated file mentioning the state in comments is not the sole owner", () => {
  const dirty = cloneProducts();
  const row = hotspot(dirty, SCHEDULER).stateLifetimes.find((s) => s.state === "Scheduler.nodes");
  // semantic_query.rs mentions "nodes" in prose/comments; it declares none.
  row.soleOwner = "crates/verter_session/src/semantic_query.rs";
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-state-lifetimes" &&
        e.code === "state-owner-without-declaration" &&
        e.detail.includes("semantic_query.rs"),
    ),
    JSON.stringify(result.errors),
  );
});

// ---------------------------------------------------------------------------
// Retained assoc items cover receiver-call usage (F16/F19): the ffi
// consumer calls PartialReasonSet::is_empty / ::iter through receiver
// syntax, so unlisting either must fail.
// ---------------------------------------------------------------------------

test("ARH1-surface dirty twin: receiver-called assoc items must stay retained (is_empty, iter)", () => {
  for (const member of ["is_empty", "iter"]) {
    const dirty = cloneProducts();
    const surface = hotspot(dirty, SEMANTIC_QUERY).minimalPublicSurface;
    surface.retainedAssocItems = surface.retainedAssocItems.filter(
      (i) => i !== `PartialReasonSet::${member}`,
    );
    const result = validate(dirty, loadManifest(), arh0);
    assert.equal(result.ok, false, member);
    assert.ok(
      result.errors.some(
        (e) =>
          e.caseId === "ARH1-surface" &&
          e.code === "assoc-item-unretained" &&
          e.detail.includes(`PartialReasonSet::${member}`) &&
          e.detail.includes("component_meta.rs"),
      ),
      `${member}: ${JSON.stringify(result.errors)}`,
    );
  }
});

// ---------------------------------------------------------------------------
// Capability-table completeness (F17): constructor rows, surface
// declarations and retained rows cannot be deleted silently.
// ---------------------------------------------------------------------------

test("ARH1-constructor dirty twin: deleting a constructor capability row is drift, not silence", () => {
  const dirty = cloneProducts();
  const rows = hotspot(dirty, SCHEDULER).constructorCapabilities;
  const idx = rows.findIndex((c) => c.constructor === "new");
  rows.splice(idx, 1);
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-constructor" &&
        e.code === "constructor-population-drift" &&
        e.detail.includes("pub fn Scheduler::new"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-surface dirty twin: clearing the surface declarations list is rejected", () => {
  const dirty = cloneProducts();
  hotspot(dirty, SCHEDULER).surfaceDeclarations = [];
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-surface" &&
        e.code === "missing-surface-declaration" &&
        e.detail.includes("pub mod scheduler;"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-surface dirty twin: deleting a retained fn row uncovers the pub fn and is rejected", () => {
  const dirty = cloneProducts();
  const surface = hotspot(dirty, SCHEDULER).minimalPublicSurface;
  surface.retainedFns = surface.retainedFns.filter((fn) => fn !== "len");
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-surface" &&
        e.code === "surface-item-uncovered" &&
        e.detail.includes("pub fn len"),
    ),
    JSON.stringify(result.errors),
  );
});

// ---------------------------------------------------------------------------
// Inline crate paths are imports (F18): macro/derive/alias paths in code
// position are measured, so an undeclared one is drift and a forbidden one
// is caught without a use statement.
// ---------------------------------------------------------------------------

test("ARH1-import-direction dirty twin: an inline-only verter reference must be declared", () => {
  const dirty = cloneProducts();
  const aid = hotspot(dirty, SCHEDULER).allowedImportDirection;
  // scheduler.rs production calls verter_audit::attribute! inline.
  aid.verter = aid.verter.filter((v) => v !== "verter_audit");
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-import-direction" &&
        e.code === "import-drift" &&
        e.detail.includes("measured verter import verter_audit is not declared"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-import-direction: inline paths are measured and noise is not", () => {
  const synth = [
    "fn main() {",
    "    let x = verter_session::spawn();", // inline crate path, no use statement
    '    let label = "verter_semantic::fake";', // string data, never an import
    "    let n: u64 = u64::from(3u8);", // primitive path root
    "    let v: Vec<u8> = (0..2).collect::<Vec<u8>>();", // turbofish
    "    #[allow(clippy::let_and_return)]", // tool-lint namespace
    "    let y = x;",
    "    y",
    "}",
  ].join("\n");
  const measured = measureImports(synth);
  assert.ok(measured.verter.has("verter_session"), "inline crate path is measured");
  assert.ok(!measured.verter.has("verter_semantic"), "string literals are not imports");
  assert.deepEqual([...measured.external], []);
});

// ---------------------------------------------------------------------------
// Field narrowing joins to qualified field use (F21): an ambiguous field
// name is only a consumer through the owning type's struct literal, and a
// same-named field of an unrelated type is never a false consumer.
// ---------------------------------------------------------------------------

test("ARH1-surface dirty twin: a recorded field consumer without qualified use is rejected", () => {
  const dirty = cloneProducts();
  const row = hotspot(dirty, SCHEDULER).minimalPublicSurface.narrow.find(
    (n) => n.item === "tombstones",
  );
  // host_construction.rs hits "tombstones" in neither Scheduler-qualified
  // form (SessionOverlayRoot owns the ambiguous mentions).
  row.consumersAffected.push("crates/verter_session/src/host_construction.rs");
  const result = validate(dirty, loadManifest(), arh0);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH1-surface" &&
        e.code === "narrow-consumer-without-reference" &&
        e.detail.includes("Scheduler.tombstones"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH1-surface: field use forms are type-qualified", () => {
  // A struct literal of the owning type names the field.
  assert.equal(
    fieldUseForms(
      "let s = Scheduler { nodes: m, tombstones: t };",
      "Scheduler",
      "tombstones",
      false,
    ).literal,
    true,
  );
  // An ambiguous field name: a bare receiver access on ANOTHER type is not
  // the owning type's consumer.
  assert.equal(
    fieldUseForms("let n = overlay.tombstones.len();", "Scheduler", "tombstones", false).any,
    false,
  );
  // An unambiguous field name: a bare receiver access outside the crate is
  // the owning type's consumer.
  assert.equal(
    fieldUseForms("let g = sched.generation_floors.len();", "Scheduler", "generation_floors", true)
      .receiver,
    true,
  );
});
