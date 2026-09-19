import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  loadManifest,
  loadProducts,
  mandatoryCases,
  REPO_ROOT,
  selectedCaseIds,
  validate,
} from "./verify.mjs";

const clean = loadProducts();

const cloneProducts = () => structuredClone(clean);

const hotspot = (products, path) =>
  products["characterization"].hotspots.find((h) => h.path === path);

test("ARH2-ratification: clean products validate and cover every mandatory case surface", () => {
  const result = validate(clean);
  assert.equal(result.ok, true, JSON.stringify(result.errors, null, 2));
  assert.deepEqual(mandatoryCases().sort(), [
    "ARH2-characterization",
    "ARH2-deletion",
    "ARH2-population",
    "ARH2-ratification",
    "ARH2-separation",
  ]);
  // The live re-derivation claims are real: predecessor validates ran inside.
  assert.ok(clean["characterization"].ac3.concerns.length === 5);
  assert.ok(clean["complexity-measurements"].structural.length === 5);
});

test("ARH2-population dirty twin: dropped hotspot is rejected (AC1)", () => {
  const dirty = cloneProducts();
  const hotspots = dirty["characterization"].hotspots;
  hotspots.splice(
    hotspots.findIndex((h) => h.path.endsWith("build.rs")),
    1,
  );
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-population" && e.code === "hotspot-population-drift",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-population dirty twin: invented responsibility is rejected (AC1)", () => {
  const dirty = cloneProducts();
  const h = hotspot(dirty, "crates/verter_scheduler/src/scheduler.rs");
  h.responsibilities.push({
    responsibility: "template codegen",
    survivingOwner: "crates/verter_scheduler/src/scheduler.rs",
  });
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-population" && e.code === "responsibility-invented",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-population dirty twin: dropped responsibility is rejected (AC1)", () => {
  const dirty = cloneProducts();
  const h = hotspot(dirty, "crates/verter_scheduler/src/scheduler.rs");
  h.responsibilities.splice(
    h.responsibilities.findIndex((r) => r.responsibility === "batch coordination"),
    1,
  );
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-population" && e.code === "responsibility-dropped",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-population dirty twin: stale structural LOC is rejected (live invariant)", () => {
  const dirty = cloneProducts();
  const row = dirty["complexity-measurements"].structural.find((r) =>
    r.path.endsWith("semantic_query.rs"),
  );
  row.fileLoc += 1;
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH2-population" &&
        e.code === "structural-loc-drift" &&
        e.detail.includes("semantic_query.rs"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-population dirty twin: stale population count is rejected (live invariant)", () => {
  const dirty = cloneProducts();
  dirty["complexity-measurements"].populations.arh0Inventory.packages += 1;
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH2-population" &&
        e.code === "population-count-drift" &&
        e.detail.includes("packages"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-deletion dirty twin: unexecuted deletion is rejected (AC1)", () => {
  const dirty = cloneProducts();
  dirty["characterization"].deletion.deletedPath = "crates/verter_scheduler";
  // A path that exists: the deletion must be proven executed against the tree.
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH2-deletion" && e.code === "deletion-not-executed"),
    JSON.stringify(result.errors),
  );
});

test("ARH2-deletion dirty twin: an arbitrary absent path is not the executed deletion (AC1)", () => {
  const dirty = cloneProducts();
  dirty["characterization"].deletion.deletedPath = "packages/definitely-never-existed";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH2-deletion" && e.code === "deletion-path-unbound"),
    JSON.stringify(result.errors),
  );
});

test("ARH2-deletion dirty twin: empty same-change refresh is rejected (AC1)", () => {
  const dirty = cloneProducts();
  dirty["characterization"].deletion.sameChangeRefresh = [];
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH2-deletion" && e.code === "deletion-refresh-empty"),
    JSON.stringify(result.errors),
  );
});

test("ARH2-deletion dirty twin: unknown satisfied debt is rejected", () => {
  const dirty = cloneProducts();
  dirty["characterization"].deletion.satisfies = "ARH0-DEBT-99";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH2-deletion" && e.code === "deletion-debt-unknown"),
    JSON.stringify(result.errors),
  );
});

test("ARH2-deletion dirty twin: cutover row ownership must stay ARH2's", () => {
  const dirty = cloneProducts();
  dirty["characterization"].deletion.cutoverRow = "ARH1-CUT-2";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH2-deletion" && e.code.startsWith("deletion-")),
    JSON.stringify(result.errors),
  );
});

test("ARH2-deletion: the executed path is absent from the tree and from every shipped product", () => {
  const deleted = clean["characterization"].deletion.deletedPath;
  assert.equal(fs.existsSync(deleted), false);
  const arh0Inventory = fs.readFileSync(
    new URL("../ARH0/products/codebase-inventory.json", import.meta.url),
    "utf8",
  );
  assert.ok(!arh0Inventory.includes(`"module": "${deleted}"`));
  const arh0Debt = fs.readFileSync(
    new URL("../ARH0/products/debt-register.json", import.meta.url),
    "utf8",
  );
  assert.ok(!arh0Debt.includes(`"candidatePath": "${deleted}"`));
  const arh1Cutover = fs.readFileSync(
    new URL("../ARH1/products/cutover-register.json", import.meta.url),
    "utf8",
  );
  assert.ok(!arh1Cutover.includes(`"candidatePath": "${deleted}"`));
});

test("ARH2-characterization dirty twin: witness naming a nonexistent test is rejected (AC2)", () => {
  const dirty = cloneProducts();
  const h = hotspot(dirty, "crates/verter_scheduler/src/scheduler.rs");
  h.pins[0].witnesses[0].test = "dag_tests_phantom_witness";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-characterization" && e.code === "witness-test-missing",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-characterization dirty twin: a filter that selects nothing is rejected (AC2)", () => {
  const dirty = cloneProducts();
  const h = hotspot(dirty, "crates/verter_session/src/semantic_query.rs");
  h.pins[0].filter = "unrelated_module";
  h.pins[0].command = `cargo nextest run -p verter_session ${h.pins[0].filter}`;
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-characterization" && e.code === "pin-filter-selects-nothing",
    ),
    JSON.stringify(result.errors),
  );
});

const retargetLane = (products, from, to) => {
  for (const id of ["production-behavior", "test-cost"]) {
    const lanes = products["complexity-measurements"].dimensions.find(
      (d) => d.id === id,
    ).mechanisms;
    const idx = lanes.indexOf(from);
    if (idx !== -1) lanes[idx] = to;
  }
};

test("ARH2-characterization dirty twin: nested inline module is not a cross-product witness id (AC2)", () => {
  const dirty = cloneProducts();
  const retarget = (pin) => {
    if (pin.filter !== "scheduler::tests") return;
    const previous = pin.command;
    pin.filter = "scheduler::pool_topology";
    pin.command = `cargo nextest run -p verter_scheduler ${pin.filter}`;
    retargetLane(dirty, previous, pin.command);
  };
  for (const h of dirty["characterization"].hotspots) {
    for (const pin of h.pins) retarget(pin);
  }
  for (const r of dirty["characterization"].routes) {
    for (const pin of r.pins) retarget(pin);
  }
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH2-characterization" &&
        e.code === "pin-filter-selects-nothing" &&
        e.detail.includes("scheduler::pool_topology"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-characterization dirty twin: witness is bound to its enclosing module (AC2)", () => {
  const dirty = cloneProducts();
  const pin = dirty["characterization"].routes.find((r) => r.cutoverRow === "ARH1-CUT-2").pins[0];
  const previous = pin.command;
  pin.witnesses = [
    pin.witnesses.find((w) => w.test === "tombstone_rejects_pre_remove_source_submission"),
  ];
  pin.filter = "scheduler::tombstone_rejects_pre_remove_source_submission";
  pin.command = `cargo nextest run -p verter_scheduler ${pin.filter}`;
  retargetLane(dirty, previous, pin.command);
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH2-characterization" &&
        e.code === "pin-filter-selects-nothing" &&
        e.detail.includes("scheduler::tests::tombstone_rejects_pre_remove_source_submission"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-characterization dirty twin: filesystem src:: filter selecting no compiled module is rejected (AC2)", () => {
  const dirty = cloneProducts();
  const pin = hotspot(dirty, "crates/verter_scheduler/src/scheduler.rs").pins.find(
    (p) => p.filter === "dag_tests",
  );
  const previous = pin.command;
  pin.filter = "src::dag_tests";
  pin.command = `cargo nextest run -p verter_scheduler ${pin.filter}`;
  retargetLane(dirty, previous, pin.command);
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH2-characterization" &&
        e.code === "pin-filter-selects-nothing" &&
        e.detail.includes("src::dag_tests"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-characterization dirty twin: nextest command omitting run is rejected", () => {
  const dirty = cloneProducts();
  const pin = hotspot(dirty, "crates/verter_scheduler/src/scheduler.rs").pins[0];
  const previous = pin.command;
  pin.command = `cargo nextest -p verter_scheduler ${pin.filter}`;
  retargetLane(dirty, previous, pin.command);
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-characterization" && e.code === "pin-command-not-canonical",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-characterization dirty twin: witness without a test attribute is rejected (AC2)", () => {
  const dirty = cloneProducts();
  const h = hotspot(dirty, "crates/verter_scheduler/src/scheduler.rs");
  // A real fn of the file, but not a test: the mod-level use statement
  // `use crate::source_loader::MemorySourceLoader;` never carries #[test].
  h.pins[0].witnesses[0] = {
    file: "crates/verter_scheduler/src/dag_tests.rs",
    test: "work_node_identity_has_exactly_three_variants",
  };
  // That one IS a test; point the filter check away by using a non-test fn.
  h.pins[0].witnesses[0].test = "next_ready";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH2-characterization"),
    JSON.stringify(result.errors),
  );
});

const COVERAGE_WITNESS =
  "crates/verter_session/src/project_semantic_dispatch/flow_return_coverage_tests.rs";
const COVERAGE_FN = "vue_script_setup_functions_serve_under_the_instance_owner_only";

function withReadOverlay(rel, mutate, run) {
  const target = path.resolve(REPO_ROOT, rel);
  const original = fs.readFileSync;
  fs.readFileSync = function overlayRead(file, encoding, ...rest) {
    const text = original.call(fs, file, encoding, ...rest);
    if (typeof text === "string" && path.resolve(String(file)) === target) {
      return mutate(text);
    }
    return text;
  };
  try {
    return run();
  } finally {
    fs.readFileSync = original;
  }
}

test("ARH2-characterization dirty twin: ignored pinned witness is rejected (AC2)", () => {
  const result = withReadOverlay(
    COVERAGE_WITNESS,
    (text) =>
      text.replace(
        new RegExp(`#\\[test\\]\\r?\\nfn ${COVERAGE_FN}\\(`),
        `#[test] #[ignore]\nfn ${COVERAGE_FN}(`,
      ),
    () => validate(cloneProducts()),
  );
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH2-characterization" && e.code === "witness-ignored"),
    JSON.stringify(result.errors),
  );
});

test("ARH2-characterization dirty twin: cfg-disabled pinned witness is rejected (AC2)", () => {
  const result = withReadOverlay(
    COVERAGE_WITNESS,
    (text) => text.replace(`fn ${COVERAGE_FN}(`, `#[cfg(any())] fn ${COVERAGE_FN}(`),
    () => validate(cloneProducts()),
  );
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-characterization" && e.code === "witness-cfg-disabled",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-characterization dirty twin: dropped narrowing-route characterization is rejected (AC1)", () => {
  const dirty = cloneProducts();
  const routes = dirty["characterization"].routes;
  routes.splice(
    routes.findIndex((r) => r.cutoverRow === "ARH1-CUT-3"),
    1,
  );
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH2-characterization" &&
        e.code === "route-characterization-cardinality" &&
        e.detail.includes("ARH1-CUT-3"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-characterization dirty twin: duplicated route characterization is rejected (AC1)", () => {
  const dirty = cloneProducts();
  const routes = dirty["characterization"].routes;
  routes.push(structuredClone(routes.find((r) => r.cutoverRow === "ARH1-CUT-4")));
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH2-characterization" &&
        e.code === "route-characterization-cardinality" &&
        e.detail.includes("ARH1-CUT-4"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-characterization dirty twin: pre-narrowing surface that is no longer pub is rejected", () => {
  const dirty = cloneProducts();
  const route = dirty["characterization"].routes.find((r) => r.cutoverRow === "ARH1-CUT-2");
  route.surface.items = ["tombstones", "generation_floors", "deferred_blocker_ids", "node"];
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        (e.code === "route-surface-drift" || e.code === "route-surface-not-live") &&
        e.caseId === "ARH2-characterization",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-characterization dirty twin: CUT-4 retained-type drift is rejected (AC1)", () => {
  const dirty = cloneProducts();
  const route = dirty["characterization"].routes.find((r) => r.cutoverRow === "ARH1-CUT-4");
  route.surface.retainedTypes = route.surface.retainedTypes.filter((t) => t !== "HashValue");
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH2-characterization" &&
        e.code === "route-surface-drift" &&
        e.detail.includes("retainedTypes"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-characterization dirty twin: CUT-4 consumer-population drift is rejected (AC1)", () => {
  const dirty = cloneProducts();
  const route = dirty["characterization"].routes.find((r) => r.cutoverRow === "ARH1-CUT-4");
  route.surface.consumersAffected = route.surface.consumersAffected.slice(1);
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH2-characterization" &&
        e.code === "route-surface-drift" &&
        e.detail.includes("consumersAffected"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-characterization dirty twin: CUT-4 invented surface is rejected (AC1)", () => {
  const dirty = cloneProducts();
  const route = dirty["characterization"].routes.find((r) => r.cutoverRow === "ARH1-CUT-4");
  route.surface = { kind: "invented", derivedLive: "false" };
  route.pins[0].behavior = "unrelated";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-characterization" && e.code === "route-surface-drift",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-characterization dirty twin: missing AC3 concern is rejected (AC3)", () => {
  const dirty = cloneProducts();
  const concerns = dirty["characterization"].ac3.concerns;
  concerns.splice(
    concerns.findIndex((c) => c.concern === "edit/revert"),
    1,
  );
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH2-characterization" &&
        e.code === "ac3-concern-missing" &&
        e.detail.includes("edit/revert"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-characterization dirty twin: AC3 evidence naming a missing file is rejected (AC3)", () => {
  const dirty = cloneProducts();
  const concerns = dirty["characterization"].ac3.concerns;
  concerns.find((c) => c.concern === "cancellation").evidence[0].file =
    "crates/verter_scheduler/src/gone.rs";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-characterization" && e.code === "witness-file-missing",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: dropped dimension is rejected (charter separation)", () => {
  const dirty = cloneProducts();
  const dims = dirty["complexity-measurements"].dimensions;
  dims.splice(
    dims.findIndex((d) => d.id === "application-latency"),
    1,
  );
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH2-separation" &&
        e.code === "dimension-cardinality" &&
        e.detail.includes("application-latency"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: invented gate cell is rejected (AC5)", () => {
  const dirty = cloneProducts();
  const dims = dirty["complexity-measurements"].dimensions;
  dims.find((d) => d.id === "application-latency").mechanisms = ["B6_INVENTED_CELL"];
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH2-separation" && e.code === "gate-cell-unknown"),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: missing mechanisms are rejected (AC5)", () => {
  const dirty = cloneProducts();
  for (const dim of dirty["complexity-measurements"].dimensions) delete dim.mechanisms;
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-separation" && e.code === "mechanism-binding-missing",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: empty mechanisms are rejected (AC5)", () => {
  const dirty = cloneProducts();
  for (const dim of dirty["complexity-measurements"].dimensions) dim.mechanisms = [];
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-separation" && e.code === "mechanism-binding-missing",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: suffix-forged runner class is rejected (AC5)", () => {
  const dirty = cloneProducts();
  dirty["complexity-measurements"].numberPolicy.runnerClass =
    "apple-silicon-laptop-8core-24gib-forged";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH2-separation" && e.code === "runner-class-unbound"),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: absent runner class is rejected (AC5)", () => {
  const dirty = cloneProducts();
  delete dirty["complexity-measurements"].numberPolicy.runnerClass;
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH2-separation" && e.code === "runner-class-unbound"),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: clean and warm recipes measuring different crates are rejected (AC5)", () => {
  const dirty = cloneProducts();
  const dim = dirty["complexity-measurements"].dimensions.find(
    (d) => d.id === "clean-warm-build-time",
  );
  dim.mechanisms.find((recipe) => recipe.cacheState === "warm").command =
    "cargo build -p verter_debug_assert --timings";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-separation" && e.code === "cargo-build-recipe-identity-mismatch",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: clean/warm missing a cache state is rejected (AC5)", () => {
  const dirty = cloneProducts();
  const dim = dirty["complexity-measurements"].dimensions.find(
    (d) => d.id === "clean-warm-build-time",
  );
  dim.mechanisms = dim.mechanisms.filter((recipe) => recipe.cacheState === "clean");
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-separation" && e.code === "dimension-cache-state-incomplete",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: clean recipe without prepare is rejected (AC5)", () => {
  const dirty = cloneProducts();
  const dim = dirty["complexity-measurements"].dimensions.find(
    (d) => d.id === "clean-warm-build-time",
  );
  delete dim.mechanisms.find((recipe) => recipe.cacheState === "clean").prepare;
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-separation" && e.code === "cargo-build-clean-prepare-missing",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: prewarming the target is not a clean prepare (AC5)", () => {
  const dirty = cloneProducts();
  const dim = dirty["complexity-measurements"].dimensions.find(
    (d) => d.id === "clean-warm-build-time",
  );
  dim.mechanisms.find((recipe) => recipe.cacheState === "clean").prepare =
    "cargo build -p verter_scheduler -p verter_session";
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-separation" && e.code === "cargo-build-clean-prepare-not-clean",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: application-latency cell that bypasses the host is rejected (AC5)", () => {
  const dirty = cloneProducts();
  dirty["complexity-measurements"].dimensions.find(
    (d) => d.id === "application-latency",
  ).mechanisms = ["B6_COMPILER_ROUTE_OVERHEAD"];
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-separation" && e.code === "gate-cell-wrong-operation",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: test-cost bound to unrelated gate cells is rejected (AC5)", () => {
  const dirty = cloneProducts();
  const dim = dirty["complexity-measurements"].dimensions.find((d) => d.id === "test-cost");
  dim.mechanismKind = "gate-cell";
  dim.mechanisms = ["BF2_VUE_ORACLE_MANIFEST_GENERATE", "BF2_SVELTE_ORACLE_MANIFEST_GENERATE"];
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-separation" && e.code === "dimension-mechanism-kind-mismatch",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: committed wall-clock number is rejected (AC5)", () => {
  const dirty = cloneProducts();
  const dims = dirty["complexity-measurements"].dimensions;
  dims.find((d) => d.id === "test-cost").wallNs = 12345;
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-separation" && e.code === "dimension-commits-wall-clock",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: lane recorded in the dimension but pinned nowhere is rejected", () => {
  const dirty = cloneProducts();
  const dims = dirty["complexity-measurements"].dimensions;
  dims
    .find((d) => d.id === "production-behavior")
    .mechanisms.push("cargo nextest run -p verter_scheduler phantom_lane");
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-separation" && e.code === "behavior-lane-invented",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: pinned lane missing from the dimension is rejected", () => {
  const dirty = cloneProducts();
  const dims = dirty["complexity-measurements"].dimensions;
  const lanes = dims.find((d) => d.id === "production-behavior").mechanisms;
  lanes.splice(lanes.indexOf("cargo nextest run -p verter_session stable_key_tests"), 1);
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH2-separation" && e.code === "behavior-lane-unbound"),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: drifted over-threshold count is rejected (live invariant)", () => {
  const dirty = cloneProducts();
  dirty["complexity-measurements"].godModuleBasis.productionFilesOverThreshold = 32;
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH2-separation" && e.code === "threshold-basis-drift"),
    JSON.stringify(result.errors),
  );
});

test("ARH2-separation dirty twin: threshold ceiling detached from the live guard is rejected", () => {
  const dirty = cloneProducts();
  dirty["complexity-measurements"].thresholds[0].defaultMaxLines = 5000;
  const result = validate(dirty);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) => e.caseId === "ARH2-separation" && e.code === "threshold-ceiling-mismatch",
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-ratification dirty twin: manifest recording an unimplemented case is rejected (AC1)", () => {
  const dirtyManifest = structuredClone(loadManifest());
  dirtyManifest.cases.push({
    id: "ARH2-phantom",
    disposition: "reject",
    twins: ["clean products"],
  });
  const result = validate(cloneProducts(), dirtyManifest);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some((e) => e.caseId === "ARH2-ratification" && e.code === "manifest-case-drift"),
    JSON.stringify(result.errors),
  );
});

test("ARH2-ratification dirty twin: manifest dropping an implemented case is rejected (AC1)", () => {
  const dirtyManifest = structuredClone(loadManifest());
  const idx = dirtyManifest.cases.findIndex((c) => c.id === "ARH2-separation");
  dirtyManifest.cases.splice(idx, 1);
  const result = validate(cloneProducts(), dirtyManifest);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH2-ratification" &&
        e.code === "manifest-case-drift" &&
        e.detail.includes("ARH2-separation"),
    ),
    JSON.stringify(result.errors),
  );
});

test("ARH2-ratification dirty twin: a different existing script is not the canonical verify command", () => {
  const dirtyManifest = structuredClone(loadManifest());
  dirtyManifest.verify = "node scripts/affected-tests.mjs";
  const result = validate(cloneProducts(), dirtyManifest);
  assert.equal(result.ok, false);
  assert.ok(
    result.errors.some(
      (e) =>
        e.caseId === "ARH2-ratification" &&
        e.code === "manifest-command-drift" &&
        e.detail.includes("canonical"),
    ),
    JSON.stringify(result.errors),
  );
});

test("source references are optional context, independent of commit identity", () => {
  const products = cloneProducts();
  for (const product of Object.values(products)) {
    delete product.candidate;
    delete product.sourceReference;
  }
  const withoutHistory = validate(products);
  assert.equal(withoutHistory.ok, true, JSON.stringify(withoutHistory.errors));
  // Different landing titles and dates describe history; they prove no invariant.
  Object.values(products).forEach((product, index) => {
    product.sourceReference = { title: "Historical landing " + index, date: "2026-09-19" };
  });
  const withContext = validate(products);
  assert.equal(withContext.ok, true, JSON.stringify(withContext.errors));
});

test("ARH2 CI: architecture-health filter selects performance methodology inputs", () => {
  const ci = fs.readFileSync(new URL("../../../.github/workflows/ci.yml", import.meta.url), "utf8");
  const start = ci.indexOf("\n            arch:\n");
  assert.notEqual(start, -1, "ci.yml must declare the arch filter");
  const rest = ci.slice(start + 1);
  const next = rest.search(/\n            [a-z_]+:\n/);
  const block = next === -1 ? rest : rest.slice(0, next);
  const paths = [...block.matchAll(/- '([^']+)'/g)].map((m) => m[1]);
  assert.ok(
    paths.includes("performance-gates.toml"),
    `arch filter omits performance-gates.toml: ${paths.join(", ")}`,
  );
  assert.ok(
    paths.includes("scripts/validate-performance-gates.mjs"),
    `arch filter omits scripts/validate-performance-gates.mjs: ${paths.join(", ")}`,
  );
});

test("ARH2-verify CLI: the manifest verify command runs validate() and exits 0 on the clean tree", () => {
  const verifyPath = fileURLToPath(new URL("./verify.mjs", import.meta.url));
  const stdout = execFileSync(process.execPath, [verifyPath], {
    encoding: "utf8",
    env: {
      ...process.env,
      GIT_DIR: fileURLToPath(new URL("./missing-git-history", import.meta.url)),
    },
  });
  assert.match(stdout, /ARH2 verify: PASS/);
});
